use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use walkdir::WalkDir;

const TEMPLATE_REPO: &str = "https://github.com/amd-cmaxwell/ffi-testing.git";
const TEMPLATE_PREFIXES: &[(&str, &str)] = &[
    ("uniffi_math_demo", "_"),
    ("uniffi-math-demo", "-"),
    ("uniffi_testing", "_"),
    ("uniffi-testing", "-"),
];

pub fn clone_template(dest: &Path, template_url: Option<&str>) -> Result<()> {
    let url = template_url.unwrap_or(TEMPLATE_REPO);
    eprintln!("  Cloning template from {url}...");

    let temp_dir = tempfile::tempdir().context("failed to create temp directory")?;
    let clone_path = temp_dir.path().join("template");

    git2::build::RepoBuilder::new()
        .clone(url, &clone_path)
        .context("failed to clone template repository")?;

    let git_dir = clone_path.join(".git");
    if git_dir.exists() {
        fs::remove_dir_all(&git_dir).context("failed to remove .git directory")?;
    }

    fs::create_dir_all(dest)?;
    copy_dir_contents(&clone_path, dest)
        .context("failed to copy template into destination")?;

    Ok(())
}

fn copy_dir_contents(src: &Path, dest: &Path) -> Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        if dest_path.exists() {
            continue;
        }

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            fs::copy(&src_path, &dest_path)?;
        }
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            fs::copy(&src_path, &dest_path)?;
        }
    }
    Ok(())
}

pub fn templatize(project_dir: &Path, project_name: &str) -> Result<()> {
    let project_name_underscore = project_name.replace('-', "_");

    for entry in WalkDir::new(project_dir)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            name != ".git" && name != "target" && name != "bindings"
        })
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        let is_text = matches!(
            ext,
            "toml" | "rs" | "md" | "yml" | "yaml" | "sh" | "py" | "go" | "json" | "ts" | "mjs" | "html"
        ) || path.file_name().map_or(false, |n| {
            let name = n.to_string_lossy();
            name == "Makefile" || name == ".gitignore"
        });

        if !is_text {
            continue;
        }

        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let mut replaced = content.clone();
        for (prefix, sep) in TEMPLATE_PREFIXES {
            let replacement = if *sep == "_" {
                &project_name_underscore
            } else {
                project_name
            };
            replaced = replaced.replace(prefix, replacement);
        }

        if replaced != content {
            fs::write(path, replaced)
                .with_context(|| format!("failed to write {}", path.display()))?;
        }
    }

    Ok(())
}

pub fn strip_example_crates(project_dir: &Path, project_name: &str) -> Result<()> {
    let examples_to_remove = ["complex", "polynomials", "linear-algebra", "trig", "sequences"];

    for name in &examples_to_remove {
        let lib_crate = project_dir.join("crates/lib").join(name);
        if lib_crate.exists() {
            fs::remove_dir_all(&lib_crate)
                .with_context(|| format!("failed to remove crates/lib/{name}"))?;
        }

        let ffi_crate = project_dir.join("crates/ffi").join(name);
        if ffi_crate.exists() {
            fs::remove_dir_all(&ffi_crate)
                .with_context(|| format!("failed to remove crates/ffi/{name}"))?;
        }
    }

    rewrite_workspace_toml(project_dir)?;
    write_skeleton_examples(project_dir, project_name)?;
    strip_docs(project_dir)?;

    Ok(())
}

fn strip_docs(project_dir: &Path) -> Result<()> {
    let docs_dir = project_dir.join("docs");
    if !docs_dir.exists() {
        return Ok(());
    }

    let keep = [
        "adr-ffi-crate-separation.md",
        "bindings-branch-flow.md",
        "github-actions-plan.md",
        "makefile-debug-logging.md",
        "packaging-plan.md",
        "wasm-packaging-plan.md",
    ];

    for entry in fs::read_dir(&docs_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !keep.contains(&name_str.as_ref()) {
            let path = entry.path();
            if path.is_dir() {
                fs::remove_dir_all(&path)
                    .with_context(|| format!("failed to remove docs/{name_str}"))?;
            } else {
                fs::remove_file(&path)
                    .with_context(|| format!("failed to remove docs/{name_str}"))?;
            }
        }
    }

    Ok(())
}

