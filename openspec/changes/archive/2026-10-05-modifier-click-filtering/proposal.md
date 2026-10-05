# Proposal

## Why

Drilling into a large log usually starts from a value the user can already see — a request id in a row that looks wrong. Today that value must be selected, copied, and typed into the query input by hand as `requestId='…'`. Letting the user click the cell while holding a modifier key turns that multi-step ritual into one action, which matters most exactly when the file is too large to browse.

## What Changes

- Add a configurable filter modifier key — Ctrl (default), Alt, Shift, or Cmd-or-Ctrl (the platform command modifier: Cmd on macOS, Ctrl elsewhere) — selectable in the toolbar and persisted in the workspace file.
- Clicking a table cell of a structured row with the filter modifier held appends a field-equality term for that cell to the query input — e.g. a `requestId` cell with value `abc-123` adds `requestId='abc-123'` — and the query is applied immediately, filtering the rows.
- Values are quoted per the existing query grammar (single quotes, double quotes when the value contains a single quote); terms are appended to any existing query text, combining with AND per the existing language rules. A value that cannot be represented in the grammar (contains both quote kinds or a line break) adds nothing.
- Modifier-clicks are a filtering action, not a selection action: they leave the row selection unchanged. Plain clicks keep toggling the selection exactly as before.
- Clicking cells that carry no field value — raw lines, and structured cells whose entry lacks that field — with the modifier held does nothing.

Assumptions made (not user-specified): terms are appended to the existing query (AND), not replacing it; the modifier setting lives in the toolbar next to the query input and persists in the workspace; duplicate terms are not deduplicated (appending mirrors typing).

## Capabilities

### New Capabilities

- `modifier-click-filtering`: Filtering rows by clicking a cell with a user-configured modifier key held: the modifier setting, the generated field-equality term, append-and-apply semantics, and which cells are eligible.

### Modified Capabilities

- `log-table-view`: The "Selecting rows by clicking" requirement is scoped to clicks made without the filter modifier, so modifier-clicks and selection clicks remain unambiguous, disjoint actions.

## Impact

- `src/app.rs`: modifier setting state + toolbar control; cell click handling gains a modifier branch that formats the term, appends it to `QueryUi::input`, and applies the query immediately; term-quoting helper.
- `src/persistence.rs`: `Workspace` gains a persisted `filter_modifier` field (backward-compatible serde default).
- `src/query.rs`: unchanged — the generated terms use the existing grammar and parser.
- Existing tests unaffected; new unit tests for term formatting, append semantics, persistence, and click dispatch.
