use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::agent_events::AgentEventSender;
use anyhow::{Context, bail};
use catcode_context::{ContextStack, TokenBudget};
use catcode_core::Provider;
use catcode_provider::{
    anthropic::AnthropicProvider, deepseek::DeepSeekProvider, glm::GLMProvider,
    google::GoogleProvider, minimax::MiniMaxProvider, mock::MockProvider, ollama::OllamaProvider,
    openai::OpenAIProvider, openrouter::OpenRouterProvider, qwen::QwenProvider,
    volcengine::VolcengineProvider,
};
use catcode_tools::ToolRegistry;

use crate::{
    AgentLoop, AgentLoopResult, AuditLogMiddleware, Config, Database, build_context_pack,
    build_harness_plan, build_verification_repair_prompt, capture_git_snapshot,
    default_middleware_chain, run_auto_verification,
};

/// Runtime options for a single agent run.
#[derive(Clone)]
pub struct AgentRuntimeOptions {
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub system_prompt: String,
    pub session_id: Option<String>,
    pub audit_db: Option<Database>,
    pub auto_repair: bool,
}

impl Default for AgentRuntimeOptions {
    fn default() -> Self {
        Self {
            provider_id: None,
            model_id: None,
            system_prompt: default_system_prompt().to_string(),
            session_id: None,
            audit_db: None,
            auto_repair: true,
        }
    }
}

/// Shared runtime used by CLI, TUI, and daemon API execution.
#[derive(Debug, Clone, Default)]
pub struct AgentRuntime;

impl AgentRuntime {
    pub fn new() -> Self {
        Self
    }

    /// Run one user message through the full AgentLoop.
    pub async fn run_once(
        &self,
        message: &str,
        project_dir: &Path,
        options: AgentRuntimeOptions,
    ) -> anyhow::Result<AgentLoopResult> {
        self.run_once_with_events(message, project_dir, options, None)
            .await
    }

