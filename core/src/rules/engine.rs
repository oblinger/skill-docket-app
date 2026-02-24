//! RETE evaluation engine (M11.3) with append-field warnings (M11.4).
//!
//! Wires `rules::Expression` + `rules::Rule` evaluation against
//! `namespace::ParameterStore`. Uses a simplified three-layer RETE
//! network: alpha nodes (single-condition filters), beta nodes
//! (variable joins), and conflict resolution (priority ordering).

use std::collections::HashMap;
use serde_json::Value;

use crate::namespace::store::{GetResult, ParameterStore};
use crate::rules::expr::{Condition, Expression, Operator, PathPattern, PathSegment};
use crate::rules::format::{ActionOp, Rule};


// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A rule match: which rule fired and with what variable bindings.
#[derive(Debug, Clone)]
pub struct RuleMatch {
    pub rule_index: usize,
    pub bindings: HashMap<String, String>,
}

/// A warning emitted during evaluation (e.g., SET on an append-field).
#[derive(Debug, Clone)]
pub struct EngineWarning {
    pub path: String,
    pub message: String,
}

/// Result of an evaluation pass.
#[derive(Debug, Clone)]
pub struct EvalResult {
    pub fired_rules: Vec<RuleMatch>,
    pub warnings: Vec<EngineWarning>,
    pub iterations: usize,
}

impl EvalResult {
    fn empty() -> Self {
        EvalResult {
            fired_rules: Vec::new(),
            warnings: Vec::new(),
            iterations: 0,
        }
    }

    fn merge(&mut self, other: &EvalResult) {
        self.fired_rules.extend(other.fired_rules.iter().cloned());
        self.warnings.extend(other.warnings.iter().cloned());
        self.iterations += other.iterations;
    }
}


// ---------------------------------------------------------------------------
// Alpha node
// ---------------------------------------------------------------------------

/// An alpha node tests a single condition against the store and produces
/// `(matched_key, bindings)` pairs.
#[derive(Debug, Clone)]
struct AlphaNode {
    condition: Condition,
}

impl AlphaNode {
    fn new(condition: Condition) -> Self {
        AlphaNode { condition }
    }

    /// Evaluate this condition against the store, returning all
    /// `(matched_key, variable_bindings)` pairs.
    fn evaluate(&self, store: &ParameterStore) -> Vec<(String, HashMap<String, String>)> {
        let pattern_str = path_pattern_to_query(&self.condition.path);
        let matching_keys = store.keys_matching(&pattern_str);

        // Handle the case where the pattern is concrete (no wildcards)
        // and the key does not exist (for IsEmpty checks).
        if matching_keys.is_empty() {
            if self.condition.operator == Operator::IsEmpty {
                if !self.condition.path.has_variables() {
                    // Concrete path, not found — IsEmpty is true.
                    return vec![(pattern_str, HashMap::new())];
                }
            }
            return Vec::new();
        }

        let mut results = Vec::new();

        for key in &matching_keys {
            // Extract variable bindings by matching the key against the pattern.
            let bindings = match extract_bindings(&self.condition.path, key) {
                Some(b) => b,
                None => continue,
            };

            // Get the value from the store.
            let value = match store.get(key) {
                Ok(GetResult::Single(v)) => Some(v),
                _ => None,
            };

            // Evaluate the operator.
            let matches = eval_operator(&self.condition.operator, &value, &self.condition.value);

            if matches {
                results.push((key.clone(), bindings));
            }
        }

        results
    }
}


// ---------------------------------------------------------------------------
// Beta join
// ---------------------------------------------------------------------------

/// Join multiple alpha node outputs on shared variables.
fn beta_join(
    alpha_outputs: &[Vec<(String, HashMap<String, String>)>],
) -> Vec<HashMap<String, String>> {
    if alpha_outputs.is_empty() {
        return vec![HashMap::new()];
    }

    let mut current: Vec<HashMap<String, String>> = Vec::new();

    // Start with the first alpha node's outputs.
    for (_key, bindings) in &alpha_outputs[0] {
        current.push(bindings.clone());
    }

    // Join with each subsequent alpha node.
    for alpha_output in &alpha_outputs[1..] {
        let mut joined = Vec::new();

        for existing in &current {
            for (_key, new_bindings) in alpha_output {
                if let Some(merged) = merge_bindings(existing, new_bindings) {
                    joined.push(merged);
                }
            }
        }

        current = joined;
    }

    // Deduplicate bindings.
    let mut seen = Vec::new();
    let mut unique = Vec::new();
    for b in current {
        if !seen.contains(&b) {
            seen.push(b.clone());
            unique.push(b);
        }
    }

    unique
}


