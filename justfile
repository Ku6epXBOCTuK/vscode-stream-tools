set windows-shell := ["powershell", "-NoProfile", "-Command"]

port := "45890"
install_dir := env_var("USERPROFILE") / "bin" / "stream-tools"

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
    pnpm --filter stream-tools-extension run typecheck
    pnpm --filter stream-tools-extension run test
    pnpm exec biome check .

# Compile the extension
ext-build:
    pnpm --filter stream-tools-extension run compile
    Copy-Item LICENSE extension/LICENSE -Force

# Build the .vsix package
ext-package: ext-build
    pnpm --filter stream-tools-extension run package

# Install the .vsix into the portable VS Code
ext-install: ext-package
    C:/VSCode/VSCodeKX/bin/code.cmd --install-extension extension/stream-tools-extension.vsix --force

# Install workspace dependencies
ext-setup:
    pnpm install

# Install the binary and its assets into ~/bin/stream-tools
install: backend-build frontend-build assets
    try { Invoke-RestMethod -Method Post "http://127.0.0.1:{{port}}/api/shutdown" -TimeoutSec 3 | Out-Null } catch { }; $gone = $false; foreach ($attempt in 1..60) { if (-not (Get-Process -Name stream-tools -ErrorAction SilentlyContinue)) { $gone = $true; break }; Start-Sleep -Milliseconds 100 }; if (-not $gone) { Get-Process -Name stream-tools -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep -Milliseconds 500 }
    New-Item -ItemType Directory -Force "{{install_dir}}/assets" | Out-Null
    Copy-Item -Recurse -Force target/release/assets/* "{{install_dir}}/assets/"
    $ok = $false; foreach ($attempt in 1..20) { try { Copy-Item -Force target/release/stream-tools.exe "{{install_dir}}/stream-tools.exe" -ErrorAction Stop; $ok = $true; break } catch { Start-Sleep -Milliseconds 300 } }; if (-not $ok) { throw "cannot replace {{install_dir}}/stream-tools.exe: close the running process and retry" }

# Ask the running backend to exit
stop:
    try { Invoke-RestMethod -Method Post "http://127.0.0.1:{{port}}/api/shutdown" -TimeoutSec 3 | Out-Null } catch { }; $gone = $false; foreach ($attempt in 1..60) { if (-not (Get-Process -Name stream-tools -ErrorAction SilentlyContinue)) { $gone = $true; break }; Start-Sleep -Milliseconds 100 }; if (-not $gone) { Get-Process -Name stream-tools -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep -Milliseconds 500 }

# Print the health payload of the running backend
health:
    Invoke-RestMethod "http://127.0.0.1:{{port}}/api/health" | ConvertTo-Json -Depth 6

# Show the last lines of the backend log
logs:
    if (Test-Path "{{install_dir}}/stream-tools.log") { Get-Content "{{install_dir}}/stream-tools.log" -Tail 80 } else { "no log yet: {{install_dir}}/stream-tools.log" }
