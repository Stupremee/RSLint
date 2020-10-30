use crate::{glue::fs2, project_root};
use anyhow::{Context, Result};
use cargo_toml::Manifest;
use serde::Serialize;
use std::{fs, path::Path, process::Command};
use toml::Value;

const SCOPES_DIR: &str = "crates/rslint_scope";

pub fn build_datalog(debug: bool, check: bool) -> Result<()> {
    let scopes_dir = project_root().join(SCOPES_DIR);

    /*
    let mut cmd = if cfg!(windows) {
        // let has_wsl = Command::new("wsl").arg("--help").output().is_ok();
        // if has_wsl {
        //     let mut cmd = Command::new("wsl");
        //     cmd.args(&["--", "exec", "\"$BASH\"", "&&", "ddlog"]);
        //     cmd
        // } else {
        //     eprintln!("wsl was not found, ddlog was not run");
        //     return Ok(());
        // }
        eprintln!("windows is currently unsupported, run from within wsl");
        return Ok(());
    } else {
        Command::new("ddlog")
    };

    cmd.args(&[
        "--action",
        if check { "validate" } else { "compile" },
        "--output-dir",
    ])
    .arg(&scopes_dir)
    .args(&["--omit-profile", "--omit-workspace"]);

    if debug {
        cmd.args(&[
            "--output-internal-relations",
            "--output-input-relations",
            "INPUT_",
        ]);
    }

    let status = dbg!(cmd.status()).context("failed to run ddlog")?;
    if !status.success() {
        eprintln!("ddlog exited with error code {:?}", status.code());
        return Ok(());
    }
    */

    // let ddlog_dir = scopes_dir.join("rslint_scoping_ddlog");
    let generated_dir = scopes_dir.join("generated");
    // if !ddlog_dir.exists() {
    //     eprintln!("could not find generated code, exiting");
    //     return Ok(());
    // }
    //
    // if generated_dir.exists() {
    //     fs2::remove_dir_all(&generated_dir).context("failed to remove the old generated code")?;
    // }
    //
    // fs::rename(&ddlog_dir, &generated_dir)
    //     .context("failed to rename the generated code's folder")?;

    edit_generated_code(&generated_dir)?;

    Ok(())
}

const LIBRARY_DEPS: &[&str] = &["ddlog_ovsdb_adapter", "cmd_parser", "rustop", "flatbuffers"];
const LIBRARY_FEATURES: &[&str] = &["ovsdb", "flatbuf", "command-line"];

const TYPES_DEPS: &[&str] = &["ddlog_ovsdb_adapter", "flatbuffers"];
const TYPES_FEATURES: &[&str] = &["ovsdb", "flatbuf"];

const EXTRA_LIBS: &[&str] = &["distributed_datalog", "ovsdb", "cmd_parser", ".cargo"];
const EXTRA_FILES: &[&str] = &["src/main.rs", "ddlog_ovsdb_test.c", "ddlog.h"];

fn edit_generated_code(generated_dir: &Path) -> Result<()> {
    // Edit generated/Cargo.toml
    let library_path = generated_dir.join("Cargo.toml");
    let mut library_toml = edit_toml(
        "generated/Cargo.toml",
        &library_path,
        LIBRARY_DEPS,
        LIBRARY_FEATURES,
    )?;

    library_toml.bin.clear();
    library_toml.features.get_mut("default").map(Vec::clear);

    write_toml("generated/Cargo.toml", &library_path, &library_toml)?;

    // Edit generated/types/Cargo.toml
    let types_path = generated_dir.join("types/Cargo.toml");
    let types_toml = edit_toml(
        "generated/types/Cargo.toml",
        &types_path,
        TYPES_DEPS,
        TYPES_FEATURES,
    )?;
    write_toml("generated/types/Cargo.toml", &types_path, &types_toml)?;

    // Remove extra libraries
    for lib in EXTRA_LIBS.iter().copied() {
        fs2::remove_dir_all(generated_dir.join(lib)).ok();
    }

    // Remove extra files
    for file in EXTRA_FILES.iter().copied() {
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
