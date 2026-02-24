use std::collections::HashMap;
use std::path::Path;

use crate::types::config::{BackoffStrategy, PoolConfigYaml, Settings};


/// Returns sensible defaults for all settings fields.
pub fn default_settings() -> Settings {
    Settings {
        version: "0.1.0".into(),
        health_check_interval: 5000,
        heartbeat_timeout: 30000,
        message_timeout: 10000,
        snapshot_interval: 60000,
        project_root: ".".into(),
        ready_prompt_pattern: r"\$\s*$".into(),
        max_retries: 3,
        backoff_strategy: BackoffStrategy::Exponential,
        ssh_retries: 5,
        ssh_backoff: vec![1000, 2000, 4000, 8000, 16000],
        alert_targets: vec!["pm".into()],
        escalation_timeout: 300000,
        pool_configs: HashMap::new(),
        pool_auto_expand: false,
    }
}


/// Load `Settings` from a YAML-like key:value file. No serde_yaml dependency;
/// this is a simple hand-rolled line parser that handles `key: value` pairs,
/// lists (lines starting with `  - `), and ignores blank lines / comments.
pub fn load(path: &Path) -> Result<Settings, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
    parse(&content)
}


/// Save `Settings` to a YAML-like key:value file.
pub fn save(path: &Path, settings: &Settings) -> Result<(), String> {
    let content = serialize(settings);
    std::fs::write(path, content)
        .map_err(|e| format!("cannot write {}: {}", path.display(), e))
}


/// Parse settings from a YAML-like string.
pub fn parse(content: &str) -> Result<Settings, String> {
    let mut s = default_settings();
    let mut current_key: Option<String> = None;
    let mut list_buf: Vec<String> = Vec::new();
    // Track pool config being built: pool.<role>.<field>
    let mut pool_building: HashMap<String, PartialPoolConfig> = HashMap::new();

    for raw_line in content.lines() {
        let line = raw_line.trim_end();

        // Skip blank lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // List item: "  - value"
        if line.starts_with("  - ") || line.starts_with("- ") {
            let val = line.trim_start_matches("  - ").trim_start_matches("- ").trim();
            list_buf.push(val.to_string());
            continue;
        }

        // If we were accumulating a list, flush it
        if let Some(ref key) = current_key {
            if !list_buf.is_empty() {
                apply_list(&mut s, key, &list_buf)?;
                list_buf.clear();
            }
        }

        // Key: value pair
        if let Some(colon_pos) = line.find(':') {
            let key = line[..colon_pos].trim().to_string();
            let val = line[colon_pos + 1..].trim().to_string();

            // Handle pool.<role>.<field> keys
            if key.starts_with("pool.") {
                let parts: Vec<&str> = key.splitn(3, '.').collect();
                if parts.len() == 3 {
                    let role = parts[1].to_string();
                    let field = parts[2];
                    let entry = pool_building.entry(role).or_insert_with(PartialPoolConfig::new);
                    match field {
                        "size" => entry.size = Some(parse_u32("pool size", &val)?),
                        "path" => entry.path = Some(unquote(&val)),
                        "max_size" => entry.max_size = Some(parse_u32("pool max_size", &val)?),
                        _ => {} // Ignore unknown pool fields
                    }
                }
                current_key = Some(key);
            } else if val.is_empty() {
                // This key introduces a list
                current_key = Some(key);
                list_buf.clear();
            } else {
                apply_scalar(&mut s, &key, &val)?;
                current_key = Some(key);
            }
        }
    }

    // Flush trailing list
    if let Some(ref key) = current_key {
        if !list_buf.is_empty() {
            apply_list(&mut s, key, &list_buf)?;
        }
    }

    // Convert built pool configs
    for (role, partial) in pool_building {
        if let (Some(size), Some(path)) = (partial.size, partial.path) {
            s.pool_configs.insert(role, PoolConfigYaml {
                size,
                path,
                max_size: partial.max_size,
            });
        }
    }

    Ok(s)
}


/// Helper struct for accumulating pool config during parsing.
struct PartialPoolConfig {
    size: Option<u32>,
    path: Option<String>,
    max_size: Option<u32>,
}

impl PartialPoolConfig {
    fn new() -> Self {
        PartialPoolConfig {
            size: None,
            path: None,
            max_size: None,
        }
    }
}


