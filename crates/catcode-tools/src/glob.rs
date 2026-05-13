use async_trait::async_trait;
use catcode_core::{OperationLevel, Tool, ToolContext, ToolResult};
use glob::glob;
use serde_json::{Value, json};

/// Finds files matching a glob pattern.
///
/// Parameters:
/// - `pattern` (string, required): Glob pattern (e.g., "*.rs", "src/**/*.rs").
pub struct GlobTool;

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        "Find files matching a glob pattern. Returns matching file paths."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern to match (e.g., '*.rs', 'src/**/*.rs')."
                }
            },
            "required": ["pattern"]
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
        let pattern = match args.get("pattern").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return ToolResult::error("Missing required argument: pattern"),
        };

        // Resolve pattern relative to working directory
        let full_pattern = if let Some(ref wd) = ctx.working_dir {
            let base = wd.to_string_lossy();
            if pattern.starts_with('/') {
                pattern.to_string()
            } else {
                format!("{}/{}", base, pattern)
            }
        } else {
            pattern.to_string()
        };

        // Execute glob
        let matches: Vec<String> = match glob(&full_pattern) {
            Ok(paths) => paths
                .filter_map(|entry| match entry {
                    Ok(path) => {
                        // Make path relative to working dir if possible
                        let display_path = if let Some(ref wd) = ctx.working_dir {
                            path.strip_prefix(wd)
                                .unwrap_or(&path)
                                .to_string_lossy()
                                .to_string()
                        } else {
                            path.to_string_lossy().to_string()
                        };
                        Some(display_path)
                    }
                    Err(_) => None,
                })
                .collect(),
            Err(e) => {
                return ToolResult::error(format!("Invalid glob pattern '{}': {}", pattern, e));
            }
        };

        if matches.is_empty() {
            ToolResult::success(format!("No files matched pattern '{}'", pattern))
        } else {
            let mut sorted = matches;
            sorted.sort();
            ToolResult::success(sorted.join("\n"))
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
    async fn test_glob_simple_pattern() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("file_a.rs"), "").unwrap();
        fs::write(tmp.path().join("file_b.rs"), "").unwrap();
        fs::write(tmp.path().join("file_c.txt"), "").unwrap();

        let tool = GlobTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"pattern": "*.rs"}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.output.contains("file_a.rs"));
        assert!(result.output.contains("file_b.rs"));
        assert!(!result.output.contains("file_c.txt"));
    }

    #[tokio::test]
    async fn test_glob_nested_pattern() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("src")).unwrap();
        fs::write(tmp.path().join("src").join("main.rs"), "").unwrap();
        fs::write(tmp.path().join("src").join("lib.rs"), "").unwrap();
        fs::write(tmp.path().join("README.md"), "").unwrap();

        let tool = GlobTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"pattern": "src/**/*.rs"}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.output.contains("main.rs"));
        assert!(result.output.contains("lib.rs"));
        assert!(!result.output.contains("README.md"));
    }

    #[tokio::test]
    async fn test_glob_no_matches() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("file.txt"), "").unwrap();

        let tool = GlobTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"pattern": "*.rs"}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.output.contains("No files matched") || result.output.is_empty());
    }

    #[tokio::test]
    async fn test_glob_missing_pattern() {
        let tmp = TempDir::new().unwrap();
        let tool = GlobTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({}), &ctx).await;

        assert!(result.is_error);
        assert!(result.output.contains("pattern"));
    }

    #[tokio::test]
    async fn test_glob_invalid_pattern() {
        let tmp = TempDir::new().unwrap();
        let tool = GlobTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"pattern": "[invalid"}), &ctx).await;

        assert!(result.is_error);
    }

    #[test]
    fn test_glob_metadata() {
        let tool = GlobTool;
        assert_eq!(tool.name(), "glob");
        assert!(matches!(
            tool.operation_level(),
            catcode_core::OperationLevel::Safe
        ));
    }
}
