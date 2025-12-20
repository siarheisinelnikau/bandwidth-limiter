param([string]$OutputDirectory, [string]$Executable)
$ErrorActionPreference = 'Stop'
Push-Location (Split-Path $PSScriptRoot -Parent)
try {
    if (-not $Executable) { $Executable = Join-Path (Get-Location) 'dist/BandwidthLimiter-v0.1.0.exe' }
    $Executable = (Resolve-Path -LiteralPath $Executable).Path
    if (-not $OutputDirectory) { $OutputDirectory = Join-Path (Get-Location) ('.test-output/preview-' + [guid]::NewGuid().ToString('N')) }
    if (-not [System.IO.Path]::IsPathRooted($OutputDirectory)) { throw 'Output directory must be absolute' }
    if (Test-Path -LiteralPath $OutputDirectory) { throw 'Use a new output directory to avoid stale preview files' }
    $start = [System.Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $Executable; $start.WorkingDirectory = (Get-Location).Path
    $start.UseShellExecute = $false; $start.CreateNoWindow = $true
    $start.WindowStyle = [System.Diagnostics.ProcessWindowStyle]::Hidden
    $start.RedirectStandardError = $true
    $start.ArgumentList.Add('--render-preview'); $start.ArgumentList.Add($OutputDirectory)
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    $capture = [System.Diagnostics.Process]::Start($start)
    if (-not $capture.WaitForExit(25000)) { $capture.Kill($true); $capture.WaitForExit(); throw "The isolated preview PID $($capture.Id) exceeded 25 seconds and was stopped" }
    $failure = $capture.StandardError.ReadToEnd()
    if ($capture.ExitCode -ne 0) { throw "Headless preview failed: $failure" }
    $metadata = @{ executable_sha256=(Get-FileHash -LiteralPath $Executable -Algorithm SHA256).Hash.ToLowerInvariant(); pid=$capture.Id; exit_code=$capture.ExitCode; wall_ms=$watch.ElapsedMilliseconds; cpu_ms=$capture.TotalProcessorTime.TotalMilliseconds; mode='headless fixtures; no native window' }
    [System.IO.File]::WriteAllText((Join-Path $OutputDirectory 'invocation.json'), ($metadata | ConvertTo-Json), [System.Text.UTF8Encoding]::new($false))
    Write-Output $OutputDirectory
} finally { Pop-Location }
