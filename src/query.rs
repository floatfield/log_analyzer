//! The log query language: parsing and matching.
//!
//! Grammar (design D1), with AND binding tighter than OR and parentheses
//! for grouping:
//!
//! ```text
//! expr    := andExpr ( 'or' andExpr )*
//! andExpr := atom+
//! atom    := '(' expr ')' | term
//! term    := field '=' value | quoted-phrase | bare-word
//! value   := '…' / "…" (may contain spaces) | run of non-space, non-paren chars
//! ```
//!
//! `or` is a keyword only as a standalone bare word (case-insensitive);
//! inside quotes or as a field value it is literal text. Bare and quoted
//! substring terms match the full line text case-insensitively (quoted
//! phrases may contain spaces). `field=value` terms compare case-sensitively
//! against structured entries only. Field names match
//! `[@A-Za-z_][@A-Za-z0-9_.]*`.

use std::fmt;

use serde_json::Value;

/// Evaluated shape of a parsed query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// Every child must match.
    And(Vec<Expr>),
    /// At least one child must match.
    Or(Vec<Expr>),
    /// Case-insensitive substring over the raw line text.
    Substring {
        /// Lowercased needle, precomputed at parse time.
        needle_lower: String,
    },
    /// Case-sensitive textual equality against a structured entry's field.
    Field { name: String, value: String },
}

/// Reason a query could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid query: {}", self.message)
    }
}

impl std::error::Error for ParseError {}

/// True when `c` may start a field name.
fn is_field_start(c: char) -> bool {
    matches!(c, '@' | 'a'..='z' | 'A'..='Z' | '_')
}

/// True when `c` may continue a field name.
fn is_field_continue(c: char) -> bool {
    is_field_start(c) || c.is_ascii_digit() || c == '.'
}

/// True when `name` is a valid query field name: it starts with `@`, an ASCII
/// letter, or `_`, and continues with those, digits, or `.` (module doc:
/// field-name grammar). Exposed so a modifier-clicked cell can be rejected up
/// front when its column name cannot form a term (spec:
/// modifier-click-filtering / Ineligible cells do nothing).
pub fn is_valid_field_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if is_field_start(first) => {}
        _ => return false,
    }
    chars.all(is_field_continue)
}

/// Lexer token.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Or,
    LParen,
    RParen,
    /// Bare word or quoted phrase (already lowercased).
    Substring(String),
    /// `field=value` with the value possibly quoted.
    FieldEq {
        name: String,
        value: String,
    },
}

/// Character cursor over the input.
struct Cursor<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(src: &'a str) -> Self {
        Self { src, pos: 0 }
    }

    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn skip_ws(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() {
                self.bump();
            } else {
                break;
            }
        }
    }

    /// True when `ch` ends a lexeme (whitespace or parenthesis).
    fn is_delimiter(ch: char) -> bool {
        ch.is_whitespace() || ch == '(' || ch == ')'
    }
}

fn unterminated_quote(quote: char) -> ParseError {
    ParseError {
        message: format!("unterminated quote (missing closing `{quote}`)"),
    }
}

/// Read a quoted lexeme after its opening quote was consumed; leaves the
/// closing quote consumed.
fn scan_quoted(cur: &mut Cursor<'_>, quote: char) -> Result<String, ParseError> {
    let mut text = String::new();
    loop {
        match cur.bump() {
            None => return Err(unterminated_quote(quote)),
            Some(ch) if ch == quote => return Ok(text),
            Some(ch) => text.push(ch),
        }
    }
}

/// Read an unquoted value: a run of non-delimiter characters.
fn scan_bare_value(cur: &mut Cursor<'_>) -> String {
    let mut value = String::new();
    while let Some(ch) = cur.peek() {
        if Cursor::is_delimiter(ch) {
            break;
        }
        cur.bump();
        value.push(ch);
    }
    value
}

