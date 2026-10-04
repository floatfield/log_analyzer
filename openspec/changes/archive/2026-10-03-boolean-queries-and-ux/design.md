# Design

## Context

The app is a single-binary egui/eframe 0.33 log explorer. The query engine (`src/query.rs`) currently flattens the input into AND-only terms and matches them linearly. UI state lives in `src/app.rs` (`LogAnalyzerApp`), which owns the loaded file, discovered fields, visible-column selection, and the debounced `QueryUi` state machine. Column defaults are applied once field discovery completes (`maybe_apply_defaults` in `src/app.rs`). There is no persistence layer today, and visuals use the egui default (dark). See proposal.md — Why for motivation.

## Goals / Non-Goals

**Goals:**

- Boolean query language (OR, parentheses, quoted values/phrases) with AND-over-OR precedence, parsed to an AST and evaluated per row.
- Severity-based row tinting driven by the `level` field, readable on the light theme.
- Light theme at startup.
- Column-panel field-name filter that narrows the list without touching selection.
- Durable workspace state (favorites, per-file visible columns) in a user config file.
- One-action query reset control.

**Non-Goals:**

- No theme switching or theme persistence — light only.
- No query persistence or query history.
- No AND/OR explicit keywords other than case-insensitive `or`; no NOT operator.
- No favorites ordering, renaming, or nested groups of favorites.
- No change to file access, indexing, or virtualization.

## Decisions

### D1 — Query engine: tokenizer + recursive-descent parser → AST

Grammar (AND binds tighter than OR; `or` matched as a standalone, case-insensitive word):

```text
expr    := andExpr ( 'or' andExpr )*
andExpr := atom+                -- juxtaposition
atom    := '(' expr ')' | term
term    := field '=' value | quoted-phrase | bare-word
value   := quoted (single/double) | run of non-space, non-paren chars
```

AST: `enum Expr { Or(Vec<Expr>), And(Vec<Expr>), Substring(String), FieldEq { field, value } }`.

- Quote rules: `'…'` / `"…"` strip the quotes and may contain spaces and `or` as text; an unterminated quote is a parse error.
- Parse errors (`=value`, `field=`, unbalanced parens, unterminated quote) make `Query` construction fail; `app.rs` keeps its existing invalid-query behavior (indicator + all rows shown) by treating a failed parse as "no active query".
- Evaluation: `Expr::matches(entry, line)` — `Substring` checks full line text case-insensitively (covers raw lines); `FieldEq` consults the structured entry only. Recursion depth is bounded by input length; queries are tiny, so per-row cost stays negligible relative to the mmap scan.
- Alternatives considered: flat term list with an `Or` marker (awkward for grouping/precedence); a full parser crate (`nom`/`pest`) — unnecessary for this grammar size; hand-rolled keeps deps zero and errors simple.
- Backward compatibility: existing AND-only queries parse to `And(...)` with identical semantics. Port existing `query.rs` tests unchanged; `matches`-style public API shape is preserved.

### D2 — Row highlighting via per-cell background paint

egui_extras `TableBuilder` has no row-background API, so for a highlighted row each cell paints a filled rect first: `ui.painter().rect_filled(ui.available_rect_before_wrap(), …, tint)` before the widget is drawn — the standard egui pattern for row tinting.

- Tint source: reuse the per-row structured-entry access the table already performs for column values; read `level`, compare case-insensitively — `error` → red, `warn`/`warning` → yellow, anything else → no tint. Raw lines never tint.
- Colors: translucent fills (alpha ≈ 0.25, e.g. `Color32::from_rgba_unmultiplied(220, 50, 50, 64)`) over the light-theme background so text contrast is preserved; constants defined next to the table code.
- Alternatives: `Visuals::widgets` recolor (wrong granularity), overlay painter pass (z-order fragility).

### D3 — Light theme at startup

`ctx.set_visuals(egui::Visuals::light())` once in the app creator (`main.rs` closure / `LogAnalyzerApp::new`). No preference file; `app-appearance` specifies light-only.

### D4 — Persistence: JSON file + `dirs`, new `src/persistence.rs`

