# Build and manually stage the experimental WFP backend

These are **instructions for later review in a disposable Windows VM**, not steps executed by the development agent. The driver is source-only and uncompiled in the recorded session. Do not install it on a production machine. The commands below create/load services and affect selected TCP traffic when deliberately run by a VM administrator.

## Build without installation

Prerequisites: Windows x64, Rust 1.92+, existing Visual Studio C++ x64 tools, matching Windows SDK/WDK 10.0.26100.0 including `km/fwpsk.h`, the `WindowsKernelModeDriver10.0` MSBuild toolset, and its Spectre-mitigated libraries. The driver uses `ExAllocatePool2`, so its OS target is Windows 10 2004/build 19041 or newer. No toolchain installer is invoked by repository scripts. A newer/older WDK combination must be separately checked; the project permits an explicit SDK version override rather than assuming compatibility.

From the repository, in an existing x64 Native Tools/WDK developer shell:

```powershell
cargo fetch --locked --manifest-path backend/service/Cargo.toml
./scripts/check-wfp.ps1 -SharedC
./scripts/build-wfp.ps1 -Offline -Driver -SdkVersion 10.0.26100.0
```

`check-wfp.ps1` runs only fixtures: Rust policy/ABI/security tests and a freestanding C multi-flow budget test. It does not open the driver, access BFE, register with SCM, or use real networking. The build script compiles the Rust broker, then calls:

```powershell
msbuild.exe backend/driver/BandwidthLimiterWfp.vcxproj /m:2 /p:Configuration=Release /p:Platform=x64 /p:WindowsTargetPlatformVersion=10.0.26100.0
```

Expected output is `dist/wfp/BandwidthLimiterService.exe`, `dist/wfp/BandwidthLimiterWfp.sys` and an example policy. Without WDK, `./scripts/build-wfp.ps1 -Offline` still builds **only the service**; `-Driver` fails with the exact missing-header/toolset error rather than manufacturing a `.sys`. Intermediate files remain under this repository's `target`. Signing, catalog generation, SCM registration and executable startup are never build steps. This is a legacy non-PnP callout driver, with manual SCM staging instead of an INF-based device installer.

## Signing and VM preparation

First obtain a successful WDK build and review warnings, Code Analysis and symbols. Keep the `.pdb` for crash analysis. Windows x64 kernel code integrity requires an acceptable driver signature. Use an appropriately provisioned isolated driver-test VM and an existing test-signing workflow/certificate, or a production-signed package when one exists. **This repository creates no certificates, changes no boot/Secure Boot/test-signing policy and installs no trust roots.** If the VM refuses the signature, stop and fix its test environment; do not disable protections on the user's host. `Get-AuthenticodeSignature` can inspect a file but does not alone prove kernel-loading acceptance.

Take a VM snapshot, prepare kernel debugging and use a small expendable TCP fixture client/server. Before loading the driver, save its hash, build configuration, PDB, OS build and signature evidence. Driver Verifier and any boot-affecting setup belong to that disposable VM's deliberate test procedure. No driver binary or signed package is supplied as verified by this session.

## Protect the installation and policy

In an elevated PowerShell **inside the VM**, copy only reviewed, signed build outputs. Set `$source` to the checkout's absolute `dist/wfp` path; do not use an untrusted download directory for service execution.

```powershell
$source = 'C:\src\bandwidth-limiter\dist\wfp'
$install = Join-Path $env:ProgramFiles 'BandwidthLimiterWfp'
New-Item -ItemType Directory -Path $install -ErrorAction Stop
Copy-Item -LiteralPath (Join-Path $source 'BandwidthLimiterService.exe') -Destination $install
Copy-Item -LiteralPath (Join-Path $source 'BandwidthLimiterWfp.sys') -Destination $install
Copy-Item -LiteralPath (Join-Path $source 'policy.example.json') -Destination (Join-Path $install 'policy.json')
icacls.exe $install /inheritance:r /grant:r '*S-1-5-18:(OI)(CI)(F)' '*S-1-5-32-544:(OI)(CI)(F)'
icacls.exe $install /setowner '*S-1-5-32-544' /T
```

