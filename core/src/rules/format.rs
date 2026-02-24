//! Rule format parsers — parse rules written in arrow, table, or block
//! format into a unified `Rule` AST.
//!
//! All three formats produce identical `Rule` structs with the same
//! `Expression` conditions and `RuleAction` lists.

use serde::{Deserialize, Serialize};

use super::expr::Expression;


// ---------------------------------------------------------------------------
// RuleAction
// ---------------------------------------------------------------------------

/// The operator in a rule action assignment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionOp {
    /// `=` — set a value.
    Set,
    /// `+=` — append to a value.
    Append,
}

/// An action that a rule produces when it fires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleAction {
    pub path: String,
    pub operator: ActionOp,
    pub value: String,
}

impl RuleAction {
    /// Parse `"task.$t.status = in_progress"` or `"agent.$a.inbox += msg"`.
    pub fn parse(input: &str) -> Result<Self, String> {
        let input = input.trim();
        if input.is_empty() {
            return Err("empty action".into());
        }

        // Try "+=" first (before "=") to avoid partial match.
        if let Some(pos) = input.find("+=") {
            let path = input[..pos].trim().to_string();
            let value = input[pos + 2..].trim().to_string();
            if path.is_empty() {
                return Err("empty path in action".into());
            }
            if value.is_empty() {
                return Err("empty value in action".into());
            }
            return Ok(RuleAction {
                path,
                operator: ActionOp::Append,
                value,
            });
        }

        // Try "=" — but not "==" which is a condition operator.
        if let Some(pos) = find_single_eq(input) {
            let path = input[..pos].trim().to_string();
            let value = input[pos + 1..].trim().to_string();
            if path.is_empty() {
                return Err("empty path in action".into());
            }
            if value.is_empty() {
                return Err("empty value in action".into());
            }
            return Ok(RuleAction {
                path,
                operator: ActionOp::Set,
                value,
            });
        }

        Err(format!("no assignment operator found in action: '{}'", input))
    }
}

/// Find a single `=` that is not part of `==`, `!=`, `>=`, `<=`, or `+=`.
fn find_single_eq(input: &str) -> Option<usize> {
    let bytes = input.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] == b'=' {
            let prev = if i > 0 { bytes[i - 1] } else { 0 };
            let next = if i + 1 < bytes.len() { bytes[i + 1] } else { 0 };
            if prev != b'!' && prev != b'<' && prev != b'>' && prev != b'+'
                && prev != b'=' && next != b'='
            {
                return Some(i);
            }
        }
    }
    None
}


// ---------------------------------------------------------------------------
// Rule
// ---------------------------------------------------------------------------

/// A complete rule: conditions produce actions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub name: Option<String>,
    pub conditions: Expression,
    pub actions: Vec<RuleAction>,
    pub priority: Option<i32>,
}


// ---------------------------------------------------------------------------
// Arrow format parser
// ---------------------------------------------------------------------------

/// Parse arrow format rules: `condition --> action; action`.
///
/// Each rule must contain `-->`. Multiple actions are separated by `;`.
/// Parenthesized conditions may span multiple lines. Lines without `-->`
/// are joined to adjacent lines until a complete rule is formed.
pub fn parse_arrow_rules(input: &str) -> Result<Vec<Rule>, String> {
    let joined = join_arrow_lines(input);
    let mut rules = Vec::new();

    for line in joined.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let arrow_pos = line.find("-->").ok_or_else(|| {
            format!("arrow rule missing '-->': '{}'", line)
        })?;

        let cond_str = line[..arrow_pos].trim();
        let action_str = line[arrow_pos + 3..].trim();

        // Strip outer parens from condition if present.
        let cond_str = strip_outer_parens(cond_str);

        let conditions = Expression::parse(cond_str)?;
        let actions = parse_action_list(action_str)?;

        if actions.is_empty() {
            return Err(format!("arrow rule has no actions: '{}'", line));
        }

        rules.push(Rule {
            name: None,
            conditions,
            actions,
            priority: None,
        });
    }

    if rules.is_empty() {
        return Err("no arrow rules found".into());
    }
    Ok(rules)
}

