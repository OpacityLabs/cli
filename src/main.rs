mod config;

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use config::{Flow, Platform};
use darklua_core::rules::bundle::BundleRequireMode;
use darklua_core::rules::{
    InjectGlobalValue, RemoveCompoundAssignment, RemoveContinue, RemoveIfExpression, RemoveTypes,
    Rule,
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

    /// Generate completions for a given shell
    #[command(name = "completions")]
    GenerateCompletions {
        /// The shell to generate completions for
        shell: String,
    },
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

fn get_global_inject_rules(platform: &Platform, flow: &Flow) -> Vec<Box<dyn Rule>> {
    let mut rules: Vec<Box<dyn Rule>> = vec![
        Box::new(InjectGlobalValue::string("FLOW_NAME", flow.name.clone())),
        Box::new(InjectGlobalValue::string("FLOW_ALIAS", flow.alias.clone())),
        Box::new(InjectGlobalValue::string(
            "PLATFORM_NAME",
            platform.name.clone(),
        )),
        Box::new(InjectGlobalValue::string(
            "PLATFORM_DESCRIPTION",
            platform.description.clone(),
        )),
    ];

    if let Some(min_sdk_version) = &flow.min_sdk_version {
        rules.push(Box::new(InjectGlobalValue::string(
            "MIN_SDK_VERSION",
            min_sdk_version.clone(),
        )));
    }

    if let Some(retrieves) = &flow.retrieves {
        rules.push(Box::new(InjectGlobalValue::string(
            "RETRIEVES",
            retrieves.join(", "),
        )));
    }

    rules
}

fn bundle(config_path: &str) -> Result<()> {
    let config = config::Config::from_file(config_path)?;
    let resources = Resources::from_file_system();

    std::fs::create_dir_all(&config.settings.output_directory)?;

    for platform in &config.platforms {
        println!("Processing platform: {}", platform.name);

        for flow in &platform.flows {
            println!("Bundling {} ({})", flow.name, flow.alias);
            let input = PathBuf::from(&flow.path);

            let output = PathBuf::from(&config.settings.output_directory)
                .join(format!("{}.bundle.lua", flow.alias));

            let mut config = Configuration::empty();
            config = config.with_bundle_configuration(
                BundleConfiguration::new(BundleRequireMode::Path(Default::default()))
                    .with_modules_identifier("__BUNDLE_MODULES"),
            );

            let rules: Vec<Box<dyn Rule>> = vec![
                Box::new(RemoveContinue::default()),
                Box::new(RemoveCompoundAssignment::default()),
                Box::new(RemoveTypes::default()),
                Box::new(RemoveIfExpression::default()),
            ];
            let rules = rules
                .into_iter()
                .chain(get_global_inject_rules(platform, flow))
                .collect::<Vec<Box<dyn Rule>>>();

            for rule in rules {
                config = config.with_rule(rule);
            }

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

    let execution_dir = env::current_dir()?;

    let file_paths = config.get_flows_paths();

    std::process::Command::new("luau-analyze")
        .current_dir(execution_dir.clone())
        .args(&file_paths)
        .status()?;

    Ok(())
}

fn generate_completions(shell: &str) -> Result<()> {
    let mut app = Cli::command();

    match shell {
        "bash" => clap_complete::generate(
            clap_complete::shells::Bash,
            &mut app,
            "opacity-cli",
            &mut std::io::stdout(),
        ),
        "zsh" => clap_complete::generate(
            clap_complete::shells::Zsh,
            &mut app,
            "opacity-cli",
            &mut std::io::stdout(),
        ),
        "fish" => clap_complete::generate(
            clap_complete::shells::Fish,
            &mut app,
            "opacity-cli",
            &mut std::io::stdout(),
        ),
        "powershell" => clap_complete::generate(
            clap_complete::shells::PowerShell,
            &mut app,
            "opacity-cli",
            &mut std::io::stdout(),
        ),
        _ => anyhow::bail!("Unsupported shell: {}", shell),
    }
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Bundle => bundle(&cli.config)?,
        Commands::Analyze => analyze(&cli.config)?,
        Commands::GenerateCompletions { shell } => generate_completions(&shell)?,
    }

    Ok(())
}
