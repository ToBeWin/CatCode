// === Provider Errors ===

#[derive(Debug, thiserror::Error)]
/// [`ProviderError`]
pub enum ProviderError {
    #[error("API request failed: {0}")]
/// [`RequestFailed`].
    RequestFailed(String),

    #[error("Rate limited, retry after {retry_after_ms}ms")]
/// [`RateLimited`].
    RateLimited { retry_after_ms: u64 },

    #[error("Authentication failed: {0}")]
/// [`AuthFailed`].
    AuthFailed(String),

    #[error("Model not found: {0}")]
/// [`ModelNotFound`].
    ModelNotFound(String),

    #[error("Stream error: {0}")]
/// [`StreamError`].
    StreamError(String),

    #[error("Timeout after {0}ms")]
/// [`Timeout`].
    Timeout(u64),

    #[error("Provider unavailable: {0}")]
/// [`Unavailable`].
    Unavailable(String),
}

// === Tool Errors ===

#[derive(Debug, thiserror::Error)]
/// [`ToolError`]
pub enum ToolError {
    #[error("Tool not found: {0}")]
/// [`NotFound`].
    NotFound(String),

    #[error("Invalid arguments: {0}")]
/// [`InvalidArgs`].
    InvalidArgs(String),

    #[error("Execution failed: {0}")]
/// [`ExecutionFailed`].
    ExecutionFailed(String),

    #[error("Permission denied: {0}")]
/// [`PermissionDenied`].
    PermissionDenied(String),

    #[error("Timeout after {0}ms")]
/// [`Timeout`].
    Timeout(u64),
}

// === Middleware Errors ===

#[derive(Debug, thiserror::Error)]
/// [`MiddlewareError`]
pub enum MiddlewareError {
    #[error("Middleware '{name}' failed: {message}")]
/// [`ExecutionFailed`].
    ExecutionFailed { name: String, message: String },

    #[error("Loop detected: {0}")]
/// [`LoopDetected`].
    LoopDetected(String),

    #[error("Guardrail denied: {0}")]
/// [`GuardrailDenied`].
    GuardrailDenied(String),
}

// === Context Errors ===

#[derive(Debug, thiserror::Error)]
/// [`ContextError`]
pub enum ContextError {
    #[error("Token budget exhausted: used {used}/{limit}")]
/// [`BudgetExhausted`].
    BudgetExhausted { used: u64, limit: u64 },

    #[error("Compression failed: {0}")]
/// [`CompressionFailed`].
    CompressionFailed(String),

    #[error("Memory error: {0}")]
/// [`MemoryError`].
    MemoryError(String),
}

// === Config Errors ===

#[derive(Debug, thiserror::Error)]
/// [`ConfigError`]
pub enum ConfigError {
    #[error("Config file not found: {0}")]
/// [`NotFound`].
    NotFound(String),

    #[error("Invalid config: {0}")]
/// [`Invalid`].
    Invalid(String),

    #[error("Missing required field: {0}")]
/// [`MissingField`].
    MissingField(String),
}

// === Unified Error ===

