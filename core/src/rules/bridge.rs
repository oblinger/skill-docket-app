//! Python bridge — decorator registry, inline rules, and markdown
//! extraction for the Python integration layer (M12.2).
//!
//! This module is pure Rust: it parses Python-style `@when` decorators,
//! extracts embedded Python from markdown, and generates Python source.
//! No Python runtime is required.

use super::expr::Expression;
use super::format::{parse_rules_auto, ActionOp, Rule, RuleAction};


// ---------------------------------------------------------------------------
// DecoratorHandler / DecoratorRegistry  (M12.2.1)
// ---------------------------------------------------------------------------

/// A registered Python decorator handler.
#[derive(Debug, Clone)]
pub struct DecoratorHandler {
    /// The original pattern string (e.g., "task.$t.status == complete")
    pub pattern: String,
    /// Parsed expression from the pattern.
    pub expression: Expression,
    /// Python function identifier (module:function_name or just function_name).
    pub handler_id: String,
    /// Variable names extracted from the pattern (for keyword argument mapping).
    pub variables: Vec<String>,
}

/// Registry of `@when` decorator handlers.
#[derive(Debug, Clone, Default)]
pub struct DecoratorRegistry {
    handlers: Vec<DecoratorHandler>,
}

impl DecoratorRegistry {
    pub fn new() -> Self {
        DecoratorRegistry {
            handlers: Vec::new(),
        }
    }

    /// Register a `@when` handler.  Parses the pattern string into an
    /// `Expression` and extracts variable names.  Returns the handler index.
    pub fn register(&mut self, pattern: &str, handler_id: &str) -> Result<usize, String> {
        let expression = Expression::parse(pattern)?;
        let variables = expression
            .variables()
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        let idx = self.handlers.len();
        self.handlers.push(DecoratorHandler {
            pattern: pattern.to_string(),
            expression,
            handler_id: handler_id.to_string(),
            variables,
        });
        Ok(idx)
    }

    /// Get all registered handlers.
    pub fn handlers(&self) -> &[DecoratorHandler] {
        &self.handlers
    }

    /// Convert all registered handlers into `Rule`s suitable for the RETE
    /// engine.  Each handler becomes a rule whose action is
    /// `flow.decorator.$idx.fire = true` — a sentinel that the Python
    /// bridge watches for.
    pub fn to_rules(&self) -> Vec<Rule> {
        self.handlers
            .iter()
            .enumerate()
            .map(|(idx, handler)| Rule {
                name: Some(format!("decorator_{}", handler.handler_id)),
                conditions: handler.expression.clone(),
                actions: vec![RuleAction {
                    path: format!("flow.decorator.{}.fire", idx),
                    operator: ActionOp::Set,
                    value: "true".to_string(),
                }],
                priority: None,
            })
            .collect()
    }

    /// Number of registered handlers.
    pub fn len(&self) -> usize {
        self.handlers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }
}


// ---------------------------------------------------------------------------
// Inline rules parser  (M12.2.2)
// ---------------------------------------------------------------------------

/// Parse inline rules from a string (as would be passed to `rules("...")`
/// in Python) and return the parsed `Rule` vector.  Delegates to
/// `parse_rules_auto`.
pub fn parse_inline_rules(input: &str) -> Result<Vec<Rule>, String> {
    parse_rules_auto(input)
}


// ---------------------------------------------------------------------------
// Markdown Python extraction  (M12.2.3)
// ---------------------------------------------------------------------------

/// A piece of Python extracted from a markdown Rules section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractedPython {
    /// A `@when` decorator with its handler function.
    Decorator {
        pattern: String,
        function_name: String,
        function_body: String,
        /// Parameters extracted from the `def` line (variable names).
        parameters: Vec<String>,
    },
    /// An inline `rules("...")` call with the raw rules text.
    InlineRules { rules_text: String },
    /// Bare rule text that should be parsed as declarative rules.
    BareRules { rules_text: String },
}

/// Result of scanning a markdown document for Rules sections.
#[derive(Debug, Clone)]
pub struct MarkdownExtraction {
    /// All Python fragments found, in document order.
    pub fragments: Vec<ExtractedPython>,
    /// The source file path (if provided).
    pub source: Option<String>,
}

/// Scan markdown text for Rules sections and extract Python fragments.
pub fn extract_python_from_markdown(content: &str) -> MarkdownExtraction {
    let lines: Vec<&str> = content.lines().collect();
    let mut fragments = Vec::new();

    // Find Rules section boundaries.
    let sections = find_rules_sections(&lines);

    for (start, end) in sections {
        extract_from_section(&lines[start..end], &mut fragments);
    }

    MarkdownExtraction {
        fragments,
        source: None,
    }
}

