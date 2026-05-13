//! Benchmark system for evaluating provider+model combinations.
//!
//! Tracks success rate, token usage, latency, and cost across test cases.

use serde::{Deserialize, Serialize};

/// A benchmark test case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkCase {
    pub id: String,
    pub name: String,
    pub description: String,
    /// The prompt/task to send to the model.
    pub prompt: String,
    /// Expected output pattern (substring match or regex).
    pub expected_pattern: String,
    /// Whether to use regex matching (default: substring).
    pub use_regex: bool,
}

/// Result of running a single benchmark case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub case_id: String,
    pub provider_id: String,
    pub model_id: String,
    pub passed: bool,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_tokens: u64,
    pub latency_ms: u64,
    pub cost_usd: f64,
    pub output_preview: String,
    pub error: Option<String>,
}

/// Aggregated benchmark report for a provider+model combination.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkReport {
    pub provider_id: String,
    pub model_id: String,
    pub total_cases: usize,
    pub passed: usize,
    pub failed: usize,
    pub pass_rate: f64,
    pub avg_latency_ms: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_tokens: u64,
    pub total_cost_usd: f64,
    pub results: Vec<BenchmarkResult>,
}

impl BenchmarkReport {
/// Aggregate benchmark results into a report.
    pub fn from_results(provider_id: &str, model_id: &str, results: Vec<BenchmarkResult>) -> Self {
        let total = results.len();
        let passed = results.iter().filter(|r| r.passed).count();
        let failed = total - passed;
        let pass_rate = if total > 0 {
            passed as f64 / total as f64
        } else {
            0.0
        };
        let avg_latency = if total > 0 {
            results.iter().map(|r| r.latency_ms).sum::<u64>() / total as u64
        } else {
            0
        };

        Self {
            provider_id: provider_id.to_string(),
            model_id: model_id.to_string(),
            total_cases: total,
            passed,
            failed,
            pass_rate,
            avg_latency_ms: avg_latency,
            total_input_tokens: results.iter().map(|r| r.input_tokens).sum(),
            total_output_tokens: results.iter().map(|r| r.output_tokens).sum(),
            total_cache_tokens: results.iter().map(|r| r.cache_tokens).sum(),
            total_cost_usd: results.iter().map(|r| r.cost_usd).sum(),
            results,
        }
    }

/// One-line summary of the benchmark report.
    pub fn summary_line(&self) -> String {
        format!(
            "{}/{}: {}/{} passed ({:.0}%) | avg {}ms | ${:.4} | {}ms avg latency",
            self.provider_id,
            self.model_id,
            self.passed,
            self.total_cases,
            self.pass_rate * 100.0,
            self.model_id,
            self.total_cost_usd,
            self.avg_latency_ms,
        )
    }
}

/// Built-in benchmark test cases for coding tasks.
pub fn default_benchmark_cases() -> Vec<BenchmarkCase> {
    vec![
        BenchmarkCase {
            id: "hello-world".to_string(),
            name: "Hello World".to_string(),
            description: "Generate a hello world function".to_string(),
            prompt: "Write a Rust function called `hello_world` that returns the string `\"Hello, World!\"`. Only output the function, no explanation.".to_string(),
            expected_pattern: "fn hello_world".to_string(),
            use_regex: false,
        },
        BenchmarkCase {
            id: "fibonacci".to_string(),
            name: "Fibonacci".to_string(),
            description: "Generate a fibonacci function".to_string(),
            prompt: "Write a Rust function `fibonacci(n: u64) -> u64` that returns the nth fibonacci number using iteration. Only output the function.".to_string(),
            expected_pattern: "fn fibonacci".to_string(),
            use_regex: false,
        },
        BenchmarkCase {
            id: "binary-search".to_string(),
            name: "Binary Search".to_string(),
            description: "Generate a binary search function".to_string(),
            prompt: "Write a Rust function `binary_search(arr: &[i32], target: i32) -> Option<usize>` that performs binary search. Only output the function.".to_string(),
            expected_pattern: "fn binary_search".to_string(),
            use_regex: false,
        },
        BenchmarkCase {
            id: "reverse-string".to_string(),
            name: "Reverse String".to_string(),
            description: "Generate a string reversal function".to_string(),
            prompt: "Write a Rust function `reverse_string(s: &str) -> String` that reverses a string. Only output the function.".to_string(),
            expected_pattern: "fn reverse_string".to_string(),
            use_regex: false,
        },
        BenchmarkCase {
            id: "factorial".to_string(),
            name: "Factorial".to_string(),
            description: "Generate a factorial function".to_string(),
            prompt: "Write a Rust function `factorial(n: u64) -> u64` that computes factorial iteratively. Only output the function.".to_string(),
            expected_pattern: "fn factorial".to_string(),
            use_regex: false,
        },
    ]
}

