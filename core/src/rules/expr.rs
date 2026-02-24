//! Expression language parser — parses conditional expressions with path
//! patterns, variable binding, wildcards, and comparison/logical operators.
//!
//! Expressions look like `task.$t.status == complete AND agent.$a.status == idle`.
//! The parser produces an AST of `Expression` nodes that can later be
//! evaluated against a namespace store.

use serde::{Deserialize, Serialize};


// ---------------------------------------------------------------------------
// PathSegment / PathPattern
// ---------------------------------------------------------------------------

/// A segment in a namespace path pattern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PathSegment {
    /// Exact text: "task", "AUTH1", "status"
    Literal(String),
    /// Variable binding: `$var` matches any single segment and binds it.
    Variable(String),
    /// Wildcard: `*` matches any single segment without binding.
    Wildcard,
}

/// A dotted path pattern like `task.$t.status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathPattern {
    pub segments: Vec<PathSegment>,
}

impl PathPattern {
    /// Parse `"task.$t.status"` into a `PathPattern`.
    pub fn parse(input: &str) -> Result<Self, String> {
        let input = input.trim();
        if input.is_empty() {
            return Err("empty path".into());
        }
        let segments: Result<Vec<PathSegment>, String> = input
            .split('.')
            .map(|seg| {
                let seg = seg.trim();
                if seg.is_empty() {
                    Err("empty path segment".into())
                } else if seg == "*" {
                    Ok(PathSegment::Wildcard)
                } else if let Some(var) = seg.strip_prefix('$') {
                    if var.is_empty() {
                        Err("empty variable name after '$'".into())
                    } else {
                        Ok(PathSegment::Variable(var.to_string()))
                    }
                } else {
                    Ok(PathSegment::Literal(seg.to_string()))
                }
            })
            .collect();
        Ok(PathPattern { segments: segments? })
    }

    /// Format back to dotted string representation.
    pub fn to_string(&self) -> String {
        self.segments
            .iter()
            .map(|s| match s {
                PathSegment::Literal(l) => l.clone(),
                PathSegment::Variable(v) => format!("${}", v),
                PathSegment::Wildcard => "*".to_string(),
            })
            .collect::<Vec<_>>()
            .join(".")
    }

    /// Whether this pattern contains any variable segments.
    pub fn has_variables(&self) -> bool {
        self.segments
            .iter()
            .any(|s| matches!(s, PathSegment::Variable(_)))
    }

    /// All variable names referenced in this pattern.
    pub fn variables(&self) -> Vec<&str> {
        self.segments
            .iter()
            .filter_map(|s| match s {
                PathSegment::Variable(v) => Some(v.as_str()),
                _ => None,
            })
            .collect()
    }
}


// ---------------------------------------------------------------------------
// Operator
// ---------------------------------------------------------------------------

/// Comparison operators supported in conditions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Operator {
    /// `==`
    Eq,
    /// `!=`
    NotEq,
    /// `>`
    Gt,
    /// `<`
    Lt,
    /// `>=`
    GtEq,
    /// `<=`
    LtEq,
    /// `contains`
    Contains,
    /// `is empty`
    IsEmpty,
    /// `is not empty`
    IsNotEmpty,
}


// ---------------------------------------------------------------------------
// Condition
// ---------------------------------------------------------------------------

/// A single condition: `path operator [value]`.
///
/// For `IsEmpty` / `IsNotEmpty`, `value` is `None`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Condition {
    pub path: PathPattern,
    pub operator: Operator,
    pub value: Option<String>,
}

