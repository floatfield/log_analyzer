# log-file-access Specification

## Purpose

Defines how the application opens and reads log files of arbitrary size, so that browsing and filtering never depend on loading or parsing an entire file into memory at once.

## Requirements

### Requirement: Opening a log file

The system SHALL let the user pick a local file through a native file dialog and display its lines as rows. The dialog SHALL offer files of any extension: a file's name SHALL NOT determine whether it can be opened or how its lines are interpreted.

#### Scenario: Open a valid file

- **WHEN** the user selects an existing readable file in the open dialog
- **THEN** the table displays that file's rows

#### Scenario: Any extension is openable

- **WHEN** the user picks a readable file whose extension is not `.log` or `.txt` (for example `capture.out`, `trace.jsonl`, or a file with no extension)
- **THEN** the file opens like any other log file: its lines are shown as rows and classified per line as structured entries or raw lines

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

### Requirement: Reloading the current file

The system SHALL provide a control that re-reads the currently open file from disk and replaces the displayed rows with the file's current contents, and SHALL keep the control inactive when no file is open. An active query SHALL remain in place and continue to filter the reloaded contents.

#### Scenario: Reload picks up changes on disk

- **WHEN** the file's contents on disk change after it was opened and the user activates the reload control
- **THEN** the displayed rows are replaced with the file's current contents, including previously unseen lines

#### Scenario: Query remains applied after reload

- **WHEN** a query filters the rows and the user activates the reload control
- **THEN** the query input keeps its text and the reloaded rows are filtered by it

#### Scenario: No file open

- **WHEN** no file is open and the reload control is shown
- **THEN** the control is inactive and activating it changes nothing
