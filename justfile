set shell := ["bash", "-euo", "pipefail", "-c"]

cargo := "cargo"
toolchain := env_var_or_default("RUSTUP_TOOLCHAIN", "stable")
mutants_args := env_var_or_default("MUTANTS_ARGS", "--file src/workflow/transaction.rs --file src/workflow/transaction/commit.rs --file src/workflow/transaction/file_io.rs")
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

# Tests that read the live project data root (PROFLOW_DATA, default ./data).
# The production config and licensed Bible corpora are not tracked, so these
# tests are #[ignore]d in the deterministic gates. On macOS every ignored lib
# test belongs to this set. Off macOS one executor test that asserts native
# TextKit font evidence is ignored too, and is skipped here because no data root
# can make it pass. Skips loudly when no data root is present.
live-data:
    root="${PROFLOW_DATA:-data}"; \
    if [[ ! -f "$root/proflow.config.json" ]]; then \
      echo "skipped live-data: no project data root at $root (set PROFLOW_DATA)"; \
      exit 0; \
    fi; \
    skip=(); \
    if [[ "$(uname -s)" != Darwin ]]; then skip=(--skip workflow::execute::tests::); fi; \
    {{ cargo }} test -p proflow --lib --all-features -- --ignored ${skip[@]+"${skip[@]}"}

# Native ProPresenter fidelity of the proflow side. Codec, package, and schema
# fidelity gates (`just parity`, `just proto`, `just parity-corpus`,
# `just parity-library`) live in the shared `../propresenter` repository.
parity:
    {{ cargo }} run --quiet --features dev-tools --bin parity_smoke >/dev/null

# External 1.0 gate after importing a generated playlist into the current
# ProPresenter release and exporting its saved-back form. Pass both thumbnail
# directories to enable the optional decoded-pixel visual oracle.
release-audit generated saved_back approved_thumbnails="" actual_thumbnails="":
    if [[ -n "{{ approved_thumbnails }}" || -n "{{ actual_thumbnails }}" ]]; then \
      [[ -n "{{ approved_thumbnails }}" && -n "{{ actual_thumbnails }}" ]] || { echo "both thumbnail directories are required" >&2; exit 2; }; \
      {{ cargo }} run --quiet --features dev-tools --bin release_audit -- "{{ generated }}" "{{ saved_back }}" "{{ approved_thumbnails }}" "{{ actual_thumbnails }}"; \
    else \
      {{ cargo }} run --quiet --features dev-tools --bin release_audit -- "{{ generated }}" "{{ saved_back }}"; \
    fi

# Fast local gate for active editing.
local: fmt structure check clippy test

# Normal completion gate.
ci: local doctest doc audit deny machete

# Heavy completion gate for invariant-sensitive changes.
deep: ci parity prop mutants invariants live-data

# Show the local workspace build storage. No files are removed.
cache-status:
    @if [ -d target ]; then du -sh target; else echo "No local build cache."; fi
    @if [ -d target/debug ]; then du -sh target/debug; fi
    @if [ -d target/debug/incremental ]; then du -sh target/debug/incremental; fi

# Clear local development artifacts only when they exceed the budget in GiB.
# Cargo holds its build lock during cleanup. Release artifacts are retained.
cache-trim budget_gib="8":
    @budget={{quote(budget_gib)}}; \
        [[ "$budget" =~ ^[1-9][0-9]{0,3}$ ]] || { echo "Budget must be 1 to 9999 GiB." >&2; exit 2; }; \
        if [ ! -d target/debug ]; then echo "No local development cache."; exit 0; fi; \
        size_kib=$(du -sk target/debug | awk '{print $1}'); \
        if (( size_kib > budget * 1024 * 1024 )); then \
            cargo clean --profile dev --target-dir "$PWD/target"; \
        else \
            echo "Local development cache is within the $budget GiB budget."; \
        fi

# Check handwritten source and test sizes against the repository limits.
structure:
    python3 scripts/audit_structure.py
