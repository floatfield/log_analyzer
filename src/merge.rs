//! Streaming merge of several log files into one output file (spec:
//! log-merge). Inputs must have their structured entries in ascending
//! `@timestamp` order; the output preserves that order (a stable k-way merge,
//! not a sort) and stamps every entry with a `system` property naming the
//! file it came from. Lines that are not JSON objects are carried through
//! unchanged, adjacent to their same-input neighbors.

use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::log_file::parse_object;

/// A validated entry paired with its `@timestamp`.
type CursorEntry = (String, Map<String, Value>);

/// Where and why a merge failed (spec: log-merge / Input contract
/// enforcement). `line` is 1-based; 0 when no particular line is involved
/// (e.g. the input could not be opened).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeError {
    /// The path as given: an input path, or the output path for I/O failures.
    pub file: String,
    /// 1-based input line number, or 0 when no line applies.
    pub line: usize,
    pub reason: String,
}

impl std::fmt::Display for MergeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.line == 0 {
            write!(f, "{}: {}", self.file, self.reason)
        } else {
            write!(f, "{}:{}: {}", self.file, self.line, self.reason)
        }
    }
}

/// Validated line cursor over one input file (design D2). Lines are consumed
/// lazily; non-object lines buffer into [`FileCursor::pending_raw`] and the
/// merge flushes them immediately before this cursor's next entry — or at
/// exhaustion — so a raw line lands between its same-input neighbors.
#[derive(Debug)]
struct FileCursor {
    reader: BufReader<File>,
    /// Path as given, for error messages.
    display: String,
    /// File name component used to stamp entries (design D4).
    system: String,
    /// Lines consumed so far; error lines are 1-based, hence +1 at report time.
    line_no: usize,
    /// `@timestamp` of the previous entry; the ascending check compares
    /// entries only — raw lines have no timestamp.
    prev_timestamp: Option<String>,
    pending_raw: Vec<String>,
    finished: bool,
}

impl FileCursor {
    fn open(path: &Path) -> Result<Self, MergeError> {
        let file = File::open(path).map_err(|e| MergeError {
            file: path.display().to_string(),
            line: 0,
            reason: format!("failed to open input: {e}"),
        })?;
        Ok(Self {
            reader: BufReader::new(file),
            display: path.display().to_string(),
            system: system_name(path),
            line_no: 0,
            prev_timestamp: None,
            pending_raw: Vec::new(),
            finished: false,
        })
    }

    /// Next entry of this file, or `None` at end of file. Raw lines met along
    /// the way are buffered, not returned.
    fn next_entry(&mut self) -> Result<Option<CursorEntry>, MergeError> {
        loop {
            if self.finished {
                return Ok(None);
            }
            let mut line = String::new();
            let read = self.reader.read_line(&mut line).map_err(|e| MergeError {
                file: self.display.clone(),
                line: self.line_no + 1,
                reason: format!("input is not valid UTF-8: {e}"),
            });
            let read = read?;
            if read == 0 {
                self.finished = true;
                return Ok(None);
            }
            self.line_no += 1;
            let content = strip_line_terminator(line);
            match parse_object(content.as_bytes()) {
                Some(entry) => {
                    let timestamp = match entry.get("@timestamp") {
                        Some(Value::String(text)) => text.clone(),
                        _ => {
                            return Err(
                                self.error("entry has no `@timestamp` string property".to_owned())
                            );
                        }
                    };
                    if let Some(prev) = &self.prev_timestamp
                        && timestamp.as_str() < prev.as_str()
                    {
                        return Err(self.error(format!(
                            "entry is out of ascending `@timestamp` order ({timestamp} after {prev})"
                        )));
                    }
                    self.prev_timestamp = Some(timestamp.clone());
                    return Ok(Some((timestamp, entry)));
                }
                None => self.pending_raw.push(content),
            }
        }
    }

