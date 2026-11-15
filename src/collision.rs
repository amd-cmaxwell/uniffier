use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use console::style;
use walkdir::WalkDir;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OnCollision {
    Skip,
    Overwrite,
}

pub fn detect_collisions(src: &Path, dest: &Path) -> Vec<PathBuf> {
    let mut collisions = Vec::new();

    for entry in WalkDir::new(src).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }

        if let Ok(rel) = entry.path().strip_prefix(src) {
            let dest_path = dest.join(rel);
            if dest_path.exists() {
                collisions.push(rel.to_path_buf());
            }
        }
    }

    collisions.sort();
    collisions
}

pub fn resolve_collisions(
    collisions: &[PathBuf],
    force: bool,
    auto_yes: bool,
) -> Result<OnCollision> {
    if collisions.is_empty() {
        return Ok(OnCollision::Skip);
    }

    if force {
        eprintln!(
            "{} Overwriting {} existing file(s) (--force)",
            style("⚠").yellow(),
            collisions.len()
        );
        return Ok(OnCollision::Overwrite);
    }

    eprintln!(
        "\n{} The following existing files would be overwritten by the template:",
        style("⚠").yellow()
    );
    for path in collisions {
        eprintln!("    {}", style(path.display()).dim());
    }
    eprintln!();

    if auto_yes || !std::io::stdin().is_terminal() {
        bail!("file collisions detected — use --force to overwrite, or resolve manually");
    }

    let choices = &[
        "Skip (keep your existing files)",
        "Overwrite (replace with template files)",
        "Abort",
    ];
    let selection = dialoguer::Select::new()
        .with_prompt("How should these collisions be handled?")
        .items(choices)
        .default(0)
        .interact()?;

    match selection {
        0 => Ok(OnCollision::Skip),
        1 => Ok(OnCollision::Overwrite),
        _ => bail!("aborted due to file collisions"),
    }
}
