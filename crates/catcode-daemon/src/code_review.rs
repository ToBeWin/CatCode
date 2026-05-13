use catcode_core::provider::{Provider, ProviderContext};
use catcode_core::{ChatRequest, Message, Role};
use std::sync::Arc;

/// Severity level of a review finding.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ReviewSeverity {
/// [`Error`].
    Error,
/// [`Warning`].
    Warning,
/// [`Info`].
    Info,
}

/// Category of a review finding.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ReviewCategory {
/// [`Bug`].
    Bug,
/// [`Security`].
    Security,
/// [`Performance`].
    Performance,
/// [`Style`].
    Style,
/// [`BestPractice`].
    BestPractice,
/// [`Maintainability`].
    Maintainability,
/// [`Documentation`].
    Documentation,
/// [`Testing`].
    Testing,
}

/// A single review finding/comment.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReviewFinding {
    pub severity: ReviewSeverity,
    pub category: ReviewCategory,
    pub file: String,
    pub line: Option<u64>,
    pub title: String,
    pub description: String,
    pub suggestion: Option<String>,
}

/// Full code review result.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CodeReview {
    pub title: String,
    pub summary: String,
    pub files_reviewed: Vec<String>,
    pub findings: Vec<ReviewFinding>,
    pub positive_notes: Vec<String>,
    pub overall_score: u8,
}

impl CodeReview {
    fn new(title: &str) -> Self {
        Self {
            title: title.to_string(),
            summary: String::new(),
            files_reviewed: Vec::new(),
            findings: Vec::new(),
            positive_notes: Vec::new(),
            overall_score: 100,
        }
    }

    fn compute_score(&mut self) {
        if self.findings.is_empty() {
            self.overall_score = 100;
            return;
        }
        let mut deductions = 0u64;
        for f in &self.findings {
            match f.severity {
                ReviewSeverity::Error => deductions += 15,
                ReviewSeverity::Warning => deductions += 5,
                ReviewSeverity::Info => deductions += 1,
            }
        }
        self.overall_score = 100u8.saturating_sub(deductions.min(100) as u8);
    }
}

/// A code reviewer that performs pattern-based and LLM-assisted analysis.
pub struct CodeReviewer {
    files: Vec<(String, String)>,
    diffs: Vec<String>,
}

impl CodeReviewer {
/// Create a new empty code reviewer.
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            diffs: Vec::new(),
        }
    }

    /// Add a file (path + content) for review.
    pub fn add_file(&mut self, path: &str, content: &str) -> &mut Self {
        self.files.push((path.to_string(), content.to_string()));
        self
    }

    /// Add a git diff output for review.
    pub fn add_diff(&mut self, diff: &str) -> &mut Self {
        self.diffs.push(diff.to_string());
        self
    }

    /// Run pattern-based review (fast, no LLM needed).
    pub fn review_patterns(&self) -> CodeReview {
        let mut review = CodeReview::new("Pattern-based Code Review");

        for (path, content) in &self.files {
            review.files_reviewed.push(path.clone());
            let lines: Vec<&str> = content.lines().collect();

            review
                .findings
                .extend(Self::check_todos(path, &lines));
            review
                .findings
                .extend(Self::check_debug_prints(path, &lines));
            review
                .findings
                .extend(Self::check_secrets(path, &lines));
            review
                .findings
                .extend(Self::check_unwrap(path, &lines));
            review
                .findings
                .extend(Self::check_public_docs(path, &lines));
            review
                .findings
                .extend(Self::check_long_functions(path, &lines));
            review
                .findings
                .extend(Self::check_nesting_depth(path, &lines));
        }

        for diff in &self.diffs {
            let files = Self::parse_diff_files(diff);
            for (path, _old_lines, new_lines) in &files {
                if !review.files_reviewed.contains(path) {
                    review.files_reviewed.push(path.clone());
                }
                review
                    .findings
                    .extend(Self::check_large_diff(path, new_lines.len()));
            }
        }

        review.compute_score();
        review
    }

    /// Run LLM-based deep review.
    pub async fn review_deep(
        &self,
        provider: Arc<dyn Provider>,
        model: &str,
    ) -> CodeReview {
        let mut review = CodeReview::new("Deep LLM Code Review");

        for (path, _) in &self.files {
            if !review.files_reviewed.contains(path) {
                review.files_reviewed.push(path.clone());
            }
        }

        let diff_text = if !self.diffs.is_empty() {
            self.diffs.join("\n---\n")
        } else {
            self.files
                .iter()
                .map(|(p, c)| format!("=== {} ===\n{}", p, c))
                .collect::<Vec<_>>()
                .join("\n")
        };

        if diff_text.trim().is_empty() {
            review.summary = "No content provided for review.".to_string();
            review.overall_score = 100;
            return review;
        }

        let system_prompt = "\
You are an expert code reviewer. Analyze the provided code changes and return your findings \
in a structured format.

For each issue, include:
- severity: one of Error, Warning, Info
- category: one of Bug, Security, Performance, Style, BestPractice, Maintainability, Documentation, Testing
- file: the file path
- line: the line number if applicable, or null
- title: a short title
- description: what the issue is
- suggestion: how to fix it (or null)

Also include positive notes about what was done well.

Return your response in the following JSON format:
{
  \"summary\": \"overall assessment\",
  \"findings\": [
    {
      \"severity\": \"Warning\",
      \"category\": \"Bug\",
      \"file\": \"src/main.rs\",
      \"line\": 42,
      \"title\": \"Potential null dereference\",
      \"description\": \"...\",
      \"suggestion\": \"...\"
    }
  ],
  \"positive_notes\": [\"Good use of error handling\"],
  \"overall_score\": 85
}

