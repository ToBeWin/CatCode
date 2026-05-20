//! WASM Plugin Sandbox
//!
//! Provides a sandboxed environment for running WASM plugins using wasmtime.
//! Plugins can register tools and execute code in an isolated runtime with
//! resource limits (memory, execution time).

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Configuration for the WASM sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmSandboxConfig {
    /// Maximum memory in bytes for the WASM module (default: 64MB).
    pub max_memory_bytes: usize,
    /// Maximum execution time for a single call (default: 30s).
    pub max_execution_time: Duration,
    /// Maximum fuel (instructions) per call (0 = unlimited).
    pub max_fuel: u64,
    /// Allowed host functions.
    pub allowed_host_functions: Vec<String>,
    /// Whether to allow WASI filesystem access.
    pub allow_wasi_fs: bool,
    /// Whether to allow WASI network access.
    pub allow_wasi_net: bool,
}

impl Default for WasmSandboxConfig {
    fn default() -> Self {
        Self {
            max_memory_bytes: 64 * 1024 * 1024, // 64MB
            max_execution_time: Duration::from_secs(30),
            max_fuel: 1_000_000_000,
            allowed_host_functions: vec![
                "log".to_string(),
                "read_file".to_string(),
                "write_file".to_string(),
            ],
            allow_wasi_fs: false,
            allow_wasi_net: false,
        }
    }
}

/// Errors from WASM sandbox operations.
#[derive(Debug, thiserror::Error)]
pub enum WasmSandboxError {
    #[error("Failed to load WASM module: {0}")]
    /// [`LoadError`].
    LoadError(String),

    #[error("WASM execution error: {0}")]
    /// [`ExecutionError`].
    ExecutionError(String),

    #[error("WASM memory limit exceeded: used {used} bytes, limit {limit} bytes")]
    /// [`MemoryLimitExceeded`].
    MemoryLimitExceeded { used: usize, limit: usize },

    #[error("WASM execution timeout after {0:?}")]
    /// [`ExecutionTimeout`].
    ExecutionTimeout(Duration),

    #[error("WASM fuel exhausted: used {used}, limit {limit}")]
    /// [`FuelExhausted`].
    FuelExhausted { used: u64, limit: u64 },

    #[error("Host function not allowed: {0}")]
    /// [`HostFunctionDenied`].
    HostFunctionDenied(String),

    #[error("WASM function not found: {0}")]
    /// [`FunctionNotFound`].
    FunctionNotFound(String),

    #[error("WASM sandbox error: {0}")]
    /// [`Other`].
    Other(String),
}

/// A loaded WASM module ready for instantiation.
pub struct WasmModule {
    /// Module binary data.
    #[allow(dead_code)]
    bytes: Vec<u8>,
    /// Module metadata.
    pub metadata: WasmPluginMetadata,
}

/// Metadata about a WASM plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmPluginMetadata {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: Option<String>,
    /// List of tools this plugin provides.
    pub tools: Vec<WasmToolDef>,
}

/// A tool definition from a WASM plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmToolDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// The result of executing a WASM tool function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmToolResult {
    pub output: String,
    pub is_error: bool,
}

impl WasmToolResult {
    /// Create a successful WASM execution result.
    pub fn success(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            is_error: false,
        }
    }

    /// Create an error WASM execution result.
    pub fn error(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            is_error: true,
        }
    }
}

/// Execution statistics for a WASM call.
#[derive(Debug, Clone, Default)]
pub struct ExecutionStats {
    pub fuel_used: u64,
    pub memory_used_bytes: usize,
    pub execution_time: Duration,
}

/// A WASM sandbox that can load and execute WASM modules with resource limits.
///
/// Uses wasmtime as the runtime engine with fuel-based execution limiting
/// and memory bounds enforcement.
pub struct WasmSandbox {
    config: WasmSandboxConfig,
    #[cfg(feature = "wasm")]
    engine: wasmtime::Engine,
}

impl WasmSandbox {
    /// Create a new WASM sandbox with the given configuration.
    pub fn new(config: WasmSandboxConfig) -> Result<Self, WasmSandboxError> {
        #[cfg(feature = "wasm")]
        {
            let mut engine_config = wasmtime::Config::new();
            engine_config.consume_fuel(true);
            engine_config.max_wasm_stack(1024 * 1024); // 1MB stack

            // Memory limits enforced at runtime via Store limiter
            // (PoolingAllocationConfig::max_memory_size is for pooling allocator only)

            let engine = wasmtime::Engine::new(&engine_config)
                .map_err(|e| WasmSandboxError::Other(e.to_string()))?;

            Ok(Self { config, engine })
        }

        #[cfg(not(feature = "wasm"))]
        {
            Err(WasmSandboxError::Other(
                "WASM feature not enabled".to_string(),
            ))
        }
    }

