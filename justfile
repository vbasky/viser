# viser developer tasks — `just` command runner

_default:
    @just --list

# Build the workspace
build:
    cargo build

# Build release with LTO
build-release:
    cargo build --release

# Run all tests
test:
    cargo test

# Run tests with release optimizations (faster)
test-release:
    cargo test --release

# Run clippy with workspace lints
lint:
    cargo clippy --workspace --all-targets

# Format all code
fmt:
    cargo fmt

# Check formatting without changing files
fmt-check:
    cargo fmt --check

# Build documentation
docs:
    cargo doc --no-deps --document-private-items

# Open docs in browser
docs-open: docs
    open target/doc/viser_ffmpeg/index.html

# Run per-title analysis
per-title input resolutions codecs:
    cargo run --release -p viser-cli -- per-title analyze -i {{input}} --resolutions {{resolutions}} --codecs {{codecs}}

# Run per-shot analysis
per-shot input target-bitrate:
    cargo run --release -p viser-cli -- per-shot analyze -i {{input}} --target-bitrate {{target-bitrate}}

# Run segment-level CRF adaptation
per-segment input target-vmaf:
    cargo run --release -p viser-cli -- per-segment analyze -i {{input}} --target-vmaf {{target-vmaf}}

# Launch comparison player
compare reference encoded vmaf-data:
    cargo run --release -p viser-cli -- compare --reference {{reference}} --encoded {{encoded}} --vmaf-data {{vmaf-data}}

# Run all checks: fmt, clippy, test, doc
check-all: fmt-check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test
    cargo doc --no-deps --document-private-items

# Release a version end-to-end: bump → commit → tag → push → GitHub release → publish
# Usage: just release 0.2.0   (tree must be clean, on master, gh authenticated)
release version:
    ./scripts/release.sh {{version}}

# Clean build artifacts
clean:
    cargo clean

# Update dependencies
update:
    cargo update

# Run cargo audit (requires: cargo install cargo-audit)
audit:
    cargo audit

# Run cargo deny (requires: cargo install cargo-deny)
deny-check:
    cargo deny check
