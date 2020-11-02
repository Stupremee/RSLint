use crate::{glue::fs2, project_root};
use anyhow::{Context, Result};
use cargo_toml::Manifest;
use std::path::Path;
use toml::Value;

/// The path of the rslint_scope crate
const SCOPES_DIR: &str = "crates/rslint_scope";
/// The name of the generated code's directory
const GENERATED_DIR: &str = "generated";

/// The Cargo.toml of the main ddlog libraryy
const LIBRARY_TOML: &str = "generated/Cargo.toml";
/// Extra dependencies of the main ddlog library
const LIBRARY_DEPS: &[&str] = &["ddlog_ovsdb_adapter", "cmd_parser", "rustop", "flatbuffers"];
/// Extra features of the main ddlog library
const LIBRARY_FEATURES: &[&str] = &["ovsdb", "flatbuf", "command-line"];

/// The Cargo.toml of the ddlog types library
const TYPES_TOML: &str = "generated/types/Cargo.toml";
/// Extra dependencies of the ddlog types library
const TYPES_DEPS: &[&str] = &["ddlog_ovsdb_adapter", "flatbuffers"];
/// Extra features of the ddlog types library
const TYPES_FEATURES: &[&str] = &["ovsdb", "flatbuf"];

/// Extra generated libraries that will be deleted
const EXTRA_LIBS: &[&str] = &["distributed_datalog", "ovsdb", "cmd_parser", ".cargo"];
/// Extra generated files that will be deleted
const EXTRA_FILES: &[&str] = &["src/main.rs", "ddlog_ovsdb_test.c", "ddlog.h"];

/// Trims the extra stuff from the code generated by ddlog
pub fn trim_datalog() -> Result<()> {
    let scopes_dir = project_root().join(SCOPES_DIR);
    let generated_dir = scopes_dir.join(GENERATED_DIR);

    if generated_dir.exists() {
        trim_generated_code(&scopes_dir, &generated_dir)?;
    }

    Ok(())
}

fn trim_generated_code(scopes_dir: &Path, generated_dir: &Path) -> Result<()> {
    // Edit generated/Cargo.toml
    println!("editing {}...", LIBRARY_TOML);
    let library_path = scopes_dir.join(LIBRARY_TOML);
    let mut library_toml = edit_toml(LIBRARY_TOML, &library_path, LIBRARY_DEPS, LIBRARY_FEATURES)?;

    library_toml.bin.clear();
    library_toml.features.get_mut("default").map(Vec::clear);

    write_toml(LIBRARY_TOML, &library_path, &library_toml)?;

    // Edit generated/types/Cargo.toml
    println!("editing {}...", TYPES_TOML);
    let types_path = scopes_dir.join(TYPES_TOML);
    let types_toml = edit_toml(TYPES_TOML, &types_path, TYPES_DEPS, TYPES_FEATURES)?;
    write_toml(TYPES_TOML, &types_path, &types_toml)?;

    // Remove extra libraries
    for lib in EXTRA_LIBS.iter().copied() {
        println!("deleting {}...", lib);
        fs2::remove_dir_all(generated_dir.join(lib)).ok();
    }

    // Remove extra files
    for file in EXTRA_FILES.iter().copied() {
        println!("removing {}...", file);
        fs2::remove_file(generated_dir.join(file)).ok();
    }

    Ok(())
}

fn edit_toml(
    name: &str,
    path: &Path,
    dependencies: &[&str],
    features: &[&str],
) -> Result<Manifest> {
    let failed_manifest = || format!("failed to load manifest for {} at {}", name, path.display());
    let contents = fs2::read_to_string(path).with_context(failed_manifest)?;
    let mut manifest = Manifest::from_str(&contents).with_context(failed_manifest)?;

    // Remove extra dependencies
    for dep in dependencies.iter().copied() {
        manifest.dependencies.remove(dep);
    }

    // Remove extra features
    for feature in features.iter().copied() {
        manifest.features.remove(feature);
    }

    // Make the crate a library
    if let Some(lib) = manifest.lib.as_mut() {
        lib.crate_type = vec!["lib".to_owned()];
    }

    Ok(manifest)
}

fn write_toml(name: &str, path: &Path, manifest: &Manifest) -> Result<()> {
    let failed_toml = || format!("failed to render toml for {}", name);
    let toml = toml::to_string(&Value::try_from(manifest).with_context(failed_toml)?)
        .with_context(failed_toml)?
        .replace("[profile]", "");

    fs2::write(path, toml).with_context(|| {
        format!(
            "failed to write edited manifest for {} to {}",
            name,
            path.display(),
        )
    })
}
