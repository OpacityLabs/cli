# Opacity CLI

A command-line tool for bundling and analyzing Luau files using luau-lsp. This tool provides a convenient way to manage Luau modules across different platforms and flows, with powerful static analysis capabilities.

## Features

- **Bundling**: Bundle Luau files with configurable output formats
- **Rebundle Mode**: Serve bundled flows over HTTP with optional file rebundling
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

# Serve your Luau files, (--rebundle) will auto bundle new changes.
opacity-cli serve --rebundle
```

### Source map

Every bundle ends with one comment line that tells the SDK which source file each bundle line came from:

```lua
--@opacity-sourcemap [{"path":"src/amazon/auth.luau","start":12,"end":26},{"path":"src/amazon/flows/overview.luau","start":27,"end":40}]
```

- `start` and `end` are bundle line numbers, 1-based and inclusive.
- `end` is the bundle line that holds the last line of the file.
- `path` is relative to the directory the CLI runs in, uses `/` separators, and has no `./` prefix. A file outside that directory keeps its full path.
- Entries are sorted by `start` and do not overlap.
- Bundle lines before the first entry hold generated code (hoisted types and the module table) and belong to no file.
- The bundle keeps every source line on the same line number inside its range. Comments are removed and leave a blank line.

The SDK resolves a frame on bundle line `n` to `path` line `n - start + 1`.

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

### Mapper testing

To also have the `serve` command serve mappers, you need to create a `mappers.location` file in the root of the project with the following content:

```
<mappers-folder>
```

Where `<mappers-folder>` is the folder containing the mappers, the path has to be ABSOLUTE.

To get it, just navigate to the mappers folder and run `pwd`.

Every time you change a mapper, or the first time you want to serve the mappers, make sure you run the `cargo xtask aggregate && opacity-cli bundle` in the mappers folder.

If you wish to add/remove mappers, you have to shut down the cli http server, modify the opacity.toml file, and then run it again. Changes to the `opacity.toml` are not updated automatically, a server restart is required!