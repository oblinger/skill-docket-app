//! Remote host configuration and registry.
//!
//! `RemoteConfig` describes a single remote host (SSH coordinates, workspace path,
//! GPU count, labels). `RigRegistry` stores a collection of remotes with an
//! optional default, providing CRUD, label-based filtering, and YAML round-trip
//! serialisation (hand-rolled, no external YAML crate required).

use serde::{Deserialize, Serialize};


// ---------------------------------------------------------------------------
// RemoteConfig
// ---------------------------------------------------------------------------

/// Configuration for a single remote host.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RemoteConfig {
    /// Short name used to reference this remote (e.g. "r1", "gpu-a100").
    pub name: String,
    /// Hostname or IP address.
    pub host: String,
    /// SSH port.
    pub port: u16,
    /// SSH user.
    pub user: String,
    /// Path to an SSH private key, if not using the default.
    pub ssh_key: Option<String>,
    /// Working directory on the remote host.
    pub workspace_dir: String,
    /// Number of GPUs available, if known.
    pub gpu_count: Option<u32>,
    /// Arbitrary labels for filtering (e.g. "a100", "high-mem").
    pub labels: Vec<String>,
}

impl RemoteConfig {
    /// Build the `user@host` string used in SSH/rsync commands.
    pub fn user_at_host(&self) -> String {
        format!("{}@{}", self.user, self.host)
    }

    /// Build base SSH arguments (port, key, user@host) without a command.
    pub fn ssh_base_args(&self) -> Vec<String> {
        let mut args = vec![
            "-p".to_string(),
            self.port.to_string(),
            "-o".to_string(),
            "StrictHostKeyChecking=no".to_string(),
            "-o".to_string(),
            "ConnectTimeout=10".to_string(),
        ];
        if let Some(ref key) = self.ssh_key {
            args.push("-i".to_string());
            args.push(key.clone());
        }
        args.push(self.user_at_host());
        args
    }
}


// ---------------------------------------------------------------------------
// RigRegistry
// ---------------------------------------------------------------------------

/// A collection of remote host configurations with an optional default.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RigRegistry {
    remotes: Vec<RemoteConfig>,
    default_remote: Option<String>,
}

