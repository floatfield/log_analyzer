# Spec Delta

## ADDED Requirements

### Requirement: Selecting rows by clicking

The table SHALL let the user select a row by clicking it, deselect it by clicking the selected row again, and move the selection by clicking a different row. Opening a file and reloading the current file SHALL clear the selection.

#### Scenario: Clicking a row selects it

- **WHEN** the user clicks a row
- **THEN** that row becomes the selected row and is visually highlighted

#### Scenario: Clicking the selected row deselects it

- **WHEN** the user clicks the currently selected row
- **THEN** the selection is cleared and the highlight is removed

#### Scenario: Clicking another row moves the selection

- **WHEN** a row is selected and the user clicks a different row
- **THEN** the different row becomes the selected row and the previous highlight is removed

#### Scenario: Opening a file clears the selection

- **WHEN** a row is selected and the user opens a file
- **THEN** no row is selected

#### Scenario: Reloading clears the selection

- **WHEN** a row is selected and the user activates the reload control
- **THEN** no row is selected

### Requirement: Row detail pane

The right panel SHALL be split vertically: the column selection interface SHALL occupy the upper pane and a row detail pane SHALL occupy the lower pane. When a structured entry is selected, the detail pane SHALL show its fields as pretty-printed JSON; when a raw line is selected, it SHALL show that line's original text. With no selection, the pane SHALL show a placeholder instead of row contents. The pane SHALL provide a Copy control that is active only while a row is selected.

#### Scenario: Split layout

- **WHEN** the main view is shown
- **THEN** the right panel shows the column selection interface above the row detail pane

#### Scenario: Structured row detail

- **WHEN** the selected row's line parses as a JSON object
- **THEN** the detail pane shows the object's fields as pretty-printed JSON

#### Scenario: Raw line detail

- **WHEN** the selected row's line does not parse as a JSON object
- **THEN** the detail pane shows the line's original text

#### Scenario: No selection placeholder

- **WHEN** no row is selected
- **THEN** the detail pane shows a placeholder instead of row contents

#### Scenario: Copying the detail text

- **WHEN** a row is selected and the user activates the Copy control
- **THEN** the detail text shown by the pane is placed on the system clipboard

#### Scenario: Copy inactive without selection

- **WHEN** no row is selected
- **THEN** the Copy control is inactive and activating it changes nothing