impl Condition {
    /// Parse a single condition like `"task.$t.status == complete"`.
    pub fn parse(input: &str) -> Result<Self, String> {
        let input = input.trim();
        if input.is_empty() {
            return Err("empty condition".into());
        }

        // Try "is not empty" first (3-word operator).
        if let Some(pos) = find_case_insensitive(input, " is not empty") {
            let path_str = &input[..pos];
            let path = PathPattern::parse(path_str)?;
            let after_end = pos + " is not empty".len();
            if after_end < input.len() {
                let after = input[after_end..].trim();
                if !after.is_empty() {
                    return Err(format!(
                        "unexpected text after 'is not empty': '{}'",
                        after
                    ));
                }
            }
            return Ok(Condition {
                path,
                operator: Operator::IsNotEmpty,
                value: None,
            });
        }

        // Try "is empty" (2-word operator).
        if let Some(pos) = find_case_insensitive(input, " is empty") {
            let path_str = &input[..pos];
            let path = PathPattern::parse(path_str)?;
            let after_end = pos + " is empty".len();
            if after_end < input.len() {
                let after = input[after_end..].trim();
                if !after.is_empty() {
                    return Err(format!(
                        "unexpected text after 'is empty': '{}'",
                        after
                    ));
                }
            }
            return Ok(Condition {
                path,
                operator: Operator::IsEmpty,
                value: None,
            });
        }

        // Try symbolic operators: >=, <=, !=, ==, >, <
        let ops: &[(&str, Operator)] = &[
            (">=", Operator::GtEq),
            ("<=", Operator::LtEq),
            ("!=", Operator::NotEq),
            ("==", Operator::Eq),
            (">", Operator::Gt),
            ("<", Operator::Lt),
        ];

        for (sym, op) in ops {
            if let Some(pos) = input.find(sym) {
                let path_str = input[..pos].trim();
                let value_str = input[pos + sym.len()..].trim();
                let path = PathPattern::parse(path_str)?;
                if value_str.is_empty() {
                    return Err(format!("missing value after '{}'", sym));
                }
                return Ok(Condition {
                    path,
                    operator: op.clone(),
                    value: Some(value_str.to_string()),
                });
            }
        }

        // Try "contains" keyword.
        if let Some(pos) = find_word_boundary(input, "contains") {
            let path_str = input[..pos].trim();
            let value_str = input[pos + "contains".len()..].trim();
            let path = PathPattern::parse(path_str)?;
            if value_str.is_empty() {
                return Err("missing value after 'contains'".into());
            }
            return Ok(Condition {
                path,
                operator: Operator::Contains,
                value: Some(value_str.to_string()),
            });
        }

        Err(format!("no recognized operator in condition: '{}'", input))
    }
}


// ---------------------------------------------------------------------------
// Expression
// ---------------------------------------------------------------------------

/// Logical expression tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Expression {
    /// A single comparison condition.
    Condition(Condition),
    /// All sub-expressions must be true.
    And(Vec<Expression>),
    /// At least one sub-expression must be true.
    Or(Vec<Expression>),
    /// Negation.
    Not(Box<Expression>),
}

impl Expression {
    /// Parse a full expression with AND/OR/NOT connectives.
    ///
    /// Connective keywords are case-insensitive (`AND`, `and`, `And`).
    /// Paths and values are case-sensitive.
    pub fn parse(input: &str) -> Result<Self, String> {
        let input = input.trim();
        if input.is_empty() {
            return Err("empty expression".into());
        }
        parse_or_expr(input)
    }

    /// All variable names used anywhere in this expression.
    pub fn variables(&self) -> Vec<&str> {
        let mut vars = Vec::new();
        self.collect_variables(&mut vars);
        vars.sort();
        vars.dedup();
        vars
    }

    /// Collect all `Condition` nodes, flattened.
    pub fn conditions(&self) -> Vec<&Condition> {
        let mut out = Vec::new();
        self.collect_conditions(&mut out);
        out
    }

    fn collect_variables<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            Expression::Condition(c) => {
                out.extend(c.path.variables());
                // Values like "$a" are also variable references.
                if let Some(v) = &c.value {
                    if let Some(var) = v.strip_prefix('$') {
                        if !var.is_empty() {
                            out.push(var);
                        }
                    }
                }
            }
            Expression::And(exprs) | Expression::Or(exprs) => {
                for e in exprs {
                    e.collect_variables(out);
                }
            }
            Expression::Not(inner) => inner.collect_variables(out),
        }
    }

    fn collect_conditions<'a>(&'a self, out: &mut Vec<&'a Condition>) {
        match self {
            Expression::Condition(c) => out.push(c),
            Expression::And(exprs) | Expression::Or(exprs) => {
                for e in exprs {
                    e.collect_conditions(out);
                }
            }
            Expression::Not(inner) => inner.collect_conditions(out),
        }
    }
}


// ---------------------------------------------------------------------------
// Internal parser helpers
// ---------------------------------------------------------------------------

/// Parse an OR-level expression: `A OR B OR C`.
fn parse_or_expr(input: &str) -> Result<Expression, String> {
    let parts = split_top_level(input, "OR")?;
    if parts.len() == 1 {
        return parse_and_expr(parts[0].trim());
    }
    let exprs: Result<Vec<Expression>, String> = parts
        .iter()
        .map(|p| parse_and_expr(p.trim()))
        .collect();
    Ok(Expression::Or(exprs?))
}

