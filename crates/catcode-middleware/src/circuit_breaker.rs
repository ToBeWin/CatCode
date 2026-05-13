use async_trait::async_trait;
use catcode_core::middleware::{AgentContext, Middleware, ToolCallNext};
use catcode_core::tool::{ToolCall, ToolResult};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Instant;

/// Circuit Breaker states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Normal operation — requests pass through.
/// [`Closed`].
    Closed,
    /// Too many failures — requests are blocked immediately.
/// [`Open`].
    Open,
    /// Probing — a limited number of requests pass through to test recovery.
/// [`HalfOpen`].
    HalfOpen,
}

/// Middleware that implements the Circuit Breaker pattern.
///
/// After `failure_threshold` consecutive failures, the circuit opens and
/// all tool calls fail immediately with a "circuit open" error. After
/// `recovery_timeout_secs`, the circuit moves to half-open and allows
/// `half_open_max_calls` probe calls. If all probes succeed, the circuit
/// closes; if any fail, it re-opens.
#[derive(Debug)]
pub struct CircuitBreakerMiddleware {
    failure_threshold: u32,
    recovery_timeout_secs: u64,
    half_open_max_calls: u32,
    state: Mutex<CircuitState>,
    consecutive_failures: AtomicU32,
    half_open_calls: AtomicU32,
    opened_at: Mutex<Option<Instant>>,
}

impl CircuitBreakerMiddleware {
    pub fn new(failure_threshold: u32, recovery_timeout_secs: u64, half_open_max_calls: u32) -> Self {
        Self {
            failure_threshold,
            recovery_timeout_secs,
            half_open_max_calls,
            state: Mutex::new(CircuitState::Closed),
            consecutive_failures: AtomicU32::new(0),
            half_open_calls: AtomicU32::new(0),
            opened_at: Mutex::new(None),
        }
    }

    /// Get the current circuit state.
    pub fn state(&self) -> CircuitState {
        *self.state.lock().unwrap()
    }

    /// Reset the circuit breaker to closed state.
    pub fn reset(&self) {
        *self.state.lock().unwrap() = CircuitState::Closed;
        self.consecutive_failures.store(0, Ordering::SeqCst);
        self.half_open_calls.store(0, Ordering::SeqCst);
        *self.opened_at.lock().unwrap() = None;
    }

    /// Check if the circuit should transition from Open to HalfOpen.
    fn check_recovery(&self) {
        let state = self.state.lock().unwrap();
        if *state != CircuitState::Open {
            return;
        }
        drop(state);

        let should_transition = {
            let opened = self.opened_at.lock().unwrap();
            matches!(*opened, Some(instant) if instant.elapsed().as_secs() >= self.recovery_timeout_secs)
        };
        if should_transition {
            let mut state = self.state.lock().unwrap();
            *state = CircuitState::HalfOpen;
            self.half_open_calls.store(0, Ordering::SeqCst);
            tracing::info!("Circuit breaker transitioning to HalfOpen");
        }
    }

    /// Record a successful call.
    fn record_success(&self) {
        let state = self.state.lock().unwrap();
        match *state {
            CircuitState::Closed => {
                self.consecutive_failures.store(0, Ordering::SeqCst);
            }
            CircuitState::HalfOpen => {
                let calls = self.half_open_calls.fetch_add(1, Ordering::SeqCst) + 1;
                if calls >= self.half_open_max_calls {
                    drop(state);
                    let mut state = self.state.lock().unwrap();
                    *state = CircuitState::Closed;
                    self.consecutive_failures.store(0, Ordering::SeqCst);
                    *self.opened_at.lock().unwrap() = None;
                    tracing::info!("Circuit breaker closed after successful probes");
                }
            }
            CircuitState::Open => {}
        }
    }

