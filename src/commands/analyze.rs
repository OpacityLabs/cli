use crate::config;
use anyhow::{Context, Result};
use which::which;

use std::env;

fn check_luau_lsp() -> Result<()> {
    which("luau-lsp").context("luau-lsp is not installed. Please install it first (https://github.com/JohnnyMorganz/luau-lsp/releases)")?;
    Ok(())
}


pub fn analyze(config_path: &str) -> Result<()> {
    check_luau_lsp()?;
    let config = config::Config::from_file(config_path)?;

    let execution_dir = env::current_dir()?;

    let definition_files = config.settings.definition_files.clone().unwrap_or_default();
    let definition_files_args = definition_files
        .iter()
        .flat_map(|file| ["--definitions".to_string(), file.to_string()])
        .collect::<Vec<String>>();

    let file_paths = config.get_flows_paths();

    let status = std::process::Command::new("luau-lsp")
        .stdout(std::process::Stdio::inherit())
        .current_dir(execution_dir.clone())
        .arg("analyze")
        .args(&definition_files_args)
        .args(&file_paths)
        .status()?;

    if status.code().unwrap_or(-1) != 0 {
        anyhow::bail!("luau-lsp analysis failed");
    }

    Ok(())
}