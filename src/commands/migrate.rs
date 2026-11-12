use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::Args;
use console::style;

use crate::template;
use crate::workspace::{self, ProjectKind};

#[derive(Args)]
pub struct MigrateArgs {
    /// Template repository URL
    #[arg(long)]
    pub template: Option<String>,

    /// Skip interactive prompts, use defaults
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Project directory (defaults to current directory)
    #[arg(default_value = ".")]
    pub path: PathBuf,
}

pub fn run(args: MigrateArgs) -> Result<()> {
    let project_dir = args.path.canonicalize().unwrap_or(args.path.clone());

    if project_dir.join("crates/ffi").exists() {
        bail!("project already has an FFI structure (crates/ffi/ exists)");
    }

    let project_kind = workspace::detect_project(&project_dir)?
        .ok_or_else(|| anyhow::anyhow!("no Cargo.toml found in {}", project_dir.display()))?;

    match project_kind {
        ProjectKind::SingleCrate { name, version } => {
            migrate_single_crate(&project_dir, &name, &version, &args)?;
        }
        ProjectKind::Workspace { members } => {
            migrate_workspace(&project_dir, &members, &args)?;
        }
    }

    Ok(())
}

fn migrate_single_crate(
    project_dir: &Path,
    crate_name: &str,
    crate_version: &str,
    args: &MigrateArgs,
) -> Result<()> {
    eprintln!(
        "{} Migrating single crate '{}' to FFI workspace...",
        style("●").cyan(),
        style(crate_name).bold()
    );

    if !args.yes && std::io::stdin().is_terminal() {
        let proceed = dialoguer::Confirm::new()
            .with_prompt(format!(
                "This will restructure '{crate_name}' into a workspace. Continue?"
            ))
            .default(true)
            .interact()?;
        if !proceed {
            eprintln!("Aborted.");
            return Ok(());
        }
    }

    let lib_dir = project_dir.join("crates/lib").join(crate_name);
    fs::create_dir_all(lib_dir.join("src"))
        .context("failed to create crates/lib/ directory")?;

    eprintln!("  Moving crate into crates/lib/{crate_name}/...");
    move_crate_contents(project_dir, &lib_dir)?;

    eprintln!("  Rewriting crate Cargo.toml for workspace...");
    rewrite_crate_toml_for_workspace(&lib_dir)?;

    eprintln!("  Writing workspace Cargo.toml...");
    write_workspace_cargo_toml(project_dir, crate_name, crate_version)?;

    eprintln!("  Creating FFI wrapper crate...");
    create_ffi_crate(project_dir, crate_name)?;

    eprintln!("  Creating shared FFI types crate...");
    create_ffi_types_crate(project_dir)?;

    eprintln!("  Copying template assets (Makefile, scripts, templates, tools)...");
    clone_template_assets(project_dir, args)?;

    eprintln!("{} Migration complete.", style("✓").green());
    eprintln!();
    eprintln!("  Project structure:");
    eprintln!("    crates/lib/{crate_name}/   ← your original crate");
    eprintln!("    crates/ffi/{crate_name}/   ← UniFFI wrapper (edit this)");
    eprintln!("    crates/ffi/types/          ← shared FFI types");
    eprintln!("    crates/ffi/tools/          ← bindgen tool binaries");
    eprintln!();
    eprintln!("  Next steps:");
    eprintln!("    # Add FFI exports in crates/ffi/{crate_name}/src/lib.rs");
    eprintln!("    ffi build --all");

    Ok(())
}

/// Move all crate source files from project root into the lib crate directory.
/// Preserves: src/, tests/, benches/, examples/, build.rs, and the crate Cargo.toml.
/// Leaves behind: Cargo.lock, .git/, .gitignore, and non-crate files.
fn move_crate_contents(project_dir: &Path, lib_dir: &Path) -> Result<()> {
    let dirs_to_move = ["src", "tests", "benches", "examples"];
    for dir_name in &dirs_to_move {
        let src = project_dir.join(dir_name);
        let dest = lib_dir.join(dir_name);
        if src.exists() {
            fs::rename(&src, &dest)
                .with_context(|| format!("failed to move {dir_name}/"))?;
        }
    }

    let files_to_move = ["build.rs"];
    for file_name in &files_to_move {
        let src = project_dir.join(file_name);
        let dest = lib_dir.join(file_name);
        if src.exists() {
            fs::rename(&src, &dest)
                .with_context(|| format!("failed to move {file_name}"))?;
        }
    }

    let original_toml = project_dir.join("Cargo.toml");
    let dest_toml = lib_dir.join("Cargo.toml");
    fs::copy(&original_toml, &dest_toml).context("failed to copy Cargo.toml")?;

    Ok(())
}

