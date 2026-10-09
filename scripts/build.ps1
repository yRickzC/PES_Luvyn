$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location (Join-Path $projectRoot 'ui')
try {
    npm ci
    if ($LASTEXITCODE -ne 0) { throw 'npm ci failed' }
    npm run build
    if ($LASTEXITCODE -ne 0) { throw 'UI build failed' }
} finally { Pop-Location }
Push-Location $projectRoot
try {
    cargo test --workspace --features desktop --locked
    if ($LASTEXITCODE -ne 0) { throw 'Tests failed' }
    cargo build --release --features desktop --locked
    if ($LASTEXITCODE -ne 0) { throw 'Rust build failed' }
} finally { Pop-Location }

