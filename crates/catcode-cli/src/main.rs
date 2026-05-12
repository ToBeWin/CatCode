use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "catcode", version, about = "CatCode - AI coding agent")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
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

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Daemon { action } => match action {
            DaemonAction::Start => {
                println!("Starting CatCode daemon...");
                println!("Use 'catcode-daemon' binary directly, or run the TUI via 'catcode-tui'.");
            }
            DaemonAction::Status => {
                println!("Daemon status: checking...");
                println!("(not implemented - use `catcode-tui` or check daemon process)");
            }
            DaemonAction::Restart => {
                println!("Restarting CatCode daemon...");
                println!("Use 'catcode-daemon' binary directly, or run the TUI via 'catcode-tui'.");
            }
        },
        Commands::Session { action } => match action {
            SessionAction::List => {
                println!("Active sessions:");
                println!("  (Session management coming soon)");
            }
            SessionAction::Create { name } => {
                println!("Creating session '{}'...", name);
                println!("  (Session management coming soon)");
            }
        },
        Commands::Run { message } => {
            println!("Running agent with message: {}", message);
            println!("  (Non-interactive agent mode coming soon)");
            println!("  Mock response: Received '{}', processing not yet implemented.", message);
        }
        Commands::Version => {
            println!("CatCode version {}", env!("CARGO_PKG_VERSION"));
        }
        Commands::Help => {
            // clap prints help automatically, but we handle the explicit subcommand
            println!("CatCode - AI coding agent");
            println!();
            println!("Usage: catcode <command> [options]");
            println!();
            println!("Commands:");
            println!("  daemon start|status|restart    Manage the background daemon");
            println!("  session list|create <name>     Manage agent sessions");
            println!("  run <message>                  Run non-interactive agent");
            println!("  version                        Print version");
            println!("  help                           Print this help");
        }
    }

    Ok(())
}
