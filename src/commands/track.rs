use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context, Result};
use clap::Args;
use console::style;

#[derive(Args)]
pub struct TrackArgs {
    /// Paths to uniffied-workflow repos to set up
    pub paths: Vec<PathBuf>,
}

pub fn run(args: TrackArgs) -> Result<()> {
    let paths = if args.paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        args.paths
    };

    for path in &paths {
        if !path.exists() {
            bail!("path does not exist: {}", path.display());
        }
        if !path.is_dir() {
            bail!("not a directory: {}", path.display());
        }

        eprintln!(
            "{} Tracking {}",
            style("●").cyan(),
            style(path.display()).bold()
        );

        let git_dir = path.join(".git");
        if !git_dir.exists() {
            eprintln!("  Initializing git repository...");
            git2::Repository::init(path)
                .with_context(|| format!("failed to init git repo in {}", path.display()))?;
        }

        eprintln!("  Running make setup...");
        let status = Command::new("make")
            .arg("setup")
            .current_dir(path)
            .status()
            .with_context(|| format!("failed to run make setup in {}", path.display()))?;

        if !status.success() {
            bail!("make setup failed in {}", path.display());
        }

        eprintln!("{} {} ready.", style("✓").green(), path.display());
    }

    Ok(())
}
