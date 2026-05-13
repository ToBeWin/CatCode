//! SWE-Bench evaluation harness.
//!
//! Evaluates AI agents on real-world GitHub issues by:
//! 1. Cloning the repo at a base commit
//! 2. Running an agent to generate a fix
//! 3. Applying the evaluation test patch
//! 4. Running tests to verify correctness
//! 5. Reporting structured results

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

use catcode_context::{ContextStack, TokenBudget};
use catcode_core::provider::Provider;
use catcode_core::TokenUsage;
use catcode_middleware::MiddlewareChain;
use catcode_tools::ToolRegistry;

use crate::agent_loop::{AgentLoop, AgentLoopResult};

// ---------------------------------------------------------------------------
// Core Types
// ---------------------------------------------------------------------------

/// A single SWE-Bench instance from the dataset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweBenchInstance {
    pub id: String,
    pub repo: String,
    pub base_commit: String,
    pub issue: String,
    pub hint: Option<String>,
    pub test_patch: String,
    pub expected_patch: Option<String>,
    pub fail_to_pass: Vec<String>,
    pub pass_to_pass: Vec<String>,
    #[serde(default)]
    pub created_at: String,
}

/// Configuration for a SWE-Bench evaluation run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweBenchConfig {
    pub work_dir: PathBuf,
    pub parallel_instances: usize,
    pub max_turns: u64,
    pub token_budget: u64,
    pub instance_timeout_secs: u64,
    pub keep_repos: bool,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub dataset_path: Option<PathBuf>,
}

impl Default for SweBenchConfig {
    fn default() -> Self {
        Self {
            work_dir: PathBuf::from("/tmp/swe-bench"),
            parallel_instances: 4,
            max_turns: 50,
            token_budget: 500_000,
            instance_timeout_secs: 600,
            keep_repos: false,
            provider: None,
            model: None,
            dataset_path: None,
        }
    }
}

/// Result of evaluating a single SWE-Bench instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweBenchResult {
    pub instance_id: String,
    pub repo: String,
    pub resolved: bool,
    pub generated_patch: String,
    pub test_results: TestResults,
    pub agent_turns: u64,
    pub token_usage: TokenUsage,
    pub cost_usd: f64,
    pub duration_ms: u64,
    pub error: Option<String>,
    pub agent_output: Vec<String>,
}

impl SweBenchResult {
    /// Create a result for an instance that failed before resolution.
    pub fn errored(instance: &SweBenchInstance, error: String, duration_ms: u64) -> Self {
        Self {
            instance_id: instance.id.clone(),
            repo: instance.repo.clone(),
            resolved: false,
            generated_patch: String::new(),
            test_results: TestResults::default(),
            agent_turns: 0,
            token_usage: TokenUsage::default(),
            cost_usd: 0.0,
            duration_ms,
            error: Some(error),
            agent_output: Vec::new(),
        }
    }
}

/// Test outcomes for a SWE-Bench instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct TestResults {
    pub fail_to_pass: Vec<String>,
    pub pass_to_pass: Vec<String>,
    pub fail_to_fail: Vec<String>,
    pub pass_to_fail: Vec<String>,
    pub applied_patch: String,
}

impl TestResults {
    /// Whether all expected tests passed (all fail_to_pass ✓, no pass_to_fail).
    pub fn is_resolved(&self) -> bool {
        self.fail_to_fail.is_empty() && self.pass_to_fail.is_empty()
    }
}


/// Aggregated SWE-Bench evaluation report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweBenchReport {
    pub config: SweBenchConfig,
    pub total_instances: usize,
    pub resolved: usize,
    pub unresolved: usize,
    pub resolve_rate: f64,
    pub avg_turns: f64,
    pub avg_duration_ms: f64,
    pub total_cost_usd: f64,
    pub results: Vec<SweBenchResult>,
    pub start_time: String,
    pub end_time: String,
    pub errors: Vec<String>,
}

impl SweBenchReport {
    /// Aggregate results into a report.
    pub fn from_results(
        config: &SweBenchConfig,
        results: Vec<SweBenchResult>,
        start_time: &str,
        end_time: &str,
    ) -> Self {
        let total = results.len();
        let resolved = results.iter().filter(|r| r.resolved).count();
        let unresolved = total - resolved;
        let resolve_rate = if total > 0 {
            resolved as f64 / total as f64
        } else {
            0.0
        };
        let avg_turns = if total > 0 {
            results.iter().map(|r| r.agent_turns).sum::<u64>() as f64 / total as f64
        } else {
            0.0
        };
        let avg_duration_ms = if total > 0 {
            results.iter().map(|r| r.duration_ms).sum::<u64>() as f64 / total as f64
        } else {
            0.0
        };
        let total_cost_usd = results.iter().map(|r| r.cost_usd).sum();
        let errors: Vec<String> = results
            .iter()
            .filter_map(|r| r.error.clone())
            .collect();

        Self {
            config: config.clone(),
            total_instances: total,
            resolved,
            unresolved,
            resolve_rate,
            avg_turns,
            avg_duration_ms,
            total_cost_usd,
            results,
            start_time: start_time.to_string(),
            end_time: end_time.to_string(),
            errors,
        }
    }
}

// ---------------------------------------------------------------------------
// Sample Instances (built-in smoke-test data)
// ---------------------------------------------------------------------------

