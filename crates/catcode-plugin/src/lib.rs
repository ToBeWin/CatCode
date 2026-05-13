//! # catcode-plugin
//!
//! Extension system for CatCode providing three tiers of extensibility:
//!
//! - **Skill**: TOML-defined configuration files (no code) that define prompt templates,
//!   tool preferences, context rules, and hooks for specific domains.
//!
//! - **Plugin**: Dynamic Rust/WASM libraries that can register new tools, providers,
//!   and modify harness behavior at runtime.
//!
//! - **MCP**: Model Context Protocol client for connecting to external MCP servers
//!   that provide tools and resources.

/// The `mcp` module.
pub mod mcp;
/// The `plugin` module.
pub mod plugin;
/// The `skill` module.
pub mod skill;
/// The `wasm_sandbox` module.
pub mod wasm_sandbox;

use std::path::Path;
use std::sync::Arc;

/// Unified extension manager that coordinates skills, plugins, and MCP servers.
pub struct ExtensionManager {
    skills: skill::SkillRegistry,
    plugins: plugin::PluginRegistry,
    mcp: mcp::McpRegistry,
}

impl ExtensionManager {
/// Create an empty extension manager.
    pub fn new() -> Self {
        Self {
            skills: skill::SkillRegistry::new(),
            plugins: plugin::PluginRegistry::new(),
            mcp: mcp::McpRegistry::new(),
        }
    }

    // === Skill management ===

    /// Load skills from a directory.
    pub fn load_skills(&mut self, dir: &Path) -> Result<usize, skill::SkillError> {
        self.skills.load_dir(dir)
    }

    /// Register a skill directly.
    pub fn register_skill(&mut self, skill: skill::Skill) {
        self.skills.register(skill);
    }

    /// Get the skill registry.
    pub fn skills(&self) -> &skill::SkillRegistry {
        &self.skills
    }

    // === Plugin management ===

    /// Register a plugin.
    pub fn register_plugin(
        &mut self,
        plugin: Arc<dyn plugin::Plugin>,
    ) -> Result<(), plugin::PluginError> {
        self.plugins.register(plugin)
    }

    /// Unregister a plugin.
    pub fn unregister_plugin(&mut self, id: &str) -> Result<(), plugin::PluginError> {
        self.plugins.unregister(id)
    }

    /// Get the plugin registry.
    pub fn plugins(&self) -> &plugin::PluginRegistry {
        &self.plugins
    }

    // === MCP management ===

    /// Add and connect to an MCP server.
    pub async fn add_mcp_server(&mut self, config: mcp::McpServerConfig) -> Result<(), mcp::McpError> {
        self.mcp.add_server(config).await
    }

    /// Remove an MCP server.
    pub async fn remove_mcp_server(&mut self, id: &str) -> Result<(), mcp::McpError> {
        self.mcp.remove_server(id).await
    }

    /// Get the MCP registry.
    pub fn mcp(&self) -> &mcp::McpRegistry {
        &self.mcp
    }

    // === Combined queries ===

    /// Build the combined system prompt suffix from all loaded skills.
    pub fn combined_system_suffix(&self) -> String {
        self.skills.combined_system_suffix()
    }

    /// Get all plugin-registered tools.
    pub fn plugin_tools(&self) -> Vec<plugin::PluginTool> {
        self.plugins.all_tools()
    }

    /// Get all plugin-registered providers.
    pub fn plugin_providers(&self) -> Vec<Arc<dyn catcode_core::provider::Provider>> {
        self.plugins.all_providers()
    }
}

impl Default for ExtensionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plugin::PluginMetadata;
    use async_trait::async_trait;
    use std::collections::HashMap;

    struct NoopPlugin {
        id: String,
    }

    #[async_trait]
    impl plugin::Plugin for NoopPlugin {
        fn metadata(&self) -> PluginMetadata {
            PluginMetadata {
                id: self.id.clone(),
                name: self.id.clone(),
                version: "0.1.0".to_string(),
                description: "noop".to_string(),
                author: None,
            }
        }
    }

    #[test]
    fn test_extension_manager_new() {
        let mgr = ExtensionManager::new();
        assert!(mgr.skills().list_names().is_empty());
        assert!(mgr.plugins().list().is_empty());
        assert!(mgr.mcp().list_servers().is_empty());
    }

    #[test]
    fn test_extension_manager_skills() {
        let mut mgr = ExtensionManager::new();
        let skill = skill::Skill {
            skill: skill::SkillMetadata {
                name: "test".to_string(),
                version: "1.0.0".to_string(),
                description: "test".to_string(),
            },
            rules: skill::SkillRules::default(),
            prompts: skill::SkillPrompts {
                system_suffix: "hello".to_string(),
            },
            context: skill::SkillContext::default(),
            hooks: skill::SkillHooks::default(),
        };
        mgr.register_skill(skill);
        assert_eq!(mgr.combined_system_suffix(), "hello");
    }

    #[test]
    fn test_extension_manager_plugins() {
        let mut mgr = ExtensionManager::new();
        let plugin = Arc::new(NoopPlugin {
            id: "test-plugin".to_string(),
        });
        mgr.register_plugin(plugin).unwrap();
        assert_eq!(mgr.plugins().list().len(), 1);

        mgr.unregister_plugin("test-plugin").unwrap();
        assert_eq!(mgr.plugins().list().len(), 0);
    }

    #[tokio::test]
    async fn test_extension_manager_mcp() {
        let mut mgr = ExtensionManager::new();
        let config = mcp::McpServerConfig {
            id: "test".to_string(),
            name: "Test".to_string(),
            command: "echo".to_string(),
            args: vec![],
            env: HashMap::new(),
            enabled: true,
        };
        mgr.add_mcp_server(config).await.unwrap();
        assert_eq!(mgr.mcp().list_servers().len(), 1);
    }

    #[test]
    fn test_extension_manager_default() {
        let mgr = ExtensionManager::default();
        assert!(mgr.skills().list_names().is_empty());
    }
}
