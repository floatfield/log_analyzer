# Spec Delta

## MODIFIED Requirements

### Requirement: Field equality terms

A `field=value` term SHALL match structured entries whose named field's value equals the given value, compared case-sensitively as text. The value MAY be wrapped in single or double quotes (`field='v'`, `field="v"`); the quotes are not part of the compared value, and a quoted value MAY contain spaces. Raw lines SHALL never match a field term.

#### Scenario: Field equality match

- **WHEN** the query is `level=ERROR`
- **THEN** exactly the structured entries whose `level` field is `ERROR` match

#### Scenario: Field absent from entry

- **WHEN** the query is `service=auth` and an entry has no `service` field
- **THEN** that entry does not match

#### Scenario: Raw line never matches a field term

- **WHEN** the query is `level=ERROR` and the file contains a raw line that includes the text `level=ERROR`
- **THEN** that raw line does not match by the field term (it can only match via substring terms)

#### Scenario: Quoted value containing spaces

- **WHEN** the query is `message="foo bar"`
- **THEN** exactly the structured entries whose `message` field equals `foo bar` match, and entries whose message merely contains that phrase as part of longer text do not match

#### Scenario: Single-quoted value

- **WHEN** the query is `requestId='some-request-id'`
- **THEN** exactly the structured entries whose `requestId` field equals `some-request-id` match

### Requirement: Combining terms

Terms separated by the keyword `or` (case-insensitive) SHALL combine with OR semantics. Terms placed next to each other with no keyword SHALL combine with AND semantics, and AND SHALL bind tighter than OR. A row is shown when the whole expression evaluates to true for it.

#### Scenario: Field term and substring combined

- **WHEN** the query is `level=ERROR timeout`
- **THEN** only entries whose `level` is `ERROR` and whose line text contains `timeout` (case-insensitively) match

#### Scenario: Multiple field terms

- **WHEN** the query is `level=ERROR service=auth`
- **THEN** only entries with both `level` equal to `ERROR` and `service` equal to `auth` match

#### Scenario: Or between field terms

- **WHEN** the query is `level=ERROR or level=WARN`
- **THEN** entries whose `level` is either `ERROR` or `WARN` match, and entries with any other level do not

#### Scenario: And binds tighter than or

- **WHEN** the query is `level=ERROR timeout or service=auth`
- **THEN** the rows matching are exactly those matching `(level=ERROR timeout) or service=auth`, not `level=ERROR and (timeout or service=auth)`

### Requirement: Invalid query feedback

The system SHALL detect a malformed query — for example `=value`, `field=`, an unbalanced opening or closing parenthesis, or an unterminated quoted value — indicate the invalidity to the user, and show all rows unfiltered while the query remains invalid.

#### Scenario: Malformed term

- **WHEN** the query input contains `field=`
- **THEN** the UI indicates the query is invalid and the table shows all rows

#### Scenario: Unbalanced parenthesis

- **WHEN** the query input contains `(level=ERROR timeout`
- **THEN** the UI indicates the query is invalid and the table shows all rows

#### Scenario: Unterminated quote

- **WHEN** the query input contains `requestId='abc`
- **THEN** the UI indicates the query is invalid and the table shows all rows

## ADDED Requirements

### Requirement: Grouping with parentheses

The query language SHALL allow parentheses to group sub-expressions, overriding the default AND-over-OR precedence. Groups MAY be nested.

#### Scenario: Grouping overrides precedence

- **WHEN** the query is `(requestId='some-request-id' or requestId='another-request-id') message="foo bar"`
- **THEN** only entries whose `requestId` is one of the two values and whose `message` equals `foo bar` match

#### Scenario: Nested groups

- **WHEN** the query is `((level=ERROR service=auth) or level=WARN) timeout`
- **THEN** only rows matching `service=auth`-and-`level=ERROR`, or `level=WARN`, whose line text also contains `timeout` match

### Requirement: Quoted phrase terms

A quoted bare term that is not part of a `field=value` term SHALL act as a substring term that MAY contain spaces, matched case-insensitively against the full line text.

#### Scenario: Phrase substring with spaces

- **WHEN** the query is `"connection timeout"` and a row's line text contains `Connection Timeout` inside longer text
- **THEN** that row matches

#### Scenario: Unmatched phrase

- **WHEN** the query is a quoted phrase that occurs nowhere in the file
- **THEN** no rows match and the empty result is visible to the user

### Requirement: Query reset control

The system SHALL provide a control next to the query input that, in one action, clears the query input and restores the unfiltered view.

#### Scenario: Reset after filtering

- **WHEN** the user activates the reset control while a query filters the rows
- **THEN** the query input is cleared, all rows are shown again in file order, and any invalid-query indication is gone
