//! Typed parameter store (M10.2).
//!
//! In-memory key-value store using `serde_json::Value` for typed values.
//! Supports GET (with wildcard patterns), SET, APPEND, and dirty tracking
//! for batch flush.

use std::collections::{HashMap, HashSet};
use serde_json::Value;
use super::path::NamespacePath;

/// Alias for stored values — `serde_json::Value` supports all JSON types.
pub type StoreValue = Value;


/// Result of a GET operation — may return one value, many (wildcard), or nothing.
#[derive(Debug, Clone)]
pub enum GetResult {
    /// Exact path matched a single value.
    Single(StoreValue),
    /// Wildcard pattern matched multiple entries.
    Multiple(Vec<(String, StoreValue)>),
    /// No match found.
    NotFound,
}


/// In-memory parameter store keyed by dotted path strings.
#[derive(Debug, Clone)]
pub struct ParameterStore {
    /// All state, keyed by full dotted path string.
    data: HashMap<String, StoreValue>,
    /// Paths that have been modified since last flush.
    dirty: HashSet<String>,
}

impl ParameterStore {
    /// Create an empty store.
    pub fn new() -> Self {
        ParameterStore {
            data: HashMap::new(),
            dirty: HashSet::new(),
        }
    }

    /// GET a value by path.
    ///
    /// If the path is a wildcard pattern, returns all matching entries.
    /// If it is a concrete path, returns the single value or NotFound.
    pub fn get(&self, path: &str) -> Result<GetResult, String> {
        let parsed = NamespacePath::parse(path)?;

        if parsed.is_pattern() {
            let matches = self.keys_matching_parsed(&parsed);
            if matches.is_empty() {
                Ok(GetResult::NotFound)
            } else {
                let entries: Vec<(String, StoreValue)> = matches
                    .into_iter()
                    .filter_map(|k| self.data.get(&k).map(|v| (k, v.clone())))
                    .collect();
                Ok(GetResult::Multiple(entries))
            }
        } else {
            match self.data.get(path) {
                Some(v) => Ok(GetResult::Single(v.clone())),
                None => Ok(GetResult::NotFound),
            }
        }
    }

    /// SET a value at a concrete path.
    ///
    /// Wildcard paths cannot be used as SET targets.
    pub fn set(&mut self, path: &str, value: StoreValue) -> Result<(), String> {
        let parsed = NamespacePath::parse(path)?;
        if parsed.is_pattern() {
            return Err("cannot SET on a wildcard pattern".to_string());
        }
        self.data.insert(path.to_string(), value);
        self.dirty.insert(path.to_string());
        Ok(())
    }

    /// APPEND a value to a path with array semantics.
    ///
    /// - If the path doesn't exist, creates `[value]`.
    /// - If the path holds a non-array value, converts to `[existing, value]`.
    /// - If the path holds an array, appends `value` to it.
    pub fn append(&mut self, path: &str, value: StoreValue) -> Result<(), String> {
        let parsed = NamespacePath::parse(path)?;
        if parsed.is_pattern() {
            return Err("cannot APPEND on a wildcard pattern".to_string());
        }

        let new_val = match self.data.remove(path) {
            None => Value::Array(vec![value]),
            Some(Value::Array(mut arr)) => {
                arr.push(value);
                Value::Array(arr)
            }
            Some(existing) => Value::Array(vec![existing, value]),
        };

        self.data.insert(path.to_string(), new_val);
        self.dirty.insert(path.to_string());
        Ok(())
    }

    /// Get all dirty paths since last flush.
    pub fn dirty_paths(&self) -> &HashSet<String> {
        &self.dirty
    }

    /// Clear dirty tracking (called after flush).
    pub fn clear_dirty(&mut self) {
        self.dirty.clear();
    }

    /// Get all keys matching a pattern string.
    pub fn keys_matching(&self, pattern: &str) -> Vec<String> {
        match NamespacePath::parse(pattern) {
            Ok(parsed) => self.keys_matching_parsed(&parsed),
            Err(_) => Vec::new(),
        }
    }

    /// Remove a value at a concrete path.
    pub fn remove(&mut self, path: &str) -> Option<StoreValue> {
        let removed = self.data.remove(path);
        if removed.is_some() {
            self.dirty.insert(path.to_string());
        }
        removed
    }

    /// Bulk load from a HashMap (used on startup).
    ///
    /// Replaces all existing data. Does not mark anything as dirty.
    pub fn load(&mut self, data: HashMap<String, StoreValue>) {
        self.data = data;
        self.dirty.clear();
    }

    /// Export all data as a reference to the internal map.
    pub fn export(&self) -> &HashMap<String, StoreValue> {
        &self.data
    }

    /// Number of entries in the store.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// True if the store has no entries.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    // -------------------------------------------------------------------
    // Internal
    // -------------------------------------------------------------------

    fn keys_matching_parsed(&self, pattern: &NamespacePath) -> Vec<String> {
        self.data
            .keys()
            .filter(|key| {
                if let Ok(concrete) = NamespacePath::parse(key) {
                    pattern.match_path(&concrete).is_some()
                } else {
                    false
                }
            })
            .cloned()
            .collect()
    }
}

