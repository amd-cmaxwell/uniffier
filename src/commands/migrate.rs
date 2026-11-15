use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::Args;
use console::style;

use crate::collision::{self, OnCollision};
use crate::template;
use crate::workspace::{self, CrateInfo, ProjectKind};

#[derive(Args)]
pub struct MigrateArgs {
    /// Template repository URL
    #[arg(long)]
    pub template: Option<String>,

    /// Skip interactive prompts, use defaults
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Overwrite existing files that collide with template files
    #[arg(long, short = 'f')]
    pub force: bool,

    /// Include demo example crates and apps from the template
    #[arg(long)]
    pub demo: bool,

    /// Create FFI wrappers without moving existing crates into crates/lib/
    #[arg(long)]
    pub in_place: bool,

    /// Only add FFI wrappers for these crates (comma-separated)
    #[arg(long, value_delimiter = ',', conflicts_with = "skip_ffi")]
    pub ffi_crates: Vec<String>,

    /// Skip FFI wrappers for these crates (comma-separated)
    #[arg(long, value_delimiter = ',', conflicts_with = "ffi_crates")]
    pub skip_ffi: Vec<String>,

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
        .ok_or_else(|| anyhow::anyhow!("no Rust crates found in {}", project_dir.display()))?;

    match project_kind {
        ProjectKind::SingleCrate { name, version } => {
            migrate_single_crate(&project_dir, &name, &version, &args)?;
        }
        ProjectKind::Workspace { members } => {
            migrate_workspace(&project_dir, &members, &args)?;
        }
        ProjectKind::LooseCrates {
            root_crate,
            sub_crates,
        } => {
            migrate_loose_crates(&project_dir, root_crate, sub_crates, &args)?;
        }
    }

    Ok(())
}

