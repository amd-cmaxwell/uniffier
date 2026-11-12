# Plan: `ffi` CLI Tool

## Context

We're building a Rust CLI tool called `ffi` that bootstraps new Rust projects with FFI support and migrates existing ones. It uses the [ffi-testing](https://github.com/amd-cmaxwell/ffi-testing) repo as a template — that repo is a Cargo workspace with `crates/lib/*` (domain logic) and `crates/ffi/*` (UniFFI wrappers), plus a Makefile that generates Python, Go, and JS/WASM bindings.

The CLI should feel like `uv` or `cargo` — fast, opinionated, with interactive prompts via `dialoguer`.

## Subcommands

| Command | Description |
|---------|-------------|
| `ffi init <name>` | Scaffold a new FFI-ready Rust workspace from the template |
| `ffi migrate` | Add FFI structure to an existing Rust crate or workspace |
| `ffi build` | Build bindings (delegates to `make generate-*`) |
| `ffi test` | Run binding smoke tests (delegates to `make test-*`) |

## Project Structure

```
Cargo.toml          # single crate: uniffier
src/
  main.rs           # entrypoint, clap app
  commands/
    mod.rs
    init.rs         # ffi init
    migrate.rs      # ffi migrate
    build.rs        # ffi build
    test.rs         # ffi test
  template.rs       # git clone + templatize logic
  workspace.rs      # Cargo workspace detection/manipulation
```

## Dependencies

- `clap` (derive) — CLI parsing
- `dialoguer` — interactive prompts (project name, description, select languages)
- `console` — terminal styling (used by dialoguer)
- `git2` — clone template repo without shelling out to git
- `toml_edit` — modify Cargo.toml files preserving formatting
- `anyhow` — error handling

## Command Details

### `ffi init [name]`

1. Prompt for project name if not provided as argument
2. Prompt for project description
3. Clone `amd-cmaxwell/ffi-testing` (or a configurable template URL) into `<name>/`
4. Remove `.git/` directory from clone
5. Templatize: replace `uniffi-testing` with the user's project name in:
   - `Cargo.toml` (workspace members, package names, dependencies)
   - All `crates/*/Cargo.toml` files
   - `uniffi.toml`
   - `Makefile` (no changes needed — it auto-derives from git remote)
   - `.github/workflows/*.yml`
6. Strip the example domain crates (`complex`, `polynomials`, `linear-algebra`, `trig`, `sequences`), keeping only `core` + `types` as starter crates
7. Initialize a fresh git repo
8. Print next-steps instructions

### `ffi migrate`

Must be run from inside an existing Rust project. Two modes:

**Single crate:** Detected by `Cargo.toml` with `[package]` but no `[workspace]`.
1. Convert to a workspace: move existing crate to `crates/lib/<name>/`
2. Create workspace `Cargo.toml` at root
3. Create `crates/ffi/<name>/` with UniFFI wrapper crate
4. Create `crates/ffi/types/` for shared FFI types
5. Create `crates/ffi/tools/` for bindgen binaries
6. Copy Makefile, scripts, templates, .githooks from template
7. Create `uniffi.toml`

**Workspace:** Detected by `[workspace]` in `Cargo.toml`.
1. Prompt user to select which crates to add FFI wrappers for
2. Create `crates/ffi/` structure alongside existing crates
3. Add FFI crates to workspace members
4. Copy Makefile, scripts, templates, .githooks from template

### `ffi build`

Flags: `--python`, `--go`, `--js`, `--all` (default: `--all`)

Delegates to `make generate-python`, `make generate-go`, `make generate-js`, or `make generate-all`.

### `ffi test`

Flags: `--python`, `--go`, `--js`, `--web`, `--all` (default: `--all`)

Delegates to `make test-python`, `make test-go`, etc.

## Implementation Order

1. **Scaffold the Rust project** — `cargo init --name uniffier`, set up Cargo.toml with dependencies, create module structure
2. **Implement CLI skeleton** — clap derive structs for all subcommands
3. **Implement `ffi init`** — clone, templatize, strip examples
4. **Implement `ffi build`** — make delegation
5. **Implement `ffi test`** — make delegation
6. **Implement `ffi migrate`** — workspace detection + restructuring (most complex)

## Binary Name

The `Cargo.toml` will use `[[bin]] name = "ffi"` so the installed binary is `ffi`, while the package name remains `uniffier`.

## Verification

1. `cargo build` — compiles the CLI
2. `ffi init test-project` — creates a scaffolded project
3. `cd test-project` — enter the scaffolded project
4. `ffi build --all` (from inside the scaffolded project) — generates bindings
5. `ffi test --all` — runs smoke tests
