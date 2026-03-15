# Build and deliberately stage the WinDivert backend

These commands are for later review/use in an isolated Windows x64 VM. **None of the installation/start/traffic steps was executed during development.** The GUI and service build, but the external runtime, actual IPC, process attribution and real rates are unverified. No hardware-program account or custom driver build is needed to evaluate an accepted, unchanged official signed WinDivert runtime.

## Build without installing or running anything

Use the existing Rust 1.92+ MSVC x64 toolchain and C++ build tools. WDK, WinDivert headers/import libraries, an EV certificate and administrator rights are not build prerequisites. If dependencies are not cached, explicitly fetch them first; all recorded checks/builds used cached dependencies.

```powershell
cargo fetch --locked
cargo fetch --locked --manifest-path backend/packet-service/Cargo.toml
./scripts/check.ps1
./scripts/check-windivert.ps1
./scripts/package.ps1 -Offline
./scripts/build-windivert.ps1 -Offline
```

Outputs: `dist/BandwidthLimiter-v0.1.0.exe`, GUI checksum, and `dist/windivert/BandwidthLimiterPacketService.exe` with its checksum and these instructions. The build/check scripts do not download a DLL/driver, register services, elevate, start executables except fixture test binaries, or capture packets. They do not package an operational WinDivert runtime.

## Review the external runtime first

