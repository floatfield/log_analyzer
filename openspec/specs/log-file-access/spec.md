# log-file-access Specification

## Purpose

Defines how the application opens and reads log files of arbitrary size, so that browsing and filtering never depend on loading or parsing an entire file into memory at once.

## Requirements

### Requirement: Opening a log file

The system SHALL let the user pick a local log file through a native file dialog and display its lines as rows.

#### Scenario: Open a valid file

- **WHEN** the user selects an existing readable file in the open dialog
- **THEN** the table displays that file's rows

#### Scenario: Open fails

- **WHEN** the selected file cannot be read (missing or permission denied)
- **THEN** the system shows an error indication and keeps the previously loaded state unchanged

### Requirement: Lazy line access

The system SHALL index line positions and read line contents on demand, so that opening a file does not require reading or parsing the whole file before rows can be shown.

#### Scenario: Opening a very large file

- **WHEN** the user opens a log file much larger than available memory tolerance
- **THEN** rows become visible without the system having read the entire file first, and the UI does not freeze

#### Scenario: Jumping to an arbitrary region

- **WHEN** the user scrolls or jumps to a region far from the start of the file
- **THEN** the rows in that region are produced by reading only that region of the file

### Requirement: Line classification

The system SHALL classify each line: a line whose content parses as a JSON object is a structured entry; every other line is a raw line.

#### Scenario: JSON object line

- **WHEN** a line contains a JSON object such as `{"level": "INFO", "message": "hi"}`
- **THEN** it is treated as a structured entry with the object's fields

#### Scenario: Non-object JSON line

- **WHEN** a line parses as JSON but is not an object (for example `123` or `[1,2]`)
- **THEN** it is treated as a raw line

#### Scenario: Non-JSON line

- **WHEN** a line is not valid JSON, such as `PANIC: unexpected state`
- **THEN** it is treated as a raw line

### Requirement: File-order row sequence

The system SHALL present all rows in original file order, interleaving structured entries and raw lines without reordering.

#### Scenario: Out-of-order timestamps preserved

- **WHEN** a file contains structured entries whose timestamps are not monotonically increasing
- **THEN** the rows keep the order in which the lines appear in the file

#### Scenario: Raw lines interleaved

- **WHEN** raw lines appear between structured entries
- **THEN** each raw line appears as a row at its original position between those entries

### Requirement: Field name discovery

The system SHALL discover the set of field names that occur in the file's structured entries and expose that set for column configuration.

#### Scenario: Fields from mixed entries

- **WHEN** structured entries in the file use fields such as `@timestamp`, `level`, `message`, `requestId`, `service`, `userId`, `durationMs`, and `stackTrace`
- **THEN** the selectable field set contains all of these names

#### Scenario: Discovery does not block the UI

- **WHEN** field discovery is still scanning a large file
- **THEN** the UI stays responsive and the selectable field set grows as discovery progresses
