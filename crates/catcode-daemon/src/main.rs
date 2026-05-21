use async_trait::async_trait;
use catcode_daemon::{
    AgentRuntime, AgentRuntimeOptions, Config, Database, DiffSummary, build_harness_plan,
    capture_git_snapshot, load_config, review_workspace_changes, run_handoff_report,
};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::broadcast;

struct LocalMessageRunner {
    db: Database,
}

struct LocalHarnessPlanner;
struct LocalChangesProvider;
struct LocalReviewProvider;
struct LocalHandoffProvider;

#[async_trait]
impl catcode_api::MessageRunner for LocalMessageRunner {
    async fn run_message(
        &self,
        session: catcode_api::ApiSession,
        message: String,
    ) -> anyhow::Result<catcode_api::RunMessageResult> {
        let project_dir = PathBuf::from(&session.project_dir);
        let result = AgentRuntime::new()
            .run_once(
                &message,
                &project_dir,
                AgentRuntimeOptions {
                    provider_id: Some(session.provider_id),
                    model_id: Some(session.model_id),
                    session_id: Some(session.id),
                    audit_db: Some(self.db.clone()),
                    system_prompt: "You are CatCode, a concise coding agent served by the local daemon. Use tools when needed and keep responses focused.".to_string(),
                    ..Default::default()
                },
            )
            .await?;
        Ok(catcode_api::RunMessageResult {
            response: result.response,
            input_tokens: result.total_usage.input_tokens,
            output_tokens: result.total_usage.output_tokens,
            cache_tokens: result.total_usage.cache_read_tokens,
        })
    }
}

#[async_trait]
impl catcode_api::HarnessPlanner for LocalHarnessPlanner {
    async fn build_harness_plan(
        &self,
        project_dir: &Path,
        task: &str,
    ) -> anyhow::Result<catcode_api::ApiHarnessPlan> {
        let plan = build_harness_plan(project_dir, task);
        Ok(catcode_api::ApiHarnessPlan {
            task_summary: plan.task_summary,
            phases: plan
                .phases
                .into_iter()
                .map(|phase| format!("{phase:?}"))
                .collect(),
            repo: catcode_api::ApiRepoProfile {
                has_git: plan.repo.has_git,
                language_stack: plan.repo.language_stack,
                package_managers: plan.repo.package_managers,
                test_commands: plan.repo.test_commands,
                important_files: plan.repo.important_files,
            },
            verification: catcode_api::ApiVerificationPlan {
                commands: plan
                    .verification
                    .commands
                    .into_iter()
                    .map(|command| catcode_api::ApiVerificationCommand {
                        command: command.command,
                        reason: command.reason,
                        auto_run: command.auto_run,
                    })
                    .collect(),
                safety_note: plan.verification.safety_note,
            },
            instructions: plan.instructions,
        })
    }
}

#[async_trait]
impl catcode_api::WorkspaceChangesProvider for LocalChangesProvider {
    async fn workspace_changes(
        &self,
        project_dir: &Path,
    ) -> anyhow::Result<catcode_api::ApiWorkspaceChanges> {
        let Some(snapshot) = capture_git_snapshot(project_dir).await else {
            anyhow::bail!("failed to read git status for {}", project_dir.display());
        };
        let diff = DiffSummary::from_snapshot(&snapshot);
        let clean = diff.changed_files.is_empty();
        let summary = if clean {
            "Working tree clean.".to_string()
        } else {
            diff.summary_line()
        };

        Ok(catcode_api::ApiWorkspaceChanges {
            project_dir: project_dir.display().to_string(),
            clean,
            changed_files: diff.changed_files,
            summary,
        })
    }
}

#[async_trait]
impl catcode_api::CodeReviewProvider for LocalReviewProvider {
    async fn review_workspace(
        &self,
        project_dir: &Path,
    ) -> anyhow::Result<catcode_api::ApiCodeReview> {
        let review = review_workspace_changes(project_dir).await?;
        Ok(catcode_api::ApiCodeReview {
            title: review.title,
            summary: review.summary,
            files_reviewed: review.files_reviewed,
            findings: review
                .findings
                .into_iter()
                .map(|finding| catcode_api::ApiReviewFinding {
                    severity: format!("{:?}", finding.severity),
                    category: format!("{:?}", finding.category),
                    file: finding.file,
                    line: finding.line,
                    title: finding.title,
                    description: finding.description,
                    suggestion: finding.suggestion,
                })
                .collect(),
            positive_notes: review.positive_notes,
            overall_score: review.overall_score,
        })
    }
}

