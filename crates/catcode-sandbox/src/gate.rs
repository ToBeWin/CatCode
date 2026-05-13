use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use catcode_core::OperationLevel;

/// Result of an approval request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalResult {
    /// Operation approved.
/// [`Approved`].
    Approved,
    /// Approved, and auto-approve all future operations of this type in this session.
/// [`ApprovedAlways`].
    ApprovedAlways,
    /// Operation rejected.
/// [`Rejected`].
    Rejected,
    /// No response within timeout — defaults to rejected.
/// [`Timeout`].
    Timeout,
}

/// An operation pending approval.
#[derive(Debug, Clone)]
pub struct ApprovalRequest {
    /// Unique ID for this request.
    pub id: String,
    /// Tool name.
    pub tool: String,
    /// Arguments to the tool.
    pub args: serde_json::Value,
    /// Why approval is needed.
    pub reason: String,
    /// Operation level.
    pub level: OperationLevel,
}

/// Decision callback type — resolves the approval request.
/// [`ApprovalCallback`]
pub type ApprovalCallback = Box<dyn FnOnce(ApprovalResult) + Send + 'static>;

/// Gate that controls approval of sensitive/dangerous operations.
///
/// In interactive mode, this sends requests to the TUI for user approval.
/// In non-interactive mode, it applies the configured policy.
pub struct ApprovalGate {
    /// Auto-approved tool names for this session.
    auto_approved: Arc<Mutex<HashSet<String>>>,
    /// Default behavior when no interactive handler is set.
    default_policy: ApprovalPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Policy for determining when human approval is required.
pub enum ApprovalPolicy {
    /// Always approve (for testing / trusted environments).
/// [`AutoApprove`].
    AutoApprove,
    /// Always reject dangerous operations.
/// [`AutoReject`].
    AutoReject,
    /// Use the interactive callback.
/// [`Interactive`].
    Interactive,
}

impl ApprovalGate {
/// Create a new approval gate with the given policy.
    pub fn new(default_policy: ApprovalPolicy) -> Self {
        Self {
            auto_approved: Arc::new(Mutex::new(HashSet::new())),
            default_policy,
        }
    }

    /// Check if an operation needs approval.
    pub fn needs_approval(&self, level: OperationLevel) -> bool {
        match level {
            OperationLevel::Safe => false,
            OperationLevel::Sensitive => false, // Sensitive ops are logged but auto-approved
            OperationLevel::Dangerous => true,
        }
    }

    /// Check if a tool was previously marked as auto-approved.
    pub fn is_auto_approved(&self, tool: &str) -> bool {
        self.auto_approved
            .lock()
            .map(|set| set.contains(tool))
            .unwrap_or(false)
    }

    /// Mark a tool as auto-approved for this session.
    pub fn set_auto_approved(&self, tool: &str) {
        if let Ok(mut set) = self.auto_approved.lock() {
            set.insert(tool.to_string());
        }
    }

    /// Request approval for an operation.
    ///
    /// Returns the approval result. In interactive mode, this sends
    /// a request through the callback channel and waits for a response.
    pub async fn request_approval(&self, request: &ApprovalRequest) -> ApprovalResult {
        // Check if tool was auto-approved
        if self.is_auto_approved(&request.tool) {
            return ApprovalResult::ApprovedAlways;
        }

        match self.default_policy {
            ApprovalPolicy::AutoApprove => ApprovalResult::Approved,
            ApprovalPolicy::AutoReject => ApprovalResult::Rejected,
            ApprovalPolicy::Interactive => {
                // In interactive mode, this would send to TUI
                // For now, default to reject (the TUI integration handles this)
                ApprovalResult::Timeout
            }
        }
    }

    /// Request approval with an interactive callback.
    ///
    /// The callback is called with the result when approval is resolved.
    pub async fn request_approval_with_callback(
        &self,
        request: &ApprovalRequest,
        callback: ApprovalCallback,
    ) {
        let result = self.request_approval(request).await;
        callback(result);
    }
}

impl Default for ApprovalGate {
    fn default() -> Self {
        Self::new(ApprovalPolicy::AutoApprove)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_needs_no_approval() {
        let gate = ApprovalGate::default();
        assert!(!gate.needs_approval(OperationLevel::Safe));
    }

    #[test]
    fn test_sensitive_needs_no_approval() {
        let gate = ApprovalGate::default();
        assert!(!gate.needs_approval(OperationLevel::Sensitive));
    }

    #[test]
    fn test_dangerous_needs_approval() {
        let gate = ApprovalGate::default();
        assert!(gate.needs_approval(OperationLevel::Dangerous));
    }

    #[test]
    fn test_auto_approved_tools() {
        let gate = ApprovalGate::default();
        assert!(!gate.is_auto_approved("bash"));

        gate.set_auto_approved("bash");
        assert!(gate.is_auto_approved("bash"));
    }

    #[tokio::test]
    async fn test_auto_approve_policy() {
        let gate = ApprovalGate::new(ApprovalPolicy::AutoApprove);
        let request = ApprovalRequest {
            id: "test".to_string(),
            tool: "bash".to_string(),
            args: serde_json::json!({}),
            reason: "test".to_string(),
            level: OperationLevel::Dangerous,
        };

        let result = gate.request_approval(&request).await;
        assert_eq!(result, ApprovalResult::Approved);
    }

    #[tokio::test]
    async fn test_auto_reject_policy() {
        let gate = ApprovalGate::new(ApprovalPolicy::AutoReject);
        let request = ApprovalRequest {
            id: "test".to_string(),
            tool: "bash".to_string(),
            args: serde_json::json!({}),
            reason: "test".to_string(),
            level: OperationLevel::Dangerous,
        };

        let result = gate.request_approval(&request).await;
        assert_eq!(result, ApprovalResult::Rejected);
    }

    #[tokio::test]
    async fn test_auto_approved_tool_skips_gate() {
        let gate = ApprovalGate::new(ApprovalPolicy::AutoReject);
        gate.set_auto_approved("bash");

        let request = ApprovalRequest {
            id: "test".to_string(),
            tool: "bash".to_string(),
            args: serde_json::json!({}),
            reason: "test".to_string(),
            level: OperationLevel::Dangerous,
        };

        let result = gate.request_approval(&request).await;
        assert_eq!(result, ApprovalResult::ApprovedAlways);
    }
}
