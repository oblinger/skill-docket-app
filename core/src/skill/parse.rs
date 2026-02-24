use crate::skill::types::*;

/// Parse a SKILL.md document from its text content.
pub fn parse_skill(content: &str) -> Result<SkillDocument, SkillParseError> {
    let (frontmatter, body) = extract_frontmatter(content)?;
    let (tables, instructions) = extract_tables_and_instructions(&body);

    let mut fields: Option<FieldsTable> = None;
    let mut lifecycle: Option<LifecycleTable> = None;
    let mut nodes: Option<NodesTable> = None;
    let mut edges: Option<EdgesTable> = None;

    for table in &tables {
        if table.header.is_empty() {
            continue;
        }
        let tag = table.header[0].trim().to_lowercase();
        if tag.contains("fields") && fields.is_none() {
            fields = Some(parse_fields_table(table)?);
        } else if tag.contains("lifecycle") && lifecycle.is_none() {
            lifecycle = Some(parse_lifecycle_table(table)?);
        } else if tag.contains("nodes") && nodes.is_none() {
            nodes = Some(parse_nodes_table(table)?);
        } else if tag.contains("edges") && edges.is_none() {
            edges = Some(parse_edges_table(table)?);
        }
        // unrecognized tables are silently skipped
    }

    Ok(SkillDocument {
        frontmatter,
        fields,
        lifecycle,
        nodes,
        edges,
        instructions,
    })
}

// ---------------------------------------------------------------------------
// Frontmatter extraction
// ---------------------------------------------------------------------------

fn extract_frontmatter(content: &str) -> Result<(Frontmatter, String), SkillParseError> {
    let trimmed = content.trim_start();

    // Detect opener
    let delimiter = if trimmed.starts_with("---") {
        "---"
    } else if trimmed.starts_with("~~~") {
        "~~~"
    } else {
        // No frontmatter
        return Ok((Frontmatter::default(), content.to_string()));
    };

    // Find the opening line end
    let after_open = match trimmed.find('\n') {
        Some(pos) => pos + 1,
        None => return Ok((Frontmatter::default(), content.to_string())),
    };

    // Find closing delimiter
    let rest = &trimmed[after_open..];
    let close_pos = rest
        .find(&format!("\n{}", delimiter))
        .or_else(|| {
            // Handle case where closing delimiter is at the start of rest
            if rest.starts_with(delimiter) {
                Some(0)
            } else {
                None
            }
        });

    let (yaml_text, body_start) = match close_pos {
        Some(pos) if rest[pos..].starts_with(delimiter) => {
            // Closing delimiter at the very start of rest
            let yaml = &rest[..pos];
            let after_close = pos + delimiter.len();
            let skip_newline = if rest[after_close..].starts_with('\n') {
                1
            } else {
                0
            };
            (yaml, after_open + after_close + skip_newline)
        }
        Some(pos) => {
            let yaml = &rest[..pos];
            // +1 for the \n before delimiter
            let after_close = pos + 1 + delimiter.len();
            let skip_newline = if rest.len() > after_close && rest.as_bytes()[after_close] == b'\n'
            {
                1
            } else {
                0
            };
            (yaml, after_open + after_close + skip_newline)
        }
        None => {
            return Err(SkillParseError::InvalidFrontmatter(
                "unclosed frontmatter delimiter".to_string(),
            ));
        }
    };

    let yaml_str = yaml_text.trim();
    if yaml_str.is_empty() {
        return Ok((Frontmatter::default(), trimmed[body_start..].to_string()));
    }

    let raw: FrontmatterRaw = serde_yaml::from_str(yaml_str)
        .map_err(|e| SkillParseError::InvalidFrontmatter(e.to_string()))?;

    Ok((raw.into(), trimmed[body_start..].to_string()))
}

// ---------------------------------------------------------------------------
// Table extraction
// ---------------------------------------------------------------------------

struct RawTable {
    header: Vec<String>,
    _header_line: usize,
    rows: Vec<Vec<String>>,
}

fn split_table_row(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    // Strip leading and trailing pipes
    let inner = if trimmed.starts_with('|') && trimmed.ends_with('|') {
        &trimmed[1..trimmed.len() - 1]
    } else if trimmed.starts_with('|') {
        &trimmed[1..]
    } else if trimmed.ends_with('|') {
        &trimmed[..trimmed.len() - 1]
    } else {
        trimmed
    };
    inner.split('|').map(|s| s.trim().to_string()).collect()
}

