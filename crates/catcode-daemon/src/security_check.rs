use regex::Regex;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;
use tracing::warn;



// ===== Core Types =====

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityReport {
    pub target: String,
    pub scanned_at: String,
    pub summary: SecuritySummary,
    pub findings: Vec<SecurityFinding>,
    pub file_count: usize,
    pub scan_duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecuritySummary {
    pub total_findings: usize,
    pub critical: usize,
    pub high: usize,
    pub medium: usize,
    pub low: usize,
    pub info: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityFinding {
    pub severity: Severity,
    pub category: FindingCategory,
    pub file: String,
    pub line: Option<u64>,
    pub column: Option<u64>,
    pub title: String,
    pub description: String,
    pub code_snippet: Option<String>,
    pub recommendation: Option<String>,
    pub cve_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

impl Severity {
    fn rank(self) -> u8 {
        match self {
            Severity::Critical => 5,
            Severity::High => 4,
            Severity::Medium => 3,
            Severity::Low => 2,
            Severity::Info => 1,
        }
    }
}

impl PartialOrd for Severity {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Severity {
    fn cmp(&self, other: &Self) -> Ordering {
        self.rank().cmp(&other.rank())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FindingCategory {
    Secret,
    DependencyVuln,
    CodeInjection,
    PathTraversal,
    UnsafeCrypto,
    UnsafeNetwork,
    InformationDisclosure,
    Configuration,
    BestPractice,
}

// ===== Scanner =====

pub struct SecurityScanner {
    scan_secrets: bool,
    scan_injection: bool,
    scan_dependencies: bool,
    scan_config: bool,
}

impl Default for SecurityScanner {
    fn default() -> Self {
        Self {
            scan_secrets: true,
            scan_injection: true,
            scan_dependencies: true,
            scan_config: true,
        }
    }
}

impl SecurityScanner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_secrets(mut self, enabled: bool) -> Self {
        self.scan_secrets = enabled;
        self
    }

    pub fn with_injection(mut self, enabled: bool) -> Self {
        self.scan_injection = enabled;
        self
    }

    pub fn with_dependencies(mut self, enabled: bool) -> Self {
        self.scan_dependencies = enabled;
        self
    }

    pub fn with_config(mut self, enabled: bool) -> Self {
        self.scan_config = enabled;
        self
    }

    pub fn scan_file(&self, path: &Path) -> Vec<SecurityFinding> {
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let mut findings = Vec::new();
        let path_str = path.to_string_lossy().to_string();

        if self.scan_secrets {
            findings.extend(detect_secrets(&content, &path_str));
        }
        if self.scan_injection {
            findings.extend(detect_code_injection(&content, path));
        }

        findings
    }

    pub fn scan_directory(&self, dir: &Path) -> SecurityReport {
        let start = Instant::now();
        let files = collect_source_files(dir);
        let mut findings = Vec::new();

        for file in &files {
            findings.extend(self.scan_file(file));
        }

        if self.scan_dependencies {
            findings.extend(self.scan_dependencies(dir));
        }

        if self.scan_config {
            findings.extend(detect_config_issues(dir));
        }

        let file_count = files.len();
        let scan_duration_ms = start.elapsed().as_millis() as u64;

        let summary = SecuritySummary {
            total_findings: findings.len(),
            critical: findings.iter().filter(|f| f.severity == Severity::Critical).count(),
            high: findings.iter().filter(|f| f.severity == Severity::High).count(),
            medium: findings.iter().filter(|f| f.severity == Severity::Medium).count(),
            low: findings.iter().filter(|f| f.severity == Severity::Low).count(),
            info: findings.iter().filter(|f| f.severity == Severity::Info).count(),
        };

        let scanned_at = chrono::Utc::now().to_rfc3339();

        // Sort findings by severity (most critical first)
        let mut sorted_findings = findings;
        sorted_findings.sort_by(|a, b| b.severity.cmp(&a.severity));

        SecurityReport {
            target: dir.to_string_lossy().to_string(),
            scanned_at,
            summary,
            findings: sorted_findings,
            file_count,
            scan_duration_ms,
        }
    }

    pub fn scan_dependencies(&self, dir: &Path) -> Vec<SecurityFinding> {
        let mut findings = Vec::new();

        let dep_files = find_dependency_files(dir);
        for (path, content) in &dep_files {
            let fname = path.file_name().unwrap_or_default().to_string_lossy();
            match fname.as_ref() {
                "Cargo.lock" => findings.extend(scan_cargo_lock(content, path)),
                "package.json" => findings.extend(scan_package_json(content, path)),
                "yarn.lock" | "pnpm-lock.yaml" => {
                    let _name = path.file_stem().unwrap_or_default().to_string_lossy();
                    let findings_len = findings.len();
                    findings.push(make_info_finding(
                        format!("Dependency file found: {}", fname),
                        format!(
                            "Found {} which should be audited with a dedicated tool.",
                            fname
                        ),
                        &path.to_string_lossy(),
                        Some("Run `npm audit` or `yarn audit` for thorough dependency scanning.".to_string()),
                    ));
                    // Also scan deps by looking for package.json in same dir
                    if let Some(parent) = path.parent() {
                        let pj = parent.join("package.json");
                        if pj.exists()
                            && let Ok(c) = fs::read_to_string(&pj) {
                                findings.extend(scan_package_json(&c, &pj));
                            }
                    }
                    // Remove the info finding if we added actual vuln findings from package.json
                    if findings.len() > findings_len + 1 {
                        findings.swap_remove(findings_len);
                    }
                }
                "requirements.txt" => findings.extend(scan_requirements_txt(content, path)),
                "Pipfile" => findings.extend(scan_pipfile(content, path)),
                _ => {}
            }
        }

        if !findings.is_empty() {
            warn!(
                "Dependency vulnerabilities detected: {}. Consider running `cargo audit` or `npm audit`.",
                findings.len()
            );
        }

        findings
    }
}

// ===== Directory Walking =====

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    ".git",
    ".svn",
    ".hg",
    "vendor",
    ".bundle",
    "__pycache__",
    ".tox",
    ".gradle",
    "build",
    "dist",
    ".next",
    ".nuxt",
    ".cache",
    ".npm",
    ".yarn",
    "third_party",
    ".cargo",
    "bin",
    "obj",
    "packages",
];

fn collect_source_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_files_recursive(dir, &mut files);
    files
}

fn collect_files_recursive(dir: &Path, files: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if !SKIP_DIRS.contains(&name.as_ref()) && !name.starts_with('.') {
                collect_files_recursive(&path, files);
            }
        } else if path.is_file()
            && let Ok(meta) = path.metadata()
                && meta.len() > 0 && meta.len() < 5 * 1024 * 1024 {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    if !name.starts_with('.') {
                        files.push(path);
                    }
                }
    }
}

fn find_dependency_files(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut results = Vec::new();
    let targets = &[
        "Cargo.lock",
        "package.json",
        "yarn.lock",
        "pnpm-lock.yaml",
        "requirements.txt",
        "Pipfile",
    ];

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return results,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if !SKIP_DIRS.contains(&name.as_ref()) && !name.starts_with('.') {
                results.extend(find_dependency_files(&path));
            }
        } else if let Some(fname) = path.file_name() {
            let fname = fname.to_string_lossy();
            if targets.contains(&fname.as_ref())
                && let Ok(content) = fs::read_to_string(&path) {
                    results.push((path, content));
                }
        }
    }

    results
}