    /// Run with an optional event sender for real-time TUI progress.
    pub async fn run_once_with_events(
        &self,
        message: &str,
        project_dir: &Path,
        options: AgentRuntimeOptions,
        event_tx: Option<AgentEventSender>,
    ) -> anyhow::Result<AgentLoopResult> {
        let config = load_config(project_dir)?;
        let provider_id = options
            .provider_id
            .unwrap_or_else(|| config.defaults.provider.clone());
        let model_id = options
            .model_id
            .unwrap_or_else(|| config.defaults.model.clone());
        let provider = build_provider(&provider_id)?;
        let project_rules = load_project_rules(project_dir);
        let harness_plan = build_harness_plan(project_dir, message);
        let before_snapshot = capture_git_snapshot(project_dir).await;
        let context_pack = build_context_pack(
            project_dir,
            message,
            &harness_plan.repo,
            before_snapshot.as_ref(),
        )
        .await;
        let harness_tx = event_tx.clone();
        if let Some(tx) = event_tx.as_ref() {
            for step in harness_plan.startup_steps() {
                let _ = tx.send(crate::AgentStreamEvent::HarnessStep {
                    phase: step.phase,
                    status: step.status,
                    message: step.message,
                });
            }
            let _ = tx.send(crate::AgentStreamEvent::HarnessStep {
                phase: crate::HarnessPhase::ContextPack,
                status: crate::HarnessStepStatus::Done,
                message: context_pack.summary_line(),
            });
            let _ = tx.send(crate::AgentStreamEvent::Status(harness_plan.status_line()));
        }
        let system_prompt = format!(
            "{}{}{}",
            options.system_prompt,
            harness_plan.system_prompt_block(),
            context_pack.system_prompt_block()
        );
        let context = ContextStack::new(system_prompt, project_rules);
        let budget = TokenBudget::new(
            config.budget.session_limit_tokens,
            config.budget.per_request_limit_tokens,
            config.budget.warning_threshold,
        );

        let tools = Arc::new(ToolRegistry::with_builtins());
        let mut middleware_chain = default_middleware_chain();
        if let Some(db) = options.audit_db {
            middleware_chain.add(AuditLogMiddleware::new(db));
        }
        let middleware = Arc::new(middleware_chain);
        let mut agent = AgentLoop::new(provider, tools, middleware, context, budget, model_id);
        if let Some(tx) = event_tx {
            agent = agent.with_event_tx(tx);
        }

        let session_id = options.session_id.clone();
        let mut result = agent
            .run_intelligent_with_session(message, project_dir, session_id.clone())
            .await;
        let run_succeeded = result.is_ok();
        let after_snapshot = capture_git_snapshot(project_dir).await;
        let changed = before_snapshot
            .as_ref()
            .zip(after_snapshot.as_ref())
            .map(|(before, after)| after.changed_since(before))
            .unwrap_or(false);

        if let Some(tx) = harness_tx.as_ref() {
            for step in harness_plan.completion_steps(
                before_snapshot.as_ref(),
                after_snapshot.as_ref(),
                run_succeeded,
            ) {
                let _ = tx.send(crate::AgentStreamEvent::HarnessStep {
                    phase: step.phase,
                    status: step.status,
                    message: step.message,
                });
            }
        }

        if changed
            && run_succeeded
            && let Some(command) = harness_plan.verification.primary_auto_runnable()
        {
            if let Some(tx) = harness_tx.as_ref() {
                let _ = tx.send(crate::AgentStreamEvent::HarnessStep {
                    phase: crate::HarnessPhase::Verification,
                    status: crate::HarnessStepStatus::Running,
                    message: format!("Running {}", command.command),
                });
            }
            if let Some(verification_result) =
                run_auto_verification(project_dir, &harness_plan.verification).await
            {
                if let Some(tx) = harness_tx.as_ref() {
                    let _ = tx.send(crate::AgentStreamEvent::HarnessStep {
                        phase: crate::HarnessPhase::Verification,
                        status: if verification_result.success {
                            crate::HarnessStepStatus::Done
                        } else {
                            crate::HarnessStepStatus::Failed
                        },
                        message: verification_result.actionable_summary(),
                    });
                }
                if options.auto_repair
                    && !verification_result.success
                    && let Some(repair_prompt) =
                        build_verification_repair_prompt(&verification_result)
                {
                    if let Some(tx) = harness_tx.as_ref() {
                        let _ = tx.send(crate::AgentStreamEvent::HarnessStep {
                            phase: crate::HarnessPhase::Recovery,
                            status: crate::HarnessStepStatus::Running,
                            message:
                                "Attempting one focused repair pass from verification diagnostics."
                                    .to_string(),
                        });
                    }
                    let repair_result = agent
                        .run_intelligent_with_session(&repair_prompt, project_dir, session_id)
                        .await;
                    match repair_result {
                        Ok(repair) => {
                            if let Some(tx) = harness_tx.as_ref() {
                                let _ = tx.send(crate::AgentStreamEvent::HarnessStep {
                                    phase: crate::HarnessPhase::Recovery,
                                    status: crate::HarnessStepStatus::Done,
                                    message: "Repair pass completed; rerunning verification."
                                        .to_string(),
                                });
                            }
                            if let Some(recheck) =
                                run_auto_verification(project_dir, &harness_plan.verification).await
                                && let Some(tx) = harness_tx.as_ref()
                            {
                                let _ = tx.send(crate::AgentStreamEvent::HarnessStep {
                                    phase: crate::HarnessPhase::Verification,
                                    status: if recheck.success {
                                        crate::HarnessStepStatus::Done
                                    } else {
                                        crate::HarnessStepStatus::Failed
                                    },
                                    message: format!(
                                        "After repair: {}",
                                        recheck.actionable_summary()
                                    ),
                                });
                            }
                            if let Ok(initial) = result {
                                result = Ok(merge_agent_results(initial, repair));
                            } else {
                                result = Ok(repair);
                            }
                        }
                        Err(err) => {
                            if let Some(tx) = harness_tx.as_ref() {
                                let _ = tx.send(crate::AgentStreamEvent::HarnessStep {
                                    phase: crate::HarnessPhase::Recovery,
                                    status: crate::HarnessStepStatus::Failed,
                                    message: format!("Repair pass failed: {err}"),
                                });
                            }
                        }
                    }
                }
            }
        }

        result.map_err(|err| anyhow::anyhow!("agent run failed: {err}"))
    }
}