Focus on:
- Architecture and design issues
- Logic correctness and edge cases
- API compatibility concerns
- Performance bottlenecks
- Security vulnerabilities
- Test coverage gaps";

        let request = ChatRequest {
            model: model.to_string(),
            messages: vec![
                Message {
                    role: Role::User,
                    content: format!(
                        "Please review the following code changes:\n\n```\n{}\n```",
                        diff_text
                    ),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                },
            ],
            tools: None,
            system: Some(system_prompt.to_string()),
            max_tokens: Some(4096),
            temperature: Some(0.3),
            stream: false,
        };

        let provider_ctx = ProviderContext::default();

        match provider.chat(request, &provider_ctx).await {
            Ok(response) => {
                let text = response.text_content();
                Self::parse_review_response(&text, &mut review);
            }
            Err(e) => {
                review.summary = format!("LLM review failed: {}", e);
                review.overall_score = 0;
            }
        }

        review
    }

    /// Combined review: patterns first, then LLM deep review.
    pub async fn review_full(
        &self,
        provider: Arc<dyn Provider>,
        model: &str,
    ) -> CodeReview {
        let mut review = self.review_patterns();
        let deep = self.review_deep(provider, model).await;

        for f in deep.findings {
            if !review.findings.iter().any(|existing| {
                existing.file == f.file && existing.title == f.title
            }) {
                review.findings.push(f);
            }
        }

        for n in deep.positive_notes {
            if !review.positive_notes.contains(&n) {
                review.positive_notes.push(n);
            }
        }

        if deep.overall_score < review.overall_score {
            review.overall_score = deep.overall_score;
        }
        if !deep.summary.is_empty() {
            review.summary = deep.summary;
        }

        review
    }

    // ── Pattern detector: TODO/FIXME/HACK comments ──

    fn check_todos(path: &str, lines: &[&str]) -> Vec<ReviewFinding> {
        let markers = ["TODO", "FIXME", "HACK", "XXX", "WORKAROUND"];
        let mut findings = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            for marker in &markers {
                if let Some(pos) = line.find(marker) {
                    let comment_start = line[..pos].trim();
                    if comment_start.is_empty()
                        || comment_start.starts_with("//")
                        || comment_start.starts_with('#')
                        || comment_start.starts_with("--")
                        || comment_start.starts_with("/*")
                        || comment_start.starts_with('*')
                    {
                        findings.push(ReviewFinding {
                            severity: ReviewSeverity::Info,
                            category: ReviewCategory::Maintainability,
                            file: path.to_string(),
                            line: Some((i + 1) as u64),
                            title: format!("{} comment left in code", marker),
                            description: format!(
                                "{} marker found at line {}: {}",
                                marker,
                                i + 1,
                                line.trim()
                            ),
                            suggestion: Some(format!(
                                "Resolve the {} before merging or add a tracking issue reference.",
                                marker
                            )),
                        });
                        break;
                    }
                }
            }
        }
        findings
    }

    // ── Pattern detector: debug print statements ──

    fn check_debug_prints(path: &str, lines: &[&str]) -> Vec<ReviewFinding> {
        let mut findings = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("println!") && !trimmed.starts_with("println!(\"TODO")
            {
                findings.push(ReviewFinding {
                    severity: ReviewSeverity::Warning,
                    category: ReviewCategory::Style,
                    file: path.to_string(),
                    line: Some((i + 1) as u64),
                    title: "Debug print statement".to_string(),
                    description: format!("println! used at line {}: {}", i + 1, trimmed),
                    suggestion: Some(
                        "Remove or replace with a proper logging macro (info!, debug!, warn!)."
                            .to_string(),
                    ),
                });
            }
            if trimmed.starts_with("dbg!") {
                findings.push(ReviewFinding {
                    severity: ReviewSeverity::Warning,
                    category: ReviewCategory::Style,
                    file: path.to_string(),
                    line: Some((i + 1) as u64),
                    title: "Debug print statement".to_string(),
                    description: format!("dbg! used at line {}: {}", i + 1, trimmed),
                    suggestion: Some(
                        "Remove dbg! calls before committing. Use logging or a debugger instead."
                            .to_string(),
                    ),
                });
            }
            if trimmed.starts_with("eprintln!") {
                findings.push(ReviewFinding {
                    severity: ReviewSeverity::Info,
                    category: ReviewCategory::Style,
                    file: path.to_string(),
                    line: Some((i + 1) as u64),
                    title: "Debug print statement".to_string(),
                    description: format!("eprintln! used at line {}: {}", i + 1, trimmed),
                    suggestion: Some(
                        "Consider using logging macros (warn!, error!) instead of eprintln!."
                            .to_string(),
                    ),
                });
            }
            if trimmed.starts_with("console.log")
                || trimmed.starts_with("print(")
            {
                findings.push(ReviewFinding {
                    severity: ReviewSeverity::Warning,
                    category: ReviewCategory::Style,
                    file: path.to_string(),
                    line: Some((i + 1) as u64),
                    title: "Debug print statement".to_string(),
                    description: format!(
                        "Console print at line {}: {}",
                        i + 1,
                        trimmed
                    ),
                    suggestion: Some(
                        "Remove debug print statements or use a proper logging framework."
                            .to_string(),
                    ),
                });
            }
        }
        findings
    }

    // ── Pattern detector: hardcoded secrets ──

    fn check_secrets(path: &str, lines: &[&str]) -> Vec<ReviewFinding> {
        let secret_patterns: Vec<(&str, &str)> = vec![
            ("AWS Access Key", "AKIA"),
            ("GitHub Token", "ghp_"),
            ("GitHub Old Token", "gho_"),
            ("GitHub PAT", "github_pat_"),
            ("GitLab Token", "glpat-"),
            ("Slack Token", "xoxb-"),
            ("Slack Webhook", "hooks.slack.com"),
            ("Stripe Key", "sk_live_"),
            ("Stripe Test Key", "sk_test_"),
            ("Generic API Key", "api_key"),
            ("Password", "password"),
            ("Secret", "secret"),
            ("Token", "token"),
        ];

        let mut findings = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            let lower = line.to_lowercase();
            for (name, pattern) in &secret_patterns {
                if lower.contains(&pattern.to_lowercase()) {
                    // skip if part of a known safe pattern (documentation, example, test)
                    let trimmed = line.trim();
                    let ignored = trimmed.starts_with("//")
                        || trimmed.starts_with('#')
                        || trimmed.starts_with("--")
                        || trimmed.starts_with("/*")
                        || trimmed.starts_with('*')
                        || trimmed.starts_with("example")
                        || trimmed.starts_with("///")
                        || lower.contains("example_key")
                        || lower.contains("your_key_here")
                        || lower.contains("placeholder");
                    if !ignored {
                        findings.push(ReviewFinding {
                            severity: ReviewSeverity::Error,
                            category: ReviewCategory::Security,
                            file: path.to_string(),
                            line: Some((i + 1) as u64),
                            title: "Potential hardcoded secret".to_string(),
                            description: format!(
                                "Possible {} detected at line {}: {}",
                                name,
                                i + 1,
                                line.trim()
                            ),
                            suggestion: Some(format!(
                                "Move '{}' to environment variables or a secrets manager.",
                                name
                            )),
                        });
                        break;
                    }
                }
            }
        }
        findings
    }

    // ── Pattern detector: .unwrap() / .expect() calls ──

    fn check_unwrap(path: &str, lines: &[&str]) -> Vec<ReviewFinding> {
        let mut findings = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            // skip comments
            if trimmed.starts_with("//")
                || trimmed.starts_with('#')
                || trimmed.starts_with("--")
                || trimmed.starts_with("/*")
                || trimmed.starts_with('*')
            {
                continue;
            }
            if trimmed.contains(".unwrap()") || trimmed.contains(".unwrap();") {
                findings.push(ReviewFinding {
                    severity: ReviewSeverity::Warning,
                    category: ReviewCategory::BestPractice,
                    file: path.to_string(),
                    line: Some((i + 1) as u64),
                    title: "Unsafe unwrap call".to_string(),
                    description: format!(
                        ".unwrap() call at line {}: {}",
                        i + 1,
                        trimmed
                    ),
                    suggestion: Some(
                        "Replace with proper error handling: match/if-let, ?, or handle the Err case."
                            .to_string(),
                    ),
                });
            }
            if trimmed.contains(".expect(") {
                findings.push(ReviewFinding {
                    severity: ReviewSeverity::Info,
                    category: ReviewCategory::BestPractice,
                    file: path.to_string(),
                    line: Some((i + 1) as u64),
                    title: "Expect call may panic".to_string(),
                    description: format!(
                        ".expect() call at line {}: {}",
                        i + 1,
                        trimmed
                    ),
                    suggestion: Some(
                        "Consider using proper error propagation (?) instead of expect.".to_string(),
                    ),
                });
            }
        }
        findings
    }

    // ── Pattern detector: large diffs ──

    fn check_large_diff(path: &str, new_line_count: usize) -> Vec<ReviewFinding> {
        const THRESHOLD: usize = 500;
        if new_line_count > THRESHOLD {
            vec![ReviewFinding {
                severity: ReviewSeverity::Warning,
                category: ReviewCategory::Maintainability,
                file: path.to_string(),
                line: None,
                title: "Large diff detected".to_string(),
                description: format!(
                    "{} has {} new/changed lines (threshold: {}). Large diffs are hard to review.",
                    path, new_line_count, THRESHOLD
                ),
                suggestion: Some(
                    "Consider splitting this change into smaller, focused commits.".to_string(),
                ),
            }]
        } else {
            Vec::new()
        }
    }

    // ── Pattern detector: missing public API docs ──

    fn check_public_docs(path: &str, lines: &[&str]) -> Vec<ReviewFinding> {
        let mut findings = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("pub fn")
                || trimmed.starts_with("pub struct")
                || trimmed.starts_with("pub enum")
                || trimmed.starts_with("pub trait")
            {
                let has_doc = if i >= 1 {
                    let prev = lines[i - 1].trim();
                    prev.starts_with("///") || prev.starts_with("/**")
                } else {
                    false
                };
                if !has_doc {
                    let name = trimmed.split('{').next().unwrap_or(trimmed);
                    findings.push(ReviewFinding {
                        severity: ReviewSeverity::Info,
                        category: ReviewCategory::Documentation,
                        file: path.to_string(),
                        line: Some((i + 1) as u64),
                        title: "Missing documentation".to_string(),
                        description: format!(
                            "Public item missing doc comment at line {}: {}",
                            i + 1,
                            name
                        ),
                        suggestion: Some(
                            "Add a doc comment (///) explaining the purpose and usage.".to_string(),
                        ),
                    });
                }
            }
        }
        findings
    }

    // ── Pattern detector: long functions ──

    fn check_long_functions(path: &str, lines: &[&str]) -> Vec<ReviewFinding> {
        const MAX_FN_LINES: usize = 100;
        let mut findings = Vec::new();

        let mut i = 0;
        while i < lines.len() {
            let trimmed = lines[i].trim();
            if (trimmed.starts_with("fn ") || trimmed.starts_with("pub fn "))
                && trimmed.contains('(')
            {
                let fn_name = trimmed
                    .split('(')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .trim_start_matches("pub ");
                let fn_start = i;

                // Find matching closing brace (simple brace counting)
                let mut brace_count = 0usize;
                let mut fn_end = fn_start;
                let mut started = false;
                for (j, line) in lines.iter().enumerate().skip(fn_start) {
                    for c in line.chars() {
                        if c == '{' {
                            brace_count += 1;
                            started = true;
                        } else if c == '}' {
                            brace_count = brace_count.saturating_sub(1);
                        }
                    }
                    if started && brace_count == 0 {
                        fn_end = j;
                        break;
                    }
                }

                let fn_lines = fn_end - fn_start + 1;
                if fn_lines > MAX_FN_LINES {
                    findings.push(ReviewFinding {
                        severity: ReviewSeverity::Warning,
                        category: ReviewCategory::Maintainability,
                        file: path.to_string(),
                        line: Some((fn_start + 1) as u64),
                        title: "Very long function".to_string(),
                        description: format!(
                            "Function '{}' is {} lines (threshold: {}). Long functions are hard to understand and maintain.",
                            fn_name, fn_lines, MAX_FN_LINES
                        ),
                        suggestion: Some(
                            "Consider refactoring into smaller helper functions.".to_string(),
                        ),
                    });
                }
                i = fn_end;
            }
            i += 1;
        }
        findings
    }

    // ── Pattern detector: deep nesting ──

    fn check_nesting_depth(path: &str, lines: &[&str]) -> Vec<ReviewFinding> {
        const MAX_NESTING: usize = 4;
        let mut findings = Vec::new();
        let mut depth = 0usize;
        let mut max_depth_line = 0usize;
        let mut max_depth = 0usize;

        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//")
                || trimmed.starts_with('#')
                || trimmed.starts_with("--")
                || trimmed.starts_with("/*")
                || trimmed.starts_with('*')
                || trimmed.starts_with("///")
            {
                continue;
            }

            let open = trimmed.matches('{').count();
            let close = trimmed.matches('}').count();

            // Depth before processing this line's braces
            let effective_depth = depth;
            if effective_depth > max_depth {
                max_depth = effective_depth;
                max_depth_line = i + 1;
            }

            // Update depth
            depth = depth.saturating_add(open).saturating_sub(close);
        }

        if max_depth > MAX_NESTING {
            findings.push(ReviewFinding {
                severity: ReviewSeverity::Warning,
                category: ReviewCategory::Style,
                file: path.to_string(),
                line: Some(max_depth_line as u64),
                title: "Deep nesting detected".to_string(),
                description: format!(
                    "Maximum nesting depth of {} at line {} (threshold: {}). Deep nesting reduces readability.",
                    max_depth, max_depth_line, MAX_NESTING
                ),
                suggestion: Some(
                    "Extract nested blocks into separate functions or use early returns.".to_string(),
                ),
            });
        }
        findings
    }

    // ── Helpers ──

    /// Parse diff text into list of (file_path, old_lines, new_lines).
    fn parse_diff_files(diff: &str) -> Vec<(String, Vec<String>, Vec<String>)> {
        let mut files = Vec::new();
        let mut current_file = String::new();
        let mut old_lines: Vec<String> = Vec::new();
        let mut new_lines: Vec<String> = Vec::new();
        let mut in_hunk = false;

        for line in diff.lines() {
            if line.starts_with("diff --git") {
                if in_hunk && !current_file.is_empty() {
                    files.push((current_file.clone(), old_lines.clone(), new_lines.clone()));
                    old_lines.clear();
                    new_lines.clear();
                }
                in_hunk = false;
                // Extract file path from "diff --git a/xxx b/xxx"
                if let Some(path) = line.split_whitespace().nth(3) {
                    current_file = path.trim_start_matches("b/").to_string();
                }
            } else if line.starts_with("--- ") || line.starts_with("+++ ") {
                // Skip filename headers
            } else if line.starts_with("@@") {
                in_hunk = true;
            } else if in_hunk {
                if line.starts_with("+") && !line.starts_with("+++") {
                    new_lines.push(line[1..].to_string());
                    old_lines.push(String::new());
                } else if line.starts_with("-") && !line.starts_with("---") {
                    old_lines.push(line[1..].to_string());
                    new_lines.push(String::new());
                } else if line.starts_with(' ') {
                    let content = if line.is_empty() {
                        String::new()
                    } else {
                        line.strip_prefix(' ').unwrap_or(line).to_string()
                    };
                    old_lines.push(content.clone());
                    new_lines.push(content);
                }
            }
        }

        if in_hunk && !current_file.is_empty() {
            files.push((current_file, old_lines, new_lines));
        }

        files
    }

    /// Parse the LLM JSON response and merge into review.
    fn parse_review_response(text: &str, review: &mut CodeReview) {
        // Try to extract JSON from code fence
        let json_str = if let Some(start) = text.find("```json") {
            let start = start + 7;
            let end = text[start..].find("```").map(|e| start + e).unwrap_or(text.len());
            text[start..end].trim()
        } else if let Some(start) = text.find('{') {
            let end = text[start..].rfind('}').map(|e| start + e + 1).unwrap_or(text.len());
            &text[start..end]
        } else {
            text
        };

        match serde_json::from_str::<serde_json::Value>(json_str) {
            Ok(val) => {
                if let Some(summary) = val.get("summary").and_then(|v| v.as_str()) {
                    review.summary = summary.to_string();
                }
                if let Some(notes) = val.get("positive_notes").and_then(|v| v.as_array()) {
                    for n in notes {
                        if let Some(s) = n.as_str() {
                            review.positive_notes.push(s.to_string());
                        }
                    }
                }
                if let Some(score) = val.get("overall_score").and_then(|v| v.as_u64()) {
                    review.overall_score = score.min(100) as u8;
                }
                if let Some(findings) = val.get("findings").and_then(|v| v.as_array()) {
                    for f in findings {
                        if let Some(finding) = Self::parse_finding(f) {
                            review.findings.push(finding);
                        }
                    }
                }
            }
            Err(_) => {
                review.summary = text.lines().next().unwrap_or("").to_string();
            }
        }
    }

    fn parse_finding(val: &serde_json::Value) -> Option<ReviewFinding> {
        let severity = match val.get("severity").and_then(|v| v.as_str())? {
            "Error" => ReviewSeverity::Error,
            "Warning" => ReviewSeverity::Warning,
            "Info" => ReviewSeverity::Info,
            _ => return None,
        };
        let category = match val.get("category").and_then(|v| v.as_str())? {
            "Bug" => ReviewCategory::Bug,
            "Security" => ReviewCategory::Security,
            "Performance" => ReviewCategory::Performance,
            "Style" => ReviewCategory::Style,
            "BestPractice" => ReviewCategory::BestPractice,
            "Maintainability" => ReviewCategory::Maintainability,
            "Documentation" => ReviewCategory::Documentation,
            "Testing" => ReviewCategory::Testing,
            _ => return None,
        };
        Some(ReviewFinding {
            severity,
            category,
            file: val.get("file").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            line: val.get("line").and_then(|v| v.as_u64()),
            title: val.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            description: val.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            suggestion: val.get("suggestion").and_then(|v| v.as_str()).map(|s| s.to_string()),
        })
    }
}