impl RigRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        RigRegistry {
            remotes: Vec::new(),
            default_remote: None,
        }
    }

    /// Add a remote. Fails if a remote with the same name already exists.
    pub fn add(&mut self, config: RemoteConfig) -> Result<(), String> {
        if self.remotes.iter().any(|r| r.name == config.name) {
            return Err(format!("remote '{}' already exists", config.name));
        }
        self.remotes.push(config);
        Ok(())
    }

    /// Remove a remote by name, returning it. Clears the default if it matched.
    pub fn remove(&mut self, name: &str) -> Result<RemoteConfig, String> {
        let idx = self
            .remotes
            .iter()
            .position(|r| r.name == name)
            .ok_or_else(|| format!("remote '{}' not found", name))?;
        let removed = self.remotes.remove(idx);
        if self.default_remote.as_deref() == Some(name) {
            self.default_remote = None;
        }
        Ok(removed)
    }

    /// Look up a remote by name.
    pub fn get(&self, name: &str) -> Option<&RemoteConfig> {
        self.remotes.iter().find(|r| r.name == name)
    }

    /// Look up a remote by name (mutable).
    pub fn get_mut(&mut self, name: &str) -> Option<&mut RemoteConfig> {
        self.remotes.iter_mut().find(|r| r.name == name)
    }

    /// Return all registered remotes.
    pub fn list(&self) -> &[RemoteConfig] {
        &self.remotes
    }

    /// The name of the current default remote, if set.
    pub fn default_name(&self) -> Option<&str> {
        self.default_remote.as_deref()
    }

    /// Set the default remote. Fails if the named remote does not exist.
    pub fn set_default(&mut self, name: &str) -> Result<(), String> {
        if !self.remotes.iter().any(|r| r.name == name) {
            return Err(format!("remote '{}' not found", name));
        }
        self.default_remote = Some(name.to_string());
        Ok(())
    }

    /// Resolve an optional name to a concrete remote config. If `name` is `None`,
    /// uses the default. Fails if nothing can be resolved.
    pub fn resolve(&self, name: Option<&str>) -> Result<&RemoteConfig, String> {
        let target = match name {
            Some(n) => n,
            None => self
                .default_remote
                .as_deref()
                .ok_or_else(|| "no remote specified and no default set".to_string())?,
        };
        self.get(target)
            .ok_or_else(|| format!("remote '{}' not found", target))
    }

    /// Return all remotes that carry the given label.
    pub fn by_label(&self, label: &str) -> Vec<&RemoteConfig> {
        self.remotes
            .iter()
            .filter(|r| r.labels.iter().any(|l| l == label))
            .collect()
    }

    /// Serialise the registry to a minimal YAML string. Hand-rolled to avoid
    /// pulling in a YAML crate.
    pub fn serialize_yaml(&self) -> String {
        let mut out = String::new();
        if let Some(ref def) = self.default_remote {
            out.push_str(&format!("default: {}\n", def));
        }
        out.push_str("remotes:\n");
        for r in &self.remotes {
            out.push_str(&format!("  - name: {}\n", r.name));
            out.push_str(&format!("    host: {}\n", r.host));
            out.push_str(&format!("    port: {}\n", r.port));
            out.push_str(&format!("    user: {}\n", r.user));
            if let Some(ref key) = r.ssh_key {
                out.push_str(&format!("    ssh_key: {}\n", key));
            }
            out.push_str(&format!("    workspace_dir: {}\n", r.workspace_dir));
            if let Some(gpus) = r.gpu_count {
                out.push_str(&format!("    gpu_count: {}\n", gpus));
            }
            if !r.labels.is_empty() {
                out.push_str("    labels:\n");
                for label in &r.labels {
                    out.push_str(&format!("      - {}\n", label));
                }
            }
        }
        out
    }

    /// Parse a YAML string produced by `serialize_yaml` back into a `RigRegistry`.
    ///
    /// This is a deliberately simple line-oriented parser that handles exactly
    /// the format emitted by `serialize_yaml`. It is not a general YAML parser.
    pub fn parse_yaml(yaml: &str) -> Result<RigRegistry, String> {
        let mut registry = RigRegistry::new();
        let mut current: Option<PartialRemote> = None;
        let mut in_labels = false;

        for (line_no, raw_line) in yaml.lines().enumerate() {
            let line = raw_line.trim_end();

            // Top-level default
            if line.starts_with("default:") {
                let val = line["default:".len()..].trim().to_string();
                if !val.is_empty() {
                    registry.default_remote = Some(val);
                }
                continue;
            }

            // Top-level "remotes:" header
            if line.trim() == "remotes:" {
                continue;
            }

            // Start of a new remote entry
            if line.trim_start().starts_with("- name:") {
                // Flush previous
                if let Some(partial) = current.take() {
                    registry
                        .remotes
                        .push(partial.finish(line_no)?);
                }
                let val = line.split("- name:").nth(1).unwrap_or("").trim().to_string();
                current = Some(PartialRemote::new(val));
                in_labels = false;
                continue;
            }

            // Inside a remote entry
            if let Some(ref mut partial) = current {
                let trimmed = line.trim();

                // Label list items
                if in_labels {
                    if trimmed.starts_with("- ") {
                        partial
                            .labels
                            .push(trimmed["- ".len()..].trim().to_string());
                        continue;
                    } else {
                        in_labels = false;
                        // fall through to other field parsing
                    }
                }

                if trimmed.starts_with("host:") {
                    partial.host = Some(trimmed["host:".len()..].trim().to_string());
                } else if trimmed.starts_with("port:") {
                    partial.port = Some(
                        trimmed["port:".len()..]
                            .trim()
                            .parse::<u16>()
                            .map_err(|e| format!("line {}: bad port: {}", line_no + 1, e))?,
                    );
                } else if trimmed.starts_with("user:") {
                    partial.user = Some(trimmed["user:".len()..].trim().to_string());
                } else if trimmed.starts_with("ssh_key:") {
                    let v = trimmed["ssh_key:".len()..].trim().to_string();
                    if !v.is_empty() {
                        partial.ssh_key = Some(v);
                    }
                } else if trimmed.starts_with("workspace_dir:") {
                    partial.workspace_dir =
                        Some(trimmed["workspace_dir:".len()..].trim().to_string());
                } else if trimmed.starts_with("gpu_count:") {
                    partial.gpu_count = Some(
                        trimmed["gpu_count:".len()..]
                            .trim()
                            .parse::<u32>()
                            .map_err(|e| {
                                format!("line {}: bad gpu_count: {}", line_no + 1, e)
                            })?,
                    );
                } else if trimmed.starts_with("labels:") {
                    in_labels = true;
                }
            }
        }

        // Flush last entry
        if let Some(partial) = current.take() {
            let line_count = yaml.lines().count();
            registry.remotes.push(partial.finish(line_count)?);
        }

        // Validate default refers to a real remote
        if let Some(ref def) = registry.default_remote {
            if !registry.remotes.iter().any(|r| r.name == *def) {
                return Err(format!(
                    "default remote '{}' not found in remotes list",
                    def
                ));
            }
        }

        Ok(registry)
    }
}