fn is_separator_row(line: &str) -> bool {
    let cells = split_table_row(line);
    if cells.is_empty() {
        return false;
    }
    cells.iter().all(|c| {
        let t = c.trim().trim_matches(':');
        !t.is_empty() && t.chars().all(|ch| ch == '-')
    })
}

fn extract_tables_and_instructions(body: &str) -> (Vec<RawTable>, String) {
    let lines: Vec<&str> = body.lines().collect();
    let mut tables: Vec<RawTable> = Vec::new();
    let mut instruction_parts: Vec<String> = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];

        // Check if this line starts a table (starts with |)
        if line.trim_start().starts_with('|') {
            // We need at least a header and separator row
            if i + 1 < lines.len()
                && lines[i + 1].trim_start().starts_with('|')
                && is_separator_row(lines[i + 1])
            {
                let header = split_table_row(line);
                let header_line = i;
                i += 2; // skip header and separator

                let mut rows: Vec<Vec<String>> = Vec::new();
                while i < lines.len() && lines[i].trim_start().starts_with('|') {
                    if !is_separator_row(lines[i]) {
                        rows.push(split_table_row(lines[i]));
                    }
                    i += 1;
                }

                tables.push(RawTable {
                    header,
                    _header_line: header_line,
                    rows,
                });
                continue;
            }
        }

        // Not a table line — it's instruction content
        instruction_parts.push(line.to_string());
        i += 1;
    }

    // Join instruction lines and trim leading/trailing blank lines
    let instructions = instruction_parts.join("\n").trim().to_string();
    (tables, instructions)
}

// ---------------------------------------------------------------------------
// Table parsers
// ---------------------------------------------------------------------------

fn parse_fields_table(table: &RawTable) -> Result<FieldsTable, SkillParseError> {
    // Detect column layout from headers
    let headers_lower: Vec<String> = table.header.iter().map(|h| h.to_lowercase()).collect();

    let has_merge = headers_lower.iter().any(|h| h.contains("merge"));
    let has_default = headers_lower.iter().any(|h| h.contains("default"));

    let fields = table
        .rows
        .iter()
        .map(|row| {
            match (has_merge, has_default) {
                (true, true) => {
                    // 5 columns: Fields | Type | Merge | Default | Description
                    FieldDef {
                        name: row.get(0).cloned().unwrap_or_default(),
                        field_type: row.get(1).cloned().unwrap_or_default(),
                        merge: normalize_dash(row.get(2).map(|s| s.as_str())),
                        default: normalize_dash(row.get(3).map(|s| s.as_str())),
                        description: row.get(4).cloned().unwrap_or_default(),
                    }
                }
                (true, false) => {
                    // 4 columns: Fields | Type | Merge | Description
                    FieldDef {
                        name: row.get(0).cloned().unwrap_or_default(),
                        field_type: row.get(1).cloned().unwrap_or_default(),
                        merge: normalize_dash(row.get(2).map(|s| s.as_str())),
                        default: None,
                        description: row.get(3).cloned().unwrap_or_default(),
                    }
                }
                (false, true) => {
                    // 4 columns: Fields | Type | Default | Description
                    FieldDef {
                        name: row.get(0).cloned().unwrap_or_default(),
                        field_type: row.get(1).cloned().unwrap_or_default(),
                        merge: None,
                        default: normalize_dash(row.get(2).map(|s| s.as_str())),
                        description: row.get(3).cloned().unwrap_or_default(),
                    }
                }
                (false, false) => {
                    // Minimal: Fields | Type | Description
                    FieldDef {
                        name: row.get(0).cloned().unwrap_or_default(),
                        field_type: row.get(1).cloned().unwrap_or_default(),
                        merge: None,
                        default: None,
                        description: row.get(2).cloned().unwrap_or_default(),
                    }
                }
            }
        })
        .collect();

    Ok(FieldsTable { fields })
}