/// Parse an AND-level expression: `A AND B AND C`.
fn parse_and_expr(input: &str) -> Result<Expression, String> {
    let parts = split_top_level(input, "AND")?;
    if parts.len() == 1 {
        return parse_unary(parts[0].trim());
    }
    let exprs: Result<Vec<Expression>, String> = parts
        .iter()
        .map(|p| parse_unary(p.trim()))
        .collect();
    Ok(Expression::And(exprs?))
}

/// Parse a unary expression: `NOT X` or an atom (possibly parenthesized).
fn parse_unary(input: &str) -> Result<Expression, String> {
    let input = input.trim();

    // Check for NOT prefix (case-insensitive).
    if input.len() > 3 {
        let prefix = &input[..3];
        let after = input.as_bytes().get(3).copied().unwrap_or(0);
        if prefix.eq_ignore_ascii_case("NOT") && (after == b' ' || after == b'(') {
            let rest = input[3..].trim();
            let inner = parse_unary(rest)?;
            return Ok(Expression::Not(Box::new(inner)));
        }
    }

    // Parenthesized group.
    if input.starts_with('(') {
        let end = find_matching_paren(input)?;
        if end == input.len() - 1 {
            // Entire input is parenthesized — unwrap and re-parse.
            let inner = &input[1..end];
            return parse_or_expr(inner.trim());
        }
        // Otherwise, this is a syntax issue — paren doesn't wrap entire atom.
        return Err(format!(
            "unexpected content after closing paren: '{}'",
            &input[end + 1..]
        ));
    }

    // Atom: a single condition.
    let cond = Condition::parse(input)?;
    Ok(Expression::Condition(cond))
}

/// Split `input` on a top-level keyword (e.g., "AND" or "OR"),
/// respecting parenthesized groups. The keyword match is case-insensitive
/// and must be surrounded by whitespace.
fn split_top_level<'a>(input: &'a str, keyword: &str) -> Result<Vec<&'a str>, String> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    let bytes = input.as_bytes();
    let kw_len = keyword.len();
    let input_len = input.len();
    let mut i = 0;

    while i < input_len {
        let b = bytes[i];
        if b == b'(' {
            depth += 1;
            i += 1;
        } else if b == b')' {
            if depth == 0 {
                return Err("unmatched ')'".into());
            }
            depth -= 1;
            i += 1;
        } else if depth == 0
            && i + kw_len <= input_len
            && i > 0
            && bytes[i - 1] == b' '
            && input[i..i + kw_len].eq_ignore_ascii_case(keyword)
            && (i + kw_len == input_len || bytes[i + kw_len] == b' ')
        {
            parts.push(&input[start..i - 1]); // exclude trailing space
            start = i + kw_len;
            // Skip leading space after keyword.
            if start < input_len && bytes[start] == b' ' {
                start += 1;
            }
            i = start;
        } else {
            i += 1;
        }
    }

    if depth != 0 {
        return Err("unmatched '('".into());
    }

    parts.push(&input[start..]);
    Ok(parts)
}

/// Find the index of the closing ')' that matches the opening '(' at
/// position 0 in `input`.
fn find_matching_paren(input: &str) -> Result<usize, String> {
    debug_assert!(input.starts_with('('));
    let mut depth = 0;
    for (i, ch) in input.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(i);
                }
            }
            _ => {}
        }
    }
    Err("unmatched '('".into())
}

/// Case-insensitive search for `needle` in `haystack`, returning the byte
/// offset of the first occurrence (if any).
fn find_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    let h = haystack.to_ascii_lowercase();
    let n = needle.to_ascii_lowercase();
    h.find(&n)
}

