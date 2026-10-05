# Tasks

## 1. Filter modifier setting and persistence

- [x] 1.1 Add `FilterModifier` (Ctrl/Alt/Shift/Command) with lowercase serde representation and the mapping to `egui::Modifiers`, plus a `Workspace::filter_modifier` field with `#[serde(default)]`; prove old workspace files load as Ctrl and the choice round-trips (`cargo test filter_modifier_defaults_and_round_trips`)
- [x] 1.2 Add the `filter_modifier` field to `LogAnalyzerApp` (initialized from the loaded workspace, written through on change) and the toolbar combo box next to the query input; verify changing it persists immediately (`cargo test filter_modifier_change_persists`, `cargo build` + manual toolbar check)

## 2. Term generation

- [x] 2.1 Expose `query::is_valid_field_name` and add the pure `filter_term(field, value) -> Option<String>` helper (single-quote default, double quotes when the value contains a single quote, `None` for both-quote/newline values and invalid field names, no display truncation) with unit tests covering each branch and the clicked-row-matches-its-own-term property (`cargo test filter_term_formats_and_rejects_values`)

## 3. Click dispatch and query application

- [x] 3.1 Add `QueryUi::append_filter_term` with the `force_next_apply` flag consumed by `poll`'s apply decision; unit-test that appending to an empty and a non-empty input yields the AND-combined text and applies without the debounce wait (`cargo test append_filter_term_forces_apply`)
- [x] 3.2 Thread the modifier and `&mut QueryUi` into `render_table` and dispatch each cell's click: configured modifier held → filter path (append term, no selection change), otherwise the existing selection toggle; verify the app compiles and existing tests stay green (`cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check`)

## 4. Integration verification

- [x] 4.1 Launch the app on the E2E fixture, Ctrl-click a structured cell filled with text, and confirm the query input receives `requestId='…'` and the rows filter; confirm plain click still toggles selection, modifier-click leaves the selection untouched, and raw/empty cells do nothing
