use anyhow::{bail, Context, Result};
use prost_build::Config;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

fn main() -> Result<()> {
    let check_only = env::args().skip(1).any(|arg| arg == "--check");

    let repo_root = repo_root()?;
    let proto_dir = repo_root.join("crates/propresenter/proto");
    let generated_dir = repo_root.join("crates/propresenter/src/generated");
    let target_file = generated_dir.join("rv_data.rs");
    let fds_file = repo_root.join("target/schema-audit/proflow.pb");

    let temp = tempdir().context("create temporary output directory")?;
    let generated = generate_rv_data(&proto_dir, temp.path(), &fds_file)?;

    if check_only {
        let current = fs::read(&target_file)
            .with_context(|| format!("read current generated file {}", target_file.display()))?;
        let regenerated = fs::read(&generated)
            .with_context(|| format!("read regenerated file {}", generated.display()))?;

        if current != regenerated {
            bail!(
                "Generated protobufs are out of date: {}\nRun `cargo run --manifest-path tools/proto-gen/Cargo.toml` to regenerate.",
                target_file.display()
            );
        }

        println!("Generated protobufs are up to date.");
        return Ok(());
    }

    fs::copy(&generated, &target_file)
        .with_context(|| format!("write regenerated protobufs to {}", target_file.display()))?;

    println!("Regenerated {}", target_file.display());
    Ok(())
}

fn repo_root() -> Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
        .context("resolve repository root from tool manifest path")
}

fn proto_files(proto_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files: Vec<_> = fs::read_dir(proto_dir)
        .with_context(|| format!("read proto directory {}", proto_dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "proto"))
        .collect();
    files.sort();
    Ok(files)
}

fn generate_rv_data(proto_dir: &Path, out_dir: &Path, fds_file: &Path) -> Result<PathBuf> {
    let protoc = protoc_bin_vendored::protoc_bin_path().context("locate vendored protoc")?;
    env::set_var("PROTOC", protoc);

    if let Some(parent) = fds_file.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create descriptor set directory {}", parent.display()))?;
    }

    let mut config = Config::new();
    config.out_dir(out_dir);
    config.type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]");
    config.file_descriptor_set_path(fds_file);

    let protos = proto_files(proto_dir)?;
    config
        .compile_protos(&protos, &[proto_dir.to_path_buf()])
        .context("compile ProPresenter protobuf definitions")?;

    let generated = out_dir.join("rv.data.rs");
    if !generated.is_file() {
        bail!(
            "Expected generated file {} was not produced",
            generated.display()
        );
    }

    Ok(generated)
}
