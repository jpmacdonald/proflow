set shell := ["bash", "-euo", "pipefail", "-c"]

cargo := "cargo"
toolchain := env_var_or_default("RUSTUP_TOOLCHAIN", "stable")
mutants_args := env_var_or_default("MUTANTS_ARGS", "--file src/workflow/transaction.rs")
proptest_cases := env_var_or_default("PROPTEST_CASES", "10000")

default:
    @just --list

# Print the active Rust and Cargo toolchain.
version:
    rustc --version
    {{ cargo }} --version

# Verify that optional harness tools are installed.
tools-check:
    @missing=0; \
    for tool in cargo-nextest cargo-audit cargo-deny cargo-mutants cargo-machete; do \
      if ! command -v "$tool" >/dev/null 2>&1; then \
        echo "missing: $tool"; \
        missing=1; \
      fi; \
    done; \
    exit "$missing"

# Install the standard harness tools.
tools-install:
    cargo install --locked cargo-nextest
    cargo install cargo-audit
    cargo install cargo-deny
    cargo install cargo-mutants
    cargo install cargo-machete

# Check formatting without rewriting files.
fmt:
    {{ cargo }} fmt --all -- --check

# Rewrite formatting.
fmt-fix:
    {{ cargo }} fmt --all

# Type-check everything the project exposes.
check:
    {{ cargo }} check --workspace --all-targets --all-features

# Harsh lint gate. Keep this strict in every environment.
clippy:
    {{ cargo }} clippy --workspace --all-features -- -D warnings
    {{ cargo }} clippy --workspace --all-targets --all-features -- -D warnings -A clippy::unwrap_used -A clippy::expect_used -A clippy::panic -A clippy::manual_let_else -A clippy::unreadable_literal -A clippy::needless_raw_string_hashes -A clippy::naive_bytecount -A clippy::float_cmp -A clippy::cast_possible_wrap

# Run the normal Rust test suite.
test:
    if cargo nextest --version >/dev/null 2>&1; then \
      {{ cargo }} nextest run --workspace --all-features; \
    else \
      {{ cargo }} test --workspace --all-features; \
    fi

# Run opt-in Planning Center smoke tests serially; deterministic gates exclude them.
pco-smoke:
    {{ cargo }} test --test planning_center_api --features integration_test -- --ignored --nocapture --test-threads=1

# Run doctests separately so documentation examples cannot rot quietly.
doctest:
    {{ cargo }} test --workspace --doc --all-features

# Build documentation as a compile gate.
doc:
    RUSTDOCFLAGS="-D warnings" {{ cargo }} doc --workspace --all-features --no-deps

# RustSec advisory check.
audit:
    {{ cargo }} audit

# Dependency policy: advisories, licenses, bans, duplicate versions, sources.
deny:
    {{ cargo }} deny check

# Detect unused dependencies.
machete:
    {{ cargo }} machete

# Public API compatibility check for library crates.
semver:
    if cargo semver-checks --version >/dev/null 2>&1; then \
      {{ cargo }} semver-checks check-release; \
    else \
      echo "missing optional tool: cargo-semver-checks"; \
    fi

# High-volume property tests for checked boundary invariants.
prop:
    PROPTEST_CASES={{ proptest_cases }} {{ cargo }} test --workspace --all-features property_

# Mutation testing defaults to the transactional write boundary. Override with
# MUTANTS_ARGS='...' for broader sweeps; mutating every debug binary is not a
# useful completion gate.
mutants:
    {{ cargo }} mutants {{ mutants_args }}

# Project-specific semantic invariants: atomic config persistence, stable workflow
# identity, and package comparison reflexivity.
invariants:
    {{ cargo }} test --workspace --all-features write_project_config_round_trips
    {{ cargo }} test --workspace --all-features expansion_outputs_have_stable_keys_and_respect_declared_type
    {{ cargo }} test --workspace --all-features compare_identical_package_is_compatible

# Native ProPresenter fidelity: schema-exact codecs and an independently
# exported playlist reconstructed through the production writer.
parity:
    {{ cargo }} test --test propresenter_codec_fidelity
    {{ cargo }} test --test propresenter_native_export committed_independent_portable_export_has_complete_media_and_links -- --exact
    {{ cargo }} test --lib propresenter::native_zip::tests::writes_native_forced_zip64_records_and_global_member_order
    {{ cargo }} test --lib propresenter::package::tests::native_package_reconstruction_matches_evidenced_shape
    {{ cargo }} test --lib propresenter::render::slide_instance::tests::every_committed_native_template_instantiates_a_closed_local_graph -- --exact
    {{ cargo }} run --quiet --features dev-tools --bin parity_smoke >/dev/null

# The checked-in prost output must match the authoritative ProPresenter schema.
proto:
    {{ cargo }} run --manifest-path tools/proto-gen/Cargo.toml -- --check

# Audit a local directory of independent native exports without making that
# machine-specific corpus part of the normal repository gate.
parity-corpus corpus_dir:
    PROFLOW_LIVE_PLAYLIST_EXPORT_DIR="{{ corpus_dir }}" {{ cargo }} test --test propresenter_codec_fidelity live_exported_playlist_documents_round_trip_byte_exactly -- --ignored --exact --nocapture
    PROFLOW_LIVE_PLAYLIST_EXPORT_DIR="{{ corpus_dir }}" {{ cargo }} test --test propresenter_native_export native_export_corpus_matches_evidenced_archive_and_media_shape -- --ignored --exact --nocapture

# Audit a workstation library and its raw playlist document in place. The
# tests are read-only and deliberately stay outside deterministic CI.
parity-library library_dir:
    PROFLOW_LIVE_LIBRARY_DIR="{{ library_dir }}" {{ cargo }} test --test propresenter_codec_fidelity live_native_presentations_round_trip_byte_exactly -- --ignored --exact --nocapture
    PROFLOW_LIVE_LIBRARY_DIR="{{ library_dir }}" {{ cargo }} test --test propresenter_codec_fidelity live_playlist_document_round_trips_byte_exactly -- --ignored --exact --nocapture

# Fast local gate for active editing.
local: fmt check clippy test

# Normal completion gate.
ci: local doctest doc audit deny machete

# Heavy completion gate for invariant-sensitive changes.
deep: ci parity proto prop mutants invariants
