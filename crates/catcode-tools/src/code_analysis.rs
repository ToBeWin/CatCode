use async_trait::async_trait;
use catcode_core::{OperationLevel, Tool, ToolContext, ToolResult};
use serde_json::{Value, json};
use std::collections::HashMap;
use tokio::fs;

use crate::read_file::resolve_path;

/// Analyzes a source code file — counts lines, words, characters,
/// and detects function/class/struct/enum/trait definitions via pattern matching.
///
/// Parameters:
/// - `path` (string, required): File path to analyze.
/// - `language` (string, optional): Programming language hint. Auto-detected from extension.
pub struct CodeAnalysisTool;

#[async_trait]
impl Tool for CodeAnalysisTool {
    fn name(&self) -> &str {
        "code_analysis"
    }

    fn description(&self) -> &str {
        "Analyze a source code file. Returns line/word/char counts and detected definitions (functions, classes, structs, enums, traits)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to analyze. Relative paths resolve against the working directory."
                },
                "language": {
                    "type": "string",
                    "description": "Programming language hint (auto-detected from extension if not provided)."
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

        let content = match fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(e) => {
                return ToolResult::error(format!("Failed to read {}: {}", path.display(), e));
            }
        };

        let _language: Option<&str> = args
            .get("language")
            .and_then(|v| v.as_str());

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();
        let non_empty_lines = lines.iter().filter(|l| !l.trim().is_empty()).count();
        let code_lines = lines
            .iter()
            .filter(|l| {
                let t = l.trim();
                !t.is_empty() && !t.starts_with("//") && !t.starts_with('#')
                    && !t.starts_with("--") && !t.starts_with("/*")
                    && !t.starts_with('*')
            })
            .count();
        let word_count: usize = lines.iter().map(|l| l.split_whitespace().count()).sum();
        let char_count = content.chars().count();

        let definitions = extract_definitions(&lines);

        let result = json!({
            "path": path.to_string_lossy(),
            "language": detect_language(&path),
            "stats": {
                "total_lines": total_lines,
                "non_empty_lines": non_empty_lines,
                "code_lines": code_lines,
                "word_count": word_count,
                "char_count": char_count,
            },
            "definitions": definitions,
        });

        ToolResult::success(serde_json::to_string_pretty(&result).unwrap())
    }
}

fn detect_language(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => "rust",
        Some("py") => "python",
        Some("js") | Some("jsx") => "javascript",
        Some("ts") | Some("tsx") => "typescript",
        Some("go") => "go",
        Some("java") => "java",
        Some("rb") => "ruby",
        Some("c") | Some("h") => "c",
        Some("cpp") | Some("hpp") | Some("cc") | Some("cxx") => "cpp",
        Some("kt") | Some("kts") => "kotlin",
        Some("swift") => "swift",
        Some("scala") => "scala",
        Some("php") => "php",
        Some("r") => "r",
        Some("sh") | Some("bash") | Some("zsh") => "shell",
        Some("toml") => "toml",
        Some("yaml") | Some("yml") => "yaml",
        Some("json") => "json",
        Some("md") => "markdown",
        Some("html") | Some("htm") => "html",
        Some("css") => "css",
        Some("sql") => "sql",
        _ => "unknown",
    }
}

