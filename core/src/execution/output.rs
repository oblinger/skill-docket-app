//! Output capture and buffering — ring buffers, pattern matching, aggregation.
//!
//! Provides `OutputBuffer` for per-execution output capture with configurable
//! max capacity (ring buffer eviction), `PatternMatcher` for scanning output
//! lines against configurable patterns, and `OutputAggregator` for tracking
//! multiple output buffers across executions.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// OutputStream
// ---------------------------------------------------------------------------

/// Which output stream a line came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputStream {
    Stdout,
    Stderr,
}

// ---------------------------------------------------------------------------
// OutputLine
// ---------------------------------------------------------------------------

/// A single line of captured output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputLine {
    pub text: String,
    pub timestamp_ms: u64,
    pub stream: OutputStream,
    pub line_number: usize,
}

// ---------------------------------------------------------------------------
// OutputBuffer
// ---------------------------------------------------------------------------

/// A ring buffer of output lines with configurable max capacity.
///
/// When `max_lines` is reached, the oldest lines are evicted to make room
/// for new ones.
#[derive(Debug)]
pub struct OutputBuffer {
    lines: Vec<OutputLine>,
    max_lines: usize,
    total_pushed: usize,
    total_bytes: usize,
}

impl OutputBuffer {
    /// Create a new buffer with the given maximum line capacity.
    pub fn new(max_lines: usize) -> Self {
        OutputBuffer {
            lines: Vec::new(),
            max_lines,
            total_pushed: 0,
            total_bytes: 0,
        }
    }

    /// Push a new line into the buffer. If at capacity, the oldest line is evicted.
    pub fn push_line(&mut self, text: &str, stream: OutputStream, timestamp_ms: u64) {
        self.total_pushed += 1;
        self.total_bytes += text.len();

        let line = OutputLine {
            text: text.to_string(),
            timestamp_ms,
            stream,
            line_number: self.total_pushed,
        };

        if self.lines.len() >= self.max_lines {
            // Evict oldest (front).
            if !self.lines.is_empty() {
                let removed = self.lines.remove(0);
                self.total_bytes = self.total_bytes.saturating_sub(removed.text.len());
            }
        }

        self.lines.push(line);
    }

    /// Return all lines currently in the buffer.
    pub fn lines(&self) -> &[OutputLine] {
        &self.lines
    }

    /// Return the last N lines (or fewer if the buffer has less).
    pub fn last_n(&self, n: usize) -> &[OutputLine] {
        if n >= self.lines.len() {
            &self.lines
        } else {
            &self.lines[self.lines.len() - n..]
        }
    }

    /// Search for lines containing the given pattern (simple substring match).
    pub fn search(&self, pattern: &str) -> Vec<&OutputLine> {
        self.lines
            .iter()
            .filter(|line| line.text.contains(pattern))
            .collect()
    }

    /// Clear all lines from the buffer.
    pub fn clear(&mut self) {
        self.lines.clear();
        self.total_bytes = 0;
    }

    /// Number of lines currently in the buffer.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Total bytes of text currently in the buffer.
    pub fn byte_count(&self) -> usize {
        self.total_bytes
    }

    /// Total number of lines ever pushed (including evicted ones).
    pub fn total_lines_pushed(&self) -> usize {
        self.total_pushed
    }
}

// ---------------------------------------------------------------------------
// PatternAction
// ---------------------------------------------------------------------------

/// What to do when a pattern matches an output line.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum PatternAction {
    Capture,
    Alert,
    Ignore,
    Transform { replacement: String },
}

// ---------------------------------------------------------------------------
// OutputPattern
// ---------------------------------------------------------------------------

/// A pattern to match against output lines, with an associated action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputPattern {
    pub pattern: String,
    pub action: PatternAction,
}

// ---------------------------------------------------------------------------
// PatternMatch
// ---------------------------------------------------------------------------

/// A match result: which line matched and what action to take.
#[derive(Debug, Clone)]
pub struct PatternMatch<'a> {
    pub line: &'a OutputLine,
    pub pattern: &'a OutputPattern,
}

