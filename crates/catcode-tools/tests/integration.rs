//! Integration tests for the full ToolRegistry with all built-in tools.

use catcode_core::ToolContext;
use catcode_tools::ToolRegistry;
use serde_json::json;
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
    assert!(names.contains(&"patch_file".to_string()));
    assert!(names.contains(&"git_status".to_string()));
    assert!(names.contains(&"git_diff".to_string()));
    assert!(names.contains(&"git_commit".to_string()));
    assert!(names.contains(&"delete_file".to_string()));
    assert!(names.contains(&"web_fetch".to_string()));
    assert!(names.contains(&"code_analysis".to_string()));
    assert_eq!(names.len(), 13);
}

#[test]
fn test_registry_llm_schema_count() {
    let reg = ToolRegistry::with_builtins();
    let schemas = reg.to_llm_schema();
    assert_eq!(schemas.len(), 13);

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
        .dispatch(
            "bash",
            json!({"command": "cat src/main.rs | grep -c println"}),
            &ctx,
        )
        .await
        .unwrap();
    assert!(!bash_result.is_error);
    assert!(bash_result.output.contains("1"));
}

#[tokio::test]
async fn test_dispatch_nonexistent_tool() {
    let reg = ToolRegistry::with_builtins();
    let ctx = ToolContext::default();
    let result = reg.dispatch("nonexistent_tool", json!({}), &ctx).await;
    assert!(result.is_err());
}
