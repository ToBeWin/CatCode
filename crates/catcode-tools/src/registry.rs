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
        reg.register(Arc::new(crate::BashTool::new()));
        reg.register(Arc::new(crate::SearchFilesTool::new()));
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

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use catcode_core::{OperationLevel, Tool, ToolContext, ToolResult};
    use serde_json::json;
    use std::sync::Arc;

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
        let tool = Arc::new(MockTool {
            tool_name: "test_tool".to_string(),
        });
        registry.register(tool);

        assert!(registry.get("test_tool").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_registry_list() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(MockTool {
            tool_name: "tool_a".to_string(),
        }));
        registry.register(Arc::new(MockTool {
            tool_name: "tool_b".to_string(),
        }));

        let list = registry.list();
        assert_eq!(list.len(), 2);
        let names: Vec<&str> = list.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"tool_a"));
        assert!(names.contains(&"tool_b"));
    }

    #[test]
    fn test_registry_to_llm_schema() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(MockTool {
            tool_name: "test_tool".to_string(),
        }));

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
        registry.register(Arc::new(MockTool {
            tool_name: "mock".to_string(),
        }));

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
