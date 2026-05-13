use async_trait::async_trait;

use crate::policy::SandboxPolicy;

/// Output from sandbox execution.
#[derive(Debug, Clone)]
pub struct SandboxOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub timed_out: bool,
}

/// A command to be executed in a sandbox.
#[derive(Debug, Clone)]
pub struct SandboxCommand {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub working_dir: Option<std::path::PathBuf>,
}

impl SandboxCommand {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
            working_dir: None,
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    pub fn working_dir(mut self, dir: std::path::PathBuf) -> Self {
        self.working_dir = Some(dir);
        self
    }
}

/// Trait for sandbox backends (Docker, firejail, native, etc.).
#[async_trait]
pub trait SandboxBackend: Send + Sync {
    /// Execute a command under the given policy.
    async fn execute(
        &self,
        cmd: &SandboxCommand,
        policy: &SandboxPolicy,
    ) -> Result<SandboxOutput, SandboxError>;

    /// Check if this backend is available on the system.
    fn is_available(&self) -> bool;

    /// Human-readable name of this backend.
    fn name(&self) -> &str;
}

/// Errors during sandbox execution.
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("Execution timed out after {0}s")]
    Timeout(u64),

    #[error("Path denied by policy: {0}")]
    PathDenied(String),

    #[error("Backend not available: {0}")]
    NotAvailable(String),

    #[error("Execution failed: {0}")]
    ExecutionFailed(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Native (no sandbox) backend — executes commands directly.
///
/// This is the fallback when Docker/firejail are not available.
/// It enforces policy checks (path, timeout) but does NOT provide isolation.
pub struct NativeSandbox;

impl NativeSandbox {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NativeSandbox {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SandboxBackend for NativeSandbox {
    async fn execute(
        &self,
        cmd: &SandboxCommand,
        policy: &SandboxPolicy,
    ) -> Result<SandboxOutput, SandboxError> {
        // Pre-execution policy checks
        if let Some(ref dir) = cmd.working_dir
            && !policy.is_path_allowed(dir)
        {
            return Err(SandboxError::PathDenied(dir.display().to_string()));
        }

        let mut tokio_cmd = tokio::process::Command::new(&cmd.program);
        tokio_cmd.args(&cmd.args);
        for (k, v) in &cmd.env {
            tokio_cmd.env(k, v);
        }
        if let Some(ref dir) = cmd.working_dir {
            tokio_cmd.current_dir(dir);
        }

        // Apply timeout
        let timeout = policy.timeout_secs;
        let output = if timeout > 0 {
            match tokio::time::timeout(std::time::Duration::from_secs(timeout), tokio_cmd.output())
                .await
            {
                Ok(result) => result?,
                Err(_) => {
                    return Ok(SandboxOutput {
                        stdout: String::new(),
                        stderr: format!("Timed out after {}s", timeout),
                        exit_code: -1,
                        timed_out: true,
                    });
                }
            }
        } else {
            tokio_cmd.output().await?
        };

        // Truncate output if needed
        let stdout = truncate_bytes(&output.stdout, policy.max_output_bytes);
        let stderr = truncate_bytes(&output.stderr, policy.max_output_bytes / 2);

        Ok(SandboxOutput {
            stdout,
            stderr,
            exit_code: output.status.code().unwrap_or(-1),
            timed_out: false,
        })
    }

    fn is_available(&self) -> bool {
        true // Native is always available
    }

    fn name(&self) -> &str {
        "native"
    }
}

/// Truncate a byte slice to max_bytes, converting to UTF-8 lossily.
pub(crate) fn truncate_bytes(bytes: &[u8], max_bytes: usize) -> String {
    if bytes.len() <= max_bytes {
        String::from_utf8_lossy(bytes).to_string()
    } else {
        String::from_utf8_lossy(&bytes[..max_bytes]).to_string() + "\n... (truncated)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_command_builder() {
        let cmd = SandboxCommand::new("ls")
            .arg("-la")
            .args(["--color=auto", "/tmp"])
            .env("HOME", "/root")
            .working_dir(std::path::PathBuf::from("/tmp"));

        assert_eq!(cmd.program, "ls");
        assert_eq!(cmd.args.len(), 3);
        assert_eq!(cmd.env.len(), 1);
        assert!(cmd.working_dir.is_some());
    }

    #[test]
    fn test_native_sandbox_available() {
        let backend = NativeSandbox::new();
        assert!(backend.is_available());
        assert_eq!(backend.name(), "native");
    }

    #[tokio::test]
    async fn test_native_sandbox_execute_echo() {
        let backend = NativeSandbox::new();
        let cmd = SandboxCommand::new("echo").arg("hello");
        let policy = SandboxPolicy::default();

        let output = backend.execute(&cmd, &policy).await.unwrap();
        assert_eq!(output.stdout.trim(), "hello");
        assert_eq!(output.exit_code, 0);
        assert!(!output.timed_out);
    }

    #[tokio::test]
    async fn test_native_sandbox_path_denied() {
        let backend = NativeSandbox::new();
        let cmd = SandboxCommand::new("ls").working_dir(std::path::PathBuf::from("/etc"));
        let policy = SandboxPolicy {
            allowed_paths: vec![std::path::PathBuf::from("/tmp")],
            ..Default::default()
        };

        let result = backend.execute(&cmd, &policy).await;
        assert!(matches!(result, Err(SandboxError::PathDenied(_))));
    }

    #[test]
    fn test_truncate_bytes_short() {
        assert_eq!(truncate_bytes(b"hello", 100), "hello");
    }

    #[test]
    fn test_truncate_bytes_long() {
        let result = truncate_bytes(b"hello world", 5);
        assert!(result.starts_with("hello"));
        assert!(result.contains("truncated"));
    }

}
