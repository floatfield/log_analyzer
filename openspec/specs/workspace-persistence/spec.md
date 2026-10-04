# workspace-persistence Specification

## Purpose

Defines the workspace state the application remembers across sessions: which files are favorites and which columns were last selected for each file, so returning to a log restores the user's context.

## Requirements

### Requirement: File favorites

The system SHALL let the user mark the currently open file as a favorite and unmark it, SHALL persist favorites across restarts, and SHALL let the user reopen a favorite file from a list of favorites.

#### Scenario: Marking a favorite

- **WHEN** the user activates the favorite control for the open file
- **THEN** the file appears in the favorites list

#### Scenario: Favorites persist across restarts

- **WHEN** the user marks a file as favorite, closes the application, and starts it again
- **THEN** the file is still in the favorites list

#### Scenario: Reopening a favorite

- **WHEN** the user picks a file from the favorites list
- **THEN** that file is opened and displayed like any other opened file

#### Scenario: Removing a favorite

- **WHEN** the user removes a file from the favorites list or unmarks it while open
- **THEN** the file no longer appears in the favorites list

### Requirement: Per-file column persistence

The system SHALL remember the visible-column selection per file path and SHALL restore it when that file is opened again. A file with no remembered selection SHALL open with the default columns. Changes to the selection SHALL update the remembered selection.

#### Scenario: Columns restored on reopen

- **WHEN** the user selects columns for a file, closes it, and later reopens the same file
- **THEN** the table shows the columns selected in the earlier session

#### Scenario: Selection updates are remembered

- **WHEN** the user adds or removes a visible column for a file that has a remembered selection
- **THEN** the remembered selection is updated, so a later reopen reflects the change

#### Scenario: File without remembered selection uses defaults

- **WHEN** a file is opened that has no remembered column selection
- **THEN** the table shows the default columns

### Requirement: Graceful handling of unreadable persisted state

If the persisted workspace state cannot be read or parsed, the application SHALL start with empty favorites and no remembered column selections instead of failing.

#### Scenario: Corrupted state file

- **WHEN** the persisted state file is missing or contains invalid data and the application starts
- **THEN** the application starts normally with empty favorites and default column behavior