fn find_import(content: &str, marker: &str) -> Option<String> {
    content.lines()
        .filter(|line| line.contains(marker))
        .find_map(|line| {
            let start = line.find('"')? + 1;
            let end = line.rfind('"')?;
            if start < end {
                Some(line[start..end].to_string())
            } else {
                None
            }
        })
}

fn write_skeleton_examples(project_dir: &Path, project_name: &str) -> Result<()> {
    let name_u = project_name.replace('-', "_");
    let core_mod = format!("{name_u}_core_ffi");
    let types_mod = format!("{name_u}_types_ffi");

    let py_demo_dir = project_dir.join("examples/py-demo");
    if py_demo_dir.exists() {
        let py_demo_path = py_demo_dir.join("py_demo.py");
        let content = format!(
            "from {core_mod} import add\n\n\n\
             def main():\n\
             \x20   result = add(2, 3)\n\
             \x20   assert result == 5, f\"expected 5, got {{result}}\"\n\
             \x20   print(f\"add(2, 3) = {{result}}\")\n\
             \x20   print(\"\\nAll tests passed!\")\n"
        );
        fs::write(&py_demo_path, content)
            .context("failed to write py_demo.py skeleton")?;

        let main_path = py_demo_dir.join("__main__.py");
        fs::write(&main_path, "from py_demo import main\n\nmain()\n")
            .context("failed to write __main__.py skeleton")?;

        let pyproject_path = py_demo_dir.join("pyproject.toml");
        let pkg_name = project_name;
        let content = format!(
            "[project]\n\
             name = \"{pkg_name}\"\n\
             version = \"0.1.0\"\n\
             requires-python = \">=3.9\"\n\n\
             [project.scripts]\n\
             {pkg_name} = \"py_demo:main\"\n\n\
             [build-system]\n\
             requires = [\"hatchling\"]\n\
             build-backend = \"hatchling.build\"\n\n\
             [tool.hatch.build.targets.wheel]\n\
             packages = [\"py_demo.py\"]\n"
        );
        fs::write(&pyproject_path, content)
            .context("failed to write pyproject.toml skeleton")?;
    }

    let go_path = project_dir.join("examples/go-demo/main.go");
    if go_path.exists() {
        let existing = fs::read_to_string(&go_path)?;
        if let Some(core_import) = find_import(&existing, &format!("{core_mod}\"")) {
            let content = format!(
                "package main\n\n\
                 import (\n\
                 \t\"fmt\"\n\n\
                 \tcore_ffi \"{core_import}\"\n\
                 )\n\n\
                 func main() {{\n\
                 \tfmt.Printf(\"add(2, 3) = %d\\n\", core_ffi.Add(2, 3))\n\
                 \tfmt.Println(\"\\nAll tests passed!\")\n\
                 }}\n"
            );
            fs::write(&go_path, content)
                .context("failed to write Go skeleton")?;
        }
    }

    let js_path = project_dir.join("examples/js-demo/main.mjs");
    if js_path.exists() {
        let existing = fs::read_to_string(&js_path)?;
        let core_pkg = find_import(&existing, "core-ffi");
        let types_pkg = find_import(&existing, "types-ffi");
        if let (Some(core_pkg), Some(types_pkg)) = (core_pkg, types_pkg) {
            let content = format!(
                "import {{ describe, it, before }} from \"node:test\";\n\
                 import assert from \"node:assert/strict\";\n\
                 import {{ createRequire }} from \"node:module\";\n\
                 import {{ pathToFileURL }} from \"node:url\";\n\n\
                 import {{ uniffiInitAsync as initTypes }} from \"{types_pkg}\";\n\
                 import {{ uniffiInitAsync as initCore, add }} from \"{core_pkg}\";\n\n\
                 const require = createRequire(import.meta.url);\n\
                 const wasmUrl = (pkg, name) =>\n\
                 \x20 pathToFileURL(require.resolve(`${{pkg}}/src/${{name}}.wasm`));\n\n\
                 before(async () => {{\n\
                 \x20 await Promise.all([\n\
                 \x20   initTypes(wasmUrl(\"{types_pkg}\", \"{types_mod}\")),\n\
                 \x20   initCore(wasmUrl(\"{core_pkg}\", \"{core_mod}\")),\n\
                 \x20 ]);\n\
                 }});\n\n\
                 describe(\"core\", () => {{\n\
                 \x20 it(\"adds two numbers\", () => {{\n\
                 \x20   assert.equal(add(2, 3), 5);\n\
                 \x20 }});\n\
                 }});\n"
            );
            fs::write(&js_path, content)
                .context("failed to write JS skeleton")?;
        }
    }

    let web_ts_path = project_dir.join("examples/web-demo/main.ts");
    if web_ts_path.exists() {
        let existing = fs::read_to_string(&web_ts_path)?;
        let core_pkg = find_import(&existing, "core-ffi");
        let types_pkg = find_import(&existing, "types-ffi");
        if let (Some(core_pkg), Some(types_pkg)) = (core_pkg, types_pkg) {
            let content = format!(
                "import {{\n\
                 \x20 uniffiInitAsync as initTypes,\n\
                 }} from \"{types_pkg}\";\n\
                 import {{\n\
                 \x20 uniffiInitAsync as initCore,\n\
                 \x20 add,\n\
                 }} from \"{core_pkg}\";\n\n\
                 async function main() {{\n\
                 \x20 const loading = document.getElementById(\"loading\")!;\n\
                 \x20 const appEl = document.getElementById(\"app\")!;\n\n\
                 \x20 try {{\n\
                 \x20   await Promise.all([\n\
                 \x20     initTypes(fetch(new URL(\n\
                 \x20       `./node_modules/{types_pkg}/src/{types_mod}.wasm`, import.meta.url))),\n\
                 \x20     initCore(fetch(new URL(\n\
                 \x20       `./node_modules/{core_pkg}/src/{core_mod}.wasm`, import.meta.url))),\n\
                 \x20   ]);\n\
                 \x20 }} catch (e: any) {{\n\
                 \x20   loading.textContent = `Failed to load WASM: ${{e.message || e}}`;\n\
                 \x20   loading.style.color = \"red\";\n\
                 \x20   return;\n\
                 \x20 }}\n\n\
                 \x20 loading.style.display = \"none\";\n\
                 \x20 appEl.style.display = \"block\";\n\n\
                 \x20 const result = add(2, 3);\n\
                 \x20 appEl.innerHTML = `<h2>Core Arithmetic</h2>\n\
                 \x20   <p>add(2, 3) = ${{result}}</p>\n\
                 \x20   <p>${{result === 5 ? \"Test passed!\" : \"Test FAILED!\"}}</p>`;\n\
                 }}\n\n\
                 main();\n"
            );
            fs::write(&web_ts_path, content)
                .context("failed to write web demo skeleton")?;
        }
    }

    let vite_config_path = project_dir.join("examples/web-demo/vite.config.ts");
    if vite_config_path.exists() {
        let existing = fs::read_to_string(&vite_config_path)?;
        let core_pkg = find_import(&existing, "core-ffi");
        let types_pkg = find_import(&existing, "types-ffi");
        if let (Some(core_pkg), Some(types_pkg)) = (core_pkg, types_pkg) {
            let content = format!(
                "import {{ defineConfig }} from \"vite\";\n\n\
                 export default defineConfig({{\n\
                 \x20 assetsInclude: [\"**/*.wasm\"],\n\
                 \x20 optimizeDeps: {{\n\
                 \x20   exclude: [\n\
                 \x20     \"{types_pkg}\",\n\
                 \x20     \"{core_pkg}\",\n\
                 \x20   ],\n\
                 \x20 }},\n\
                 }});\n"
            );
            fs::write(&vite_config_path, content)
                .context("failed to write vite.config.ts skeleton")?;
        }
    }

    let index_path = project_dir.join("examples/web-demo/index.html");
    if index_path.exists() {
        let content = "<!DOCTYPE html>\n\
             <html lang=\"en\">\n\
             \x20 <head>\n\
             \x20   <meta charset=\"UTF-8\" />\n\
             \x20   <meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\" />\n\
             \x20   <title>UniFFI WASM Demo</title>\n\
             \x20   <style>\n\
             \x20     body { font-family: system-ui; padding: 2rem; max-width: 600px; margin: 0 auto; }\n\
             \x20     .pass { color: green; }\n\
             \x20     .fail { color: red; }\n\
             \x20   </style>\n\
             \x20 </head>\n\
             \x20 <body>\n\
             \x20   <h1>UniFFI WASM Demo</h1>\n\
             \x20   <div id=\"loading\">Loading WASM modules...</div>\n\
             \x20   <div id=\"app\"></div>\n\
             \x20   <script type=\"module\" src=\"/main.ts\"></script>\n\
             \x20 </body>\n\
             </html>\n";
        fs::write(&index_path, content)
            .context("failed to write index.html skeleton")?;
    }

    Ok(())
}