// ---------------------------------------------------------------------------
// PatternMatcher
// ---------------------------------------------------------------------------

/// Scans output lines against a set of patterns and returns matches.
#[derive(Debug)]
pub struct PatternMatcher {
    patterns: Vec<OutputPattern>,
}

impl PatternMatcher {
    /// Create a new matcher with the given patterns.
    pub fn new(patterns: Vec<OutputPattern>) -> Self {
        PatternMatcher { patterns }
    }

    /// Add a pattern to the matcher.
    pub fn add_pattern(&mut self, pattern: OutputPattern) {
        self.patterns.push(pattern);
    }

    /// Scan a single line against all patterns. Returns all matching patterns.
    pub fn scan_line<'a>(&'a self, line: &'a OutputLine) -> Vec<PatternMatch<'a>> {
        self.patterns
            .iter()
            .filter(|p| line.text.contains(&p.pattern))
            .map(|p| PatternMatch { line, pattern: p })
            .collect()
    }

    /// Scan all lines in a buffer, returning all matches.
    pub fn scan_buffer<'a>(&'a self, buffer: &'a OutputBuffer) -> Vec<PatternMatch<'a>> {
        let mut matches = Vec::new();
        for line in buffer.lines() {
            matches.extend(self.scan_line(line));
        }
        matches
    }

    /// Return the number of registered patterns.
    pub fn pattern_count(&self) -> usize {
        self.patterns.len()
    }
}

// ---------------------------------------------------------------------------
// OutputAggregator
// ---------------------------------------------------------------------------

/// Tracks multiple output buffers, keyed by execution ID.
#[derive(Debug)]
pub struct OutputAggregator {
    buffers: HashMap<String, OutputBuffer>,
    default_max_lines: usize,
}

impl OutputAggregator {
    /// Create a new aggregator with the given default buffer capacity.
    pub fn new(default_max_lines: usize) -> Self {
        OutputAggregator {
            buffers: HashMap::new(),
            default_max_lines,
        }
    }

    /// Get or create the buffer for the given execution ID.
    pub fn buffer_for(&mut self, execution_id: &str) -> &mut OutputBuffer {
        let max = self.default_max_lines;
        self.buffers
            .entry(execution_id.to_string())
            .or_insert_with(|| OutputBuffer::new(max))
    }

    /// Push a line to the buffer for the given execution ID.
    pub fn push_line(
        &mut self,
        execution_id: &str,
        text: &str,
        stream: OutputStream,
        timestamp_ms: u64,
    ) {
        self.buffer_for(execution_id)
            .push_line(text, stream, timestamp_ms);
    }

    /// Get an immutable reference to a buffer, if it exists.
    pub fn get_buffer(&self, execution_id: &str) -> Option<&OutputBuffer> {
        self.buffers.get(execution_id)
    }

    /// Remove the buffer for the given execution ID.
    pub fn remove_buffer(&mut self, execution_id: &str) -> bool {
        self.buffers.remove(execution_id).is_some()
    }

    /// Number of tracked buffers.
    pub fn buffer_count(&self) -> usize {
        self.buffers.len()
    }

    /// Total line count across all buffers.
    pub fn total_line_count(&self) -> usize {
        self.buffers.values().map(|b| b.line_count()).sum()
    }

