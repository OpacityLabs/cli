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
use tracing::{warn, Level};

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
    Bundle,

    /// Analyze all Luau files
    Analyze,

    /// Generate completions for a given shell
    #[command(name = "completions")]
    GenerateCompletions {
        /// The shell to generate completions for
        shell: String,
    },

    /// Serve Lua flows over HTTP, rebundling the requested flow on each request
    Serve {
        /// Deprecated: rebundling is on by default, this flag is no longer needed
        #[arg(short, long, conflicts_with = "no_rebundle")]
        rebundle: bool,

        /// Serve the already bundled flows without rebundling them
        #[arg(short, long)]
        no_rebundle: bool,

        /// Port to serve on
        #[arg(short, long, default_value_t = 8080)]
        port: u16,
    },
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match &cli.command {
        Commands::Bundle => bundle(&cli.config, false)?,
        Commands::Analyze => analyze(&cli.config)?,
        Commands::GenerateCompletions { shell } => generate_completions(shell)?,
        Commands::Serve {
            rebundle,
            no_rebundle,
            port,
        } => {
            if *rebundle {
                warn!("--rebundle is deprecated and no longer needed: rebundling is enabled by default. Use --no-rebundle to turn it off.");
            }
            serve(&cli.config, !*no_rebundle, *port).await?
        }
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