fn resolve_ffi_selection(
    crate_names: &[String],
    args: &MigrateArgs,
    label: &str,
) -> Result<Vec<usize>> {
    let all_indices: Vec<usize> = (0..crate_names.len()).collect();

    let indices = if !args.ffi_crates.is_empty() {
        let mut selected = Vec::new();
        for name in &args.ffi_crates {
            match crate_names.iter().position(|n| n == name) {
                Some(i) => selected.push(i),
                None => bail!(
                    "unknown crate '{}' in --ffi-crates (discovered: {})",
                    name,
                    crate_names.join(", ")
                ),
            }
        }
        selected
    } else if !args.skip_ffi.is_empty() {
        let mut skip_set = std::collections::HashSet::new();
        for name in &args.skip_ffi {
            match crate_names.iter().position(|n| n == name) {
                Some(i) => { skip_set.insert(i); }
                None => bail!(
                    "unknown crate '{}' in --skip-ffi (discovered: {})",
                    name,
                    crate_names.join(", ")
                ),
            }
        }
        all_indices.into_iter().filter(|i| !skip_set.contains(i)).collect()
    } else if !args.yes && std::io::stdin().is_terminal() && crate_names.len() > 1 {
        dialoguer::MultiSelect::new()
            .with_prompt(label)
            .items(crate_names)
            .defaults(&vec![true; crate_names.len()])
            .interact()?
    } else {
        all_indices
    };

    if indices.is_empty() {
        bail!("no crates selected for FFI migration");
    }

    Ok(indices)
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

    if args.in_place {
        eprintln!("  Creating FFI wrapper crate...");
        create_ffi_crate(project_dir, crate_name, "../..")?;

        eprintln!("  Creating shared FFI types crate...");
        create_ffi_types_crate(project_dir)?;

        eprintln!("  Copying template assets (Makefile, scripts, templates, tools)...");
        clone_template_assets(project_dir, args)?;

        eprintln!("  Updating Cargo.toml to workspace...");
        let original_toml_path = project_dir.join("Cargo.toml");
        let content = fs::read_to_string(&original_toml_path)?;
        let mut doc = content.parse::<toml_edit::DocumentMut>()?;

        let mut ws = toml_edit::Table::new();
        let mut members_arr = toml_edit::Array::new();
        members_arr.push(".");
        members_arr.push("crates/ffi/*");
        ws.insert("members", toml_edit::Item::Value(toml_edit::Value::Array(members_arr)));
        let mut exclude_arr = toml_edit::Array::new();
        exclude_arr.push("crates/ffi/tools");
        ws.insert("exclude", toml_edit::Item::Value(toml_edit::Value::Array(exclude_arr)));
        ws.insert("resolver", toml_edit::Item::Value("2".into()));
        doc.insert("workspace", toml_edit::Item::Table(ws));

        fs::write(&original_toml_path, doc.to_string())?;
    } else {
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
        create_ffi_crate(project_dir, crate_name, &format!("../../lib/{crate_name}"))?;

        eprintln!("  Creating shared FFI types crate...");
        create_ffi_types_crate(project_dir)?;

        eprintln!("  Copying template assets (Makefile, scripts, templates, tools)...");
        clone_template_assets(project_dir, args)?;
    }

    if args.demo {
        eprintln!("  Applying project name to demo files...");
        template::templatize(project_dir, crate_name)?;
    }

    eprintln!("{} Migration complete.", style("✓").green());
    eprintln!();
    if args.in_place {
        eprintln!("  Project structure:");
        eprintln!("    ./                        <- your original crate");
        eprintln!("    crates/ffi/{crate_name}/   <- UniFFI wrapper (edit this)");
        eprintln!("    crates/ffi/types/          <- shared FFI types");
        eprintln!("    crates/ffi/tools/          <- bindgen tool binaries");
    } else {
        eprintln!("  Project structure:");
        eprintln!("    crates/lib/{crate_name}/   <- your original crate");
        eprintln!("    crates/ffi/{crate_name}/   <- UniFFI wrapper (edit this)");
        eprintln!("    crates/ffi/types/          <- shared FFI types");
        eprintln!("    crates/ffi/tools/          <- bindgen tool binaries");
    }
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

    let crate_names: Vec<String> = existing_crates.iter().map(|c| c.name.clone()).collect();

    let ffi_indices = resolve_ffi_selection(&crate_names, args, "Select crates to add FFI wrappers for")?;

    let ffi_set: std::collections::HashSet<usize> = ffi_indices.into_iter().collect();

    if !args.yes && std::io::stdin().is_terminal() {
        let ffi_names: Vec<&str> = ffi_set.iter().map(|&i| existing_crates[i].name.as_str()).collect();
        let action = if args.in_place { "Add FFI wrappers (in-place)" } else { "Restructure and add FFI wrappers" };
        let proceed = dialoguer::Confirm::new()
            .with_prompt(format!("{action} for: {}. Continue?", ffi_names.join(", ")))
            .default(true)
            .interact()?;
        if !proceed {
            eprintln!("Aborted.");
            return Ok(());
        }
    }

    if args.in_place {
        for (i, dc) in existing_crates.iter().enumerate() {
            if ffi_set.contains(&i) {
                let lib_path = relative_path_from_ffi_crate(&dc.rel_path);
                eprintln!("  Creating FFI wrapper for {}...", dc.name);
                create_ffi_crate(project_dir, &dc.name, &lib_path)?;
            }
        }

        eprintln!("  Creating shared FFI types crate...");
        create_ffi_types_crate(project_dir)?;

        eprintln!("  Copying template assets...");
        clone_template_assets(project_dir, args)?;

        let ffi_names: Vec<String> = ffi_set.iter().map(|&i| existing_crates[i].name.clone()).collect();
        eprintln!("  Updating workspace Cargo.toml...");
        add_ffi_to_workspace_toml(project_dir, &ffi_names)?;
    } else {
        fs::create_dir_all(project_dir.join("crates/lib"))?;
        fs::create_dir_all(project_dir.join("crates"))?;

        let mut workspace_crates = Vec::new();
        let mut has_non_lib_crates = false;

        for (i, dc) in existing_crates.iter().enumerate() {
            let is_ffi = ffi_set.contains(&i);
            let dest_prefix = if is_ffi { "crates/lib" } else { "crates" };
            let dest = project_dir.join(dest_prefix).join(&dc.name);
            let src = project_dir.join(&dc.rel_path);

            if src != dest {
                eprintln!("  Moving '{}' into {}/{}...", dc.rel_path, dest_prefix, dc.name);
                fs::create_dir_all(dest.parent().unwrap())?;
                fs::rename(&src, &dest)
                    .with_context(|| format!("failed to move {}", dc.rel_path))?;
                rewrite_crate_toml_for_workspace(&dest)?;
            }

            workspace_crates.push(WorkspaceCrate {
                name: dc.name.clone(),
                version: dc.version.clone(),
                path: format!("{dest_prefix}/{}", dc.name),
                has_ffi: is_ffi,
            });

            if !is_ffi {
                has_non_lib_crates = true;
            }
        }

        eprintln!("  Writing workspace Cargo.toml...");
        let mut member_patterns = vec!["crates/lib/*"];
        if has_non_lib_crates {
            member_patterns.push("crates/*");
        }
        write_workspace_cargo_toml_multi(project_dir, &workspace_crates, &member_patterns)?;

        for (i, dc) in existing_crates.iter().enumerate() {
            if ffi_set.contains(&i) {
                eprintln!("  Creating FFI wrapper for {}...", dc.name);
                create_ffi_crate(project_dir, &dc.name, &format!("../../lib/{}", dc.name))?;
            }
        }

        eprintln!("  Creating shared FFI types crate...");
        create_ffi_types_crate(project_dir)?;

        eprintln!("  Copying template assets...");
        clone_template_assets(project_dir, args)?;
    }

    if args.demo {
        let project_name = project_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project")
            .to_string();
        eprintln!("  Applying project name to demo files...");
        template::templatize(project_dir, &project_name)?;
    }

    eprintln!("{} Migration complete.", style("✓").green());
    eprintln!();
    eprintln!("  Next steps:");
    eprintln!("    # Add FFI exports in crates/ffi/*/src/lib.rs");
    eprintln!("    ffi build --all");

    Ok(())
}

