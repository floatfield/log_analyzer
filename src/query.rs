//! The log query language: parsing and matching.
//!
//! Grammar (design D5): whitespace-separated terms combined with AND.
//! A term containing `=` is a field-equality term (`field=value`, value is the
//! rest of the term, no spaces); any other term is a case-insensitive
//! substring term over the raw line text. Field names match
//! `[@A-Za-z_][@A-Za-z0-9_.]*`.

use std::fmt;

use serde_json::Value;

/// One parsed query term.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Term {
    /// Case-insensitive substring over the raw line text.
    Substring {
        /// Lowercased needle, precomputed at parse time.
        needle_lower: String,
    },
    /// Case-sensitive textual equality against a structured entry's field.
    Field { name: String, value: String },
}

/// A parsed query: zero or more AND-combined terms.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    terms: Vec<Term>,
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

fn valid_field_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if is_field_start(first) => {}
        _ => return false,
    }
    chars.all(is_field_continue)
}

/// Parse a query string into a [`Query`]. The empty (or all-whitespace) query
/// is valid and matches everything.
pub fn parse(input: &str) -> Result<Query, ParseError> {
    let mut terms = Vec::new();
    for term in input.split_whitespace() {
        match term.find('=') {
            None => terms.push(Term::Substring {
                needle_lower: term.to_lowercase(),
            }),
            Some(0) => {
                return Err(ParseError {
                    message: format!("missing field name before '=' in `{term}`"),
                });
            }
            Some(pos) if pos == term.len() - 1 => {
                return Err(ParseError {
                    message: format!("missing value after '=' in `{term}`"),
                });
            }
            Some(pos) => {
                let name = &term[..pos];
                if !valid_field_name(name) {
                    return Err(ParseError {
                        message: format!("invalid field name `{name}`"),
                    });
                }
                terms.push(Term::Field {
                    name: name.to_owned(),
                    value: term[pos + 1..].to_owned(),
                });
            }
        }
    }
    Ok(Query { terms })
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

impl Query {
    /// The parsed terms, in order. Test-only: production matching iterates
    /// terms internally in [`Query::matches`].
    #[cfg(test)]
    pub fn terms(&self) -> &[Term] {
        &self.terms
    }

    /// True when the query has no terms (matches every row).
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// True when any term needs the full raw line text (substring terms).
    /// Callers can skip materializing the line text for field-only queries.
    pub fn needs_line_text(&self) -> bool {
        self.terms
            .iter()
            .any(|term| matches!(term, Term::Substring { .. }))
    }

    /// Evaluate this query against one row.
    ///
    /// `line_text` is the full raw line text (used by substring terms);
    /// `entry` is the parsed JSON object for structured rows and `None` for
    /// raw rows. All terms must match.
    pub fn matches(&self, line_text: &str, entry: Option<&serde_json::Map<String, Value>>) -> bool {
        self.terms.iter().all(|term| match term {
            Term::Substring { needle_lower } => line_text.to_lowercase().contains(needle_lower),
            Term::Field { name, value } => entry
                .and_then(|map| map.get(name))
                .map(field_value_as_text)
                .is_some_and(|text| text == *value),
        })
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

    // ---- 3.1 parser tests ----

    #[test]
    fn parse_valid_queries() {
        let q = parse("level=ERROR timeout").unwrap();
        assert_eq!(q.terms().len(), 2);
        assert_eq!(
            q.terms()[0],
            Term::Field {
                name: "level".into(),
                value: "ERROR".into()
            }
        );
        assert_eq!(
            q.terms()[1],
            Term::Substring {
                needle_lower: "timeout".into()
            }
        );
    }

    #[test]
    fn parse_handles_extra_whitespace() {
        let q = parse("  level=ERROR\ttimeout  \n").unwrap();
        assert_eq!(q.terms().len(), 2);
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
    fn parse_rejects_invalid_field_names() {
        assert!(parse("1abc=x").is_err());
        assert!(parse("a!=x").is_err());
        assert!(parse("-x=1").is_err());
    }

    #[test]
    fn parse_allows_at_and_dots_in_field_names() {
        let q = parse("@timestamp=x a.b=c _d=e").unwrap();
        assert_eq!(q.terms().len(), 3);
        assert_eq!(
            q.terms()[0],
            Term::Field {
                name: "@timestamp".into(),
                value: "x".into()
            }
        );
    }

    #[test]
    fn parse_first_equals_sign_splits() {
        let q = parse("a=b=c").unwrap();
        assert_eq!(
            q.terms()[0],
            Term::Field {
                name: "a".into(),
                value: "b=c".into()
            }
        );
    }

    #[test]
    fn needs_line_text_tracks_substring_terms() {
        assert!(!parse("").unwrap().needs_line_text());
        assert!(!parse("level=ERROR service=auth").unwrap().needs_line_text());
        assert!(parse("timeout").unwrap().needs_line_text());
        assert!(parse("level=ERROR timeout").unwrap().needs_line_text());
    }

    // ---- 3.2 matcher tests (one per spec scenario) ----

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

    // ---- 3.3 round-trip / value rendering ----

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
        // Values cannot contain spaces (no quoting, design D5), so a spaced
        // body text is not expressible as one field term; compact JSON is.
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
