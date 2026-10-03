# Spec Delta

## Purpose

Defines how log rows are presented in the main table, so users can browse very large files smoothly and choose which fields they see as columns.

## ADDED Requirements

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