fn relative_path_from_ffi_crate(lib_rel_path: &str) -> String {
    let depth = lib_rel_path.matches('/').count();
    let ups = "../".repeat(depth + 2);
    format!("{ups}{lib_rel_path}")
}

fn migrate_loose_crates(
    project_dir: &Path,
    root_crate: Option<CrateInfo>,
    sub_crates: Vec<CrateInfo>,
    args: &MigrateArgs,
) -> Result<()> {
    let mut all_crates: Vec<CrateInfo> = Vec::new();
    if let Some(rc) = root_crate {
        all_crates.push(rc);
    }
    all_crates.extend(sub_crates);

    if all_crates.is_empty() {
        bail!("no crates found to migrate");
    }

    let crate_names: Vec<String> = all_crates.iter().map(|c| c.name.clone()).collect();

    eprintln!(
        "{} Found {} crate(s) at project root: {}",
        style("●").cyan(),
        all_crates.len(),
        crate_names.join(", ")
    );

    let ffi_indices = resolve_ffi_selection(&crate_names, args, "Select crates to add FFI wrappers for")?;

    let ffi_set: std::collections::HashSet<usize> = ffi_indices.into_iter().collect();

    let ffi_names: Vec<String> = ffi_set.iter().map(|&i| crate_names[i].clone()).collect();
    let non_ffi_names: Vec<String> = (0..all_crates.len())
        .filter(|i| !ffi_set.contains(i))
        .map(|i| crate_names[i].clone())
        .collect();

    if !args.yes && std::io::stdin().is_terminal() {
        let mut summary = format!("FFI wrappers: {}", ffi_names.join(", "));
        if !non_ffi_names.is_empty() {
            summary.push_str(&format!("\n  Other crates: {}", non_ffi_names.join(", ")));
        }
        summary.push_str("\n  This will restructure the project into a workspace. Continue?");
        let proceed = dialoguer::Confirm::new()
            .with_prompt(summary)
            .default(true)
            .interact()?;
        if !proceed {
            eprintln!("Aborted.");
            return Ok(());
        }
    }

    if args.in_place {
        let mut member_patterns: Vec<String> = Vec::new();
        let mut workspace_crates = Vec::new();

        for (i, crate_info) in all_crates.iter().enumerate() {
            let rel = if crate_info.dir_name.is_empty() {
                ".".to_string()
            } else {
                crate_info.dir_name.clone()
            };
            if !member_patterns.contains(&rel) {
                member_patterns.push(rel.clone());
            }

            workspace_crates.push(WorkspaceCrate {
                name: crate_info.name.clone(),
                version: crate_info.version.clone(),
                path: if crate_info.dir_name.is_empty() { ".".to_string() } else { crate_info.dir_name.clone() },
                has_ffi: ffi_set.contains(&i),
            });

            if ffi_set.contains(&i) {
                let lib_path = relative_path_from_ffi_crate(&rel);
                eprintln!("  Creating FFI wrapper for {}...", crate_info.name);
                create_ffi_crate(project_dir, &crate_info.name, &lib_path)?;
            }
        }

        member_patterns.push("crates/ffi/*".to_string());
        let patterns_ref: Vec<&str> = member_patterns.iter().map(|s| s.as_str()).collect();

        eprintln!("  Writing workspace Cargo.toml...");
        write_workspace_cargo_toml_multi(project_dir, &workspace_crates, &patterns_ref)?;
    } else {
        fs::create_dir_all(project_dir.join("crates/lib"))?;
        fs::create_dir_all(project_dir.join("crates"))?;

        let mut workspace_crates = Vec::new();
        let mut has_non_lib_crates = false;

        for (i, crate_info) in all_crates.iter().enumerate() {
            let is_ffi = ffi_set.contains(&i);
            let dest_prefix = if is_ffi { "crates/lib" } else { "crates" };
            let dest = project_dir.join(dest_prefix).join(&crate_info.name);

            if crate_info.dir_name.is_empty() {
                fs::create_dir_all(dest.join("src"))?;
                eprintln!(
                    "  Moving root crate '{}' into {}/{}...",
                    crate_info.name, dest_prefix, crate_info.name
                );
                move_crate_contents(project_dir, &dest)?;
            } else {
                let src = project_dir.join(&crate_info.dir_name);
                eprintln!(
                    "  Moving '{}' into {}/{}...",
                    crate_info.dir_name, dest_prefix, crate_info.name
                );
                fs::rename(&src, &dest)
                    .with_context(|| format!("failed to move {}", crate_info.dir_name))?;
            }

            rewrite_crate_toml_for_workspace(&dest)?;

            workspace_crates.push(WorkspaceCrate {
                name: crate_info.name.clone(),
                version: crate_info.version.clone(),
                path: format!("{dest_prefix}/{}", crate_info.name),
                has_ffi: is_ffi,
            });

            if !is_ffi {
                has_non_lib_crates = true;
            }
        }

        eprintln!("  Writing workspace Cargo.toml...");
        let mut member_patterns = vec!["crates/lib/*"];
        if has_non_lib_crates {
            member_patterns.push("crates/*");
        }
        write_workspace_cargo_toml_multi(project_dir, &workspace_crates, &member_patterns)?;

        for (i, crate_info) in all_crates.iter().enumerate() {
            if ffi_set.contains(&i) {
                eprintln!("  Creating FFI wrapper for {}...", crate_info.name);
                create_ffi_crate(project_dir, &crate_info.name, &format!("../../lib/{}", crate_info.name))?;
            }
        }
    }

    eprintln!("  Creating shared FFI types crate...");
    create_ffi_types_crate(project_dir)?;

    eprintln!("  Copying template assets...");
    clone_template_assets(project_dir, args)?;

    if args.demo {
        let project_name = project_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project")
            .to_string();
        eprintln!("  Applying project name to demo files...");
        template::templatize(project_dir, &project_name)?;
    }

    eprintln!("{} Migration complete.", style("✓").green());
    eprintln!();
    eprintln!("  Project structure:");
    for (i, crate_info) in all_crates.iter().enumerate() {
        if ffi_set.contains(&i) {
            if args.in_place {
                let loc = if crate_info.dir_name.is_empty() { "./" } else { &crate_info.dir_name };
                eprintln!("    {loc:<30} <- lib crate (FFI)");
            } else {
                eprintln!("    crates/lib/{}/   <- lib crate (FFI)", crate_info.name);
            }
            eprintln!("    crates/ffi/{}/   <- UniFFI wrapper", crate_info.name);
        } else if !args.in_place {
            eprintln!("    crates/{}/       <- lib crate", crate_info.name);
        }
    }
    eprintln!("    crates/ffi/types/          <- shared FFI types");
    eprintln!("    crates/ffi/tools/          <- bindgen tool binaries");
    eprintln!();
    eprintln!("  Next steps:");
    eprintln!("    # Add FFI exports in crates/ffi/*/src/lib.rs");
    eprintln!("    ffi build --all");

    Ok(())
}