/// Generate a Python source file from extracted fragments.
///
/// The output is valid Python that, when run, registers all decorators
/// and loads all inline rules via the `cmx` package.
pub fn generate_python_source(extraction: &MarkdownExtraction) -> String {
    let mut out = String::new();

    // Header
    if let Some(src) = &extraction.source {
        out.push_str(&format!("# Auto-generated from {}\n", src));
    } else {
        out.push_str("# Auto-generated by CMX\n");
    }
    out.push_str("import cmx\n\n");

    for fragment in &extraction.fragments {
        match fragment {
            ExtractedPython::Decorator {
                pattern,
                function_name,
                function_body,
                parameters,
            } => {
                out.push_str(&format!("@cmx.when(\"{}\")\n", pattern));
                out.push_str(&format!(
                    "def {}({}):\n",
                    function_name,
                    parameters.join(", ")
                ));
                for line in function_body.lines() {
                    out.push_str(&format!("    {}\n", line));
                }
                out.push('\n');
            }
            ExtractedPython::InlineRules { rules_text } => {
                out.push_str("cmx.rules(\"\"\"\n");
                for line in rules_text.lines() {
                    out.push_str(line);
                    out.push('\n');
                }
                out.push_str("\"\"\")\n\n");
            }
            ExtractedPython::BareRules { rules_text } => {
                out.push_str("cmx.rules(\"\"\"\n");
                for line in rules_text.lines() {
                    out.push_str(line);
                    out.push('\n');
                }
                out.push_str("\"\"\")\n\n");
            }
        }
    }

    out
}


// ---------------------------------------------------------------------------
// Internal: section finding
// ---------------------------------------------------------------------------

/// Return `(start, end)` line-index pairs for each Rules section body.
/// `start` is the first line AFTER the heading; `end` is exclusive.
fn find_rules_sections(lines: &[&str]) -> Vec<(usize, usize)> {
    let mut sections = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();
        if let Some(level) = heading_level(trimmed) {
            if is_rules_heading(trimmed) {
                // Collect content until next heading of equal or higher level.
                let body_start = i + 1;
                let mut body_end = lines.len();
                for j in body_start..lines.len() {
                    let t = lines[j].trim();
                    if let Some(next_level) = heading_level(t) {
                        if next_level <= level {
                            body_end = j;
                            break;
                        }
                    }
                }
                sections.push((body_start, body_end));
                i = body_end;
                continue;
            }
        }
        i += 1;
    }

    sections
}

/// Return the heading level (number of `#` characters) if the line is an
/// ATX heading, or `None` otherwise.
fn heading_level(line: &str) -> Option<usize> {
    let trimmed = line.trim();
    if !trimmed.starts_with('#') {
        return None;
    }
    let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
    // Must be followed by a space or end of line.
    if hashes == trimmed.len() {
        return Some(hashes);
    }
    if trimmed.as_bytes().get(hashes) == Some(&b' ') {
        return Some(hashes);
    }
    None
}

/// Check if an ATX heading is a "Rules" heading (case-insensitive).
fn is_rules_heading(line: &str) -> bool {
    let trimmed = line.trim();
    // Strip leading '#' characters and whitespace.
    let text = trimmed.trim_start_matches('#').trim();
    text.eq_ignore_ascii_case("rules")
}


// ---------------------------------------------------------------------------
// Internal: fragment extraction
// ---------------------------------------------------------------------------

/// Extract Python fragments from lines within a Rules section body.
fn extract_from_section(lines: &[&str], fragments: &mut Vec<ExtractedPython>) {
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        // Skip blanks and comments.
        if trimmed.is_empty() || trimmed.starts_with("<!--") {
            i += 1;
            continue;
        }

        // @when decorator?
        if trimmed.starts_with("@when(") {
            if let Some((fragment, consumed)) = parse_decorator(&lines[i..]) {
                fragments.push(fragment);
                i += consumed;
                continue;
            }
        }

        // rules(""" ...) or rules(''' ...)?
        if trimmed.starts_with("rules(\"\"\"") || trimmed.starts_with("rules('''") {
            if let Some((fragment, consumed)) = parse_inline_rules_call(&lines[i..]) {
                fragments.push(fragment);
                i += consumed;
                continue;
            }
        }

        // Otherwise: bare rule text. Collect consecutive non-blank,
        // non-decorator, non-rules() lines.
        let start = i;
        while i < lines.len() {
            let t = lines[i].trim();
            if t.is_empty() {
                break;
            }
            if t.starts_with("@when(") || t.starts_with("rules(\"\"\"") || t.starts_with("rules('''") {
                break;
            }
            // Skip markdown-style comments within bare rules.
            if t.starts_with("<!--") {
                break;
            }
            i += 1;
        }
        if i > start {
            let text: Vec<&str> = lines[start..i]
                .iter()
                .map(|l| l.trim())
                .collect();
            fragments.push(ExtractedPython::BareRules {
                rules_text: text.join("\n"),
            });
        }
    }
}

