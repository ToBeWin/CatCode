use async_trait::async_trait;
use catcode_core::OperationLevel;
use catcode_core::middleware::{AgentContext, Middleware, ToolCallNext};
use catcode_core::tool::{ToolCall, ToolResult};
use catcode_sandbox::{ApprovalGate, ApprovalPolicy, OperationClassifier};
use tracing::{debug, warn};

/// Middleware that classifies tool operations and enforces sandbox policies.
///
/// For safe operations: pass through.
/// For sensitive operations: log and pass through.
/// For dangerous operations: request approval before proceeding.
pub struct SandboxMiddleware {
    gate: ApprovalGate,
}

impl SandboxMiddleware {
    pub fn new(gate: ApprovalGate) -> Self {
        Self { gate }
    }

    /// Create with auto-approve policy (for development/testing).
    pub fn auto_approve() -> Self {
        Self::new(ApprovalGate::new(ApprovalPolicy::AutoApprove))
    }

    /// Create with auto-reject policy (for untrusted environments).
    pub fn auto_reject() -> Self {
        Self::new(ApprovalGate::new(ApprovalPolicy::AutoReject))
    }
}

#[async_trait]
impl Middleware for SandboxMiddleware {
    fn name(&self) -> &str {
        "sandbox"
    }

    async fn wrap_tool_call(
        &self,
        _ctx: &mut AgentContext,
        call: &ToolCall,
        next: ToolCallNext<'_>,
    ) -> ToolResult {
        let level = OperationClassifier::classify(&call.name, &call.args);

        match level {
            OperationLevel::Safe => {
                debug!(tool = %call.name, "Safe operation, passing through");
                next.execute(call).await
            }
            OperationLevel::Sensitive => {
                debug!(tool = %call.name, "Sensitive operation, logging and passing through");
                next.execute(call).await
            }
            OperationLevel::Dangerous => {
                // Check if tool was auto-approved
                if self.gate.is_auto_approved(&call.name) {
                    debug!(tool = %call.name, "Auto-approved dangerous operation");
                    return next.execute(call).await;
                }

                warn!(
                    tool = %call.name,
                    args = %call.args,
                    "Dangerous operation detected"
                );

                // In non-interactive mode, use the gate's default policy
                let request = catcode_sandbox::ApprovalRequest {
                    id: call.id.clone(),
                    tool: call.name.clone(),
                    args: call.args.clone(),
                    reason: format!("Operation level: {:?}", level),
                    level,
                };

                let result = self.gate.request_approval(&request).await;
                match result {
                    catcode_sandbox::ApprovalResult::Approved
                    | catcode_sandbox::ApprovalResult::ApprovedAlways => {
                        debug!(tool = %call.name, "Approved, executing");
                        next.execute(call).await
                    }
                    catcode_sandbox::ApprovalResult::Rejected => {
                        warn!(tool = %call.name, "Rejected by approval gate");
                        ToolResult::error(format!(
                            "Operation '{}' rejected: requires approval for dangerous operations",
                            call.name
                        ))
                    }
                    catcode_sandbox::ApprovalResult::Timeout => {
                        warn!(tool = %call.name, "Approval timed out");
                        ToolResult::error(format!(
                            "Operation '{}' timed out waiting for approval",
                            call.name
                        ))
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::middleware::ToolCallNext;

    fn make_call(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            id: "test".to_string(),
            name: name.to_string(),
            args,
        }
    }

    #[tokio::test]
    async fn test_safe_operation_passes_through() {
        let mw = SandboxMiddleware::auto_approve();
        let mut ctx = AgentContext::new("test");
        let call = make_call("read_file", serde_json::json!({"path": "src/main.rs"}));

        let next = ToolCallNext::new(|_call| async { ToolResult::success("file content") });
        let result = mw.wrap_tool_call(&mut ctx, &call, next).await;
        assert!(!result.is_error);
        assert_eq!(result.output, "file content");
    }

    #[tokio::test]
    async fn test_sensitive_operation_passes_through() {
        let mw = SandboxMiddleware::auto_approve();
        let mut ctx = AgentContext::new("test");
        let call = make_call(
            "write_file",
            serde_json::json!({"path": "src/main.rs", "content": "fn main() {}"}),
        );

        let next = ToolCallNext::new(|_call| async { ToolResult::success("written") });
        let result = mw.wrap_tool_call(&mut ctx, &call, next).await;
        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn test_dangerous_operation_auto_approved() {
        let mw = SandboxMiddleware::auto_approve();
        let mut ctx = AgentContext::new("test");
        let call = make_call("bash", serde_json::json!({"command": "rm -rf /tmp/test"}));

        let next = ToolCallNext::new(|_call| async { ToolResult::success("deleted") });
        let result = mw.wrap_tool_call(&mut ctx, &call, next).await;
        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn test_dangerous_operation_auto_rejected() {
        let mw = SandboxMiddleware::auto_reject();
        let mut ctx = AgentContext::new("test");
        let call = make_call("bash", serde_json::json!({"command": "rm -rf /"}));

        let next =
            ToolCallNext::new(|_call| async { ToolResult::success("should not reach here") });
        let result = mw.wrap_tool_call(&mut ctx, &call, next).await;
        assert!(result.is_error);
        assert!(result.output.contains("rejected"));
    }

    #[tokio::test]
    async fn test_auto_approved_tool_skips_gate() {
        let gate = ApprovalGate::new(ApprovalPolicy::AutoReject);
        gate.set_auto_approved("bash");

        let mw = SandboxMiddleware::new(gate);
        let mut ctx = AgentContext::new("test");
        let call = make_call("bash", serde_json::json!({"command": "echo hello"}));

        let next = ToolCallNext::new(|_call| async { ToolResult::success("hello") });
        let result = mw.wrap_tool_call(&mut ctx, &call, next).await;
        assert!(!result.is_error);
        assert_eq!(result.output, "hello");
    }
}
