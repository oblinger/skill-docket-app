//! Dotted path parser and namespace resolution (M10.1).
//!
//! Parses paths like `task.AUTH1.status`, `agent.worker1.health`,
//! `config.heartbeat_timeout` into structured types with namespace
//! discrimination, wildcard matching, and variable binding.

use std::collections::HashMap;
use std::fmt;
use serde::{Serialize, Deserialize};


/// Top-level namespace categories.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Namespace {
    /// task.* — task state, fields, progress
    Task,
    /// agent.* — agent lifecycle, health, assignment
    Agent,
    /// flow.* — workflow execution state
    Flow,
    /// project.* — project-level configuration and metadata
    Project,
    /// config.* — runtime configuration parameters
    Config,
    /// session.* — ephemeral session data
    Session,
}

impl Namespace {
    /// The canonical string prefix for this namespace.
    pub fn as_str(&self) -> &'static str {
        match self {
            Namespace::Task => "task",
            Namespace::Agent => "agent",
            Namespace::Flow => "flow",
            Namespace::Project => "project",
            Namespace::Config => "config",
            Namespace::Session => "session",
        }
    }
}

impl fmt::Display for Namespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}


/// A single segment within a dotted path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathSegment {
    /// Exact literal match, e.g. `AUTH1`
    Literal(String),
    /// Single wildcard `*` — matches any single segment
    Wildcard,
    /// Double wildcard `**` — matches zero or more segments
    DoubleWildcard,
    /// Variable binding `$var` — matches any single segment and captures it
    Variable(String),
}

impl fmt::Display for PathSegment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PathSegment::Literal(s) => f.write_str(s),
            PathSegment::Wildcard => f.write_str("*"),
            PathSegment::DoubleWildcard => f.write_str("**"),
            PathSegment::Variable(name) => write!(f, "${}", name),
        }
    }
}


/// A parsed dotted path with namespace and segments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespacePath {
    pub namespace: Namespace,
    pub segments: Vec<PathSegment>,
}

impl NamespacePath {
    /// Parse a dotted string like `task.AUTH1.status` into a NamespacePath.
    ///
    /// The first segment must be a known namespace. Remaining segments may
    /// be literals, `*` wildcards, `**` double wildcards, or `$var` variables.
    pub fn parse(input: &str) -> Result<Self, String> {
        let input = input.trim();
        if input.is_empty() {
            return Err("empty path".to_string());
        }

        let parts: Vec<&str> = input.split('.').collect();
        if parts.is_empty() {
            return Err("empty path".to_string());
        }

        let namespace = resolve_namespace(parts[0])?;
        let mut segments = Vec::new();

        for part in &parts[1..] {
            if part.is_empty() {
                return Err(format!("empty segment in path '{}'", input));
            }
            let seg = if *part == "**" {
                PathSegment::DoubleWildcard
            } else if *part == "*" {
                PathSegment::Wildcard
            } else if part.starts_with('$') {
                let var_name = &part[1..];
                if var_name.is_empty() {
                    return Err("empty variable name".to_string());
                }
                PathSegment::Variable(var_name.to_string())
            } else {
                PathSegment::Literal(part.to_string())
            };
            segments.push(seg);
        }

        Ok(NamespacePath { namespace, segments })
    }

    /// Format back to a dotted string.
    pub fn to_dotted(&self) -> String {
        let mut out = self.namespace.as_str().to_string();
        for seg in &self.segments {
            out.push('.');
            out.push_str(&seg.to_string());
        }
        out
    }

    /// True if this path contains any wildcard or variable segments.
    pub fn is_pattern(&self) -> bool {
        self.segments.iter().any(|s| {
            matches!(
                s,
                PathSegment::Wildcard | PathSegment::DoubleWildcard | PathSegment::Variable(_)
            )
        })
    }

