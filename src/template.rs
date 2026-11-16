use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use tempfile::TempDir;
use walkdir::WalkDir;

use crate::collision::OnCollision;

const TEMPLATE_REPO: &str = "https://github.com/amd-cmaxwell/ffi-testing.git";
const TEMPLATE_PREFIXES: &[(&str, &str)] = &[
    ("uniffi_math_demo", "_"),
    ("uniffi-math-demo", "-"),
    ("uniffi_testing", "_"),
    ("uniffi-testing", "-"),
];

pub fn clone_template_to_temp(template_url: Option<&str>) -> Result<TempDir> {
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

    Ok(temp_dir)
}

pub fn copy_template_to_dest(
    temp_dir: &Path,
    dest: &Path,
    on_collision: OnCollision,
) -> Result<()> {
    let clone_path = temp_dir.join("template");
    fs::create_dir_all(dest)?;
    copy_dir_contents(&clone_path, dest, on_collision)
        .context("failed to copy template into destination")?;
    Ok(())
}

pub fn template_clone_path(temp_dir: &Path) -> std::path::PathBuf {
    temp_dir.join("template")
}

fn copy_dir_contents(src: &Path, dest: &Path, on_collision: OnCollision) -> Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        if entry.file_name() == ".gitignore" {
            continue;
        }

        if dest_path.exists() && on_collision == OnCollision::Skip {
            continue;
        }

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dest_path, on_collision)?;
        } else {
            fs::copy(&src_path, &dest_path)?;
        }
    }
    Ok(())
}

pub fn merge_gitignore(template_dir: &Path, project_dir: &Path) -> Result<()> {
    let src = template_dir.join(".gitignore");
    if !src.exists() {
        return Ok(());
    }

    let template_content = fs::read_to_string(&src).context("failed to read template .gitignore")?;

    let dest = project_dir.join(".gitignore");
    if dest.exists() {
        let existing = fs::read_to_string(&dest).context("failed to read existing .gitignore")?;
        let existing_set: std::collections::BTreeSet<&str> = existing
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();

        let mut new_lines = Vec::new();
        for line in template_content.lines() {
            if line.is_empty() || line.starts_with('#') {
                new_lines.push(line);
            } else if !existing_set.contains(line) {
                new_lines.push(line);
            }
        }

        let new_lines: Vec<&str> = new_lines
            .into_iter()
            .skip_while(|l| l.is_empty() || l.starts_with('#'))
            .collect();

        if new_lines.iter().any(|l| !l.is_empty() && !l.starts_with('#')) {
            let mut content = existing;
            if !content.ends_with('\n') {
                content.push('\n');
            }
            content.push_str("\n# Added by ffi\n# ==============\n");
            for line in &new_lines {
                content.push_str(line);
                content.push('\n');
            }
            fs::write(&dest, content).context("failed to write .gitignore")?;
        }
    } else {
        fs::copy(&src, &dest).context("failed to copy .gitignore")?;
    }

    Ok(())
}

