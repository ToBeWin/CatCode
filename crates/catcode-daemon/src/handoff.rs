//! Final handoff gate for coding-agent runs.
//!
//! This module combines the three signals a user needs before trusting a
//! coding turn: changed files, local review findings, and verification result.

use crate::{
    CodeReview, DiffSummary, ReviewSeverity, VerificationRunResult, build_harness_plan,
    capture_git_snapshot, review_workspace_changes, run_auto_verification,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffReport {
    pub project_dir: String,
    pub task_summary: String,
    pub changes: DiffSummary,
    pub review: CodeReview,
    pub verification: Option<VerificationRunResult>,
    pub ready: bool,
    pub blockers: Vec<String>,
    pub recommendations: Vec<String>,
}

pub async fn run_handoff_report(project_dir: &Path, task: &str) -> anyhow::Result<HandoffReport> {
    let plan = build_harness_plan(project_dir, task);
    let Some(snapshot) = capture_git_snapshot(project_dir).await else {
        anyhow::bail!("failed to read git status for {}", project_dir.display());
    };
    let changes = DiffSummary::from_snapshot(&snapshot);
    let review = review_workspace_changes(project_dir).await?;
    let verification = if changes.is_empty() {
        None
    } else {
        run_auto_verification(project_dir, &plan.verification).await
    };

    let mut blockers = Vec::new();
    let mut recommendations = Vec::new();

    if changes.is_empty() {
        recommendations.push("No working tree changes detected.".to_string());
    }

    let error_count = review
        .findings
        .iter()
        .filter(|finding| finding.severity == ReviewSeverity::Error)
        .count();
    if error_count > 0 {
        blockers.push(format!("{error_count} error-level review finding(s)"));
    }

    let warning_count = review
        .findings
        .iter()
        .filter(|finding| finding.severity == ReviewSeverity::Warning)
        .count();
    if warning_count > 0 {
        recommendations.push(format!("{warning_count} warning-level review finding(s)"));
    }

    match verification.as_ref() {
        Some(result) if result.success => {
            recommendations.push(format!("Verification passed: {}", result.command));
        }
        Some(result) => {
            blockers.push(format!(
                "Verification failed: {}",
                result.actionable_summary()
            ));
            if let Some(diagnostic) = result.diagnostic() {
                recommendations.push(format!("Verification diagnostic: {}", diagnostic.summary));
                for suggestion in diagnostic.suggestions {
                    recommendations.push(suggestion);
                }
            }
            if let Some(plan) = result.repair_plan() {
                recommendations.push(format!(
                    "Repair verification command: {}",
                    plan.verification_command
                ));
                for file in plan.files_to_inspect {
                    recommendations.push(format!("Inspect failing file: {file}"));
                }
            }
        }
        None if !changes.is_empty() => {
            recommendations.push(
                "No auto-runnable verification command was executed for these changes.".to_string(),
            );
        }
        None => {}
    }

    let ready = blockers.is_empty();
    Ok(HandoffReport {
        project_dir: project_dir.display().to_string(),
        task_summary: plan.task_summary,
        changes,
        review,
        verification,
        ready,
        blockers,
        recommendations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_handoff_report_for_clean_repo() {
        let tmp = tempfile::TempDir::new().unwrap();
        tokio::process::Command::new("git")
            .arg("init")
            .current_dir(tmp.path())
            .output()
            .await
            .unwrap();

        let report = run_handoff_report(tmp.path(), "inspect").await.unwrap();

        assert!(report.ready);
        assert!(report.changes.changed_files.is_empty());
        assert!(
            report
                .recommendations
                .iter()
                .any(|item| item.contains("No working tree changes"))
        );
    }

    #[tokio::test]
    async fn test_handoff_report_reports_warning_without_blocking() {
        let tmp = tempfile::TempDir::new().unwrap();
        tokio::process::Command::new("git")
            .arg("init")
            .current_dir(tmp.path())
            .output()
            .await
            .unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("src/lib.rs"),
            "pub fn inspect(value: i32) {\n    dbg!(value);\n}\n",
        )
        .unwrap();

        let report = run_handoff_report(tmp.path(), "inspect warning")
            .await
            .unwrap();

        assert!(report.ready);
        assert!(report.blockers.is_empty());
        assert!(report.review.findings.iter().any(|finding| {
            finding.severity == ReviewSeverity::Warning
                && finding.title.contains("Debug print statement")
        }));
        assert!(
            report
                .recommendations
                .iter()
                .any(|item| item.contains("warning-level review finding"))
        );
    }

    #[tokio::test]
    async fn test_handoff_report_blocks_on_review_error() {
        let tmp = tempfile::TempDir::new().unwrap();
        tokio::process::Command::new("git")
            .arg("init")
            .current_dir(tmp.path())
            .output()
            .await
            .unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("src/lib.rs"),
            "pub const API_KEY: &str = \"sk-test-1234567890abcdef\";\n",
        )
        .unwrap();

        let report = run_handoff_report(tmp.path(), "inspect secret")
            .await
            .unwrap();

        assert!(!report.ready);
        assert!(report.review.findings.iter().any(|finding| {
            finding.severity == ReviewSeverity::Error && finding.title.contains("hardcoded secret")
        }));
        assert!(
            report
                .blockers
                .iter()
                .any(|item| item.contains("error-level review finding"))
        );
    }

    #[tokio::test]
    async fn test_handoff_report_blocks_on_verification_failure() {
        let tmp = tempfile::TempDir::new().unwrap();
        tokio::process::Command::new("git")
            .arg("init")
            .current_dir(tmp.path())
            .output()
            .await
            .unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"handoff_failure_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(tmp.path().join("src/lib.rs"), "pub fn broken() {\n").unwrap();

        let report = run_handoff_report(tmp.path(), "verify broken rust")
            .await
            .unwrap();

        assert!(!report.ready);
        assert!(report.verification.as_ref().is_some_and(|run| !run.success));
        assert!(
            report
                .blockers
                .iter()
                .any(|item| item.contains("Verification failed"))
        );
    }
}