fn extract_definitions(lines: &[&str]) -> HashMap<String, Vec<String>> {
    let mut functions = Vec::new();
    let mut classes = Vec::new();
    let mut structs = Vec::new();
    let mut enums = Vec::new();
    let mut traits = Vec::new();
    let mut interfaces = Vec::new();
    let mut methods = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        // fn name(...
        if let Some(name) = capture_after_prefix(trimmed, "fn ")
            && !name.starts_with('_')
        {
            functions.push(format!("{} (line {})", name, i + 1));
        }

        // class Name
        if let Some(name) = capture_after_prefix(trimmed, "class ") {
            let name = name.split(|c: char| c.is_whitespace() || c == '{' || c == ':').next().unwrap_or(name);
            if !name.is_empty() {
                classes.push(format!("{} (line {})", name, i + 1));
            }
        }

        // struct Name
        if let Some(name) = capture_after_prefix(trimmed, "struct ") {
            let name = name.split(|c: char| c.is_whitespace() || c == '{' || c == ';').next().unwrap_or(name);
            if !name.is_empty() {
                structs.push(format!("{} (line {})", name, i + 1));
            }
        }

        // enum Name
        if let Some(name) = capture_after_prefix(trimmed, "enum ") {
            let name = name.split(|c: char| c.is_whitespace() || c == '{' || c == ';').next().unwrap_or(name);
            if !name.is_empty() {
                enums.push(format!("{} (line {})", name, i + 1));
            }
        }

        // trait Name
        if let Some(name) = capture_after_prefix(trimmed, "trait ") {
            let name = name.split(|c: char| c.is_whitespace() || c == '{' || c == ';').next().unwrap_or(name);
            if !name.is_empty() {
                traits.push(format!("{} (line {})", name, i + 1));
            }
        }

        // interface Name (Go, Java, TypeScript)
        if let Some(name) = capture_after_prefix(trimmed, "interface ") {
            let name = name.split(|c: char| c.is_whitespace() || c == '{' || c == ';').next().unwrap_or(name);
            if !name.is_empty() {
                interfaces.push(format!("{} (line {})", name, i + 1));
            }
        }

        // def name(  (Python, Ruby)
        if let Some(name) = capture_after_prefix(trimmed, "def ") {
            let name = name.split('(').next().unwrap_or(name);
            if !name.is_empty() && !name.starts_with('_') {
                functions.push(format!("{} (line {})", name, i + 1));
            }
        }

        // function name(  (PHP, JavaScript)
        if let Some(name) = capture_after_prefix(trimmed, "function ") {
            let name = name.split('(').next().unwrap_or(name);
            if !name.is_empty() {
                functions.push(format!("{} (line {})", name, i + 1));
            }
        }

        // defmodule / defimpl / defprotocol (Elixir)
        // fun name(  (Kotlin)  — but this overlaps with "fn " for Rust
        // proc name(  (Rust macros)
        if let Some(name) = capture_after_prefix(trimmed, "macro ") {
            let name = name.split('!').next().unwrap_or(name);
            if !name.is_empty() {
                functions.push(format!("{} (line {})", name, i + 1));
            }
        }

        // func Name (Go)
        if let Some(name) = capture_after_prefix(trimmed, "func ") {
            let name = if let Some(paren_idx) = name.find('(') {
                // Check for receiver: func (r *Receiver) Name(args)
                let after_paren = &name[paren_idx..];
                if let Some(close_paren) = after_paren.find(')') {
                    let after_receiver = after_paren[close_paren + 1..].trim();
                    if let Some(func_name) = after_receiver.split('(').next() {
                        let func_name = func_name.trim();
                        if !func_name.is_empty() {
                            methods.push(format!("{} (line {})", func_name, i + 1));
                            continue;
                        }
                    }
                }
                name.split('(').next().unwrap_or(name)
            } else {
                name
            };
            if !name.is_empty() {
                functions.push(format!("{} (line {})", name, i + 1));
            }
        }

        // sub name (Perl)
        if let Some(name) = capture_after_prefix(trimmed, "sub ") {
            let name = name.split(|c: char| c.is_whitespace() || c == '{').next().unwrap_or(name);
            if !name.is_empty() {
                functions.push(format!("{} (line {})", name, i + 1));
            }
        }
    }

    let mut result = HashMap::new();
    if !functions.is_empty() {
        result.insert("functions".to_string(), functions);
    }
    if !classes.is_empty() {
        result.insert("classes".to_string(), classes);
    }
    if !structs.is_empty() {
        result.insert("structs".to_string(), structs);
    }
    if !enums.is_empty() {
        result.insert("enums".to_string(), enums);
    }
    if !traits.is_empty() {
        result.insert("traits".to_string(), traits);
    }
    if !interfaces.is_empty() {
        result.insert("interfaces".to_string(), interfaces);
    }
    if !methods.is_empty() {
        result.insert("methods".to_string(), methods);
    }
    result
}

