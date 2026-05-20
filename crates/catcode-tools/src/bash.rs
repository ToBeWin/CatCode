use async_trait::async_trait;
use catcode_core::{OperationLevel, Tool, ToolContext, ToolResult};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::process::Command;
use std::time::Instant;
use tokio::io::AsyncBufReadExt;

const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_OUTPUT_BYTES: usize = 1024 * 1024; // 1 MB

/// Progress event emitted during bash execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BashProgress {
    /// [`Started`].
    Started { command: String },
    /// [`Stdout`].
    Stdout { data: String, elapsed_secs: f64 },
    /// [`Stderr`].
    Stderr { data: String, elapsed_secs: f64 },
    /// [`Completed`].
    Completed { exit_code: i32, duration_secs: f64 },
    /// [`TimedOut`].
    TimedOut { duration_secs: f64 },
    /// [`Backgrounded`].
    Backgrounded { task_id: String },
}

/// Executes a shell command via `/bin/sh -c`.
///
/// Parameters:
/// - `command` (string, required): The shell command to execute.
///
/// The command runs in the working directory from `ctx.working_dir`.
/// stdout and stderr are combined in the output.
pub struct BashTool {
    #[allow(dead_code)]
    timeout_secs: u64,
}

impl BashTool {
    /// Auto-background threshold in milliseconds (15 seconds).
    const AUTO_BACKGROUND_MS: u64 = 15_000;

    pub fn new() -> Self {
        Self {
            timeout_secs: DEFAULT_TIMEOUT_SECS,
        }
    }

    /// Configure timeout.
    pub fn with_timeout(timeout_secs: u64) -> Self {
        Self { timeout_secs }
    }

    /// Execute a command with streaming progress events and auto-background detection.
    ///
    /// Returns both the [`ToolResult`] and a [`Vec<BashProgress>`] capturing
    /// lifecycle events including stdout/stderr lines, timing, and auto-background.
    pub async fn execute_streaming(
        command: &str,
        _timeout_secs: u64,
        working_dir: Option<&std::path::Path>,
    ) -> (ToolResult, Vec<BashProgress>) {
        let start = Instant::now();
        let mut progress = Vec::new();

        progress.push(BashProgress::Started {
            command: command.to_string(),
        });

        let mut cmd = tokio::process::Command::new("sh");
        cmd.args(["-c", command]);

        if let Some(dir) = working_dir {
            cmd.current_dir(dir);
        }

        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                return (
                    ToolResult::error(format!("Failed to spawn command: {}", e)),
                    progress,
                );
            }
        };

        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let mut stdout_reader = tokio::io::BufReader::new(stdout).lines();
        let mut stderr_reader = tokio::io::BufReader::new(stderr).lines();

        let mut stdout_output = String::new();
        let mut stderr_output = String::new();
        let mut backgrounded = false;
        let mut stdout_done = false;
        let mut stderr_done = false;

        loop {
            tokio::select! {
                result = stdout_reader.next_line(), if !stdout_done => {
                    match result {
                        Ok(Some(line)) => {
                            stdout_output.push_str(&line);
                            stdout_output.push('\n');
                            progress.push(BashProgress::Stdout {
                                data: line,
                                elapsed_secs: start.elapsed().as_secs_f64(),
                            });
                        }
                        _ => { stdout_done = true; }
                    }
                }
                result = stderr_reader.next_line(), if !stderr_done => {
                    match result {
                        Ok(Some(line)) => {
                            stderr_output.push_str(&line);
                            stderr_output.push('\n');
                            progress.push(BashProgress::Stderr {
                                data: line,
                                elapsed_secs: start.elapsed().as_secs_f64(),
                            });
                        }
                        _ => { stderr_done = true; }
                    }
                }
                _ = tokio::time::sleep(std::time::Duration::from_millis(Self::AUTO_BACKGROUND_MS)), if !backgrounded => {
                    backgrounded = true;
                    progress.push(BashProgress::Backgrounded {
                        task_id: format!("bash-{}", uuid::Uuid::new_v4()),
                    });
                }
            }

            if stdout_done && stderr_done {
                break;
            }
        }

        let status = match child.wait().await {
            Ok(s) => s,
            Err(e) => {
                return (
                    ToolResult::error(format!("Command failed: {}", e)),
                    progress,
                );
            }
        };

        let duration = start.elapsed().as_secs_f64();
        let exit_code = status.code().unwrap_or(-1);

        progress.push(BashProgress::Completed {
            exit_code,
            duration_secs: duration,
        });

        let mut combined = String::new();
        if !stdout_output.is_empty() {
            combined.push_str(&stdout_output);
        }
        if !stderr_output.is_empty() {
            if !combined.is_empty() {
                combined.push_str("\n--- stderr ---\n");
            }
            combined.push_str(&stderr_output);
        }

        if combined.len() > MAX_OUTPUT_BYTES {
            let truncated = &combined[..MAX_OUTPUT_BYTES];
            combined = format!(
                "{}\n... (output truncated, {} bytes total)",
                truncated,
                combined.len()
            );
        }

        if combined.is_empty() {
            combined = "(no output)".to_string();
        }

        let result = if exit_code == 0 {
            ToolResult::success(combined)
        } else {
            ToolResult::error(format!(
                "Command failed with exit code {}:\n{}",
                exit_code, combined
            ))
        };

        (result, progress)
    }

    /// Detect and extract structured hints from command output.
    ///
    /// CatCode uses `[catcode-hint: ...]` tags (analogous to Claude Code's
    /// `<claude-code-hint />` XML protocol on stderr).
    pub fn extract_hints(output: &str) -> Vec<String> {
        let mut hints = Vec::new();
        for line in output.lines() {
            if let Some(hint) = line.strip_prefix("[catcode-hint:")
                && let Some(end) = hint.find(']')
            {
                hints.push(hint[..end].trim().to_string());
            }
        }
        hints
    }
}

