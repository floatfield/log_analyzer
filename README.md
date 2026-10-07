# log_analyzer

A cross-platform GUI log explorer for large JSON-lines log files, plus a
`log-merge` CLI utility that merges several log files into one time-ordered
file.

## Building

Both binaries are built by a single `cargo build`; they end up in
`target/debug/` (or `target/release/`):

```sh
cargo build --release
```

- `target/release/log_analyzer` — the GUI explorer (`src/main.rs`)
- `target/release/log-merge` — the merge utility (`src/bin/log_merge.rs`)

To build just one of them:

```sh
cargo build --release --bin log_analyzer
cargo build --release --bin log-merge
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

Exit codes: `0` success, `1` contract/I/O error (reported as
`<file>:<line>: <reason>`), `2` usage error (including an output path that
names an input).

## Tests

```sh
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```
