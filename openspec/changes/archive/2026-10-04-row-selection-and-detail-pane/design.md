# Design

## Context

The right `SidePanel` currently holds only the column selection UI; the table's cells render non-interactive labels, so rows cannot be inspected. Selection must survive virtualized scrolling and query re-applies, which reshuffle row indices but not file line numbers. (Motivation: proposal.md.)

## Goals / Non-Goals

**Goals:**

- Click-to-select on every rendered row (structured and raw, in both the per-column and single-column raw layouts) with visible feedback.
- Detail pane reads the selected line on demand through the existing shared index — no new precomputation, no memory growth with file size.
- Selection stays correct across filtering: track the file line number, not the table row index.

**Non-Goals:**

- Persisting the selection across restarts or into the workspace file.
- Multi-row selection, keyboard navigation, or selection-driven actions (e.g., filter-by-row).
- Syntax highlighting or folding in the detail pane.

## Decisions

- **D1: Selection keyed by file line number (`Option<usize>` on the app).** Table row indices change whenever a query re-applies or the index grows; line numbers are stable for the open file. Opening/reloading replaces the file, so `on_open_result_inner` resets the field. Alternative considered: storing the row index — rejected because a re-filter would silently repoint the selection at an unrelated row.
- **D2: Click detection via per-cell `Ui::interact` hit regions.** Each cell overlay uses `ui.max_rect()` (the full cell area before content) with id `("row-hit", line_no, column)` and `Sense::click()`, plus a pointing-hand hover cursor. `egui_extras::Table` offers no row-click hook, and per-cell regions work uniformly in both the columns layout and the raw single-column layout. Clicking any cell of a row toggles through the shared `apply_row_click` helper. Two hit-testing constraints discovered during verification shape the cell code: (1) the overlay must be created **after** the cell content — the topmost widget wins egui's hit test, and a truncated label whose galley overflows the cell otherwise shadows clicks landing on text, so only empty cell space would select; (2) cell labels set `show_tooltip_when_elided(false)` — the elided-text hover tooltip pops a layer over the pointer that blocks clicks to layers behind it.
- **D3: Selection tint painted over the severity tint.** The selected row overpaints the level tint with a translucent blue (`SELECTED_ROW_TINT`), so error/warn rows still read as both selected and severe. Keeping two translucent paints preserves the existing severity spec behavior without changing `level_tint`.
- **D4: Right panel split with `SidePanel::right` + `TopBottomPanel::show_inside`.** The columns UI moves unchanged into a resizable top pane (`default_height` 340, `min_height` 140); the detail pane fills the remainder. Alternative considered: two `SidePanel`s — rejected, egui cannot nest two right side panels vertically.
- **D5: Detail text via `serde_json::to_string_pretty` on the parsed object, raw lossy text otherwise, truncated at `DETAIL_MAX_CHARS` (20k chars).** Reuses `log_file::parse_object` (same classifier as the table), keeps the pane cheap on huge rows, and mirrors `truncate_chars` used for cells. A Copy button passes the pane text to `Context::copy_text`; it is disabled with no selection.

## Risks / Trade-offs

- [Pretty-printing sorts keys (BTreeMap ordering), so key order differs from the file] → Accepted: sorted output is deterministic and matches how cells already read fields; raw text remains one click away on unfiltered rows.
- [Interact overlays sit under the header row's resize handles at table edges] → Cell ids are unique per (line, column) and the overlay only claims clicks, so drag-resize on headers is unaffected.
- [Selected line truncated to 20k chars in the pane] → Generous for log inspection; full raw text is still in the file and copyable there.
- [Selection lost on reload even if line count is unchanged] → Accepted: reload may change any line; re-selecting is one click.

## Migration Plan

Single-binary UI change; no data migration. Rollback = revert the commit.

## Open Questions

(none)