    /// Total byte count across all buffers.
    pub fn total_byte_count(&self) -> usize {
        self.buffers.values().map(|b| b.byte_count()).sum()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- OutputBuffer tests --

    #[test]
    fn buffer_push_and_read() {
        let mut buf = OutputBuffer::new(100);
        buf.push_line("hello", OutputStream::Stdout, 1000);
        buf.push_line("world", OutputStream::Stderr, 2000);

        assert_eq!(buf.line_count(), 2);
        assert_eq!(buf.lines()[0].text, "hello");
        assert_eq!(buf.lines()[1].text, "world");
    }

    #[test]
    fn buffer_line_numbers_sequential() {
        let mut buf = OutputBuffer::new(100);
        buf.push_line("a", OutputStream::Stdout, 100);
        buf.push_line("b", OutputStream::Stdout, 200);
        buf.push_line("c", OutputStream::Stdout, 300);

        assert_eq!(buf.lines()[0].line_number, 1);
        assert_eq!(buf.lines()[1].line_number, 2);
        assert_eq!(buf.lines()[2].line_number, 3);
    }

    #[test]
    fn buffer_stream_tracking() {
        let mut buf = OutputBuffer::new(100);
        buf.push_line("out", OutputStream::Stdout, 100);
        buf.push_line("err", OutputStream::Stderr, 200);

        assert_eq!(buf.lines()[0].stream, OutputStream::Stdout);
        assert_eq!(buf.lines()[1].stream, OutputStream::Stderr);
    }

    #[test]
    fn buffer_ring_eviction() {
        let mut buf = OutputBuffer::new(3);
        buf.push_line("a", OutputStream::Stdout, 100);
        buf.push_line("b", OutputStream::Stdout, 200);
        buf.push_line("c", OutputStream::Stdout, 300);
        buf.push_line("d", OutputStream::Stdout, 400);

        assert_eq!(buf.line_count(), 3);
        assert_eq!(buf.lines()[0].text, "b");
        assert_eq!(buf.lines()[1].text, "c");
        assert_eq!(buf.lines()[2].text, "d");
    }

    #[test]
    fn buffer_ring_eviction_multiple() {
        let mut buf = OutputBuffer::new(2);
        for i in 0..10 {
            buf.push_line(&format!("line{}", i), OutputStream::Stdout, i as u64 * 100);
        }

        assert_eq!(buf.line_count(), 2);
        assert_eq!(buf.lines()[0].text, "line8");
        assert_eq!(buf.lines()[1].text, "line9");
        assert_eq!(buf.total_lines_pushed(), 10);
    }

    #[test]
    fn buffer_last_n() {
        let mut buf = OutputBuffer::new(100);
        for i in 0..5 {
            buf.push_line(&format!("line{}", i), OutputStream::Stdout, i as u64 * 100);
        }

        let last2 = buf.last_n(2);
        assert_eq!(last2.len(), 2);
        assert_eq!(last2[0].text, "line3");
        assert_eq!(last2[1].text, "line4");
    }

    #[test]
    fn buffer_last_n_more_than_available() {
        let mut buf = OutputBuffer::new(100);
        buf.push_line("only", OutputStream::Stdout, 100);

        let last5 = buf.last_n(5);
        assert_eq!(last5.len(), 1);
        assert_eq!(last5[0].text, "only");
    }

    #[test]
    fn buffer_search() {
        let mut buf = OutputBuffer::new(100);
        buf.push_line("test passed", OutputStream::Stdout, 100);
        buf.push_line("warning: unused var", OutputStream::Stderr, 200);
        buf.push_line("test failed", OutputStream::Stdout, 300);
        buf.push_line("all done", OutputStream::Stdout, 400);

        let results = buf.search("test");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].text, "test passed");
        assert_eq!(results[1].text, "test failed");
    }

    #[test]
    fn buffer_search_no_match() {
        let mut buf = OutputBuffer::new(100);
        buf.push_line("hello world", OutputStream::Stdout, 100);

        let results = buf.search("xyz");
        assert!(results.is_empty());
    }

    #[test]
    fn buffer_clear() {
        let mut buf = OutputBuffer::new(100);
        buf.push_line("hello", OutputStream::Stdout, 100);
        buf.push_line("world", OutputStream::Stdout, 200);
        buf.clear();

        assert_eq!(buf.line_count(), 0);
        assert_eq!(buf.byte_count(), 0);
    }

    #[test]
    fn buffer_byte_count() {
        let mut buf = OutputBuffer::new(100);
        buf.push_line("hello", OutputStream::Stdout, 100); // 5 bytes
        buf.push_line("world!", OutputStream::Stdout, 200); // 6 bytes

        assert_eq!(buf.byte_count(), 11);
    }

    #[test]
    fn buffer_byte_count_after_eviction() {
        let mut buf = OutputBuffer::new(2);
        buf.push_line("aaa", OutputStream::Stdout, 100); // 3
        buf.push_line("bb", OutputStream::Stdout, 200); // 2
        buf.push_line("c", OutputStream::Stdout, 300); // 1 (evicts "aaa")

        assert_eq!(buf.byte_count(), 3); // "bb" + "c"
    }

    #[test]
    fn buffer_empty() {
        let buf = OutputBuffer::new(100);
        assert_eq!(buf.line_count(), 0);
        assert_eq!(buf.byte_count(), 0);
        assert!(buf.lines().is_empty());
        assert!(buf.last_n(5).is_empty());
    }

    #[test]
    fn buffer_single_capacity() {
        let mut buf = OutputBuffer::new(1);
        buf.push_line("first", OutputStream::Stdout, 100);
        buf.push_line("second", OutputStream::Stdout, 200);

        assert_eq!(buf.line_count(), 1);
        assert_eq!(buf.lines()[0].text, "second");
    }

    // -- PatternMatcher tests --

    #[test]
    fn pattern_match_capture() {
        let matcher = PatternMatcher::new(vec![OutputPattern {
            pattern: "error".into(),
            action: PatternAction::Capture,
        }]);

        let line = OutputLine {
            text: "fatal error occurred".into(),
            timestamp_ms: 100,
            stream: OutputStream::Stderr,
            line_number: 1,
        };

        let matches = matcher.scan_line(&line);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].pattern.action, PatternAction::Capture);
    }

    #[test]
    fn pattern_no_match() {
        let matcher = PatternMatcher::new(vec![OutputPattern {
            pattern: "error".into(),
            action: PatternAction::Alert,
        }]);

        let line = OutputLine {
            text: "all good".into(),
            timestamp_ms: 100,
            stream: OutputStream::Stdout,
            line_number: 1,
        };

        let matches = matcher.scan_line(&line);
        assert!(matches.is_empty());
    }

    #[test]
    fn pattern_multiple_matches() {
        let matcher = PatternMatcher::new(vec![
            OutputPattern {
                pattern: "test".into(),
                action: PatternAction::Capture,
            },
            OutputPattern {
                pattern: "fail".into(),
                action: PatternAction::Alert,
            },
        ]);

        let line = OutputLine {
            text: "test failed".into(),
            timestamp_ms: 100,
            stream: OutputStream::Stdout,
            line_number: 1,
        };

        let matches = matcher.scan_line(&line);
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn pattern_scan_buffer() {
        let matcher = PatternMatcher::new(vec![OutputPattern {
            pattern: "ERROR".into(),
            action: PatternAction::Alert,
        }]);

        let mut buf = OutputBuffer::new(100);
        buf.push_line("INFO: starting", OutputStream::Stdout, 100);
        buf.push_line("ERROR: disk full", OutputStream::Stderr, 200);
        buf.push_line("INFO: retrying", OutputStream::Stdout, 300);
        buf.push_line("ERROR: still full", OutputStream::Stderr, 400);

        let matches = matcher.scan_buffer(&buf);
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn pattern_add_pattern() {
        let mut matcher = PatternMatcher::new(vec![]);
        assert_eq!(matcher.pattern_count(), 0);

        matcher.add_pattern(OutputPattern {
            pattern: "warn".into(),
            action: PatternAction::Capture,
        });
        assert_eq!(matcher.pattern_count(), 1);
    }

    #[test]
    fn pattern_transform_action_serde() {
        let action = PatternAction::Transform {
            replacement: "[REDACTED]".into(),
        };
        let json = serde_json::to_string(&action).unwrap();
        let back: PatternAction = serde_json::from_str(&json).unwrap();
        assert_eq!(back, action);
    }

    #[test]
    fn output_pattern_serde() {
        let p = OutputPattern {
            pattern: "error".into(),
            action: PatternAction::Alert,
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: OutputPattern = serde_json::from_str(&json).unwrap();
        assert_eq!(back.pattern, "error");
    }

    #[test]
    fn output_line_serde() {
        let line = OutputLine {
            text: "hello".into(),
            timestamp_ms: 1000,
            stream: OutputStream::Stdout,
            line_number: 42,
        };
        let json = serde_json::to_string(&line).unwrap();
        let back: OutputLine = serde_json::from_str(&json).unwrap();
        assert_eq!(back.text, "hello");
        assert_eq!(back.line_number, 42);
        assert_eq!(back.stream, OutputStream::Stdout);
    }

    // -- OutputAggregator tests --

    #[test]
    fn aggregator_push_and_get() {
        let mut agg = OutputAggregator::new(100);
        agg.push_line("e1", "hello", OutputStream::Stdout, 100);
        agg.push_line("e1", "world", OutputStream::Stdout, 200);
        agg.push_line("e2", "other", OutputStream::Stderr, 300);

        assert_eq!(agg.buffer_count(), 2);
        assert_eq!(agg.get_buffer("e1").unwrap().line_count(), 2);
        assert_eq!(agg.get_buffer("e2").unwrap().line_count(), 1);
    }

    #[test]
    fn aggregator_remove_buffer() {
        let mut agg = OutputAggregator::new(100);
        agg.push_line("e1", "hello", OutputStream::Stdout, 100);
        assert!(agg.remove_buffer("e1"));
        assert!(!agg.remove_buffer("e1"));
        assert_eq!(agg.buffer_count(), 0);
    }

    #[test]
    fn aggregator_get_nonexistent() {
        let agg = OutputAggregator::new(100);
        assert!(agg.get_buffer("nope").is_none());
    }

    #[test]
    fn aggregator_total_lines() {
        let mut agg = OutputAggregator::new(100);
        agg.push_line("e1", "a", OutputStream::Stdout, 100);
        agg.push_line("e1", "b", OutputStream::Stdout, 200);
        agg.push_line("e2", "c", OutputStream::Stdout, 300);

        assert_eq!(agg.total_line_count(), 3);
    }

    #[test]
    fn aggregator_total_bytes() {
        let mut agg = OutputAggregator::new(100);
        agg.push_line("e1", "hello", OutputStream::Stdout, 100); // 5
        agg.push_line("e2", "ab", OutputStream::Stdout, 200); // 2

        assert_eq!(agg.total_byte_count(), 7);
    }

    #[test]
    fn aggregator_buffer_for_creates() {
        let mut agg = OutputAggregator::new(50);
        let buf = agg.buffer_for("new-exec");
        buf.push_line("test", OutputStream::Stdout, 100);
        assert_eq!(agg.get_buffer("new-exec").unwrap().line_count(), 1);
    }

    #[test]
    fn aggregator_empty() {
        let agg = OutputAggregator::new(100);
        assert_eq!(agg.buffer_count(), 0);
        assert_eq!(agg.total_line_count(), 0);
        assert_eq!(agg.total_byte_count(), 0);
    }

    #[test]
    fn buffer_timestamp_preserved() {
        let mut buf = OutputBuffer::new(100);
        buf.push_line("msg", OutputStream::Stdout, 42000);
        assert_eq!(buf.lines()[0].timestamp_ms, 42000);
    }

    #[test]
    fn pattern_ignore_action() {
        let matcher = PatternMatcher::new(vec![OutputPattern {
            pattern: "debug".into(),
            action: PatternAction::Ignore,
        }]);

        let line = OutputLine {
            text: "debug: verbose info".into(),
            timestamp_ms: 100,
            stream: OutputStream::Stdout,
            line_number: 1,
        };

        let matches = matcher.scan_line(&line);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].pattern.action, PatternAction::Ignore);
    }
}
