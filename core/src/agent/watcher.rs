use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Status derived from watching agent output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum WatchStatus {
    Active { activity: String },
    Waiting,
    Error { message: String },
    Completed { result: String },
    Unresponsive,
}

/// Result of analyzing an agent's output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchResult {
    pub agent: String,
    pub timestamp_ms: u64,
    pub status: WatchStatus,
    pub output_lines: Vec<String>,
    pub progress: Option<f64>,
}

/// A pattern that matches agent output lines and maps them to a status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputPattern {
    pub name: String,
    pub pattern: String,
    pub extract_status: PatternStatus,
}

/// What status to assign when a pattern matches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PatternStatus {
    Active { activity: String },
    Waiting,
    Error { message: String },
    Completed { result: String },
}

impl PatternStatus {
    /// Convert to a WatchStatus.
    pub fn to_watch_status(&self) -> WatchStatus {
        match self {
            PatternStatus::Active { activity } => WatchStatus::Active {
                activity: activity.clone(),
            },
            PatternStatus::Waiting => WatchStatus::Waiting,
            PatternStatus::Error { message } => WatchStatus::Error {
                message: message.clone(),
            },
            PatternStatus::Completed { result } => WatchStatus::Completed {
                result: result.clone(),
            },
        }
    }
}

/// Progress extraction patterns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressPattern {
    pub name: String,
    /// A substring to look for in output lines, followed by a number
    /// (e.g., "progress:" matches "progress: 75" -> 0.75).
    pub prefix: String,
    /// Whether the extracted number is a percentage (0-100) or fraction (0.0-1.0).
    pub is_percentage: bool,
}

/// Watches agent output, applies patterns, and extracts status.
pub struct AgentWatcher {
    patterns: Vec<OutputPattern>,
    progress_patterns: Vec<ProgressPattern>,
    last_watch: HashMap<String, WatchResult>,
    watch_interval_ms: u64,
}

impl AgentWatcher {
    /// Create a new watcher with the given check interval.
    pub fn new(interval_ms: u64) -> Self {
        Self {
            patterns: Vec::new(),
            progress_patterns: Vec::new(),
            last_watch: HashMap::new(),
            watch_interval_ms: interval_ms,
        }
    }

    /// Create a watcher pre-loaded with common patterns for Claude agents.
    pub fn with_defaults(interval_ms: u64) -> Self {
        let mut w = Self::new(interval_ms);

        w.add_pattern(OutputPattern {
            name: "error_pattern".into(),
            pattern: "Error:".into(),
            extract_status: PatternStatus::Error {
                message: "error detected in output".into(),
            },
        });
        w.add_pattern(OutputPattern {
            name: "panic_pattern".into(),
            pattern: "panic".into(),
            extract_status: PatternStatus::Error {
                message: "panic detected in output".into(),
            },
        });
        w.add_pattern(OutputPattern {
            name: "test_pass".into(),
            pattern: "test result: ok".into(),
            extract_status: PatternStatus::Completed {
                result: "tests passed".into(),
            },
        });
        w.add_pattern(OutputPattern {
            name: "compiling".into(),
            pattern: "Compiling".into(),
            extract_status: PatternStatus::Active {
                activity: "compiling".into(),
            },
        });
        w.add_pattern(OutputPattern {
            name: "running_tests".into(),
            pattern: "running".into(),
            extract_status: PatternStatus::Active {
                activity: "running tests".into(),
            },
        });
        w.add_pattern(OutputPattern {
            name: "waiting_prompt".into(),
            pattern: "$ ".into(),
            extract_status: PatternStatus::Waiting,
        });

        w.add_progress_pattern(ProgressPattern {
            name: "percentage".into(),
            prefix: "progress:".into(),
            is_percentage: true,
        });
        w.add_progress_pattern(ProgressPattern {
            name: "completion".into(),
            prefix: "completion:".into(),
            is_percentage: true,
        });

        w
    }

    /// Add an output pattern.
    pub fn add_pattern(&mut self, pattern: OutputPattern) {
        self.patterns.push(pattern);
    }