/// Normalize a cell value: treat `—`, `-`, and empty as None.
fn normalize_dash(val: Option<&str>) -> Option<String> {
    match val {
        None => None,
        Some(s) => {
            let t = s.trim();
            if t.is_empty() || t == "—" || t == "-" || t == "\u{2014}" {
                None
            } else {
                Some(t.to_string())
            }
        }
    }
}

fn parse_lifecycle_table(table: &RawTable) -> Result<LifecycleTable, SkillParseError> {
    let states = table
        .rows
        .iter()
        .map(|row| LifecycleState {
            name: row.get(0).cloned().unwrap_or_default(),
            description: row.get(1).cloned().unwrap_or_default(),
        })
        .collect();

    Ok(LifecycleTable { states })
}

fn parse_nodes_table(table: &RawTable) -> Result<NodesTable, SkillParseError> {
    let nodes = table
        .rows
        .iter()
        .map(|row| NodeDef {
            name: row.get(0).cloned().unwrap_or_default(),
            role: row.get(1).cloned().unwrap_or_default(),
            description: row.get(2).cloned().unwrap_or_default(),
        })
        .collect();

    Ok(NodesTable { nodes })
}

fn parse_edges_table(table: &RawTable) -> Result<EdgesTable, SkillParseError> {
    let mut edges = Vec::new();

    for row in &table.rows {
        let from_str = row.get(0).map(|s| s.as_str()).unwrap_or("");
        let to_str = row.get(1).map(|s| s.as_str()).unwrap_or("");
        let cond_str = row.get(2).map(|s| s.as_str()).unwrap_or("");

        let from = parse_edge_endpoint(from_str)?;
        let to = parse_edge_endpoint(to_str)?;
        let condition = normalize_dash(Some(cond_str));

        edges.push(EdgeDef {
            from,
            to,
            condition,
        });
    }

    Ok(EdgesTable { edges })
}