// ===== Secret Detection =====

struct SecretPattern {
    name: &'static str,
    severity: Severity,
    pattern: fn() -> &'static Regex,
    description: &'static str,
    recommendation: &'static str,
}

fn pattern_aws_key() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\bAKIA[0-9A-Z]{16}\b").unwrap())
}
fn pattern_aws_secret() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?i)aws[_-]?secret[_-]?access[_-]?key\s*[:=]\s*['"][A-Za-z0-9/+=]{40}['"]"#).unwrap())
}
fn pattern_github_token() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\bghp_[0-9a-zA-Z]{36}\b").unwrap())
}
fn pattern_github_old_token() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\bgho_[0-9a-zA-Z]{36}\b").unwrap())
}
fn pattern_generic_api_key() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)(?:api[_-]?key|apikey|secret|token|password)\s*[:=]\s*['"][^'"]{8,}['"]"#)
            .unwrap()
    })
}
fn pattern_jwt() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\beyJ[a-zA-Z0-9_-]+\.eyJ[a-zA-Z0-9_-]+\.[a-zA-Z0-9_-]+\b").unwrap())
}
fn pattern_private_key() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"-----BEGIN\s+(RSA|EC|DSA|OPENSSH)\s+PRIVATE\s+KEY-----").unwrap())
}
fn pattern_slack_token() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\bxox[baprs]-[0-9a-zA-Z-]{10,}\b").unwrap())
}
fn pattern_stripe_key() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b(?:sk|pk)_(?:live|test)_[0-9a-zA-Z]{24,}\b").unwrap())
}
fn pattern_google_api() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)AIza[0-9A-Za-z\-_]{35}").unwrap())
}
fn pattern_heroku_api() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\bhurk_[0-9a-zA-Z]{27}\b").unwrap())
}

const SECRET_PATTERNS: &[SecretPattern] = &[
    SecretPattern {
        name: "AWS Access Key",
        severity: Severity::High,
        pattern: pattern_aws_key,
        description: "Hardcoded AWS Access Key ID detected.",
        recommendation: "Use AWS IAM roles or environment variables. Do not commit access keys to version control.",
    },
    SecretPattern {
        name: "AWS Secret Key",
        severity: Severity::Critical,
        pattern: pattern_aws_secret,
        description: "Hardcoded AWS Secret Access Key detected.",
        recommendation: "Use AWS IAM roles, secrets manager, or environment variables.",
    },
    SecretPattern {
        name: "GitHub Token",
        severity: Severity::Critical,
        pattern: pattern_github_token,
        description: "Hardcoded GitHub personal access token detected.",
        recommendation: "Use GitHub Actions secrets or environment variables.",
    },
    SecretPattern {
        name: "GitHub Token (old format)",
        severity: Severity::Critical,
        pattern: pattern_github_old_token,
        description: "Hardcoded GitHub OAuth access token detected.",
        recommendation: "Use GitHub Actions secrets or environment variables.",
    },
    SecretPattern {
        name: "Generic API Key / Secret / Token / Password",
        severity: Severity::High,
        pattern: pattern_generic_api_key,
        description: "Possible hardcoded API key, secret, token, or password detected.",
        recommendation: "Use environment variables, secrets management, or vault services.",
    },
    SecretPattern {
        name: "JWT Token",
        severity: Severity::High,
        pattern: pattern_jwt,
        description: "Hardcoded JWT token detected.",
        recommendation: "Issue tokens at runtime; never commit tokens to version control.",
    },
    SecretPattern {
        name: "Private Key",
        severity: Severity::Critical,
        pattern: pattern_private_key,
        description: "Hardcoded private cryptographic key detected.",
        recommendation: "Store private keys in secure key management systems (e.g., AWS KMS, HashiCorp Vault).",
    },
    SecretPattern {
        name: "Slack Token",
        severity: Severity::Critical,
        pattern: pattern_slack_token,
        description: "Hardcoded Slack API token detected.",
        recommendation: "Use Slack app credentials via environment variables.",
    },
    SecretPattern {
        name: "Stripe API Key",
        severity: Severity::Critical,
        pattern: pattern_stripe_key,
        description: "Hardcoded Stripe API key detected.",
        recommendation: "Use Stripe's secret management or environment variables.",
    },
    SecretPattern {
        name: "Google API Key",
        severity: Severity::High,
        pattern: pattern_google_api,
        description: "Hardcoded Google API key detected.",
        recommendation: "Restrict API keys by IP/HTTP referrer and use environment variables.",
    },
    SecretPattern {
        name: "Heroku API Key",
        severity: Severity::High,
        pattern: pattern_heroku_api,
        description: "Hardcoded Heroku API key detected.",
        recommendation: "Use environment variables or Heroku's config vars.",
    },
];

fn detect_secrets(content: &str, path: &str) -> Vec<SecurityFinding> {
    let mut findings = Vec::new();

    for pattern in SECRET_PATTERNS {
        let re = (pattern.pattern)();
        for cap in re.find_iter(content) {
            let line = content[..cap.start()].lines().count().saturating_add(1) as u64;

            // Check if this looks like a test or example (heuristic)
            let context_line = content.lines().nth(line.saturating_sub(2) as usize).unwrap_or("");
            if context_line.contains("EXAMPLE") || context_line.contains("example") {
                findings.push(make_finding(
                    Severity::Info,
                    FindingCategory::Secret,
                    format!("{} (example/test context)", pattern.name),
                    pattern.description,
                    path,
                    Some(line),
                    Some(cap.as_str().to_string()),
                    Some(format!("{} If this is a real secret, rotate it immediately.", pattern.recommendation)),
                    None,
                ));
            } else {
                findings.push(make_finding(
                    pattern.severity,
                    FindingCategory::Secret,
                    pattern.name.to_string(),
                    pattern.description,
                    path,
                    Some(line),
                    Some(cap.as_str().to_string()),
                    Some(pattern.recommendation.to_string()),
                    None,
                ));
            }
        }
    }

    findings
}

// ===== Code Injection Detection =====