struct DiscoveredCrate {
    name: String,
    version: String,
    rel_path: String,
}

impl std::fmt::Display for DiscoveredCrate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.name, self.rel_path)
    }
}

fn discover_workspace_crates(project_dir: &Path, members: &[String]) -> Result<Vec<DiscoveredCrate>> {
    let mut crates = Vec::new();

    for member in members {
        if member.ends_with('*') {
            let base = member.trim_end_matches('*').trim_end_matches('/');
            let base_dir = project_dir.join(base);
            if base_dir.is_dir() {
                for entry in fs::read_dir(&base_dir)? {
                    let entry = entry?;
                    let toml_path = entry.path().join("Cargo.toml");
                    if toml_path.exists() {
                        if let Some(dir_name) = entry.file_name().to_str() {
                            let rel = format!("{base}/{dir_name}");
                            let (name, version) = read_crate_name_version(&toml_path, dir_name);
                            crates.push(DiscoveredCrate { name, version, rel_path: rel });
                        }
                    }
                }
            }
        } else {
            let member_dir = project_dir.join(member);
            let toml_path = member_dir.join("Cargo.toml");
            if toml_path.exists() {
                let dir_name = member_dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(member);
                let (name, version) = read_crate_name_version(&toml_path, dir_name);
                crates.push(DiscoveredCrate { name, version, rel_path: member.clone() });
            }
        }
    }

    crates.sort_by(|a, b| a.name.cmp(&b.name));
    crates.dedup_by(|a, b| a.name == b.name);
    Ok(crates)
}

