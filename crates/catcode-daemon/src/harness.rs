//! Coding harness planning and repository profiling.
//!
//! This module keeps the engineering workflow outside the model prompt as
//! structured state. The runtime can then provide the model with a compact,
//! deterministic harness plan for every run.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::process::Command;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessPhase {
    Intake,
    RepoScan,
    TaskPlan,
    ContextPack,
    Edit,
    DiffReview,
    Verification,
    Recovery,
    FinalReport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessStepStatus {
    Pending,
    Running,
    Done,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessStep {
    pub phase: HarnessPhase,
    pub status: HarnessStepStatus,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitSnapshot {
    pub porcelain: String,
}

impl GitSnapshot {
    pub fn changed_since(&self, before: &GitSnapshot) -> bool {
        self.porcelain != before.porcelain
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffSummary {
    pub changed_files: Vec<String>,
}

impl DiffSummary {
    pub fn from_snapshot(snapshot: &GitSnapshot) -> Self {
        let mut changed_files = snapshot
            .porcelain
            .lines()
            .filter_map(parse_porcelain_path)
            .collect::<Vec<_>>();
        dedup(&mut changed_files);
        Self { changed_files }
    }

    pub fn is_empty(&self) -> bool {
        self.changed_files.is_empty()
    }

    pub fn summary_line(&self) -> String {
        if self.changed_files.is_empty() {
            return "Working tree changed; review git diff before final handoff.".to_string();
        }

        let count = self.changed_files.len();
        let label = if count == 1 { "file" } else { "files" };
        let mut visible = self
            .changed_files
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>();
        if count > visible.len() {
            visible.push(format!("and {} more", count - visible.len()));
        }

        format!(
            "Working tree changed ({count} {label}): {}. Review git diff before final handoff.",
            visible.join(", ")
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoProfile {
    pub has_git: bool,
    pub language_stack: Vec<String>,
    pub package_managers: Vec<String>,
    pub test_commands: Vec<String>,
    pub important_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextFile {
    pub path: String,
    pub reason: String,
    pub snippet: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextPack {
    pub files: Vec<ContextFile>,
    pub hints: Vec<String>,
    pub total_chars: usize,
}

impl ContextPack {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn summary_line(&self) -> String {
        if self.files.is_empty() {
            return "No context files selected; inspect the repository before editing.".to_string();
        }

        let mut visible = self
            .files
            .iter()
            .take(5)
            .map(|file| file.path.clone())
            .collect::<Vec<_>>();
        if self.files.len() > visible.len() {
            visible.push(format!("and {} more", self.files.len() - visible.len()));
        }

        format!(
            "Packed {} context file(s): {}",
            self.files.len(),
            visible.join(", ")
        )
    }

    pub fn system_prompt_block(&self) -> String {
        let hints = self
            .hints
            .iter()
            .map(|hint| format!("- {hint}"))
            .collect::<Vec<_>>()
            .join("\n");

        if self.files.is_empty() {
            return format!("\n\nCatCode context pack:\nHints:\n{hints}");
        }

        let files = self
            .files
            .iter()
            .map(|file| {
                let truncated = if file.truncated { " (truncated)" } else { "" };
                format!(
                    "### {}{}\nReason: {}\n```text\n{}\n```",
                    file.path, truncated, file.reason, file.snippet
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        format!("\n\nCatCode context pack:\nHints:\n{hints}\n\nFiles:\n{files}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationCommand {
    pub command: String,
    pub reason: String,
    pub auto_run: bool,
}

impl VerificationCommand {
    fn describe(&self) -> String {
        let mode = if self.auto_run {
            "auto-runnable"
        } else {
            "manual/agent-confirmed"
        };
        format!("{} ({mode}; {})", self.command, self.reason)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationRunResult {
    pub command: String,
    pub success: bool,
    pub timed_out: bool,
    pub exit_code: Option<i32>,
    pub duration_ms: u128,
    pub stdout_tail: String,
    pub stderr_tail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationDiagnostic {
    pub summary: String,
    pub locations: Vec<String>,
    pub suggestions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationRepairPlan {
    pub summary: String,
    pub files_to_inspect: Vec<String>,
    pub steps: Vec<String>,
    pub verification_command: String,
}

impl VerificationRunResult {
    pub fn summary(&self) -> String {
        if self.timed_out {
            return format!("{} timed out after {}ms", self.command, self.duration_ms);
        }

        let status = if self.success { "passed" } else { "failed" };
        let code = self
            .exit_code
            .map(|code| format!("exit {code}"))
            .unwrap_or_else(|| "terminated by signal".to_string());
        let detail = if self.stderr_tail.trim().is_empty() {
            self.stdout_tail.trim()
        } else {
            self.stderr_tail.trim()
        };

        if detail.is_empty() {
            format!("{} {status} ({code})", self.command)
        } else {
            format!("{} {status} ({code}): {}", self.command, detail)
        }
    }

    pub fn actionable_summary(&self) -> String {
        let summary = self.summary();
        let Some(diagnostic) = self.diagnostic() else {
            return summary;
        };

        let locations = if diagnostic.locations.is_empty() {
            "no precise location".to_string()
        } else {
            diagnostic.locations.join(", ")
        };
        format!(
            "{summary}\nDiagnostic: {} ({locations})",
            diagnostic.summary
        )
    }

    pub fn diagnostic(&self) -> Option<VerificationDiagnostic> {
        if self.success {
            return None;
        }

        if self.timed_out {
            return Some(VerificationDiagnostic {
                summary: format!("{} timed out", self.command),
                locations: Vec::new(),
                suggestions: vec![
                    "Run a narrower verification command if possible.".to_string(),
                    "Inspect the most recent changed files before retrying.".to_string(),
                ],
            });
        }

        let text = if self.stderr_tail.trim().is_empty() {
            self.stdout_tail.as_str()
        } else {
            self.stderr_tail.as_str()
        };
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return None;
        }

        let summary = verification_failure_summary(trimmed);
        let mut locations = verification_failure_locations(trimmed);
        dedup(&mut locations);
        locations.truncate(8);

        Some(VerificationDiagnostic {
            summary,
            locations,
            suggestions: verification_failure_suggestions(&self.command),
        })
    }

    pub fn repair_plan(&self) -> Option<VerificationRepairPlan> {
        let diagnostic = self.diagnostic()?;
        let mut files_to_inspect = diagnostic
            .locations
            .iter()
            .filter_map(|location| location_file(location))
            .collect::<Vec<_>>();
        dedup(&mut files_to_inspect);
        files_to_inspect.truncate(6);

        let mut steps = Vec::new();
        if files_to_inspect.is_empty() {
            steps.push(
                "Inspect the most recent changed files and the verification output.".to_string(),
            );
        } else {
            steps.push(format!(
                "Read the first failing file before editing: {}.",
                files_to_inspect[0]
            ));
        }
        steps.push(format!(
            "Fix only the issue indicated by: {}",
            diagnostic.summary
        ));
        steps.push("Keep the repair patch minimal and preserve unrelated changes.".to_string());
        let verification_command = narrow_verification_command(&self.command, &files_to_inspect);
        steps.push(format!("Rerun verification: {verification_command}"));

        Some(VerificationRepairPlan {
            summary: format!("Repair plan for {}", self.command),
            files_to_inspect,
            steps,
            verification_command,
        })
    }
}

pub fn build_verification_repair_prompt(result: &VerificationRunResult) -> Option<String> {
    let diagnostic = result.diagnostic()?;
    let repair = result.repair_plan()?;
    let files = if repair.files_to_inspect.is_empty() {
        "none detected".to_string()
    } else {
        repair.files_to_inspect.join(", ")
    };
    let steps = repair
        .steps
        .iter()
        .map(|step| format!("- {step}"))
        .collect::<Vec<_>>()
        .join("\n");

    Some(format!(
        "Verification failed after your previous changes. Attempt exactly one focused repair pass.\n\
         Command: {}\n\
         Failure: {}\n\
         Locations: {}\n\
         Files to inspect first: {}\n\
         Repair steps:\n{}\n\
         After editing, expect the harness to rerun: {}\n\
         Constraints: inspect before editing, keep the patch minimal, preserve unrelated changes, and stop after this repair attempt.",
        result.command,
        diagnostic.summary,
        if diagnostic.locations.is_empty() {
            "none detected".to_string()
        } else {
            diagnostic.locations.join(", ")
        },
        files,
        steps,
        repair.verification_command
    ))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationPlan {
    pub commands: Vec<VerificationCommand>,
    pub safety_note: String,
}

impl VerificationPlan {
    pub fn primary(&self) -> Option<&VerificationCommand> {
        self.commands.first()
    }

    pub fn primary_auto_runnable(&self) -> Option<&VerificationCommand> {
        self.commands
            .iter()
            .find(|command| command.auto_run && can_auto_run_verification(&command.command))
    }

    pub fn summary(&self) -> String {
        self.primary()
            .map(VerificationCommand::describe)
            .unwrap_or_else(|| self.safety_note.clone())
    }

    fn prompt_block(&self) -> String {
        if self.commands.is_empty() {
            return self.safety_note.clone();
        }

        let commands = self
            .commands
            .iter()
            .map(|command| format!("- {}", command.describe()))
            .collect::<Vec<_>>()
            .join("\n");
        format!("{commands}\nSafety: {}", self.safety_note)
    }
}

pub async fn run_auto_verification(
    project_dir: &Path,
    plan: &VerificationPlan,
) -> Option<VerificationRunResult> {
    let command = plan.primary_auto_runnable()?;
    run_verification_command(project_dir, command, Duration::from_secs(120)).await
}

async fn run_verification_command(
    project_dir: &Path,
    command: &VerificationCommand,
    timeout: Duration,
) -> Option<VerificationRunResult> {
    if !command.auto_run || !can_auto_run_verification(&command.command) {
        return None;
    }

    let argv = split_command_args(&command.command)?;
    let program = argv.first()?;
    let started = Instant::now();
    let mut child = Command::new(program);
    child.args(&argv[1..]).current_dir(project_dir);

    let output = match tokio::time::timeout(timeout, child.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(_)) => return None,
        Err(_) => {
            return Some(VerificationRunResult {
                command: command.command.clone(),
                success: false,
                timed_out: true,
                exit_code: None,
                duration_ms: started.elapsed().as_millis(),
                stdout_tail: String::new(),
                stderr_tail: String::new(),
            });
        }
    };

    Some(VerificationRunResult {
        command: command.command.clone(),
        success: output.status.success(),
        timed_out: false,
        exit_code: output.status.code(),
        duration_ms: started.elapsed().as_millis(),
        stdout_tail: tail_lossy(&output.stdout, 1200),
        stderr_tail: tail_lossy(&output.stderr, 1200),
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessPlan {
    pub task_summary: String,
    pub phases: Vec<HarnessPhase>,
    pub repo: RepoProfile,
    pub verification: VerificationPlan,
    pub instructions: Vec<String>,
}

impl HarnessPlan {
    pub fn system_prompt_block(&self) -> String {
        let phases = self
            .phases
            .iter()
            .map(|phase| format!("{phase:?}"))
            .collect::<Vec<_>>()
            .join(" -> ");
        let stack = format_list(&self.repo.language_stack);
        let managers = format_list(&self.repo.package_managers);
        let files = format_list(&self.repo.important_files);
        let verification = self.verification.prompt_block();
        let instructions = self
            .instructions
            .iter()
            .map(|item| format!("- {item}"))
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            "\n\nCatCode harness plan:\nTask: {}\nPhases: {}\nRepo: git={} stack={} package_managers={} important_files={}\nSuggested verification: {}\nHarness instructions:\n{}",
            self.task_summary,
            phases,
            self.repo.has_git,
            stack,
            managers,
            files,
            verification,
            instructions
        )
    }

    pub fn status_line(&self) -> String {
        let stack = if self.repo.language_stack.is_empty() {
            "unknown stack".to_string()
        } else {
            self.repo.language_stack.join("+")
        };
        let verification = self.verification.summary();
        format!("Harness: {stack}; verify with {verification}")
    }

    pub fn startup_steps(&self) -> Vec<HarnessStep> {
        let stack = if self.repo.language_stack.is_empty() {
            "unknown stack".to_string()
        } else {
            self.repo.language_stack.join("+")
        };
        let flow = if self.phases.contains(&HarnessPhase::Edit) {
            "code-change flow: inspect, plan, edit, diff, verify, recover"
        } else {
            "read-only flow: inspect, pack context, report"
        };
        let verification = self.verification.summary();

        vec![
            HarnessStep {
                phase: HarnessPhase::RepoScan,
                status: HarnessStepStatus::Done,
                message: format!("Detected {stack}; git={}", self.repo.has_git),
            },
            HarnessStep {
                phase: HarnessPhase::TaskPlan,
                status: HarnessStepStatus::Done,
                message: flow.to_string(),
            },
            HarnessStep {
                phase: HarnessPhase::ContextPack,
                status: HarnessStepStatus::Done,
                message: format!("Injected harness plan; suggested verification: {verification}"),
            },
        ]
    }

    pub fn completion_steps(
        &self,
        before: Option<&GitSnapshot>,
        after: Option<&GitSnapshot>,
        run_succeeded: bool,
    ) -> Vec<HarnessStep> {
        let changed = before
            .zip(after)
            .map(|(before, after)| after.changed_since(before))
            .unwrap_or(false);
        let verification = self.verification.summary();
        let mut steps = Vec::new();

        if changed {
            let summary = after
                .map(DiffSummary::from_snapshot)
                .filter(|summary| !summary.is_empty())
                .map(|summary| summary.summary_line())
                .unwrap_or_else(|| {
                    "Working tree changed; review git diff before final handoff.".to_string()
                });
            steps.push(HarnessStep {
                phase: HarnessPhase::DiffReview,
                status: HarnessStepStatus::Done,
                message: summary,
            });
            steps.push(HarnessStep {
                phase: HarnessPhase::Verification,
                status: HarnessStepStatus::Pending,
                message: format!("Verification plan: {verification}"),
            });
        } else {
            steps.push(HarnessStep {
                phase: HarnessPhase::DiffReview,
                status: HarnessStepStatus::Skipped,
                message: "No working tree changes detected after this turn.".to_string(),
            });
            steps.push(HarnessStep {
                phase: HarnessPhase::Verification,
                status: HarnessStepStatus::Skipped,
                message: "No code changes detected; verification is optional.".to_string(),
            });
        }

        if run_succeeded {
            steps.push(HarnessStep {
                phase: HarnessPhase::FinalReport,
                status: HarnessStepStatus::Pending,
                message: "Summarize outcome, changed files, and verification status.".to_string(),
            });
        } else {
            steps.push(HarnessStep {
                phase: HarnessPhase::Recovery,
                status: HarnessStepStatus::Pending,
                message: "Agent run failed; inspect the error and prepare a recovery plan."
                    .to_string(),
            });
        }

        steps
    }
}

pub async fn capture_git_snapshot(project_dir: &Path) -> Option<GitSnapshot> {
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        Command::new("git")
            .arg("-C")
            .arg(project_dir)
            .arg("status")
            .arg("--porcelain=v1")
            .arg("-uall")
            .output(),
    )
    .await
    .ok()?
    .ok()?;

    if !output.status.success() {
        return None;
    }

    Some(GitSnapshot {
        porcelain: String::from_utf8_lossy(&output.stdout).to_string(),
    })
}

pub async fn build_context_pack(
    project_dir: &Path,
    user_message: &str,
    repo: &RepoProfile,
    snapshot: Option<&GitSnapshot>,
) -> ContextPack {
    let mut candidates = BTreeMap::<String, ContextCandidate>::new();
    let task_tokens = task_tokens(user_message);
    let code_change = likely_code_change(user_message);

    for file in &repo.important_files {
        add_context_candidate(
            &mut candidates,
            file,
            80,
            if file.ends_with(".md") {
                "repo guidance"
            } else {
                "repo manifest"
            },
        );
    }

    if let Some(snapshot) = snapshot {
        for file in DiffSummary::from_snapshot(snapshot).changed_files {
            add_context_candidate(&mut candidates, &file, 120, "currently changed");
        }
    }

    for file in list_tracked_files(project_dir).await {
        if !is_context_file(&file) {
            continue;
        }

        let file_lower = file.to_lowercase();
        let mut matched = false;
        for token in &task_tokens {
            if file_lower.contains(token) {
                add_context_candidate(&mut candidates, &file, 70, "task keyword match");
                matched = true;
            }
        }

        if code_change && is_test_like_path(&file_lower) {
            add_context_candidate(&mut candidates, &file, 18, "nearby test surface");
        }
        if !matched && is_entrypoint_path(&file_lower) {
            add_context_candidate(&mut candidates, &file, 12, "common entrypoint");
        }
    }

    let mut ranked = candidates.into_values().collect::<Vec<_>>();
    ranked.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.path.len().cmp(&b.path.len()))
            .then_with(|| a.path.cmp(&b.path))
    });

    let mut files = Vec::new();
    let mut total_chars = 0usize;
    for candidate in ranked.into_iter().take(10) {
        if total_chars >= 6000 {
            break;
        }

        let Some((snippet, truncated)) = read_context_snippet(project_dir, &candidate.path).await
        else {
            continue;
        };
        total_chars += snippet.chars().count();
        files.push(ContextFile {
            path: candidate.path,
            reason: candidate.reasons.into_iter().collect::<Vec<_>>().join(", "),
            snippet,
            truncated,
        });
    }

    let mut hints = vec![
        "Use this context pack as a starting map, not as a substitute for reading files before editing.".to_string(),
        "Prefer files marked currently changed or task keyword match when choosing the first tool calls.".to_string(),
    ];
    if files.is_empty() {
        hints.push(
            "No high-confidence context files were selected; inspect manifests, source roots, and tests before editing."
                .to_string(),
        );
    }

    ContextPack {
        files,
        hints,
        total_chars,
    }
}

pub fn build_harness_plan(project_dir: &Path, user_message: &str) -> HarnessPlan {
    let repo = profile_repo(project_dir);
    let verification = build_verification_plan(&repo);
    let phases = if likely_code_change(user_message) {
        vec![
            HarnessPhase::Intake,
            HarnessPhase::RepoScan,
            HarnessPhase::TaskPlan,
            HarnessPhase::ContextPack,
            HarnessPhase::Edit,
            HarnessPhase::DiffReview,
            HarnessPhase::Verification,
            HarnessPhase::Recovery,
            HarnessPhase::FinalReport,
        ]
    } else {
        vec![
            HarnessPhase::Intake,
            HarnessPhase::RepoScan,
            HarnessPhase::ContextPack,
            HarnessPhase::FinalReport,
        ]
    };

    HarnessPlan {
        task_summary: summarize_task(user_message),
        phases,
        repo,
        verification,
        instructions: vec![
            "Inspect the relevant repository files before editing.".to_string(),
            "Prefer focused, reviewable changes over broad rewrites.".to_string(),
            "Use the verification plan when code changes are made; only auto-run commands marked auto-runnable.".to_string(),
            "If verification fails, diagnose the failure and attempt one scoped recovery pass."
                .to_string(),
            "Report changed files, verification status, and remaining blockers.".to_string(),
        ],
    }
}

fn build_verification_plan(repo: &RepoProfile) -> VerificationPlan {
    let commands = repo
        .test_commands
        .iter()
        .map(|command| VerificationCommand {
            command: command.clone(),
            reason: verification_reason(command),
            auto_run: can_auto_run_verification(command),
        })
        .collect::<Vec<_>>();

    let safety_note = if commands.is_empty() {
        "No repository-specific verification command detected; use targeted manual verification."
            .to_string()
    } else {
        "The daemon describes verification commands but does not execute them by default; auto-run is reserved for allowlisted, non-mutating commands.".to_string()
    };

    VerificationPlan {
        commands,
        safety_note,
    }
}

fn verification_reason(command: &str) -> String {
    match command {
        "cargo check --workspace" => "fast Rust compile/type check".to_string(),
        "cargo test --workspace" => "workspace Rust test suite".to_string(),
        "go test ./..." => "Go package test suite".to_string(),
        "pytest" => "Python test suite".to_string(),
        "npm test" => "project package test script".to_string(),
        "make test" => "project Makefile test target".to_string(),
        _ => "repository-detected verification command".to_string(),
    }
}

fn can_auto_run_verification(command: &str) -> bool {
    if command
        .chars()
        .any(|ch| matches!(ch, ';' | '&' | '|' | '`' | '$' | '>' | '<' | '\n'))
    {
        return false;
    }

    matches!(
        command,
        "cargo check --workspace" | "cargo test --workspace" | "go test ./..." | "pytest"
    )
}

fn split_command_args(command: &str) -> Option<Vec<String>> {
    if command.trim().is_empty() || !can_auto_run_verification(command) {
        return None;
    }

    Some(
        command
            .split_whitespace()
            .map(|part| part.to_string())
            .collect(),
    )
}

fn tail_lossy(bytes: &[u8], max_chars: usize) -> String {
    let text = String::from_utf8_lossy(bytes);
    let char_count = text.chars().count();
    if char_count <= max_chars {
        return text.to_string();
    }

    text.chars().skip(char_count - max_chars).collect()
}

fn verification_failure_summary(text: &str) -> String {
    let mut fallback = None;
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if line.starts_with("Checking ")
            || line.starts_with("Compiling ")
            || line.starts_with("Finished ")
        {
            continue;
        }
        if fallback.is_none() {
            fallback = Some(line.to_string());
        }

        let lower = line.to_lowercase();
        if lower.starts_with("error")
            || lower.starts_with("failed")
            || lower.contains("error:")
            || lower.contains("failed")
        {
            return line.to_string();
        }
    }

    fallback.unwrap_or_else(|| "Verification failed; inspect command output.".to_string())
}

fn verification_failure_locations(text: &str) -> Vec<String> {
    let mut locations = Vec::new();
    for line in text.lines().map(str::trim) {
        if let Some(location) = line.strip_prefix("-->").and_then(first_location_token) {
            locations.push(location);
            continue;
        }

        for token in line.split_whitespace().filter_map(first_location_token) {
            locations.push(token);
        }
    }
    locations
}

fn first_location_token(text: &str) -> Option<String> {
    let token = text
        .split_whitespace()
        .next()?
        .trim_matches(|ch: char| matches!(ch, ',' | ';' | ':' | ')' | '(' | '[' | ']'));
    if looks_like_location(token) {
        Some(token.to_string())
    } else {
        None
    }
}

fn looks_like_location(token: &str) -> bool {
    let mut parts = token.split(':');
    let Some(path) = parts.next() else {
        return false;
    };
    if !(path.contains('/') || path.contains('.')) {
        return false;
    }

    parts.any(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit()))
}

fn verification_failure_suggestions(command: &str) -> Vec<String> {
    if command.starts_with("cargo ") {
        vec![
            "Open the listed Rust source location before editing.".to_string(),
            "Prefer the smallest compile-fix patch, then rerun cargo check.".to_string(),
        ]
    } else if command == "pytest" {
        vec![
            "Open the first failing test or traceback location before editing.".to_string(),
            "Rerun the narrow failing test when possible.".to_string(),
        ]
    } else if command.starts_with("go test") {
        vec![
            "Open the listed Go file and inspect the failing package boundary.".to_string(),
            "Rerun go test for the narrow package when possible.".to_string(),
        ]
    } else {
        vec![
            "Inspect the listed location before editing.".to_string(),
            "Rerun the same verification command after a focused fix.".to_string(),
        ]
    }
}

fn location_file(location: &str) -> Option<String> {
    let path = location.split(':').next()?.trim();
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

fn narrow_verification_command(command: &str, files_to_inspect: &[String]) -> String {
    if command == "pytest"
        && let Some(first) = files_to_inspect
            .iter()
            .find(|file| file.starts_with("tests/") && file.ends_with(".py"))
    {
        return format!("pytest {first} -q");
    }

    if command.starts_with("go test")
        && let Some(first) = files_to_inspect.iter().find(|file| file.ends_with(".go"))
        && let Some(package_dir) = first.rsplit_once('/').map(|(dir, _)| dir)
    {
        return format!("go test ./{}", package_dir.trim_start_matches("./"));
    }

    command.to_string()
}

fn parse_porcelain_path(line: &str) -> Option<String> {
    if line.len() < 4 {
        return None;
    }

    let path = line.get(3..)?.trim();
    if path.is_empty() {
        return None;
    }

    let path = path
        .split(" -> ")
        .last()
        .unwrap_or(path)
        .trim()
        .trim_matches('"')
        .to_string();

    if path.is_empty() { None } else { Some(path) }
}

fn profile_repo(project_dir: &Path) -> RepoProfile {
    let mut profile = RepoProfile {
        has_git: project_dir.join(".git").exists(),
        language_stack: Vec::new(),
        package_managers: Vec::new(),
        test_commands: Vec::new(),
        important_files: Vec::new(),
    };

    detect_file(project_dir, &mut profile, "Cargo.toml", |profile| {
        profile.language_stack.push("Rust".to_string());
        profile.package_managers.push("cargo".to_string());
        profile
            .test_commands
            .push("cargo check --workspace".to_string());
        profile
            .test_commands
            .push("cargo test --workspace".to_string());
    });
    detect_file(project_dir, &mut profile, "package.json", |profile| {
        profile
            .language_stack
            .push("JavaScript/TypeScript".to_string());
        profile
            .package_managers
            .push(node_package_manager(project_dir));
        profile.test_commands.push("npm test".to_string());
    });
    detect_file(project_dir, &mut profile, "pyproject.toml", |profile| {
        profile.language_stack.push("Python".to_string());
        profile.package_managers.push("python".to_string());
        profile.test_commands.push("pytest".to_string());
    });
    detect_file(project_dir, &mut profile, "go.mod", |profile| {
        profile.language_stack.push("Go".to_string());
        profile.package_managers.push("go".to_string());
        profile.test_commands.push("go test ./...".to_string());
    });
    detect_file(project_dir, &mut profile, "Makefile", |profile| {
        profile.package_managers.push("make".to_string());
        profile.test_commands.push("make test".to_string());
    });

    for candidate in [
        "AGENTS.md",
        "CLAUDE.md",
        "README.md",
        "Cargo.toml",
        "package.json",
        "pyproject.toml",
        "go.mod",
        "Makefile",
    ] {
        if project_dir.join(candidate).exists() {
            profile.important_files.push(candidate.to_string());
        }
    }

    dedup(&mut profile.language_stack);
    dedup(&mut profile.package_managers);
    dedup(&mut profile.test_commands);
    profile
}

fn detect_file(
    project_dir: &Path,
    profile: &mut RepoProfile,
    relative: &str,
    apply: impl FnOnce(&mut RepoProfile),
) {
    if project_dir.join(relative).exists() {
        apply(profile);
    }
}

fn node_package_manager(project_dir: &Path) -> String {
    if project_dir.join("pnpm-lock.yaml").exists() {
        "pnpm".to_string()
    } else if project_dir.join("yarn.lock").exists() {
        "yarn".to_string()
    } else {
        "npm".to_string()
    }
}

#[derive(Debug)]
struct ContextCandidate {
    path: String,
    score: i32,
    reasons: BTreeSet<String>,
}

fn add_context_candidate(
    candidates: &mut BTreeMap<String, ContextCandidate>,
    path: &str,
    score: i32,
    reason: &str,
) {
    if !is_context_file(path) {
        return;
    }

    let entry = candidates
        .entry(path.to_string())
        .or_insert_with(|| ContextCandidate {
            path: path.to_string(),
            score: 0,
            reasons: BTreeSet::new(),
        });
    entry.score += score;
    entry.reasons.insert(reason.to_string());
}

async fn list_tracked_files(project_dir: &Path) -> Vec<String> {
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        Command::new("git")
            .arg("-C")
            .arg(project_dir)
            .arg("ls-files")
            .output(),
    )
    .await;

    let Ok(Ok(output)) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect()
}

async fn read_context_snippet(project_dir: &Path, relative: &str) -> Option<(String, bool)> {
    let path = project_dir.join(relative);
    let metadata = tokio::fs::metadata(&path).await.ok()?;
    if !metadata.is_file() || metadata.len() > 256 * 1024 {
        return None;
    }

    let text = tokio::fs::read_to_string(&path).await.ok()?;
    let normalized = text
        .lines()
        .take(80)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    if normalized.is_empty() {
        return None;
    }

    let max_chars = 900usize;
    let char_count = normalized.chars().count();
    if char_count <= max_chars {
        Some((normalized, false))
    } else {
        Some((normalized.chars().take(max_chars).collect(), true))
    }
}

fn task_tokens(user_message: &str) -> Vec<String> {
    let mut tokens = user_message
        .split(|ch: char| !ch.is_alphanumeric() && ch != '_' && ch != '-')
        .map(|part| part.trim_matches(|ch: char| ch == '_' || ch == '-'))
        .filter(|part| part.chars().count() >= 3)
        .map(|part| part.to_lowercase())
        .filter(|part| !TASK_STOP_WORDS.contains(&part.as_str()))
        .collect::<Vec<_>>();
    dedup(&mut tokens);
    tokens
}

const TASK_STOP_WORDS: &[&str] = &[
    "the",
    "and",
    "for",
    "with",
    "this",
    "that",
    "继续",
    "推进",
    "优化",
    "完成度",
];

fn is_context_file(path: &str) -> bool {
    let lower = path.to_lowercase();
    if lower
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return false;
    }

    if lower.contains("/target/")
        || lower.starts_with("target/")
        || lower.contains("/node_modules/")
        || lower.starts_with("node_modules/")
        || lower.contains("/.git/")
        || lower.starts_with(".git/")
        || lower.contains("/dist/")
        || lower.starts_with("dist/")
    {
        return false;
    }

    let Some(name) = lower.rsplit('/').next() else {
        return false;
    };
    if matches!(
        name,
        "cargo.lock" | "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock"
    ) {
        return false;
    }

    matches!(
        lower.rsplit('.').next(),
        Some(
            "rs" | "toml"
                | "md"
                | "json"
                | "ts"
                | "tsx"
                | "js"
                | "jsx"
                | "py"
                | "go"
                | "yaml"
                | "yml"
        )
    ) || matches!(
        name,
        "makefile" | "dockerfile" | "agents.md" | "claude.md" | "readme.md"
    )
}

fn is_test_like_path(path: &str) -> bool {
    path.contains("/tests/")
        || path.starts_with("tests/")
        || path.contains("_test.")
        || path.contains(".test.")
        || path.contains(".spec.")
}

fn is_entrypoint_path(path: &str) -> bool {
    matches!(
        path,
        "src/lib.rs" | "src/main.rs" | "lib.rs" | "main.rs" | "app.ts" | "app.tsx" | "index.ts"
    ) || path.ends_with("/src/lib.rs")
        || path.ends_with("/src/main.rs")
}

fn likely_code_change(user_message: &str) -> bool {
    let text = user_message.to_lowercase();
    [
        "fix",
        "implement",
        "add",
        "change",
        "continue",
        "enhance",
        "improve",
        "update",
        "refactor",
        "test",
        "bug",
        "修",
        "实现",
        "添加",
        "优化",
        "推进",
        "改",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

fn summarize_task(user_message: &str) -> String {
    let normalized = user_message
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.chars().count() <= 160 {
        normalized
    } else {
        let mut summary = normalized.chars().take(157).collect::<String>();
        summary.push_str("...");
        summary
    }
}

fn format_list(items: &[String]) -> String {
    if items.is_empty() {
        "none".to_string()
    } else {
        items.join(", ")
    }
}

fn dedup(items: &mut Vec<String>) {
    let mut seen = std::collections::BTreeSet::new();
    items.retain(|item| seen.insert(item.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_rust_repo() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        std::fs::write(tmp.path().join("README.md"), "# Test\n").unwrap();

        let profile = profile_repo(tmp.path());

        assert!(profile.language_stack.contains(&"Rust".to_string()));
        assert!(profile.package_managers.contains(&"cargo".to_string()));
        assert!(
            profile
                .test_commands
                .contains(&"cargo check --workspace".to_string())
        );
        assert!(profile.important_files.contains(&"Cargo.toml".to_string()));
    }

    #[test]
    fn test_harness_plan_for_code_change_includes_verification() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("Cargo.toml"), "[workspace]\n").unwrap();

        let plan = build_harness_plan(tmp.path(), "fix the parser bug");

        assert!(plan.phases.contains(&HarnessPhase::Edit));
        assert!(plan.phases.contains(&HarnessPhase::Verification));
        assert!(
            plan.system_prompt_block()
                .contains("cargo check --workspace")
        );
        assert_eq!(
            plan.verification.primary().unwrap().command,
            "cargo check --workspace"
        );
        assert!(plan.verification.primary().unwrap().auto_run);
    }

    #[test]
    fn test_harness_plan_treats_improve_as_code_change() {
        let tmp = tempfile::TempDir::new().unwrap();

        let plan = build_harness_plan(tmp.path(), "improve coding harness visibility");

        assert!(plan.phases.contains(&HarnessPhase::Edit));
        assert!(plan.phases.contains(&HarnessPhase::DiffReview));
    }

    #[test]
    fn test_harness_plan_for_read_only_task_skips_edit() {
        let tmp = tempfile::TempDir::new().unwrap();

        let plan = build_harness_plan(tmp.path(), "explain the architecture");

        assert!(!plan.phases.contains(&HarnessPhase::Edit));
        assert!(plan.phases.contains(&HarnessPhase::RepoScan));
    }

    #[test]
    fn test_task_summary_is_truncated() {
        let long = "a".repeat(220);

        let summary = summarize_task(&long);

        assert!(summary.ends_with("..."));
        assert_eq!(summary.chars().count(), 160);
    }

    #[test]
    fn test_status_line_uses_first_verification_command() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("Cargo.toml"), "[workspace]\n").unwrap();

        let plan = build_harness_plan(tmp.path(), "add tests");

        assert!(plan.status_line().contains("Rust"));
        assert!(plan.status_line().contains("cargo check --workspace"));
    }

    #[test]
    fn test_startup_steps_are_structured() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("Cargo.toml"), "[workspace]\n").unwrap();

        let plan = build_harness_plan(tmp.path(), "fix parser");
        let steps = plan.startup_steps();

        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].phase, HarnessPhase::RepoScan);
        assert_eq!(steps[0].status, HarnessStepStatus::Done);
        assert!(steps[0].message.contains("Rust"));
        assert!(steps[2].message.contains("cargo check --workspace"));
        assert!(steps[2].message.contains("auto-runnable"));
    }

    #[test]
    fn test_completion_steps_detect_changes() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        let plan = build_harness_plan(tmp.path(), "fix parser");
        let before = GitSnapshot {
            porcelain: String::new(),
        };
        let after = GitSnapshot {
            porcelain: " M src/lib.rs\n".to_string(),
        };

        let steps = plan.completion_steps(Some(&before), Some(&after), true);

        assert_eq!(steps[0].phase, HarnessPhase::DiffReview);
        assert_eq!(steps[0].status, HarnessStepStatus::Done);
        assert!(steps[0].message.contains("src/lib.rs"));
        assert_eq!(steps[1].phase, HarnessPhase::Verification);
        assert_eq!(steps[1].status, HarnessStepStatus::Pending);
        assert!(steps[1].message.contains("cargo check --workspace"));
        assert!(steps[1].message.contains("auto-runnable"));
    }

    #[test]
    fn test_diff_summary_parses_porcelain_paths() {
        let snapshot = GitSnapshot {
            porcelain: " M crates/foo.rs\n?? new.rs\nR  old.rs -> moved.rs\n".to_string(),
        };

        let summary = DiffSummary::from_snapshot(&snapshot);

        assert_eq!(
            summary.changed_files,
            vec![
                "crates/foo.rs".to_string(),
                "new.rs".to_string(),
                "moved.rs".to_string()
            ]
        );
        assert!(summary.summary_line().contains("3 files"));
        assert!(summary.summary_line().contains("moved.rs"));
    }

    #[test]
    fn test_verification_plan_marks_package_scripts_manual() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("package.json"),
            r#"{"scripts":{"test":"vitest"}}"#,
        )
        .unwrap();

        let plan = build_harness_plan(tmp.path(), "fix ui tests");
        let command = plan.verification.primary().unwrap();

        assert_eq!(command.command, "npm test");
        assert!(!command.auto_run);
        assert!(command.reason.contains("package test script"));
        assert!(
            plan.verification
                .summary()
                .contains("manual/agent-confirmed")
        );
    }

    #[test]
    fn test_verification_plan_rejects_shell_metacharacters_for_auto_run() {
        assert!(!can_auto_run_verification(
            "cargo test --workspace; rm -rf target"
        ));
        assert!(!can_auto_run_verification("pytest | tee test.log"));
        assert!(can_auto_run_verification("pytest"));
    }

    #[test]
    fn test_split_command_args_rechecks_allowlist() {
        assert_eq!(
            split_command_args("cargo check --workspace").unwrap(),
            vec!["cargo", "check", "--workspace"]
        );
        assert!(split_command_args("npm test").is_none());
        assert!(split_command_args("cargo check --workspace && echo ok").is_none());
    }

    #[test]
    fn test_verification_run_summary() {
        let result = VerificationRunResult {
            command: "cargo check --workspace".to_string(),
            success: false,
            timed_out: false,
            exit_code: Some(101),
            duration_ms: 42,
            stdout_tail: String::new(),
            stderr_tail: "compile failed".to_string(),
        };

        assert!(result.summary().contains("failed"));
        assert!(result.summary().contains("compile failed"));
    }

    #[test]
    fn test_verification_diagnostic_extracts_rust_location() {
        let result = VerificationRunResult {
            command: "cargo check --workspace".to_string(),
            success: false,
            timed_out: false,
            exit_code: Some(101),
            duration_ms: 42,
            stdout_tail: String::new(),
            stderr_tail: "error[E0425]: cannot find value `missing` in this scope\n --> src/lib.rs:2:5\n  |\n2 | missing\n  | ^^^^^^^\nerror: could not compile `demo`".to_string(),
        };

        let diagnostic = result.diagnostic().unwrap();

        assert!(diagnostic.summary.contains("error[E0425]"));
        assert_eq!(diagnostic.locations, vec!["src/lib.rs:2:5"]);
        assert!(
            diagnostic
                .suggestions
                .iter()
                .any(|item| item.contains("Rust source location"))
        );
        let repair = result.repair_plan().unwrap();
        assert_eq!(repair.files_to_inspect, vec!["src/lib.rs"]);
        assert_eq!(repair.verification_command, "cargo check --workspace");
        assert!(
            repair
                .steps
                .iter()
                .any(|step| step.contains("Read the first failing file"))
        );
        let prompt = build_verification_repair_prompt(&result).unwrap();
        assert!(prompt.contains("Attempt exactly one focused repair pass"));
        assert!(prompt.contains("src/lib.rs"));
        assert!(prompt.contains("cargo check --workspace"));
        assert!(result.actionable_summary().contains("Diagnostic"));
    }

    #[test]
    fn test_verification_diagnostic_handles_pytest_location() {
        let result = VerificationRunResult {
            command: "pytest".to_string(),
            success: false,
            timed_out: false,
            exit_code: Some(1),
            duration_ms: 42,
            stdout_tail:
                "tests/test_parser.py:12: AssertionError\nFAILED tests/test_parser.py::test_parse"
                    .to_string(),
            stderr_tail: String::new(),
        };

        let diagnostic = result.diagnostic().unwrap();

        assert!(diagnostic.summary.contains("tests/test_parser.py"));
        assert_eq!(diagnostic.locations, vec!["tests/test_parser.py:12"]);
        assert!(
            diagnostic
                .suggestions
                .iter()
                .any(|item| item.contains("failing test"))
        );
        let repair = result.repair_plan().unwrap();
        assert_eq!(repair.files_to_inspect, vec!["tests/test_parser.py"]);
        assert_eq!(
            repair.verification_command,
            "pytest tests/test_parser.py -q"
        );
    }

    #[tokio::test]
    async fn test_run_auto_verification_rust_workspace() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("Cargo.toml"),
            r#"
[package]
name = "catcode-harness-test"
version = "0.1.0"
edition = "2024"
"#,
        )
        .unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("src/lib.rs"),
            "pub fn ok() -> bool { true }\n",
        )
        .unwrap();
        let plan = build_harness_plan(tmp.path(), "fix rust code");

        let result = run_auto_verification(tmp.path(), &plan.verification)
            .await
            .unwrap();

        assert_eq!(result.command, "cargo check --workspace");
        assert!(result.success, "{}", result.summary());
    }

    #[tokio::test]
    async fn test_context_pack_selects_task_matched_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        tokio::process::Command::new("git")
            .arg("init")
            .current_dir(tmp.path())
            .output()
            .await
            .unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("AGENTS.md"), "# Rules\nRead before edit.\n").unwrap();
        std::fs::write(
            tmp.path().join("src/parser.rs"),
            "pub fn parse(input: &str) -> bool { !input.is_empty() }\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("src/runtime.rs"),
            "pub fn run() -> bool { true }\n",
        )
        .unwrap();
        tokio::process::Command::new("git")
            .arg("add")
            .arg(".")
            .current_dir(tmp.path())
            .output()
            .await
            .unwrap();

        let plan = build_harness_plan(tmp.path(), "fix parser behavior");
        let pack = build_context_pack(tmp.path(), "fix parser behavior", &plan.repo, None).await;

        assert!(pack.files.iter().any(|file| file.path == "AGENTS.md"));
        assert!(pack.files.iter().any(|file| {
            file.path == "src/parser.rs" && file.reason.contains("task keyword match")
        }));
        assert!(pack.system_prompt_block().contains("parse(input"));
    }

    #[tokio::test]
    async fn test_context_pack_prioritizes_changed_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("src/new_feature.rs"),
            "pub fn new_feature() -> bool { true }\n",
        )
        .unwrap();
        let snapshot = GitSnapshot {
            porcelain: "?? src/new_feature.rs\n".to_string(),
        };
        let repo = RepoProfile {
            has_git: true,
            language_stack: vec!["Rust".to_string()],
            package_managers: vec!["cargo".to_string()],
            test_commands: vec!["cargo check --workspace".to_string()],
            important_files: Vec::new(),
        };

        let pack = build_context_pack(tmp.path(), "continue feature", &repo, Some(&snapshot)).await;

        assert_eq!(pack.files[0].path, "src/new_feature.rs");
        assert!(pack.files[0].reason.contains("currently changed"));
        assert!(pack.summary_line().contains("src/new_feature.rs"));
    }

    #[test]
    fn test_completion_steps_without_changes_skips_verification() {
        let tmp = tempfile::TempDir::new().unwrap();
        let plan = build_harness_plan(tmp.path(), "explain architecture");
        let before = GitSnapshot {
            porcelain: String::new(),
        };
        let after = GitSnapshot {
            porcelain: String::new(),
        };

        let steps = plan.completion_steps(Some(&before), Some(&after), true);

        assert_eq!(steps[0].status, HarnessStepStatus::Skipped);
        assert_eq!(steps[1].status, HarnessStepStatus::Skipped);
    }

    #[test]
    fn test_completion_steps_failed_run_requests_recovery() {
        let tmp = tempfile::TempDir::new().unwrap();
        let plan = build_harness_plan(tmp.path(), "fix parser");

        let steps = plan.completion_steps(None, None, false);

        assert_eq!(steps.last().unwrap().phase, HarnessPhase::Recovery);
        assert_eq!(steps.last().unwrap().status, HarnessStepStatus::Pending);
    }
}
