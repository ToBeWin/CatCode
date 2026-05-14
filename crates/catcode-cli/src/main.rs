use std::path::PathBuf;

use anyhow::{Context, bail};
use catcode_daemon::{
    AgentRuntime, AgentRuntimeOptions, default_system_prompt, load_config, project_dir_or_current,
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
    /// Print version information
    Version,
    /// Print this help message
    Help,
}

#[derive(Subcommand)]
enum DaemonAction {
    /// Start the daemon
    Start,
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
                println!("Starting CatCode daemon...");
                println!("Use 'catcode-daemon' binary directly, or run the TUI via 'catcode-tui'.");
            }
            DaemonAction::Status => {
                print_daemon_status().await?;
            }
            DaemonAction::Restart => {
                println!("Restarting CatCode daemon...");
                println!("Use 'catcode-daemon' binary directly, or run the TUI via 'catcode-tui'.");
            }
        },
        Commands::Session { action } => match action {
            SessionAction::List => {
                list_remote_sessions().await?;
            }
            SessionAction::Create { name } => {
                create_remote_session(name).await?;
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
            println!("  daemon start|status|restart    Manage the background daemon");
            println!("  session list|create <name>     Manage agent sessions");
            println!("  run <message>                  Run non-interactive agent");
            println!("  version                        Print version");
            println!("  help                           Print this help");
        }
    }

    Ok(())
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
