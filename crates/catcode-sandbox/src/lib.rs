//! # catcode-sandbox
//!
//! Sandbox isolation layer for the CatCode AI coding agent.
//!
//! Provides:
//! - [`OperationClassifier`] — classifies tool operations by risk level
//! - [`SandboxBackend`] — trait for execution backends (Docker, firejail, native)
//! - [`SandboxPolicy`] — controls allowed paths, network, resources
//! - [`ApprovalGate`] — human approval for dangerous operations
//! - [`SandboxSelector`] — selects the best available backend
//!
//! Safety model:
//! - 🟢 Safe: executed directly, logged
//! - 🟡 Sensitive: executed directly, logged in audit
//! - 🔴 Dangerous: sandbox execution + optional human approval

/// The `backend` module.
pub mod backend;
/// The `classifier` module.
pub mod classifier;
/// The `gate` module.
pub mod gate;
/// The `policy` module.
pub mod policy;
/// The `selector` module.
pub mod selector;

pub use backend::{NativeSandbox, SandboxBackend, SandboxCommand, SandboxError, SandboxOutput};
pub use classifier::OperationClassifier;
pub use gate::{ApprovalGate, ApprovalPolicy, ApprovalRequest, ApprovalResult};
pub use policy::{NetworkPolicy, SandboxPolicy};
pub use selector::SandboxSelector;

/// High-level sandbox executor that combines classifier, gate, and backend.
///
/// This is the main entry point for executing tools through the sandbox layer.
pub struct SandboxExecutor {
    selector: SandboxSelector,
    gate: ApprovalGate,
    policy: SandboxPolicy,
}

impl SandboxExecutor {
    /// Create a new sandbox executor with the given policy and gate.
    pub fn new(policy: SandboxPolicy, gate: ApprovalGate) -> Self {
        Self {
            selector: SandboxSelector::new(),
            gate,
            policy,
        }
    }

    /// Get the operation level for a tool call.
    pub fn classify(&self, tool: &str, args: &serde_json::Value) -> catcode_core::OperationLevel {
        OperationClassifier::classify(tool, args)
    }

    /// Check if an operation needs approval.
    pub fn needs_approval(&self, tool: &str, args: &serde_json::Value) -> bool {
        let level = self.classify(tool, args);
        self.gate.needs_approval(level)
    }

    /// Request approval for a tool call.
    pub async fn request_approval(&self, tool: &str, args: &serde_json::Value) -> ApprovalResult {
        let level = self.classify(tool, args);
        if !self.gate.needs_approval(level) {
            return ApprovalResult::Approved;
        }

        let request = ApprovalRequest {
            id: uuid::Uuid::new_v4().to_string(),
            tool: tool.to_string(),
            args: args.clone(),
            reason: format!("Operation level: {:?}", level),
            level,
        };

        self.gate.request_approval(&request).await
    }

    /// Execute a command through the sandbox.
    pub async fn execute_command(
        &self,
        cmd: &SandboxCommand,
    ) -> Result<SandboxOutput, SandboxError> {
        let backend = self
            .selector
            .select()
            .ok_or_else(|| SandboxError::NotAvailable("No sandbox backend available".into()))?;
        backend.execute(cmd, &self.policy).await
    }

    /// Get the selector for registering custom backends.
    pub fn selector_mut(&mut self) -> &mut SandboxSelector {
        &mut self.selector
    }

    /// Get the gate for managing auto-approved tools.
    pub fn gate(&self) -> &ApprovalGate {
        &self.gate
    }
}

impl Default for SandboxExecutor {
    fn default() -> Self {
        Self::new(SandboxPolicy::default(), ApprovalGate::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_executor_classify() {
        let executor = SandboxExecutor::default();
        assert_eq!(
            executor.classify("read_file", &serde_json::json!({"path": "x"})),
            catcode_core::OperationLevel::Safe
        );
        assert_eq!(
            executor.classify("bash", &serde_json::json!({"command": "rm -rf /"})),
            catcode_core::OperationLevel::Dangerous
        );
    }

    #[test]
    fn test_executor_needs_approval() {
        let executor = SandboxExecutor::default();
        assert!(!executor.needs_approval("read_file", &serde_json::json!({})));
        assert!(executor.needs_approval("bash", &serde_json::json!({"command": "rm -rf /"})));
    }

    #[tokio::test]
    async fn test_executor_execute_echo() {
        let executor = SandboxExecutor::default();
        let cmd = SandboxCommand::new("echo").arg("test");
        let output = executor.execute_command(&cmd).await.unwrap();
        assert_eq!(output.stdout.trim(), "test");
        assert_eq!(output.exit_code, 0);
    }
}
