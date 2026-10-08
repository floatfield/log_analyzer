//! External sort of log files into one `@timestamp`-ascending output file
//! (spec: log-sort). Inputs carry no ordering requirement; entries are
//! written unchanged — no property is added or rewritten. The work streams
//! in bounded chunks (design D1): inputs are read sequentially into sorted
//! runs of at most [`RUN_ENTRIES`] entries, and the runs are k-way merged
//! into the output by the stamp-optional merge core (`merge_runs` with
//! `stamp = false`), so stability and all-or-nothing output are inherited
//! from the tested merge (design D4/D8).

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::merge::{FileCursor, MergeError, merge_runs, output_collides, write_error};

/// Entries per run file (design D1): memory is bounded by one run of entries
/// plus the merge heap over the run files.
const RUN_ENTRIES: usize = 100_000;

/// One buffered entry: its `@timestamp`, the entry itself, the raw lines
/// that preceded it in its input (flushed immediately before it, design D2),
/// and the global read position `(input_index, line_no)` — a total order
/// that makes ties deterministic (design D1/D4).
struct SortEntry {
    timestamp: String,
    entry: Map<String, Value>,
    raws: Vec<String>,
    sequence: (usize, usize),
}

/// Sort `inputs` into `output`, ascending by `@timestamp` (spec: log-sort).
/// Equal timestamps keep command-line input order, then file line order;
/// every input line (entries and raw lines) appears in the output exactly
/// once. All-or-nothing like the merge: the result is installed with a
/// temp-file rename, and any failure leaves `output` exactly as it was,
/// with no run-file leftovers.
pub fn sort_files(inputs: &[PathBuf], output: &Path) -> Result<usize, MergeError> {
    sort_files_with_budget(inputs, output, RUN_ENTRIES)
}

/// [`sort_files`] with the run size forced — private seam for tests that
/// exercise multi-run behavior with a tiny budget (design D1/D4).
fn sort_files_with_budget(
    inputs: &[PathBuf],
    output: &Path,
    run_entries: usize,
) -> Result<usize, MergeError> {
    if let Some(input) = output_collides(inputs, output) {
        return Err(MergeError {
            file: output.display().to_string(),
            line: 0,
            reason: format!("output path names the input file {input}"),
        });
    }
    let runs = write_runs(inputs, output, run_entries)?;
    // The runs are ascending by construction, so the final pass is exactly
    // the merge — unstamped (design D3/D5), installed with temp+rename.
    let result = merge_runs(&runs, output, false);
    for path in &runs {
        let _ = std::fs::remove_file(path);
    }
    result
}

/// Read `inputs` sequentially, emitting sorted run files of at most
/// `run_entries` entries each; returns the run paths in creation order. A
/// contract violation (a JSON-object line without a string `@timestamp`)
/// aborts with `file:line` context, leaving no run files behind.
fn write_runs(
    inputs: &[PathBuf],
    output: &Path,
    run_entries: usize,
) -> Result<Vec<PathBuf>, MergeError> {
    let mut runs = Vec::new();
    if let Err(e) = write_runs_inner(inputs, output, run_entries, &mut runs) {
        for path in &runs {
            let _ = std::fs::remove_file(path);
        }
        return Err(e);
    }
    Ok(runs)
}

fn write_runs_inner(
    inputs: &[PathBuf],
    output: &Path,
    run_entries: usize,
    runs: &mut Vec<PathBuf>,
) -> Result<(), MergeError> {
    let mut cursors: Vec<FileCursor> = inputs
        .iter()
        .map(|path| FileCursor::open_unordered(path))
        .collect::<Result<_, _>>()?;
    let mut buffer: Vec<SortEntry> = Vec::new();
    // Raw lines met after an input's last entry have no following entry to
    // attach to (design D2); they ride to the end of the current run.
    let mut tail: Vec<String> = Vec::new();
    for (input_index, cursor) in cursors.iter_mut().enumerate() {
        while let Some((timestamp, entry)) = cursor.next_entry()? {
            let raws = cursor.take_pending_raw();
            let sequence = (input_index, cursor.line_no());
            if buffer.len() >= run_entries {
                flush_run(&mut buffer, &mut tail, output, runs)?;
            }
            buffer.push(SortEntry {
                timestamp,
                entry,
                raws,
                sequence,
            });
        }
        tail.extend(cursor.take_pending_raw());
    }
    flush_run(&mut buffer, &mut tail, output, runs)
}