fn parse_edge_endpoint(s: &str) -> Result<EdgeEndpoint, SkillParseError> {
    let s = s.trim();

    if s.eq_ignore_ascii_case("START") {
        return Ok(EdgeEndpoint::Start);
    }
    if s.eq_ignore_ascii_case("END") {
        return Ok(EdgeEndpoint::End);
    }
    if s.eq_ignore_ascii_case("WAIT") {
        return Ok(EdgeEndpoint::Wait);
    }

    // Parallel: [A, B, C]
    if s.starts_with('[') && s.ends_with(']') {
        let inner = &s[1..s.len() - 1];
        let names: Vec<String> = inner.split(',').map(|n| n.trim().to_string()).collect();
        return Ok(EdgeEndpoint::Parallel(names));
    }

    // Dynamic fan-out: each(field) → Node  or  each(field) -> Node
    if s.starts_with("each(") {
        // Find the closing paren
        if let Some(paren_end) = s.find(')') {
            let field = s[5..paren_end].trim().to_string();
            // Find the arrow: → or ->
            let after_paren = &s[paren_end + 1..];
            let node_part = after_paren
                .trim()
                .trim_start_matches('→')
                .trim_start_matches("->")
                .trim();
            if !node_part.is_empty() {
                return Ok(EdgeEndpoint::DynamicFanOut {
                    field,
                    node: node_part.to_string(),
                });
            }
        }
        return Err(SkillParseError::InvalidEdgeEndpoint(s.to_string()));
    }

    // Plain node name
    if s.is_empty() {
        return Err(SkillParseError::InvalidEdgeEndpoint(
            "empty endpoint".to_string(),
        ));
    }

    Ok(EdgeEndpoint::Node(s.to_string()))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Test 1: Simple skill — frontmatter + instructions only
    #[test]
    fn test_simple_skill() {
        let input = r#"---
name: hello-world
description: A simple skill
---

# Instructions

Do the thing.
"#;
        let doc = parse_skill(input).unwrap();
        assert_eq!(doc.frontmatter.name.as_deref(), Some("hello-world"));
        assert_eq!(doc.frontmatter.description.as_deref(), Some("A simple skill"));
        assert_eq!(doc.kind(), SkillKind::Simple);
        assert!(doc.fields.is_none());
        assert!(doc.nodes.is_none());
        assert!(doc.edges.is_none());
        assert!(doc.instructions.contains("Do the thing."));
    }

    // Test 2: Structured skill with Fields and Lifecycle
    #[test]
    fn test_structured_skill_with_fields_and_lifecycle() {
        let input = r#"---
name: task-template
description: A structured template
---

Some instructions here.

| Fields | Type | Default | Description |
|--------|------|---------|-------------|
| title | string | untitled | The task title |
| priority | int | 5 | Priority level |

| Lifecycle | Description |
|-----------|-------------|
| draft | Initial creation |
| active | Being worked on |
| done | Completed |

More instructions.
"#;
        let doc = parse_skill(input).unwrap();
        assert_eq!(doc.kind(), SkillKind::Structured);

        let fields = doc.fields.unwrap();
        assert_eq!(fields.fields.len(), 2);
        assert_eq!(fields.fields[0].name, "title");
        assert_eq!(fields.fields[0].field_type, "string");
        assert_eq!(fields.fields[0].default.as_deref(), Some("untitled"));
        assert_eq!(fields.fields[1].name, "priority");

        let lifecycle = doc.lifecycle.unwrap();
        assert_eq!(lifecycle.states.len(), 3);
        assert_eq!(lifecycle.states[0].name, "draft");
        assert_eq!(lifecycle.states[2].name, "done");
    }

    // Test 3: Orchestration skill with all edge types
    #[test]
    fn test_orchestration_with_all_edge_types() {
        let input = r#"---
name: complex-flow
description: Tests all edge types
---

| Fields | Type | Merge | Description |
|--------|------|-------|-------------|
| items | list | append | Work items |

| Nodes | Role | Description |
|-------|------|-------------|
| plan | pm | Plan the work |
| exec | worker | Execute a subtask |
| review | pm | Review results |
| deploy | worker | Deploy |

| Edges | To | Condition |
|-------|-----|-----------|
| START | plan | — |
| plan | [review, deploy] | — |
| [review, deploy] | exec | — |
| plan | each(items) → exec | — |
| exec | WAIT | — |
| WAIT | END | approved = "yes" |
"#;
        let doc = parse_skill(input).unwrap();
        assert_eq!(doc.kind(), SkillKind::Orchestration);

        let edges = doc.edges.unwrap();
        assert_eq!(edges.edges.len(), 6);

        // START
        assert_eq!(edges.edges[0].from, EdgeEndpoint::Start);
        assert_eq!(edges.edges[0].to, EdgeEndpoint::Node("plan".to_string()));
        assert!(edges.edges[0].condition.is_none());

        // Parallel
        assert_eq!(
            edges.edges[1].to,
            EdgeEndpoint::Parallel(vec!["review".to_string(), "deploy".to_string()])
        );

        // Parallel as source
        assert_eq!(
            edges.edges[2].from,
            EdgeEndpoint::Parallel(vec!["review".to_string(), "deploy".to_string()])
        );

        // DynamicFanOut
        assert_eq!(
            edges.edges[3].to,
            EdgeEndpoint::DynamicFanOut {
                field: "items".to_string(),
                node: "exec".to_string(),
            }
        );

        // WAIT
        assert_eq!(edges.edges[4].to, EdgeEndpoint::Wait);
        assert_eq!(edges.edges[5].from, EdgeEndpoint::Wait);
        assert_eq!(edges.edges[5].to, EdgeEndpoint::End);
        assert_eq!(
            edges.edges[5].condition.as_deref(),
            Some("approved = \"yes\"")
        );
    }

    // Test 4: Tilde frontmatter delimiters
    #[test]
    fn test_tilde_frontmatter() {
        let input = "~~~\nname: tilde-skill\ndescription: Uses tildes\n~~~\n\nSome instructions.\n";
        let doc = parse_skill(input).unwrap();
        assert_eq!(doc.frontmatter.name.as_deref(), Some("tilde-skill"));
        assert!(doc.instructions.contains("Some instructions."));
    }

    // Test 5: No frontmatter
    #[test]
    fn test_no_frontmatter() {
        let input = "# Just Instructions\n\nDo stuff.\n";
        let doc = parse_skill(input).unwrap();
        assert!(doc.frontmatter.name.is_none());
        assert!(doc.frontmatter.description.is_none());
        assert_eq!(doc.kind(), SkillKind::Simple);
        assert!(doc.instructions.contains("Just Instructions"));
    }

    // Test 6: Tables in mixed order (Edges before Fields)
    #[test]
    fn test_mixed_table_order() {
        let input = r#"---
name: mixed-order
---

| Edges | To | Condition |
|-------|-----|-----------|
| START | work | — |
| work | END | — |

| Fields | Type | Description |
|--------|------|-------------|
| data | string | Some data |

| Nodes | Role | Description |
|-------|------|-------------|
| work | worker | Do work |
"#;
        let doc = parse_skill(input).unwrap();
        assert_eq!(doc.kind(), SkillKind::Orchestration);
        assert!(doc.fields.is_some());
        assert!(doc.nodes.is_some());
        assert!(doc.edges.is_some());
        assert_eq!(doc.edges.as_ref().unwrap().edges.len(), 2);
        assert_eq!(doc.fields.as_ref().unwrap().fields[0].name, "data");
        assert_eq!(doc.nodes.as_ref().unwrap().nodes[0].name, "work");
    }

    // Test 7: Unrecognized tables are skipped
    #[test]
    fn test_unrecognized_table_skipped() {
        let input = r#"---
name: skip-unknown
---

| RandomTable | Column2 |
|-------------|---------|
| foo | bar |

| Fields | Type | Description |
|--------|------|-------------|
| x | string | A field |
"#;
        let doc = parse_skill(input).unwrap();
        assert!(doc.fields.is_some());
        assert_eq!(doc.fields.as_ref().unwrap().fields.len(), 1);
        // The random table should not cause an error
    }

    // Test 8: Instructions contain all non-table content
    #[test]
    fn test_instructions_content() {
        let input = r#"---
name: instruction-test
---

# Header One

Paragraph before table.

| Fields | Type | Description |
|--------|------|-------------|
| x | string | A field |

Paragraph after table.

## Header Two

More text.
"#;
        let doc = parse_skill(input).unwrap();
        assert!(doc.instructions.contains("# Header One"));
        assert!(doc.instructions.contains("Paragraph before table."));
        assert!(doc.instructions.contains("Paragraph after table."));
        assert!(doc.instructions.contains("## Header Two"));
        assert!(doc.instructions.contains("More text."));
        // Table rows should NOT appear in instructions
        assert!(!doc.instructions.contains("| x |"));
    }

    // -----------------------------------------------------------------------
    // Test 9: Parse all 8 orchestration examples
    // -----------------------------------------------------------------------

    fn example_1() -> &'static str {
        r#"~~~
name: code-review-pipeline
description: Review code changes through lint, review, and approval stages
~~~

| Fields | Type | Merge | Description |
|--------|------|-------|-------------|
| diff | string | — | The code diff to review |
| lint_results | list | append | Linting issues found |
| review_notes | string | override | Reviewer's assessment |
| quality | string | override | "pass" or "fail" |
| revision_count | int | override | Number of revision cycles |

| Nodes | Role | Description |
|-------|------|-------------|
| lint | worker | Run linter on the diff, populate lint_results |
| review | pm | Read diff + lint_results, write review_notes and quality |
| revise | worker | Address review_notes, update diff |
| approve | pm | Final sign-off, summarize changes |

| Edges | To | Condition |
|-------|-----|-----------|
| START | lint | — |
| lint | review | — |
| review | approve | quality = "pass" |
| review | revise | quality = "fail" and revision_count < 3 |
| review | END | quality = "fail" and revision_count >= 3 |
| revise | lint | — |
| approve | END | — |

**What this demonstrates:** Linear chain, conditional branching, feedback loop.
"#
    }

    fn example_2() -> &'static str {
        r#"~~~
