# Design

## Context

The workspace is one binary crate (`log_analyzer`) whose modules live under `src/`; `log_file.rs` is already UI-free (plain byte slices, `parse_object` for JSON-object classification) but is only reachable from the GUI binary because there is no library target. The GUI never parses `@timestamp` values — they are plain string field values to the query engine. Fixture entries carry RFC 3339 UTC strings with a uniform shape (`2026-09-11T22:11:34.000Z`). See proposal.md for the motivation.

## Goals / Non-Goals

**Goals:**
- A standalone CLI binary, shareable in scripts, producing an ordinary log file the GUI already opens.
- Streaming k-way merge: memory bounded by the number of inputs, not input sizes (consistent with the analyzer's large-file approach).
- Reuse the analyzer's line-classification semantics rather than a second, divergent parser.
- Raw (non-JSON) lines pass through with their content preserved exactly.

**Non-Goals:**
- No global sort of arbitrary inputs — ascending order is an input contract; the merge preserves, never repairs, it.
- No GUI integration (no "merge" button in the analyzer).
- No timestamp-format conversion, timezone normalization, or deduplication of identical entries.
- No timestamp inference or ordering for raw lines: they have no `@timestamp`, so their placement relative to other inputs' entries is unconstrained (the spec leaves it unrestricted).
- No changes to the analyzer's specs or behavior.

## Decisions

- **D1: Library target + `src/bin`, not a separate crate.** Add `src/lib.rs` publishing `log_file` (existing) and `merge` (new); the utility is `src/bin/log_merge.rs`, a thin CLI over `log_analyzer::merge`. The GUI's `main.rs` drops its `mod log_file;` and uses the library, so classification semantics have exactly one home. Alternatives considered: a self-contained bin duplicating `parse_object` (divergence risk, ~5 lines saved nowhere), a Cargo workspace with a second crate (heavier structure than one shared module justifies today).
- **D2: Streaming k-way merge with a binary heap.** Each input is a `BufRead` line reader wrapped in a validated cursor that yields items — entries (JSON object + string `@timestamp`) and raw lines. A non-object line becomes a raw item; an entry missing a string `@timestamp`, or older than its file's previous entry (compared over entries only), aborts with file+line context. Raw lines accumulate in the cursor's pending buffer and flush to the output immediately before the cursor's next entry is emitted — or at cursor exhaustion for trailing raws — so a raw line lands between its same-input neighbors; the heap orders current-entry timestamps ascending with `(input_index)` as the tiebreak, so equal timestamps come out in command-line order and line order within a file falls out of the iterator. Memory is O(inputs). Alternative rejected: read-all-and-sort (violates the streaming goal and the "preserve, not sort" framing).
- **D3: Timestamp order = byte-wise string comparison.** With one uniform RFC 3339 UTC format per run, lexicographic order equals chronological order; this adds no dependencies and mirrors how the analyzer itself treats values. Alternatives: parse with `chrono`/`time` (new dependency, tolerant of mixed formats we explicitly do not promise). The uniform-format assumption is recorded in the proposal and enforced in spirit by the ascending-input contract (a format change mid-file typically surfaces as an order violation).
- **D4: `system` = file name component of the path as given.** `logs/web.log` stamps `web.log`. Insertion overwrites any existing `system` key (serde map insert). Full paths were rejected as stamp values: the point is a compact, queryable origin label (`system='web.log'`).
- **D5: Output is atomic, all-or-nothing.** Entries stream into a temporary file next to the output path; on success it is renamed over the output; on any failure the temporary is deleted and the output path is never created — a failed merge leaves the filesystem untouched (the no-partial-output scenario). The output-equals-input collision is checked up front, before any file is opened.
- **D6: Entries are re-serialized; raw lines are verbatim.** Stamped entry maps are written with `serde_json`, so output entry lines are normalized JSON (keys in the map's order) rather than the original bytes with surgery — that is what makes adding a property reliable. Raw lines are written back exactly as read (content with the line terminator re-attached as `\n`), no stamping, no re-serialization.
- **D7: CLI shape.** `log-merge --output <path> <input>...`: output must be named explicitly (no positional guessing, no default file), usage/argument errors exit with status 2, contract violations with status 1, success 0 — script-friendly. Hand-rolled parsing over a clap dependency: the surface is one flag.

## Risks / Trade-offs

- [Mixed timestamp formats across or within inputs order incorrectly under string compare] → documented uniform-format assumption; the ascending-input check catches most real cases; a pluggable timestamp parser can be added later without spec changes.
- [Blank lines or CRLF endings in real-world files] → blank lines are raw lines and are preserved verbatim; CRLF input is trimmed of `\r` before JSON parsing, and raw lines are written back `\n`-terminated (the output is uniformly `\n`-terminated).
- [A line that is valid JSON but not an object, or input that is not UTF-8] → non-object JSON is a raw line by the analyzer's classification; a non-UTF-8 line aborts with file+line (exact re-emission of invalid bytes is not attempted).
- [Very many inputs raise the file-descriptor count] → inputs are opened one per merge cursor; typical merges are single-digit inputs; if this ever matters, the design degrades gracefully by batching inputs.
- [Re-serialization changes entry key order/formatting] → accepted trade-off (D6); the analyzer and JSON tooling are indifferent to it, and raw lines keep their exact bytes.

## Migration Plan

Purely additive: new library target, new binary, no existing behavior touched. Rollback is removing the two new source files and the `[[bin]]` entry. The GUI change (using the library instead of a local module) is mechanical and covered by the existing 84-test suite.

## Open Questions

None.
