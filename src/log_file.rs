//! Lazy log file access: line-offset indexing, on-demand line retrieval,
//! JSON/non-JSON line classification, and field-name discovery.
//!
//! The design keeps this module free of UI concerns: everything here works on
//! plain byte slices and plain data structures so it is directly unit-testable.

// Used only by `#[cfg(test)]` helpers and the tests module below.
#[cfg(test)]
use std::collections::BTreeSet;
#[cfg(test)]
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use serde_json::Value;

/// Byte-offset index of line starts. `offsets` has `line_count() + 1` entries:
/// entry `i` is the byte offset where line `i` starts, and the final entry is
/// a sentinel equal to the total byte length, so line `i` spans
/// `offsets[i]..offsets[i + 1]`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct IndexData {
    pub offsets: Vec<u64>,
    pub done: bool,
}

impl IndexData {
    /// Number of lines currently covered by the index.
    pub fn line_count(&self) -> usize {
        self.offsets.len().saturating_sub(1)
    }

    /// Build a complete index for `bytes` in one pass. Convenience for tests
    /// and small tools: production builds the index incrementally via
    /// [`IncrementalIndexer`].
    pub fn build(bytes: &[u8]) -> Self {
        let mut data = IndexData::default();
        let mut indexer = IncrementalIndexer::new();
        while !indexer.scan_chunk(bytes, &mut data, CHUNK_BYTES) {}
        data
    }

    /// Raw bytes of line `line`, with the trailing `\n` (and `\r` of a `\r\n`
    /// pair) excluded. Returns `None` when the line number is out of range.
    pub fn line_slice<'a>(&self, bytes: &'a [u8], line: usize) -> Option<&'a [u8]> {
        if line >= self.line_count() {
            return None;
        }
        let start = self.offsets[line] as usize;
        let next = self.offsets[line + 1] as usize;
        let mut end = next;
        if end > start && bytes[end - 1] == b'\n' {
            end -= 1;
            if end > start && bytes[end - 1] == b'\r' {
                end -= 1;
            }
        }
        Some(&bytes[start..end])
    }
}

/// Chunk size used when scanning for line offsets in [`IndexData::build`];
/// production scanning chunks from `app.rs` (`INDEX_CHUNK_BYTES`).
const CHUNK_BYTES: usize = 1 << 20;

/// Incremental line-offset scanner. Feed it the same byte slice repeatedly
/// with `scan_chunk`; it makes forward progress and pushes line-start offsets
/// into the shared [`IndexData`] so a UI can display rows while indexing of a
/// large file is still running.
#[derive(Debug, Default)]
pub struct IncrementalIndexer {
    pos: usize,
    finished: bool,
}

impl IncrementalIndexer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Scan up to `budget` bytes for line starts, pushing offsets into `data`.
    /// Returns `true` when the whole input has been consumed (and `data.done`
    /// has been set), `false` if more chunks remain.
    pub fn scan_chunk(&mut self, bytes: &[u8], data: &mut IndexData, budget: usize) -> bool {
        if self.finished {
            return true;
        }
        let len = bytes.len();
        if data.offsets.is_empty() {
            data.offsets.push(0);
        }
        let budget_end = self.pos.saturating_add(budget).min(len);
        while self.pos < budget_end {
            let window = &bytes[self.pos..budget_end];
            match window.iter().position(|&b| b == b'\n') {
                Some(rel) => {
                    let newline = self.pos + rel;
                    data.offsets.push((newline + 1) as u64);
                    self.pos = newline + 1;
                }
                None => {
                    self.pos = budget_end;
                }
            }
        }
        if self.pos >= len {
            self.finished = true;
            if data.offsets.last().copied() != Some(len as u64) {
                data.offsets.push(len as u64);
            }
            data.done = true;
            return true;
        }
        false
    }
}

/// Parse a line as a top-level JSON object. Returns `None` for anything else -
/// non-object JSON (`123`, `[1,2]`) and non-JSON lines are raw lines.
pub fn parse_object(bytes: &[u8]) -> Option<serde_json::Map<String, Value>> {
    match serde_json::from_slice::<Value>(bytes) {
        Ok(Value::Object(map)) => Some(map),
        _ => None,
    }
}

/// True when the line parses as a top-level JSON object. Test-only
/// convenience: production classifies via [`parse_object`].
#[cfg(test)]
pub fn is_structured(bytes: &[u8]) -> bool {
    parse_object(bytes).is_some()
}

