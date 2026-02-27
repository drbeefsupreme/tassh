---
phase: 04-integration-and-packaging
plan: 01
subsystem: infra
tags: [systemd, clap, rust, setup, service-management]

# Dependency graph
requires:
  - phase: 03-display-and-clipboard
    provides: cssh local/remote subcommands that the unit file ExecStart lines invoke
provides:
  - src/setup.rs with systemd unit file generation, systemctl orchestration, loginctl linger, shell snippet output
  - cssh setup local --remote <host> and cssh setup remote --bind <ip> CLI subcommands
  - Binary path verification before service installation
affects:
  - 04-integration-and-packaging (future plans using setup infrastructure)

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Nested clap subcommands: Commands::Setup { target: SetupTarget } with SetupTarget::Local(SetupLocalArgs) and SetupTarget::Remote(SetupRemoteArgs)"
    - "loginctl enable-linger treated as advisory: failure prints warning to stderr but does not abort setup"
    - "Binary existence check before service install with actionable error message pointing to cargo install"

key-files:
  created:
    - src/setup.rs
  modified:
    - src/cli.rs
    - src/main.rs
    - src/lib.rs

key-decisions:
  - "loginctl enable-linger failure is a warning not a fatal error — linger may need elevated privileges on some systems"
  - "Binary path hardcoded to ~/.cargo/bin/cssh — matches cargo install --path . default install location"
  - "Shell snippet uses POSIX sh syntax (. not source) for bash and zsh compatibility"
  - "pub mod cli added to lib.rs alongside pub mod setup to enable integration test access to CLI types"

patterns-established:
  - "Setup module pattern: home_dir/binary_path/unit_dir path helpers + unit file generator + run_setup orchestrator + public entry points"
  - "systemctl orchestration: daemon-reload → enable → start sequence with anyhow error propagation"

requirements-completed: [SRVC-02, SRVC-03]

# Metrics
duration: 3min
completed: 2026-02-27
---

# Phase 04 Plan 01: Setup Subcommand Summary

**`cssh setup local/remote` generates systemd user service unit files with Restart=always, runs systemctl daemon-reload/enable/start, and prints a POSIX shell snippet for DISPLAY auto-export in SSH sessions**

## Performance

- **Duration:** 3 min
- **Started:** 2026-02-27T17:27:06Z
- **Completed:** 2026-02-27T17:30:00Z
- **Tasks:** 1
- **Files modified:** 4 (src/cli.rs, src/main.rs, src/lib.rs, src/setup.rs)

## Accomplishments
- Added `cssh setup` nested subcommand with `Local(SetupLocalArgs)` and `Remote(SetupRemoteArgs)` variants to the CLI
- Created `src/setup.rs` with full systemd unit file generation, systemctl orchestration (daemon-reload → enable → start), loginctl linger with graceful warning-on-failure, and POSIX shell snippet output
- Verified binary exists at `~/.cargo/bin/cssh` before attempting service installation, with actionable error message
- All 10 existing tests continue to pass; cargo build clean

## Task Commits

Each task was committed atomically:

1. **Task 1: Add Setup subcommand to CLI and create setup module** - `9de1f16` (feat)

**Plan metadata:** (docs commit below)

## Files Created/Modified
- `src/setup.rs` - Unit file generation, systemctl orchestration, loginctl linger, shell snippet, binary path verification
- `src/cli.rs` - Added `Commands::Setup`, `SetupTarget` enum, `SetupLocalArgs` (--remote, --port), `SetupRemoteArgs` (--bind, --port)
- `src/main.rs` - Added `mod setup;` and `Commands::Setup { target }` dispatch arm calling `setup::run_setup_local` / `setup::run_setup_remote`
- `src/lib.rs` - Added `pub mod cli` and `pub mod setup` for integration test access

## Decisions Made
- `loginctl enable-linger` failure is a warning, not a fatal error — some systems require elevated privileges or the user session may already persist
- Binary path hardcoded to `~/.cargo/bin/cssh` to match `cargo install --path .` default; provides a clear error with fix instructions if missing
- Shell snippet uses `. "$HOME/.cssh/display"` (POSIX source) not `source` for portability across bash and zsh
- Exposed `pub mod cli` in `lib.rs` so that `setup.rs` can use `SetupLocalArgs`/`SetupRemoteArgs` from the lib crate context

## Deviations from Plan

None — plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None — no external service configuration required.

## Next Phase Readiness
- `cssh setup local/remote` is fully wired; users can install the service with a single command after `cargo install --path .`
- Phase 4 plan 02 (if any) can build on the setup infrastructure
- Concern: `loginctl enable-linger` may silently fail on systems without logind — documented as blocker in STATE.md

## Self-Check: PASSED

- FOUND: src/setup.rs
- FOUND: src/cli.rs
- FOUND: src/main.rs
- FOUND: src/lib.rs
- FOUND: .planning/phases/04-integration-and-packaging/04-01-SUMMARY.md
- FOUND commit: 9de1f16

---
*Phase: 04-integration-and-packaging*
*Completed: 2026-02-27*
