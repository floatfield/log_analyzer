# Design

## Context

`LogAnalyzerApp` holds exactly one open file: `file: Option<LoadedFile>` plus app-level `rows: RowSource`, `query: QueryUi`, `cache: ParseCache`, `selected_line`, and `column_filter` (src/app.rs). `on_open_result_inner` cancels the old file's background jobs and resets all of that state on every successful open; `LoadedFile` already owns its own mmap, shared index, fields, and `open_cancel` flag, and `spawn_open_jobs` threads are keyed to that flag. The Open dialog (update loop) adds an rfd filter `"Log files" &["log","txt"]` before an `"All files"` filter; nothing else in the pipeline looks at extensions. The Favorites menu renders `workspace.favorites` in persisted (mark) order. egui 0.33 ships no Tabs widget. Existing specs: log-file-access / Opening a log file; workspace-persistence / File favorites; behavior of query/scan/selection per file is unchanged by this change.

## Goals / Non-Goals

**Goals:**

- Several logs open at once, one tab each, with the whole explored view (file, query, rows, selection, columns filter) swapping atomically on tab switch.
- Opening a file never destroys another tab's work or background loading.
- Favorites list is trivially scannable: sorted, case-insensitively.
- Any file extension opens exactly like `.log`.

**Non-Goals:**

- Persisting the open-tab set across restarts (session-only; favorites cover reopen).
- Drag-and-drop reordering of tabs, tab overflow scrolling UI, or middle-click close.
- Per-tab scroll position restoration.
- Drag-and-drop of files onto the window; recent-files list; watcher/auto-reload.
- Changing the workspace file format (favorites stay an ordered path list; sorting is presentation-only).

## Decisions

- **D1: `OpenTab` struct owns all per-file state; the app holds `tabs: Vec<OpenTab>` + `active: usize`.** `struct OpenTab { file: LoadedFile, rows: RowSource, query: QueryUi, cache: ParseCache, selected_line: Option<usize>, column_filter: String }` — exactly today's per-file fields, moved inward. App-level leftovers: `error`, `workspace`, `filter_modifier`, `workspace_path`. Accessors `active_tab()/active_tab_mut()` return `Option<&…>`; every current `self.file/rows/query/cache/selected_line/column_filter` use site reads them through the active tab. Alternative: keep app-level state and swap it out/in per switch — rejected, two sources of truth for the same state.
- **D2: Switching tabs is a pure index change.** The mmap, index, parsed state, `RowSource`, `QueryUi` (input *and* applied/invalid state), selection, and parse cache all live in the tab, so activation is `self.active = i` and the next frame renders from that tab. No reindex, no rescan, no state copying.
- **D3: Open flow: focus-if-open, else push + activate + spawn jobs — and never cancel other tabs.** `open_path_inner` computes `path_key(path)`; if some tab matches, set `active` to it and return (no re-read — Reload exists for that, and spec: the duplicate's context is preserved). Otherwise mmap as today, push the tab, set `active` to the new index, `spawn_open_jobs()` for it, and leave every other tab's `open_cancel`, scans, and state untouched. `on_open_result_inner`'s "cancel the old file" step is deleted. Failure path unchanged: set `self.error`, keep all tabs as they were. The initial `--path` argument opens tab 0.
- **D4: Tab strip = custom widgets in a `TopBottomPanel::top("tabs")` right under the toolbar, shown only when `tabs` is non-empty.** egui 0.33 has no Tabs widget (verified: no `tabs.rs` under egui src/widgets). Per tab, one horizontal group: `ui.selectable_label(i == active, file_name)` with `.on_hover_tooltip(full_path)`, then a small `"✕"` button; the label click sets `active`. Close: cancel that tab's `open_cancel` and its `RowSource::Matched` scan, remove from the vec, then `active = active.min(tabs.len().saturating_sub(1))` — which activates the left neighbor, or the empty state when the last tab closes. Empty state = today's no-file placeholders (central panel + status bar), which key off "no active tab".
- **D5: Poll, status, Reload, star, detail, and columns panel all operate on the active tab; busy-repaint considers only it.** `poll`'s query-apply/scan machinery is unchanged but reads/writes the active tab's `QueryUi`/`rows`. Background tabs need no repaint driving: nothing observes them until they are activated, and D2 renders whatever has loaded by then. `reload_current` re-reads the active tab's path into that same tab (keeping its query re-arm behavior); other tabs are untouched.
- **D6: Favorites sorting lives in `Workspace::sorted_favorites()` (persistence.rs), presentation-only.** Returns a clone sorted by `path.to_lowercase()` (stable sort keeps ties in insertion order); the Favorites menu iterates that instead of `workspace.favorites`. The persisted Vec order is untouched, so workspace files stay byte-compatible and the change is testable as a pure function.
- **D7: The Open dialog drops all `add_filter` calls.** `rfd::FileDialog::new().pick_file()` shows all files on every platform; `LoadedFile::open` has no extension logic (never did), so any picked file opens and is classified per line exactly as before. The `--path` CLI argument was already extension-agnostic.
- **D8: Modifier-click rewrites the query input, superseding the archived append behavior.** `QueryUi::append_filter_term` becomes `replace_filter_term(term)`: `self.input = term.to_owned()`, followed by the unchanged `note_edit()` + `force_next_apply = true` pair — the empty/non-empty append branch is dropped, poll's forced-apply consumption and `reset()`'s flag-clearing are untouched, and the `render_table` call site is renamed. Discarding a hand-composed multi-term query is the intended semantics (one click = one filter), not a risk to mitigate.

## Risks / Trade-offs

- [One mmap per open tab; many large tabs consume address space/RAM] → Accepted: bounded by what the user explicitly opens; closing a tab drops its mmap. A tab LRU/eviction policy was rejected as surprising data loss.
- [Per-tab `ParseCache` multiplies memory by tab count (4096 entries each)] → Accepted for now: entries are small and bounded; a shared cache keyed by `(tab, line)` was rejected as needless complexity until tabs get heavy use.
- [Background indexing of hidden tabs burns CPU while invisible] → Accepted: it is the same total work as before (previously it was merely cancelled); finishing early makes switching instant.
- [`active.min(len-1)` close semantics pick the left neighbor; no memory of "previous tab"] → Accepted: simple and predictable; MRU-order activation was rejected as fiddly.
- [Dropping the dialog filter removes the discoverable "Log files" preset] → Accepted: the user asked for any-extension opening; a filter preset would fight that on platforms where the dialog starts on the first filter.
- [Sorting favorites by lowercase path only (no natural/numeric ordering)] → Accepted: deterministic, matches the spec scenario; fancier collation is out of scope.

## Migration Plan

Pure in-memory state refactor + UI additions + one dialog call; the workspace file format is unchanged, so nothing migrates. Old workspaces load as before; favorites merely *display* sorted. Rollback = revert the commit.

## Open Questions

(none)