    /// Add a progress extraction pattern.
    pub fn add_progress_pattern(&mut self, pattern: ProgressPattern) {
        self.progress_patterns.push(pattern);
    }

    /// Remove a pattern by name. Returns true if found.
    pub fn remove_pattern(&mut self, name: &str) -> bool {
        let before = self.patterns.len();
        self.patterns.retain(|p| p.name != name);
        self.patterns.len() < before
    }

    /// List all pattern names.
    pub fn pattern_names(&self) -> Vec<&str> {
        self.patterns.iter().map(|p| p.name.as_str()).collect()
    }

    /// Analyze agent output against registered patterns.
    ///
    /// Returns a WatchResult with the detected status and progress.
    /// The last matched pattern wins (patterns are checked in order;
    /// later lines override earlier matches).
    pub fn analyze_output(&mut self, agent: &str, output: &str, now_ms: u64) -> WatchResult {
        let lines: Vec<String> = output.lines().map(|l| l.to_string()).collect();

        let mut matched_status: Option<WatchStatus> = None;

        // Check each line against patterns; last match wins
        for line in &lines {
            for pattern in &self.patterns {
                if line.contains(&pattern.pattern) {
                    matched_status = Some(pattern.extract_status.to_watch_status());
                }
            }
        }

        // If no pattern matched and there are lines, assume active
        // If no output at all, assume unresponsive
        let status = matched_status.unwrap_or_else(|| {
            if lines.is_empty() {
                WatchStatus::Unresponsive
            } else {
                WatchStatus::Active {
                    activity: "output detected".into(),
                }
            }
        });

        let progress = self.extract_progress(output);

        let result = WatchResult {
            agent: agent.to_string(),
            timestamp_ms: now_ms,
            status,
            output_lines: lines,
            progress,
        };

        self.last_watch.insert(agent.to_string(), result.clone());
        result
    }

    /// Get the last watch result for an agent.
    pub fn last_result(&self, agent: &str) -> Option<&WatchResult> {
        self.last_watch.get(agent)
    }

    /// List agents whose last watch is older than the watch interval.
    pub fn agents_needing_watch(&self, now_ms: u64) -> Vec<&str> {
        self.last_watch
            .iter()
            .filter(|(_, r)| now_ms.saturating_sub(r.timestamp_ms) >= self.watch_interval_ms)
            .map(|(name, _)| name.as_str())
            .collect()
    }

    /// Agents that have never been watched.
    /// (Callers may want to track which agents are registered separately.)
    pub fn agents_with_results(&self) -> Vec<&str> {
        self.last_watch.keys().map(|s| s.as_str()).collect()
    }

    /// Extract a progress value (0.0-1.0) from output text.
    pub fn extract_progress(&self, output: &str) -> Option<f64> {
        // Check each progress pattern; return the last match found
        let mut result: Option<f64> = None;

        for line in output.lines() {
            for pp in &self.progress_patterns {
                if let Some(pos) = line.to_lowercase().find(&pp.prefix.to_lowercase()) {
                    let after = &line[pos + pp.prefix.len()..];
                    if let Some(val) = parse_number_from_str(after) {
                        let normalized = if pp.is_percentage {
                            (val / 100.0).clamp(0.0, 1.0)
                        } else {
                            val.clamp(0.0, 1.0)
                        };
                        result = Some(normalized);
                    }
                }
            }
        }

        result
    }

    /// Clear the last watch result for an agent.
    pub fn clear_result(&mut self, agent: &str) -> bool {
        self.last_watch.remove(agent).is_some()
    }

    /// Clear all watch results.
    pub fn clear_all_results(&mut self) {
        self.last_watch.clear();
    }

    /// The configured watch interval.
    pub fn watch_interval_ms(&self) -> u64 {
        self.watch_interval_ms
    }

    /// Number of registered patterns.
    pub fn pattern_count(&self) -> usize {
        self.patterns.len()
    }
}

