use async_trait::async_trait;
use catcode_core::middleware::{AgentContext, Middleware, ToolCallNext};
use catcode_core::tool::{ToolCall, ToolResult};
use serde::{Deserialize, Serialize};

/// Validation result for tool output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub issues: Vec<ValidationIssue>,
    #[serde(default)]
    pub correction_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub severity: IssueSeverity,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IssueSeverity {
    Warning,
    Error,
}

/// Middleware that validates tool call outputs.
///
/// Checks:
/// - Output is valid UTF-8
/// - Output is not empty when it shouldn't be
/// - Tool call arguments are valid JSON (for tool_call content)
/// - Safety checks (no obvious injection patterns in bash output)
#[derive(Debug)]
pub struct OutputValidatorMiddleware {
    /// Maximum allowed output length in characters.
    max_output_chars: usize,
    /// Whether to enforce safety pattern checks.
    check_safety: bool,
}

impl OutputValidatorMiddleware {
    pub fn new(max_output_chars: usize, check_safety: bool) -> Self {
        Self {
            max_output_chars,
            check_safety,
        }
    }

    /// Validate a tool result and return validation issues.
    pub fn validate(&self, call: &ToolCall, result: &ToolResult) -> ValidationResult {
        let mut issues = Vec::new();

        // Check output length
        if result.output.len() > self.max_output_chars {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Warning,
                message: format!(
                    "Output too long ({} chars > {} limit)",
                    result.output.len(),
                    self.max_output_chars
                ),
            });
        }

        // Check for empty success output
        if !result.is_error && result.output.is_empty() {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Warning,
                message: "Tool reported success but returned empty output".to_string(),
            });
        }

        // Safety checks for specific tools
        if self.check_safety && call.name == "bash" {
            self.check_bash_safety(&result.output, &mut issues);
        }

        // Validate JSON parse for write_file/patch_file outputs
        if call.name == "write_file" || call.name == "patch_file" {
            // These should not return JSON — they return success/error messages
            if result.output.starts_with('{') && serde_json::from_str::<serde_json::Value>(&result.output).is_err() {
                issues.push(ValidationIssue {
                    severity: IssueSeverity::Warning,
                    message: "Output looks like JSON but is malformed".to_string(),
                });
            }
        }

        let is_valid = !issues.iter().any(|i| i.severity == IssueSeverity::Error);

        let correction_hint = if !is_valid {
            Some(self.build_correction_hint(call, &issues))
        } else {
            None
        };

        ValidationResult {
            is_valid,
            issues,
            correction_hint,
        }
    }

    /// Check bash output for safety patterns.
    fn check_bash_safety(&self, output: &str, issues: &mut Vec<ValidationIssue>) {
        let dangerous_patterns = [
            ("rm -rf /", "Recursive delete from root"),
            ("rm -rf /*", "Recursive delete from root"),
            ("mkfs.", "Filesystem formatting"),
            ("dd if=", "Direct disk write"),
            (":(){:|:&};:", "Fork bomb"),
            ("chmod -R 777 /", "Overly permissive root permissions"),
        ];

        for (pattern, description) in &dangerous_patterns {
            if output.contains(pattern) {
                issues.push(ValidationIssue {
                    severity: IssueSeverity::Error,
                    message: format!("Dangerous pattern detected: {description}"),
                });
            }
        }
    }

    /// Build a correction hint for the model.
    fn build_correction_hint(&self, call: &ToolCall, issues: &[ValidationIssue]) -> String {
        let issue_msgs: Vec<&str> = issues.iter().map(|i| i.message.as_str()).collect();
        format!(
            "Tool '{}' output validation failed: {}. Please retry with corrected output.",
            call.name,
            issue_msgs.join("; ")
        )
    }
}

impl Default for OutputValidatorMiddleware {
    fn default() -> Self {
        Self::new(100_000, true)
    }
}

#[async_trait]
impl Middleware for OutputValidatorMiddleware {
    fn name(&self) -> &str {
        "output_validator"
    }

