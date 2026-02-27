# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-27)

**Core value:** Ctrl-V on the remote machine pastes the local screenshot into the CLI tool — no extra steps, no file juggling
**Current focus:** Phase 3 — Display and Clipboard

## Current Position

Phase: 3 of 4 (Display and Clipboard)
Plan: 2 of ? in current phase
Status: In progress
Last activity: 2026-02-27 — Plan 03-02 complete: ClipboardReader (watch_clipboard) + ClipboardWriter (xclip/wl-copy subprocess) in src/clipboard.rs

Progress: [███░░░░░░░] 30%

## Performance Metrics

**Velocity:**
- Total plans completed: 3
- Average duration: 2.3 min
- Total execution time: 0.12 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 01-foundation | 1 | 2 min | 2 min |
| 02-transport | 1 | 3 min | 3 min |
| 03-display-and-clipboard | 2 | 4 min | 2 min |

**Recent Trend:**
- Last 5 plans: 01-01 (2 min), 02-01 (3 min), 03-01 (2 min), 03-02 (2 min)
- Trend: stable

*Updated after each plan completion*

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.
Recent decisions affecting current work:

- [Init]: Single binary with `cssh local` / `cssh remote` subcommands — simpler distribution
- [Init]: TCP over Tailscale, no custom auth — Tailscale handles encryption + identity
- [Init]: Use xclip/wl-copy subprocess for remote clipboard write — they handle X11 selection ownership (stay alive serving SelectionRequest events)
- [Init]: Xvfb for headless clipboard — xclip needs a display server; Xvfb is lightweight and reliable
- [Init]: Image-only (no text) — focused scope
- [01-01]: Port 34782 chosen as default — high unregistered range, avoids common conflicts
- [01-01]: MAGIC = [0xC5, 0x53] — 0xC5 is non-ASCII, 0x53 is 'S' (cssh)
- [01-01]: Frame::to_bytes returns Result to enforce TooLarge guard on payloads > u32::MAX
- [01-01]: #[allow(dead_code)] on protocol.rs keeps cargo build warning-free until Phase 2
- [02-01]: Port changed from 34782 to 9877 (plan spec for transport phase)
- [02-01]: server() is single-client-at-a-time for v1 — simpler error handling, sufficient for use case
- [02-01]: "auto" sentinel string in bind_addr signals Tailscale IP auto-detection in server()
- [02-01]: src/lib.rs added for dual binary+lib crate — enables integration tests to import from cssh::
- [02-01]: Frame gets #[derive(Debug)] — required by Result::unwrap() in integration tests
- [02-01]: rand::random::<f64>() used for jitter — rand 0.10 removed thread_rng().gen_range() API
- [03-01]: libc::pipe() (not pipe2/O_CLOEXEC) for Xvfb -displayfd fd inheritance
- [03-01]: Arc<Mutex<Option<Child>>> for shared Xvfb child handle between main and monitor task
- [03-01]: #[allow(dead_code)] at display.rs module level — items wired up in plans 03-02/03-03
- [03-01]: set_var(DISPLAY) with #[allow(deprecated)] — safe at startup before tokio multi-thread spawns
- [Phase 03-02]: arboard polling loop runs in single spawn_blocking (not per-poll) to keep !Send Clipboard on one thread
- [Phase 03-02]: xclip/wl-copy subprocess stored without .wait() — required for X11 selection ownership persistence
- [Phase 03-02]: startup-skip: initial clipboard hash recorded but NOT sent to avoid re-sending stale screenshot on restart

### Pending Todos

None yet.

### Roadmap Evolution

- Phase 5 added: Peer-to-peer mesh with Tailscale auto-discovery and SSH-triggered activation

### Blockers/Concerns

- [Research]: xclip `-loops 0` background-fork behavior needs empirical testing on target Ubuntu
- [Research]: wl-copy clipboard ownership lifetime (does process need to stay running?) needs testing
- [Research]: Whether Claude Code/Codex/OpenCode use subprocess xclip or OSC 52 affects Phase 3/4 path
- [Note]: arboard Wayland feature flag resolved — using `wayland-data-control` feature as specified in research

## Session Continuity

Last session: 2026-02-27
Stopped at: Completed 03-02-PLAN.md — ClipboardReader + ClipboardWriter in src/clipboard.rs complete
Resume file: None
