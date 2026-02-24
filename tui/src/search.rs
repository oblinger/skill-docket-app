//! Search engine — finds agents, tasks, and other entities by query.
//!
//! `SearchEngine` accepts a `SearchQuery` and returns matching `SearchResult`
//! items. Search is performed in-memory against provided data sets. The engine
//! supports scoped searches (agents only, tasks only, etc.) and fuzzy matching.

use serde::{Deserialize, Serialize};


// ---------------------------------------------------------------------------
// SearchScope
// ---------------------------------------------------------------------------

/// Limits the search to a specific entity type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchScope {
    /// Search across all entity types.
    All,
    /// Search only agents.
    Agents,
    /// Search only tasks.
    Tasks,
    /// Search only projects / folders.
    Projects,
    /// Search only messages.
    Messages,
    /// Search only commands (for autocomplete).
    Commands,
}

impl SearchScope {
    /// Return a short label for this scope.
    pub fn label(&self) -> &str {
        match self {
            SearchScope::All => "all",
            SearchScope::Agents => "agents",
            SearchScope::Tasks => "tasks",
            SearchScope::Projects => "projects",
            SearchScope::Messages => "messages",
            SearchScope::Commands => "commands",
        }
    }
}


// ---------------------------------------------------------------------------
// SearchQuery
// ---------------------------------------------------------------------------

/// Parameters for a search request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    /// The query string to search for.
    pub text: String,
    /// The scope to limit the search to.
    pub scope: SearchScope,
    /// Maximum number of results to return.
    pub max_results: usize,
    /// Whether to use case-insensitive matching.
    pub case_insensitive: bool,
    /// Whether to use fuzzy matching (substring vs exact prefix).
    pub fuzzy: bool,
}

impl SearchQuery {
    /// Create a new search query with defaults.
    pub fn new(text: &str) -> Self {
        SearchQuery {
            text: text.to_string(),
            scope: SearchScope::All,
            max_results: 50,
            case_insensitive: true,
            fuzzy: true,
        }
    }

    /// Builder: set the scope.
    pub fn with_scope(mut self, scope: SearchScope) -> Self {
        self.scope = scope;
        self
    }

    /// Builder: set the max results.
    pub fn with_max_results(mut self, max: usize) -> Self {
        self.max_results = max;
        self
    }

    /// Builder: set case sensitivity.
    pub fn with_case_sensitive(mut self) -> Self {
        self.case_insensitive = false;
        self
    }

    /// Builder: disable fuzzy matching (exact prefix only).
    pub fn with_exact(mut self) -> Self {
        self.fuzzy = false;
        self
    }

    /// Return whether the query text is empty.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}


// ---------------------------------------------------------------------------
// SearchResultKind
// ---------------------------------------------------------------------------

/// The kind of entity that matched the search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchResultKind {
    Agent,
    Task,
    Project,
    Message,
    Command,
}


// ---------------------------------------------------------------------------
// SearchResult
// ---------------------------------------------------------------------------

/// A single search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// The kind of entity that matched.
    pub kind: SearchResultKind,
    /// The identifier of the matched entity (agent name, task ID, etc.).
    pub id: String,
    /// A human-readable label for the result.
    pub label: String,
    /// Optional detail line (e.g. agent role, task status).
    pub detail: Option<String>,
    /// Relevance score (higher is more relevant).
    pub score: u32,
}

impl SearchResult {
    /// Create a new result.
    pub fn new(
        kind: SearchResultKind,
        id: &str,
        label: &str,
        detail: Option<&str>,
        score: u32,
    ) -> Self {
        SearchResult {
            kind,
            id: id.to_string(),
            label: label.to_string(),
            detail: detail.map(|d| d.to_string()),
            score,
        }
    }

    /// Return a one-line display string.
    pub fn display(&self) -> String {
        match &self.detail {
            Some(d) => format!("[{:?}] {} - {}", self.kind, self.label, d),
            None => format!("[{:?}] {}", self.kind, self.label),
        }
    }
}


// ---------------------------------------------------------------------------
// SearchEngine
// ---------------------------------------------------------------------------

/// In-memory search engine that matches queries against named items.
///
/// The engine does not own data — callers provide searchable items via
/// the `search_items` method. For convenience, helper methods accept
/// domain-specific slices (agent names, task tuples, etc.).
pub struct SearchEngine;

