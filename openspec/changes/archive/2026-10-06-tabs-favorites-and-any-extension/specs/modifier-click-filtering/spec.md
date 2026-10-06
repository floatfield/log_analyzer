# Spec Delta

## MODIFIED Requirements

### Requirement: Filter term from a modifier-click

Clicking a cell of a structured row while holding the configured filter modifier SHALL form the field-equality term for that cell's column and value — the term `field='value'` for a cell in column `field` showing `value` — SHALL replace the query input's contents with that term (any previous query text is discarded), and SHALL apply the query immediately so the table shows only rows matching the term.

#### Scenario: Clicking a requestId cell

- **WHEN** the query input is empty and the user Ctrl-clicks a `requestId` cell whose entry's `requestId` value is `abc-123`
- **THEN** the query input contains `requestId='abc-123'` and the table shows only structured entries whose `requestId` equals `abc-123`

#### Scenario: Appending to an existing query

- **WHEN** the query input contains `level=ERROR service='auth'` and the user modifier-clicks a `requestId` cell whose entry's `requestId` value is `abc-123`
- **THEN** the query input contains only `requestId='abc-123'` — the clicked term replaces the previous query text instead of being appended to it — and the table shows only entries whose `requestId` equals `abc-123`

#### Scenario: Filter applies without typing or pressing Enter

- **WHEN** the user modifier-clicks an eligible cell
- **THEN** the rows are filtered without the user typing in the query input or pressing Enter
