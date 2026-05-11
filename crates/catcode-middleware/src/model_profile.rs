use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Instruction style for different model capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstructionStyle {
    /// Strong models: concise, minimal instructions.
    Concise,
    /// Weaker models: detailed, step-by-step instructions.
    Explicit,
    /// Reasoning models: encourage chain-of-thought.
    ChainOfThought,
}

/// Tool call format preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolCallFormat {
    /// OpenAI-compatible function calling.
    OpenAI,
    /// Anthropic-style tool use.
    Anthropic,
    /// XML-based tool calls (for models that prefer XML).
    Xml,
}

/// Profile for a specific model, describing its capabilities and
/// how to adapt prompts for optimal performance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelProfile {
    pub model_id: String,
    pub instruction_style: InstructionStyle,
    pub tool_call_format: ToolCallFormat,
    pub max_tools_per_turn: u32,
    pub prefers_simple_prompts: bool,
    /// Whether the model supports system prompts natively.
    pub supports_system_prompt: bool,
    /// Whether the model benefits from examples in prompts.
    pub needs_examples: bool,
    /// Maximum recommended prompt length in tokens.
    pub max_prompt_tokens: u64,
}

impl ModelProfile {
    /// Create a profile for a strong model (e.g., Claude, GPT-4).
    pub fn strong(model_id: &str) -> Self {
        Self {
            model_id: model_id.to_string(),
            instruction_style: InstructionStyle::Concise,
            tool_call_format: ToolCallFormat::OpenAI,
            max_tools_per_turn: 20,
            prefers_simple_prompts: true,
            supports_system_prompt: true,
            needs_examples: false,
            max_prompt_tokens: 100_000,
        }
    }

    /// Create a profile for a balanced model (e.g., DeepSeek, Sonnet).
    pub fn balanced(model_id: &str) -> Self {
        Self {
            model_id: model_id.to_string(),
            instruction_style: InstructionStyle::Explicit,
            tool_call_format: ToolCallFormat::OpenAI,
            max_tools_per_turn: 10,
            prefers_simple_prompts: false,
            supports_system_prompt: true,
            needs_examples: true,
            max_prompt_tokens: 64_000,
        }
    }

    /// Create a profile for a fast/weak model (e.g., Haiku, small local models).
    pub fn fast(model_id: &str) -> Self {
        Self {
            model_id: model_id.to_string(),
            instruction_style: InstructionStyle::Explicit,
            tool_call_format: ToolCallFormat::OpenAI,
            max_tools_per_turn: 5,
            prefers_simple_prompts: false,
            supports_system_prompt: true,
            needs_examples: true,
            max_prompt_tokens: 32_000,
        }
    }

    /// Create a profile for a reasoning model (e.g., o3, deepseek-reasoner).
    pub fn reasoning(model_id: &str) -> Self {
        Self {
            model_id: model_id.to_string(),
            instruction_style: InstructionStyle::ChainOfThought,
            tool_call_format: ToolCallFormat::OpenAI,
            max_tools_per_turn: 15,
            prefers_simple_prompts: true,
            supports_system_prompt: true,
            needs_examples: false,
            max_prompt_tokens: 128_000,
        }
    }

    /// Adapt a system prompt for this model's capabilities.
    pub fn adapt_system_prompt(&self, base_prompt: &str) -> String {
        let mut prompt = base_prompt.to_string();

        match self.instruction_style {
            InstructionStyle::Concise => {
                // Strong models work fine with minimal prompts
            }
            InstructionStyle::Explicit => {
                prompt.push_str("\n\nBe explicit and step-by-step in your reasoning.");
                if self.needs_examples {
                    prompt.push_str("\nShow examples when explaining concepts.");
                }
            }
            InstructionStyle::ChainOfThought => {
                prompt.push_str(
                    "\n\nThink through the problem step by step. \
                     Show your reasoning before giving the final answer.",
                );
            }
        }

        prompt
    }

    /// Limit the number of tools to this model's preference.
    pub fn limit_tools(&self, tools: &mut Vec<serde_json::Value>) {
        if tools.len() > self.max_tools_per_turn as usize {
            tools.truncate(self.max_tools_per_turn as usize);
        }
    }
}

/// Registry of model profiles.
pub struct ProfileRegistry {
    profiles: HashMap<String, ModelProfile>,
}

impl ProfileRegistry {
    pub fn new() -> Self {
        Self {
            profiles: HashMap::new(),
        }
    }

    /// Register a model profile.
    pub fn register(&mut self, profile: ModelProfile) {
        self.profiles.insert(profile.model_id.clone(), profile);
    }

    /// Get a profile for a model. Falls back to a default balanced profile.
    pub fn get(&self, model_id: &str) -> ModelProfile {
        self.profiles
            .get(model_id)
            .cloned()
            .unwrap_or_else(|| ModelProfile::balanced(model_id))
    }

