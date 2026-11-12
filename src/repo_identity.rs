use std::path::Path;
use std::process::Command;

use anyhow::{bail, Result};

pub struct RepoIdentity {
    pub owner: String,
    pub name: String,
}

impl RepoIdentity {
    pub fn apply_to(&self, cmd: &mut Command) {
        cmd.arg(format!("REPO_OWNER={}", self.owner));
        cmd.arg(format!("REPO_NAME={}", self.name));
    }
}

pub fn resolve(
    project_dir: &Path,
    cli_owner: Option<&str>,
    cli_name: Option<&str>,
) -> Result<RepoIdentity> {
    let from_remote = owner_from_git_remote(project_dir);

    let owner = match (cli_owner, &from_remote) {
        (Some(o), _) => o.to_string(),
        (None, Some((o, _))) => o.clone(),
        (None, None) => {
            bail!(
                "cannot determine REPO_OWNER — no git remote configured.\n\
                 Pass --repo-owner <owner> (e.g. --repo-owner my-org)"
            );
        }
    };

    let name = match (cli_name, &from_remote) {
        (Some(n), _) => n.to_string(),
        (None, Some((_, n))) => n.clone(),
        (None, None) => dir_name(project_dir),
    };

    Ok(RepoIdentity { owner, name })
}

fn owner_from_git_remote(project_dir: &Path) -> Option<(String, String)> {
    let repo = git2::Repository::discover(project_dir).ok()?;

    for remote_name in &["upstream", "origin"] {
        if let Ok(remote) = repo.find_remote(remote_name) {
            if let Some(url) = remote.url() {
                if let Some(pair) = parse_owner_name(url) {
                    return Some(pair);
                }
            }
        }
    }

    None
}

fn parse_owner_name(url: &str) -> Option<(String, String)> {
    let stripped = url
        .trim_end_matches('/')
        .trim_end_matches(".git");

    // SSH: git@github.com:owner/repo
    if let Some(rest) = stripped.strip_prefix("git@") {
        let after_colon = rest.split_once(':')?.1;
        let parts: Vec<&str> = after_colon.splitn(2, '/').collect();
        if parts.len() == 2 {
            return Some((parts[0].to_string(), parts[1].to_string()));
        }
    }

    // HTTPS: https://github.com/owner/repo
    let path = stripped
        .strip_prefix("https://")
        .or_else(|| stripped.strip_prefix("http://"))?;
    let segments: Vec<&str> = path.split('/').collect();
    if segments.len() >= 3 {
        return Some((
            segments[segments.len() - 2].to_string(),
            segments[segments.len() - 1].to_string(),
        ));
    }

    None
}

fn dir_name(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".to_string())
}