    fn error(&self, reason: String) -> MergeError {
        MergeError {
            file: self.display.clone(),
            line: self.line_no,
            reason,
        }
    }
}

/// Strip a trailing `\n` or `\r\n`; other content is preserved byte-for-byte
/// (raw lines are re-emitted with a uniform `\n` terminator, design D6).
fn strip_line_terminator(mut line: String) -> String {
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
    line
}

/// File name component for the `system` stamp (design D4); paths without a
/// file name component fall back to the full path.
fn system_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Heap element: the cursor's current entry. "Greater" means "popped first",
/// so the ordering is reversed: smallest `@timestamp` first, ties broken by
/// the smallest input index (command-line order).
struct HeapItem {
    timestamp: String,
    input_index: usize,
    entry: Map<String, Value>,
}

impl Ord for HeapItem {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .timestamp
            .cmp(&self.timestamp)
            .then_with(|| other.input_index.cmp(&self.input_index))
    }
}

impl PartialOrd for HeapItem {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for HeapItem {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}

impl Eq for HeapItem {}

/// Merge `inputs` into `output` (design D2/D5). Returns the number of output
/// lines written (entries and raw lines). All-or-nothing: the output streams
/// into a temporary file beside `output` and is renamed over it only on
/// success; any failure leaves `output` exactly as it was.
pub fn merge_files(inputs: &[PathBuf], output: &Path) -> Result<usize, MergeError> {
    if let Some(input) = output_collides(inputs, output) {
        return Err(MergeError {
            file: output.display().to_string(),
            line: 0,
            reason: format!("output path names the input file {input}"),
        });
    }
    let mut cursors: Vec<FileCursor> = inputs
        .iter()
        .map(|path| FileCursor::open(path))
        .collect::<Result<_, _>>()?;

    let mut heap = std::collections::BinaryHeap::new();
    for (index, cursor) in cursors.iter_mut().enumerate() {
        if let Some((timestamp, entry)) = cursor.next_entry()? {
            heap.push(HeapItem {
                timestamp,
                input_index: index,
                entry,
            });
        }
    }

    let temp_path = temp_path_for(output);
    let file = File::create(&temp_path).map_err(|e| MergeError {
        file: output.display().to_string(),
        line: 0,
        reason: format!("failed to create output: {e}"),
    })?;
    let mut writer = BufWriter::new(file);
    let mut written = 0usize;
    let result = drain_heap(&mut heap, &mut cursors, &mut writer, &mut written);
    let finish = result.and_then(|()| writer.flush().map_err(|e| write_error(output, e)));
    drop(writer);
    match finish {
        Ok(()) => {
            if let Err(e) = install_output(&temp_path, output) {
                let _ = std::fs::remove_file(&temp_path);
                return Err(MergeError {
                    file: output.display().to_string(),
                    line: 0,
                    reason: format!("failed to finalize output: {e}"),
                });
            }
            Ok(written)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&temp_path);
            Err(e)
        }
    }
}

/// Pop entries in timestamp order, flushing each cursor's pending raw lines
/// just before that cursor's entry (or at its exhaustion), and stamping every
/// entry with its source file's name (design D2/D4).
fn drain_heap(
    heap: &mut std::collections::BinaryHeap<HeapItem>,
    cursors: &mut [FileCursor],
    writer: &mut BufWriter<File>,
    written: &mut usize,
) -> Result<(), MergeError> {
    while let Some(item) = heap.pop() {
        let cursor = &mut cursors[item.input_index];
        *written += flush_pending(cursor, writer)?;
        let mut entry = item.entry;
        entry.insert("system".to_owned(), Value::String(cursor.system.clone()));
        let line = serde_json::to_string(&entry).map_err(|e| {
            write_error_from(&cursor.display, format!("failed to serialize entry: {e}"))
        })?;
        writeln!(writer, "{line}").map_err(|e| {
            write_error_from(&cursor.display, format!("failed to write output: {e}"))
        })?;
        *written += 1;
        match cursor.next_entry()? {
            Some((timestamp, entry)) => heap.push(HeapItem {
                timestamp,
                input_index: item.input_index,
                entry,
            }),
            None => {
                // Trailing raw lines: right after this file's last entry.
                *written += flush_pending(cursor, writer)?;
            }
        }
    }
    Ok(())
}

