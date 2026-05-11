use catcode_core::TokenUsage;

/// Token budget tracker for a session.
///
/// Tracks cumulative token usage (input, output, cache reads) and enforces
/// session-level and per-request limits. Provides cost estimation and
/// warning threshold checks.
///
/// # Example
///
/// ```
/// use catcode_context::TokenBudget;
/// use catcode_core::TokenUsage;
///
/// let mut budget = TokenBudget::new(500_000, 50_000, 0.80);
/// budget.record_usage(&TokenUsage {
///     input_tokens: 1000,
///     output_tokens: 500,
///     cache_read_tokens: 200,
///     cache_creation_tokens: 0,
/// });
/// assert!(!budget.is_exhausted());
/// ```
#[derive(Debug, Clone)]
pub struct TokenBudget {
    /// Maximum total tokens allowed for this session.
    pub session_limit: u64,
    /// Maximum tokens per individual request.
    pub per_request_limit: u64,
    /// Ratio (0.0-1.0) at which a warning should be emitted.
    pub warning_threshold: f32,
    /// Cumulative input tokens used.
    pub input_used: u64,
    /// Cumulative output tokens used.
    pub output_used: u64,
    /// Cumulative cache-read tokens (cost savings).
    pub cache_read: u64,
}

impl TokenBudget {
    /// Create a new token budget.
    ///
    /// # Arguments
    /// * `session_limit` — maximum total tokens for the session
    /// * `per_request_limit` — maximum tokens for a single request
    /// * `warning_threshold` — ratio (0.0-1.0) at which to emit warnings
    pub fn new(session_limit: u64, per_request_limit: u64, warning_threshold: f32) -> Self {
        Self {
            session_limit,
            per_request_limit,
            warning_threshold: warning_threshold.clamp(0.0, 1.0),
            input_used: 0,
            output_used: 0,
            cache_read: 0,
        }
    }

    /// Record token usage from an API response.
    ///
    /// Accumulates input, output, and cache-read tokens into the running totals.
    pub fn record_usage(&mut self, usage: &TokenUsage) {
        self.input_used += usage.input_tokens;
        self.output_used += usage.output_tokens;
        self.cache_read += usage.cache_read_tokens;
    }

    /// Calculate the remaining token ratio for the session.
    ///
    /// Returns a value between 0.0 (fully exhausted) and 1.0 (nothing used).
    /// Returns 1.0 if session_limit is 0 (unlimited).
    pub fn remaining_ratio(&self) -> f32 {
        if self.session_limit == 0 {
            return 1.0;
        }
        let used = self.input_used + self.output_used;
        let remaining = self.session_limit.saturating_sub(used);
        remaining as f32 / self.session_limit as f32
    }

    /// Check if usage has crossed the warning threshold.
    ///
    /// Returns `true` if the used ratio exceeds `warning_threshold`.
    pub fn should_warn(&self) -> bool {
        if self.session_limit == 0 {
            return false;
        }
        let used_ratio = 1.0 - self.remaining_ratio();
        used_ratio >= self.warning_threshold
    }

    /// Check if the session budget is exhausted.
    ///
    /// Returns `true` if total usage (input + output) meets or exceeds
    /// the session limit. Always returns `false` if the limit is 0 (unlimited).
    pub fn is_exhausted(&self) -> bool {
        if self.session_limit == 0 {
            return false;
        }
        (self.input_used + self.output_used) >= self.session_limit
    }

