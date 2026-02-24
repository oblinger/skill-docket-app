use crate::types::task::{TaskNode, TaskSource, TaskStatus};


/// Parse a Roadmap.md file into a tree of TaskNode values.
///
/// Recognized heading patterns:
///   `# <marker> Milestone N -- Title`        -> depth 0
///   `## <marker> MN.S -- Title`              -> depth 1
///   `### <marker> MN.S.T -- Title`           -> depth 2
///
/// Status markers:
///   `\u{25EF}` (white circle)  = Pending
///   `\u{25B6}` (play)          = InProgress
///   `\u{2B24}` (black circle)  = Completed
///
/// A result string may follow the title after a second em-dash:
///   `### \u{2B24} M1.2.3 -- Title -- result text`
pub fn parse(content: &str) -> Result<Vec<TaskNode>, String> {
    // Collect (depth, TaskNode) pairs, then nest into a tree.
    let mut items: Vec<(usize, TaskNode)> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('#') {
            continue;
        }

        // Count heading level
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if hashes < 1 || hashes > 3 {
            continue;
        }
        let depth = hashes - 1; // # = 0, ## = 1, ### = 2

        let rest = trimmed[hashes..].trim();

        // Parse status marker
        let (status, rest) = parse_status_marker(rest)?;

        // Parse id and title: "M1.2.3 -- Title" or "M1.2.3 -- Title -- result"
        // The separator can be em-dash (\u{2014}) or double hyphen (--)
        let (id, title, result) = parse_id_title_result(rest)?;

        let node = TaskNode {
            id,
            title,
            source: TaskSource::Roadmap,
            status,
            result,
            agent: None,
            children: Vec::new(),
            spec_path: None,
        };

        items.push((depth, node));
    }

    // Build tree from flat list with depths
    let roots = nest_items(&items);
    Ok(roots)
}


/// Serialize a list of root TaskNode values back to Roadmap markdown.
pub fn serialize(tasks: &[TaskNode]) -> String {
    let mut out = String::new();
    for task in tasks {
        serialize_node(task, 1, &mut out);
    }
    out
}


fn serialize_node(node: &TaskNode, heading_level: usize, out: &mut String) {
    let marker = status_to_marker(&node.status);
    let hashes: String = "#".repeat(heading_level);

    match &node.result {
        Some(result) => {
            out.push_str(&format!(
                "{} {} {} \u{2014} {} \u{2014} {}\n",
                hashes, marker, node.id, node.title, result
            ));
        }
        None => {
            out.push_str(&format!(
                "{} {} {} \u{2014} {}\n",
                hashes, marker, node.id, node.title
            ));
        }
    }

    for child in &node.children {
        serialize_node(child, heading_level + 1, out);
    }
}


fn status_to_marker(status: &TaskStatus) -> char {
    match status {
        TaskStatus::Pending => '\u{25EF}',   // white circle
        TaskStatus::InProgress => '\u{25B6}', // play triangle
        TaskStatus::Completed => '\u{2B24}',  // black circle
        TaskStatus::Failed => '\u{25EF}',     // fall back to pending marker
        TaskStatus::Paused => '\u{25EF}',
        TaskStatus::Cancelled => '\u{25EF}',
    }
}


fn parse_status_marker(s: &str) -> Result<(TaskStatus, &str), String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty heading after #".into());
    }

    let first = s.chars().next().unwrap();
    let rest = &s[first.len_utf8()..].trim_start();

    let status = match first {
        '\u{25EF}' => TaskStatus::Pending,     // white circle
        '\u{25CB}' => TaskStatus::Pending,     // another white circle variant
        '\u{25B6}' => TaskStatus::InProgress,  // play triangle
        '\u{2B24}' => TaskStatus::Completed,   // black large circle
        '\u{2B1B}' => TaskStatus::Completed,   // black square (alternative)
        _ => {
            // No recognized marker; treat entire string as the rest, default Pending
            return Ok((TaskStatus::Pending, s));
        }
    };

    Ok((status, rest))
}


fn parse_id_title_result(s: &str) -> Result<(String, String, Option<String>), String> {
    // Split on em-dash or double-hyphen
    let parts = split_on_dash(s);

    if parts.is_empty() {
        return Err("no id/title found in heading".into());
    }

    let id = parts[0].trim().to_string();
    if id.is_empty() {
        return Err("empty task id in heading".into());
    }

    let title = if parts.len() > 1 {
        parts[1].trim().to_string()
    } else {
        id.clone()
    };

    let result = if parts.len() > 2 {
        let r = parts[2..].join(" \u{2014} ").trim().to_string();
        if r.is_empty() { None } else { Some(r) }
    } else {
        None
    };

    Ok((id, title, result))
}


/// Split a string on em-dash (\u{2014}) or ` -- ` (space-hyphen-hyphen-space).
fn split_on_dash(s: &str) -> Vec<&str> {
    // Try em-dash first
    if s.contains('\u{2014}') {
        return s.split('\u{2014}').collect();
    }
    // Fall back to double-hyphen
    if s.contains(" -- ") {
        return s.split(" -- ").collect();
    }
    // No separator found; entire string is one part
    vec![s]
}


