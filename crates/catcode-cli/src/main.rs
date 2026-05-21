use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, bail};
use catcode_daemon::{
    AgentRuntime, AgentRuntimeOptions, Config, DiffSummary, build_harness_plan,
    capture_git_snapshot, default_system_prompt, load_config, project_dir_or_current,
    review_workspace_changes, run_handoff_report,
};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "catcode",
    version,
    about = "CatCode - AI coding agent",
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Interactive config generation
    Init,
    /// Manage the CatCode daemon
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
    /// Manage agent sessions
    Session {
        #[command(subcommand)]
        action: SessionAction,
    },
    /// Run a non-interactive agent session
    Run {
        /// Message to send to the agent
        message: String,
        /// Provider id to use (overrides config)
        #[arg(long)]
        provider: Option<String>,
        /// Model id to use (overrides config)
        #[arg(long)]
        model: Option<String>,
        /// Project directory for tool execution and project rules
        #[arg(long, value_name = "DIR")]
        project_dir: Option<PathBuf>,
    },
    /// Show the coding harness plan for a repository
    Harness {
        /// Task or prompt to plan for
        task: Option<String>,
        /// Project directory to inspect
        #[arg(long, value_name = "DIR")]
        project_dir: Option<PathBuf>,
        /// Print the raw JSON plan
        #[arg(long)]
        json: bool,
    },
    /// Show current working tree changes
    Changes {
        /// Project directory to inspect
        #[arg(long, value_name = "DIR")]
        project_dir: Option<PathBuf>,
        /// Print the raw JSON summary
        #[arg(long)]
        json: bool,
    },
    /// Review current working tree changes
    Review {
        /// Project directory to inspect
        #[arg(long, value_name = "DIR")]
        project_dir: Option<PathBuf>,
        /// Print the raw JSON review
        #[arg(long)]
        json: bool,
    },
    /// Run final handoff checks for current changes
    Handoff {
        /// Task or prompt these changes are meant to satisfy
        #[arg(value_name = "TASK", num_args = 0..)]
        task: Vec<String>,
        /// Project directory to inspect
        #[arg(long, value_name = "DIR")]
        project_dir: Option<PathBuf>,
        /// Print the raw JSON handoff report
        #[arg(long)]
        json: bool,
    },
    /// Print version information
    Version,
    /// Print this help message
    Help,
}

#[derive(Subcommand)]
enum DaemonAction {
    /// Start the daemon
    Start,
    /// Stop the daemon started by this CLI
    Stop,
    /// Check daemon status
    Status,
    /// Restart the daemon
    Restart,
}

#[derive(Subcommand)]
enum SessionAction {
    /// List all sessions
    List,
    /// Create a new session
    Create {
        /// Session name
        name: String,
    },
    /// Show audit log entries for a session
    Audit {
        /// Session ID
        id: String,
    },
    /// Show persisted message history for a session
    Messages {
        /// Session ID
        id: String,
    },
    /// Show aggregated token usage for a session
    Usage {
        /// Session ID
        id: String,
    },
    /// Show a recovery plan for a session
    Recovery {
        /// Session ID
        id: String,
    },
}

/// Known provider metadata (mirrors catcode-provider crate).
const KNOWN_PROVIDERS: &[(&str, &str, &str, &[&str])] = &[
    (
        "deepseek",
        "DeepSeek",
        "deepseek-chat",
        &["deepseek-chat", "deepseek-reasoner"],
    ),
    (
        "openai",
        "OpenAI",
        "gpt-4o",
        &["gpt-4o", "gpt-4o-mini", "gpt-4.1"],
    ),
    (
        "anthropic",
        "Anthropic",
        "claude-sonnet-4-20250514",
        &["claude-sonnet-4-20250514", "claude-haiku-3-5"],
    ),
    (
        "google",
        "Google",
        "gemini-2.0-flash",
        &["gemini-2.0-flash", "gemini-2.5-pro"],
    ),
    (
        "openrouter",
        "OpenRouter",
        "openrouter/auto",
        &["openrouter/auto"],
    ),
    (
        "qwen",
        "Qwen (DashScope)",
        "qwen-plus",
        &["qwen-plus", "qwen-max"],
    ),
    ("glm", "GLM (Zhipu)", "glm-4-plus", &["glm-4-plus"]),
    ("minimax", "MiniMax", "MiniMax-M1", &["MiniMax-M1"]),
    ("volcengine", "Volcengine", "deepseek-v3", &["deepseek-v3"]),
    ("ollama", "Ollama (local)", "llama3.1", &["llama3.1"]),
];