    /// Match a concrete (non-pattern) path against this pattern.
    ///
    /// Returns `Some(bindings)` on match, where bindings maps variable names
    /// to their matched values. Returns `None` if no match.
    pub fn match_path(&self, concrete: &NamespacePath) -> Option<HashMap<String, String>> {
        if self.namespace != concrete.namespace {
            return None;
        }
        let mut bindings = HashMap::new();
        if match_segments(&self.segments, &concrete.segments, &mut bindings) {
            Some(bindings)
        } else {
            None
        }
    }
}

impl fmt::Display for NamespacePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_dotted())
    }
}


/// Parse the first segment of a dotted path into a Namespace enum.
pub fn resolve_namespace(s: &str) -> Result<Namespace, String> {
    match s {
        "task" => Ok(Namespace::Task),
        "agent" => Ok(Namespace::Agent),
        "flow" => Ok(Namespace::Flow),
        "project" => Ok(Namespace::Project),
        "config" => Ok(Namespace::Config),
        "session" => Ok(Namespace::Session),
        other => Err(format!("unknown namespace '{}'", other)),
    }
}


// ---------------------------------------------------------------------------
// Internal: recursive segment matching
// ---------------------------------------------------------------------------

fn match_segments(
    pattern: &[PathSegment],
    concrete: &[PathSegment],
    bindings: &mut HashMap<String, String>,
) -> bool {
    // Both empty — match.
    if pattern.is_empty() && concrete.is_empty() {
        return true;
    }

    // Pattern exhausted but concrete still has segments — no match.
    if pattern.is_empty() {
        return false;
    }

    match &pattern[0] {
        PathSegment::DoubleWildcard => {
            // ** matches zero or more concrete segments.
            // Try consuming 0, 1, 2, ... concrete segments.
            for skip in 0..=concrete.len() {
                if match_segments(&pattern[1..], &concrete[skip..], bindings) {
                    return true;
                }
            }
            false
        }
        PathSegment::Wildcard => {
            if concrete.is_empty() {
                return false;
            }
            // * matches exactly one concrete segment (must be a literal).
            match_segments(&pattern[1..], &concrete[1..], bindings)
        }
        PathSegment::Variable(name) => {
            if concrete.is_empty() {
                return false;
            }
            // $var matches exactly one literal segment, captures its value.
            if let PathSegment::Literal(val) = &concrete[0] {
                // Check for conflicting binding.
                if let Some(existing) = bindings.get(name) {
                    if existing != val {
                        return false;
                    }
                } else {
                    bindings.insert(name.clone(), val.clone());
                }
                match_segments(&pattern[1..], &concrete[1..], bindings)
            } else {
                false
            }
        }
        PathSegment::Literal(expected) => {
            if concrete.is_empty() {
                return false;
            }
            if let PathSegment::Literal(actual) = &concrete[0] {
                if expected == actual {
                    match_segments(&pattern[1..], &concrete[1..], bindings)
                } else {
                    false
                }
            } else {
                false
            }
        }
    }
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Parsing ---

    #[test]
    fn parse_task_path() {
        let p = NamespacePath::parse("task.AUTH1.status").unwrap();
        assert_eq!(p.namespace, Namespace::Task);
        assert_eq!(p.segments.len(), 2);
        assert_eq!(p.segments[0], PathSegment::Literal("AUTH1".into()));
        assert_eq!(p.segments[1], PathSegment::Literal("status".into()));
    }

    #[test]
    fn parse_agent_path() {
        let p = NamespacePath::parse("agent.worker1.health").unwrap();
        assert_eq!(p.namespace, Namespace::Agent);
        assert_eq!(p.segments.len(), 2);
        assert_eq!(p.segments[0], PathSegment::Literal("worker1".into()));
        assert_eq!(p.segments[1], PathSegment::Literal("health".into()));
    }

    #[test]
    fn parse_config_single_segment() {
        let p = NamespacePath::parse("config.heartbeat_timeout").unwrap();
        assert_eq!(p.namespace, Namespace::Config);
        assert_eq!(p.segments.len(), 1);
        assert_eq!(
            p.segments[0],
            PathSegment::Literal("heartbeat_timeout".into())
        );
    }

    #[test]
    fn parse_wildcard() {
        let p = NamespacePath::parse("agent.*.health").unwrap();
        assert_eq!(p.namespace, Namespace::Agent);
        assert_eq!(p.segments.len(), 2);
        assert_eq!(p.segments[0], PathSegment::Wildcard);
        assert_eq!(p.segments[1], PathSegment::Literal("health".into()));
        assert!(p.is_pattern());
    }

    #[test]
    fn parse_double_wildcard() {
        let p = NamespacePath::parse("task.**").unwrap();
        assert_eq!(p.namespace, Namespace::Task);
        assert_eq!(p.segments.len(), 1);
        assert_eq!(p.segments[0], PathSegment::DoubleWildcard);
        assert!(p.is_pattern());
    }

    #[test]
    fn parse_variable() {
        let p = NamespacePath::parse("task.$t.status").unwrap();
        assert_eq!(p.namespace, Namespace::Task);
        assert_eq!(p.segments.len(), 2);
        assert_eq!(p.segments[0], PathSegment::Variable("t".into()));
        assert_eq!(p.segments[1], PathSegment::Literal("status".into()));
        assert!(p.is_pattern());
    }

    #[test]
    fn parse_namespace_only() {
        let p = NamespacePath::parse("config").unwrap();
        assert_eq!(p.namespace, Namespace::Config);
        assert!(p.segments.is_empty());
        assert!(!p.is_pattern());
    }

    #[test]
    fn parse_all_namespaces() {
        for ns in ["task", "agent", "flow", "project", "config", "session"] {
            let p = NamespacePath::parse(&format!("{}.x", ns)).unwrap();
            assert_eq!(p.namespace.as_str(), ns);
        }
    }

    #[test]
    fn literal_path_is_not_pattern() {
        let p = NamespacePath::parse("task.AUTH1.status").unwrap();
        assert!(!p.is_pattern());
    }

    // --- Parse errors ---

    #[test]
    fn parse_empty_path() {
        assert!(NamespacePath::parse("").is_err());
    }

    #[test]
    fn parse_unknown_namespace() {
        assert!(NamespacePath::parse("bogus.x.y").is_err());
    }

    #[test]
    fn parse_empty_segment() {
        assert!(NamespacePath::parse("task..status").is_err());
    }

    #[test]
    fn parse_empty_variable_name() {
        assert!(NamespacePath::parse("task.$.status").is_err());
    }

    // --- Formatting ---

    #[test]
    fn to_dotted_round_trip() {
        let input = "task.AUTH1.status";
        let p = NamespacePath::parse(input).unwrap();
        assert_eq!(p.to_dotted(), input);
    }

    #[test]
    fn to_dotted_wildcard() {
        let input = "agent.*.health";
        let p = NamespacePath::parse(input).unwrap();
        assert_eq!(p.to_dotted(), input);
    }

    #[test]
    fn to_dotted_variable() {
        let input = "task.$t.status";
        let p = NamespacePath::parse(input).unwrap();
        assert_eq!(p.to_dotted(), input);
    }

    #[test]
    fn display_trait() {
        let p = NamespacePath::parse("flow.deploy.step1").unwrap();
        assert_eq!(format!("{}", p), "flow.deploy.step1");
    }

    // --- Matching ---

    #[test]
    fn match_variable_binding() {
        let pattern = NamespacePath::parse("task.$t.status").unwrap();
        let concrete = NamespacePath::parse("task.AUTH1.status").unwrap();
        let bindings = pattern.match_path(&concrete).unwrap();
        assert_eq!(bindings.get("t").unwrap(), "AUTH1");
    }

    #[test]
    fn match_wildcard_single() {
        let pattern = NamespacePath::parse("agent.*").unwrap();
        let concrete = NamespacePath::parse("agent.worker1").unwrap();
        assert!(pattern.match_path(&concrete).is_some());
    }

    #[test]
    fn match_wildcard_wrong_namespace() {
        let pattern = NamespacePath::parse("agent.*").unwrap();
        let concrete = NamespacePath::parse("task.AUTH1").unwrap();
        assert!(pattern.match_path(&concrete).is_none());
    }

    #[test]
    fn match_wildcard_with_suffix() {
        let pattern = NamespacePath::parse("agent.*.health").unwrap();
        let concrete = NamespacePath::parse("agent.worker1.health").unwrap();
        assert!(pattern.match_path(&concrete).is_some());
    }

    #[test]
    fn match_wildcard_too_short() {
        let pattern = NamespacePath::parse("agent.*.health").unwrap();
        let concrete = NamespacePath::parse("agent.worker1").unwrap();
        assert!(pattern.match_path(&concrete).is_none());
    }

    #[test]
    fn match_double_wildcard_zero_segments() {
        let pattern = NamespacePath::parse("task.**").unwrap();
        let concrete = NamespacePath::parse("task").unwrap();
        assert!(pattern.match_path(&concrete).is_some());
    }

    #[test]
    fn match_double_wildcard_multiple_segments() {
        let pattern = NamespacePath::parse("task.**").unwrap();
        let concrete = NamespacePath::parse("task.AUTH1.sub.deep").unwrap();
        assert!(pattern.match_path(&concrete).is_some());
    }

    #[test]
    fn match_double_wildcard_with_suffix() {
        let pattern = NamespacePath::parse("task.**.status").unwrap();
        let deep = NamespacePath::parse("task.AUTH1.sub.status").unwrap();
        assert!(pattern.match_path(&deep).is_some());
    }

    #[test]
    fn match_exact_literal() {
        let pattern = NamespacePath::parse("config.timeout").unwrap();
        let concrete = NamespacePath::parse("config.timeout").unwrap();
        let bindings = pattern.match_path(&concrete).unwrap();
        assert!(bindings.is_empty());
    }

    #[test]
    fn match_literal_mismatch() {
        let pattern = NamespacePath::parse("config.timeout").unwrap();
        let concrete = NamespacePath::parse("config.interval").unwrap();
        assert!(pattern.match_path(&concrete).is_none());
    }

    #[test]
    fn match_multiple_variables() {
        let pattern = NamespacePath::parse("task.$project.$task.status").unwrap();
        let concrete =
            NamespacePath::parse("task.myproject.AUTH1.status").unwrap();
        let bindings = pattern.match_path(&concrete).unwrap();
        assert_eq!(bindings.get("project").unwrap(), "myproject");
        assert_eq!(bindings.get("task").unwrap(), "AUTH1");
    }

    #[test]
    fn match_conflicting_variable() {
        // Same variable name $t used twice — values must be equal.
        let pattern = NamespacePath::parse("task.$t.$t").unwrap();
        let same = NamespacePath::parse("task.A.A").unwrap();
        assert!(pattern.match_path(&same).is_some());

        let diff = NamespacePath::parse("task.A.B").unwrap();
        assert!(pattern.match_path(&diff).is_none());
    }

    // --- resolve_namespace ---

    #[test]
    fn resolve_all_namespaces() {
        assert_eq!(resolve_namespace("task").unwrap(), Namespace::Task);
        assert_eq!(resolve_namespace("agent").unwrap(), Namespace::Agent);
        assert_eq!(resolve_namespace("flow").unwrap(), Namespace::Flow);
        assert_eq!(resolve_namespace("project").unwrap(), Namespace::Project);
        assert_eq!(resolve_namespace("config").unwrap(), Namespace::Config);
        assert_eq!(resolve_namespace("session").unwrap(), Namespace::Session);
    }

    #[test]
    fn resolve_unknown_namespace() {
        assert!(resolve_namespace("foobar").is_err());
    }

    // --- Namespace Display ---

    #[test]
    fn namespace_display() {
        assert_eq!(Namespace::Task.to_string(), "task");
        assert_eq!(Namespace::Session.to_string(), "session");
    }
}
