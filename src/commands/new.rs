use std::io::IsTerminal;
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

    /// Skip interactive prompts, use defaults
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Skip generating example crates and skeleton binding examples
    #[arg(long)]
    pub no_examples: bool,
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

    if args.no_examples && !args.demo {
        if !args.yes && std::io::stdin().is_terminal() {
            eprintln!(
                "{} Skipping example generation. `ffi test` and `make test-*` will not work \
                 until you manually create the expected test structure in `examples/`.",
                style("⚠").yellow()
            );
            let proceed = dialoguer::Confirm::new()
                .with_prompt("Continue without examples?")
                .default(false)
                .interact()?;
            if !proceed {
                bail!("aborted");
            }
        } else {
            eprintln!(
                "{} Skipping example generation. `ffi test` and `make test-*` will not work \
                 until you manually create the expected test structure in `examples/`.",
                style("⚠").yellow()
            );
        }
    }

    std::fs::create_dir_all(&args.path)?;

    let temp_dir = template::clone_template_to_temp(args.template.as_deref())?;
    template::copy_template_to_dest(temp_dir.path(), &args.path, OnCollision::Skip)?;
    let clone_path = template::template_clone_path(temp_dir.path());
    template::merge_gitignore(&clone_path, &args.path)?;

    if args.demo {
        eprintln!("  Keeping demo crates as-is...");
    } else {
        eprintln!("  Applying project name...");
        template::templatize(&args.path, &name)?;
        eprintln!("  Stripping demo crates...");
        template::strip_example_crates(&args.path, &name, args.no_examples)?;
    }

    template::write_project_readme(&args.path, true)?;

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