fn run_init() -> anyhow::Result<()> {
    use std::io::{self, Write};

    let config_dir = dirs::config_dir()
        .map(|p| p.join("catcode"))
        .unwrap_or_else(|| std::path::PathBuf::from("./.catcode"));
    std::fs::create_dir_all(&config_dir)?;

    let config_path = config_dir.join("config.toml");
    let local_config = std::path::PathBuf::from("./catcode.toml");

    println!("╔══════════════════════════════════════════╗");
    println!("║       CatCode Configuration             ║");
    println!("╚══════════════════════════════════════════╝");
    println!();

    // --- Provider selection ---
    println!("Available providers:");
    for (i, (id, name, default_model, _)) in KNOWN_PROVIDERS.iter().enumerate() {
        println!(
            "  {}. {} ({}) — default model: {}",
            i + 1,
            name,
            id,
            default_model
        );
    }
    println!();

    let provider_idx = loop {
        print!("Select provider [1-{}]: ", KNOWN_PROVIDERS.len());
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let trimmed = input.trim();
        if trimmed.is_empty() {
            println!("  Using default: DeepSeek");
            break 0usize;
        }
        match trimmed.parse::<usize>() {
            Ok(n) if n >= 1 && n <= KNOWN_PROVIDERS.len() => break n - 1,
            _ => println!(
                "  Please enter a number between 1 and {}.",
                KNOWN_PROVIDERS.len()
            ),
        }
    };

    let (provider_id, provider_name, default_model, available_models) =
        KNOWN_PROVIDERS[provider_idx];

    // --- API key ---
    println!();
    print!("API key (leave empty to skip): ");
    io::stdout().flush()?;
    let mut api_key = String::new();
    io::stdin().read_line(&mut api_key)?;
    let api_key = api_key.trim().to_string();

    // --- Model selection ---
    println!();
    println!("Available models for {}:", provider_name);
    for (i, m) in available_models.iter().enumerate() {
        let marker = if *m == default_model {
            " (default)"
        } else {
            ""
        };
        println!("  {}. {}{}", i + 1, m, marker);
    }
    println!();

    let model = loop {
        print!("Select model [1-{}]: ", available_models.len());
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let trimmed = input.trim();
        if trimmed.is_empty() {
            println!("  Using default: {}", default_model);
            break default_model;
        }
        match trimmed.parse::<usize>() {
            Ok(n) if n >= 1 && n <= available_models.len() => break available_models[n - 1],
            _ => println!(
                "  Please enter a number between 1 and {}.",
                available_models.len()
            ),
        }
    };

    // --- Budget ---
    println!();
    print!("Token budget per session (default: 500000): ");
    io::stdout().flush()?;
    let mut budget_input = String::new();
    io::stdin().read_line(&mut budget_input)?;
    let session_limit: u64 = match budget_input.trim() {
        "" => 500_000,
        s => s.parse().unwrap_or_else(|_| {
            println!("  Invalid number, using default: 500000");
            500_000
        }),
    };

    print!("Per-request token limit (default: 50000): ");
    io::stdout().flush()?;
    let mut per_req_input = String::new();
    io::stdin().read_line(&mut per_req_input)?;
    let per_request_limit: u64 = match per_req_input.trim() {
        "" => 50_000,
        s => s.parse().unwrap_or_else(|_| {
            println!("  Invalid number, using default: 50000");
            50_000
        }),
    };

    // --- Build config TOML ---
    let toml_str = format!(
        r#"[daemon]
host = "127.0.0.1"
port = 7070
auto_start = true
max_concurrent_sessions = 5
checkpoint_interval_turns = 10

[defaults]
provider = "{}"
model = "{}"
sandbox = false

[budget]
session_limit_tokens = {}
per_request_limit_tokens = {}
warning_threshold = 0.80

[context]
compression_enabled = true
dedup_tool_outputs = true
max_file_content_tokens = 8000

[observability]
log_level = "info"
log_format = "text"
"#,
        provider_id, model, session_limit, per_request_limit,
    );

    // --- Write config ---
    println!();
    println!("Where to write config?");
    println!("  1. {} (global)", config_path.display());
    println!("  2. ./catcode.toml (project-local)");
    print!("Choose [1]: ");
    io::stdout().flush()?;
    let mut loc_input = String::new();
    io::stdin().read_line(&mut loc_input)?;

    let (write_path, label) = match loc_input.trim() {
        "2" => (local_config, "./catcode.toml".to_string()),
        _ => {
            let p = config_path.display().to_string();
            (config_path, p)
        }
    };

    // Ensure parent dir exists
    if let Some(parent) = write_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(&write_path, &toml_str)?;
    println!();
    println!("  ✓ Config written to {}", label);

    if !api_key.is_empty() {
        // Store API key in a separate .env file (not committed)
        let env_path = write_path.with_file_name(".env");
        std::fs::write(
            &env_path,
            format!(
                "# CatCode API key for {}\nCATCODE_API_KEY={}\n",
                provider_id, api_key
            ),
        )?;
        println!("  ✓ API key saved to {}", env_path.display());
        println!(
            "  ⚠  Add '{}' to your .gitignore!",
            env_path.file_name().unwrap().to_string_lossy()
        );
    }

    println!();
    println!("Next steps:");
    println!("  1. Run 'catcode-tui' to launch the terminal UI");
    println!("  2. Type /help to see all commands");
    println!("  3. Start coding!");

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init => {
            run_init()?;
        }
        Commands::Daemon { action } => match action {
            DaemonAction::Start => {
                start_daemon_process().await?;
            }
            DaemonAction::Stop => {
                stop_daemon_process().await?;
            }
            DaemonAction::Status => {
                print_daemon_status().await?;
            }
            DaemonAction::Restart => {
                let _ = stop_daemon_process().await;
                start_daemon_process().await?;
            }
        },
        Commands::Session { action } => match action {
            SessionAction::List => {
                list_remote_sessions().await?;
            }
            SessionAction::Create { name } => {
                create_remote_session(name).await?;
            }
            SessionAction::Audit { id } => {
                show_session_audit(id).await?;
            }
            SessionAction::Messages { id } => {
                show_session_messages(id).await?;
            }
            SessionAction::Usage { id } => {
                show_session_usage(id).await?;
            }
            SessionAction::Recovery { id } => {
                show_session_recovery(id).await?;
            }
        },
        Commands::Run {
            message,
            provider,
            model,
            project_dir,
        } => {
            run_non_interactive(message, provider, model, project_dir).await?;
        }
        Commands::Harness {
            task,
            project_dir,
            json,
        } => {
            show_harness_plan(task, project_dir, json)?;
        }
        Commands::Changes { project_dir, json } => {
            show_workspace_changes(project_dir, json).await?;
        }
        Commands::Review { project_dir, json } => {
            show_code_review(project_dir, json).await?;
        }
        Commands::Handoff {
            task,
            project_dir,
            json,
        } => {
            show_handoff_report(task, project_dir, json).await?;
        }
        Commands::Version => {
            println!("CatCode version {}", env!("CARGO_PKG_VERSION"));
        }
        Commands::Help => {
            println!("CatCode - AI coding agent");
            println!();
            println!("Usage: catcode <command> [options]");
            println!();
            println!("Commands:");
            println!("  init                           Interactive config generation");
            println!("  daemon start|stop|status|restart");
            println!("                                 Manage the background daemon");
            println!(
                "  session list|create <name>|audit <id>|messages <id>|usage <id>|recovery <id>"
            );
            println!("                                 Manage agent sessions");
            println!("  run <message>                  Run non-interactive agent");
            println!("  harness [task] [--project-dir <dir>] [--json]");
            println!("                                 Show coding harness plan");
            println!("  changes [--project-dir <dir>] [--json]");
            println!("                                 Show current working tree changes");
            println!("  review [--project-dir <dir>] [--json]");
            println!("                                 Review current working tree changes");
            println!("  handoff [task] [--project-dir <dir>] [--json]");
            println!("                                 Run changes, review, and verification gate");
            println!("  version                        Print version");
            println!("  help                           Print this help");
        }
    }

    Ok(())
}

