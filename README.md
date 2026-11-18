# UniFFIer

A CLI tool for bootstrapping and managing Rust FFI projects with [UniFFI](https://mozilla.github.io/uniffi-rs/). Generates cross-language bindings for Python, Go, JavaScript/WASM, and the web from a single Rust codebase.

## Installation

The quickest way to install is with the provided script, which handles installing `make` and system dependencies automatically:

```sh
./install.sh
```

### Manual Installation

#### Prerequisites

- [Rust toolchain](https://rustup.rs/)
- Git
- `make` — if not already installed:

  | Platform | Command |
  |----------|---------|
  | Debian/Ubuntu | `sudo apt-get install -y make` |
  | Fedora | `sudo dnf install -y make` |
  | RHEL/CentOS | `sudo yum install -y make` |
  | Arch | `sudo pacman -S make` |
  | Alpine | `sudo apk add make` |
  | macOS | `xcode-select --install` |

- Language-specific toolchains as needed:
  - **Python:** [uv](https://docs.astral.sh/uv/)
  - **Go:** Go toolchain
  - **JavaScript/WASM:** Node.js and npm

Install system dependencies (C compiler, OpenSSL headers):

```sh
sudo make deps
```

Configure the Rust toolchain:

```sh
make configure
```

Then install the CLI:

```sh
make install
```

### Adding to PATH

If `$HOME/.cargo/bin` is not already in your `PATH`, add it so the `ffi` command is available:

```sh
echo 'PATH=$HOME/.cargo/bin:$PATH' >> ~/.bashrc && . ~/.bashrc
```

> On macOS/zsh, use `~/.zshrc` instead of `~/.bashrc`.

## Quick Start

Create a new FFI project:

```sh
ffi new my-project
cd my-project
ffi build
ffi test
```

Or add FFI support to an existing Rust project:

```sh
cd existing-project
ffi migrate
```

## Commands

### `ffi new <path>`

Create a new FFI-ready Rust workspace from the template at the given path.

```sh
ffi new my-math-lib
ffi new my-math-lib --demo       # keep demo crates for reference
ffi new my-math-lib --template https://github.com/org/custom-template.git
```

| Flag | Description |
|------|-------------|
| `--demo` | Keep demo crates instead of stripping to a minimal skeleton |
| `--template <url>` | Use a custom template repository |

### `ffi init [path]`

Initialize an FFI workspace in an existing empty directory. Defaults to the current directory.

```sh
mkdir my-project && cd my-project
ffi init
```

| Flag | Description |
|------|-------------|
| `--demo` | Keep demo crates |
| `--template <url>` | Use a custom template repository |
| `-y, --yes` | Skip interactive prompts |

Fails if `Cargo.toml` already exists — use `ffi migrate` for existing projects.

### `ffi migrate [path]`

Add FFI structure to an existing Rust project. Supports both single-crate and workspace layouts.

```sh
cd my-existing-crate
ffi migrate
```

For a **single crate**, this:
- Moves sources into `crates/lib/<name>/`
- Creates a workspace `Cargo.toml` with dependency inheritance
- Generates FFI wrapper and types crates
- Copies build infrastructure (Makefile, scripts, templates, git hooks)

For a **workspace**, this:
- Discovers crates from workspace member patterns
- Interactively selects which crates to wrap with FFI
- Generates FFI crates for each selection

| Flag | Description |
|------|-------------|
| `--template <url>` | Use a custom template repository |
| `-y, --yes` | Skip interactive prompts |

### `ffi build`

Build language bindings by running the appropriate `make` targets.

```sh
ffi build              # build all bindings
ffi build --python     # build only Python bindings
ffi build --go --js    # build Go and JS bindings
```

| Flag | Description |
|------|-------------|
| `--python` | Build Python bindings |
| `--go` | Build Go bindings |
| `--js` | Build JS/WASM bindings |
| `--all` | Build all bindings (default) |
| `-a, --author, --repo-owner <owner>` | GitHub owner/org (inferred from git remote if not set) |
| `-n, --name, --repo-name <name>` | Repository name (defaults to directory name) |

### `ffi test`

Run binding smoke tests via `make` targets. Stamps example import paths to match your repository before running.

```sh
ffi test              # test all bindings
ffi test --python     # test only Python
ffi test --web        # test web demo
```

| Flag | Description |
|------|-------------|
| `--python` | Test Python bindings |
| `--go` | Test Go bindings |
| `--js` | Test JS/WASM bindings |
| `--web` | Test web demo |
| `--all` | Test all bindings (default) |
| `-a, --author, --repo-owner <owner>` | GitHub owner/org |
| `-n, --name, --repo-name <name>` | Repository name |

### `ffi track [paths...]`

Initialize and set up one or more uniffied-workflow repositories. Initializes a git repo in each directory if needed and runs `make setup`.

```sh
ffi track                           # set up current directory
ffi track ../project-a ../project-b # set up multiple repos
```

## How It Works

UniFFIer scaffolds projects from a [template repository](https://github.com/amd-cmaxwell/ffi-testing) that contains a working multi-crate Rust workspace with UniFFI bindings for Python, Go, and JavaScript/WASM.

### Templating

When creating a new project, UniFFIer:

1. **Clones** the template repo into a temporary directory
2. **Templatizes** file contents — replaces template name prefixes (`uniffi_math_demo`, `uniffi-math-demo`, etc.) with your project name across all text files
3. **Strips demo crates** (unless `--demo` is passed) — removes the example math crates and replaces the demo scripts with minimal skeletons
4. **Strips docs** — removes template-specific documentation, keeping only the architectural decision records and plans relevant to the workflow
5. **Initializes** a fresh git repository

### Repository Identity

`ffi build` and `ffi test` resolve your GitHub owner and repo name to stamp into Go module paths and npm package scopes. Identity is resolved from (in order):

1. CLI flags (`--repo-owner`, `--repo-name`)
2. Git remote URL (`upstream` preferred over `origin`)
3. Directory name (for repo name only)

### Generated Project Structure

```
my-project/
├── Cargo.toml              # workspace root
├── Makefile                 # build/test targets for all languages
├── crates/
│   ├── lib/
│   │   └── core/           # your Rust library code
│   └── ffi/
│       ├── core/           # UniFFI FFI wrapper
│       └── types/          # shared FFI types
├── examples/
│   ├── py-demo/            # Python demo (installable package)
│   ├── go-demo/            # Go demo
│   ├── js-demo/            # Node.js demo
│   └── web-demo/           # Browser demo (Vite)
├── bindings/               # generated binding output
├── scripts/                # build helper scripts
└── docs/                   # architecture docs and plans
```

## License

MIT