#[async_trait]
impl catcode_api::HandoffProvider for LocalHandoffProvider {
    async fn run_handoff(
        &self,
        project_dir: &Path,
        task: &str,
    ) -> anyhow::Result<catcode_api::ApiHandoffReport> {
        let report = run_handoff_report(project_dir, task).await?;
        let clean = report.changes.changed_files.is_empty();
        let changes_summary = if clean {
            "Working tree clean.".to_string()
        } else {
            report.changes.summary_line()
        };

        Ok(catcode_api::ApiHandoffReport {
            project_dir: report.project_dir.clone(),
            task_summary: report.task_summary,
            changes: catcode_api::ApiWorkspaceChanges {
                project_dir: report.project_dir,
                clean,
                changed_files: report.changes.changed_files,
                summary: changes_summary,
            },
            review: catcode_api::ApiCodeReview {
                title: report.review.title,
                summary: report.review.summary,
                files_reviewed: report.review.files_reviewed,
                findings: report
                    .review
                    .findings
                    .into_iter()
                    .map(|finding| catcode_api::ApiReviewFinding {
                        severity: format!("{:?}", finding.severity),
                        category: format!("{:?}", finding.category),
                        file: finding.file,
                        line: finding.line,
                        title: finding.title,
                        description: finding.description,
                        suggestion: finding.suggestion,
                    })
                    .collect(),
                positive_notes: report.review.positive_notes,
                overall_score: report.review.overall_score,
            },
            verification: report
                .verification
                .map(|result| catcode_api::ApiVerificationRunResult {
                    repair_plan: result.repair_plan().map(|plan| {
                        catcode_api::ApiVerificationRepairPlan {
                            summary: plan.summary,
                            files_to_inspect: plan.files_to_inspect,
                            steps: plan.steps,
                            verification_command: plan.verification_command,
                        }
                    }),
                    diagnostics: result.diagnostic().map(|diagnostic| {
                        catcode_api::ApiVerificationDiagnostic {
                            summary: diagnostic.summary,
                            locations: diagnostic.locations,
                            suggestions: diagnostic.suggestions,
                        }
                    }),
                    command: result.command,
                    success: result.success,
                    timed_out: result.timed_out,
                    exit_code: result.exit_code,
                    duration_ms: u64::try_from(result.duration_ms).unwrap_or(u64::MAX),
                    stdout_tail: result.stdout_tail,
                    stderr_tail: result.stderr_tail,
                }),
            ready: report.ready,
            blockers: report.blockers,
            recommendations: report.recommendations,
        })
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let project_dir = std::env::current_dir()?;
    let config = load_config(&project_dir)?;
    let db_path = Config::db_path(&project_dir);
    if let Some(parent) = db_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let db = Database::new(&db_path.to_string_lossy()).await?;
    let (tx, _rx) = broadcast::channel(100);

    let api_state = catcode_api::AppState::new(
        tx,
        catcode_api::auth::AuthConfig {
            mode: catcode_api::auth::AuthMode::LocalOnly,
            token: None,
        },
    )
    .with_store(Arc::new(db.clone()))
    .with_runner(Arc::new(LocalMessageRunner { db }))
    .with_harness_planner(Arc::new(LocalHarnessPlanner))
    .with_changes_provider(Arc::new(LocalChangesProvider))
    .with_review_provider(Arc::new(LocalReviewProvider))
    .with_handoff_provider(Arc::new(LocalHandoffProvider));

    let addr: SocketAddr = format!("{}:{}", config.daemon.host, config.daemon.port).parse()?;
    tracing::info!("CatCode daemon starting on {}", addr);

    let server = catcode_api::serve(addr, api_state);

    tokio::select! {
        result = server => {
            if let Err(e) = result {
                tracing::error!("Server error: {}", e);
            }
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Shutting down...");
        }
    }

    Ok(())
}
