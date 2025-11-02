param([switch]$Loopback)
$ErrorActionPreference = 'Stop'
Push-Location (Split-Path $PSScriptRoot -Parent)
try {
    cargo fmt --all -- --check
    if ($LASTEXITCODE -ne 0) { throw 'Formatting check failed' }
    cargo clippy --locked --offline --all-targets -j 2 -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'Clippy failed' }
    cargo test --locked --offline --all-targets -j 2
    if ($LASTEXITCODE -ne 0) { throw 'Tests failed' }
    if ($Loopback) {
        cargo test --locked --offline --lib -j 2 proxy::tests -- --ignored --test-threads=1
        if ($LASTEXITCODE -ne 0) { throw 'Loopback integration tests failed' }
    }
} finally { Pop-Location }
