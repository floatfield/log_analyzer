# Proposal

## Why

Three friction points in day-to-day use: (1) comparing or correlating several logs means reopening files one at a time — each new Open replaces the log the user was just reading; (2) the favorites list grows in mark order, so finding a file means scanning an unsorted list; (3) the open dialog steers users toward `.log`/`.txt` files even though the app happily reads any line-oriented text (JSON-per-line or raw), so files with other extensions (`.out`, `.jsonl`, `.1`, or none) feel unsupported. Additionally, modifier-clicking a cell currently *appends* its term to the query (AND-combined), so drill-downs accumulate into a hand-composed query the user must manually clean up before filtering for something else; a click should simply filter for the clicked value.

## What Changes

- **Tabs for opened logs.** Every opened file gets a tab in a tab strip under the toolbar. Opening a file adds/focuses a tab instead of replacing the current one; each tab keeps its own session context (query, row source, row selection, columns-panel filter) while it is open, so switching away and back restores exactly that view. Opening a file that is already open focuses its existing tab. Tabs have a close (✕) control; closing cancels that file's background work and activates a neighboring tab. Opening another file no longer cancels the previous file's background indexing.
- **Sorted favorites.** The Favorites menu lists favorite paths sorted alphabetically (case-insensitively), regardless of the order they were marked.
- **Open any file extension.** The open dialog no longer filters by extension: any file can be picked and opens like a `.log` file (line classification already decides structured vs raw per line, independent of extension).
- **Modifier-click rewrites the query.** Modifier-clicking an eligible cell forms its `field='value'` term and *replaces* the query input's contents with it (previous query text is discarded), then applies immediately — instead of the current append/AND-combine behavior.

Assumptions (recorded, not user-confirmed): open tabs are session-only and are not persisted across restarts (favorites remain the way back); per-tab context means query text + applied filter + selection + columns-panel filter, not scroll position; sort order is the full path string compared case-insensitively; the tab strip shows the file's name with the full path as a tooltip.

## Capabilities

### New Capabilities
- `log-tabs`: Opening logs in tabs, switching between them with per-tab session context, closing tabs, and focusing an already-open file instead of duplicating it.

### Modified Capabilities
- `workspace-persistence`: The File favorites requirement gains the sorted-list behavior (the favorites list is presented sorted case-insensitively by path).
- `log-file-access`: The Opening a log file requirement is widened: the open dialog offers files of any extension, and any picked file opens like a log file.
- `modifier-click-filtering`: The Filter term from a modifier-click requirement changes from append/AND-combine to replace: the formed term rewrites the query input and is applied alone.

## Impact

- `src/app.rs`: app state becomes a collection of open tabs plus an active index (per-tab `LoadedFile`, `RowSource`, `QueryUi`, selection, parse cache, columns filter); toolbar gains a tab strip; Open/favorites/reload/status/detail read from the active tab; `on_open_result_inner` stops cancelling the previous file's background jobs; dialog drops the extension filter; `QueryUi::append_filter_term` becomes a replace operation (forced-apply flag machinery unchanged).
- `src/persistence.rs`: unchanged data shape (favorites stay a path list; sort is presentation-order).
- No new dependencies; egui 0.33 has no built-in Tabs widget, so the strip is custom widgets.
- Existing specs otherwise unaffected: query language, table rendering, and selection behave per tab exactly as today per file; the archived `modifier-click-filtering` append semantics are amended by this change's delta.
