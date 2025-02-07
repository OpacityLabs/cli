# Opacity CLI

A command-line tool for bundling and analyzing Luau files. This tool provides a convenient way to manage Luau modules across different platforms and flows.

## Features

- **Bundling**: Bundle Luau files using darklua with configurable output formats
- **Analysis**: Run luau-analyze on your Luau files with proper module resolution
- **Platform Organization**: Organize your Luau modules by platform and flows
- **Configurable**: Easy-to-use TOML configuration

## Prerequisites

Before using this tool, ensure you have the following installed:

- [Rust and Cargo](https://rustup.rs/)
- [luau-analyze](https://github.com/Roblox/luau) - For analyzing Luau files
- [darklua](https://github.com/seaofvoices/darklua) - For bundling files (`cargo install darklua`)

## Installation

```bash
# Clone the repository
git clone <repository-url>

# Install the CLI
cargo install --path .
```

## Usage

### Configuration

Create an `opacity.toml` file in your project root:

```toml
[settings]
output_directory = "bundled"

[[platforms]]
name = "Gusto"
description = "Gusto payroll and HR platform integration"

[[platforms.flows]]
name = "MyPay"
alias = "gusto:my_pay"
description = "Access and manage your pay information"
minSdkVersion = "1"
retrieves = ["pay information", "pay history"]
path = "src/gusto/my_pay.luau"

# Add more platforms and flows as needed
```

### Commands

#### Bundle Files
```bash
# Bundle all files using default config
opacity-cli bundle

# Use a specific config file
opacity-cli -c path/to/config.toml bundle
```

The bundled files will be created in the output directory specified in your config:
```
bundled/
  gusto_my_pay.bundled.lua
  gusto_pay_period_picker_data.bundled.lua
  # etc...
```

#### Analyze Files
```bash
# Analyze all files in the config
opacity-cli analyze

# Use a specific config file
opacity-cli -c path/to/config.toml analyze
```

The analyzer will:
- Run in the correct directory context for each file
- Maintain proper module resolution
- Show clear success/failure indicators
- Provide detailed error messages when needed

## Configuration Reference

### Settings Section
```toml
[settings]
output_directory = "bundled"  # Directory where bundled files will be stored
```

### Platform Section
```toml
[[platforms]]
name = "PlatformName"        # Name of the platform
description = "Description"  # Platform description

[[platforms.flows]]
name = "FlowName"           # Name of the flow
alias = "platform:flow"     # Unique identifier for the flow
description = "Description" # Flow description
minSdkVersion = "1"        # Minimum SDK version required
retrieves = ["data1", "data2"] # List of data types this flow retrieves
path = "src/path/to/file.luau" # Path to the Luau file
```

## Output Structure

Bundled files are organized as follows:
- Each file is bundled into `{output_directory}/{alias}.bundled.lua`
- The original module structure is preserved
- Files are named using their aliases for easy identification

## Error Handling

The tool provides clear error messages for common issues:
- Missing files or directories
- Analysis failures
- Bundling errors
- Configuration problems

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

