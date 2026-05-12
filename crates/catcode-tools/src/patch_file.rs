use async_trait::async_trait;
use catcode_core::{OperationLevel, Tool, ToolContext, ToolResult};
use serde_json::{Value, json};
use tokio::fs;

use crate::read_file::resolve_path;

pub struct PatchFileTool;

#[async_trait]
impl Tool for PatchFileTool {
    fn name(&self) -> &str {
        "patch_file"
    }

    fn description(&self) -> &str {
        "Apply a patch to a file by finding exact text and replacing it. The old_string must match exactly once."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to patch. Relative paths resolve against the working directory."
                },
                "old_string": {
                    "type": "string",
                    "description": "The exact text to find and replace."
                },
                "new_string": {
                    "type": "string",
                    "description": "The replacement text."
                }
            },
            "required": ["path", "old_string", "new_string"]
        })
    }

    fn operation_level(&self) -> OperationLevel {
        OperationLevel::Sensitive
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        let path_str = match args.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return ToolResult::error("Missing required argument: path"),
        };

        let old_string = match args.get("old_string").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return ToolResult::error("Missing required argument: old_string"),
        };

        let new_string = match args.get("new_string").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return ToolResult::error("Missing required argument: new_string"),
        };

        let path = resolve_path(path_str, ctx);

        let content = match fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(e) => {
                return ToolResult::error(format!("Failed to read {}: {}", path.display(), e));
            }
        };

        let count = content.matches(old_string).count();
        if count == 0 {
            return ToolResult::error(format!(
                "old_string not found in {}",
                path.display()
            ));
        }
        if count > 1 {
            return ToolResult::error(format!(
                "Found {} occurrences of old_string in {}. Expected exactly 1.",
                count,
                path.display()
            ));
        }

        if ctx.dry_run {
            return ToolResult::success(format!(
                "[dry-run] Would patch {} (replace 1 occurrence)",
                path.display()
            ));
        }

        let new_content = content.replace(old_string, new_string);

        if let Err(e) = fs::write(&path, &new_content).await {
            return ToolResult::error(format!("Failed to write {}: {}", path.display(), e));
        }

        ToolResult::success(format!(
            "Patched {} (replaced 1 occurrence)",
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
    async fn test_patch_file_basic() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("file.txt"), "hello world foo").unwrap();

        let tool = PatchFileTool;
        let ctx = make_ctx(tmp.path());
        let result = tool
            .execute(
                json!({"path": "file.txt", "old_string": "world", "new_string": "there"}),
                &ctx,
            )
            .await;

        assert!(!result.is_error);
        let content = fs::read_to_string(tmp.path().join("file.txt")).unwrap();
        assert_eq!(content, "hello there foo");
    }

    #[tokio::test]
    async fn test_patch_file_not_found() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("file.txt"), "hello world").unwrap();

        let tool = PatchFileTool;
        let ctx = make_ctx(tmp.path());
        let result = tool
            .execute(
                json!({"path": "file.txt", "old_string": "zzz", "new_string": "xxx"}),
                &ctx,
            )
            .await;

        assert!(result.is_error);
        assert!(result.output.contains("not found"));
    }

    #[tokio::test]
    async fn test_patch_file_multiple_matches() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("file.txt"), "a a a").unwrap();

        let tool = PatchFileTool;
        let ctx = make_ctx(tmp.path());
        let result = tool
            .execute(
                json!({"path": "file.txt", "old_string": "a", "new_string": "b"}),
                &ctx,
            )
            .await;

        assert!(result.is_error);
        assert!(result.output.contains("Found"));
    }

    #[tokio::test]
    async fn test_patch_file_missing_args() {
        let tmp = TempDir::new().unwrap();
        let tool = PatchFileTool;
        let ctx = make_ctx(tmp.path());

        let result = tool.execute(json!({"path": "f.txt"}), &ctx).await;
        assert!(result.is_error);
        assert!(result.output.contains("old_string"));

        let result = tool.execute(json!({"old_string": "a", "new_string": "b"}), &ctx).await;
        assert!(result.is_error);
        assert!(result.output.contains("path"));
    }

    #[tokio::test]
    async fn test_patch_file_dry_run() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("file.txt"), "hello world").unwrap();

        let tool = PatchFileTool;
        let ctx = ToolContext {
            session_id: Some("test".to_string()),
            project_dir: Some(tmp.path().to_path_buf()),
            working_dir: Some(tmp.path().to_path_buf()),
            dry_run: true,
        };
        let result = tool
            .execute(
                json!({"path": "file.txt", "old_string": "world", "new_string": "there"}),
                &ctx,
            )
            .await;

        assert!(!result.is_error);
        assert!(result.output.contains("dry-run"));
        let content = fs::read_to_string(tmp.path().join("file.txt")).unwrap();
        assert_eq!(content, "hello world");
    }

    #[test]
    fn test_patch_file_metadata() {
        let tool = PatchFileTool;
        assert_eq!(tool.name(), "patch_file");
        assert!(matches!(
            tool.operation_level(),
            catcode_core::OperationLevel::Sensitive
        ));
    }

    #[test]
    fn test_patch_file_schema() {
        let tool = PatchFileTool;
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["path"].is_object());
        assert!(schema["properties"]["old_string"].is_object());
        assert!(schema["properties"]["new_string"].is_object());
    }
}
