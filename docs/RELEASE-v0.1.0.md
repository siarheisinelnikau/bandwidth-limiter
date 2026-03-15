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
- System TCP (WinDivert) controls with manual service status and authenticated administrator Apply/Pause over a distinct version 3 local endpoint; no automatic connection, installation, startup or elevation.
- Rust packet service with protected runtime loading, NO_INSTALL WinDivert 2.2 binding, new-flow executable attribution, IPv4/IPv6 TCP parsing, shared directional payload budgets and bounded FIFO queues. Native loading/traffic tests remain pending; external signed DLL/driver are not bundled.
- Retained custom WFP v2 driver/broker sources and safe legacy VM recovery instructions. The current GUI uses WinDivert instead; no custom kernel build/signing is required by its user-mode build.

## Known limitations and validation

The explicit proxy requires SOCKS5 configuration in each client. The new System TCP backend requires a separately prepared, compatible signed WinDivert runtime and service; it has not been loaded or tested with real packets. It shapes only newly observed identifiable selected TCP, allowing old/unmatched/unsupported traffic to pass. Queues can overflow/expire, low caps can delay whole packets, and Pause/reapply can release bursts/reorder/drop packets without deliberately aborting TCP. Payload counters include retransmissions. Apply is transient; service restart begins paused. UDP/QUIC and System TCP quotas/schedules/Call mode remain unsupported. The legacy custom driver remains uncompiled. Native input/tray/idle performance, real shaping and updater replacement still require validation. Quotas are not tamper-resistant; a forced exit can lose the last checkpoint interval, and a stalled OS DNS resolver can delay proxy shutdown.

No GUI (including self-capture), service or driver was started during the WinDivert migration. No networking or updater operation occurred. Two loopback integration tests remain ignored after the earlier sandbox Winsock 10013 error. The README images are historical pre-WinDivert captures; current screenshots and visual QA were deferred under the user's no-GUI restriction. See `docs/VERIFICATION.md` and `docs/UI-VERIFICATION.md` for exact evidence and limits.

Publication is not part of this session. Canonical repository/release availability is not assumed; update-check failures are shown as errors.
