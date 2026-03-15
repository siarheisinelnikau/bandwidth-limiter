param([switch]$Offline)
$ErrorActionPreference = 'Stop'
Push-Location (Split-Path $PSScriptRoot -Parent)
try {
    $arguments = @('build','--locked','--release','--target','x86_64-pc-windows-msvc','--manifest-path','backend/packet-service/Cargo.toml','-j','2')
    if ($Offline) { $arguments += '--offline' }
    & cargo @arguments
    if ($LASTEXITCODE -ne 0) { throw 'WinDivert packet service build failed' }
    New-Item -ItemType Directory -Path dist/windivert -Force | Out-Null
    Copy-Item -LiteralPath target/x86_64-pc-windows-msvc/release/bandwidth-limiter-packet-service.exe -Destination dist/windivert/BandwidthLimiterPacketService.exe
    New-Item -ItemType Directory -Path dist/windivert/docs -Force | Out-Null
    Copy-Item -LiteralPath docs/WINDIVERT-SETUP.md,docs/WINDIVERT-DESIGN.md,docs/WFP-SETUP.md,docs/WFP-DESIGN.md,docs/THIRD-PARTY.md -Destination dist/windivert/docs
    $readme = 'BandwidthLimiterPacketService.exe is a user-mode broker, not a complete driver runtime. Start with docs/WINDIVERT-SETUP.md. WinDivert.dll and WinDivert64.sys are deliberately not downloaded, bundled, installed or loaded by this build. The service starts paused and uses NO_INSTALL only after authenticated Apply.'
    [System.IO.File]::WriteAllText((Join-Path (Get-Location) 'dist/windivert/README.txt'),$readme,[System.Text.UTF8Encoding]::new($false))
    $hash=(Get-FileHash -LiteralPath dist/windivert/BandwidthLimiterPacketService.exe -Algorithm SHA256).Hash.ToLowerInvariant()
    [System.IO.File]::WriteAllText((Join-Path (Get-Location) 'dist/windivert/SHA256SUMS'),"$hash  BandwidthLimiterPacketService.exe`n",[System.Text.UTF8Encoding]::new($false))
    Write-Output 'Built packet service only. No runtime download, installation, startup or traffic capture occurred.'
} finally { Pop-Location }