/// Write out a cursor's buffered raw lines, unchanged and unstamped.
fn flush_pending(
    cursor: &mut FileCursor,
    writer: &mut BufWriter<File>,
) -> Result<usize, MergeError> {
    let raws = std::mem::take(&mut cursor.pending_raw);
    let count = raws.len();
    for raw in raws {
        writeln!(writer, "{raw}").map_err(|e| {
            write_error_from(&cursor.display, format!("failed to write output: {e}"))
        })?;
    }
    Ok(count)
}

fn write_error(output: &Path, e: std::io::Error) -> MergeError {
    write_error_from(
        &output.display().to_string(),
        format!("failed to write output: {e}"),
    )
}

fn write_error_from(file: &str, reason: String) -> MergeError {
    MergeError {
        file: file.to_owned(),
        line: 0,
        reason,
    }
}

/// True when `path` designates the same file as `output`: exact path
/// equality, or identity after resolving symlinks and relative components
/// (design D5). A not-yet-existing output is resolved through its parent.
fn output_collides(inputs: &[PathBuf], output: &Path) -> Option<String> {
    let output = canonical_path(output);
    for input in inputs {
        if output.as_deref() == Some(input.as_path()) {
            return Some(input.display().to_string());
        }
        if canonical_path(input).is_some_and(|resolved| Some(resolved) == output) {
            return Some(input.display().to_string());
        }
    }
    None
}

/// Fully resolved path, or `None` when neither the path nor its parent can be
/// resolved.
fn canonical_path(path: &Path) -> Option<PathBuf> {
    if let Ok(resolved) = std::fs::canonicalize(path) {
        return Some(resolved);
    }
    let parent = std::fs::canonicalize(path.parent()?).ok()?;
    Some(parent.join(path.file_name()?))
}

/// Hidden temporary file in the output's directory (same filesystem so the
/// final rename is atomic); the pid keeps concurrent runs apart.
fn temp_path_for(output: &Path) -> PathBuf {
    let name = output
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".to_owned());
    output
        .parent()
        .unwrap_or(Path::new("."))
        .join(format!(".{name}.log-merge-tmp-{}", std::process::id()))
}

