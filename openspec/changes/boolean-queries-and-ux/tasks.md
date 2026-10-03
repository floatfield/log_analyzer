# Tasks

## 1. Query engine rewrite (`src/query.rs`)

- [ ] 1.1 Add a tokenizer (bare words, single/double-quoted lexemes, `field=value` terms, parens, standalone case-insensitive `or`) with unit tests for quoted values containing spaces and an unterminated-quote error; verify with `cargo test query`
- [ ] 1.2 Implement the recursive-descent parser to the `Expr` AST (AND-by-juxtaposition binding tighter than OR, nested groups) with unit tests for `or` case-insensitivity, precedence on `level=ERROR timeout or service=auth`, and the proposal example `(requestId='a' or requestId='b') message="foo bar"`; verify with `cargo test query`
- [ ] 1.3 Implement AST matching (`Substring` case-insensitive on full line text incl. raw lines; `FieldEq` case-sensitive on structured entries only) keeping the public query API shape; port existing `query.rs` tests unchanged and verify all pass with `cargo test query`
- [ ] 1.4 Cover parse errors (`=value`, `field=`, unbalanced `(`/`)`, unterminated quote) returning an error result; verify with `cargo test query`

## 2. Query UI (`src/app.rs`)

- [ ] 2.1 Route new parse-error kinds through the existing invalid-query path (indicator shown, all rows listed); verify via the `QueryUi` state-machine tests with `cargo test query_ui`
- [ ] 2.2 Add a reset button next to the query input that clears the text and invalid flag, restoring all rows; verify via a `QueryUi` test (reset after filtered/invalid state) with `cargo test query_ui`

## 3. Light theme and row highlighting (`src/main.rs`, `src/app.rs`)

- [ ] 3.1 Set `egui::Visuals::light()` at startup; verify the app launches in light theme with a screenshot
- [ ] 3.2 Implement level classification (`error` → red, `warn`/`warning` → yellow, case-insensitive; raw lines and other levels untinted) with unit tests for the classifier; verify with `cargo test app`
- [ ] 3.3 Paint translucent per-cell row backgrounds for classified rows on the light theme; verify visually on a fixture containing error/warn/info/raw rows with a screenshot

## 4. Columns panel filter (`src/app.rs`)

- [ ] 4.1 Add a filter function (case-insensitive substring on field names) with unit tests including the empty-filter passthrough; verify with `cargo test app`
- [ ] 4.2 Add the filter `TextEdit` above the Columns panel list, rendering only matching names while leaving selection and table columns untouched; verify with an app test (filter applied → hidden selected column stays in the visible-column set) and visually with a screenshot

## 5. Workspace persistence (`src/persistence.rs`, `src/app.rs`)

- [ ] 5.1 Add the `dirs` dependency and create `src/persistence.rs`: `Workspace` (favorites + per-file columns) with load/save using atomic write (temp + rename) and injectable file path for tests; unit tests for round-trip and corrupted/missing file → empty workspace; verify with `cargo test persistence`
- [ ] 5.2 Wire loading in `LogAnalyzerApp::new` and write-through saves on mutations, resolving the config dir via `dirs` with an in-memory no-op fallback when unavailable; verify with an app test using an injected test config path; `cargo test app`
- [ ] 5.3 Add favorites UI: toolbar star toggle for the open file and a favorites list with click-to-open and per-row remove; verify with app tests (toggle updates and persists the workspace; removing an open file's favorite unmarks the star) with `cargo test app`, then visually with a screenshot
- [ ] 5.4 Restore per-file columns: on field-discovery completion apply the saved selection when present, else defaults; column changes update the saved selection; verify with an app test (save selection → fresh app instance → reopen → same columns) with `cargo test app`

## 6. Integration verification

- [ ] 6.1 Run `cargo test`, `cargo clippy -- -D warnings`, and `cargo fmt --check`; fix any fallout
- [ ] 6.2 End-to-end pass on a fixture: boolean query with grouping and quoted values, `or` precedence, reset button, invalid-query feedback, row highlighting, column filter, favorites and column restore across a restart; capture screenshots as evidence
- [ ] 6.3 Re-run the `#[ignore]` scale test to confirm the AST-evaluating hot loop keeps its budgets (first rows < 1s, cancel < 2s); verify with `cargo test -- --ignored --nocapture`
