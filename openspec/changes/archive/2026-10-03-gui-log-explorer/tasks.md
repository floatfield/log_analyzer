# Tasks

## 1. Project setup

- [x] 1.1 Add dependencies to `Cargo.toml` (`eframe`, `egui_extras`, `serde`, `serde_json`, `memmap2`, `rfd`) and create empty modules `app`, `log_file`, `query`; verify `cargo build` succeeds and `cargo run` opens an empty window
- [x] 1.2 Replace hello-world `main.rs` with the eframe entry point per design D8; verify `cargo run` shows the application window with a menu/toolbar placeholder

## 2. Log file access (`log_file.rs`)

- [x] 2.1 Implement memory map + line-offset index builder (handle final line without newline and `\r\n`) with unit tests over inline fixtures; verify tests pass and index matches line count
- [x] 2.2 Implement on-demand line retrieval by row number returning the raw line text; verify unit tests for first, middle, and last lines (including missing trailing newline)
- [x] 2.3 Implement line classification (JSON object -> structured, everything else -> raw) with unit tests covering JSON objects, non-object JSON (`123`, `[1,2]`), and noise lines from `log_exaples/first.log`; verify tests pass
- [x] 2.4 Implement streaming field-name discovery over the index and unit test it against `log_exaples/first.log` fields (`@timestamp`, `level`, `message`, `requestId`, `service`, `userId`, `durationMs`, `stackTrace`); verify the expected field set is returned

## 3. Query engine (`query.rs`)

- [x] 3.1 Implement parser producing terms per design D5 and unit tests for valid queries, whitespace handling, and malformed terms (`=v`, `f=`, empty query); verify tests pass
- [x] 3.2 Implement the matcher: case-insensitive substring over raw line text, case-sensitive `field=value` textual equality on structured entries, AND across terms; verify unit tests for each spec scenario in `specs/log-query/spec.md`
- [x] 3.3 Wire parser + matcher into a single `matches(line_text, parsed_entry) -> bool` API returning errors for invalid queries; verify round-trip unit tests (parse -> evaluate -> expected bool)

## 4. Background scanning and app wiring (`app.rs`)

- [x] 4.1 Implement open-file flow: `rfd` dialog -> mmap + background index build with progressive availability and cancellation; verify by opening `log_exaples/first.log` in the app and seeing rows appear
- [x] 4.2 Implement background query-scan worker (mpsc results, AtomicBool cancel, field-name collection piggybacked per design D6); verify a unit/integration test that scanning a fixture yields the expected matching line numbers and that a cancel flag stops the scan
- [x] 4.3 Implement app state updates: poll scan results each frame, request repaint while work is in flight, keep previously loaded state when a file open fails; verify manually and with a state-machine unit test for open-failure retention

## 5. Table UI (`app.rs`)

- [x] 5.1 Implement the virtualized table over the active row list (`egui_extras::TableBuilder`) rendering only visible rows; verify smooth manual scrolling over `log_exaples/first.log` and a unit test that the rendered row range maps to the correct line offsets
- [x] 5.2 Render structured rows into configured columns with empty cells for missing fields, and raw lines as dimmed full text in the first visible column; verify against fixture rows (JSON entry, `PANIC:` line, stack-frame line)
- [x] 5.3 Implement the column picker (fields discovered so far, defaults `@timestamp`/`level`/`message`, add/remove applies immediately); verify manual add/remove updates the table instantly

## 6. Query UI (`app.rs`)

- [x] 6.1 Add the dedicated query input with Enter-to-apply and ~250 ms debounce, progressive result updates, and row count display; verify manual filtering on `level=ERROR`, `timeout`, and `level=ERROR timeout` against the fixture
- [x] 6.2 Show invalid-query feedback (distinct input styling + hint) while showing all rows unfiltered, and restore all rows when the query is cleared; verify manual checks for `field=`, `=v`, and clearing

## 7. Integration verification

- [x] 7.1 Verify all spec scenarios from the three spec deltas against the running app using `log_exaples/first.log` (file order with out-of-order timestamps, raw interleave, column default/add/remove, query semantics, invalid feedback, clear-restore)
- [x] 7.2 Generate a synthetic multi-GB-scale log and verify open latency, scrolling responsiveness, and query cancellation behave per spec (no freeze; progressive results)
- [x] 7.3 Run `cargo test`, `cargo clippy -- -D warnings`, and `cargo fmt --check`; fix findings and verify all three pass