fn apply_scalar(s: &mut Settings, key: &str, val: &str) -> Result<(), String> {
    match key {
        "version" => {
            s.version = unquote(val);
        }
        "health_check_interval" => {
            s.health_check_interval = parse_u64(key, val)?;
        }
        "heartbeat_timeout" => {
            s.heartbeat_timeout = parse_u64(key, val)?;
        }
        "message_timeout" => {
            s.message_timeout = parse_u64(key, val)?;
        }
        "snapshot_interval" => {
            s.snapshot_interval = parse_u64(key, val)?;
        }
        "project_root" => {
            s.project_root = unquote(val);
        }
        "ready_prompt_pattern" => {
            s.ready_prompt_pattern = unquote(val);
        }
        "max_retries" => {
            s.max_retries = parse_u32(key, val)?;
        }
        "backoff_strategy" => {
            s.backoff_strategy = match val.to_lowercase().as_str() {
                "exponential" => BackoffStrategy::Exponential,
                "linear" => BackoffStrategy::Linear,
                "fixed" => BackoffStrategy::Fixed,
                _ => return Err(format!("unknown backoff_strategy: {}", val)),
            };
        }
        "ssh_retries" => {
            s.ssh_retries = parse_u32(key, val)?;
        }
        "escalation_timeout" => {
            s.escalation_timeout = parse_u64(key, val)?;
        }
        "pool_auto_expand" => {
            s.pool_auto_expand = match val.to_lowercase().as_str() {
                "true" | "yes" | "1" => true,
                "false" | "no" | "0" => false,
                _ => return Err(format!("invalid bool for pool_auto_expand: {}", val)),
            };
        }
        _ => {
            // Unknown keys are silently ignored for forward-compatibility
        }
    }
    Ok(())
}


fn apply_list(s: &mut Settings, key: &str, items: &[String]) -> Result<(), String> {
    match key {
        "ssh_backoff" => {
            s.ssh_backoff = items
                .iter()
                .map(|v| parse_u64("ssh_backoff item", v))
                .collect::<Result<Vec<u64>, String>>()?;
        }
        "alert_targets" => {
            s.alert_targets = items.iter().map(|v| unquote(v)).collect();
        }
        _ => {
            // Unknown list keys are silently ignored
        }
    }
    Ok(())
}


fn parse_u64(key: &str, val: &str) -> Result<u64, String> {
    val.parse::<u64>()
        .map_err(|_| format!("invalid u64 for {}: {}", key, val))
}


fn parse_u32(key: &str, val: &str) -> Result<u32, String> {
    val.parse::<u32>()
        .map_err(|_| format!("invalid u32 for {}: {}", key, val))
}