fn read_crate_name_version(toml_path: &Path, fallback_name: &str) -> (String, String) {
    let content = fs::read_to_string(toml_path).unwrap_or_default();
    let doc = content.parse::<toml_edit::DocumentMut>().ok();
    let name = doc.as_ref()
        .and_then(|d| d.get("package"))
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or(fallback_name)
        .to_string();
    let version = doc.as_ref()
        .and_then(|d| d.get("package"))
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .unwrap_or("0.1.0")
        .to_string();
    (name, version)
}

fn discover_demo_crates(template_ffi_dir: &Path) -> Vec<String> {
    let skip = ["tools", "types"];
    let mut crates = Vec::new();
    if let Ok(entries) = fs::read_dir(template_ffi_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            if entry.path().join("Cargo.toml").exists() {
                if let Some(name) = entry.file_name().to_str() {
                    if !skip.contains(&name) {
                        crates.push(name.to_string());
                    }
                }
            }
        }
    }
    crates.sort();
    crates
}

fn clone_template_assets(project_dir: &Path, args: &MigrateArgs) -> Result<()> {
    let temp_dir = template::clone_template_to_temp(args.template.as_deref())?;
    let temp_path = template::template_clone_path(temp_dir.path());

    let assets = ["Makefile", "scripts", "templates", ".githooks"];

    let mut all_collisions = Vec::new();
    for asset in &assets {
        let src = temp_path.join(asset);
        if src.exists() {
            let asset_collisions = collision::detect_collisions(&src, &project_dir.join(asset));
            for rel in asset_collisions {
                all_collisions.push(PathBuf::from(asset).join(rel));
            }
        }
    }

    let extra_assets: &[(&str, &str)] = &[
        ("crates/ffi/tools", "crates/ffi/tools"),
        ("uniffi.toml", "uniffi.toml"),
        ("bindings/README.md", "bindings/README.md"),
    ];
    for (src_rel, dest_rel) in extra_assets {
        let src = temp_path.join(src_rel);
        let dest = project_dir.join(dest_rel);
        if src.is_file() && dest.exists() {
            all_collisions.push(PathBuf::from(dest_rel));
        } else if src.is_dir() {
            let asset_collisions = collision::detect_collisions(&src, &dest);
            for rel in asset_collisions {
                all_collisions.push(PathBuf::from(dest_rel).join(rel));
            }
        }
    }

    let demo_crates = if args.demo {
        let crates = discover_demo_crates(&temp_path.join("crates/ffi"));
        for dir in &["examples", "docs"] {
            let src = temp_path.join(dir);
            if src.exists() {
                let collisions = collision::detect_collisions(&src, &project_dir.join(dir));
                for rel in collisions {
                    all_collisions.push(PathBuf::from(*dir).join(rel));
                }
            }
        }
        for name in &crates {
            for prefix in &["crates/lib", "crates/ffi"] {
                let rel = format!("{prefix}/{name}");
                let src = temp_path.join(&rel);
                if src.exists() {
                    let collisions = collision::detect_collisions(&src, &project_dir.join(&rel));
                    for c in collisions {
                        all_collisions.push(PathBuf::from(&rel).join(c));
                    }
                }
            }
        }
        crates
    } else {
        Vec::new()
    };

    let on_collision = collision::resolve_collisions(&all_collisions, args.force, args.yes)?;

    for asset in &assets {
        let src = temp_path.join(asset);
        let dest = project_dir.join(asset);

        if src.is_dir() {
            template::copy_dir_recursive(&src, &dest, on_collision)?;
        } else if src.is_file() {
            if !dest.exists() || on_collision == OnCollision::Overwrite {
                fs::copy(&src, &dest).with_context(|| format!("failed to copy {asset}"))?;
            }
        }
    }

    let tools_src = temp_path.join("crates/ffi/tools");
    let tools_dest = project_dir.join("crates/ffi/tools");
    if tools_src.exists() {
        if !tools_dest.exists() || on_collision == OnCollision::Overwrite {
            template::copy_dir_recursive(&tools_src, &tools_dest, on_collision)?;
        }
    }

    let uniffi_dest = project_dir.join("uniffi.toml");
    if !uniffi_dest.exists() || on_collision == OnCollision::Overwrite {
        let uniffi_src = temp_path.join("uniffi.toml");
        if uniffi_src.exists() {
            fs::copy(&uniffi_src, &uniffi_dest).context("failed to copy uniffi.toml")?;
        }
    }

    let bindings_dir = project_dir.join("bindings");
    if !bindings_dir.exists() {
        fs::create_dir_all(&bindings_dir)?;
        let readme_src = temp_path.join("bindings/README.md");
        if readme_src.exists() {
            fs::copy(&readme_src, bindings_dir.join("README.md"))?;
        }
    } else if on_collision == OnCollision::Overwrite {
        let readme_src = temp_path.join("bindings/README.md");
        if readme_src.exists() {
            fs::copy(&readme_src, bindings_dir.join("README.md"))?;
        }
    }

    if args.demo {
        for dir in &["examples", "docs"] {
            let src = temp_path.join(dir);
            let dest = project_dir.join(dir);
            if src.exists() {
                template::copy_dir_recursive(&src, &dest, on_collision)?;
            }
        }
        for name in &demo_crates {
            for prefix in &["crates/lib", "crates/ffi"] {
                let src = temp_path.join(prefix).join(name);
                let dest = project_dir.join(prefix).join(name);
                if src.exists() {
                    template::copy_dir_recursive(&src, &dest, on_collision)?;
                }
            }
        }
    }

    Ok(())
}