fn rewrite_workspace_toml(project_dir: &Path) -> Result<()> {
    let cargo_toml_path = project_dir.join("Cargo.toml");
    let content = fs::read_to_string(&cargo_toml_path).context("failed to read Cargo.toml")?;

    let mut doc = content
        .parse::<toml_edit::DocumentMut>()
        .context("failed to parse Cargo.toml")?;

    if let Some(deps) = doc
        .get_mut("workspace")
        .and_then(|w| w.get_mut("dependencies"))
        .and_then(|d| d.as_table_like_mut())
    {
        let removed_crate_fragments = [
            "complex", "polynomials", "linear-algebra", "trig", "sequences",
        ];

        let keys_to_remove: Vec<String> = deps
            .iter()
            .filter_map(|(k, _)| {
                let key = k.to_string();
                if removed_crate_fragments
                    .iter()
                    .any(|frag| key.contains(frag))
                {
                    Some(key)
                } else {
                    None
                }
            })
            .collect();

        for key in keys_to_remove {
            deps.remove(&key);
        }
    }

    fs::write(&cargo_toml_path, doc.to_string()).context("failed to write Cargo.toml")?;

    Ok(())
}

pub fn stamp_example_imports(project_dir: &Path, repo_owner: &str, repo_name: &str) -> Result<()> {
    let new_go_base = format!("github.com/{repo_owner}/{repo_name}");
    let new_npm_scope = format!("@{repo_owner}/");

    let go_main = project_dir.join("examples/go-demo/main.go");
    if go_main.exists() {
        let content = fs::read_to_string(&go_main)?;
        let old_base = content.lines().find_map(|line| {
            let idx = line.find("/bindings/go/")?;
            let quote_start = line[..idx].rfind('"')? + 1;
            Some(line[quote_start..idx].to_string())
        });
        if let Some(old_base) = old_base {
            if old_base != new_go_base {
                fs::write(&go_main, content.replace(&old_base, &new_go_base))
                    .context("failed to stamp Go imports")?;
            }
        }
    }

    for rel in &[
        "examples/js-demo/main.mjs",
        "examples/web-demo/main.ts",
        "examples/web-demo/vite.config.ts",
    ] {
        let path = project_dir.join(rel);
        if !path.exists() {
            continue;
        }
        let content = fs::read_to_string(&path)?;
        let old_scope = content.lines().find_map(|line| {
            let idx = line.find("@")?;
            let rest = &line[idx..];
            let slash = rest.find('/')?;
            Some(rest[..slash + 1].to_string())
        });
        if let Some(old_scope) = old_scope {
            if old_scope != new_npm_scope {
                fs::write(&path, content.replace(&old_scope, &new_npm_scope))
                    .with_context(|| format!("failed to stamp npm scope in {rel}"))?;
            }
        }
    }

    Ok(())
}

pub fn init_git_repo(project_dir: &Path) -> Result<()> {
    git2::Repository::init(project_dir).context("failed to initialize git repository")?;
    Ok(())
}