/// Merge two binding maps. Returns `None` if they conflict (same variable,
/// different value).
fn merge_bindings(
    a: &HashMap<String, String>,
    b: &HashMap<String, String>,
) -> Option<HashMap<String, String>> {
    let mut merged = a.clone();
    for (key, val) in b {
        if let Some(existing) = merged.get(key) {
            if existing != val {
                return None; // Conflict — unification fails.
            }
        } else {
            merged.insert(key.clone(), val.clone());
        }
    }
    Some(merged)
}


// ---------------------------------------------------------------------------
// Compiled rule
// ---------------------------------------------------------------------------

/// A rule compiled into alpha/beta RETE nodes.
#[derive(Debug, Clone)]
struct CompiledRule {
    rule: Rule,
}

impl CompiledRule {
    fn compile(rule: Rule) -> Self {
        CompiledRule { rule }
    }

    /// Evaluate this rule against the store, returning all valid binding sets.
    fn evaluate(&self, store: &ParameterStore) -> Vec<HashMap<String, String>> {
        self.evaluate_expression(&self.rule.conditions, store)
    }

    /// Evaluate an expression tree recursively using alpha nodes for
    /// conditions and beta joins for AND-conjunctions with shared variables.
    fn evaluate_expression(
        &self,
        expr: &Expression,
        store: &ParameterStore,
    ) -> Vec<HashMap<String, String>> {
        match expr {
            Expression::Condition(cond) => {
                let alpha = AlphaNode::new(cond.clone());
                let results = alpha.evaluate(store);
                results.into_iter().map(|(_, bindings)| bindings).collect()
            }
            Expression::And(exprs) => {
                let alpha_outputs: Vec<Vec<(String, HashMap<String, String>)>> = exprs
                    .iter()
                    .map(|e| {
                        let binding_sets = self.evaluate_expression(e, store);
                        // Convert back to alpha-output format for beta_join.
                        binding_sets
                            .into_iter()
                            .map(|b| (String::new(), b))
                            .collect()
                    })
                    .collect();
                beta_join(&alpha_outputs)
            }
            Expression::Or(exprs) => {
                let mut all = Vec::new();
                for e in exprs {
                    all.extend(self.evaluate_expression(e, store));
                }
                // Deduplicate.
                let mut seen = Vec::new();
                let mut unique = Vec::new();
                for b in all {
                    if !seen.contains(&b) {
                        seen.push(b.clone());
                        unique.push(b);
                    }
                }
                unique
            }
            Expression::Not(inner) => {
                let inner_results = self.evaluate_expression(inner, store);
                if inner_results.is_empty() {
                    // NOT of nothing-matches = true (with empty bindings).
                    vec![HashMap::new()]
                } else {
                    // NOT of something-matches = false.
                    Vec::new()
                }
            }
        }
    }
}


// ---------------------------------------------------------------------------
// ReteEngine
// ---------------------------------------------------------------------------

/// The RETE evaluation engine.
pub struct ReteEngine {
    compiled_rules: Vec<CompiledRule>,
    append_fields: Vec<String>,
}

impl ReteEngine {
    /// Create a new empty engine.
    pub fn new() -> Self {
        ReteEngine {
            compiled_rules: Vec::new(),
            append_fields: vec![
                "inbox".to_string(),
                "log".to_string(),
                "event".to_string(),
            ],
        }
    }

    /// Compile and add a single rule to the engine.
    pub fn add_rule(&mut self, rule: Rule) {
        self.compiled_rules.push(CompiledRule::compile(rule));
    }

    /// Compile and add multiple rules to the engine.
    pub fn add_rules(&mut self, rules: Vec<Rule>) {
        for rule in rules {
            self.add_rule(rule);
        }
    }

