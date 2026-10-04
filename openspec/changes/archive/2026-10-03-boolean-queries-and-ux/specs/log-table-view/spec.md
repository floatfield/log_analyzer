# Spec Delta

## ADDED Requirements

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
