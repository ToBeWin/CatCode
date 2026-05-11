use catcode_core::types::{ChatRequest, Message, Role};
use serde::{Deserialize, Serialize};

/// Prompt Cache Optimizer marks stable content as cache boundaries
/// to maximize cache hit rates and reduce costs.
///
/// Providers like Anthropic support prompt caching where stable prefix
/// content can be cached and reused across requests. This optimizer
/// identifies which parts of a request are stable (system prompt, rules)
/// vs. dynamic (recent messages, tool outputs).
#[derive(Debug, Clone)]
pub struct PromptCacheOptimizer {
    /// Minimum number of tokens for a block to be worth caching.
    pub min_cacheable_tokens: usize,
    /// Whether to enable cache hints on system prompts.
    pub cache_system_prompt: bool,
    /// Whether to enable cache hints on early conversation messages.
    pub cache_early_messages: bool,
    /// Number of recent messages to NOT cache (they change frequently).
    pub skip_recent_messages: usize,
}

impl Default for PromptCacheOptimizer {
    fn default() -> Self {
        Self {
            min_cacheable_tokens: 1024,
            cache_system_prompt: true,
            cache_early_messages: true,
            skip_recent_messages: 2,
        }
    }
}

impl PromptCacheOptimizer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Analyze a ChatRequest and return cache optimization metadata.
    pub fn analyze(&self, request: &ChatRequest) -> CacheAnalysis {
        let mut analysis = CacheAnalysis::default();

        // System prompt is always the most cacheable part
        if self.cache_system_prompt
            && let Some(ref system) = request.system
        {
            let tokens = estimate_tokens(system);
            if tokens >= self.min_cacheable_tokens {
                analysis.cacheable_regions.push(CacheableRegion {
                    region_type: CacheRegionType::SystemPrompt,
                    token_estimate: tokens,
                    confidence: 0.95,
                });
                analysis.total_cacheable_tokens += tokens;
            }
        }

        // Early messages (before recent ones) are relatively stable
        if self.cache_early_messages && request.messages.len() > self.skip_recent_messages {
            let stable_end = request.messages.len() - self.skip_recent_messages;
            let stable_messages = &request.messages[..stable_end];

            // Group consecutive stable messages
            let mut group_start = 0;
            for (i, msg) in stable_messages.iter().enumerate() {
                let is_stable = matches!(msg.role, Role::System | Role::User | Role::Assistant)
                    && !is_tool_output(msg);

                if !is_stable || i == stable_messages.len() - 1 {
                    if i > group_start {
                        let group_tokens: usize = stable_messages[group_start..=i]
                            .iter()
                            .map(|m| estimate_tokens(&m.content))
                            .sum();

                        if group_tokens >= self.min_cacheable_tokens {
                            analysis.cacheable_regions.push(CacheableRegion {
                                region_type: CacheRegionType::EarlyMessages {
                                    start_index: group_start,
                                    end_index: i,
                                },
                                token_estimate: group_tokens,
                                confidence: 0.7,
                            });
                            analysis.total_cacheable_tokens += group_tokens;
                        }
                    }
                    group_start = i + 1;
                }
            }
        }

        // Estimate total request tokens
        analysis.total_request_tokens = estimate_request_tokens(request);

        // Calculate cache savings ratio
        if analysis.total_request_tokens > 0 {
            analysis.estimated_cache_hit_ratio =
                analysis.total_cacheable_tokens as f64 / analysis.total_request_tokens as f64;
        }

        analysis
    }

    /// Apply cache hints to a request (returns modified messages with cache markers).
    ///
    /// This is provider-specific. For Anthropic, it adds `cache_control` blocks.
    /// For now, returns a CachePlan that the provider can use.
    pub fn plan(&self, request: &ChatRequest) -> CachePlan {
        let analysis = self.analyze(request);

        CachePlan {
            cache_system: analysis
                .cacheable_regions
                .iter()
                .any(|r| matches!(r.region_type, CacheRegionType::SystemPrompt)),
            cache_message_indices: analysis
                .cacheable_regions
                .iter()
                .filter_map(|r| match r.region_type {
                    CacheRegionType::EarlyMessages { start_index, end_index } => {
                        Some((start_index, end_index))
                    }
                    _ => None,
                })
                .collect(),
            estimated_savings_tokens: analysis.total_cacheable_tokens,
            estimated_savings_ratio: analysis.estimated_cache_hit_ratio,
        }
    }
}

