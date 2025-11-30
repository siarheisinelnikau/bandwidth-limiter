# Experimental WFP design and review boundaries

## Scope and source map

The first kernel path deliberately implements **TCP receive shaping only**, for new flows of specifically configured local executables, on Windows 10 2004+ / Windows 11 x64. The separate Rust broker is an SCM LocalSystem service; there is no user pipe, network listener, shell invocation, child-process launcher or GUI control path. Its only policy input is `%ProgramFiles%/BandwidthLimiterWfp/policy.json`. A strict schema refuses upload, UDP, quota and schedule settings. This prevents the existing proxy-policy controls from being represented as driver enforcement.

`backend/shared/protocol.h` and `backend/service/src/protocol.rs` define a pointer-free ABI. Configuration is exactly 272 bytes, status is 64 bytes, and both directions validate sizes/version. Configuration allows 1–32 positive download rates, each 1 KiB/s to 1 GiB/s, and a 1–30 second lease (broker uses five seconds). Every driver IOCTL requires a read/write handle; the device's protected DACL allows only SYSTEM and Administrators. The legacy device is exclusive, so a broker owns the control channel and separate admin commands require it to be stopped.

The broker rejects policy-file or installation-directory reparse points, a null DACL, ordinary-account ownership, unsupported ACE kinds, and write/delete/DACL/owner grants to principals other than SYSTEM/Administrators. Only the protected Program Files installation is supported. Its ancestor directories and SCM binary path remain part of the trusted administrative installation boundary. Policy parsing tests and a synthetic SID test do not prove a complete security audit of this privileged boundary.

## Classification and shared pacing

1. The service resolves each full local executable path with `FwpmGetAppIdFromFileName0`, then creates IPv4/IPv6 `ALE_FLOW_ESTABLISHED` inspection filters conditioned on that app ID and IP protocol TCP. The filter's raw context holds a 1-based bounded policy slot. Driver rates are configured before filters are atomically committed.
2. The driver's ALE callback associates a nonpaged reference-counted flow context with the corresponding STREAM callout. The STREAM callouts use `FWP_CALLOUT_FLAG_CONDITIONAL_ON_FLOW`, so unrelated flows avoid them. This prototype does not discover or attach pre-existing flows. Reconnect clients after activation.
3. Each incoming stream indication reserves leading payload bytes from the slot's single global budget. Partial `countBytesEnforced` permits that leading portion; WFP reindicates the remainder. Zero credit produces `FWPS_STREAM_ACTION_DEFER`. Send indications and zero-length control indications are permitted. No classify callback sleeps, waits on a mutex, accesses a pageable policy, or calls `FwpsStreamContinue0`.
4. A timer/DPC snapshots up to 32 deferred flows with independent references and saved receive flags. It calls `FwpsStreamContinue0` outside the classify stack and spinlock. A continuation may cause another classification. Failures requeue a still-live flow and increment a visible counter; deletion releases pending accounting. References cover concurrent `flowDeleteFn` and continuation snapshots.
5. The shared freestanding C budget uses monotonic 100 ns ticks, integer fractional-byte credit, a quarter-second maximum burst and zero initial credit. Multiplication is bounded by the validated rate and clamped elapsed interval. Connections cannot multiply their per-application budget. There is no promise of strict per-flow fairness; resumed batches rotate to the end of the list.

WFP supports DEFER only for incoming streams, and requires continuation outside `classifyFn`. See [the stream IO packet contract](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/fwpsk/ns-fwpsk-fwps_stream_callout_io_packet0_) and [FwpsStreamContinue0](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/fwpsk/nf-fwpsk-fwpsstreamcontinue0). Microsoft also documents that filters must not be removed while a stream is deferred: [stream inspection](https://learn.microsoft.com/en-us/windows-hardware/drivers/network/stream-inspection). This implementation uses inline incoming flow control; it does not mix cloned out-of-band injection with inline processing.

## Drain, crash and unload protocol

The driver starts **DRAINING** and exposes no unload routine while runtime callouts are active. CONFIGURE succeeds only with no filters, flows or outstanding continuations. It arms the receive policy and a monotonic lease. The service renews the lease every 500 ms; idle armed driver maintenance wakes at most once per second, while pending receives use a 10 ms timer. There is no timer spin or per-frame polling.

The normal teardown order is:

1. Send DRAIN under the driver control mutex. Under the classify spinlock it disables new deferrals/associations and requests continuation of all pending receives.
2. Poll status until both `pending` and `continuations` are zero. A 30-second timeout **does not remove filters**.
3. Remove only the product's deterministic filter keys, management callouts, sublayer and provider in a WFP transaction. No foreign filters are enumerated or deleted.
4. CLEAR_FLOWS calls `FwpsFlowRemoveContext0` at PASSIVE_LEVEL, with temporary references covering concurrent deletion. The broker retries pending callback completion for five seconds. It refuses a new policy while any old context remains.
5. Ordinary service stop leaves the drained driver loaded so the service can restart. Explicit `--prepare-unload` additionally sends QUIESCE: it disables further configurations, unregisters runtime callouts, and only then exposes `DriverUnload`. Partial unregister failure stays QUIESCING and cannot rearm. A successful QUIESCE is terminal until driver reload.
6. A subsequent manual SCM driver stop cancels the timer, flushes queued DPCs and deletes the symbolic link/device. There is no forced unload shortcut.

Management objects use a **non-dynamic, non-persistent** engine session. They survive broker process termination but not BFE/machine restart. A dynamic session could remove a stream filter on crash while receives remain deferred, violating the stream contract. On handle cleanup the driver requests drain; if the broker hangs while retaining its handle, lease expiry also requests drain (nominally within five seconds plus the idle timer interval). Failed continuations can prevent completion; this is surfaced by counters and requires investigation in the VM, not forced filter deletion. A later service start or explicit `--recover` drains first, then removes the old deterministic objects.

Sources for context lifetime are [flow association](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/fwpsk/nf-fwpsk-fwpsflowassociatecontext0), [flow removal](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/fwpsk/nf-fwpsk-fwpsflowremovecontext0), and [flow deletion](https://learn.microsoft.com/en-us/windows-hardware/drivers/network/processing-flow-delete-callouts). These API contracts informed the source; they do not substitute for Driver Verifier evidence.

## Required review and deferred work

The driver has not been compiled, signed, loaded or exercised. Before declaring it usable, review and test: deferral/continuation concurrency and reentrancy; delayed or failed continues; classify/flow-delete/remove-context races; pending ALE association versus drain; transaction rollback notification counts; BFE stop/restart; all failure paths in registration and partial quiesce; timer/DPC rundown; invalid/parallel IOCTLs; memory pressure; load/unload with active traffic; IPv6; VPN/offload/coexistence with higher/lower-weight providers. Kernel `missedBytes` and other providers can make this backend's admitted-byte count incomplete. It is not a system-wide traffic accounting source.

WDK Code Analysis/SDV where supported, Driver Verifier and dedicated VM transfer tests are prerequisites to public driver distribution. Production signing, an installer/uninstaller, authenticated GUI-to-service IPC, live policy generations, upload/UDP scheduling, quota persistence, schedules and Call mode are future work. The GUI updater continues to replace only the portable GUI EXE; it must not be used to silently update kernel components.