/// Find `keyword` in `input` where it is preceded and followed by
/// whitespace (word boundary). Case-insensitive.
fn find_word_boundary(input: &str, keyword: &str) -> Option<usize> {
    let lower = input.to_ascii_lowercase();
    let kw = keyword.to_ascii_lowercase();
    let mut search_from = 0;
    while let Some(pos) = lower[search_from..].find(&kw) {
        let abs_pos = search_from + pos;
        let before_ok = abs_pos == 0 || input.as_bytes()[abs_pos - 1] == b' ';
        let after_pos = abs_pos + keyword.len();
        let after_ok =
            after_pos >= input.len() || input.as_bytes()[after_pos] == b' ';
        if before_ok && after_ok {
            return Some(abs_pos);
        }
        search_from = abs_pos + 1;
    }
    None
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- PathPattern parsing ---

    #[test]
    fn parse_simple_path() {
        let p = PathPattern::parse("task.AUTH1.status").unwrap();
        assert_eq!(p.segments.len(), 3);
        assert_eq!(p.segments[0], PathSegment::Literal("task".into()));
        assert_eq!(p.segments[1], PathSegment::Literal("AUTH1".into()));
        assert_eq!(p.segments[2], PathSegment::Literal("status".into()));
    }

    #[test]
    fn parse_path_with_variable() {
        let p = PathPattern::parse("task.$t.status").unwrap();
        assert_eq!(p.segments.len(), 3);
        assert_eq!(p.segments[1], PathSegment::Variable("t".into()));
        assert!(p.has_variables());
        assert_eq!(p.variables(), vec!["t"]);
    }

    #[test]
    fn parse_path_with_wildcard() {
        let p = PathPattern::parse("agent.*.health").unwrap();
        assert_eq!(p.segments.len(), 3);
        assert_eq!(p.segments[1], PathSegment::Wildcard);
    }

    #[test]
    fn path_to_string_round_trip() {
        let inputs = &[
            "task.$t.status",
            "agent.*.health",
            "config.global.timeout",
        ];
        for input in inputs {
            let p = PathPattern::parse(input).unwrap();
            assert_eq!(&p.to_string(), input);
        }
    }

    #[test]
    fn error_on_empty_path() {
        assert!(PathPattern::parse("").is_err());
    }

    #[test]
    fn error_on_empty_variable_name() {
        assert!(PathPattern::parse("task.$.status").is_err());
    }

    #[test]
    fn error_on_empty_segment() {
        assert!(PathPattern::parse("task..status").is_err());
    }

    // --- Condition parsing ---

    #[test]
    fn parse_simple_condition_eq() {
        let c = Condition::parse("task.$t.status == complete").unwrap();
        assert_eq!(c.path.to_string(), "task.$t.status");
        assert_eq!(c.operator, Operator::Eq);
        assert_eq!(c.value.as_deref(), Some("complete"));
    }

    #[test]
    fn parse_condition_not_eq() {
        let c = Condition::parse("task.$t.status != cancelled").unwrap();
        assert_eq!(c.operator, Operator::NotEq);
        assert_eq!(c.value.as_deref(), Some("cancelled"));
    }

    #[test]
    fn parse_condition_gt() {
        let c = Condition::parse("agent.$a.retries > 3").unwrap();
        assert_eq!(c.path.to_string(), "agent.$a.retries");
        assert_eq!(c.operator, Operator::Gt);
        assert_eq!(c.value.as_deref(), Some("3"));
    }

    #[test]
    fn parse_condition_lt() {
        let c = Condition::parse("agent.$a.health < 50").unwrap();
        assert_eq!(c.operator, Operator::Lt);
        assert_eq!(c.value.as_deref(), Some("50"));
    }

    #[test]
    fn parse_condition_gte() {
        let c = Condition::parse("task.$t.priority >= 5").unwrap();
        assert_eq!(c.operator, Operator::GtEq);
    }

    #[test]
    fn parse_condition_lte() {
        let c = Condition::parse("task.$t.priority <= 10").unwrap();
        assert_eq!(c.operator, Operator::LtEq);
    }

    #[test]
    fn parse_condition_contains() {
        let c = Condition::parse("agent.$a.tags contains gpu").unwrap();
        assert_eq!(c.operator, Operator::Contains);
        assert_eq!(c.value.as_deref(), Some("gpu"));
    }

    #[test]
    fn parse_condition_is_empty() {
        let c = Condition::parse("task.$t.result is empty").unwrap();
        assert_eq!(c.path.to_string(), "task.$t.result");
        assert_eq!(c.operator, Operator::IsEmpty);
        assert_eq!(c.value, None);
    }

    #[test]
    fn parse_condition_is_not_empty() {
        let c = Condition::parse("task.$t.result is not empty").unwrap();
        assert_eq!(c.operator, Operator::IsNotEmpty);
        assert_eq!(c.value, None);
    }

    #[test]
    fn error_on_unknown_operator() {
        assert!(Condition::parse("task.$t.status LIKE foo").is_err());
    }

    #[test]
    fn error_on_empty_condition() {
        assert!(Condition::parse("").is_err());
    }

    // --- Expression parsing ---

    #[test]
    fn parse_single_condition_expr() {
        let e = Expression::parse("task.$t.status == complete").unwrap();
        match &e {
            Expression::Condition(c) => {
                assert_eq!(c.operator, Operator::Eq);
            }
            _ => panic!("expected Condition, got {:?}", e),
        }
    }

    #[test]
    fn parse_and_expression() {
        let e = Expression::parse(
            "task.$t.status == ready AND agent.$a.status == idle",
        )
        .unwrap();
        match &e {
            Expression::And(parts) => assert_eq!(parts.len(), 2),
            _ => panic!("expected And, got {:?}", e),
        }
    }

    #[test]
    fn parse_three_condition_and() {
        let e = Expression::parse(
            "task.$t.status == ready AND agent.$a.status == idle AND agent.$a.role == worker",
        )
        .unwrap();
        match &e {
            Expression::And(parts) => assert_eq!(parts.len(), 3),
            _ => panic!("expected And, got {:?}", e),
        }
    }

    #[test]
    fn parse_or_expression() {
        let e = Expression::parse(
            "task.$t.status == failed OR task.$t.status == cancelled",
        )
        .unwrap();
        match &e {
            Expression::Or(parts) => assert_eq!(parts.len(), 2),
            _ => panic!("expected Or, got {:?}", e),
        }
    }

    #[test]
    fn parse_not_expression() {
        let e = Expression::parse("NOT task.$t.status == cancelled").unwrap();
        match &e {
            Expression::Not(inner) => match inner.as_ref() {
                Expression::Condition(c) => {
                    assert_eq!(c.value.as_deref(), Some("cancelled"));
                }
                _ => panic!("expected Condition inside Not"),
            },
            _ => panic!("expected Not, got {:?}", e),
        }
    }

    #[test]
    fn parse_parenthesized_expression() {
        let e = Expression::parse(
            "(task.$t.status == ready AND agent.$a.status == idle) OR task.$t.status == urgent",
        )
        .unwrap();
        match &e {
            Expression::Or(parts) => {
                assert_eq!(parts.len(), 2);
                match &parts[0] {
                    Expression::And(inner) => assert_eq!(inner.len(), 2),
                    _ => panic!("expected And inside Or"),
                }
            }
            _ => panic!("expected Or, got {:?}", e),
        }
    }

    #[test]
    fn case_insensitive_connectives() {
        // "and" lowercase
        let e1 = Expression::parse(
            "task.$t.status == a and task.$t.status == b",
        )
        .unwrap();
        assert!(matches!(e1, Expression::And(_)));

        // "or" lowercase
        let e2 = Expression::parse(
            "task.$t.status == a or task.$t.status == b",
        )
        .unwrap();
        assert!(matches!(e2, Expression::Or(_)));

        // "not" lowercase
        let e3 = Expression::parse("not task.$t.status == cancelled").unwrap();
        assert!(matches!(e3, Expression::Not(_)));

        // Mixed case
        let e4 = Expression::parse(
            "task.$t.status == a And task.$t.status == b",
        )
        .unwrap();
        assert!(matches!(e4, Expression::And(_)));
    }

    #[test]
    fn variables_extraction() {
        let e = Expression::parse(
            "task.$t.status == ready AND agent.$a.status == idle",
        )
        .unwrap();
        let vars = e.variables();
        assert_eq!(vars, vec!["a", "t"]);
    }

    #[test]
    fn variables_from_value() {
        let e = Expression::parse("task.$t.agent == $a").unwrap();
        let vars = e.variables();
        assert!(vars.contains(&"t"));
        assert!(vars.contains(&"a"));
    }

    #[test]
    fn conditions_extraction() {
        let e = Expression::parse(
            "task.$t.status == ready AND agent.$a.status == idle AND agent.$a.role == worker",
        )
        .unwrap();
        let conds = e.conditions();
        assert_eq!(conds.len(), 3);
        assert_eq!(conds[0].value.as_deref(), Some("ready"));
        assert_eq!(conds[1].value.as_deref(), Some("idle"));
        assert_eq!(conds[2].value.as_deref(), Some("worker"));
    }

    #[test]
    fn conditions_from_nested() {
        let e = Expression::parse(
            "NOT task.$t.status == cancelled OR task.$t.status == failed",
        )
        .unwrap();
        let conds = e.conditions();
        assert_eq!(conds.len(), 2);
    }

    #[test]
    fn error_on_empty_expression() {
        assert!(Expression::parse("").is_err());
    }

    #[test]
    fn error_on_unmatched_paren() {
        assert!(Expression::parse("(task.$t.status == ready").is_err());
    }

    #[test]
    fn whitespace_tolerance() {
        let c = Condition::parse("  task.$t.status   ==   complete  ").unwrap();
        assert_eq!(c.path.to_string(), "task.$t.status");
        assert_eq!(c.value.as_deref(), Some("complete"));
    }

    #[test]
    fn multiple_variables_in_path() {
        let p = PathPattern::parse("$ns.$id.status").unwrap();
        assert_eq!(p.variables(), vec!["ns", "id"]);
        assert!(p.has_variables());
    }
}
