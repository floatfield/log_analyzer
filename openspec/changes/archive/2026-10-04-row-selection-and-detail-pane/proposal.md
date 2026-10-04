# Proposal

## Why

Users can see which fields exist (Columns panel) but cannot look at a row's full contents: long values are truncated in the table, and raw lines are only shown in a clipped single column. Row inspection is the natural next step for a log explorer — click a row, read its complete pretty-printed JSON without leaving the view.

## What Changes

- Table rows become clickable: clicking a row selects and highlights it; clicking the selected row again deselects it; selecting a different row moves the selection.
- The selection is cleared whenever a file is opened or reloaded, since line numbers may shift.
- The right panel is split vertically into two panes:
  - Upper pane: the existing column selection interface (heading, field filter, checkboxes), resizable.
  - Lower pane: a "Row detail" pane showing the selected row's contents — pretty-printed JSON for structured entries, the original text for raw lines — with a Copy button and a placeholder when no row is selected.
- Note: this behavior was implemented on the working tree in direct response to the user's request before this proposal existed; the change formalizes it. The apply pass is expected to verify the implementation against these artifacts rather than write it from scratch.

## Capabilities

### New Capabilities

(none)

### Modified Capabilities

- `log-table-view`: adds row-selection requirements (click to select/deselect/move, cleared on open/reload) and a row-detail-pane requirement (vertical split of the right panel; pretty-printed JSON for structured rows, original text for raw lines; Copy button; placeholder when nothing is selected).

## Impact

- `src/app.rs`: new `selected_line` state on the app; per-cell click hit regions in `render_table`; the right `SidePanel` becomes an "inspector" panel containing a `TopBottomPanel` (columns) plus the detail pane; new helpers (`apply_row_click`, `row_detail_text`, `row_detail`).
- Selection is session-only: not persisted to the workspace.
- Detail text is truncated beyond a generous character limit to keep the pane responsive on huge rows.
