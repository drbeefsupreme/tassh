---
phase: 05-peer-to-peer-mesh
plan: 01
subsystem: ipc
tags: [async-pidfd, serde, serde_json, unix-socket, pidfd, rust]

# Dependency graph
requires:
  - phase: 04-integration-and-packaging
    provides: working tassh binary with cssh remote/local subcommands
provides:
  - IpcMessage enum (Connect, Disconnect, StatusRequest) with serde JSON serialization
  - StatusResponse and PeerInfo structs for daemon status queries
  - watch_pid() async future for poll-free SSH process exit detection via pidfd
  - Phase 5 dependencies: async-pidfd, serde (derive), serde_json
affects:
  - 05-02: daemon.rs will use IpcMessage to receive Unix socket notifications
  - 05-03: peer.rs will call watch_pid() to detect SSH process exit events

# Tech tracking
tech-stack:
  added:
    - async-pidfd v0.1.5 (pidfd_open-based process exit watching, no polling)
    - serde v1.0.228 with derive feature (JSON serialization for IPC messages)
    - serde_json v1.0.149 (JSON encoding/decoding over Unix socket)
  patterns:
    - serde tag pattern: #[serde(tag = "type")] produces {"type": "Connect", ...} discriminated union JSON
    - Pin<Box<dyn Future<Output=()> + Send>> return type for ergonomic async task spawning
    - ESRCH errno check for graceful "already exited" handling in pidfd path

key-files:
  created:
    - src/ipc.rs
    - src/pid_watcher.rs
  modified:
    - Cargo.toml
    - Cargo.lock
    - src/lib.rs

key-decisions:
  - "serde tag on IpcMessage enum produces {\"type\": \"Connect\", ...} discriminated union JSON — matches RESEARCH.md Pattern 1"
  - "watch_pid returns Pin<Box<dyn Future + Send>> for ergonomic tokio::spawn without boxing at call site"
  - "ESRCH errno maps to immediate return (process already gone), not an error — prevents phantom disconnect events"
  - "Fallback to /proc/<pid> polling at warn level — should not occur on Ubuntu 20.04+ (kernel 5.3+)"

patterns-established:
  - "IPC discriminated union: #[serde(tag = \"type\")] on enums for self-describing JSON messages"
  - "Pidfd-based process watching: wrap async-pidfd in boxed future, handle ESRCH as immediate completion"

requirements-completed: [MESH-02, SSH-04]

# Metrics
duration: 2min
completed: 2026-02-27
---

# Phase 05 Plan 01: IPC Types and PID Watcher Foundation Summary

**IPC message types (serde JSON discriminated union) and async pidfd-based SSH process exit watcher as Phase 5 foundation contracts**

## Performance

- **Duration:** 2 min
- **Started:** 2026-02-27T21:44:38Z
- **Completed:** 2026-02-27T21:46:50Z
- **Tasks:** 3
- **Files modified:** 5

## Accomplishments

- Added async-pidfd, serde (derive), and serde_json to Cargo.toml; cargo check clean
- Created src/ipc.rs with IpcMessage, StatusResponse, PeerInfo; 5 JSON round-trip unit tests all pass
- Created src/pid_watcher.rs with watch_pid() wrapping async-pidfd; handles ESRCH and /proc fallback

## Task Commits

Each task was committed atomically:

1. **Task 1: Add Phase 5 cargo dependencies** - `c03fda2` (chore)
2. **Task 2: Create src/ipc.rs with IPC message types** - `61feb2e` (feat)
3. **Task 3: Create src/pid_watcher.rs with async-pidfd wrapper** - `6b5c7e1` (feat)

## Files Created/Modified

- `src/ipc.rs` - IpcMessage enum (Connect/Disconnect/StatusRequest), StatusResponse, PeerInfo with serde derives and 5 unit tests
- `src/pid_watcher.rs` - watch_pid() async future using async-pidfd with ESRCH handling and /proc fallback
- `src/lib.rs` - Added pub mod ipc and pub mod pid_watcher declarations
- `Cargo.toml` - async-pidfd v0.1.5, serde v1.0.228 (derive), serde_json v1.0.149
- `Cargo.lock` - Updated with 11 new locked packages

## Decisions Made

- `serde tag` discriminated union pattern (`#[serde(tag = "type")]`) chosen for self-describing JSON that includes the variant name — matches RESEARCH.md Pattern 1 and is human-readable for debugging
- `watch_pid` returns `Pin<Box<dyn Future<Output=()> + Send>>` so callers can do `tokio::spawn(watch_pid(pid))` without extra boxing
- ESRCH (no such process) is treated as immediate completion, not an error — this is the correct behavior when SSH already exited before the daemon could open a pidfd on it

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None - all three tasks compiled and tested on first attempt.

## Next Phase Readiness

- src/ipc.rs and src/pid_watcher.rs are ready for import by daemon.rs and peer.rs in Plans 05-02 and 05-03
- All 23 project tests pass (5 new IPC tests + 18 pre-existing)
- No blockers

---
*Phase: 05-peer-to-peer-mesh*
*Completed: 2026-02-27*
