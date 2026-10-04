# Spec Delta

## ADDED Requirements

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
