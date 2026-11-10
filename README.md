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

### Local proofs

`serve` is the backend for the SDK's Local environment and for a local notary.

- `/v3/flows` gives each flow the session action id `flow:<alias>`.
- `/verify` tells the notary which flow a session action belongs to: its alias, its mapper and its `allowed_hosts`.
- A flow's mapper is the bundled mapper with the same alias in `mapping/`, as in production.
- The SDK and the notary download the mapper from `<base URL>/mapper/<hash>`. The base URL is the address the SDK used to fetch the flow, so the notary gets the same URL even when it reaches the server at another address.
- `--public-url <URL>` sets the base URL for every flow, for example `opacity-cli serve --public-url http://192.168.1.10:8080`.
- A flow can only reach the hosts in its `allowed_hosts` in `opacity.toml`. A flow without `allowed_hosts` can reach any host.

### Mapper testing

The `serve` command automatically serves mappers from `mapping/` in the current working directory. A `mappers.location` file is no longer used. If there is no `mapping/` directory, the server serves flows only.

Every time you change a mapper, or the first time you want to serve the mappers, make sure you run the `cargo xtask aggregate && opacity-cli bundle` in the mappers folder.

If you wish to add/remove mappers, you have to shut down the cli http server, modify the opacity.toml file, and then run it again. Changes to the `opacity.toml` are not updated automatically, a server restart is required!