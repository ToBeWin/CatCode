use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Unique identifier for this server.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Command to launch the MCP server process.
    pub command: String,
    /// Arguments for the command.
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables.
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Whether this server is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// An MCP tool definition returned by a server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub input_schema: serde_json::Value,
}

/// An MCP resource returned by a server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResource {
    pub uri: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
}

/// Result of calling an MCP tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolResult {
    pub content: Vec<McpContentBlock>,
    #[serde(default)]
    pub is_error: bool,
}

/// Content block in an MCP response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum McpContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image {
        data: String,
        mime_type: String,
    },
}

/// Errors from MCP operations.
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("MCP server not found: {0}")]
    ServerNotFound(String),

    #[error("MCP server not running: {0}")]
    ServerNotRunning(String),

    #[error("MCP protocol error: {0}")]
    ProtocolError(String),

    #[error("MCP tool not found: {0}")]
    ToolNotFound(String),

    #[error("MCP connection failed: {0}")]
    ConnectionFailed(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Represents a connection to an MCP server.
///
/// In production, this would manage the subprocess and JSON-RPC communication.
/// For now, it stores the config and provides a placeholder interface.
pub struct McpConnection {
    config: McpServerConfig,
    connected: bool,
}

impl McpConnection {
    pub fn new(config: McpServerConfig) -> Self {
        Self {
            config,
            connected: false,
        }
    }

    /// Connect to the MCP server (launch the subprocess).
    pub async fn connect(&mut self) -> Result<(), McpError> {
        // Placeholder: in production, this would spawn the subprocess
        // and perform the JSON-RPC handshake
        tracing::info!(
            server = %self.config.name,
            command = %self.config.command,
            "Connecting to MCP server"
        );
        self.connected = true;
        Ok(())
    }

    /// Disconnect from the MCP server.
    pub async fn disconnect(&mut self) -> Result<(), McpError> {
        self.connected = false;
        tracing::info!(server = %self.config.name, "Disconnected from MCP server");
        Ok(())
    }

    /// Check if the connection is active.
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Get the server config.
    pub fn config(&self) -> &McpServerConfig {
        &self.config
    }

    /// List tools available on this server.
    pub async fn list_tools(&self) -> Result<Vec<McpTool>, McpError> {
        if !self.connected {
            return Err(McpError::ServerNotRunning(self.config.name.clone()));
        }
        // Placeholder: would send tools/list via JSON-RPC
        Ok(vec![])
    }

    /// List resources available on this server.
    pub async fn list_resources(&self) -> Result<Vec<McpResource>, McpError> {
        if !self.connected {
            return Err(McpError::ServerNotRunning(self.config.name.clone()));
        }
        // Placeholder: would send resources/list via JSON-RPC
        Ok(vec![])
    }

    /// Call a tool on this server.
    pub async fn call_tool(
        &self,
        name: &str,
        _args: serde_json::Value,
    ) -> Result<McpToolResult, McpError> {
        if !self.connected {
            return Err(McpError::ServerNotRunning(self.config.name.clone()));
        }
        // Placeholder: would send tools/call via JSON-RPC
        tracing::debug!(tool = name, "MCP tool call (placeholder)");
        Ok(McpToolResult {
            content: vec![McpContentBlock::Text {
                text: format!("MCP tool '{name}' called (placeholder)"),
            }],
            is_error: false,
        })
    }
}

/// Registry for managing MCP server connections.
pub struct McpRegistry {
    connections: Vec<McpConnection>,
}

impl McpRegistry {
    pub fn new() -> Self {
        Self {
            connections: Vec::new(),
        }
    }

    /// Add and connect to an MCP server.
    pub async fn add_server(&mut self, config: McpServerConfig) -> Result<(), McpError> {
        let mut conn = McpConnection::new(config);
        conn.connect().await?;
        self.connections.push(conn);
        Ok(())
    }

    /// Remove an MCP server by id.
    pub async fn remove_server(&mut self, id: &str) -> Result<(), McpError> {
        let pos = self
            .connections
            .iter()
            .position(|c| c.config().id == id)
            .ok_or_else(|| McpError::ServerNotFound(id.to_string()))?;

        let mut conn = self.connections.remove(pos);
        conn.disconnect().await?;
        Ok(())
    }

    /// List all connected servers.
    pub fn list_servers(&self) -> Vec<&McpServerConfig> {
        self.connections.iter().map(|c| c.config()).collect()
    }

    /// Get a connection by server id.
    pub fn get(&self, id: &str) -> Option<&McpConnection> {
        self.connections.iter().find(|c| c.config().id == id)
    }

    /// Collect all tools from all connected servers.
    pub async fn all_tools(&self) -> Result<Vec<(String, McpTool)>, McpError> {
        let mut all = Vec::new();
        for conn in &self.connections {
            let tools = conn.list_tools().await?;
            for tool in tools {
                all.push((conn.config().id.clone(), tool));
            }
        }
        Ok(all)
    }
}

impl Default for McpRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_config() -> McpServerConfig {
        McpServerConfig {
            id: "test-server".to_string(),
            name: "Test MCP Server".to_string(),
            command: "npx".to_string(),
            args: vec!["-y".to_string(), "some-mcp-server".to_string()],
            env: HashMap::new(),
            enabled: true,
        }
    }

    #[test]
    fn test_mcp_config_serialization() {
        let config = sample_config();
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("test-server"));
        assert!(json.contains("npx"));

        let deserialized: McpServerConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "test-server");
        assert!(deserialized.enabled);
    }

    #[test]
    fn test_mcp_config_with_env() {
        let mut env = HashMap::new();
        env.insert("GITHUB_TOKEN".to_string(), "secret".to_string());

        let config = McpServerConfig {
            id: "github".to_string(),
            name: "GitHub".to_string(),
            command: "npx".to_string(),
            args: vec![],
            env,
            enabled: true,
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("GITHUB_TOKEN"));
    }

    #[test]
    fn test_mcp_config_disabled_default() {
        let json = serde_json::json!({
            "id": "test",
            "name": "Test",
            "command": "echo"
        });
        let config: McpServerConfig = serde_json::from_value(json).unwrap();
        assert!(config.enabled); // default true
    }

    #[test]
    fn test_mcp_tool_serialization() {
        let tool = McpTool {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                }
            }),
        };

        let json = serde_json::to_string(&tool).unwrap();
        assert!(json.contains("read_file"));

        let deserialized: McpTool = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "read_file");
    }

    #[test]
    fn test_mcp_tool_result_text() {
        let result = McpToolResult {
            content: vec![McpContentBlock::Text {
                text: "file contents".to_string(),
            }],
            is_error: false,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("file contents"));
        assert!(json.contains("text"));
    }

    #[test]
    fn test_mcp_tool_result_error() {
        let result = McpToolResult {
            content: vec![McpContentBlock::Text {
                text: "not found".to_string(),
            }],
            is_error: true,
        };

        assert!(result.is_error);
    }

    #[test]
    fn test_mcp_content_block_image() {
        let block = McpContentBlock::Image {
            data: "base64data".to_string(),
            mime_type: "image/png".to_string(),
        };

        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("image"));
        assert!(json.contains("base64data"));
    }

    #[tokio::test]
    async fn test_mcp_connection_lifecycle() {
        let config = sample_config();
        let mut conn = McpConnection::new(config);

        assert!(!conn.is_connected());
        conn.connect().await.unwrap();
        assert!(conn.is_connected());

        let tools = conn.list_tools().await.unwrap();
        assert!(tools.is_empty());

        let result = conn
            .call_tool("test", serde_json::json!({}))
            .await
            .unwrap();
        assert!(!result.is_error);

        conn.disconnect().await.unwrap();
        assert!(!conn.is_connected());
    }

    #[tokio::test]
    async fn test_mcp_connection_not_connected_error() {
        let config = sample_config();
        let conn = McpConnection::new(config);

        let result = conn.list_tools().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            McpError::ServerNotRunning(_) => {}
            _ => panic!("Expected ServerNotRunning"),
        }
    }

    #[tokio::test]
    async fn test_mcp_registry_add_and_list() {
        let mut registry = McpRegistry::new();
        registry.add_server(sample_config()).await.unwrap();

        let servers = registry.list_servers();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].id, "test-server");
    }

    #[tokio::test]
    async fn test_mcp_registry_remove() {
        let mut registry = McpRegistry::new();
        registry.add_server(sample_config()).await.unwrap();
        assert_eq!(registry.list_servers().len(), 1);

        registry.remove_server("test-server").await.unwrap();
        assert_eq!(registry.list_servers().len(), 0);
    }

    #[tokio::test]
    async fn test_mcp_registry_remove_not_found() {
        let mut registry = McpRegistry::new();
        let result = registry.remove_server("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mcp_registry_get() {
        let mut registry = McpRegistry::new();
        registry.add_server(sample_config()).await.unwrap();

        assert!(registry.get("test-server").is_some());
        assert!(registry.get("nope").is_none());
    }

    #[tokio::test]
    async fn test_mcp_registry_all_tools() {
        let mut registry = McpRegistry::new();
        registry.add_server(sample_config()).await.unwrap();

        let tools = registry.all_tools().await.unwrap();
        // Placeholder returns empty tools
        assert!(tools.is_empty());
    }

    #[test]
    fn test_mcp_resource_serialization() {
        let resource = McpResource {
            uri: "file:///path/to/file".to_string(),
            name: "file".to_string(),
            description: Some("A file".to_string()),
            mime_type: Some("text/plain".to_string()),
        };

        let json = serde_json::to_string(&resource).unwrap();
        assert!(json.contains("file:///path/to/file"));
    }
}