name: task-router
description: Classify incoming tasks and route to appropriate specialist
~~~

| Fields | Type | Merge | Description |
|--------|------|-------|-------------|
| request | string | — | The incoming task request |
| category | string | override | Classified category |
| result | string | override | Final output |

| Nodes | Role | Description |
|-------|------|-------------|
| classify | pm | Read request, determine category |
| handle_code | worker | Execute coding task |
| handle_docs | curator | Execute documentation task |
| handle_research | worker | Execute research task |
| handle_unknown | pm | Escalate to user for unclassifiable requests |

| Edges | To | Condition |
|-------|-----|-----------|
| START | classify | — |
| classify | handle_code | category = "code" |
| classify | handle_docs | category = "docs" |
| classify | handle_research | category = "research" |
| classify | handle_unknown | category = "unknown" |
| handle_code | END | — |
| handle_docs | END | — |
| handle_research | END | — |
| handle_unknown | END | — |
"#
    }

    fn example_3() -> &'static str {
        r#"~~~
name: parallel-analysis
description: Run security, performance, and style analyses in parallel
~~~

| Fields | Type | Merge | Description |
|--------|------|-------|-------------|
| code | string | — | Source code to analyze |
| security_report | string | override | Security analysis output |
| perf_report | string | override | Performance analysis output |
| style_report | string | override | Style analysis output |
| summary | string | override | Synthesized report |

