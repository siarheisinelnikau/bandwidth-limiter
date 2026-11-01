# BandwidthLimiter v0.2.0

The first published release brings an original Windows x64 bandwidth-policy utility with six interface languages and a choice of light or dark appearance. Version 0.1.0 was a local development baseline; no preceding published release was found.

## Added

- English, Russian, French, German, Spanish and Italian interfaces, including application rules, connection inventory, schedules, System TCP help, update controls, validation messages and the tray menu. Language names are shown in their own languages.
- A clear **Settings → Appearance** section with language and **Light / Dark** choices. Changes apply immediately; **Save changes** preserves both preferences. Existing settings retain their rules, quotas and update preference and gain English/Dark defaults.
- A light palette covering panels, input fields, controls, selections and status colors. The software renderer clears its bitmap using the selected background, preventing old dark pixels from surviving a theme change.
- A real loopback SOCKS5 TCP relay with executable-specific upload/download caps, shared per-application budgets, daily quotas and weekly schedules. Call mode lowers limits for marked background applications.
- A separate read-only Windows TCP/UDP connection inventory; direct-traffic byte rates are explicitly unavailable.
- Experimental System TCP (WinDivert) service controls with explicit status, Apply and Pause actions. Reviewed external runtime and service preparation are required; the GUI does not install, start or elevate them.
- Manual and nonblocking startup update checks with a saved opt-out, verified downloads with progress and a user-initiated restart/replacement flow that preserves settings and usage.

## Improved

- Worker notifications retain message templates so changing the language also updates already displayed normal notifications without translating user paths or network addresses.
- Unknown preference values fall back safely to English and Dark. Old settings migrate without a schema reset.
- Developer preview packaging now derives the executable version from Cargo rather than using an obsolete hardcoded filename.

## Fixed

- Windows proxy handshakes no longer inherit the listener's nonblocking mode. This could reset a valid SOCKS5 client between greeting and CONNECT; the loopback forwarding/half-close regression now passes.
- A saved Light or Dark choice stays selected when Windows reports the opposite system appearance, keeping the custom palette and standard widgets consistent.

## Validation and limits

Catalog/placeholder consistency, literal dynamic data and old-settings/preference roundtrips have focused regression coverage. See [the verification record](VERIFICATION.md#localization-and-appearance--2026-11-01-version-020) for the completed commands and any remaining CI limitations.

The portable proxy requires SOCKS5 configuration in each client and does not affect direct traffic or UDP/QUIC. The independent WinDivert backend remains experimental and has not been validated with a real driver or traffic; quotas, schedules and Call mode are unsupported there. Detailed Windows/updater/service diagnostics remain in their original language. Startup recovery uses English when a valid configuration is unavailable.

Current localized/light screenshots, native menu/input/tray behavior and visible/hidden runtime performance are unverified because the repository's no-GUI restriction remains in effect. Historical screenshots are explicitly labeled. Unit tests and source inspection do not certify those runtime properties.

## Download

Windows 10/11 x64: `BandwidthLimiter-v0.2.0.exe`, accompanied by `SHA256SUMS`. Settings and usage remain under `%LOCALAPPDATA%/BandwidthLimiter`. The portable GUI does not include, install or update the independent service or third-party WinDivert runtime.