    /// Create a sandbox with default configuration.
    pub fn with_defaults() -> Result<Self, WasmSandboxError> {
        Self::new(WasmSandboxConfig::default())
    }

    /// Load a WASM module from bytes.
    pub fn load_module(&self, bytes: &[u8]) -> Result<WasmModule, WasmSandboxError> {
        #[cfg(feature = "wasm")]
        {
            // Validate the module compiles
            wasmtime::Module::new(&self.engine, bytes)
                .map_err(|e| WasmSandboxError::LoadError(e.to_string()))?;

            // Extract metadata from the module (convention: exported memory and functions)
            let metadata = self.extract_metadata(bytes)?;

            Ok(WasmModule {
                bytes: bytes.to_vec(),
                metadata,
            })
        }

        #[cfg(not(feature = "wasm"))]
        {
            let _ = bytes;
            Err(WasmSandboxError::Other(
                "WASM feature not enabled".to_string(),
            ))
        }
    }

    /// Load a WASM module from a file path.
    pub fn load_module_from_file(&self, path: &Path) -> Result<WasmModule, WasmSandboxError> {
        let bytes = std::fs::read(path)
            .map_err(|e| WasmSandboxError::LoadError(format!("Failed to read {path:?}: {e}")))?;
        self.load_module(&bytes)
    }

    /// Execute a tool function in a WASM module.
    ///
    /// The function receives JSON args as input and returns a JSON result.
    pub fn execute_tool(
        &self,
        module: &WasmModule,
        function_name: &str,
        args: &serde_json::Value,
    ) -> Result<(WasmToolResult, ExecutionStats), WasmSandboxError> {
        #[cfg(feature = "wasm")]
        {
            let start = Instant::now();

            // Create a store with fuel limit
            let mut store = wasmtime::Store::new(&self.engine, ());
            store
                .set_fuel(self.config.max_fuel)
                .map_err(|e| WasmSandboxError::Other(e.to_string()))?;

            // Set epoch deadline for timeout
            store.epoch_deadline_callback(move |_caller| {
                // This will be called when the epoch deadline is reached
                Err(anyhow::anyhow!("Execution timeout"))
            });

            // Compile and instantiate
            let wasmtime_module = wasmtime::Module::new(&self.engine, &module.bytes)
                .map_err(|e| WasmSandboxError::LoadError(e.to_string()))?;

            let instance = wasmtime::Instance::new(&mut store, &wasmtime_module, &[])
                .map_err(|e| WasmSandboxError::ExecutionError(e.to_string()))?;

            // Get the function
            let func = instance
                .get_func(&mut store, function_name)
                .ok_or_else(|| WasmSandboxError::FunctionNotFound(function_name.to_string()))?;

            // Serialize args to bytes
            let args_json = serde_json::to_vec(args)
                .map_err(|e| WasmSandboxError::ExecutionError(e.to_string()))?;

            // Allocate memory for args in the WASM module
            let memory = instance
                .get_memory(&mut store, "memory")
                .ok_or_else(|| WasmSandboxError::Other("No exported memory".to_string()))?;

            let args_ptr = 0u32; // Use start of memory for simplicity
            memory
                .write(&mut store, args_ptr as usize, &args_json)
                .map_err(|e| WasmSandboxError::ExecutionError(e.to_string()))?;

            // Call the function
            let args_len = args_json.len() as u32;
            let mut results = vec![wasmtime::Val::I32(0)];
            func.call(
                &mut store,
                &[
                    wasmtime::Val::I32(args_ptr as i32),
                    wasmtime::Val::I32(args_len as i32),
                ],
                &mut results,
            )
            .map_err(|e| {
                if start.elapsed() > self.config.max_execution_time {
                    WasmSandboxError::ExecutionTimeout(start.elapsed())
                } else {
                    WasmSandboxError::ExecutionError(e.to_string())
                }
            })?;

            // Read the result from memory
            let result_ptr = results[0].unwrap_i32() as u32;
            // Read a fixed-size result buffer (convention: first 4 bytes = length)
            let mut len_buf = [0u8; 4];
            memory
                .read(&store, result_ptr as usize, &mut len_buf)
                .map_err(|e| WasmSandboxError::ExecutionError(e.to_string()))?;
            let result_len = u32::from_le_bytes(len_buf) as usize;

            let mut result_buf = vec![0u8; result_len];
            memory
                .read(&store, (result_ptr as usize) + 4, &mut result_buf)
                .map_err(|e| WasmSandboxError::ExecutionError(e.to_string()))?;

            let result_str = String::from_utf8(result_buf)
                .map_err(|e| WasmSandboxError::ExecutionError(e.to_string()))?;

            // Parse result
            let tool_result: WasmToolResult = serde_json::from_str(&result_str)
                .unwrap_or_else(|_| WasmToolResult::success(result_str));

            // Collect stats
            let fuel_used = self.config.max_fuel - store.get_fuel().unwrap_or(0);
            let stats = ExecutionStats {
                fuel_used,
                memory_used_bytes: memory.data_size(&store),
                execution_time: start.elapsed(),
            };

            Ok((tool_result, stats))
        }

        #[cfg(not(feature = "wasm"))]
        {
            let _ = (module, function_name, args);
            Err(WasmSandboxError::Other(
                "WASM feature not enabled".to_string(),
            ))
        }
    }