pub fn copy_dir_recursive(src: &Path, dest: &Path, on_collision: OnCollision) -> Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dest_path, on_collision)?;
        } else if dest_path.exists() && on_collision == OnCollision::Skip {
            continue;
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

pub fn strip_example_crates(project_dir: &Path, project_name: &str, no_examples: bool) -> Result<()> {
    let ffi_dir = project_dir.join("crates/ffi");
    if ffi_dir.is_dir() {
        for entry in fs::read_dir(&ffi_dir)? {
            let entry = entry?;
            let name_str = entry.file_name().to_string_lossy().to_string();
            if entry.path().is_dir() && name_str != "tools" {
                fs::remove_dir_all(entry.path())
                    .with_context(|| format!("failed to remove crates/ffi/{name_str}"))?;
            }
        }
    }

    let lib_dir = project_dir.join("crates/lib");
    if lib_dir.is_dir() {
        for entry in fs::read_dir(&lib_dir)? {
            let entry = entry?;
            let name_str = entry.file_name().to_string_lossy().to_string();
            if entry.path().is_dir() {
                fs::remove_dir_all(entry.path())
                    .with_context(|| format!("failed to remove crates/lib/{name_str}"))?;
            }
        }
    }

    if !no_examples {
        generate_example_foo_lib(project_dir)?;
        generate_example_foo_ffi(project_dir)?;
        generate_example_types_ffi(project_dir)?;
    }

    rewrite_workspace_toml_for_examples(project_dir, no_examples)?;

    if !no_examples {
        write_skeleton_examples(project_dir, project_name)?;
    }

    strip_docs(project_dir)?;

    Ok(())
}

fn generate_example_foo_lib(project_dir: &Path) -> Result<()> {
    let crate_dir = project_dir.join("crates/lib/example-foo");
    let src_dir = crate_dir.join("src");
    fs::create_dir_all(&src_dir)?;

    fs::write(
        crate_dir.join("Cargo.toml"),
        "[package]\n\
         name = \"example-foo\"\n\
         version = \"0.1.0\"\n\
         edition.workspace = true\n",
    ).context("failed to write example-foo Cargo.toml")?;

    fs::write(
        src_dir.join("lib.rs"),
        "pub fn foo(a: &str, b: &str) -> String {\n\
         \x20   format!(\"{a}{b}\")\n\
         }\n\n\
         pub fn bar(x: i32) -> i32 {\n\
         \x20   x * 2\n\
         }\n",
    ).context("failed to write example-foo lib.rs")?;

    Ok(())
}

fn generate_example_foo_ffi(project_dir: &Path) -> Result<()> {
    let crate_dir = project_dir.join("crates/ffi/example-foo");
    let src_dir = crate_dir.join("src");
    fs::create_dir_all(&src_dir)?;

    fs::write(
        crate_dir.join("Cargo.toml"),
        "[package]\n\
         name = \"example-foo-ffi\"\n\
         version = \"0.1.0\"\n\
         edition.workspace = true\n\n\
         [lib]\n\
         crate-type = [\"cdylib\", \"staticlib\", \"lib\"]\n\n\
         [dependencies]\n\
         example-foo.workspace = true\n\
         uniffi.workspace = true\n",
    ).context("failed to write example-foo-ffi Cargo.toml")?;

    fs::write(
        src_dir.join("lib.rs"),
        "use uniffi;\n\n\
         #[uniffi::export]\n\
         pub fn foo(a: &str, b: &str) -> String {\n\
         \x20   example_foo::foo(a, b)\n\
         }\n\n\
         #[uniffi::export]\n\
         pub fn bar(x: i32) -> i32 {\n\
         \x20   example_foo::bar(x)\n\
         }\n",
    ).context("failed to write example-foo-ffi lib.rs")?;

    Ok(())
}

fn generate_example_types_ffi(project_dir: &Path) -> Result<()> {
    let crate_dir = project_dir.join("crates/ffi/example-types");
    let src_dir = crate_dir.join("src");
    fs::create_dir_all(&src_dir)?;

    fs::write(
        crate_dir.join("Cargo.toml"),
        "[package]\n\
         name = \"example-types-ffi\"\n\
         version = \"0.1.0\"\n\
         edition.workspace = true\n\n\
         [lib]\n\
         crate-type = [\"cdylib\", \"staticlib\", \"lib\"]\n\n\
         [dependencies]\n\
         uniffi.workspace = true\n",
    ).context("failed to write example-types-ffi Cargo.toml")?;

    fs::write(
        src_dir.join("lib.rs"),
        "use uniffi;\n",
    ).context("failed to write example-types-ffi lib.rs")?;

    Ok(())
}

fn rewrite_workspace_toml_for_examples(project_dir: &Path, no_examples: bool) -> Result<()> {
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
        let keys_to_remove: Vec<String> = deps
            .iter()
            .filter_map(|(k, _)| {
                let key = k.to_string();
                if key == "uniffi" || key == "uniffi-runtime-wasm" {
                    None
                } else {
                    Some(key)
                }
            })
            .collect();

        for key in keys_to_remove {
            deps.remove(&key);
        }

        if !no_examples {
            deps.insert("example-foo", toml_edit::Item::Value(
                toml_edit::InlineTable::from_iter([
                    ("path", toml_edit::Value::from("crates/lib/example-foo")),
                    ("version", toml_edit::Value::from("0.1.0")),
                ]).into(),
            ));
            deps.insert("example-foo-ffi", toml_edit::Item::Value(
                toml_edit::InlineTable::from_iter([
                    ("path", toml_edit::Value::from("crates/ffi/example-foo")),
                    ("version", toml_edit::Value::from("0.1.0")),
                ]).into(),
            ));
            deps.insert("example-types-ffi", toml_edit::Item::Value(
                toml_edit::InlineTable::from_iter([
                    ("path", toml_edit::Value::from("crates/ffi/example-types")),
                    ("version", toml_edit::Value::from("0.1.0")),
                ]).into(),
            ));
        }
    }

    fs::write(&cargo_toml_path, doc.to_string()).context("failed to write Cargo.toml")?;

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
    let ffi_mod = "example_foo_ffi";
    let types_mod = "example_types_ffi";

    let py_demo_dir = project_dir.join("examples/py-demo");
    if py_demo_dir.exists() {
        fs::write(
            py_demo_dir.join("py_demo.py"),
            format!(
                "from {ffi_mod} import foo, bar\n\n\n\
                 def main():\n\
                 \x20   result = foo(\"hello\", \"world\")\n\
                 \x20   assert result == \"helloworld\", f\"expected 'helloworld', got '{{result}}'\"\n\
                 \x20   print(f\"foo('hello', 'world') = '{{result}}'\")\n\n\
                 \x20   doubled = bar(21)\n\
                 \x20   assert doubled == 42, f\"expected 42, got {{doubled}}\"\n\
                 \x20   print(f\"bar(21) = {{doubled}}\")\n\n\
                 \x20   print(\"\\nAll tests passed!\")\n"
            ),
        ).context("failed to write py_demo.py skeleton")?;

        fs::write(
            py_demo_dir.join("__main__.py"),
            "from py_demo import main\n\nmain()\n",
        ).context("failed to write __main__.py skeleton")?;

        let pkg_name = project_name;
        fs::write(
            py_demo_dir.join("pyproject.toml"),
            format!(
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
            ),
        ).context("failed to write pyproject.toml skeleton")?;
    }

    let go_path = project_dir.join("examples/go-demo/main.go");
    if go_path.exists() {
        let existing = fs::read_to_string(&go_path)?;
        if let Some(base_import) = find_import(&existing, "ffi\"") {
            let base = base_import.rsplit('/').skip(1).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("/");
            let foo_import = format!("{base}/{ffi_mod}");
            fs::write(
                &go_path,
                format!(
                    "package main\n\n\
                     import (\n\
                     \t\"fmt\"\n\n\
                     \tfoo_ffi \"{foo_import}\"\n\
                     )\n\n\
                     func main() {{\n\
                     \tfmt.Printf(\"foo('hello', 'world') = '%s'\\n\", foo_ffi.Foo(\"hello\", \"world\"))\n\
                     \tfmt.Printf(\"bar(21) = %d\\n\", foo_ffi.Bar(21))\n\
                     \tfmt.Println(\"\\nAll tests passed!\")\n\
                     }}\n"
                ),
            ).context("failed to write Go skeleton")?;
        }
    }

    let js_path = project_dir.join("examples/js-demo/main.mjs");
    if js_path.exists() {
        let existing = fs::read_to_string(&js_path)?;
        if let Some(scope) = find_npm_scope(&existing) {
            let foo_pkg = format!("{scope}example-foo-ffi");
            let types_pkg = format!("{scope}example-types-ffi");
            fs::write(
                &js_path,
                format!(
                    "import {{ describe, it, before }} from \"node:test\";\n\
                     import assert from \"node:assert/strict\";\n\
                     import {{ createRequire }} from \"node:module\";\n\
                     import {{ pathToFileURL }} from \"node:url\";\n\n\
                     import {{ uniffiInitAsync as initTypes }} from \"{types_pkg}\";\n\
                     import {{ uniffiInitAsync as initFoo, foo, bar }} from \"{foo_pkg}\";\n\n\
                     const require = createRequire(import.meta.url);\n\
                     const wasmUrl = (pkg, name) =>\n\
                     \x20 pathToFileURL(require.resolve(`${{pkg}}/src/${{name}}.wasm`));\n\n\
                     before(async () => {{\n\
                     \x20 await Promise.all([\n\
                     \x20   initTypes(wasmUrl(\"{types_pkg}\", \"{types_mod}\")),\n\
                     \x20   initFoo(wasmUrl(\"{foo_pkg}\", \"{ffi_mod}\")),\n\
                     \x20 ]);\n\
                     }});\n\n\
                     describe(\"example-foo\", () => {{\n\
                     \x20 it(\"concatenates strings\", () => {{\n\
                     \x20   assert.equal(foo(\"hello\", \"world\"), \"helloworld\");\n\
                     \x20 }});\n\n\
                     \x20 it(\"doubles a number\", () => {{\n\
                     \x20   assert.equal(bar(21), 42);\n\
                     \x20 }});\n\
                     }});\n"
                ),
            ).context("failed to write JS skeleton")?;
        }
    }

    let web_ts_path = project_dir.join("examples/web-demo/main.ts");
    if web_ts_path.exists() {
        let existing = fs::read_to_string(&web_ts_path)?;
        if let Some(scope) = find_npm_scope(&existing) {
            let foo_pkg = format!("{scope}example-foo-ffi");
            let types_pkg = format!("{scope}example-types-ffi");
            fs::write(
                &web_ts_path,
                format!(
                    "import {{\n\
                     \x20 uniffiInitAsync as initTypes,\n\
                     }} from \"{types_pkg}\";\n\
                     import {{\n\
                     \x20 uniffiInitAsync as initFoo,\n\
                     \x20 foo,\n\
                     \x20 bar,\n\
                     }} from \"{foo_pkg}\";\n\n\
                     async function main() {{\n\
                     \x20 const loading = document.getElementById(\"loading\")!;\n\
                     \x20 const appEl = document.getElementById(\"app\")!;\n\n\
                     \x20 try {{\n\
                     \x20   await Promise.all([\n\
                     \x20     initTypes(fetch(new URL(\n\
                     \x20       `./node_modules/{types_pkg}/src/{types_mod}.wasm`, import.meta.url))),\n\
                     \x20     initFoo(fetch(new URL(\n\
                     \x20       `./node_modules/{foo_pkg}/src/{ffi_mod}.wasm`, import.meta.url))),\n\
                     \x20   ]);\n\
                     \x20 }} catch (e: any) {{\n\
                     \x20   loading.textContent = `Failed to load WASM: ${{e.message || e}}`;\n\
                     \x20   loading.style.color = \"red\";\n\
                     \x20   return;\n\
                     \x20 }}\n\n\
                     \x20 loading.style.display = \"none\";\n\
                     \x20 appEl.style.display = \"block\";\n\n\
                     \x20 const result = foo(\"hello\", \"world\");\n\
                     \x20 const doubled = bar(21);\n\
                     \x20 appEl.innerHTML = `<h2>Example FFI</h2>\n\
                     \x20   <p>foo('hello', 'world') = '${{result}}'</p>\n\
                     \x20   <p>bar(21) = ${{doubled}}</p>\n\
                     \x20   <p>${{result === \"helloworld\" && doubled === 42 ? \"All tests passed!\" : \"Tests FAILED!\"}}</p>`;\n\
                     }}\n\n\
                     main();\n"
                ),
            ).context("failed to write web demo skeleton")?;
        }
    }

    let vite_config_path = project_dir.join("examples/web-demo/vite.config.ts");
    if vite_config_path.exists() {
        let existing = fs::read_to_string(&vite_config_path)?;
        if let Some(scope) = find_npm_scope(&existing) {
            let foo_pkg = format!("{scope}example-foo-ffi");
            let types_pkg = format!("{scope}example-types-ffi");
            fs::write(
                &vite_config_path,
                format!(
                    "import {{ defineConfig }} from \"vite\";\n\n\
                     export default defineConfig({{\n\
                     \x20 assetsInclude: [\"**/*.wasm\"],\n\
                     \x20 optimizeDeps: {{\n\
                     \x20   exclude: [\n\
                     \x20     \"{types_pkg}\",\n\
                     \x20     \"{foo_pkg}\",\n\
                     \x20   ],\n\
                     \x20 }},\n\
                     }});\n"
                ),
            ).context("failed to write vite.config.ts skeleton")?;
        }
    }

    let index_path = project_dir.join("examples/web-demo/index.html");
    if index_path.exists() {
        fs::write(
            &index_path,
            "<!DOCTYPE html>\n\
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
             </html>\n",
        ).context("failed to write index.html skeleton")?;
    }

    Ok(())
}

fn find_npm_scope(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let idx = line.find('@')?;
        let rest = &line[idx..];
        let slash = rest.find('/')?;
        Some(rest[..slash + 1].to_string())
    })
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