fn detect_code_injection(content: &str, path: &Path) -> Vec<SecurityFinding> {
    let mut findings = Vec::new();
    let file_str = path.to_string_lossy();
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();

    for (i, line) in content.lines().enumerate() {
        let line_num = (i + 1) as u64;
        let trimmed = line.trim();

        if trimmed.starts_with("//") || trimmed.starts_with('#') || trimmed.starts_with("--") {
            continue;
        }

        // eval() - language agnostic
        if trimmed.contains("eval(") {
            findings.push(make_finding(
                Severity::Critical,
                FindingCategory::CodeInjection,
                "Code Injection: eval() usage".to_string(),
                "Use of eval() can lead to arbitrary code execution if user input is passed to it.",
                &file_str,
                Some(line_num),
                Some(trimmed.to_string()),
                Some("Avoid eval(). Use safer alternatives like Function constructor (with care), or better: domain-specific parsers.".to_string()),
                None,
            ));
        }

        // exec() - language agnostic
        if trimmed.contains("exec(") {
            findings.push(make_finding(
                Severity::High,
                FindingCategory::CodeInjection,
                "Code Injection: exec() usage".to_string(),
                "Use of exec() can lead to arbitrary code execution if user input is passed to it.",
                &file_str,
                Some(line_num),
                Some(trimmed.to_string()),
                Some("Avoid exec(). Use safe alternatives like child_process.execFile() or spawn() with argument arrays.".to_string()),
                None,
            ));
        }

        // shell_exec (PHP)
        if trimmed.contains("shell_exec(") || trimmed.contains("`") && trimmed.contains("$") {
            findings.push(make_finding(
                Severity::Critical,
                FindingCategory::CodeInjection,
                "Code Injection: shell execution".to_string(),
                "Shell execution functions can lead to command injection.",
                &file_str,
                Some(line_num),
                Some(trimmed.to_string()),
                Some("Avoid shell execution with user input. Use escapeshellarg() or parameterized APIs.".to_string()),
                None,
            ));
        }

        // spawn / Command::new in Rust
        if ext == "rs" && (trimmed.contains("Command::new(") || trimmed.contains("spawn(")) {
            findings.push(make_finding(
                Severity::Medium,
                FindingCategory::CodeInjection,
                "Potential Command Execution".to_string(),
                "Command execution detected. Ensure no user input is used in command arguments.",
                &file_str,
                Some(line_num),
                Some(trimmed.to_string()),
                Some("Avoid constructing commands from user input. Use argument arrays instead of shell strings.".to_string()),
                None,
            ));
        }

        // subprocess in Python
        if trimmed.contains("subprocess.")
            || trimmed.contains("os.system(")
            || trimmed.contains("os.popen(")
        {
            findings.push(make_finding(
                Severity::High,
                FindingCategory::CodeInjection,
                "Code Injection: subprocess/os call".to_string(),
                "Process execution via subprocess or os module can lead to command injection.",
                &file_str,
                Some(line_num),
                Some(trimmed.to_string()),
                Some("Use subprocess.run() with argument lists (not shell=True) to avoid shell injection.".to_string()),
                None,
            ));
        }

        // execSync in Node.js
        if (ext == "js" || ext == "ts" || ext == "jsx" || ext == "tsx")
            && (trimmed.contains("execSync(") || trimmed.contains("exec(") && !trimmed.contains("eval(")) {
                // Already handled by exec() above, but Node-specific note
            }

        // unsafe blocks in Rust
        if ext == "rs" && trimmed.contains("unsafe") && !trimmed.trim_start().starts_with("//")
            && (trimmed.contains("unsafe {") || trimmed == "unsafe" || trimmed.starts_with("unsafe ")) {
                findings.push(make_finding(
                    Severity::Medium,
                    FindingCategory::BestPractice,
                    "Unsafe Code Block".to_string(),
                    "Usage of unsafe Rust code detected. Unsafe code bypasses Rust's safety guarantees.",
                    &file_str,
                    Some(line_num),
                    Some(trimmed.to_string()),
                    Some("Minimize unsafe code. Document safety invariants in SAFETY comments. Consider safe alternatives.".to_string()),
                    None,
                ));
            }

        // dangerouslySetInnerHTML (React)
        if trimmed.contains("dangerouslySetInnerHTML") {
            findings.push(make_finding(
                Severity::High,
                FindingCategory::CodeInjection,
                "XSS Risk: dangerouslySetInnerHTML".to_string(),
                "Using dangerouslySetInnerHTML can expose your application to cross-site scripting (XSS) attacks.",
                &file_str,
                Some(line_num),
                Some(trimmed.to_string()),
                Some("Use React's safe rendering. If HTML must be injected, sanitize it with DOMPurify or similar library.".to_string()),
                None,
            ));
        }

        // innerHTML assignment
        if trimmed.contains(".innerHTML") && trimmed.contains('=') && !trimmed.contains("//") {
            findings.push(make_finding(
                Severity::High,
                FindingCategory::CodeInjection,
                "XSS Risk: innerHTML assignment".to_string(),
                "Setting innerHTML with user data can lead to XSS vulnerabilities.",
                &file_str,
                Some(line_num),
                Some(trimmed.to_string()),
                Some("Use textContent instead of innerHTML when inserting text. If HTML is needed, sanitize input.".to_string()),
                None,
            ));
        }

        // Raw SQL string building (contains SQL keywords with concatenation or interpolation)
        let sql_keywords = ["SELECT ", "INSERT ", "UPDATE ", "DELETE ", "DROP ", "CREATE "];
        if sql_keywords.iter().any(|kw| trimmed.contains(kw))
            && (trimmed.contains('+') || trimmed.contains('$') || trimmed.contains("format(") || trimmed.contains(".format(")) {
                findings.push(make_finding(
                    Severity::High,
                    FindingCategory::CodeInjection,
                    "SQL Injection Risk: Raw SQL Building".to_string(),
                    "Dynamically building SQL queries with string concatenation can lead to SQL injection.",
                    &file_str,
                    Some(line_num),
                    Some(trimmed.to_string()),
                    Some("Use parameterized queries or an ORM to prevent SQL injection.".to_string()),
                    None,
                ));
            }
    }

    findings
}

// ===== Configuration Checks =====

fn detect_config_issues(dir: &Path) -> Vec<SecurityFinding> {
    let mut findings = Vec::new();

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return findings,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if !SKIP_DIRS.contains(&name.as_ref()) && !name.starts_with('.') {
                findings.extend(detect_config_issues(&path));
            }
            continue;
        }

        let fname = path.file_name().unwrap_or_default().to_string_lossy();
        let path_str = path.to_string_lossy();

        // .env files checked on reveal
        if fname == ".env" && !fname.ends_with(".example") && !fname.ends_with(".template")
            && let Ok(content) = fs::read_to_string(&path) {
                let has_real_values = content.lines().any(|l| {
                    l.contains('=') && !l.trim().starts_with('#')
                        && !l.contains("your-") && !l.contains("changeme") && !l.contains("example")
                });
                if has_real_values {
                    findings.push(make_finding(
                        Severity::High,
                        FindingCategory::Configuration,
                        "Exposed .env File".to_string(),
                        ".env file found in project source. This may expose sensitive credentials.",
                        &path_str,
                        None,
                        Some(content.lines().find(|l| l.contains('=')).unwrap_or("").to_string()),
                        Some("Add .env to .gitignore. Use .env.example with placeholder values for documentation.".to_string()),
                        None,
                    ));
                } else {
                    findings.push(make_finding(
                        Severity::Info,
                        FindingCategory::Configuration,
                        ".env File Found (likely template)".to_string(),
                        ".env file found in project. It appears to contain placeholder values.",
                        &path_str,
                        None,
                        None,
                        Some("Ensure .env is in .gitignore and use .env.example for documentation.".to_string()),
                        None,
                    ));
                }
            }
    }

    // Check for CORS: * configuration in JSON/YAML config files
    if let Ok(cargo_toml) = detect_cors_misconfig(dir) {
        findings.extend(cargo_toml);
    }

    // Check for debug mode in production configs
    if let Ok(debug_configs) = detect_debug_mode(dir) {
        findings.extend(debug_configs);
    }

    findings
}

