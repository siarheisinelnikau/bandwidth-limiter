# Verification record

Session date: 2026-10-31 (Europe/Minsk). Windows x64 host, Rust 1.96.1 / Cargo 1.96.1. All repository commands used `E:/work/github/bandwidth-limiter`, a local target directory and at most two Cargo jobs. No GUI was launched.

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
- The experimental WFP receive-only component now has source; its WDK build, signing and kernel testing remain pending. See the continuation record below and `WFP-SETUP.md`.

No actual traffic was throttled and no Windows networking configuration, service, driver, device or external user file was changed in this development session.

## WFP source continuation — 2026-10-31

Added an independent C callout driver, shared IOCTL/budget headers, WDK project and Rust SCM broker. The desktop application has not been connected to it; existing GUI capabilities and the earlier GUI artifact remain unchanged.

Completed safely:

- `cargo check --locked --offline --manifest-path backend/service/Cargo.toml --all-targets -j 2` passed on Windows x64.
- `scripts/check-wfp.ps1`: service formatting, Clippy with `-D warnings`, and four tests passed. Tests cover C/Rust layout, strict unsupported-policy rejection, duplicate/relative targets, and synthetic SYSTEM/Administrators/Users SIDs. No service entry point, device open, BFE call or event-log write was executed by these tests.
- With the existing MSVC x64 compiler in the command process's environment, `scripts/check-wfp.ps1 -SharedC` compiled the shared C fixture with `/W4 /WX` and ran it successfully. Eight synthetic flows share one budget; the fixture also checks burst bounds, backward time, maximum-rate arithmetic, ABI sizes and invalid configuration. Only repository-local artificial files were used.
- `scripts/build-wfp.ps1 -Offline` built `dist/wfp/BandwidthLimiterService.exe` in release mode. The ancillary broker EXE is not a portable GUI release asset and was not executed.
- `scripts/build-wfp.ps1 -Offline -Driver` built the service, then stopped explicitly because `C:/Program Files (x86)/Windows Kits/10/Include/10.0.26100.0/km/fwpsk.h` is missing. Available SDK headers are user-mode/shared headers; no WDK download or install was attempted. **The driver was not compiled and no `.sys` was produced.**

Kernel API research used the Microsoft sources linked in `WFP-DESIGN.md`; only the freestanding accounting header was compiled as C. This does not validate driver syntax, linkage, DPC safety, WFP behavior or unload correctness. Required next checks are listed in that document. No SCM service/driver registration, signing, certificate/trust change, device access, network/filter operation or GUI launch occurred during this continuation.
