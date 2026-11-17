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

pub fn strip_demo_uniffi_external_packages(project_dir: &Path) -> Result<()> {
    let uniffi_toml_path = project_dir.join("uniffi.toml");
    if !uniffi_toml_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&uniffi_toml_path)?;
    let mut doc = content
        .parse::<toml_edit::DocumentMut>()
        .context("failed to parse uniffi.toml")?;

    let modified = if let Some(ext) = doc
        .get_mut("bindings")
        .and_then(|b| b.get_mut("python"))
        .and_then(|p| p.get_mut("external_packages"))
        .and_then(|e| e.as_table_like_mut())
    {
        let demo_keys: Vec<String> = ext
            .iter()
            .filter(|(k, _)| {
                TEMPLATE_PREFIXES
                    .iter()
                    .any(|(prefix, _)| k.contains(prefix))
            })
            .map(|(k, _)| k.to_string())
            .collect();

        let had_removals = !demo_keys.is_empty();
        for key in demo_keys {
            ext.remove(&key);
        }
        had_removals
    } else {
        false
    };

    if modified {
        fs::write(&uniffi_toml_path, doc.to_string())?;
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
    } else {
        let examples_dir = project_dir.join("examples");
        if examples_dir.is_dir() {
            for entry in fs::read_dir(&examples_dir)? {
                let entry = entry?;
                let name = entry.file_name();
                if name == "README.md" {
                    continue;
                }
                let path = entry.path();
                if path.is_dir() {
                    fs::remove_dir_all(&path)?;
                } else {
                    fs::remove_file(&path)?;
                }
            }
        }
    }

    strip_docs(project_dir)?;
    strip_demo_uniffi_external_packages(project_dir)?;

    Ok(())
}