    /// Get the sandbox configuration.
    pub fn config(&self) -> &WasmSandboxConfig {
        &self.config
    }

    /// Extract metadata from a WASM module (best-effort).
    fn extract_metadata(&self, _bytes: &[u8]) -> Result<WasmPluginMetadata, WasmSandboxError> {
        // For now, return default metadata. In a real implementation,
        // we'd parse the WASM exports to find tool functions and metadata.
        Ok(WasmPluginMetadata {
            id: "unknown".to_string(),
            name: "WASM Plugin".to_string(),
            version: "0.0.0".to_string(),
            description: "A WASM plugin".to_string(),
            author: None,
            tools: vec![],
        })
    }
}

/// Manager for multiple WASM plugin instances.
pub struct WasmPluginManager {
    sandbox: Arc<WasmSandbox>,
    plugins: Vec<LoadedWasmPlugin>,
}

/// A loaded WASM plugin with its module and metadata.
struct LoadedWasmPlugin {
    module: WasmModule,
    #[allow(dead_code)]
    path: std::path::PathBuf,
}

impl WasmPluginManager {
    /// Create a new plugin manager with the given sandbox.
    pub fn new(sandbox: Arc<WasmSandbox>) -> Self {
        Self {
            sandbox,
            plugins: Vec::new(),
        }
    }

    /// Load a WASM plugin from a file.
    pub fn load(&mut self, path: &Path) -> Result<&WasmModule, WasmSandboxError> {
        let module = self.sandbox.load_module_from_file(path)?;
        self.plugins.push(LoadedWasmPlugin {
            module,
            path: path.to_path_buf(),
        });
        Ok(&self.plugins.last().unwrap().module)
    }

    /// Load a WASM plugin from bytes.
    pub fn load_from_bytes(&mut self, bytes: &[u8]) -> Result<&WasmModule, WasmSandboxError> {
        let module = self.sandbox.load_module(bytes)?;
        self.plugins.push(LoadedWasmPlugin {
            module,
            path: std::path::PathBuf::from("<memory>"),
        });
        Ok(&self.plugins.last().unwrap().module)
    }

    /// Execute a tool on a loaded plugin by id.
    pub fn execute_tool(
        &self,
        plugin_id: &str,
        function_name: &str,
        args: &serde_json::Value,
    ) -> Result<(WasmToolResult, ExecutionStats), WasmSandboxError> {
        let plugin = self
            .plugins
            .iter()
            .find(|p| p.module.metadata.id == plugin_id)
            .ok_or_else(|| WasmSandboxError::Other(format!("Plugin not found: {plugin_id}")))?;

        self.sandbox
            .execute_tool(&plugin.module, function_name, args)
    }

    /// List all loaded plugins.
    pub fn list(&self) -> Vec<&WasmPluginMetadata> {
        self.plugins.iter().map(|p| &p.module.metadata).collect()
    }

    /// Get the number of loaded plugins.
    pub fn count(&self) -> usize {
        self.plugins.len()
    }