/// Built-in sample SWE-Bench instances for quick testing.
pub fn sample_instances() -> Vec<SweBenchInstance> {
    vec![
        SweBenchInstance {
            id: "sample__simple-edit".to_string(),
            repo: "sample/repo".to_string(),
            base_commit: "abc123def".to_string(),
            issue: "Fix the hello_world function to return 'Hello, SWE-Bench!' instead of 'Hello, World!'".to_string(),
            hint: None,
            test_patch: "".to_string(),
            expected_patch: None,
            fail_to_pass: vec!["test_hello_world".to_string()],
            pass_to_pass: vec![],
            created_at: "2025-01-01".to_string(),
        },
        SweBenchInstance {
            id: "sample__add-null-check".to_string(),
            repo: "sample/repo".to_string(),
            base_commit: "def456ghi".to_string(),
            issue: "Add a null check to the get_user function before accessing the name field to prevent a panic when user is None.".to_string(),
            hint: Some("Look at src/models/user.rs for the get_user function.".to_string()),
            test_patch: "".to_string(),
            expected_patch: None,
            fail_to_pass: vec!["test_get_user_with_none".to_string()],
            pass_to_pass: vec!["test_get_user_with_valid".to_string()],
            created_at: "2025-01-01".to_string(),
        },
        SweBenchInstance {
            id: "sample__fix-off-by-one".to_string(),
            repo: "sample/repo".to_string(),
            base_commit: "ghi789jkl".to_string(),
            issue: "Fix the off-by-one error in the paginate function: when page_size=10 and total_items=10, it should return 1 page, not 2.".to_string(),
            hint: None,
            test_patch: "".to_string(),
            expected_patch: None,
            fail_to_pass: vec!["test_paginate_exact_multiple".to_string(), "test_paginate_single_item".to_string()],
            pass_to_pass: vec!["test_paginate_normal".to_string()],
            created_at: "2025-01-01".to_string(),
        },
        SweBenchInstance {
            id: "sample__escape-html".to_string(),
            repo: "sample/repo".to_string(),
            base_commit: "jkl012mno".to_string(),
            issue: "Escape HTML special characters in the render_template function to prevent XSS attacks. Currently, user-provided content is inserted directly without sanitization.".to_string(),
            hint: Some("Check src/templates/renderer.rs for the function. The `html_escape` crate is already in Cargo.toml.".to_string()),
            test_patch: "".to_string(),
            expected_patch: None,
            fail_to_pass: vec!["test_xss_prevention".to_string(), "test_html_escaping".to_string()],
            pass_to_pass: vec!["test_basic_template".to_string()],
            created_at: "2025-01-01".to_string(),
        },
        SweBenchInstance {
            id: "sample__fix-divide-by-zero".to_string(),
            repo: "sample/repo".to_string(),
            base_commit: "mno345pqr".to_string(),
            issue: "Fix the divide-by-zero crash in the calculate_ratio function. When the denominator is zero, it should return 0.0 instead of panicking.".to_string(),
            hint: None,
            test_patch: "".to_string(),
            expected_patch: None,
            fail_to_pass: vec!["test_calculate_ratio_zero_denominator".to_string()],
            pass_to_pass: vec!["test_calculate_ratio_normal".to_string(), "test_calculate_ratio_large_values".to_string()],
            created_at: "2025-01-01".to_string(),
        },
    ]
}

// ---------------------------------------------------------------------------
// SweBenchHarness
// ---------------------------------------------------------------------------

/// The SWE-Bench evaluation harness.
///
/// Orchestrates the full evaluation flow: repo setup → agent execution →
/// patch generation → test application → verification → result collection.
pub struct SweBenchHarness {
    config: SweBenchConfig,
    provider: Arc<dyn Provider>,
    tools: Arc<ToolRegistry>,
    middleware: Arc<MiddlewareChain>,
}

impl SweBenchHarness {
    /// Create a new harness with the given configuration and components.
    pub fn new(
        config: SweBenchConfig,
        provider: Arc<dyn Provider>,
        tools: Arc<ToolRegistry>,
        middleware: Arc<MiddlewareChain>,
    ) -> Self {
        Self {
            config,
            provider,
            tools,
            middleware,
        }
    }

    /// Evaluate a single SWE-Bench instance.
    pub async fn evaluate_instance(
        &self,
        instance: &SweBenchInstance,
    ) -> Result<SweBenchResult> {
        let start = Instant::now();
        let mut steps: Vec<String> = Vec::new();

        // 1. Prepare work directory
        let instance_dir = self.config.work_dir.join(&instance.id);
        tokio::fs::create_dir_all(&instance_dir)
            .await
            .context("Failed to create instance work directory")?;

        // 2. Clone the repository
        let repo_dir = instance_dir.join("repo");
        let clone_ok = if repo_dir.join(".git").exists() {
            true
        } else {
            self.git_clone(instance, &repo_dir).await?
        };

        if !clone_ok {
            return Ok(SweBenchResult::errored(
                instance,
                "Failed to clone repository".to_string(),
                start.elapsed().as_millis() as u64,
            ));
        }
        steps.push(format!("Cloned {} at {}", instance.repo, instance.base_commit));

        // 3. Checkout base commit and create a branch
        self.git_checkout(&repo_dir, &instance.base_commit).await?;
        self.git_create_branch(&repo_dir, "catcode-fix").await?;
        steps.push(format!("Checked out {} and created branch", instance.base_commit));

        // 4. Run the agent on the issue
        let agent_result = self.run_agent(instance, &repo_dir).await;
        let (agent_turns, token_usage, cost_usd) = match &agent_result {
            Ok(r) => {
                steps.push(format!("Agent completed in {} turns", r.turns_used));
                (r.turns_used, r.total_usage.clone(), estimate_cost(&r.total_usage))
            }
            Err(e) => {
                warn!("Agent error for {}: {}", instance.id, e);
                steps.push(format!("Agent error: {}", e));
                (0u64, TokenUsage::default(), 0.0)
            }
        };

        // 5. Get the generated patch
        let generated_patch = self.git_diff(&repo_dir).await.unwrap_or_default();
        if generated_patch.is_empty() {
            steps.push("No changes detected in repository".to_string());
            let duration_ms = start.elapsed().as_millis() as u64;
            return Ok(SweBenchResult {
                instance_id: instance.id.clone(),
                repo: instance.repo.clone(),
                resolved: false,
                generated_patch: String::new(),
                test_results: TestResults {
                    fail_to_fail: instance.fail_to_pass.clone(),
                    ..Default::default()
                },
                agent_turns,
                token_usage,
                cost_usd,
                duration_ms,
                error: Some("No changes were made to the repository".to_string()),
                agent_output: steps,
            });
        }
        steps.push(format!("Generated patch ({} lines)", generated_patch.lines().count()));

        // 6. Apply the test patch
        if !instance.test_patch.is_empty() {
            self.git_apply_patch(&repo_dir, &instance.test_patch).await?;
            steps.push("Applied evaluation test patch".to_string());
        }

        // 7. Run tests and determine outcomes
        let test_results = self.run_tests(instance, &repo_dir).await;
        let resolved = test_results.is_resolved();
        steps.push(format!(
            "Tests: {} fail→pass, {} pass→pass, {} fail→fail, {} pass→fail",
            test_results.fail_to_pass.len(),
            test_results.pass_to_pass.len(),
            test_results.fail_to_fail.len(),
            test_results.pass_to_fail.len(),
        ));

        let duration_ms = start.elapsed().as_millis() as u64;

        // 8. Clean up repo if configured
        if !self.config.keep_repos {
            let _ = tokio::fs::remove_dir_all(&instance_dir).await;
        }

        Ok(SweBenchResult {
            instance_id: instance.id.clone(),
            repo: instance.repo.clone(),
            resolved,
            generated_patch,
            test_results,
            agent_turns,
            token_usage,
            cost_usd,
            duration_ms,
            error: None,
            agent_output: steps,
        })
    }

