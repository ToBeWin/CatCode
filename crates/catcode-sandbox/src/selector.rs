use std::sync::Arc;

use crate::backend::{DockerSandbox, NativeSandbox, SandboxBackend};

/// Selects the best available sandbox backend.
///
/// Backend priority: Docker > Firejail > Native
pub struct SandboxSelector {
    backends: Vec<(String, Arc<dyn SandboxBackend>)>,
}

impl SandboxSelector {
    pub fn new() -> Self {
        let mut selector = Self {
            backends: Vec::new(),
        };

        // Register backends in priority order
        // Docker is preferred; fall back to native
        selector.register("docker", Arc::new(DockerSandbox::new()));
        selector.register("native", Arc::new(NativeSandbox::new()));

        selector
    }

    /// Register a backend with a name.
    pub fn register(&mut self, name: &str, backend: Arc<dyn SandboxBackend>) {
        self.backends.push((name.to_string(), backend));
    }

    /// Get the best available backend.
    pub fn select(&self) -> Option<Arc<dyn SandboxBackend>> {
        self.backends
            .iter()
            .find(|(_, b)| b.is_available())
            .map(|(_, b)| b.clone())
    }

    /// Get a specific backend by name.
    pub fn get(&self, name: &str) -> Option<Arc<dyn SandboxBackend>> {
        self.backends
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, b)| b.clone())
    }

    /// List all available backends.
    pub fn list_available(&self) -> Vec<&str> {
        self.backends
            .iter()
            .filter(|(_, b)| b.is_available())
            .map(|(n, _)| n.as_str())
            .collect()
    }
}

impl Default for SandboxSelector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selector_has_native() {
        let selector = SandboxSelector::new();
        assert!(selector.select().is_some());
        assert_eq!(selector.select().unwrap().name(), "native");
    }

    #[test]
    fn test_selector_get_by_name() {
        let selector = SandboxSelector::new();
        assert!(selector.get("native").is_some());
        assert!(selector.get("docker").is_some());
    }

    #[test]
    fn test_selector_list_available() {
        let selector = SandboxSelector::new();
        let available = selector.list_available();
        assert!(available.contains(&"native"));
    }
}