fn show_harness_plan(
    task: Option<String>,
    project_dir: Option<PathBuf>,
    json: bool,
) -> anyhow::Result<()> {
    let project_dir = project_dir_or_current(project_dir)?;
    let task = task.unwrap_or_else(|| "inspect repository harness".to_string());
    let plan = build_harness_plan(&project_dir, &task);

    if json {
        println!("{}", serde_json::to_string_pretty(&plan)?);
        return Ok(());
    }

    let phases = plan
        .phases
        .iter()
        .map(|phase| format!("{phase:?}"))
        .collect::<Vec<_>>()
        .join(" -> ");
    let stack = format_cli_list(&plan.repo.language_stack);
    let managers = format_cli_list(&plan.repo.package_managers);
    let verification = if plan.verification.commands.is_empty() {
        plan.verification.safety_note.clone()
    } else {
        plan.verification
            .commands
            .iter()
            .map(|command| {
                let mode = if command.auto_run {
                    "auto-runnable"
                } else {
                    "manual/agent-confirmed"
                };
                format!("{} ({mode}; {})", command.command, command.reason)
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let files = format_cli_list(&plan.repo.important_files);

    println!("Coding harness plan");
    println!("Project: {}", project_dir.display());
    println!("Task: {}", plan.task_summary);
    println!("Stack: {stack}");
    println!("Package managers: {managers}");
    println!("Git: {}", plan.repo.has_git);
    println!("Phases: {phases}");
    println!("Suggested verification: {verification}");
    println!("Verification safety: {}", plan.verification.safety_note);
    println!("Important files: {files}");
    println!();
    println!("Instructions:");
    for instruction in plan.instructions {
        println!("  - {instruction}");
    }

    Ok(())
}

async fn show_workspace_changes(project_dir: Option<PathBuf>, json: bool) -> anyhow::Result<()> {
    let project_dir = project_dir_or_current(project_dir)?;
    let snapshot = capture_git_snapshot(&project_dir)
        .await
        .with_context(|| format!("failed to read git status for {}", project_dir.display()))?;
    let diff = DiffSummary::from_snapshot(&snapshot);
    let clean = diff.changed_files.is_empty();
    let summary = if clean {
        "Working tree clean.".to_string()
    } else {
        diff.summary_line()
    };

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "project_dir": project_dir.display().to_string(),
                "clean": clean,
                "changed_files": diff.changed_files,
                "summary": summary,
            }))?
        );
        return Ok(());
    }

    println!("Workspace changes");
    println!("Project: {}", project_dir.display());
    println!("{}", summary);
    if !clean {
        println!();
        for file in diff.changed_files {
            println!("  - {file}");
        }
    }

    Ok(())
}