    /// Register built-in profiles for known models.
    pub fn register_defaults(&mut self) {
        // Anthropic
        self.register(ModelProfile::strong("claude-opus-4-20250514"));
        self.register(ModelProfile::strong("claude-sonnet-4-20250514"));
        self.register(ModelProfile::fast("claude-haiku-4-5-20251001"));

        // DeepSeek
        self.register(ModelProfile::balanced("deepseek-chat"));
        self.register(ModelProfile::reasoning("deepseek-reasoner"));

        // OpenAI
        self.register(ModelProfile::strong("gpt-4o"));
        self.register(ModelProfile::reasoning("o3"));
        self.register(ModelProfile::reasoning("o3-mini"));
        self.register(ModelProfile::fast("gpt-4o-mini"));

        // Ollama local models
        self.register(ModelProfile::fast("llama3.1"));
        self.register(ModelProfile::fast("qwen2.5"));
        self.register(ModelProfile::fast("codellama"));

        // Qwen (DashScope)
        self.register(ModelProfile::strong("qwen3"));
        self.register(ModelProfile::strong("qwen3-coder"));
        self.register(ModelProfile::balanced("qwen3-moe"));
        self.register(ModelProfile::balanced("qwen2.5"));
    }
}

impl Default for ProfileRegistry {
    fn default() -> Self {
        let mut registry = Self::new();
        registry.register_defaults();
        registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strong_profile() {
        let profile = ModelProfile::strong("claude-opus-4");
        assert_eq!(profile.instruction_style, InstructionStyle::Concise);
        assert!(profile.prefers_simple_prompts);
        assert!(!profile.needs_examples);
        assert_eq!(profile.max_tools_per_turn, 20);
    }

    #[test]
    fn test_balanced_profile() {
        let profile = ModelProfile::balanced("deepseek-chat");
        assert_eq!(profile.instruction_style, InstructionStyle::Explicit);
        assert!(!profile.prefers_simple_prompts);
        assert!(profile.needs_examples);
    }

    #[test]
    fn test_fast_profile() {
        let profile = ModelProfile::fast("llama3.1");
        assert_eq!(profile.instruction_style, InstructionStyle::Explicit);
        assert_eq!(profile.max_tools_per_turn, 5);
    }

    #[test]
    fn test_reasoning_profile() {
        let profile = ModelProfile::reasoning("o3");
        assert_eq!(
            profile.instruction_style,
            InstructionStyle::ChainOfThought
        );
    }

    #[test]
    fn test_adapt_system_prompt_concise() {
        let profile = ModelProfile::strong("claude-opus-4");
        let adapted = profile.adapt_system_prompt("You are helpful.");
        assert_eq!(adapted, "You are helpful.");
    }

    #[test]
    fn test_adapt_system_prompt_explicit() {
        let profile = ModelProfile::balanced("deepseek-chat");
        let adapted = profile.adapt_system_prompt("You are helpful.");
        assert!(adapted.contains("step-by-step"));
    }

    #[test]
    fn test_adapt_system_prompt_cot() {
        let profile = ModelProfile::reasoning("o3");
        let adapted = profile.adapt_system_prompt("You are helpful.");
        assert!(adapted.contains("Think through"));
    }

    #[test]
    fn test_adapt_system_prompt_fast_with_examples() {
        let profile = ModelProfile::fast("llama3.1");
        let adapted = profile.adapt_system_prompt("You are helpful.");
        assert!(adapted.contains("examples"));
    }

    #[test]
    fn test_limit_tools() {
        let profile = ModelProfile::fast("llama3.1");
        let mut tools: Vec<serde_json::Value> = (0..10)
            .map(|i| serde_json::json!({"name": format!("tool_{i}")}))
            .collect();

        profile.limit_tools(&mut tools);
        assert_eq!(tools.len(), 5);
    }

    #[test]
    fn test_limit_tools_noop_when_under_limit() {
        let profile = ModelProfile::strong("claude-opus-4");
        let mut tools: Vec<serde_json::Value> = (0..5)
            .map(|i| serde_json::json!({"name": format!("tool_{i}")}))
            .collect();

        profile.limit_tools(&mut tools);
        assert_eq!(tools.len(), 5);
    }

    #[test]
    fn test_profile_registry_defaults() {
        let registry = ProfileRegistry::default();
        let claude = registry.get("claude-opus-4-20250514");
        assert_eq!(claude.instruction_style, InstructionStyle::Concise);

        let deepseek = registry.get("deepseek-chat");
        assert_eq!(deepseek.instruction_style, InstructionStyle::Explicit);

        let unknown = registry.get("unknown-model");
        assert_eq!(unknown.instruction_style, InstructionStyle::Explicit);
    }

    #[test]
    fn test_profile_registry_custom() {
        let mut registry = ProfileRegistry::new();
        registry.register(ModelProfile::reasoning("my-custom-model"));

        let profile = registry.get("my-custom-model");
        assert_eq!(
            profile.instruction_style,
            InstructionStyle::ChainOfThought
        );
    }

    #[test]
    fn test_profile_serialization() {
        let profile = ModelProfile::strong("test-model");
        let json = serde_json::to_string(&profile).unwrap();
        assert!(json.contains("test-model"));
        assert!(json.contains("Concise"));

        let deserialized: ModelProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.model_id, "test-model");
    }
}