/// Format a benchmark report as a table.
pub fn format_report_table(report: &BenchmarkReport) -> String {
    let mut lines = vec![format!(
        "=== {}/{} ===",
        report.provider_id, report.model_id
    )];
    lines.push(format!(
        "Pass rate: {}/{} ({:.0}%)",
        report.passed, report.total_cases, report.pass_rate * 100.0
    ));
    lines.push(format!("Avg latency: {}ms", report.avg_latency_ms));
    lines.push(format!(
        "Tokens: {}in / {}out / {}cache",
        report.total_input_tokens, report.total_output_tokens, report.total_cache_tokens
    ));
    lines.push(format!("Total cost: ${:.4}", report.total_cost_usd));
    lines.push(String::new());

    for result in &report.results {
        let status = if result.passed { "PASS" } else { "FAIL" };
        lines.push(format!(
            "  [{}] {} — {}ms, {} tokens",
            status, result.case_id, result.latency_ms, result.input_tokens
        ));
        if let Some(err) = &result.error {
            lines.push(format!("        error: {}", err));
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_benchmark_case_serialization() {
        let case = default_benchmark_cases().remove(0);
        let json = serde_json::to_string(&case).unwrap();
        assert!(json.contains("hello-world"));
        let deserialized: BenchmarkCase = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "hello-world");
    }

    #[test]
    fn test_benchmark_result_serialization() {
        let result = BenchmarkResult {
            case_id: "test".to_string(),
            provider_id: "anthropic".to_string(),
            model_id: "claude-sonnet-4".to_string(),
            passed: true,
            input_tokens: 100,
            output_tokens: 50,
            cache_tokens: 20,
            latency_ms: 500,
            cost_usd: 0.001,
            output_preview: "fn test()".to_string(),
            error: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        let deserialized: BenchmarkResult = serde_json::from_str(&json).unwrap();
        assert!(deserialized.passed);
    }

    #[test]
    fn test_benchmark_report_from_results() {
        let results = vec![
            BenchmarkResult {
                case_id: "a".to_string(),
                provider_id: "test".to_string(),
                model_id: "model".to_string(),
                passed: true,
                input_tokens: 100,
                output_tokens: 50,
                cache_tokens: 0,
                latency_ms: 200,
                cost_usd: 0.001,
                output_preview: String::new(),
                error: None,
            },
            BenchmarkResult {
                case_id: "b".to_string(),
                provider_id: "test".to_string(),
                model_id: "model".to_string(),
                passed: false,
                input_tokens: 200,
                output_tokens: 100,
                cache_tokens: 0,
                latency_ms: 400,
                cost_usd: 0.002,
                output_preview: String::new(),
                error: Some("timeout".to_string()),
            },
        ];

        let report = BenchmarkReport::from_results("test", "model", results);
        assert_eq!(report.total_cases, 2);
        assert_eq!(report.passed, 1);
        assert_eq!(report.failed, 1);
        assert!((report.pass_rate - 0.5).abs() < 0.01);
        assert_eq!(report.avg_latency_ms, 300);
        assert_eq!(report.total_input_tokens, 300);
        assert_eq!(report.total_cost_usd, 0.003);
    }

    #[test]
    fn test_benchmark_report_summary() {
        let report = BenchmarkReport::from_results("anthropic", "claude-sonnet-4", vec![]);
        let summary = report.summary_line();
        assert!(summary.contains("anthropic"));
        assert!(summary.contains("claude-sonnet-4"));
    }

    #[test]
    fn test_default_benchmark_cases() {
        let cases = default_benchmark_cases();
        assert_eq!(cases.len(), 5);
        assert!(cases.iter().any(|c| c.id == "hello-world"));
        assert!(cases.iter().any(|c| c.id == "fibonacci"));
    }

    #[test]
    fn test_format_report_table() {
        let report = BenchmarkReport::from_results("test", "model", vec![]);
        let table = format_report_table(&report);
        assert!(table.contains("=== test/model ==="));
        assert!(table.contains("Pass rate"));
    }

    #[test]
    fn test_format_report_table_with_results() {
        let results = vec![BenchmarkResult {
            case_id: "hello".to_string(),
            provider_id: "test".to_string(),
            model_id: "model".to_string(),
            passed: true,
            input_tokens: 50,
            output_tokens: 30,
            cache_tokens: 0,
            latency_ms: 100,
            cost_usd: 0.0005,
            output_preview: "fn hello".to_string(),
            error: None,
        }];
        let report = BenchmarkReport::from_results("test", "model", results);
        let table = format_report_table(&report);
        assert!(table.contains("[PASS]"));
        assert!(table.contains("hello"));
    }
}
