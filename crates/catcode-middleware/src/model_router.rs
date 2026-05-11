use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Routing strategy for selecting models.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RoutingStrategy {
    /// Always use a fixed model.
    Fixed(String),
    /// Route based on task complexity.
    CostAware {
        simple_model: String,
        powerful_model: String,
        complexity_threshold: f32,
    },
    /// Try models in priority order, falling back on failure.
    Fallback(Vec<String>),
}

/// Information about provider health for routing decisions.
#[derive(Debug, Clone, Default)]
pub struct ProviderHealth {
    /// Map of provider_id -> is_healthy.
    pub providers: HashMap<String, bool>,
    /// Map of provider_id -> recent error rate (0.0-1.0).
    pub error_rates: HashMap<String, f64>,
}

/// Token budget information for routing decisions.
#[derive(Debug, Clone)]
pub struct RoutingBudget {
    pub remaining_tokens: u64,
    pub total_tokens: u64,
    pub max_cost_per_request_usd: f64,
}

/// Model Router selects the best model for a given task.
#[derive(Debug, Clone)]
pub struct ModelRouter {
    strategy: RoutingStrategy,
}

impl ModelRouter {
    pub fn new(strategy: RoutingStrategy) -> Self {
        Self { strategy }
    }

    /// Select a model based on the current context.
    pub fn select_model(
        &self,
        task_complexity: f32,
        budget: &RoutingBudget,
        health: &ProviderHealth,
    ) -> String {
        match &self.strategy {
            RoutingStrategy::Fixed(model) => model.clone(),
            RoutingStrategy::CostAware {
                simple_model,
                powerful_model,
                complexity_threshold,
            } => {
                // Use powerful model if complexity exceeds threshold AND budget allows
                if task_complexity >= *complexity_threshold
                    && budget.remaining_tokens > budget.total_tokens / 10
                {
                    // Check if the powerful model's provider is healthy
                    let provider = extract_provider(powerful_model);
                    if health.providers.get(provider).copied().unwrap_or(true) {
                        return powerful_model.clone();
                    }
                }
                simple_model.clone()
            }
            RoutingStrategy::Fallback(models) => {
                // Select the first healthy model
                for model in models {
                    let provider = extract_provider(model);
                    if health.providers.get(provider).copied().unwrap_or(true) {
                        return model.clone();
                    }
                }
                // All unhealthy — return first model anyway
                models.first().cloned().unwrap_or_default()
            }
        }
    }

    /// Get the routing strategy.
    pub fn strategy(&self) -> &RoutingStrategy {
        &self.strategy
    }
}

impl Default for ModelRouter {
    fn default() -> Self {
        Self::new(RoutingStrategy::Fixed("deepseek-chat".to_string()))
    }
}

/// Extract a provider identifier from a model name.
/// Convention: "provider/model" or just "model" (defaults to "default").
fn extract_provider(model: &str) -> &str {
    model.split('/').next().unwrap_or("default")
}