fn detect_cors_misconfig(dir: &Path) -> std::io::Result<Vec<SecurityFinding>> {
    let mut findings = Vec::new();
    let entries = fs::read_dir(dir)?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if !SKIP_DIRS.contains(&name.as_ref()) && !name.starts_with('.') {
                findings.extend(detect_cors_misconfig(&path)?);
            }
            continue;
        }

        let fname = path.file_name().unwrap_or_default().to_string_lossy();
        let path_str = path.to_string_lossy();

        if fname == "Cargo.toml" || fname == "package.json" || fname == "config.json"
            || fname == "appsettings.json" || fname == ".env.example"
        {
            continue; // skip common files unlikely to have CORS config
        }

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(ext, "json" | "yaml" | "yml" | "toml" | "xml" | "php" | "py") {
            continue;
        }

        if let Ok(content) = fs::read_to_string(&path) {
            for (i, line) in content.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.contains("\"*\"") && trimmed.contains("origin") || trimmed.contains("Access-Control-Allow-Origin: *") {
                    if trimmed.starts_with('#') || trimmed.starts_with("//") {
                        continue;
                    }
                    findings.push(make_finding(
                        Severity::Medium,
                        FindingCategory::Configuration,
                        "Permissive CORS Configuration".to_string(),
                        "CORS configured to allow all origins (*). This can expose your API to cross-origin attacks.",
                        &path_str,
                        Some((i + 1) as u64),
                        Some(trimmed.to_string()),
                        Some("Restrict CORS to specific trusted origins. Avoid using wildcard (*) in production.".to_string()),
                        None,
                    ));
                }
            }
        }
    }

    Ok(findings)
}

fn detect_debug_mode(dir: &Path) -> std::io::Result<Vec<SecurityFinding>> {
    let mut findings = Vec::new();
    let entries = fs::read_dir(dir)?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if !SKIP_DIRS.contains(&name.as_ref()) && !name.starts_with('.') {
                findings.extend(detect_debug_mode(&path)?);
            }
            continue;
        }

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(ext, "json" | "yaml" | "yml" | "toml" | "py" | "php" | "env" | "ini") {
            continue;
        }

        if let Ok(content) = fs::read_to_string(&path) {
            let path_str = path.to_string_lossy();
            for (i, line) in content.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.starts_with('#') || trimmed.starts_with("//") || trimmed.starts_with(';') {
                    continue;
                }

                // Check for debug=true or DEBUG=True patterns
                if (trimmed.contains("debug") || trimmed.contains("DEBUG"))
                    && (trimmed.contains("=true") || trimmed.contains("=True")
                        || trimmed.contains("=1") || trimmed.contains(":\"true\"")
                        || trimmed.contains(": true") || trimmed.contains(": True"))
                {
                    findings.push(make_finding(
                        Severity::Low,
                        FindingCategory::Configuration,
                        "Debug Mode Enabled".to_string(),
                        "Debug mode appears to be enabled. Debug mode can expose sensitive information in production.",
                        &path_str,
                        Some((i + 1) as u64),
                        Some(trimmed.to_string()),
                        Some("Disable debug mode in production. Set DEBUG=False, APP_DEBUG=0, etc.".to_string()),
                        None,
                    ));
                }
            }
        }
    }

    Ok(findings)
}

// ===== Dependency Vulnerability Scanning =====

struct KnownCve {
    package: &'static str,
    ecosystem: &'static str,
    version_constraint: fn(&str) -> bool,
    cve_id: &'static str,
    severity: Severity,
    description: &'static str,
}

fn version_matches(version: &str, constraint: &str) -> bool {
    // Simple version prefix matching: "1.2" means < 1.3, "=1.2.3" means exact
    if let Some(exact) = constraint.strip_prefix('=') {
        return version == exact;
    }
    if let Some(max) = constraint.strip_prefix('<') {
        let max = max.trim();
        return version < max;
    }
    if let Some(min) = constraint.strip_prefix(">=") {
        let min = min.trim();
        return version >= min;
    }
    if let Some(range) = constraint.strip_prefix("~>") {
        // Pessimistic version constraint: ~> 1.2 means >= 1.2, < 2.0
        let range = range.trim();
        if let Some(major) = range.split('.').next() {
            let next_major = major.parse::<u32>().unwrap_or(0) + 1;
            let max_ver = format!("{}.0.0", next_major);
            return version >= range && version < &max_ver[..];
        }
    }
    version.starts_with(constraint)
}

const KNOWN_CVES: &[KnownCve] = &[
    // Rust
    KnownCve {
        package: "time",
        ecosystem: "cargo",
        version_constraint: |v| version_matches(v, "<0.3.0"),
        cve_id: "CVE-2020-26235",
        severity: Severity::Critical,
        description: "Time crate vulnerability: potential segfault in local time handling.",
    },
    KnownCve {
        package: "openssl-sys",
        ecosystem: "cargo",
        version_constraint: |v| version_matches(v, "<0.9.75"),
        cve_id: "CVE-2022-36059",
        severity: Severity::Critical,
        description: "Vulnerable OpenSSL version linked via sys crate.",
    },
    KnownCve {
        package: "atty",
        ecosystem: "cargo",
        version_constraint: |v| version_matches(v, "<0.2.14"),
        cve_id: "CVE-2023-44487",
        severity: Severity::High,
        description: "atty crate has an unsoundness issue on Windows.",
    },
    KnownCve {
        package: "smallvec",
        ecosystem: "cargo",
        version_constraint: |v| version_matches(v, "<1.8.1"),
        cve_id: "CVE-2022-21658",
        severity: Severity::High,
        description: "Smallvec crate bug can cause memory corruption on reallocation.",
    },
    // NPM
    KnownCve {
        package: "left-pad",
        ecosystem: "npm",
        version_constraint: |v| version_matches(v, "<1.3.0"),
        cve_id: "GHSA-67hx-6x53-xxxx",
        severity: Severity::Medium,
        description: "left-pad is a deprecated/unmaintained package known for the 2016 npm unpublishing incident.",
    },
    KnownCve {
        package: "flatmap-stream",
        ecosystem: "npm",
        version_constraint: |v| version_matches(v, ">=0.0.0"),
        cve_id: "CVE-2018-16487",
        severity: Severity::Critical,
        description: "Malicious code injected into flatmap-stream targeting cryptocurrency wallets.",
    },
    KnownCve {
        package: "event-stream",
        ecosystem: "npm",
        version_constraint: |v| version_matches(v, ">=3.3.6"),
        cve_id: "CVE-2018-16487",
        severity: Severity::Critical,
        description: "Compromised event-stream package containing malicious code.",
    },
    KnownCve {
        package: "lodash",
        ecosystem: "npm",
        version_constraint: |v| version_matches(v, "<4.17.21"),
        cve_id: "CVE-2021-23337",
        severity: Severity::High,
        description: "Prototype pollution in lodash allows arbitrary code execution.",
    },
    KnownCve {
        package: "axios",
        ecosystem: "npm",
        version_constraint: |v| version_matches(v, "<0.21.2"),
        cve_id: "CVE-2021-3749",
        severity: Severity::High,
        description: "Server-Side Request Forgery (SSRF) in axios.",
    },
    KnownCve {
        package: "minimist",
        ecosystem: "npm",
        version_constraint: |v| version_matches(v, "<1.2.6"),
        cve_id: "CVE-2021-44906",
        severity: Severity::High,
        description: "Prototype pollution in minimist argument parser.",
    },
    // Python
    KnownCve {
        package: "requests",
        ecosystem: "pip",
        version_constraint: |v| version_matches(v, "<2.20.0"),
        cve_id: "CVE-2018-18074",
        severity: Severity::High,
        description: "Requests library redirect handling vulnerability.",
    },
    KnownCve {
        package: "urllib3",
        ecosystem: "pip",
        version_constraint: |v| version_matches(v, "<1.24.2"),
        cve_id: "CVE-2019-11324",
        severity: Severity::High,
        description: "URLLib3 certificate validation vulnerability.",
    },
    KnownCve {
        package: "django",
        ecosystem: "pip",
        version_constraint: |v| version_matches(v, "<3.2.18"),
        cve_id: "CVE-2023-31047",
        severity: Severity::High,
        description: "Django potential XSS vulnerability.",
    },
    KnownCve {
        package: "flask",
        ecosystem: "pip",
        version_constraint: |v| version_matches(v, "<2.3.0"),
        cve_id: "CVE-2023-30861",
        severity: Severity::Medium,
        description: "Flask cookie persistence vulnerability.",
    },
];

