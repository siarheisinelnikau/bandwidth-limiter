# Windows networking boundaries and the deferred backend

## What is real in this repository

[src/platform.rs](../src/platform.rs) calls `GetExtendedTcpTable`, `GetExtendedUdpTable`, `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` and `QueryFullProcessImageNameW`. It reads TCP/UDP endpoints for both IP versions and resolves executable identities. IP Helper endpoints do not themselves expose a per-process byte rate. See Microsoft's [TCP table API](https://learn.microsoft.com/en-us/windows/win32/api/iphlpapi/nf-iphlpapi-getextendedtcptable) and [UDP table API](https://learn.microsoft.com/en-us/windows/win32/api/iphlpapi/nf-iphlpapi-getextendedudptable).

TCP EStats can supply per-connection counters when enabled. They are disabled by default; enabling them with `SetPerTcpConnectionEStats` requires administrative privileges. This application does not enable or misrepresent them as available. See [GetPerTcpConnectionEStats](https://learn.microsoft.com/en-us/windows/win32/api/iphlpapi/nf-iphlpapi-getpertcpconnectionestats) and [SetPerTcpConnectionEStats](https://learn.microsoft.com/en-us/windows/win32/api/iphlpapi/nf-iphlpapi-setpertcpconnectionestats).

[src/proxy.rs](../src/proxy.rs) contains the enforcement source for the cooperative TCP backend. It waits before writing data, with per-executable directional budgets shared between flows. Reading from upstream can already fill Windows socket buffers; reduced delivery then creates TCP backpressure. Consequently this is application-delivery pacing with receive bursts, not a strict cap on inbound wire traffic. Build it with the normal Cargo/package commands in the README. It needs no installation: launch the portable application and explicitly configure the client to use SOCKS5 after validation.

## Why a system-wide slider would be misleading

Windows Filtering Platform provides a filter engine and kernel callouts; ordinary permit/block filters are not a rate shaper. Microsoft describes [WFP architecture and callouts](https://learn.microsoft.com/en-us/windows/win32/fwp/about-windows-filtering-platform). User-mode filter management also has a privilege model; local administrators can add filters. A per-application control that simply installs block filters would interrupt traffic, not pace it.

qWAVE exposes outgoing-rate operations on application-managed flows. That is not an arbitrary foreign-process inbound shaping API. See [QOSSetFlow](https://learn.microsoft.com/en-us/windows/win32/api/qos2/nf-qos2-qossetflow). Policy-based QoS likewise does not turn an unprivileged GUI into a general inbound TCP/UDP shaper.

For a transparent backend, our engineering conclusion is that a privileged broker and a signed WFP callout driver (or a separately reviewed equivalent packet interception component) are needed. Stream inspection supports deferral/injection, while packet and datagram processing have separate ownership and completion requirements. See [stream inspection](https://learn.microsoft.com/en-us/windows-hardware/drivers/network/stream-inspection) and [callout processing](https://learn.microsoft.com/en-us/windows-hardware/drivers/network/using-a-callout-for-deep-inspection). Even a driver cannot prevent packets from physically arriving from the network; inbound delivery shaping relies on buffering/backpressure or packet-level strategies.

## Separate component: not implemented or shipped

**No transparent WFP service/driver source, `.sys`, INF, installer, signing certificate, filter GUID, or service registration is included in 0.1.0.** This is a remaining requested capability, not a completed backend or a hidden switch. A kernel stub that always permits traffic would not satisfy it. The UI's system-wide enforcement status stays **UNAVAILABLE**.

The available toolchain built the user-mode Rust source. WDK builds, driver signing, registration and hardware tests were outside this unattended session. There is therefore no valid driver build/install command for this repository yet. Do not run generic `sc create`, INF, test-signing, firewall or QoS commands against it.

A future separately reviewed component needs the following concrete work before providing an installer:

1. A broker service authenticating the interactive user's control connection, validating bounded policy generations and authorizing executable identities. User-writable policy files must not become an arbitrary privileged command channel.
2. WDK x64 driver projects, a restrictive device ACL, stable versioned IOCTL messages, ALE app/flow attribution for IPv4/IPv6 and TCP/UDP, and one shared application budget across flows.
3. Correct stream deferral/continuation or bounded packet clone/reinjection queues; never sleep in `classifyFn`. Handle DPC/IRQL constraints, injection loops, flow teardown, FIN/RST, cancellation and driver unload while I/O is outstanding. Blocking or dropping as a substitute for pacing is insufficient.
4. Queue byte/time limits, fair scheduling, monotonic token accounting, call-mode updates, quota semantics and explicit accounting scope for payload versus wire bytes.
5. A disposable Windows VM with matching SDK/WDK, the driver's documented build project and test signing confined to that VM. Build and signing instructions must be tied to those actual projects and artifacts, which do not exist yet.
6. Driver Verifier and flow-lifetime tests, IPv4/IPv6, UDP, large transfers, TCP retransmission, VPNs, suspend/resume, Wi-Fi changes and service crash/unload behavior. Review coexistence with existing security products.
7. A signed production package with an explicit user-authorized installer/uninstaller. Keep all provider/filter/service registrations owned and reversible; fail safely when the broker or driver is missing. Installation steps will be documented with that real package.

No service or driver should be installed merely to test the currently available explicit proxy.
