param([switch]$Driver, [switch]$Offline, [string]$SdkVersion = '10.0.26100.0')
$ErrorActionPreference = 'Stop'
Push-Location (Split-Path $PSScriptRoot -Parent)
try {
    $cargoArguments = @('build', '--locked', '--release', '--target', 'x86_64-pc-windows-msvc', '--manifest-path', 'backend/service/Cargo.toml', '-j', '2')
    if ($Offline) { $cargoArguments += '--offline' }
    & cargo @cargoArguments
    if ($LASTEXITCODE -ne 0) { throw 'WFP service compilation failed' }
    New-Item -ItemType Directory -Path dist/wfp -Force | Out-Null
    Copy-Item -LiteralPath 'target/x86_64-pc-windows-msvc/release/bandwidth-limiter-service.exe' -Destination 'dist/wfp/BandwidthLimiterService.exe'
    Copy-Item -LiteralPath 'backend/policy.example.json' -Destination 'dist/wfp/policy.example.json'
    $artifactReadme = (Get-Content -LiteralPath 'backend/README.md' -Raw).Replace('(../docs/WFP-SETUP.md)', '(WFP-SETUP.md)').Replace('(../docs/WFP-DESIGN.md)', '(WFP-DESIGN.md)')
    [System.IO.File]::WriteAllText((Join-Path (Get-Location) 'dist/wfp/README.md'), $artifactReadme, [System.Text.UTF8Encoding]::new($false))
    Copy-Item -LiteralPath 'docs/WFP-SETUP.md' -Destination 'dist/wfp/WFP-SETUP.md'
    Copy-Item -LiteralPath 'docs/WFP-DESIGN.md' -Destination 'dist/wfp/WFP-DESIGN.md'
    if ($Driver) {
        $kernelHeader = "${env:ProgramFiles(x86)}/Windows Kits/10/Include/$SdkVersion/km/fwpsk.h"
        if (-not (Test-Path -LiteralPath $kernelHeader)) { throw "WDK is unavailable: missing $kernelHeader. Service built; no driver binary was produced." }
        $msbuild = Get-Command msbuild.exe -ErrorAction SilentlyContinue
        if (-not $msbuild) { throw 'Run from an existing x64 Native Tools/WDK developer shell with MSBuild in PATH. No toolchain will be installed.' }
        & $msbuild.Source 'backend/driver/BandwidthLimiterWfp.vcxproj' '/m:2' '/p:Configuration=Release' '/p:Platform=x64' "/p:WindowsTargetPlatformVersion=$SdkVersion"
        if ($LASTEXITCODE -ne 0) { throw 'WDK driver build failed; do not install any stale binary' }
        Copy-Item -LiteralPath 'target/wfp-driver/Release/BandwidthLimiterWfp.sys' -Destination 'dist/wfp/BandwidthLimiterWfp.sys'
    }
    $checksumFiles = @('BandwidthLimiterService.exe')
    if ($Driver) { $checksumFiles += 'BandwidthLimiterWfp.sys' }
    $checksumLines = foreach ($name in $checksumFiles) { "$((Get-FileHash -LiteralPath (Join-Path 'dist/wfp' $name) -Algorithm SHA256).Hash.ToLowerInvariant())  $name" }
    [System.IO.File]::WriteAllText((Join-Path (Get-Location) 'dist/wfp/SHA256SUMS'), ($checksumLines -join "`n") + "`n", [System.Text.UTF8Encoding]::new($false))
    Write-Output 'Built WFP artifacts only. No registration, signing, installation or execution was performed.'
} finally { Pop-Location }
