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
- Transparent system-wide enforcement needs a separately designed/built/signed/tested WFP component; its implementation is absent. See `WINDOWS-BACKEND.md`.

No actual traffic was throttled and no Windows networking configuration, service, driver, device or external user file was changed in this development session.
