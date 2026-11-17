use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};
use clap::Args;
use console::style;

use crate::repo_identity;
use crate::template;

#[derive(Args)]
pub struct TestArgs {
    /// Test Python bindings
    #[arg(long)]
    pub python: bool,

    /// Test Go bindings
    #[arg(long)]
    pub go: bool,

    /// Test JS bindings
    #[arg(long)]
    pub js: bool,

    /// Test web/browser bindings
    #[arg(long)]
    pub web: bool,

    /// Test all bindings (default if no flags given)
    #[arg(long)]
    pub all: bool,

    /// GitHub owner/org (required if no git remote is configured)
    #[arg(long, alias = "author", short = 'a')]
    pub repo_owner: Option<String>,

    /// Repository name (defaults to directory name)
    #[arg(long, alias = "name", short = 'n')]
    pub repo_name: Option<String>,
}

pub fn run(args: TestArgs) -> Result<()> {
    if !Path::new("Makefile").exists() {
        bail!("no Makefile found — are you in an FFI project directory?");
    }

    let identity = repo_identity::resolve(
        Path::new("."),
        args.repo_owner.as_deref(),
        args.repo_name.as_deref(),
    )?;

    template::stamp_template_placeholders(Path::new("."), &identity.owner, &identity.name)?;

    let targets = resolve_targets(args.python, args.go, args.js, args.web, args.all);

    for target in &targets {
        eprintln!(
            "{} Testing {} bindings...",
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

    eprintln!("{} All tests passed.", style("✓").green());
    Ok(())
}

fn resolve_targets(python: bool, go: bool, js: bool, web: bool, all: bool) -> Vec<&'static str> {
    if all || (!python && !go && !js && !web) {
        return vec!["test-all"];
    }

    let mut targets = Vec::new();
    if python {
        targets.push("test-python");
    }
    if go {
        targets.push("test-go");
    }
    if js {
        targets.push("test-js");
    }
    if web {
        targets.push("test-web");
    }
    targets
}

fn target_label(target: &str) -> &str {
    match target {
        "test-python" => "Python",
        "test-go" => "Go",
        "test-js" => "JS",
        "test-web" => "web/browser",
        "test-all" => "all",
        _ => target,
    }
}
