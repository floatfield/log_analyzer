# Spec Delta

## MODIFIED Requirements

### Requirement: File favorites

The system SHALL let the user mark the currently open file as a favorite and unmark it, SHALL persist favorites across restarts, and SHALL let the user reopen a favorite file from a list of favorites. The favorites list SHALL be presented sorted by path, compared case-insensitively, regardless of the order in which the files were marked.

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

#### Scenario: Favorites are listed sorted

- **WHEN** the favorites list contains files that were marked in an order other than alphabetical (for example `/logs/b.log` marked before `/logs/a.log` and `/Logs/c.log` before both)
- **THEN** the favorites list presents them sorted by path, compared case-insensitively (`/logs/a.log`, `/Logs/c.log`, `/logs/b.log`)