/// Rewrite the moved crate's Cargo.toml to use workspace inheritance for common fields.
fn rewrite_crate_toml_for_workspace(lib_dir: &Path) -> Result<()> {
    let toml_path = lib_dir.join("Cargo.toml");
    let content = fs::read_to_string(&toml_path)?;
    let mut doc = content.parse::<toml_edit::DocumentMut>()?;

    if let Some(pkg) = doc.get_mut("package").and_then(|p| p.as_table_like_mut()) {
        let inherit_fields = ["edition", "license", "repository", "rust-version"];
        for field in &inherit_fields {
            if pkg.contains_key(field) {
                pkg.remove(field);
                let mut inline = toml_edit::InlineTable::new();
                inline.insert("workspace", toml_edit::Value::from(true));
                pkg.insert(field, toml_edit::Item::Value(toml_edit::Value::InlineTable(inline)));
            }
        }
    }

    fs::write(&toml_path, doc.to_string())?;
    Ok(())
}

fn migrate_workspace(
    project_dir: &Path,
    members: &[String],
    args: &MigrateArgs,
) -> Result<()> {
    eprintln!(
        "{} Migrating workspace to add FFI support...",
        style("●").cyan()
    );

    let existing_crates = discover_workspace_crates(project_dir, members)?;

    if existing_crates.is_empty() {
        bail!("no crates found in workspace to migrate");
    }

    let crates_to_wrap = if !args.yes
        && std::io::stdin().is_terminal()
        && existing_crates.len() > 1
    {
        let selections = dialoguer::MultiSelect::new()
            .with_prompt("Select crates to add FFI wrappers for")
            .items(&existing_crates)
            .defaults(&vec![true; existing_crates.len()])
            .interact()?;

        selections
            .into_iter()
            .map(|i| existing_crates[i].clone())
            .collect::<Vec<_>>()
    } else {
        existing_crates
    };

    if crates_to_wrap.is_empty() {
        bail!("no crates selected for FFI migration");
    }

    if !args.yes && std::io::stdin().is_terminal() {
        let list = crates_to_wrap.join(", ");
        let proceed = dialoguer::Confirm::new()
            .with_prompt(format!("Add FFI wrappers for: {list}. Continue?"))
            .default(true)
            .interact()?;
        if !proceed {
            eprintln!("Aborted.");
            return Ok(());
        }
    }

    for crate_name in &crates_to_wrap {
        eprintln!("  Creating FFI wrapper for {crate_name}...");
        create_ffi_crate(project_dir, crate_name)?;
    }

    eprintln!("  Creating shared FFI types crate...");
    create_ffi_types_crate(project_dir)?;

    eprintln!("  Copying template assets...");
    clone_template_assets(project_dir, args)?;

    eprintln!("  Updating workspace Cargo.toml...");
    add_ffi_to_workspace_toml(project_dir, &crates_to_wrap)?;

    eprintln!("{} Migration complete.", style("✓").green());
    eprintln!();
    eprintln!("  Next steps:");
    eprintln!("    # Add FFI exports in crates/ffi/*/src/lib.rs");
    eprintln!("    ffi build --all");

    Ok(())
}

/// Discover crate names from workspace member patterns.
/// Handles both glob patterns like "crates/*" and explicit paths like "my-crate".
fn discover_workspace_crates(project_dir: &Path, members: &[String]) -> Result<Vec<String>> {
    let mut crates = Vec::new();

    for member in members {
        if member.ends_with('*') {
            let base = member.trim_end_matches('*').trim_end_matches('/');
            let base_dir = project_dir.join(base);
            if base_dir.is_dir() {
                for entry in fs::read_dir(&base_dir)? {
                    let entry = entry?;
                    if entry.path().join("Cargo.toml").exists() {
                        if let Some(name) = entry.file_name().to_str() {
                            crates.push(name.to_string());
                        }
                    }
                }
            }
        } else {
            let member_dir = project_dir.join(member);
            if member_dir.join("Cargo.toml").exists() {
                let name = member_dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(member)
                    .to_string();
                crates.push(name);
            }
        }
    }

    crates.sort();
    crates.dedup();
    Ok(crates)
}

