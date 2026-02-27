# Phase 3: Display and Clipboard - Context

**Gathered:** 2026-02-27
**Status:** Ready for planning

<domain>
## Phase Boundary

Xvfb lifecycle management on headless remotes, local clipboard reading (Wayland and X11), and remote clipboard writing with X11 selection ownership persistence. The local daemon detects new clipboard images and the remote daemon writes them to clipboard. Transport is already built (Phase 2). Full pipeline wiring, systemd packaging, and shell snippets are Phase 4.

</domain>

<decisions>
## Implementation Decisions

### Clipboard watching behavior
- Event-driven clipboard change notifications where the OS supports them, with polling fallback
- Deduplicate by content hash — skip sending if image is identical to last sent
- Log non-image clipboard changes (debug aid) but only send images
- On startup, ignore whatever is currently on the clipboard — only send newly captured images

### Error and fallback behavior
- Missing clipboard tools (xclip, wl-paste, wl-copy): fail immediately with actionable error message including install command
- Display detection: try Wayland first, fall back to X11; if neither works, fail with clear error showing what was checked
- Stale Xvfb lock files: auto-clean after verifying no process owns them, log what was cleaned
- Xvfb crash: auto-restart with exponential backoff, give up after N failures and exit with error

### Display file format
- ~/.cssh/display is a sourceable shell script: `export DISPLAY=:99`
- DISPLAY value only — no PID, timestamp, or other metadata
- Auto-create ~/.cssh/ directory if it doesn't exist
- Remove ~/.cssh/display on graceful daemon shutdown to prevent stale values

### Logging and feedback
- Quiet by default, -v flag for verbose output
- Normal mode logs: daemon started, connected to remote, image sent/received (with size in KB), display detected, errors
- All log lines include timestamps
- Verbose mode adds: clipboard check details, display detection steps, Xvfb management details

### Claude's Discretion
- Specific clipboard change notification mechanism per platform
- Polling fallback interval
- Content hashing algorithm
- Xvfb display number selection strategy
- Exponential backoff parameters for Xvfb restart
- Exact log format and logging library choice

</decisions>

<specifics>
## Specific Ideas

No specific requirements — open to standard approaches

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 03-display-and-clipboard*
*Context gathered: 2026-02-27*
