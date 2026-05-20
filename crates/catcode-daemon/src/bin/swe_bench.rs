//! SWE-Bench evaluation runner for CatCode.
//!
//! Evaluates the agent on real GitHub issues.
//! Uses sample instances by default, or loads a dataset from file.
//!
//! Usage:
//!   cargo run --bin catcode-swe-bench                     # sample instances, mock provider
//!   cargo run --bin catcode-swe-bench -- --dataset <path>  # real dataset
//!   cargo run --bin catcode-swe-bench -- --help

use catcode_daemon::swe_bench::{
    SweBenchConfig, SweBenchHarness, format_summary, load_dataset, sample_instances, save_results,
};
use catcode_middleware::MiddlewareChain;
use catcode_provider::deepseek::DeepSeekProvider;
use catcode_provider::mock::MockProvider;
use catcode_tools::ToolRegistry;
use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "catcode-swe-bench", about = "SWE-Bench evaluation runner")]
struct Args {
    /// Path to SWE-Bench dataset JSON file
    #[arg(long)]
    dataset: Option<PathBuf>,

    /// Provider name (deepseek, anthropic, openai, etc.)
    #[arg(long, default_value = "mock")]
    provider: String,

    /// Model ID
    #[arg(long)]
    model: Option<String>,

    /// Working directory for repo clones
    #[arg(long, default_value = "/tmp/swe-bench")]
    work_dir: PathBuf,

    /// Number of parallel instances
    #[arg(long, default_value_t = 2)]
    parallel: usize,

    /// Output directory for results
    #[arg(long, default_value = "./swe-bench-results")]
    output: PathBuf,

    /// Keep repos after evaluation
    #[arg(long)]
    keep_repos: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let args = Args::parse();
    let model = args.model.unwrap_or_else(|| "deepseek-chat".to_string());

    let provider: Arc<dyn catcode_core::Provider> = match args.provider.as_str() {
        "mock" => Arc::new(MockProvider::with_text_response(
            "I've analyzed the issue. Here's the fix:\n\n```rust\nfn hello_world() -> &'static str {\n    \"Hello, SWE-Bench!\"\n}\n```",
        )),
        "deepseek" => {
            let api_key =
                std::env::var("DEEPSEEK_API_KEY").expect("DEEPSEEK_API_KEY env var required");
            Arc::new(DeepSeekProvider::new(
                api_key,
                "https://api.deepseek.com".to_string(),
            ))
        }
        other => anyhow::bail!("Unsupported provider: {}. Use 'mock' or 'deepseek'", other),
    };

    let tools = Arc::new(ToolRegistry::with_builtins());
    let middleware = Arc::new(MiddlewareChain::new());

    let instances = if let Some(path) = &args.dataset {
        tracing::info!("Loading dataset from {}", path.display());
        load_dataset(path)?
    } else {
        tracing::info!("Using sample instances");
        sample_instances()
    };

    tracing::info!(
        "Running SWE-Bench evaluation: {} instances, {} parallel, provider={}",
        instances.len(),
        args.parallel,
        args.provider
    );

    let config = SweBenchConfig {
        work_dir: args.work_dir,
        parallel_instances: args.parallel,
        keep_repos: args.keep_repos,
        model: Some(model),
        ..Default::default()
    };

    let harness = SweBenchHarness::new(config, provider, tools, middleware);
    let report = harness.evaluate_all(&instances).await;

    println!("\n{}", format_summary(&report));

    save_results(&report, &args.output)?;
    tracing::info!("Results saved to {}", args.output.display());

    Ok(())
}
