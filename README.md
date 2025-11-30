# BandwidthLimiter

A Windows x64 desktop utility for explicit, per-application TCP bandwidth policies and call-friendly presets. Version 0.1.0 is an early implementation with a real SOCKS5 relay and a testable policy engine. It is independent of, and not affiliated with, NetLimiter. The implementation and interface are original.

**Desktop scope:** enforcement in the portable GUI applies only to TCP connections an application sends through its loopback SOCKS5 proxy. Applications must support SOCKS5 themselves; the GUI does not change Windows proxy, firewall, or QoS settings. Separate [experimental WFP driver/service sources](backend/README.md) now implement an incoming-TCP shaping path without SOCKS. The broker builds, but the driver is uncompiled/unvalidated, not installed, and not connected to the GUI. The desktop's system-wide enforcement status remains **UNAVAILABLE**.

## Capabilities

| Area | Implemented behavior | Boundary |
| --- | --- | --- |
| Upload and download caps | Aggregate payload pacing per executable and direction across all its proxy connections | Payload delivery / TCP backpressure, not an adapter-level receive cap |
| Application identity | Windows IP Helper maps the local connecting socket to its PID; a read-only process query resolves its full executable path | Unknown, protected and unmatched processes are refused; paths are not a security boundary |
| Call mode | Caps each enabled application marked **Background during calls**; preserves stricter base caps | Applies only to proxied TCP; no automatic call or microphone detection |
| Daily quota | Combined download + upload budget with partial-chunk reservation and local-date reset | Checkpointed every five seconds and at normal shutdown; crash/power loss can lose the last checkpoint interval |
| Weekly schedules | Selected weekdays, local times, overnight windows and all-day windows | Base caps become unlimited outside the window; call caps and quotas remain active |
| Activity | Successfully forwarded proxy payload bytes and sampled rates; daily reserved bytes | No fabricated rates for direct traffic |
| Windows inventory | Read-only IPv4/IPv6 TCP/UDP endpoints, PID, process path, state | No global per-process byte counters; unavailable identity is shown explicitly |
| Experimental WFP sources | Separate Rust SCM broker and C callout driver for executable-specific incoming TCP deferral/continuation | WDK build and VM validation pending; no GUI controls, upload, UDP, quotas or schedules in this backend |
| Updates | Background startup check with saved opt-out; manual check; download progress; SHA-256 verification; restart/replacement helper | Repository/release may be unpublished; replacement and live network behavior require manual validation |

## Build and package

Requires Windows 10/11 x64, Rust 1.92+ with the `x86_64-pc-windows-msvc` toolchain and existing Visual Studio C++ build tools. No WDK, driver, service or administrator access is required for the explicit proxy. Other platforms are not supported distributions.

```powershell
cargo build --locked -j 2
./scripts/check.ps1
./scripts/package.ps1 -Offline
```

If dependencies have not already been cached, omit `-Offline` when packaging and run `cargo fetch --locked` before the offline checks. Cargo is restricted to two jobs and the repository-local `target` directory. The package script creates `dist/BandwidthLimiter-v0.1.0.exe` and `dist/SHA256SUMS`. It does not launch the executable or publish anything. The Windows release statically links the CRT where supported; all fonts are embedded.

## Use after manual validation

1. Start the portable EXE. Open **Rules & schedules** and add the full path to an application executable, or use **Refresh → Add rule** in the Windows inventory.
2. Set caps in **KiB/s** (1024 bytes per second) and a combined daily quota in **MiB**. Zero means unlimited. Mark background download applications for Call mode. Save changes.
3. Start the loopback proxy in **Traffic**. In the selected application's own proxy settings, choose SOCKS5, host `127.0.0.1`, port `1088` (or the saved port). The proxy does not autostart on launch.
4. Route a small test transfer through it. Compare **Measured proxy activity** with the policy. Toggle **Call mode** to apply the call caps. This changes existing flows at the next payload operation without altering saved rules.
5. Stop the proxy or close BandwidthLimiter to close its active connections. Remove the SOCKS setting in the client application to restore direct connectivity.

Only SOCKS5 `CONNECT` is supported. UDP associate, BIND, QUIC, DNS-over-UDP forwarding and transparent capture are not implemented. SOCKS domain names resolve on the proxy's worker, with no TLS interception or payload inspection. Incoming proxy connections use IPv4 loopback; destination connections support IPv4 and IPv6. An application that uses separate downloader/helper executables needs a rule for each actual connecting executable. There are at most 32 concurrent proxy flows. This is a cooperative utility, not a mechanism to prevent an application from bypassing a policy.

Equal schedule start/end times mean all day on the selected weekdays. End times are exclusive. For Sunday 23:00–Monday 01:00, select Sunday with `23:00` to `01:00`. Local clock/timezone changes affect calendar schedules and quota dates; pacing uses a monotonic clock.

Settings and daily usage live under `%LOCALAPPDATA%/BandwidthLimiter`, separately from the executable. Edits take effect only after a successful save. A corrupt or unsupported settings file is preserved and produces a startup error. A data-directory file lock prevents concurrent instances. Call mode and the proxy start in the off state each launch. Closing X exits the application; there is no tray/hide-on-close behavior.

## Updates

**Settings & updates** shows the installed version, **Check for updates**, and the persistent **Check for updates on startup** toggle (on by default). Save the toggle to make it apply on the next launch. Disabled means no startup update request. Startup checks run on one background worker; no-update results and transient errors stay in Settings, while a newer version gets an inline notice.

The planned canonical repository is [siarheisinelnikau/bandwidth-limiter](https://github.com/siarheisinelnikau/bandwidth-limiter). A missing repository/release or HTTP failure is an error, never an up-to-date result. Stable semantic versions and the exact compatible Windows x64 asset name are required. Nothing has been published by this development session.

**Download and install** downloads only after a click, limits the size, and requires a GitHub API SHA-256 digest or a matching `SHA256SUMS` entry. The downloaded file must be a Windows x64 PE executable. **Restart to finish update** prepares a copy of the current executable as a helper, waits for a readiness acknowledgement and normal app exit, preserves a `.previous.exe` backup, atomically replaces the EXE and relaunches it. Data stays in LocalAppData. Errors before replacement preserve the installed EXE; helper errors are written to `updates/last-update-error.txt`. A non-writable installation directory requires a manually managed update; no elevation is requested. SHA-256 from the same release detects corruption; it does not provide independent publisher authentication. Release signing is future work.

## Verification and screenshots

See [verification status](docs/VERIFICATION.md), [architecture](docs/ARCHITECTURE.md), [Windows API constraints](docs/WINDOWS-BACKEND.md), and [draft release notes](docs/RELEASE-v0.1.0.md).

For the new backend see [WFP source status](backend/README.md), [design/lifecycle review](docs/WFP-DESIGN.md), and [manual VM build/install/removal instructions](docs/WFP-SETUP.md). `scripts/build-wfp.ps1` only builds; it does not install or launch anything. Driver/service updates are separate from portable GUI updates.

**Real main-window and settings screenshots are pending.** The unattended session explicitly forbids launching GUI applications or capturing screenshots. No mock screenshots or broken image links are included. Window layout, DPI, graphics, idle CPU, real application identity, real transfers, and update replacement remain subject to manual validation.

Licensed under [MIT](LICENSE). Dependency licenses remain their own; see [third-party notes](docs/THIRD-PARTY.md).