/// Types of cacheable content regions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheRegionType {
    /// System prompt — most stable, highest cache confidence.
    SystemPrompt,
    /// Early conversation messages — moderately stable.
    EarlyMessages { start_index: usize, end_index: usize },
}

/// A region of content that can be cached.
#[derive(Debug, Clone)]
pub struct CacheableRegion {
    pub region_type: CacheRegionType,
    pub token_estimate: usize,
    /// Confidence that this region will be cacheable (0.0-1.0).
    pub confidence: f64,
}

/// Analysis of a request's cache potential.
#[derive(Debug, Clone, Default)]
pub struct CacheAnalysis {
    pub cacheable_regions: Vec<CacheableRegion>,
    pub total_cacheable_tokens: usize,
    pub total_request_tokens: usize,
    pub estimated_cache_hit_ratio: f64,
}

/// Cache plan for a provider to execute.
#[derive(Debug, Clone, Default)]
pub struct CachePlan {
    /// Whether to mark the system prompt as cacheable.
    pub cache_system: bool,
    /// Message index ranges to mark as cacheable.
    pub cache_message_indices: Vec<(usize, usize)>,
    /// Estimated tokens that will be served from cache.
    pub estimated_savings_tokens: usize,
    /// Estimated ratio of request served from cache.
    pub estimated_savings_ratio: f64,
}

/// Check if a message is a tool output (these are dynamic and shouldn't be cached).
fn is_tool_output(msg: &Message) -> bool {
    msg.role == Role::Tool || msg.tool_call_id.is_some()
}

/// Estimate token count for a string (~4 chars per token).
fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

/// Estimate total tokens for a ChatRequest.
fn estimate_request_tokens(request: &ChatRequest) -> usize {
    let system_tokens = request
        .system
        .as_ref()
        .map(|s| estimate_tokens(s))
        .unwrap_or(0);

    let message_tokens: usize = request
        .messages
        .iter()
        .map(|m| estimate_tokens(&m.content) + 4) // +4 for role/formatting overhead
        .sum();

    system_tokens + message_tokens
}

/// Token usage statistics for cache tracking.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CacheStats {
    pub total_requests: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub tokens_saved: u64,
    pub cost_saved_usd: f64,
}

impl CacheStats {
    pub fn record_hit(&mut self, tokens_saved: u64, cost_per_mtok: f64) {
        self.total_requests += 1;
        self.cache_hits += 1;
        self.tokens_saved += tokens_saved;
        self.cost_saved_usd += (tokens_saved as f64 / 1_000_000.0) * cost_per_mtok;
    }

    pub fn record_miss(&mut self) {
        self.total_requests += 1;
        self.cache_misses += 1;
    }