| Nodes | Role | Description |
|-------|------|-------------|
| security | worker | Analyze code for security vulnerabilities |
| performance | worker | Analyze code for performance issues |
| style | worker | Analyze code for style and conventions |
| synthesize | pm | Combine all three reports into a summary |

| Edges | To | Condition |
|-------|-----|-----------|
| START | [security, performance, style] | — |
| [security, performance, style] | synthesize | — |
| synthesize | END | — |
"#
    }

    fn example_4() -> &'static str {
        r#"~~~
name: orchestrator-worker
description: Decompose task into subtasks, dispatch workers, synthesize results
~~~

| Fields | Type | Merge | Description |
|--------|------|-------|-------------|
| task | string | — | The overall task to accomplish |
| subtasks | list | override | Decomposed subtask descriptions |
| results | list | append | Collected worker results |
| synthesis | string | override | Final synthesized output |

| Nodes | Role | Description |
|-------|------|-------------|
| plan | pm | Decompose task into subtasks list |
| execute | worker | Execute a single subtask, return result |
| collect | pm | Review all results, produce synthesis |

| Edges | To | Condition |
|-------|-----|-----------|
| START | plan | — |
| plan | each(subtasks) → execute | — |
| execute | collect | — |
| collect | END | — |
"#
    }

    fn example_5() -> &'static str {
        r#"~~~
name: evaluator-optimizer
description: Generate and refine content until quality threshold is met
~~~

| Fields | Type | Merge | Description |
|--------|------|-------|-------------|
| prompt | string | — | What to generate |
| draft | string | override | Current draft |
| feedback | string | override | Evaluator's feedback |
| score | int | override | Quality score (0-100) |
| iteration | int | override | Current iteration number |

| Nodes | Role | Description |
|-------|------|-------------|
| generate | worker | Produce or revise draft based on prompt and feedback |
| evaluate | pm | Score the draft, provide feedback if score < 80 |

| Edges | To | Condition |
|-------|-----|-----------|
| START | generate | — |
| generate | evaluate | — |
| evaluate | generate | score < 80 and iteration < 5 |
| evaluate | END | score >= 80 or iteration >= 5 |
"#
    }

    fn example_6() -> &'static str {
        r#"~~~
name: deploy-with-approval
description: Build, test, get human approval, then deploy
~~~

| Fields | Type | Merge | Description |
|--------|------|-------|-------------|
| branch | string | — | Git branch to deploy |
| build_log | string | override | Build output |
| test_results | string | override | Test output |
| approval | string | override | "approved" or "rejected" |
| deploy_log | string | override | Deployment output |

| Nodes | Role | Description |
|-------|------|-------------|
| build | worker | Build the branch |
| test | worker | Run test suite |
| request_approval | pm | Present results to user, request approval |
| deploy | worker | Deploy to production |
| rollback | worker | Clean up failed deployment |

| Edges | To | Condition |
|-------|-----|-----------|
| START | build | — |
| build | test | — |
| test | request_approval | — |
| request_approval | WAIT | — |
| WAIT | deploy | approval = "approved" |
| WAIT | rollback | approval = "rejected" |
| deploy | END | — |
| rollback | END | — |
"#
    }

    fn example_7() -> &'static str {
        r#"~~~
name: full-feature-delivery
description: End-to-end feature delivery from spec to production
~~~

| Fields | Type | Merge | Description |
|--------|------|-------|-------------|
| feature_spec | string | — | What to build |
| design_doc | string | override | Design output |
| implementation | string | override | Code output |
| review_result | string | override | Review outcome |
| deploy_result | string | override | Deployment outcome |

