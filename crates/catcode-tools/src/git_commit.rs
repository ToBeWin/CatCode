use async_trait::async_trait;
use catcode_core::{OperationLevel, Tool, ToolContext, ToolResult};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::Command;

/// Creates a git commit with the given message.
///
/// Parameters:
/// - `message` (string, required): Commit message.
/// - `paths` (array of strings, optional): Specific files to commit. Defaults to all tracked changes.
pub struct GitCommitTool;

#[async_trait]
impl Tool for GitCommitTool {
    fn name(&self) -> &str {
        "git_commit"
    }

    fn description(&self) -> &str {
        "Create a git commit with the given message. Stages specified files (or all changes) then commits."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "Commit message."
                },
                "paths": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Specific files to commit. If omitted, all changes are staged."
                }
            },
            "required": ["message"]
        })
    }

    fn operation_level(&self) -> OperationLevel {
        OperationLevel::Sensitive
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    fn is_read_only(&self) -> bool {
        false
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        let message = match args.get("message").and_then(|v| v.as_str()) {
            Some(m) => m.trim(),
            None => return ToolResult::error("Missing required argument: message"),
        };

        if message.is_empty() {
            return ToolResult::error("Commit message cannot be empty");
        }

        let repo_path = resolve_repo_path(ctx);

        if ctx.dry_run {
            let paths_desc = if let Some(paths) = args.get("paths").and_then(|v| v.as_array()) {
                let files: Vec<&str> = paths.iter().filter_map(|v| v.as_str()).collect();
                format!("{} files", files.len())
            } else {
                "all changes".to_string()
            };
            return ToolResult::success(format!(
                "[dry-run] Would commit {} with message: {}",
                paths_desc, message
            ));
        }

        // Stage files
        if let Some(paths) = args.get("paths").and_then(|v| v.as_array()) {
            for path_val in paths {
                let file = match path_val.as_str() {
                    Some(f) => f,
                    None => continue,
                };
                let mut add_cmd = Command::new("git");
                add_cmd.arg("add").arg(file).current_dir(&repo_path);
                let output = match add_cmd.output() {
                    Ok(o) => o,
                    Err(e) => {
                        return ToolResult::error(format!("Failed to git add '{}': {}", file, e));
                    }
                };
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return ToolResult::error(format!(
                        "git add '{}' failed (exit {}): {}",
                        file,
                        output.status.code().unwrap_or(-1),
                        stderr,
                    ));
                }
            }
        } else {
            let mut add_cmd = Command::new("git");
            add_cmd.args(["add", "-A"]).current_dir(&repo_path);
            let output = match add_cmd.output() {
                Ok(o) => o,
                Err(e) => {
                    return ToolResult::error(format!("Failed to git add -A: {}", e));
                }
            };
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return ToolResult::error(format!(
                    "git add -A failed (exit {}): {}",
                    output.status.code().unwrap_or(-1),
                    stderr,
                ));
            }
        }

        // Commit
        let mut commit_cmd = Command::new("git");
        commit_cmd
            .args(["commit", "-m", message])
            .current_dir(&repo_path);
        let output = match commit_cmd.output() {
            Ok(o) => o,
            Err(e) => {
                return ToolResult::error(format!("Failed to run git commit: {}", e));
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            return ToolResult::error(format!(
                "git commit failed (exit {}): {}",
                output.status.code().unwrap_or(-1),
                if stdout.trim().is_empty() {
                    stderr.trim()
                } else {
                    stdout.trim()
                },
            ));
        }

        ToolResult::success(stdout.trim().to_string())
    }
}

fn resolve_repo_path(ctx: &ToolContext) -> PathBuf {
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
    async fn test_git_commit_basic() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());
        fs::write(tmp.path().join("file.txt"), "content").unwrap();

        let tool = GitCommitTool;
        let ctx = make_ctx(tmp.path());
        let result = tool
            .execute(json!({"message": "initial commit"}), &ctx)
            .await;

        assert!(!result.is_error);
        assert!(result.output.contains("commit") || result.output.contains("file changed"));
    }

    #[tokio::test]
    async fn test_git_commit_with_paths() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());
        fs::write(tmp.path().join("a.txt"), "a").unwrap();
        fs::write(tmp.path().join("b.txt"), "b").unwrap();

        let tool = GitCommitTool;
        let ctx = make_ctx(tmp.path());
        let result = tool
            .execute(json!({"message": "commit a", "paths": ["a.txt"]}), &ctx)
            .await;

        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn test_git_commit_empty_message() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());

        let tool = GitCommitTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"message": ""}), &ctx).await;

        assert!(result.is_error);
        assert!(result.output.contains("empty"));
    }

    #[tokio::test]
    async fn test_git_commit_missing_message() {
        let tmp = TempDir::new().unwrap();
        let tool = GitCommitTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({}), &ctx).await;

        assert!(result.is_error);
        assert!(result.output.contains("message"));
    }

    #[tokio::test]
    async fn test_git_commit_dry_run() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());
        fs::write(tmp.path().join("file.txt"), "content").unwrap();

        let tool = GitCommitTool;
        let ctx = ToolContext {
            session_id: Some("test".to_string()),
            project_dir: Some(tmp.path().to_path_buf()),
            working_dir: Some(tmp.path().to_path_buf()),
            dry_run: true,
        };
        let result = tool.execute(json!({"message": "test commit"}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.output.contains("dry-run"));
    }

    #[tokio::test]
    async fn test_git_commit_not_a_repo() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("file.txt"), "content").unwrap();

        let tool = GitCommitTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"message": "test"}), &ctx).await;

        assert!(result.is_error);
    }

    #[test]
    fn test_git_commit_metadata() {
        let tool = GitCommitTool;
        assert_eq!(tool.name(), "git_commit");
        assert!(matches!(
            tool.operation_level(),
            catcode_core::OperationLevel::Sensitive
        ));
    }

    #[test]
    fn test_git_commit_schema() {
        let tool = GitCommitTool;
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["message"].is_object());
        assert!(schema["properties"]["paths"].is_object());
    }
}
