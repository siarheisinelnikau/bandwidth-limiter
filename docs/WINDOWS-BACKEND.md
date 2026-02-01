# Windows networking boundaries and the experimental backend

## What is real in this repository

[src/platform.rs](../src/platform.rs) calls `GetExtendedTcpTable`, `GetExtendedUdpTable`, `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` and `QueryFullProcessImageNameW`. It reads TCP/UDP endpoints for both IP versions and resolves executable identities. IP Helper endpoints do not themselves expose a per-process byte rate. See Microsoft's [TCP table API](https://learn.microsoft.com/en-us/windows/win32/api/iphlpapi/nf-iphlpapi-getextendedtcptable) and [UDP table API](https://learn.microsoft.com/en-us/windows/win32/api/iphlpapi/nf-iphlpapi-getextendedudptable).

TCP EStats can supply per-connection counters when enabled. They are disabled by default; enabling them with `SetPerTcpConnectionEStats` requires administrative privileges. This application does not enable or misrepresent them as available. See [GetPerTcpConnectionEStats](https://learn.microsoft.com/en-us/windows/win32/api/iphlpapi/nf-iphlpapi-getpertcpconnectionestats) and [SetPerTcpConnectionEStats](https://learn.microsoft.com/en-us/windows/win32/api/iphlpapi/nf-iphlpapi-setpertcpconnectionestats).

[src/proxy.rs](../src/proxy.rs) contains the enforcement source for the cooperative TCP backend. It waits before writing data, with per-executable directional budgets shared between flows. Reading from upstream can already fill Windows socket buffers; reduced delivery then creates TCP backpressure. Consequently this is application-delivery pacing with receive bursts, not a strict cap on inbound wire traffic. Build it with the normal Cargo/package commands in the README. It needs no installation: launch the portable application and explicitly configure the client to use SOCKS5 after validation.

## Why a system-wide slider would be misleading

Windows Filtering Platform provides a filter engine and kernel callouts; ordinary permit/block filters are not a rate shaper. Microsoft describes [WFP architecture and callouts](https://learn.microsoft.com/en-us/windows/win32/fwp/about-windows-filtering-platform). User-mode filter management also has a privilege model; local administrators can add filters. A per-application control that simply installs block filters would interrupt traffic, not pace it.

qWAVE exposes outgoing-rate operations on application-managed flows. That is not an arbitrary foreign-process inbound shaping API. See [QOSSetFlow](https://learn.microsoft.com/en-us/windows/win32/api/qos2/nf-qos2-qossetflow). Policy-based QoS likewise does not turn an unprivileged GUI into a general inbound TCP/UDP shaper.

For a transparent backend, our engineering conclusion is that a privileged broker and a signed WFP callout driver (or a separately reviewed equivalent packet interception component) are needed. Stream inspection supports deferral/injection, while packet and datagram processing have separate ownership and completion requirements. See [stream inspection](https://learn.microsoft.com/en-us/windows-hardware/drivers/network/stream-inspection) and [callout processing](https://learn.microsoft.com/en-us/windows-hardware/drivers/network/using-a-callout-for-deep-inspection). Even a driver cannot prevent packets from physically arriving from the network; inbound delivery shaping relies on buffering/backpressure or packet-level strategies.

## Separate component: experimental source now included

The repository includes [a C WFP callout driver, Rust SCM broker and shared control library](../backend/README.md). Version 2 associates selected executable TCP flows at ALE, then clones, absorbs, queues and asynchronously injects both directions against independent shared upload/download budgets. Receive-only DEFER cannot limit outgoing streams; it is removed from this driver rather than mixed with OOB forwarding. Strict service policies refuse unsupported fields. Ordinary block filters are not the pacing mechanism.

The GUI, shared control and Rust service build and their fixtures pass. The System TCP (WFP) page now offers manual status and administrator-authorized transient Apply/Pause through a bounded local pipe with SCM server identity verification. It does not install/start/elevate anything. No real pipe exchange or driver/service startup has occurred. No `.sys` was produced because `km/fwpsk.h`/WDK are absent; kernel behavior remains unverified. [Manual VM build/install/removal instructions](WFP-SETUP.md) are tied to the WDK project; [lifecycle notes](WFP-DESIGN.md) explain queue ownership, failure bounds and quiesce. Policy changes close tracked connections. Incoming OOB delivery pacing does not suppress ACKs and may exhaust its bounded queue; this prototype is not production enforcement.

A production backend still needs the following work:

1. A successful WDK build and code analysis, followed by Driver Verifier and lifecycle tests in an isolated VM. Clone/injection ownership, ordering, concurrent deletion, transaction rollback, BFE restart, memory pressure and partial unregister failures remain unverified kernel paths.
2. Actual IPC ACL/token/server-identity/cancellation tests with ordinary, UAC-filtered and elevated clients, including endpoint spoofing, stalled clients and late results. Current authentication evidence is compilation and pure dispatcher fixtures only.
3. Measured upload/download behavior, receive overflow/large-indication handling and low-rate latency, plus a separate UDP design. Whole-indication credit is at least 64 KiB, so the quarter-second figure alone does not describe burst/delay bounds.
4. Driver-backed Call mode, quotas, schedules, persistence and appropriate telemetry, with honest payload/wire accounting and coexistence behavior.
5. A reviewed signed production package and explicit installer/uninstaller. The manual VM staging instructions are not a production installer, and portable GUI self-updates do not update kernel components.

No service or driver should be installed merely to test the currently available explicit proxy.