/// Estimate task complexity from a text description (0.0 to 1.0).
///
/// This is a simple heuristic. In production, this could use a classifier model.
pub fn estimate_complexity(description: &str) -> f32 {
    let lower = description.to_lowercase();
    let mut score: f32 = 0.3; // baseline

    // Longer descriptions tend to be more complex
    let word_count = lower.split_whitespace().count();
    if word_count > 50 {
        score += 0.1;
    }
    if word_count > 100 {
        score += 0.1;
    }

    // Keywords indicating complexity
    let complex_keywords = [
        "refactor", "architecture", "design", "optimize", "security",
        "concurrent", "async", "distributed", "database", "migration",
        "performance", "benchmark", "algorithm", "protocol",
    ];
    for keyword in &complex_keywords {
        if lower.contains(keyword) {
            score += 0.05;
        }
    }

    // Keywords indicating simplicity
    let simple_keywords = ["fix typo", "rename", "update comment", "readme", "format"];
    for keyword in &simple_keywords {
        if lower.contains(keyword) {
            score -= 0.1;
        }
    }

    score.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixed_strategy() {
        let router = ModelRouter::new(RoutingStrategy::Fixed("gpt-4o".to_string()));
        let budget = RoutingBudget {
            remaining_tokens: 1000,
            total_tokens: 1000,
            max_cost_per_request_usd: 1.0,
        };
        let health = ProviderHealth::default();

        assert_eq!(router.select_model(0.9, &budget, &health), "gpt-4o");
    }

    #[test]
    fn test_cost_aware_simple_task() {
        let router = ModelRouter::new(RoutingStrategy::CostAware {
            simple_model: "deepseek-chat".to_string(),
            powerful_model: "claude-opus-4".to_string(),
            complexity_threshold: 0.6,
        });
        let budget = RoutingBudget {
            remaining_tokens: 100_000,
            total_tokens: 100_000,
            max_cost_per_request_usd: 1.0,
        };
        let health = ProviderHealth::default();

        assert_eq!(
            router.select_model(0.3, &budget, &health),
            "deepseek-chat"
        );
    }

    #[test]
    fn test_cost_aware_complex_task() {
        let router = ModelRouter::new(RoutingStrategy::CostAware {
            simple_model: "deepseek-chat".to_string(),
            powerful_model: "claude-opus-4".to_string(),
            complexity_threshold: 0.6,
        });
        let budget = RoutingBudget {
            remaining_tokens: 100_000,
            total_tokens: 100_000,
            max_cost_per_request_usd: 1.0,
        };
        let health = ProviderHealth::default();

        assert_eq!(
            router.select_model(0.8, &budget, &health),
            "claude-opus-4"
        );
    }

    #[test]
    fn test_cost_aware_low_budget_falls_back() {
        let router = ModelRouter::new(RoutingStrategy::CostAware {
            simple_model: "deepseek-chat".to_string(),
            powerful_model: "claude-opus-4".to_string(),
            complexity_threshold: 0.6,
        });
        let budget = RoutingBudget {
            remaining_tokens: 5_000,  // Less than 10% of total
            total_tokens: 100_000,
            max_cost_per_request_usd: 1.0,
        };
        let health = ProviderHealth::default();

        // Even complex task falls back to simple model when budget is low
        assert_eq!(
            router.select_model(0.8, &budget, &health),
            "deepseek-chat"
        );
    }

    #[test]
    fn test_cost_aware_unhealthy_provider() {
        let router = ModelRouter::new(RoutingStrategy::CostAware {
            simple_model: "deepseek-chat".to_string(),
            powerful_model: "anthropic/claude-opus-4".to_string(),
            complexity_threshold: 0.6,
        });
        let budget = RoutingBudget {
            remaining_tokens: 100_000,
            total_tokens: 100_000,
            max_cost_per_request_usd: 1.0,
        };
        let mut health = ProviderHealth::default();
        health.providers.insert("anthropic".to_string(), false);

        // Powerful model's provider is unhealthy — falls back to simple
        assert_eq!(
            router.select_model(0.8, &budget, &health),
            "deepseek-chat"
        );
    }

    #[test]
    fn test_fallback_strategy_first_healthy() {
        let router = ModelRouter::new(RoutingStrategy::Fallback(vec![
            "anthropic/claude-sonnet-4".to_string(),
            "deepseek-chat".to_string(),
            "ollama/llama3".to_string(),
        ]));
        let budget = RoutingBudget {
            remaining_tokens: 100_000,
            total_tokens: 100_000,
            max_cost_per_request_usd: 1.0,
        };
        let mut health = ProviderHealth::default();
        health.providers.insert("anthropic".to_string(), false);

        assert_eq!(
            router.select_model(0.5, &budget, &health),
            "deepseek-chat"
        );
    }

    #[test]
    fn test_fallback_all_unhealthy() {
        let router = ModelRouter::new(RoutingStrategy::Fallback(vec![
            "model-a".to_string(),
            "model-b".to_string(),
        ]));
        let budget = RoutingBudget {
            remaining_tokens: 100_000,
            total_tokens: 100_000,
            max_cost_per_request_usd: 1.0,
        };
        let mut health = ProviderHealth::default();
        health.providers.insert("model-a".to_string(), false);
        health.providers.insert("model-b".to_string(), false);

        // Falls back to first model even if unhealthy
        assert_eq!(
            router.select_model(0.5, &budget, &health),
            "model-a"
        );
    }

    #[test]
    fn test_extract_provider() {
        assert_eq!(extract_provider("anthropic/claude-sonnet-4"), "anthropic");
        assert_eq!(extract_provider("deepseek-chat"), "deepseek-chat");
    }

    #[test]
    fn test_estimate_complexity_simple() {
        let score = estimate_complexity("Fix the typo in README");
        assert!(score < 0.4);
    }

    #[test]
    fn test_estimate_complexity_moderate() {
        let score = estimate_complexity("Add a new endpoint to the API");
        assert!(score >= 0.3 && score <= 0.6);
    }

    #[test]
    fn test_estimate_complexity_high() {
        let score = estimate_complexity(
            "Refactor the architecture to support concurrent async \
             distributed database migration with performance benchmarks",
        );
        assert!(score > 0.6);
    }

    #[test]
    fn test_default_router() {
        let router = ModelRouter::default();
        match router.strategy() {
            RoutingStrategy::Fixed(model) => assert_eq!(model, "deepseek-chat"),
            _ => panic!("Expected Fixed strategy"),
        }
    }
}
