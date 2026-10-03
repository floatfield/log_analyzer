# Spec Delta

## Purpose

Defines the query language used to filter visible rows, supporting free-text substring matching and JSON field value matching entered in a dedicated query input.

## ADDED Requirements

### Requirement: Substring terms

A bare term in the query SHALL match any row whose full line text contains the term, case-insensitively; this includes raw lines.

#### Scenario: Bare term matches structured and raw rows

- **WHEN** the query is `timeout` and the file contains an entry whose text mentions "Timeout" and a raw line `WARNING: connection timeout`
- **THEN** both rows match

#### Scenario: No substring matches

- **WHEN** the query is a bare term that occurs nowhere in the file
- **THEN** no rows match and the empty result is visible to the user

### Requirement: Field equality terms

A `field=value` term SHALL match structured entries whose named field's value equals the given value, compared case-sensitively as text. Raw lines SHALL never match a field term.

#### Scenario: Field equality match

- **WHEN** the query is `level=ERROR`
- **THEN** exactly the structured entries whose `level` field is `ERROR` match

#### Scenario: Field absent from entry

- **WHEN** the query is `service=auth` and an entry has no `service` field
- **THEN** that entry does not match

#### Scenario: Raw line never matches a field term

- **WHEN** the query is `level=ERROR` and the file contains a raw line that includes the text `level=ERROR`
- **THEN** that raw line does not match by the field term (it can only match via substring terms)

### Requirement: Combining terms

Multiple terms in a query SHALL be combined with AND semantics: a row is shown only when every term matches it.

#### Scenario: Field term and substring combined

- **WHEN** the query is `level=ERROR timeout`
- **THEN** only entries whose `level` is `ERROR` and whose line text contains `timeout` (case-insensitively) match

#### Scenario: Multiple field terms

- **WHEN** the query is `level=ERROR service=auth`
- **THEN** only entries with both `level` equal to `ERROR` and `service` equal to `auth` match

### Requirement: Empty query shows all rows

An empty query SHALL show every row of the file.

#### Scenario: Clearing the query

- **WHEN** the user clears the query input after a filtered view
- **THEN** all rows of the file are shown again in file order

### Requirement: Invalid query feedback

The system SHALL detect a malformed query term (for example `=value` or `field=`), indicate the invalidity to the user, and show all rows unfiltered while the query remains invalid.

#### Scenario: Malformed term

- **WHEN** the query input contains `field=`
- **THEN** the UI indicates the query is invalid and the table shows all rows