/// Scan one word-like lexeme starting at a non-delimiter, non-quote char.
/// Handles `field=value` (with a possibly quoted value) and the `or` keyword.
fn scan_word(cur: &mut Cursor<'_>, toks: &mut Vec<Tok>) -> Result<(), ParseError> {
    let mut name = String::new();
    loop {
        let ch = match cur.peek() {
            None => break,
            Some(ch) => ch,
        };
        if Cursor::is_delimiter(ch) {
            break;
        }
        if ch == '=' {
            cur.bump();
            let value = match cur.peek() {
                Some(q @ ('\'' | '"')) => {
                    cur.bump();
                    scan_quoted(cur, q)?
                }
                Some(ch) if !Cursor::is_delimiter(ch) => scan_bare_value(cur),
                _ => {
                    return Err(ParseError {
                        message: "missing value after '='".to_owned(),
                    });
                }
            };
            if !is_valid_field_name(&name) {
                let shown = if name.is_empty() { "" } else { &name };
                let what = if name.is_empty() {
                    "missing field name before '='"
                } else {
                    "invalid field name"
                };
                return Err(ParseError {
                    message: if shown.is_empty() {
                        what.to_owned()
                    } else {
                        format!("{what} `{shown}`")
                    },
                });
            }
            toks.push(Tok::FieldEq { name, value });
            return Ok(());
        }
        cur.bump();
        name.push(ch);
    }
    if name.eq_ignore_ascii_case("or") {
        toks.push(Tok::Or);
    } else {
        toks.push(Tok::Substring(name.to_lowercase()));
    }
    Ok(())
}

fn tokenize(input: &str) -> Result<Vec<Tok>, ParseError> {
    let mut toks = Vec::new();
    let mut cur = Cursor::new(input);
    loop {
        cur.skip_ws();
        match cur.peek() {
            None => break,
            Some('(') => {
                cur.bump();
                toks.push(Tok::LParen);
            }
            Some(')') => {
                cur.bump();
                toks.push(Tok::RParen);
            }
            Some(q @ ('\'' | '"')) => {
                cur.bump();
                let text = scan_quoted(&mut cur, q)?;
                toks.push(Tok::Substring(text.to_lowercase()));
            }
            Some(_) => scan_word(&mut cur, &mut toks)?,
        }
    }
    Ok(toks)
}

/// Recursive-descent parser over the token stream.
struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn bump(&mut self) -> Option<Tok> {
        let tok = self.toks.get(self.pos).cloned();
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    /// `expr := andExpr ( 'or' andExpr )*`
    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        let mut branches = vec![self.parse_and()?];
        while matches!(self.peek(), Some(Tok::Or)) {
            self.bump();
            branches.push(self.parse_and()?);
        }
        if branches.len() == 1 {
            Ok(branches.pop().expect("len checked"))
        } else {
            Ok(Expr::Or(branches))
        }
    }

    /// `andExpr := atom+` — juxtaposition means AND.
    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut atoms = Vec::new();
        while let Some(tok) = self.peek() {
            match tok {
                Tok::Or | Tok::RParen => break,
                _ => atoms.push(self.parse_atom()?),
            }
        }
        match atoms.len() {
            0 => Err(ParseError {
                message: "expected a term".to_owned(),
            }),
            1 => Ok(atoms.pop().expect("len checked")),
            _ => Ok(Expr::And(atoms)),
        }
    }

    /// `atom := '(' expr ')' | term`
    fn parse_atom(&mut self) -> Result<Expr, ParseError> {
        match self.bump() {
            Some(Tok::LParen) => {
                if matches!(self.peek(), Some(Tok::RParen)) {
                    return Err(ParseError {
                        message: "empty group `()`".to_owned(),
                    });
                }
                let expr = self.parse_expr()?;
                match self.bump() {
                    Some(Tok::RParen) => Ok(expr),
                    _ => Err(ParseError {
                        message: "missing closing `)`".to_owned(),
                    }),
                }
            }
            Some(Tok::Substring(needle_lower)) => Ok(Expr::Substring { needle_lower }),
            Some(Tok::FieldEq { name, value }) => Ok(Expr::Field { name, value }),
            _ => Err(ParseError {
                message: "expected a term".to_owned(),
            }),
        }
    }
}

/// Parse a query string into a [`Query`]. The empty (or all-whitespace) query
/// is valid and matches everything.
pub fn parse(input: &str) -> Result<Query, ParseError> {
    let toks = tokenize(input)?;
    if toks.is_empty() {
        return Ok(Query { root: None });
    }
    let mut parser = Parser { toks, pos: 0 };
    let expr = parser.parse_expr()?;
    if parser.pos != parser.toks.len() {
        // Only a stray `)` can remain: parse_and stops at RParen at top level.
        return Err(ParseError {
            message: "unbalanced `)`".to_owned(),
        });
    }
    Ok(Query { root: Some(expr) })
}