fn merge_agent_results(mut initial: AgentLoopResult, repair: AgentLoopResult) -> AgentLoopResult {
    initial.response = if repair.response.trim().is_empty() {
        initial.response
    } else {
        repair.response
    };
    initial.total_usage = initial.total_usage + repair.total_usage;
    initial.turns_used += repair.turns_used;
    initial.hit_max_turns |= repair.hit_max_turns;
    initial.messages.extend(repair.messages);
    if initial.auto_plan.is_none() {
        initial.auto_plan = repair.auto_plan;
    }
    initial.self_healed += repair.self_healed + 1;
    initial.sub_agents_spawned += repair.sub_agents_spawned;
    for model in repair.models_used {
        if !initial.models_used.contains(&model) {
            initial.models_used.push(model);
        }
    }
    initial
}

/// Load config using the standard search order.
pub fn load_config(project_dir: &Path) -> anyhow::Result<Config> {
    let local = project_dir.join("catcode.toml");
    if local.exists() {
        return Config::load(&local).with_context(|| format!("failed to load {}", local.display()));
    }

    let project_scoped = Config::config_path(project_dir);
    if project_scoped.exists() {
        return Config::load(&project_scoped)
            .with_context(|| format!("failed to load {}", project_scoped.display()));
    }

    if let Some(global) = dirs::config_dir().map(|p| p.join("catcode").join("config.toml"))
        && global.exists()
    {
        return Config::load(&global)
            .with_context(|| format!("failed to load {}", global.display()));
    }

    Ok(Config::default())
}

/// Build a provider from env/config ids.
pub fn build_provider(provider_id: &str) -> anyhow::Result<Arc<dyn Provider>> {
    let provider: Arc<dyn Provider> = match provider_id {
        "mock" => Arc::new(MockProvider::with_text_response(
            "Mock provider response. Configure a real provider or pass --provider ollama/deepseek/etc. to run against a model.",
        )),
        "ollama" => Arc::new(OllamaProvider::new(env_or_default(
            "OLLAMA_BASE_URL",
            "http://localhost:11434",
        ))),
        "deepseek" => Arc::new(DeepSeekProvider::new(
            api_key(provider_id, "DEEPSEEK_API_KEY")?,
            env_or_default("DEEPSEEK_BASE_URL", "https://api.deepseek.com"),
        )),
        "openai" => Arc::new(OpenAIProvider::new(
            api_key(provider_id, "OPENAI_API_KEY")?,
            env_or_default("OPENAI_BASE_URL", "https://api.openai.com/v1"),
        )),
        "anthropic" => Arc::new(AnthropicProvider::new(
            api_key(provider_id, "ANTHROPIC_API_KEY")?,
            env_or_default("ANTHROPIC_BASE_URL", "https://api.anthropic.com"),
        )),
        "google" => Arc::new(GoogleProvider::new(
            api_key(provider_id, "GOOGLE_API_KEY")?,
            env_or_default(
                "GOOGLE_BASE_URL",
                "https://generativelanguage.googleapis.com/v1beta",
            ),
        )),
        "openrouter" => Arc::new(OpenRouterProvider::new(
            api_key(provider_id, "OPENROUTER_API_KEY")?,
            env_or_default("OPENROUTER_BASE_URL", "https://openrouter.ai/api/v1"),
        )),
        "qwen" => Arc::new(QwenProvider::new(
            api_key(provider_id, "QWEN_API_KEY")?,
            env_or_default(
                "QWEN_BASE_URL",
                "https://dashscope.aliyuncs.com/compatible-mode/v1",
            ),
        )),
        "glm" => Arc::new(GLMProvider::new(
            api_key(provider_id, "GLM_API_KEY")?,
            env_or_default("GLM_BASE_URL", "https://open.bigmodel.cn/api/paas/v4"),
        )),
        "minimax" => Arc::new(MiniMaxProvider::new(
            api_key(provider_id, "MINIMAX_API_KEY")?,
            env_or_default("MINIMAX_BASE_URL", "https://api.minimax.chat/v1"),
        )),
        "volcengine" => Arc::new(VolcengineProvider::new(
            api_key(provider_id, "VOLCENGINE_API_KEY")?,
            env_or_default(
                "VOLCENGINE_BASE_URL",
                "https://ark.cn-beijing.volces.com/api/v3",
            ),
        )),
        other => bail!(
            "unknown provider '{}'. Supported: mock, ollama, deepseek, openai, anthropic, google, openrouter, qwen, glm, minimax, volcengine",
            other
        ),
    };
    Ok(provider)
}

pub fn default_system_prompt() -> &'static str {
    "\
You are CatCode, an AI coding harness for real software projects.

