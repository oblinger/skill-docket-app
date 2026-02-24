//! Heartbeat parser — extracts agent state from tmux pane capture output.
//!
//! When CMX captures a pane's contents, this module inspects the last few
//! lines to determine what state the agent is in: waiting at a prompt (Ready),
//! actively running (Busy), showing an error (Error), or indeterminate (Unknown).

/// The state of an agent as inferred from its pane capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentState {
    /// Agent is at a prompt, ready to receive input.
    Ready,
    /// Agent appears to be executing a command or processing.
    Busy,
    /// An error pattern was detected in the output.
    Error,
    /// Could not determine the agent's state.
    Unknown,
}

/// The result of parsing a pane capture.
#[derive(Debug, Clone)]
pub struct HeartbeatResult {
    /// Inferred state of the agent.
    pub state: AgentState,
    /// Context window usage percentage, if detected (e.g. "Context: 73%").
    pub context_percent: Option<u32>,
    /// The last non-empty line from the capture.
    pub last_line: String,
}

/// Common error patterns to look for in pane output.
const ERROR_PATTERNS: &[&str] = &[
    "Traceback (most recent call last)",
    "Error:",
    "error:",
    "ERROR:",
    "FAILED",
    "panic:",
    "fatal:",
    "FATAL:",
    "exception:",
    "Exception:",
];

/// Parse the captured output of a tmux pane to determine agent state.
///
/// # Arguments
///
/// * `output` — the raw text captured from the pane.
/// * `prompt_pattern` — a substring that indicates the agent is at a prompt
///   (e.g. `"$ "` or `"❯ "` or a regex-like simple pattern). For simplicity,
///   this uses substring matching, not full regex.
pub fn parse_capture(output: &str, prompt_pattern: &str) -> HeartbeatResult {
    let lines: Vec<&str> = output.lines().collect();
    let last_line = find_last_nonempty(&lines).unwrap_or("").to_string();
    let context_percent = detect_context_percent(&lines);

    // Check the tail of the output (last 5 lines) for error patterns.
    let tail_start = if lines.len() > 5 { lines.len() - 5 } else { 0 };
    let tail = &lines[tail_start..];

    // Check for errors first — error state takes priority over prompt detection.
    if has_error_pattern(tail) {
        return HeartbeatResult {
            state: AgentState::Error,
            context_percent,
            last_line,
        };
    }

    // Check if the last non-empty line looks like a prompt.
    if !last_line.is_empty() && last_line.contains(prompt_pattern) {
        return HeartbeatResult {
            state: AgentState::Ready,
            context_percent,
            last_line,
        };
    }

    // Check for Claude Code specific prompt patterns in last few lines.
    if is_claude_prompt(&lines) {
        return HeartbeatResult {
            state: AgentState::Ready,
            context_percent,
            last_line,
        };
    }

    // If we have output but no prompt, the agent is probably busy.
    if !output.trim().is_empty() {
        return HeartbeatResult {
            state: AgentState::Busy,
            context_percent,
            last_line,
        };
    }

    HeartbeatResult {
        state: AgentState::Unknown,
        context_percent,
        last_line,
    }
}

/// Find the last non-empty, non-whitespace line.
fn find_last_nonempty<'a>(lines: &[&'a str]) -> Option<&'a str> {
    lines.iter().rev().find(|l| !l.trim().is_empty()).copied()
}

/// Detect context window usage from lines matching "Context: NN%" or similar.
fn detect_context_percent(lines: &[&str]) -> Option<u32> {
    // Search from the end since the most recent context indicator is most relevant.
    for line in lines.iter().rev() {
        let trimmed = line.trim();
        // Match patterns like "Context: 73%" or "context: 73%" or "Context 73%"
        if let Some(pct) = extract_context_percent(trimmed) {
            return Some(pct);
        }
    }
    None
}

/// Try to extract a percentage from a line like "Context: 73%" or "context: 45%".
fn extract_context_percent(line: &str) -> Option<u32> {
    let lower = line.to_lowercase();
    // Find "context" followed eventually by a number and "%"
    if let Some(ctx_pos) = lower.find("context") {
        let after_ctx = &line[ctx_pos + 7..];
        // Walk forward to find digits followed by '%'
        let mut num_start = None;
        for (i, ch) in after_ctx.chars().enumerate() {
            if ch.is_ascii_digit() {
                if num_start.is_none() {
                    num_start = Some(i);
                }
            } else if ch == '%' {
                if let Some(start) = num_start {
                    if let Ok(pct) = after_ctx[start..i].parse::<u32>() {
                        if pct <= 100 {
                            return Some(pct);
                        }
                    }
                }
                break;
            } else if num_start.is_some() {
                // Non-digit, non-% after digits — not a match.
                break;
            }
        }
    }
    None
}

