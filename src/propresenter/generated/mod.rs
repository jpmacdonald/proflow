//! Generated protobuf modules.
//!
//! These modules are auto-generated from ProPresenter's protobuf definitions.
//! Only exposing the modules we need for our implementation.

// Suppress warnings for auto-generated code
#![allow(missing_docs, clippy::all, clippy::pedantic, clippy::nursery)]

/// The `rv_data` module contains generated protobuf types used throughout the app.
///
/// Keep the dead-code allowance because we intentionally generate a much larger schema
/// than the subset we currently construct directly, and keep rustfmt off the module
/// declaration so `cargo fmt` does not churn the generated file on stable toolchains.
/// Regenerate `rv_data.rs` with `cargo run --manifest-path tools/proto-gen/Cargo.toml`.
#[allow(dead_code, unused_imports)]
#[rustfmt::skip]
pub mod rv_data;
