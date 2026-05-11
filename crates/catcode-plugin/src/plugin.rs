use async_trait::async_trait;
use catcode_core::provider::Provider;
use catcode_core::types::ToolDefinition;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

/// Metadata about a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub author: Option<String>,
}

/// Context provided to plugin hooks.
pub struct PluginContext {
    pub session_id: Option<String>,
    pub project_dir: Option<String>,
    pub metadata: HashMap<String, String>,
}

/// A Plugin is a dynamically-loadable extension that can register new tools,
/// providers, or modify harness behavior.
#[async_trait]
pub trait Plugin: Send + Sync {
    /// Return plugin metadata.
    fn metadata(&self) -> PluginMetadata;

    /// Called when the plugin is loaded. Register tools, providers, etc.
    fn on_load(&self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Called when the plugin is unloaded. Clean up resources.
    fn on_unload(&self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Tools provided by this plugin.
    fn tools(&self) -> Vec<PluginTool> {
        vec![]
    }

    /// Provider provided by this plugin (if any).
    fn provider(&self) -> Option<Arc<dyn Provider>> {
        None
    }
}

/// A tool registered by a plugin.
#[derive(Clone)]
pub struct PluginTool {
    pub definition: ToolDefinition,
    pub handler: Arc<dyn PluginToolHandler>,
}

impl fmt::Debug for PluginTool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PluginTool")
            .field("definition", &self.definition)
            .field("handler", &"<dyn PluginToolHandler>")
            .finish()
    }
}

/// Handler for executing a plugin tool.
#[async_trait]
pub trait PluginToolHandler: Send + Sync {
    async fn execute(&self, args: serde_json::Value, ctx: &PluginContext) -> PluginToolResult;
}

/// Result of a plugin tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginToolResult {
    pub output: String,
    pub is_error: bool,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

impl PluginToolResult {
    pub fn success(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            is_error: false,
            metadata: HashMap::new(),
        }
    }

    pub fn error(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            is_error: true,
            metadata: HashMap::new(),
        }
    }
}

/// Errors related to plugin management.
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("Plugin not found: {0}")]
    NotFound(String),

    #[error("Plugin already registered: {0}")]
    AlreadyRegistered(String),

    #[error("Plugin load failed: {0}")]
    LoadFailed(String),

    #[error("Plugin tool execution failed: {0}")]
    ToolFailed(String),
}

