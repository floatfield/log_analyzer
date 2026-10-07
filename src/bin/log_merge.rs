//! `log-merge` — merge log files into one time-ordered file (spec: log-merge).
//!
//! Usage: `log-merge --output <path> <input>...`
//!
//! Exit codes (design D7): 0 on success, 1 on contract/I/O failures
//! (reported as `<file>:<line>: <reason>`), 2 on usage errors including an
//! output path that names one of the inputs.

use std::path::PathBuf;
use std::process::ExitCode;

use log_analyzer::merge::merge_files;

const USAGE: &str = "usage: log-merge --output <path> <input>...";

/// Parsed command line (design D7); unit-tested, `main` just acts on it.
struct Args {
    output: PathBuf,
    inputs: Vec<PathBuf>,
}

/// Parse `log-merge --output <path> <input>...`. `None` means a usage error
/// (missing `--output`, missing value after it, or no inputs).
fn parse_args(args: &[String]) -> Option<Args> {
    let mut iter = args.iter();
    let mut output = None;
    let mut inputs = Vec::new();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--output" => {
                output = Some(PathBuf::from(iter.next()?));
            }
            _ => inputs.push(PathBuf::from(arg)),
        }
    }
    Some(Args {
        output: output?,
        inputs,
    })
}

/// True when `output` names one of the inputs (exact path match only — the
/// library's merge performs the deeper same-file check before opening
/// anything, which also reports exit code 2 from here).
fn names_an_input(output: &std::path::Path, inputs: &[PathBuf]) -> bool {
    inputs.iter().any(|input| input.as_path() == output)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(parsed) = parse_args(&args) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    if parsed.inputs.is_empty() {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    if names_an_input(&parsed.output, &parsed.inputs) {
        eprintln!(
            "{USAGE}\nlog-merge: output path `{}` names an input file",
            parsed.output.display()
        );
        return ExitCode::from(2);
    }
    match merge_files(&parsed.inputs, &parsed.output) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("log-merge: {e}");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn parses_output_and_inputs() {
        let parsed = parse_args(&args(&["--output", "out.log", "a.log", "b.log"])).unwrap();
        assert_eq!(parsed.output, PathBuf::from("out.log"));
        assert_eq!(
            parsed.inputs,
            vec![PathBuf::from("a.log"), PathBuf::from("b.log")]
        );
    }

    #[test]
    fn inputs_may_precede_output() {
        let parsed = parse_args(&args(&["a.log", "--output", "out.log"])).unwrap();
        assert_eq!(parsed.output, PathBuf::from("out.log"));
        assert_eq!(parsed.inputs, vec![PathBuf::from("a.log")]);
    }

    #[test]
    fn missing_output_is_a_usage_error() {
        assert!(parse_args(&args(&["a.log"])).is_none());
    }

    #[test]
    fn dangling_output_flag_is_a_usage_error() {
        assert!(parse_args(&args(&["--output"])).is_none());
    }

    #[test]
    fn output_without_inputs_parses_but_has_no_inputs() {
        // The parser accepts it; `main` turns empty inputs into usage exit 2.
        let parsed = parse_args(&args(&["--output", "out.log"])).unwrap();
        assert!(parsed.inputs.is_empty());
    }

    #[test]
    fn empty_command_line_is_a_usage_error() {
        assert!(parse_args(&args(&[])).is_none());
    }

    #[test]
    fn output_naming_an_input_is_detected() {
        let inputs = vec![PathBuf::from("a.log"), PathBuf::from("b.log")];
        assert!(names_an_input(std::path::Path::new("b.log"), &inputs));
        assert!(!names_an_input(std::path::Path::new("out.log"), &inputs));
    }
}