/// Join lines into complete arrow rules. Lines are accumulated until we
/// have balanced parentheses AND the accumulated text contains `-->`.
fn join_arrow_lines(input: &str) -> String {
    let mut result = Vec::new();
    let mut pending = String::new();
    let mut paren_depth = 0i32;

    for line in input.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            // Blank line — flush pending if we have a complete rule.
            if !pending.is_empty() {
                if pending.contains("-->") && paren_depth <= 0 {
                    result.push(pending.clone());
                    pending.clear();
                    paren_depth = 0;
                }
                // Otherwise keep accumulating (blank line inside multi-line rule).
            }
            continue;
        }

        if pending.is_empty() {
            pending.push_str(trimmed);
        } else {
            pending.push(' ');
            pending.push_str(trimmed);
        }
        paren_depth += count_parens(trimmed);

        // A rule is complete when parens are balanced and we have -->.
        if paren_depth <= 0 && pending.contains("-->") {
            result.push(pending.clone());
            pending.clear();
            paren_depth = 0;
        }
    }

    // Flush any remaining.
    if !pending.is_empty() {
        result.push(pending);
    }

    result.join("\n")
}


// ---------------------------------------------------------------------------
// Table format parser
// ---------------------------------------------------------------------------

/// Parse table format rules: a markdown table with When and Then columns.
///
/// The first row must be a header containing "When" and "Then" (case-
/// insensitive). The second row is the separator (`|---|---|`). Subsequent
/// rows are rules.
pub fn parse_table_rules(input: &str) -> Result<Vec<Rule>, String> {
    let lines: Vec<&str> = input.lines().collect();

    // Find header row.
    let header_idx = lines
        .iter()
        .position(|l| {
            let lower = l.to_ascii_lowercase();
            lower.contains("when") && lower.contains("then")
        })
        .ok_or("table missing When/Then header")?;

    let header = lines[header_idx];
    let (when_col, then_col) = find_table_columns(header)?;

    // Skip separator row.
    let data_start = header_idx + 2;
    if data_start > lines.len() {
        return Err("table has no data rows".into());
    }

    let mut rules = Vec::new();

    for line in &lines[data_start..] {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if !line.contains('|') {
            continue;
        }

        let cells = split_table_row(line);
        if cells.len() < 2 {
            continue;
        }

        let when_str = cells
            .get(when_col)
            .map(|s| s.trim())
            .unwrap_or("")
            .trim();
        let then_str = cells
            .get(then_col)
            .map(|s| s.trim())
            .unwrap_or("")
            .trim();

        if when_str.is_empty() || then_str.is_empty() {
            continue;
        }

        let conditions = Expression::parse(when_str)?;
        let actions = parse_action_list(then_str)?;

        rules.push(Rule {
            name: None,
            conditions,
            actions,
            priority: None,
        });
    }

    if rules.is_empty() {
        return Err("no table rules found".into());
    }
    Ok(rules)
}


// ---------------------------------------------------------------------------
// Block format parser
// ---------------------------------------------------------------------------

/// Parse indented block format rules.
///
/// Each rule starts with `when:` and is followed by indented conditions
/// (one per line, implicitly ANDed). The `then:` keyword starts the
/// action section with one action per indented line. Rules are separated
/// by blank lines or another `when:`.
pub fn parse_block_rules(input: &str) -> Result<Vec<Rule>, String> {
    let mut rules = Vec::new();
    let mut current_when: Vec<String> = Vec::new();
    let mut current_then: Vec<String> = Vec::new();
    let mut in_then = false;
    let mut saw_when = false;

    for line in input.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            // Blank line — flush current rule if we have one.
            if saw_when {
                flush_block_rule(
                    &current_when,
                    &current_then,
                    &mut rules,
                )?;
                current_when.clear();
                current_then.clear();
                in_then = false;
                saw_when = false;
            }
            continue;
        }

        if trimmed.eq_ignore_ascii_case("when:") {
            // Start of a new rule — flush any pending.
            if saw_when {
                flush_block_rule(
                    &current_when,
                    &current_then,
                    &mut rules,
                )?;
                current_when.clear();
                current_then.clear();
            }
            in_then = false;
            saw_when = true;
            continue;
        }

        if trimmed.eq_ignore_ascii_case("then:") {
            if !saw_when {
                return Err("'then:' without preceding 'when:'".into());
            }
            in_then = true;
            continue;
        }

        if saw_when && !in_then {
            current_when.push(trimmed.to_string());
        } else if in_then {
            current_then.push(trimmed.to_string());
        }
    }

    // Flush final rule.
    if saw_when {
        flush_block_rule(&current_when, &current_then, &mut rules)?;
    }

    if rules.is_empty() {
        return Err("no block rules found".into());
    }
    Ok(rules)
}


