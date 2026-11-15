use std::io::IsTerminal;
use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::Args;
use console::style;

use crate::collision;
use crate::template;

#[derive(Args)]
pub struct InitArgs {
    /// Directory to initialize (defaults to current directory)
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Template repository URL
    #[arg(long)]
    pub template: Option<String>,

    /// Skip interactive prompts, use defaults
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Keep the demo crates (complex, polynomials, etc.) instead of stripping them
    #[arg(long)]
    pub demo: bool,

    /// Overwrite existing files that collide with template files
    #[arg(long)]
    pub force: bool,
}

pub fn run(args: InitArgs) -> Result<()> {
    let dest = &args.path;

    if dest.join("Cargo.toml").exists() {
        bail!(
            "directory '{}' already contains a Cargo.toml — use `ffi migrate` to add FFI support to an existing Rust project",
            dest.display()
        );
    }

    let name = dest
        .canonicalize()
        .unwrap_or_else(|_| dest.to_path_buf())
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("cannot determine project name from path"))?
        .to_string_lossy()
        .to_string();

    let name = if !args.yes && std::io::stdin().is_terminal() {
        dialoguer::Input::new()
            .with_prompt("Project name")
            .default(name)
            .interact_text()?
    } else {
        name
    };

    eprintln!(
        "{} Initializing project {} in {}...",
        style("●").cyan(),
        style(&name).bold(),
        style(dest.display()).bold()
    );

    let temp_dir = template::clone_template_to_temp(args.template.as_deref())?;
    let clone_path = template::template_clone_path(temp_dir.path());

    let collisions = collision::detect_collisions(&clone_path, dest);
    let on_collision = collision::resolve_collisions(&collisions, args.force, args.yes)?;

    template::copy_template_to_dest(temp_dir.path(), dest, on_collision)?;

    eprintln!("  Applying project name...");
    template::templatize(dest, &name)?;

    if !args.demo {
        eprintln!("  Stripping demo crates...");
        template::strip_example_crates(dest, &name)?;
    }

    if !dest.join(".git").exists() {
        eprintln!("  Initializing git repository...");
        template::init_git_repo(dest)?;
    }

    eprintln!();
    eprintln!(
        "{} Project initialized at {}",
        style("✓").green(),
        dest.display()
    );
    eprintln!();
    eprintln!("  Next steps:");
    eprintln!("    ffi build --all");

    Ok(())
}