/// Extract the first floating-point or integer number from a string.
fn parse_number_from_str(s: &str) -> Option<f64> {
    let trimmed = s.trim();
    // Find the start of a number
    let start = trimmed.find(|c: char| c.is_ascii_digit() || c == '.')?;
    let rest = &trimmed[start..];
    // Collect digits and at most one decimal point
    let mut end = 0;
    let mut has_dot = false;
    for ch in rest.chars() {
        if ch.is_ascii_digit() {
            end += ch.len_utf8();
        } else if ch == '.' && !has_dot {
            has_dot = true;
            end += ch.len_utf8();
        } else {
            break;
        }
    }
    if end == 0 {
        return None;
    }
    rest[..end].parse::<f64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- PatternStatus ----

    #[test]
    fn pattern_status_to_watch_status() {
        let ps = PatternStatus::Active {
            activity: "building".into(),
        };
        let ws = ps.to_watch_status();
        assert_eq!(
            ws,
            WatchStatus::Active {
                activity: "building".into()
            }
        );

        let ps = PatternStatus::Waiting;
        assert_eq!(ps.to_watch_status(), WatchStatus::Waiting);

        let ps = PatternStatus::Error {
            message: "crash".into(),
        };
        assert_eq!(
            ps.to_watch_status(),
            WatchStatus::Error {
                message: "crash".into()
            }
        );

        let ps = PatternStatus::Completed {
            result: "done".into(),
        };
        assert_eq!(
            ps.to_watch_status(),
            WatchStatus::Completed {
                result: "done".into()
            }
        );
    }

    // ---- WatchStatus serde ----

    #[test]
    fn watch_status_active_serde() {
        let ws = WatchStatus::Active {
            activity: "compiling".into(),
        };
        let json = serde_json::to_string(&ws).unwrap();
        assert!(json.contains("\"status\":\"active\""));
        let back: WatchStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ws);
    }

    #[test]
    fn watch_status_waiting_serde() {
        let ws = WatchStatus::Waiting;
        let json = serde_json::to_string(&ws).unwrap();
        let back: WatchStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ws);
    }

    #[test]
    fn watch_status_error_serde() {
        let ws = WatchStatus::Error {
            message: "segfault".into(),
        };
        let json = serde_json::to_string(&ws).unwrap();
        let back: WatchStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ws);
    }

    #[test]
    fn watch_status_completed_serde() {
        let ws = WatchStatus::Completed {
            result: "all tests passed".into(),
        };
        let json = serde_json::to_string(&ws).unwrap();
        let back: WatchStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ws);
    }

    #[test]
    fn watch_status_unresponsive_serde() {
        let ws = WatchStatus::Unresponsive;
        let json = serde_json::to_string(&ws).unwrap();
        let back: WatchStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ws);
    }

    // ---- WatchResult serde ----

    #[test]
    fn watch_result_serde_round_trip() {
        let wr = WatchResult {
            agent: "w1".into(),
            timestamp_ms: 5000,
            status: WatchStatus::Active {
                activity: "testing".into(),
            },
            output_lines: vec!["line 1".into(), "line 2".into()],
            progress: Some(0.45),
        };
        let json = serde_json::to_string(&wr).unwrap();
        let back: WatchResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.agent, "w1");
        assert_eq!(back.timestamp_ms, 5000);
        assert_eq!(back.output_lines.len(), 2);
        assert_eq!(back.progress, Some(0.45));
    }

    // ---- parse_number_from_str ----

    #[test]
    fn parse_number_integer() {
        assert_eq!(parse_number_from_str("75"), Some(75.0));
    }

    #[test]
    fn parse_number_float() {
        assert_eq!(parse_number_from_str("0.85"), Some(0.85));
    }

    #[test]
    fn parse_number_with_prefix_whitespace() {
        assert_eq!(parse_number_from_str("  42 "), Some(42.0));
    }

    #[test]
    fn parse_number_with_suffix() {
        assert_eq!(parse_number_from_str(" 99%"), Some(99.0));
    }

    #[test]
    fn parse_number_no_number() {
        assert_eq!(parse_number_from_str("no numbers here"), None);
    }

    #[test]
    fn parse_number_empty() {
        assert_eq!(parse_number_from_str(""), None);
    }

    #[test]
    fn parse_number_just_dot() {
        // A lone dot should not parse as a number
        assert_eq!(parse_number_from_str("."), None);
    }

    #[test]
    fn parse_number_leading_text() {
        assert_eq!(parse_number_from_str("value=3.14 done"), Some(3.14));
    }

    // ---- AgentWatcher: basic ----

    #[test]
    fn new_watcher_empty() {
        let w = AgentWatcher::new(5000);
        assert_eq!(w.pattern_count(), 0);
        assert_eq!(w.watch_interval_ms(), 5000);
    }

    #[test]
    fn with_defaults_has_patterns() {
        let w = AgentWatcher::with_defaults(5000);
        assert!(w.pattern_count() > 0);
        let names = w.pattern_names();
        assert!(names.contains(&"error_pattern"));
        assert!(names.contains(&"panic_pattern"));
        assert!(names.contains(&"compiling"));
    }

    #[test]
    fn add_and_remove_pattern() {
        let mut w = AgentWatcher::new(5000);
        w.add_pattern(OutputPattern {
            name: "test".into(),
            pattern: "PASS".into(),
            extract_status: PatternStatus::Completed {
                result: "passed".into(),
            },
        });
        assert_eq!(w.pattern_count(), 1);

        assert!(w.remove_pattern("test"));
        assert_eq!(w.pattern_count(), 0);

        assert!(!w.remove_pattern("nonexistent"));
    }

    // ---- analyze_output ----

    #[test]
    fn analyze_output_matches_pattern() {
        let mut w = AgentWatcher::new(5000);
        w.add_pattern(OutputPattern {
            name: "err".into(),
            pattern: "ERROR".into(),
            extract_status: PatternStatus::Error {
                message: "error found".into(),
            },
        });

        let result = w.analyze_output("w1", "some output\nERROR: something broke\nmore", 1000);
        assert_eq!(
            result.status,
            WatchStatus::Error {
                message: "error found".into()
            }
        );
        assert_eq!(result.agent, "w1");
        assert_eq!(result.output_lines.len(), 3);
    }

    #[test]
    fn analyze_output_last_match_wins() {
        let mut w = AgentWatcher::new(5000);
        w.add_pattern(OutputPattern {
            name: "err".into(),
            pattern: "ERROR".into(),
            extract_status: PatternStatus::Error {
                message: "error".into(),
            },
        });
        w.add_pattern(OutputPattern {
            name: "ok".into(),
            pattern: "OK".into(),
            extract_status: PatternStatus::Completed {
                result: "ok".into(),
            },
        });

        // Last line matches "OK" pattern, which should override "ERROR" from earlier
        let result = w.analyze_output("w1", "ERROR: something\nthen OK all good", 1000);
        assert_eq!(
            result.status,
            WatchStatus::Completed {
                result: "ok".into()
            }
        );
    }

    #[test]
    fn analyze_output_no_match_with_output() {
        let mut w = AgentWatcher::new(5000);
        let result = w.analyze_output("w1", "some random output", 1000);
        assert_eq!(
            result.status,
            WatchStatus::Active {
                activity: "output detected".into()
            }
        );
    }

    #[test]
    fn analyze_output_empty_is_unresponsive() {
        let mut w = AgentWatcher::new(5000);
        let result = w.analyze_output("w1", "", 1000);
        assert_eq!(result.status, WatchStatus::Unresponsive);
        assert!(result.output_lines.is_empty());
    }

    #[test]
    fn analyze_output_stores_last_result() {
        let mut w = AgentWatcher::new(5000);
        w.analyze_output("w1", "hello", 1000);
        assert!(w.last_result("w1").is_some());
        assert_eq!(w.last_result("w1").unwrap().timestamp_ms, 1000);
    }

    #[test]
    fn analyze_output_updates_last_result() {
        let mut w = AgentWatcher::new(5000);
        w.analyze_output("w1", "first", 1000);
        w.analyze_output("w1", "second", 2000);
        assert_eq!(w.last_result("w1").unwrap().timestamp_ms, 2000);
    }

    // ---- Progress extraction ----

    #[test]
    fn extract_progress_percentage() {
        let mut w = AgentWatcher::new(5000);
        w.add_progress_pattern(ProgressPattern {
            name: "pct".into(),
            prefix: "progress:".into(),
            is_percentage: true,
        });

        let p = w.extract_progress("progress: 75");
        assert_eq!(p, Some(0.75));
    }

    #[test]
    fn extract_progress_fraction() {
        let mut w = AgentWatcher::new(5000);
        w.add_progress_pattern(ProgressPattern {
            name: "frac".into(),
            prefix: "completion:".into(),
            is_percentage: false,
        });

        let p = w.extract_progress("completion: 0.85");
        assert_eq!(p, Some(0.85));
    }

    #[test]
    fn extract_progress_clamps_percentage() {
        let mut w = AgentWatcher::new(5000);
        w.add_progress_pattern(ProgressPattern {
            name: "pct".into(),
            prefix: "progress:".into(),
            is_percentage: true,
        });

        let p = w.extract_progress("progress: 150");
        assert_eq!(p, Some(1.0));
    }

    #[test]
    fn extract_progress_clamps_fraction() {
        let mut w = AgentWatcher::new(5000);
        w.add_progress_pattern(ProgressPattern {
            name: "frac".into(),
            prefix: "completion:".into(),
            is_percentage: false,
        });

        let p = w.extract_progress("completion: 1.5");
        assert_eq!(p, Some(1.0));
    }

    #[test]
    fn extract_progress_no_match() {
        let w = AgentWatcher::new(5000);
        let p = w.extract_progress("no progress info here");
        assert_eq!(p, None);
    }

    #[test]
    fn extract_progress_last_line_wins() {
        let mut w = AgentWatcher::new(5000);
        w.add_progress_pattern(ProgressPattern {
            name: "pct".into(),
            prefix: "progress:".into(),
            is_percentage: true,
        });

        let p = w.extract_progress("progress: 25\nsome stuff\nprogress: 80");
        assert_eq!(p, Some(0.80));
    }

    #[test]
    fn extract_progress_case_insensitive_prefix() {
        let mut w = AgentWatcher::new(5000);
        w.add_progress_pattern(ProgressPattern {
            name: "pct".into(),
            prefix: "Progress:".into(),
            is_percentage: true,
        });

        let p = w.extract_progress("progress: 50");
        assert_eq!(p, Some(0.50));
    }

    #[test]
    fn analyze_output_includes_progress() {
        let mut w = AgentWatcher::new(5000);
        w.add_progress_pattern(ProgressPattern {
            name: "pct".into(),
            prefix: "progress:".into(),
            is_percentage: true,
        });

        let result = w.analyze_output("w1", "working...\nprogress: 60\nstill going", 1000);
        assert_eq!(result.progress, Some(0.60));
    }

    // ---- agents_needing_watch ----

    #[test]
    fn agents_needing_watch_when_stale() {
        let mut w = AgentWatcher::new(5000);
        w.analyze_output("w1", "hello", 1000);
        w.analyze_output("w2", "hello", 3000);

        // At t=4000, w1 is 3000ms old (< 5000), w2 is 1000ms old
        let needing = w.agents_needing_watch(4000);
        assert!(needing.is_empty());

        // At t=7000, w1 is 6000ms old (>= 5000), w2 is 4000ms old
        let needing = w.agents_needing_watch(7000);
        assert_eq!(needing.len(), 1);
        assert_eq!(needing[0], "w1");

        // At t=9000, both are stale
        let mut needing = w.agents_needing_watch(9000);
        needing.sort();
        assert_eq!(needing.len(), 2);
    }

    #[test]
    fn agents_needing_watch_empty() {
        let w = AgentWatcher::new(5000);
        assert!(w.agents_needing_watch(1000).is_empty());
    }

    // ---- agents_with_results ----

    #[test]
    fn agents_with_results_lists_watched() {
        let mut w = AgentWatcher::new(5000);
        w.analyze_output("w1", "hello", 1000);
        w.analyze_output("w2", "world", 1000);

        let mut agents = w.agents_with_results();
        agents.sort();
        assert_eq!(agents, vec!["w1", "w2"]);
    }

    // ---- clear_result ----

    #[test]
    fn clear_result_removes_agent() {
        let mut w = AgentWatcher::new(5000);
        w.analyze_output("w1", "hello", 1000);
        assert!(w.clear_result("w1"));
        assert!(w.last_result("w1").is_none());
    }

    #[test]
    fn clear_result_nonexistent() {
        let mut w = AgentWatcher::new(5000);
        assert!(!w.clear_result("ghost"));
    }

    #[test]
    fn clear_all_results_empties() {
        let mut w = AgentWatcher::new(5000);
        w.analyze_output("w1", "hello", 1000);
        w.analyze_output("w2", "world", 1000);
        w.clear_all_results();
        assert!(w.agents_with_results().is_empty());
    }

    // ---- PatternStatus serde ----

    #[test]
    fn pattern_status_serde() {
        let statuses = vec![
            PatternStatus::Active {
                activity: "building".into(),
            },
            PatternStatus::Waiting,
            PatternStatus::Error {
                message: "crash".into(),
            },
            PatternStatus::Completed {
                result: "done".into(),
            },
        ];
        for s in statuses {
            let json = serde_json::to_string(&s).unwrap();
            let back: PatternStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(back, s);
        }
    }

    // ---- OutputPattern serde ----

    #[test]
    fn output_pattern_serde() {
        let p = OutputPattern {
            name: "test".into(),
            pattern: "PASS".into(),
            extract_status: PatternStatus::Completed {
                result: "passed".into(),
            },
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: OutputPattern = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "test");
        assert_eq!(back.pattern, "PASS");
    }

    // ---- ProgressPattern serde ----

    #[test]
    fn progress_pattern_serde() {
        let p = ProgressPattern {
            name: "pct".into(),
            prefix: "progress:".into(),
            is_percentage: true,
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: ProgressPattern = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "pct");
        assert!(back.is_percentage);
    }

    // ---- Full workflow with defaults ----

    #[test]
    fn default_patterns_detect_error() {
        let mut w = AgentWatcher::with_defaults(5000);
        let output = "Starting build...\nCompiling main.rs\nError: undefined variable\n";
        let result = w.analyze_output("w1", output, 1000);
        // "Error:" pattern should match
        assert!(matches!(result.status, WatchStatus::Error { .. }));
    }

    #[test]
    fn default_patterns_detect_compilation() {
        let mut w = AgentWatcher::with_defaults(5000);
        let output = "Compiling claudimux-core v0.1.0\n";
        let result = w.analyze_output("w1", output, 1000);
        assert!(matches!(result.status, WatchStatus::Active { .. }));
    }

    #[test]
    fn default_patterns_detect_test_pass() {
        let mut w = AgentWatcher::with_defaults(5000);
        let output = "running 5 tests\ntest result: ok. 5 passed\n";
        let result = w.analyze_output("w1", output, 1000);
        assert!(matches!(result.status, WatchStatus::Completed { .. }));
    }

    #[test]
    fn default_patterns_detect_waiting() {
        let mut w = AgentWatcher::with_defaults(5000);
        let output = "$ ";
        let result = w.analyze_output("w1", output, 1000);
        assert_eq!(result.status, WatchStatus::Waiting);
    }

    #[test]
    fn default_progress_extraction() {
        let mut w = AgentWatcher::with_defaults(5000);
        let output = "Building...\nprogress: 45\nstill going";
        let result = w.analyze_output("w1", output, 1000);
        assert_eq!(result.progress, Some(0.45));
    }

    // ---- Multiple agents workflow ----

    #[test]
    fn watch_multiple_agents() {
        let mut w = AgentWatcher::new(5000);
        w.add_pattern(OutputPattern {
            name: "done".into(),
            pattern: "DONE".into(),
            extract_status: PatternStatus::Completed {
                result: "finished".into(),
            },
        });

        w.analyze_output("w1", "working...", 1000);
        w.analyze_output("w2", "DONE", 1000);
        w.analyze_output("w3", "", 1000);

        assert!(matches!(
            w.last_result("w1").unwrap().status,
            WatchStatus::Active { .. }
        ));
        assert!(matches!(
            w.last_result("w2").unwrap().status,
            WatchStatus::Completed { .. }
        ));
        assert_eq!(
            w.last_result("w3").unwrap().status,
            WatchStatus::Unresponsive
        );
    }
}