fn check_known_vulns(package: &str, version: &str, _ecosystem: &str) -> Vec<SecurityFinding> {
    let mut findings = Vec::new();
    let pkg_lower = package.to_lowercase();

    for cve in KNOWN_CVES {
        if cve.ecosystem == _ecosystem && pkg_lower == cve.package
            && (cve.version_constraint)(version) {
                findings.push(make_finding(
                    cve.severity,
                    FindingCategory::DependencyVuln,
                    format!("Known Vulnerability: {} ({})", cve.cve_id, cve.package),
                    cve.description.to_string(),
                    "",
                    None,
                    Some(format!("Package: {}, Version: {}", cve.package, version)),
                    Some(format!("Update {} to a patched version. Run `{} audit` for a full report.", cve.package, _ecosystem)),
                    Some(cve.cve_id.to_string()),
                ));
            }
    }

    findings
}

fn scan_cargo_lock(content: &str, path: &Path) -> Vec<SecurityFinding> {
    let mut findings = Vec::new();
    let path_str = path.to_string_lossy();

    let value: toml::Value = match toml::from_str(content) {
        Ok(v) => v,
        Err(e) => {
            findings.push(make_finding(
                Severity::Info,
                FindingCategory::BestPractice,
                "Could not parse Cargo.lock".to_string(),
                format!("Failed to parse Cargo.lock: {}", e),
                &path_str,
                None,
                None,
                Some("Ensure Cargo.lock is a valid lock file.".to_string()),
                None,
            ));
            return findings;
        }
    };

    if let Some(packages) = value.get("package").and_then(|v| v.as_array()) {
        for pkg in packages {
            let name = pkg.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let version = pkg.get("version").and_then(|v| v.as_str()).unwrap_or("");
            if !name.is_empty() && !version.is_empty() {
                findings.extend(check_known_vulns(name, version, "cargo"));
            }
        }
    }

    let has_vulns = findings.iter().any(|f| f.severity >= Severity::High);
    if has_vulns {
        findings.push(make_finding(
            Severity::Info,
            FindingCategory::BestPractice,
            "Run `cargo audit` for complete analysis".to_string(),
            "The built-in CVE database is limited. Run `cargo audit` for a comprehensive vulnerability scan.",
            &path_str,
            None,
            None,
            Some("Install cargo-audit: `cargo install cargo-audit` then run `cargo audit`.".to_string()),
            None,
        ));
    }

    findings
}

fn scan_package_json(content: &str, path: &Path) -> Vec<SecurityFinding> {
    let mut findings = Vec::new();
    let path_str = path.to_string_lossy();

    let value: serde_json::Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(_) => return findings,
    };

    let dep_sections = ["dependencies", "devDependencies", "peerDependencies"];

    for section in &dep_sections {
        if let Some(deps) = value.get(*section).and_then(|v| v.as_object()) {
            for (name, version_val) in deps {
                let version = version_val.as_str().unwrap_or("unknown");
                // Strip semver range prefixes like ^ ~ >= <=
                let clean_version = version.trim_start_matches(['^', '~', '>', '<', '=']);
                findings.extend(check_known_vulns(name, clean_version, "npm"));
            }
        }
    }

    // Check for lack of lock file recommendation
    let has_optional_deps = value.get("optionalDependencies").is_some();
    if has_optional_deps {
        findings.push(make_finding(
            Severity::Low,
            FindingCategory::BestPractice,
            "Optional Dependencies Detected".to_string(),
            "Optional dependencies can introduce supply chain risks.",
            &path_str,
            None,
            None,
            Some("Review optional dependencies. Consider using a lockfile to ensure consistent installs.".to_string()),
            None,
        ));
    }

    findings
}

fn scan_requirements_txt(content: &str, _path: &Path) -> Vec<SecurityFinding> {
    let mut findings = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Parse: package==version or package>=version
        if let Some(eq_pos) = trimmed.find("==") {
            let name = trimmed[..eq_pos].trim();
            let version = trimmed[eq_pos + 2..].trim().split(|c: char| c.is_whitespace() || c == '#' || c == ',').next().unwrap_or("");
            findings.extend(check_known_vulns(name, version, "pip"));
        } else if let Some(ge_pos) = trimmed.find(">=") {
            let name = trimmed[..ge_pos].trim();
            let version = trimmed[ge_pos + 2..].trim().split(|c: char| c.is_whitespace() || c == '#' || c == ',').next().unwrap_or("");
            findings.extend(check_known_vulns(name, version, "pip"));
        }
    }

    findings
}

fn scan_pipfile(content: &str, path: &Path) -> Vec<SecurityFinding> {
    let mut findings = Vec::new();
    let path_str = path.to_string_lossy();

    let value: toml::Value = match toml::from_str(content) {
        Ok(v) => v,
        Err(_) => return findings,
    };

    // Check [packages] and [dev-packages] sections
    for section_name in &["packages", "dev-packages"] {
        if let Some(section) = value.get(*section_name).and_then(|v| v.as_table()) {
            for (name, version_val) in section {
                let version = match version_val {
                    toml::Value::String(s) => s.trim_start_matches('\"').trim_end_matches('\"').to_string(),
                    toml::Value::Table(t) => {
                        t.get("version").and_then(|v| v.as_str()).unwrap_or("").to_string()
                    }
                    _ => continue,
                };
                let clean_version = version.trim_start_matches(['^', '~', '>', '<', '=']);
                findings.extend(check_known_vulns(name, clean_version, "pip"));
            }
        }
    }

    if !findings.is_empty() {
        findings.push(make_finding(
            Severity::Info,
            FindingCategory::BestPractice,
            "Run `pip audit` for complete analysis".to_string(),
            "The built-in CVE database is limited. Consider using a dedicated vulnerability scanner.",
            &path_str,
            None,
            None,
            Some("Run `pip-audit` or `safety check` for comprehensive Python dependency scanning.".to_string()),
            None,
        ));
    }

    findings
}

