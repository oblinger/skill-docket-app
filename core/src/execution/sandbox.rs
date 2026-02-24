//! Execution environment setup — sandbox configuration, env resolution, env file parsing.
//!
//! Provides `SandboxBuilder` for constructing execution environments with a
//! fluent API, `EnvironmentResolver` for merging environment variables from
//! multiple sources, and `EnvFile` for parsing KEY=VALUE env files.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// SandboxConfig
// ---------------------------------------------------------------------------

/// Configuration for an execution's sandbox environment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    pub working_dir: String,
    pub env_vars: HashMap<String, String>,
    pub inherit_env: bool,
    pub clear_env: bool,
    pub env_file: Option<String>,
    pub path_additions: Vec<String>,
}

impl SandboxConfig {
    /// Create a minimal config with just a working directory.
    pub fn new(working_dir: &str) -> Self {
        SandboxConfig {
            working_dir: working_dir.to_string(),
            env_vars: HashMap::new(),
            inherit_env: false,
            clear_env: false,
            env_file: None,
            path_additions: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// SandboxBuilder
// ---------------------------------------------------------------------------

/// Fluent builder for constructing `SandboxConfig` instances.
pub struct SandboxBuilder {
    config: SandboxConfig,
}

impl SandboxBuilder {
    /// Start building a sandbox with the given working directory.
    pub fn new(working_dir: &str) -> Self {
        SandboxBuilder {
            config: SandboxConfig::new(working_dir),
        }
    }

    /// Set an environment variable.
    pub fn env(mut self, key: &str, value: &str) -> Self {
        self.config.env_vars.insert(key.to_string(), value.to_string());
        self
    }

    /// Enable inheriting the parent process environment.
    pub fn inherit(mut self) -> Self {
        self.config.inherit_env = true;
        self
    }

    /// Clear the environment before applying variables.
    pub fn clear_env(mut self) -> Self {
        self.config.clear_env = true;
        self
    }

    /// Set the path to an env file to load.
    pub fn env_file(mut self, path: &str) -> Self {
        self.config.env_file = Some(path.to_string());
        self
    }

    /// Add a directory to the PATH.
    pub fn add_to_path(mut self, dir: &str) -> Self {
        self.config.path_additions.push(dir.to_string());
        self
    }

    /// Consume the builder and return the config.
    pub fn build(self) -> SandboxConfig {
        self.config
    }
}

// ---------------------------------------------------------------------------
// EnvFile
// ---------------------------------------------------------------------------

/// Parser for env files in KEY=VALUE format.
///
/// Supports:
/// - Lines of `KEY=VALUE`
/// - Lines of `KEY="quoted value"` or `KEY='single quoted'`
/// - Comments starting with `#`
/// - Empty lines (skipped)
/// - Inline comments after the value are NOT stripped (to match `.env` convention)
pub struct EnvFile;

impl EnvFile {
    /// Parse env file content into a map of key-value pairs.
    pub fn parse(content: &str) -> HashMap<String, String> {
        let mut vars = HashMap::new();

        for line in content.lines() {
            let trimmed = line.trim();

            // Skip empty lines and comments.
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // Find the first '=' to split key and value.
            if let Some(eq_pos) = trimmed.find('=') {
                let key = trimmed[..eq_pos].trim().to_string();
                let raw_value = trimmed[eq_pos + 1..].trim();

                if key.is_empty() {
                    continue;
                }

                let value = Self::unquote(raw_value);
                vars.insert(key, value);
            }
        }

        vars
    }

    /// Remove surrounding quotes from a value (double or single).
    fn unquote(s: &str) -> String {
        if s.len() >= 2 {
            if (s.starts_with('"') && s.ends_with('"'))
                || (s.starts_with('\'') && s.ends_with('\''))
            {
                return s[1..s.len() - 1].to_string();
            }
        }
        s.to_string()
    }
}

// ---------------------------------------------------------------------------
// EnvironmentResolver
// ---------------------------------------------------------------------------

/// Merges environment variables from multiple sources according to a
/// `SandboxConfig`.
pub struct EnvironmentResolver;

impl EnvironmentResolver {
    /// Resolve the final environment map from a sandbox config.
    ///
    /// Resolution order:
    /// 1. If `clear_env`, start empty; otherwise if `inherit_env`, start with
    ///    a base env (passed as parameter to support testing).
    /// 2. Apply `env_file` variables (if provided as parsed content).
    /// 3. Apply `env_vars` from the config (highest priority).
    /// 4. Apply `path_additions` to the PATH variable.
    pub fn resolve(
        config: &SandboxConfig,
        base_env: &HashMap<String, String>,
        env_file_content: Option<&str>,
    ) -> HashMap<String, String> {
        let mut env: HashMap<String, String> = if config.clear_env {
            HashMap::new()
        } else if config.inherit_env {
            base_env.clone()
        } else {
            HashMap::new()
        };

        // Apply env file if provided.
        if let Some(content) = env_file_content {
            let file_vars = EnvFile::parse(content);
            for (k, v) in file_vars {
                env.insert(k, v);
            }
        }

        // Apply explicit env vars (highest priority).
        for (k, v) in &config.env_vars {
            env.insert(k.clone(), v.clone());
        }

        // Apply PATH additions.
        if !config.path_additions.is_empty() {
            let existing_path = env.get("PATH").cloned().unwrap_or_default();
            let additions = config.path_additions.join(":");
            let new_path = if existing_path.is_empty() {
                additions
            } else {
                format!("{}:{}", additions, existing_path)
            };
            env.insert("PATH".to_string(), new_path);
        }

        env
    }

    /// Build the command environment tuple: (command_args, env_map).
    ///
    /// Returns the original command and the resolved environment.
    pub fn build_command_env(
        config: &SandboxConfig,
        base_cmd: &[String],
        base_env: &HashMap<String, String>,
        env_file_content: Option<&str>,
    ) -> (Vec<String>, HashMap<String, String>) {
        let env = Self::resolve(config, base_env, env_file_content);
        (base_cmd.to_vec(), env)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- SandboxBuilder tests --

    #[test]
    fn builder_basic() {
        let config = SandboxBuilder::new("/work").build();
        assert_eq!(config.working_dir, "/work");
        assert!(!config.inherit_env);
        assert!(!config.clear_env);
        assert!(config.env_vars.is_empty());
        assert!(config.path_additions.is_empty());
        assert!(config.env_file.is_none());
    }

    #[test]
    fn builder_with_env() {
        let config = SandboxBuilder::new("/work")
            .env("FOO", "bar")
            .env("BAZ", "qux")
            .build();

        assert_eq!(config.env_vars.get("FOO").unwrap(), "bar");
        assert_eq!(config.env_vars.get("BAZ").unwrap(), "qux");
    }

    #[test]
    fn builder_inherit() {
        let config = SandboxBuilder::new("/work").inherit().build();
        assert!(config.inherit_env);
    }

    #[test]
    fn builder_clear_env() {
        let config = SandboxBuilder::new("/work").clear_env().build();
        assert!(config.clear_env);
    }

    #[test]
    fn builder_env_file() {
        let config = SandboxBuilder::new("/work")
            .env_file("/path/to/.env")
            .build();
        assert_eq!(config.env_file, Some("/path/to/.env".into()));
    }

    #[test]
    fn builder_path_additions() {
        let config = SandboxBuilder::new("/work")
            .add_to_path("/usr/local/bin")
            .add_to_path("/opt/bin")
            .build();

        assert_eq!(config.path_additions.len(), 2);
        assert_eq!(config.path_additions[0], "/usr/local/bin");
        assert_eq!(config.path_additions[1], "/opt/bin");
    }

    #[test]
    fn builder_chaining() {
        let config = SandboxBuilder::new("/work")
            .env("A", "1")
            .inherit()
            .add_to_path("/bin")
            .env_file(".env")
            .env("B", "2")
            .build();

        assert_eq!(config.env_vars.len(), 2);
        assert!(config.inherit_env);
        assert_eq!(config.path_additions.len(), 1);
        assert!(config.env_file.is_some());
    }

    // -- EnvFile tests --

    #[test]
    fn env_file_basic() {
        let content = "FOO=bar\nBAZ=qux";
        let vars = EnvFile::parse(content);
        assert_eq!(vars.get("FOO").unwrap(), "bar");
        assert_eq!(vars.get("BAZ").unwrap(), "qux");
    }

    #[test]
    fn env_file_comments() {
        let content = "# This is a comment\nFOO=bar\n# Another comment\nBAZ=qux";
        let vars = EnvFile::parse(content);
        assert_eq!(vars.len(), 2);
    }

    #[test]
    fn env_file_empty_lines() {
        let content = "FOO=bar\n\n\nBAZ=qux\n\n";
        let vars = EnvFile::parse(content);
        assert_eq!(vars.len(), 2);
    }

    #[test]
    fn env_file_double_quotes() {
        let content = "FOO=\"hello world\"";
        let vars = EnvFile::parse(content);
        assert_eq!(vars.get("FOO").unwrap(), "hello world");
    }

    #[test]
    fn env_file_single_quotes() {
        let content = "FOO='hello world'";
        let vars = EnvFile::parse(content);
        assert_eq!(vars.get("FOO").unwrap(), "hello world");
    }

    #[test]
    fn env_file_no_value() {
        let content = "FOO=";
        let vars = EnvFile::parse(content);
        assert_eq!(vars.get("FOO").unwrap(), "");
    }

    #[test]
    fn env_file_equals_in_value() {
        let content = "FOO=bar=baz";
        let vars = EnvFile::parse(content);
        assert_eq!(vars.get("FOO").unwrap(), "bar=baz");
    }

    #[test]
    fn env_file_spaces_around_key() {
        let content = "  FOO  = bar";
        let vars = EnvFile::parse(content);
        assert_eq!(vars.get("FOO").unwrap(), "bar");
    }

    #[test]
    fn env_file_empty_content() {
        let vars = EnvFile::parse("");
        assert!(vars.is_empty());
    }

    #[test]
    fn env_file_only_comments() {
        let content = "# comment\n# another";
        let vars = EnvFile::parse(content);
        assert!(vars.is_empty());
    }

    #[test]
    fn env_file_no_equals_skipped() {
        let content = "INVALID_LINE\nFOO=bar";
        let vars = EnvFile::parse(content);
        assert_eq!(vars.len(), 1);
        assert_eq!(vars.get("FOO").unwrap(), "bar");
    }

    #[test]
    fn env_file_empty_key_skipped() {
        let content = "=value\nFOO=bar";
        let vars = EnvFile::parse(content);
        assert_eq!(vars.len(), 1);
    }

    // -- EnvironmentResolver tests --

    #[test]
    fn resolve_empty_config() {
        let config = SandboxConfig::new("/work");
        let base = HashMap::new();
        let env = EnvironmentResolver::resolve(&config, &base, None);
        assert!(env.is_empty());
    }

    #[test]
    fn resolve_inherit_env() {
        let config = SandboxBuilder::new("/work").inherit().build();
        let mut base = HashMap::new();
        base.insert("HOME".into(), "/home/user".into());

        let env = EnvironmentResolver::resolve(&config, &base, None);
        assert_eq!(env.get("HOME").unwrap(), "/home/user");
    }

    #[test]
    fn resolve_clear_env() {
        let config = SandboxBuilder::new("/work").inherit().clear_env().build();
        let mut base = HashMap::new();
        base.insert("HOME".into(), "/home/user".into());

        let env = EnvironmentResolver::resolve(&config, &base, None);
        assert!(env.is_empty());
    }

    #[test]
    fn resolve_explicit_vars_override() {
        let config = SandboxBuilder::new("/work")
            .inherit()
            .env("HOME", "/custom")
            .build();

        let mut base = HashMap::new();
        base.insert("HOME".into(), "/home/user".into());

        let env = EnvironmentResolver::resolve(&config, &base, None);
        assert_eq!(env.get("HOME").unwrap(), "/custom");
    }

    #[test]
    fn resolve_env_file_content() {
        let config = SandboxBuilder::new("/work")
            .env_file(".env")
            .build();

        let env_content = "DB_HOST=localhost\nDB_PORT=5432";
        let base = HashMap::new();
        let env = EnvironmentResolver::resolve(&config, &base, Some(env_content));

        assert_eq!(env.get("DB_HOST").unwrap(), "localhost");
        assert_eq!(env.get("DB_PORT").unwrap(), "5432");
    }

    #[test]
    fn resolve_explicit_overrides_env_file() {
        let config = SandboxBuilder::new("/work")
            .env("DB_HOST", "production.db")
            .build();

        let env_content = "DB_HOST=localhost";
        let base = HashMap::new();
        let env = EnvironmentResolver::resolve(&config, &base, Some(env_content));

        assert_eq!(env.get("DB_HOST").unwrap(), "production.db");
    }

    #[test]
    fn resolve_path_additions() {
        let config = SandboxBuilder::new("/work")
            .add_to_path("/usr/local/bin")
            .add_to_path("/opt/bin")
            .build();

        let base = HashMap::new();
        let env = EnvironmentResolver::resolve(&config, &base, None);

        let path = env.get("PATH").unwrap();
        assert!(path.starts_with("/usr/local/bin:/opt/bin"));
    }

    #[test]
    fn resolve_path_prepended_to_existing() {
        let config = SandboxBuilder::new("/work")
            .inherit()
            .add_to_path("/custom/bin")
            .build();

        let mut base = HashMap::new();
        base.insert("PATH".into(), "/usr/bin:/bin".into());

        let env = EnvironmentResolver::resolve(&config, &base, None);
        let path = env.get("PATH").unwrap();
        assert_eq!(path, "/custom/bin:/usr/bin:/bin");
    }

    #[test]
    fn resolve_full_stack() {
        let config = SandboxBuilder::new("/work")
            .inherit()
            .env("OVERRIDE", "yes")
            .add_to_path("/new/bin")
            .build();

        let mut base = HashMap::new();
        base.insert("PATH".into(), "/usr/bin".into());
        base.insert("HOME".into(), "/home/user".into());
        base.insert("OVERRIDE".into(), "no".into());

        let env_content = "EXTRA=from_file";
        let env = EnvironmentResolver::resolve(&config, &base, Some(env_content));

        assert_eq!(env.get("HOME").unwrap(), "/home/user");
        assert_eq!(env.get("OVERRIDE").unwrap(), "yes");
        assert_eq!(env.get("EXTRA").unwrap(), "from_file");
        assert!(env.get("PATH").unwrap().starts_with("/new/bin:"));
    }

    #[test]
    fn build_command_env() {
        let config = SandboxBuilder::new("/work")
            .env("RUST_LOG", "debug")
            .build();

        let cmd = vec!["cargo".into(), "test".into()];
        let base = HashMap::new();
        let (result_cmd, result_env) =
            EnvironmentResolver::build_command_env(&config, &cmd, &base, None);

        assert_eq!(result_cmd, vec!["cargo", "test"]);
        assert_eq!(result_env.get("RUST_LOG").unwrap(), "debug");
    }

    #[test]
    fn sandbox_config_serde() {
        let config = SandboxBuilder::new("/work")
            .env("A", "1")
            .inherit()
            .add_to_path("/bin")
            .env_file(".env")
            .build();

        let json = serde_json::to_string(&config).unwrap();
        let back: SandboxConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.working_dir, "/work");
        assert_eq!(back.env_vars.get("A").unwrap(), "1");
        assert!(back.inherit_env);
        assert_eq!(back.path_additions.len(), 1);
    }

    #[test]
    fn env_file_mixed_content() {
        let content = r#"
# Database config
DB_HOST=localhost
DB_PORT=5432
DB_NAME="my_database"

# App config
APP_SECRET='super-secret-key'
DEBUG=true
"#;
        let vars = EnvFile::parse(content);
        assert_eq!(vars.len(), 5);
        assert_eq!(vars.get("DB_NAME").unwrap(), "my_database");
        assert_eq!(vars.get("APP_SECRET").unwrap(), "super-secret-key");
        assert_eq!(vars.get("DEBUG").unwrap(), "true");
    }
}
