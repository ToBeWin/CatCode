use async_trait::async_trait;
use catcode_core::{OperationLevel, Tool, ToolContext, ToolResult};
use serde_json::{Value, json};
use std::process::Command;

const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_OUTPUT_BYTES: usize = 1024 * 1024; // 1 MB

/// Executes a shell command via `/bin/sh -c`.
///
/// Parameters:
/// - `command` (string, required): The shell command to execute.
///
/// The command runs in the working directory from `ctx.working_dir`.
/// stdout and stderr are combined in the output.
pub struct BashTool {
    #[allow(dead_code)]
    timeout_secs: u64,
}

impl BashTool {
    pub fn new() -> Self {
        Self {
            timeout_secs: DEFAULT_TIMEOUT_SECS,
        }
    }

    pub fn with_timeout(timeout_secs: u64) -> Self {
        Self { timeout_secs }
    }
}

impl Default for BashTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Execute a shell command. stdout and stderr are combined in the output. Dangerous commands require approval."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute."
                }
            },
            "required": ["command"]
        })
    }

    fn operation_level(&self) -> OperationLevel {
        OperationLevel::Dangerous
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        let command = match args.get("command").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => return ToolResult::error("Missing required argument: command"),
        };

        // Dry run
        if ctx.dry_run {
            return ToolResult::success(format!("[dry-run] Would execute: {}", command));
        }

        // Build command
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c").arg(command);

        // Set working directory
        if let Some(ref wd) = ctx.working_dir {
            cmd.current_dir(wd);
        } else if let Some(ref pd) = ctx.project_dir {
            cmd.current_dir(pd);
        }

        // Execute
        let output = match cmd.output() {
            Ok(o) => o,
            Err(e) => {
                return ToolResult::error(format!("Failed to execute command: {}", e));
            }
        };

        // Combine stdout + stderr
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let mut combined = String::new();

        if !stdout.is_empty() {
            combined.push_str(&stdout);
        }
        if !stderr.is_empty() {
            if !combined.is_empty() {
                combined.push_str("\n--- stderr ---\n");
            }
            combined.push_str(&stderr);
        }

        // Truncate if too large
        if combined.len() > MAX_OUTPUT_BYTES {
            let truncated = &combined[..MAX_OUTPUT_BYTES];
            combined = format!(
                "{}\n... (output truncated, {} bytes total)",
                truncated,
                combined.len()
            );
        }

        if combined.is_empty() {
            combined = "(no output)".to_string();
        }

        // Check exit code
        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            return ToolResult::error(format!(
                "Command failed with exit code {}:\n{}",
                code, combined
            ));
        }

        ToolResult::success(combined)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::{Tool, ToolContext};
    use serde_json::json;
    use tempfile::TempDir;

    fn make_ctx(project_dir: &std::path::Path) -> ToolContext {
        ToolContext {
            session_id: Some("test".to_string()),
            project_dir: Some(project_dir.to_path_buf()),
            working_dir: Some(project_dir.to_path_buf()),
            dry_run: false,
        }
    }

    #[tokio::test]
    async fn test_bash_simple_command() {
        let tmp = TempDir::new().unwrap();
        let tool = BashTool::new();
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"command": "echo hello"}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.output.contains("hello"));
    }

    #[tokio::test]
    async fn test_bash_exit_code_zero() {
        let tmp = TempDir::new().unwrap();
        let tool = BashTool::new();
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"command": "true"}), &ctx).await;

        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn test_bash_exit_code_nonzero() {
        let tmp = TempDir::new().unwrap();
        let tool = BashTool::new();
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"command": "false"}), &ctx).await;

        assert!(result.is_error);
        assert!(result.output.contains("exit code"));
    }

    #[tokio::test]
    async fn test_bash_stderr_captured() {
        let tmp = TempDir::new().unwrap();
        let tool = BashTool::new();
        let ctx = make_ctx(tmp.path());
        let result = tool
            .execute(json!({"command": "echo error_msg >&2"}), &ctx)
            .await;

        assert!(!result.is_error);
        assert!(result.output.contains("error_msg"));
    }

    #[tokio::test]
    async fn test_bash_working_directory() {
        let tmp = TempDir::new().unwrap();
        let tool = BashTool::new();
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"command": "pwd"}), &ctx).await;

        assert!(!result.is_error);
        assert!(
            result
                .output
                .contains(&tmp.path().to_string_lossy().to_string())
        );
    }

    #[tokio::test]
    async fn test_bash_missing_command() {
        let tmp = TempDir::new().unwrap();
        let tool = BashTool::new();
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({}), &ctx).await;

        assert!(result.is_error);
        assert!(result.output.contains("command"));
    }

    #[tokio::test]
    async fn test_bash_dry_run() {
        let tmp = TempDir::new().unwrap();
        let tool = BashTool::new();
        let ctx = ToolContext {
            session_id: Some("test".to_string()),
            project_dir: Some(tmp.path().to_path_buf()),
            working_dir: Some(tmp.path().to_path_buf()),
            dry_run: true,
        };
        let result = tool.execute(json!({"command": "rm -rf /"}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.output.contains("dry-run"));
    }

    #[tokio::test]
    async fn test_bash_command_with_args() {
        let tmp = TempDir::new().unwrap();
        let tool = BashTool::new();
        let ctx = make_ctx(tmp.path());
        let result = tool
            .execute(json!({"command": "echo -n no-newline"}), &ctx)
            .await;

        assert!(!result.is_error);
        assert!(result.output.contains("no-newline"));
    }

    #[test]
    fn test_bash_metadata() {
        let tool = BashTool::new();
        assert_eq!(tool.name(), "bash");
        assert!(matches!(
            tool.operation_level(),
            catcode_core::OperationLevel::Dangerous
        ));
    }
}