Obtain the unchanged x64 `WinDivert.dll` and `WinDivert64.sys` from the [official WinDivert release](https://github.com/basil00/WinDivert/releases/tag/v2.2.2). The bindings target ABI 2.2; use the reviewed 2.2.2 pair, not an arbitrary DLL from PATH or another application. The upstream distribution is signed, but the exact artifact must be checked for current kernel trust/load acceptance, architecture and compatibility in the VM. Authenticode success alone does not certify all these properties. Do not restore blocked/quarantined binaries, add exclusions or disable host protections merely to make this prototype load.

Keep artifact hashes, signer/catalog/timestamp evidence, OS build and original source/license provenance. A future redistributable package must satisfy the chosen LGPLv3 terms, including notices/license texts/corresponding-source availability and compatible library replacement rights. No third-party runtime is currently bundled; [third-party notes](../docs/THIRD-PARTY.md) explain that boundary. Modifying/rebuilding the driver requires a new acceptable signature and defeats this reuse strategy. Review [backend design](WINDIVERT-DESIGN.md) before staging.

## Protected staging, only after review in the VM

Take a snapshot and ensure an expendable client/server and independent observer are ready. In an elevated VM PowerShell, use a new, absent product directory. `$brokerSource` and `$runtimeSource` below must be absolute directories containing the reviewed files; do not execute service code from a user-writable download directory.

```powershell
$brokerSource = 'C:\src\bandwidth-limiter\dist\windivert'
$runtimeSource = 'C:\Reviewed\WinDivert-2.2.2-A\x64'
$install = Join-Path $env:ProgramFiles 'BandwidthLimiterPacket'
New-Item -ItemType Directory -Path $install -ErrorAction Stop
Copy-Item -LiteralPath (Join-Path $brokerSource 'BandwidthLimiterPacketService.exe') -Destination $install
Copy-Item -LiteralPath (Join-Path $runtimeSource 'WinDivert.dll') -Destination $install
Copy-Item -LiteralPath (Join-Path $runtimeSource 'WinDivert64.sys') -Destination $install
icacls.exe $install /inheritance:r /grant:r '*S-1-5-18:(OI)(CI)(F)' '*S-1-5-32-544:(OI)(CI)(F)'
icacls.exe $install /setowner '*S-1-5-32-544' /T
```

Check exit codes and actual ACLs on the directory and all three files; retain protected Program Files ancestors. Do not reuse unknown explicit ACEs/reparse points. The service refuses Apply outside the fixed product directory or with ordinary-account mutation rights/owners. Standard users communicate through the separate pipe; they do not need read/write access to these runtime binaries.

## Manual driver/service registration

The product service name below must be absent. The upstream driver uses the standard `WinDivert` device/service namespace; first inspect whether another application already owns/uses it. **Do not replace, reconfigure, stop or delete an existing unrelated WinDivert registration.** A matching already running reviewed runtime may be reused only after compatibility review; the following driver create/start commands assume a new isolated VM with no such registration.

```powershell
sc.exe create WinDivert type= kernel start= demand binPath= (Join-Path $install 'WinDivert64.sys')
sc.exe start WinDivert
$serviceCommand = '"' + (Join-Path $install 'BandwidthLimiterPacketService.exe') + '" --service'
sc.exe create BandwidthLimiterPacketService type= own start= demand obj= LocalSystem depend= BFE binPath= $serviceCommand
sc.exe start BandwidthLimiterPacketService
sc.exe query BandwidthLimiterPacketService
```

Starting the packet service alone creates only its control endpoint and remains paused. It never installs/starts the driver. All its WinDivert opens use `NO_INSTALL`, so Apply fails if the device is not already available. Services are demand-start; no boot autostart or certificate/trust setup is supplied. A blocked kernel signature requires a reviewed upstream resolution or another backend, not disabling user-host protection.

## GUI workflow and traffic checks

1. Launch the GUI deliberately in the VM. **System TCP (WinDivert) → Check service** reads a snapshot without loading a driver. An ordinary/UAC-filtered client is read-only; Apply/Pause require an elevated administrator-authorized client. The GUI provides no elevation or service-start button.
2. In Applications, save enabled full EXE paths and download/upload caps in KiB/s. Zero in one direction means unlimited. Disable/remove unsupported enabled quotas/schedules and turn off proxy Call mode before System TCP Apply. Do not route the same test traffic through the proxy as well.
3. Choose **Apply saved TCP limits**, then create/reconnect the selected applications' TCP connections. Only newly observed, attributable flows are capped. Initial/missed/unidentified packets, old connections, loopback, UDP/QUIC, impostor traffic and unsupported packet forms can bypass shaping; the GUI shows unmatched/unknown passes instead of claiming complete interception.
4. Verify both directions and their simultaneous aggregate behavior over one/many connections/apps, IPv4/IPv6 and several rates with an independent server. Counters are selected successfully reinjected TCP payload including retransmissions, not unique delivered bytes or adapter rates. Whole-packet bursts/low-rate latency and receive buffering must be measured. See the design for queue/expiry/resource limits.
5. **Pause System TCP** releases queued traffic without pacing and closes handles; no TCP abort is deliberately issued. Pause/reapply/stop can still produce a burst, reordering, dropped queued packets or retransmissions. Check errors/counters and use a disposable test client. Check service after a timeout before retrying a command that may still finish.

Status is manual and internally coalesced at 200 ms. Apply is transient; service restart begins paused. GUI Save alone does not apply a policy. GUI Quit stops only the proxy and leaves the separate service active. There is no driver-backed quota/schedule/Call mode, UDP relay or tamper-resistant enforcement claim.

## Stop, remove and migrate

```powershell
sc.exe stop BandwidthLimiterPacketService
sc.exe query BandwidthLimiterPacketService
```

Wait for STOPPED, inspect Application events/errors and verify capture stopped. Only for a driver registration created by this VM procedure, after confirming no other WinDivert clients/handles depend on it:

```powershell
sc.exe stop WinDivert
sc.exe query WinDivert
```

Only after both are confirmed STOPPED, and only for these deliberately created registrations:

```powershell
sc.exe delete BandwidthLimiterPacketService
sc.exe delete WinDivert
```

Remove only the reviewed product directory after checking it contains no unrelated files, or revert the VM snapshot. No automatic uninstall/deletion script is included. Update the broker/runtime while stopped, recheck signatures/ACLs and restart manually. Portable GUI self-updates do not replace broker/DLL/driver files.

For a VM that previously loaded this project's experimental WFP driver, first use its [legacy drain/quiesce/unload procedure](WFP-SETUP.md). Never remove active OOB WFP filters directly or run both backends concurrently on the same test flows. The new GUI deliberately cannot control the old v2 pipe; new v3 policy/response identities are incompatible. Neither migration nor legacy cleanup is performed automatically.
