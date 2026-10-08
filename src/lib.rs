//! Library shared by the `log_analyzer` GUI, the `log-merge` and `log-sort`
//! utilities, and other log tooling: line indexing/classification
//! (`log_file`), log file merging (`merge`), external sorting (`sort`), and
//! shared CLI argument parsing (`cli`).

pub mod cli;
pub mod log_file;
pub mod merge;
pub mod sort;
