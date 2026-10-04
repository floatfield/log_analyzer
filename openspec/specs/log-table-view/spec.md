# log-table-view Specification

## Purpose

Defines how log rows are presented in the main table, so users can browse very large files smoothly and choose which fields they see as columns.

## Requirements

### Requirement: Virtualized scrolling

The table SHALL render only the rows that are currently visible, keeping scrolling responsive regardless of the total number of rows.

#### Scenario: Scrolling a huge file

- **WHEN** the user scrolls through a file with millions of rows
- **THEN** only the visible rows are rendered and the UI does not freeze

#### Scenario: Scroll position reflects row count

- **WHEN** the user drags the scrollbar to a position in the middle of a large file
- **THEN** the rows shown correspond to the lines at that relative position in the file

### Requirement: Structured row content

The table SHALL display a structured entry's field values in the columns mapped to those fields.

#### Scenario: Entry with visible fields

- **WHEN** columns for `@timestamp`, `level`, and `message` are shown and a row's entry has those fields
- **THEN** the row shows each field's value in its column

#### Scenario: Entry missing an optional field

- **WHEN** a shown column corresponds to a field the row's entry does not have
- **THEN** that cell is rendered empty

### Requirement: Raw row content

The table SHALL display each raw line as a row preserving its original text.

#### Scenario: Raw line visible

- **WHEN** the file contains the line `PANIC: unexpected state`
- **THEN** a row is visible containing that exact text

#### Scenario: Continuation-style lines

- **WHEN** the file contains a bare stack frame line such as a tab-indented `at com.example.Router.dispatch(Router.java:88)`
- **THEN** that line appears as its own raw row with its original text

### Requirement: Configurable columns

The table SHALL let the user choose which discovered fields are shown as columns, SHALL provide a default column selection when a file opens, and SHALL apply changes immediately.

#### Scenario: Default columns on open

- **WHEN** a file is opened and the user has not changed the selection
- **THEN** the table shows default columns including `@timestamp`, `level`, and `message`

#### Scenario: Adding a column

- **WHEN** the user adds a field such as `service` to the visible columns
- **THEN** the table immediately shows a `service` column populated for rows that have that field

#### Scenario: Removing a column

- **WHEN** the user removes a visible column
- **THEN** the table immediately no longer shows that column

### Requirement: Level-based row highlighting

The table SHALL tint the background of structured rows by their `level` field, compared case-insensitively: `error` rows in red, `warn` or `warning` rows in yellow. All other rows, including raw lines, SHALL keep the default background.

#### Scenario: Error rows highlighted red

- **WHEN** a structured entry's `level` field is `ERROR`
- **THEN** that row's background is tinted red

#### Scenario: Warning rows highlighted yellow

- **WHEN** a structured entry's `level` field is `WARN` or `WARNING`
- **THEN** that row's background is tinted yellow

#### Scenario: Other rows keep the default background

- **WHEN** a row is a raw line or a structured entry whose `level` is missing or is anything other than an error or warning level
- **THEN** that row keeps the default background

### Requirement: Filtering the field list

The Columns panel SHALL provide an input that narrows the listed field names to those containing the entered text, case-insensitively. Clearing the input SHALL restore the full list. Filtering SHALL NOT change which columns are selected or displayed.

#### Scenario: Typing narrows the list

- **WHEN** the user types `req` into the filter input
- **THEN** only field names containing `req` (such as `requestId`) are listed

#### Scenario: Clearing restores the full list

- **WHEN** the user clears the filter input
- **THEN** all discovered field names are listed again

#### Scenario: Hidden selected columns stay applied

- **WHEN** a selected column's name does not match the current filter text
- **THEN** it is not listed but remains selected and displayed in the table
