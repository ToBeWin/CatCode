use async_trait::async_trait;
use catcode_core::{OperationLevel, Tool, ToolContext, ToolResult};
use serde_json::{Value, json};
use tokio::fs;

use crate::read_file::resolve_path;

/// Lists the contents of a directory.
///
/// Parameters:
/// - `path` (string, required): Directory path. Relative paths resolve against `ctx.working_dir`.
pub struct ListDirTool;

#[async_trait]
impl Tool for ListDirTool {
    fn name(&self) -> &str {
        "list_dir"
    }

    fn description(&self) -> &str {
        "List the contents of a directory. Directories are shown with a trailing slash."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the directory to list. Relative paths resolve against the working directory."
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
        let path_str = match args.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return ToolResult::error("Missing required argument: path"),
        };

        let path = resolve_path(path_str, ctx);

        // Verify it's a directory
        match fs::metadata(&path).await {
            Ok(meta) => {
                if !meta.is_dir() {
                    return ToolResult::error(format!("{} is not a directory", path.display()));
                }
            }
            Err(e) => {
                return ToolResult::error(format!("Failed to access {}: {}", path.display(), e));
            }
        }

        // Read directory entries
        let mut entries = match fs::read_dir(&path).await {
            Ok(e) => e,
            Err(e) => {
                return ToolResult::error(format!(
                    "Failed to read directory {}: {}",
                    path.display(),
                    e
                ));
            }
        };

        let mut items: Vec<String> = Vec::new();
        loop {
            match entries.next_entry().await {
                Ok(Some(entry)) => {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let file_type = entry.file_type().await;
                    let is_dir = file_type.map(|ft| ft.is_dir()).unwrap_or(false);
                    if is_dir {
                        items.push(format!("{}/", name));
                    } else {
                        items.push(name);
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    return ToolResult::error(format!("Error reading entry: {}", e));
                }
            }
        }

        items.sort();

        if items.is_empty() {
            ToolResult::success(format!("{} (empty directory)", path.display()))
        } else {
            let listing = items.join("\n");
            ToolResult::success(format!("{}:\n{}", path.display(), listing))
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
    async fn test_list_dir_basic() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("file_a.txt"), "a").unwrap();
        fs::write(tmp.path().join("file_b.txt"), "b").unwrap();
        fs::create_dir(tmp.path().join("subdir")).unwrap();

        let tool = ListDirTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"path": "."}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.output.contains("file_a.txt"));
        assert!(result.output.contains("file_b.txt"));
        assert!(result.output.contains("subdir/"));
    }

    #[tokio::test]
    async fn test_list_dir_empty() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("empty")).unwrap();

        let tool = ListDirTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"path": "empty"}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.output.contains("empty") || result.output.is_empty());
    }

    #[tokio::test]
    async fn test_list_dir_not_found() {
        let tmp = TempDir::new().unwrap();
        let tool = ListDirTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"path": "nonexistent"}), &ctx).await;

        assert!(result.is_error);
    }

    #[tokio::test]
    async fn test_list_dir_missing_path() {
        let tmp = TempDir::new().unwrap();
        let tool = ListDirTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({}), &ctx).await;

        assert!(result.is_error);
        assert!(result.output.contains("path"));
    }

    #[tokio::test]
    async fn test_list_dir_shows_file_vs_dir() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("file.txt"), "content").unwrap();
        fs::create_dir(tmp.path().join("mydir")).unwrap();

        let tool = ListDirTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"path": "."}), &ctx).await;

        assert!(!result.is_error);
        // Directories should have trailing /
        assert!(result.output.contains("mydir/"));
        // Files should not
        assert!(result.output.contains("file.txt"));
        assert!(!result.output.contains("file.txt/"));
    }

    #[test]
    fn test_list_dir_metadata() {
        let tool = ListDirTool;
        assert_eq!(tool.name(), "list_dir");
        assert!(matches!(
            tool.operation_level(),
            catcode_core::OperationLevel::Safe
        ));
    }
}
