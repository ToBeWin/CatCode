use async_trait::async_trait;
use catcode_core::{OperationLevel, Tool, ToolContext, ToolResult};
use serde_json::{Value, json};

/// Fetches content from a URL via HTTP GET.
///
/// Parameters:
/// - `url` (string, required): URL to fetch.
/// - `format` (string, optional): Response format — "text", "markdown", or "html". Default: "text".
/// - `timeout` (integer, optional): Timeout in seconds. Default: 30.
pub struct WebFetchTool;

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        "Fetch content from a URL via HTTP GET. Returns the response body as text."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "URL to fetch."
                },
                "format": {
                    "type": "string",
                    "enum": ["text", "markdown", "html"],
                    "description": "Response format. Default: 'text'."
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in seconds. Default: 30.",
                    "minimum": 1,
                    "maximum": 300
                }
            },
            "required": ["url"]
        })
    }

    fn operation_level(&self) -> OperationLevel {
        OperationLevel::Dangerous
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        let url = match args.get("url").and_then(|v| v.as_str()) {
            Some(u) => u.trim(),
            None => return ToolResult::error("Missing required argument: url"),
        };

        if url.is_empty() {
            return ToolResult::error("URL cannot be empty");
        }

        // Validate URL format
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return ToolResult::error(format!("Invalid URL scheme: {}. Must start with http:// or https://", url));
        }

        let timeout_secs = args
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(30)
            .clamp(1, 300);

        let _format = args
            .get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("text");

        if ctx.dry_run {
            return ToolResult::success(format!(
                "[dry-run] Would fetch {} (timeout: {}s, format: {})",
                url, timeout_secs, _format
            ));
        }

        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .user_agent("CatCode/1.0")
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                return ToolResult::error(format!("Failed to create HTTP client: {}", e));
            }
        };

        let response = match client.get(url).send().await {
            Ok(r) => r,
            Err(e) => {
                if e.is_timeout() {
                    return ToolResult::error(format!(
                        "Request timed out after {}s: {}",
                        timeout_secs, url
                    ));
                }
                if e.is_connect() {
                    return ToolResult::error(format!(
                        "Failed to connect to {}: {}",
                        url, e
                    ));
                }
                return ToolResult::error(format!("Request failed for {}: {}", url, e));
            }
        };

        let status = response.status();
        if !status.is_success() {
            return ToolResult::error(format!(
                "HTTP {} {} for {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("Unknown"),
                url
            ));
        }

        let body = match response.text().await {
            Ok(b) => b,
            Err(e) => {
                return ToolResult::error(format!("Failed to read response body: {}", e));
            }
        };

        ToolResult::success(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_core::{Tool, ToolContext};
    use serde_json::json;

    fn make_ctx() -> ToolContext {
        ToolContext {
            session_id: Some("test".to_string()),
            project_dir: None,
            working_dir: None,
            dry_run: false,
        }
    }

    #[tokio::test]
    async fn test_web_fetch_invalid_url_scheme() {
        let tool = WebFetchTool;
        let ctx = make_ctx();
        let result = tool
            .execute(json!({"url": "ftp://example.com/file"}), &ctx)
            .await;

        assert!(result.is_error);
        assert!(result.output.contains("URL scheme"));
    }

    #[tokio::test]
    async fn test_web_fetch_empty_url() {
        let tool = WebFetchTool;
        let ctx = make_ctx();
        let result = tool
            .execute(json!({"url": ""}), &ctx)
            .await;

        assert!(result.is_error);
        assert!(result.output.contains("empty"));
    }

    #[tokio::test]
    async fn test_web_fetch_missing_url() {
        let tool = WebFetchTool;
        let ctx = make_ctx();
        let result = tool.execute(json!({}), &ctx).await;

        assert!(result.is_error);
        assert!(result.output.contains("url"));
    }

    #[tokio::test]
    async fn test_web_fetch_dry_run() {
        let tool = WebFetchTool;
        let ctx = ToolContext {
            session_id: Some("test".to_string()),
            project_dir: None,
            working_dir: None,
            dry_run: true,
        };
        let result = tool
            .execute(json!({"url": "https://example.com"}), &ctx)
            .await;

        assert!(!result.is_error);
        assert!(result.output.contains("dry-run"));
    }

    #[tokio::test]
    async fn test_web_fetch_unreachable() {
        let tool = WebFetchTool;
        let ctx = make_ctx();
        let result = tool
            .execute(json!({"url": "https://127.0.0.1:1/"}), &ctx)
            .await;

        assert!(result.is_error);
    }

    #[test]
    fn test_web_fetch_metadata() {
        let tool = WebFetchTool;
        assert_eq!(tool.name(), "web_fetch");
        assert!(matches!(
            tool.operation_level(),
            catcode_core::OperationLevel::Dangerous
        ));
    }

    #[test]
    fn test_web_fetch_schema() {
        let tool = WebFetchTool;
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["url"].is_object());
        assert!(schema["properties"]["format"].is_object());
        assert!(schema["properties"]["timeout"].is_object());
    }
}
