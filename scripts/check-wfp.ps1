param([switch]$SharedC)
$ErrorActionPreference = 'Stop'
Push-Location (Split-Path $PSScriptRoot -Parent)
try {
    cargo fmt --manifest-path backend/service/Cargo.toml -- --check
    if ($LASTEXITCODE -ne 0) { throw 'WFP service formatting check failed' }
    cargo clippy --locked --offline --manifest-path backend/service/Cargo.toml --all-targets -j 2 -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'WFP service Clippy failed' }
    cargo test --locked --offline --manifest-path backend/service/Cargo.toml -j 2
    if ($LASTEXITCODE -ne 0) { throw 'WFP service fixture tests failed' }
    if ($SharedC) {
        if (-not (Get-Command cl.exe -ErrorAction SilentlyContinue)) { throw 'Shared C tests need an existing x64 MSVC developer shell' }
        New-Item -ItemType Directory -Path .test-output/wfp -Force | Out-Null
        cl.exe /nologo /W4 /WX /TC backend/tests/budget_test.c /Fe.test-output/wfp/budget-test.exe /Fo.test-output/wfp/budget-test.obj
        if ($LASTEXITCODE -ne 0) { throw 'Shared C fixture compilation failed' }
        & '.\.test-output\wfp\budget-test.exe'
        if ($LASTEXITCODE -ne 0) { throw 'Shared C synthetic budget tests failed' }
    }
} finally { Pop-Location }