impl Default for CodeReviewer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catcode_provider::mock::MockProvider;

    // ── Finding building ──

    #[test]
    fn test_create_review_finding() {
        let f = ReviewFinding {
            severity: ReviewSeverity::Error,
            category: ReviewCategory::Security,
            file: "src/main.rs".to_string(),
            line: Some(42),
            title: "Hardcoded secret".to_string(),
            description: "API key found in source".to_string(),
            suggestion: Some("Use env var".to_string()),
        };
        assert_eq!(f.severity, ReviewSeverity::Error);
        assert_eq!(f.category, ReviewCategory::Security);
        assert_eq!(f.line, Some(42));
    }

    #[test]
    fn test_create_code_review() {
        let review = CodeReview::new("Test Review");
        assert_eq!(review.title, "Test Review");
        assert!(review.findings.is_empty());
        assert!(review.files_reviewed.is_empty());
        assert_eq!(review.overall_score, 100);
    }

    #[test]
    fn test_score_computation() {
        let mut review = CodeReview::new("Score Test");
        review.findings.push(ReviewFinding {
            severity: ReviewSeverity::Error,
            category: ReviewCategory::Bug,
            file: "f.rs".to_string(),
            line: None,
            title: "Bug".to_string(),
            description: "".to_string(),
            suggestion: None,
        });
        review.compute_score();
        assert_eq!(review.overall_score, 85);
    }

    #[test]
    fn test_score_multiple_findings() {
        let mut review = CodeReview::new("Score Test");
        for _ in 0..7 {
            review.findings.push(ReviewFinding {
                severity: ReviewSeverity::Error,
                category: ReviewCategory::Bug,
                file: "f.rs".to_string(),
                line: None,
                title: "Bug".to_string(),
                description: "".to_string(),
                suggestion: None,
            });
        }
        review.compute_score();
        assert_eq!(review.overall_score, 0);
    }

    #[test]
    fn test_score_mixed_severity() {
        let mut review = CodeReview::new("Mixed");
        review.findings.push(ReviewFinding {
            severity: ReviewSeverity::Error,
            category: ReviewCategory::Bug,
            file: "f.rs".to_string(),
            line: None,
            title: "Err".to_string(),
            description: "".to_string(),
            suggestion: None,
        });
        review.findings.push(ReviewFinding {
            severity: ReviewSeverity::Warning,
            category: ReviewCategory::Style,
            file: "f.rs".to_string(),
            line: None,
            title: "Warn".to_string(),
            description: "".to_string(),
            suggestion: None,
        });
        review.findings.push(ReviewFinding {
            severity: ReviewSeverity::Info,
            category: ReviewCategory::Documentation,
            file: "f.rs".to_string(),
            line: None,
            title: "Info".to_string(),
            description: "".to_string(),
            suggestion: None,
        });
        review.compute_score();
        assert_eq!(review.overall_score, 79);
    }

    #[test]
    fn test_empty_review_score() {
        let mut review = CodeReview::new("Empty");
        review.compute_score();
        assert_eq!(review.overall_score, 100);
    }

    // ── Serialization ──

    #[test]
    fn test_finding_serialization() {
        let f = ReviewFinding {
            severity: ReviewSeverity::Warning,
            category: ReviewCategory::Performance,
            file: "src/lib.rs".to_string(),
            line: Some(10),
            title: "Slow loop".to_string(),
            description: "Nested loop may be O(n²)".to_string(),
            suggestion: Some("Use a hash map".to_string()),
        };
        let json = serde_json::to_string(&f).unwrap();
        let back: ReviewFinding = serde_json::from_str(&json).unwrap();
        assert_eq!(back.severity, ReviewSeverity::Warning);
        assert_eq!(back.category, ReviewCategory::Performance);
        assert_eq!(back.line, Some(10));
        assert_eq!(back.suggestion, Some("Use a hash map".to_string()));
    }

    #[test]
    fn test_code_review_serialization() {
        let mut review = CodeReview::new("Serialize Test");
        review.files_reviewed.push("a.rs".to_string());
        review.findings.push(ReviewFinding {
            severity: ReviewSeverity::Error,
            category: ReviewCategory::Bug,
            file: "a.rs".to_string(),
            line: Some(1),
            title: "Null".to_string(),
            description: "desc".to_string(),
            suggestion: None,
        });
        let json = serde_json::to_string(&review).unwrap();
        let back: CodeReview = serde_json::from_str(&json).unwrap();
        assert_eq!(back.title, "Serialize Test");
        assert_eq!(back.findings.len(), 1);
        assert_eq!(back.files_reviewed.len(), 1);
    }

    // ── Pattern detectors ──

    fn make_code_reviewer(content: &str) -> CodeReviewer {
        let mut cr = CodeReviewer::new();
        cr.add_file("test.rs", content);
        cr
    }

    #[test]
    fn test_check_todos() {
        let lines = vec![
            "fn main() {",
            "    // TODO: implement this",
            "    let x = 1;",
            "    // FIXME: this is wrong",
            "    // HACK: workaround for bug",
            "}",
        ];
        let findings = CodeReviewer::check_todos("test.rs", &lines);
        assert_eq!(findings.len(), 3);
        assert!(findings.iter().any(|f| f.title.contains("TODO")));
        assert!(findings.iter().any(|f| f.title.contains("FIXME")));
        assert!(findings.iter().any(|f| f.title.contains("HACK")));
        for f in &findings {
            assert_eq!(f.severity, ReviewSeverity::Info);
            assert_eq!(f.category, ReviewCategory::Maintainability);
            assert_eq!(f.file, "test.rs");
        }
    }

    #[test]
    fn test_check_todos_in_string_literal() {
        // TODO in a string should not be flagged (only comments)
        let lines = vec![r#"    println!("TODO: do this later");"#];
        let findings = CodeReviewer::check_todos("test.rs", &lines);
        assert!(
            findings.is_empty(),
            "TODO in string literal should not be flagged"
        );
    }

    #[test]
    fn test_check_todos_no_false_positive() {
        let lines = vec![
            "    let todo_list = vec![];",
            "    let todo = true;",
        ];
        let findings = CodeReviewer::check_todos("test.rs", &lines);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_check_debug_prints() {
        let lines = vec![
            "fn main() {",
            "    println!(\"hello\");",
            "    dbg!(x);",
            "    eprintln!(\"error\");",
            "}",
        ];
        let findings = CodeReviewer::check_debug_prints("test.rs", &lines);
        // println!, dbg!, eprintln! = 3 findings
        assert_eq!(findings.len(), 3);
        assert!(findings.iter().any(|f| f.title == "Debug print statement"));
    }

    #[test]
    fn test_check_debug_prints_no_false() {
        let lines = vec![
            "    println!(\"TODO: finish implementation\");",
            "    // dbg! macro usage",
        ];
        let findings = CodeReviewer::check_debug_prints("test.rs", &lines);
        // println!("TODO:... should not be flagged
        assert!(findings.is_empty());
    }

    #[test]
    fn test_check_secrets() {
        let lines = vec![
            "    let aws_key = \"AKIAIOSFODNN7EXAMPLE\";",
            "    let github = \"ghp_abc123def456\";",
            "    let normal = 42;",
        ];
        let findings = CodeReviewer::check_secrets("test.rs", &lines);
        assert_eq!(findings.len(), 2);
        for f in &findings {
            assert_eq!(f.severity, ReviewSeverity::Error);
            assert_eq!(f.category, ReviewCategory::Security);
        }
    }

    #[test]
    fn test_check_secrets_ignores_comments() {
        let lines = vec![
            "    // AKIAIOSFODNN7EXAMPLE is just an example",
            "    # ghp_abc123 in config",
        ];
        let findings = CodeReviewer::check_secrets("test.rs", &lines);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_check_secrets_placeholder() {
        let lines = vec![
            "    const KEY: &str = \"your_key_here\";",
            "    let token = \"placeholder\";",
        ];
        let findings = CodeReviewer::check_secrets("test.rs", &lines);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_check_unwrap() {
        let lines = vec![
            "fn main() {",
            "    let x = some_result.unwrap();",
            "    let y = other.expect(\"msg\");",
            "}",
        ];
        let findings = CodeReviewer::check_unwrap("test.rs", &lines);
        assert_eq!(findings.len(), 2);
        assert!(findings[0].title.contains("unwrap"));
        assert!(findings[1].title.contains("Expect"));
    }

    #[test]
    fn test_check_unwrap_in_comment_ignored() {
        let lines = vec![
            "    // .unwrap() is fine here",
            "    // .expect(\"msg\") also okay",
        ];
        let findings = CodeReviewer::check_unwrap("test.rs", &lines);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_check_large_diff() {
        let findings = CodeReviewer::check_large_diff("big.rs", 600);
        assert!(!findings.is_empty());
        assert_eq!(findings[0].severity, ReviewSeverity::Warning);

        let small = CodeReviewer::check_large_diff("small.rs", 10);
        assert!(small.is_empty());
    }

    #[test]
    fn test_check_public_docs() {
        let lines = vec![
            "/// Documented function",
            "pub fn documented() {}",
            "pub fn undocumented() {}",
            "/// Another one",
            "pub struct Foo;",
            "pub enum Bar;",
        ];
        let findings = CodeReviewer::check_public_docs("test.rs", &lines);
        assert_eq!(findings.len(), 2); // undocumented() and Bar
        for f in &findings {
            assert_eq!(f.category, ReviewCategory::Documentation);
        }
    }

    #[test]
    fn test_check_public_docs_all_documented() {
        let lines = vec![
            "/// Does x",
            "pub fn x() {}",
            "/// Does y",
            "pub fn y() {}",
        ];
        let findings = CodeReviewer::check_public_docs("test.rs", &lines);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_check_long_functions() {
        let mut content = String::from("fn short() {}\n");
        content.push_str("fn very_long() {\n");
        for _ in 0..105 {
            content.push_str("    let x = 1;\n");
        }
        content.push_str("}\n");

        let lines: Vec<&str> = content.lines().collect();
        let findings = CodeReviewer::check_long_functions("test.rs", &lines);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("long function"));
    }

    #[test]
    fn test_check_long_functions_no_false() {
        let content = "fn short() {\n    let x = 1;\n}\nfn also_short(a: i32) -> i32 {\n    a + 1\n}\n";
        let lines: Vec<&str> = content.lines().collect();
        let findings = CodeReviewer::check_long_functions("test.rs", &lines);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_check_nesting_depth() {
        let content = "fn main() {\n    if true {\n        for _ in 0..10 {\n            loop {\n                match x {\n                    1 => {}\n                    _ => {}\n                }\n            }\n        }\n    }\n}\n";
        let lines: Vec<&str> = content.lines().collect();
        let findings = CodeReviewer::check_nesting_depth("test.rs", &lines);
        assert!(!findings.is_empty());
        assert_eq!(findings[0].category, ReviewCategory::Style);
    }

    #[test]
    fn test_check_nesting_depth_shallow() {
        let content = "fn main() {\n    if true {\n        println!(\"ok\");\n    }\n}\n";
        let lines: Vec<&str> = content.lines().collect();
        let findings = CodeReviewer::check_nesting_depth("test.rs", &lines);
        assert!(findings.is_empty());
    }

    // ── Diff parsing ──

    #[test]
    fn test_parse_diff_files() {
        let diff = "\
diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@
 fn main() {
-    let old = 1;
+    let new = 1;
+    let added = 2;
 }
";
        let files = CodeReviewer::parse_diff_files(diff);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0, "src/main.rs");
        assert_eq!(files[0].1.len(), 5); // context + removed + empty + empty + context
        assert_eq!(files[0].2.len(), 5); // context + empty + added + added + context
    }

    #[test]
    fn test_parse_diff_files_empty() {
        let files = CodeReviewer::parse_diff_files("");
        assert!(files.is_empty());
    }

    #[test]
    fn test_parse_diff_multiple_files() {
        let diff = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1 +1,2 @@
-old
+new
+extra
diff --git a/b.rs b/b.rs
--- a/b.rs
+++ b/b.rs
@@ -1 +1 @@
-old
+new
";
        let files = CodeReviewer::parse_diff_files(diff);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].0, "a.rs");
        assert_eq!(files[1].0, "b.rs");
    }

    // ── Builder pattern ──

    #[test]
    fn test_builder_add_file() {
        let mut cr = CodeReviewer::new();
        cr.add_file("a.rs", "content");
        assert_eq!(cr.files.len(), 1);
        assert_eq!(cr.diffs.len(), 0);
    }

    #[test]
    fn test_builder_add_diff() {
        let mut cr = CodeReviewer::new();
        cr.add_diff("some diff");
        assert_eq!(cr.diffs.len(), 1);
        assert_eq!(cr.files.len(), 0);
    }

    #[test]
    fn test_builder_chain() {
        let mut cr = CodeReviewer::new();
        cr.add_file("a.rs", "content").add_diff("diff content");
        assert_eq!(cr.files.len(), 1);
        assert_eq!(cr.diffs.len(), 1);
    }

    #[test]
    fn test_review_patterns_empty() {
        let cr = CodeReviewer::new();
        let review = cr.review_patterns();
        assert!(review.findings.is_empty());
        assert!(review.files_reviewed.is_empty());
        assert_eq!(review.overall_score, 100);
    }

    #[test]
    fn test_review_patterns_with_clean_code() {
        let content = "\
/// Does something
pub fn do_something() {
    let x = 42;
    if x > 0 {
        println!(\"positive\");
    }
}
";
        let cr = make_code_reviewer(content);
        let review = cr.review_patterns();
        // println! should be flagged, but "TODO" in println! shouldn't be flagged since
        // the println! content check has the exception for println!("TODO...
        #[allow(unused_mut)]
        let mut has_println = false;
        for f in &review.findings {
            if f.title == "Debug print statement" {
                has_println = true;
            }
        }
        // println!("positive") is not a TODO, so it should be flagged
        assert!(has_println, "println! should be flagged");
    }

    #[test]
    fn test_review_with_multiple_files() {
        let mut cr = CodeReviewer::new();
        cr.add_file("a.rs", "fn x() {\n    // TODO: fix\n}");
        cr.add_file("b.rs", "fn y() {\n    dbg!(1);\n}");
        let review = cr.review_patterns();
        assert_eq!(review.files_reviewed.len(), 2);
        assert!(review.files_reviewed.contains(&"a.rs".to_string()));
        assert!(review.files_reviewed.contains(&"b.rs".to_string()));
    }

    // ── LLM deep review ──

    #[tokio::test]
    async fn test_review_deep_empty() {
        let provider = Arc::new(MockProvider::with_text_response(""));
        let cr = CodeReviewer::new();
        let review = cr.review_deep(provider, "mock-model").await;
        assert_eq!(review.overall_score, 100);
        assert!(review.summary.contains("No content"));
    }

    #[tokio::test]
    async fn test_review_deep_with_content() {
        let response = r#"{
            "summary": "Looks good overall",
            "findings": [
                {
                    "severity": "Warning",
                    "category": "BestPractice",
                    "file": "test.rs",
                    "line": 5,
                    "title": "Use ? instead of unwrap",
                    "description": "unwrap may panic",
                    "suggestion": "Use ? operator"
                }
            ],
            "positive_notes": ["Clean code structure"],
            "overall_score": 90
        }"#;
        let provider = Arc::new(MockProvider::with_text_response(response));
        let mut cr = CodeReviewer::new();
        cr.add_file("test.rs", "fn main() {\n    let x = result.unwrap();\n}");
        let review = cr.review_deep(provider, "mock-model").await;
        assert_eq!(review.findings.len(), 1);
        assert_eq!(review.positive_notes.len(), 1);
        assert_eq!(review.overall_score, 90);
        assert_eq!(review.findings[0].title, "Use ? instead of unwrap");
    }

    #[tokio::test]
    async fn test_review_deep_handles_provider_error() {
        let provider = Arc::new(MockProvider::new(vec![])); // empty responses = error
        let mut cr = CodeReviewer::new();
        cr.add_file("test.rs", "fn main() {}");
        let review = cr.review_deep(provider, "mock-model").await;
        assert_eq!(review.overall_score, 0);
        assert!(review.summary.contains("failed"));
    }

    #[tokio::test]
    async fn test_review_deep_parses_json_fence() {
        let response = "\
Some preamble text.

```json
{
    \"summary\": \"Good work\",
    \"findings\": [],
    \"positive_notes\": [\"Nice error handling\"],
    \"overall_score\": 95
}
```

Some trailing text.";
        let provider = Arc::new(MockProvider::with_text_response(response));
        let mut cr = CodeReviewer::new();
        cr.add_file("test.rs", "fn main() {}");
        let review = cr.review_deep(provider, "mock-model").await;
        assert_eq!(review.overall_score, 95);
        assert_eq!(review.positive_notes.len(), 1);
    }

    // ── Full review ──

    #[tokio::test]
    async fn test_review_full() {
        let response = r#"{
            "summary": "Minor issues found",
            "findings": [],
            "positive_notes": [],
            "overall_score": 95
        }"#;
        let provider = Arc::new(MockProvider::with_text_response(response));
        let content = "fn x() {\n    // TODO: clean this\n    println!(\"debug\");\n}\n";
        let mut cr = CodeReviewer::new();
        cr.add_file("test.rs", content);
        let review = cr.review_full(provider, "mock-model").await;
        // pattern should find TODO + println!
        assert!(
            review.findings.len() >= 2,
            "should have at least TODO and println findings"
        );
        assert!(review.files_reviewed.contains(&"test.rs".to_string()));
    }

    // ── Diff integration ──

    #[test]
    fn test_review_with_diff() {
        let diff = "\
diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -1 +1,2 @@
-old content
+new content
+// TODO: refactor this
";
        let mut cr = CodeReviewer::new();
        cr.add_diff(diff);
        let review = cr.review_patterns();
        // The diff has a TODO, but check_todos only runs on files, not diffs
        // The large diff check runs on diffs
        assert!(review.files_reviewed.contains(&"src/main.rs".to_string()));
    }
}
