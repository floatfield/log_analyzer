# app-appearance Specification

## Purpose

Defines the application's visual theme. The application renders with a light theme by default so text and table content read clearly on bright backgrounds.

## Requirements

### Requirement: Light theme by default

The application SHALL render with a light theme from startup, covering the table, panels, toolbar, and dialogs, without requiring the user to configure anything.

#### Scenario: Fresh start uses light theme

- **WHEN** the application is started for the first time
- **THEN** all UI surfaces (table, toolbar, side panels, dialogs) are rendered in the light theme

#### Scenario: Highlighting remains readable on the light theme

- **WHEN** rows with error or warning highlighting are shown under the light theme
- **THEN** the row text remains readable against the tinted backgrounds
