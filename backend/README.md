# System TCP backends

The current GUI targets **WinDivert through a separate Rust packet service**, using unmodified external upstream runtime binaries instead of this project's own kernel driver. Sources compile and isolated fixtures pass; nothing was installed/started and real IPC/driver/traffic behavior remains unverified.

| Component | Implementation | Boundary |
| --- | --- | --- |
| `packet-control/` | Version 3 strict policy/frames, bounded overlapped local pipe, SCM server PID check | Compiled and pure fixtures tested; real authentication/ACL/cancellation tests pending |
| `packet-service/` | SCM lifecycle, restricted runtime loading, NO_INSTALL WinDivert 2.2 handles, FLOW attribution, IPv4/IPv6 TCP parser and directional queues/budgets | Compiled/linted and synthetic scheduler/packet/authorization tests passed; native capture untested |
| External `WinDivert.dll` / `WinDivert64.sys` | Reviewed unchanged x64 runtime supplied upstream | Not downloaded, bundled, installed, loaded or signature/load-verified in this session |
| `service/`, `control/`, `driver/`, `shared/`, `tests/` | Retained earlier custom WFP v2 broker/driver/ABI/budget fixtures | Separate legacy endpoint; own driver remains uncompiled without WDK; current GUI does not use it |

Start with [WinDivert build/staging/removal](../docs/WINDIVERT-SETUP.md) and [implemented design, bounds and remaining tests](../docs/WINDIVERT-DESIGN.md). Build-only scripts are `scripts/check-windivert.ps1` and `scripts/build-windivert.ps1`; they neither install nor open a driver.

**Check service** reads aggregate status. Authenticated administrator **Apply saved TCP limits** activates saved enabled directional caps, and **Pause System TCP** stops capture/releases queues. Startup/restart is paused; no automatic policy load, service startup, elevation or GUI launch probe occurs. GUI Quit stops its proxy but leaves this independent service active. Unsupported quotas/schedules/Call mode are rejected. Newly observed identified TCP flows only are shaped; unknown/unmatched/old/scoped/unsupported traffic can pass. Queue overflow/expiry can drop packets. Pause/reapply can cause a burst/reordering/retransmission; it does not deliberately abort TCP.

The protected runtime directory is `%ProgramFiles%/BandwidthLimiterPacket`. The service, executable and runtime files must have documented restrictive ownership/ACLs. All WinDivert handles use `NO_INSTALL`; the driver must already be prepared/running by a separate administrator action. Reusing a signed upstream runtime avoids creating/signing our own driver, but runtime trust/compatibility and LGPLv3 distribution obligations still require review. Portable GUI updates do not update service/runtime files.

Legacy [WFP design](../docs/WFP-DESIGN.md) and [safe VM drain/quiesce/unload](../docs/WFP-SETUP.md) remain for old deployments. Do not remove live OOB filters or run the old/new shapers on the same traffic. `build-wfp.ps1` is retained for legacy source work only.
