# Tasks

## 1. Library target

- [x] 1.1 Add `src/lib.rs` declaring `pub mod log_file;` and `pub mod merge;` (empty merge module placeholder is fine to start), keep `src/main.rs` as the GUI binary using the library for `log_file` (drop its `mod log_file;`, switch the `crate::log_file` uses in `src/app.rs` to the library crate), and confirm nothing else moved (`cargo test`, `cargo clippy -- -D warnings` green with the existing 84 tests)

## 2. Merge core (`src/merge.rs`, unit tests colocated)

- [x] 2.1 Validated line reader: wrap a `BufRead` line iterator into a cursor yielding entries and raw lines — a non-object line is a raw item; fail with file+line context on a missing/non-string `@timestamp` and on an entry older than its predecessor (per-file `prev` compare over entries only); unit-test each violation, the happy path, and raw-line yield (`cargo test merge::`)
- [x] 2.2 Streaming k-way merge: binary heap over per-file cursors ordering by `@timestamp` bytes with input-index tiebreak (equal timestamps in command-line order, line order within a file); each cursor's pending raw lines flush immediately before its next entry and at its exhaustion, so a raw line lands between its same-input neighbors; unit-test a two-file interleave, the tie case, single-file pass-through, and raw placement (leading/between/trailing) (`cargo test merge::`)
- [x] 2.3 Origin stamping: insert `system` = file name component of the input path as given into every entry, overwriting any existing `system` value; raw lines pass through unstamped; unit-test stamping, the overwrite case, and raw untouched (`cargo test merge::`)
- [x] 2.4 All-or-nothing output: stream stamped entries into a temp file beside the output path and rename on success; on any failure delete the temp and leave the output path untouched; unit-test success replaces an existing output and a mid-stream failure produces no output file (`cargo test merge::`)

## 3. CLI (`src/bin/log_merge.rs`)

- [x] 3.1 Argument parsing: `log-merge --output <path> <input>...` hand-rolled; usage message + exit code 2 on missing output/inputs, early rejection (before opening anything) when the output path names an input file, exit 0 on success; unit-test the parser (`cargo test log_merge` or bin tests)
- [x] 3.2 Error reporting: contract violations print `<file>:<line>: <reason>` to stderr and exit 1 (`cargo test` on the error formatting)
- [x] 3.3 Integration test in `tests/`: run the binary on two fixture files with raw lines interleaved — assert the merged output is ascending by `@timestamp` across entries, contains every input entry exactly once with the right `system` value, contains the raw lines unchanged, and a violation case leaves the output path untouched (`cargo test --test merge_cli`)

## 4. Verification

- [x] 4.1 Full gates: `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check` green
- [x] 4.2 Manual end-to-end: merge two fixture logs with different time ranges, a same-timestamp tie, and raw lines, check the output is time-ordered with correct `system` stamps and raw lines intact, then open it in the GUI analyzer and filter `system='web.log'` (or the fixture's file name) to confirm the merged file behaves like any other log
