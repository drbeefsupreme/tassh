---
phase: 05-peer-to-peer-mesh
plan: 03
subsystem: cli
tags: [clap, cli, ipc, unix-socket, systemd, ssh-config, rust]

# Dependency graph
requires:
  - phase: 05-02
    provides: run_daemon(), socket_path(), IpcMessage types, PeerRegistry
  - phase: 04-integration-and-packaging
    provides: run_setup_local, run_setup_remote, shell_snippet_command, working CLI scaffold
provides:
  - Commands::Daemon — unified daemon subcommand dispatching to daemon::run_daemon()
  - Commands::Notify — fast fire-and-forget IPC notifier for SSH LocalCommand
  - Commands::Status — daemon status query over Unix socket
  - SetupTarget::Daemon — installs tassh-daemon.service + SSH config stanza
  - run_setup_daemon() — systemd unit writer + SSH config appender
affects:
  - 05-04: integration tests can now invoke `tassh daemon`, `tassh notify`, `tassh status`

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "notify fire-and-forget: 200ms connect + 100ms write timeout pair ensures LocalCommand never delays SSH session"
    - "always-exit-0 notify: notify failure is logged at debug level only — SSH must succeed regardless"
    - "SSH config idempotency: check for existing # tassh: marker before appending stanza"
    - "LocalCommand conflict detection: warn user instead of silently overwriting existing LocalCommand entries"

key-files:
  created: []
  modified:
    - src/cli.rs
    - src/main.rs
    - src/setup.rs

key-decisions:
  - "Notify always exits 0: SSH LocalCommand failure would break the SSH connection; daemon unavailability must be silent"
  - "200ms/100ms notify timeout: short enough to be imperceptible in SSH startup, long enough for IPC round-trip on local socket"
  - "SSH config idempotency via # tassh: marker: comment-based detection is simple and survives whitespace/format changes"
  - "LocalCommand conflict detection warns rather than errors: user may have legitimate existing LocalCommand; setup completes successfully and prints the stanza for manual addition"

# Metrics
duration: 3min
completed: 2026-02-27
---

# Phase 05 Plan 03: CLI Integration — Daemon, Notify, Status, Setup Daemon Summary

**Complete CLI integration wiring `tassh daemon` to run_daemon(), `tassh notify` as a fast IPC notifier for SSH LocalCommand, `tassh status` querying the daemon over Unix socket, and `tassh setup daemon` generating tassh-daemon.service plus SSH config stanza**

## Performance

- **Duration:** 3 min
- **Started:** 2026-02-27T21:53:36Z
- **Completed:** 2026-02-27T21:56:06Z
- **Tasks:** 3
- **Files modified:** 3

## Accomplishments

- Updated src/cli.rs to add Commands::Daemon(DaemonArgs), Commands::Notify(NotifyArgs), SetupTarget::Daemon(SetupDaemonArgs); marked Local/Remote/Setup Local/Setup Remote as DEPRECATED in help text
- Updated src/main.rs to dispatch all new subcommands; added send_notify() with 200ms/100ms timeout pair (fire-and-forget, always exits 0); added run_status() that sends StatusRequest and prints peer table
- Updated src/setup.rs with run_setup_daemon() generating tassh-daemon.service unit, detecting existing SSH LocalCommand conflicts, appending ssh_config_stanza(), printing migration note
- All 28 project tests pass (13 lib + 13 bin + 2 integration)
- `tassh --help` shows daemon, notify, status, setup with deprecation notes

## Task Commits

Each task was committed atomically:

1. **Task 1: Add Daemon and Notify subcommands to src/cli.rs** - `bf9a862` (feat)
2. **Task 2: Wire subcommands in src/main.rs** - `792b478` (feat)
3. **Task 3: Add run_setup_daemon to src/setup.rs** - `2b55a80` (feat)

## Files Created/Modified

- `src/cli.rs` - Commands::Daemon(DaemonArgs), Commands::Notify(NotifyArgs), SetupTarget::Daemon(SetupDaemonArgs); deprecation notes in Local/Remote/Setup Local/Setup Remote
- `src/main.rs` - Daemon/Notify/Status/Setup Daemon dispatch; send_notify() async helper with timeout pair; run_status() IPC client
- `src/setup.rs` - run_setup_daemon(), tassh_daemon_unit(), ssh_config_stanza(); idempotency check for existing SSH config

## Decisions Made

- Notify always exits 0: SSH LocalCommand failure breaks the SSH connection — daemon unavailability must be handled silently with only a debug-level log
- 200ms connect + 100ms write timeout pair: short enough to be imperceptible in SSH startup latency, long enough for IPC round-trip on a local Unix socket
- SSH config idempotency via `# tassh:` marker: comment-based detection is simple and robust against whitespace/format changes
- LocalCommand conflict detection warns rather than errors: user may have existing LocalCommand entries that serve other purposes; setup completes and prints the stanza for manual addition

## Deviations from Plan

None — plan executed exactly as written.

## Issues Encountered

Task 1 (cli.rs) could not pass `cargo check` in isolation because main.rs had non-exhaustive match arms. This is expected mid-plan state; Tasks 1, 2, and 3 were executed sequentially before verifying. All three compiled cleanly once complete, and all 28 tests pass.

## Next Phase Readiness

- `tassh daemon`, `tassh notify`, `tassh status`, `tassh setup daemon` are all fully implemented
- Integration tests in Plan 05-04 can spawn `tassh daemon`, call `tassh notify`, and query `tassh status`
- Complete SSH-triggered clipboard sync workflow is available: `tassh setup daemon` → restart daemon → SSH to peer → automatic clipboard sync
- All 28 project tests pass
- No blockers

---
*Phase: 05-peer-to-peer-mesh*
*Completed: 2026-02-27*