/// Remove surrounding quotes if present.
fn unquote(s: &str) -> String {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"'))
        || (s.starts_with('\'') && s.ends_with('\''))
    {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}


/// Serialize a Settings value to YAML-like text.
pub fn serialize(s: &Settings) -> String {
    let mut out = String::new();
    out.push_str(&format!("version: \"{}\"
", s.version));
    out.push_str(&format!("health_check_interval: {}\n", s.health_check_interval));
    out.push_str(&format!("heartbeat_timeout: {}\n", s.heartbeat_timeout));
    out.push_str(&format!("message_timeout: {}\n", s.message_timeout));
    out.push_str(&format!("snapshot_interval: {}\n", s.snapshot_interval));
    out.push_str(&format!("project_root: \"{}\"\n", s.project_root));
    out.push_str(&format!(
        "ready_prompt_pattern: \"{}\"\n",
        s.ready_prompt_pattern
    ));
    out.push_str(&format!("max_retries: {}\n", s.max_retries));
    let bs = match s.backoff_strategy {
        BackoffStrategy::Exponential => "exponential",
        BackoffStrategy::Linear => "linear",
        BackoffStrategy::Fixed => "fixed",
    };
    out.push_str(&format!("backoff_strategy: {}\n", bs));
    out.push_str(&format!("ssh_retries: {}\n", s.ssh_retries));
    out.push_str("ssh_backoff:\n");
    for v in &s.ssh_backoff {
        out.push_str(&format!("  - {}\n", v));
    }
    out.push_str("alert_targets:\n");
    for t in &s.alert_targets {
        out.push_str(&format!("  - {}\n", t));
    }
    out.push_str(&format!("escalation_timeout: {}\n", s.escalation_timeout));
    out.push_str(&format!("pool_auto_expand: {}\n", s.pool_auto_expand));
    // Serialize pool configs as pool.<role>.<field> keys
    let mut roles: Vec<&String> = s.pool_configs.keys().collect();
    roles.sort();
    for role in roles {
        let cfg = &s.pool_configs[role];
        out.push_str(&format!("pool.{}.size: {}\n", role, cfg.size));
        out.push_str(&format!("pool.{}.path: \"{}\"\n", role, cfg.path));
        if let Some(max) = cfg.max_size {
            out.push_str(&format!("pool.{}.max_size: {}\n", role, max));
        }
    }
    out
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_are_valid() {
        let s = default_settings();
        assert_eq!(s.health_check_interval, 5000);
        assert_eq!(s.max_retries, 3);
        assert_eq!(s.ssh_backoff.len(), 5);
    }

    #[test]
    fn parse_minimal_yaml() {
        let text = "health_check_interval: 8000\nmax_retries: 5\n";
        let s = parse(text).unwrap();
        assert_eq!(s.health_check_interval, 8000);
        assert_eq!(s.max_retries, 5);
        // Other fields should be defaults
        assert_eq!(s.heartbeat_timeout, 30000);
    }

    #[test]
    fn parse_full_yaml() {
        let text = "\
health_check_interval: 10000
heartbeat_timeout: 60000
message_timeout: 20000
snapshot_interval: 120000
project_root: \"/my/project\"
ready_prompt_pattern: \"\\\\$\\\\s*$\"
max_retries: 7
backoff_strategy: linear
ssh_retries: 10
ssh_backoff:
  - 500
  - 1000
  - 2000
alert_targets:
  - pm
  - pilot
escalation_timeout: 600000
";
        let s = parse(text).unwrap();
        assert_eq!(s.health_check_interval, 10000);
        assert_eq!(s.heartbeat_timeout, 60000);
        assert_eq!(s.project_root, "/my/project");
        assert_eq!(s.max_retries, 7);
        assert_eq!(s.backoff_strategy, BackoffStrategy::Linear);
        assert_eq!(s.ssh_retries, 10);
        assert_eq!(s.ssh_backoff, vec![500, 1000, 2000]);
        assert_eq!(s.alert_targets, vec!["pm", "pilot"]);
        assert_eq!(s.escalation_timeout, 600000);
    }

    #[test]
    fn parse_with_comments_and_blanks() {
        let text = "\
# This is a comment
health_check_interval: 3000

# Another comment
max_retries: 2
";
        let s = parse(text).unwrap();
        assert_eq!(s.health_check_interval, 3000);
        assert_eq!(s.max_retries, 2);
    }

    #[test]
    fn parse_quoted_values() {
        let text = "project_root: '/projects/cmx'\n";
        let s = parse(text).unwrap();
        assert_eq!(s.project_root, "/projects/cmx");
    }

    #[test]
    fn parse_unknown_keys_ignored() {
        let text = "health_check_interval: 1000\nfoo_bar: baz\n";
        let s = parse(text).unwrap();
        assert_eq!(s.health_check_interval, 1000);
    }

    #[test]
    fn parse_invalid_number_fails() {
        let text = "health_check_interval: not_a_number\n";
        let result = parse(text);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid u64"));
    }

    #[test]
    fn parse_invalid_backoff_strategy() {
        let text = "backoff_strategy: random\n";
        let result = parse(text);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown backoff_strategy"));
    }

    #[test]
    fn round_trip() {
        let original = default_settings();
        let text = serialize(&original);
        let parsed = parse(&text).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn round_trip_custom() {
        let mut s = default_settings();
        s.health_check_interval = 99;
        s.project_root = "/custom/path".into();
        s.backoff_strategy = BackoffStrategy::Fixed;
        s.ssh_backoff = vec![100, 200];
        s.alert_targets = vec!["alpha".into(), "beta".into()];
        let text = serialize(&s);
        let parsed = parse(&text).unwrap();
        assert_eq!(parsed, s);
    }

    #[test]
    fn parse_empty_returns_defaults() {
        let s = parse("").unwrap();
        assert_eq!(s, default_settings());
    }

    #[test]
    fn load_nonexistent_file() {
        let result = load(Path::new("/nonexistent/settings.yaml"));
        assert!(result.is_err());
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = std::env::temp_dir().join("cmx_test_settings");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("settings.yaml");

        let mut s = default_settings();
        s.max_retries = 42;
        save(&path, &s).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.max_retries, 42);
        assert_eq!(loaded, s);

        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- Pool config tests ---

    #[test]
    fn default_settings_have_empty_pool_configs() {
        let s = default_settings();
        assert!(s.pool_configs.is_empty());
        assert!(!s.pool_auto_expand);
    }

    #[test]
    fn parse_settings_with_pool_config() {
        let text = "\
pool_auto_expand: true
pool.worker.size: 3
pool.worker.path: \"/tmp/work\"
pool.worker.max_size: 6
pool.pilot.size: 1
pool.pilot.path: \"/tmp/pilot\"
";
        let s = parse(text).unwrap();
        assert!(s.pool_auto_expand);
        assert_eq!(s.pool_configs.len(), 2);
        let worker = &s.pool_configs["worker"];
        assert_eq!(worker.size, 3);
        assert_eq!(worker.path, "/tmp/work");
        assert_eq!(worker.max_size, Some(6));
        let pilot = &s.pool_configs["pilot"];
        assert_eq!(pilot.size, 1);
        assert_eq!(pilot.path, "/tmp/pilot");
        assert_eq!(pilot.max_size, None);
    }

    #[test]
    fn round_trip_pool_config() {
        let mut s = default_settings();
        s.pool_auto_expand = true;
        s.pool_configs.insert("worker".into(), PoolConfigYaml {
            size: 3,
            path: "/tmp/work".into(),
            max_size: Some(6),
        });
        s.pool_configs.insert("pilot".into(), PoolConfigYaml {
            size: 1,
            path: "/tmp/pilot".into(),
            max_size: None,
        });
        let text = serialize(&s);
        let parsed = parse(&text).unwrap();
        assert_eq!(parsed, s);
    }
}