impl Default for RigRegistry {
    fn default() -> Self {
        Self::new()
    }
}


// ---------------------------------------------------------------------------
// Partial helper for YAML parsing
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct PartialRemote {
    name: String,
    host: Option<String>,
    port: Option<u16>,
    user: Option<String>,
    ssh_key: Option<String>,
    workspace_dir: Option<String>,
    gpu_count: Option<u32>,
    labels: Vec<String>,
}

impl PartialRemote {
    fn new(name: String) -> Self {
        PartialRemote {
            name,
            host: None,
            port: None,
            user: None,
            ssh_key: None,
            workspace_dir: None,
            gpu_count: None,
            labels: Vec::new(),
        }
    }

    fn finish(self, line_hint: usize) -> Result<RemoteConfig, String> {
        Ok(RemoteConfig {
            name: self.name,
            host: self
                .host
                .ok_or_else(|| format!("line ~{}: missing 'host'", line_hint))?,
            port: self.port.unwrap_or(22),
            user: self
                .user
                .ok_or_else(|| format!("line ~{}: missing 'user'", line_hint))?,
            ssh_key: self.ssh_key,
            workspace_dir: self
                .workspace_dir
                .ok_or_else(|| format!("line ~{}: missing 'workspace_dir'", line_hint))?,
            gpu_count: self.gpu_count,
            labels: self.labels,
        })
    }
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(name: &str) -> RemoteConfig {
        RemoteConfig {
            name: name.to_string(),
            host: "10.0.0.1".to_string(),
            port: 22,
            user: "ubuntu".to_string(),
            ssh_key: None,
            workspace_dir: "/home/ubuntu/work".to_string(),
            gpu_count: None,
            labels: Vec::new(),
        }
    }

    fn make_labeled(name: &str, labels: &[&str]) -> RemoteConfig {
        let mut cfg = make_config(name);
        cfg.labels = labels.iter().map(|s| s.to_string()).collect();
        cfg
    }

    // -- RemoteConfig --

    #[test]
    fn user_at_host_format() {
        let cfg = make_config("r1");
        assert_eq!(cfg.user_at_host(), "ubuntu@10.0.0.1");
    }