/// Collect the field names of all structured entries into `out`. `progress`
/// receives the number of lines processed so far; the scan stops early when
/// `cancel` is set. Test-only: production collects fields piggybacked on
/// query scans and open jobs (design D6).
#[cfg(test)]
pub fn scan_fields(
    bytes: &[u8],
    data: &IndexData,
    out: &mut BTreeSet<String>,
    cancel: &AtomicBool,
    progress: &AtomicUsize,
) {
    for line in 0..data.line_count() {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        if let Some(map) = data.line_slice(bytes, line).and_then(parse_object) {
            out.extend(map.keys().cloned());
        }
        progress.store(line + 1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ops::Range;

    fn lines(bytes: &[u8]) -> Vec<String> {
        let data = IndexData::build(bytes);
        (0..data.line_count())
            .map(|i| String::from_utf8_lossy(data.line_slice(bytes, i).unwrap()).into_owned())
            .collect()
    }

    #[test]
    fn index_counts_lines_with_trailing_newline() {
        let data = IndexData::build(b"a\nbb\nccc\n");
        assert_eq!(data.line_count(), 3);
        assert_eq!(lines(b"a\nbb\nccc\n"), vec!["a", "bb", "ccc"]);
    }

    #[test]
    fn index_handles_missing_trailing_newline() {
        let data = IndexData::build(b"a\nbb\nccc");
        assert_eq!(data.line_count(), 3);
        assert_eq!(lines(b"a\nbb\nccc"), vec!["a", "bb", "ccc"]);
    }

    #[test]
    fn index_handles_empty_file() {
        let data = IndexData::build(b"");
        assert_eq!(data.line_count(), 0);
        assert!(data.done);
        assert!(lines(b"").is_empty());
    }

    #[test]
    fn index_handles_crlf_and_blank_lines() {
        assert_eq!(lines(b"\r\nline2\r\n"), vec!["", "line2"]);
        assert_eq!(lines(b"x"), vec!["x"]);
    }

    #[test]
    fn incremental_chunked_scan_matches_sync_build() {
        let bytes = b"first\nsecond\nthird\nlast without newline";
        for chunk in [1usize, 3, 7, 1024] {
            let mut data = IndexData::default();
            let mut indexer = IncrementalIndexer::new();
            let mut calls = 0;
            while !indexer.scan_chunk(bytes, &mut data, chunk) {
                calls += 1;
                assert!(calls < 10_000, "scan_chunk made no progress");
            }
            assert_eq!(data, IndexData::build(bytes), "chunk size {chunk}");
            assert!(data.done);
        }
    }

    #[test]
    fn line_slice_first_middle_last() {
        let bytes = b"one\ntwo\nthree";
        let data = IndexData::build(bytes);
        assert_eq!(data.line_slice(bytes, 0), Some(&b"one"[..]));
        assert_eq!(data.line_slice(bytes, 1), Some(&b"two"[..]));
        assert_eq!(data.line_slice(bytes, 2), Some(&b"three"[..]));
        assert_eq!(data.line_slice(bytes, 3), None);
    }

    #[test]
    fn line_slice_strips_crlf() {
        let bytes = b"a\r\nbb\r\n";
        let data = IndexData::build(bytes);
        assert_eq!(data.line_slice(bytes, 0), Some(&b"a"[..]));
        assert_eq!(data.line_slice(bytes, 1), Some(&b"bb"[..]));
    }

    #[test]
    fn classify_json_object_is_structured() {
        let parsed = parse_object(br#"{"level": "INFO", "message": "hi"}"#).unwrap();
        assert_eq!(parsed.get("level").and_then(Value::as_str), Some("INFO"));
        assert!(is_structured(br#"  {"a": 1}  "#));
    }

    #[test]
    fn classify_non_object_json_is_raw() {
        assert!(!is_structured(b"123"));
        assert!(!is_structured(b"[1,2]"));
        assert!(!is_structured(b"\"just a string\""));
    }

    #[test]
    fn classify_non_json_is_raw() {
        assert!(!is_structured(b"PANIC: unexpected state"));
        assert!(!is_structured(
            b"\tat com.example.Router.dispatch(Router.java:88)"
        ));
        assert!(!is_structured(b"WARNING: disk space low on /var (7% free)"));
        assert!(!is_structured(b""));
    }

    #[test]
    fn scan_fields_collects_union_of_keys() {
        let bytes = b"{\"a\": 1, \"b\": 2}\nnoise\n{\"b\": 3, \"c\": \"x\"}\n[1]\n";
        let data = IndexData::build(bytes);
        let mut fields = BTreeSet::new();
        scan_fields(
            bytes,
            &data,
            &mut fields,
            &AtomicBool::new(false),
            &AtomicUsize::new(0),
        );
        let expected: BTreeSet<String> = ["a", "b", "c"].into_iter().map(str::to_owned).collect();
        assert_eq!(fields, expected);
    }

    #[test]
    fn scan_fields_finds_fixture_fields() {
        let bytes = std::fs::read("log_examples/first.log").expect("fixture file readable");
        let data = IndexData::build(&bytes);
        let mut fields = BTreeSet::new();
        scan_fields(
            &bytes,
            &data,
            &mut fields,
            &AtomicBool::new(false),
            &AtomicUsize::new(0),
        );
        for expected in [
            "@timestamp",
            "level",
            "message",
            "requestId",
            "service",
            "userId",
            "durationMs",
            "stackTrace",
        ] {
            assert!(fields.contains(expected), "missing field {expected}");
        }
    }

    #[test]
    fn scan_fields_respects_cancel() {
        let bytes = b"{\"a\": 1}\n{\"b\": 2}\n";
        let data = IndexData::build(bytes);
        let mut fields = BTreeSet::new();
        scan_fields(
            bytes,
            &data,
            &mut fields,
            &AtomicBool::new(true),
            &AtomicUsize::new(0),
        );
        assert!(fields.is_empty());
    }

    #[test]
    fn line_ranges_are_contiguous() {
        let bytes = b"a\nbb\nccc";
        let data = IndexData::build(bytes);
        let ranges: Vec<Range<usize>> = (0..data.line_count())
            .map(|i| {
                let start = data.offsets[i] as usize;
                let end = data.offsets[i + 1] as usize;
                start..end
            })
            .collect();
        assert_eq!(ranges[0].start, 0);
        for pair in ranges.windows(2) {
            assert_eq!(pair[0].end, pair[1].start);
        }
        assert_eq!(*data.offsets.last().unwrap(), bytes.len() as u64);
    }
}
