# Task log — 2026-03-31 — Already-Running Chrome Browser Study

## Actions
- Added `find_cdp_from_active_port_file()`: scans macOS/Linux/Windows Chrome profile dirs for `DevToolsActivePort` files to auto-detect running Chrome with CDP
- Added `scan_common_cdp_ports()`: probes ports 9220-9225 as a fallback
- Added `auto_detect_running_chrome_cdp()`: public high-level function combining both detection strategies
- Updated `ensure_chrome_running()`: checks DevToolsActivePort + port scan before launching headless Chrome; adds contextual hint if wrong port found; passes `--user-data-dir` to headless launch (Chrome 136+ compliance)
- Fixed `chrome_launch_command()` on macOS: replaced `open -a "Google Chrome"` with direct binary path + `--user-data-dir` (fixes Chrome 136+ rejection and macOS singleton delegation)
- Updated `/browser status` in `app.rs`: shows detected running Chrome with CDP when in default mode
- Updated `/browser connect` in `app.rs`: shows hint when Chrome-with-CDP is found on a different port
- Added 7 new unit tests covering DevToolsActivePort parsing, zero-port filtering, corrupt-file resilience, Chrome 136+ compliance

## Decisions
- Did NOT auto-switch the global CDP endpoint in `ensure_chrome_running` when a different port is found — that would silently break the user's explicit configuration
- Used `edgecrab-chrome-debug-*` prefix in temp dir naming so the scanner finds previously-launched edgecrab-managed Chrome instances too
- Kept the `--headless` path and temp profile for backgroundexecution (no change to that flow)

## Key Insight (First Principles)
CDP requires opt-in at Chrome launch time — there is no way to retroactively attach to a Chrome instance that wasn't started with `--remote-debugging-port`. The ONLY valid "connect to already-running Chrome" path is through `DevToolsActivePort` file detection (Chrome that was already started with CDP) or explicit port probing.  Chrome 136+ (March 2025) also blocks using the default profile dir with CDP — a `--user-data-dir` pointing to a non-standard dir is now mandatory.

## Next Steps
- Consider a `/browser detect` subcommand that explicitly runs `auto_detect_running_chrome_cdp` and auto-connects if found
- The `chrome_launch_command` macOS branch could also try to find the actual Chrome binary (not just hardcode the Google Chrome path)
