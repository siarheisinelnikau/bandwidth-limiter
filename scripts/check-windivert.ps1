param()
$ErrorActionPreference = 'Stop'
Push-Location (Split-Path $PSScriptRoot -Parent)
try {
    foreach ($manifest in @('backend/packet-control/Cargo.toml','backend/packet-service/Cargo.toml')) {
        & cargo fmt --manifest-path $manifest -- --check
        if ($LASTEXITCODE -ne 0) { throw "Formatting failed: $manifest" }
        & cargo clippy --locked --offline --manifest-path $manifest --all-targets -j 2 -- -D warnings
        if ($LASTEXITCODE -ne 0) { throw "Clippy failed: $manifest" }
        & cargo test --locked --offline --manifest-path $manifest -j 2
        if ($LASTEXITCODE -ne 0) { throw "Fixture tests failed: $manifest" }
    }
    Write-Output 'Fixture checks only. No WinDivert DLL, driver, SCM service or real traffic was opened.'
} finally { Pop-Location }