    pub fn hit_rate(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            self.cache_hits as f64 / self.total_requests as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::types::Message;

    fn make_request(system: Option<&str>, messages: Vec<Message>) -> ChatRequest {
        ChatRequest {
            model: "test".to_string(),
            messages,
            tools: None,
            system: system.map(|s| s.to_string()),
            max_tokens: None,
            temperature: None,
            stream: false,
        }
    }

    #[test]
    fn test_analyze_with_large_system_prompt() {
        let system = "x".repeat(8000); // ~2000 tokens
        let request = make_request(Some(&system), vec![Message::user("hi")]);

        let optimizer = PromptCacheOptimizer::new();
        let analysis = optimizer.analyze(&request);

        assert!(!analysis.cacheable_regions.is_empty());
        assert!(analysis.total_cacheable_tokens > 1000);
    }

    #[test]
    fn test_analyze_small_system_prompt_not_cached() {
        let request = make_request(Some("hi"), vec![Message::user("hello")]);

        let optimizer = PromptCacheOptimizer::new();
        let analysis = optimizer.analyze(&request);

        // Small system prompt doesn't meet minimum threshold
        assert!(analysis
            .cacheable_regions
            .iter()
            .all(|r| r.region_type != CacheRegionType::SystemPrompt));
    }

    #[test]
    fn test_analyze_early_messages() {
        let system = "x".repeat(8000);
        let messages: Vec<Message> = (0..10)
            .map(|_i| Message::user(format!("message {}", "y".repeat(1000))))
            .collect();
        let request = make_request(Some(&system), messages);

        let optimizer = PromptCacheOptimizer {
            skip_recent_messages: 2,
            ..Default::default()
        };
        let analysis = optimizer.analyze(&request);

        // Should have system prompt + early messages cached
        assert!(analysis.cacheable_regions.len() >= 2);
    }

    #[test]
    fn test_plan_basic() {
        let system = "x".repeat(8000);
        let request = make_request(
            Some(&system),
            vec![
                Message::user("a".repeat(2000)),
                Message::assistant("b".repeat(2000)),
                Message::user("c"),
            ],
        );

        let optimizer = PromptCacheOptimizer::new();
        let plan = optimizer.plan(&request);

        assert!(plan.cache_system);
        assert!(plan.estimated_savings_tokens > 0);
    }

    #[test]
    fn test_no_system_prompt() {
        let request = make_request(None, vec![Message::user("hello")]);

        let optimizer = PromptCacheOptimizer::new();
        let analysis = optimizer.analyze(&request);

        assert!(analysis
            .cacheable_regions
            .iter()
            .all(|r| r.region_type != CacheRegionType::SystemPrompt));
    }

    #[test]
    fn test_cache_hit_ratio() {
        let system = "x".repeat(20000); // ~5000 tokens
        let request = make_request(
            Some(&system),
            vec![Message::user("short question")],
        );

        let optimizer = PromptCacheOptimizer::new();
        let analysis = optimizer.analyze(&request);

        // Most of the request is the system prompt
        assert!(analysis.estimated_cache_hit_ratio > 0.8);
    }

    #[test]
    fn test_cache_stats() {
        let mut stats = CacheStats::default();
        stats.record_hit(5000, 3.0);
        stats.record_hit(3000, 3.0);
        stats.record_miss();

        assert_eq!(stats.total_requests, 3);
        assert_eq!(stats.cache_hits, 2);
        assert_eq!(stats.cache_misses, 1);
        assert_eq!(stats.tokens_saved, 8000);
        assert!((stats.hit_rate() - 0.667).abs() < 0.01);
        assert!(stats.cost_saved_usd > 0.0);
    }

    #[test]
    fn test_cache_stats_empty() {
        let stats = CacheStats::default();
        assert_eq!(stats.hit_rate(), 0.0);
    }

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens("hello"), 2); // 5 chars / 4 = 2
        assert_eq!(estimate_tokens("x".repeat(100).as_str()), 25);
    }

    #[test]
    fn test_custom_config() {
        let optimizer = PromptCacheOptimizer {
            min_cacheable_tokens: 500,
            cache_system_prompt: false,
            cache_early_messages: true,
            skip_recent_messages: 5,
        };

        let request = make_request(
            Some(&"x".repeat(4000)),
            vec![Message::user("y".repeat(4000))],
        );

        let analysis = optimizer.analyze(&request);
        // System prompt not cached (disabled)
        assert!(analysis
            .cacheable_regions
            .iter()
            .all(|r| r.region_type != CacheRegionType::SystemPrompt));
    }
}
