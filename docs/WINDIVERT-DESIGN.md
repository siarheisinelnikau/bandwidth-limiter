# WinDivert System TCP backend

Implemented 2026-11-01. User-mode GUI, control library and service compile without a WDK or installed WinDivert SDK. The WinDivert DLL/driver are external, unmodified runtime prerequisites. No runtime binary was downloaded, loaded or installed during implementation, and no actual packets were captured. This is compiled implementation plus isolated fixture evidence, not verified system-wide enforcement.

## Components and control boundary

- `backend/packet-control`: strict version 3 policy/framing, local overlapped pipe transport, deadlines/cancellation and SCM server identity verification.
- `backend/packet-service`: SCM service, protected runtime loader, WinDivert 2.2 x64 ABI, FLOW attribution, TCP parser, bounded directional scheduler and packet worker.
- `src/traffic.rs`, `runtime.rs`, `ui.rs`: saved-rule conversion, blocked background client worker and System TCP (WinDivert) controls. Save and ordinary launch do not connect to a service. The proxy and packet backend have independent lifetimes/actions.
- `backend/service`, `backend/control`, `driver`, `shared`: retained experimental WFP v2 source. The new GUI does not connect to its pipe/device or manage its filters. It must be separately drained/unloaded in a VM before migration; there is no automatic legacy cleanup.

New service identity: `BandwidthLimiterPacketService`; pipe: `\\.\pipe\BandwidthLimiter.WinDivert.v3`; protocol/policy version: 3; required response backend: `windivert-2.2-tcp`. Wrong versions/backend identities are refused. There is no compatibility alias to the old WFP service.

The pipe holds one first/exclusive local server instance and rejects remote clients. Its DACL gives SYSTEM/Administrators full rights and interactive users only read-data/write-data/synchronize, without create-instance rights. After reading a request, the broker impersonates the caller only to check enabled Administrators membership, then reverts before backend work. Status is read-only; Apply/Pause are administrator-only at both IPC and controller layers. Failure to revert aborts the process. The client verifies that the pipe PID equals the running SCM service PID before sending policy data.

Frames are strict length-prefixed JSON up to 128 KiB, with two-second frame I/O deadlines. The pipe worker waits at most 35 seconds for backend work; client response wait is 38 seconds. Acknowledgement precedes server disconnect. Kernel I/O cancellation is reaped before borrowed buffers/events are freed. A timed-out authorized operation may still finish; check status before retrying. These native authentication/cancellation paths are compiled but have not been exercised against a real service.

## Activation and protected loading

The service starts **paused**, without reading startup policy, loading a DLL, opening WinDivert or capturing traffic. There is no automatic policy persistence/reload. It waits for IPC/SCM stop events without an idle maintenance timer. The GUI likewise has no launch probe, polling, service startup or elevation action.

Authenticated Apply validates all saved enabled TCP caps first. Unsupported enabled quotas/schedules and active Call mode are rejected as a whole; unlimited rules are omitted, and 1..32 capped local EXEs are required. At least one direction per rule must be nonzero. Paths/rates/duplicate aliases/unknown fields are bounded and checked without shell execution.

The service must be staged in `%ProgramFiles%/BandwidthLimiterPacket`. Before `LoadLibraryExW`, it checks the directory, running EXE, `WinDivert.dll` and `WinDivert64.sys` for a non-reparse, SYSTEM/Administrators-owned restrictive ACL. DLL resolution uses a protected absolute path plus DLL-directory/system-directory search flags, not PATH/current-directory lookup. Administrators and the staged reviewed runtime remain trusted. Installation must also protect ancestors; these checks are not a defense against a malicious administrator. Kernel trust acceptance is ultimately checked by Windows when a deliberately prepared driver is loaded; no Authenticode/WHCP compatibility claim is made here.

Every `WinDivertOpen` uses **NO_INSTALL**. It will not register/start the driver when opening the device fails. A separately staged and already running reviewed driver is required. FLOW sniff/read-only monitoring opens before NETWORK capture, both requiring driver ABI 2.2 and bounded kernel queue settings. This does not avoid a kernel component; it avoids producing/signing our own one. No automatic downloading or driver trust-policy changes exist.

Protected runtime validation/loading happens before the old session is paused. Reapply then drains the old session and starts a new one; later activation failure can leave shaping paused. An already failed/closed worker can be replaced by Apply. GUI Quit stops only its proxy; the service remains independent. Service restart requires Apply again.

## Attribution and packet parsing

WinDivert NETWORK metadata has no PID. FLOW events provide endpoint ID, PID and local/remote IP/port tuple. The worker resolves a process once on establishment using a query-limited handle, compares process creation time to the event timestamp to reject obvious PID reuse, and matches the full path to the policy. QPC events are compared to sampled FILETIME/QPC clocks; clock changes, inaccessible/terminated processes and race conditions can make attribution unavailable. It is attribution for a cooperative utility, not a security boundary.

Only newly observed, identified selected TCP flows enter the 512-entry map. Old connections, missed establishment events, the race between event delivery and first packets, and capacity/identity failures can leave packets unshaped. They pass immediately with an unmatched/unknown counter. No retroactive association or exact first-packet guarantee is claimed. FLOW deletion is keyed by endpoint ID, so a stale delete cannot erase a replacement connection; generation timestamps prevent old packet timestamps from being assigned to a newer tuple. Lost deletion events can retain an entry until tuple replacement/session restart; the map stays bounded.

