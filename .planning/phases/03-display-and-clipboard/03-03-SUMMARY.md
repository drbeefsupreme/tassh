---
phase: 03-display-and-clipboard
plan: 03
subsystem: infra
tags: [wiring, main, transport, clipboard, display, sigterm, ctrl-c, tokio-select, signal-handling]

# Dependency graph
requires:
  - phase: 03-display-and-clipboard/03-01
    provides: DisplayManager::detect_and_init(), shutdown(), DisplayEnvironment enum
  - phase: 03-display-and-clipboard/03-02
    provides: watch_clipboard(), ClipboardWriter, check_clipboard_tools()
  - phase: 02-transport
    provides: transport::client(), transport::server(), Frame
provides:
  - Wired local daemon: clipboard watcher -> Frame -> transport client
  - Wired remote daemon: display init -> tool check -> transport server -> clipboard write
  - SIGTERM + Ctrl-C clean shutdown for both subcommands
  - transport::server() accepts DisplayEnvironment and writes frames to clipboard
affects: [binary, cssh-local, cssh-remote, integration-tests]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - tokio::select! for concurrent task + signal handling (local and remote)
    - tokio::signal::unix::signal(SignalKind::terminate()) for SIGTERM on remote
    - tokio::signal::ctrl_c() for Ctrl-C on both subcommands
    - watch_handle.abort() to cancel clipboard watcher on Ctrl-C
    - ClipboardWriter::new(display_env) created per-connection in server()
    - watch_clipboard() sends Frame::new_png(png_bytes) directly (not raw Vec<u8>)

key-files:
  created: []
  modified:
    - src/main.rs (local arm: clipboard watcher spawn + select; remote arm: display init + tool check + SIGTERM + shutdown)
    - src/transport.rs (server() takes DisplayEnvironment param, writes frames via ClipboardWriter)
    - src/clipboard.rs (watch_clipboard signature changed from Sender<Vec<u8>> to Sender<Frame>)
    - src/lib.rs (added pub mod clipboard; pub mod display; for integration test access)

key-decisions:
  - "watch_clipboard() signature changed to Sender<Frame> (Option A) — simpler than a converter task"
  - "ClipboardWriter created per-connection in server() — each connection gets fresh writer state"
  - "display_mgr.env is Copy so passes cleanly to server() without consuming display_mgr"
  - "watch_handle.abort() called after Ctrl-C to ensure watcher doesn't linger"
  - "display_mgr.shutdown() called unconditionally after select! exits for clean Xvfb teardown"

patterns-established:
  - "Pattern: tokio::select! with signal handlers for daemon lifetime management"
  - "Pattern: Copy enum (DisplayEnvironment) passed to server() preserving ownership of DisplayManager for shutdown"

requirements-completed: [DISP-01, DISP-04, CLRD-01, CLWR-01, CLWR-04, CLWR-05]

# Metrics
duration: 2min
completed: 2026-02-27
---

# Phase 3 Plan 03: Integration Wiring Summary

**watch_clipboard()->Sender<Frame> feeds transport::client() on local side; transport::server(display_env) writes received frames via ClipboardWriter on remote side; SIGTERM/Ctrl-C trigger clean Xvfb shutdown**

## Performance

- **Duration:** 2 min
- **Started:** 2026-02-27T16:55:22Z
- **Completed:** 2026-02-27T16:57:13Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments

- `src/main.rs` local arm: removes `_tx` placeholder, spawns `clipboard::watch_clipboard(tx)` as background tokio task, uses `tokio::select!` for transport client vs Ctrl-C, calls `watch_handle.abort()` on shutdown
- `src/main.rs` remote arm: calls `display::DisplayManager::detect_and_init()`, checks clipboard tools, installs SIGTERM handler via `tokio::signal::unix::signal`, uses `tokio::select!` for server vs signal, calls `display_mgr.shutdown()` unconditionally after select
- `src/transport.rs`: `server()` gains `display_env: DisplayEnvironment` parameter, creates `ClipboardWriter::new(display_env)` per connection, calls `writer.write(&frame.payload)` on every received frame
- `src/clipboard.rs`: `watch_clipboard()` signature changed to `Sender<Frame>`, wraps PNG bytes in `Frame::new_png()`, `#[allow(dead_code)]` removed
- `src/lib.rs`: added `pub mod clipboard; pub mod display;` for integration test access
- All 10 tests pass: 8 protocol unit tests + 2 transport integration tests

## Task Commits

Each task was committed atomically:

1. **Task 1: Wire clipboard.rs — watch_clipboard sends Frame** - `84e9d99` (feat)
2. **Task 1+2: Wire main.rs local + remote pipelines** - `74bc7b6` (feat)
3. **Task 2: Update transport.rs server + lib.rs** - `64aeafe` (feat)

## Files Created/Modified

- `src/main.rs` - Full local pipeline (clipboard watcher + transport client + Ctrl-C) and remote pipeline (display init + tool check + SIGTERM + server + shutdown)
- `src/transport.rs` - server() updated: DisplayEnvironment param, ClipboardWriter per connection, writes frames to clipboard
- `src/clipboard.rs` - watch_clipboard() sends Frame (not Vec<u8>), #[allow(dead_code)] removed
- `src/lib.rs` - Re-exports clipboard and display modules for integration tests

## Decisions Made

- Changed `watch_clipboard()` to `Sender<Frame>` (Option A from plan) — eliminates a converter task, simpler, and `protocol::Frame` was already importable in clipboard.rs.
- `ClipboardWriter` is created per-connection inside `server()` rather than being passed in. This keeps the server signature minimal — only the `DisplayEnvironment` enum (Copy) is needed.
- `display_mgr.env` is passed to `server()` by copy, preserving `display_mgr` ownership for the subsequent `shutdown()` call after the select exits.
- `watch_handle.abort()` is called after Ctrl-C so the blocking clipboard watcher thread is cancelled and doesn't hold up process exit.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None. `cargo build` and `cargo test` pass on first attempt.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Phase 3 is complete: display module, clipboard module, and full daemon wiring are all done
- `cssh local --host <tailscale-host>` will watch clipboard, encode to PNG, send over TCP
- `cssh remote` will init display, check tools, receive frames, write to clipboard
- Clean shutdown (SIGTERM/Ctrl-C) kills Xvfb and removes `~/.cssh/display`
- Phase 4 (CLI/UX polish, status command, logging improvements) can begin

---
*Phase: 03-display-and-clipboard*
*Completed: 2026-02-27*

## Self-Check: PASSED

- FOUND: src/main.rs
- FOUND: src/transport.rs
- FOUND: src/clipboard.rs
- FOUND: src/lib.rs
- FOUND: 03-03-SUMMARY.md
- FOUND: commit 84e9d99 (feat(03-03): wire local subcommand)
- FOUND: commit 74bc7b6 (feat(03-03): wire main.rs local + remote pipelines)
- FOUND: commit 64aeafe (feat(03-03): update transport server to write frames to clipboard)