    #[test]
    fn ssh_base_args_default_key() {
        let cfg = make_config("r1");
        let args = cfg.ssh_base_args();
        assert!(args.contains(&"-p".to_string()));
        assert!(args.contains(&"22".to_string()));
        assert!(args.contains(&"ubuntu@10.0.0.1".to_string()));
        // No -i flag when ssh_key is None.
        assert!(!args.contains(&"-i".to_string()));
    }

    #[test]
    fn ssh_base_args_with_key() {
        let mut cfg = make_config("r1");
        cfg.ssh_key = Some("/home/me/.ssh/gpu_key".to_string());
        let args = cfg.ssh_base_args();
        assert!(args.contains(&"-i".to_string()));
        assert!(args.contains(&"/home/me/.ssh/gpu_key".to_string()));
    }

    // -- RigRegistry CRUD --

    #[test]
    fn add_and_get() {
        let mut reg = RigRegistry::new();
        reg.add(make_config("r1")).unwrap();
        assert!(reg.get("r1").is_some());
        assert_eq!(reg.get("r1").unwrap().host, "10.0.0.1");
    }

    #[test]
    fn add_duplicate_fails() {
        let mut reg = RigRegistry::new();
        reg.add(make_config("r1")).unwrap();
        let err = reg.add(make_config("r1")).unwrap_err();
        assert!(err.contains("already exists"));
    }

    #[test]
    fn remove_existing() {
        let mut reg = RigRegistry::new();
        reg.add(make_config("r1")).unwrap();
        let removed = reg.remove("r1").unwrap();
        assert_eq!(removed.name, "r1");
        assert!(reg.get("r1").is_none());
    }

    #[test]
    fn remove_missing_fails() {
        let mut reg = RigRegistry::new();
        assert!(reg.remove("nope").is_err());
    }

    #[test]
    fn remove_clears_default() {
        let mut reg = RigRegistry::new();
        reg.add(make_config("r1")).unwrap();
        reg.set_default("r1").unwrap();
        assert_eq!(reg.default_name(), Some("r1"));
        reg.remove("r1").unwrap();
        assert!(reg.default_name().is_none());
    }

    #[test]
    fn get_mut_modifies() {
        let mut reg = RigRegistry::new();
        reg.add(make_config("r1")).unwrap();
        reg.get_mut("r1").unwrap().port = 2222;
        assert_eq!(reg.get("r1").unwrap().port, 2222);
    }

    #[test]
    fn list_returns_all() {
        let mut reg = RigRegistry::new();
        reg.add(make_config("r1")).unwrap();
        reg.add(make_config("r2")).unwrap();
        assert_eq!(reg.list().len(), 2);
    }

    // -- Default --

    #[test]
    fn set_default_valid() {
        let mut reg = RigRegistry::new();
        reg.add(make_config("r1")).unwrap();
        reg.set_default("r1").unwrap();
        assert_eq!(reg.default_name(), Some("r1"));
    }

    #[test]
    fn set_default_invalid() {
        let mut reg = RigRegistry::new();
        assert!(reg.set_default("nope").is_err());
    }

    // -- Resolve --

    #[test]
    fn resolve_explicit_name() {
        let mut reg = RigRegistry::new();
        reg.add(make_config("r1")).unwrap();
        let cfg = reg.resolve(Some("r1")).unwrap();
        assert_eq!(cfg.name, "r1");
    }

    #[test]
    fn resolve_uses_default() {
        let mut reg = RigRegistry::new();
        reg.add(make_config("r1")).unwrap();
        reg.set_default("r1").unwrap();
        let cfg = reg.resolve(None).unwrap();
        assert_eq!(cfg.name, "r1");
    }

    #[test]
    fn resolve_no_default_fails() {
        let reg = RigRegistry::new();
        assert!(reg.resolve(None).is_err());
    }

    #[test]
    fn resolve_missing_name_fails() {
        let mut reg = RigRegistry::new();
        reg.add(make_config("r1")).unwrap();
        assert!(reg.resolve(Some("r2")).is_err());
    }

    // -- Labels --

