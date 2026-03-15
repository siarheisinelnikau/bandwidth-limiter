# Windows networking boundaries and the experimental backend

## What is real in this repository

[src/platform.rs](../src/platform.rs) calls `GetExtendedTcpTable`, `GetExtendedUdpTable`, `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` and `QueryFullProcessImageNameW`. It reads TCP/UDP endpoints for both IP versions and resolves executable identities. IP Helper endpoints do not themselves expose a per-process byte rate. See Microsoft's [TCP table API](https://learn.microsoft.com/en-us/windows/win32/api/iphlpapi/nf-iphlpapi-getextendedtcptable) and [UDP table API](https://learn.microsoft.com/en-us/windows/win32/api/iphlpapi/nf-iphlpapi-getextendedudptable).

TCP EStats can supply per-connection counters when enabled. They are disabled by default; enabling them with `SetPerTcpConnectionEStats` requires administrative privileges. This application does not enable or misrepresent them as available. See [GetPerTcpConnectionEStats](https://learn.microsoft.com/en-us/windows/win32/api/iphlpapi/nf-iphlpapi-getpertcpconnectionestats) and [SetPerTcpConnectionEStats](https://learn.microsoft.com/en-us/windows/win32/api/iphlpapi/nf-iphlpapi-setpertcpconnectionestats).

[src/proxy.rs](../src/proxy.rs) contains the enforcement source for the cooperative TCP backend. It waits before writing data, with per-executable directional budgets shared between flows. Reading from upstream can already fill Windows socket buffers; reduced delivery then creates TCP backpressure. Consequently this is application-delivery pacing with receive bursts, not a strict cap on inbound wire traffic. Build it with the normal Cargo/package commands in the README. It needs no installation: launch the portable application and explicitly configure the client to use SOCKS5 after validation.

## Why a system-wide slider would be misleading

Windows Filtering Platform provides a filter engine and kernel callouts; ordinary permit/block filters are not a rate shaper. Microsoft describes [WFP architecture and callouts](https://learn.microsoft.com/en-us/windows/win32/fwp/about-windows-filtering-platform). User-mode filter management also has a privilege model; local administrators can add filters. A per-application control that simply installs block filters would interrupt traffic, not pace it.

qWAVE exposes outgoing-rate operations on application-managed flows. That is not an arbitrary foreign-process inbound shaping API. See [QOSSetFlow](https://learn.microsoft.com/en-us/windows/win32/api/qos2/nf-qos2-qossetflow). Policy-based QoS likewise does not turn an unprivileged GUI into a general inbound TCP/UDP shaper.

For a transparent backend, our engineering conclusion is that a privileged broker and a signed WFP callout driver (or a separately reviewed equivalent packet interception component) are needed. Stream inspection supports deferral/injection, while packet and datagram processing have separate ownership and completion requirements. See [stream inspection](https://learn.microsoft.com/en-us/windows-hardware/drivers/network/stream-inspection) and [callout processing](https://learn.microsoft.com/en-us/windows-hardware/drivers/network/using-a-callout-for-deep-inspection). Even a driver cannot prevent packets from physically arriving from the network; inbound delivery shaping relies on buffering/backpressure or packet-level strategies.

## Current System TCP component: WinDivert

The [packet service](../backend/README.md) now dynamically binds an unchanged external signed WinDivert 2.2 runtime. Its Rust worker captures/reinjects packets, correlates new FLOW tuples with process paths and applies independent upload/download payload budgets across selected connections. GUI Check/Apply/Pause use a new authenticated local endpoint. All WinDivert opens specify NO_INSTALL; service startup/status do not load a DLL or driver. The user no longer needs to produce/sign this project's own kernel module for this path, subject to verifying the exact upstream runtime and its distribution terms.

User-mode source compiles and pure fixtures pass. External runtime loading, actual pipe identity/tokens, attribution races, packet behavior and rates remain untested. Only newly observed identifiable TCP is shaped; unmatched/old/unsupported traffic can bypass limits. Whole packets, retransmissions, finite queues and pause flushing affect rates/latency. UDP/QUIC, quotas, schedules and System TCP Call mode are unsupported. See [design and evidence limits](WINDIVERT-DESIGN.md) and [manual VM staging](WINDIVERT-SETUP.md). No driver/service was installed or started.

## Legacy custom WFP source

The repository includes [a C WFP callout driver, Rust SCM broker and shared control library](../backend/README.md). Version 2 associates selected executable TCP flows at ALE, then clones, absorbs, queues and asynchronously injects both directions against independent shared upload/download budgets. Receive-only DEFER cannot limit outgoing streams; it is removed from this driver rather than mixed with OOB forwarding. Strict service policies refuse unsupported fields. Ordinary block filters are not the pacing mechanism.

The earlier GUI/shared control/Rust broker compiled and their fixtures passed, but the current GUI no longer talks to that v2 pipe. No `.sys` was produced because `km/fwpsk.h`/WDK are absent; custom kernel behavior remains unverified. [Legacy VM recovery/unload instructions](WFP-SETUP.md) and [lifecycle notes](WFP-DESIGN.md) remain relevant if this source is ever staged in an old test VM. Its OOB policy changes close tracked connections. This is separate from the new WinDivert packet lifecycle.

A production release of that legacy custom-driver backend would still need the following work:

1. A successful WDK build and code analysis, followed by Driver Verifier and lifecycle tests in an isolated VM. Clone/injection ownership, ordering, concurrent deletion, transaction rollback, BFE restart, memory pressure and partial unregister failures remain unverified kernel paths.
2. Actual IPC ACL/token/server-identity/cancellation tests with ordinary, UAC-filtered and elevated clients, including endpoint spoofing, stalled clients and late results. Current authentication evidence is compilation and pure dispatcher fixtures only.
3. Measured upload/download behavior, receive overflow/large-indication handling and low-rate latency, plus a separate UDP design. Whole-indication credit is at least 64 KiB, so the quarter-second figure alone does not describe burst/delay bounds.
4. Driver-backed Call mode, quotas, schedules, persistence and appropriate telemetry, with honest payload/wire accounting and coexistence behavior.
5. A reviewed signed production package and explicit installer/uninstaller. The manual VM staging instructions are not a production installer, and portable GUI self-updates do not update kernel components.

No service or driver should be installed merely to test the currently available explicit proxy.
