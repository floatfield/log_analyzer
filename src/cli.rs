//! Shared `--output <path> <input>...` command-line parsing (design D7 of
//! log-sort): the utilities accept the same shape, print their own usage
//! line, and map usage errors to exit code 2.

use std::path::{Path, PathBuf};

/// Parsed command line of the form `--output <path> <input>...`.
#[derive(Debug, PartialEq, Eq)]
pub struct Args {
    /// Destination path (`--output <path>`).
    pub output: PathBuf,
    /// Input files in command-line order (at least one).
    pub inputs: Vec<PathBuf>,
}

/// Why a command line was rejected. Carries the caller's usage line so the
/// rejection text is complete.
#[derive(Debug, PartialEq, Eq)]
pub enum UsageError {
    /// Not `--output <path> <input>...` with at least one input: a missing
    /// `--output`, a dangling `--output` flag, or no input files.
    Usage { usage: String },
    /// The output path is spelled exactly like one of the input paths.
    OutputNamesInput { output: PathBuf, usage: String },
}

impl UsageError {
    /// The full stderr text for this error: the program's usage line, plus a
    /// specific reason line for the output-collision case.
    pub fn message(&self, program: &str) -> String {
        match self {
            UsageError::Usage { usage } => usage.clone(),
            UsageError::OutputNamesInput { output, usage } => format!(
                "{usage}\n{program}: output path `{}` names an input file",
                output.display()
            ),
        }
    }
}

/// True when `output` is spelled exactly like one of `inputs` (deeper
/// same-file detection stays in the library and is reported through
/// [`UsageError::OutputNamesInput`] by the caller).
pub fn names_an_input(output: &Path, inputs: &[PathBuf]) -> bool {
    inputs.iter().any(|input| input.as_path() == output)
}

/// Parse `--output <path> <input>...` (flag and inputs in any order, at least
/// one input; the output must not name an input). `usage` is carried in the
/// rejection text of [`UsageError::message`].
pub fn parse_output_inputs(args: &[String], usage: &str) -> Result<Args, UsageError> {
    let mut iter = args.iter();
    let mut output = None;
    let mut inputs = Vec::new();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--output" => {
                let value = iter.next().ok_or_else(|| usage_error(usage))?;
                output = Some(PathBuf::from(value));
            }
            _ => inputs.push(PathBuf::from(arg)),
        }
    }
    let Some(output) = output else {
        return Err(usage_error(usage));
    };
    if inputs.is_empty() {
        return Err(usage_error(usage));
    }
    if names_an_input(&output, &inputs) {
        return Err(UsageError::OutputNamesInput {
            output,
            usage: usage.to_owned(),
        });
    }
    Ok(Args { output, inputs })
}

fn usage_error(usage: &str) -> UsageError {
    UsageError::Usage {
        usage: usage.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const USAGE: &str = "usage: log-merge --output <path> <input>...";

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn parses_output_and_inputs() {
        let parsed =
            parse_output_inputs(&args(&["--output", "out.log", "a.log", "b.log"]), USAGE).unwrap();
        assert_eq!(parsed.output, PathBuf::from("out.log"));
        assert_eq!(
            parsed.inputs,
            vec![PathBuf::from("a.log"), PathBuf::from("b.log")]
        );
    }

    #[test]
    fn inputs_may_precede_output() {
        let parsed = parse_output_inputs(&args(&["a.log", "--output", "out.log"]), USAGE).unwrap();
        assert_eq!(parsed.output, PathBuf::from("out.log"));
        assert_eq!(parsed.inputs, vec![PathBuf::from("a.log")]);
    }

    #[test]
    fn missing_output_is_a_usage_error() {
        assert_eq!(
            parse_output_inputs(&args(&["a.log"]), USAGE),
            Err(usage_error(USAGE))
        );
    }

    #[test]
    fn dangling_output_flag_is_a_usage_error() {
        assert_eq!(
            parse_output_inputs(&args(&["--output"]), USAGE),
            Err(usage_error(USAGE))
        );
    }

    #[test]
    fn output_without_inputs_is_a_usage_error() {
        assert_eq!(
            parse_output_inputs(&args(&["--output", "out.log"]), USAGE),
            Err(usage_error(USAGE))
        );
    }

    #[test]
    fn empty_command_line_is_a_usage_error() {
        assert_eq!(
            parse_output_inputs(&args(&[]), USAGE),
            Err(usage_error(USAGE))
        );
    }

    #[test]
    fn output_naming_an_input_is_rejected() {
        assert_eq!(
            parse_output_inputs(&args(&["--output", "b.log", "a.log", "b.log"]), USAGE),
            Err(UsageError::OutputNamesInput {
                output: PathBuf::from("b.log"),
                usage: USAGE.to_owned(),
            })
        );
    }

    #[test]
    fn names_an_input_detects_exact_matches_only() {
        let inputs = vec![PathBuf::from("a.log"), PathBuf::from("b.log")];
        assert!(names_an_input(std::path::Path::new("b.log"), &inputs));
        assert!(!names_an_input(std::path::Path::new("out.log"), &inputs));
        assert!(!names_an_input(std::path::Path::new("a.log.bak"), &inputs));
    }

    #[test]
    fn usage_error_message_is_the_usage_line() {
        assert_eq!(usage_error(USAGE).message("log-merge"), USAGE);
    }

    #[test]
    fn collision_message_names_the_output() {
        let msg = UsageError::OutputNamesInput {
            output: PathBuf::from("b.log"),
            usage: USAGE.to_owned(),
        }
        .message("log-merge");
        assert_eq!(
            msg,
            format!("{USAGE}\nlog-merge: output path `b.log` names an input file")
        );
    }
}
