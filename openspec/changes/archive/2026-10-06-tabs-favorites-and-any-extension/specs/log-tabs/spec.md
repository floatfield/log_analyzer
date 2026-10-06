# log-tabs Specification

## Purpose

Lets the user work with several logs at once: every opened file gets a tab, and switching tabs swaps the whole explored view — file, filter, and selection — without reopening anything.

## ADDED Requirements

### Requirement: Tab per opened file

The application SHALL show a tab strip and give every opened file its own tab, labeled with the file's name. Opening a file SHALL activate a tab for it instead of replacing an already-open file: the previously opened file SHALL remain open in its own tab and SHALL keep loading (background indexing and field discovery continue for it).

#### Scenario: Opening a second file keeps the first

- **WHEN** a file is open and the user opens a different file
- **THEN** the newly opened file becomes the active tab and the first file remains open in its own tab

#### Scenario: First file keeps loading in the background

- **WHEN** the user opens a large file and immediately opens another file
- **THEN** the first file's tab still exists and that file finishes indexing while another tab is active

#### Scenario: Tab is labeled with the file name

- **WHEN** a file is opened in a tab
- **THEN** the tab shows the file's name (with the full path available as the tab's tooltip)

### Requirement: Switching tabs

Activating a tab SHALL make that file the explored log: the table, column panel, row detail pane, and status bar SHALL reflect that file, and the tab's session context SHALL be restored exactly as it was when the tab was last active — its query input, its filtering result, and its row selection.

#### Scenario: Switching to another tab

- **WHEN** two files are open and the user clicks the first file's tab
- **THEN** the table shows the first file's rows and the column panel lists its fields

#### Scenario: Per-tab context survives a round trip

- **WHEN** the user filters tab A by a query and selects a row, switches to tab B, then switches back to tab A
- **THEN** tab A again shows its query text in the input, the same filtered rows, and the same selected row

### Requirement: Closing a tab

Every tab SHALL provide a close control. Closing a tab SHALL stop that file's background work and SHALL NOT affect other tabs. Closing the active tab SHALL activate a neighboring tab; closing the last tab SHALL return the application to the no-file-open state.

#### Scenario: Closing an inactive tab

- **WHEN** two files are open and the user closes the inactive tab
- **THEN** that tab disappears, the active view is unchanged, and the remaining file keeps working

#### Scenario: Closing the active tab activates a neighbor

- **WHEN** the user closes the active tab while another tab exists
- **THEN** a neighboring tab becomes active and is shown

#### Scenario: Closing the last tab

- **WHEN** the user closes the only open tab
- **THEN** the application shows the no-file-open state

### Requirement: Opening an already-open file focuses its tab

Opening a file that is already open in a tab SHALL activate that existing tab instead of opening a duplicate, and the tab's session context SHALL be preserved.

#### Scenario: Reopening an open file

- **WHEN** a file is open in a tab with an active query and the user opens the same file again
- **THEN** no duplicate tab is created, the file's existing tab becomes active, and its query still filters its rows