    /// Record a failed call.
    fn record_failure(&self) {
        let state = self.state.lock().unwrap();
        match *state {
            CircuitState::Closed => {
                let failures = self.consecutive_failures.fetch_add(1, Ordering::SeqCst) + 1;
                if failures >= self.failure_threshold {
                    drop(state);
                    let mut state = self.state.lock().unwrap();
                    *state = CircuitState::Open;
                    *self.opened_at.lock().unwrap() = Some(Instant::now());
                    tracing::warn!(
                        failures = failures,
                        "Circuit breaker opened due to consecutive failures"
                    );
                }
            }
            CircuitState::HalfOpen => {
                // Any failure in half-open re-opens the circuit
                drop(state);
                let mut state = self.state.lock().unwrap();
                *state = CircuitState::Open;
                *self.opened_at.lock().unwrap() = Some(Instant::now());
                self.half_open_calls.store(0, Ordering::SeqCst);
                tracing::warn!("Circuit breaker re-opened after half-open failure");
            }
            CircuitState::Open => {}
        }
    }
}

impl Default for CircuitBreakerMiddleware {
    fn default() -> Self {
        Self::new(5, 30, 2)
    }
}

#[async_trait]
impl Middleware for CircuitBreakerMiddleware {
    fn name(&self) -> &str {
        "circuit_breaker"
    }

