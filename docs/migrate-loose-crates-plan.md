# Plan: Handle loose crates at project root in `ffi migrate`

## Context

Currently `ffi migrate` handles two project structures: a single crate at root (`SingleCrate`) and an existing Cargo workspace (`Workspace`). A third common layout exists: multiple standalone crates sitting at the project root without a workspace Cargo.toml (or a root crate alongside subdirectory crates). The migrate command should detect all crates, let the user choose which get FFI wrappers (in interactive mode), and restructure them into a proper workspace.

## Behavior

1. **Discovery**: Scan the project root for crates — the root itself (if it has `[package]` in Cargo.toml) plus any subdirectories containing Cargo.toml
2. **Interactive selection**: Present discovered crates to the user; they select which ones get FFI wrappers
3. **Restructuring**:
   - Selected FFI crates → `crates/lib/<name>/`
   - Non-selected crates → `crates/<name>/`
   - Root crate (if present) gets its files moved out of root; subdirectory crates get renamed/moved
4. **Noninteractive** (`-y`): all crates get FFI wrappers by default

## Changes

### 1. `src/workspace.rs` — add `LooseCrates` variant

Add a new `ProjectKind` variant:
```rust
LooseCrates { root_crate: Option<(String, String)>, sub_crates: Vec<(String, String)> }
```

In `detect_project`:
- After checking for `[workspace]` and `[package]`, scan subdirectories for Cargo.tomls
- If root is a `[package]` AND subdirectory crates exist → return `LooseCrates` with root + subs
- If no root package but subdirectory crates exist → return `LooseCrates` with `root_crate: None`
- If root is a `[package]` with NO subdirectory crates → keep returning `SingleCrate` (unchanged)

Helper: `discover_root_crates(dir) -> Vec<(name, version)>` — reads each subdir's Cargo.toml for package name/version.

### 2. `src/commands/migrate.rs` — add `migrate_loose_crates` handler

New match arm in `run()` for `ProjectKind::LooseCrates`.

`migrate_loose_crates(project_dir, root_crate, sub_crates, args)`:
1. Collect all crate names into a single list (root + subs)
2. Interactive: `dialoguer::MultiSelect` — user picks which crates get FFIs (default: all selected)
3. Noninteractive (`-y`): all crates get FFIs
4. For the root crate (if present): reuse `move_crate_contents` to move `src/`, `tests/`, etc. into destination
5. For subdirectory crates: `fs::rename` the subdir into `crates/lib/<name>/` or `crates/<name>/`
6. Rewrite each moved crate's Cargo.toml for workspace inheritance (reuse `rewrite_crate_toml_for_workspace`)
7. Write workspace Cargo.toml with all crates as members (adapt `write_workspace_cargo_toml` to handle multiple crates)
8. Create FFI wrapper crates for selected crates (reuse `create_ffi_crate`)
9. Create shared FFI types crate (reuse `create_ffi_types_crate`)
10. Copy template assets (reuse `clone_template_assets`)

### 3. `src/commands/migrate.rs` — generalize `write_workspace_cargo_toml`

Currently takes a single `crate_name`/`crate_version`. Generalize to accept a list of `(name, version, dest_prefix)` tuples so it can write workspace members for both `crates/lib/*` and `crates/*`, and workspace dependencies for all crates.

## Reusable functions (no changes needed)
- `move_crate_contents` — moves root crate files into a target dir
- `rewrite_crate_toml_for_workspace` — rewrites a crate's Cargo.toml for workspace inheritance
- `create_ffi_crate`, `create_ffi_types_crate` — create FFI scaffolding
- `clone_template_assets` — copy template infrastructure

## Verification
- `cargo build` compiles cleanly
- `ffi migrate --help` shows no new flags (behavior is auto-detected)
- Test with a directory containing a root crate + subdirectory crates
