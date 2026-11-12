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
use commands::serve::{parse_public_url, serve};
use url::Url;

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
    Bundle,

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

        /// Base URL the SDK and the notary download mappers from [default: the address the SDK used to fetch the flow]
        #[arg(long, value_parser = parse_public_url)]
        public_url: Option<Url>,
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
            port,
            public_url,
        } => serve(&cli.config, *rebundle, *port, public_url.clone()).await?,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serve_takes_a_public_url() {
        let cli = Cli::try_parse_from([
            "opacity-cli",
            "serve",
            "--public-url",
            "http://192.168.1.10:8080/",
        ])
        .unwrap();

        let Commands::Serve { public_url, .. } = cli.command else {
            panic!("expected the serve command");
        };
        assert_eq!(
            public_url.as_ref().map(Url::as_str),
            Some("http://192.168.1.10:8080/")
        );
    }

    #[test]
    fn serve_refuses_a_public_url_without_a_scheme() {
        let parsed =
            Cli::try_parse_from(["opacity-cli", "serve", "--public-url", "192.168.1.10:8080"]);
        assert!(parsed.is_err());
    }
}
