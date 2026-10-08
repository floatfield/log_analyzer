# log_analyzer

A cross-platform GUI log explorer for large JSON-lines log files, plus two
CLI utilities: `log-merge` (combine time-ordered log files) and `log-sort`
(bring log files of any entry order into ascending `@timestamp` form).

## Building

All three binaries are built by a single `cargo build`; they end up in
`target/debug/` (or `target/release/`):

```sh
cargo build --release
```

- `target/release/log_analyzer` — the GUI explorer (`src/main.rs`)
- `target/release/log-merge` — the merge utility (`src/bin/log_merge.rs`)
- `target/release/log-sort` — the sort utility (`src/bin/log_sort.rs`)

To build just one of them:

```sh
cargo build --release --bin log_analyzer
cargo build --release --bin log-merge
cargo build --release --bin log-sort
```

Drop `--release` for a faster debug build.

## Running

GUI explorer — optionally pass a file to open immediately:

```sh
./target/release/log_analyzer [path/to/log.jsonl]
```

Merge utility — entries must be ascending by `@timestamp` within each input;
non-JSON lines are preserved. Every output entry is stamped with a `system`
property naming its source file:

```sh
./target/release/log-merge --output merged.log web.log auth.log
```

Sort utility — inputs have **no** ordering requirement (any `@timestamp`
order is accepted); entries are written unchanged, with no property added —
in particular no `system` stamp (run `log-merge` afterwards if you want
origin stamps). Non-JSON lines are preserved; equal timestamps keep the
command-line input order, then the file line order. Large inputs sort in
bounded memory (chunked external sort):

```sh
./target/release/log-sort --output sorted.log capture.log
./target/release/log-sort --output sorted.log web.log auth.log
```

Both utilities share the same CLI shape and exit codes: `0` success, `1`
contract/I/O error (reported as `<file>:<line>: <reason>`), `2` usage error
(including an output path that names an input).

## Tests

```sh
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```