fn capture_after_prefix<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    if let Some(rest) = line.strip_prefix(prefix) {
        let rest = rest.trim();
        if rest.is_empty() {
            return None;
        }
        // Take up to the first paren, brace, angle bracket, colon (for generics), semicolon, or space
        let name = rest.split(['(', '{', ';', '<'])
            .next()
            .unwrap_or(rest)
            .trim();
        if name.is_empty() {
            None
        } else {
            Some(name)
        }
    } else {
        None
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
    async fn test_code_analysis_basic() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("main.rs"),
            "fn main() {\n    println!(\"hello\");\n}\n",
        )
        .unwrap();

        let tool = CodeAnalysisTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"path": "main.rs"}), &ctx).await;

        assert!(!result.is_error);
        let output = &result.output;
        assert!(output.contains("total_lines"));
        assert!(output.contains("word_count"));
        assert!(output.contains("char_count"));
    }

    #[tokio::test]
    async fn test_code_analysis_empty_file() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("empty.rs"), "").unwrap();

        let tool = CodeAnalysisTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"path": "empty.rs"}), &ctx).await;

        assert!(!result.is_error);
        let output = &result.output;
        assert!(output.contains("\"total_lines\": 0"));
    }

    #[tokio::test]
    async fn test_code_analysis_detects_functions() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("lib.rs"),
            "fn hello() {}\nfn add(a: i32, b: i32) -> i32 { a + b }\n",
        )
        .unwrap();

        let tool = CodeAnalysisTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"path": "lib.rs"}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.output.contains("hello"));
        assert!(result.output.contains("add"));
    }

    #[tokio::test]
    async fn test_code_analysis_detects_structs() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("types.rs"),
            "struct Point { x: i32, y: i32 }\nenum Color { Red, Green, Blue }\ntrait Draw { fn draw(&self); }\n",
        )
        .unwrap();

        let tool = CodeAnalysisTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"path": "types.rs"}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.output.contains("Point"));
        assert!(result.output.contains("Color"));
        assert!(result.output.contains("Draw"));
    }

    #[tokio::test]
    async fn test_code_analysis_file_not_found() {
        let tmp = TempDir::new().unwrap();
        let tool = CodeAnalysisTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"path": "nonexistent.rs"}), &ctx).await;

        assert!(result.is_error);
    }

    #[tokio::test]
    async fn test_code_analysis_missing_path() {
        let tmp = TempDir::new().unwrap();
        let tool = CodeAnalysisTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({}), &ctx).await;

        assert!(result.is_error);
        assert!(result.output.contains("path"));
    }

    #[tokio::test]
    async fn test_code_analysis_language_detection() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("script.py"), "def hello():\n    pass\n").unwrap();

        let tool = CodeAnalysisTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"path": "script.py"}), &ctx).await;

        assert!(!result.is_error);
        // Should detect language from extension
        assert!(result.output.contains("python"));
    }

    #[test]
    fn test_code_analysis_metadata() {
        let tool = CodeAnalysisTool;
        assert_eq!(tool.name(), "code_analysis");
        assert!(matches!(
            tool.operation_level(),
            catcode_core::OperationLevel::Safe
        ));
    }

    #[test]
    fn test_code_analysis_schema() {
        let tool = CodeAnalysisTool;
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["path"].is_object());
        assert!(schema["properties"]["language"].is_object());
    }

    #[test]
    fn test_detect_language() {
        assert_eq!(detect_language(std::path::Path::new("foo.rs")), "rust");
        assert_eq!(detect_language(std::path::Path::new("foo.py")), "python");
        assert_eq!(detect_language(std::path::Path::new("foo.js")), "javascript");
        assert_eq!(detect_language(std::path::Path::new("foo.ts")), "typescript");
        assert_eq!(detect_language(std::path::Path::new("foo.go")), "go");
        assert_eq!(detect_language(std::path::Path::new("foo.java")), "java");
        assert_eq!(detect_language(std::path::Path::new("foo.rb")), "ruby");
        assert_eq!(detect_language(std::path::Path::new("foo.unknown")), "unknown");
    }

    #[test]
    fn test_capture_after_prefix() {
        assert_eq!(capture_after_prefix("fn main()", "fn "), Some("main"));
        assert_eq!(
            capture_after_prefix("fn add(a: i32, b: i32) -> i32", "fn "),
            Some("add")
        );
        assert_eq!(
            capture_after_prefix("struct Point {", "struct "),
            Some("Point")
        );
        assert_eq!(
            capture_after_prefix("enum Color {", "enum "),
            Some("Color")
        );
        assert_eq!(capture_after_prefix("fn ", "fn "), None);
        assert_eq!(capture_after_prefix("not a match", "fn "), None);
    }
}
