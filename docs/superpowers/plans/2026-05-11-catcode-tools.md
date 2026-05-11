# catcode-tools Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement ToolRegistry and 6 built-in tools (read_file, write_file, bash, search, glob, list_dir)

**Architecture:** Each tool implements the Tool trait from catcode-core. ToolRegistry manages discovery and dispatch.

**Tech Stack:** Rust, tokio, glob crate, grep-searcher or rg subprocess

---

## Prerequisites

catcode-core must be implemented first (see `2026-05-11-catcode-core.md`). The following types are imported from `catcode-core`:

```rust
// From catcode-core::tool
use catcode_core::{Tool, ToolResult, ToolContext, OperationLevel};

// From catcode-core::error
use catcode_core::ToolError;
```

---

## File Structure

```
crates/catcode-tools/
├── Cargo.toml
└── src/
    ├── lib.rs              # ToolRegistry + re-exports
    ├── read_file.rs        # Read file tool
    ├── write_file.rs       # Write file tool
    ├── bash.rs             # Shell command tool
    ├── search.rs           # Search files tool (ripgrep)
    ├── glob.rs             # Glob pattern matching
    └── list_dir.rs         # Directory listing
```

---

## Chunk 1: Project Setup + ToolRegistry

### Task 1: Initialize catcode-tools crate

**Files:**
- Modify: `Cargo.toml` (workspace root — add member)
- Create: `crates/catcode-tools/Cargo.toml`
- Create: `crates/catcode-tools/src/lib.rs`

- [ ] **Step 1: Add catcode-tools to workspace members**

Edit the workspace root `Cargo.toml` to add the new member:

```toml
[workspace]
resolver = "2"
members = [
    "crates/catcode-core",
    "crates/catcode-tools",
]
```

- [ ] **Step 2: Create catcode-tools Cargo.toml**

```toml
[package]
name = "catcode-tools"
version.workspace = true
edition.workspace = true

[dependencies]
catcode-core = { path = "../catcode-core" }
serde = { workspace = true }
serde_json = { workspace = true }
async-trait = { workspace = true }
tokio = { workspace = true }
glob = "0.3"
anyhow = { workspace = true }
tracing = "0.1"

[dev-dependencies]
tokio = { workspace = true, features = ["full", "test-util"] }
tempfile = "3"
```

- [ ] **Step 3: Create minimal lib.rs**

```rust
pub mod read_file;
pub mod write_file;
pub mod bash;
pub mod search;
pub mod glob;
pub mod list_dir;

mod registry;

pub use registry::ToolRegistry;
pub use read_file::ReadFileTool;
pub use write_file::WriteFileTool;
pub use bash::BashTool;
pub use search::SearchFilesTool;
pub use glob::GlobTool;
pub use list_dir::ListDirTool;
```

- [ ] **Step 4: Verify it compiles (will fail — modules missing)**

Run: `cargo check -p catcode-tools`
Expected: FAIL (module files not found)

- [ ] **Step 5: Create stub modules so it compiles**

Create each file with a placeholder. For each of `read_file.rs`, `write_file.rs`, `bash.rs`, `search.rs`, `glob.rs`, `list_dir.rs`:

```rust
// Temporary stub — will be replaced in later tasks
```

Create `registry.rs`:

```rust
// Temporary stub — will be replaced in Task 2
```

- [ ] **Step 6: Verify it compiles**

Run: `cargo check -p catcode-tools`
Expected: Compiles (with warnings about unused imports)

---

### Task 2: Implement ToolRegistry

**Files:**
- Create: `crates/catcode-tools/src/registry.rs`

