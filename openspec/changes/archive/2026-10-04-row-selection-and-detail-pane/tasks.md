# Tasks

## 1. Row selection state and click handling

- [x] 1.1 Add `selected_line: Option<usize>` to the app state, initialized to `None` and cleared in the file-open path, with a unit test proving an open resets the selection (`cargo test row_selection_clears_on_open`)
- [x] 1.2 Add the `apply_row_click` toggle/move helper with a unit test covering select, deselect, and move cases (`cargo test row_click_toggles_and_moves_selection`)
- [x] 1.3 Make table cells clickable in `render_table` (per-cell hit regions with pointing-hand cursor, in both the column layout and the raw single-column layout), passing the selection through from the central panel, and paint the selection tint over the severity tint on the selected row; verify the app compiles and existing table tests stay green (`cargo test`)

## 2. Split right panel and detail pane

- [x] 2.1 Convert the right side panel into a vertically split inspector: resizable upper pane holding the existing Columns UI (heading, filter, checkboxes) unchanged, with the detail pane below (`cargo build` and manual layout check)
- [x] 2.2 Add the `row_detail_text`/`row_detail` helpers (pretty-printed JSON for parsed objects, lossy raw text otherwise, truncated beyond the detail limit) with unit tests for JSON pretty-printing, raw passthrough, and truncation (`cargo test row_detail_pretty_prints`)
- [x] 2.3 Render the detail pane: "Row detail" heading, line number, Copy button (active only with a selection, copying the pane text), and the placeholder when nothing is selected (`cargo build` and manual check with no file open)

## 3. Integration verification

- [x] 3.1 Run the full quality gate: `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check` — all green
- [x] 3.2 Launch the app on the E2E fixture, click a structured row, and capture a screenshot showing the selected-row highlight, the split right panel, and the pretty-printed JSON detail; also confirm a raw line row shows its original text and Copy is inactive with no selection