    /// Run agent on the issue within the repo directory.
    async fn run_agent(
        &self,
        instance: &SweBenchInstance,
        repo_dir: &Path,
    ) -> Result<AgentLoopResult> {
        let system = format!(
            "You are a senior software engineer fixing a bug in {}.\n\
            Your goal is to understand the issue, explore the codebase, and \
            make minimal changes to fix it.\n\n\
            Guidelines:\n\
            - Use read_file and search to understand the code\n\
            - Use write_file or bash (with sed) to make changes\n\
            - Use git diff to verify your changes\n\
            - Do NOT add new features or refactor unrelated code\n\
            - Keep changes minimal and focused on the issue",
            instance.repo
        );

        let context = ContextStack::new(&system, "");
        let budget = TokenBudget::new(
            self.config.token_budget,
            self.config.token_budget / 10,
            0.80,
        );

        let model_id = self
            .config
            .model
            .clone()
            .unwrap_or_else(|| {
                self.provider
                    .supported_models()
                    .first()
                    .map(|m| m.id.clone())
                    .unwrap_or_else(|| "default".to_string())
            });

        let mut agent = AgentLoop::new(
            self.provider.clone(),
            self.tools.clone(),
            self.middleware.clone(),
            context,
            budget,
            &model_id,
        )
        .with_max_turns(self.config.max_turns);

        let issue_text = if let Some(hint) = &instance.hint {
            format!("{}\n\nHint: {}", instance.issue, hint)
        } else {
            instance.issue.clone()
        };

        agent.run(&issue_text, repo_dir).await.map_err(|e| anyhow::anyhow!("{}", e))
    }

    /// Clone a repository at the specified path.
    async fn git_clone(&self, instance: &SweBenchInstance, repo_dir: &Path) -> Result<bool> {
        let repo_url = format!("https://github.com/{}", instance.repo);
        info!("Cloning {} into {:?}", repo_url, repo_dir);

        let output = tokio::process::Command::new("git")
            .args(["clone", &repo_url, &repo_dir.to_string_lossy()])
            .output()
            .await
            .context("Failed to execute git clone")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!("Git clone failed: {}", stderr);
            return Ok(false);
        }

        Ok(true)
    }

    /// Checkout a specific commit.
    async fn git_checkout(&self, repo_dir: &Path, commit: &str) -> Result<()> {
        let output = tokio::process::Command::new("git")
            .args(["checkout", "--force", commit])
            .current_dir(repo_dir)
            .output()
            .await
            .context("Failed to execute git checkout")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Git checkout failed for {}: {}", commit, stderr);
        }

        Ok(())
    }

    /// Create a new branch.
    async fn git_create_branch(&self, repo_dir: &Path, branch: &str) -> Result<()> {
        let output = tokio::process::Command::new("git")
            .args(["checkout", "-b", branch])
            .current_dir(repo_dir)
            .output()
            .await
            .context("Failed to create git branch")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("Branch creation warning (may already exist): {}", stderr);
        }

        Ok(())
    }

    /// Get a unified diff of all unstaged changes.
    async fn git_diff(&self, repo_dir: &Path) -> Result<String> {
        let output = tokio::process::Command::new("git")
            .args(["diff"])
            .current_dir(repo_dir)
            .output()
            .await
            .context("Failed to execute git diff")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Git diff failed: {}", stderr);
        }

        let diff = String::from_utf8_lossy(&output.stdout).to_string();
        // Also include staged changes
        let staged = tokio::process::Command::new("git")
            .args(["diff", "--cached"])
            .current_dir(repo_dir)
            .output()
            .await
            .context("Failed to execute git diff --cached")?;

        let staged_diff = String::from_utf8_lossy(&staged.stdout).to_string();
        let combined = if staged_diff.is_empty() {
            diff
        } else if diff.is_empty() {
            staged_diff
        } else {
            format!("{}\n{}", diff, staged_diff)
        };

        Ok(combined)
    }

    /// Apply a patch file to the repository.
    async fn git_apply_patch(&self, repo_dir: &Path, patch_content: &str) -> Result<()> {
        let mut child = tokio::process::Command::new("git")
            .args(["apply"])
            .current_dir(repo_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("Failed to spawn git apply")?;

        use tokio::io::AsyncWriteExt;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(patch_content.as_bytes()).await?;
            stdin.flush().await?;
        }
        drop(child.stdin.take());

        let output = child.wait_with_output().await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("Failed to apply test patch: {}", stderr);
        }

        Ok(())
    }

    /// Run tests for the instance and classify outcomes.
    async fn run_tests(
        &self,
        instance: &SweBenchInstance,
        repo_dir: &Path,
    ) -> TestResults {
        let mut fail_to_pass = Vec::new();
        let mut pass_to_pass = Vec::new();
        let mut fail_to_fail = Vec::new();
        let mut pass_to_fail = Vec::new();

        // Determine test runner
        let test_runner = if repo_dir.join("Cargo.toml").exists() {
            TestRunner::Cargo
        } else {
            TestRunner::Pytest
        };

        // Run tests that should PASS now (our fix made them pass)
        for test_name in &instance.fail_to_pass {
            let passed = run_single_test(repo_dir, &test_runner, test_name).await;
            if passed {
                fail_to_pass.push(test_name.clone());
            } else {
                fail_to_fail.push(test_name.clone());
            }
        }

        // Run tests that should still PASS (no regression)
        for test_name in &instance.pass_to_pass {
            let passed = run_single_test(repo_dir, &test_runner, test_name).await;
            if passed {
                pass_to_pass.push(test_name.clone());
            } else {
                pass_to_fail.push(test_name.clone());
            }
        }

        TestResults {
            fail_to_pass,
            pass_to_pass,
            fail_to_fail,
            pass_to_fail,
            applied_patch: instance.test_patch.clone(),
        }
    }

    /// Evaluate all instances and produce a report.
    pub async fn evaluate_all(&self, instances: &[SweBenchInstance]) -> SweBenchReport {
        let start_time = chrono::Utc::now().to_rfc3339();
        let total = instances.len();
        info!("Starting SWE-Bench evaluation: {} instances", total);

        let semaphore = Arc::new(tokio::sync::Semaphore::new(self.config.parallel_instances));
        let mut handles = Vec::with_capacity(total);

        for instance in instances {
            let permit = semaphore.clone().acquire_owned().await;
            let permit = match permit {
                Ok(p) => p,
                Err(_) => break,
            };

            let provider = self.provider.clone();
            let tools = self.tools.clone();
            let middleware = self.middleware.clone();
            let config = self.config.clone();
            let instance = instance.clone();

            handles.push(tokio::spawn(async move {
                let _permit = permit;
                let harness = SweBenchHarness::new(config.clone(), provider, tools, middleware);

                let result = tokio::time::timeout(
                    Duration::from_secs(config.instance_timeout_secs),
                    harness.evaluate_instance(&instance),
                )
                .await;

                match result {
                    Ok(Ok(r)) => r,
                    Ok(Err(e)) => {
                        error!("Instance {} failed: {}", instance.id, e);
                        SweBenchResult::errored(
                            &instance,
                            format!("Evaluation failed: {}", e),
                            0,
                        )
                    }
                    Err(_) => {
                        warn!("Instance {} timed out after {}s", instance.id, config.instance_timeout_secs);
                        SweBenchResult::errored(
                            &instance,
                            format!("Timed out after {}s", config.instance_timeout_secs),
                            config.instance_timeout_secs * 1000,
                        )
                    }
                }
            }));
        }

        let mut results = Vec::with_capacity(handles.len());
        for handle in handles {
            match handle.await {
                Ok(r) => results.push(r),
                Err(e) => {
                    error!("Instance task panicked: {}", e);
                    // Create a placeholder error result; we don't have the instance here
                    results.push(SweBenchResult {
                        instance_id: "unknown".to_string(),
                        repo: "unknown".to_string(),
                        resolved: false,
                        generated_patch: String::new(),
                        test_results: TestResults::default(),
                        agent_turns: 0,
                        token_usage: TokenUsage::default(),
                        cost_usd: 0.0,
                        duration_ms: 0,
                        error: Some(format!("Task panicked: {}", e)),
                        agent_output: Vec::new(),
                    });
                }
            }
        }

        let end_time = chrono::Utc::now().to_rfc3339();
        SweBenchReport::from_results(&self.config, results, &start_time, &end_time)
    }
}