/// Parse a `@when("pattern")` decorator and its following `def` + body.
/// Returns `(fragment, lines_consumed)`.
fn parse_decorator(lines: &[&str]) -> Option<(ExtractedPython, usize)> {
    let first = lines[0].trim();

    // Extract pattern from @when("pattern") or @when('pattern').
    let pattern = extract_when_pattern(first)?;

    // Next non-blank line should be a `def`.
    let mut def_idx = 1;
    while def_idx < lines.len() && lines[def_idx].trim().is_empty() {
        def_idx += 1;
    }
    if def_idx >= lines.len() {
        return None;
    }

    let def_line = lines[def_idx].trim();
    let (func_name, params) = parse_def_line(def_line)?;

    // Collect indented body lines after the def.
    let mut body_lines = Vec::new();
    let mut j = def_idx + 1;
    while j < lines.len() {
        let raw = lines[j];
        // Body lines are indented (start with whitespace) or blank.
        if raw.trim().is_empty() {
            // Blank line might be part of function body — peek ahead.
            if j + 1 < lines.len() && is_indented(lines[j + 1]) {
                body_lines.push("");
                j += 1;
                continue;
            }
            break;
        }
        if is_indented(raw) {
            body_lines.push(raw.trim());
            j += 1;
        } else {
            break;
        }
    }

    let function_body = body_lines.join("\n");

    Some((
        ExtractedPython::Decorator {
            pattern,
            function_name: func_name,
            function_body,
            parameters: params,
        },
        j,
    ))
}

/// Extract the pattern string from `@when("pattern")` or `@when('pattern')`.
fn extract_when_pattern(line: &str) -> Option<String> {
    let line = line.trim();
    let rest = line.strip_prefix("@when(")?;
    let rest = rest.strip_suffix(')')?;
    let rest = rest.trim();
    // Strip quotes (single or double).
    if (rest.starts_with('"') && rest.ends_with('"'))
        || (rest.starts_with('\'') && rest.ends_with('\''))
    {
        Some(rest[1..rest.len() - 1].to_string())
    } else {
        None
    }
}

/// Parse `def function_name(param1, param2):` and return (name, params).
fn parse_def_line(line: &str) -> Option<(String, Vec<String>)> {
    let line = line.trim();
    let rest = line.strip_prefix("def ")?;
    let paren_start = rest.find('(')?;
    let paren_end = rest.find(')')?;
    let name = rest[..paren_start].trim().to_string();
    let params_str = &rest[paren_start + 1..paren_end];
    let params: Vec<String> = params_str
        .split(',')
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    Some((name, params))
}

/// Check if a line starts with whitespace (is indented).
fn is_indented(line: &str) -> bool {
    line.starts_with(' ') || line.starts_with('\t')
}

