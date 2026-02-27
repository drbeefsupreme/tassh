---
phase: 03-display-and-clipboard
plan: 02
subsystem: infra
tags: [clipboard, arboard, xclip, wl-copy, sha2, image, spawn-blocking, tokio-process, x11, wayland]

# Dependency graph
requires:
  - phase: 03-display-and-clipboard/03-01
    provides: DisplayEnvironment enum usage pattern, arboard/sha2/image deps already in Cargo.toml
  - phase: 01-foundation
    provides: DisplayEnvironment enum in protocol.rs
provides:
  - watch_clipboard() async fn for local daemon clipboard polling
  - ClipboardWriter struct for remote daemon clipboard writing
  - check_clipboard_tools() for startup tool availability verification
  - SHA-256 content hash deduplication for clipboard change detection
  - RGBA-to-PNG encoding pipeline using image crate
affects: [03-03, local-daemon, remote-daemon, clipboard-bridge]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - arboard get_image() must run inside spawn_blocking (X11 backend is !Send + blocking I/O)
    - entire polling loop kept inside single spawn_blocking to avoid !Send issue across await points
    - xclip subprocess stored without .wait() — must stay alive to serve SelectionRequest events
    - wl-copy similarly kept alive for Wayland clipboard ownership
    - child.kill().await before spawning new clipboard holder prevents zombie accumulation

key-files:
  created:
    - src/clipboard.rs (watch_clipboard, ClipboardWriter, check_clipboard_tools)
  modified: []

key-decisions:
  - "arboard polling loop runs in a single spawn_blocking (not per-poll) to keep !Send Clipboard on one thread"
  - "startup-skip: initial clipboard hash is recorded but NOT sent to avoid re-sending stale screenshot"
  - "xclip/wl-copy subprocess stored without .wait() — required for X11 selection ownership persistence"
  - "child.kill().await before new write prevents zombie subprocess accumulation (CLWR-05)"
  - "-display flag passed to xclip using current $DISPLAY env var value"

patterns-established:
  - "Pattern: arboard usage: spawn_blocking wrapping entire polling loop, not individual calls"
  - "Pattern: ClipboardWriter.write() always kills previous child before spawning new one"
  - "Pattern: check_clipboard_tools() called at startup for actionable install-hint errors"

requirements-completed: [CLRD-01, CLRD-02, CLRD-03, CLRD-04, CLWR-01, CLWR-02, CLWR-03, CLWR-04, CLWR-05]

# Metrics
duration: 2min
completed: 2026-02-27
---

# Phase 3 Plan 02: Clipboard Read/Write Summary

**arboard polling loop (spawn_blocking, SHA-256 dedup, startup-skip) for local clipboard reads plus xclip/wl-copy subprocess writer with zombie-safe child management for remote clipboard writes**

## Performance

- **Duration:** 2 min
- **Started:** 2026-02-27T16:51:32Z
- **Completed:** 2026-02-27T16:53:01Z
- **Tasks:** 2
- **Files modified:** 1

## Accomplishments

- `src/clipboard.rs` with complete `watch_clipboard` implementation: arboard inside spawn_blocking, 500ms poll, SHA-256 hash deduplication, startup-skip (no stale re-send), RGBA-to-PNG via image crate, mpsc channel send
- `ClipboardWriter` with `write()` dispatching to `xclip` (X11/Xvfb) or `wl-copy` (Wayland) with correct MIME types, kills previous clipboard holder before each write, stores child without `.wait()`
- `check_clipboard_tools()` for startup tool availability verification with install hints for both xclip and wl-copy
- Display auto-detection at watch_clipboard entry: WAYLAND_DISPLAY then DISPLAY, clear error if neither set

## Task Commits

Both tasks implemented in one atomic pass to the same file:

1. **Task 1: ClipboardReader — watch_clipboard local side + Task 2: ClipboardWriter remote side** - `cbea34c` (feat)

**Plan metadata:** (docs commit hash to follow)

## Files Created/Modified

- `src/clipboard.rs` - watch_clipboard (local), ClipboardWriter (remote), check_clipboard_tools, rgba_to_png helper, content_hash helper

## Decisions Made

- Ran the entire arboard polling loop inside a single `spawn_blocking` rather than calling it per-poll. arboard's X11 backend is `!Send` and cannot be moved across `await` points. Keeping it inside one blocking thread is the correct approach.
- Used `std::thread::sleep(500ms)` inside the blocking task rather than `tokio::time::sleep` — appropriate for synchronous blocking thread.
- Stored `ClipboardWriter::current_child` without calling `.wait()` — xclip and wl-copy must stay alive to service X11 `SelectionRequest` events or Wayland clipboard ownership. This is critical for paste to work.
- `-display` flag passed explicitly to xclip from `$DISPLAY` env var, consistent with Xvfb display publishing in plan 03-01.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- `watch_clipboard` is ready to be wired into the local daemon's main loop (plan 03-03)
- `ClipboardWriter` is ready to be wired into the remote daemon's receive loop (plan 03-03)
- `check_clipboard_tools` is ready to be called at remote daemon startup
- All clipboard bridge logic is complete; plan 03-03 only needs to connect the pieces to the transport layer

---
*Phase: 03-display-and-clipboard*
*Completed: 2026-02-27*

## Self-Check: PASSED

- FOUND: src/clipboard.rs
- FOUND: 03-02-SUMMARY.md
- FOUND: commit cbea34c (feat(03-02): implement watch_clipboard — ClipboardReader local side)
