set windows-shell := ["powershell", "-NoProfile", "-Command"]

# Show available recipes
default:
    @just --list

# Run the backend with the built frontend next to the binary
backend: backend-build frontend-build assets
    -./target/release/stream-tools.exe

# Build the backend in release mode
backend-build:
    cargo build --release -p stream-tools

# Build the frontend into crates/app/assets/app
frontend-build:
    pnpm --filter @stream-tools/frontend run build

# Copy the static assets next to the binary
assets:
    New-Item -ItemType Directory -Force target/release/assets | Out-Null
    Copy-Item -Recurse -Force crates/app/assets/* target/release/assets/

# Lint and test the workspace
check:
    cargo fmt --all --check
    cargo clippy --workspace --release -- -D warnings
    cargo test --workspace
    pnpm --filter @stream-tools/frontend run check
    pnpm --filter @stream-tools/frontend run test
    pnpm exec biome check .

# Install workspace dependencies
ext-setup:
    pnpm install