/// Registry for managing loaded plugins.
pub struct PluginRegistry {
    plugins: Vec<Arc<dyn Plugin>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }

    /// Register a plugin.
    pub fn register(&mut self, plugin: Arc<dyn Plugin>) -> Result<(), PluginError> {
        let meta = plugin.metadata();
        if self.plugins.iter().any(|p| p.metadata().id == meta.id) {
            return Err(PluginError::AlreadyRegistered(meta.id));
        }

        plugin.on_load().map_err(|e| {
            PluginError::LoadFailed(format!("Failed to load plugin '{}': {e}", meta.id))
        })?;

        self.plugins.push(plugin);
        Ok(())
    }

    /// Unregister a plugin by id.
    pub fn unregister(&mut self, id: &str) -> Result<(), PluginError> {
        let pos = self
            .plugins
            .iter()
            .position(|p| p.metadata().id == id)
            .ok_or_else(|| PluginError::NotFound(id.to_string()))?;

        let plugin = self.plugins.remove(pos);
        plugin.on_unload().map_err(|e| {
            PluginError::LoadFailed(format!("Failed to unload plugin '{id}': {e}"))
        })?;

        Ok(())
    }

    /// List all registered plugin metadata.
    pub fn list(&self) -> Vec<PluginMetadata> {
        self.plugins.iter().map(|p| p.metadata()).collect()
    }

    /// Get a plugin by id.
    pub fn get(&self, id: &str) -> Option<&Arc<dyn Plugin>> {
        self.plugins.iter().find(|p| p.metadata().id == id)
    }

    /// Collect all tools from all registered plugins.
    pub fn all_tools(&self) -> Vec<PluginTool> {
        self.plugins.iter().flat_map(|p| p.tools()).collect()
    }

    /// Collect all providers from all registered plugins.
    pub fn all_providers(&self) -> Vec<Arc<dyn Provider>> {
        self.plugins.iter().filter_map(|p| p.provider()).collect()
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::error::ProviderError;
    use catcode_core::provider::{ModelInfo, ProviderCapabilities, ProviderContext, TokenCounter};
    use catcode_core::types::{ChatRequest, ChatResponse};

    // === Test plugin implementation ===

    struct TestPlugin {
        id: String,
        tools: Vec<PluginTool>,
    }

    impl TestPlugin {
        fn new(id: &str) -> Self {
            Self {
                id: id.to_string(),
                tools: vec![],
            }
        }

        fn with_tool(mut self, name: &str) -> Self {
            self.tools.push(PluginTool {
                definition: ToolDefinition {
                    name: name.to_string(),
                    description: format!("Tool {name}"),
                    parameters: serde_json::json!({"type": "object"}),
                },
                handler: Arc::new(TestToolHandler),
            });
            self
        }
    }

    struct TestToolHandler;

    #[async_trait]
    impl PluginToolHandler for TestToolHandler {
        async fn execute(&self, _args: serde_json::Value, _ctx: &PluginContext) -> PluginToolResult {
            PluginToolResult::success("done")
        }
    }

    #[async_trait]
    impl Plugin for TestPlugin {
        fn metadata(&self) -> PluginMetadata {
            PluginMetadata {
                id: self.id.clone(),
                name: format!("Test Plugin {}", self.id),
                version: "0.1.0".to_string(),
                description: "A test plugin".to_string(),
                author: None,
            }
        }

        fn tools(&self) -> Vec<PluginTool> {
            self.tools.clone()
        }
    }

    // === Test provider plugin ===

    struct TestProviderPlugin;

    struct DummyProvider;

    #[async_trait]
    impl Provider for DummyProvider {
        fn id(&self) -> &str {
            "dummy"
        }
        fn display_name(&self) -> &str {
            "Dummy"
        }
        fn supported_models(&self) -> Vec<ModelInfo> {
            vec![]
        }
        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities::default()
        }
        async fn chat(
            &self,
            _request: ChatRequest,
            _ctx: &ProviderContext,
        ) -> Result<ChatResponse, ProviderError> {
            unimplemented!()
        }
        async fn health_check(&self) -> Result<(), ProviderError> {
            Ok(())
        }
        fn token_counter(&self) -> Box<dyn TokenCounter> {
            unimplemented!()
        }
    }

    #[async_trait]
    impl Plugin for TestProviderPlugin {
        fn metadata(&self) -> PluginMetadata {
            PluginMetadata {
                id: "provider-plugin".to_string(),
                name: "Provider Plugin".to_string(),
                version: "0.1.0".to_string(),
                description: "Provides a dummy provider".to_string(),
                author: None,
            }
        }

        fn provider(&self) -> Option<Arc<dyn Provider>> {
            Some(Arc::new(DummyProvider))
        }
    }

    // === Tests ===

    #[test]
    fn test_plugin_registry_register_and_list() {
        let mut registry = PluginRegistry::new();
        let plugin = Arc::new(TestPlugin::new("test1"));
        registry.register(plugin).unwrap();

        let list = registry.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "test1");
    }

    #[test]
    fn test_plugin_registry_duplicate_id() {
        let mut registry = PluginRegistry::new();
        registry
            .register(Arc::new(TestPlugin::new("dup")))
            .unwrap();
        let result = registry.register(Arc::new(TestPlugin::new("dup")));
        assert!(result.is_err());
        match result.unwrap_err() {
            PluginError::AlreadyRegistered(id) => assert_eq!(id, "dup"),
            _ => panic!("Expected AlreadyRegistered"),
        }
    }

    #[test]
    fn test_plugin_registry_unregister() {
        let mut registry = PluginRegistry::new();
        registry
            .register(Arc::new(TestPlugin::new("removable")))
            .unwrap();
        assert_eq!(registry.list().len(), 1);

        registry.unregister("removable").unwrap();
        assert_eq!(registry.list().len(), 0);
    }

    #[test]
    fn test_plugin_registry_unregister_not_found() {
        let mut registry = PluginRegistry::new();
        let result = registry.unregister("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_plugin_registry_get() {
        let mut registry = PluginRegistry::new();
        registry
            .register(Arc::new(TestPlugin::new("findme")))
            .unwrap();
        assert!(registry.get("findme").is_some());
        assert!(registry.get("nope").is_none());
    }

    #[test]
    fn test_plugin_all_tools() {
        let mut registry = PluginRegistry::new();
        registry
            .register(Arc::new(TestPlugin::new("p1").with_tool("tool_a")))
            .unwrap();
        registry
            .register(Arc::new(TestPlugin::new("p2").with_tool("tool_b")))
            .unwrap();

        let tools = registry.all_tools();
        assert_eq!(tools.len(), 2);
        let names: Vec<&str> = tools.iter().map(|t| t.definition.name.as_str()).collect();
        assert!(names.contains(&"tool_a"));
        assert!(names.contains(&"tool_b"));
    }

    #[test]
    fn test_plugin_all_providers() {
        let mut registry = PluginRegistry::new();
        registry
            .register(Arc::new(TestPlugin::new("no-provider")))
            .unwrap();
        registry
            .register(Arc::new(TestProviderPlugin))
            .unwrap();

        let providers = registry.all_providers();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id(), "dummy");
    }

    #[test]
    fn test_plugin_tool_result() {
        let success = PluginToolResult::success("output");
        assert!(!success.is_error);
        assert_eq!(success.output, "output");

        let error = PluginToolResult::error("failed");
        assert!(error.is_error);
        assert_eq!(error.output, "failed");
    }

    #[test]
    fn test_plugin_metadata_serialization() {
        let meta = PluginMetadata {
            id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            description: "A test plugin".to_string(),
            author: Some("Test Author".to_string()),
        };

        let json = serde_json::to_string(&meta).unwrap();
        assert!(json.contains("test"));
        assert!(json.contains("Test Author"));

        let deserialized: PluginMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "test");
    }

    #[tokio::test]
    async fn test_plugin_tool_handler_execute() {
        let handler = TestToolHandler;
        let ctx = PluginContext {
            session_id: None,
            project_dir: None,
            metadata: HashMap::new(),
        };
        let result = handler
            .execute(serde_json::json!({}), &ctx)
            .await;
        assert!(!result.is_error);
        assert_eq!(result.output, "done");
    }
}