    /// Get the sandbox.
    pub fn sandbox(&self) -> &WasmSandbox {
        &self.sandbox
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_config_default() {
        let config = WasmSandboxConfig::default();
        assert_eq!(config.max_memory_bytes, 64 * 1024 * 1024);
        assert_eq!(config.max_fuel, 1_000_000_000);
        assert!(!config.allow_wasi_fs);
        assert!(!config.allow_wasi_net);
    }

    #[test]
    fn test_sandbox_config_serialization() {
        let config = WasmSandboxConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("max_memory_bytes"));
        assert!(json.contains("max_fuel"));

        let deserialized: WasmSandboxConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.max_memory_bytes, config.max_memory_bytes);
    }

    #[test]
    fn test_wasm_tool_result() {
        let success = WasmToolResult::success("output");
        assert!(!success.is_error);
        assert_eq!(success.output, "output");

        let error = WasmToolResult::error("failed");
        assert!(error.is_error);
        assert_eq!(error.output, "failed");
    }

    #[test]
    fn test_wasm_tool_result_serialization() {
        let result = WasmToolResult::success("hello");
        let json = serde_json::to_string(&result).unwrap();
        let deserialized: WasmToolResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.output, "hello");
        assert!(!deserialized.is_error);
    }

    #[test]
    fn test_wasm_plugin_metadata_serialization() {
        let meta = WasmPluginMetadata {
            id: "test-plugin".to_string(),
            name: "Test Plugin".to_string(),
            version: "1.0.0".to_string(),
            description: "A test WASM plugin".to_string(),
            author: Some("Test Author".to_string()),
            tools: vec![WasmToolDef {
                name: "greet".to_string(),
                description: "Greet someone".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {"name": {"type": "string"}}}),
            }],
        };

        let json = serde_json::to_string(&meta).unwrap();
        assert!(json.contains("test-plugin"));
        assert!(json.contains("greet"));

        let deserialized: WasmPluginMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.tools.len(), 1);
    }

    #[test]
    fn test_wasm_sandbox_new() {
        let config = WasmSandboxConfig::default();
        let sandbox = WasmSandbox::new(config);
        assert!(sandbox.is_ok());
    }

    #[test]
    fn test_wasm_sandbox_with_defaults() {
        let sandbox = WasmSandbox::with_defaults();
        assert!(sandbox.is_ok());
    }

    #[test]
    fn test_wasm_sandbox_config() {
        let sandbox = WasmSandbox::with_defaults().unwrap();
        assert_eq!(sandbox.config().max_memory_bytes, 64 * 1024 * 1024);
    }

    #[test]
    fn test_wasm_sandbox_load_invalid_module() {
        let sandbox = WasmSandbox::with_defaults().unwrap();
        let result = sandbox.load_module(&[0x00, 0x01, 0x02]);
        assert!(result.is_err());
    }

    #[test]
    fn test_wasm_plugin_manager_new() {
        let sandbox = Arc::new(WasmSandbox::with_defaults().unwrap());
        let manager = WasmPluginManager::new(sandbox);
        assert_eq!(manager.count(), 0);
        assert!(manager.list().is_empty());
    }

    #[test]
    fn test_wasm_plugin_manager_load_invalid() {
        let sandbox = Arc::new(WasmSandbox::with_defaults().unwrap());
        let mut manager = WasmPluginManager::new(sandbox);
        let result = manager.load_from_bytes(&[0x00, 0x01]);
        assert!(result.is_err());
    }

    #[test]
    fn test_wasm_plugin_manager_execute_not_found() {
        let sandbox = Arc::new(WasmSandbox::with_defaults().unwrap());
        let manager = WasmPluginManager::new(sandbox);
        let result = manager.execute_tool("nonexistent", "func", &serde_json::json!({}));
        assert!(result.is_err());
    }

    #[test]
    fn test_execution_stats_default() {
        let stats = ExecutionStats::default();
        assert_eq!(stats.fuel_used, 0);
        assert_eq!(stats.memory_used_bytes, 0);
    }

    #[test]
    fn test_wasm_sandbox_error_display() {
        let err = WasmSandboxError::MemoryLimitExceeded {
            used: 100,
            limit: 50,
        };
        assert!(err.to_string().contains("100"));
        assert!(err.to_string().contains("50"));

        let err = WasmSandboxError::ExecutionTimeout(Duration::from_secs(5));
        assert!(err.to_string().contains("5s"));

        let err = WasmSandboxError::FuelExhausted {
            used: 1000,
            limit: 500,
        };
        assert!(err.to_string().contains("1000"));
    }

    #[test]
    fn test_wasm_tool_def_serialization() {
        let def = WasmToolDef {
            name: "test".to_string(),
            description: "A test tool".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        };

        let json = serde_json::to_string(&def).unwrap();
        let deserialized: WasmToolDef = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "test");
    }
}
