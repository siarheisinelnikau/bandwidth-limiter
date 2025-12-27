# Design and rendering verification — 2026-10-31

Applied `product-design` and `rust-performance`, including their self-capture and pre-launch rendering/load guidance. No computer use or ordinary native GUI launch was performed. Source work and finite headless rendering were used to respect the session's desktop restrictions.

## Workflow and implementation

The old dashboard combined proxy setup, application rates, a full endpoint inventory and a separate rule editor. The new primary workflow is Applications → select/add EXE → set caps → save → start limiter. Connections is a separate read-only picker. Settings provides Call mode caps and the required update actions; less frequent connection/storage and experimental WFP details are disclosed on demand. Quotas and schedules remain accessible in each application's named section. A single persistent draft bar saves all changes.

The palette uses a dark neutral background, navy surfaces, readable secondary text, teal selection/success and amber warning text. Type roles are 25/17/14/12 points with common spacing and explicit control outlines. Color is accompanied by status text and labels. Long endpoint addresses are available as details rather than competing with app names in the default view. Empty, populated, disabled and recoverable error states are included.

The native host now uses egui 0.33.3 → egui_software_backend 0.0.2 CPU rasterization → softbuffer 0.4.8 Win32 GDI. The pinned softbuffer Windows source uses DIB/GDI operations. eframe/glow/wgpu are removed from the dependency graph. PE inspection of the final release found no OpenGL32, D3D11/12, DXGI, VCRUNTIME or MSVCP DLL imports. That supports source inspection; it is not evidence that normal native presentation was exercised, nor a claim that Windows composition is disabled.

Native X hides only when a tray icon was successfully created. The menu has Open, Start/Pause limiter, Call mode, Settings and Quit. Menu events wake the event loop directly, and background results are drained without requiring a hidden layout pass. A second launch signals a data-directory-specific named event; the existing process requests show/restore/focus. The event is not a policy or privileged control channel. Actual Windows activation and menu behavior are untested.

## Actual application-generated captures

The packaged optimized `BandwidthLimiter-v0.1.0.exe` was invoked through `scripts/render-preview.ps1`. Its early `--render-preview` path constructs the actual `App` UI with synthetic data and the shared CPU `Paint` implementation. It creates no native window, tray, instance event, network worker, update check or normal data-directory access. Each of five fixed cases receives two bounded layout/raster passes; the internal limit is 20 seconds between cases and the external supervisor kills only its own capture process if it exceeds 25 seconds. Images are RGBA PNGs, atomically completed at their paths.

The final capture was PID 3536, exit code 0, 277 ms observed wall time and 93.75 ms process CPU time for the entire invocation. The internal five-screen capture reported 94 ms. These are one finite capture's measurements, **not idle CPU or an interactive latency benchmark**. See [capture manifest](screenshots/capture.json) and [invocation/hash](screenshots/invocation.json).

Inspected all five output images:

| Image | Scope |
| --- | --- |
| [Applications](screenshots/applications.png) | 980 × 720 client area, 100%, artificial populated rules/rates |
| [Settings](screenshots/settings.png) | 980 × 720 client area, 100%, artificial state with startup checks disabled |
| [Empty](screenshots/empty.png) | 980 × 720, no rules, disabled Start and explicit Add workflow |
| [Error](screenshots/error.png) | 980 × 720, artificial save failure, preserved draft and visible retry/save action |
| [Applications at 125%](screenshots/applications-125.png) | 1050 × 775 physical pixels / 840 × 620 logical viewport; scrollable columns |

The first capture exposed inherited horizontal layouts that pushed the detail editor outside the viewport. Both columns now use explicit top-down layout. It also revealed previous-frame text surviving in transparent areas; the CPU buffer is now cleared before compositing and a headless frame-removal regression test passes. Control strokes/weak fills were aligned with the palette. The affected screens were rebuilt, recaptured and inspected. These are real application renders with visible PREVIEW DATA labels, not fabricated OS screenshots or AI mockups. Native title bars, tray and system menus are outside their scope.

## Scheduling and tests

Idle policy control now blocks on its bounded command channel instead of waking on an idle maintenance timer. Active proxy/monitor work still samples once per second. Immutable `Arc<View>` snapshots avoid copying the endpoint table on pointer events. The worker publishes repaint requests only for changed snapshots or completed commands; update-download progress coalesces notifications at 200 ms. The tray updates only when its displayed state changes.

`Scheduling` uses Wait/WaitUntil, finite deadlines and a dirty flag; there is no Poll branch. Hidden/minimized paths skip layout, rasterization and presentation, keeping only data/commands dirty for restore. OS expose requests can present a retained bitmap without rebuilding the scene. Unit tests cover earliest-deadline selection, return to no scheduled work, hidden wake requests producing no due frame, and restoration becoming dirty. These are tests of scheduling state, not a live HWND assertion.

Final checks passed: formatting, Clippy with `-D warnings`, 13 library tests plus three renderer/scheduler tests, optimized packaging, PE architecture/import inspection and all five headless captures. Two loopback socket tests remain ignored after the prior sandbox error 10013. Cargo sometimes copied incremental-cache files after environmental hard-link warnings; no Clippy findings remained. No driver or broker code was installed or run.

## Runtime gate remains incomplete

Normal visible, idle, hidden/minimized, native tray/restore, second-launch focus, keyboard/accessibility traversal, graphics presentation and long-running resource behavior have not been measured. Do not infer they pass from the PNGs, source or unit tests. No CPU/GPU/present claim is made for normal desktop operation. WFP remains source-only without a WDK-built/validated driver, and is not connected to this UI.

For a separately authorized bounded native test, the executable supports `--diagnostics ABSOLUTE_JSON_PATH`. On exit it writes bounded visibility segments, layout/present counts and aggregate layout/raster/presentation time. Combine it with read-only process sampling and actual native window-state inspection. It is not a background telemetry service or a periodic file writer. Expected acceptance evidence includes settled idle frames stopping, zero hidden layout/raster/presents, correct restore/quit from tray, current data on reopen and no duplicate workers. Investigate any stutter/load before extended interaction. The command was not run in this session.
