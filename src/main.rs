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

    /// Serve Lua flows over HTTP, rebundling each flow when the SDK fetches it
    Serve {
        /// Deprecated: has no effect, rebundling is the default
        #[arg(short, long, hide = true, conflicts_with = "no_rebundle")]
        rebundle: bool,

        /// Serve the already bundled flows without rebundling them
        #[arg(long)]
        no_rebundle: bool,

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
            no_rebundle,
        } => {
            if *rebundle {
                warn!("--rebundle is deprecated and has no effect: rebundling is the default. Use --no-rebundle to turn it off.");
            }
            serve(&cli.config, !*no_rebundle, *port, public_url.clone()).await?
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

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

    /// The `rebundle` and `no_rebundle` flags `serve` parses from these arguments
    fn serve_flags(args: &[&str]) -> Result<(bool, bool), clap::error::ErrorKind> {
        let cli = Cli::try_parse_from(["opacity-cli", "serve"].iter().chain(args))
            .map_err(|error| error.kind())?;
        let Commands::Serve {
            rebundle,
            no_rebundle,
            ..
        } = cli.command
        else {
            panic!("expected the serve command");
        };
        Ok((rebundle, no_rebundle))
    }

    #[test]
    fn serve_rebundles_unless_told_not_to() {
        assert_eq!(serve_flags(&[]), Ok((false, false)));
        assert_eq!(serve_flags(&["--no-rebundle"]), Ok((false, true)));
    }

    #[test]
    fn serve_still_takes_the_deprecated_rebundle_flag() {
        assert_eq!(serve_flags(&["--rebundle"]), Ok((true, false)));
        assert_eq!(serve_flags(&["-r"]), Ok((true, false)));
    }

    #[test]
    fn serve_refuses_rebundle_together_with_no_rebundle() {
        for args in [["--rebundle", "--no-rebundle"], ["-r", "--no-rebundle"]] {
            assert_eq!(
                serve_flags(&args),
                Err(clap::error::ErrorKind::ArgumentConflict),
                "{args:?}"
            );
        }
    }

    #[test]
    fn no_rebundle_has_no_short_flag() {
        assert_eq!(
            serve_flags(&["-n"]),
            Err(clap::error::ErrorKind::UnknownArgument)
        );
    }

    #[test]
    fn serve_help_lists_no_rebundle_but_not_the_deprecated_flag() {
        let help = Cli::command()
            .find_subcommand_mut("serve")
            .expect("serve is a subcommand")
            .render_help()
            .to_string();
        let flags: Vec<&str> = help
            .split_whitespace()
            .map(|word| word.trim_end_matches(','))
            .collect();

        assert!(flags.contains(&"--no-rebundle"), "{help}");
        assert!(!flags.contains(&"--rebundle"), "{help}");
        assert!(!flags.contains(&"-r"), "{help}");
    }
}
