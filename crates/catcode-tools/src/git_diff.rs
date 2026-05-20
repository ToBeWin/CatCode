use async_trait::async_trait;
use catcode_core::{OperationLevel, Tool, ToolContext, ToolResult};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::Command;

/// Runs `git diff` in the working directory.
///
/// Parameters:
/// - `path` (string, optional): Specific file path to diff.
/// - `staged` (boolean, optional): Show staged changes (git diff --cached).
pub struct GitDiffTool;

#[async_trait]
impl Tool for GitDiffTool {
    fn name(&self) -> &str {
        "git_diff"
    }

    fn description(&self) -> &str {
        "Show changes between commits, commit and working tree, etc. Runs git diff."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Specific file path to diff. Diffs all files if omitted."
                },
                "staged": {
                    "type": "boolean",
                    "description": "If true, show staged changes (git diff --cached). Default: false."
                }
            },
            "required": []
        })
    }

    fn operation_level(&self) -> OperationLevel {
        OperationLevel::Safe
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn is_read_only(&self) -> bool {
        true
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        let repo_path = if let Some(ref wd) = ctx.working_dir {
            wd.clone()
        } else if let Some(ref pd) = ctx.project_dir {
            pd.clone()
        } else {
            PathBuf::from(".")
        };

        let mut cmd = Command::new("git");
        cmd.arg("diff");

        if args
            .get("staged")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            cmd.arg("--cached");
        }

        if let Some(file_path) = args.get("path").and_then(|v| v.as_str()) {
            cmd.arg("--").arg(file_path);
        }

        cmd.current_dir(&repo_path);

        let output = match cmd.output() {
            Ok(o) => o,
            Err(e) => {
                return ToolResult::error(format!("Failed to run git diff: {}", e));
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            return ToolResult::error(format!(
                "git diff failed (exit {}): {}",
                output.status.code().unwrap_or(-1),
                stderr,
            ));
        }

        if stdout.trim().is_empty() {
            ToolResult::success("(no changes)".to_string())
        } else {
            ToolResult::success(stdout.trim().to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::{Tool, ToolContext};
    use serde_json::json;
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    fn init_git_repo(path: &std::path::Path) {
        Command::new("git")
            .args(["init"])
            .current_dir(path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(path)
            .output()
            .unwrap();
    }

    fn make_ctx(project_dir: &std::path::Path) -> ToolContext {
        ToolContext {
            session_id: Some("test".to_string()),
            project_dir: Some(project_dir.to_path_buf()),
            working_dir: Some(project_dir.to_path_buf()),
            dry_run: false,
        }
    }

    #[tokio::test]
    async fn test_git_diff_no_changes() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());

        let tool = GitDiffTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.output.contains("no changes") || result.output.is_empty());
    }

    #[tokio::test]
    async fn test_git_diff_unstaged() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());
        fs::write(tmp.path().join("file.txt"), "original").unwrap();
        Command::new("git")
            .args(["add", "file.txt"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        fs::write(tmp.path().join("file.txt"), "modified").unwrap();

        let tool = GitDiffTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.output.contains("-original") || result.output.contains("+modified"));
    }

    #[tokio::test]
    async fn test_git_diff_staged() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());
        fs::write(tmp.path().join("file.txt"), "original").unwrap();
        Command::new("git")
            .args(["add", "file.txt"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(tmp.path())
            .output()
            .unwrap();

        fs::write(tmp.path().join("file.txt"), "staged change").unwrap();
        Command::new("git")
            .args(["add", "file.txt"])
            .current_dir(tmp.path())
            .output()
            .unwrap();

        let tool = GitDiffTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"staged": true}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.output.contains("+staged change"));
    }

    #[tokio::test]
    async fn test_git_diff_not_a_repo() {
        let tmp = TempDir::new().unwrap();

        let tool = GitDiffTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({}), &ctx).await;

        assert!(result.is_error);
    }

    #[test]
    fn test_git_diff_metadata() {
        let tool = GitDiffTool;
        assert_eq!(tool.name(), "git_diff");
        assert!(matches!(
            tool.operation_level(),
            catcode_core::OperationLevel::Safe
        ));
    }

    #[test]
    fn test_git_diff_schema() {
        let tool = GitDiffTool;
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["path"].is_object());
        assert!(schema["properties"]["staged"].is_object());
    }
}