/// Parse a `rules("""...""")` or `rules('''...''')` block.
/// Returns `(fragment, lines_consumed)`.
fn parse_inline_rules_call(lines: &[&str]) -> Option<(ExtractedPython, usize)> {
    let first = lines[0].trim();

    // Determine which quote style.
    let (delim, opener) = if first.starts_with("rules(\"\"\"") {
        ("\"\"\"", "rules(\"\"\"")
    } else if first.starts_with("rules('''") {
        ("'''", "rules('''")
    } else {
        return None;
    };

    let closing = format!("{})", delim);

    // Check if it's all on one line.
    let after_open = &first[opener.len()..];
    if let Some(close_pos) = after_open.find(&closing) {
        let text = &after_open[..close_pos];
        return Some((
            ExtractedPython::InlineRules {
                rules_text: text.to_string(),
            },
            1,
        ));
    }

    // Multi-line: collect until closing delimiter.
    let mut body_lines = Vec::new();
    // First line might have text after the opening delimiter.
    if !after_open.trim().is_empty() {
        body_lines.push(after_open.trim());
    }

    let mut j = 1;
    while j < lines.len() {
        let raw = lines[j].trim();
        if raw.contains(&closing) {
            // Grab text before the closing delimiter.
            if let Some(pos) = raw.find(&closing) {
                let before = &raw[..pos];
                if !before.trim().is_empty() {
                    body_lines.push(before.trim());
                }
            }
            j += 1;
            break;
        }
        body_lines.push(raw);
        j += 1;
    }

    Some((
        ExtractedPython::InlineRules {
            rules_text: body_lines.join("\n"),
        },
        j,
    ))
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::expr::Operator;

    // 1. Register decorator — pattern parses, variables extracted.
    #[test]
    fn register_decorator() {
        let mut reg = DecoratorRegistry::new();
        let idx = reg
            .register("task.$t.status == complete", "on_complete")
            .unwrap();
        assert_eq!(idx, 0);
        assert_eq!(reg.len(), 1);

        let h = &reg.handlers()[0];
        assert_eq!(h.pattern, "task.$t.status == complete");
        assert_eq!(h.handler_id, "on_complete");
        assert_eq!(h.variables, vec!["t"]);

        // Verify the expression parsed correctly.
        let conds = h.expression.conditions();
        assert_eq!(conds.len(), 1);
        assert_eq!(conds[0].operator, Operator::Eq);
        assert_eq!(conds[0].value.as_deref(), Some("complete"));
    }

    // 2. Decorator to rules — correct RETE rule structure.
    #[test]
    fn decorator_to_rules() {
        let mut reg = DecoratorRegistry::new();
        reg.register("task.$t.status == complete", "on_complete")
            .unwrap();

        let rules = reg.to_rules();
        assert_eq!(rules.len(), 1);
        let r = &rules[0];
        assert_eq!(r.name.as_deref(), Some("decorator_on_complete"));
        assert_eq!(r.actions.len(), 1);
        assert_eq!(r.actions[0].path, "flow.decorator.0.fire");
        assert_eq!(r.actions[0].value, "true");
        assert_eq!(r.actions[0].operator, ActionOp::Set);
    }

    // 3. Multiple decorators — all converted correctly.
    #[test]
    fn multiple_decorators() {
        let mut reg = DecoratorRegistry::new();
        reg.register("task.$t.status == complete", "on_complete")
            .unwrap();
        reg.register(
            "agent.$a.health == unhealthy",
            "on_unhealthy",
        )
        .unwrap();
        reg.register(
            "task.$t.status == failed AND task.$t.retries > 3",
            "on_give_up",
        )
        .unwrap();
        assert_eq!(reg.len(), 3);
        assert!(!reg.is_empty());

        let rules = reg.to_rules();
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[0].actions[0].path, "flow.decorator.0.fire");
        assert_eq!(rules[1].actions[0].path, "flow.decorator.1.fire");
        assert_eq!(rules[2].actions[0].path, "flow.decorator.2.fire");
    }

    // 4. Inline rules parse — arrow format.
    #[test]
    fn inline_rules_parse_arrow() {
        let input = "task.$t.status == ready --> task.$t.status = in_progress";
        let rules = parse_inline_rules(input).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].actions[0].value, "in_progress");
    }

    // 5. Inline rules auto-detect — table format.
    #[test]
    fn inline_rules_auto_detect_table() {
        let input = "\
| When | Then |
|------|------|
| task.$t.status == ready | task.$t.status = in_progress |";
        let rules = parse_inline_rules(input).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].actions[0].value, "in_progress");
    }

    // 6. Extract decorator from markdown.
    #[test]
    fn extract_decorator_from_markdown() {
        let md = "\
# Project Spec

Some intro text.

## Rules

@when(\"task.$t.status == complete\")
def on_complete(t):
    print(f\"Task {t} done\")

## Other Section

Ignored content.
";
        let extraction = extract_python_from_markdown(md);
        assert_eq!(extraction.fragments.len(), 1);
        match &extraction.fragments[0] {
            ExtractedPython::Decorator {
                pattern,
                function_name,
                function_body,
                parameters,
            } => {
                assert_eq!(pattern, "task.$t.status == complete");
                assert_eq!(function_name, "on_complete");
                assert_eq!(parameters, &["t".to_string()]);
                assert!(function_body.contains("print"));
            }
            other => panic!("expected Decorator, got {:?}", other),
        }
    }

    // 7. Extract inline rules from markdown.
    #[test]
    fn extract_inline_rules_from_markdown() {
        let md = "\
## Rules

rules(\"\"\"
task.$t.status == ready --> task.$t.status = in_progress
\"\"\")
";
        let extraction = extract_python_from_markdown(md);
        assert_eq!(extraction.fragments.len(), 1);
        match &extraction.fragments[0] {
            ExtractedPython::InlineRules { rules_text } => {
                assert!(rules_text.contains("-->"));
                assert!(rules_text.contains("in_progress"));
            }
            other => panic!("expected InlineRules, got {:?}", other),
        }
    }

    // 8. Extract bare rules from markdown.
    #[test]
    fn extract_bare_rules_from_markdown() {
        let md = "\
## Rules

task.$t.status == ready --> task.$t.status = in_progress
agent.$a.health == unhealthy --> agent.$a.status = error
";
        let extraction = extract_python_from_markdown(md);
        assert_eq!(extraction.fragments.len(), 1);
        match &extraction.fragments[0] {
            ExtractedPython::BareRules { rules_text } => {
                assert!(rules_text.contains("-->"));
                assert!(rules_text.contains("in_progress"));
                assert!(rules_text.contains("error"));
            }
            other => panic!("expected BareRules, got {:?}", other),
        }
    }

    // 9. Extract mixed content — correct order.
    #[test]
    fn extract_mixed_content() {
        let md = "\
## Rules

@when(\"task.$t.status == complete\")
def on_complete(t):
    print(t)

task.$t.status == ready --> task.$t.status = in_progress

rules(\"\"\"
agent.$a.health == unhealthy --> agent.$a.status = error
\"\"\")
";
        let extraction = extract_python_from_markdown(md);
        assert_eq!(extraction.fragments.len(), 3);
        assert!(matches!(extraction.fragments[0], ExtractedPython::Decorator { .. }));
        assert!(matches!(extraction.fragments[1], ExtractedPython::BareRules { .. }));
        assert!(matches!(extraction.fragments[2], ExtractedPython::InlineRules { .. }));
    }

    // 10. Generate Python source.
    #[test]
    fn generate_python_source_output() {
        let extraction = MarkdownExtraction {
            fragments: vec![
                ExtractedPython::Decorator {
                    pattern: "task.$t.status == complete".to_string(),
                    function_name: "on_complete".to_string(),
                    function_body: "print(f\"Task {t} done\")".to_string(),
                    parameters: vec!["t".to_string()],
                },
                ExtractedPython::BareRules {
                    rules_text: "task.$t.status == ready --> task.$t.status = in_progress"
                        .to_string(),
                },
            ],
            source: Some("project.md".to_string()),
        };
        let py = generate_python_source(&extraction);

        assert!(py.contains("# Auto-generated from project.md"));
        assert!(py.contains("import cmx"));
        assert!(py.contains("@cmx.when(\"task.$t.status == complete\")"));
        assert!(py.contains("def on_complete(t):"));
        assert!(py.contains("cmx.rules(\"\"\""));
        assert!(py.contains("in_progress"));
    }

    // 11. Rules section detection — only content under ## Rules is extracted.
    #[test]
    fn rules_section_detection() {
        let md = "\
## Introduction

task.$t.status == ready --> task.$t.status = ignored

## Rules

task.$t.status == ready --> task.$t.status = extracted

## Deployment

More ignored content.
";
        let extraction = extract_python_from_markdown(md);
        assert_eq!(extraction.fragments.len(), 1);
        match &extraction.fragments[0] {
            ExtractedPython::BareRules { rules_text } => {
                assert!(rules_text.contains("extracted"));
                assert!(!rules_text.contains("ignored"));
            }
            other => panic!("expected BareRules, got {:?}", other),
        }
    }

    // 12. Nested heading stops extraction.
    #[test]
    fn nested_heading_stops_extraction() {
        let md = "\
### Rules

task.$t.status == ready --> task.$t.status = in_rules

### Other

task.$t.status == ready --> task.$t.status = not_in_rules

## Higher Level

Also not in rules.
";
        let extraction = extract_python_from_markdown(md);
        assert_eq!(extraction.fragments.len(), 1);
        match &extraction.fragments[0] {
            ExtractedPython::BareRules { rules_text } => {
                assert!(rules_text.contains("in_rules"));
                assert!(!rules_text.contains("not_in_rules"));
            }
            other => panic!("expected BareRules, got {:?}", other),
        }
    }
}