| Nodes | Role | Description |
|-------|------|-------------|
| design | pm | Produce design document from spec |
| implement | worker | Write code based on design |
| review | flow:code-review-pipeline | Run the code review orchestration (Example 1) |
| deploy | flow:deploy-with-approval | Run the deployment orchestration (Example 6) |

| Edges | To | Condition |
|-------|-----|-----------|
| START | design | — |
| design | implement | — |
| implement | review | — |
| review | deploy | review_result = "pass" |
| review | implement | review_result = "fail" |
| deploy | END | — |
"#
    }

    fn example_8() -> &'static str {
        r#"~~~
name: project-monitor
description: Continuously monitor active projects, detect and respond to problems
~~~

| Fields | Type | Merge | Description |
|--------|------|-------|-------------|
| projects | list | override | List of active project names |
| alerts | list | append | Unresolved alerts |
| agent_states | map | merge | Current state of each agent |

| Nodes | Role | Description |
|-------|------|-------------|
| check_heartbeats | — | Poll all agents, update agent_states |
| detect_stalls | pm | Review agent_states, generate alerts for stalled agents |
| triage | pm | Prioritize alerts, decide action for each |
| intervene | worker | Execute the decided action (retry, restart, escalate) |
| report | pilot | Summarize status to user |

| Edges | To | Condition |
|-------|-----|-----------|
| START | check_heartbeats | — |
| check_heartbeats | detect_stalls | — |
| detect_stalls | triage | alerts not empty |
| detect_stalls | check_heartbeats | alerts empty, after: 60s |
| triage | each(alerts) → intervene | — |
| intervene | report | — |
| report | check_heartbeats | after: 60s |
"#
    }

    #[test]
    fn test_example_1_code_review_pipeline() {
        let doc = parse_skill(example_1()).unwrap();
        assert_eq!(doc.frontmatter.name.as_deref(), Some("code-review-pipeline"));
        assert_eq!(doc.kind(), SkillKind::Orchestration);

        let fields = doc.fields.unwrap();
        assert_eq!(fields.fields.len(), 5);
        assert_eq!(fields.fields[0].name, "diff");
        assert!(fields.fields[0].merge.is_none()); // "—" normalizes to None
        assert_eq!(fields.fields[1].merge.as_deref(), Some("append"));

        let nodes = doc.nodes.unwrap();
        assert_eq!(nodes.nodes.len(), 4);
        assert_eq!(nodes.nodes[0].name, "lint");
        assert_eq!(nodes.nodes[0].role, "worker");

        let edges = doc.edges.unwrap();
        assert_eq!(edges.edges.len(), 7);
        assert_eq!(edges.edges[0].from, EdgeEndpoint::Start);
        assert_eq!(edges.edges[6].to, EdgeEndpoint::End);
        // Check conditional edge
        assert_eq!(
            edges.edges[2].condition.as_deref(),
            Some("quality = \"pass\"")
        );
    }

    #[test]
    fn test_example_2_task_router() {
        let doc = parse_skill(example_2()).unwrap();
        assert_eq!(doc.frontmatter.name.as_deref(), Some("task-router"));
        assert_eq!(doc.kind(), SkillKind::Orchestration);

        let nodes = doc.nodes.unwrap();
        assert_eq!(nodes.nodes.len(), 5);
        assert_eq!(nodes.nodes[2].role, "curator");

        let edges = doc.edges.unwrap();
        assert_eq!(edges.edges.len(), 9);
        // All handle_* nodes go to END
        for edge in &edges.edges[5..9] {
            assert_eq!(edge.to, EdgeEndpoint::End);
        }
    }

    #[test]
    fn test_example_3_parallel_analysis() {
        let doc = parse_skill(example_3()).unwrap();
        assert_eq!(doc.frontmatter.name.as_deref(), Some("parallel-analysis"));
        assert_eq!(doc.kind(), SkillKind::Orchestration);

        let edges = doc.edges.unwrap();
        assert_eq!(edges.edges.len(), 3);
        // START → [security, performance, style]
        assert_eq!(
            edges.edges[0].to,
            EdgeEndpoint::Parallel(vec![
                "security".to_string(),
                "performance".to_string(),
                "style".to_string()
            ])
        );
        // [security, performance, style] → synthesize
        assert_eq!(
            edges.edges[1].from,
            EdgeEndpoint::Parallel(vec![
                "security".to_string(),
                "performance".to_string(),
                "style".to_string()
            ])
        );
    }

    #[test]
    fn test_example_4_orchestrator_worker() {
        let doc = parse_skill(example_4()).unwrap();
        assert_eq!(
            doc.frontmatter.name.as_deref(),
            Some("orchestrator-worker")
        );
        assert_eq!(doc.kind(), SkillKind::Orchestration);

        let edges = doc.edges.unwrap();
        assert_eq!(edges.edges.len(), 4);
        // plan → each(subtasks) → execute
        assert_eq!(
            edges.edges[1].to,
            EdgeEndpoint::DynamicFanOut {
                field: "subtasks".to_string(),
                node: "execute".to_string(),
            }
        );
    }

    #[test]
    fn test_example_5_evaluator_optimizer() {
        let doc = parse_skill(example_5()).unwrap();
        assert_eq!(
            doc.frontmatter.name.as_deref(),
            Some("evaluator-optimizer")
        );
        assert_eq!(doc.kind(), SkillKind::Orchestration);

        let edges = doc.edges.unwrap();
        assert_eq!(edges.edges.len(), 4);
        // Feedback loop: evaluate → generate with condition
        assert_eq!(
            edges.edges[2].condition.as_deref(),
            Some("score < 80 and iteration < 5")
        );
    }

    #[test]
    fn test_example_6_deploy_with_approval() {
        let doc = parse_skill(example_6()).unwrap();
        assert_eq!(
            doc.frontmatter.name.as_deref(),
            Some("deploy-with-approval")
        );
        assert_eq!(doc.kind(), SkillKind::Orchestration);

        let edges = doc.edges.unwrap();
        assert_eq!(edges.edges.len(), 8);
        // request_approval → WAIT
        assert_eq!(edges.edges[3].to, EdgeEndpoint::Wait);
        // WAIT → deploy
        assert_eq!(edges.edges[4].from, EdgeEndpoint::Wait);
        assert_eq!(edges.edges[4].to, EdgeEndpoint::Node("deploy".to_string()));
        // WAIT → rollback
        assert_eq!(edges.edges[5].from, EdgeEndpoint::Wait);
    }

    #[test]
    fn test_example_7_full_feature_delivery() {
        let doc = parse_skill(example_7()).unwrap();
        assert_eq!(
            doc.frontmatter.name.as_deref(),
            Some("full-feature-delivery")
        );
        assert_eq!(doc.kind(), SkillKind::Orchestration);

        let nodes = doc.nodes.unwrap();
        // Check subflow nodes
        assert_eq!(nodes.nodes[2].role, "flow:code-review-pipeline");
        assert_eq!(nodes.nodes[3].role, "flow:deploy-with-approval");

        let edges = doc.edges.unwrap();
        assert_eq!(edges.edges.len(), 6);
    }

    #[test]
    fn test_example_8_project_monitor() {
        let doc = parse_skill(example_8()).unwrap();
        assert_eq!(doc.frontmatter.name.as_deref(), Some("project-monitor"));
        assert_eq!(doc.kind(), SkillKind::Orchestration);

        let fields = doc.fields.unwrap();
        assert_eq!(fields.fields.len(), 3);
        assert_eq!(fields.fields[2].field_type, "map");
        assert_eq!(fields.fields[2].merge.as_deref(), Some("merge"));

        let nodes = doc.nodes.unwrap();
        assert_eq!(nodes.nodes.len(), 5);
        assert_eq!(nodes.nodes[0].role, "—"); // em-dash role

        let edges = doc.edges.unwrap();
        assert_eq!(edges.edges.len(), 7);
        // Dynamic fan-out: triage → each(alerts) → intervene
        assert_eq!(
            edges.edges[4].to,
            EdgeEndpoint::DynamicFanOut {
                field: "alerts".to_string(),
                node: "intervene".to_string(),
            }
        );
        // Timer-based condition
        assert_eq!(
            edges.edges[3].condition.as_deref(),
            Some("alerts empty, after: 60s")
        );
    }
}