async fn show_code_review(project_dir: Option<PathBuf>, json: bool) -> anyhow::Result<()> {
    let project_dir = project_dir_or_current(project_dir)?;
    let review = review_workspace_changes(&project_dir).await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&review)?);
        return Ok(());
    }

    println!("Code review");
    println!("Project: {}", project_dir.display());
    println!("Score: {}/100", review.overall_score);
    println!("{}", review.summary);
    println!(
        "Files reviewed: {}",
        format_cli_list(&review.files_reviewed)
    );

    if review.findings.is_empty() {
        if !review.positive_notes.is_empty() {
            println!();
            println!("Positive notes:");
            for note in review.positive_notes {
                println!("  - {note}");
            }
        }
        return Ok(());
    }

    println!();
    println!("Findings:");
    for finding in review.findings {
        let line = finding
            .line
            .map(|line| format!(":{line}"))
            .unwrap_or_default();
        println!(
            "  - {:?}/{:?} {}{}: {}",
            finding.severity, finding.category, finding.file, line, finding.title
        );
        println!("    {}", finding.description);
        if let Some(suggestion) = finding.suggestion {
            println!("    Suggestion: {suggestion}");
        }
    }

    Ok(())
}

async fn show_handoff_report(
    task: Vec<String>,
    project_dir: Option<PathBuf>,
    json: bool,
) -> anyhow::Result<()> {
    let project_dir = project_dir_or_current(project_dir)?;
    let task = if task.is_empty() {
        "final handoff".to_string()
    } else {
        task.join(" ")
    };
    let report = run_handoff_report(&project_dir, &task).await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("Final handoff");
    println!("Project: {}", report.project_dir);
    println!("Task: {}", report.task_summary);
    println!("Ready: {}", if report.ready { "yes" } else { "no" });
    println!(
        "Changes: {}",
        if report.changes.changed_files.is_empty() {
            "none".to_string()
        } else {
            report.changes.summary_line()
        }
    );
    println!(
        "Review: score {}/100, {} finding(s)",
        report.review.overall_score,
        report.review.findings.len()
    );
    match report.verification.as_ref() {
        Some(result) => println!("Verification: {}", result.summary()),
        None => println!("Verification: not run"),
    }
    if let Some(diagnostic) = report
        .verification
        .as_ref()
        .and_then(|result| result.diagnostic())
    {
        println!("Diagnostic: {}", diagnostic.summary);
        if !diagnostic.locations.is_empty() {
            println!("Locations: {}", diagnostic.locations.join(", "));
        }
    }
    if let Some(plan) = report
        .verification
        .as_ref()
        .and_then(|result| result.repair_plan())
    {
        println!("Repair plan: {}", plan.summary);
        if !plan.files_to_inspect.is_empty() {
            println!("Inspect: {}", plan.files_to_inspect.join(", "));
        }
        println!("Repair verification: {}", plan.verification_command);
    }

    if !report.blockers.is_empty() {
        println!();
        println!("Blockers:");
        for blocker in &report.blockers {
            println!("  - {blocker}");
        }
    }

    if !report.recommendations.is_empty() {
        println!();
        println!("Recommendations:");
        for recommendation in &report.recommendations {
            println!("  - {recommendation}");
        }
    }

    if !report.review.findings.is_empty() {
        println!();
        println!("Top findings:");
        for finding in report.review.findings.iter().take(8) {
            let line = finding
                .line
                .map(|line| format!(":{line}"))
                .unwrap_or_default();
            println!(
                "  - {:?}/{:?} {}{}: {}",
                finding.severity, finding.category, finding.file, line, finding.title
            );
        }
    }

    Ok(())
}

