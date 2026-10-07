# Proposal

## Why

Correlating events across several systems means juggling files: the explorer shows one log per tab, and the user must mentally interleave timelines by eye. A single merged file — one row per event across all inputs, ordered by time, each entry tagged with the system it came from — turns that into one view, and the existing query engine can then filter it (`system='auth'`, `level=ERROR system='web'`). Because merged output is itself a normal log file, the analyzer needs no changes to consume it.

## What Changes

- **New utility program `log-merge`** (a second binary in this workspace, separate from the GUI): reads multiple log files and writes one output file containing all of their entries.
- **Order preserved.** Every input is required to have its entries in ascending `@timestamp` order; the merged output preserves that property — entries are emitted in non-decreasing `@timestamp` order (a stable merge, not a global sort).
- **Origin stamped.** Every entry in the output gains a `system` property whose value is the name of the file the entry originates from.
- **Raw lines preserved.** Lines that are not JSON objects are kept in the output unchanged — they carry no timestamp, so they are emitted adjacent to their neighboring lines from the same input (their placement among other inputs' entries is unconstrained) and receive no `system` stamp.
- **Contract violations fail fast.** An input whose JSON entries are out of ascending `@timestamp` order, or an entry without a string `@timestamp`, aborts the merge with an error naming the file and line; no output file is produced.

Assumptions (recorded, not user-confirmed): the `system` value is the input file's name component (e.g. `web.log` for `logs/web.log`); an entry that already has a `system` property has it overwritten with the true origin; equal timestamps interleave by the order the inputs were given on the command line, then by line order within an input; input timestamps share one uniform RFC 3339 UTC format per merge run, so string order equals chronological order; raw lines are emitted adjacent to their same-input neighbors (leading raw lines before the file's first entry, trailing ones after its last) and are written back verbatim; entries are re-serialized JSON objects (key order normalized), not byte-copies of the input lines.

## Capabilities

### New Capabilities
- `log-merge`: Merging several ascending-by-`@timestamp` log files into one file that keeps the entries time-ordered and stamps each entry with its source file's name via the `system` property; input contract enforcement and the command-line interface of the `log-merge` utility.

### Modified Capabilities
<!-- None: the GUI analyzer and its specs are unchanged; merged output is an ordinary log file it already opens. -->

## Impact

- `Cargo.toml`: gains a second binary target (`log-merge`); no new dependencies (serde_json for entry parsing/re-serialization; std only for I/O).
- `src/lib.rs` (new): library target so the utility can share the analyzer's line-classification semantics (`log_file::parse_object`) instead of duplicating them; `src/main.rs`/`src/app.rs` switch their `log_file` uses to the library crate.
- `src/bin/log_merge.rs` (new): CLI parsing, streaming k-way merge with raw-line pass-through, contract validation, output writing.
- No changes to the GUI app's behavior or to existing specs; the merge utility is line-oriented and streaming (memory independent of input size), consistent with the analyzer's large-file approach.
