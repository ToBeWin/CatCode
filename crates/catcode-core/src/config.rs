use serde::{Deserialize, Serialize};

// === AppConfig ===

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
/// [`AppConfig`]
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
/// [`DaemonConfig`]
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
/// [`DefaultsConfig`]
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
/// [`BudgetConfig`]
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
/// [`ContextConfig`]
pub struct ContextConfig {
    pub compression_enabled: bool,
    pub compression_threshold_ratio: f32,
    pub dedup_tool_outputs: bool,
    pub roll_history_enabled: bool,
    pub filter_relevance_enabled: bool,
    pub max_file_content_tokens: u64,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            compression_enabled: true,
            compression_threshold_ratio: 0.75,
            dedup_tool_outputs: true,
            roll_history_enabled: true,
            filter_relevance_enabled: true,
            max_file_content_tokens: 8000,
        }
    }
}

// === MiddlewareConfig ===

#[derive(Debug, Clone, Serialize, Deserialize)]
/// [`MiddlewareConfig`]
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
/// [`LoopDetectionConfig`]
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
/// [`RetryConfig`]
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
/// [`TimeoutConfig`]
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
/// [`MemoryMiddlewareConfig`]
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
/// [`ProviderConfig`]
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

    #[test]
    fn test_budget_config_defaults() {
        let config = BudgetConfig::default();
        assert_eq!(config.session_limit_tokens, 500_000);
        assert_eq!(config.per_request_limit_tokens, 50_000);
        assert!((config.warning_threshold - 0.80).abs() < f32::EPSILON);
        assert_eq!(config.on_limit_reached, "pause");
    }

    #[test]
    fn test_context_config_defaults() {
        let config = ContextConfig::default();
        assert!(config.compression_enabled);
        assert!((config.compression_threshold_ratio - 0.75).abs() < f32::EPSILON);
        assert!(config.dedup_tool_outputs);
        assert!(config.roll_history_enabled);
        assert!(config.filter_relevance_enabled);
        assert_eq!(config.max_file_content_tokens, 8000);
    }

    #[test]
    fn test_loop_detection_config_defaults() {
        let config = LoopDetectionConfig::default();
        assert_eq!(config.warn_threshold, 3);
        assert_eq!(config.hard_limit, 5);
        assert_eq!(config.window_size, 20);
    }

    #[test]
    fn test_retry_config_defaults() {
        let config = RetryConfig::default();
        assert_eq!(config.max_attempts, 3);
        assert_eq!(config.base_delay_ms, 1000);
        assert_eq!(config.max_delay_ms, 30000);
    }

    #[test]
    fn test_timeout_config_defaults() {
        let config = TimeoutConfig::default();
        assert_eq!(config.request_timeout_secs, 120);
    }

    #[test]
    fn test_memory_middleware_config_defaults() {
        let config = MemoryMiddlewareConfig::default();
        assert_eq!(config.debounce_seconds, 30);
        assert_eq!(config.max_facts, 100);
        assert!((config.fact_confidence_threshold - 0.7).abs() < f32::EPSILON);
        assert_eq!(config.max_injection_tokens, 2000);
    }

    #[test]
    fn test_defaults_config_all_fields() {
        let config = DefaultsConfig::default();
        assert_eq!(config.provider, "deepseek");
        assert_eq!(config.model, "deepseek-chat");
        assert!(config.sandbox);
    }

    #[test]
    fn test_provider_config_all_fields() {
        let config = ProviderConfig {
            api_key: Some("sk-test".to_string()),
            base_url: Some("https://api.test.com".to_string()),
            models: Some(vec!["model-a".to_string(), "model-b".to_string()]),
        };
        assert_eq!(config.api_key.as_deref(), Some("sk-test"));
        assert_eq!(config.base_url.as_deref(), Some("https://api.test.com"));
        assert_eq!(config.models.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn test_provider_config_none_fields() {
        let config = ProviderConfig {
            api_key: None,
            base_url: None,
            models: None,
        };
        assert!(config.api_key.is_none());
        assert!(config.base_url.is_none());
        assert!(config.models.is_none());
    }

    #[test]
    fn test_middleware_config_enabled_list() {
        let config = MiddlewareConfig::default();
        assert!(config.enabled.contains(&"loop_detection".to_string()));
        assert!(config.enabled.contains(&"retry".to_string()));
        assert!(config.enabled.contains(&"timeout".to_string()));
        assert_eq!(config.enabled.len(), 5);
    }

    #[test]
    fn test_app_config_serialization_roundtrip() {
        let config = AppConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.defaults.provider, "deepseek");
        assert_eq!(deserialized.daemon.port, 7070);
        assert_eq!(deserialized.budget.session_limit_tokens, 500_000);
        assert!(deserialized.context.compression_enabled);
    }

    #[test]
    fn test_daemon_config_serialization_roundtrip() {
        let config = DaemonConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: DaemonConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.host, "127.0.0.1");
        assert_eq!(deserialized.port, 7070);
        assert_eq!(deserialized.max_concurrent_sessions, 5);
        assert_eq!(deserialized.checkpoint_interval_turns, 10);
    }

    #[test]
    fn test_budget_config_custom_values() {
        let config = BudgetConfig {
            session_limit_tokens: 1_000_000,
            per_request_limit_tokens: 100_000,
            warning_threshold: 0.9,
            on_limit_reached: "stop".to_string(),
        };
        assert_eq!(config.session_limit_tokens, 1_000_000);
        assert_eq!(config.per_request_limit_tokens, 100_000);
        assert_eq!(config.on_limit_reached, "stop");
    }
}