fn format_cli_list(items: &[String]) -> String {
    if items.is_empty() {
        "none".to_string()
    } else {
        items.join(", ")
    }
}

async fn run_non_interactive(
    message: String,
    provider_override: Option<String>,
    model_override: Option<String>,
    project_dir: Option<PathBuf>,
) -> anyhow::Result<()> {
    let project_dir = project_dir_or_current(project_dir)?;

    let config = load_config(&project_dir)?;
    let provider_id = provider_override
        .clone()
        .unwrap_or_else(|| config.defaults.provider.clone());
    let model_id = model_override
        .clone()
        .unwrap_or_else(|| config.defaults.model.clone());

    println!(
        "Running CatCode with provider '{}' model '{}' in {}",
        provider_id,
        model_id,
        project_dir.display()
    );

    let result = AgentRuntime::new()
        .run_once(
            &message,
            &project_dir,
            AgentRuntimeOptions {
                provider_id: provider_override,
                model_id: model_override,
                system_prompt: default_system_prompt().to_string(),
                ..Default::default()
            },
        )
        .await?;

    if let Some(plan) = result.auto_plan.as_deref()
        && !plan.trim().is_empty()
    {
        println!("\nPlan:\n{}\n", plan.trim());
    }

    println!("{}", result.response.trim());
    println!(
        "\nTurns: {} | Tokens: input {}, output {}",
        result.turns_used, result.total_usage.input_tokens, result.total_usage.output_tokens
    );

    Ok(())
}

