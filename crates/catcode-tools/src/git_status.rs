use async_trait::async_trait;
use catcode_core::{OperationLevel, Tool, ToolContext, ToolResult};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::Command;

/// Runs `git status` in the working directory.
///
/// Parameters:
/// - `path` (string, optional): Path to the git repository. Defaults to `ctx.working_dir`.
pub struct GitStatusTool;

#[async_trait]
impl Tool for GitStatusTool {
    fn name(&self) -> &str {
        "git_status"
    }

    fn description(&self) -> &str {
        "Show the working tree status. Runs git status in the repository directory."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the git repository. Defaults to the working directory."
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
        let repo_path = resolve_repo_path(&args, ctx);

        let mut cmd = Command::new("git");
        cmd.arg("status");
        cmd.current_dir(&repo_path);

        let output = match cmd.output() {
            Ok(o) => o,
            Err(e) => {
                return ToolResult::error(format!("Failed to run git status: {}", e));
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            return ToolResult::error(format!(
                "git status failed (exit {}): {}",
                output.status.code().unwrap_or(-1),
                stderr,
            ));
        }

        ToolResult::success(stdout.trim().to_string())
    }
}

fn resolve_repo_path(args: &Value, ctx: &ToolContext) -> PathBuf {
    if let Some(p) = args.get("path").and_then(|v| v.as_str()) {
        let path = PathBuf::from(p);
        if path.is_absolute() {
            return path;
        } else if let Some(ref wd) = ctx.working_dir {
            return wd.join(path);
        } else if let Some(ref pd) = ctx.project_dir {
            return pd.join(path);
        }
        return path;
    }
    if let Some(ref wd) = ctx.working_dir {
        wd.clone()
    } else if let Some(ref pd) = ctx.project_dir {
        pd.clone()
    } else {
        PathBuf::from(".")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::{Tool, ToolContext};
    use serde_json::json;
    use std::fs;
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
    async fn test_git_status_clean() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());

        let tool = GitStatusTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.output.contains("nothing to commit") || result.output.contains("clean"));
    }

    #[tokio::test]
    async fn test_git_status_modified() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());
        fs::write(tmp.path().join("file.txt"), "modified").unwrap();

        let tool = GitStatusTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.output.contains("modified") || result.output.contains("file.txt"));
    }

    #[tokio::test]
    async fn test_git_status_not_a_repo() {
        let tmp = TempDir::new().unwrap();

        let tool = GitStatusTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({}), &ctx).await;

        assert!(result.is_error);
    }

    #[tokio::test]
    async fn test_git_status_with_path() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());
        let sub = tmp.path().join("sub");
        fs::create_dir(&sub).unwrap();
        init_git_repo(&sub);

        let tool = GitStatusTool;
        let ctx = make_ctx(tmp.path());
        let result = tool
            .execute(json!({"path": sub.to_str().unwrap()}), &ctx)
            .await;

        assert!(!result.is_error);
    }

    #[test]
    fn test_git_status_metadata() {
        let tool = GitStatusTool;
        assert_eq!(tool.name(), "git_status");
        assert!(matches!(
            tool.operation_level(),
            catcode_core::OperationLevel::Safe
        ));
    }

    #[test]
    fn test_git_status_schema() {
        let tool = GitStatusTool;
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
    }
}
