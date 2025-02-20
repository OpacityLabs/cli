mod config;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use darklua_core::rules::bundle::BundleRequireMode;
use darklua_core::rules::{
    RemoveCompoundAssignment, RemoveContinue, RemoveIfExpression, RemoveTypes, Rule,
};
use darklua_core::{
    process, BundleConfiguration, Configuration, GeneratorParameters, Options, Resources,
};
use std::env;
use std::path::PathBuf;
use std::time::Instant;
use which::which;

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
}

fn check_luau_analyze() -> Result<()> {
    which("luau-analyze").context("luau-analyze is not installed. Please install it first")?;
    Ok(())
}

fn process_bundle(resources: &Resources, options: Options) -> Result<()> {
    let process_start = Instant::now();
    let result =
        process(resources, options).map_err(|e| anyhow::anyhow!("Processing failed: {:?}", e))?;

    match result.result() {
        Ok(_) => {
            println!("Successfully processed in {:?}", process_start.elapsed());
            Ok(())
        }
        Err(err) => {
            anyhow::bail!("Failed to process: {:?}", err);
        }
    }
}

fn bundle(config_path: &str) -> Result<()> {
    let config = config::Config::from_file(config_path)?;
    let resources = Resources::from_file_system();

    std::fs::create_dir_all(&config.settings.output_directory)?;

    for platform in config.platforms {
        println!("Processing platform: {}", platform.name);

        for flow in platform.flows {
            println!("Bundling {} ({})", flow.name, flow.alias);
            let input = PathBuf::from(&flow.path);

            let output = PathBuf::from(&config.settings.output_directory)
                .join(format!("{}.bundle.lua", flow.alias));

            let mut config = Configuration::empty();
            config = config.with_bundle_configuration(
                BundleConfiguration::new(BundleRequireMode::Path(Default::default()))
                    .with_modules_identifier("__BUNDLE_MODULES"),
            );

            let remove_continue: Box<dyn Rule> = Box::new(RemoveContinue::default());
            config = config.with_rule(remove_continue);

            let remove_compound_assignment: Box<dyn Rule> =
                Box::new(RemoveCompoundAssignment::default());
            config = config.with_rule(remove_compound_assignment);

            let remove_types: Box<dyn Rule> = Box::new(RemoveTypes::default());
            config = config.with_rule(remove_types);

            let remove_if_expression: Box<dyn Rule> = Box::new(RemoveIfExpression::default());
            config = config.with_rule(remove_if_expression);

            let options = Options::new(&input)
                .with_output(&output)
                .with_generator_override(GeneratorParameters::RetainLines)
                .with_configuration(config);

            process_bundle(&resources, options)?;
        }
    }

    Ok(())
}

fn analyze(config_path: &str) -> Result<()> {
    check_luau_analyze()?;
    let config = config::Config::from_file(config_path)?;
    let mut had_errors = false;

    let execution_dir = env::current_dir()?;

    let file_paths = config.get_flows_paths();

    let status = std::process::Command::new("luau-analyze")
        .current_dir(execution_dir.clone())
        .args(&file_paths)
        .status()?;

    if status.success() {
        println!("✓ OK");
    } else {
        println!("✗ Failed");
        had_errors = true;
    }

    if had_errors {
        anyhow::bail!("Some files failed analysis");
    }

    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Bundle => bundle(&cli.config)?,
        Commands::Analyze => analyze(&cli.config)?,
    }

    Ok(())
}