fn clone_template_assets(project_dir: &Path, args: &MigrateArgs) -> Result<()> {
    let temp_dir = tempfile::tempdir().context("failed to create temp directory")?;
    let temp_path = temp_dir.path();

    template::clone_template(temp_path, args.template.as_deref())?;

    let assets = ["Makefile", "scripts", "templates", ".githooks"];

    for asset in &assets {
        let src = temp_path.join(asset);
        let dest = project_dir.join(asset);

        if src.is_dir() {
            copy_dir_recursive(&src, &dest)?;
        } else if src.is_file() && !dest.exists() {
            fs::copy(&src, &dest).with_context(|| format!("failed to copy {asset}"))?;
        }
    }

    let tools_src = temp_path.join("crates/ffi/tools");
    let tools_dest = project_dir.join("crates/ffi/tools");
    if tools_src.exists() && !tools_dest.exists() {
        copy_dir_recursive(&tools_src, &tools_dest)?;
    }

    if !project_dir.join("uniffi.toml").exists() {
        let uniffi_src = temp_path.join("uniffi.toml");
        if uniffi_src.exists() {
            fs::copy(&uniffi_src, project_dir.join("uniffi.toml"))
                .context("failed to copy uniffi.toml")?;
        }
    }

    let bindings_dir = project_dir.join("bindings");
    if !bindings_dir.exists() {
        fs::create_dir_all(&bindings_dir)?;
        let readme_src = temp_path.join("bindings/README.md");
        if readme_src.exists() {
            fs::copy(&readme_src, bindings_dir.join("README.md"))?;
        }
    }

    Ok(())
}

fn create_ffi_crate(project_dir: &Path, crate_name: &str) -> Result<()> {
    let ffi_dir = project_dir.join("crates/ffi").join(crate_name);
    if ffi_dir.exists() {
        return Ok(());
    }

    let ffi_src_dir = ffi_dir.join("src");
    fs::create_dir_all(&ffi_src_dir)?;

    let crate_name_underscore = crate_name.replace('-', "_");

    let cargo_toml = format!(
        r#"[package]
name = "{crate_name}-ffi"
version = "0.1.0"
edition.workspace = true

[lib]
crate-type = ["cdylib", "staticlib", "lib"]
name = "{crate_name_underscore}_ffi"

[features]
wasm = ["uniffi/wasm-unstable-single-threaded", "uniffi/scaffolding-ffi-buffer-fns", "dep:uniffi-runtime-wasm"]

[dependencies]
uniffi = {{ version = "0.31" }}
uniffi-runtime-wasm = {{ version = "0.31.0-6", optional = true }}
{crate_name} = {{ path = "../../lib/{crate_name}" }}
"#
    );

    fs::write(ffi_dir.join("Cargo.toml"), cargo_toml)?;

    let lib_rs = format!(
        r#"#[cfg(target_arch = "wasm32")]
extern crate uniffi_runtime_wasm as _;

uniffi::setup_scaffolding!();

// TODO: Add FFI exports for {crate_name}
// Example:
// #[uniffi::export]
// pub fn my_function() -> String {{
//     {crate_name_underscore}::my_function()
// }}
"#
    );

    fs::write(ffi_src_dir.join("lib.rs"), lib_rs)?;

    let uniffi_toml = r#"[bindings.go]
go_mod = "github.com/OWNER/REPO/bindings/go"
"#;
    fs::write(ffi_dir.join("uniffi.toml"), uniffi_toml)?;

    Ok(())
}

fn create_ffi_types_crate(project_dir: &Path) -> Result<()> {
    let types_dir = project_dir.join("crates/ffi/types");
    if types_dir.exists() {
        return Ok(());
    }

    let types_src_dir = types_dir.join("src");
    fs::create_dir_all(&types_src_dir)?;

    let cargo_toml = r#"[package]
name = "ffi-types"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib", "staticlib", "lib"]
name = "ffi_types"

[features]
wasm = ["uniffi/wasm-unstable-single-threaded", "uniffi/scaffolding-ffi-buffer-fns", "dep:uniffi-runtime-wasm"]

[dependencies]
uniffi = { version = "0.31" }
uniffi-runtime-wasm = { version = "0.31.0-6", optional = true }
"#;

    fs::write(types_dir.join("Cargo.toml"), cargo_toml)?;

    let lib_rs = r#"#[cfg(target_arch = "wasm32")]
extern crate uniffi_runtime_wasm as _;

uniffi::setup_scaffolding!();

// Add shared FFI types here.
// Example:
// #[derive(Debug, Clone, uniffi::Record)]
// pub struct MyType {
//     pub field: String,
// }
"#;

    fs::write(types_src_dir.join("lib.rs"), lib_rs)?;

    Ok(())
}