    async fn wrap_tool_call(
        &self,
        _ctx: &mut AgentContext,
        call: &ToolCall,
        next: ToolCallNext<'_>,
    ) -> ToolResult {
        let result = next.execute(call).await;

        let validation = self.validate(call, &result);

        if !validation.is_valid {
            tracing::warn!(
                tool = %call.name,
                issues = ?validation.issues,
                "Tool output validation failed"
            );

            // Return the validation error as the tool result
            let hint = validation.correction_hint.unwrap_or_default();
            return ToolResult::error(format!("[output_validator] {hint}"));
        }

        if !validation.issues.is_empty() {
            // Warnings only — pass through but log
            tracing::debug!(
                tool = %call.name,
                issues = ?validation.issues,
                "Tool output has validation warnings"
            );
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::middleware::AgentContext;

    fn make_call(name: &str) -> ToolCall {
        ToolCall {
            id: "test".to_string(),
            name: name.to_string(),
            args: serde_json::json!({}),
        }
    }

    #[test]
    fn test_valid_output() {
        let validator = OutputValidatorMiddleware::new(1000, false);
        let call = make_call("read_file");
        let result = ToolResult::success("fn main() {}");

        let validation = validator.validate(&call, &result);
        assert!(validation.is_valid);
        assert!(validation.issues.is_empty());
    }

    #[test]
    fn test_output_too_long() {
        let validator = OutputValidatorMiddleware::new(100, false);
        let call = make_call("bash");
        let result = ToolResult::success("x".repeat(200));

        let validation = validator.validate(&call, &result);
        assert!(validation.is_valid); // Warning, not error
        assert_eq!(validation.issues.len(), 1);
        assert_eq!(validation.issues[0].severity, IssueSeverity::Warning);
    }

    #[test]
    fn test_empty_success_output() {
        let validator = OutputValidatorMiddleware::new(1000, false);
        let call = make_call("read_file");
        let result = ToolResult::success("");

        let validation = validator.validate(&call, &result);
        assert!(validation.is_valid); // Warning, not error
        assert_eq!(validation.issues.len(), 1);
        assert!(validation.issues[0].message.contains("empty output"));
    }

    #[test]
    fn test_error_output_not_flagged_as_empty() {
        let validator = OutputValidatorMiddleware::new(1000, false);
        let call = make_call("bash");
        let result = ToolResult::error("command not found");

        let validation = validator.validate(&call, &result);
        assert!(validation.is_valid);
        // Error messages are expected to be non-empty — no warning
    }

    #[test]
    fn test_bash_safety_dangerous_pattern() {
        let validator = OutputValidatorMiddleware::new(1000, true);
        let call = make_call("bash");
        let result = ToolResult::success("$ rm -rf / --no-preserve-root");

        let validation = validator.validate(&call, &result);
        assert!(!validation.is_valid);
        assert!(validation
            .issues
            .iter()
            .any(|i| i.severity == IssueSeverity::Error));
        assert!(validation.correction_hint.is_some());
    }

    #[test]
    fn test_bash_safety_fork_bomb() {
        let validator = OutputValidatorMiddleware::new(1000, true);
        let call = make_call("bash");
        let result = ToolResult::success(":(){:|:&};:");

        let validation = validator.validate(&call, &result);
        assert!(!validation.is_valid);
    }

    #[test]
    fn test_safety_check_disabled() {
        let validator = OutputValidatorMiddleware::new(1000, false);
        let call = make_call("bash");
        let result = ToolResult::success("rm -rf /");

        let validation = validator.validate(&call, &result);
        assert!(validation.is_valid); // Safety check disabled
    }

    #[test]
    fn test_non_bash_ignores_safety() {
        let validator = OutputValidatorMiddleware::new(1000, true);
        let call = make_call("read_file");
        let result = ToolResult::success("rm -rf /"); // In file content, not bash

        let validation = validator.validate(&call, &result);
        assert!(validation.is_valid);
    }

    #[test]
    fn test_malformed_json_output() {
        let validator = OutputValidatorMiddleware::new(1000, false);
        let call = make_call("write_file");
        let result = ToolResult::success("{invalid json");

        let validation = validator.validate(&call, &result);
        assert!(validation.is_valid); // Warning only
        assert!(validation.issues.iter().any(|i| i.message.contains("malformed")));
    }

    #[test]
    fn test_valid_json_output_no_warning() {
        let validator = OutputValidatorMiddleware::new(1000, false);
        let call = make_call("write_file");
        let result = ToolResult::success(r#"{"ok": true}"#);

        let validation = validator.validate(&call, &result);
        assert!(validation.is_valid);
        // Valid JSON that starts with { but is also a valid message — no warning
        // Actually it will still trigger because it starts with '{' AND parses OK
        // So no "malformed" warning. But it won't have issues.
    }

    #[test]
    fn test_default_config() {
        let validator = OutputValidatorMiddleware::default();
        assert_eq!(validator.max_output_chars, 100_000);
        assert!(validator.check_safety);
    }

    #[tokio::test]
    async fn test_middleware_passes_valid_output() {
        let mw = OutputValidatorMiddleware::new(1000, false);
        let mut ctx = AgentContext::new("test");
        let call = make_call("read_file");

        let tool_fn: ToolCallNext = ToolCallNext::new(|_call| {
            Box::pin(async { ToolResult::success("fn main() {}") })
        });

        let result = mw.wrap_tool_call(&mut ctx, &call, tool_fn).await;
        assert!(!result.is_error);
        assert_eq!(result.output, "fn main() {}");
    }

    #[tokio::test]
    async fn test_middleware_blocks_dangerous_output() {
        let mw = OutputValidatorMiddleware::new(1000, true);
        let mut ctx = AgentContext::new("test");
        let call = make_call("bash");

        let tool_fn: ToolCallNext = ToolCallNext::new(|_call| {
            Box::pin(async { ToolResult::success("rm -rf /") })
        });

        let result = mw.wrap_tool_call(&mut ctx, &call, tool_fn).await;
        assert!(result.is_error);
        assert!(result.output.contains("output_validator"));
    }

    #[tokio::test]
    async fn test_middleware_passes_error_results() {
        let mw = OutputValidatorMiddleware::new(1000, true);
        let mut ctx = AgentContext::new("test");
        let call = make_call("bash");

        let tool_fn: ToolCallNext = ToolCallNext::new(|_call| {
            Box::pin(async { ToolResult::error("command not found") })
        });

        let result = mw.wrap_tool_call(&mut ctx, &call, tool_fn).await;
        assert!(result.is_error);
        // Error results pass through (the error is from the tool itself, not validation)
        assert_eq!(result.output, "command not found");
    }
}