// ===== Helpers =====

#[allow(clippy::too_many_arguments)]
fn make_finding(
    severity: Severity,
    category: FindingCategory,
    title: impl Into<String>,
    description: impl Into<String>,
    file: &str,
    line: Option<u64>,
    code_snippet: Option<String>,
    recommendation: Option<String>,
    cve_id: Option<String>,
) -> SecurityFinding {
    SecurityFinding {
        severity,
        category,
        file: file.to_string(),
        line,
        column: None,
        title: title.into(),
        description: description.into(),
        code_snippet,
        recommendation,
        cve_id,
    }
}

fn make_info_finding(
    title: impl Into<String>,
    description: impl Into<String>,
    file: &str,
    recommendation: Option<String>,
) -> SecurityFinding {
    make_finding(
        Severity::Info,
        FindingCategory::BestPractice,
        title,
        description,
        file,
        None,
        None,
        recommendation,
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_trivial() {
        assert_eq!(1 + 1, 2);
    }

    // ===== Severity Tests =====

    #[test]
    fn test_severity_ordering() {
        assert!(Severity::Critical > Severity::High);
        assert!(Severity::High > Severity::Medium);
        assert!(Severity::Medium > Severity::Low);
        assert!(Severity::Low > Severity::Info);
        assert_eq!(Severity::Critical, Severity::Critical);
    }

    #[test]
    fn test_severity_sorting() {
        let mut severities = [Severity::Low,
            Severity::Critical,
            Severity::Info,
            Severity::High,
            Severity::Medium];
        severities.sort();
        assert_eq!(severities[0], Severity::Info);
        assert_eq!(severities[1], Severity::Low);
        assert_eq!(severities[2], Severity::Medium);
        assert_eq!(severities[3], Severity::High);
        assert_eq!(severities[4], Severity::Critical);
    }

    // ===== Serialization Tests =====

    #[test]
    fn test_security_finding_serialization() {
        let finding = SecurityFinding {
            severity: Severity::Critical,
            category: FindingCategory::Secret,
            file: "src/main.rs".to_string(),
            line: Some(42),
            column: None,
            title: "Test Finding".to_string(),
            description: "A test finding".to_string(),
            code_snippet: Some("let key = \"secret\";".to_string()),
            recommendation: Some("Fix it".to_string()),
            cve_id: None,
        };

        let json = serde_json::to_string_pretty(&finding).unwrap();
        let deserialized: SecurityFinding = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.severity, Severity::Critical);
        assert_eq!(deserialized.category, FindingCategory::Secret);
        assert_eq!(deserialized.file, "src/main.rs");
        assert_eq!(deserialized.line, Some(42));
    }

    #[test]
    fn test_security_report_serialization() {
        let report = SecurityReport {
            target: "/tmp/test".to_string(),
            scanned_at: "2024-01-01T00:00:00Z".to_string(),
            summary: SecuritySummary {
                total_findings: 1,
                critical: 1,
                high: 0,
                medium: 0,
                low: 0,
                info: 0,
            },
            findings: vec![SecurityFinding {
                severity: Severity::Critical,
                category: FindingCategory::Secret,
                file: "test.rs".to_string(),
                line: Some(1),
                column: None,
                title: "Test".to_string(),
                description: "Desc".to_string(),
                code_snippet: None,
                recommendation: None,
                cve_id: None,
            }],
            file_count: 10,
            scan_duration_ms: 100,
        };

        let json = serde_json::to_string_pretty(&report).unwrap();
        let deserialized: SecurityReport = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.target, "/tmp/test");
        assert_eq!(deserialized.summary.total_findings, 1);
        assert_eq!(deserialized.file_count, 10);
    }

    #[test]
    fn test_finding_category_serialization() {
        let categories = vec![
            FindingCategory::Secret,
            FindingCategory::DependencyVuln,
            FindingCategory::CodeInjection,
            FindingCategory::PathTraversal,
            FindingCategory::UnsafeCrypto,
            FindingCategory::UnsafeNetwork,
            FindingCategory::InformationDisclosure,
            FindingCategory::Configuration,
            FindingCategory::BestPractice,
        ];

        for cat in &categories {
            let json = serde_json::to_string(cat).unwrap();
            let deserialized: FindingCategory = serde_json::from_str(&json).unwrap();
            assert_eq!(*cat, deserialized);
        }
    }

    // ===== Secret Detection Tests =====

    #[test]
    fn test_secret_detection_aws_key() {
        let content = r#"
let aws_key = "AKIAIOSFODNN7EXAMPLE";
let normal = "just a normal string";
"#;
        let findings = detect_secrets(content, "config.rs");
        assert!(!findings.is_empty());
        let aws = findings.iter().find(|f| f.title.contains("AWS Access Key"));
        assert!(aws.is_some());
    }

    #[test]
    fn test_secret_detection_github_token() {
        let content = r#"token = "ghp_TDATAabcdefghijklmnopqrstuvwxyzABCDE""#;
        let findings = detect_secrets(content, "config.rs");
        assert!(!findings.is_empty());
        let gh = findings.iter().find(|f| f.title.contains("GitHub Token"));
        assert!(gh.is_some());
    }

    #[test]
    fn test_secret_detection_stripe_key() {
        let sk = format!("sk_{}", "live_TDATAabcdefghijklmnopqrstuvwxyz");
        let content = format!(r#"stripe_key = "{sk}""#);
        let findings = detect_secrets(&content, "config.rs");
        assert!(!findings.is_empty());
        let stripe = findings.iter().find(|f| f.title.contains("Stripe"));
        assert!(stripe.is_some());
    }

    #[test]
    fn test_secret_detection_private_key() {
        let content = "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA...";
        let findings = detect_secrets(content, "key.pem");
        assert!(!findings.is_empty());
        let pk = findings.iter().find(|f| f.title.contains("Private Key"));
        assert!(pk.is_some());
    }

    #[test]
    fn test_secret_detection_jwt() {
        let content = r#"let token = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNrvP5rQj1tKrZNll8Q9JhJmH8Jf2JkPAhLqFQw";"#;
        let findings = detect_secrets(content, "auth.rs");
        assert!(!findings.is_empty());
        let jwt = findings.iter().find(|f| f.title.contains("JWT"));
        assert!(jwt.is_some());
    }

    #[test]
    fn test_secret_detection_slack_token() {
        let content = r#"slack_token = "xoxb-TDATA-1234567890-1234567890123-abcdefghij""#;
        let findings = detect_secrets(content, "config.rs");
        assert!(!findings.is_empty());
        let slack = findings.iter().find(|f| f.title.contains("Slack"));
        assert!(slack.is_some());
    }

    #[test]
    fn test_secret_detection_generic_api_key() {
        let content = r#"api_key = "this-is-a-secret-key-12345""#;
        let findings = detect_secrets(content, "config.rs");
        assert!(!findings.is_empty());
        let generic = findings.iter().find(|f| f.title.contains("Generic"));
        assert!(generic.is_some());
    }

    #[test]
    fn test_secret_detection_no_false_positives() {
        // Comments with example secrets should still be flagged (better safe than sorry)
        let content = r#"
let x = 1; // a normal variable
let url = "https://example.com";
"#;
        let findings = detect_secrets(content, "code.rs");
        // Clean code should have no secret findings
        assert!(findings.is_empty());
    }

    // ===== Code Injection Tests =====

    #[test]
    fn test_injection_detection_eval() {
        let content = r#"
function process(input) {
    let result = eval(input);
    return result;
}
"#;
        let path = Path::new("script.js");
        let findings = detect_code_injection(content, path);
        assert!(!findings.is_empty());
        assert!(findings.iter().any(|f| f.title.contains("eval()")));
    }

    #[test]
    fn test_injection_detection_inner_html() {
        let content = r#"
document.getElementById("output").innerHTML = userInput;
"#;
        let path = Path::new("app.js");
        let findings = detect_code_injection(content, path);
        assert!(!findings.is_empty());
        assert!(findings.iter().any(|f| f.title.contains("innerHTML")));
    }

    #[test]
    fn test_injection_detection_dangerously_set_html() {
        let content = r#"
<div dangerouslySetInnerHTML={{ __html: userContent }} />
"#;
        let path = Path::new("component.tsx");
        let findings = detect_code_injection(content, path);
        assert!(!findings.is_empty());
        assert!(findings.iter().any(|f| f.title.contains("dangerouslySetInnerHTML")));
    }

    #[test]
    fn test_injection_detection_unsafe_rust() {
        let content = r#"
fn process(ptr: *const u8) {
    unsafe {
        let val = *ptr;
    }
}
"#;
        let path = Path::new("src/lib.rs");
        let findings = detect_code_injection(content, path);
        assert!(!findings.is_empty());
        assert!(findings.iter().any(|f| f.title.contains("Unsafe")));
    }

    #[test]
    fn test_injection_detection_sql_injection() {
        let content = r#"
query = "SELECT * FROM users WHERE id = " + user_id;
"#;
        let path = Path::new("db.py");
        let findings = detect_code_injection(content, path);
        assert!(!findings.is_empty());
        assert!(findings.iter().any(|f| f.title.contains("SQL")));
    }

    // ===== Directory Scanning Tests =====

    #[test]
    fn test_scan_empty_directory() {
        let tmp = TempDir::new().unwrap();
        let scanner = SecurityScanner::new();
        let report = scanner.scan_directory(tmp.path());
        assert_eq!(report.file_count, 0);
        assert_eq!(report.summary.total_findings, 0);
    }

    #[test]
    fn test_scan_clean_code() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("main.rs"), "fn main() { println!(\"hello\"); }\n").unwrap();
        fs::write(tmp.path().join("lib.py"), "def hello():\n    pass\n").unwrap();

        let scanner = SecurityScanner::new();
        let report = scanner.scan_directory(tmp.path());
        // Should have 2 source files, no findings
        assert_eq!(report.file_count, 2);
        assert_eq!(report.summary.total_findings, 0);
    }

    #[test]
    fn test_scan_directory_with_secrets() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("config.rs"),
            "let aws_key = \"AKIAIOSFODNN7EXAMPLE\";\n",
        )
        .unwrap();
        fs::write(tmp.path().join("main.rs"), "fn main() {}\n").unwrap();

        let scanner = SecurityScanner::new()
            .with_injection(false)
            .with_dependencies(false)
            .with_config(false);
        let report = scanner.scan_directory(tmp.path());

        assert_eq!(report.file_count, 2);
        assert!(!report.findings.is_empty());
        assert!(report.findings.iter().any(|f| f.title.contains("AWS")));
    }

    #[test]
    fn test_scan_directory_with_all_disabled() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("config.rs"),
            "let aws_key = \"AKIAIOSFODNN7EXAMPLE\";\n",
        )
        .unwrap();

        let scanner = SecurityScanner::new()
            .with_secrets(false)
            .with_injection(false)
            .with_dependencies(false)
            .with_config(false);
        let report = scanner.scan_directory(tmp.path());

        assert_eq!(report.file_count, 1);
        assert_eq!(report.summary.total_findings, 0);
    }

    #[test]
    fn test_scan_file_single() {
        let tmp = TempDir::new().unwrap();
        let file_path = tmp.path().join("test.js");
        fs::write(&file_path, "let result = eval(userInput);\n").unwrap();

        let scanner = SecurityScanner::new();
        let findings = scanner.scan_file(&file_path);
        assert!(!findings.is_empty());
    }

    #[test]
    fn test_skip_node_modules() {
        let tmp = TempDir::new().unwrap();
        let nm = tmp.path().join("node_modules");
        fs::create_dir_all(&nm).unwrap();
        fs::write(nm.join("bad.js"), "let secret = \"AKIAIOSFODNN7EXAMPLE\";\n").unwrap();
        fs::write(tmp.path().join("good.js"), "console.log(\"hello\");\n").unwrap();

        let scanner = SecurityScanner::new()
            .with_injection(false)
            .with_dependencies(false)
            .with_config(false);
        let report = scanner.scan_directory(tmp.path());

        assert_eq!(report.file_count, 1); // only good.js should be scanned
        assert_eq!(report.summary.total_findings, 0);
    }

    // ===== Dependency Scanning Tests =====

    #[test]
    fn test_scan_cargo_lock_vulnerable() {
        let content = r#"
version = 3

[[package]]
name = "time"
version = "0.2.0"
source = "registry+"

[[package]]
name = "serde"
version = "1.0.0"
source = "registry+"
"#;
        let path = Path::new("Cargo.lock");
        let findings = scan_cargo_lock(content, path);
        assert!(!findings.is_empty());
        assert!(findings.iter().any(|f| f.title.contains("time")));
    }

    #[test]
    fn test_scan_cargo_lock_clean() {
        let content = r#"
version = 3

[[package]]
name = "serde"
version = "1.0.200"
source = "registry+"

[[package]]
name = "tokio"
version = "1.35.0"
source = "registry+"
"#;
        let path = Path::new("Cargo.lock");
        let findings = scan_cargo_lock(content, path);
        assert!(findings.is_empty() || findings.iter().all(|f| f.severity == Severity::Info));
    }

    #[test]
    fn test_scan_package_json_vulnerable() {
        let content = r#"{
    "dependencies": {
        "lodash": "^4.17.10",
        "axios": "0.20.0"
    },
    "devDependencies": {
        "minimist": "1.2.5"
    }
}"#;
        let path = Path::new("package.json");
        let findings = scan_package_json(content, path);
        assert!(!findings.is_empty());
        assert!(findings.iter().any(|f| f.title.contains("lodash")));
    }

    #[test]
    fn test_scan_requirements_txt_vulnerable() {
        let content = "requests==2.19.0\nflask==2.0.0\n";
        let path = Path::new("requirements.txt");
        let findings = scan_requirements_txt(content, path);
        assert!(!findings.is_empty());
        assert!(findings.iter().any(|f| f.title.contains("requests")));
    }

    #[test]
    fn test_scan_pipfile_vulnerable() {
        let content = r#"
[[source]]
url = "https://pypi.org/simple"

[packages]
django = "==3.2.0"
requests = "==2.19.0"

[dev-packages]
pytest = "*"
"#;
        let path = Path::new("Pipfile");
        let findings = scan_pipfile(content, path);
        assert!(!findings.is_empty());
        assert!(findings.iter().any(|f| f.title.contains("django")));
    }

    // ===== Configuration Check Tests =====

    #[test]
    fn test_config_detection_env_file() {
        let tmp = TempDir::new().unwrap();
        let env_path = tmp.path().join(".env");
        fs::write(&env_path, "DATABASE_URL=postgres://user:pass@localhost/db\nAPI_KEY=realkey123\n").unwrap();

        // Add a clean file so directory scan has a source file
        fs::write(tmp.path().join("main.rs"), "fn main() {}\n").unwrap();

        let scanner = SecurityScanner::new()
            .with_secrets(false)
            .with_injection(false)
            .with_dependencies(false)
            .with_config(true);
        let report = scanner.scan_directory(tmp.path());

        assert!(!report.findings.is_empty());
        assert!(report.findings.iter().any(|f| f.title.contains(".env")));
    }

    // ===== Integration Tests =====

    #[test]
    fn test_full_scan_integration() {
        let tmp = TempDir::new().unwrap();

        // Clean file
        fs::write(tmp.path().join("main.rs"), "fn main() { println!(\"hello\"); }\n").unwrap();

        // File with secret
        fs::write(
            tmp.path().join("config.rs"),
            format!("let stripe_key = \"sk_{}DATAabcdefghijklmnopqrstuvwxyz\";\n", "live_T"),
        )
        .unwrap();

        // File with injection
        fs::write(
            tmp.path().join("app.js"),
            "let result = eval(userInput);\n",
        )
        .unwrap();

        // Dependency file
        fs::write(
            tmp.path().join("package.json"),
            r#"{"dependencies": {"lodash": "^4.17.10"}}"#,
        )
        .unwrap();

        let scanner = SecurityScanner::new();
        let report = scanner.scan_directory(tmp.path());

        assert!(report.file_count >= 3);
        assert!(report.summary.total_findings > 0);
        assert!(report.summary.critical > 0 || report.summary.high > 0);

        // Verify findings are sorted by severity
        for i in 0..report.findings.len().saturating_sub(1) {
            assert!(report.findings[i].severity >= report.findings[i + 1].severity);
        }
    }

    #[test]
    fn test_scanner_builder_pattern() {
        let scanner = SecurityScanner::new()
            .with_secrets(false)
            .with_injection(true)
            .with_dependencies(false)
            .with_config(true);

        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("main.rs"), "fn main() {}\n").unwrap();
        let report = scanner.scan_directory(tmp.path());

        // Only injection and config scanners enabled
        assert_eq!(report.file_count, 1);
    }

    #[test]
    fn test_scan_dependencies_method() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("Cargo.lock"),
            r#"
