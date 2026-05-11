pub mod anthropic;
pub mod deepseek;
pub mod glm;
pub mod google;
pub mod minimax;
pub mod mock;
pub mod ollama;
pub mod openai;
pub mod qwen;

pub use catcode_core::provider::*;

use std::collections::HashMap;
use std::sync::Arc;

/// Registry for managing multiple providers.
pub struct ProviderRegistry {
    providers: HashMap<String, Arc<dyn Provider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    /// Register a provider. Uses the provider's id() as the key.
    pub fn register(&mut self, provider: Arc<dyn Provider>) {
        self.providers.insert(provider.id().to_string(), provider);
    }

    /// Get a provider by id.
    pub fn get(&self, id: &str) -> Option<Arc<dyn Provider>> {
        self.providers.get(id).cloned()
    }

    /// List all registered provider ids.
    pub fn list_ids(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }

    /// List providers that pass health check.
    pub async fn list_healthy(&self) -> Vec<Arc<dyn Provider>> {
        let mut healthy = Vec::new();
        for provider in self.providers.values() {
            if provider.health_check().await.is_ok() {
                healthy.push(provider.clone());
            }
        }
        healthy
    }

    /// Get all registered providers.
    pub fn all(&self) -> Vec<Arc<dyn Provider>> {
        self.providers.values().cloned().collect()
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockProvider;

    #[test]
    fn test_registry_register_and_get() {
        let mut registry = ProviderRegistry::new();
        let mock = Arc::new(MockProvider::with_text_response("hello"));
        registry.register(mock.clone());

        let got = registry.get("mock").unwrap();
        assert_eq!(got.id(), "mock");
    }

    #[test]
    fn test_registry_get_nonexistent() {
        let registry = ProviderRegistry::new();
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_registry_list_ids() {
        let mut registry = ProviderRegistry::new();
        registry.register(Arc::new(MockProvider::with_text_response("a")));
        let ids = registry.list_ids();
        assert!(ids.contains(&"mock".to_string()));
    }

    #[test]
    fn test_registry_default() {
        let registry = ProviderRegistry::default();
        assert!(registry.list_ids().is_empty());
    }

    #[tokio::test]
    async fn test_registry_list_healthy() {
        let mut registry = ProviderRegistry::new();
        registry.register(Arc::new(MockProvider::with_text_response("hello")));

        let healthy = registry.list_healthy().await;
        assert_eq!(healthy.len(), 1);
        assert_eq!(healthy[0].id(), "mock");
    }

    #[tokio::test]
    async fn test_registry_all() {
        let mut registry = ProviderRegistry::new();
        registry.register(Arc::new(MockProvider::with_text_response("a")));

        let all = registry.all();
        assert_eq!(all.len(), 1);
    }
}
