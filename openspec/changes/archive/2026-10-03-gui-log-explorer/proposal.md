# Proposal

## Why

Log analysis today requires ad-hoc `grep`/`jq` pipelines that are slow to iterate on and give no overview of a file's structure. The project needs a cross-platform desktop GUI that opens log files (large ones included) and lets a user browse, filter, and query them interactively without the UI freezing or the whole file being loaded into memory.

## What Changes

- Turn the hello-world binary into a cross-platform desktop application built on egui/eframe.
- Add log file opening (native file picker) with lazy access: lines are indexed by byte offset and read on demand, so multi-hundred-MB / multi-GB files open instantly.
- Parse each line as JSON when possible; keep non-JSON lines and display them as raw-text rows in file order.
- Present log entries as rows in a virtualized table where only visible rows are rendered.
- Add configurable columns: the user chooses which JSON fields are shown as columns (defaults provided).
- Add a query language entered in a dedicated query input that matches by JSON field values (`field=value`) and by free-text substring; query results filter the table.
- The UI remains responsive while scrolling and querying regardless of file size.

Out of scope for this change: time-range filters, OR/NOT query operators and parentheses, querying into nested JSON (the `body` field is treated as opaque text), multi-file workspaces, export, and log tailing/streaming.

## Capabilities

### New Capabilities

- `log-file-access`: Opening and lazily reading log files - line-offset indexing, on-demand line retrieval, JSON vs non-JSON line classification, and behavior for very large files.
- `log-table-view`: The table presentation of log lines - virtualized scrolling, row content for JSON and non-JSON lines, file-order row sequence, and user-configurable visible columns.
- `log-query`: The query language - syntax, matching semantics for field-value and substring terms, combination of terms, and how results affect the visible rows.

### Modified Capabilities

(none - no specs exist yet)

## Impact

- `src/main.rs` becomes the eframe app entry point; new modules under `src/` (file access/indexing, query parsing/matching, UI) with colocated unit tests.
- `Cargo.toml` gains dependencies: `eframe`/`egui`, `serde_json` (line parsing), `memmap2` (lazy file access), `rfd` (native file dialog). Cross-platform: Windows, macOS, Linux.
- Performance contract: opening a file must not require parsing it in full; scrolling and filtering must not parse the whole file.
- No external APIs or data formats beyond reading local log files.
