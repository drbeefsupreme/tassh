---
phase: 02-transport
plan: 01
subsystem: transport
tags: [tcp, tokio, socket2, tracing, keepalive, reconnect, backoff, framing]

# Dependency graph
requires:
  - phase: 01-foundation
    provides: "Frame, FrameError, protocol framing (to_bytes/from_bytes)"
provides:
  - "TCP transport: server() listener, client() with auto-reconnect"
  - "send_frame / recv_frame over OwnedWriteHalf / OwnedReadHalf"
  - "TCP keepalive via socket2 (25s detection)"
  - "10s write timeout with WriteTimeout error"
  - "Exponential backoff with jitter (1s..30s) on reconnect"
  - "Tracing-based logging (silent by default, verbose with RUST_LOG)"
  - "Updated CLI: --remote, --bind, port 9877"
  - "Integration tests proving frame traversal over TCP loopback"
affects: [03-clipboard, 04-remote-paste]

# Tech tracking
tech-stack:
  added: [socket2 0.6.2, rand 0.10.0, tracing 0.1.44, tracing-subscriber 0.3.22]
  patterns:
    - "OwnedReadHalf/OwnedWriteHalf for split TCP streams"
    - "tokio::time::timeout wrapping write_all for write deadlines"
    - "SockRef::from(stream).set_tcp_keepalive for keepalive config"
    - "mpsc::Receiver<Frame> as backpressure channel into client()"
    - "Integration tests via src/lib.rs exposing pub modules"

key-files:
  created:
    - src/transport.rs
    - src/lib.rs
    - tests/transport_integration.rs
  modified:
    - Cargo.toml
    - Cargo.lock
    - src/cli.rs
    - src/main.rs
    - src/protocol.rs

key-decisions:
  - "Port changed from 34782 to 9877 (plan spec)"
  - "server() is single-client-at-a-time for v1 — no tokio::spawn per connection"
  - "Bind addr 'auto' sentinel string signals Tailscale IP auto-detection"
  - "src/lib.rs added to expose modules for integration tests (binary+lib dual crate)"
  - "Frame gets #[derive(Debug)] for integration test ergonomics"
  - "rand::random::<f64>() used instead of thread_rng().gen_range() — rand 0.10 API"

patterns-established:
  - "TransportError wraps Io, WriteTimeout, ConnectionClosed, Frame variants"
  - "recv_frame detects EOF via io::ErrorKind::UnexpectedEof -> ConnectionClosed"
  - "client() loop: connect -> keepalive -> reset backoff -> send loop -> on error break"

requirements-completed: [XPRT-01, XPRT-03]

# Metrics
duration: 3min
completed: 2026-02-27
---

# Phase 2 Plan 01: Transport Summary

**TCP transport layer with auto-reconnect client, keepalive, write timeout, and integration tests proving byte-perfect frame traversal over TCP loopback**

## Performance

- **Duration:** 3 min
- **Started:** 2026-02-27T16:06:40Z
- **Completed:** 2026-02-27T16:09:40Z
- **Tasks:** 3
- **Files modified:** 7

## Accomplishments
- Full TCP transport layer in src/transport.rs: `server()`, `client()`, `send_frame()`, `recv_frame()`, `apply_keepalive()`
- Auto-reconnect with exponential backoff (1s initial, 30s cap) and 25% jitter
- TCP keepalive configured for ~25s detection (10s idle, 5s interval, 3 retries)
- 10-second write timeout that triggers reconnect on stalled connections
- CLI updated: `--remote` flag (was `--remote-host`), `--bind` added to RemoteArgs, port default changed to 9877
- Tracing initialized in main — silent unless `RUST_LOG` is set
- Integration tests prove PNG frames traverse TCP loopback with byte-perfect fidelity

## Task Commits

Each task was committed atomically:

1. **Task 1: Add dependencies and update CLI flags** - `c05b50e` (feat)
2. **Task 2: Implement transport module and wire into main** - `c19622e` (feat)
3. **Task 3: Integration test — frame traversal over TCP loopback** - `41b37fa` (feat)

**Plan metadata:** (docs commit — see below)

## Files Created/Modified
- `src/transport.rs` - Full TCP transport: server, client, send_frame, recv_frame, apply_keepalive, backoff
- `src/lib.rs` - Exposes protocol and transport modules for integration tests
- `tests/transport_integration.rs` - Two integration tests: loopback traversal + reconnect after restart
- `Cargo.toml` - Added socket2, rand, tracing, tracing-subscriber
- `src/cli.rs` - --remote flag, --bind on RemoteArgs, port 9877 default
- `src/main.rs` - Tracing init, parse_remote(), transport::client/server dispatch
- `src/protocol.rs` - Removed #![allow(dead_code)], added #[derive(Debug)] to Frame

## Decisions Made
- Used `rand::random::<f64>()` instead of `thread_rng().gen_range()` — rand 0.10 removed the old `Rng::gen_range` method style; `rand::random()` works cleanly
- Added `src/lib.rs` to expose modules for integration tests — Rust integration tests in `tests/` can only import from library crates; adding lib.rs is the standard approach for dual binary+lib crates
- Server is single-client-at-a-time in v1 (no per-connection `tokio::spawn`) — sufficient for the use case, simplifies error handling
- Bind addr uses `"auto"` as sentinel string to signal Tailscale IP auto-detection in main.rs → server()

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Fixed main.rs using renamed field `remote_host`**
- **Found during:** Task 1 (after renaming `remote_host` to `remote` in cli.rs)
- **Issue:** main.rs still referenced `args.remote_host` which no longer existed
- **Fix:** Updated main.rs reference to `args.remote`; Task 2 then rewrote main.rs completely
- **Files modified:** src/main.rs
- **Verification:** `cargo build` passes
- **Committed in:** c05b50e (Task 1 commit)

**2. [Rule 1 - Bug] Added `#[derive(Debug)]` to Frame**
- **Found during:** Task 3 (integration test compilation)
- **Issue:** `oneshot_tx.send(frame).unwrap()` requires `Frame: Debug` for the `Result::unwrap()` bound
- **Fix:** Added `#[derive(Debug)]` to the `Frame` struct in protocol.rs
- **Files modified:** src/protocol.rs
- **Verification:** Integration tests compile and pass
- **Committed in:** 41b37fa (Task 3 commit)

---

**Total deviations:** 2 auto-fixed (1 blocking, 1 bug)
**Impact on plan:** Both fixes were necessary for compilation. No scope creep.

## Issues Encountered
- rand 0.10 API: `thread_rng().gen_range(0.0..x)` is no longer valid; used `rand::random::<f64>() * x` instead — produces equivalent jitter
- Integration tests require a library crate target; added src/lib.rs as standard solution

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- TCP transport is complete and tested — frames flow bidirectionally over TCP
- `client()` accepts `mpsc::Receiver<Frame>` ready to receive from Phase 3 clipboard watcher
- `server()` ready to dispatch received frames to Phase 3/4 clipboard writer
- Tailscale auto-detection in place but needs empirical testing (see existing blockers in STATE.md)

## Self-Check: PASSED

All files verified present:
- src/transport.rs: FOUND
- src/lib.rs: FOUND
- tests/transport_integration.rs: FOUND
- src/cli.rs: FOUND
- .planning/phases/02-transport/02-01-SUMMARY.md: FOUND

All commits verified present:
- c05b50e: FOUND (Task 1)
- c19622e: FOUND (Task 2)
- 41b37fa: FOUND (Task 3)

---
*Phase: 02-transport*
*Completed: 2026-02-27*
