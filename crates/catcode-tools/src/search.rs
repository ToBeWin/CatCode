use async_trait::async_trait;
use catcode_core::{OperationLevel, Tool, ToolContext, ToolResult};
use serde_json::{Value, json};
use std::process::Command;

const MAX_MATCHES: usize = 200;
const MAX_LINE_LENGTH: usize = 500;

/// Searches file contents using ripgrep (`rg`).
///
/// Parameters:
/// - `pattern` (string, required): Regex pattern to search for.
/// - `glob` (string, optional): File glob filter (e.g., "*.rs").
/// - `case_insensitive` (bool, optional): Case-insensitive search.
pub struct SearchFilesTool {
    max_matches: usize,
}

impl SearchFilesTool {
    pub fn new() -> Self {
        Self {
            max_matches: MAX_MATCHES,
        }
    }

    pub fn with_max_matches(max: usize) -> Self {
        Self { max_matches: max }
    }
}

impl Default for SearchFilesTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for SearchFilesTool {
    fn name(&self) -> &str {
        "search_files"
    }

    fn description(&self) -> &str {
        "Search file contents using ripgrep. Supports regex patterns, glob filters, and case-insensitive mode."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regex pattern to search for."
                },
                "glob": {
                    "type": "string",
                    "description": "File glob filter (e.g., '*.rs'). Searches all files if omitted."
                },
                "case_insensitive": {
                    "type": "boolean",
                    "description": "If true, search is case-insensitive. Default: false."
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

        // Check if rg is available
        if Command::new("rg").arg("--version").output().is_err() {
            return ToolResult::error(
                "ripgrep (rg) is not installed. Install it with: brew install ripgrep / apt install ripgrep"
                    .to_string(),
            );
        }

        // Build rg command
        let mut cmd = Command::new("rg");
        cmd.arg("--no-heading")
            .arg("--line-number")
            .arg("--max-count")
            .arg(self.max_matches.to_string());

        // Case insensitive
        if args
            .get("case_insensitive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            cmd.arg("--ignore-case");
        }

        // Glob filter
        if let Some(glob_pattern) = args.get("glob").and_then(|v| v.as_str()) {
            cmd.arg("--glob").arg(glob_pattern);
        }

        // Working directory
        if let Some(ref wd) = ctx.working_dir {
            cmd.current_dir(wd);
        } else if let Some(ref pd) = ctx.project_dir {
            cmd.current_dir(pd);
        }

        // Pattern and search path
        cmd.arg(pattern).arg(".");

        // Execute
        let output = match cmd.output() {
            Ok(o) => o,
            Err(e) => {
                return ToolResult::error(format!("Failed to run rg: {}", e));
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        // rg exits with code 1 when no matches found — that's not an error
        let exit_code = output.status.code().unwrap_or(-1);
        if exit_code > 1 {
            return ToolResult::error(format!("ripgrep failed (exit {}): {}", exit_code, stderr));
        }

        if stdout.trim().is_empty() {
            return ToolResult::success(format!("No matches found for '{}'", pattern));
        }

        // Truncate long lines
        let lines: Vec<&str> = stdout.lines().collect();
        let mut result_lines = Vec::new();
        for line in lines.iter().take(self.max_matches) {
            if line.len() > MAX_LINE_LENGTH {
                result_lines.push(format!("{}...", &line[..MAX_LINE_LENGTH]));
            } else {
                result_lines.push(line.to_string());
            }
        }

        if lines.len() > self.max_matches {
            result_lines.push(format!(
                "... (showing first {} of {} matches)",
                self.max_matches,
                lines.len()
            ));
        }

        ToolResult::success(result_lines.join("\n"))
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
    async fn test_search_basic_match() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("file.txt"),
            "hello world\nfoo bar\nhello again",
        )
        .unwrap();

        let tool = SearchFilesTool::new();
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"pattern": "hello"}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.output.contains("hello world"));
        assert!(result.output.contains("hello again"));
        assert!(!result.output.contains("foo bar"));
    }

    #[tokio::test]
    async fn test_search_no_matches() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("file.txt"), "nothing here").unwrap();

        let tool = SearchFilesTool::new();
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"pattern": "zzzzz"}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.output.contains("No matches found") || result.output.is_empty());
    }

    #[tokio::test]
    async fn test_search_with_glob_filter() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("code.rs"), "fn main() {}").unwrap();
        fs::write(tmp.path().join("docs.txt"), "fn main() function").unwrap();

        let tool = SearchFilesTool::new();
        let ctx = make_ctx(tmp.path());
        let result = tool
            .execute(json!({"pattern": "fn main", "glob": "*.rs"}), &ctx)
            .await;

        assert!(!result.is_error);
        assert!(result.output.contains("code.rs"));
    }

    #[tokio::test]
    async fn test_search_missing_pattern() {
        let tmp = TempDir::new().unwrap();
        let tool = SearchFilesTool::new();
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({}), &ctx).await;

        assert!(result.is_error);
        assert!(result.output.contains("pattern"));
    }

    #[tokio::test]
    async fn test_search_case_insensitive() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("file.txt"), "Hello WORLD\nhello world").unwrap();

        let tool = SearchFilesTool::new();
        let ctx = make_ctx(tmp.path());
        let result = tool
            .execute(json!({"pattern": "hello", "case_insensitive": true}), &ctx)
            .await;

        assert!(!result.is_error);
        // Should match both lines
        let match_count = result
            .output
            .lines()
            .filter(|l| l.contains("hello") || l.contains("Hello"))
            .count();
        assert!(
            match_count >= 2,
            "Expected at least 2 matches, got: {}",
            result.output
        );
    }

    #[test]
    fn test_search_metadata() {
        let tool = SearchFilesTool::new();
        assert_eq!(tool.name(), "search_files");
        assert!(matches!(
            tool.operation_level(),
            catcode_core::OperationLevel::Safe
        ));
    }
}
