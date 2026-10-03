# Design

## Context

The crate is a hello-world scaffold (edition 2024, no dependencies), so this change defines the initial architecture. Target behavior and requirements are in proposal.md and the three spec deltas (`log-file-access`, `log-table-view`, `log-query`). The sample fixture `log_exaples/first.log` (directory name intentionally kept as-is) exercises JSON entries, non-JSON noise lines, bare stack-frame lines, and out-of-order timestamps.

## Goals / Non-Goals

**Goals:**

- UI thread never performs work proportional to file size; all full-file work happens on background threads.
- Opening a file costs one sequential index pass, not a full parse.
- Scrolling costs O(visible rows), each read straight from the file.
- A single, testable query engine independent of the UI.

**Non-Goals:**

- No persistent config storage (column choices are per-session; eframe storage can come later).
- No async runtime (tokio etc.) - plain threads + channels are sufficient.
- No write/modify capability, no tailing, no multi-file workspaces.

## Decisions

### D1: egui/eframe over Tauri

Immediate-mode rendering means per-frame cost is inherently bounded to what is drawn; virtual scrolling falls out of `egui`'s `ScrollArea::show_rows` (or `egui_extras::TableBuilder`, which virtualizes rows and gives resizable columns for free). Single language, single binary, `cargo build` works on Windows/macOS/Linux with no JS toolchain.
*Alternatives:* Tauri (web toolchain + IPC chunking complexity, rejected for MVP), Iced/Slint (less proven large-table virtualization).

### D2: Memory map + line-offset index

`memmap2::Mmap` over the file; one background pass builds `Vec<u64>` of line-start byte offsets (scanning for `\n`, handling a final line without newline and `\r\n`). Row *n* is the byte range `offset[n]..offset[n+1]`. Opening = mmap + index pass; nothing is parsed up front.
*Alternatives:* `BufReader` scan (equally correct, slightly more copies; mmap chosen for zero-copy slicing); storing parsed rows in memory (rejected: latency and memory for GB files).

### D3: Parse on demand, small LRU cache

JSON parsing happens only for rows being displayed or evaluated. An LRU cache (a few thousand entries) of parsed `serde_json::Value`s avoids reparsing during back-and-forth scrolling. Structured = top-level JSON object; anything else (including non-object JSON) is a raw line, kept as its original byte slice.

### D4: Query evaluation as a background matching scan

Filtering cannot happen lazily per-frame because virtual scrolling needs the result count up front. On query apply (Enter, or ~250 ms debounce), a background thread streams the file via the offset index, parses each line, evaluates the query, and collects matching line numbers into `Vec<u32>`. The table virtualizes over that vector; a cancellation flag (AtomicBool) aborts the scan when the query changes. Results update progressively as the scan proceeds, so partial results appear immediately on large files.
*Alternatives:* incremental per-page evaluation (complex scroll semantics), persistent field index (premature for MVP).

### D5: Query language semantics

Grammar (whitespace-separated terms, AND semantics):

```
query  := term*
term   := field_term | substring
field  := [@A-Za-z_][@A-Za-z0-9_.]*   (top-level and dotted names reserved for later nesting; `@` allowed for keys such as `@timestamp`)
field_term := field "=" value        (value = rest of term, no spaces; quoting out of scope)
```

- Substring term: case-insensitive `contains` over the raw line text (structured and raw rows).
- Field term: case-sensitive textual equality against the field's value rendered as text (`serde_json::Value` string form; numbers match their default rendering). Raw rows never match field terms.
- Parse errors (`=v`, `f=`, empty field name) yield a parse error result: UI shows an invalid indicator and applies no filtering.
- `body` is opaque: it participates only as its raw JSON text.

### D6: Field discovery piggybacks on scans

A background scan on open (and any query scan) can collect field names into a shared `BTreeSet<String>`. The column picker lists names discovered so far; the set grows as scans proceed. One scanner implementation serves both purposes.

### D7: Raw row rendering

`egui_extras::TableBuilder` cannot merge cells across a row, so raw lines render their full text in the first visible column (dimmed/monospace style to distinguish them from structured cells); other cells empty. A dedicated full-width rendering mode can come later.
*Alternative:* pseudo-field `raw` column (deferred).

### D8: Module layout

```
src/
  main.rs      eframe app entry, window options
  app.rs       App state (file, index, query state, column selection), UI panels
  log_file.rs  mmap, offset index builder, line reading, classification, field scan
  query.rs     lexer, parser, matcher (pure, heavily unit-tested)
```

Background jobs communicate via shared state (`Arc<RwLock<IndexData>>`, `Arc<RwLock<Vec<u32>>>` scan results, `Arc<Mutex<BTreeSet<String>>>` discovered fields) and `Arc<AtomicBool>` cancellation — chosen over `mpsc` during implementation so results update in place and appear progressively; the UI polls the shared state each frame and calls `ctx.request_repaint_after()` while work is in flight.

### D9: Matching case rules (assumptions recorded)

Substring = case-insensitive; field equality = case-sensitive. Query application is explicit-ish (Enter or debounce) rather than per-keystroke full rescans.

## Risks / Trade-offs

- [Full-file scan per query is slow on multi-GB files] -> progressive results + cancellation; results for typical files arrive in seconds; a parsed-field cache is a future optimization.
- [File modified while open shifts offsets] -> read-only assumption for MVP; "reopen" refreshes; document the limitation.
- [Index build blocks first paint on huge files] -> index built in chunks with progress; UI shows rows as prefix offsets become available.
- [Very long lines (stack traces) strain text layout] -> wrap/truncate at a max line length when rendering; content still matches queries in full.
- [Numbers matched as text (`1.0` vs `1`)] -> documented consequence of D5; acceptable for MVP.
- [egui styling is not native] -> accepted for a developer tool.

## Migration Plan

Greenfield: replace hello-world `main.rs`; no data or API migrations. Rollback = revert commit.

## Open Questions

None blocking. Column-choice persistence and a `raw` pseudo-column are conscious deferrals, not unknowns.