    #[test]
    fn by_label_filters() {
        let mut reg = RigRegistry::new();
        reg.add(make_labeled("r1", &["a100", "high-mem"])).unwrap();
        reg.add(make_labeled("r2", &["v100"])).unwrap();
        reg.add(make_labeled("r3", &["a100"])).unwrap();

        let a100s = reg.by_label("a100");
        assert_eq!(a100s.len(), 2);
        assert!(a100s.iter().any(|r| r.name == "r1"));
        assert!(a100s.iter().any(|r| r.name == "r3"));
    }

    #[test]
    fn by_label_no_match() {
        let mut reg = RigRegistry::new();
        reg.add(make_config("r1")).unwrap();
        assert!(reg.by_label("a100").is_empty());
    }

    // -- YAML round-trip --

    #[test]
    fn yaml_round_trip_minimal() {
        let mut reg = RigRegistry::new();
        reg.add(make_config("r1")).unwrap();
        let yaml = reg.serialize_yaml();
        let parsed = RigRegistry::parse_yaml(&yaml).unwrap();
        assert_eq!(parsed.list().len(), 1);
        assert_eq!(parsed.list()[0], reg.list()[0]);
    }

    #[test]
    fn yaml_round_trip_full() {
        let mut reg = RigRegistry::new();
        let mut cfg = RemoteConfig {
            name: "gpu-1".to_string(),
            host: "10.0.1.50".to_string(),
            port: 2222,
            user: "deploy".to_string(),
            ssh_key: Some("/keys/gpu.pem".to_string()),
            workspace_dir: "/data/workspace".to_string(),
            gpu_count: Some(4),
            labels: vec!["a100".to_string(), "high-mem".to_string()],
        };
        reg.add(cfg.clone()).unwrap();

        cfg.name = "cpu-1".to_string();
        cfg.host = "10.0.2.10".to_string();
        cfg.ssh_key = None;
        cfg.gpu_count = None;
        cfg.labels = Vec::new();
        reg.add(cfg).unwrap();

        reg.set_default("gpu-1").unwrap();

        let yaml = reg.serialize_yaml();
        let parsed = RigRegistry::parse_yaml(&yaml).unwrap();

        assert_eq!(parsed.default_name(), Some("gpu-1"));
        assert_eq!(parsed.list().len(), 2);
        assert_eq!(parsed.list()[0].name, "gpu-1");
        assert_eq!(parsed.list()[0].port, 2222);
        assert_eq!(parsed.list()[0].gpu_count, Some(4));
        assert_eq!(parsed.list()[0].labels.len(), 2);
        assert_eq!(parsed.list()[1].name, "cpu-1");
        assert!(parsed.list()[1].ssh_key.is_none());
        assert!(parsed.list()[1].gpu_count.is_none());
    }

    #[test]
    fn yaml_parse_default_port() {
        // Port not specified should default to 22.
        let yaml = "\
remotes:
  - name: r1
    host: 1.2.3.4
    user: alice
    workspace_dir: /tmp
";
        let parsed = RigRegistry::parse_yaml(yaml).unwrap();
        assert_eq!(parsed.list()[0].port, 22);
    }

    #[test]
    fn yaml_parse_missing_host_fails() {
        let yaml = "\
remotes:
  - name: r1
    user: alice
    workspace_dir: /tmp
";
        assert!(RigRegistry::parse_yaml(yaml).is_err());
    }

    #[test]
    fn yaml_parse_bad_default_fails() {
        let yaml = "\
default: nonexistent
remotes:
  - name: r1
    host: 1.2.3.4
    user: alice
    workspace_dir: /tmp
";
        let err = RigRegistry::parse_yaml(yaml).unwrap_err();
        assert!(err.contains("nonexistent"));
    }

    #[test]
    fn default_trait() {
        let reg = RigRegistry::default();
        assert!(reg.list().is_empty());
        assert!(reg.default_name().is_none());
    }
}
