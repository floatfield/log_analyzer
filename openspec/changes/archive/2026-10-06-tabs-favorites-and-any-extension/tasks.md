# Tasks

## 1. Sorted favorites

- [x] 1.1 Add `Workspace::sorted_favorites()` returning a clone sorted by lowercased path (stable on ties), leaving the persisted order untouched; unit-test ordering including a case-insensitive pair and tie stability, and that `workspace.favorites` itself is unchanged (`cargo test sorted_favorites_orders_case_insensitively`)
- [x] 1.2 Render the Favorites menu from `sorted_favorites()`; verify marking order no longer decides display order (`cargo build` + manual menu check)

## 2. Open any extension

- [x] 2.1 Remove the `add_filter` calls from the Open dialog so `rfd::FileDialog::new().pick_file()` offers every file; verify a non-`.log` file (e.g. `.out`, extensionless) opens, classifies lines, and discovers fields (`cargo build` + manual open check)

## 3. Modifier-click rewrites the query input

- [x] 3.1 Turn `QueryUi::append_filter_term` into `replace_filter_term` (assign the term to `input` instead of appending; keep `note_edit()` and `force_next_apply = true`), rename the `render_table` call site, and rework the unit test to cover: an empty input becomes exactly the term; input `level=ERROR service='auth'` becomes only `requestId='abc-123'`; the forced apply still fires without typing or pressing Enter (`cargo test replace_filter_term_rewrites_input`)

## 4. Tab state model

- [x] 4.1 Introduce `OpenTab` (file, rows, query, cache, selected_line, column_filter) and replace `LogAnalyzerApp`'s per-file fields with `tabs: Vec<OpenTab>` + `active: usize`; add `active_tab/active_tab_mut` accessors and mechanically move every use site (toolbar, columns panel, detail, status, poll, snapshot_view, reload, tests) onto the active tab; keep the build green with single-tab behavior identical to today (`cargo test`, `cargo clippy -- -D warnings`)
- [x] 4.2 Rework the open flow: focus the existing tab on `path_key` match without re-reading or resetting its state; otherwise push, activate, and `spawn_open_jobs` without cancelling other tabs' jobs (delete the old cancel-previous step); failures set `error` and leave all tabs untouched; unit-test duplicate-open focus + preserved query, and that opening a second file keeps the first tab's file and state (`cargo test open_focuses_existing_tab_and_preserves_context`, `cargo test opening_second_file_keeps_first_tab`)
- [x] 4.3 Move `reload_current` to re-open the active tab's path into that same tab, and scope `poll`'s busy-repaint to the active tab; verify the existing reload test still passes unchanged (`cargo test reload_picks_up_appended_lines_and_keeps_query`)

## 5. Tab strip UI

- [x] 5.1 Add the tab strip (`TopBottomPanel::top("tabs")` under the toolbar, hidden with no tabs): per tab a `selectable_label(file_name)` with full-path tooltip plus a `✕` close button; label click switches `active`; close cancels that tab's `open_cancel` and active scan, removes it, and clamps `active` to the left neighbor; verify switching, closing inactive/active/last tabs, and the empty state (`cargo build` + manual strip check, `cargo test`)
- [x] 5.2 Keep every per-file control wired to the active tab end-to-end: query apply/scan, Reset, star/favorite, columns panel + its per-tab filter text, row selection/detail, status bar counts; confirm two tabs hold independent queries and selections across switches (`cargo test`, manual two-tab pass)

## 6. Integration verification

- [x] 6.1 Launch on two fixtures: open A, query it, open B (A keeps loading), switch back (query and selection intact), reopen A's path from favorites (focuses existing tab, no duplicate), close tabs down to the empty state; confirm the Favorites menu lists entries sorted and the Open dialog offers an extensionless file; with a non-empty query in the input, modifier-click a cell and confirm the input is rewritten to the clicked term alone and the rows filter by it

> Note: the Open dialog exercises a pre-existing crash in this environment —
> the synchronous `rfd::FileDialog::pick_file()` inside `update()` blocks the
> egui frame on xdg-desktop-portal and the process aborts around dialog
> spawn/result. Confirmed pre-existing: HEAD had the identical blocking call
> (`git show HEAD:src/app.rs` lines 794–797); this change only removed its
> `add_filter` lines. The dialog's spec claim (no extension filter, every file
> offered) was still verified live via the portal chooser. Fixing the sync
> dialog (async rfd channel) is out of scope for this change.