/// Replace `output` with `temp` (design D5). Unix rename already replaces;
/// on Windows (rename fails onto an existing file) remove and retry.
fn install_output(temp: &Path, output: &Path) -> std::io::Result<()> {
    match std::fs::rename(temp, output) {
        Ok(()) => Ok(()),
        Err(e) if output.exists() => std::fs::remove_file(output)
            .and_then(|()| std::fs::rename(temp, output))
            .map_err(|_| e),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write a test input file; every string gets a `\n` terminator.
    fn write_input(name: &str, lines: &[&str]) -> PathBuf {
        let path = std::env::temp_dir().join(format!("log_merge_test_{name}"));
        let mut contents = lines.join("\n");
        if !lines.is_empty() {
            contents.push('\n');
        }
        std::fs::write(&path, contents).unwrap();
        path
    }

    fn entry(timestamp: &str) -> String {
        format!(r#"{{"@timestamp": "{timestamp}", "message": "m-{timestamp}"}}"#)
    }

    fn cursor_for(name: &str, lines: &[&str]) -> (PathBuf, FileCursor) {
        let path = write_input(name, lines);
        let cursor = FileCursor::open(&path).unwrap();
        (path, cursor)
    }

    fn err_of(result: Result<Option<CursorEntry>, MergeError>) -> MergeError {
        result.expect_err("expected a contract violation")
    }

    // ---- 2.1: validated line reader ----

    #[test]
    fn cursor_yields_entries_and_buffers_raws() {
        let (path, mut cursor) = cursor_for(
            "raw_buf",
            &[
                &entry("2026-01-01T10:00:00.000Z"),
                "# a raw comment",
                "",
                &entry("2026-01-01T10:01:00.000Z"),
            ],
        );
        let (first_ts, _) = cursor.next_entry().unwrap().unwrap();
        assert_eq!(first_ts, "2026-01-01T10:00:00.000Z");
        // Raw lines between the two entries are buffered, not returned.
        let (second_ts, _) = cursor.next_entry().unwrap().unwrap();
        assert_eq!(second_ts, "2026-01-01T10:01:00.000Z");
        assert_eq!(cursor.pending_raw, vec!["# a raw comment", ""]);
        assert!(cursor.next_entry().unwrap().is_none(), "EOF");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn cursor_rejects_entry_without_string_timestamp() {
        for bad in [r#"{"level": "INFO"}"#, r#"{"@timestamp": 5}"#] {
            let (path, mut cursor) =
                cursor_for("bad_ts", &[&entry("2026-01-01T10:00:00.000Z"), bad]);
            cursor.next_entry().unwrap().unwrap();
            let e = err_of(cursor.next_entry());
            assert_eq!(e.line, 2, "{bad}");
            assert_eq!(e.file, path.display().to_string());
            assert!(e.reason.contains("@timestamp"), "{}", e.reason);
            let _ = std::fs::remove_file(&path);
        }
    }

    #[test]
    fn cursor_rejects_out_of_order_entries() {
        let (path, mut cursor) = cursor_for(
            "out_of_order",
            &[
                &entry("2026-01-01T10:02:00.000Z"),
                &entry("2026-01-01T10:01:00.000Z"),
            ],
        );
        cursor.next_entry().unwrap().unwrap();
        let e = err_of(cursor.next_entry());
        assert_eq!(e.line, 2);
        assert!(e.reason.contains("ascending"), "{}", e.reason);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn cursor_allows_equal_consecutive_timestamps() {
        let (_, mut cursor) = cursor_for(
            "equal_ts",
            &[
                &entry("2026-01-01T10:00:00.000Z"),
                &entry("2026-01-01T10:00:00.000Z"),
            ],
        );
        cursor.next_entry().unwrap().unwrap();
        cursor.next_entry().unwrap().unwrap();
    }

    #[test]
    fn cursor_reports_unopenable_input() {
        let e = FileCursor::open(&std::env::temp_dir().join("log_merge_test_missing")).unwrap_err();
        assert_eq!(e.line, 0);
        assert!(e.reason.contains("failed to open input"), "{}", e.reason);
        assert!(e.to_string().starts_with(&e.file), "{}", e.to_string());
    }

    #[test]
    fn system_name_uses_file_component() {
        assert_eq!(system_name(Path::new("logs/web.log")), "web.log");
        assert_eq!(system_name(Path::new("web.log")), "web.log");
        assert_eq!(system_name(Path::new("/")), "/");
    }

    // ---- 2.2: streaming k-way merge ----

    /// Every JSON output line: parse and pull out (timestamp, system).
    fn output_entries(contents: &str) -> Vec<(String, String)> {
        contents
            .lines()
            .filter_map(|line| parse_object(line.as_bytes()))
            .map(|entry| {
                (
                    match entry.get("@timestamp") {
                        Some(Value::String(text)) => text.clone(),
                        other => panic!("entry without string @timestamp: {other:?}"),
                    },
                    match entry.get("system") {
                        Some(Value::String(text)) => text.clone(),
                        other => panic!("entry without system: {other:?}"),
                    },
                )
            })
            .collect()
    }

    #[test]
    fn merges_interleaved_inputs_in_timestamp_order() {
        let a = write_input(
            "inter_a",
            &[
                &entry("2026-01-01T10:00:00.000Z"),
                &entry("2026-01-01T10:03:00.000Z"),
            ],
        );
        let b = write_input(
            "inter_b",
            &[
                &entry("2026-01-01T10:01:00.000Z"),
                &entry("2026-01-01T10:02:00.000Z"),
            ],
        );
        let out = std::env::temp_dir().join("log_merge_test_inter_out");
        let written = merge_files(&[a.clone(), b.clone()], &out).unwrap();
        let contents = std::fs::read_to_string(&out).unwrap();
        let entries = output_entries(&contents);
        let stamps: Vec<&str> = entries.iter().map(|(ts, _)| ts.as_str()).collect();
        assert_eq!(
            stamps,
            vec![
                "2026-01-01T10:00:00.000Z",
                "2026-01-01T10:01:00.000Z",
                "2026-01-01T10:02:00.000Z",
                "2026-01-01T10:03:00.000Z",
            ]
        );
        let systems: Vec<String> = entries.iter().map(|(_, sys)| sys.clone()).collect();
        assert_eq!(
            systems,
            vec![
                system_name(&a),
                system_name(&b),
                system_name(&b),
                system_name(&a),
            ]
        );
        assert_eq!(written, 4);
        for path in [&a, &b, &out] {
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn equal_timestamps_follow_input_order() {
        let a = write_input("tie_a", &[&entry("2026-01-01T10:00:00.000Z")]);
        let b = write_input("tie_b", &[&entry("2026-01-01T10:00:00.000Z")]);
        let out = std::env::temp_dir().join("log_merge_test_tie_out");
        merge_files(&[a.clone(), b.clone()], &out).unwrap();
        let entries = output_entries(&std::fs::read_to_string(&out).unwrap());
        let systems: Vec<String> = entries.iter().map(|(_, sys)| sys.clone()).collect();
        assert_eq!(
            systems,
            vec![system_name(&a), system_name(&b)],
            "first input wins the tie"
        );
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn single_file_passes_through_in_order() {
        let a = write_input(
            "single_a",
            &[
                &entry("2026-01-01T10:00:00.000Z"),
                &entry("2026-01-01T10:01:00.000Z"),
            ],
        );
        let out = std::env::temp_dir().join("log_merge_test_single_out");
        merge_files(&[a], &out).unwrap();
        let entries = output_entries(&std::fs::read_to_string(&out).unwrap());
        let stamps: Vec<&str> = entries.iter().map(|(ts, _)| ts.as_str()).collect();
        assert_eq!(stamps.len(), 2);
        assert!(stamps.windows(2).all(|w| w[0] <= w[1]));
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn raw_lines_land_between_same_input_neighbors() {
        // A: entry 10:00, raw, blank, entry 10:02, trailing raw; B: entry 10:01.
        let a = write_input(
            "raws_a",
            &[
                &entry("2026-01-01T10:00:00.000Z"),
                "# hello from a",
                "",
                &entry("2026-01-01T10:02:00.000Z"),
                "# tail of a",
            ],
        );
        let b = write_input("raws_b", &[&entry("2026-01-01T10:01:00.000Z")]);
        let out = std::env::temp_dir().join("log_merge_test_raws_out");
        let written = merge_files(&[a, b], &out).unwrap();
        assert_eq!(written, 6, "3 entries + 3 raw lines");
        let contents = std::fs::read_to_string(&out).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        // A's raw lines stay between A's own entries (B's 10:01 entry may sit
        // in the same gap — raw placement across inputs is unrestricted).
        let hello = lines.iter().position(|l| *l == "# hello from a").unwrap();
        let blank = lines.iter().position(|l| l.is_empty()).unwrap();
        let first_a = 0; // A's 10:00 entry popped first
        let second_a = lines
            .iter()
            .position(|l| {
                parse_object(l.as_bytes()).is_some_and(|e| {
                    e.get("@timestamp") == Some(&Value::String("2026-01-01T10:02:00.000Z".into()))
                })
            })
            .unwrap();
        assert!(
            first_a < hello && hello < blank && blank < second_a,
            "{lines:?}"
        );
        let tail = lines.iter().position(|l| *l == "# tail of a").unwrap();
        assert!(
            tail > second_a,
            "trailing raw follows A's last entry: {lines:?}"
        );
        // Raw lines are verbatim and unstamped (no system property on them).
        assert_eq!(lines[hello], "# hello from a");
        for line in &lines {
            if let Some(entry) = parse_object(line.as_bytes()) {
                assert!(entry.contains_key("system"), "entries are stamped: {line}");
            }
        }
        let _ = std::fs::remove_file(&out);
    }

    // ---- 2.3: system stamping ----

    #[test]
    fn stamping_overwrites_existing_system() {
        let a = write_input(
            "stamp_a",
            &[r#"{"@timestamp": "2026-01-01T10:00:00.000Z", "system": "somewhere-else"}"#],
        );
        let out = std::env::temp_dir().join("log_merge_test_stamp_out");
        merge_files(std::slice::from_ref(&a), &out).unwrap();
        let entries = output_entries(&std::fs::read_to_string(&out).unwrap());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].1, system_name(&a), "true origin wins");
        let _ = std::fs::remove_file(&out);
    }

    // ---- 2.4: all-or-nothing output ----

    #[test]
    fn failed_merge_leaves_output_and_temp_untouched() {
        let a = write_input("fail_a", &[&entry("2026-01-01T10:00:00.000Z")]);
        // Out-of-order on the second entry: the failure surfaces mid-merge,
        // after the temp file already has content.
        let b = write_input(
            "fail_b",
            &[
                &entry("2026-01-01T10:01:00.000Z"),
                &entry("2026-01-01T10:00:30.000Z"),
            ],
        );
        let out = std::env::temp_dir().join("log_merge_test_fail_out");
        std::fs::write(&out, "previous contents").unwrap();
        let err = merge_files(&[a.clone(), b.clone()], &out).unwrap_err();
        assert!(err.reason.contains("ascending"), "{}", err.reason);
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "previous contents");
        // No temp leftovers for this output (scanned by exact output name;
        // other tests' temp files in the shared temp dir are not our concern).
        let leftovers: Vec<_> = std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .contains("log_merge_test_fail_out.log-merge-tmp")
            })
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        for path in [&a, &b, &out] {
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn successful_merge_replaces_existing_output() {
        let a = write_input("repl_a", &[&entry("2026-01-01T10:00:00.000Z")]);
        let out = std::env::temp_dir().join("log_merge_test_repl_out");
        std::fs::write(&out, "stale").unwrap();
        merge_files(&[a], &out).unwrap();
        assert_ne!(std::fs::read_to_string(&out).unwrap(), "stale");
        assert_eq!(
            output_entries(&std::fs::read_to_string(&out).unwrap()).len(),
            1
        );
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn output_colliding_with_input_is_rejected() {
        let a = write_input("coll_a", &[&entry("2026-01-01T10:00:00.000Z")]);
        let err = merge_files(std::slice::from_ref(&a), &a).unwrap_err();
        assert!(
            err.reason.contains("names the input file"),
            "{}",
            err.reason
        );
        // The input is untouched.
        assert!(std::fs::read_to_string(&a).unwrap().contains("@timestamp"));
        let _ = std::fs::remove_file(&a);
    }

    #[test]
    fn empty_inputs_produce_an_empty_output() {
        let a = write_input("empty_a", &[]);
        let out = std::env::temp_dir().join("log_merge_test_empty_out");
        assert_eq!(merge_files(&[a], &out).unwrap(), 0);
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "");
        let _ = std::fs::remove_file(&out);
    }
}