/// Sort the buffered entries by `(timestamp, sequence)`, write them to the
/// next run file — each entry's raw lines immediately before it, any
/// trailing raw lines after the last entry — and clear the buffer. The run
/// file is ascending, so the final merge accepts it as an input.
fn flush_run(
    buffer: &mut Vec<SortEntry>,
    tail: &mut Vec<String>,
    output: &Path,
    runs: &mut Vec<PathBuf>,
) -> Result<(), MergeError> {
    if buffer.is_empty() && tail.is_empty() {
        return Ok(());
    }
    buffer.sort_by(|a, b| {
        a.timestamp
            .cmp(&b.timestamp)
            .then(a.sequence.cmp(&b.sequence))
    });
    let path = run_path_for(output, runs.len());
    let file = File::create(&path).map_err(|e| MergeError {
        file: output.display().to_string(),
        line: 0,
        reason: format!("failed to create sort run file: {e}"),
    })?;
    let mut writer = BufWriter::new(file);
    let result = write_run_contents(buffer, tail, output, &mut writer)
        .and_then(|()| writer.flush().map_err(|e| write_error(output, e)));
    drop(writer);
    if let Err(e) = result {
        let _ = std::fs::remove_file(&path);
        return Err(e);
    }
    runs.push(path);
    buffer.clear();
    tail.clear();
    Ok(())
}

fn write_run_contents(
    buffer: &[SortEntry],
    tail: &[String],
    output: &Path,
    writer: &mut BufWriter<File>,
) -> Result<(), MergeError> {
    for run_entry in buffer {
        for raw in &run_entry.raws {
            writeln!(writer, "{raw}").map_err(|e| write_error(output, e))?;
        }
        let line = serde_json::to_string(&run_entry.entry)
            .map_err(|e| write_error(output, std::io::Error::other(e)))?;
        writeln!(writer, "{line}").map_err(|e| write_error(output, e))?;
    }
    for raw in tail {
        writeln!(writer, "{raw}").map_err(|e| write_error(output, e))?;
    }
    Ok(())
}

