# BandwidthLimiter v0.1.0 — draft, not published

First implementation of an original Windows bandwidth-policy utility. This release line has no preceding published release. The package targets Windows 10/11 x64 with a portable executable named `BandwidthLimiter-v0.1.0.exe` and a `SHA256SUMS` file.

## Added

- Native desktop interface for executable-specific upload/download caps, combined daily quotas and weekly schedules, including overnight windows.
- A loopback SOCKS5 TCP relay with Windows process attribution and aggregate per-application pacing across multiple connections. Only enabled executable rules are accepted.
- Call mode that lowers caps for selected background applications while preserving stricter existing limits.
- Measured proxy payload counters and a separate read-only Windows IPv4/IPv6 TCP/UDP connection inventory. Direct-traffic byte rates are explicitly unavailable.
- Persistent settings and daily usage checkpoints, a data-directory instance lock, and preservation of invalid settings for inspection.
- Background startup update checks with a saved opt-out, manual checks, checksum-verified downloads with progress, and a user-initiated restart/replacement helper.
- Offline policy/protocol/storage/updater fixtures, opt-in loopback integration tests, Windows CI and local packaging scripts.
- A simplified Applications editor, separate Connections view and unified Settings page, with a shared visual palette and clear saved/draft state.
- CPU rasterization with Win32 GDI presentation, reactive redraw scheduling and a compact tray menu. Native runtime performance and tray behavior are still unverified.
- Finite headless rendering of real application components to PNG, plus frame-removal and scheduling regression tests and opt-in native frame diagnostics.

## Known limitations and validation

Desktop enforcement requires the client application to use SOCKS5. Separate experimental incoming-TCP WFP driver and broker sources have been added, but the driver is uncompiled/unvalidated and not integrated into the GUI. WFP upload/UDP, driver-backed quotas/schedules/Call mode, UDP relay, QUIC and historical traffic reports remain absent. Tray/restore, native input and idle/hidden frame behavior require manual validation. Download limits pace delivery/backpressure and do not strictly cap network-adapter arrival. A forced exit may lose up to the last usage checkpoint interval; quotas are not tamper-resistant. A stalled OS DNS resolver can delay shutdown.

The unattended build session did not open native GUI windows, operate real applications, install drivers/services, alter networking, or execute the updater. Loopback integration tests were attempted but blocked by sandbox Winsock error 10013. Real application-generated client-area PNGs now cover preview fixtures, without desktop access or native presentation. Live Windows identity/shaping, native graphics/idle behavior and update replacement remain pending. See `docs/VERIFICATION.md` and `docs/UI-VERIFICATION.md` for exact evidence and its limits.

Publication is not part of this session. Canonical repository/release availability is not assumed; update-check failures are shown as errors.
