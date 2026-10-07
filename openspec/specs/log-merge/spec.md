# log-merge Specification

## Purpose

Lets the user combine several per-system log files into one merged log file: entries stay ordered by time across inputs, and each entry records which system's file it came from, so the merged file can be explored (and filtered by origin) like any single log. Lines that are not JSON entries are carried through unchanged.

## Requirements

### Requirement: Time-ordered merge

The utility SHALL write every entry of all given input files into one output file, emitted in non-decreasing `@timestamp` order. Entries whose `@timestamp` values are equal SHALL be emitted in the order their input files were given, then in file line order. Every input entry SHALL appear in the output exactly once. The output SHALL also contain every input line that is not a JSON object, unchanged; the lines of one input SHALL keep their relative order, and since such lines carry no timestamp, their placement among the entries of other inputs is unrestricted.

#### Scenario: Two files interleave by time

- **WHEN** the user merges two files whose entries interleave in time (A has `10:00`, B has `10:01`, A has `10:02`)
- **THEN** the output contains the three entries in that time order regardless of which file each came from

#### Scenario: Equal timestamps keep input order

- **WHEN** entries from different input files carry the same `@timestamp`
- **THEN** the entries of the file given first on the command line come first, each file's entries in their file order

#### Scenario: Raw lines are preserved between their neighbors

- **WHEN** an input contains non-JSON lines before, between, or after its JSON entries
- **THEN** the output contains those lines unchanged, with that input's JSON entries still in ascending `@timestamp` order around them

#### Scenario: Large files merge with bounded memory

- **WHEN** the inputs are much larger than available memory tolerance
- **THEN** the merge completes without loading whole files into memory, and the UI-level symptom (unresponsive machine) never occurs because the utility streams entry by entry

### Requirement: System property stamps the origin

The utility SHALL add a `system` property to every entry it writes, whose value is the name of the input file the entry originates from. An entry that already has a `system` property SHALL have it replaced with the true origin. Raw lines are not entries: they SHALL pass through without a `system` property.

#### Scenario: Every entry is stamped

- **WHEN** the user merges files into an output
- **THEN** every entry in the output is a JSON object with a `system` property

#### Scenario: The stamp is the source file's name

- **WHEN** an entry originates from an input given as `logs/web.log`
- **THEN** the entry's `system` value is `web.log`

#### Scenario: Pre-existing system value is replaced

- **WHEN** an input entry already has a `system` property with a different value
- **THEN** the output entry's `system` value is the source file's name, not the original value

#### Scenario: Raw lines are not stamped

- **WHEN** an input contains non-JSON lines and the merge succeeds
- **THEN** those lines appear in the output unchanged, with no `system` property added

### Requirement: Input contract enforcement

The utility SHALL merge any input whose every line is either a JSON object entry with a string `@timestamp`, or a line that is not a JSON object, and whose entries are in ascending `@timestamp` order. When an input violates the contract — an entry without a string `@timestamp`, or an entry older than its predecessor — the utility SHALL fail with an error naming the offending file and line number, exit with a non-zero status, and SHALL NOT produce an output file.

#### Scenario: Raw lines satisfy the contract

- **WHEN** an input contains lines that do not parse as JSON objects
- **THEN** the merge treats them as raw lines and succeeds, carrying them into the output

#### Scenario: Entry without a timestamp aborts the merge

- **WHEN** an input entry has no `@timestamp` property, or its value is not a string
- **THEN** the utility reports the file and line number and exits non-zero

#### Scenario: Out-of-order input aborts the merge

- **WHEN** an input's entries are not in ascending `@timestamp` order
- **THEN** the utility reports the file and the line of the offending entry and exits non-zero

#### Scenario: Failed merge produces no output

- **WHEN** the merge fails on any input
- **THEN** the output path is left exactly as it was before the attempt — no partial output is produced, and an existing file there is not modified

### Requirement: Command-line interface

The utility SHALL be invoked as `log-merge --output <path> <input>...`, requiring at least one input file and an output path. If the output path designates the same file as one of the inputs, the utility SHALL fail without reading or writing anything. A successful merge SHALL replace any existing file at the output path.

#### Scenario: Merging two files

- **WHEN** the user runs `log-merge --output merged.log a.log b.log`
- **THEN** `merged.log` contains all entries of `a.log` and `b.log`, time-ordered and stamped

#### Scenario: Existing output is replaced

- **WHEN** the output path already exists and the merge succeeds
- **THEN** the output file's previous contents are replaced by the merged result

#### Scenario: Output colliding with an input is rejected

- **WHEN** the output path names the same file as one of the inputs
- **THEN** the utility fails with an error and modifies no file

#### Scenario: Missing arguments show usage

- **WHEN** the utility is run without an output path or without any input file
- **THEN** it prints a usage message and exits non-zero
