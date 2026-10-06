# Spec Delta

## MODIFIED Requirements

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
