use clap::{Parser, Subcommand};

mod app;
mod daemon;

#[derive(Parser)]
#[command(name = "yola-tui", about = "Terminal UI for YOLA", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Daemon port (default: 7779)
    #[arg(long, short, default_value = "7779")]
    port: u16,
}

#[derive(Subcommand)]
enum Commands {
    /// Start interactive chat mode (default)
    Chat,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command.unwrap_or(Commands::Chat) {
        Commands::Chat => app::run(cli.port).await,
    }
}
