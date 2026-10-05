# Design

## Context

Cell clicks already route through per-cell `ui.interact` hit regions in `render_table` (spec: log-table-view / Selecting rows by clicking); the overlay must stay the topmost widget and elided-label tooltips stay suppressed (design of row-selection-and-detail-pane, D2). The query input (`QueryUi`) applies text edits on a 250 ms debounce or on Enter. The workspace file already persists global settings additively with serde defaults. The query grammar accepts `field='value'` / `field="value"` with juxtaposition = AND; field names are `[@A-Za-z_][@A-Za-z0-9_.]*`. (Motivation: proposal.md.)

## Goals / Non-Goals

**Goals:**

- One modifier-click turns a visible cell value into a filter, with the rows updating in the same breath.
- The modifier is a real user choice (Ctrl / Alt / Shift / platform command), remembered across restarts.
- Selection behavior stays exactly as shipped for plain clicks; the two actions never collide.

**Non-Goals:**

- Removing terms from the query by modifier-clicking again, or any other term editing beyond appending.
- OR-combining repeated modifier-clicks, deduplication of identical terms, or any grouping/parenthesis generation.
- Configuring multiple simultaneous modifiers (e.g. Ctrl+Shift) or mouse-button choice.
- Making raw lines filterable through clicks.

## Decisions

- **D1: `FilterModifier` enum persisted as a string in the workspace.** `enum FilterModifier { Ctrl, Alt, Shift, Command }`, serialized lowercase (`"ctrl"` …) into a new `Workspace::filter_modifier` field with `#[serde(default)]`, so existing workspace files load unchanged and degrade to Ctrl. `Command` maps to egui's `Modifiers::command()` — Cmd on macOS, Ctrl elsewhere — giving macOS users the requested cmd behavior with one portable option. `LogAnalyzerApp` keeps a `filter_modifier` field initialized from the loaded workspace and written through on change. Alternative considered: session-only setting — rejected, the user explicitly wants to configure it once.
- **D2: Toolbar combo box next to the query input.** A `egui::ComboBox` labeled `Filter modifier` after the Reset button; changing it updates the app field and calls `save_workspace()` immediately. Alternatives considered: a settings menu — rejected, one control deserves no menu; config-file-only — rejected, discoverability matters for a mouse gesture.
- **D3: Read modifiers from frame input at the click site; dispatch in `render_table`.** When a cell's hit region reports `clicked()`, read `ui.input(|i| i.modifiers)` and test the configured flag. Matching → filter path; not matching → existing `apply_row_click`. `render_table` gains two parameters: the configured `FilterModifier` and `&mut QueryUi` (same borrow pattern as the existing `&mut self.cache` / `&mut self.selected_line`). Alternative considered: egui's per-interaction modifier APIs — the frame input read is simpler and equally correct because clicks and modifiers are read from the same frame's input state.
- **D4: `filter_term(field, value) -> Option<String>` builds the term from the entry value, never from the displayed text.** The value is `field_value_as_text(entry.get(column))` — the exact text field-equality compares (query.rs eval) — so the clicked row itself always matches the generated term. Formatting: `field='value'` by default; `field="value"` when the value contains a single quote; `None` when it contains both quote kinds or a line break (unrepresentable), and `None` when the field name is not a valid query field name (expose `query::is_valid_field_name`; JSON keys are arbitrary strings, and an unqueryable name would otherwise poison the whole query into "invalid"). Display truncation (`MAX_CELL_CHARS`) is not applied — a truncated value would filter wrongly. The helper is pure and unit-tested.
- **D5: Append with immediate forced apply.** `QueryUi::append_filter_term(term)`: appends ` term` (or `term` when the input is empty), then forces an apply on the same/next frame — implemented as a `force_next_apply` flag that `poll` ORs into its existing `take_pending_apply(force)` call, so the table filters without waiting out the 250 ms debounce. On the query-is-invalid path nothing changes: if the user had already typed an invalid query, the appended term keeps it invalid and the existing invalid-query feedback shows all rows.
- **D6: Modifier path never touches selection.** The filter branch returns before `apply_row_click`; the selection is untouched (spec: modifier-click does not change selection).

## Risks / Trade-offs

- [AND-appending a second value of the same field yields an empty result (`requestId='a' requestId='b'`)] → Accepted: matches the requested "added to the search field" semantics and the language's AND rule; the user edits the text or hits Reset. OR-chaining was rejected as surprising implicit logic.
- [No term deduplication: clicking the same cell twice appends twice] → Accepted: harmless semantically, mirrors typing; dedupe logic would fight manual edits of the input.
- [`Modifiers::command()` is platform-defined, so "Cmd" on macOS is really Ctrl elsewhere] → Intended: one portable "command" option covers the user's cmd-on-macOS example; Ctrl remains the explicit default.
- [Huge cell values produce huge query terms] → Accepted: correctness over aesthetics — truncating would silently filter for the wrong value; the input remains editable.
- [Modifier-click inside a text edit or on a header has no special behavior] → Out of scope: only table cell hit regions are affected; existing widgets keep their defaults.

## Migration Plan

Additive UI + workspace field with serde default; no data migration. Old workspace files load as Ctrl. Rollback = revert the commit.

## Open Questions

(none)