impl SearchEngine {
    /// Search a list of `(id, label, detail)` items against a query.
    ///
    /// Returns results sorted by score (descending), capped at
    /// `query.max_results`.
    pub fn search_items(
        query: &SearchQuery,
        items: &[(String, String, Option<String>)],
        kind: SearchResultKind,
    ) -> Vec<SearchResult> {
        if query.is_empty() {
            return Vec::new();
        }

        let needle = if query.case_insensitive {
            query.text.to_lowercase()
        } else {
            query.text.clone()
        };

        let mut results = Vec::new();

        for (id, label, detail) in items {
            let hay_id = if query.case_insensitive {
                id.to_lowercase()
            } else {
                id.clone()
            };
            let hay_label = if query.case_insensitive {
                label.to_lowercase()
            } else {
                label.clone()
            };

            let score = Self::score_match(&needle, &hay_id, &hay_label, query.fuzzy);
            if score > 0 {
                results.push(SearchResult::new(
                    kind.clone(),
                    id,
                    label,
                    detail.as_deref(),
                    score,
                ));
            }
        }

        // Sort by score descending, then by id ascending for stability.
        results.sort_by(|a, b| b.score.cmp(&a.score).then(a.id.cmp(&b.id)));
        results.truncate(query.max_results);
        results
    }

    /// Search agent names and roles.
    ///
    /// `agents` is a slice of `(name, role)` pairs.
    pub fn search_agents(
        query: &SearchQuery,
        agents: &[(String, String)],
    ) -> Vec<SearchResult> {
        let items: Vec<(String, String, Option<String>)> = agents
            .iter()
            .map(|(name, role)| (name.clone(), name.clone(), Some(role.clone())))
            .collect();
        Self::search_items(query, &items, SearchResultKind::Agent)
    }

    /// Search task IDs and titles.
    ///
    /// `tasks` is a slice of `(id, title, status)` triples.
    pub fn search_tasks(
        query: &SearchQuery,
        tasks: &[(String, String, String)],
    ) -> Vec<SearchResult> {
        let items: Vec<(String, String, Option<String>)> = tasks
            .iter()
            .map(|(id, title, status)| (id.clone(), title.clone(), Some(status.clone())))
            .collect();
        Self::search_items(query, &items, SearchResultKind::Task)
    }

    /// Search project names and paths.
    ///
    /// `projects` is a slice of `(name, path)` pairs.
    pub fn search_projects(
        query: &SearchQuery,
        projects: &[(String, String)],
    ) -> Vec<SearchResult> {
        let items: Vec<(String, String, Option<String>)> = projects
            .iter()
            .map(|(name, path)| (name.clone(), name.clone(), Some(path.clone())))
            .collect();
        Self::search_items(query, &items, SearchResultKind::Project)
    }

    /// Search command names.
    ///
    /// `commands` is a slice of command name strings.
    pub fn search_commands(
        query: &SearchQuery,
        commands: &[String],
    ) -> Vec<SearchResult> {
        let items: Vec<(String, String, Option<String>)> = commands
            .iter()
            .map(|name| (name.clone(), name.clone(), None))
            .collect();
        Self::search_items(query, &items, SearchResultKind::Command)
    }

    // -------------------------------------------------------------------
    // Scoring
    // -------------------------------------------------------------------

    /// Score a match between needle and the id/label haystack.
    ///
    /// Returns 0 for no match. Higher is better.
    ///
    /// Scoring rules:
    /// - Exact id match: 100
    /// - Id starts with needle: 80
    /// - Label starts with needle: 70
    /// - Id contains needle (fuzzy only): 50
    /// - Label contains needle (fuzzy only): 40
    /// - Otherwise: 0
    fn score_match(needle: &str, hay_id: &str, hay_label: &str, fuzzy: bool) -> u32 {
        if hay_id == needle {
            return 100;
        }
        if hay_id.starts_with(needle) {
            return 80;
        }
        if hay_label == needle {
            return 90;
        }
        if hay_label.starts_with(needle) {
            return 70;
        }
        if fuzzy {
            if hay_id.contains(needle) {
                return 50;
            }
            if hay_label.contains(needle) {
                return 40;
            }
        }
        0
    }
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- SearchScope ---

    #[test]
    fn scope_labels() {
        assert_eq!(SearchScope::All.label(), "all");
        assert_eq!(SearchScope::Agents.label(), "agents");
        assert_eq!(SearchScope::Tasks.label(), "tasks");
        assert_eq!(SearchScope::Projects.label(), "projects");
        assert_eq!(SearchScope::Messages.label(), "messages");
        assert_eq!(SearchScope::Commands.label(), "commands");
    }