async fn print_daemon_status() -> anyhow::Result<()> {
    let base = api_base_url()?;
    let url = format!("{base}/api/v1/health");
    let value = reqwest::get(&url)
        .await
        .with_context(|| format!("failed to connect to CatCode daemon at {url}"))?
        .error_for_status()
        .with_context(|| format!("CatCode daemon returned an error for {url}"))?
        .json::<serde_json::Value>()
        .await?;

    println!(
        "Daemon status: {}",
        value["status"].as_str().unwrap_or("unknown")
    );
    println!("Endpoint: {}", base);
    Ok(())
}

async fn start_daemon_process() -> anyhow::Result<()> {
    let base = api_base_url()?;
    if daemon_health_ok(&base).await {
        println!("CatCode daemon already running at {base}");
        return Ok(());
    }

    let daemon_bin = find_daemon_binary()?;
    let mut child = Command::new(&daemon_bin)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to start {}", daemon_bin.display()))?;

    for _ in 0..30 {
        if daemon_health_ok(&base).await {
            write_daemon_pid(child.id())?;
            println!("CatCode daemon started at {base} (pid {})", child.id());
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    let _ = child.kill();
    let _ = child.wait();
    bail!(
        "started {} but health check did not become ready at {base}",
        daemon_bin.display()
    )
}

async fn stop_daemon_process() -> anyhow::Result<()> {
    let pid_path = daemon_pid_path()?;
    let pid = std::fs::read_to_string(&pid_path)
        .with_context(|| format!("no daemon pid file found at {}", pid_path.display()))?
        .trim()
        .parse::<u32>()
        .with_context(|| format!("invalid daemon pid in {}", pid_path.display()))?;

    let status = Command::new("kill")
        .arg(pid.to_string())
        .status()
        .with_context(|| format!("failed to signal daemon pid {pid}"))?;

    if !status.success() {
        bail!("failed to stop daemon pid {pid}");
    }

    let base = api_base_url()?;
    for _ in 0..30 {
        if !daemon_health_ok(&base).await {
            let _ = std::fs::remove_file(&pid_path);
            println!("CatCode daemon stopped (pid {pid})");
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    bail!("sent stop signal to daemon pid {pid}, but health check is still responding at {base}")
}

async fn daemon_health_ok(base: &str) -> bool {
    let url = format!("{base}/api/v1/health");
    match reqwest::get(url).await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

fn find_daemon_binary() -> anyhow::Result<PathBuf> {
    let current_exe = std::env::current_exe().context("failed to resolve current executable")?;
    if let Some(dir) = current_exe.parent() {
        let sibling = dir.join("catcode-daemon");
        if sibling.exists() {
            return Ok(sibling);
        }
    }

    Ok(PathBuf::from("catcode-daemon"))
}

fn write_daemon_pid(pid: u32) -> anyhow::Result<()> {
    let pid_path = daemon_pid_path()?;
    if let Some(parent) = pid_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(pid_path, pid.to_string())?;
    Ok(())
}

fn daemon_pid_path() -> anyhow::Result<PathBuf> {
    let project_dir = std::env::current_dir().context("failed to resolve project directory")?;
    Ok(Config::data_dir(&project_dir).join("daemon.pid"))
}

async fn list_remote_sessions() -> anyhow::Result<()> {
    let base = api_base_url()?;
    let url = format!("{base}/api/v1/sessions");
    let value = reqwest::get(&url)
        .await
        .with_context(|| format!("failed to connect to CatCode daemon at {url}"))?
        .error_for_status()
        .with_context(|| format!("CatCode daemon returned an error for {url}"))?
        .json::<serde_json::Value>()
        .await?;

    let Some(sessions) = value["data"].as_array() else {
        bail!("unexpected sessions response: {value}");
    };

    if sessions.is_empty() {
        println!("No active sessions.");
        return Ok(());
    }

    println!("Active sessions:");
    for session in sessions {
        println!(
            "  {}  {}  [{} / {}]  turns={}",
            session["id"].as_str().unwrap_or("<missing-id>"),
            session["name"].as_str().unwrap_or("<unnamed>"),
            session["provider_id"].as_str().unwrap_or("?"),
            session["model_id"].as_str().unwrap_or("?"),
            session["turn_count"].as_u64().unwrap_or(0)
        );
    }
    Ok(())
}

async fn create_remote_session(name: String) -> anyhow::Result<()> {
    let project_dir = std::env::current_dir().context("failed to resolve project directory")?;
    let config = load_config(&project_dir)?;
    let base = api_base_url()?;
    let url = format!("{base}/api/v1/sessions");
    let body = serde_json::json!({
        "name": name,
        "project_dir": project_dir,
        "model_id": config.defaults.model,
        "provider_id": config.defaults.provider,
    });

    let value = reqwest::Client::new()
        .post(&url)
        .json(&body)
        .send()
        .await
        .with_context(|| format!("failed to connect to CatCode daemon at {url}"))?
        .error_for_status()
        .with_context(|| format!("CatCode daemon returned an error for {url}"))?
        .json::<serde_json::Value>()
        .await?;

    if !value["ok"].as_bool().unwrap_or(false) {
        bail!("failed to create session: {}", value["error"]);
    }

    let data = &value["data"];
    println!(
        "Created session {} ({}) with {}/{}",
        data["id"].as_str().unwrap_or("<missing-id>"),
        data["name"].as_str().unwrap_or("<unnamed>"),
        data["provider_id"].as_str().unwrap_or("?"),
        data["model_id"].as_str().unwrap_or("?")
    );
    Ok(())
}

async fn show_session_audit(id: String) -> anyhow::Result<()> {
    let base = api_base_url()?;
    let url = format!("{base}/api/v1/sessions/{id}/audit");
    let value = reqwest::get(&url)
        .await
        .with_context(|| format!("failed to connect to CatCode daemon at {url}"))?
        .error_for_status()
        .with_context(|| format!("CatCode daemon returned an error for {url}"))?
        .json::<serde_json::Value>()
        .await?;

    if !value["ok"].as_bool().unwrap_or(false) {
        bail!("failed to fetch audit log: {}", value["error"]);
    }

    let Some(entries) = value["data"].as_array() else {
        bail!("unexpected audit response: {value}");
    };

    if entries.is_empty() {
        println!("No audit log entries for session {id}.");
        return Ok(());
    }

    println!("Audit log for session {id}:");
    for entry in entries {
        let tool = entry["tool"].as_str().unwrap_or("-");
        let level = entry["level"].as_str().unwrap_or("unknown");
        let operation = entry["operation"].as_str().unwrap_or("operation");
        let result = entry["result"].as_str().unwrap_or("unknown");
        let created_at = entry["created_at"].as_str().unwrap_or("-");
        println!("  {created_at}  {level:<9}  {operation:<10}  {tool:<16}  {result}");
    }

    Ok(())
}

async fn show_session_messages(id: String) -> anyhow::Result<()> {
    let base = api_base_url()?;
    let url = format!("{base}/api/v1/sessions/{id}/messages");
    let value = reqwest::get(&url)
        .await
        .with_context(|| format!("failed to connect to CatCode daemon at {url}"))?
        .error_for_status()
        .with_context(|| format!("CatCode daemon returned an error for {url}"))?
        .json::<serde_json::Value>()
        .await?;

    if !value["ok"].as_bool().unwrap_or(false) {
        bail!("failed to fetch messages: {}", value["error"]);
    }

    let Some(messages) = value["data"].as_array() else {
        bail!("unexpected messages response: {value}");
    };

    if messages.is_empty() {
        println!("No persisted messages for session {id}.");
        return Ok(());
    }

    println!("Messages for session {id}:");
    for message in messages {
        let role = message["role"].as_str().unwrap_or("unknown");
        let created_at = message["created_at"].as_str().unwrap_or("-");
        let content = message["content"].as_str().unwrap_or("");
        println!("\n[{created_at}] {role}");
        println!("{}", content.trim());
    }

    Ok(())
}

async fn show_session_usage(id: String) -> anyhow::Result<()> {
    let base = api_base_url()?;
    let url = format!("{base}/api/v1/sessions/{id}/usage");
    let value = reqwest::get(&url)
        .await
        .with_context(|| format!("failed to connect to CatCode daemon at {url}"))?
        .error_for_status()
        .with_context(|| format!("CatCode daemon returned an error for {url}"))?
        .json::<serde_json::Value>()
        .await?;

    if !value["ok"].as_bool().unwrap_or(false) {
        bail!("failed to fetch usage: {}", value["error"]);
    }

    let data = &value["data"];
    println!("Usage for session {id}:");
    println!(
        "  input={} output={} cache={} total={} cost=${:.6}",
        data["input_tokens"].as_i64().unwrap_or(0),
        data["output_tokens"].as_i64().unwrap_or(0),
        data["cache_read_tokens"].as_i64().unwrap_or(0),
        data["total_tokens"].as_i64().unwrap_or(0),
        data["cost_usd"].as_f64().unwrap_or(0.0),
    );

    Ok(())
}

async fn show_session_recovery(id: String) -> anyhow::Result<()> {
    let base = api_base_url()?;
    let url = format!("{base}/api/v1/sessions/{id}/recovery");
    let value = reqwest::get(&url)
        .await
        .with_context(|| format!("failed to connect to CatCode daemon at {url}"))?
        .error_for_status()
        .with_context(|| format!("CatCode daemon returned an error for {url}"))?
        .json::<serde_json::Value>()
        .await?;

    if !value["ok"].as_bool().unwrap_or(false) {
        bail!("failed to fetch recovery plan: {}", value["error"]);
    }

    let data = &value["data"];
    println!(
        "Recovery plan for session {}:",
        data["session_id"].as_str().unwrap_or(&id)
    );
    println!(
        "  state={} total_tokens={}",
        data["state"].as_str().unwrap_or("unknown"),
        data["usage"]["total_tokens"].as_i64().unwrap_or(0)
    );
    if let Some(reason) = data["failure_reason"].as_str() {
        println!("  failure={reason}");
    }
    println!();
    println!(
        "{}",
        data["summary"].as_str().unwrap_or("No summary available.")
    );

    if let Some(steps) = data["next_steps"].as_array()
        && !steps.is_empty()
    {
        println!();
        println!("Next steps:");
        for (idx, step) in steps.iter().enumerate() {
            println!("  {}. {}", idx + 1, step.as_str().unwrap_or(""));
        }
    }

    if let Some(messages) = data["recent_messages"].as_array()
        && !messages.is_empty()
    {
        println!();
        println!("Recent messages:");
        for message in messages {
            println!(
                "  [{}] {}",
                message["role"].as_str().unwrap_or("unknown"),
                message["content"].as_str().unwrap_or("").trim()
            );
        }
    }

    Ok(())
}

fn api_base_url() -> anyhow::Result<String> {
    if let Ok(url) = std::env::var("CATCODE_API_URL") {
        return Ok(url.trim_end_matches('/').to_string());
    }

    let project_dir = std::env::current_dir().context("failed to resolve project directory")?;
    let config = load_config(&project_dir)?;
    Ok(format!(
        "http://{}:{}",
        config.daemon.host, config.daemon.port
    ))
}