/// Render a JSON value as the plain text used for field-equality comparison
/// (strings compare as their content, numbers by their default rendering,
/// containers as compact JSON).
pub fn field_value_as_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_owned(),
        other => other.to_string(),
    }
}

/// Evaluate one AST node against a row.
fn eval(expr: &Expr, line_text: &str, entry: Option<&serde_json::Map<String, Value>>) -> bool {
    match expr {
        Expr::And(children) => children.iter().all(|c| eval(c, line_text, entry)),
        Expr::Or(children) => children.iter().any(|c| eval(c, line_text, entry)),
        Expr::Substring { needle_lower } => line_text.to_lowercase().contains(needle_lower),
        Expr::Field { name, value } => entry
            .and_then(|map| map.get(name))
            .map(field_value_as_text)
            .is_some_and(|text| text == *value),
    }
}

fn needs_line_text_expr(expr: &Expr) -> bool {
    match expr {
        Expr::And(children) | Expr::Or(children) => children.iter().any(needs_line_text_expr),
        Expr::Substring { .. } => true,
        Expr::Field { .. } => false,
    }
}

/// A parsed query.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    /// `None` for the empty query (matches every row).
    root: Option<Expr>,
}

impl Query {
    /// The parsed expression tree, if any. Test-only: production matching
    /// evaluates internally in [`Query::matches`].
    #[cfg(test)]
    pub fn root(&self) -> Option<&Expr> {
        self.root.as_ref()
    }

    /// True when the query is empty (matches every row).
    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    /// True when any term needs the full raw line text (substring terms).
    /// Callers can skip materializing the line text for field-only queries.
    pub fn needs_line_text(&self) -> bool {
        self.root.as_ref().is_some_and(needs_line_text_expr)
    }

    /// Evaluate this query against one row.
    ///
    /// `line_text` is the full raw line text (used by substring terms);
    /// `entry` is the parsed JSON object for structured rows and `None` for
    /// raw rows.
    pub fn matches(&self, line_text: &str, entry: Option<&serde_json::Map<String, Value>>) -> bool {
        match &self.root {
            None => true,
            Some(expr) => eval(expr, line_text, entry),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Map;
    use serde_json::json;

    fn entry(pairs: &[(&str, Value)]) -> Map<String, Value> {
        let mut map = Map::new();
        for (key, value) in pairs {
            map.insert((*key).to_owned(), value.clone());
        }
        map
    }

    /// The AND-combined atoms of a simple (ungrouped, OR-free) query.
    fn and_atoms<'a>(q: &'a Query) -> &'a [Expr] {
        match q.root() {
            Some(Expr::And(atoms)) => atoms,
            other => panic!("expected And, got {other:?}"),
        }
    }

    // ---- 1.1 tokenizer tests ----