The network filter selects TCP excluding loopback and impostor traffic; UDP/QUIC and local-machine traffic are outside scope. The parser checks IPv4 lengths/options and fragments, IPv6 lengths and up to eight supported extension headers, and TCP header/payload lengths. Unsupported fragments/extensions/jumbograms, malformed packets and scoped IPv6 link-local/multicast addresses are passed without a cap. FLOW has no interface scope, so equal scoped tuples are not treated as safely attributable. Host-order IPv4-mapped/IPv6 address words match the documented ABI. IPv6/offload/VPN/provider coexistence remains a VM test requirement.

## Scheduling, ownership and accounting

Each rule owns separate download/upload buckets shared across its connections. Credit starts at zero on Apply and is bounded by the larger of 64 KiB and a quarter-second of rate. Zero in one direction is unlimited. TCP **payload bytes**, including retransmissions, consume credit; IP/TCP headers and payload-free controls do not. Whole packets are admitted, so a 64,000-byte payload at 1 KiB/s waits 62.5 seconds from zero credit. No TCP splitting/modification or TLS interception is used.

There are two FIFO queues per selected flow, with round-robin service of active queues. Payload-free ACK/FIN/control packets remain behind earlier queued data in their direction; they pass immediately when there is no earlier queued payload. Queues are bounded to 4096 packets / 8 MiB of owned packet data globally and 128 packets per flow; native read/send buffers and queue metadata add bounded overhead. The receive allocation is 65,575 bytes. Overflow drops the new selected packet; 120-second-old queued packets expire. TCP may retransmit; resulting performance/latency is unverified. No silent inline fallback bypasses a full selected queue.

One worker owns two stable boxed overlapped receive operations, a per-send event and all user queues. It handles at most one FLOW event, one NETWORK completion and 64 queued sends per turn, checking stop between batches. Completed unselected packets are promptly reinjected. Checksums are recalculated before injection to handle offload metadata. A send has a 250 ms wait budget, then cancellation plus reaping; error stops the session and attempts bounded cleanup. OS/kernel completion and synchronous DLL calls can still delay shutdown beyond user-mode deadlines; no unsafe thread termination or freeing pending I/O is used.

With empty queues, no pending status publication and no events, the capture worker waits indefinitely. Token waits use the next credit/expiry deadline. Changed status is coalesced at 200 ms with a final publication deadline, so the last burst's counters become visible even if the worker then becomes idle. Status reads create no WinDivert handle. Counters reset on Apply and measure successful selected payload reinjection, not unique delivery, adapter arrival, live rates or all driver-internal drops. Unmatched/unknown means all captured TCP packets without a selected tracked association, including unrelated apps.

## Pause, errors and migration limits

Pause/stop/reapply shut down new capture, flush user-owned queues without pacing in flow/direction order, drain already captured packets and close handles. Cleanup has a two-second user-mode flush deadline; unsent owned packets are dropped/countable and native completion is still safely reaped. It does **not** call WFP flow abort or deliberately close TCP connections. A burst, packet reordering/loss or TCP retransmission can occur as newly uncaptured traffic overtakes draining packets. Fatal worker errors clear active state and preserve a visible error/counters. Driver-internal queue expiry/drop behavior is not fully observable through this API.

The old stream OOB driver's abort/drain/unload contract does not apply to packet ownership. Never run both shapers on the same test traffic. Stopping this service does not automatically stop/delete a shared WinDivert registration or remove another application's handles. Portability is now GUI EXE plus separately staged broker and upstream runtime; the portable updater still updates only the GUI.

## Evidence and remaining tests

Fixtures cover multi-flow bidirectional aggregate budgets, FIFO/control ordering, zero/unlimited directions, expiry/overflow/flow bounds, generation/deletion, low-rate large-packet delays, failed-send counters, IPv4/IPv6 parsing, documented x64 ABI offsets, strict protocol/backend identity and fake controller authorization. They perform no DLL loading, real pipe exchange, process attribution, SCM registration or packet I/O.

Before distribution: review the exact official signed runtime and LGPLv3 distribution obligations; stage only in a disposable VM; verify driver trust/load with current Secure Boot/HVCI/policies; test real pipe tokens/ACLs/spoofed servers/cancellation; measure one/many apps/flows and both directions against an independent server; cover first-packet races, short transfers, PID/tuple reuse, large/offloaded packets, errors/overflow, queue expiry, lost FLOW events, pause/reapply/stop/crash and coexistence. Confirm idle CPU/waits and actual native GUI layout/tray behavior separately. The user's no-GUI/no-install restriction leaves all those runtime checks unperformed.

API declarations and contracts were checked against the official [WinDivert 2.2.2 header](https://github.com/basil00/WinDivert/blob/v2.2.2/include/windivert.h), [DLL source](https://github.com/basil00/WinDivert/blob/v2.2.2/dll/windivert.c), [manual](https://reqrypt.org/windivert-doc.html), and [license](https://github.com/basil00/WinDivert/blob/v2.2.2/LICENSE). No WinDivert implementation or runtime binary is copied into this repository.