/// Check whether any of the given lines contain a known error pattern.
fn has_error_pattern(lines: &[&str]) -> bool {
    for line in lines {
        for pattern in ERROR_PATTERNS {
            if line.contains(pattern) {
                return true;
            }
        }
    }
    false
}

/// Detect Claude Code prompt patterns. Claude Code shows a `>` or `❯` prompt
/// when ready for input, often preceded by context info.
fn is_claude_prompt(lines: &[&str]) -> bool {
    // Check last 3 lines for common Claude Code prompt markers.
    let check_count = lines.len().min(3);
    let start = lines.len() - check_count;
    for line in &lines[start..] {
        let trimmed = line.trim();
        if trimmed == ">" || trimmed == ">" || trimmed.ends_with("> ") {
            return true;
        }
        // Claude Code prompt often looks like: "claude-code > " or just "> "
        if trimmed.ends_with('>') && trimmed.len() < 40 {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_at_shell_prompt() {
        let output = "some output\n$ ";
        let result = parse_capture(output, "$ ");
        assert_eq!(result.state, AgentState::Ready);
        assert_eq!(result.last_line, "$ ");
    }

    #[test]
    fn busy_running_command() {
        let output = "running tests...\ntest_foo ... ok\ntest_bar ... ok";
        let result = parse_capture(output, "$ ");
        assert_eq!(result.state, AgentState::Busy);
    }

    #[test]
    fn error_python_traceback() {
        let output = "running...\nTraceback (most recent call last)\n  File \"x.py\", line 1\nNameError: name 'x' is not defined";
        let result = parse_capture(output, "$ ");
        assert_eq!(result.state, AgentState::Error);
    }

    #[test]
    fn error_generic_error() {
        let output = "compiling...\nError: cannot find module 'foo'";
        let result = parse_capture(output, "$ ");
        assert_eq!(result.state, AgentState::Error);
    }

    #[test]
    fn unknown_empty_output() {
        let result = parse_capture("", "$ ");
        assert_eq!(result.state, AgentState::Unknown);
        assert!(result.last_line.is_empty());
    }

    #[test]
    fn unknown_whitespace_only() {
        let result = parse_capture("   \n  \n  ", "$ ");
        assert_eq!(result.state, AgentState::Unknown);
    }

    #[test]
    fn detects_context_percent() {
        let output = "Working on task...\nContext: 73%\n$ ";
        let result = parse_capture(output, "$ ");
        assert_eq!(result.context_percent, Some(73));
    }

    #[test]
    fn detects_context_percent_lowercase() {
        let output = "context: 45%\nprompt $ ";
        let result = parse_capture(output, "$ ");
        assert_eq!(result.context_percent, Some(45));
    }

    #[test]
    fn no_context_percent_when_absent() {
        let output = "just some output\n$ ";
        let result = parse_capture(output, "$ ");
        assert!(result.context_percent.is_none());
    }

    #[test]
    fn claude_prompt_detection() {
        let output = "Task complete.\n>";
        let result = parse_capture(output, "$ ");
        assert_eq!(result.state, AgentState::Ready);
    }

    #[test]
    fn error_takes_priority_over_prompt() {
        let output = "Error: something broke\n$ ";
        let result = parse_capture(output, "$ ");
        // Error is in the tail, so error state wins.
        assert_eq!(result.state, AgentState::Error);
    }

    #[test]
    fn last_line_captured() {
        let output = "line1\nline2\nline3";
        let result = parse_capture(output, "$ ");
        assert_eq!(result.last_line, "line3");
    }

    #[test]
    fn context_percent_rejects_over_100() {
        assert!(extract_context_percent("Context: 150%").is_none());
    }

    #[test]
    fn extract_context_various_formats() {
        assert_eq!(extract_context_percent("Context: 50%"), Some(50));
        assert_eq!(extract_context_percent("context:50%"), Some(50));
        assert_eq!(extract_context_percent("Context 99%"), Some(99));
        assert_eq!(extract_context_percent("Context: 0%"), Some(0));
    }
}