    /// Configure which field names trigger overwrite warnings when SET
    /// is used instead of APPEND.
    pub fn set_append_fields(&mut self, fields: Vec<String>) {
        self.append_fields = fields;
    }

    /// Full evaluation of all rules against the current store state.
    /// Does NOT execute actions — just reports which rules would fire.
    pub fn evaluate(&self, store: &ParameterStore) -> EvalResult {
        let mut matches_with_priority: Vec<(i32, usize, HashMap<String, String>)> = Vec::new();

        for (idx, compiled) in self.compiled_rules.iter().enumerate() {
            let binding_sets = compiled.evaluate(store);
            let priority = compiled.rule.priority.unwrap_or(0);

            for bindings in binding_sets {
                matches_with_priority.push((priority, idx, bindings));
            }
        }

        // Conflict resolution: sort by priority descending (higher first).
        matches_with_priority.sort_by(|a, b| b.0.cmp(&a.0));

        let fired_rules = matches_with_priority
            .into_iter()
            .map(|(_, idx, bindings)| RuleMatch {
                rule_index: idx,
                bindings,
            })
            .collect();

        EvalResult {
            fired_rules,
            warnings: Vec::new(),
            iterations: 1,
        }
    }

    /// Incremental evaluation after a single state change.
    /// Re-evaluates all rules against current store state.
    pub fn propagate_change(&self, store: &ParameterStore, _changed_path: &str) -> EvalResult {
        self.evaluate(store)
    }

    /// Evaluate all rules, execute their actions on the store, and return
    /// the results. This is a single evaluation + execution pass.
    pub fn step(&self, store: &mut ParameterStore) -> EvalResult {
        let eval = self.evaluate(store);

        let mut warnings = Vec::new();

        // Execute all fired rule actions.
        for rule_match in &eval.fired_rules {
            let compiled = &self.compiled_rules[rule_match.rule_index];
            for action in &compiled.rule.actions {
                let resolved_path = substitute_variables(&action.path, &rule_match.bindings);
                let resolved_value = substitute_variables(&action.value, &rule_match.bindings);

                // Check for append-field warnings (M11.4).
                if action.operator == ActionOp::Set {
                    if let Some(last_segment) = resolved_path.rsplit('.').next() {
                        if self.append_fields.iter().any(|f| f == last_segment) {
                            warnings.push(EngineWarning {
                                path: resolved_path.clone(),
                                message: format!(
                                    "SET on append-field '{}'; consider using APPEND (+=) instead",
                                    last_segment
                                ),
                            });
                        }
                    }
                }

                match action.operator {
                    ActionOp::Set => {
                        let _ = store.set(&resolved_path, Value::String(resolved_value));
                    }
                    ActionOp::Append => {
                        let _ = store.append(&resolved_path, Value::String(resolved_value));
                    }
                }
            }
        }

        EvalResult {
            fired_rules: eval.fired_rules,
            warnings,
            iterations: 1,
        }
    }

    /// Repeatedly evaluate and execute rules until no more rules fire
    /// (quiescence) or `max_iterations` is reached.
    pub fn run_to_quiescence(
        &self,
        store: &mut ParameterStore,
        max_iterations: usize,
    ) -> EvalResult {
        let mut total = EvalResult::empty();

        for _ in 0..max_iterations {
            let step_result = self.step(store);
            let fired_count = step_result.fired_rules.len();
            total.merge(&step_result);

            if fired_count == 0 {
                break;
            }
        }

        total
    }
}

impl Default for ReteEngine {
    fn default() -> Self {
        Self::new()
    }
}


// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Convert a `PathPattern` (from expression) to a query string suitable
/// for `ParameterStore::keys_matching`. Variables become wildcards.
fn path_pattern_to_query(pattern: &PathPattern) -> String {
    pattern
        .segments
        .iter()
        .map(|seg| match seg {
            PathSegment::Literal(l) => l.clone(),
            PathSegment::Variable(_) => "*".to_string(),
            PathSegment::Wildcard => "*".to_string(),
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// Extract variable bindings by matching a concrete key against a
/// `PathPattern`.
fn extract_bindings(pattern: &PathPattern, key: &str) -> Option<HashMap<String, String>> {
    let key_parts: Vec<&str> = key.split('.').collect();
    let pat_parts = &pattern.segments;

    if key_parts.len() != pat_parts.len() {
        return None;
    }

    let mut bindings = HashMap::new();

    for (seg, key_part) in pat_parts.iter().zip(key_parts.iter()) {
        match seg {
            PathSegment::Literal(l) => {
                if l != key_part {
                    return None;
                }
            }
            PathSegment::Variable(var) => {
                if let Some(existing) = bindings.get(var.as_str()) {
                    if existing != key_part {
                        return None; // Conflicting binding.
                    }
                } else {
                    bindings.insert(var.clone(), key_part.to_string());
                }
            }
            PathSegment::Wildcard => {
                // Matches anything, no binding.
            }
        }
    }

    Some(bindings)
}

/// Evaluate a comparison operator against a value from the store.
fn eval_operator(op: &Operator, value: &Option<Value>, expected: &Option<String>) -> bool {
    match op {
        Operator::IsEmpty => match value {
            None => true,
            Some(Value::Null) => true,
            Some(Value::String(s)) => s.is_empty(),
            Some(Value::Array(a)) => a.is_empty(),
            _ => false,
        },
        Operator::IsNotEmpty => match value {
            None => false,
            Some(Value::Null) => false,
            Some(Value::String(s)) => !s.is_empty(),
            Some(Value::Array(a)) => !a.is_empty(),
            Some(_) => true,
        },
        _ => {
            let expected_str = match expected {
                Some(s) => s,
                None => return false,
            };

            let actual_str = match value {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Number(n)) => n.to_string(),
                Some(Value::Bool(b)) => b.to_string(),
                Some(Value::Null) => "null".to_string(),
                None => return false,
                _ => return false,
            };

            match op {
                Operator::Eq => {
                    if let (Ok(a), Ok(b)) = (actual_str.parse::<f64>(), expected_str.parse::<f64>()) {
                        (a - b).abs() < f64::EPSILON
                    } else {
                        actual_str == *expected_str
                    }
                }
                Operator::NotEq => {
                    if let (Ok(a), Ok(b)) = (actual_str.parse::<f64>(), expected_str.parse::<f64>()) {
                        (a - b).abs() >= f64::EPSILON
                    } else {
                        actual_str != *expected_str
                    }
                }
                Operator::Gt => {
                    if let (Ok(a), Ok(b)) = (actual_str.parse::<f64>(), expected_str.parse::<f64>()) {
                        a > b
                    } else {
                        actual_str > *expected_str
                    }
                }
                Operator::Lt => {
                    if let (Ok(a), Ok(b)) = (actual_str.parse::<f64>(), expected_str.parse::<f64>()) {
                        a < b
                    } else {
                        actual_str < *expected_str
                    }
                }
                Operator::GtEq => {
                    if let (Ok(a), Ok(b)) = (actual_str.parse::<f64>(), expected_str.parse::<f64>()) {
                        a >= b
                    } else {
                        actual_str >= *expected_str
                    }
                }
                Operator::LtEq => {
                    if let (Ok(a), Ok(b)) = (actual_str.parse::<f64>(), expected_str.parse::<f64>()) {
                        a <= b
                    } else {
                        actual_str <= *expected_str
                    }
                }
                Operator::Contains => {
                    match value {
                        Some(Value::Array(arr)) => {
                            arr.iter().any(|item| {
                                match item {
                                    Value::String(s) => s == expected_str,
                                    Value::Number(n) => n.to_string() == *expected_str,
                                    _ => false,
                                }
                            })
                        }
                        Some(Value::String(s)) => s.contains(expected_str.as_str()),
                        _ => false,
                    }
                }
                // IsEmpty and IsNotEmpty already handled above.
                _ => false,
            }
        }
    }
}

/// Substitute `$var` references in a string with values from bindings.
fn substitute_variables(template: &str, bindings: &HashMap<String, String>) -> String {
    let mut result = template.to_string();
    for (var, val) in bindings {
        let pattern = format!("${}", var);
        result = result.replace(&pattern, val);
    }
    result
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use crate::rules::format::parse_arrow_rules;

    // Helper: make a simple rule from arrow syntax.
    fn arrow_rule(input: &str) -> Rule {
        parse_arrow_rules(input).unwrap().remove(0)
    }

    // Helper: make a rule with a priority.
    fn arrow_rule_with_priority(input: &str, priority: i32) -> Rule {
        let mut r = arrow_rule(input);
        r.priority = Some(priority);
        r
    }

    // -----------------------------------------------------------------------
    // 1. Alpha node test: single condition filters entries correctly
    // -----------------------------------------------------------------------

    #[test]
    fn alpha_node_filters_entries() {
        let mut store = ParameterStore::new();
        store.set("task.AUTH1.status", json!("ready")).unwrap();
        store.set("task.AUTH2.status", json!("done")).unwrap();
        store.set("task.AUTH3.status", json!("ready")).unwrap();

        let cond = Condition::parse("task.*.status == ready").unwrap();
        let alpha = AlphaNode::new(cond);
        let results = alpha.evaluate(&store);

        assert_eq!(results.len(), 2);
        let keys: Vec<&str> = results.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"task.AUTH1.status"));
        assert!(keys.contains(&"task.AUTH3.status"));
    }

    // -----------------------------------------------------------------------
    // 2. Variable binding test: $t binds correctly across matching keys
    // -----------------------------------------------------------------------

    #[test]
    fn variable_binding_extracts_correctly() {
        let mut store = ParameterStore::new();
        store.set("task.AUTH1.status", json!("ready")).unwrap();
        store.set("task.AUTH2.status", json!("done")).unwrap();

        let cond = Condition::parse("task.$t.status == ready").unwrap();
        let alpha = AlphaNode::new(cond);
        let results = alpha.evaluate(&store);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "task.AUTH1.status");
        assert_eq!(results[0].1.get("t").unwrap(), "AUTH1");
    }

    // -----------------------------------------------------------------------
    // 3. Beta join test: two conditions with shared $t unify correctly
    // -----------------------------------------------------------------------

    #[test]
    fn beta_join_unifies_shared_variables() {
        let mut store = ParameterStore::new();
        store.set("task.AUTH1.status", json!("ready")).unwrap();
        store.set("task.AUTH1.priority", json!("high")).unwrap();
        store.set("task.AUTH2.status", json!("ready")).unwrap();
        store.set("task.AUTH2.priority", json!("low")).unwrap();

        let rule = arrow_rule(
            "task.$t.status == ready AND task.$t.priority == high --> task.$t.status = in_progress",
        );

        let mut engine = ReteEngine::new();
        engine.add_rule(rule);

        let result = engine.evaluate(&store);
        assert_eq!(result.fired_rules.len(), 1);
        assert_eq!(
            result.fired_rules[0].bindings.get("t").unwrap(),
            "AUTH1"
        );
    }

    // -----------------------------------------------------------------------
    // 4. Conflict resolution test: higher-priority rule fires first
    // -----------------------------------------------------------------------

    #[test]
    fn conflict_resolution_by_priority() {
        let mut store = ParameterStore::new();
        store.set("task.AUTH1.status", json!("ready")).unwrap();

        let low = arrow_rule_with_priority(
            "task.$t.status == ready --> task.$t.status = queued",
            1,
        );
        let high = arrow_rule_with_priority(
            "task.$t.status == ready --> task.$t.status = in_progress",
            10,
        );

        let mut engine = ReteEngine::new();
        engine.add_rule(low);
        engine.add_rule(high);

        let result = engine.evaluate(&store);
        assert_eq!(result.fired_rules.len(), 2);
        // Higher priority (rule index 1, priority 10) fires first.
        assert_eq!(result.fired_rules[0].rule_index, 1);
        assert_eq!(result.fired_rules[1].rule_index, 0);
    }

    // -----------------------------------------------------------------------
    // 5. M11.3.5: task.$t.status == ready AND task.$t.priority == high
    //    fires only for high-priority task
    // -----------------------------------------------------------------------

    #[test]
    fn m11_3_5_ready_high_priority_only() {
        let mut store = ParameterStore::new();
        store.set("task.AUTH1.status", json!("ready")).unwrap();
        store.set("task.AUTH1.priority", json!("high")).unwrap();
        store.set("task.AUTH2.status", json!("ready")).unwrap();
        store.set("task.AUTH2.priority", json!("low")).unwrap();

        let rule = arrow_rule(
            "task.$t.status == ready AND task.$t.priority == high --> task.$t.status = in_progress",
        );

        let mut engine = ReteEngine::new();
        engine.add_rule(rule);

        let result = engine.step(&mut store);

        assert_eq!(result.fired_rules.len(), 1);
        assert_eq!(
            result.fired_rules[0].bindings.get("t").unwrap(),
            "AUTH1"
        );

        // Verify AUTH1 transitioned, AUTH2 did not.
        match store.get("task.AUTH1.status").unwrap() {
            GetResult::Single(v) => assert_eq!(v, json!("in_progress")),
            other => panic!("expected Single, got {:?}", other),
        }
        match store.get("task.AUTH2.status").unwrap() {
            GetResult::Single(v) => assert_eq!(v, json!("ready")),
            other => panic!("expected Single, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // 6. M11.3.6: Rule chain — task complete -> agent idle -> task cleared
    // -----------------------------------------------------------------------

    #[test]
    fn m11_3_6_rule_chain() {
        let mut store = ParameterStore::new();
        store.set("task.AUTH1.status", json!("complete")).unwrap();
        store.set("task.AUTH1.assignee", json!("worker1")).unwrap();
        store.set("agent.worker1.status", json!("busy")).unwrap();
        store.set("agent.worker1.task", json!("AUTH1")).unwrap();

        // Rule A: task complete -> agent idle
        let rule_a = arrow_rule(
            "task.$t.status == complete --> agent.worker1.status = idle",
        );
        // Rule B: agent idle -> agent task cleared
        let rule_b = arrow_rule(
            "agent.$a.status == idle --> agent.$a.task = none",
        );

        let mut engine = ReteEngine::new();
        engine.add_rule(rule_a);
        engine.add_rule(rule_b);

        let result = engine.run_to_quiescence(&mut store, 10);

        // Both rules should have fired across iterations.
        assert!(result.fired_rules.len() >= 2);

        // Verify final state.
        match store.get("agent.worker1.status").unwrap() {
            GetResult::Single(v) => assert_eq!(v, json!("idle")),
            other => panic!("expected Single, got {:?}", other),
        }
        match store.get("agent.worker1.task").unwrap() {
            GetResult::Single(v) => assert_eq!(v, json!("none")),
            other => panic!("expected Single, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // 7. Quiescence test: cascading rules reach stable state
    // -----------------------------------------------------------------------

    #[test]
    fn run_to_quiescence_cascading() {
        let mut store = ParameterStore::new();
        store.set("task.T1.status", json!("submitted")).unwrap();

        // Rule chain: submitted -> queued -> ready -> in_progress (then stops)
        let r1 = arrow_rule(
            "task.$t.status == submitted --> task.$t.status = queued",
        );
        let r2 = arrow_rule(
            "task.$t.status == queued --> task.$t.status = ready",
        );
        let r3 = arrow_rule(
            "task.$t.status == ready --> task.$t.status = in_progress",
        );

        let mut engine = ReteEngine::new();
        engine.add_rules(vec![r1, r2, r3]);

        let result = engine.run_to_quiescence(&mut store, 10);

        // Should eventually reach in_progress and stop.
        match store.get("task.T1.status").unwrap() {
            GetResult::Single(v) => assert_eq!(v, json!("in_progress")),
            other => panic!("expected Single, got {:?}", other),
        }

        // Should have fired 3 rules total across iterations.
        assert_eq!(result.fired_rules.len(), 3);
        assert!(result.iterations <= 10);
    }

    // -----------------------------------------------------------------------
    // 8. Max iterations guard test: infinite loop protection
    // -----------------------------------------------------------------------

    #[test]
    fn max_iterations_guard() {
        let mut store = ParameterStore::new();
        store.set("task.T1.status", json!("a")).unwrap();

        // Infinite loop: a -> b -> a -> b -> ...
        let r1 = arrow_rule("task.$t.status == a --> task.$t.status = b");
        let r2 = arrow_rule("task.$t.status == b --> task.$t.status = a");

        let mut engine = ReteEngine::new();
        engine.add_rules(vec![r1, r2]);

        let result = engine.run_to_quiescence(&mut store, 5);

        // Should stop at max_iterations.
        assert_eq!(result.iterations, 5);
        assert_eq!(result.fired_rules.len(), 5);
    }

    // -----------------------------------------------------------------------
    // 9. M11.4.1: SET on append-field triggers warning
    // -----------------------------------------------------------------------

    #[test]
    fn m11_4_1_set_on_append_field_warns() {
        let mut store = ParameterStore::new();
        store.set("agent.worker1.status", json!("idle")).unwrap();

        let rule = arrow_rule(
            "agent.$a.status == idle --> agent.$a.inbox = hello",
        );

        let mut engine = ReteEngine::new();
        engine.add_rule(rule);

        let result = engine.step(&mut store);

        assert_eq!(result.fired_rules.len(), 1);
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].path.contains("inbox"));
        assert!(result.warnings[0].message.contains("APPEND"));
    }

    // -----------------------------------------------------------------------
    // 10. M11.4.2: APPEND on append-field does NOT trigger warning
    // -----------------------------------------------------------------------

    #[test]
    fn m11_4_2_append_on_append_field_no_warning() {
        let mut store = ParameterStore::new();
        store.set("agent.worker1.status", json!("idle")).unwrap();

        let rule = arrow_rule(
            "agent.$a.status == idle --> agent.$a.inbox += hello",
        );

        let mut engine = ReteEngine::new();
        engine.add_rule(rule);

        let result = engine.step(&mut store);

        assert_eq!(result.fired_rules.len(), 1);
        assert!(result.warnings.is_empty());
    }

    // -----------------------------------------------------------------------
    // 11. Custom append fields test
    // -----------------------------------------------------------------------

    #[test]
    fn custom_append_fields() {
        let mut store = ParameterStore::new();
        store.set("task.T1.status", json!("ready")).unwrap();

        // Rule that SETs a custom append field.
        let rule = arrow_rule(
            "task.$t.status == ready --> task.$t.history = started",
        );

        let mut engine = ReteEngine::new();
        engine.set_append_fields(vec!["history".to_string(), "audit".to_string()]);
        engine.add_rule(rule);

        let result = engine.step(&mut store);

        assert_eq!(result.fired_rules.len(), 1);
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].path.contains("history"));

        // Default fields should NOT trigger with custom config.
        let mut store2 = ParameterStore::new();
        store2.set("agent.w1.status", json!("idle")).unwrap();

        let rule2 = arrow_rule(
            "agent.$a.status == idle --> agent.$a.inbox = msg",
        );

        let mut engine2 = ReteEngine::new();
        engine2.set_append_fields(vec!["history".to_string()]);
        engine2.add_rule(rule2);

        let result2 = engine2.step(&mut store2);
        assert!(result2.warnings.is_empty(), "inbox should not warn with custom fields");
    }

    // -----------------------------------------------------------------------
    // Additional tests for completeness
    // -----------------------------------------------------------------------

    #[test]
    fn evaluate_without_executing() {
        let mut store = ParameterStore::new();
        store.set("task.T1.status", json!("ready")).unwrap();

        let rule = arrow_rule(
            "task.$t.status == ready --> task.$t.status = in_progress",
        );

        let mut engine = ReteEngine::new();
        engine.add_rule(rule);

        // evaluate() should not modify the store.
        let result = engine.evaluate(&store);
        assert_eq!(result.fired_rules.len(), 1);

        // Store unchanged.
        match store.get("task.T1.status").unwrap() {
            GetResult::Single(v) => assert_eq!(v, json!("ready")),
            other => panic!("expected Single, got {:?}", other),
        }
    }

    #[test]
    fn propagate_change_returns_matches() {
        let mut store = ParameterStore::new();
        store.set("task.T1.status", json!("ready")).unwrap();

        let rule = arrow_rule(
            "task.$t.status == ready --> task.$t.status = in_progress",
        );

        let mut engine = ReteEngine::new();
        engine.add_rule(rule);

        let result = engine.propagate_change(&store, "task.T1.status");
        assert_eq!(result.fired_rules.len(), 1);
    }

    #[test]
    fn no_rules_fire_when_conditions_unmet() {
        let mut store = ParameterStore::new();
        store.set("task.T1.status", json!("done")).unwrap();

        let rule = arrow_rule(
            "task.$t.status == ready --> task.$t.status = in_progress",
        );

        let mut engine = ReteEngine::new();
        engine.add_rule(rule);

        let result = engine.evaluate(&store);
        assert!(result.fired_rules.is_empty());
    }

    #[test]
    fn multiple_entities_fire_same_rule() {
        let mut store = ParameterStore::new();
        store.set("task.T1.status", json!("ready")).unwrap();
        store.set("task.T2.status", json!("ready")).unwrap();
        store.set("task.T3.status", json!("done")).unwrap();

        let rule = arrow_rule(
            "task.$t.status == ready --> task.$t.status = in_progress",
        );

        let mut engine = ReteEngine::new();
        engine.add_rule(rule);

        let result = engine.step(&mut store);
        assert_eq!(result.fired_rules.len(), 2);

        // Both T1 and T2 should transition.
        match store.get("task.T1.status").unwrap() {
            GetResult::Single(v) => assert_eq!(v, json!("in_progress")),
            other => panic!("expected Single, got {:?}", other),
        }
        match store.get("task.T2.status").unwrap() {
            GetResult::Single(v) => assert_eq!(v, json!("in_progress")),
            other => panic!("expected Single, got {:?}", other),
        }
        // T3 unchanged.
        match store.get("task.T3.status").unwrap() {
            GetResult::Single(v) => assert_eq!(v, json!("done")),
            other => panic!("expected Single, got {:?}", other),
        }
    }

    #[test]
    fn variable_substitution_in_actions() {
        let mut store = ParameterStore::new();
        store.set("task.AUTH1.status", json!("ready")).unwrap();

        let rule = arrow_rule(
            "task.$t.status == ready --> task.$t.status = in_progress",
        );

        let mut engine = ReteEngine::new();
        engine.add_rule(rule);

        let result = engine.step(&mut store);
        assert_eq!(result.fired_rules.len(), 1);

        match store.get("task.AUTH1.status").unwrap() {
            GetResult::Single(v) => assert_eq!(v, json!("in_progress")),
            other => panic!("expected Single, got {:?}", other),
        }
    }

    #[test]
    fn append_action_creates_array() {
        let mut store = ParameterStore::new();
        store.set("task.T1.status", json!("ready")).unwrap();

        let rule = arrow_rule(
            "task.$t.status == ready --> task.$t.log += started",
        );

        let mut engine = ReteEngine::new();
        engine.add_rule(rule);

        let result = engine.step(&mut store);
        assert_eq!(result.fired_rules.len(), 1);

        match store.get("task.T1.log").unwrap() {
            GetResult::Single(v) => assert_eq!(v, json!(["started"])),
            other => panic!("expected Single, got {:?}", other),
        }
    }

    #[test]
    fn empty_engine_no_matches() {
        let store = ParameterStore::new();
        let engine = ReteEngine::new();
        let result = engine.evaluate(&store);
        assert!(result.fired_rules.is_empty());
    }

    #[test]
    fn run_to_quiescence_no_rules_fires_zero() {
        let mut store = ParameterStore::new();
        store.set("task.T1.status", json!("ready")).unwrap();

        let engine = ReteEngine::new();
        let result = engine.run_to_quiescence(&mut store, 10);
        assert!(result.fired_rules.is_empty());
        assert_eq!(result.iterations, 1);
    }

    #[test]
    fn numeric_comparison() {
        let mut store = ParameterStore::new();
        store.set("agent.w1.retries", json!(5)).unwrap();
        store.set("agent.w2.retries", json!(1)).unwrap();

        let rule = arrow_rule(
            "agent.$a.retries > 3 --> agent.$a.status = error",
        );

        let mut engine = ReteEngine::new();
        engine.add_rule(rule);

        let result = engine.step(&mut store);
        assert_eq!(result.fired_rules.len(), 1);
        assert_eq!(result.fired_rules[0].bindings.get("a").unwrap(), "w1");
    }

    #[test]
    fn set_on_non_append_field_no_warning() {
        let mut store = ParameterStore::new();
        store.set("task.T1.status", json!("ready")).unwrap();

        let rule = arrow_rule(
            "task.$t.status == ready --> task.$t.status = in_progress",
        );

        let mut engine = ReteEngine::new();
        engine.add_rule(rule);

        let result = engine.step(&mut store);
        assert!(result.warnings.is_empty());
    }
}
