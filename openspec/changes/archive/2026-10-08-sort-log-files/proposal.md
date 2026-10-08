# Proposal

## Why

Log files are not always produced in time order — concatenated captures, retried writers, and hand-stitched exports can interleave timestamps arbitrarily, and `log-merge` rejects such inputs by contract. A sort utility completes the toolchain: any file can be brought into the ascending `@timestamp` form the analyzer, `log-merge`, and downstream tooling expect.

## What Changes

- New `log-sort` binary: `log-sort --output <path> <input>...` sorts the entries of one or more log files into a single output ordered ascending by `@timestamp`.
- Entries are written unchanged — no property is added or rewritten (stamping is `log-merge`'s job); equal timestamps keep command-line input order, then line order (a stable sort).
- Lines that are not JSON entries are preserved unchanged; within one input their relative order and their neighbors are kept as far as the sorted order permits; placement among other inputs' lines is unrestricted (same convention as `log-merge`).
- Inputs have no ordering requirement — unlike `log-merge`, any entry order is accepted; a line that is a JSON object without a string `@timestamp` fails the run with `file:line` context.
- All-or-nothing output: the result streams into a temp file beside the output path and renames on success; any failure leaves the output path untouched. An output path naming an input is rejected before anything is opened.
- Shared CLI argument handling and the merge core's stamping step are refactored into reusable library pieces; `log-merge` behavior is unchanged.
- README documents the third binary.

## Capabilities

### New Capabilities
- `log-sort`: Sorting log files into one `@timestamp`-ascending output via the `log-sort` utility.

### Modified Capabilities
<!-- none: the log-merge requirements are unchanged; making its internal
     stamping step reusable is implementation-only. -->

## Impact

- `Cargo.toml`: new `[[bin]]` target `log-sort` (`src/bin/log_sort.rs`).
- `src/merge.rs`: extract an internal merge that makes origin stamping optional; `pub merge_files` keeps its signature and stamping behavior.
- New `src/sort.rs` in the library: chunked external sort (sorted runs + final merge pass) so memory stays bounded for large inputs.
- New shared argument-parsing helper in the library used by both binaries.
- New `tests/sort_cli.rs` integration tests; README updated.

## Assumptions

- Multiple inputs concatenate into one sorted output (mirrors `log-merge`'s CLI shape); the user said "files", plural.
- Raw-line handling follows the convention this project just established for `log-merge`: preserve verbatim, keep each input's raw lines in relative order next to their same-input neighbors where possible; since raw lines carry no timestamp their exact placement is unrestricted.
- Output goes to `--output` (never in-place), matching `log-merge`; in-place sorting is not offered.
- Timestamps compare as strings (uniform RFC 3339 UTC inputs), as in `log-merge`.
- Entries are re-serialized (compact JSON) exactly as `log-merge` does; raw lines are verbatim.
