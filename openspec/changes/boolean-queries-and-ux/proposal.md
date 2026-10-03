# Proposal

## Why

Day-to-day log exploration needs richer filtering than the current AND-only query language (multi-value field matches such as two request IDs, phrase substrings containing spaces), plus quality-of-life features: readable light visuals, quick visual scanning of severity, faster column selection, and session-to-session memory of favorites and per-file column layouts.

## What Changes

- Extend the query language to boolean expressions: parentheses grouping, case-insensitive `or` between terms (AND, written as juxtaposition, binds tighter than OR), and quoted values (`'...'` / `"..."`) so field values and phrase terms may contain spaces — e.g. `(requestId='some-request-id' or requestId='another-request-id') message="foo bar"`. Existing AND-only queries keep their meaning.
- Add a reset button next to the query input that clears the query and restores all rows.
- Highlight table rows by severity: rows whose `level` field is `ERROR` tinted red, `WARN`/`WARNING` tinted yellow.
- Add a filter input to the Columns panel that narrows the displayed field-name list.
- Switch the application theme to light.
- Add file favorites: mark the open file as favorite and reopen favorites from the UI; favorites persist across sessions.
- Persist the visible-column selection per file path, so reopening a file restores its previously selected columns; persisted state survives restarts.

## Capabilities

### New Capabilities
- `workspace-persistence`: persisted per-user workspace state — favorite files and per-file visible-column selections — including how they are restored and updated.
- `app-appearance`: the application's visual theme (light theme by default).

### Modified Capabilities
- `log-query`: grammar gains OR, parentheses, and quoted values/phrases (AND precedence unchanged); new reset-query control; invalid-query feedback extended to unbalanced quotes/parentheses.
- `log-table-view`: rows gain severity-based highlighting; the column picker gains a filter input for field names.

## Impact

- `src/query.rs`: parser rewritten as a tokenizer + AST (AND/OR/nested groups, quoted lexemes); matcher evaluates the AST; public `matches` API shape preserved.
- `src/app.rs`: query input row (reset button), Columns panel (filter field), table row rendering (severity tint), toolbar (favorites toggle + favorites menu), light-theme visuals, column-restore/preference-update hooks.
- New `src/persistence.rs`: JSON config file (favorites, per-file columns) under the user's config directory; new `dirs` dependency for cross-platform config paths.
- Performance: query-scan hot loop now evaluates an AST instead of a flat term list; per-row cost stays linear and allocation-light.
- No breaking changes to file access, indexing, or table virtualization.
