//! End-to-end tests for the `log-sort` binary (spec: log-sort). Each test
//! runs the real binary as a child process against fixture files.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output};

fn exe() -> &'static str {
    env!("CARGO_BIN_EXE_log-sort")
}

/// A fixture directory unique per test, under the temp dir.
struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("sort_cli_{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self { dir }
    }

    fn write(&self, name: &str, lines: &[&str]) -> PathBuf {
        let path = self.dir.join(name);
        let mut file = std::fs::File::create(&path).unwrap();
        for line in lines {
            writeln!(file, "{line}").unwrap();
        }
        path
    }

    fn run(&self, args: &[&std::path::Path]) -> Output {
        self.run_with_output(&self.dir.join("sorted.log"), args)
    }

    fn run_with_output(&self, output: &std::path::Path, args: &[&std::path::Path]) -> Output {
        Command::new(exe())
            .arg("--output")
            .arg(output)
            .args(args)
            .output()
            .expect("log-sort binary runs")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn stderr_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn sorts_unsorted_fixtures_with_raw_lines() {
    let fx = Fixture::new("unsorted");
    // Genuinely unsorted: descending in places, duplicate timestamps within
    // one input and across inputs, a raw line in the middle, and an entry
    // that already carries a `system` property (which must be kept, not
    // replaced).
    let a = fx.write(
        "a.log",
        &[
            r#"{"@timestamp": "2026-01-01T10:03:00.000Z", "message": "a-1003"}"#,
            r#"{"@timestamp": "2026-01-01T10:00:00.000Z", "message": "a-1000"}"#,
            "# raw comment from a",
            r#"{"@timestamp": "2026-01-01T10:01:00.000Z", "message": "a-1001", "system": "stale-origin"}"#,
            r#"{"@timestamp": "2026-01-01T10:01:00.000Z", "message": "a-tie"}"#,
        ],
    );
    let b = fx.write(
        "b.log",
        &[
            r#"{"@timestamp": "2026-01-01T10:01:00.000Z", "message": "b-1001"}"#,
            "PANIC: b tail",
        ],
    );

    // A pre-existing output file is replaced by a successful sort.
    std::fs::write(fx.dir.join("sorted.log"), "stale contents").unwrap();

    let output = fx.run(&[&a, &b]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_text(&output)
    );

    let sorted = std::fs::read_to_string(fx.dir.join("sorted.log")).unwrap();
    assert!(
        !sorted.contains("stale contents"),
        "output replaced: {sorted}"
    );
    let lines: Vec<&str> = sorted.lines().collect();
    assert_eq!(lines.len(), 7, "5 entries + 2 raw lines: {sorted}");

    // Every entry parses; collect (timestamp, message, has-system).
    let mut timestamps = Vec::new();
    let mut messages = Vec::new();
    for line in &lines {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            timestamps.push(
                value["@timestamp"]
                    .as_str()
                    .unwrap_or_else(|| panic!("entry without @timestamp: {line}"))
                    .to_owned(),
            );
            messages.push(
                value["message"]
                    .as_str()
                    .unwrap_or_else(|| panic!("entry without message: {line}"))
                    .to_owned(),
            );
        }
    }
    assert_eq!(messages.len(), 5, "5 entries: {sorted}");

    // Entries ascend by @timestamp.
    let mut ascending = timestamps.clone();
    ascending.sort();
    assert_eq!(timestamps, ascending, "entries ascending: {lines:?}");

    // Stable order: a's duplicates keep file order and a precedes b.
    assert_eq!(
        messages,
        vec!["a-1000", "a-1001", "a-tie", "b-1001", "a-1003"],
        "equal timestamps keep input order: {lines:?}"
    );

    // Raw lines verbatim; a's raw attaches before a's next entry, b's
    // trailing raw comes after b's entry.
    assert_eq!(lines[1], "# raw comment from a", "{lines:?}");
    assert_eq!(lines[6], "PANIC: b tail", "{lines:?}");
    assert!(lines.iter().all(|l| *l != "stale contents"));

    // Entries keep exactly their input properties: the existing `system`
    // value survives and no `system` is added anywhere else.
    assert!(
        lines[2].contains(r#""system":"stale-origin""#),
        "existing system kept: {lines:?}"
    );
    for line in &lines {
        if serde_json::from_str::<serde_json::Value>(line).is_ok() {
            let has_system = line.contains("\"system\"");
            let should = line.contains("a-1001");
            assert_eq!(has_system, should, "system only on a-1001: {line}");
        }
    }

    // No run-file leftovers in the fixture directory.
    let leftovers: Vec<_> = std::fs::read_dir(&fx.dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains(".log-sort-run-"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn violation_leaves_output_untouched_and_exits_one() {
    let fx = Fixture::new("violation");
    let good = fx.write(
        "good.log",
        &[r#"{"@timestamp": "2026-01-01T10:00:00.000Z", "message": "g"}"#],
    );
    let bad = fx.write(
        "bad.log",
        &[
            r#"{"@timestamp": "2026-01-01T10:05:00.000Z", "message": "b1"}"#,
            r#"{"level": "INFO"}"#,
        ],
    );
    let out_path = fx.dir.join("sorted.log");
    std::fs::write(&out_path, "sentinel").unwrap();

    let output = fx.run(&[&good, &bad]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let err = stderr_text(&output);
    assert!(
        err.contains(&format!("{}:2:", bad.display())),
        "file:line prefix in stderr: {err}"
    );
    assert!(err.contains("@timestamp"), "reason in stderr: {err}");
    // The output path is exactly as it was before the attempt.
    assert_eq!(
        std::fs::read_to_string(&out_path).unwrap(),
        "sentinel",
        "output untouched"
    );
    // No run-file leftovers.
    let leftovers: Vec<_> = std::fs::read_dir(&fx.dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains(".log-sort-run-"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn usage_errors_exit_two() {
    let fx = Fixture::new("usage");
    let a = fx.write(
        "a.log",
        &[r#"{"@timestamp": "2026-01-01T10:00:00.000Z", "message": "a"}"#],
    );

    // No arguments at all.
    let output = Command::new(exe()).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr_text(&output).contains("usage: log-sort"),
        "{}",
        stderr_text(&output)
    );

    // No input files.
    let output = Command::new(exe())
        .arg("--output")
        .arg(fx.dir.join("sorted.log"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");

    // Output names an input: rejected before anything is opened.
    let output = fx.run_with_output(&a, &[&a]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(
        stderr_text(&output).contains("names an input file"),
        "{}",
        stderr_text(&output)
    );
    // The input is untouched by the rejected run.
    assert!(
        std::fs::read_to_string(&a).unwrap().contains("@timestamp"),
        "input preserved"
    );
}

#[test]
fn single_file_sort_replaces_existing_output() {
    let fx = Fixture::new("single");
    let a = fx.write(
        "capture.log",
        &[
            r#"{"@timestamp": "2026-01-01T10:03:00.000Z", "message": "third"}"#,
            r#"{"@timestamp": "2026-01-01T10:00:00.000Z", "message": "first"}"#,
            r#"{"@timestamp": "2026-01-01T10:01:00.000Z", "message": "second"}"#,
        ],
    );
    let out_path = fx.dir.join("sorted.log");
    std::fs::write(&out_path, "stale").unwrap();

    let output = fx.run(&[&a]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_text(&output)
    );

    let sorted = std::fs::read_to_string(&out_path).unwrap();
    let messages: Vec<String> = sorted
        .lines()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line).unwrap()["message"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(messages, vec!["first", "second", "third"]);
}
