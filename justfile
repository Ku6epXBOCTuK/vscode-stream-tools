set windows-shell := ["powershell", "-NoProfile", "-Command"]

# Show available recipes
default:
    @just --list

# Run the backend (release build: audio encoding is too slow in debug)
backend: backend-build
    -./target/release/stream-tools.exe

# Build the backend in release mode
backend-build:
    cargo build --release -p stream-tools

# Lint and test the workspace
check:
    cargo fmt --all --check
    cargo clippy --workspace --release -- -D warnings
    cargo test --workspace
    pnpm exec biome check .

# Install workspace dependencies
ext-setup:
    pnpm install
