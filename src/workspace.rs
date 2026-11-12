use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

pub enum ProjectKind {
    SingleCrate { name: String, version: String },
    Workspace { members: Vec<String> },
}

pub fn detect_project(dir: &Path) -> Result<Option<ProjectKind>> {
    let cargo_toml = dir.join("Cargo.toml");
    if !cargo_toml.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&cargo_toml).context("failed to read Cargo.toml")?;
    let doc = content
        .parse::<toml_edit::DocumentMut>()
        .context("failed to parse Cargo.toml")?;

    if doc.get("workspace").is_some() {
        let members = doc
            .get("workspace")
            .and_then(|w| w.get("members"))
            .and_then(|m| m.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        return Ok(Some(ProjectKind::Workspace { members }));
    }

    if let Some(name) = doc
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
    {
        let version = doc
            .get("package")
            .and_then(|p| p.get("version"))
            .and_then(|v| v.as_str())
            .unwrap_or("0.1.0")
            .to_string();

        return Ok(Some(ProjectKind::SingleCrate {
            name: name.to_string(),
            version,
        }));
    }

    Ok(None)
}
