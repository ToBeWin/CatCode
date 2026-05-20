use async_trait::async_trait;
use catcode_core::{OperationLevel, Tool, ToolContext, ToolResult};
use serde_json::{Value, json};
use tokio::fs;

use crate::read_file::resolve_path;

/// Deletes a file or directory.
///
/// Parameters:
/// - `path` (string, required): Path to delete.
/// - `recursive` (boolean, optional): Delete directories recursively.
pub struct DeleteFileTool;

#[async_trait]
impl Tool for DeleteFileTool {
    fn name(&self) -> &str {
        "delete_file"
    }

    fn description(&self) -> &str {
        "Delete a file or directory. Use recursive=true for non-empty directories."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file or directory to delete. Relative paths resolve against the working directory."
                },
                "recursive": {
                    "type": "boolean",
                    "description": "If true and the path is a directory, delete it recursively. Default: false."
                }
            },
            "required": ["path"]
        })
    }

    fn operation_level(&self) -> OperationLevel {
        OperationLevel::Dangerous
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    fn is_read_only(&self) -> bool {
        false
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        let path_str = match args.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return ToolResult::error("Missing required argument: path"),
        };

        let path = resolve_path(path_str, ctx);

        let metadata = match fs::metadata(&path).await {
            Ok(m) => m,
            Err(e) => {
                return ToolResult::error(format!(
                    "Path does not exist: {} ({})",
                    path.display(),
                    e
                ));
            }
        };

        if ctx.dry_run {
            let what = if metadata.is_dir() {
                "directory"
            } else {
                "file"
            };
            return ToolResult::success(format!(
                "[dry-run] Would delete {} {}",
                what,
                path.display()
            ));
        }

        if metadata.is_dir() {
            let recursive = args
                .get("recursive")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            if recursive {
                match fs::remove_dir_all(&path).await {
                    Ok(_) => ToolResult::success(format!(
                        "Deleted directory {} (recursive)",
                        path.display()
                    )),
                    Err(e) => ToolResult::error(format!(
                        "Failed to delete directory {}: {}",
                        path.display(),
                        e
                    )),
                }
            } else {
                // Try non-recursive first
                match fs::remove_dir(&path).await {
                    Ok(_) => ToolResult::success(format!("Deleted directory {}", path.display())),
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::DirectoryNotEmpty {
                            ToolResult::error(format!(
                                "Directory {} is not empty. Use recursive=true to delete it.",
                                path.display()
                            ))
                        } else {
                            ToolResult::error(format!(
                                "Failed to delete directory {}: {}",
                                path.display(),
                                e
                            ))
                        }
                    }
                }
            }
        } else {
            match fs::remove_file(&path).await {
                Ok(_) => ToolResult::success(format!("Deleted file {}", path.display())),
                Err(e) => {
                    ToolResult::error(format!("Failed to delete file {}: {}", path.display(), e))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::{Tool, ToolContext};
    use serde_json::json;
    use std::fs;
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
    async fn test_delete_file_basic() {
        let tmp = TempDir::new().unwrap();
        let file_path = tmp.path().join("to_delete.txt");
        fs::write(&file_path, "delete me").unwrap();

        let tool = DeleteFileTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"path": "to_delete.txt"}), &ctx).await;

        assert!(!result.is_error);
        assert!(!file_path.exists());
    }

    #[tokio::test]
    async fn test_delete_file_not_found() {
        let tmp = TempDir::new().unwrap();
        let tool = DeleteFileTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"path": "nonexistent.txt"}), &ctx).await;

        assert!(result.is_error);
        assert!(result.output.contains("does not exist"));
    }

    #[tokio::test]
    async fn test_delete_dir_non_recursive_fails() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("mydir");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("file.txt"), "content").unwrap();

        let tool = DeleteFileTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"path": "mydir"}), &ctx).await;

        assert!(result.is_error);
        assert!(result.output.contains("not empty"));
        assert!(dir.exists());
    }

    #[tokio::test]
    async fn test_delete_dir_recursive() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("mydir");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("file.txt"), "content").unwrap();

        let tool = DeleteFileTool;
        let ctx = make_ctx(tmp.path());
        let result = tool
            .execute(json!({"path": "mydir", "recursive": true}), &ctx)
            .await;

        assert!(!result.is_error);
        assert!(!dir.exists());
    }

    #[tokio::test]
    async fn test_delete_file_dry_run() {
        let tmp = TempDir::new().unwrap();
        let file_path = tmp.path().join("keep.txt");
        fs::write(&file_path, "keep me").unwrap();

        let tool = DeleteFileTool;
        let ctx = ToolContext {
            session_id: Some("test".to_string()),
            project_dir: Some(tmp.path().to_path_buf()),
            working_dir: Some(tmp.path().to_path_buf()),
            dry_run: true,
        };
        let result = tool.execute(json!({"path": "keep.txt"}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.output.contains("dry-run"));
        assert!(file_path.exists());
    }

    #[tokio::test]
    async fn test_delete_file_missing_path() {
        let tmp = TempDir::new().unwrap();
        let tool = DeleteFileTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({}), &ctx).await;

        assert!(result.is_error);
        assert!(result.output.contains("path"));
    }

    #[test]
    fn test_delete_file_metadata() {
        let tool = DeleteFileTool;
        assert_eq!(tool.name(), "delete_file");
        assert!(matches!(
            tool.operation_level(),
            catcode_core::OperationLevel::Dangerous
        ));
    }

    #[test]
    fn test_delete_file_schema() {
        let tool = DeleteFileTool;
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["path"].is_object());
        assert!(schema["properties"]["recursive"].is_object());
    }

    #[tokio::test]
    async fn test_delete_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("emptydir");
        fs::create_dir(&dir).unwrap();

        let tool = DeleteFileTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"path": "emptydir"}), &ctx).await;

        assert!(!result.is_error);
        assert!(!dir.exists());
    }
}
