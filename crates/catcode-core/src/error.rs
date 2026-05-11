// === Provider Errors ===

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("API request failed: {0}")]
    RequestFailed(String),

    #[error("Rate limited, retry after {retry_after_ms}ms")]
    RateLimited { retry_after_ms: u64 },

    #[error("Authentication failed: {0}")]
    AuthFailed(String),

    #[error("Model not found: {0}")]
    ModelNotFound(String),

    #[error("Stream error: {0}")]
    StreamError(String),

    #[error("Timeout after {0}ms")]
    Timeout(u64),

    #[error("Provider unavailable: {0}")]
    Unavailable(String),
}

// === Tool Errors ===

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("Tool not found: {0}")]
    NotFound(String),

    #[error("Invalid arguments: {0}")]
    InvalidArgs(String),

    #[error("Execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Timeout after {0}ms")]
    Timeout(u64),
}

// === Middleware Errors ===

#[derive(Debug, thiserror::Error)]
pub enum MiddlewareError {
    #[error("Middleware '{name}' failed: {message}")]
    ExecutionFailed { name: String, message: String },

    #[error("Loop detected: {0}")]
    LoopDetected(String),

    #[error("Guardrail denied: {0}")]
    GuardrailDenied(String),
}

// === Context Errors ===

#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    #[error("Token budget exhausted: used {used}/{limit}")]
    BudgetExhausted { used: u64, limit: u64 },

    #[error("Compression failed: {0}")]
    CompressionFailed(String),

    #[error("Memory error: {0}")]
    MemoryError(String),
}

// === Config Errors ===

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Config file not found: {0}")]
    NotFound(String),

    #[error("Invalid config: {0}")]
    Invalid(String),

    #[error("Missing required field: {0}")]
    MissingField(String),
}

// === Unified Error ===

#[derive(Debug, thiserror::Error)]
pub enum CatCodeError {
    #[error("Provider error: {0}")]
    Provider(#[from] ProviderError),

    #[error("Tool error: {0}")]
    Tool(#[from] ToolError),

    #[error("Middleware error: {0}")]
    Middleware(#[from] MiddlewareError),

    #[error("Context error: {0}")]
    Context(#[from] ContextError),

    #[error("Config error: {0}")]
    Config(#[from] ConfigError),

    #[error("{0}")]
    Other(String),
}

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
}
