use crate::Cli;
use anyhow::Result;
use clap::CommandFactory;

pub fn generate_completions(shell: &str) -> Result<()> {
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