- [ ] **Step 1: Write ToolRegistry tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::{OperationLevel, Tool, ToolContext, ToolResult};
    use async_trait::async_trait;
    use serde_json::json;

    // Mock tool for testing
    struct MockTool {
        tool_name: String,
    }

    #[async_trait]
    impl Tool for MockTool {
        fn name(&self) -> &str {
            &self.tool_name
        }
        fn description(&self) -> &str {
            "A mock tool for testing"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            json!({
                "type": "object",
                "properties": {},
                "required": []
            })
        }
        fn operation_level(&self) -> OperationLevel {
            OperationLevel::Safe
        }
        async fn execute(&self, _args: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
            ToolResult::success("mock result")
        }
    }

    #[test]
    fn test_registry_register_and_get() {
        let mut registry = ToolRegistry::new();
        let tool = Arc::new(MockTool { tool_name: "test_tool".to_string() });
        registry.register(tool);

        assert!(registry.get("test_tool").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_registry_list() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(MockTool { tool_name: "tool_a".to_string() }));
        registry.register(Arc::new(MockTool { tool_name: "tool_b".to_string() }));

        let list = registry.list();
        assert_eq!(list.len(), 2);
        let names: Vec<&str> = list.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"tool_a"));
        assert!(names.contains(&"tool_b"));
    }

    #[test]
    fn test_registry_to_llm_schema() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(MockTool { tool_name: "test_tool".to_string() }));

        let schemas = registry.to_llm_schema();
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0]["name"], "test_tool");
    }

    #[test]
    fn test_registry_get_nonexistent_returns_none() {
        let registry = ToolRegistry::new();
        assert!(registry.get("missing").is_none());
    }

    #[tokio::test]
    async fn test_registry_dispatch_success() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(MockTool { tool_name: "mock".to_string() }));

        let ctx = ToolContext::default();
        let result = registry.dispatch("mock", json!({}), &ctx).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().output, "mock result");
    }

    #[tokio::test]
    async fn test_registry_dispatch_not_found() {
        let registry = ToolRegistry::new();
        let ctx = ToolContext::default();
        let result = registry.dispatch("missing", json!({}), &ctx).await;
        assert!(result.is_err());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p catcode-tools --lib registry`
Expected: FAIL (module is a stub)

- [ ] **Step 3: Implement ToolRegistry**

```rust
use std::collections::HashMap;
use std::sync::Arc;

use catcode_core::{Tool, ToolContext, ToolResult};

/// Metadata about a registered tool, for listing purposes.
#[derive(Debug, Clone)]
pub struct ToolMeta {
    pub name: String,
    pub description: String,
    pub operation_level: catcode_core::OperationLevel,
}

/// Central registry for all available tools.
///
/// Tools are registered at startup and looked up by name when the LLM
/// requests a tool call. The registry also generates the JSON schema
/// array that gets sent to the model.
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool. If a tool with the same name already exists, it is replaced.
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    /// Look up a tool by name.
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    /// List metadata for all registered tools.
    pub fn list(&self) -> Vec<ToolMeta> {
        self.tools
            .values()
            .map(|t| ToolMeta {
                name: t.name().to_string(),
                description: t.description().to_string(),
                operation_level: t.operation_level(),
            })
            .collect()
    }

    /// Generate the JSON schema array for all registered tools,
    /// suitable for passing to an LLM's `tools` parameter.
    pub fn to_llm_schema(&self) -> Vec<serde_json::Value> {
        self.tools
            .values()
            .map(|t| {
                serde_json::json!({
                    "name": t.name(),
                    "description": t.description(),
                    "parameters": t.parameters_schema(),
                })
            })
            .collect()
    }

    /// Dispatch a tool call by name. Returns an error if the tool is not found.
    pub async fn dispatch(
        &self,
        name: &str,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, catcode_core::ToolError> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| catcode_core::ToolError::NotFound(name.to_string()))?;
        Ok(tool.execute(args, ctx).await)
    }

    /// Create a registry pre-populated with all 6 built-in tools.
    pub fn with_builtins() -> Self {
        let mut reg = Self::new();
        reg.register(Arc::new(crate::ReadFileTool));
        reg.register(Arc::new(crate::WriteFileTool));
        reg.register(Arc::new(crate::BashTool));
        reg.register(Arc::new(crate::SearchFilesTool));
        reg.register(Arc::new(crate::GlobTool));
        reg.register(Arc::new(crate::ListDirTool));
        reg
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p catcode-tools --lib registry`
Expected: All 6 tests pass

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/catcode-tools/
git commit -m "feat(tools): add ToolRegistry with dispatch and LLM schema generation"
```

---

## Chunk 2: read_file Tool

### Task 3: Implement read_file tool

**Files:**
- Create: `crates/catcode-tools/src/read_file.rs`

- [ ] **Step 1: Write read_file tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::{Tool, ToolContext, ToolResult};
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
        let result = tool.execute(json!({"path": "lines.txt", "offset": 1, "limit": 2}), &ctx).await;

        assert!(!result.is_error);
        // offset=1 means skip 1 line, limit=2 means show 2 lines
        assert_eq!(result.output, "line2\nline3\n");
    }

    #[tokio::test]
    async fn test_read_file_not_found() {
        let tmp = TempDir::new().unwrap();
        let tool = ReadFileTool;
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"path": "nonexistent.txt"}), &ctx).await;

        assert!(result.is_error);
        assert!(result.output.contains("not found") || result.output.contains("No such file"));
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
        let result = tool.execute(json!({"path": file_path.to_str().unwrap()}), &ctx).await;

        assert!(!result.is_error);
        assert_eq!(result.output, "absolute content");
    }

    #[test]
    fn test_read_file_metadata() {
        let tool = ReadFileTool;
        assert_eq!(tool.name(), "read_file");
        assert!(matches!(tool.operation_level(), catcode_core::OperationLevel::Safe));
    }

    #[test]
    fn test_read_file_schema() {
        let tool = ReadFileTool;
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["path"].is_object());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p catcode-tools --lib read_file`
Expected: FAIL (module is a stub)

- [ ] **Step 3: Implement read_file.rs**

```rust
use async_trait::async_trait;
use catcode_core::{OperationLevel, Tool, ToolContext, ToolResult};
use serde_json::{json, Value};
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
fn resolve_path(path_str: &str, ctx: &ToolContext) -> PathBuf {
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
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p catcode-tools --lib read_file`
Expected: All 7 tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/catcode-tools/src/read_file.rs
git commit -m "feat(tools): implement read_file with line range support"
```

---

## Chunk 3: write_file Tool

### Task 4: Implement write_file tool

**Files:**
- Create: `crates/catcode-tools/src/write_file.rs`

- [ ] **Step 1: Write write_file tests**

```rust
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
            .execute(json!({"path": "new_file.txt", "content": "hello world"}), &ctx)
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
            .execute(json!({"path": "existing.txt", "content": "new content"}), &ctx)
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
            .execute(json!({"path": "deep/nested/dir/file.txt", "content": "deep"}), &ctx)
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
            .execute(json!({"path": "dry.txt", "content": "should not write"}), &ctx)
            .await;

        assert!(!result.is_error);
        assert!(!tmp.path().join("dry.txt").exists());
    }

    #[test]
    fn test_write_file_metadata() {
        let tool = WriteFileTool;
        assert_eq!(tool.name(), "write_file");
        assert!(matches!(tool.operation_level(), catcode_core::OperationLevel::Sensitive));
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p catcode-tools --lib write_file`
Expected: FAIL (module is a stub)

- [ ] **Step 3: Implement write_file.rs**

```rust
use async_trait::async_trait;
use catcode_core::{OperationLevel, Tool, ToolContext, ToolResult};
use serde_json::{json, Value};
use std::path::PathBuf;
use tokio::fs;

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
        if let Some(parent) = path.parent() {
            if let Err(e) = fs::create_dir_all(parent).await {
                return ToolResult::error(format!(
                    "Failed to create parent directories for {}: {}",
                    path.display(),
                    e
                ));
            }
        }

        // Write file
        if let Err(e) = fs::write(&path, content).await {
            return ToolResult::error(format!("Failed to write {}: {}", path.display(), e));
        }

        ToolResult::success(format!("Wrote {} bytes to {}", content.len(), path.display()))
    }
}

fn resolve_path(path_str: &str, ctx: &ToolContext) -> PathBuf {
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
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p catcode-tools --lib write_file`
Expected: All 8 tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/catcode-tools/src/write_file.rs
git commit -m "feat(tools): implement write_file with parent dir creation and dry-run"
```

---

## Chunk 4: list_dir Tool

### Task 5: Implement list_dir tool

**Files:**
- Create: `crates/catcode-tools/src/list_dir.rs`

- [ ] **Step 1: Write list_dir tests**

```rust
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
        assert!(matches!(tool.operation_level(), catcode_core::OperationLevel::Safe));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p catcode-tools --lib list_dir`
Expected: FAIL (module is a stub)

- [ ] **Step 3: Implement list_dir.rs**

```rust
use async_trait::async_trait;
use catcode_core::{OperationLevel, Tool, ToolContext, ToolResult};
use serde_json::{json, Value};
use std::path::PathBuf;
use tokio::fs;

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
                return ToolResult::error(format!("Failed to read directory {}: {}", path.display(), e));
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

fn resolve_path(path_str: &str, ctx: &ToolContext) -> PathBuf {
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
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p catcode-tools --lib list_dir`
Expected: All 6 tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/catcode-tools/src/list_dir.rs
git commit -m "feat(tools): implement list_dir with directory entry sorting"
```

---

## Chunk 5: bash Tool

### Task 6: Implement bash tool

**Files:**
- Create: `crates/catcode-tools/src/bash.rs`

- [ ] **Step 1: Write bash tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::{Tool, ToolContext};
    use serde_json::json;
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
    async fn test_bash_simple_command() {
        let tmp = TempDir::new().unwrap();
        let tool = BashTool::new();
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"command": "echo hello"}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.output.contains("hello"));
    }

    #[tokio::test]
    async fn test_bash_exit_code_zero() {
        let tmp = TempDir::new().unwrap();
        let tool = BashTool::new();
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"command": "true"}), &ctx).await;

        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn test_bash_exit_code_nonzero() {
        let tmp = TempDir::new().unwrap();
        let tool = BashTool::new();
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"command": "false"}), &ctx).await;

        assert!(result.is_error);
        assert!(result.output.contains("exit code"));
    }

    #[tokio::test]
    async fn test_bash_stderr_captured() {
        let tmp = TempDir::new().unwrap();
        let tool = BashTool::new();
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"command": "echo error_msg >&2"}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.output.contains("error_msg"));
    }

    #[tokio::test]
    async fn test_bash_working_directory() {
        let tmp = TempDir::new().unwrap();
        let tool = BashTool::new();
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"command": "pwd"}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.output.contains(&tmp.path().to_string_lossy().to_string()));
    }

    #[tokio::test]
    async fn test_bash_missing_command() {
        let tmp = TempDir::new().unwrap();
        let tool = BashTool::new();
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({}), &ctx).await;

        assert!(result.is_error);
        assert!(result.output.contains("command"));
    }

    #[tokio::test]
    async fn test_bash_dry_run() {
        let tmp = TempDir::new().unwrap();
        let tool = BashTool::new();
        let ctx = ToolContext {
            session_id: Some("test".to_string()),
            project_dir: Some(tmp.path().to_path_buf()),
            working_dir: Some(tmp.path().to_path_buf()),
            dry_run: true,
        };
        let result = tool.execute(json!({"command": "rm -rf /"}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.output.contains("dry-run"));
    }

    #[tokio::test]
    async fn test_bash_command_with_args() {
        let tmp = TempDir::new().unwrap();
        let tool = BashTool::new();
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"command": "echo -n no-newline"}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.output.contains("no-newline"));
    }

    #[test]
    fn test_bash_metadata() {
        let tool = BashTool::new();
        assert_eq!(tool.name(), "bash");
        assert!(matches!(tool.operation_level(), catcode_core::OperationLevel::Dangerous));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p catcode-tools --lib bash`
Expected: FAIL (module is a stub)

- [ ] **Step 3: Implement bash.rs**

```rust
use async_trait::async_trait;
use catcode_core::{OperationLevel, Tool, ToolContext, ToolResult};
use serde_json::{json, Value};
use std::process::Command;

const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_OUTPUT_BYTES: usize = 1024 * 1024; // 1 MB

/// Executes a shell command via `/bin/sh -c`.
///
/// Parameters:
/// - `command` (string, required): The shell command to execute.
///
/// The command runs in the working directory from `ctx.working_dir`.
/// stdout and stderr are combined in the output.
pub struct BashTool {
    timeout_secs: u64,
}

impl BashTool {
    pub fn new() -> Self {
        Self {
            timeout_secs: DEFAULT_TIMEOUT_SECS,
        }
    }

    pub fn with_timeout(timeout_secs: u64) -> Self {
        Self { timeout_secs }
    }
}

impl Default for BashTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Execute a shell command. stdout and stderr are combined in the output. Dangerous commands require approval."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute."
                }
            },
            "required": ["command"]
        })
    }

    fn operation_level(&self) -> OperationLevel {
        OperationLevel::Dangerous
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        let command = match args.get("command").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => return ToolResult::error("Missing required argument: command"),
        };

        // Dry run
        if ctx.dry_run {
            return ToolResult::success(format!("[dry-run] Would execute: {}", command));
        }

        // Build command
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c").arg(command);

        // Set working directory
        if let Some(ref wd) = ctx.working_dir {
            cmd.current_dir(wd);
        } else if let Some(ref pd) = ctx.project_dir {
            cmd.current_dir(pd);
        }

        // Execute
        let output = match cmd.output() {
            Ok(o) => o,
            Err(e) => {
                return ToolResult::error(format!("Failed to execute command: {}", e));
            }
        };

        // Combine stdout + stderr
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let mut combined = String::new();

        if !stdout.is_empty() {
            combined.push_str(&stdout);
        }
        if !stderr.is_empty() {
            if !combined.is_empty() {
                combined.push_str("\n--- stderr ---\n");
            }
            combined.push_str(&stderr);
        }

        // Truncate if too large
        if combined.len() > MAX_OUTPUT_BYTES {
            let truncated = &combined[..MAX_OUTPUT_BYTES];
            combined = format!(
                "{}\n... (output truncated, {} bytes total)",
                truncated,
                combined.len()
            );
        }

        if combined.is_empty() {
            combined = "(no output)".to_string();
        }

        // Check exit code
        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            return ToolResult::error(format!(
                "Command failed with exit code {}:\n{}",
                code, combined
            ));
        }

        ToolResult::success(combined)
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p catcode-tools --lib bash`
Expected: All 9 tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/catcode-tools/src/bash.rs
git commit -m "feat(tools): implement bash with exit code handling and output truncation"
```