Check every command's exit code. The example assumes a **new, absent** directory, so it cannot inherit stale explicit permissions. Do not reuse a directory with unknown ACEs. Keep the Program Files ancestors protected as well. Change `policy.json` to the **actual local full path** of the disposable client EXE; the sample `C:\Lab\DownloadFixture.exe` is not a shipped application. Rates are per executable, shared over its incoming IPv4/IPv6 TCP streams. Save as UTF-8 without a BOM; only `version`, `rules`, `executable` and `download_kib` are accepted. User-writable configuration locations, UNC/device paths, reparse files and ordinary-account owners are refused.

```powershell
& (Join-Path $install 'BandwidthLimiterService.exe') --validate-policy (Join-Path $install 'policy.json')
```

Validation parses a bounded file only; it does not resolve app IDs or touch BFE/SCM/the driver. The service's runtime ACL checks additionally require SYSTEM/Administrators-only mutation rights on both file and directory. Adjusting the policy requires an administrator; there is no desktop IPC or policy auto-reload.

## Register and start only in the test VM

The names below must be absent before first registration. Do not replace existing unrelated services. The broker uses LocalSystem and depends on BFE plus this driver; both product services are demand-start, not automatically enabled at boot.

```powershell
sc.exe create BandwidthLimiterWfp type= kernel start= demand binPath= (Join-Path $install 'BandwidthLimiterWfp.sys')
$serviceCommand = '"' + (Join-Path $install 'BandwidthLimiterService.exe') + '" --service'
sc.exe create BandwidthLimiterService type= own start= demand obj= LocalSystem depend= BFE/BandwidthLimiterWfp binPath= $serviceCommand
sc.exe start BandwidthLimiterWfp
sc.exe start BandwidthLimiterService
sc.exe query BandwidthLimiterService
```

Confirm RUNNING and examine Windows Application events for `BandwidthLimiterService` if startup fails. Error event strings are recorded without registering a custom message DLL; Event Viewer may say the description resource is absent while showing the raw diagnostic. Service-specific exit code 1 means initialization/heartbeat/drain failed. Stop immediately if Driver Verifier reports a fault. The portable GUI is not involved and must not claim driver enforcement.

Create **new** TCP transfers in the selected fixture after activation. Existing connections are not retroactively associated. Validate aggregate rates with one and multiple connections, IPv4/IPv6, zero-token start, 250 ms burst allowance, FIN/RST and slow readers. Upload and UDP should remain unchanged. Keep an independent observer/server measurement; admitted payload counts are not wire-byte totals.

## Stop, recover and remove

```powershell
sc.exe stop BandwidthLimiterService
sc.exe query BandwidthLimiterService
```

Wait for STOPPED. Normal stop sends DRAIN, waits for all deferred streams/continuations, removes only owned WFP objects, then releases flow contexts. The driver stays loaded but drained. To change rules, replace the protected policy while stopped, restart the service and reconnect clients. After a failed/crashed broker, this explicit administrative command repeats the guarded recovery path:

```powershell
& (Join-Path $install 'BandwidthLimiterService.exe') --recover
& (Join-Path $install 'BandwidthLimiterService.exe') --status
```

These commands require the broker to be stopped because its device handle is exclusive. A status-only open also closes that administrative handle afterward; cleanup requests drain if a stale policy was armed. There is no live status endpoint for other processes yet.

Before unloading the driver:

```powershell
& (Join-Path $install 'BandwidthLimiterService.exe') --prepare-unload
sc.exe stop BandwidthLimiterWfp
sc.exe query BandwidthLimiterWfp
```

**Check that prepare-unload succeeded and returned state 3 (QUIESCED), then that the driver is STOPPED.** QUIESCE unregisters callouts and enables unload only after filters, flow contexts, deferred data and continuations are gone. It prevents further configurations until driver reload. Partial failure returns an error and state 4 (QUIESCING); retry preparation, inspect diagnostics and keep the VM snapshot available. Never force-remove deferred stream filters or unload a busy driver. Drain timeout deliberately retains the filters. If a kernel failure prevents safe recovery, use the disposable VM recovery procedure rather than forcing unload.

Only after both product services are confirmed STOPPED may the VM administrator remove their registrations:

```powershell
sc.exe delete BandwidthLimiterService
sc.exe delete BandwidthLimiterWfp
```

Remove only the explicitly staged `BandwidthLimiterWfp` directory after confirming it contains no unrelated data, or revert the VM snapshot. No recursive deletion or automatic uninstall script is supplied. Driver/service updates require the same drain/quiesce/stop sequence before replacing their files. The GUI's portable EXE updater does not update them.
