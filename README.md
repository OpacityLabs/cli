# Opacity CLI

A command-line tool for bundling and analyzing Luau files using luau-lsp. This tool provides a convenient way to manage Luau modules across different platforms and flows, with powerful static analysis capabilities.

## Features

- **Bundling**: Bundle Luau files with configurable output formats
- **Static Analysis**: Analyze Luau files using luau-lsp for type checking, linting, and code quality
- **Generate Completions**: Generate completions for the CLI
- **Platform Organization**: Organize your Luau modules by platform and flows
- **Configurable**: Easy-to-use TOML configuration

### Installation

First, install luau-lsp (and make sure you have it in the PATH):

`https://github.com/JohnnyMorganz/luau-lsp/releases`

Then, install the Opacity CLI:

```bash
# Install Opacity CLI
cargo install --git https://github.com/OpacityLabs/cli
```

### Usage

```bash
# Bundle your Luau files
opacity-cli bundle --config config.toml

# Analyze your Luau files with luau-lsp
opacity-cli analyze --config config.toml
```

### Analysis Features

The analyze command uses luau-lsp to provide:
- Type checking
- Linting
- Code quality analysis
- Error detection
- Best practices validation

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