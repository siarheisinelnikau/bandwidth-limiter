# Experimental WFP backend sources

This directory contains a Windows kernel driver, Rust SCM broker and shared GUI control client for **experimental per-executable TCP upload/download pacing**, without SOCKS settings. User-mode components build; the driver remains uncompiled without WDK. No component was installed, started or used to alter actual traffic during development.

| Component | Implementation | Current verification |
| --- | --- | --- |
| `driver/driver.c` | ALE association, conditional IPv4/IPv6 stream callouts, two FIFO clone/injection queues per flow, independent shared directional budgets, bounded ownership/completions, lease/abort/unload | Source review only; WDK compilation and VM tests pending |
| `shared/` | Fixed-width versioned IOCTL ABI and integer pacing budget, 32 rules / 512 flow bound | Shared C fixture compiled and passed with MSVC |
| `service/` | LocalSystem SCM service, executable ALE filters, restricted driver handle, policy validation, ACL checks, heartbeat, guarded recovery/removal, Application event-log errors | Windows build, Clippy and four synthetic tests passed |
| `control/` | Version 2 policy/frames, bounded overlapped local pipe transport, SCM server identity check | Build/lint and pure protocol fixtures passed; real IPC untested |
| `policy.example.json` | Version 2 download/upload caps, with zero meaning unlimited in one direction | Strict parser fixtures passed |

Start with [build, installation and removal instructions](../docs/WFP-SETUP.md) and [lifecycle/review notes](../docs/WFP-DESIGN.md). The GUI's **System TCP (WFP)** page now explicitly checks the service and applies/pauses saved caps through authenticated local IPC. No connection or installation occurs automatically. Administrator authorization is required for changes; ordinary users can request aggregate status.

The v2 driver clones and absorbs selected stream data in both directions, then injects it when a shared directional budget permits. It never uses receive-only DEFER for upload or mixes inline passthrough into an OOB flow. UDP, quotas, schedules and WFP Call mode remain unsupported and are rejected. Apply replaces only the in-memory service policy; restart reloads the protected JSON file. GUI Quit does not stop WFP.

**Pause/reapply/service stop/lease loss close tracked TCP connections** to avoid removing filters while OOB work is active. Reconnect clients after Apply. Incoming OOB pacing controls delivery, not physical packet arrival or ACK suppression; its bounded queue can overflow under fast senders. Oversized indications/resource failures terminate affected tracked connections. Untracked flow allocation failure leaves a connection unshaped and increments a counter. These limits and the unverified kernel path preclude a production enforcement claim.
