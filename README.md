# Opacity CLI

A command-line tool for bundling and analyzing Luau files. This tool provides a convenient way to manage Luau modules across different platforms and flows.

## Features

- **Bundling**: Bundle Luau files with configurable output formats
- **Analysis**: Analyze Luau files with luau-analyze
- **Generate Completions**: Generate completions for the CLI
- **Platform Organization**: Organize your Luau modules by platform and flows
- **Configurable**: Easy-to-use TOML configuration
- **Auto bundle and serve**: Bundles and serves Luau files on save



### Installation

```bash
brew install luau
cargo install --git https://github.com/OpacityLabs/cli
```
 

### Usage

```bash
opacity-cli bundle --config config.toml
```

```bash
opacity-cli analyze --config config.toml
```

```bash
opacity-cli watch --config config.toml
```



### Generating Completions

To generate completions for the CLI, run the following command:

```bash
opacity-cli completions <shell> > <output-file>
```

Where `<shell>` is one of `bash`, `zsh`, `fish`, or `powershell` and `<output-file>` is the file to save the completions to.

Example:

```bash
opacity-cli completions zsh > ~/.oh-my-zsh/completions/_opacity-cli
```