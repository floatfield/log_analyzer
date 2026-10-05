# modifier-click-filtering Specification

## Purpose

Lets the user turn a visible cell value into a query filter with one click: clicking a structured cell while holding a user-configured modifier key appends a field-equality term for that cell to the query input and filters the rows.

## ADDED Requirements

### Requirement: Configurable filter modifier

The system SHALL let the user choose which modifier key triggers filter-on-click from Ctrl, Alt, Shift, and the platform command modifier (Cmd on macOS, Ctrl elsewhere). Ctrl SHALL be the default. The choice SHALL persist across restarts and SHALL take effect immediately when changed.

#### Scenario: Default modifier out of the box

- **WHEN** the user starts the app without having changed the setting
- **THEN** the filter modifier is Ctrl

#### Scenario: Changing the modifier

- **WHEN** the user selects Alt as the filter modifier
- **THEN** filter-on-click is triggered by holding Alt, immediately and from then on

#### Scenario: Modifier choice survives a restart

- **WHEN** the user selected Alt, closed the app, and starts it again
- **THEN** the filter modifier is Alt again

### Requirement: Filter term from a modifier-click

Clicking a cell of a structured row while holding the configured filter modifier SHALL append a field-equality term for that cell's column and value to the query input — the term `field='value'` for a cell in column `field` showing `value` — and SHALL apply the query immediately so the table shows only rows matching the whole query. Appending SHALL combine the new term with any existing query text by AND, per the query language's juxtaposition rule.

#### Scenario: Clicking a requestId cell

- **WHEN** the query input is empty and the user Ctrl-clicks a `requestId` cell whose entry's `requestId` value is `abc-123`
- **THEN** the query input contains `requestId='abc-123'` and the table shows only structured entries whose `requestId` equals `abc-123`

#### Scenario: Appending to an existing query

- **WHEN** the query input contains `level=ERROR` and the user modifier-clicks a `service` cell whose value is `auth`
- **THEN** the query input contains `level=ERROR service='auth'` and the table shows only entries matching that whole query

#### Scenario: Filter applies without typing or pressing Enter

- **WHEN** the user modifier-clicks an eligible cell
- **THEN** the rows are filtered without the user typing in the query input or pressing Enter

### Requirement: Term value quoting

The generated term SHALL quote the cell value so the query parser reads it as one literal value: single quotes by default, double quotes when the value contains a single quote. A value that cannot be represented as a quoted value in the query grammar — containing both quote kinds or a line break — SHALL add no term.

#### Scenario: Value with spaces

- **WHEN** the user modifier-clicks a `message` cell whose value is `connection lost`
- **THEN** the query input receives the term `message='connection lost'` and entries whose message equals `connection lost` match

#### Scenario: Value containing a single quote

- **WHEN** the user modifier-clicks a cell whose value is `it's broken`
- **THEN** the generated term quotes the value with double quotes so the parser reads the full value including the single quote

#### Scenario: Unrepresentable value

- **WHEN** the user modifier-clicks a cell whose value contains both a single and a double quote
- **THEN** the query input is unchanged

### Requirement: Modifier-click does not change selection

A modifier-click on a cell SHALL act only as a filter action: it SHALL NOT select, deselect, or move the row selection.

#### Scenario: Selection survives a modifier-click

- **WHEN** a row is selected and the user modifier-clicks a cell of any row
- **THEN** the selection still points at the previously selected row

### Requirement: Ineligible cells do nothing

Modifier-clicking a cell that cannot produce a valid query term SHALL leave the query input and the table unchanged: cells of raw lines, structured cells whose entry has no value for that column, and structured cells whose column name is not a valid query field name (the names the query grammar accepts).

#### Scenario: Modifier-clicking a raw row

- **WHEN** the user modifier-clicks the cell of a raw line row
- **THEN** the query input is unchanged and the visible rows are unchanged

#### Scenario: Modifier-clicking an empty cell

- **WHEN** the user modifier-clicks a structured cell whose entry has no value for that column
- **THEN** the query input is unchanged and the visible rows are unchanged

#### Scenario: Modifier-clicking a column with an unqueryable name

- **WHEN** a structured entry has a field whose name the query grammar cannot represent (for example a name containing a space) and the user modifier-clicks that field's cell
- **THEN** the query input is unchanged and the visible rows are unchanged
