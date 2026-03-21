param([switch]$Offline)
$ErrorActionPreference = 'Stop'
Push-Location (Split-Path $PSScriptRoot -Parent)
try {
    $manifest = Get-Content -LiteralPath Cargo.toml -Raw
    $version = [regex]::Match($manifest, '(?m)^version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"').Groups[1].Value
    if (-not $version) { throw 'Cannot read package version' }
    $cargoArguments = @('build', '--release', '--locked', '--target', 'x86_64-pc-windows-msvc', '-j', '2')
    if ($Offline) { $cargoArguments += '--offline' }
    & cargo @cargoArguments
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed' }
    New-Item -ItemType Directory -Path dist -Force | Out-Null
    $artifact = "BandwidthLimiter-v$version.exe"
    Copy-Item -LiteralPath 'target/x86_64-pc-windows-msvc/release/bandwidth-limiter.exe' -Destination (Join-Path dist $artifact)
    Copy-Item -LiteralPath '../rust-auto-updator/LICENSE' -Destination 'dist/LICENSE-rust-auto-updator.txt' -Force
    $hash = (Get-FileHash -LiteralPath (Join-Path dist $artifact) -Algorithm SHA256).Hash.ToLowerInvariant()
    [System.IO.File]::WriteAllText((Join-Path (Get-Location) 'dist/SHA256SUMS'), "$hash  $artifact`n", [System.Text.UTF8Encoding]::new($false))
    Write-Output "Built dist/$artifact and dist/SHA256SUMS. No upload was performed."
} finally { Pop-Location }