impl Default for ParameterStore {
    fn default() -> Self {
        Self::new()
    }
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn set_and_get_string() {
        let mut store = ParameterStore::new();
        store
            .set("task.AUTH1.status", json!("in_progress"))
            .unwrap();
        match store.get("task.AUTH1.status").unwrap() {
            GetResult::Single(v) => assert_eq!(v, json!("in_progress")),
            other => panic!("expected Single, got {:?}", other),
        }
    }

    #[test]
    fn set_and_get_number() {
        let mut store = ParameterStore::new();
        store.set("config.timeout", json!(60000)).unwrap();
        match store.get("config.timeout").unwrap() {
            GetResult::Single(v) => {
                assert_eq!(v, json!(60000));
                assert!(v.as_i64().unwrap() > 50000);
            }
            other => panic!("expected Single, got {:?}", other),
        }
    }

    #[test]
    fn set_and_get_object() {
        let mut store = ParameterStore::new();
        let obj = json!({"role": "worker", "host": "gpu-1"});
        store.set("agent.worker1.config", obj.clone()).unwrap();
        match store.get("agent.worker1.config").unwrap() {
            GetResult::Single(v) => assert_eq!(v, obj),
            other => panic!("expected Single, got {:?}", other),
        }
    }

    #[test]
    fn set_and_get_bool() {
        let mut store = ParameterStore::new();
        store.set("config.debug", json!(true)).unwrap();
        match store.get("config.debug").unwrap() {
            GetResult::Single(v) => assert_eq!(v, json!(true)),
            other => panic!("expected Single, got {:?}", other),
        }
    }

    #[test]
    fn set_and_get_null() {
        let mut store = ParameterStore::new();
        store.set("session.token", json!(null)).unwrap();
        match store.get("session.token").unwrap() {
            GetResult::Single(v) => assert!(v.is_null()),
            other => panic!("expected Single, got {:?}", other),
        }
    }

    #[test]
    fn get_nonexistent() {
        let store = ParameterStore::new();
        match store.get("task.NOPE.status").unwrap() {
            GetResult::NotFound => {}
            other => panic!("expected NotFound, got {:?}", other),
        }
    }

    #[test]
    fn append_to_nonexistent() {
        let mut store = ParameterStore::new();
        store.append("task.AUTH1.log", json!("first entry")).unwrap();
        match store.get("task.AUTH1.log").unwrap() {
            GetResult::Single(v) => assert_eq!(v, json!(["first entry"])),
            other => panic!("expected Single, got {:?}", other),
        }
    }

    #[test]
    fn append_to_string_converts() {
        let mut store = ParameterStore::new();
        store.set("task.AUTH1.notes", json!("existing")).unwrap();
        store.append("task.AUTH1.notes", json!("new")).unwrap();
        match store.get("task.AUTH1.notes").unwrap() {
            GetResult::Single(v) => assert_eq!(v, json!(["existing", "new"])),
            other => panic!("expected Single, got {:?}", other),
        }
    }

    #[test]
    fn append_to_array_extends() {
        let mut store = ParameterStore::new();
        store
            .set("task.AUTH1.log", json!(["a", "b"]))
            .unwrap();
        store.append("task.AUTH1.log", json!("c")).unwrap();
        match store.get("task.AUTH1.log").unwrap() {
            GetResult::Single(v) => assert_eq!(v, json!(["a", "b", "c"])),
            other => panic!("expected Single, got {:?}", other),
        }
    }

    #[test]
    fn wildcard_get_returns_all_matching() {
        let mut store = ParameterStore::new();
        store.set("agent.worker1.health", json!("ok")).unwrap();
        store.set("agent.worker2.health", json!("stale")).unwrap();
        store.set("agent.pm1.health", json!("ok")).unwrap();
        store
            .set("task.AUTH1.status", json!("in_progress"))
            .unwrap();

        match store.get("agent.*.health").unwrap() {
            GetResult::Multiple(entries) => {
                assert_eq!(entries.len(), 3);
                let keys: HashSet<String> =
                    entries.iter().map(|(k, _)| k.clone()).collect();
                assert!(keys.contains("agent.worker1.health"));
                assert!(keys.contains("agent.worker2.health"));
                assert!(keys.contains("agent.pm1.health"));
            }
            other => panic!("expected Multiple, got {:?}", other),
        }
    }

    #[test]
    fn wildcard_get_no_match() {
        let mut store = ParameterStore::new();
        store.set("task.AUTH1.status", json!("done")).unwrap();

        match store.get("agent.*.health").unwrap() {
            GetResult::NotFound => {}
            other => panic!("expected NotFound, got {:?}", other),
        }
    }

    #[test]
    fn set_marks_dirty() {
        let mut store = ParameterStore::new();
        assert!(store.dirty_paths().is_empty());
        store.set("config.timeout", json!(5000)).unwrap();
        assert!(store.dirty_paths().contains("config.timeout"));
    }

