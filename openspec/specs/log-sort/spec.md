# log-sort Specification

## Purpose

Lets the user bring log files whose entries are in any `@timestamp` order into the ascending form the analyzer and `log-merge` expect: one output file whose entries are sorted by time, with every line preserved and nothing added to the entries.

## Requirements

### Requirement: Ascending output

The utility SHALL write every entry of all given input files into one output file, emitted in non-decreasing `@timestamp` order. Entries whose `@timestamp` values are equal SHALL be emitted in the order their input files were given, then in file line order. Every input entry SHALL appear in the output exactly once. The output SHALL also contain every input line that is not a JSON object, unchanged; the lines of one input SHALL keep their relative order, and since such lines carry no timestamp, their placement among the entries of other inputs is unrestricted.

#### Scenario: Unsorted file is sorted

- **WHEN** the user sorts a file whose entries are `10:03`, `10:00`, `10:01`
- **THEN** the output contains the same three entries in the order `10:00`, `10:01`, `10:03`

#### Scenario: Equal timestamps keep input order

- **WHEN** entries from different input files carry the same `@timestamp`
- **THEN** the entries of the file given first on the command line come first, each file's entries in their file order

#### Scenario: Raw lines are preserved

- **WHEN** an input contains non-JSON lines before, between, or after its JSON entries
- **THEN** the output contains those lines unchanged, and each input's raw lines keep their relative order

#### Scenario: Large files sort with bounded memory

- **WHEN** the inputs are much larger than available memory tolerance
- **THEN** the sort completes without loading whole files into memory, streaming the work in bounded chunks

### Requirement: Entries are not modified

The utility SHALL emit each entry with the properties it had in the input. The utility SHALL NOT add, remove, or overwrite any entry property; in particular it SHALL NOT add a `system` property.

#### Scenario: No properties added

- **WHEN** the user sorts a file whose entries have only `@timestamp`, `level`, and `message` properties
- **THEN** every output entry has exactly those properties with the same values, and no `system` property

#### Scenario: Existing system value is kept

- **WHEN** an input entry already has a `system` property
- **THEN** the output entry's `system` value is the input's value, not replaced

### Requirement: Input contract enforcement

The utility SHALL sort any input whose every line is either a JSON object entry with a string `@timestamp`, or a line that is not a JSON object. There is no ordering requirement on the input entries. When an input contains an entry without a string `@timestamp`, the utility SHALL fail with an error naming the offending file and line number, exit with a non-zero status, and SHALL NOT produce an output file.

#### Scenario: Raw lines satisfy the contract

- **WHEN** an input contains lines that do not parse as JSON objects
- **THEN** the sort treats them as raw lines and succeeds, carrying them into the output

#### Scenario: Any entry order is accepted

- **WHEN** an input's entries are in descending or arbitrary `@timestamp` order
- **THEN** the sort succeeds and produces ascending output

#### Scenario: Entry without a timestamp aborts the sort

- **WHEN** an input entry has no `@timestamp` property, or its value is not a string
- **THEN** the utility reports the file and line number and exits non-zero

#### Scenario: Failed sort produces no output

- **WHEN** the sort fails on any input
- **THEN** the output path is left exactly as it was before the attempt — no partial output is produced, and an existing file there is not modified

### Requirement: Command-line interface

The utility SHALL be invoked as `log-sort --output <path> <input>...`, requiring at least one input file and an output path. If the output path designates the same file as one of the inputs, the utility SHALL fail without reading or writing anything. A successful sort SHALL replace any existing file at the output path.

#### Scenario: Sorting a single file

- **WHEN** the user runs `log-sort --output sorted.log capture.log`
- **THEN** `sorted.log` contains every line of `capture.log` with the entries in ascending `@timestamp` order

#### Scenario: Existing output is replaced

- **WHEN** the output path already exists and the sort succeeds
- **THEN** the output file's previous contents are replaced by the sorted result

#### Scenario: Output colliding with an input is rejected

- **WHEN** the output path names the same file as one of the inputs
- **THEN** the utility fails with an error and modifies no file

#### Scenario: Missing arguments show usage

- **WHEN** the utility is run without an output path or without any input file
- **THEN** it prints a usage message and exits non-zero
