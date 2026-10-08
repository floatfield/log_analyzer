# Tasks

## 1. Merge-core refactor (stamp optional, shared plumbing)

- [x] 1.1 Make origin stamping optional: add `pub fn merge_runs(inputs: &[PathBuf], output: &Path, stamp: bool) -> Result<usize, MergeError>` in `src/merge.rs` threading `stamp` into `drain_heap` (skip the `system` insert when false), keep `pub fn merge_files` as the `stamp = true` wrapper with unchanged signature/behavior, and make `output_collides`, `temp_path_for`, `install_output`, `canonical_path` `pub(crate)` for reuse; confirm `cargo test merge::` and `cargo test --test merge_cli` still green with no behavior change

## 2. Shared CLI parsing (`src/cli.rs`, unit tests colocated)

- [x] 2.1 Extract `pub mod cli` into the library: `parse_output_inputs(&[String], usage: &str)` returning output + inputs or a usage error, plus the `names_an_input` pre-check; switch `src/bin/log_merge.rs` to use it (own usage string, same exit codes 0/1/2); unit-test parsing, dangling `--output`, empty inputs, collision detection (`cargo test cli::` and `cargo test --bin log-merge`)

## 3. Sort core (`src/sort.rs`, unit tests colocated)

- [x] 3.1 Run writer: read inputs sequentially into runs of at most `RUN_ENTRIES` entries (module constant); entries sorted stably by `(timestamp, (input_index, line_no))`; each entry's buffered raw lines flush immediately before it and trailing raws after the input's last entry, written to a temp run file named `.<name>.log-sort-run-<n>-<pid>`; fail with file+line `MergeError` on a JSON-object line lacking a string `@timestamp`; unit-test run contents (order, ties, raw attachment, violation) with a tiny forced budget (`cargo test sort::`)
- [x] 3.2 Sort driver `pub fn sort_files(inputs: &[PathBuf], output: &Path) -> Result<usize, MergeError>`: collision check, write runs, final unstamped `merge_runs` over the run files in creation order, temp+rename install, delete temps on any failure; unit-test a single unsorted input, multi-input tie stability, raw-line preservation (leading/between/trailing, entry-less input), entries carry no added/changed properties, empty inputs → empty output, output collision rejected, failure leaves output untouched (`cargo test sort::`)

## 4. CLI (`src/bin/log_sort.rs`)

- [x] 4.1 `log-sort --output <path> <input>...` on the shared `cli` module: usage + exit 2 on missing output/inputs and early exit-2 rejection when the output names an input; exit 0 on success; contract/I/O failures print `log-sort: <file>:<line>: <reason>` to stderr and exit 1; add `[[bin]] name = "log-sort" path = "src/bin/log_sort.rs"` to `Cargo.toml`; unit-test the binary wiring (`cargo test --bin log-sort`)

## 5. Integration test

- [x] 5.1 `tests/sort_cli.rs` using `CARGO_BIN_EXE_log-sort`: run the real binary on a genuinely unsorted fixture with raw lines — assert output entries ascend by `@timestamp`, every input line appears exactly once, entries keep exactly their input properties (no `system` added, existing `system` untouched), raw lines verbatim, equal timestamps in input order; a missing-`@timestamp` fixture exits 1 with `file:line` on stderr and leaves a pre-existing output file untouched; missing args exit 2 with usage; output-naming-input exits 2 (`cargo test --test sort_cli`)

## 6. Verification

- [x] 6.1 Full gates: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` green; update README to document the three binaries and `log-sort` usage
- [x] 6.2 Manual end-to-end: sort an unsorted fixture (raw lines, duplicate timestamps, no `system` properties), verify the output is ascending with lines intact, then chain `log-sort` → `log-merge` on overlapping inputs and open both in the GUI analyzer to confirm they behave like any other log
