# BandwidthLimiter

Repository: `bandwidth-limiter`
Priority: 8 of 15 (user-requested launch order)
Product: BandwidthLimiter
Purpose: Per-application bandwidth limits and call-friendly presets
Inspiration: https://netlimiter.com/

Independent open-source implementation; no affiliation, competitor code or copied assets.

## Requested implementation

Create a native Windows interface showing per-application network activity with configurable upload/download caps, quotas and schedules, including a 'Call mode' that limits selected background downloads. Distinguish observed traffic from actual enforced limits. Implement a testable rule/policy engine and real Windows integration source.

Determine the capabilities and constraints of supported Windows networking APIs. If genuine per-application inbound shaping requires a privileged service/callout driver, clearly separate that component and provide source/build/install instructions for later review. Never substitute a chart, firewall blocking or an unenforced slider and claim bandwidth shaping works. Do not install/register services or drivers, change firewall/QoS/network configuration, request elevation, or throttle the user's actual traffic now. Exercise rules and adapters with fixtures/synthetic traffic, and label any unavailable enforcement backend explicitly in the UI/docs.

## Packaging and publication target

Windows standalone asset: `BandwidthLimiter-vVERSION.exe` (for the initial release, `BandwidthLimiter-v0.1.0.exe`). Append architecture only when distributing multiple executable architectures in the same release.
Planned canonical repository: https://github.com/siarheisinelnikau/bandwidth-limiter (not yet created or published; keep availability/error reporting truthful).

## Working agreement

Follow AGENTS.md. Complete as much real implementation and safe noninteractive verification as possible in this unattended code-only session. Manual UI, device and hardware tests will happen later. Record required external components and their verification status. Keep changes committed locally in meaningful increments.