/// Build a tree from a flat list of (depth, TaskNode) pairs.
fn nest_items(items: &[(usize, TaskNode)]) -> Vec<TaskNode> {
    if items.is_empty() {
        return Vec::new();
    }

    let mut roots: Vec<TaskNode> = Vec::new();
    // Stack: (depth, node). We'll flush nodes as we go.
    let mut stack: Vec<(usize, TaskNode)> = Vec::new();

    for (depth, node) in items {
        let depth = *depth;
        let node = node.clone();

        // Pop stack entries that are at the same level or deeper
        while let Some((sd, _)) = stack.last() {
            if *sd >= depth {
                let (_, popped) = stack.pop().unwrap();
                if let Some((_, parent)) = stack.last_mut() {
                    parent.children.push(popped);
                } else {
                    roots.push(popped);
                }
            } else {
                break;
            }
        }

        stack.push((depth, node));
    }

    // Flush remaining stack
    while let Some((_, popped)) = stack.pop() {
        if let Some((_, parent)) = stack.last_mut() {
            parent.children.push(popped);
        } else {
            roots.push(popped);
        }
    }

    roots
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_milestone() {
        let md = "# \u{25EF} M1 \u{2014} Core Daemon\n";
        let tasks = parse(md).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, "M1");
        assert_eq!(tasks[0].title, "Core Daemon");
        assert_eq!(tasks[0].status, TaskStatus::Pending);
    }

    #[test]
    fn parse_nested_sections() {
        let md = "\
# \u{25B6} M1 \u{2014} Core Daemon
## \u{25EF} M1.1 \u{2014} Socket Protocol
### \u{2B24} M1.1.1 \u{2014} Message Format
";
        let tasks = parse(md).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].status, TaskStatus::InProgress);
        assert_eq!(tasks[0].children.len(), 1);
        assert_eq!(tasks[0].children[0].id, "M1.1");
        assert_eq!(tasks[0].children[0].children.len(), 1);
        assert_eq!(tasks[0].children[0].children[0].status, TaskStatus::Completed);
    }

    #[test]
    fn parse_with_result_string() {
        let md = "### \u{2B24} M1.2.3 \u{2014} Build Types \u{2014} all structs defined\n";
        let tasks = parse(md).unwrap();
        assert_eq!(tasks[0].id, "M1.2.3");
        assert_eq!(tasks[0].result.as_deref(), Some("all structs defined"));
    }

    #[test]
    fn parse_double_hyphen_separator() {
        let md = "# \u{25EF} M1 -- Core Daemon\n";
        let tasks = parse(md).unwrap();
        assert_eq!(tasks[0].id, "M1");
        assert_eq!(tasks[0].title, "Core Daemon");
    }

    #[test]
    fn parse_multiple_milestones() {
        let md = "\
# \u{25EF} M1 \u{2014} First
# \u{25B6} M2 \u{2014} Second
# \u{2B24} M3 \u{2014} Third
";
        let tasks = parse(md).unwrap();
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].status, TaskStatus::Pending);
        assert_eq!(tasks[1].status, TaskStatus::InProgress);
        assert_eq!(tasks[2].status, TaskStatus::Completed);
    }

    #[test]
    fn parse_ignores_non_heading_lines() {
        let md = "\
Some intro text.

# \u{25EF} M1 \u{2014} Core

Body paragraph here.

## \u{25EF} M1.1 \u{2014} Sub
";
        let tasks = parse(md).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].children.len(), 1);
    }

    #[test]
    fn parse_mixed_statuses() {
        let md = "\
# \u{25B6} M1 \u{2014} Active
## \u{2B24} M1.1 \u{2014} Done
## \u{25EF} M1.2 \u{2014} Todo
## \u{25B6} M1.3 \u{2014} Working
";
        let tasks = parse(md).unwrap();
        let m1 = &tasks[0];
        assert_eq!(m1.children.len(), 3);
        assert_eq!(m1.children[0].status, TaskStatus::Completed);
        assert_eq!(m1.children[1].status, TaskStatus::Pending);
        assert_eq!(m1.children[2].status, TaskStatus::InProgress);
    }

    #[test]
    fn serialize_round_trip() {
        let md = "\
# \u{25B6} M1 \u{2014} Core Daemon
## \u{25EF} M1.1 \u{2014} Socket Protocol
### \u{2B24} M1.1.1 \u{2014} Message Format \u{2014} done
";
        let tasks = parse(md).unwrap();
        let output = serialize(&tasks);
        let reparsed = parse(&output).unwrap();
        assert_eq!(reparsed.len(), 1);
        assert_eq!(reparsed[0].id, "M1");
        assert_eq!(reparsed[0].children[0].id, "M1.1");
        assert_eq!(reparsed[0].children[0].children[0].result.as_deref(), Some("done"));
    }

    #[test]
    fn serialize_empty() {
        assert_eq!(serialize(&[]), "");
    }

    #[test]
    fn parse_empty() {
        let tasks = parse("").unwrap();
        assert!(tasks.is_empty());
    }

    #[test]
    fn parse_siblings_at_depth_2() {
        let md = "\
# \u{25EF} M1 \u{2014} Root
## \u{25EF} M1.1 \u{2014} A
### \u{25EF} M1.1.1 \u{2014} Leaf A
### \u{25EF} M1.1.2 \u{2014} Leaf B
## \u{25EF} M1.2 \u{2014} B
";
        let tasks = parse(md).unwrap();
        assert_eq!(tasks[0].children.len(), 2);
        assert_eq!(tasks[0].children[0].children.len(), 2);
        assert_eq!(tasks[0].children[0].children[1].id, "M1.1.2");
    }
}
