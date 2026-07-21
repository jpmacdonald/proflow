//! Build the native macOS text-layout oracle used by production rendering.

use std::env;
use std::error::Error;
use std::path::PathBuf;
use std::process::Command;

fn main() -> Result<(), Box<dyn Error>> {
    const SOURCE: &str = "tools/text-fit-oracle/main.swift";
    println!("cargo:rerun-if-changed={SOURCE}");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return Ok(());
    }

    let output = PathBuf::from(env::var_os("OUT_DIR").ok_or("Cargo did not provide OUT_DIR")?)
        .join("proflow-text-fit-oracle");
    let status = Command::new("xcrun")
        .args(["swiftc", "-warnings-as-errors", "-O", SOURCE, "-o"])
        .arg(&output)
        .status()?;
    if !status.success() {
        return Err(format!("swiftc failed to build {SOURCE} ({status})").into());
    }

    println!(
        "cargo:rustc-env=PROFLOW_TEXT_FIT_ORACLE_PATH={}",
        output.display()
    );
    Ok(())
}