fn write_workspace_cargo_toml(
    project_dir: &Path,
    crate_name: &str,
    crate_version: &str,
) -> Result<()> {
    let original_toml_path = project_dir.join("Cargo.toml");
    let original_content = fs::read_to_string(&original_toml_path)?;
    let original_doc = original_content.parse::<toml_edit::DocumentMut>()?;

    let edition = original_doc
        .get("package")
        .and_then(|p| p.get("edition"))
        .and_then(|e| e.as_str())
        .unwrap_or("2021");

    let license = original_doc
        .get("package")
        .and_then(|p| p.get("license"))
        .and_then(|l| l.as_str());

    let repository = original_doc
        .get("package")
        .and_then(|p| p.get("repository"))
        .and_then(|r| r.as_str());

    let mut workspace_pkg = format!("edition = \"{edition}\"\n");
    if let Some(lic) = license {
        workspace_pkg.push_str(&format!("license = \"{lic}\"\n"));
    }
    if let Some(repo) = repository {
        workspace_pkg.push_str(&format!("repository = \"{repo}\"\n"));
    }

    let cargo_toml = format!(
        r#"[workspace]
members = [
    "crates/lib/*",
    "crates/ffi/*",
]
exclude = [
    "crates/ffi/tools",
]
resolver = "2"

[workspace.package]
{workspace_pkg}
[workspace.dependencies]
{crate_name} = {{ path = "crates/lib/{crate_name}", version = "{crate_version}" }}
{crate_name}-ffi = {{ path = "crates/ffi/{crate_name}", version = "0.1.0" }}
ffi-types = {{ path = "crates/ffi/types", version = "0.1.0" }}
uniffi = {{ version = "0.31" }}
uniffi-runtime-wasm = {{ version = "0.31.0-6" }}
"#
    );

    fs::write(&original_toml_path, cargo_toml)?;

    Ok(())
}

fn add_ffi_to_workspace_toml(project_dir: &Path, crate_names: &[String]) -> Result<()> {
    let cargo_toml_path = project_dir.join("Cargo.toml");
    let content = fs::read_to_string(&cargo_toml_path)?;
    let mut doc = content.parse::<toml_edit::DocumentMut>()?;

    if let Some(workspace) = doc.get_mut("workspace").and_then(|w| w.as_table_like_mut()) {
        if let Some(members) = workspace
            .get_mut("members")
            .and_then(|m| m.as_array_mut())
        {
            let ffi_pattern = "crates/ffi/*";
            let already_has = members.iter().any(|v| v.as_str() == Some(ffi_pattern));
            if !already_has {
                members.push(ffi_pattern);
            }
        }

        if let Some(exclude) = workspace
            .get_mut("exclude")
            .and_then(|e| e.as_array_mut())
        {
            let tools_pattern = "crates/ffi/tools";
            let already_has = exclude.iter().any(|v| v.as_str() == Some(tools_pattern));
            if !already_has {
                exclude.push(tools_pattern);
            }
        } else {
            let mut arr = toml_edit::Array::new();
            arr.push("crates/ffi/tools");
            workspace.insert(
                "exclude",
                toml_edit::Item::Value(toml_edit::Value::Array(arr)),
            );
        }

        let deps = workspace
            .entry("dependencies")
            .or_insert(toml_edit::Item::Table(toml_edit::Table::new()))
            .as_table_like_mut();

        if let Some(deps) = deps {
            for crate_name in crate_names {
                let ffi_name = format!("{crate_name}-ffi");
                if !deps.contains_key(&ffi_name) {
                    let mut inline = toml_edit::InlineTable::new();
                    inline.insert(
                        "path",
                        toml_edit::Value::from(format!("crates/ffi/{crate_name}")),
                    );
                    inline.insert("version", toml_edit::Value::from("0.1.0"));
                    deps.insert(
                        &ffi_name,
                        toml_edit::Item::Value(toml_edit::Value::InlineTable(inline)),
                    );
                }
            }

            if !deps.contains_key("ffi-types") {
                let mut inline = toml_edit::InlineTable::new();
                inline.insert("path", toml_edit::Value::from("crates/ffi/types"));
                inline.insert("version", toml_edit::Value::from("0.1.0"));
                deps.insert(
                    "ffi-types",
                    toml_edit::Item::Value(toml_edit::Value::InlineTable(inline)),
                );
            }

            if !deps.contains_key("uniffi") {
                let mut inline = toml_edit::InlineTable::new();
                inline.insert("version", toml_edit::Value::from("0.31"));
                deps.insert(
                    "uniffi",
                    toml_edit::Item::Value(toml_edit::Value::InlineTable(inline)),
                );
            }

            if !deps.contains_key("uniffi-runtime-wasm") {
                let mut inline = toml_edit::InlineTable::new();
                inline.insert("version", toml_edit::Value::from("0.31.0-6"));
                deps.insert(
                    "uniffi-runtime-wasm",
                    toml_edit::Item::Value(toml_edit::Value::InlineTable(inline)),
                );
            }
        }
    }

    fs::write(&cargo_toml_path, doc.to_string())?;

    Ok(())
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            fs::copy(&src_path, &dest_path)?;
        }
    }

    Ok(())
}
