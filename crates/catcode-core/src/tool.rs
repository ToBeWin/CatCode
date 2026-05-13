use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// === ToolProgress ===

/// Progress event during streaming tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolProgress {
    /// Tool started.
/// [`Started`].
    Started {
        tool_name: String,
        tool_args: serde_json::Value,
    },
    /// Partial output during execution.
/// [`Progress`].
    Progress {
        tool_name: String,
        output: String,
    },
    /// Tool completed.
/// [`Completed`].
    Completed {
        tool_name: String,
        result: ToolResult,
    },
    /// Tool failed.
/// [`Failed`].
    Failed {
        tool_name: String,
        error: String,
    },
}

// === OperationLevel ===

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
/// [`OperationLevel`]
pub enum OperationLevel {
/// [`Safe`].
    Safe,
/// [`Sensitive`].
    Sensitive,
/// [`Dangerous`].
    Dangerous,
}

// === ToolResult ===

#[derive(Debug, Clone, Serialize, Deserialize)]
/// [`ToolResult`]
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

/// Error.
    pub fn error(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            is_error: true,
            metadata: serde_json::Value::Null,
        }
    }

/// Configure metadata.
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }
}

// === ToolContext ===

#[derive(Debug, Clone, Default)]
/// [`ToolContext`]
pub struct ToolContext {
    pub session_id: Option<String>,
    pub project_dir: Option<std::path::PathBuf>,
    pub working_dir: Option<std::path::PathBuf>,
    pub dry_run: bool,
}

// === Tool Trait ===

#[async_trait]
/// [`Tool`]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;
    fn operation_level(&self) -> OperationLevel;

    /// Whether this tool can run concurrently with other safe tools.
    /// Returns true for read-only tools (read_file, grep, glob, etc.)
    /// Returns false for write tools that modify state (write_file, edit, bash)
    fn is_concurrency_safe(&self) -> bool {
        false
    }

    /// Whether this tool is read-only (can batch with other reads).
    fn is_read_only(&self) -> bool {
        false
    }

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

    #[test]
    fn test_tool_result_success_empty() {
        let result = ToolResult::success("");
        assert_eq!(result.output, "");
        assert!(!result.is_error);
    }

    #[test]
    fn test_tool_result_error_empty() {
        let result = ToolResult::error("");
        assert_eq!(result.output, "");
        assert!(result.is_error);
    }

    #[test]
    fn test_tool_result_with_metadata_on_error() {
        let result = ToolResult::error("fail").with_metadata(serde_json::json!({"code": 500}));
        assert!(result.is_error);
        assert_eq!(result.metadata["code"], 500);
    }

    #[test]
    fn test_tool_result_long_output() {
        let long = "a".repeat(100_000);
        let result = ToolResult::success(&long);
        assert_eq!(result.output.len(), 100_000);
        assert!(!result.is_error);
    }

    #[test]
    fn test_tool_context_custom() {
        let ctx = ToolContext {
            session_id: Some("sess_001".to_string()),
            project_dir: Some("/home/user/project".into()),
            working_dir: Some("/home/user/project/src".into()),
            dry_run: true,
        };
        assert_eq!(ctx.session_id.as_deref(), Some("sess_001"));
        assert_eq!(ctx.project_dir.as_ref().unwrap().to_str(), Some("/home/user/project"));
        assert!(ctx.dry_run);
    }

    #[test]
    fn test_operation_level_debug() {
        assert_eq!(format!("{:?}", OperationLevel::Safe), "Safe");
        assert_eq!(format!("{:?}", OperationLevel::Sensitive), "Sensitive");
        assert_eq!(format!("{:?}", OperationLevel::Dangerous), "Dangerous");
    }

    #[test]
    fn test_operation_level_ordering() {
        assert!(OperationLevel::Safe < OperationLevel::Sensitive);
        assert!(OperationLevel::Sensitive < OperationLevel::Dangerous);
        assert!(OperationLevel::Safe < OperationLevel::Dangerous);
    }

    #[test]
    fn test_tool_progress_started() {
        let progress = ToolProgress::Started {
            tool_name: "bash".to_string(),
            tool_args: serde_json::json!({"cmd": "ls"}),
        };
        match progress {
            ToolProgress::Started { tool_name, .. } => assert_eq!(tool_name, "bash"),
            _ => panic!("expected Started"),
        }
    }

    #[test]
    fn test_tool_progress_all_variants() {
        let _p1 = ToolProgress::Progress {
            tool_name: "bash".to_string(),
            output: "output".to_string(),
        };
        let _p2 = ToolProgress::Completed {
            tool_name: "bash".to_string(),
            result: ToolResult::success("done"),
        };
        let _p3 = ToolProgress::Failed {
            tool_name: "bash".to_string(),
            error: "error".to_string(),
        };
    }

    #[test]
    fn test_tool_call_creation() {
        let call = ToolCall {
            id: "call_abc".to_string(),
            name: "read_file".to_string(),
            args: serde_json::json!({"path": "/tmp/test.txt"}),
        };
        assert_eq!(call.id, "call_abc");
        assert_eq!(call.name, "read_file");
    }
}