    #[test]
    fn quoted_double_value_with_spaces() {
        let q = parse(r#"message="foo bar""#).unwrap();
        assert_eq!(
            q.root(),
            Some(&Expr::Field {
                name: "message".into(),
                value: "foo bar".into()
            })
        );
        let exact = entry(&[("message", json!("foo bar"))]);
        let longer = entry(&[("message", json!("foo bar baz"))]);
        assert!(q.matches("x", Some(&exact)));
        assert!(!q.matches("x", Some(&longer)));
    }

    #[test]
    fn quoted_single_value_with_spaces() {
        let q = parse("requestId='some-request-id'").unwrap();
        assert_eq!(
            q.root(),
            Some(&Expr::Field {
                name: "requestId".into(),
                value: "some-request-id".into()
            })
        );
    }

    #[test]
    fn quoted_value_may_contain_or_and_parens() {
        let q = parse("note='or (not) and'").unwrap();
        assert_eq!(
            q.root(),
            Some(&Expr::Field {
                name: "note".into(),
                value: "or (not) and".into()
            })
        );
    }

    #[test]
    fn quoted_or_is_a_substring_not_a_keyword() {
        let q = parse("'or'").unwrap();
        assert_eq!(
            q.root(),
            Some(&Expr::Substring {
                needle_lower: "or".into()
            })
        );
        assert!(q.matches("word OR another", None));
    }

    #[test]
    fn or_keyword_is_case_insensitive() {
        for input in [
            "level=ERROR or level=WARN",
            "level=ERROR OR level=WARN",
            "level=ERROR Or level=WARN",
        ] {
            let q = parse(input).unwrap();
            assert_eq!(
                q.root(),
                Some(&Expr::Or(vec![
                    Expr::Field {
                        name: "level".into(),
                        value: "ERROR".into()
                    },
                    Expr::Field {
                        name: "level".into(),
                        value: "WARN".into()
                    },
                ])),
                "{input}"
            );
        }
    }

    #[test]
    fn or_as_field_value_is_literal() {
        let q = parse("status=or").unwrap();
        assert_eq!(
            q.root(),
            Some(&Expr::Field {
                name: "status".into(),
                value: "or".into()
            })
        );
    }

    // ---- 1.2 parser tests ----

    #[test]
    fn parse_valid_queries() {
        let q = parse("level=ERROR timeout").unwrap();
        let atoms = and_atoms(&q);
        assert_eq!(atoms.len(), 2);
        assert_eq!(
            atoms[0],
            Expr::Field {
                name: "level".into(),
                value: "ERROR".into()
            }
        );
        assert_eq!(
            atoms[1],
            Expr::Substring {
                needle_lower: "timeout".into()
            }
        );
    }

    #[test]
    fn parse_handles_extra_whitespace() {
        let q = parse("  level=ERROR\ttimeout  \n").unwrap();
        assert_eq!(and_atoms(&q).len(), 2);
    }

    #[test]
    fn parse_empty_query_is_valid_and_matches_all() {
        for input in ["", "   "] {
            let q = parse(input).unwrap();
            assert!(q.is_empty());
            assert!(q.matches("anything", None));
            assert!(q.matches("anything", Some(&entry(&[]))));
        }
    }

    #[test]
    fn and_binds_tighter_than_or() {
        let q = parse("level=ERROR timeout or service=auth").unwrap();
        assert_eq!(
            q.root(),
            Some(&Expr::Or(vec![
                Expr::And(vec![
                    Expr::Field {
                        name: "level".into(),
                        value: "ERROR".into()
                    },
                    Expr::Substring {
                        needle_lower: "timeout".into()
                    },
                ]),
                Expr::Field {
                    name: "service".into(),
                    value: "auth".into()
                },
            ]))
        );
        // Semantics: (level=ERROR AND timeout) OR service=auth
        let left = entry(&[("level", json!("ERROR")), ("message", json!("net timeout"))]);
        let middle = entry(&[("level", json!("WARN")), ("service", json!("auth"))]);
        let right = entry(&[("level", json!("WARN")), ("service", json!("gateway"))]);
        assert!(q.matches("text with timeout", Some(&left)));
        assert!(q.matches("x", Some(&middle)));
        assert!(!q.matches("x", Some(&right)));
    }

    #[test]
    fn grouping_overrides_precedence() {
        // The proposal's example query.
        let q = parse(
            "(requestId='some-request-id' or requestId='another-request-id') message=\"foo bar\"",
        )
        .unwrap();
        let first = entry(&[
            ("requestId", json!("some-request-id")),
            ("message", json!("foo bar")),
        ]);
        let second = entry(&[
            ("requestId", json!("another-request-id")),
            ("message", json!("foo bar")),
        ]);
        let wrong_msg = entry(&[
            ("requestId", json!("some-request-id")),
            ("message", json!("foo bar baz")),
        ]);
        let wrong_id = entry(&[
            ("requestId", json!("other-id")),
            ("message", json!("foo bar")),
        ]);
        assert!(q.matches("x", Some(&first)));
        assert!(q.matches("x", Some(&second)));
        assert!(!q.matches("x", Some(&wrong_msg)));
        assert!(!q.matches("x", Some(&wrong_id)));
    }

    #[test]
    fn nested_groups() {
        let q = parse("((level=ERROR service=auth) or level=WARN) timeout").unwrap();
        let err_auth = entry(&[("level", json!("ERROR")), ("service", json!("auth"))]);
        let warn = entry(&[("level", json!("WARN"))]);
        let err_other = entry(&[("level", json!("ERROR")), ("service", json!("gateway"))]);
        assert!(q.matches("saw a timeout", Some(&err_auth)));
        assert!(q.matches("saw a timeout", Some(&warn)));
        assert!(!q.matches("saw a timeout", Some(&err_other)));
        assert!(!q.matches("no keyword here", Some(&warn)));
    }

    #[test]
    fn parens_grouping_or_of_substrings() {
        let q = parse("(alpha or beta) gamma").unwrap();
        assert!(q.matches("ALPHA GAMMA", None));
        assert!(q.matches("Beta Gamma", None));
        assert!(!q.matches("alpha delta", None));
    }

    #[test]
    fn parse_allows_at_and_dots_in_field_names() {
        let q = parse("@timestamp=x a.b=c _d=e").unwrap();
        let atoms = and_atoms(&q);
        assert_eq!(atoms.len(), 3);
        assert_eq!(
            atoms[0],
            Expr::Field {
                name: "@timestamp".into(),
                value: "x".into()
            }
        );
    }

    #[test]
    fn parse_first_equals_sign_splits() {
        let q = parse("a=b=c").unwrap();
        assert_eq!(
            q.root(),
            Some(&Expr::Field {
                name: "a".into(),
                value: "b=c".into()
            })
        );
    }

    // ---- 1.4 parse-error tests ----

    #[test]
    fn parse_rejects_missing_field_name() {
        let err = parse("=value").unwrap_err();
        assert!(err.message.contains("missing field name"), "{err}");
    }

    #[test]
    fn parse_rejects_missing_value() {
        let err = parse("field=").unwrap_err();
        assert!(err.message.contains("missing value"), "{err}");
    }

    #[test]
    fn parse_rejects_unterminated_quote() {
        let err = parse("requestId='abc").unwrap_err();
        assert!(err.message.contains("unterminated quote"), "{err}");
        assert!(parse("message=\"foo bar").is_err());
    }

    #[test]
    fn parse_rejects_unbalanced_parentheses() {
        let open = parse("(level=ERROR timeout").unwrap_err();
        assert!(open.message.contains("missing closing"), "{open}");
        let close = parse("level=ERROR) timeout").unwrap_err();
        assert!(close.message.contains("unbalanced"), "{close}");
    }

    #[test]
    fn is_valid_field_name_follows_grammar() {
        assert!(is_valid_field_name("requestId"));
        assert!(is_valid_field_name("@timestamp"));
        assert!(is_valid_field_name("a.b_c1"));
        assert!(!is_valid_field_name(""));
        assert!(!is_valid_field_name("1abc"));
        assert!(!is_valid_field_name("user name"));
        assert!(!is_valid_field_name("a-b"));
    }

    #[test]
    fn parse_rejects_dangling_or() {
        assert!(parse("level=ERROR or").is_err());
        assert!(parse("or level=ERROR").is_err());
        assert!(parse("level=ERROR or or level=WARN").is_err());
    }

    #[test]
    fn parse_rejects_empty_group() {
        let err = parse("()").unwrap_err();
        assert!(err.message.contains("empty group"), "{err}");
    }

    #[test]
    fn parse_rejects_invalid_field_names() {
        assert!(parse("1abc=x").is_err());
        assert!(parse("a!=x").is_err());
        assert!(parse("-x=1").is_err());
    }

    #[test]
    fn needs_line_text_tracks_substring_terms() {
        assert!(!parse("").unwrap().needs_line_text());
        assert!(!parse("level=ERROR service=auth").unwrap().needs_line_text());
        assert!(parse("timeout").unwrap().needs_line_text());
        assert!(parse("level=ERROR timeout").unwrap().needs_line_text());
        assert!(
            parse("level=ERROR or \"a phrase\"")
                .unwrap()
                .needs_line_text()
        );
    }

    // ---- 1.3 matcher tests (one per spec scenario, ported) ----

    #[test]
    fn substring_matches_structured_and_raw_rows_case_insensitively() {
        let q = parse("timeout").unwrap();
        let timed_out = entry(&[("message", json!("request Timeout"))]);
        assert!(q.matches("{\"message\": \"request Timeout\"}", Some(&timed_out)));
        assert!(q.matches("WARNING: connection timeout", None));
    }

    #[test]
    fn substring_without_any_match_yields_no_rows() {
        let q = parse("zzz_no_such_term").unwrap();
        assert!(!q.matches("hello world", None));
        assert!(!q.matches("{\"a\": 1}", Some(&entry(&[("a", json!(1))]))));
    }

    #[test]
    fn field_equality_matches_only_exact_entries() {
        let q = parse("level=ERROR").unwrap();
        let err = entry(&[("level", json!("ERROR"))]);
        let warn = entry(&[("level", json!("WARN"))]);
        assert!(q.matches("{\"level\": \"ERROR\"}", Some(&err)));
        assert!(!q.matches("{\"level\": \"WARN\"}", Some(&warn)));
        // Case-sensitive: lowercase value does not match.
        let lower = entry(&[("level", json!("error"))]);
        assert!(!q.matches("{\"level\": \"error\"}", Some(&lower)));
    }

    #[test]
    fn field_absent_from_entry_does_not_match() {
        let q = parse("service=auth").unwrap();
        let no_service = entry(&[("level", json!("INFO"))]);
        assert!(!q.matches("{\"level\": \"INFO\"}", Some(&no_service)));
    }

    #[test]
    fn raw_line_never_matches_field_term() {
        let q = parse("level=ERROR").unwrap();
        assert!(!q.matches("some text level=ERROR in it", None));
    }

    #[test]
    fn field_term_and_substring_combine_with_and() {
        let q = parse("level=ERROR timeout").unwrap();
        let both = entry(&[
            ("level", json!("ERROR")),
            ("message", json!("upstream Timeout")),
        ]);
        let no_timeout = entry(&[("level", json!("ERROR")), ("message", json!("all good"))]);
        let wrong_level = entry(&[("level", json!("WARN")), ("message", json!("timeout here"))]);
        let both_line = r#"{"level": "ERROR", "message": "upstream Timeout"}"#;
        let no_timeout_line = r#"{"level": "ERROR", "message": "all good"}"#;
        let wrong_level_line = r#"{"level": "WARN", "message": "timeout here"}"#;
        assert!(q.matches(both_line, Some(&both)));
        assert!(!q.matches(no_timeout_line, Some(&no_timeout)));
        assert!(!q.matches(wrong_level_line, Some(&wrong_level)));
    }

    #[test]
    fn multiple_field_terms_combine_with_and() {
        let q = parse("level=ERROR service=auth").unwrap();
        let both = entry(&[("level", json!("ERROR")), ("service", json!("auth"))]);
        let only_level = entry(&[("level", json!("ERROR")), ("service", json!("gateway"))]);
        assert!(q.matches("x", Some(&both)));
        assert!(!q.matches("x", Some(&only_level)));
    }

    #[test]
    fn quoted_phrase_matches_substring_with_spaces() {
        let q = parse("\"connection timeout\"").unwrap();
        assert!(q.matches("Error: Connection Timeout while dialing", None));
        assert!(!q.matches("connection timed out", None));
    }

    // ---- round-trip / value rendering (ported) ----

    #[test]
    fn field_equality_matches_numbers_by_text_rendering() {
        let q = parse("durationMs=2758").unwrap();
        let numeric = entry(&[("durationMs", json!(2758))]);
        let numeric_string = entry(&[("durationMs", json!("2758"))]);
        assert!(q.matches("x", Some(&numeric)));
        assert!(q.matches("x", Some(&numeric_string)));
        assert!(!q.matches("x", Some(&entry(&[("durationMs", json!(2759))]))));
    }

    #[test]
    fn body_field_compares_as_opaque_text() {
        let q = parse(r#"body={"page":7,"filter":"active"}"#).unwrap();
        let with_body = entry(&[("body", json!(r#"{"page":7,"filter":"active"}"#))]);
        assert!(q.matches("x", Some(&with_body)));
        assert!(!q.matches(
            "x",
            Some(&entry(&[(
                "body",
                json!(r#"{"page":9,"filter":"active"}"#)
            )]))
        ));
    }

    #[test]
    fn round_trip_parse_then_evaluate_mixed_terms() {
        let q = parse("service=auth cache miss").unwrap();
        let hit = entry(&[("service", json!("auth")), ("message", json!("cache MISS"))]);
        let miss = entry(&[("service", json!("auth")), ("message", json!("db ok"))]);
        let hit_line = r#"{"service": "auth", "message": "cache MISS"}"#;
        let miss_line = r#"{"service": "auth", "message": "db ok"}"#;
        assert!(q.matches(hit_line, Some(&hit)));
        assert!(!q.matches(miss_line, Some(&miss)));
        assert!(!q.matches("cache miss service=gateway text", None));
    }
}
