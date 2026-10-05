# log-table-view Specification

## MODIFIED Requirements

### Requirement: Selecting rows by clicking

The table SHALL let the user select a row by clicking it without the configured filter modifier held (a click holding the filter modifier is a filter action, not a selection action — see the modifier-click-filtering capability), deselect it by clicking the selected row again, and move the selection by clicking a different row. Opening a file and reloading the current file SHALL clear the selection.

#### Scenario: Clicking a row selects it

- **WHEN** the user clicks a row without the filter modifier held
- **THEN** that row becomes the selected row and is visually highlighted

#### Scenario: Clicking the selected row deselects it

- **WHEN** the user clicks the currently selected row without the filter modifier held
- **THEN** the selection is cleared and the highlight is removed

#### Scenario: Clicking another row moves the selection

- **WHEN** a row is selected and the user clicks a different row without the filter modifier held
- **THEN** the different row becomes the selected row and the previous highlight is removed

#### Scenario: Opening a file clears the selection

- **WHEN** a row is selected and the user opens a file
- **THEN** no row is selected

#### Scenario: Reloading clears the selection

- **WHEN** a row is selected and the user activates the reload control
- **THEN** no row is selected
