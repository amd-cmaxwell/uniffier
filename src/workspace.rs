use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

pub struct CrateInfo {
    pub name: String,
    pub version: String,
    pub dir_name: String,
}

pub enum ProjectKind {
    SingleCrate { name: String, version: String },
    Workspace { members: Vec<String> },
    LooseCrates {
        root_crate: Option<CrateInfo>,
        sub_crates: Vec<CrateInfo>,
    },
}

fn discover_sub_crates(dir: &Path) -> Result<Vec<CrateInfo>> {
    let mut crates = Vec::new();
    let skip = [".git", "target", "node_modules", ".github"];

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let dir_name = entry.file_name().to_string_lossy().to_string();
            if skip.contains(&dir_name.as_str()) || dir_name.starts_with('.') {
                continue;
            }
            let sub_toml = path.join("Cargo.toml");
            if !sub_toml.exists() {
                continue;
            }
            if let Ok(content) = fs::read_to_string(&sub_toml) {
                if let Ok(doc) = content.parse::<toml_edit::DocumentMut>() {
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
                        crates.push(CrateInfo {
                            name: name.to_string(),
                            version,
                            dir_name,
                        });
                    }
                }
            }
        }
    }

    crates.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(crates)
}

pub fn detect_project(dir: &Path) -> Result<Option<ProjectKind>> {
    let cargo_toml = dir.join("Cargo.toml");
    if !cargo_toml.exists() {
        let sub_crates = discover_sub_crates(dir)?;
        if sub_crates.is_empty() {
            return Ok(None);
        }
        return Ok(Some(ProjectKind::LooseCrates {
            root_crate: None,
            sub_crates,
        }));
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

        let sub_crates = discover_sub_crates(dir)?;
        if !sub_crates.is_empty() {
            return Ok(Some(ProjectKind::LooseCrates {
                root_crate: Some(CrateInfo {
                    dir_name: String::new(),
                    name: name.to_string(),
                    version,
                }),
                sub_crates,
            }));
        }

        return Ok(Some(ProjectKind::SingleCrate {
            name: name.to_string(),
            version,
        }));
    }

    Ok(None)
}
