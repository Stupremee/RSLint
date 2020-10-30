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

fn edit_generated_code(generated_dir: &Path) -> Result<()> {
    // Remove extra libraries
    for lib in ["distributed_datalog", "ovsdb", "cmd_parser"]
        .iter()
        .copied()
    {
        fs2::remove_dir_all(generated_dir.join(lib)).ok();
    }

    fs2::remove_dir_all(generated_dir.join(".cargo")).ok();
    fs2::remove_file(generated_dir.join("src/main.rs")).ok();
    fs2::remove_file(generated_dir.join("ddlog_ovsdb_test.c")).ok();
    fs2::remove_file(generated_dir.join("ddlog.h")).ok();

    // Edit generated/Cargo.toml
    let library_path = generated_dir.join("Cargo.toml");
    let library_dependencies = ["ddlog_ovsdb_adapter", "cmd_parser", "rustop", "flatbuffers"];
    let library_features = ["ovsdb", "flatbuf", "command-line"];
    let mut library_toml = edit_toml(
        "generated/Cargo.toml",
        &library_path,
        &library_dependencies,
        &library_features,
    )?;

    // Remove the binary builds
    library_toml.bin.clear();

    write_toml("generated/Cargo.toml", &library_path, &library_toml)?;

    // Edit generated/types/Cargo.toml
    let types_path = generated_dir.join("types/Cargo.toml");
    let types_dependencies = ["ddlog_ovsdb_adapter", "flatbuffers"];
    let types_features = ["ovsdb", "flatbuf"];
    let types_toml = edit_toml(
        "generated/types/Cargo.toml",
        &types_path,
        &types_dependencies,
        &types_features,
    )?;
    write_toml("generated/types/Cargo.toml", &types_path, &types_toml)?;

    Ok(())
}

fn edit_toml(
    name: &str,
    path: &Path,
    dependencies: &[&str],
    features: &[&str],
) -> Result<Manifest> {
    let mut manifest = Manifest::from_path(path)
        .with_context(|| format!("failed to load manifest for {}", name))?;

    // Remove extra dependencies
    for dep in dependencies.iter().copied() {
        manifest.dependencies.remove(dep);
    }

    // Remove extra features
    for feature in features.iter().copied() {
        manifest.features.remove(feature);
    }

    Ok(manifest)
}

fn write_toml(name: &str, path: &Path, manifest: &Manifest) -> Result<()> {
    fs2::write(
        path,
        // FIXME: cargo_toml is broken, make it not
        toml::to_string_pretty(&manifest)
            .with_context(|| format!("failed to render toml for {}", name))?,
    )
    .with_context(|| {
        format!(
            "failed to write edited manifest for {} at {}",
            name,
            path.display(),
        )
    })
}
