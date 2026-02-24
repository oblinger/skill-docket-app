use std::collections::HashMap;


/// An agent entry parsed from a configuration markdown document.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentEntry {
    pub name: String,
    pub role: String,
    pub task: Option<String>,
    pub path: String,
}


/// Parsed representation of a CMX configuration markdown document.
///
/// The document has sections like:
///
/// ```markdown
/// ## Version
/// 1.0
///
/// ## Agents
/// | Name | Role | Task | Path |
/// |------|------|------|------|
/// | worker1 | worker | CMX1 | /projects/cmx |
///
/// ## Sessions
/// | Session | Tile |
/// |---------|------|
/// | cmx-main | dev-env |
///
/// ## Layouts
/// | Session | Layout |
/// |---------|--------|
/// | cmx-main | row(pilot 30%, col(worker1, worker2) 70%) |
/// ```
#[derive(Debug, Clone)]
pub struct ConfigDoc {
    pub version: String,
    pub agents: Vec<AgentEntry>,
    pub sessions: Vec<(String, String)>,
    pub layouts: HashMap<String, String>,
}


impl ConfigDoc {
    /// Parse a configuration markdown document.
    pub fn parse(content: &str) -> Result<ConfigDoc, String> {
        let mut version = String::new();
        let mut agents: Vec<AgentEntry> = Vec::new();
        let mut sessions: Vec<(String, String)> = Vec::new();
        let mut layouts: HashMap<String, String> = HashMap::new();

        let mut current_section = Section::None;

        for line in content.lines() {
            let trimmed = line.trim();

            // Detect section headers
            if trimmed.starts_with("## ") {
                let header = trimmed[3..].trim().to_lowercase();
                current_section = match header.as_str() {
                    "version" => Section::Version,
                    "agents" => Section::Agents,
                    "sessions" => Section::Sessions,
                    "layouts" => Section::Layouts,
                    _ => Section::None,
                };
                continue;
            }

            // Skip blank lines, table header separators, and table headers
            if trimmed.is_empty() || is_table_separator(trimmed) {
                continue;
            }

            match current_section {
                Section::Version => {
                    if version.is_empty() && !trimmed.is_empty() {
                        version = trimmed.to_string();
                    }
                }
                Section::Agents => {
                    if let Some(entry) = parse_agent_row(trimmed) {
                        agents.push(entry);
                    }
                }
                Section::Sessions => {
                    if let Some(pair) = parse_two_col_row(trimmed) {
                        sessions.push(pair);
                    }
                }
                Section::Layouts => {
                    if let Some(pair) = parse_two_col_row(trimmed) {
                        layouts.insert(pair.0, pair.1);
                    }
                }
                Section::None => {}
            }
        }

        if version.is_empty() {
            version = "1.0".into();
        }

        Ok(ConfigDoc {
            version,
            agents,
            sessions,
            layouts,
        })
    }

    /// Serialize the config doc back to markdown.
    pub fn serialize(&self) -> String {
        let mut out = String::new();

        out.push_str("## Version\n\n");
        out.push_str(&self.version);
        out.push('\n');

        out.push_str("\n## Agents\n\n");
        out.push_str("| Name | Role | Task | Path |\n");
        out.push_str("|------|------|------|------|\n");
        for a in &self.agents {
            let task = a.task.as_deref().unwrap_or("");
            out.push_str(&format!("| {} | {} | {} | {} |\n", a.name, a.role, task, a.path));
        }

        out.push_str("\n## Sessions\n\n");
        out.push_str("| Session | Tile |\n");
        out.push_str("|---------|------|\n");
        for (session, tile) in &self.sessions {
            out.push_str(&format!("| {} | {} |\n", session, tile));
        }

        out.push_str("\n## Layouts\n\n");
        out.push_str("| Session | Layout |\n");
        out.push_str("|---------|--------|\n");
        // Sort for deterministic output
        let mut layout_entries: Vec<(&String, &String)> = self.layouts.iter().collect();
        layout_entries.sort_by_key(|(k, _)| k.as_str());
        for (session, layout) in layout_entries {
            out.push_str(&format!("| {} | {} |\n", session, layout));
        }

        out
    }

    /// Return agent entries.
    pub fn agent_entries(&self) -> &[AgentEntry] {
        &self.agents
    }

    /// Return session entries.
    pub fn session_entries(&self) -> &[(String, String)] {
        &self.sessions
    }

    /// Update (or insert) the layout expression for a session.
    pub fn update_layout(&mut self, session: &str, layout_expr: &str) {
        self.layouts
            .insert(session.to_string(), layout_expr.to_string());
    }
}


#[derive(Debug, Clone, Copy, PartialEq)]
enum Section {
    None,
    Version,
    Agents,
    Sessions,
    Layouts,
}


/// Check if a line is a markdown table separator like `|---|---|`
fn is_table_separator(line: &str) -> bool {
    let stripped = line.replace('-', "").replace('|', "").replace(' ', "").replace(':', "");
    stripped.is_empty()
}


/// Parse a 4-column table row into an AgentEntry.
/// Expected: `| name | role | task | path |`
fn parse_agent_row(line: &str) -> Option<AgentEntry> {
    if !line.contains('|') {
        return None;
    }
    let cols = parse_table_cols(line);
    if cols.len() < 4 {
        return None;
    }
    // Skip the header row (contains "Name", "Role", etc.)
    if cols[0].eq_ignore_ascii_case("name") {
        return None;
    }
    let task = if cols[2].is_empty() {
        None
    } else {
        Some(cols[2].clone())
    };
    Some(AgentEntry {
        name: cols[0].clone(),
        role: cols[1].clone(),
        task,
        path: cols[3].clone(),
    })
}