pub fn generate_example_foo_lib(project_dir: &Path) -> Result<()> {
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

pub fn generate_example_foo_ffi(project_dir: &Path) -> Result<()> {
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
         crate-type = [\"cdylib\", \"staticlib\", \"lib\"]\n\
         name = \"example_foo_ffi\"\n\n\
         [features]\n\
         wasm = [\"uniffi/wasm-unstable-single-threaded\", \"uniffi/scaffolding-ffi-buffer-fns\", \"dep:uniffi-runtime-wasm\"]\n\n\
         [dependencies]\n\
         uniffi.workspace = true\n\
         uniffi-runtime-wasm = { workspace = true, optional = true }\n\
         example-foo.workspace = true\n",
    ).context("failed to write example-foo-ffi Cargo.toml")?;

    fs::write(
        src_dir.join("lib.rs"),
        "#[cfg(target_arch = \"wasm32\")]\n\
         extern crate uniffi_runtime_wasm as _;\n\n\
         uniffi::setup_scaffolding!();\n\n\
         #[uniffi::export]\n\
         pub fn foo(a: &str, b: &str) -> String {\n\
         \x20   example_foo::foo(a, b)\n\
         }\n\n\
         #[uniffi::export]\n\
         pub fn bar(x: i32) -> i32 {\n\
         \x20   example_foo::bar(x)\n\
         }\n",
    ).context("failed to write example-foo-ffi lib.rs")?;

    let uniffi_link = crate_dir.join("uniffi.toml");
    if !uniffi_link.exists() {
        std::os::unix::fs::symlink("../../../uniffi.toml", &uniffi_link)
            .context("failed to symlink uniffi.toml into example-foo-ffi")?;
    }

    Ok(())
}

pub fn generate_example_types_ffi(project_dir: &Path) -> Result<()> {
    let crate_dir = project_dir.join("crates/ffi/example-types");
    let src_dir = crate_dir.join("src");
    fs::create_dir_all(&src_dir)?;

    fs::write(
        crate_dir.join("Cargo.toml"),
        "[package]\n\
         name = \"example-types-ffi\"\n\
         version = \"0.1.0\"\n\
         edition.workspace = true\n\
         readme = \"README.md\"\n\n\
         [lib]\n\
         crate-type = [\"cdylib\", \"staticlib\", \"lib\"]\n\
         name = \"example_types_ffi\"\n\n\
         [features]\n\
         wasm = [\"uniffi/wasm-unstable-single-threaded\", \"uniffi/scaffolding-ffi-buffer-fns\", \"dep:uniffi-runtime-wasm\"]\n\n\
         [dependencies]\n\
         uniffi.workspace = true\n\
         uniffi-runtime-wasm = { workspace = true, optional = true }\n\
         thiserror = \"2\"\n",
    ).context("failed to write example-types-ffi Cargo.toml")?;

    fs::write(
        src_dir.join("lib.rs"),
        "// Shared FFI Types Crate (Example)\n\
         //\n\
         // This crate demonstrates how to create a shared FFI-only types crate.\n\
         // Types defined here are exported via UniFFI and available to all\n\
         // language bindings without being tied to any specific lib crate.\n\
         //\n\
         // Use a crate like this when multiple FFI crates need to share common\n\
         // types (enums, records, errors) across binding boundaries.\n\
         //\n\
         // This example crate can be safely deleted once you understand the\n\
         // pattern — create your own `crates/ffi/<name>` crate to replace it.\n\
         \n\
         #[cfg(target_arch = \"wasm32\")]\n\
         extern crate uniffi_runtime_wasm as _;\n\
         \n\
         uniffi::setup_scaffolding!();\n\
         \n\
         /// An example shared error type.\n\
         ///\n\
         /// FFI crates can depend on this crate and use this error type in their\n\
         /// exported functions, so all language bindings share a single definition.\n\
         #[derive(Debug, thiserror::Error, uniffi::Error)]\n\
         pub enum ExampleError {\n\
         \x20   #[error(\"{message}\")]\n\
         \x20   General { message: String },\n\
         }\n\
         \n\
         /// An example shared record type.\n\
         ///\n\
         /// Records defined here are visible to every language binding and can be\n\
         /// passed to or returned from any FFI crate that depends on this one.\n\
         #[derive(Debug, Clone, uniffi::Record)]\n\
         pub struct ExampleMetadata {\n\
         \x20   pub key: String,\n\
         \x20   pub value: String,\n\
         }\n",
    ).context("failed to write example-types-ffi lib.rs")?;

    fs::write(
        crate_dir.join("README.md"),
        "# example-types-ffi\n\
         \n\
         This is an **example** shared FFI types crate. It provides common UniFFI\n\
         type definitions — errors and records — so that every FFI crate in the\n\
         workspace can use a single, consistent set of types across all generated\n\
         language bindings.\n\
         \n\
         It is safe to delete once you understand the pattern — create your own\n\
         `crates/ffi/<name>` crate to replace it.\n\
         \n\
         See [`crates/ffi/README.md`](../README.md) for the general pattern.\n\
         \n\
         ## Exported types\n\
         \n\
         | Type | Kind | Description |\n\
         |------|------|-------------|\n\
         | `ExampleError` | `uniffi::Error` | Shared error enum with a `General` variant |\n\
         | `ExampleMetadata` | `uniffi::Record` | Key-value metadata record |\n\
         \n\
         ## Consuming crates\n\
         \n\
         Any FFI crate that uses these types should depend on this crate and\n\
         declare it as an external package in its `uniffi.toml` (or symlink the\n\
         root `uniffi.toml`) so that UniFFI imports the shared types rather than\n\
         re-generating them:\n\
         \n\
         ```toml\n\
         [bindings.python.external_packages]\n\
         example_types_ffi = \"example_types_ffi\"\n\
         ```\n\
         \n\
         > **Important:** This crate does **not** have its own `uniffi.toml`\n\
         > listing itself as an external package — doing so would prevent UniFFI\n\
         > from generating the type definitions in the first place. Only consuming\n\
         > crates should declare it in `external_packages`.\n",
    ).context("failed to write example-types-ffi README.md")?;

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

pub fn write_skeleton_examples(project_dir: &Path, project_name: &str) -> Result<()> {
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
                 uniffi-py-demo = \"py_demo:main\"\n\
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

pub fn add_example_workspace_deps(project_dir: &Path) -> Result<()> {
    let cargo_toml_path = project_dir.join("Cargo.toml");
    let content = fs::read_to_string(&cargo_toml_path)?;
    let mut doc = content.parse::<toml_edit::DocumentMut>()?;

    if let Some(ws) = doc.get_mut("workspace").and_then(|w| w.as_table_like_mut()) {
        if let Some(members) = ws.get_mut("members").and_then(|m| m.as_array_mut()) {
            let has_lib = members.iter().any(|v| v.as_str() == Some("crates/lib/*"));
            if !has_lib {
                members.push("crates/lib/*");
            }
        }

        if let Some(deps) = ws.get_mut("dependencies").and_then(|d| d.as_table_like_mut()) {
            let example_deps = [
                ("example-foo", "crates/lib/example-foo"),
                ("example-foo-ffi", "crates/ffi/example-foo"),
                ("example-types-ffi", "crates/ffi/example-types"),
            ];
            for (name, path) in &example_deps {
                if !deps.contains_key(name) {
                    let mut inline = toml_edit::InlineTable::new();
                    inline.insert("path", toml_edit::Value::from(*path));
                    inline.insert("version", toml_edit::Value::from("0.1.0"));
                    deps.insert(
                        name,
                        toml_edit::Item::Value(toml_edit::Value::InlineTable(inline)),
                    );
                }
            }
        }
    }

    fs::write(&cargo_toml_path, doc.to_string())?;
    Ok(())
}

pub fn add_uniffi_external_packages(
    project_dir: &Path,
    packages: &[(&str, &str)],
) -> Result<()> {
    let uniffi_toml_path = project_dir.join("uniffi.toml");
    let content = if uniffi_toml_path.exists() {
        fs::read_to_string(&uniffi_toml_path)?
    } else {
        String::new()
    };

    let mut doc = content
        .parse::<toml_edit::DocumentMut>()
        .context("failed to parse uniffi.toml")?;

    let bindings = doc
        .entry("bindings")
        .or_insert(toml_edit::Item::Table(toml_edit::Table::new()))
        .as_table_mut()
        .context("bindings is not a table")?;
    let python = bindings
        .entry("python")
        .or_insert(toml_edit::Item::Table(toml_edit::Table::new()))
        .as_table_mut()
        .context("bindings.python is not a table")?;
    let ext = python
        .entry("external_packages")
        .or_insert(toml_edit::Item::Table(toml_edit::Table::new()))
        .as_table_mut()
        .context("bindings.python.external_packages is not a table")?;

    for (crate_name, module_name) in packages {
        if !ext.contains_key(crate_name) {
            ext.insert(crate_name, toml_edit::Item::Value((*module_name).into()));
        }
    }

    fs::write(&uniffi_toml_path, doc.to_string())?;
    Ok(())
}

pub fn stamp_template_placeholders(
    project_dir: &Path,
    repo_owner: &str,
    repo_name: &str,
) -> Result<()> {
    let git_host = "github.com";
    let npm_scope = repo_owner.to_string();

    let replacements: &[(&str, &str)] = &[
        ("%GIT_HOST%", git_host),
        ("%REPO_OWNER%", repo_owner),
        ("%REPO_NAME%", repo_name),
        ("%NPM_SCOPE%", &npm_scope),
    ];

    let stamp_files = [
        "uniffi.toml",
        "examples/go-demo/main.go",
        "examples/js-demo/main.mjs",
        "examples/web-demo/main.ts",
        "examples/web-demo/vite.config.ts",
    ];

    for rel in &stamp_files {
        let path = project_dir.join(rel);
        if !path.exists() {
            continue;
        }
        let content = fs::read_to_string(&path)?;
        let mut replaced = content.clone();

        for (placeholder, value) in replacements {
            replaced = replaced.replace(placeholder, value);
        }

        if replaced != content {
            fs::write(&path, replaced)
                .with_context(|| format!("failed to stamp placeholders in {rel}"))?;
        }
    }

    Ok(())
}

pub fn write_project_readme(project_dir: &Path, overwrite: bool) -> Result<()> {
    let readme_path = project_dir.join("README.md");
    if readme_path.exists() && !overwrite {
        return Ok(());
    }

    let project_name = project_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");

    let ws_description = read_workspace_description(&project_dir.join("Cargo.toml"));

    let lib_crates = scan_crate_dir(&project_dir.join("crates/lib"), ws_description.as_deref())?;
    let ffi_crates = scan_crate_dir(&project_dir.join("crates/ffi"), ws_description.as_deref())?;
    let other_crates = scan_crate_dir(&project_dir.join("crates"), ws_description.as_deref())?;

    let has_examples = project_dir.join("examples").is_dir();

    let mut out = String::new();
    out.push_str(&format!("# {project_name}\n\n"));

    if let Some(desc) = &ws_description {
        out.push_str(&format!("{desc}\n\n"));
    }

    out.push_str("## Project structure\n\n");
    out.push_str("This project uses [UniFFI](https://mozilla.github.io/uniffi-rs/) ");
    out.push_str("to generate language bindings (Python, Go, JS/WASM) from Rust.\n\n");
    out.push_str("```\n");
    out.push_str(&format!("{project_name}/\n"));
    if !lib_crates.is_empty() {
        out.push_str("  crates/lib/       # library crates\n");
    }
    if !ffi_crates.is_empty() {
        out.push_str("  crates/ffi/       # UniFFI wrapper crates\n");
    }
    if !other_crates.is_empty() {
        out.push_str("  crates/           # other crates\n");
    }
    if has_examples {
        out.push_str("  examples/         # language-specific usage examples\n");
    }
    out.push_str("  bindings/         # generated bindings (output of `ffi build`)\n");
    out.push_str("```\n\n");

    if !lib_crates.is_empty() {
        out.push_str("## Library crates\n\n");
        out.push_str("| Crate | Description |\n");
        out.push_str("|-------|-------------|\n");
        for (name, desc) in &lib_crates {
            out.push_str(&format!("| `{name}` | {desc} |\n"));
        }
        out.push('\n');
    }

    if !ffi_crates.is_empty() {
        out.push_str("## FFI crates\n\n");
        out.push_str("Each FFI crate wraps its corresponding library crate ");
        out.push_str("and exposes a UniFFI-compatible interface.\n\n");
        out.push_str("| Crate | Description |\n");
        out.push_str("|-------|-------------|\n");
        for (name, desc) in &ffi_crates {
            out.push_str(&format!("| `{name}` | {desc} |\n"));
        }
        out.push('\n');
    }

    if !other_crates.is_empty() {
        out.push_str("## Other crates\n\n");
        out.push_str("| Crate | Description |\n");
        out.push_str("|-------|-------------|\n");
        for (name, desc) in &other_crates {
            out.push_str(&format!("| `{name}` | {desc} |\n"));
        }
        out.push('\n');
    }

    if has_examples {
        out.push_str("## Examples\n\n");
        let mut example_dirs: Vec<_> = fs::read_dir(project_dir.join("examples"))?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();
        example_dirs.sort_by_key(|e| e.file_name());
        if !example_dirs.is_empty() {
            for entry in &example_dirs {
                let name = entry.file_name().to_string_lossy().to_string();
                out.push_str(&format!("- `examples/{name}/`\n"));
            }
            out.push('\n');
        }
    }

    out.push_str("## Getting started\n\n");
    out.push_str("```sh\n");
    out.push_str("# Build all bindings\n");
    out.push_str("ffi build --all\n\n");
    out.push_str("# Or use Make targets\n");
    out.push_str("make build\n");
    out.push_str("```\n\n");
    out.push_str("See the [Makefile](Makefile) for available targets.\n");

    fs::write(&readme_path, out)?;
    Ok(())
}

fn scan_crate_dir(dir: &Path, ws_description: Option<&str>) -> Result<Vec<(String, String)>> {
    let mut result = Vec::new();
    if !dir.is_dir() {
        return Ok(result);
    }
    let mut entries: Vec<_> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("Cargo.toml").exists())
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let dir_name = entry.file_name().to_string_lossy().to_string();
        if dir_name == "tools" {
            continue;
        }
        let toml_path = entry.path().join("Cargo.toml");
        let (name, description) = read_crate_name_description(&toml_path, &dir_name, ws_description);
        result.push((name, description));
    }
    Ok(result)
}

fn read_workspace_description(ws_toml_path: &Path) -> Option<String> {
    let content = fs::read_to_string(ws_toml_path).ok()?;
    let doc = content.parse::<toml_edit::DocumentMut>().ok()?;
    doc.get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get("description"))
        .and_then(|d| d.as_str())
        .map(|s| s.to_string())
}

fn read_crate_name_description(toml_path: &Path, fallback: &str, ws_description: Option<&str>) -> (String, String) {
    let content = match fs::read_to_string(toml_path) {
        Ok(c) => c,
        Err(_) => return (fallback.to_string(), String::new()),
    };
    let doc = match content.parse::<toml_edit::DocumentMut>() {
        Ok(d) => d,
        Err(_) => return (fallback.to_string(), String::new()),
    };
    let pkg = doc.get("package");
    let name = pkg
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or(fallback)
        .to_string();
    let description = pkg
        .and_then(|p| p.get("description"))
        .and_then(|d| {
            d.as_str().map(|s| s.to_string()).or_else(|| {
                d.as_inline_table()
                    .and_then(|t| t.get("workspace"))
                    .and_then(|v| v.as_bool())
                    .and_then(|b| if b { ws_description.map(|s| s.to_string()) } else { None })
            })
        })
        .unwrap_or_default();
    (name, description)
}

pub fn init_git_repo(project_dir: &Path) -> Result<()> {
    git2::Repository::init(project_dir).context("failed to initialize git repository")?;
    Ok(())
}
