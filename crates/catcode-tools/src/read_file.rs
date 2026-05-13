use async_trait::async_trait;
use catcode_core::{OperationLevel, Tool, ToolContext, ToolResult};
use serde_json::{Value, json};
use std::path::PathBuf;
use tokio::fs;

/// Reads a file from disk, optionally restricted to a line range.
///
/// Parameters:
/// - `path` (string, required): File path. Relative paths resolve against `ctx.working_dir`.
/// - `offset` (number, optional): Number of lines to skip from the start (0-based).
/// - `limit` (number, optional): Maximum number of lines to return.
pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read a file's contents. Supports optional offset and limit for reading specific line ranges."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to read. Relative paths resolve against the working directory."
                },
                "offset": {
                    "type": "integer",
                    "description": "Number of lines to skip from the start (0-based). Default: 0.",
                    "minimum": 0
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to return. Default: all lines.",
                    "minimum": 1
                }
            },
            "required": ["path"]
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
        // Extract path argument
        let path_str = match args.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return ToolResult::error("Missing required argument: path"),
        };

        // Resolve path
        let path = resolve_path(path_str, ctx);

        // Read file
        let content = match fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(e) => {
                return ToolResult::error(format!("Failed to read {}: {}", path.display(), e));
            }
        };

        // Apply line range if specified
        let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let limit = args.get("limit").and_then(|v| v.as_u64());

        let output = if offset > 0 || limit.is_some() {
            let lines: Vec<&str> = content.lines().collect();
            let sliced = &lines[offset..];
            let output_lines = if let Some(lim) = limit {
                let lim = lim as usize;
                &sliced[..lim.min(sliced.len())]
            } else {
                sliced
            };
            output_lines.join("\n")
        } else {
            content
        };

        ToolResult::success(output)
    }
}

/// Resolve a path string against the tool context.
/// Absolute paths are used as-is. Relative paths resolve against working_dir.
pub(crate) fn resolve_path(path_str: &str, ctx: &ToolContext) -> PathBuf {
    let path = PathBuf::from(path_str);
    if path.is_absolute() {
        path
    } else if let Some(ref wd) = ctx.working_dir {
        wd.join(path)
    } else if let Some(ref pd) = ctx.project_dir {
        pd.join(path)
    } else {
        path
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
    async fn test_read_file_full() {
        let tmp = TempDir::new().unwrap();
        let file_path = tmp.path().join("hello.txt");
        fs::write(&file_path, "hello world\nline 2\nline 3\n").unwrap();

        let tool = ReadFileTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"path": "hello.txt"}), &ctx).await;

        assert!(!result.is_error);
        assert_eq!(result.output, "hello world\nline 2\nline 3\n");
    }

    #[tokio::test]
    async fn test_read_file_with_line_range() {
        let tmp = TempDir::new().unwrap();
        let file_path = tmp.path().join("lines.txt");
        fs::write(&file_path, "line1\nline2\nline3\nline4\nline5\n").unwrap();

        let tool = ReadFileTool;
        let ctx = make_ctx(tmp.path());
        let result = tool
            .execute(json!({"path": "lines.txt", "offset": 1, "limit": 2}), &ctx)
            .await;

        assert!(!result.is_error);
        // offset=1 means skip 1 line, limit=2 means show 2 lines
        assert_eq!(result.output, "line2\nline3");
    }

    #[tokio::test]
    async fn test_read_file_not_found() {
        let tmp = TempDir::new().unwrap();
        let tool = ReadFileTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"path": "nonexistent.txt"}), &ctx).await;

        assert!(result.is_error);
        assert!(
            result.output.contains("not found")
                || result.output.contains("No such file")
                || result.output.contains("Failed to read")
        );
    }

    #[tokio::test]
    async fn test_read_file_missing_path_arg() {
        let tmp = TempDir::new().unwrap();
        let tool = ReadFileTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({}), &ctx).await;

        assert!(result.is_error);
        assert!(result.output.contains("path"));
    }

    #[tokio::test]
    async fn test_read_file_absolute_path() {
        let tmp = TempDir::new().unwrap();
        let file_path = tmp.path().join("abs.txt");
        fs::write(&file_path, "absolute content").unwrap();

        let tool = ReadFileTool;
        let ctx = make_ctx(tmp.path());
        let result = tool
            .execute(json!({"path": file_path.to_str().unwrap()}), &ctx)
            .await;

        assert!(!result.is_error);
        assert_eq!(result.output, "absolute content");
    }

    #[test]
    fn test_read_file_metadata() {
        let tool = ReadFileTool;
        assert_eq!(tool.name(), "read_file");
        assert!(matches!(
            tool.operation_level(),
            catcode_core::OperationLevel::Safe
        ));
    }

    #[test]
    fn test_read_file_schema() {
        let tool = ReadFileTool;
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["path"].is_object());
    }
}
