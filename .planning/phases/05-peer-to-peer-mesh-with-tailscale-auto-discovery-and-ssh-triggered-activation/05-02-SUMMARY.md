---
phase: 05-peer-to-peer-mesh
plan: 02
subsystem: daemon
tags: [tokio, broadcast, unix-socket, ipc, peer-registry, tcp-server, pidfd, rust]

# Dependency graph
requires:
  - phase: 05-01
    provides: IpcMessage types, watch_pid(), serde/serde_json/async-pidfd dependencies
  - phase: 04-integration-and-packaging
    provides: working tassh binary with display/clipboard/transport infrastructure
provides:
  - PeerRegistry: multi-peer connection state management keyed by hostname
  - PeerState: session count, connected flag, watched_pids HashSet, close_tx
  - run_daemon(): unified daemon entry point (IPC server + TCP server + clipboard broadcast)
  - DEFAULT_PORT: u16 constant (9877)
  - socket_path(): Unix socket path at ~/.tassh/daemon.sock
affects:
  - 05-03: CLI subcommands will call run_daemon(), send IPC Connect/Disconnect messages
  - 05-04: integration tests will exercise daemon IPC and peer mesh end-to-end

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "mpsc-to-broadcast bridge: watch_clipboard() uses mpsc::Sender<Frame>; daemon bridges to broadcast::Sender<Arc<Frame>> for multi-peer distribution"
    - "Arc<Frame> broadcast: avoids PNG payload cloning per subscriber — send once, receive N times"
    - "Unix socket single-instance: connect-test before bind detects live daemon; remove_file() clears stale socket"
    - "close_tx drop signal: mpsc::Sender<()> dropped to signal peer connection task to shut down cleanly"
    - "PID deduplication: watched_pids HashSet prevents double-counting ControlMaster SSH sessions"

key-files:
  created:
    - src/peer.rs
    - src/daemon.rs
  modified:
    - src/lib.rs

key-decisions:
  - "mpsc-to-broadcast bridge: watch_clipboard() returns mpsc channel; daemon spawns a relay task to forward to broadcast::Sender — decouples clipboard watcher API from multi-peer distribution concern"
  - "Arc<Frame> in broadcast channel: avoids cloning potentially large PNG payloads for each connected peer"
  - "Single-instance via Unix socket: connect-test before bind is simpler and more reliable than PID files (no stale PID edge cases)"
  - "probe_remote() 3s TCP timeout: short enough to not delay SSH startup noticeably, long enough for Tailscale route establishment"

# Metrics
duration: 2min
completed: 2026-02-27
---

# Phase 05 Plan 02: Daemon Core — PeerRegistry, IPC Server, TCP Server, Clipboard Broadcast Summary

**Unified daemon core with PeerRegistry for multi-host state, Unix socket IPC server, remote daemon probing via TCP, and broadcast channel distributing clipboard frames to all connected peers**

## Performance

- **Duration:** 2 min
- **Started:** 2026-02-27T21:48:49Z
- **Completed:** 2026-02-27T21:50:53Z
- **Tasks:** 3
- **Files modified:** 3

## Accomplishments

- Created src/peer.rs with PeerRegistry (HashMap-keyed peer state) and PeerState (session_count, connected, watched_pids, pid_watcher_handles, close_tx)
- Created src/daemon.rs with run_daemon() implementing single-instance Unix socket, Xvfb init, clipboard watcher bridge, TCP server, and IPC dispatch loop
- Updated src/lib.rs to export daemon and peer modules alongside existing modules
- All 23 project tests pass (13 lib + 8 main + 2 integration)

## Task Commits

Each task was committed atomically:

1. **Task 1: Create src/peer.rs with PeerRegistry and PeerState** - `49895f7` (feat)
2. **Task 2: Create src/daemon.rs with IPC server and peer orchestration** - `a4ccf0f` (feat)
3. **Task 3: Update src/lib.rs to export new modules** - included in Tasks 1 and 2 commits (lib.rs was updated incrementally)

## Files Created/Modified

- `src/peer.rs` - PeerRegistry (get_or_create, get, get_mut, remove, list_peers, subscribe_clipboard) and PeerState with Arc<Frame> broadcast channel
- `src/daemon.rs` - run_daemon(), handle_ipc_connection(), handle_connect(), handle_disconnect(), handle_pid_exit(), probe_remote(), start_peer_connection(), run_tcp_server(), resolve_tailscale_ip()
- `src/lib.rs` - Added pub mod daemon and pub mod peer declarations

## Decisions Made

- `mpsc-to-broadcast bridge` chosen over changing watch_clipboard() signature — decouples the clipboard watcher API (already established in Phase 3) from the multi-peer broadcast concern; a relay task spawned in daemon.rs bridges the two channel types
- `Arc<Frame>` in the broadcast channel avoids cloning potentially large PNG payloads for each connected peer — one Arc allocation, N reference increments
- Single-instance guard via Unix socket connect-test is simpler and more reliable than PID files — no stale PID edge cases from crashes
- `probe_remote()` uses a 3-second TCP connect timeout — short enough to not delay SSH startup noticeably, long enough for Tailscale route establishment after SSH connection

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None - all three tasks compiled and tested on first attempt, all 23 tests pass.

## Next Phase Readiness

- src/daemon.rs and src/peer.rs are ready for import by CLI subcommands in Plan 05-03
- run_daemon() is the entry point that Plan 05-03 will wire to `tassh daemon` subcommand
- socket_path() is the canonical path that Plan 05-03 IPC client will use to send Connect/Disconnect messages
- All 23 project tests pass
- No blockers

---
*Phase: 05-peer-to-peer-mesh*
*Completed: 2026-02-27*