/// Hidden run file in the output's directory (same filesystem as the
/// output's temp file); the index and pid keep runs apart and ordered.
fn run_path_for(output: &Path, index: usize) -> PathBuf {
    let name = output
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".to_owned());
    output.parent().unwrap_or(Path::new(".")).join(format!(
        ".{name}.log-sort-run-{index}-{}",
        std::process::id()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log_file::parse_object;

    /// Write a test input file; every string gets a `\n` terminator.
    fn write_input(name: &str, lines: &[&str]) -> PathBuf {
        let path = std::env::temp_dir().join(format!("log_sort_test_{name}"));
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

    /// Entries are re-serialized compactly (design: as log-merge does); this
    /// is the expected output form of an input entry line.
    fn compact(entry_text: &str) -> String {
        serde_json::to_string(&parse_object(entry_text.as_bytes()).unwrap()).unwrap()
    }

    /// (timestamp, raw line) of every entry line of `contents`, in line order.
    fn output_entries(contents: &str) -> Vec<(String, String)> {
        contents
            .lines()
            .filter_map(|line| parse_object(line.as_bytes()).map(|e| (e, line)))
            .map(|(e, line)| {
                let ts = match e.get("@timestamp") {
                    Some(Value::String(text)) => text.clone(),
                    other => panic!("entry without string @timestamp: {other:?}"),
                };
                (ts, line.to_owned())
            })
            .collect()
    }

    fn all_lines(contents: &str) -> Vec<&str> {
        contents.lines().collect()
    }

    // ---- 3.1: run writer ----

    #[test]
    fn runs_are_sorted_with_ties_by_input_then_line() {
        let a = write_input(
            "runs_a",
            &[
                &entry("2026-01-01T10:03:00.000Z"),
                &entry("2026-01-01T10:00:00.000Z"),
            ],
        );
        let b = write_input(
            "runs_b",
            &[
                &entry("2026-01-01T10:02:00.000Z"),
                &entry("2026-01-01T10:00:00.000Z"),
            ],
        );
        let out = std::env::temp_dir().join("log_sort_test_runs_out");
        let runs = write_runs(&[a.clone(), b.clone()], &out, 2).unwrap();
        assert_eq!(runs.len(), 2, "4 entries with budget 2");
        // Run 0 holds the first two entries read (both of a); run 1 both of
        // b. Each run is ascending on its own.
        let run0 = std::fs::read_to_string(&runs[0]).unwrap();
        let stamps: Vec<_> = output_entries(&run0)
            .iter()
            .map(|(ts, _)| ts.clone())
            .collect();
        assert_eq!(
            stamps,
            vec!["2026-01-01T10:00:00.000Z", "2026-01-01T10:03:00.000Z"]
        );
        // Ties within a run break by (input_index, line_no): b's first line
        // precedes b's second.
        let run1 = std::fs::read_to_string(&runs[1]).unwrap();
        let lines = all_lines(&run1);
        let (first, second) = (lines[0], lines[1]);
        assert_eq!(
            output_entries(first).remove(0).0,
            "2026-01-01T10:00:00.000Z"
        );
        assert_eq!(
            output_entries(second).remove(0).0,
            "2026-01-01T10:02:00.000Z"
        );
        for path in [&a, &b, &out] {
            let _ = std::fs::remove_file(path);
        }
        for path in &runs {
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn run_raw_lines_attach_before_their_entry() {
        let a = write_input(
            "runraw_a",
            &[
                "# lead",
                &entry("2026-01-01T10:01:00.000Z"),
                "# between",
                &entry("2026-01-01T10:03:00.000Z"),
            ],
        );
        let b = write_input(
            "runraw_b",
            &[
                &entry("2026-01-01T10:00:00.000Z"),
                &entry("2026-01-01T10:02:00.000Z"),
                "# tail of b",
            ],
        );
        let out = std::env::temp_dir().join("log_sort_test_runraw_out");
        let runs = write_runs(&[a, b], &out, 2).unwrap();
        // Inputs are read sequentially: a's two entries fill run 0 (budget
        // 2), each with its raw lines immediately before it.
        let run0 = std::fs::read_to_string(&runs[0]).unwrap();
        assert_eq!(
            all_lines(&run0),
            vec![
                "# lead",
                &compact(&entry("2026-01-01T10:01:00.000Z")),
                "# between",
                &compact(&entry("2026-01-01T10:03:00.000Z")),
            ]
        );
        // Run 1: b's entries in ascending order, then b's trailing raw.
        let run1 = std::fs::read_to_string(&runs[1]).unwrap();
        assert_eq!(
            all_lines(&run1),
            vec![
                compact(&entry("2026-01-01T10:00:00.000Z")),
                compact(&entry("2026-01-01T10:02:00.000Z")),
                "# tail of b".to_owned(),
            ]
        );
        let _ = std::fs::remove_file(&out);
        for path in &runs {
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn entryless_input_raws_reach_a_run() {
        let a = write_input("rawonly_sort_a", &["# only raw lines", "nothing else"]);
        let out = std::env::temp_dir().join("log_sort_test_rawonly_out");
        let runs = write_runs(&[a], &out, 4).unwrap();
        assert_eq!(runs.len(), 1, "the tail alone still makes a run");
        assert_eq!(
            std::fs::read_to_string(&runs[0]).unwrap(),
            "# only raw lines\nnothing else\n"
        );
        let _ = std::fs::remove_file(&out);
        for path in &runs {
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn run_writer_rejects_missing_timestamp_with_file_and_line() {
        let a = write_input(
            "runbad_a",
            &[&entry("2026-01-01T10:00:00.000Z"), r#"{"level": "INFO"}"#],
        );
        let out = std::env::temp_dir().join("log_sort_test_runbad_out");
        let e = write_runs(std::slice::from_ref(&a), &out, 4).unwrap_err();
        assert_eq!(e.file, a.display().to_string());
        assert_eq!(e.line, 2);
        assert!(e.reason.contains("@timestamp"), "{}", e.reason);
        // No run files survive a failed run write (none were created here).
        let _ = std::fs::remove_file(&a);
        let _ = std::fs::remove_file(&out);
    }

    // ---- 3.2: sort driver ----

    #[test]
    fn sorts_single_unsorted_file() {
        let a = write_input(
            "single_sort_a",
            &[
                &entry("2026-01-01T10:03:00.000Z"),
                &entry("2026-01-01T10:00:00.000Z"),
                &entry("2026-01-01T10:01:00.000Z"),
            ],
        );
        let out = std::env::temp_dir().join("log_sort_test_single_out");
        let written = sort_files_with_budget(&[a], &out, 2).unwrap();
        assert_eq!(written, 3);
        let stamps: Vec<_> = output_entries(&std::fs::read_to_string(&out).unwrap())
            .iter()
            .map(|(ts, _)| ts.clone())
            .collect();
        assert_eq!(
            stamps,
            vec![
                "2026-01-01T10:00:00.000Z",
                "2026-01-01T10:01:00.000Z",
                "2026-01-01T10:03:00.000Z",
            ]
        );
        // No run leftovers.
        let leftovers: Vec<_> = std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .contains("log_sort_test_single_out.log-sort-run-")
            })
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn ties_keep_input_then_line_order_across_runs() {
        let a = write_input(
            "tie_sort_a",
            &[
                r#"{"@timestamp": "2026-01-01T10:00:00.000Z", "message": "a-first"}"#,
                r#"{"@timestamp": "2026-01-01T10:00:00.000Z", "message": "a-second"}"#,
            ],
        );
        let b = write_input(
            "tie_sort_b",
            &[r#"{"@timestamp": "2026-01-01T10:00:00.000Z", "message": "b-only"}"#],
        );
        let out = std::env::temp_dir().join("log_sort_test_tie_out");
        // Budget 1: every entry gets its own run — the merge tiebreak alone
        // must reproduce the stable order (design D4): within a, file line
        // order; across inputs, command-line order.
        sort_files_with_budget(&[a.clone(), b.clone()], &out, 1).unwrap();
        let contents = std::fs::read_to_string(&out).unwrap();
        let messages: Vec<String> = contents
            .lines()
            .map(|line| {
                parse_object(line.as_bytes())
                    .unwrap()
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap()
                    .to_owned()
            })
            .collect();
        assert_eq!(
            messages,
            vec!["a-first", "a-second", "b-only"],
            "stable order across per-entry runs"
        );
        let _ = std::fs::remove_file(&out);
        for path in [&a, &b] {
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn tie_order_is_pinned_by_message_content() {
        let a = write_input(
            "tie_msg_a",
            &[r#"{"@timestamp": "2026-01-01T10:00:00.000Z", "message": "from-a"}"#],
        );
        let b = write_input(
            "tie_msg_b",
            &[r#"{"@timestamp": "2026-01-01T10:00:00.000Z", "message": "from-b"}"#],
        );
        let out = std::env::temp_dir().join("log_sort_test_tie_msg_out");
        sort_files_with_budget(&[a.clone(), b.clone()], &out, 1).unwrap();
        let contents = std::fs::read_to_string(&out).unwrap();
        let lines = all_lines(&contents);
        assert!(lines[0].contains("from-a"), "{lines:?}");
        assert!(lines[1].contains("from-b"), "{lines:?}");
        let _ = std::fs::remove_file(&out);
        for path in [&a, &b] {
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn raw_lines_are_preserved_and_entryless_inputs_survive() {
        let a = write_input(
            "raw_sort_a",
            &[
                "# leading raw",
                &entry("2026-01-01T10:02:00.000Z"),
                &entry("2026-01-01T10:00:00.000Z"),
                "# trailing raw",
            ],
        );
        let b = write_input("raw_sort_b", &["# entryless"]);
        let out = std::env::temp_dir().join("log_sort_test_raw_out");
        let written = sort_files_with_budget(&[a, b], &out, 1).unwrap();
        assert_eq!(written, 5, "2 entries + 3 raw lines");
        let contents = std::fs::read_to_string(&out).unwrap();
        let lines = all_lines(&contents);
        assert_eq!(lines.len(), 5);
        // Every input line appears exactly once.
        for expected in [
            "# leading raw".to_owned(),
            compact(&entry("2026-01-01T10:02:00.000Z")),
            compact(&entry("2026-01-01T10:00:00.000Z")),
            "# trailing raw".to_owned(),
            "# entryless".to_owned(),
        ] {
            assert_eq!(
                lines.iter().filter(|l| **l == expected).count(),
                1,
                "exactly one {expected} in {lines:?}"
            );
        }
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn entries_keep_exactly_their_input_properties() {
        let original =
            r#"{"@timestamp": "2026-01-01T10:00:00.000Z", "system": "kept", "level": "INFO"}"#;
        let a = write_input("props_a", &[original]);
        let out = std::env::temp_dir().join("log_sort_test_props_out");
        sort_files_with_budget(std::slice::from_ref(&a), &out, 1).unwrap();
        let contents = std::fs::read_to_string(&out).unwrap();
        let parsed: Vec<Map<String, Value>> = contents
            .lines()
            .map(|line| parse_object(line.as_bytes()).unwrap())
            .collect();
        assert_eq!(parsed.len(), 1);
        let expected = parse_object(original.as_bytes()).unwrap();
        assert_eq!(
            parsed[0], expected,
            "no property added, changed, or dropped"
        );
        let _ = std::fs::remove_file(&out);
        let _ = std::fs::remove_file(&a);
    }

    #[test]
    fn empty_inputs_produce_an_empty_output() {
        let a = write_input("empty_sort_a", &[]);
        let out = std::env::temp_dir().join("log_sort_test_empty_out");
        assert_eq!(sort_files_with_budget(&[a], &out, 1).unwrap(), 0);
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "");
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn output_colliding_with_input_is_rejected() {
        let a = write_input("coll_sort_a", &[&entry("2026-01-01T10:00:00.000Z")]);
        let e = sort_files_with_budget(std::slice::from_ref(&a), &a, 1).unwrap_err();
        assert!(e.reason.contains("names the input file"), "{}", e.reason);
        assert!(std::fs::read_to_string(&a).unwrap().contains("@timestamp"));
        let _ = std::fs::remove_file(&a);
    }

    #[test]
    fn failed_sort_leaves_output_and_no_run_leftovers() {
        let a = write_input(
            "fail_sort_a",
            &[&entry("2026-01-01T10:00:00.000Z"), r#"{"@timestamp": 5}"#],
        );
        let out = std::env::temp_dir().join("log_sort_test_fail_out");
        std::fs::write(&out, "previous contents").unwrap();
        let e = sort_files_with_budget(&[a], &out, 1).unwrap_err();
        assert_eq!(e.line, 2);
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "previous contents");
        let leftovers: Vec<_> = std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .contains("log_sort_test_fail_out.log-sort-run-")
            })
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn descending_input_sorts_ascending() {
        let a = write_input(
            "desc_sort_a",
            &[
                &entry("2026-01-01T10:05:00.000Z"),
                &entry("2026-01-01T10:04:00.000Z"),
                &entry("2026-01-01T10:03:00.000Z"),
            ],
        );
        let out = std::env::temp_dir().join("log_sort_test_desc_out");
        sort_files_with_budget(&[a], &out, 2).unwrap();
        let stamps: Vec<_> = output_entries(&std::fs::read_to_string(&out).unwrap())
            .iter()
            .map(|(ts, _)| ts.clone())
            .collect();
        assert_eq!(
            stamps,
            vec![
                "2026-01-01T10:03:00.000Z",
                "2026-01-01T10:04:00.000Z",
                "2026-01-01T10:05:00.000Z",
            ]
        );
        let _ = std::fs::remove_file(&out);
    }
}