impl Default for BashTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Execute a shell command. stdout and stderr are combined in the output. Dangerous commands require approval."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute."
                }
            },
            "required": ["command"]
        })
    }

    fn operation_level(&self) -> OperationLevel {
        OperationLevel::Dangerous
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    fn is_read_only(&self) -> bool {
        false
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        let command = match args.get("command").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => return ToolResult::error("Missing required argument: command"),
        };

        if ctx.dry_run {
            return ToolResult::success(format!("[dry-run] Would execute: {}", command));
        }

        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c").arg(command);

        if let Some(ref wd) = ctx.working_dir {
            cmd.current_dir(wd);
        } else if let Some(ref pd) = ctx.project_dir {
            cmd.current_dir(pd);
        }

        let output = match cmd.output() {
            Ok(o) => o,
            Err(e) => {
                return ToolResult::error(format!("Failed to execute command: {}", e));
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let mut combined = String::new();

        if !stdout.is_empty() {
            combined.push_str(&stdout);
        }
        if !stderr.is_empty() {
            if !combined.is_empty() {
                combined.push_str("\n--- stderr ---\n");
            }
            combined.push_str(&stderr);
        }

        if combined.len() > MAX_OUTPUT_BYTES {
            let truncated = &combined[..MAX_OUTPUT_BYTES];
            combined = format!(
                "{}\n... (output truncated, {} bytes total)",
                truncated,
                combined.len()
            );
        }

        if combined.is_empty() {
            combined = "(no output)".to_string();
        }

        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            return ToolResult::error(format!(
                "Command failed with exit code {}:\n{}",
                code, combined
            ));
        }

        ToolResult::success(combined)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::{Tool, ToolContext};
    use serde_json::json;
    use tempfile::TempDir;

    fn make_ctx(project_dir: &std::path::Path) -> ToolContext {
        ToolContext {
            session_id: Some("test".to_string()),
            project_dir: Some(project_dir.to_path_buf()),
            working_dir: Some(project_dir.to_path_buf()),
            dry_run: false,
        }
    }

    #[tokio::test]
    async fn test_bash_simple_command() {
        let tmp = TempDir::new().unwrap();
        let tool = BashTool::new();
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"command": "echo hello"}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.output.contains("hello"));
    }

    #[tokio::test]
    async fn test_bash_exit_code_zero() {
        let tmp = TempDir::new().unwrap();
        let tool = BashTool::new();
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"command": "true"}), &ctx).await;

        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn test_bash_exit_code_nonzero() {
        let tmp = TempDir::new().unwrap();
        let tool = BashTool::new();
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"command": "false"}), &ctx).await;

        assert!(result.is_error);
        assert!(result.output.contains("exit code"));
    }

    #[tokio::test]
    async fn test_bash_stderr_captured() {
        let tmp = TempDir::new().unwrap();
        let tool = BashTool::new();
        let ctx = make_ctx(tmp.path());
        let result = tool
            .execute(json!({"command": "echo error_msg >&2"}), &ctx)
            .await;

        assert!(!result.is_error);
        assert!(result.output.contains("error_msg"));
    }

    #[tokio::test]
    async fn test_bash_working_directory() {
        let tmp = TempDir::new().unwrap();
        let tool = BashTool::new();
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({"command": "pwd"}), &ctx).await;

        assert!(!result.is_error);
        assert!(
            result
                .output
                .contains(&tmp.path().to_string_lossy().to_string())
        );
    }

    #[tokio::test]
    async fn test_bash_missing_command() {
        let tmp = TempDir::new().unwrap();
        let tool = BashTool::new();
        let ctx = make_ctx(tmp.path());
        let result = tool.execute(json!({}), &ctx).await;

        assert!(result.is_error);
        assert!(result.output.contains("command"));
    }

    #[tokio::test]
    async fn test_bash_dry_run() {
        let tmp = TempDir::new().unwrap();
        let tool = BashTool::new();
        let ctx = ToolContext {
            session_id: Some("test".to_string()),
            project_dir: Some(tmp.path().to_path_buf()),
            working_dir: Some(tmp.path().to_path_buf()),
            dry_run: true,
        };
        let result = tool.execute(json!({"command": "rm -rf /"}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.output.contains("dry-run"));
    }

    #[tokio::test]
    async fn test_bash_command_with_args() {
        let tmp = TempDir::new().unwrap();
        let tool = BashTool::new();
        let ctx = make_ctx(tmp.path());
        let result = tool
            .execute(json!({"command": "echo -n no-newline"}), &ctx)
            .await;

        assert!(!result.is_error);
        assert!(result.output.contains("no-newline"));
    }

    #[test]
    fn test_bash_metadata() {
        let tool = BashTool::new();
        assert_eq!(tool.name(), "bash");
        assert!(matches!(
            tool.operation_level(),
            catcode_core::OperationLevel::Dangerous
        ));
    }

    // --- Streaming execution tests ---

    #[tokio::test]
    async fn test_bash_streaming_echo() {
        let (result, progress) =
            BashTool::execute_streaming("echo hello streaming", 30, None).await;

        assert!(!result.is_error);
        assert!(result.output.contains("hello streaming"));

        assert!(
            progress
                .iter()
                .any(|e| matches!(e, BashProgress::Started { .. }))
        );
        assert!(
            progress.iter().any(
                |e| matches!(e, BashProgress::Stdout { data, .. } if data == "hello streaming")
            )
        );
        assert!(
            progress
                .iter()
                .any(|e| matches!(e, BashProgress::Completed { exit_code: 0, .. }))
        );
    }

    #[tokio::test]
    async fn test_bash_streaming_with_stderr() {
        let (result, progress) =
            BashTool::execute_streaming("echo err_out >&2 && echo ok_out", 30, None).await;

        assert!(!result.is_error);
        assert!(result.output.contains("ok_out"));
        assert!(result.output.contains("err_out"));

        let stderr_events: Vec<_> = progress
            .iter()
            .filter(|e| matches!(e, BashProgress::Stderr { .. }))
            .collect();
        assert!(
            !stderr_events.is_empty(),
            "should have stderr progress events"
        );
    }

    #[tokio::test]
    async fn test_bash_streaming_nonzero_exit() {
        let (result, progress) = BashTool::execute_streaming("exit 42", 30, None).await;

        assert!(result.is_error);
        assert!(result.output.contains("42"));

        let completed = progress.iter().find_map(|e| {
            if let BashProgress::Completed { exit_code, .. } = e {
                Some(*exit_code)
            } else {
                None
            }
        });
        assert_eq!(completed, Some(42));
    }

    #[tokio::test]
    async fn test_bash_progress_events_order() {
        let (_result, progress) =
            BashTool::execute_streaming("echo first && echo second", 30, None).await;

        let mut kinds = progress.iter().map(|e| match e {
            BashProgress::Started { .. } => "started",
            BashProgress::Stdout { .. } => "stdout",
            BashProgress::Stderr { .. } => "stderr",
            BashProgress::Completed { .. } => "completed",
            BashProgress::TimedOut { .. } => "timeout",
            BashProgress::Backgrounded { .. } => "backgrounded",
        });

        assert_eq!(kinds.next(), Some("started"));

        let mut found_completed = false;
        for kind in kinds {
            if kind == "completed" {
                found_completed = true;
                break;
            }
        }
        assert!(found_completed, "streaming must end with Completed event");
    }

    #[tokio::test]
    async fn test_bash_streaming_empty_output() {
        let (result, progress) = BashTool::execute_streaming("true", 30, None).await;

        assert!(!result.is_error);
        assert!(result.output.contains("no output"));

        let has_completed = progress
            .iter()
            .any(|e| matches!(e, BashProgress::Completed { exit_code: 0, .. }));
        assert!(has_completed);
    }

    #[tokio::test]
    async fn test_bash_streaming_no_background_for_short_cmd() {
        let (_result, progress) = BashTool::execute_streaming("echo quick", 30, None).await;

        let bg: Vec<_> = progress
            .iter()
            .filter(|e| matches!(e, BashProgress::Backgrounded { .. }))
            .collect();
        assert!(bg.is_empty(), "short command should NOT be backgrounded");
    }

    // --- Hint extraction tests ---

    #[test]
    fn test_extract_hints_empty() {
        let hints = BashTool::extract_hints("plain output\nno hints here");
        assert!(hints.is_empty());
    }

    #[test]
    fn test_extract_hints_single() {
        let hints = BashTool::extract_hints("line before\n[catcode-hint: refresh_ui]\nline after");
        assert_eq!(hints, vec!["refresh_ui"]);
    }

    #[test]
    fn test_extract_hints_multiple() {
        let hints =
            BashTool::extract_hints("[catcode-hint: hint_a]\n[catcode-hint: hint_b with spaces]");
        assert_eq!(hints, vec!["hint_a", "hint_b with spaces"]);
    }

    #[test]
    fn test_extract_hints_partial_tag() {
        let hints = BashTool::extract_hints("this has [catcode-hint: no closing bracket");
        assert!(hints.is_empty(), "incomplete tag should be ignored");
    }

    // --- Concurrency safety tests ---

    #[test]
    fn test_bash_is_not_concurrency_safe() {
        let tool = BashTool::new();
        assert!(!tool.is_concurrency_safe());
    }

    #[test]
    fn test_bash_is_not_read_only() {
        let tool = BashTool::new();
        assert!(!tool.is_read_only());
    }

    #[test]
    fn test_bash_progress_serde_roundtrip() {
        let events = vec![
            BashProgress::Started {
                command: "echo hi".into(),
            },
            BashProgress::Stdout {
                data: "hi".into(),
                elapsed_secs: 0.5,
            },
            BashProgress::Stderr {
                data: "warn".into(),
                elapsed_secs: 0.6,
            },
            BashProgress::Completed {
                exit_code: 0,
                duration_secs: 1.2,
            },
            BashProgress::TimedOut {
                duration_secs: 60.0,
            },
            BashProgress::Backgrounded {
                task_id: "bash-abc".into(),
            },
        ];

        let json = serde_json::to_string(&events).unwrap();
        let deserialized: Vec<BashProgress> = serde_json::from_str(&json).unwrap();
        assert_eq!(events.len(), deserialized.len());
    }

    #[tokio::test]
    async fn test_bash_streaming_working_directory() {
        let tmp = TempDir::new().unwrap();

        let (result, _progress) = BashTool::execute_streaming("pwd", 30, Some(tmp.path())).await;

        assert!(!result.is_error);
        assert!(
            result
                .output
                .contains(&tmp.path().to_string_lossy().to_string())
        );
    }
}
