use async_trait::async_trait;
use catcode_core::OperationLevel;
use catcode_core::middleware::{AgentContext, Middleware, ToolCallNext};
use catcode_core::tool::{ToolCall, ToolResult};
use catcode_sandbox::OperationClassifier;
use tracing::warn;

use crate::Database;

/// Middleware that records mutating tool calls into the SQLite audit log.
#[derive(Clone)]
pub struct AuditLogMiddleware {
    db: Database,
}

impl AuditLogMiddleware {
    pub fn new(db: Database) -> Self {
        Self { db }
    }
}

#[async_trait]
impl Middleware for AuditLogMiddleware {
    fn name(&self) -> &str {
        "audit_log"
    }

    async fn wrap_tool_call(
        &self,
        ctx: &mut AgentContext,
        call: &ToolCall,
        next: ToolCallNext<'_>,
    ) -> ToolResult {
        let level = OperationClassifier::classify(&call.name, &call.args);
        let result = next.execute(call).await;

        if matches!(level, OperationLevel::Sensitive | OperationLevel::Dangerous)
            && let Err(err) = self
                .db
                .insert_audit_log(
                    &ctx.session_id,
                    "tool_call",
                    Some(&call.name),
                    Some(&call.args.to_string()),
                    operation_level_name(level),
                    None,
                    if result.is_error { "error" } else { "success" },
                )
                .await
        {
            warn!(
                error = %err,
                session_id = %ctx.session_id,
                tool = %call.name,
                "Failed to write audit log"
            );
        }

        result
    }
}

fn operation_level_name(level: OperationLevel) -> &'static str {
    match level {
        OperationLevel::Safe => "safe",
        OperationLevel::Sensitive => "sensitive",
        OperationLevel::Dangerous => "dangerous",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::middleware::ToolCallNext;

    #[tokio::test]
    async fn test_sensitive_tool_call_is_audited() {
        let db = Database::new_in_memory().await.unwrap();
        db.upsert_session("s1", "test", "running", "/tmp", "model", "provider", 0)
            .await
            .unwrap();
        let middleware = AuditLogMiddleware::new(db.clone());
        let mut ctx = AgentContext::new("s1");
        let call = ToolCall {
            id: "call-1".to_string(),
            name: "write_file".to_string(),
            args: serde_json::json!({"path": "a.txt", "content": "hello"}),
        };

        let next = ToolCallNext::new(|_call| async { ToolResult::success("written") });
        let result = middleware.wrap_tool_call(&mut ctx, &call, next).await;

        assert!(!result.is_error);
        let logs = db.get_audit_log("s1").await.unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].tool.as_deref(), Some("write_file"));
        assert_eq!(logs[0].level, "sensitive");
        assert_eq!(logs[0].result, "success");
    }

    #[tokio::test]
    async fn test_safe_tool_call_is_not_audited() {
        let db = Database::new_in_memory().await.unwrap();
        db.upsert_session("s1", "test", "running", "/tmp", "model", "provider", 0)
            .await
            .unwrap();
        let middleware = AuditLogMiddleware::new(db.clone());
        let mut ctx = AgentContext::new("s1");
        let call = ToolCall {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            args: serde_json::json!({"path": "a.txt"}),
        };

        let next = ToolCallNext::new(|_call| async { ToolResult::success("content") });
        let result = middleware.wrap_tool_call(&mut ctx, &call, next).await;

        assert!(!result.is_error);
        assert!(db.get_audit_log("s1").await.unwrap().is_empty());
    }
}
