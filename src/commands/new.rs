use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::Args;
use console::style;

use crate::collision::OnCollision;
use crate::template;

#[derive(Args)]
pub struct NewArgs {
    /// Directory to create for the new project
    pub path: PathBuf,

    /// Template repository URL
    #[arg(long)]
    pub template: Option<String>,

    /// Keep the demo crates (complex, polynomials, etc.) instead of stripping them
    #[arg(long)]
    pub demo: bool,
}

pub fn run(args: NewArgs) -> Result<()> {
    if args.path.exists() {
        bail!("directory '{}' already exists", args.path.display());
    }

    let name = args
        .path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("invalid path: {}", args.path.display()))?
        .to_string_lossy()
        .to_string();

    eprintln!(
        "{} Creating project {} at {}...",
        style("●").cyan(),
        style(&name).bold(),
        style(args.path.display()).bold()
    );

    std::fs::create_dir_all(&args.path)?;

    let temp_dir = template::clone_template_to_temp(args.template.as_deref())?;
    template::copy_template_to_dest(temp_dir.path(), &args.path, OnCollision::Skip)?;

    eprintln!("  Applying project name...");
    template::templatize(&args.path, &name)?;

    if !args.demo {
        eprintln!("  Stripping demo crates...");
        template::strip_example_crates(&args.path, &name)?;
    }

    eprintln!("  Initializing git repository...");
    template::init_git_repo(&args.path)?;

    eprintln!();
    eprintln!(
        "{} Project created at {}",
        style("✓").green(),
        args.path.display()
    );
    eprintln!();
    eprintln!("  Next steps:");
    eprintln!("    cd {}", args.path.display());
    eprintln!("    ffi build --all");

    Ok(())
}