    /// Estimate the cost in USD for the current usage.
    ///
    /// Prices are per million tokens. The estimate uses raw input and output
    /// token counts (cache-read tokens are not subtracted — providers may
    /// offer discounts, but that is provider-specific).
    ///
    /// # Arguments
    /// * `input_price` — USD price per million input tokens
    /// * `output_price` — USD price per million output tokens
    pub fn estimate_cost(&self, input_price: f64, output_price: f64) -> f64 {
        let input_cost = self.input_used as f64 * input_price / 1_000_000.0;
        let output_cost = self.output_used as f64 * output_price / 1_000_000.0;
        input_cost + output_cost
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_budget() -> TokenBudget {
        TokenBudget::new(500_000, 50_000, 0.80)
    }

    #[test]
    fn test_new_budget() {
        let budget = make_budget();
        assert_eq!(budget.session_limit, 500_000);
        assert_eq!(budget.per_request_limit, 50_000);
        assert!((budget.warning_threshold - 0.80).abs() < 0.01);
        assert_eq!(budget.input_used, 0);
        assert_eq!(budget.output_used, 0);
        assert_eq!(budget.cache_read, 0);
    }

    #[test]
    fn test_record_usage() {
        let mut budget = make_budget();
        budget.record_usage(&TokenUsage {
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: 200,
            cache_creation_tokens: 100,
        });
        assert_eq!(budget.input_used, 1000);
        assert_eq!(budget.output_used, 500);
        assert_eq!(budget.cache_read, 200);
    }

    #[test]
    fn test_record_usage_accumulates() {
        let mut budget = make_budget();
        budget.record_usage(&TokenUsage {
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
        });
        budget.record_usage(&TokenUsage {
            input_tokens: 2000,
            output_tokens: 1000,
            cache_read_tokens: 100,
            cache_creation_tokens: 0,
        });
        assert_eq!(budget.input_used, 3000);
        assert_eq!(budget.output_used, 1500);
        assert_eq!(budget.cache_read, 100);
    }

    #[test]
    fn test_remaining_ratio_full() {
        let budget = make_budget();
        assert!((budget.remaining_ratio() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_remaining_ratio_partial() {
        let mut budget = make_budget();
        budget.input_used = 250_000;
        budget.output_used = 0;
        // used = 250k out of 500k => remaining = 0.5
        assert!((budget.remaining_ratio() - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_remaining_ratio_exhausted() {
        let mut budget = make_budget();
        budget.input_used = 500_000;
        budget.output_used = 0;
        assert!((budget.remaining_ratio()).abs() < 0.001);
    }

    #[test]
    fn test_remaining_ratio_unlimited() {
        let budget = TokenBudget::new(0, 50_000, 0.80);
        assert!((budget.remaining_ratio() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_should_warn_below_threshold() {
        let mut budget = make_budget();
        budget.input_used = 300_000;
        budget.output_used = 0;
        // used ratio = 0.60, threshold = 0.80
        assert!(!budget.should_warn());
    }

    #[test]
    fn test_should_warn_at_threshold() {
        let mut budget = make_budget();
        budget.input_used = 400_000;
        budget.output_used = 0;
        // used ratio = 0.80, threshold = 0.80
        assert!(budget.should_warn());
    }

    #[test]
    fn test_should_warn_above_threshold() {
        let mut budget = make_budget();
        budget.input_used = 450_000;
        budget.output_used = 0;
        // used ratio = 0.90
        assert!(budget.should_warn());
    }

    #[test]
    fn test_should_warn_unlimited() {
        let budget = TokenBudget::new(0, 50_000, 0.80);
        assert!(!budget.should_warn());
    }

    #[test]
    fn test_is_exhausted_not() {
        let mut budget = make_budget();
        budget.input_used = 400_000;
        budget.output_used = 50_000;
        assert!(!budget.is_exhausted());
    }

    #[test]
    fn test_is_exhausted_exact() {
        let mut budget = make_budget();
        budget.input_used = 400_000;
        budget.output_used = 100_000;
        assert!(budget.is_exhausted());
    }

    #[test]
    fn test_is_exhausted_over() {
        let mut budget = make_budget();
        budget.input_used = 500_000;
        budget.output_used = 100_000;
        assert!(budget.is_exhausted());
    }

    #[test]
    fn test_is_exhausted_unlimited() {
        let budget = TokenBudget::new(0, 50_000, 0.80);
        assert!(!budget.is_exhausted());
    }

    #[test]
    fn test_estimate_cost() {
        let mut budget = make_budget();
        budget.input_used = 1_000_000;
        budget.output_used = 500_000;
        // $3/M input, $15/M output
        let cost = budget.estimate_cost(3.0, 15.0);
        // input: 1M * 3/M = $3.0, output: 0.5M * 15/M = $7.5 => $10.5
        assert!((cost - 10.5).abs() < 0.01);
    }

    #[test]
    fn test_estimate_cost_zero() {
        let budget = make_budget();
        let cost = budget.estimate_cost(3.0, 15.0);
        assert!((cost).abs() < 0.0001);
    }

    #[test]
    fn test_warning_threshold_clamped() {
        let budget = TokenBudget::new(500_000, 50_000, 1.5);
        assert!((budget.warning_threshold - 1.0).abs() < 0.001);

        let budget = TokenBudget::new(500_000, 50_000, -0.5);
        assert!((budget.warning_threshold).abs() < 0.001);
    }
}
