---
phase: 04-integration-and-packaging
plan: 02
subsystem: infra
tags: [e2e, validation, xvfb, clipboard, ssh, systemd, xclip]

# Dependency graph
requires:
  - phase: 04-integration-and-packaging
    plan: 01
    provides: cssh setup local/remote subcommands and systemd user service generation
  - phase: 03-display-and-clipboard
    provides: clipboard watcher, display manager, transport server/client with frame relay
provides:
  - E2E validation: Ctrl-V in Claude Code / Codex / OpenCode on remote shows local screenshot
  - Bug fix: cssh remote always forces Xvfb so SSH sessions read clipboard via ~/.cssh/display
affects:
  - 05-peer-to-peer-mesh (future phase can rely on confirmed E2E clipboard bridge)

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "force_xvfb=true param in DisplayManager::detect_and_init() — cssh remote bypasses host Wayland/X11 and always spawns Xvfb so SSH sessions see the same clipboard"

key-files:
  created: []
  modified:
    - src/display.rs
    - src/main.rs

key-decisions:
  - "cssh remote always passes force_xvfb=true to detect_and_init() — ensures ~/.cssh/display is always written and SSH sessions can read the correct clipboard regardless of the host desktop environment"

patterns-established:
  - "force_xvfb pattern: remote daemon ignores host Wayland/X11 and always uses its own Xvfb instance so the published ~/.cssh/display path is always valid for SSH sessions"

requirements-completed: [E2E-01, E2E-02, E2E-03]

# Metrics
duration: ~30min (including hardware testing)
completed: 2026-02-27
---

# Phase 04 Plan 02: E2E Validation Summary

**Ctrl-V on remote SSH session pastes local screenshot into Claude Code, Codex, and OpenCode — validated on real hardware; fixed bug where cssh remote used host Wayland clipboard instead of Xvfb**

## Performance

- **Duration:** ~30 min (including hardware setup, testing, and bug fix)
- **Started:** 2026-02-27T17:49:39Z
- **Completed:** 2026-02-27
- **Tasks:** 1 (human-verify checkpoint)
- **Files modified:** 2 (src/display.rs, src/main.rs)

## Accomplishments
- Validated full E2E clipboard bridge on real hardware across all three target tools
- E2E-01: Claude Code shows `[Image #1]` within seconds of a local screenshot (Ctrl-V)
- E2E-02: Codex shows pasted screenshot image on Ctrl-V
- E2E-03: OpenCode shows pasted screenshot image on Ctrl-V
- Found and fixed critical bug: cssh remote was using the host Wayland clipboard instead of Xvfb, breaking clipboard visibility for SSH sessions
- Multi-paste verified: second screenshot replaces first in clipboard correctly

## Task Commits

1. **Bug fix found during E2E testing: force Xvfb in cssh remote** - `3f481a7` (fix)

**Plan metadata:** (docs commit below)

## Files Created/Modified
- `src/display.rs` - Added `force_xvfb: bool` parameter to `detect_and_init()`; when true, skips Wayland/X11 detection and always spawns Xvfb
- `src/main.rs` - Changed `cssh remote` to call `detect_and_init(true)` — always forces Xvfb

## Decisions Made
- `cssh remote` passes `force_xvfb=true` — even on machines with a live Wayland or X11 desktop session, the remote daemon spawns its own Xvfb and writes `~/.cssh/display`; this guarantees SSH sessions that source the file can always reach the correct clipboard

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] cssh remote used host Wayland clipboard instead of Xvfb**
- **Found during:** Task 1 (E2E hardware testing)
- **Issue:** On the remote machine, `cssh remote` detected `$WAYLAND_DISPLAY` and used the Wayland clipboard. SSH sessions sourcing `~/.cssh/display` expected an Xvfb display, but `~/.cssh/display` was never written because the Wayland path exits early. Ctrl-V in CLI tools showed nothing.
- **Fix:** Added `force_xvfb: bool` parameter to `DisplayManager::detect_and_init()`. The remote daemon always calls `detect_and_init(true)`, bypassing Wayland/X11 detection and always spawning Xvfb + publishing `~/.cssh/display`.
- **Files modified:** `src/display.rs`, `src/main.rs`
- **Verification:** All three E2E tests passed after the fix; `echo $DISPLAY` in SSH session prints `:1` (or similar)
- **Committed in:** `3f481a7` (fix commit)

---

**Total deviations:** 1 auto-fixed (Rule 1 — bug)
**Impact on plan:** Critical correctness fix — without it, cssh remote silently used the wrong clipboard on any machine with a desktop session. No scope creep.

## Issues Encountered

The Wayland clipboard bypass bug was the only issue. After the fix, all E2E tests passed on first attempt. Service logs (`journalctl --user -u cssh-remote -f`) confirmed Xvfb spawning and display publishing.

## User Setup Required

None — fix is transparent; `cargo install --path .` on both machines picks up the corrected binary.

## Next Phase Readiness
- Core value proposition is proven: Ctrl-V on remote SSH pastes local screenshot into Claude Code, Codex, and OpenCode
- Phase 4 is complete — all requirements satisfied (E2E-01, E2E-02, E2E-03, SRVC-02, SRVC-03)
- Phase 5 (peer-to-peer mesh with Tailscale auto-discovery) can build on the confirmed working clipboard bridge

## Self-Check: PASSED

- FOUND: src/display.rs (modified — force_xvfb parameter added)
- FOUND: src/main.rs (modified — detect_and_init(true) call)
- FOUND commit: 3f481a7
- FOUND: .planning/phases/04-integration-and-packaging/04-02-SUMMARY.md

---
*Phase: 04-integration-and-packaging*
*Completed: 2026-02-27*
