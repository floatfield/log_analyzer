# Design

## Context

`log-merge` (spec: log-merge, `src/merge.rs`) merges inputs that are already ascending: a per-file `FileCursor` classifies lines (`parse_object`), buffers non-object lines in `pending_raw`, and a `BinaryHeap` k-way pass stamps each entry with `system` and writes through a temp file renamed on success (`temp_path_for`, `install_output`, `output_collides` via `canonical_path`). Both binaries hand-roll the same `--output <path> <input>...` parser with exit codes 0/1/2. The library crate (`src/lib.rs`) already exposes `log_file` and `merge`.

Sorting differs from merging in two ways: inputs carry **no ordering requirement**, and the output must **not** be stamped. See proposal.md — Why.

## Goals / Non-Goals

**Goals:**
- Sort any number of inputs into one ascending, stable output without loading whole files into memory.
- Preserve raw lines verbatim with their same-input neighbors (merge's established convention).
- Reuse the tested merge core rather than writing a second streaming writer.
- Keep `log-merge`'s behavior and public API unchanged.

**Non-Goals:**
- No parallel key, no numeric/timezone-aware timestamp parsing (byte-string compare, same as log-merge).
- No in-place mode, no CLI knob for the memory budget.
- No change to `log-merge` CLI or its spec.

## Decisions

- **D1 — Chunked external sort.** Read inputs sequentially, filling runs of at most `RUN_ENTRIES` entries (module constant, 100_000). A run holds entries plus their attached raw lines, sorted stably by `(timestamp, sequence)` where `sequence = (input_index, line_no)` — a total order that makes ties deterministic. Runs go to temp files named like merge's (`.<name>.log-sort-run-<n>-<pid>`). The final pass merges the runs (ascending inputs by construction) into the output. Memory is bounded by one run plus the heap.
- **D2 — Raw lines attach to the entry that follows them** (flushed immediately before it, exactly like `FileCursor::pending_raw`), so they travel with that entry through the chunk sort. Trailing raw lines with no following entry attach after the input's last entry (or, for an entry-less input, are emitted at that input's contribution end). Spec keeps placement unrestricted; this only pins implementation determinism.
- **D3 — Make stamping optional in the merge core.** Thread a `stamp: bool` (or equivalently skip the `system` insert when unset) from a new `pub fn merge_runs(inputs, output, stamp)` down into `drain_heap`; `pub merge_files(inputs, output)` keeps its exact signature and becomes the `stamp = true` wrapper. The sort's final pass calls it with `stamp = false`. Alternatives rejected: a second unstamped writer (duplicates tested streaming code) and sorting-then-unstamping (lossy, wrong).
- **D4 — Stability across runs.** Merge ties break by input index; the final merge's "inputs" are the runs in creation order, and run order refines `(input_index, line_no)` because chunks fill sequentially. Within a run, ties were already sequence-sorted. Composition is therefore exactly the stable global order. Covered by a forced-small-budget test.
- **D5 — Uniform code path.** Even a single-run input goes through the final merge pass (a one-element heap is a cheap pass-through) — no divergent fast path to test around.
- **D6 — Timestamps compare as byte strings** (design D3 of log-merge): uniform RFC 3339 UTC inputs assumed, documented in the README the same way.
- **D7 — Shared CLI parsing.** Extract the argument parser into `pub mod cli` in the library (`parse_output_inputs(&[String], usage: &str) -> Result<Args, UsageError>` shape), parameterized by the usage string; both binaries print their own usage and map errors to exit 2. The `names_an_input` pre-check moves in with it.
- **D8 — Reuse output plumbing.** `output_collides`, `temp_path_for`, `install_output`, and `MergeError` become `pub(crate)` (or move behind small helpers in `merge.rs`) so `sort.rs` reuses them; collision and all-or-nothing semantics are then identical by construction.
- **D9 — Contract checks stay per line.** The run reader validates each entry has a string `@timestamp` (file+line context via `MergeError`); unlike merge, there is no ascending check. `README` gains a `log-sort` section (three binaries).

## Risks / Trade-offs

- Run temp files consume disk next to the output (same filesystem); acceptable — merge already writes a full temp copy of the output.
- Entry re-serialization normalizes JSON formatting (compact, reordered keys), as in log-merge; raw lines are the verbatim record.
- A budget constant fixes memory per run (~100k entries); very wide entries could still make a run large — accepted, no CLI knob (non-goal).
- Non-UTF-8 input aborts mid-read; all-or-nothing output keeps the destination untouched.
- Duplicate timestamps across many inputs rely on the heap tiebreak — pinned by the stability test.
