use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};
use clap::Args;
use console::style;

use crate::repo_identity;

#[derive(Args)]
pub struct BuildArgs {
    /// Build Python bindings
    #[arg(long)]
    pub python: bool,

    /// Build Go bindings
    #[arg(long)]
    pub go: bool,

    /// Build JS/WASM bindings
    #[arg(long)]
    pub js: bool,

    /// Build all bindings (default if no flags given)
    #[arg(long)]
    pub all: bool,

    /// GitHub owner/org (required if no git remote is configured)
    #[arg(long, alias = "author", short = 'a')]
    pub repo_owner: Option<String>,

    /// Repository name (defaults to directory name)
    #[arg(long, alias = "name", short = 'n')]
    pub repo_name: Option<String>,
}

pub fn run(args: BuildArgs) -> Result<()> {
    if !Path::new("Makefile").exists() {
        bail!("no Makefile found — are you in an FFI project directory?");
    }

    let identity = repo_identity::resolve(
        Path::new("."),
        args.repo_owner.as_deref(),
        args.repo_name.as_deref(),
    )?;

    let targets = resolve_targets(args.python, args.go, args.js, args.all);

    for target in &targets {
        eprintln!(
            "{} Building {} bindings...",
            style("●").cyan(),
            style(target_label(target)).bold()
        );

        let mut cmd = Command::new("make");
        cmd.arg(target);
        identity.apply_to(&mut cmd);

        let status = cmd
            .status()
            .with_context(|| format!("failed to run make {target}"))?;

        if !status.success() {
            bail!("make {} failed with exit code {}", target, status);
        }
    }

    eprintln!("{} Build complete.", style("✓").green());
    Ok(())
}

fn resolve_targets(python: bool, go: bool, js: bool, all: bool) -> Vec<&'static str> {
    if all || (!python && !go && !js) {
        return vec!["generate-all"];
    }

    let mut targets = Vec::new();
    if python {
        targets.push("generate-python");
    }
    if go {
        targets.push("generate-go");
    }
    if js {
        targets.push("generate-js");
    }
    targets
}

fn target_label(target: &str) -> &str {
    match target {
        "generate-python" => "Python",
        "generate-go" => "Go",
        "generate-js" => "JS/WASM",
        "generate-all" => "all",
        _ => target,
    }
}