// ---------------------------------------------------------------------------
// Auto-detect
// ---------------------------------------------------------------------------

/// Auto-detect format and parse. Tries table, arrow, then block in order.
pub fn parse_rules_auto(input: &str) -> Result<Vec<Rule>, String> {
    let trimmed = input.trim();

    // Table: look for a header row with "When" and "Then" and pipes.
    if trimmed.lines().any(|l| {
        let lower = l.to_ascii_lowercase();
        lower.contains('|') && lower.contains("when") && lower.contains("then")
    }) {
        return parse_table_rules(input);
    }

    // Arrow: look for "-->".
    if trimmed.contains("-->") {
        return parse_arrow_rules(input);
    }

    // Block: look for "when:" keyword.
    if trimmed.lines().any(|l| l.trim().eq_ignore_ascii_case("when:")) {
        return parse_block_rules(input);
    }

    Err("could not auto-detect rule format (no '-->', '| When | Then |', or 'when:' found)".into())
}


// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Parse semicolon-separated actions.
fn parse_action_list(input: &str) -> Result<Vec<RuleAction>, String> {
    let mut actions = Vec::new();
    for part in input.split(';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        actions.push(RuleAction::parse(part)?);
    }
    Ok(actions)
}

/// Count net open parens in a string.
fn count_parens(s: &str) -> i32 {
    let mut depth = 0i32;
    for ch in s.chars() {
        if ch == '(' {
            depth += 1;
        } else if ch == ')' {
            depth -= 1;
        }
    }
    depth
}

/// Strip balanced outer parentheses from a string.
fn strip_outer_parens(s: &str) -> &str {
    let s = s.trim();
    if s.starts_with('(') && s.ends_with(')') {
        // Verify the parens match (the closing paren at the end matches
        // the opening at the start).
        let inner = &s[1..s.len() - 1];
        let mut depth = 0i32;
        let mut valid = true;
        for ch in inner.chars() {
            if ch == '(' {
                depth += 1;
            } else if ch == ')' {
                depth -= 1;
                if depth < 0 {
                    valid = false;
                    break;
                }
            }
        }
        if valid && depth == 0 {
            return inner.trim();
        }
    }
    s
}

/// Determine which column indices correspond to "When" and "Then".
fn find_table_columns(header: &str) -> Result<(usize, usize), String> {
    let cells = split_table_row(header);
    let mut when_col = None;
    let mut then_col = None;
    for (i, cell) in cells.iter().enumerate() {
        let lower = cell.trim().to_ascii_lowercase();
        if lower == "when" {
            when_col = Some(i);
        } else if lower == "then" {
            then_col = Some(i);
        }
    }
    match (when_col, then_col) {
        (Some(w), Some(t)) => Ok((w, t)),
        _ => Err("table header must contain 'When' and 'Then' columns".into()),
    }
}

/// Split a markdown table row into cells, stripping the outer pipes.
fn split_table_row(line: &str) -> Vec<&str> {
    let line = line.trim();
    // Strip leading and trailing pipe.
    let line = line.strip_prefix('|').unwrap_or(line);
    let line = line.strip_suffix('|').unwrap_or(line);
    line.split('|').collect()
}