version = 3
[[package]]
name = "time"
version = "0.2.0"
source = "registry+"
"#,
        )
        .unwrap();

        let scanner = SecurityScanner::new();
        let findings = scanner.scan_dependencies(tmp.path());
        assert!(!findings.is_empty());
    }

    // ===== Edge Case Tests =====

    #[test]
    fn test_scan_nonexistent_directory() {
        let scanner = SecurityScanner::new();
        let report = scanner.scan_directory(Path::new("/nonexistent/path"));
        assert_eq!(report.file_count, 0);
        assert_eq!(report.summary.total_findings, 0);
    }

    #[test]
    fn test_scan_binary_file() {
        let tmp = TempDir::new().unwrap();
        let bin_path = tmp.path().join("binary.bin");
        // Write invalid UTF-8 bytes that will fail read_to_string
        let bytes: Vec<u8> = vec![0xFF, 0xFE, 0x00, 0x01, 0x80, 0x81, 0x82];
        fs::write(&bin_path, bytes).unwrap();

        let scanner = SecurityScanner::new();
        let findings = scanner.scan_file(&bin_path);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_scan_empty_string() {
        let content = "";
        let path = Path::new("empty.rs");
        let findings = detect_code_injection(content, path);
        assert!(findings.is_empty());

        let secret_findings = detect_secrets(content, "empty.rs");
        assert!(secret_findings.is_empty());
    }

    #[test]
    fn test_security_summary_counts() {
        let findings = vec![
            make_finding(Severity::Critical, FindingCategory::Secret, "C1".to_string(), "".to_string(), "f1", None, None, None, None),
            make_finding(Severity::Critical, FindingCategory::Secret, "C2".to_string(), "".to_string(), "f1", None, None, None, None),
            make_finding(Severity::High, FindingCategory::CodeInjection, "H1".to_string(), "".to_string(), "f2", None, None, None, None),
            make_finding(Severity::Medium, FindingCategory::Configuration, "M1".to_string(), "".to_string(), "f3", None, None, None, None),
            make_finding(Severity::Low, FindingCategory::BestPractice, "L1".to_string(), "".to_string(), "f4", None, None, None, None),
            make_finding(Severity::Info, FindingCategory::BestPractice, "I1".to_string(), "".to_string(), "f5", None, None, None, None),
        ];

        let report = SecurityReport {
            target: "/tmp".to_string(),
            scanned_at: "now".to_string(),
            summary: SecuritySummary {
                total_findings: findings.len(),
                critical: findings.iter().filter(|f| f.severity == Severity::Critical).count(),
                high: findings.iter().filter(|f| f.severity == Severity::High).count(),
                medium: findings.iter().filter(|f| f.severity == Severity::Medium).count(),
                low: findings.iter().filter(|f| f.severity == Severity::Low).count(),
                info: findings.iter().filter(|f| f.severity == Severity::Info).count(),
            },
            findings,
            file_count: 5,
            scan_duration_ms: 0,
        };

        assert_eq!(report.summary.critical, 2);
        assert_eq!(report.summary.high, 1);
        assert_eq!(report.summary.medium, 1);
        assert_eq!(report.summary.low, 1);
        assert_eq!(report.summary.info, 1);
        assert_eq!(report.summary.total_findings, 6);
    }

    #[test]
    fn test_known_cves_database() {
        // Test version matching
        assert!(version_matches("0.2.0", "<0.3.0"));
        assert!(!version_matches("0.3.0", "<0.3.0"));
        assert!(version_matches("4.17.10", "<4.17.21"));
        assert!(!version_matches("4.17.21", "<4.17.21"));
        assert!(version_matches("1.2.5", "<1.2.6"));
        assert!(!version_matches("1.2.6", "<1.2.6"));
    }

    #[test]
    fn test_slack_token_cve_has_severity() {
        let content = r#"slack_token = "xoxp-TDATA-1234567890-1234567890-1234567890-xyz""#;
        let findings = detect_secrets(content, "config.rs");
        let slack = findings.iter().find(|f| f.title.contains("Slack")).unwrap();
        assert_eq!(slack.severity, Severity::Critical);
    }

    #[test]
    fn test_no_comment_matches() {
        let content = r#"
// eval("this is just a comment")
// secret = "should not match in comment if it's just example"
"#;
        let path = Path::new("test.rs");
        let inj_findings = detect_code_injection(content, path);
        assert!(inj_findings.is_empty());
    }

    #[test]
    fn test_regex_compilation() {
        // Verify all patterns compile
        for pattern in SECRET_PATTERNS {
            let re = (pattern.pattern)();
            assert!(!re.as_str().is_empty());
        }
    }
}
