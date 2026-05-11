use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// === OperationLevel ===

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationLevel {
    Safe,
    Sensitive,
    Dangerous,
}

// === ToolResult ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub output: String,
    pub is_error: bool,
    pub metadata: serde_json::Value,
}

impl ToolResult {
    pub fn success(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            is_error: false,
            metadata: serde_json::Value::Null,
        }
    }

    pub fn error(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            is_error: true,
            metadata: serde_json::Value::Null,
        }
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }
}

// === ToolContext ===

#[derive(Debug, Clone, Default)]
pub struct ToolContext {
    pub session_id: Option<String>,
    pub project_dir: Option<std::path::PathBuf>,
    pub working_dir: Option<std::path::PathBuf>,
    pub dry_run: bool,
}

// === Tool Trait ===

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;
    fn operation_level(&self) -> OperationLevel;
    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> ToolResult;
}

// Re-export types from types.rs for convenience
pub use crate::types::{ToolCall, ToolDefinition};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_operation_level_variants() {
        assert!(matches!(OperationLevel::Safe, OperationLevel::Safe));
        assert!(matches!(
            OperationLevel::Sensitive,
            OperationLevel::Sensitive
        ));
        assert!(matches!(
            OperationLevel::Dangerous,
            OperationLevel::Dangerous
        ));
    }

    #[test]
    fn test_tool_result_success() {
        let result = ToolResult::success("file content here");
        assert_eq!(result.output, "file content here");
        assert!(!result.is_error);
    }

    #[test]
    fn test_tool_result_error() {
        let result = ToolResult::error("file not found");
        assert_eq!(result.output, "file not found");
        assert!(result.is_error);
    }

    #[test]
    fn test_tool_result_with_metadata() {
        let result = ToolResult::success("ok").with_metadata(serde_json::json!({"bytes": 100}));
        assert!(!result.is_error);
        assert_eq!(result.metadata["bytes"], 100);
    }

    #[test]
    fn test_tool_context_default() {
        let ctx = ToolContext::default();
        assert!(ctx.session_id.is_none());
        assert!(ctx.project_dir.is_none());
        assert!(!ctx.dry_run);
    }
}