#[derive(Debug, thiserror::Error)]
/// [`CatCodeError`]
pub enum CatCodeError {
    #[error("Provider error: {0}")]
/// [`Provider`].
    Provider(#[from] ProviderError),

    #[error("Tool error: {0}")]
/// [`Tool`].
    Tool(#[from] ToolError),

    #[error("Middleware error: {0}")]
/// [`Middleware`].
    Middleware(#[from] MiddlewareError),

    #[error("Context error: {0}")]
/// [`Context`].
    Context(#[from] ContextError),

    #[error("Config error: {0}")]
/// [`Config`].
    Config(#[from] ConfigError),

    #[error("{0}")]
/// [`Other`].
    Other(String),
}

/// [`Result`]
pub type Result<T> = std::result::Result<T, CatCodeError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_error_conversion() {
        let provider_err = ProviderError::RateLimited {
            retry_after_ms: 1000,
        };
        let catcode_err: CatCodeError = provider_err.into();
        assert!(catcode_err.to_string().contains("Rate limited"));
    }

    #[test]
    fn test_tool_error_conversion() {
        let tool_err = ToolError::NotFound("bash".to_string());
        let catcode_err: CatCodeError = tool_err.into();
        assert!(catcode_err.to_string().contains("Tool not found"));
    }

    #[test]
    fn test_middleware_error_conversion() {
        let mw_err = MiddlewareError::LoopDetected("repeated read_file".to_string());
        let catcode_err: CatCodeError = mw_err.into();
        assert!(catcode_err.to_string().contains("Loop detected"));
    }

    #[test]
    fn test_context_error_conversion() {
        let ctx_err = ContextError::BudgetExhausted {
            used: 50000,
            limit: 50000,
        };
        let catcode_err: CatCodeError = ctx_err.into();
        assert!(catcode_err.to_string().contains("Token budget exhausted"));
    }

    #[test]
    fn test_other_error() {
        let err = CatCodeError::Other("something went wrong".to_string());
        assert_eq!(err.to_string(), "something went wrong");
    }

    #[test]
    fn test_provider_error_request_failed() {
        let err = ProviderError::RequestFailed("connection refused".to_string());
        assert_eq!(err.to_string(), "API request failed: connection refused");
    }

    #[test]
    fn test_provider_error_rate_limited() {
        let err = ProviderError::RateLimited { retry_after_ms: 5000 };
        assert!(err.to_string().contains("5000ms"));
    }

    #[test]
    fn test_provider_error_auth_failed() {
        let err = ProviderError::AuthFailed("invalid key".to_string());
        assert!(err.to_string().contains("Authentication failed"));
    }

    #[test]
    fn test_provider_error_model_not_found() {
        let err = ProviderError::ModelNotFound("gpt-5".to_string());
        assert_eq!(err.to_string(), "Model not found: gpt-5");
    }

    #[test]
    fn test_provider_error_stream_error() {
        let err = ProviderError::StreamError("unexpected EOF".to_string());
        assert_eq!(err.to_string(), "Stream error: unexpected EOF");
    }

    #[test]
    fn test_provider_error_timeout() {
        let err = ProviderError::Timeout(30000);
        assert_eq!(err.to_string(), "Timeout after 30000ms");
    }

    #[test]
    fn test_provider_error_unavailable() {
        let err = ProviderError::Unavailable("server down".to_string());
        assert_eq!(err.to_string(), "Provider unavailable: server down");
    }

    #[test]
    fn test_tool_error_not_found() {
        let err = ToolError::NotFound("write_file".to_string());
        assert_eq!(err.to_string(), "Tool not found: write_file");
    }

    #[test]
    fn test_tool_error_invalid_args() {
        let err = ToolError::InvalidArgs("missing path".to_string());
        assert_eq!(err.to_string(), "Invalid arguments: missing path");
    }

    #[test]
    fn test_tool_error_execution_failed() {
        let err = ToolError::ExecutionFailed("permission denied".to_string());
        assert_eq!(err.to_string(), "Execution failed: permission denied");
    }

    #[test]
    fn test_tool_error_permission_denied() {
        let err = ToolError::PermissionDenied("not allowed".to_string());
        assert_eq!(err.to_string(), "Permission denied: not allowed");
    }

    #[test]
    fn test_tool_error_timeout() {
        let err = ToolError::Timeout(10000);
        assert_eq!(err.to_string(), "Timeout after 10000ms");
    }

    #[test]
    fn test_middleware_error_execution_failed() {
        let err = MiddlewareError::ExecutionFailed {
            name: "retry".to_string(),
            message: "max attempts reached".to_string(),
        };
        assert_eq!(err.to_string(), "Middleware 'retry' failed: max attempts reached");
    }

    #[test]
    fn test_middleware_error_guardrail_denied() {
        let err = MiddlewareError::GuardrailDenied("harmful content".to_string());
        assert_eq!(err.to_string(), "Guardrail denied: harmful content");
    }

    #[test]
    fn test_context_error_compression_failed() {
        let err = ContextError::CompressionFailed("invalid format".to_string());
        assert_eq!(err.to_string(), "Compression failed: invalid format");
    }

    #[test]
    fn test_context_error_memory_error() {
        let err = ContextError::MemoryError("capacity exceeded".to_string());
        assert_eq!(err.to_string(), "Memory error: capacity exceeded");
    }

    #[test]
    fn test_config_error_not_found() {
        let err = ConfigError::NotFound("config.toml".to_string());
        assert_eq!(err.to_string(), "Config file not found: config.toml");
    }

    #[test]
    fn test_config_error_invalid() {
        let err = ConfigError::Invalid("bad format".to_string());
        assert_eq!(err.to_string(), "Invalid config: bad format");
    }

    #[test]
    fn test_config_error_missing_field() {
        let err = ConfigError::MissingField("api_key".to_string());
        assert_eq!(err.to_string(), "Missing required field: api_key");
    }

    #[test]
    fn test_config_error_conversion() {
        let err = ConfigError::NotFound("config.toml".to_string());
        let catcode_err: CatCodeError = err.into();
        assert!(matches!(catcode_err, CatCodeError::Config(_)));
    }

    #[test]
    fn test_tool_error_into_catcode_error() {
        let err = ToolError::InvalidArgs("x".to_string());
        let catcode_err: CatCodeError = err.into();
        assert!(matches!(catcode_err, CatCodeError::Tool(_)));
    }

    #[test]
    fn test_middleware_error_into_catcode_error() {
        let err = MiddlewareError::GuardrailDenied("x".to_string());
        let catcode_err: CatCodeError = err.into();
        assert!(matches!(catcode_err, CatCodeError::Middleware(_)));
    }

    #[test]
    fn test_context_error_into_catcode_error() {
        let err = ContextError::MemoryError("x".to_string());
        let catcode_err: CatCodeError = err.into();
        assert!(matches!(catcode_err, CatCodeError::Context(_)));
    }

    #[test]
    fn test_provider_error_debug() {
        let err = ProviderError::Timeout(5000);
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("Timeout"));
    }
}
