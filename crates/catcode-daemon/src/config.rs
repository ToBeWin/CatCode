use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Top-level CatCode configuration, loaded from `config.toml`.
///
/// All fields have sensible defaults. Use `Config::load()` to load from
/// a file, or `Config::default_config()` for a hardcoded default.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub daemon: DaemonConfig,
    pub defaults: DefaultsConfig,
    pub budget: BudgetConfig,
    pub context: ContextConfigToml,
    pub observability: ObservabilityConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Daemon server configuration.
pub struct DaemonConfig {
    pub host: String,
    pub port: u16,
    pub auto_start: bool,
    pub max_concurrent_sessions: usize,
    pub checkpoint_interval_turns: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Default provider and model settings.
pub struct DefaultsConfig {
    pub provider: String,
    pub model: String,
    pub sandbox: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Token budget limits and thresholds.
pub struct BudgetConfig {
    pub session_limit_tokens: u64,
    pub per_request_limit_tokens: u64,
    pub warning_threshold: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Context management configuration (TOML schema).
pub struct ContextConfigToml {
    pub compression_enabled: bool,
    pub dedup_tool_outputs: bool,
    pub max_file_content_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Logging and observability settings.
pub struct ObservabilityConfig {
    pub log_level: String,
    pub log_format: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            daemon: DaemonConfig {
                host: "127.0.0.1".to_string(),
                port: 7070,
                auto_start: true,
                max_concurrent_sessions: 5,
                checkpoint_interval_turns: 10,
            },
            defaults: DefaultsConfig {
                provider: "deepseek".to_string(),
                model: "deepseek-chat".to_string(),
                sandbox: false,
            },
            budget: BudgetConfig {
                session_limit_tokens: 500_000,
                per_request_limit_tokens: 50_000,
                warning_threshold: 0.80,
            },
            context: ContextConfigToml {
                compression_enabled: true,
                dedup_tool_outputs: true,
                max_file_content_tokens: 8000,
            },
            observability: ObservabilityConfig {
                log_level: "info".to_string(),
                log_format: "text".to_string(),
            },
        }
    }
}

impl Config {
    /// Load configuration from a TOML file.
    ///
    /// Falls back to `Config::default_config()` if the file doesn't exist.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            tracing::info!("Config file not found at {:?}, using defaults", path);
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    /// Load from a file if it exists, otherwise use defaults.
    pub fn load_or_default(path: &Path) -> Self {
        Self::load(path).unwrap_or_default()
    }

    /// Get the data directory (`.catcode/`) relative to a project root.
    pub fn data_dir(project_dir: &Path) -> PathBuf {
        project_dir.join(".catcode")
    }

    /// Get the default config file path for a project.
    pub fn config_path(project_dir: &Path) -> PathBuf {
        Self::data_dir(project_dir).join("config.toml")
    }

    /// Get the checkpoints directory for a project.
    pub fn checkpoints_dir(project_dir: &Path) -> PathBuf {
        Self::data_dir(project_dir).join("checkpoints")
    }

    /// Get the database path for a project.
    pub fn db_path(project_dir: &Path) -> PathBuf {
        Self::data_dir(project_dir).join("catcode.db")
    }

    /// Serialize the config to TOML string.
    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.daemon.host, "127.0.0.1");
        assert_eq!(config.daemon.port, 7070);
        assert!(config.daemon.auto_start);
        assert_eq!(config.daemon.max_concurrent_sessions, 5);
        assert_eq!(config.defaults.provider, "deepseek");
        assert_eq!(config.defaults.model, "deepseek-chat");
    }

    #[test]
    fn test_load_nonexistent_returns_default() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nonexistent.toml");
        let config = Config::load(&path).unwrap();
        assert_eq!(config.daemon.port, 7070);
    }

    #[test]
    fn test_load_from_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        fs::write(
            &path,
            r#"
[daemon]
host = "0.0.0.0"
port = 9090
auto_start = false
max_concurrent_sessions = 10
checkpoint_interval_turns = 5

[defaults]
provider = "anthropic"
model = "claude-sonnet-4-5"
sandbox = true

[budget]
session_limit_tokens = 1000000
per_request_limit_tokens = 100000
warning_threshold = 0.90

[context]
compression_enabled = true
dedup_tool_outputs = false
max_file_content_tokens = 4000

[observability]
log_level = "debug"
log_format = "json"
"#,
        )
        .unwrap();

        let config = Config::load(&path).unwrap();
        assert_eq!(config.daemon.host, "0.0.0.0");
        assert_eq!(config.daemon.port, 9090);
        assert!(!config.daemon.auto_start);
        assert_eq!(config.defaults.provider, "anthropic");
        assert_eq!(config.defaults.model, "claude-sonnet-4-5");
        assert!(config.defaults.sandbox);
        assert_eq!(config.budget.session_limit_tokens, 1_000_000);
    }

    #[test]
    fn test_load_or_default() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("missing.toml");
        let config = Config::load_or_default(&path);
        assert_eq!(config.daemon.port, 7070);
    }

    #[test]
    fn test_to_toml_roundtrip() {
        let config = Config::default();
        let toml_str = config.to_toml();
        assert!(toml_str.contains("[daemon]"));
        assert!(toml_str.contains("port = 7070"));

        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.daemon.port, 7070);
    }

    #[test]
    fn test_data_dir_paths() {
        let project = PathBuf::from("/home/user/myproject");
        assert_eq!(
            Config::data_dir(&project),
            PathBuf::from("/home/user/myproject/.catcode")
        );
        assert_eq!(
            Config::config_path(&project),
            PathBuf::from("/home/user/myproject/.catcode/config.toml")
        );
        assert_eq!(
            Config::checkpoints_dir(&project),
            PathBuf::from("/home/user/myproject/.catcode/checkpoints")
        );
    }
}