/// Parse a 2-column table row into a (String, String) pair.
fn parse_two_col_row(line: &str) -> Option<(String, String)> {
    if !line.contains('|') {
        return None;
    }
    let cols = parse_table_cols(line);
    if cols.len() < 2 {
        return None;
    }
    // Skip header rows
    if cols[0].eq_ignore_ascii_case("session")
        || cols[0].eq_ignore_ascii_case("name")
    {
        return None;
    }
    Some((cols[0].clone(), cols[1].clone()))
}


/// Split a markdown table row by `|` and trim each cell.
/// Leading and trailing empty segments (from the outer pipes) are removed,
/// but interior empty cells are preserved.
fn parse_table_cols(line: &str) -> Vec<String> {
    let parts: Vec<String> = line.split('|').map(|s| s.trim().to_string()).collect();
    // Strip the first and last elements if they are empty (from leading/trailing |)
    let start = if parts.first().map_or(false, |s| s.is_empty()) { 1 } else { 0 };
    let end = if parts.last().map_or(false, |s| s.is_empty()) {
        parts.len() - 1
    } else {
        parts.len()
    };
    if start >= end {
        return Vec::new();
    }
    parts[start..end].to_vec()
}


#[cfg(test)]
mod tests {
    use super::*;

    fn sample_doc() -> &'static str {
        "\
## Version

2.0

## Agents

| Name | Role | Task | Path |
|------|------|------|------|
| worker1 | worker | CMX1 | /projects/cmx |
| pilot1 | pilot | | /projects/cmx |

## Sessions

| Session | Tile |
|---------|------|
| cmx-main | dev-env |
| cmx-work | worker-panel |

## Layouts

| Session | Layout |
|---------|--------|
| cmx-main | row(pilot 30%, col(w1, w2) 70%) |
"
    }

    #[test]
    fn parse_version() {
        let doc = ConfigDoc::parse(sample_doc()).unwrap();
        assert_eq!(doc.version, "2.0");
    }

    #[test]
    fn parse_agents() {
        let doc = ConfigDoc::parse(sample_doc()).unwrap();
        assert_eq!(doc.agents.len(), 2);
        assert_eq!(doc.agents[0].name, "worker1");
        assert_eq!(doc.agents[0].role, "worker");
        assert_eq!(doc.agents[0].task, Some("CMX1".into()));
        assert_eq!(doc.agents[1].name, "pilot1");
        assert_eq!(doc.agents[1].task, None);
    }

    #[test]
    fn parse_sessions() {
        let doc = ConfigDoc::parse(sample_doc()).unwrap();
        assert_eq!(doc.sessions.len(), 2);
        assert_eq!(doc.sessions[0], ("cmx-main".into(), "dev-env".into()));
    }

    #[test]
    fn parse_layouts() {
        let doc = ConfigDoc::parse(sample_doc()).unwrap();
        assert_eq!(doc.layouts.len(), 1);
        assert!(doc.layouts.contains_key("cmx-main"));
    }

    #[test]
    fn parse_empty_doc() {
        let doc = ConfigDoc::parse("").unwrap();
        assert_eq!(doc.version, "1.0"); // default
        assert!(doc.agents.is_empty());
        assert!(doc.sessions.is_empty());
    }

    #[test]
    fn serialize_round_trip() {
        let doc = ConfigDoc::parse(sample_doc()).unwrap();
        let serialized = doc.serialize();
        let reparsed = ConfigDoc::parse(&serialized).unwrap();
        assert_eq!(reparsed.version, doc.version);
        assert_eq!(reparsed.agents.len(), doc.agents.len());
        assert_eq!(reparsed.sessions.len(), doc.sessions.len());
        assert_eq!(reparsed.layouts.len(), doc.layouts.len());
    }

    #[test]
    fn agent_entries_accessor() {
        let doc = ConfigDoc::parse(sample_doc()).unwrap();
        let entries = doc.agent_entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, "/projects/cmx");
    }

    #[test]
    fn session_entries_accessor() {
        let doc = ConfigDoc::parse(sample_doc()).unwrap();
        let entries = doc.session_entries();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn parse_no_version_defaults() {
        let md = "\
## Agents

| Name | Role | Task | Path |
|------|------|------|------|
| w1 | worker | T1 | /tmp |
";
        let doc = ConfigDoc::parse(md).unwrap();
        assert_eq!(doc.version, "1.0");
        assert_eq!(doc.agents.len(), 1);
    }

    #[test]
    fn parse_unknown_sections_ignored() {
        let md = "\
## Version

1.0

## FooBar

Some random content here.

## Agents

| Name | Role | Task | Path |
|------|------|------|------|
| w1 | worker | | /tmp |
";
        let doc = ConfigDoc::parse(md).unwrap();
        assert_eq!(doc.agents.len(), 1);
    }

    #[test]
    fn serialize_empty_task() {
        let doc = ConfigDoc {
            version: "1.0".into(),
            agents: vec![AgentEntry {
                name: "w1".into(),
                role: "worker".into(),
                task: None,
                path: "/tmp".into(),
            }],
            sessions: vec![],
            layouts: HashMap::new(),
        };
        let s = doc.serialize();
        assert!(s.contains("| w1 | worker |  | /tmp |"));
    }
}
