use async_trait::async_trait;
use catcode_core::{OperationLevel, Tool, ToolContext, ToolResult};
use serde_json::{Value, json};
use tokio::fs;

use crate::read_file::resolve_path;

/// Writes content to a file. Creates parent directories if needed.
///
/// Parameters:
/// - `path` (string, required): File path. Relative paths resolve against `ctx.working_dir`.
/// - `content` (string, required): Content to write.
pub struct WriteFileTool;

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write content to a file. Creates parent directories if they don't exist. Overwrites existing files."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to write. Relative paths resolve against the working directory."
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file."
                }
            },
            "required": ["path", "content"]
        })
    }

    fn operation_level(&self) -> OperationLevel {
        OperationLevel::Sensitive
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        // Extract path
        let path_str = match args.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return ToolResult::error("Missing required argument: path"),
        };

        // Extract content
        let content = match args.get("content").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => return ToolResult::error("Missing required argument: content"),
        };

        // Resolve path
        let path = resolve_path(path_str, ctx);

        // Dry run — report what would happen without writing
        if ctx.dry_run {
            return ToolResult::success(format!(
                "[dry-run] Would write {} bytes to {}",
                content.len(),
                path.display()
            ));
        }

        // Create parent directories
        if let Some(parent) = path.parent()
            && let Err(e) = fs::create_dir_all(parent).await
        {
            return ToolResult::error(format!(
                "Failed to create parent directories for {}: {}",
                path.display(),
                e
            ));
        }

        // Write file
        if let Err(e) = fs::write(&path, content).await {
            return ToolResult::error(format!("Failed to write {}: {}", path.display(), e));
        }

        ToolResult::success(format!(
            "Wrote {} bytes to {}",
            content.len(),
            path.display()
        ))
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
    async fn test_write_file_create_new() {
        let tmp = TempDir::new().unwrap();
        let tool = WriteFileTool;
        let ctx = make_ctx(tmp.path());

        let result = tool
            .execute(
                json!({"path": "new_file.txt", "content": "hello world"}),
                &ctx,
            )
            .await;

        assert!(!result.is_error);
        let written = fs::read_to_string(tmp.path().join("new_file.txt")).unwrap();
        assert_eq!(written, "hello world");
    }

    #[tokio::test]
    async fn test_write_file_overwrite() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("existing.txt"), "old content").unwrap();

        let tool = WriteFileTool;
        let ctx = make_ctx(tmp.path());
        let result = tool
            .execute(
                json!({"path": "existing.txt", "content": "new content"}),
                &ctx,
            )
            .await;

        assert!(!result.is_error);
        let written = fs::read_to_string(tmp.path().join("existing.txt")).unwrap();
        assert_eq!(written, "new content");
    }

    #[tokio::test]
    async fn test_write_file_creates_parent_dirs() {
        let tmp = TempDir::new().unwrap();
        let tool = WriteFileTool;
        let ctx = make_ctx(tmp.path());

        let result = tool
            .execute(
                json!({"path": "deep/nested/dir/file.txt", "content": "deep"}),
                &ctx,
            )
            .await;

        assert!(!result.is_error);
        let written = fs::read_to_string(tmp.path().join("deep/nested/dir/file.txt")).unwrap();
        assert_eq!(written, "deep");
    }

    #[tokio::test]
    async fn test_write_file_missing_path() {
        let tmp = TempDir::new().unwrap();
        let tool = WriteFileTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"content": "data"}), &ctx).await;

        assert!(result.is_error);
        assert!(result.output.contains("path"));
    }

    #[tokio::test]
    async fn test_write_file_missing_content() {
        let tmp = TempDir::new().unwrap();
        let tool = WriteFileTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"path": "file.txt"}), &ctx).await;

        assert!(result.is_error);
        assert!(result.output.contains("content"));
    }

    #[tokio::test]
    async fn test_write_file_dry_run() {
        let tmp = TempDir::new().unwrap();
        let tool = WriteFileTool;
        let ctx = ToolContext {
            session_id: Some("test".to_string()),
            project_dir: Some(tmp.path().to_path_buf()),
            working_dir: Some(tmp.path().to_path_buf()),
            dry_run: true,
        };

        let result = tool
            .execute(
                json!({"path": "dry.txt", "content": "should not write"}),
                &ctx,
            )
            .await;

        assert!(!result.is_error);
        assert!(!tmp.path().join("dry.txt").exists());
    }

    #[test]
    fn test_write_file_metadata() {
        let tool = WriteFileTool;
        assert_eq!(tool.name(), "write_file");
        assert!(matches!(
            tool.operation_level(),
            catcode_core::OperationLevel::Sensitive
        ));
    }

    #[test]
    fn test_write_file_schema() {
        let tool = WriteFileTool;
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["path"].is_object());
        assert!(schema["properties"]["content"].is_object());
    }
}
