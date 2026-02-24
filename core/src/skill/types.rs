use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;

// ---------------------------------------------------------------------------
// Top-level parsed result
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SkillDocument {
    pub frontmatter: Frontmatter,
    pub fields: Option<FieldsTable>,
    pub lifecycle: Option<LifecycleTable>,
    pub nodes: Option<NodesTable>,
    pub edges: Option<EdgesTable>,
    pub instructions: String,
}

impl SkillDocument {
    pub fn kind(&self) -> SkillKind {
        let has_nodes = self.nodes.is_some();
        let has_edges = self.edges.is_some();
        let has_fields = self.fields.is_some();
        let has_lifecycle = self.lifecycle.is_some();

        if has_nodes || has_edges {
            SkillKind::Orchestration
        } else if has_fields || has_lifecycle {
            SkillKind::Structured
        } else {
            SkillKind::Simple
        }
    }
}

// ---------------------------------------------------------------------------
// Skill classification
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillKind {
    Simple,
    Structured,
    Orchestration,
}

// ---------------------------------------------------------------------------
// Frontmatter
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct FrontmatterRaw {
    pub name: Option<String>,
    pub description: Option<String>,
    pub user_invocable: Option<bool>,
    pub allowed_tools: Option<Vec<String>>,
    pub model: Option<String>,
    pub agent: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_yaml::Value>,
}

#[derive(Debug, Clone, Default)]
pub struct Frontmatter {
    pub name: Option<String>,
    pub description: Option<String>,
    pub user_invocable: Option<bool>,
    pub allowed_tools: Option<Vec<String>>,
    pub model: Option<String>,
    pub agent: Option<String>,
    pub extra: HashMap<String, String>,
}

impl From<FrontmatterRaw> for Frontmatter {
    fn from(raw: FrontmatterRaw) -> Self {
        let extra = raw
            .extra
            .into_iter()
            .map(|(k, v)| {
                let s = match &v {
                    serde_yaml::Value::String(s) => s.clone(),
                    other => format!("{:?}", other),
                };
                (k, s)
            })
            .collect();
        Frontmatter {
            name: raw.name,
            description: raw.description,
            user_invocable: raw.user_invocable,
            allowed_tools: raw.allowed_tools,
            model: raw.model,
            agent: raw.agent,
            extra,
        }
    }
}

// ---------------------------------------------------------------------------
// Fields table
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct FieldsTable {
    pub fields: Vec<FieldDef>,
}

#[derive(Debug, Clone)]
pub struct FieldDef {
    pub name: String,
    pub field_type: String,
    pub merge: Option<String>,
    pub default: Option<String>,
    pub description: String,
}

// ---------------------------------------------------------------------------
// Lifecycle table
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct LifecycleTable {
    pub states: Vec<LifecycleState>,
}

#[derive(Debug, Clone)]
pub struct LifecycleState {
    pub name: String,
    pub description: String,
}

// ---------------------------------------------------------------------------
// Nodes table
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct NodesTable {
    pub nodes: Vec<NodeDef>,
}

#[derive(Debug, Clone)]
pub struct NodeDef {
    pub name: String,
    pub role: String,
    pub description: String,
}

// ---------------------------------------------------------------------------
// Edges table
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct EdgesTable {
    pub edges: Vec<EdgeDef>,
}

#[derive(Debug, Clone)]
pub struct EdgeDef {
    pub from: EdgeEndpoint,
    pub to: EdgeEndpoint,
    pub condition: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdgeEndpoint {
    Start,
    End,
    Wait,
    Node(String),
    Parallel(Vec<String>),
    DynamicFanOut { field: String, node: String },
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum SkillParseError {
    InvalidFrontmatter(String),
    MalformedTable { line: usize, reason: String },
    InvalidEdgeEndpoint(String),
    IoError(std::io::Error),
}

impl fmt::Display for SkillParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SkillParseError::InvalidFrontmatter(msg) => {
                write!(f, "invalid frontmatter: {}", msg)
            }
            SkillParseError::MalformedTable { line, reason } => {
                write!(f, "malformed table at line {}: {}", line, reason)
            }
            SkillParseError::InvalidEdgeEndpoint(msg) => {
                write!(f, "invalid edge endpoint: {}", msg)
            }
            SkillParseError::IoError(e) => write!(f, "IO error: {}", e),
        }
    }
}

impl std::error::Error for SkillParseError {}

impl From<std::io::Error> for SkillParseError {
    fn from(e: std::io::Error) -> Self {
        SkillParseError::IoError(e)
    }
}