/// Flush accumulated when/then lines into a Rule.
fn flush_block_rule(
    when_lines: &[String],
    then_lines: &[String],
    rules: &mut Vec<Rule>,
) -> Result<(), String> {
    if when_lines.is_empty() {
        return Err("block rule has empty 'when:' section".into());
    }
    if then_lines.is_empty() {
        return Err("block rule missing 'then:' section".into());
    }

    // Multiple when-lines are implicitly ANDed.
    let conditions = if when_lines.len() == 1 {
        Expression::parse(&when_lines[0])?
    } else {
        let exprs: Result<Vec<Expression>, String> = when_lines
            .iter()
            .map(|l| Expression::parse(l))
            .collect();
        Expression::And(exprs?)
    };

    let actions: Result<Vec<RuleAction>, String> = then_lines
        .iter()
        .map(|l| RuleAction::parse(l))
        .collect();

    rules.push(Rule {
        name: None,
        conditions,
        actions: actions?,
        priority: None,
    });

    Ok(())
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- RuleAction parsing ---

    #[test]
    fn parse_action_set() {
        let a = RuleAction::parse("task.$t.status = in_progress").unwrap();
        assert_eq!(a.path, "task.$t.status");
        assert_eq!(a.operator, ActionOp::Set);
        assert_eq!(a.value, "in_progress");
    }

    #[test]
    fn parse_action_append() {
        let a = RuleAction::parse("agent.$a.inbox += msg").unwrap();
        assert_eq!(a.path, "agent.$a.inbox");
        assert_eq!(a.operator, ActionOp::Append);
        assert_eq!(a.value, "msg");
    }

    #[test]
    fn error_on_empty_action() {
        assert!(RuleAction::parse("").is_err());
    }

    #[test]
    fn error_on_action_no_operator() {
        assert!(RuleAction::parse("task.status in_progress").is_err());
    }

    // --- Arrow format ---

    #[test]
    fn parse_simple_arrow_rule() {
        let rules = parse_arrow_rules(
            "task.$t.status == ready AND agent.$a.status == idle --> task.$t.status = in_progress",
        )
        .unwrap();
        assert_eq!(rules.len(), 1);
        let r = &rules[0];
        assert_eq!(r.actions.len(), 1);
        assert_eq!(r.actions[0].value, "in_progress");
        let conds = r.conditions.conditions();
        assert_eq!(conds.len(), 2);
    }

    #[test]
    fn parse_arrow_multiple_actions() {
        let rules = parse_arrow_rules(
            "task.$t.status == ready --> task.$t.status = in_progress; task.$t.agent = $a",
        )
        .unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].actions.len(), 2);
        assert_eq!(rules[0].actions[0].value, "in_progress");
        assert_eq!(rules[0].actions[1].value, "$a");
    }

    #[test]
    fn parse_arrow_multiple_rules() {
        let input = "\
task.$t.status == ready --> task.$t.status = in_progress
agent.$a.health == unhealthy --> agent.$a.status = error";
        let rules = parse_arrow_rules(input).unwrap();
        assert_eq!(rules.len(), 2);
    }

    #[test]
    fn error_arrow_missing_separator() {
        assert!(parse_arrow_rules("task.$t.status == ready = in_progress").is_err());
    }

    // --- Table format ---

    #[test]
    fn parse_table_two_rules() {
        let input = "\
| When | Then |
|------|------|
| task.$t.status == ready AND agent.$a.status == idle | task.$t.status = in_progress |
| agent.$a.health == unhealthy | agent.$a.status = error |";
        let rules = parse_table_rules(input).unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].actions[0].value, "in_progress");
        assert_eq!(rules[1].actions[0].value, "error");
    }

    #[test]
    fn error_table_wrong_columns() {
        let input = "\
| If | Do |
|------|------|
| task.$t.status == ready | task.$t.status = done |";
        assert!(parse_table_rules(input).is_err());
    }

    #[test]
    fn error_table_no_data() {
        let input = "\
| When | Then |
|------|------|";
        assert!(parse_table_rules(input).is_err());
    }

    // --- Block format ---

    #[test]
    fn parse_block_rule() {
        let input = "\
when:
    task.$t.status == ready
    agent.$a.status == idle
then:
    task.$t.status = in_progress
    task.$t.agent = $a";
        let rules = parse_block_rules(input).unwrap();
        assert_eq!(rules.len(), 1);
        let r = &rules[0];
        assert_eq!(r.actions.len(), 2);
        // Multiple when-conditions are ANDed.
        let conds = r.conditions.conditions();
        assert_eq!(conds.len(), 2);
    }

    #[test]
    fn parse_block_single_condition() {
        let input = "\
when:
    agent.$a.health == unhealthy
then:
    agent.$a.status = error";
        let rules = parse_block_rules(input).unwrap();
        assert_eq!(rules.len(), 1);
        let conds = rules[0].conditions.conditions();
        assert_eq!(conds.len(), 1);
    }

    #[test]
    fn parse_block_multiple_rules() {
        let input = "\
when:
    task.$t.status == ready
then:
    task.$t.status = in_progress

when:
    agent.$a.health == unhealthy
then:
    agent.$a.status = error";
        let rules = parse_block_rules(input).unwrap();
        assert_eq!(rules.len(), 2);
    }

    #[test]
    fn error_block_missing_then() {
        let input = "\
when:
    task.$t.status == ready";
        assert!(parse_block_rules(input).is_err());
    }

    #[test]
    fn error_block_then_without_when() {
        let input = "\
then:
    task.$t.status = done";
        assert!(parse_block_rules(input).is_err());
    }

    // --- Cross-format equivalence ---

    #[test]
    fn same_rule_all_formats() {
        let arrow_input =
            "task.$t.status == ready AND agent.$a.status == idle --> task.$t.status = in_progress";
        let table_input = "\
| When | Then |
|------|------|
| task.$t.status == ready AND agent.$a.status == idle | task.$t.status = in_progress |";
        let block_input = "\
when:
    task.$t.status == ready
    agent.$a.status == idle
then:
    task.$t.status = in_progress";

        let arrow = parse_arrow_rules(arrow_input).unwrap();
        let table = parse_table_rules(table_input).unwrap();
        let block = parse_block_rules(block_input).unwrap();

        assert_eq!(arrow.len(), 1);
        assert_eq!(table.len(), 1);
        assert_eq!(block.len(), 1);

        // Actions should be identical.
        assert_eq!(arrow[0].actions, table[0].actions);
        assert_eq!(arrow[0].actions, block[0].actions);

        // Conditions should have the same condition count.
        assert_eq!(
            arrow[0].conditions.conditions().len(),
            table[0].conditions.conditions().len(),
        );
        assert_eq!(
            arrow[0].conditions.conditions().len(),
            block[0].conditions.conditions().len(),
        );

        // Verify condition content matches.
        let arrow_conds = arrow[0].conditions.conditions();
        let table_conds = table[0].conditions.conditions();
        let block_conds = block[0].conditions.conditions();

        for i in 0..arrow_conds.len() {
            assert_eq!(arrow_conds[i], table_conds[i]);
            assert_eq!(arrow_conds[i], block_conds[i]);
        }
    }

    // --- Auto-detect ---

    #[test]
    fn auto_detect_arrow() {
        let input =
            "task.$t.status == ready --> task.$t.status = in_progress";
        let rules = parse_rules_auto(input).unwrap();
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn auto_detect_table() {
        let input = "\
| When | Then |
|------|------|
| task.$t.status == ready | task.$t.status = in_progress |";
        let rules = parse_rules_auto(input).unwrap();
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn auto_detect_block() {
        let input = "\
when:
    task.$t.status == ready
then:
    task.$t.status = in_progress";
        let rules = parse_rules_auto(input).unwrap();
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn auto_detect_error_on_unknown() {
        assert!(parse_rules_auto("this is not a rule format").is_err());
    }

    // --- Parenthesized arrow ---

    #[test]
    fn parse_arrow_with_parens() {
        let input = "\
(task.$t.status == ready
 AND agent.$a.status == idle)
--> task.$t.status = in_progress";
        let rules = parse_arrow_rules(input).unwrap();
        assert_eq!(rules.len(), 1);
        let conds = rules[0].conditions.conditions();
        assert_eq!(conds.len(), 2);
    }

    // --- Action with whitespace ---

    #[test]
    fn action_whitespace_tolerance() {
        let a = RuleAction::parse("  task.$t.status  =  done  ").unwrap();
        assert_eq!(a.path, "task.$t.status");
        assert_eq!(a.value, "done");
    }

    // --- Append action in rule ---

    #[test]
    fn arrow_rule_with_append_action() {
        let rules = parse_arrow_rules(
            "task.$t.status == ready --> agent.$a.log += assigned",
        )
        .unwrap();
        assert_eq!(rules[0].actions[0].operator, ActionOp::Append);
        assert_eq!(rules[0].actions[0].value, "assigned");
    }

    // --- Block format: consecutive when: without blank line ---

    #[test]
    fn parse_block_consecutive_when() {
        let input = "\
when:
    task.$t.status == ready
then:
    task.$t.status = in_progress
when:
    agent.$a.health == unhealthy
then:
    agent.$a.status = error";
        let rules = parse_block_rules(input).unwrap();
        assert_eq!(rules.len(), 2);
    }
}
