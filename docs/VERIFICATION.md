# Verification record

Session date: 2026-10-31 (Europe/Minsk). Windows x64 host, Rust 1.96.1 / Cargo 1.96.1. All repository commands used `E:/work/github/bandwidth-limiter`, a local target directory and at most two Cargo jobs. No native GUI was launched. Records below are chronological; the final WFP v2 section supersedes earlier backend scope, artifact and test counts.

## Completed checks

- Dependency resolution with `cargo generate-lockfile --offline`; all required dependencies were cached locally.
- `cargo check --offline --all-targets -j 2` passed, including the native Windows bindings and GUI source.
- `cargo clippy --locked --offline --all-targets -j 2 -- -D warnings` passed.
- `cargo test --offline --all-targets -j 2`: 13 tests passed, two socket integration tests ignored. These cover shared multi-flow budgets, live cap changes, call-mode precedence, bidirectional quota reservation, midnight reset, schedule boundaries/overnight windows, path validation, fragmented SOCKS5 requests, unsupported commands/authentication, atomic settings round trips, corruption preservation, exclusive locks, semantic releases, canonical asset URLs, checksum matching and PE architecture checks.
- Isolated persistence fixtures use `.test-output` inside this repository. The original OS temporary-directory fixture encountered Access Denied on Windows replacement under the sandbox; the in-repository fixture passed.

## Blocked socket checks

Both loopback integration tests were attempted. The sandbox returned Winsock **10013 / PermissionDenied** on socket connection. No elevation, firewall change or other workaround was attempted. The tests remain explicit opt-in tests instead of silently reporting success:

```powershell
cargo test --locked --offline --lib -j 2 proxy::tests -- --ignored --test-threads=1
```

One sends only a tiny request/response between an isolated loopback test endpoint and proxy, including half-close and byte accounting. The other rejects an unavailable fixture process identity. The tests inject a fixture identity rather than enumerating or changing real application traffic. GitHub CI is configured to execute them on its ordinary Windows runner; no CI run has occurred in this local session.

## Release build

`./scripts/package.ps1 -Offline` successfully built and packaged the optimized Windows x64 executable with two jobs. Output: `dist/BandwidthLimiter-v0.1.0.exe` (7,613,440 bytes) plus `dist/SHA256SUMS`. The package was not launched. A static PE inspection confirmed AMD64 / PE32+, Windows GUI subsystem and no VCRUNTIME/MSVCP DLL imports. Windows system DLLs and OpenGL are used. Packaging does not imply live validation.

The final `./scripts/check.ps1` passed formatting, Clippy and all 13 non-socket tests after the final source changes. Cargo emitted environmental incremental-cache hard-link warnings and copied the cache files instead; these did not fail any check. No shared target/cache configuration was changed. SHA-256 is provided alongside the local binary rather than fixed in this source document.

## Deferred manual checks

- Launch the packaged EXE on Windows 10 and 11 x64; inspect all screens, resizing, 100/150/200% DPI, keyboard focus, clipping and readable status/error messages.
- Capture genuine main-window and settings screenshots for the README. They are required before documenting the GUI as visually verified.
- Measure idle and active CPU for the packaged executable, including minimized state. Check reactive wakeups, graphics presentation and long-running resource use. The event-loop adapter has compiled but has not been observed.
- Exercise actual Windows process attribution, protected/terminated processes, multiple executable helpers, IPv4/IPv6 TCP and UDP inventory, and table refresh races. Current Windows API source was compiled only; no real process inventory was collected.
- Configure an expendable SOCKS5-capable client and an isolated test server. Verify upload and download payload rates, multiple connections, caps changed mid-flow, quota exhaustion/refunds, schedule changes, midnight, Call mode and disabled/removed rules. Compare payload counters against server-side measurements and account for TCP buffering.
- Verify stop/close during handshake, DNS resolution, connect, rate waits and half-closed flows. Check the 32-flow bound and no accepted LAN connections. Standard DNS lookup cancellation is a known limit.
- On disposable application copies only, test missing/unpublished releases, rate limits, TLS failures, opt-out causing zero startup requests, partial downloads, wrong hashes, wrong PE architecture, user cancellation before restart, helper readiness, replacement, non-writable directories, recovery backup and settings preservation. No updater network request, helper or executable replacement was exercised here.
- Complete a public-distribution dependency notice bundle, signing review and real release publication via the authorized GitHub plugin when separately requested.
- The experimental WFP component now has bidirectional source; its WDK build, signing and kernel testing remain pending. See the final continuation record below and `WFP-SETUP.md`.

No actual traffic was throttled and no Windows networking configuration, service, driver, device or external user file was changed in this development session.

## Initial WFP v1 source continuation — 2026-10-31 (historical)

Added an independent C callout driver, shared IOCTL/budget headers, WDK project and Rust SCM broker. The desktop application has not been connected to it; existing GUI capabilities and the earlier GUI artifact remain unchanged.

Completed safely:

- `cargo check --locked --offline --manifest-path backend/service/Cargo.toml --all-targets -j 2` passed on Windows x64.
- `scripts/check-wfp.ps1`: service formatting, Clippy with `-D warnings`, and four tests passed. Tests cover C/Rust layout, strict unsupported-policy rejection, duplicate/relative targets, and synthetic SYSTEM/Administrators/Users SIDs. No service entry point, device open, BFE call or event-log write was executed by these tests.
- With the existing MSVC x64 compiler in the command process's environment, `scripts/check-wfp.ps1 -SharedC` compiled the shared C fixture with `/W4 /WX` and ran it successfully. Eight synthetic flows share one budget; the fixture also checks burst bounds, backward time, maximum-rate arithmetic, ABI sizes and invalid configuration. Only repository-local artificial files were used.
- `scripts/build-wfp.ps1 -Offline` built `dist/wfp/BandwidthLimiterService.exe` in release mode. The ancillary broker EXE is not a portable GUI release asset and was not executed.
- `scripts/build-wfp.ps1 -Offline -Driver` built the service, then stopped explicitly because `C:/Program Files (x86)/Windows Kits/10/Include/10.0.26100.0/km/fwpsk.h` is missing. Available SDK headers are user-mode/shared headers; no WDK download or install was attempted. **The driver was not compiled and no `.sys` was produced.**

