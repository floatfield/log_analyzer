//! End-to-end tests for the `log-merge` binary (spec: log-merge). Each test
//! runs the real binary as a child process against fixture files.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output};

fn exe() -> &'static str {
    env!("CARGO_BIN_EXE_log-merge")
}

/// A fixture directory unique per test, under the target dir so `cargo clean`
/// sweeps it.
struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("merge_cli_{name}"));
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
        self.run_with_output(&self.dir.join("merged.log"), args)
    }

    fn run_with_output(&self, output: &std::path::Path, args: &[&std::path::Path]) -> Output {
        Command::new(exe())
            .arg("--output")
            .arg(output)
            .args(args)
            .output()
            .expect("log-merge binary runs")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn entry(ts: &str, system: Option<&str>) -> String {
    let base = format!(r#"{{"@timestamp": "{ts}", "message": "m-{ts}""#);
    match system {
        Some(s) => format!("{base}, \"system\": \"{s}\"}}"),
        None => format!("{base}}}"),
    }
}

fn stderr_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn merges_two_fixtures_with_raw_lines() {
    let fx = Fixture::new("interleave");
    let a = fx.write(
        "web.log",
        &[
            &entry("2026-01-01T10:00:00.000Z", None),
            "# raw comment from web",
            &entry("2026-01-01T10:03:00.000Z", None),
            "PANIC: web tail",
        ],
    );
    let b = fx.write(
        "auth.log",
        &[
            &entry("2026-01-01T10:01:00.000Z", None),
            &entry("2026-01-01T10:02:00.000Z", None),
            &entry("2026-01-01T10:03:00.000Z", Some("stale-origin")),
        ],
    );

    let output = fx.run(&[&a, &b]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_text(&output)
    );

    let merged = std::fs::read_to_string(fx.dir.join("merged.log")).unwrap();
    let lines: Vec<&str> = merged.lines().collect();
    assert_eq!(lines.len(), 7, "5 entries + 2 raw lines: {merged}");

    // Entries are ascending by `@timestamp` across the whole file; every
    // input entry appears exactly once; `system` names the true origin.
    let mut timestamps = Vec::new();
    let mut pairs: Vec<(String, String)> = Vec::new();
    for line in &lines {
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(value) => {
                let ts = value["@timestamp"]
                    .as_str()
                    .unwrap_or_else(|| panic!("entry without @timestamp: {line}"))
                    .to_owned();
                let system = value["system"]
                    .as_str()
                    .unwrap_or_else(|| panic!("entry without system: {line}"))
                    .to_owned();
                timestamps.push(ts.clone());
                pairs.push((ts, system));
            }
            Err(_) => {
                // Raw line: verbatim, one of web's two raw lines.
                assert!(
                    *line == "# raw comment from web" || *line == "PANIC: web tail",
                    "unexpected raw line: {line}"
                );
            }
        }
    }
    let mut sorted = timestamps.clone();
    sorted.sort();
    assert_eq!(
        timestamps, sorted,
        "entries ascending by @timestamp: {lines:?}"
    );

    // Each input entry exactly once, stamped with its true origin. 10:03
    // exists in both inputs, so both origins appear at that timestamp.
    pairs.sort();
    let expected = vec![
        ("2026-01-01T10:00:00.000Z".to_owned(), "web.log".to_owned()),
        ("2026-01-01T10:01:00.000Z".to_owned(), "auth.log".to_owned()),
        ("2026-01-01T10:02:00.000Z".to_owned(), "auth.log".to_owned()),
        ("2026-01-01T10:03:00.000Z".to_owned(), "auth.log".to_owned()),
        ("2026-01-01T10:03:00.000Z".to_owned(), "web.log".to_owned()),
    ];
    assert_eq!(pairs, expected, "every entry exactly once, right origin");

    // The pre-existing `system` on an input entry was overwritten.
    assert!(
        merged.contains("\"system\":\"auth.log\""),
        "stale system overwritten: {merged}"
    );
    assert!(
        !merged.contains("stale-origin"),
        "stale system gone: {merged}"
    );
}

#[test]
fn violation_leaves_output_untouched_and_exits_one() {
    let fx = Fixture::new("violation");
    let good = fx.write("good.log", &[&entry("2026-01-01T10:00:00.000Z", None)]);
    let bad = fx.write(
        "bad.log",
        &[
            &entry("2026-01-01T10:05:00.000Z", None),
            &entry("2026-01-01T10:04:00.000Z", None),
        ],
    );
    let out_path = fx.dir.join("merged.log");
    std::fs::write(&out_path, "sentinel").unwrap();

    let output = fx.run(&[&good, &bad]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let err = stderr_text(&output);
    assert!(
        err.contains(&format!("{}:2:", bad.display())),
        "file:line prefix in stderr: {err}"
    );
    assert!(err.contains("ascending"), "reason in stderr: {err}");
    // The output path is exactly as it was before the attempt.
    assert_eq!(
        std::fs::read_to_string(&out_path).unwrap(),
        "sentinel",
        "output untouched"
    );
}

#[test]
fn usage_errors_exit_two() {
    let fx = Fixture::new("usage");
    let a = fx.write("a.log", &[&entry("2026-01-01T10:00:00.000Z", None)]);

    // No --output.
    let output = Command::new(exe()).arg(&a).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr_text(&output).contains("usage:"),
        "{}",
        stderr_text(&output)
    );

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
fn missing_input_exits_one_without_output() {
    let fx = Fixture::new("missing");
    let a = fx.write("a.log", &[&entry("2026-01-01T10:00:00.000Z", None)]);
    let ghost = fx.dir.join("ghost.log");

    let output = fx.run(&[&a, &ghost]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let err = stderr_text(&output);
    assert!(
        err.contains("failed to open input"),
        "reason in stderr: {err}"
    );
    assert!(!fx.dir.join("merged.log").exists(), "no output produced");
}