fn create_ffi_crate(project_dir: &Path, crate_name: &str, lib_path: &str) -> Result<()> {
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
{crate_name} = {{ path = "{lib_path}" }}
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

struct WorkspaceCrate {
    name: String,
    version: String,
    path: String,
    has_ffi: bool,
}

fn write_workspace_cargo_toml(
    project_dir: &Path,
    crate_name: &str,
    crate_version: &str,
) -> Result<()> {
    let crates = vec![WorkspaceCrate {
        name: crate_name.to_string(),
        version: crate_version.to_string(),
        path: format!("crates/lib/{crate_name}"),
        has_ffi: true,
    }];
    write_workspace_cargo_toml_multi(project_dir, &crates, &["crates/lib/*"])
}

fn write_workspace_cargo_toml_multi(
    project_dir: &Path,
    crates: &[WorkspaceCrate],
    member_patterns: &[&str],
) -> Result<()> {
    let original_toml_path = project_dir.join("Cargo.toml");

    let (edition, license, repository) = if original_toml_path.exists() {
        let original_content = fs::read_to_string(&original_toml_path)?;
        let original_doc = original_content.parse::<toml_edit::DocumentMut>()?;
        (
            original_doc
                .get("package")
                .and_then(|p| p.get("edition"))
                .and_then(|e| e.as_str())
                .unwrap_or("2021")
                .to_string(),
            original_doc
                .get("package")
                .and_then(|p| p.get("license"))
                .and_then(|l| l.as_str())
                .map(String::from),
            original_doc
                .get("package")
                .and_then(|p| p.get("repository"))
                .and_then(|r| r.as_str())
                .map(String::from),
        )
    } else {
        ("2021".to_string(), None, None)
    };

    let mut workspace_pkg = format!("edition = \"{edition}\"\n");
    if let Some(lic) = &license {
        workspace_pkg.push_str(&format!("license = \"{lic}\"\n"));
    }
    if let Some(repo) = &repository {
        workspace_pkg.push_str(&format!("repository = \"{repo}\"\n"));
    }

    let members_str = member_patterns
        .iter()
        .chain(std::iter::once(&"crates/ffi/*"))
        .map(|p| format!("    \"{p}\","))
        .collect::<Vec<_>>()
        .join("\n");

    let mut deps = String::new();
    for c in crates {
        deps.push_str(&format!(
            "{} = {{ path = \"{}\", version = \"{}\" }}\n",
            c.name, c.path, c.version
        ));
        if c.has_ffi {
            deps.push_str(&format!(
                "{}-ffi = {{ path = \"crates/ffi/{}\", version = \"0.1.0\" }}\n",
                c.name, c.name
            ));
        }
    }
    deps.push_str("ffi-types = { path = \"crates/ffi/types\", version = \"0.1.0\" }\n");
    deps.push_str("uniffi = { version = \"0.31\" }\n");
    deps.push_str("uniffi-runtime-wasm = { version = \"0.31.0-6\" }\n");

    let cargo_toml = format!(
        r#"[workspace]
members = [
{members_str}
]
exclude = [
    "crates/ffi/tools",
]
resolver = "2"

[workspace.package]
{workspace_pkg}
[workspace.dependencies]
{deps}"#
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