Kernel API research used the Microsoft sources linked in `WFP-DESIGN.md`; only the freestanding accounting header was compiled as C. This does not validate driver syntax, linkage, DPC safety, WFP behavior or unload correctness. Required next checks are listed in that document. No SCM service/driver registration, signing, certificate/trust change, device access, network/filter operation or GUI launch occurred during this continuation.

## CPU UI and design continuation — 2026-10-31

The desktop's renderer and layout have changed since the initial build record above. Current code uses egui CPU rasterization and Win32 GDI presentation, without eframe/OpenGL/wgpu. Application selection/editing, Connections and Settings are separated; a tray menu and named-event reopening path were added. None of the native window/tray paths was launched. The previous GUI artifact has been replaced by a freshly packaged `dist/BandwidthLimiter-v0.1.0.exe` (7,389,184 bytes) and matching SHA256SUMS.

Formatting, Clippy, 13 library tests and three scheduler/raster tests passed (16 passed, two socket tests ignored). The packaged executable then ran only its finite headless fixture branch, producing actual client-area PNGs without a native window, worker, update request or user settings. All five final PNGs were inspected and the main/settings images are now embedded in the README. The earlier screenshot deferral is superseded **only for these application-rendered client areas**; OS chrome, tray, input and normal runtime performance remain unverified. PE inspection found AMD64 GUI subsystem and no OpenGL/D3D/DXGI/VCRUNTIME/MSVCP imports. Full evidence and the incomplete native runtime gate are recorded in [UI-VERIFICATION.md](UI-VERIFICATION.md).

## WFP v2 GUI control and upload continuation — 2026-10-31 (current)

The GUI now has manual System TCP (WFP) service controls. GUI/service share a strict version 2 policy and framed local pipe protocol, overlapped cancellation/deadlines, SCM server PID verification and token-based administrator authorization for mutations. Apply is in-memory; startup still uses protected JSON. The driver source now clones/absorbs/paces/injects both stream directions with separate shared budgets. Kernel ABI v2 is 528-byte configuration / 96-byte status; mixed versions are incompatible. Pause/reapply/stop/lease loss close tracked connections before removing filters.

Completed checks after the final source changes:

- `scripts/check.ps1`: formatting and Clippy with `-D warnings` passed; **14 library + 3 renderer/scheduler tests passed**, two socket tests remain ignored. The added GUI fixture refuses unsupported WFP quotas/schedules/Call mode instead of sending a partial policy.
- `scripts/check-wfp.ps1`: control and service formatting/Clippy passed; **3 shared control + 4 service tests passed**. They cover strict envelope/size/rate/rule/path validation, upload-to-kernel conversion, C/Rust layouts, synthetic SIDs and a fake controller that rejects unauthorized changes and invalid policies before mutation. A new regression test exposed Serde ignoring extra fields on unit commands; commands now use empty struct variants, and the test passes. These tests create no real pipe and do not impersonate an actual client.
- The freestanding shared C fixture compiled with existing MSVC `/W4 /WX` and passed: eight artificial flows share independent 4 KiB/s download and 8 KiB/s upload budgets, yielding 40/80 KiB in ten seconds. It also checks whole-64-KiB admission at 1 KiB/s after 64 seconds, overflow bounds and v2 ABI. This compiles **only the shared headers**, not `driver.c`.
- `scripts/package.ps1 -Offline` and `scripts/build-wfp.ps1 -Offline` passed. Current local artifacts are `dist/BandwidthLimiter-v0.1.0.exe` (7,481,856 bytes) and `dist/wfp/BandwidthLimiterService.exe` (576,000 bytes), with SHA256SUMS. The GUI's static PE inspection again found AMD64 / Windows GUI subsystem and no OpenGL/D3D/DXGI/VCRUNTIME/MSVCP imports.
- Seven finite actual application-generated fixture renders were inspected, including artificial WFP connected/unavailable states. The current GUI hash matches [invocation metadata](screenshots/invocation.json); all images/manifests were refreshed in `docs/screenshots`. Capture used no native window, ordinary workers, normal settings, service connection, inventory or update request. It exited successfully; native idle/hidden/input/tray performance remains unmeasured.

The WDK kernel header remains absent. The driver was **not compiled, signed, loaded or tested**, and no `.sys` is supplied as verified. No real named-pipe exchange, SCM identity/authentication test, driver device access, BFE/filter mutation or upload/download measurement occurred. Compilation and fake authorization fixtures do not establish the real privileged boundary. The broker executable was not started. Nothing was installed or published; all changes are local commits and computer use was not used.

Material experimental limits: 64 KiB whole-indication pacing can delay low-rate delivery; receive OOB pacing does not suppress ACKs and can overflow the bounded queue; oversize/resource/injection failures close affected tracked TCP connections; failure to allocate a flow context before association leaves that flow unshaped and increments a counter. Required WDK, Driver Verifier, IPC and VM traffic/lifecycle checks are listed in [WFP-DESIGN.md](WFP-DESIGN.md). [WFP-SETUP.md](WFP-SETUP.md) contains manual future VM staging/recovery commands; none were executed.
