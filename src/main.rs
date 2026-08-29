pub mod config;
mod commands {
    pub mod analyze;
    pub mod bundle;
    pub mod generate_completions;
    pub mod serve;
}

use commands::analyze::analyze;
use commands::bundle::bundle;
use commands::generate_completions::generate_completions;
use commands::serve::serve;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::Level;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(short, long, default_value = "opacity.toml")]
    config: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Bundle all Luau files
    Bundle {
        /// Bundle only this flow (matched by alias); bundles every flow when omitted
        flow_name: Option<String>,
    },

    /// Analyze all Luau files
    Analyze,

    /// Generate completions for a given shell
    #[command(name = "completions")]
    GenerateCompletions {
        /// The shell to generate completions for
        shell: String,
    },

    /// Serve Lua flows over HTTP (and rebundle only the requested flow, if rebundle is enabled)
    Serve {
        /// Rebundle only the requested flow, if rebundle is enabled
        #[arg(short, long)]
        rebundle: bool,

        /// Port to serve on
        #[arg(short, long, default_value_t = 8080)]
        port: u16,
    },
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match &cli.command {
        Commands::Bundle { flow_name } => bundle(&cli.config, false, flow_name.as_deref())?,
        Commands::Analyze => analyze(&cli.config)?,
        Commands::GenerateCompletions { shell } => generate_completions(shell)?,
        Commands::Serve { rebundle, port } => serve(&cli.config, *rebundle, *port).await?,
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let console_subscriber = tracing_subscriber::fmt::Subscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(console_subscriber).unwrap();
    run().await.map_err(|err| anyhow::anyhow!("Error: {}", err))
}