Core contract:
- Understand the repository before changing code. Inspect relevant files, tests, and existing conventions first.
- Make the smallest correct change that solves the user's task. Preserve unrelated user changes.
- Use tools deliberately: read before edit, prefer precise patches, and keep tool output summarized.
- After code changes, run the most relevant verification command you can reasonably run.
- If verification fails, diagnose the failure, attempt a focused fix when safe, and report remaining blockers clearly.
- Keep users oriented with concise progress, changed files, and verification results.
- For broad or risky tasks, plan first, then execute in reviewable steps."
}

pub fn load_project_rules(project_dir: &Path) -> String {
    std::fs::read_to_string(project_dir.join("AGENTS.md")).unwrap_or_default()
}

pub fn project_dir_or_current(project_dir: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    project_dir
        .map(Ok)
        .unwrap_or_else(std::env::current_dir)
        .context("failed to resolve project directory")
}

fn api_key(provider_id: &str, env_var: &str) -> anyhow::Result<String> {
    std::env::var(env_var)
        .or_else(|_| std::env::var("CATCODE_API_KEY"))
        .with_context(|| {
            format!(
                "provider '{}' requires {} or CATCODE_API_KEY to be set",
                provider_id, env_var
            )
        })
}

fn env_or_default(env_var: &str, default: &str) -> String {
    std::env::var(env_var).unwrap_or_else(|_| default.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_options_default() {
        let options = AgentRuntimeOptions::default();
        assert!(options.provider_id.is_none());
        assert!(options.model_id.is_none());
        assert!(options.system_prompt.contains("CatCode"));
        assert!(options.system_prompt.contains("coding harness"));
        assert!(options.system_prompt.contains("verification"));
        assert!(options.auto_repair);
    }

    #[test]
    fn test_merge_agent_results_combines_repair_usage() {
        let mut initial = AgentLoopResult {
            response: "initial".to_string(),
            total_usage: Default::default(),
            turns_used: 2,
            hit_max_turns: false,
            messages: Vec::new(),
            auto_plan: None,
            self_healed: 0,
            sub_agents_spawned: 1,
            models_used: vec!["model-a".to_string()],
        };
        initial.total_usage.input_tokens = 10;
        let mut repair = AgentLoopResult {
            response: "repair".to_string(),
            total_usage: Default::default(),
            turns_used: 1,
            hit_max_turns: true,
            messages: Vec::new(),
            auto_plan: Some("plan".to_string()),
            self_healed: 2,
            sub_agents_spawned: 3,
            models_used: vec!["model-a".to_string(), "model-b".to_string()],
        };
        repair.total_usage.output_tokens = 5;

        let merged = merge_agent_results(initial, repair);

        assert_eq!(merged.response, "repair");
        assert_eq!(merged.total_usage.input_tokens, 10);
        assert_eq!(merged.total_usage.output_tokens, 5);
        assert_eq!(merged.turns_used, 3);
        assert!(merged.hit_max_turns);
        assert_eq!(merged.self_healed, 3);
        assert_eq!(merged.sub_agents_spawned, 4);
        assert_eq!(
            merged.models_used,
            vec!["model-a".to_string(), "model-b".to_string()]
        );
    }

    #[test]
    fn test_build_mock_provider() {
        let provider = build_provider("mock").unwrap();
        assert_eq!(provider.id(), "mock");
    }

    #[test]
    fn test_load_project_rules_missing() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert_eq!(load_project_rules(tmp.path()), "");
    }

    #[tokio::test]
    async fn test_runtime_run_once_mock() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("catcode.toml"),
            r#"
[daemon]
host = "127.0.0.1"
port = 7070
auto_start = true
max_concurrent_sessions = 5
checkpoint_interval_turns = 10

[defaults]
provider = "mock"
model = "mock-model"
sandbox = false

[budget]
session_limit_tokens = 500000
per_request_limit_tokens = 50000
warning_threshold = 0.80

[context]
compression_enabled = true
dedup_tool_outputs = true
max_file_content_tokens = 8000

[observability]
log_level = "info"
log_format = "text"
"#,
        )
        .unwrap();

        let result = AgentRuntime::new()
            .run_once("hello", tmp.path(), AgentRuntimeOptions::default())
            .await
            .unwrap();
        assert_eq!(result.turns_used, 1);
        assert!(result.response.contains("Mock provider response"));
    }
}
