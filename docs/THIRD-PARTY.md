# Third-party components

BandwidthLimiter's own source is MIT-licensed. `Cargo.lock` records exact resolved dependencies. Distribution must retain applicable dependency notices; this document is a summary, not a replacement for their license texts.

| Direct component | Purpose | Declared license |
| --- | --- | --- |
| egui / egui-winit | Interface, input, clipboard and embedded fonts | MIT OR Apache-2.0; bundled font notices also apply |
| egui_software_backend | CPU rasterization | MIT OR Apache-2.0 |
| softbuffer | Win32 GDI bitmap presentation | MIT OR Apache-2.0 |
| tray-icon / muda | Native tray icon and menu | MIT OR Apache-2.0 |
| png | Bounded application preview files | MIT OR Apache-2.0 |
| winit | Window events and reactive event-loop adapter | Apache-2.0 |
| chrono | Local calendar scheduling | MIT OR Apache-2.0 |
| serde / serde_json | Settings and release data | MIT OR Apache-2.0 |
| fs2 | Data-directory instance lock | MIT OR Apache-2.0 |
| semver | Stable update version comparison | MIT OR Apache-2.0 |
| sha2 | Download integrity | MIT OR Apache-2.0 |
| ureq | HTTPS update requests | MIT OR Apache-2.0 |
| windows-sys | Windows API bindings | MIT OR Apache-2.0 |
| tempfile (tests only) | Isolated fixture directories | MIT OR Apache-2.0 |

Transitive components include rustls/ring, WebPKI roots, image/font libraries, graphics bindings and platform libraries. Their upstream license/notice files must be included when preparing public distribution. A full generated notice bundle and signing review are still release preparation work. No competitor code, assets, packet drivers or commercial libraries were copied into the project.