    async fn wrap_tool_call(
        &self,
        _ctx: &mut AgentContext,
        call: &ToolCall,
        next: ToolCallNext<'_>,
    ) -> ToolResult {
        // Check if we should transition from Open to HalfOpen
        self.check_recovery();

        // Read current state in a scoped block to ensure MutexGuard is dropped
        let current_state = { *self.state.lock().unwrap() };

        match current_state {
            CircuitState::Open => {
                tracing::debug!(tool = %call.name, "Tool call blocked by circuit breaker (Open)");
                ToolResult::error(format!(
                    "[circuit_breaker] Circuit is open — too many consecutive failures. \
                     Will retry after {}s recovery period.",
                    self.recovery_timeout_secs
                ))
            }
            CircuitState::Closed | CircuitState::HalfOpen => {
                let result = next.execute(call).await;

                if result.is_error {
                    self.record_failure();
                } else {
                    self.record_success();
                }

                result
            }
        }
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

    #[tokio::test]
    async fn test_circuit_starts_closed() {
        let cb = CircuitBreakerMiddleware::new(3, 1, 1);
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_circuit_stays_closed_on_success() {
        let cb = CircuitBreakerMiddleware::new(3, 1, 1);
        let mut ctx = AgentContext::new("test");
        let call = make_call("tool");

        let tool_fn: ToolCallNext = ToolCallNext::new(|_call| {
            Box::pin(async { ToolResult::success("ok") })
        });

        let result = cb.wrap_tool_call(&mut ctx, &call, tool_fn).await;
        assert!(!result.is_error);
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_circuit_opens_after_threshold() {
        let cb = CircuitBreakerMiddleware::new(3, 60, 1);
        let mut ctx = AgentContext::new("test");
        let call = make_call("tool");

        // Fail 3 times to open the circuit
        for _ in 0..3 {
            let tool_fn: ToolCallNext = ToolCallNext::new(|_call| {
                Box::pin(async { ToolResult::error("fail") })
            });
            cb.wrap_tool_call(&mut ctx, &call, tool_fn).await;
        }

        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[tokio::test]
    async fn test_circuit_blocks_when_open() {
        let cb = CircuitBreakerMiddleware::new(2, 60, 1);
        let mut ctx = AgentContext::new("test");
        let call = make_call("tool");

        // Open the circuit
        for _ in 0..2 {
            let tool_fn: ToolCallNext = ToolCallNext::new(|_call| {
                Box::pin(async { ToolResult::error("fail") })
            });
            cb.wrap_tool_call(&mut ctx, &call, tool_fn).await;
        }

        // Now the circuit is open — calls should be blocked without executing
        let tool_fn: ToolCallNext = ToolCallNext::new(|_call| {
            Box::pin(async { ToolResult::success("should not run") })
        });
        let result = cb.wrap_tool_call(&mut ctx, &call, tool_fn).await;
        assert!(result.is_error, "Expected blocked call, got: {}", result.output);
        assert!(
            result.output.contains("circuit is open") || result.output.contains("Circuit is open"),
            "Expected circuit open message, got: {}",
            result.output
        );
    }

    #[tokio::test]
    async fn test_circuit_resets_on_success_in_closed() {
        let cb = CircuitBreakerMiddleware::new(3, 60, 1);
        let mut ctx = AgentContext::new("test");
        let call = make_call("tool");

        // Fail twice (below threshold)
        for _ in 0..2 {
            let tool_fn: ToolCallNext = ToolCallNext::new(|_call| {
                Box::pin(async { ToolResult::error("fail") })
            });
            cb.wrap_tool_call(&mut ctx, &call, tool_fn).await;
        }
        assert_eq!(cb.state(), CircuitState::Closed);

        // Success resets the counter
        let tool_fn: ToolCallNext = ToolCallNext::new(|_call| {
            Box::pin(async { ToolResult::success("ok") })
        });
        cb.wrap_tool_call(&mut ctx, &call, tool_fn).await;

        // Should still be closed, and consecutive failures reset
        assert_eq!(cb.state(), CircuitState::Closed);
        assert_eq!(cb.consecutive_failures.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn test_circuit_manual_reset() {
        let cb = CircuitBreakerMiddleware::new(1, 3600, 1);
        let mut ctx = AgentContext::new("test");
        let call = make_call("tool");

        // Open the circuit
        let tool_fn: ToolCallNext = ToolCallNext::new(|_call| {
            Box::pin(async { ToolResult::error("fail") })
        });
        cb.wrap_tool_call(&mut ctx, &call, tool_fn).await;
        assert_eq!(cb.state(), CircuitState::Open);

        // Manual reset
        cb.reset();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert_eq!(cb.consecutive_failures.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn test_circuit_half_open_recovery() {
        // Use 0 recovery timeout for testing
        let cb = CircuitBreakerMiddleware::new(1, 0, 1);
        let mut ctx = AgentContext::new("test");
        let call = make_call("tool");

        // Open the circuit
        let tool_fn: ToolCallNext = ToolCallNext::new(|_call| {
            Box::pin(async { ToolResult::error("fail") })
        });
        cb.wrap_tool_call(&mut ctx, &call, tool_fn).await;
        assert_eq!(cb.state(), CircuitState::Open);

        // After recovery timeout, should transition to half-open
        // (0-second timeout means immediate transition on next call)
        let tool_fn: ToolCallNext = ToolCallNext::new(|_call| {
            Box::pin(async { ToolResult::success("recovered") })
        });
        let result = cb.wrap_tool_call(&mut ctx, &call, tool_fn).await;
        assert!(!result.is_error);
        // After enough successful probes, should be closed
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_circuit_half_open_failure_reopens() {
        let cb = CircuitBreakerMiddleware::new(1, 0, 2);
        let mut ctx = AgentContext::new("test");
        let call = make_call("tool");

        // Open the circuit
        let tool_fn: ToolCallNext = ToolCallNext::new(|_call| {
            Box::pin(async { ToolResult::error("fail") })
        });
        cb.wrap_tool_call(&mut ctx, &call, tool_fn).await;

        // First probe succeeds (half-open)
        let tool_fn: ToolCallNext = ToolCallNext::new(|_call| {
            Box::pin(async { ToolResult::success("ok") })
        });
        cb.wrap_tool_call(&mut ctx, &call, tool_fn).await;
        assert_eq!(cb.state(), CircuitState::HalfOpen);

        // Second probe fails — should re-open
        let tool_fn: ToolCallNext = ToolCallNext::new(|_call| {
            Box::pin(async { ToolResult::error("fail again") })
        });
        cb.wrap_tool_call(&mut ctx, &call, tool_fn).await;
        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[test]
    fn test_default_config() {
        let cb = CircuitBreakerMiddleware::default();
        assert_eq!(cb.failure_threshold, 5);
        assert_eq!(cb.recovery_timeout_secs, 30);
        assert_eq!(cb.half_open_max_calls, 2);
    }
}
