//! `log-sort` — sort log files into one `@timestamp`-ascending file (spec:
//! log-sort).
//!
//! Usage: `log-sort --output <path> <input>...`
//!
//! Exit codes: 0 on success, 1 on contract/I/O failures (reported as
//! `<file>:<line>: <reason>`), 2 on usage errors including an output path
//! that names one of the inputs.

use std::process::ExitCode;

use log_analyzer::cli::parse_output_inputs;
use log_analyzer::sort::sort_files;

const PROGRAM: &str = "log-sort";
const USAGE: &str = "usage: log-sort --output <path> <input>...";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let parsed = match parse_output_inputs(&args, USAGE) {
        Ok(parsed) => parsed,
        Err(e) => {
            eprintln!("{}", e.message(PROGRAM));
            return ExitCode::from(2);
        }
    };
    match sort_files(&parsed.inputs, &parsed.output) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{PROGRAM}: {e}");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use log_analyzer::cli::UsageError;
    use std::path::PathBuf;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn shared_parser_accepts_the_log_sort_shape() {
        let parsed = parse_output_inputs(&args(&["--output", "out.log", "a.log"]), USAGE).unwrap();
        assert_eq!(parsed.output, PathBuf::from("out.log"));
        assert_eq!(parsed.inputs, vec![PathBuf::from("a.log")]);
    }

    #[test]
    fn usage_error_renders_the_log_sort_usage_line() {
        let e = UsageError::Usage {
            usage: USAGE.to_owned(),
        };
        assert_eq!(e.message(PROGRAM), USAGE);
    }

    #[test]
    fn collision_error_names_the_log_sort_program() {
        let msg = UsageError::OutputNamesInput {
            output: PathBuf::from("a.log"),
            usage: USAGE.to_owned(),
        }
        .message(PROGRAM);
        assert_eq!(
            msg,
            format!("{USAGE}\nlog-sort: output path `a.log` names an input file")
        );
    }
}