- New dependency `dirs` for the cross-platform config dir; file at `dirs::config_dir()/log_analyzer/workspace.json`.
- Schema:

```json
{ "favorites": ["/abs/path-a"], "columns": { "/abs/path-a": ["level", "message"] } }
```

- API: `Workspace::load() -> Workspace` (read/parse failure ⇒ empty workspace, per workspace-persistence spec), `Workspace::save(&self)` (atomic: write temp file + rename), small mutation helpers (`toggle_favorite`, `set_columns`, `remove_favorite`).
- Path keying: `std::fs::canonicalize` when it succeeds, else the raw path string (keeps favorites to files that may have moved/deleted).
- Wiring: load once in `LogAnalyzerApp::new`; every mutation saves synchronously — the file is tiny, and atomic rename prevents torn writes. No background writer, no debouncing.
- Column restore: extend the existing `maybe_apply_defaults` hook — when discovery completes, if the workspace has a saved selection for the path, apply it; otherwise apply defaults. Column add/remove afterwards updates the saved selection (write-through).
- Alternatives: TOML + `serde` already in the tree, but JSON matches the log domain and `serde_json` is already a dependency; storing in the egui persistence (`apps/<id>.app.json`) couples app state to egui's storage format.

### D5 — Reset control and column filter (UI-only state)

- Reset button beside the query input calls the existing `QueryUi` clear path (text + invalid flag), which already restores all rows.
- Column filter is a single-line `TextEdit` above the field list; its text is session-only app state (the specs don't persist it). The checkbox list renders the filtered subset (`name.contains(filter)` case-insensitive); selection set and table columns are untouched, so hidden-but-selected columns remain applied.

### D6 — Favorites surface

- Toolbar star toggle for the open file (filled/unfilled reflects state).
- Favorites dropdown from the toolbar listing favorites; each row is click-to-open plus a remove affordance. Opening uses the existing `open_path` flow, so restore-of-columns applies automatically.

### D7 — Reload reuses the open flow

The reload button calls the existing `open_path` on the current file's stored path — no new indexing, scanning, or cancellation machinery. Consequences, all inherited from that flow:

- Any in-flight scan is cancelled before the rebuild starts; index and field discovery run fresh over the re-read contents.
- Per-file columns are restored from the workspace keyed by the unchanged path (same as a reopen), so the user's column selection survives reload.
- The query input keeps its text and continues filtering the reloaded rows (decided: preserved — the tail-and-filter workflow expects the filter to survive a refresh); scroll resets to the top as with any open.
- Disabled when no file is open. If the file vanished or became unreadable, the failure surfaces through the existing open-failure handling.

Alternatives considered: filesystem watching with auto-refresh (out of scope — manual control requested; a watcher is a separate change); diff-based partial re-index (complexity unjustified given a full re-index of even a 2GB file costs ~2.4s).

## Risks / Trade-offs

- [Parser rewrite regresses existing queries] → keep current `query.rs` tests green untouched; add grammar cases (precedence, groups, quotes) as unit tests before rewiring.
- [AST eval slower than flat term list in hot scan loop] → AST is tiny and allocation-free at match time; verified by the existing `#[ignore]` scale test staying within its budgets.
- [Corrupt/partial workspace file] → atomic write (temp+rename); load failures fall back to empty state (spec'd), never panic.
- [Tint readability under light theme] → translucent alpha fills; verified visually via the screenshot flow used in the previous change.
- [`dirs` config dir unavailable (exotic platforms)] → if `config_dir()` returns `None`, persistence becomes a no-op in-memory workspace; app functions normally.
- [Favorites to deleted files] → open follows existing failure handling (error toast); removal from the list still possible.
- [Reload on a deleted/rotated file] → failure surfaces via the existing open-error path; reload then behaves like any failed open, with the error shown to the user.

## Migration Plan

Single binary; no data migration. `workspace.json` is created on first mutation. Rollback = previous binary ignores the file. File-access mechanics (open, indexing, cancellation) are reused, not changed; reload only adds a new trigger for the existing flow.

## Open Questions

None — remaining choices (exact tint alphas, button labels) are cosmetic and verifiable during apply.
