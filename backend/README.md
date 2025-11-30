# Experimental WFP backend sources

This directory adds a separate Windows kernel driver and a Rust SCM broker for **system-level incoming TCP shaping by executable path**, without SOCKS settings. These are experimental sources, not a validated driver distribution. The service builds locally; the driver has not been compiled because WDK is absent. Nothing in this directory has been installed, loaded or used to alter actual traffic during development.

| Component | Implementation | Current verification |
| --- | --- | --- |
| `driver/driver.c` | ALE flow context association, conditional IPv4/IPv6 stream callouts, incoming deferral, timer-driven continuation, shared token budgets, lease/drain/unload protocol | Source review only; WDK compilation and VM tests pending |
| `shared/` | Fixed-width versioned IOCTL ABI and integer pacing budget, 32 rules / 512 flow bound | Shared C fixture compiled and passed with MSVC |
| `service/` | LocalSystem SCM service, executable ALE filters, restricted driver handle, policy validation, ACL checks, heartbeat, guarded recovery/removal, Application event-log errors | Windows build, Clippy and four synthetic tests passed |
| `policy.example.json` | Receive-only constant-rate policy; no upload/UDP/quota/schedule fields | Strict parser fixtures passed |

Start with [build, installation and removal instructions](../docs/WFP-SETUP.md). See [lifecycle and review notes](../docs/WFP-DESIGN.md) before loading any binary. The portable GUI remains a separate application with a SOCKS5 backend: its policy editor does not control this experimental service, and its **system-wide enforcement unavailable** status remains truthful for that GUI.

Upload shaping and UDP are not implemented by this WFP driver. `FWPS_STREAM_ACTION_DEFER` is receive-only; the code always permits outgoing stream data. Unknown policy fields are refused, not silently applied as bandwidth controls. This backend currently has no quotas, schedules, call-mode control, historical traffic data or live GUI integration. Service changes require an administrator to stop it, replace the protected policy and restart it; reconnect selected applications afterward.

This is still inbound TCP flow control, not a guarantee that packets cannot arrive at the network adapter. NIC/offload buffers, retransmissions, other WFP providers and TCP behavior affect wire measurements. When flow capacity/allocation is exhausted, tracking fails open and increments `untracked_flows`; it must not be advertised as complete enforcement in that condition.
