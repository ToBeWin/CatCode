#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let project_dir = std::env::current_dir()?;
    catcode_tui::run(project_dir).await
}
