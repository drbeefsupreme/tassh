---
phase: 01-foundation
plan: 01
subsystem: infra
tags: [rust, clap, tokio, thiserror, protocol, framing]

# Dependency graph
requires: []
provides:
  - Cargo.toml project manifest with clap/thiserror/tokio dependencies
  - src/cli.rs — Cli, Commands (Local/Remote/Status), LocalArgs, RemoteArgs structs
  - src/main.rs — tokio::main entry point dispatching CLI subcommands
  - src/protocol.rs — Frame, FrameError, DisplayEnvironment with to_bytes/from_bytes
  - src/transport.rs, src/clipboard.rs, src/display.rs — stub modules for future phases
affects:
  - 02-transport (Frame::to_bytes/from_bytes is the framing contract)
  - 03-clipboard (module structure and DisplayEnvironment enum)
  - 04-integration (CLI subcommand dispatch pattern)

# Tech tracking
tech-stack:
  added:
    - clap 4 (derive + env features) — CLI argument parsing
    - thiserror 2 — error derive macros
    - tokio 1 (full) — async runtime
  patterns:
    - Single binary with subcommand dispatch (cli.rs → main.rs match)
    - Length-prefixed big-endian framing: 2 magic + 1 version + 1 type + 4 length + payload
    - Self-contained serialization: Frame::to_bytes/from_bytes own all wire logic
    - #[allow(dead_code)] on protocol.rs until Phase 2 wires up consumers

key-files:
  created:
    - Cargo.toml
    - src/main.rs
    - src/cli.rs
    - src/protocol.rs
    - src/transport.rs
    - src/clipboard.rs
    - src/display.rs
  modified: []

key-decisions:
  - "Port 34782 chosen as default — high unregistered range, avoids common conflicts"
  - "MAGIC = [0xC5, 0x53] — 0xC5 is non-ASCII (prevents text confusion), 0x53 is 'S'"
  - "Frame::to_bytes returns Result<Vec<u8>, FrameError> to enforce TooLarge guard on payloads > u32::MAX"
  - "#[allow(dead_code)] added to protocol.rs so cargo build stays warning-free until Phase 2"
  - "Protocol module declared in main.rs even though Phase 1 doesn't use it — avoids module restructuring later"

patterns-established:
  - "All wire protocol logic lives in src/protocol.rs — other modules import from there"
  - "CLI args use long flags with env fallbacks: --port / CSSH_PORT pattern"
  - "Subcommand dispatch: match cli.command { Commands::X(args) => ... }"

requirements-completed: [SRVC-01, XPRT-02]

# Metrics
duration: 2min
completed: 2026-02-27
---

# Phase 1 Plan 01: Foundation Scaffold Summary

**Single-binary cssh Rust project with clap subcommand dispatch, big-endian length-prefixed frame protocol, and 8 unit tests proving byte-perfect round-trip fidelity**

## Performance

- **Duration:** 2 min
- **Started:** 2026-02-27T15:24:58Z
- **Completed:** 2026-02-27T15:27:01Z
- **Tasks:** 2
- **Files modified:** 7

## Accomplishments
- Cargo project with clap 4, thiserror 2, tokio 1 — builds clean with zero warnings
- CLI subcommands: `cssh local --remote-host X`, `cssh remote`, `cssh status` all functional with env fallbacks
- Wire protocol: `Frame` with 8-byte header (2 magic + 1 version + 1 type + 4 length) and big-endian u32 payload length
- 8 unit tests covering round-trip (PNG, empty, 64KB), all error variants (InvalidMagic, UnsupportedVersion, TooShort, LengthMismatch), and DisplayEnvironment equality

## Task Commits

Each task was committed atomically:

1. **Task 1: Scaffold Cargo project with CLI subcommands and stub modules** - `19139c4` (feat)
2. **Task 2: Implement Frame, FrameError, DisplayEnvironment with unit tests** - `6795649` (feat)

## Files Created/Modified
- `Cargo.toml` — Project manifest with clap 4 (derive+env), thiserror 2, tokio 1 (full)
- `src/main.rs` — Tokio async entry point with clap dispatch to Local/Remote/Status subcommands
- `src/cli.rs` — Cli struct, Commands enum, LocalArgs (remote_host + port), RemoteArgs (port)
- `src/protocol.rs` — Frame, FrameError (5 variants), DisplayEnvironment (4 variants), 8 unit tests
- `src/transport.rs` — Stub: TCP transport layer (Phase 2)
- `src/clipboard.rs` — Stub: Clipboard read/write operations (Phase 3)
- `src/display.rs` — Stub: Display environment detection and Xvfb management (Phase 3)

## Decisions Made
- Port 34782 chosen as default — high unregistered range, avoids common port conflicts
- MAGIC = [0xC5, 0x53]: 0xC5 is non-ASCII (no confusion with text protocols), 0x53 is 'S' (cssh)
- `Frame::to_bytes` returns `Result` (not plain `Vec`) to enforce the TooLarge guard
- Added `#[allow(dead_code)]` to protocol.rs to keep `cargo build` warning-free — symbols will be consumed in Phase 2

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Created minimal protocol.rs stub before Task 2 to enable Task 1 build**
- **Found during:** Task 1 (cargo build)
- **Issue:** `mod protocol;` in main.rs requires a protocol.rs file to exist; Task 2 hadn't run yet
- **Fix:** Created a one-line stub `//! Wire protocol framing types (implemented in Task 2)` so Task 1 could build and be committed cleanly
- **Files modified:** src/protocol.rs (stub, then replaced in Task 2)
- **Verification:** cargo build succeeded with zero warnings after stub; replaced with full implementation in Task 2
- **Committed in:** 19139c4 (Task 1 commit), then replaced by 6795649 (Task 2)

**2. [Rule 1 - Bug] Added `#[allow(dead_code)]` to silence unused symbol warnings**
- **Found during:** Task 2 (cargo build after implementing protocol.rs)
- **Issue:** All exported protocol symbols (constants, types, Frame) triggered dead_code warnings since main.rs doesn't use them yet
- **Fix:** Added `#![allow(dead_code)]` at module level — appropriate because these are designed for Phase 2 consumption
- **Files modified:** src/protocol.rs
- **Verification:** `cargo build` outputs zero warnings
- **Committed in:** 6795649 (Task 2 commit)

---

**Total deviations:** 2 auto-fixed (1 blocking, 1 bug/warning)
**Impact on plan:** Both fixes were necessary for the zero-warnings requirement. No scope creep.

## Issues Encountered
None beyond the deviations documented above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Frame framing contract (to_bytes/from_bytes) is ready for Phase 2 TCP transport
- Module structure (transport.rs, clipboard.rs, display.rs) is in place for Phase 2 and 3
- CLI subcommand dispatch is wired and will be extended in Phase 4 with real daemon logic
- No blockers for Phase 2

---
*Phase: 01-foundation*
*Completed: 2026-02-27*

## Self-Check: PASSED

- Cargo.toml: FOUND
- src/main.rs: FOUND
- src/cli.rs: FOUND
- src/protocol.rs: FOUND
- src/transport.rs: FOUND
- src/clipboard.rs: FOUND
- src/display.rs: FOUND
- .planning/phases/01-foundation/01-01-SUMMARY.md: FOUND
- Commit 19139c4: FOUND
- Commit 6795649: FOUND
