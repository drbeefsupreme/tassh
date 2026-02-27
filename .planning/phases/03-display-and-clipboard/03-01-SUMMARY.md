---
phase: 03-display-and-clipboard
plan: 01
subsystem: infra
tags: [xvfb, display, x11, wayland, libc, arboard, image, sha2, anyhow, process-management]

# Dependency graph
requires:
  - phase: 01-foundation
    provides: DisplayEnvironment enum in protocol.rs
  - phase: 02-transport
    provides: tokio runtime already in Cargo.toml
provides:
  - DisplayManager with detect_and_init() for Wayland/X11/headless detection
  - Xvfb lifecycle management with -displayfd auto-selection
  - Stale lock file cleanup via libc kill(pid,0) liveness check
  - ~/.cssh/display publication for SSH session DISPLAY export
  - Xvfb auto-restart with exponential backoff (5 attempts, 2/4/8/16/32s)
  - Phase 3 dependencies: anyhow, arboard, image, sha2, libc
affects: [03-02, 03-03, clipboard-writer, remote-daemon]

# Tech tracking
tech-stack:
  added:
    - anyhow = "1" (ergonomic error handling)
    - arboard = "3" with wayland-data-control feature (clipboard X11+Wayland)
    - image = "0.25" with png feature (RGBA-to-PNG encoding)
    - sha2 = "0.10" (content hash deduplication)
    - libc = "0.2" (kill(pid,0) for PID liveness check)
  patterns:
    - libc::pipe() (not pipe2/O_CLOEXEC) for fd inheritance by Xvfb child
    - -displayfd flag for atomic free-display-number selection (no hardcoded :99)
    - Arc<Mutex<Option<Child>>> for shared Xvfb child handle between main and monitor task
    - tokio::spawn background monitor task with exponential backoff restart

key-files:
  created:
    - src/display.rs (DisplayManager, Xvfb lifecycle, lock cleanup, display publishing)
  modified:
    - Cargo.toml (5 new dependencies: anyhow, arboard, image, sha2, libc)

key-decisions:
  - "libc::pipe() used instead of pipe2(O_CLOEXEC) so Xvfb child inherits the write fd"
  - "Arc<Mutex<Option<Child>>> enables monitor task to swap in restarted Xvfb child handle"
  - "#[allow(dead_code)] on display.rs module-level suppresses warnings until Phase 3 wiring"
  - "set_var(DISPLAY) called with #[allow(deprecated)] — safe at startup before any threads read it"

patterns-established:
  - "Pattern: DisplayManager::detect_and_init() is the single entry point for display setup"
  - "Pattern: publish_display() writes export DISPLAY=:N\\n (sourceable shell format)"
  - "Pattern: monitor_xvfb() background task with Arc<Mutex<>> child handle"

requirements-completed: [DISP-01, DISP-02, DISP-03, DISP-04]

# Metrics
duration: 2min
completed: 2026-02-27
---

# Phase 3 Plan 01: Display and Clipboard Summary

**DisplayManager with Xvfb -displayfd lifecycle, stale lock cleanup via kill(pid,0), and ~/.cssh/display publication for headless SSH remotes**

## Performance

- **Duration:** 2 min
- **Started:** 2026-02-27T16:47:22Z
- **Completed:** 2026-02-27T16:49:05Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments

- `src/display.rs` with complete `DisplayManager` implementation: Wayland/X11/headless detection, Xvfb spawn with `-displayfd`, stale lock cleanup, `~/.cssh/display` publishing, and clean shutdown
- `Cargo.toml` updated with all Phase 3 dependencies (anyhow, arboard with wayland-data-control, image with png feature, sha2, libc)
- Xvfb auto-restart background monitor task using exponential backoff (2s/4s/8s/16s/32s, max 5 attempts)
- Stale lock file cleanup: scans `/tmp/.X{N}-lock` for N in 0..100, verifies PID liveness with `libc::kill(pid, 0)`, removes stale entries

## Task Commits

Each task was committed atomically:

1. **Task 1: Add Phase 3 dependencies to Cargo.toml** - `93a5d9f` (chore)
2. **Task 2: Implement DisplayManager in src/display.rs** - `bbe1d5b` (feat)

**Plan metadata:** (docs commit hash to follow)

## Files Created/Modified

- `src/display.rs` - Complete DisplayManager: detect_and_init(), shutdown(), Xvfb lifecycle with -displayfd, stale lock cleanup, ~/.cssh/display publishing, auto-restart monitor
- `Cargo.toml` - Added anyhow, arboard (wayland-data-control), image (png), sha2, libc

## Decisions Made

- Used `libc::pipe()` (not `pipe2` with `O_CLOEXEC`) so the write fd is inherited by Xvfb child process without close-on-exec. This is the only correct approach for `-displayfd`.
- Used `Arc<Mutex<Option<Child>>>` to share the Xvfb child handle between the main `DisplayManager` and the background monitor task, allowing the monitor to swap in a new child on restart without requiring a separate channel.
- Added `#[allow(dead_code)]` at module level (same pattern as `protocol.rs` in Phase 1) — items will be wired up in Phase 3 plans 02 and 03.
- `std::env::set_var("DISPLAY", ...)` called with `#[allow(deprecated)]` — safe at startup before the tokio multi-thread runtime spawns more threads, documented in the code.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- `DisplayManager` is ready for wiring into the remote daemon's main.rs in a later plan
- All Phase 3 dependencies are now in Cargo.toml and resolve correctly
- `ClipboardWriter` (plan 03-02) can import `DisplayEnvironment` from `crate::protocol` as needed
- The `-displayfd` approach avoids hardcoded display numbers, eliminating stale-lock race conditions

---
*Phase: 03-display-and-clipboard*
*Completed: 2026-02-27*