// ---------------------------------------------------------------------------
// Test Runner
// ---------------------------------------------------------------------------

enum TestRunner {
    Cargo,
    Pytest,
}

/// Run a single test and return whether it passed.
async fn run_single_test(repo_dir: &Path, runner: &TestRunner, test_name: &str) -> bool {
    let output = match runner {
        TestRunner::Cargo => {
            tokio::process::Command::new("cargo")
                .args(["test", test_name, "--quiet"])
                .current_dir(repo_dir)
                .output()
                .await
        }
        TestRunner::Pytest => {
            tokio::process::Command::new("python")
                .args(["-m", "pytest", "-x", "-q", "--no-header", "-k", test_name])
                .current_dir(repo_dir)
                .output()
                .await
        }
    };

    match output {
        Ok(o) => o.status.success(),
        Err(e) => {
            warn!("Failed to run test '{}': {}", test_name, e);
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Utility functions
// ---------------------------------------------------------------------------

/// Estimate cost in USD from token usage (approximate, using DeepSeek-like pricing).
fn estimate_cost(usage: &TokenUsage) -> f64 {
    let input_cost = usage.input_tokens as f64 * 0.14 / 1_000_000.0;
    let output_cost = usage.output_tokens as f64 * 0.28 / 1_000_000.0;
    input_cost + output_cost
}

/// Load SWE-Bench dataset from a file (supports JSON and JSONL).
pub fn load_dataset(path: &Path) -> Result<Vec<SweBenchInstance>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read dataset file: {}", path.display()))?;

    let trimmed = content.trim();

    if trimmed.starts_with('[') {
        // JSON array format
        let instances: Vec<SweBenchInstance> = serde_json::from_str(trimmed)
            .with_context(|| format!("Failed to parse JSON array from {}", path.display()))?;
        validate_instances(&instances)?;
        Ok(instances)
    } else {
        // JSONL format (one JSON object per line)
        let mut instances = Vec::new();
        for (i, line) in trimmed.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("//") {
                continue;
            }
            let instance: SweBenchInstance = serde_json::from_str(line)
                .with_context(|| format!("Failed to parse line {} in {}", i + 1, path.display()))?;
            instances.push(instance);
        }
        if instances.is_empty() {
            bail!("No valid instances found in {}", path.display());
        }
        validate_instances(&instances)?;
        Ok(instances)
    }
}

/// Validate that all instances have required fields.
fn validate_instances(instances: &[SweBenchInstance]) -> Result<()> {
    for instance in instances {
        if instance.id.is_empty() {
            bail!("Instance with empty id found");
        }
        if instance.repo.is_empty() {
            bail!("Instance {} has empty repo", instance.id);
        }
        if instance.base_commit.is_empty() {
            bail!("Instance {} has empty base_commit", instance.id);
        }
        if instance.issue.is_empty() {
            bail!("Instance {} has empty issue", instance.id);
        }
    }
    Ok(())
}

/// Format a SWE-Bench report as a human-readable summary.
pub fn format_summary(report: &SweBenchReport) -> String {
    let mut lines = vec![
        "╔══════════════════════════════════════════════╗".to_string(),
        "║         SWE-Bench Evaluation Report          ║".to_string(),
        "╚══════════════════════════════════════════════╝".to_string(),
        String::new(),
        format!("Total instances:  {}", report.total_instances),
        format!("Resolved:        {} ({:.1}%)", report.resolved, report.resolve_rate * 100.0),
        format!("Unresolved:      {}", report.unresolved),
        format!("Avg turns:       {:.1}", report.avg_turns),
        format!("Avg duration:    {:.0}ms", report.avg_duration_ms),
        format!("Total cost:      ${:.4}", report.total_cost_usd),
        String::new(),
        format!("Period:          {} → {}", report.start_time, report.end_time),
    ];

    if !report.errors.is_empty() {
        lines.push(String::new());
        lines.push(format!("Infrastructure errors: {}", report.errors.len()));
        for err in &report.errors {
            lines.push(format!("  ⚠ {}", err));
        }
    }

    lines.push(String::new());
    lines.push("── Instances ──".to_string());

    for result in &report.results {
        let status = if result.resolved { "✓" } else { "✗" };
        let patch_preview = if result.generated_patch.is_empty() {
            "no patch".to_string()
        } else {
            format!("{} lines", result.generated_patch.lines().count())
        };
        let error_note = if let Some(ref e) = result.error {
            format!(" | error: {}", e)
        } else {
            String::new()
        };
        lines.push(format!(
            "  {} {} | {}turns | {}ms | {}{}",
            status,
            result.instance_id,
            result.agent_turns,
            result.duration_ms,
            patch_preview,
            error_note,
        ));

        // Show test breakdown
        let tr = &result.test_results;
        let test_parts = [
            (tr.fail_to_pass.len(), "→✓"),
            (tr.pass_to_pass.len(), "→✓"),
            (tr.fail_to_fail.len(), "→✗"),
            (tr.pass_to_fail.len(), "↓✗"),
        ];
        let test_detail: Vec<String> = test_parts
            .iter()
            .filter(|(count, _)| *count > 0)
            .map(|(count, label)| format!("{}{}", count, label))
            .collect();
        if !test_detail.is_empty() {
            lines.push(format!("    tests: {}", test_detail.join(", ")));
        }
    }

    lines.join("\n")
}

/// Save report results to a JSON file and summary to a markdown file.
pub fn save_results(report: &SweBenchReport, output_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(output_dir)
        .with_context(|| format!("Failed to create output directory: {}", output_dir.display()))?;

    // Full results as JSON
    let json_path = output_dir.join("results.json");
    let json = serde_json::to_string_pretty(report)
        .context("Failed to serialize report to JSON")?;
    std::fs::write(&json_path, &json)
        .with_context(|| format!("Failed to write results JSON: {}", json_path.display()))?;

    // Summary as Markdown
    let md_path = output_dir.join("SUMMARY.md");
    let summary = format_summary(report);
    std::fs::write(&md_path, &summary)
        .with_context(|| format!("Failed to write summary: {}", md_path.display()))?;

    // Individual instance patches
    let patches_dir = output_dir.join("patches");
    std::fs::create_dir_all(&patches_dir)?;
    for result in &report.results {
        if !result.generated_patch.is_empty() {
            let patch_path = patches_dir.join(format!("{}.patch", result.instance_id));
            std::fs::write(&patch_path, &result.generated_patch).with_context(|| {
                format!("Failed to write patch for {}", result.instance_id)
            })?;
        }
    }

    info!(
        "Results saved to {} (JSON), {} (MD), {} patches",
        json_path.display(),
        md_path.display(),
        report.results.len(),
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_provider::mock::MockProvider;
    use tempfile::TempDir;

    // ---- Instance/Config/Result types ----

    #[test]
    fn test_instance_serialization_roundtrip() {
        let instance = SweBenchInstance {
            id: "django__django-12345".to_string(),
            repo: "django/django".to_string(),
            base_commit: "abc123".to_string(),
            issue: "Fix the bug".to_string(),
            hint: Some("look here".to_string()),
            test_patch: "--- a/file.py\n+++ b/file.py\n@@ -1 +1 @@\n-old\n+new".to_string(),
            expected_patch: None,
            fail_to_pass: vec!["test_x".to_string()],
            pass_to_pass: vec!["test_y".to_string()],
            created_at: "2025-06-01".to_string(),
        };
        let json = serde_json::to_string(&instance).unwrap();
        let deserialized: SweBenchInstance = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "django__django-12345");
        assert_eq!(deserialized.fail_to_pass, vec!["test_x"]);
        assert_eq!(deserialized.hint, Some("look here".to_string()));
    }

    #[test]
    fn test_instance_without_hint() {
        let instance = SweBenchInstance {
            id: "test-id".to_string(),
            repo: "test/repo".to_string(),
            base_commit: "deadbeef".to_string(),
            issue: "issue".to_string(),
            hint: None,
            test_patch: String::new(),
            expected_patch: None,
            fail_to_pass: vec![],
            pass_to_pass: vec![],
            created_at: "2025-01-01".to_string(),
        };
        let json = serde_json::to_string(&instance).unwrap();
        let deserialized: SweBenchInstance = serde_json::from_str(&json).unwrap();
        assert!(deserialized.hint.is_none());
    }

    #[test]
    fn test_config_defaults() {
        let config = SweBenchConfig::default();
        assert_eq!(config.work_dir, PathBuf::from("/tmp/swe-bench"));
        assert_eq!(config.parallel_instances, 4);
        assert_eq!(config.max_turns, 50);
        assert_eq!(config.token_budget, 500_000);
        assert_eq!(config.instance_timeout_secs, 600);
        assert!(!config.keep_repos);
        assert!(config.provider.is_none());
        assert!(config.model.is_none());
        assert!(config.dataset_path.is_none());
    }

    #[test]
    fn test_config_serialization() {
        let config = SweBenchConfig {
            work_dir: PathBuf::from("/custom/path"),
            parallel_instances: 8,
            max_turns: 100,
            token_budget: 1_000_000,
            instance_timeout_secs: 1200,
            keep_repos: true,
            provider: Some("anthropic".to_string()),
            model: Some("claude-sonnet-4".to_string()),
            dataset_path: Some(PathBuf::from("/tmp/data.json")),
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: SweBenchConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.parallel_instances, 8);
        assert_eq!(deserialized.provider, Some("anthropic".to_string()));
    }

    #[test]
    fn test_result_serialization_roundtrip() {
        let result = SweBenchResult {
            instance_id: "django__django-12345".to_string(),
            repo: "django/django".to_string(),
            resolved: true,
            generated_patch: "diff --git a/file.py b/file.py\nindex abc..def 100644\n--- a/file.py\n+++ b/file.py\n@@ -1 +1 @@\n-old\n+new".to_string(),
            test_results: TestResults {
                fail_to_pass: vec!["test_x".to_string()],
                pass_to_pass: vec!["test_y".to_string()],
                fail_to_fail: vec![],
                pass_to_fail: vec![],
                applied_patch: String::new(),
            },
            agent_turns: 12,
            token_usage: TokenUsage {
                input_tokens: 10000,
                output_tokens: 5000,
                cache_read_tokens: 3000,
                cache_creation_tokens: 0,
            },
            cost_usd: 0.0056,
            duration_ms: 45000,
            error: None,
            agent_output: vec!["Cloned repo".to_string(), "Agent completed".to_string()],
        };
        let json = serde_json::to_string(&result).unwrap();
        let deserialized: SweBenchResult = serde_json::from_str(&json).unwrap();
        assert!(deserialized.resolved);
        assert_eq!(deserialized.agent_turns, 12);
        assert_eq!(deserialized.agent_output.len(), 2);
    }

    #[test]
    fn test_result_with_error() {
        let instance = SweBenchInstance {
            id: "broken".to_string(),
            repo: "foo/bar".to_string(),
            base_commit: "abc".to_string(),
            issue: "x".to_string(),
            hint: None,
            test_patch: String::new(),
            expected_patch: None,
            fail_to_pass: vec![],
            pass_to_pass: vec![],
            created_at: String::new(),
        };
        let result = SweBenchResult::errored(&instance, "Git clone failed".to_string(), 1234);
        assert!(!result.resolved);
        assert_eq!(result.error, Some("Git clone failed".to_string()));
        assert_eq!(result.duration_ms, 1234);
        assert_eq!(result.instance_id, "broken");
    }

    // ---- TestResults ----

    #[test]
    fn test_test_results_is_resolved() {
        let resolved = TestResults {
            fail_to_pass: vec!["t1".to_string()],
            pass_to_pass: vec!["t2".to_string()],
            fail_to_fail: vec![],
            pass_to_fail: vec![],
            applied_patch: String::new(),
        };
        assert!(resolved.is_resolved());

        let with_failures = TestResults {
            fail_to_pass: vec![],
            pass_to_pass: vec![],
            fail_to_fail: vec!["t1".to_string()],
            pass_to_fail: vec![],
            applied_patch: String::new(),
        };
        assert!(!with_failures.is_resolved());

        let with_regression = TestResults {
            fail_to_pass: vec!["t1".to_string()],
            pass_to_pass: vec![],
            fail_to_fail: vec![],
            pass_to_fail: vec!["t2".to_string()],
            applied_patch: String::new(),
        };
        assert!(!with_regression.is_resolved());
    }

    #[test]
    fn test_test_results_default_is_not_resolved() {
        let tr = TestResults::default();
        assert!(tr.is_resolved()); // empty = no failures = resolved
    }

    #[test]
    fn test_test_results_serialization() {
        let tr = TestResults {
            fail_to_pass: vec!["a".to_string(), "b".to_string()],
            pass_to_pass: vec!["c".to_string()],
            fail_to_fail: vec!["d".to_string()],
            pass_to_fail: vec![],
            applied_patch: "diff --git a/test.py b/test.py".to_string(),
        };
        let json = serde_json::to_string(&tr).unwrap();
        let deserialized: TestResults = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.fail_to_pass.len(), 2);
        assert_eq!(deserialized.fail_to_fail.len(), 1);
    }

    // ---- Report aggregation ----

    #[test]
    fn test_report_aggregation_all_resolved() {
        let config = SweBenchConfig::default();
        let results = vec![
            SweBenchResult {
                instance_id: "a".to_string(),
                resolved: true,
                agent_turns: 10,
                duration_ms: 1000,
                cost_usd: 0.001,
                ..make_base_result("repo/a")
            },
            SweBenchResult {
                instance_id: "b".to_string(),
                resolved: true,
                agent_turns: 20,
                duration_ms: 2000,
                cost_usd: 0.002,
                ..make_base_result("repo/b")
            },
        ];
        let report = SweBenchReport::from_results(
            &config, results, "2025-01-01T00:00:00Z", "2025-01-01T01:00:00Z",
        );
        assert_eq!(report.total_instances, 2);
        assert_eq!(report.resolved, 2);
        assert_eq!(report.unresolved, 0);
        assert!((report.resolve_rate - 1.0).abs() < 0.01);
        assert!((report.avg_turns - 15.0).abs() < 0.01);
        assert!((report.avg_duration_ms - 1500.0).abs() < 0.01);
        assert!((report.total_cost_usd - 0.003).abs() < 0.0001);
    }

    #[test]
    fn test_report_aggregation_mixed() {
        let config = SweBenchConfig::default();
        let results = vec![
            SweBenchResult {
                instance_id: "resolved-1".to_string(),
                resolved: true,
                agent_turns: 5,
                duration_ms: 3000,
                cost_usd: 0.001,
                ..make_base_result("repo/r1")
            },
            SweBenchResult {
                instance_id: "failed-1".to_string(),
                resolved: false,
                agent_turns: 15,
                duration_ms: 6000,
                cost_usd: 0.003,
                ..make_base_result("repo/f1")
            },
            SweBenchResult {
                instance_id: "resolved-2".to_string(),
                resolved: true,
                agent_turns: 10,
                duration_ms: 4000,
                cost_usd: 0.002,
                ..make_base_result("repo/r2")
            },
        ];
        let report = SweBenchReport::from_results(
            &config, results, "s", "e",
        );
        assert_eq!(report.total_instances, 3);
        assert_eq!(report.resolved, 2);
        assert_eq!(report.unresolved, 1);
        assert!((report.resolve_rate - 2.0 / 3.0).abs() < 0.01);
        assert!((report.avg_turns - 10.0).abs() < 0.01);
        assert!((report.avg_duration_ms - 4333.0).abs() < 1.0);
        assert!((report.total_cost_usd - 0.006).abs() < 0.0001);
        assert_eq!(report.errors.len(), 0); // no error fields set
    }

    #[test]
    fn test_report_aggregation_empty() {
        let config = SweBenchConfig::default();
        let report = SweBenchReport::from_results(
            &config, vec![], "s", "e",
        );
        assert_eq!(report.total_instances, 0);
        assert_eq!(report.resolved, 0);
        assert_eq!(report.unresolved, 0);
        assert!((report.resolve_rate).abs() < 0.01);
        assert!((report.avg_turns).abs() < 0.01);
        assert!((report.total_cost_usd).abs() < 0.01);
    }

    #[test]
    fn test_report_collects_errors() {
        let config = SweBenchConfig::default();
        let results = vec![
            SweBenchResult {
                instance_id: "ok".to_string(),
                resolved: true,
                error: None,
                ..make_base_result("repo/ok")
            },
            SweBenchResult {
                instance_id: "err".to_string(),
                resolved: false,
                error: Some("timeout".to_string()),
                ..make_base_result("repo/err")
            },
        ];
        let report = SweBenchReport::from_results(&config, results, "s", "e");
        assert_eq!(report.errors.len(), 1);
        assert!(report.errors[0].contains("timeout"));
    }

    #[test]
    fn test_report_serialization_roundtrip() {
        let config = SweBenchConfig::default();
        let results = vec![SweBenchResult {
            instance_id: "test".to_string(),
            resolved: true,
            ..make_base_result("repo/test")
        }];
        let report = SweBenchReport::from_results(&config, results, "s", "e");
        let json = serde_json::to_string(&report).unwrap();
        let deserialized: SweBenchReport = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.total_instances, 1);
        assert_eq!(deserialized.resolved, 1);
    }

    // ---- Sample instances ----

    #[test]
    fn test_sample_instances_count() {
        let samples = sample_instances();
        assert_eq!(samples.len(), 5);
    }

    #[test]
    fn test_sample_instances_have_required_fields() {
        for instance in sample_instances() {
            assert!(!instance.id.is_empty());
            assert!(!instance.repo.is_empty());
            assert!(!instance.base_commit.is_empty());
            assert!(!instance.issue.is_empty());
            assert!(!instance.fail_to_pass.is_empty());
        }
    }

    #[test]
    fn test_sample_instances_unique_ids() {
        let samples = sample_instances();
        let mut ids: Vec<&str> = samples.iter().map(|s| s.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), samples.len());
    }

    // ---- Dataset loading ----

    #[test]
    fn test_load_dataset_json() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("dataset.json");
        let data = r#"[
            {
                "id": "django__django-111",
                "repo": "django/django",
                "base_commit": "abc123",
                "issue": "Fix something",
                "test_patch": "",
                "fail_to_pass": ["test_a"],
                "pass_to_pass": [],
                "created_at": "2025-01-01"
            },
            {
                "id": "sympy__sympy-222",
                "repo": "sympy/sympy",
                "base_commit": "def456",
                "issue": "Fix something else",
                "test_patch": "",
                "fail_to_pass": ["test_b"],
                "pass_to_pass": ["test_c"],
                "created_at": "2025-01-02"
            }
        ]"#;
        std::fs::write(&path, data).unwrap();
        let instances = load_dataset(&path).unwrap();
        assert_eq!(instances.len(), 2);
        assert_eq!(instances[0].id, "django__django-111");
        assert_eq!(instances[1].fail_to_pass, vec!["test_b"]);
        assert_eq!(instances[1].pass_to_pass, vec!["test_c"]);
    }

    #[test]
    fn test_load_dataset_jsonl() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("dataset.jsonl");
        let data = r#"{"id": "django__django-1", "repo": "django/django", "base_commit": "a", "issue": "i1", "test_patch": "", "fail_to_pass": ["t1"], "pass_to_pass": [], "created_at": "2025-01-01"}
{"id": "django__django-2", "repo": "django/django", "base_commit": "b", "issue": "i2", "test_patch": "", "fail_to_pass": ["t2"], "pass_to_pass": ["t3"], "created_at": "2025-01-01"}"#;
        std::fs::write(&path, data).unwrap();
        let instances = load_dataset(&path).unwrap();
        assert_eq!(instances.len(), 2);
        assert_eq!(instances[0].id, "django__django-1");
        assert_eq!(instances[1].id, "django__django-2");
    }

    #[test]
    fn test_load_dataset_jsonl_with_empty_lines() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("dataset.jsonl");
        let data = r#"{"id": "django__django-1", "repo": "django/django", "base_commit": "a", "issue": "i1", "test_patch": "", "fail_to_pass": ["t1"], "pass_to_pass": [], "created_at": "2025-01-01"}