    #[test]
    fn append_marks_dirty() {
        let mut store = ParameterStore::new();
        store.append("task.AUTH1.log", json!("entry")).unwrap();
        assert!(store.dirty_paths().contains("task.AUTH1.log"));
    }

    #[test]
    fn clear_dirty_resets() {
        let mut store = ParameterStore::new();
        store.set("config.x", json!(1)).unwrap();
        store.set("config.y", json!(2)).unwrap();
        assert_eq!(store.dirty_paths().len(), 2);
        store.clear_dirty();
        assert!(store.dirty_paths().is_empty());
    }

    #[test]
    fn remove_works() {
        let mut store = ParameterStore::new();
        store.set("task.AUTH1.status", json!("done")).unwrap();
        store.clear_dirty();

        let removed = store.remove("task.AUTH1.status");
        assert_eq!(removed, Some(json!("done")));
        assert!(store.dirty_paths().contains("task.AUTH1.status"));

        match store.get("task.AUTH1.status").unwrap() {
            GetResult::NotFound => {}
            other => panic!("expected NotFound, got {:?}", other),
        }
    }

    #[test]
    fn remove_nonexistent_returns_none() {
        let mut store = ParameterStore::new();
        assert!(store.remove("task.NOPE").is_none());
        assert!(store.dirty_paths().is_empty());
    }

    #[test]
    fn load_replaces_data_and_clears_dirty() {
        let mut store = ParameterStore::new();
        store.set("config.a", json!(1)).unwrap();

        let mut data = HashMap::new();
        data.insert("config.b".to_string(), json!(2));
        data.insert("config.c".to_string(), json!(3));
        store.load(data);

        assert!(store.dirty_paths().is_empty());
        assert_eq!(store.len(), 2);
        match store.get("config.a").unwrap() {
            GetResult::NotFound => {}
            other => panic!("expected old key gone, got {:?}", other),
        }
        match store.get("config.b").unwrap() {
            GetResult::Single(v) => assert_eq!(v, json!(2)),
            other => panic!("expected Single, got {:?}", other),
        }
    }

    #[test]
    fn export_returns_all_data() {
        let mut store = ParameterStore::new();
        store.set("config.a", json!(1)).unwrap();
        store.set("config.b", json!(2)).unwrap();
        let exported = store.export();
        assert_eq!(exported.len(), 2);
        assert_eq!(exported.get("config.a").unwrap(), &json!(1));
    }

    #[test]
    fn set_on_wildcard_path_fails() {
        let mut store = ParameterStore::new();
        assert!(store.set("agent.*.health", json!("ok")).is_err());
    }

    #[test]
    fn append_on_wildcard_path_fails() {
        let mut store = ParameterStore::new();
        assert!(store.append("agent.*.log", json!("x")).is_err());
    }

    #[test]
    fn keys_matching_pattern() {
        let mut store = ParameterStore::new();
        store.set("task.AUTH1.status", json!("done")).unwrap();
        store.set("task.AUTH2.status", json!("pending")).unwrap();
        store.set("task.AUTH1.assignee", json!("worker1")).unwrap();
        store.set("agent.worker1.health", json!("ok")).unwrap();

        let mut keys = store.keys_matching("task.*.status");
        keys.sort();
        assert_eq!(keys, vec!["task.AUTH1.status", "task.AUTH2.status"]);
    }

    #[test]
    fn keys_matching_double_wildcard() {
        let mut store = ParameterStore::new();
        store.set("task.AUTH1.status", json!("done")).unwrap();
        store.set("task.AUTH1.sub.deep", json!(42)).unwrap();
        store.set("agent.w1.health", json!("ok")).unwrap();

        let mut keys = store.keys_matching("task.**");
        keys.sort();
        assert_eq!(
            keys,
            vec!["task.AUTH1.status", "task.AUTH1.sub.deep"]
        );
    }

    #[test]
    fn overwrite_value() {
        let mut store = ParameterStore::new();
        store.set("config.timeout", json!(1000)).unwrap();
        store.set("config.timeout", json!(5000)).unwrap();
        match store.get("config.timeout").unwrap() {
            GetResult::Single(v) => assert_eq!(v, json!(5000)),
            other => panic!("expected Single, got {:?}", other),
        }
    }

    #[test]
    fn len_and_is_empty() {
        let mut store = ParameterStore::new();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);

        store.set("config.x", json!(1)).unwrap();
        assert!(!store.is_empty());
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn get_invalid_path_returns_error() {
        let store = ParameterStore::new();
        assert!(store.get("").is_err());
        assert!(store.get("bogus.x").is_err());
    }

    #[test]
    fn multiple_dirty_operations() {
        let mut store = ParameterStore::new();
        store.set("config.a", json!(1)).unwrap();
        store.set("config.b", json!(2)).unwrap();
        store.append("config.c", json!(3)).unwrap();
        store.set("config.a", json!(10)).unwrap(); // overwrite
        // a, b, c are dirty
        assert_eq!(store.dirty_paths().len(), 3);
        assert!(store.dirty_paths().contains("config.a"));
        assert!(store.dirty_paths().contains("config.b"));
        assert!(store.dirty_paths().contains("config.c"));
    }
}