    #[test]
    fn scope_serde_round_trip() {
        let scopes = [
            SearchScope::All,
            SearchScope::Agents,
            SearchScope::Tasks,
            SearchScope::Projects,
            SearchScope::Messages,
            SearchScope::Commands,
        ];
        for s in &scopes {
            let json = serde_json::to_string(s).unwrap();
            let back: SearchScope = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, s);
        }
    }

    // --- SearchQuery ---

    #[test]
    fn query_new_defaults() {
        let q = SearchQuery::new("test");
        assert_eq!(q.text, "test");
        assert_eq!(q.scope, SearchScope::All);
        assert_eq!(q.max_results, 50);
        assert!(q.case_insensitive);
        assert!(q.fuzzy);
    }

    #[test]
    fn query_builder_scope() {
        let q = SearchQuery::new("x").with_scope(SearchScope::Agents);
        assert_eq!(q.scope, SearchScope::Agents);
    }

    #[test]
    fn query_builder_max_results() {
        let q = SearchQuery::new("x").with_max_results(10);
        assert_eq!(q.max_results, 10);
    }

    #[test]
    fn query_builder_case_sensitive() {
        let q = SearchQuery::new("x").with_case_sensitive();
        assert!(!q.case_insensitive);
    }

    #[test]
    fn query_builder_exact() {
        let q = SearchQuery::new("x").with_exact();
        assert!(!q.fuzzy);
    }

    #[test]
    fn query_is_empty() {
        assert!(SearchQuery::new("").is_empty());
        assert!(!SearchQuery::new("foo").is_empty());
    }

    #[test]
    fn query_serde_round_trip() {
        let q = SearchQuery::new("test").with_scope(SearchScope::Tasks).with_max_results(5);
        let json = serde_json::to_string(&q).unwrap();
        let back: SearchQuery = serde_json::from_str(&json).unwrap();
        assert_eq!(back.text, "test");
        assert_eq!(back.scope, SearchScope::Tasks);
        assert_eq!(back.max_results, 5);
    }

    // --- SearchResult ---

    #[test]
    fn result_new_fields() {
        let r = SearchResult::new(SearchResultKind::Agent, "w1", "Worker 1", Some("worker"), 80);
        assert_eq!(r.kind, SearchResultKind::Agent);
        assert_eq!(r.id, "w1");
        assert_eq!(r.label, "Worker 1");
        assert_eq!(r.detail, Some("worker".into()));
        assert_eq!(r.score, 80);
    }

    #[test]
    fn result_display_with_detail() {
        let r = SearchResult::new(SearchResultKind::Task, "T1", "Core daemon", Some("in_progress"), 70);
        let d = r.display();
        assert!(d.contains("T1") || d.contains("Core daemon"));
        assert!(d.contains("in_progress"));
    }

    #[test]
    fn result_display_without_detail() {
        let r = SearchResult::new(SearchResultKind::Command, "status", "status", None, 100);
        let d = r.display();
        assert!(d.contains("status"));
    }

    #[test]
    fn result_serde_round_trip() {
        let r = SearchResult::new(SearchResultKind::Project, "cmx", "ClaudiMux", Some("/proj"), 50);
        let json = serde_json::to_string(&r).unwrap();
        let back: SearchResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "cmx");
        assert_eq!(back.score, 50);
    }

    // --- SearchEngine ---

    fn sample_items() -> Vec<(String, String, Option<String>)> {
        vec![
            ("w1".into(), "Worker 1".into(), Some("worker".into())),
            ("w2".into(), "Worker 2".into(), Some("worker".into())),
            ("pilot".into(), "Pilot Agent".into(), Some("pilot".into())),
            ("pm".into(), "Project Manager".into(), Some("pm".into())),
        ]
    }

    #[test]
    fn search_empty_query_returns_empty() {
        let q = SearchQuery::new("");
        let results = SearchEngine::search_items(&q, &sample_items(), SearchResultKind::Agent);
        assert!(results.is_empty());
    }

    #[test]
    fn search_exact_id_match() {
        let q = SearchQuery::new("w1");
        let results = SearchEngine::search_items(&q, &sample_items(), SearchResultKind::Agent);
        assert!(!results.is_empty());
        assert_eq!(results[0].id, "w1");
        assert_eq!(results[0].score, 100);
    }

    #[test]
    fn search_prefix_id_match() {
        let q = SearchQuery::new("w");
        let results = SearchEngine::search_items(&q, &sample_items(), SearchResultKind::Agent);
        assert!(results.len() >= 2); // w1 and w2
        assert!(results.iter().all(|r| r.score >= 80));
    }

    #[test]
    fn search_prefix_label_match() {
        let q = SearchQuery::new("Pilot");
        let results = SearchEngine::search_items(&q, &sample_items(), SearchResultKind::Agent);
        assert!(!results.is_empty());
        assert_eq!(results[0].id, "pilot");
    }

    #[test]
    fn search_fuzzy_substring() {
        let q = SearchQuery::new("Manager");
        let results = SearchEngine::search_items(&q, &sample_items(), SearchResultKind::Agent);
        assert!(!results.is_empty());
        assert_eq!(results[0].id, "pm");
    }

    #[test]
    fn search_no_fuzzy_no_substring() {
        let q = SearchQuery::new("Manager").with_exact();
        let results = SearchEngine::search_items(&q, &sample_items(), SearchResultKind::Agent);
        assert!(results.is_empty()); // "Manager" doesn't start with any id or label
    }

    #[test]
    fn search_case_insensitive() {
        let q = SearchQuery::new("pilot");
        let results = SearchEngine::search_items(&q, &sample_items(), SearchResultKind::Agent);
        assert!(!results.is_empty());
        assert_eq!(results[0].id, "pilot");
    }

    #[test]
    fn search_case_sensitive() {
        let q = SearchQuery::new("PILOT").with_case_sensitive();
        let results = SearchEngine::search_items(&q, &sample_items(), SearchResultKind::Agent);
        // "PILOT" doesn't match "pilot" or "Pilot Agent" case-sensitively
        assert!(results.is_empty());
    }

    #[test]
    fn search_max_results() {
        let q = SearchQuery::new("w").with_max_results(1);
        let results = SearchEngine::search_items(&q, &sample_items(), SearchResultKind::Agent);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn search_results_sorted_by_score() {
        let items = vec![
            ("worker".into(), "A Worker".into(), None),
            ("w".into(), "W".into(), None),
        ];
        let q = SearchQuery::new("w");
        let results = SearchEngine::search_items(&q, &items, SearchResultKind::Agent);
        assert!(results.len() >= 2);
        // "w" exact match = 100, "worker" prefix = 80
        assert!(results[0].score >= results[1].score);
    }

    // --- Domain-specific search helpers ---

    #[test]
    fn search_agents_helper() {
        let agents = vec![
            ("w1".into(), "worker".into()),
            ("pilot".into(), "pilot".into()),
        ];
        let q = SearchQuery::new("w1");
        let results = SearchEngine::search_agents(&q, &agents);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].kind, SearchResultKind::Agent);
        assert_eq!(results[0].id, "w1");
    }

    #[test]
    fn search_tasks_helper() {
        let tasks = vec![
            ("T1".into(), "Core daemon".into(), "in_progress".into()),
            ("T2".into(), "Socket protocol".into(), "pending".into()),
        ];
        let q = SearchQuery::new("Core");
        let results = SearchEngine::search_tasks(&q, &tasks);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].kind, SearchResultKind::Task);
    }

    #[test]
    fn search_projects_helper() {
        let projects = vec![
            ("cmx".into(), "/projects/cmx".into()),
            ("vmt".into(), "/projects/vmt".into()),
        ];
        let q = SearchQuery::new("cmx");
        let results = SearchEngine::search_projects(&q, &projects);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].kind, SearchResultKind::Project);
    }

    #[test]
    fn search_commands_helper() {
        let commands = vec![
            "status".into(),
            "agent.new".into(),
            "agent.list".into(),
            "task.list".into(),
        ];
        let q = SearchQuery::new("agent");
        let results = SearchEngine::search_commands(&q, &commands);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.kind == SearchResultKind::Command));
    }

    #[test]
    fn search_commands_prefix() {
        let commands = vec![
            "status".into(),
            "agent.new".into(),
            "agent.list".into(),
        ];
        let q = SearchQuery::new("s").with_exact();
        let results = SearchEngine::search_commands(&q, &commands);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "status");
    }

    // --- Scoring ---

    #[test]
    fn score_exact_id_highest() {
        let score = SearchEngine::score_match("w1", "w1", "Worker 1", true);
        assert_eq!(score, 100);
    }

    #[test]
    fn score_exact_label_high() {
        let score = SearchEngine::score_match("worker 1", "w1", "worker 1", true);
        assert_eq!(score, 90);
    }

    #[test]
    fn score_prefix_id() {
        let score = SearchEngine::score_match("w", "w1", "Worker 1", true);
        assert_eq!(score, 80);
    }

    #[test]
    fn score_prefix_label() {
        let score = SearchEngine::score_match("work", "w1", "worker 1", true);
        assert_eq!(score, 70);
    }

    #[test]
    fn score_fuzzy_id_contains() {
        let score = SearchEngine::score_match("ork", "worker", "Worker", true);
        assert_eq!(score, 50);
    }

    #[test]
    fn score_fuzzy_label_contains() {
        let score = SearchEngine::score_match("anag", "pm", "manager", true);
        assert_eq!(score, 40);
    }

    #[test]
    fn score_no_match() {
        let score = SearchEngine::score_match("xyz", "w1", "Worker 1", true);
        assert_eq!(score, 0);
    }

    #[test]
    fn score_no_fuzzy_no_contains() {
        let score = SearchEngine::score_match("ork", "worker", "Worker", false);
        assert_eq!(score, 0);
    }
}
