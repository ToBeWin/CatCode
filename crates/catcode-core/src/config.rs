use serde::{Deserialize, Serialize};

// === AppConfig ===

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppConfig {
    pub daemon: DaemonConfig,
    pub defaults: DefaultsConfig,
    pub budget: BudgetConfig,
    pub context: ContextConfig,
    pub middleware: MiddlewareConfig,
    pub providers: std::collections::HashMap<String, ProviderConfig>,
}

// === DaemonConfig ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    pub host: String,
    pub port: u16,
    pub auto_start: bool,
    pub max_concurrent_sessions: usize,
    pub checkpoint_interval_turns: usize,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 7070,
            auto_start: true,
            max_concurrent_sessions: 5,
            checkpoint_interval_turns: 10,
        }
    }
}

// === DefaultsConfig ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefaultsConfig {
    pub provider: String,
    pub model: String,
    pub sandbox: bool,
}

impl Default for DefaultsConfig {
    fn default() -> Self {
        Self {
            provider: "deepseek".to_string(),
            model: "deepseek-chat".to_string(),
            sandbox: true,
        }
    }
}

// === BudgetConfig ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetConfig {
    pub session_limit_tokens: u64,
    pub per_request_limit_tokens: u64,
    pub warning_threshold: f32,
    pub on_limit_reached: String,
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            session_limit_tokens: 500_000,
            per_request_limit_tokens: 50_000,
            warning_threshold: 0.80,
            on_limit_reached: "pause".to_string(),
        }
    }
}

// === ContextConfig ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextConfig {
    pub compression_enabled: bool,
    pub compression_threshold_ratio: f32,
    pub dedup_tool_outputs: bool,
    pub max_file_content_tokens: u64,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            compression_enabled: true,
            compression_threshold_ratio: 0.75,
            dedup_tool_outputs: true,
            max_file_content_tokens: 8000,
        }
    }
}

// === MiddlewareConfig ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiddlewareConfig {
    pub enabled: Vec<String>,
    pub loop_detection: LoopDetectionConfig,
    pub retry: RetryConfig,
    pub timeout: TimeoutConfig,
    pub memory: MemoryMiddlewareConfig,
}

impl Default for MiddlewareConfig {
    fn default() -> Self {
        Self {
            enabled: vec![
                "loop_detection".to_string(),
                "tool_error_handling".to_string(),
                "retry".to_string(),
                "timeout".to_string(),
                "token_usage".to_string(),
            ],
            loop_detection: LoopDetectionConfig::default(),
            retry: RetryConfig::default(),
            timeout: TimeoutConfig::default(),
            memory: MemoryMiddlewareConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopDetectionConfig {
    pub warn_threshold: u32,
    pub hard_limit: u32,
    pub window_size: usize,
}

impl Default for LoopDetectionConfig {
    fn default() -> Self {
        Self {
            warn_threshold: 3,
            hard_limit: 5,
            window_size: 20,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay_ms: 1000,
            max_delay_ms: 30000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutConfig {
    pub request_timeout_secs: u64,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            request_timeout_secs: 120,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryMiddlewareConfig {
    pub debounce_seconds: u64,
    pub max_facts: usize,
    pub fact_confidence_threshold: f32,
    pub max_injection_tokens: u64,
}

impl Default for MemoryMiddlewareConfig {
    fn default() -> Self {
        Self {
            debounce_seconds: 30,
            max_facts: 100,
            fact_confidence_threshold: 0.7,
            max_injection_tokens: 2000,
        }
    }
}

// === ProviderConfig ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub models: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert_eq!(config.defaults.provider, "deepseek");
        assert_eq!(config.defaults.model, "deepseek-chat");
        assert!(config.defaults.sandbox);
    }

    #[test]
    fn test_middleware_config_defaults() {
        let config = MiddlewareConfig::default();
        assert_eq!(config.loop_detection.warn_threshold, 3);
        assert_eq!(config.loop_detection.hard_limit, 5);
        assert_eq!(config.retry.max_attempts, 3);
        assert_eq!(config.timeout.request_timeout_secs, 120);
        assert_eq!(config.memory.debounce_seconds, 30);
    }

    #[test]
    fn test_daemon_config_defaults() {
        let config = DaemonConfig::default();
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 7070);
        assert!(config.auto_start);
    }
}