---

## Chunk 6: glob Tool

### Task 7: Implement glob tool

**Files:**
- Create: `crates/catcode-tools/src/glob.rs`

- [ ] **Step 1: Write glob tests**

```rust
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
        assert!(result.output.contains("no matches") || result.output.is_empty());
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
        assert!(matches!(tool.operation_level(), catcode_core::OperationLevel::Safe));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p catcode-tools --lib glob`
Expected: FAIL (module is a stub)

- [ ] **Step 3: Implement glob.rs**

```rust
use async_trait::async_trait;
use catcode_core::{OperationLevel, Tool, ToolContext, ToolResult};
use glob::glob;
use serde_json::{json, Value};
use std::path::PathBuf;

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
                            path.strip_prefix(wd).unwrap_or(&path).to_string_lossy().to_string()
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
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p catcode-tools --lib glob`
Expected: All 6 tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/catcode-tools/src/glob.rs
git commit -m "feat(tools): implement glob with relative path display"
```

---

## Chunk 7: search_files Tool

### Task 8: Implement search_files tool (ripgrep wrapper)

**Files:**
- Create: `crates/catcode-tools/src/search.rs`

- [ ] **Step 1: Write search tests**

```rust
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
        fs::write(tmp.path().join("file.txt"), "hello world\nfoo bar\nhello again").unwrap();

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
        assert!(result.output.contains("no matches") || result.output.is_empty());
    }

    #[tokio::test]
    async fn test_search_with_glob_filter() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("code.rs"), "fn main() {}").unwrap();
        fs::write(tmp.path().join("docs.txt"), "fn main() function").unwrap();

        let tool = SearchFilesTool::new();
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"pattern": "fn main", "glob": "*.rs"}), &ctx).await;

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
        let result = tool.execute(json!({"pattern": "hello", "case_insensitive": true}), &ctx).await;

        assert!(!result.is_error);
        // Should match both lines
        let match_count = result.output.lines().filter(|l| l.contains("hello") || l.contains("Hello")).count();
        assert!(match_count >= 2, "Expected at least 2 matches, got: {}", result.output);
    }

    #[test]
    fn test_search_metadata() {
        let tool = SearchFilesTool::new();
        assert_eq!(tool.name(), "search_files");
        assert!(matches!(tool.operation_level(), catcode_core::OperationLevel::Safe));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p catcode-tools --lib search`
Expected: FAIL (module is a stub)

- [ ] **Step 3: Implement search.rs**

```rust
use async_trait::async_trait;
use catcode_core::{OperationLevel, Tool, ToolContext, ToolResult};
use serde_json::{json, Value};
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

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        let pattern = match args.get("pattern").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return ToolResult::error("Missing required argument: pattern"),
        };

        // Check if rg is available
        if Command::new("rg").arg("--version").output().is_err() {
            return ToolResult::error(
                "ripgrep (rg) is not installed. Install it with: brew install ripgrep / apt install ripgrep".to_string()
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
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p catcode-tools --lib search`
Expected: All 6 tests pass (requires `rg` installed on system)

- [ ] **Step 5: Commit**

```bash
git add crates/catcode-tools/src/search.rs
git commit -m "feat(tools): implement search_files wrapping ripgrep"
```

---

## Chunk 8: Integration + Final Wiring

### Task 9: Update lib.rs and verify full crate

**Files:**
- Modify: `crates/catcode-tools/src/lib.rs`

- [ ] **Step 1: Verify lib.rs is complete**

Ensure `crates/catcode-tools/src/lib.rs` contains:

```rust
//! catcode-tools: Built-in tools for the CatCode AI agent.
//!
//! Each tool implements the `catcode_core::Tool` trait. The `ToolRegistry`
//! manages tool discovery, dispatch, and LLM schema generation.

pub mod bash;
pub mod glob;
pub mod list_dir;
pub mod read_file;
pub mod search;
pub mod write_file;

mod registry;

pub use bash::BashTool;
pub use glob::GlobTool;
pub use list_dir::ListDirTool;
pub use read_file::ReadFileTool;
pub use registry::ToolRegistry;
pub use search::SearchFilesTool;
pub use write_file::WriteFileTool;
```

- [ ] **Step 2: Run full test suite**

Run: `cargo test -p catcode-tools`
Expected: All tests across all modules pass

- [ ] **Step 3: Run clippy**

Run: `cargo clippy -p catcode-tools -- -D warnings`
Expected: Zero warnings

- [ ] **Step 4: Run fmt**

Run: `cargo fmt -p catcode-tools`
Expected: Formats without changes (already formatted)

- [ ] **Step 5: Commit**

```bash
git add crates/catcode-tools/
git commit -m "feat(tools): complete catcode-tools crate with 6 built-in tools"
```

---

### Task 10: Integration test — ToolRegistry with all builtins

**Files:**
- Create: `crates/catcode-tools/tests/integration.rs`

- [ ] **Step 1: Write integration test**

```rust
//! Integration tests for the full ToolRegistry with all built-in tools.

use catcode_core::{Tool, ToolContext};
use catcode_tools::ToolRegistry;
use serde_json::json;
use std::fs;
use tempfile::TempDir;

fn make_ctx(project_dir: &std::path::Path) -> ToolContext {
    ToolContext {
        session_id: Some("integration-test".to_string()),
        project_dir: Some(project_dir.to_path_buf()),
        working_dir: Some(project_dir.to_path_buf()),
        dry_run: false,
    }
}

#[test]
fn test_registry_has_all_builtins() {
    let reg = ToolRegistry::with_builtins();
    let names: Vec<String> = reg.list().iter().map(|m| m.name.clone()).collect();

    assert!(names.contains(&"read_file".to_string()));
    assert!(names.contains(&"write_file".to_string()));
    assert!(names.contains(&"bash".to_string()));
    assert!(names.contains(&"search_files".to_string()));
    assert!(names.contains(&"glob".to_string()));
    assert!(names.contains(&"list_dir".to_string()));
    assert_eq!(names.len(), 6);
}

#[test]
fn test_registry_llm_schema_count() {
    let reg = ToolRegistry::with_builtins();
    let schemas = reg.to_llm_schema();
    assert_eq!(schemas.len(), 6);

    // Each schema should have name, description, parameters
    for schema in &schemas {
        assert!(schema["name"].is_string());
        assert!(schema["description"].is_string());
        assert!(schema["parameters"].is_object());
    }
}

#[tokio::test]
async fn test_full_workflow_write_read_search() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx(tmp.path());
    let reg = ToolRegistry::with_builtins();

    // 1. Write a file
    let write_result = reg
        .dispatch(
            "write_file",
            json!({"path": "src/main.rs", "content": "fn main() {\n    println!(\"hello\");\n}"}),
            &ctx,
        )
        .await
        .unwrap();
    assert!(!write_result.is_error);

    // 2. Read it back
    let read_result = reg
        .dispatch("read_file", json!({"path": "src/main.rs"}), &ctx)
        .await
        .unwrap();
    assert!(!read_result.is_error);
    assert!(read_result.output.contains("fn main"));

    // 3. Search for content
    let search_result = reg
        .dispatch("search_files", json!({"pattern": "println"}), &ctx)
        .await
        .unwrap();
    assert!(!search_result.is_error);
    assert!(search_result.output.contains("println"));

    // 4. Glob for the file
    let glob_result = reg
        .dispatch("glob", json!({"pattern": "src/**/*.rs"}), &ctx)
        .await
        .unwrap();
    assert!(!glob_result.is_error);
    assert!(glob_result.output.contains("main.rs"));

    // 5. List the directory
    let list_result = reg
        .dispatch("list_dir", json!({"path": "src"}), &ctx)
        .await
        .unwrap();
    assert!(!list_result.is_error);
    assert!(list_result.output.contains("main.rs"));

    // 6. Run a bash command
    let bash_result = reg
        .dispatch("bash", json!({"command": "cat src/main.rs | wc -l"}), &ctx)
        .await
        .unwrap();
    assert!(!bash_result.is_error);
    assert!(bash_result.output.contains("3"));
}

#[tokio::test]
async fn test_dispatch_nonexistent_tool() {
    let reg = ToolRegistry::with_builtins();
    let ctx = ToolContext::default();
    let result = reg.dispatch("nonexistent_tool", json!({}), &ctx).await;
    assert!(result.is_err());
}
```

- [ ] **Step 2: Run integration tests**

Run: `cargo test -p catcode-tools --test integration`
Expected: All 4 tests pass

- [ ] **Step 3: Run full test suite**

Run: `cargo test -p catcode-tools`
Expected: All unit + integration tests pass

- [ ] **Step 4: Commit**

```bash
git add crates/catcode-tools/tests/
git commit -m "test(tools): add integration tests for ToolRegistry with all builtins"
```

---

## Summary

| Task | Files | Tests | Time |
|---|---|---|---|
| 1. Project setup | Cargo.toml, lib.rs, stubs | 0 | ~3 min |
| 2. ToolRegistry | registry.rs | 6 | ~5 min |
| 3. read_file | read_file.rs | 7 | ~4 min |
| 4. write_file | write_file.rs | 8 | ~4 min |
| 5. list_dir | list_dir.rs | 6 | ~3 min |
| 6. bash | bash.rs | 9 | ~5 min |
| 7. glob | glob.rs | 6 | ~3 min |
| 8. search_files | search.rs | 6 | ~4 min |
| 9. Final wiring | lib.rs | 0 | ~2 min |
| 10. Integration tests | integration.rs | 4 | ~3 min |

**Total: ~36 minutes, 52 tests, 8 files**