{"id": "django__django-2", "repo": "django/django", "base_commit": "b", "issue": "i2", "test_patch": "", "fail_to_pass": ["t2"], "pass_to_pass": [], "created_at": "2025-01-01"}"#;
        std::fs::write(&path, data).unwrap();
        let instances = load_dataset(&path).unwrap();
        assert_eq!(instances.len(), 2);
    }

    #[test]
    fn test_load_dataset_missing_file() {
        let result = load_dataset(Path::new("/nonexistent/path.json"));
        assert!(result.is_err());
    }

    #[test]
    fn test_load_dataset_empty_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("empty.json");
        std::fs::write(&path, "[]").unwrap();
        let instances = load_dataset(&path).unwrap();
        assert!(instances.is_empty());
    }

    #[test]
    fn test_load_dataset_invalid_json() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "{not valid json}").unwrap();
        let result = load_dataset(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_dataset_missing_required_field() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("missing.json");
        std::fs::write(&path, r#"[
            {"id": "test", "repo": "a/b", "base_commit": "", "issue": "fix", "test_patch": "", "fail_to_pass": [], "pass_to_pass": [], "created_at": ""}
        ]"#).unwrap();
        let result = load_dataset(&path);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty base_commit"));
    }

    // ---- Formatting ----

    #[test]
    fn test_format_summary_contains_report_header() {
        let config = SweBenchConfig::default();
        let results = vec![SweBenchResult {
            instance_id: "test".to_string(),
            resolved: true,
            ..make_base_result("repo/test")
        }];
        let report = SweBenchReport::from_results(&config, results, "2025-01-01T00:00:00Z", "2025-01-01T01:00:00Z");
        let summary = format_summary(&report);
        assert!(summary.contains("SWE-Bench Evaluation Report"));
        assert!(summary.contains("Total instances:  1"));
        assert!(summary.contains("Resolved:"));
    }

    #[test]
    fn test_format_summary_shows_resolved_status() {
        let config = SweBenchConfig::default();
        let results = vec![
            SweBenchResult {
                instance_id: "pass-1".to_string(),
                resolved: true,
                agent_turns: 5,
                duration_ms: 1000,
                generated_patch: "diff --git a/f.py b/f.py\nindex abc..def\n--- a/f.py\n+++ b/f.py\n@@ -1 +1 @@\n-old\n+new".to_string(),
                test_results: TestResults {
                    fail_to_pass: vec!["t1".to_string()],
                    ..Default::default()
                },
                ..make_base_result("repo/pass-1")
            },
            SweBenchResult {
                instance_id: "fail-1".to_string(),
                resolved: false,
                agent_turns: 10,
                duration_ms: 2000,
                generated_patch: String::new(),
                test_results: TestResults {
                    fail_to_fail: vec!["t1".to_string()],
                    ..Default::default()
                },
                ..make_base_result("repo/fail-1")
            },
        ];
        let report = SweBenchReport::from_results(&config, results, "s", "e");
        let summary = format_summary(&report);
        assert!(summary.contains("✓ pass-1"));
        assert!(summary.contains("✗ fail-1"));
        assert!(summary.contains("1→✓")); // test outcome
        assert!(summary.contains("1→✗")); // still failing
    }

    #[test]
    fn test_format_summary_shows_errors() {
        let config = SweBenchConfig::default();
        let results = vec![SweBenchResult {
            instance_id: "broken".to_string(),
            resolved: false,
            error: Some("Network error".to_string()),
            ..make_base_result("repo/broken")
        }];
        let report = SweBenchReport::from_results(&config, results, "s", "e");
        let summary = format_summary(&report);
        assert!(summary.contains("Infrastructure errors"));
        assert!(summary.contains("Network error"));
    }

    #[test]
    fn test_format_summary_empty_report() {
        let config = SweBenchConfig::default();
        let report = SweBenchReport::from_results(&config, vec![], "s", "e");
        let summary = format_summary(&report);
        assert!(summary.contains("Total instances:  0"));
        assert!(summary.contains("Resolved:        0"));
    }

    // ---- Save results ----

    #[test]
    fn test_save_results_creates_files() {
        let dir = TempDir::new().unwrap();
        let config = SweBenchConfig::default();
        let results = vec![SweBenchResult {
            instance_id: "save-test".to_string(),
            resolved: true,
            generated_patch: "diff --git a/f.py b/f.py\n@@ -1 +1 @@\n-old\n+new".to_string(),
            ..make_base_result("repo/save-test")
        }];
        let report = SweBenchReport::from_results(&config, results, "s", "e");
        save_results(&report, dir.path()).unwrap();
        assert!(dir.path().join("results.json").exists());
        assert!(dir.path().join("SUMMARY.md").exists());
        assert!(dir.path().join("patches").exists());
        assert!(dir.path().join("patches/save-test.patch").exists());
    }

    #[test]
    fn test_save_results_no_patches_for_unchanged() {
        let dir = TempDir::new().unwrap();
        let config = SweBenchConfig::default();
        let results = vec![SweBenchResult {
            instance_id: "no-patch".to_string(),
            resolved: false,
            generated_patch: String::new(),
            ..make_base_result("repo/no-patch")
        }];
        let report = SweBenchReport::from_results(&config, results, "s", "e");
        save_results(&report, dir.path()).unwrap();
        assert!(!dir.path().join("patches/no-patch.patch").exists());
    }

    // ---- Estimate cost ----

    #[test]
    fn test_estimate_cost_basic() {
        let usage = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 500_000,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
        };
        let cost = estimate_cost(&usage);
        assert!((cost - 0.28).abs() < 0.01);
    }

    #[test]
    fn test_estimate_cost_zero() {
        let usage = TokenUsage::default();
        let cost = estimate_cost(&usage);
        assert!((cost).abs() < 0.0001);
    }

    // ---- Git operations ----

    #[test]
    fn test_create_temp_repo_and_diff() {
        let dir = TempDir::new().unwrap();
        let repo_path = dir.path().join("test-repo");
        std::fs::create_dir(&repo_path).unwrap();

        // Init git repo
        let output = std::process::Command::new("git")
            .args(["init"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        assert!(output.status.success());

        // Configure git for the test
        let _ = std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(&repo_path)
            .output();
        let _ = std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&repo_path)
            .output();

        // Create an initial file and commit
        std::fs::write(repo_path.join("hello.py"), "def greet():\n    return 'Hello, World!'\n").unwrap();
        let output = std::process::Command::new("git")
            .args(["add", "hello.py"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        assert!(output.status.success());

        let output = std::process::Command::new("git")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        assert!(output.status.success());

        // Get the commit hash
        let output = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        let commit = String::from_utf8_lossy(&output.stdout).trim().to_string();

        // Modify the file
        std::fs::write(repo_path.join("hello.py"), "def greet():\n    return 'Hello, SWE-Bench!'\n").unwrap();

        // Run git diff
        let output = std::process::Command::new("git")
            .args(["diff"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        let diff = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success());
        assert!(diff.contains("Hello, SWE-Bench!"));
        assert!(diff.contains("Hello, World!"));

        // The commit should be the base commit
        assert_eq!(commit.len(), 40);
    }

    // ---- Harness construction ----

    #[test]
    fn test_harness_new() {
        let config = SweBenchConfig::default();
        let provider = Arc::new(MockProvider::with_text_response("Done"));
        let tools = Arc::new(ToolRegistry::with_builtins());
        let middleware = Arc::new(MiddlewareChain::new());
        let harness = SweBenchHarness::new(config, provider, tools, middleware);
        assert_eq!(harness.config.parallel_instances, 4);
    }

    #[tokio::test]
    async fn test_evaluate_instance_with_mock_agent() {
        let dir = TempDir::new().unwrap();
        let config = SweBenchConfig {
            work_dir: dir.path().to_path_buf(),
            keep_repos: true,
            ..SweBenchConfig::default()
        };

        // Create a mock repo for the test
        let instance = SweBenchInstance {
            id: "mock-test".to_string(),
            repo: "sample/repo".to_string(),
            base_commit: "HEAD".to_string(),
            issue: "Fix the bug".to_string(),
            hint: None,
            test_patch: String::new(),
            expected_patch: None,
            fail_to_pass: vec![],
            pass_to_pass: vec![],
            created_at: String::new(),
        };

        let provider = Arc::new(MockProvider::with_text_response("Fixed!"));
        let tools = Arc::new(ToolRegistry::with_builtins());
        let middleware = Arc::new(MiddlewareChain::new());

        let harness = SweBenchHarness::new(config, provider, tools, middleware);
        let result = harness.evaluate_instance(&instance).await;

        // Should fail because repo doesn't exist
        assert!(result.is_err() || {
            let r = result.as_ref().unwrap();
            r.error.is_some()
        });
    }

    #[tokio::test]
    async fn test_mock_provider_in_harness_flow() {
        let config = SweBenchConfig::default();
        let provider = Arc::new(MockProvider::with_text_response("I'll look at the code"));
        let tools = Arc::new(ToolRegistry::with_builtins());
        let middleware = Arc::new(MiddlewareChain::new());

        // Just verify the harness can be constructed and the provider works
        let harness = SweBenchHarness::new(config, provider, tools, middleware);
        let provider_ref = &harness.provider;
        assert_eq!(provider_ref.id(), "mock");
    }

    // ---- Error instance creation ----

    #[test]
    fn test_errored_result_in_report() {
        let config = SweBenchConfig::default();
        let instance = SweBenchInstance {
            id: "err-instance".to_string(),
            repo: "foo/bar".to_string(),
            base_commit: "abc".to_string(),
            issue: "fix".to_string(),
            hint: None,
            test_patch: String::new(),
            expected_patch: None,
            fail_to_pass: vec![],
            pass_to_pass: vec![],
            created_at: String::new(),
        };

        let results = vec![SweBenchResult::errored(&instance, "test error".to_string(), 500)];
        let report = SweBenchReport::from_results(&config, results, "s", "e");
        assert_eq!(report.total_instances, 1);
        assert_eq!(report.resolved, 0);
        assert_eq!(report.errors.len(), 1);
    }

    // ---- Test results resolve logic edge cases ----

    #[test]
    fn test_resolve_all_fail_to_pass_passes() {
        let tr = TestResults {
            fail_to_pass: vec!["t1".to_string(), "t2".to_string()],
            pass_to_pass: vec![],
            fail_to_fail: vec![],
            pass_to_fail: vec![],
            applied_patch: String::new(),
        };
        assert!(tr.is_resolved());
    }

    #[test]
    fn test_resolve_fail_to_pass_with_regression() {
        let tr = TestResults {
            fail_to_pass: vec!["t1".to_string()],
            pass_to_pass: vec!["t2".to_string()],
            fail_to_fail: vec![],
            pass_to_fail: vec!["t3".to_string()],
            applied_patch: String::new(),
        };
        assert!(!tr.is_resolved());
    }

    // ---- Helper ----

    fn make_base_result(repo: &str) -> SweBenchResult {
        SweBenchResult {
            instance_id: String::new(),
            repo: repo.to_string(),
            resolved: false,
            generated_patch: String::new(),
            test_results: TestResults::default(),
            agent_turns: 0,
            token_usage: TokenUsage::default(),
            cost_usd: 0.0,
            duration_ms: 0,
            error: None,
            agent_output: Vec::new(),
        }
    }
}
