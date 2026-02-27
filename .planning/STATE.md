# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-27)

**Core value:** Ctrl-V on the remote machine pastes the local screenshot into the CLI tool — no extra steps, no file juggling
**Current focus:** Phase 2 — Transport

## Current Position

Phase: 2 of 4 (Transport)
Plan: 1 of ? in current phase
Status: In progress
Last activity: 2026-02-27 — Plan 02-01 complete: TCP transport layer with auto-reconnect, keepalive, integration tests

Progress: [██░░░░░░░░] 20%

## Performance Metrics

**Velocity:**
- Total plans completed: 2
- Average duration: 2.5 min
- Total execution time: 0.08 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 01-foundation | 1 | 2 min | 2 min |
| 02-transport | 1 | 3 min | 3 min |

**Recent Trend:**
- Last 5 plans: 01-01 (2 min), 02-01 (3 min)
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

### Pending Todos

None yet.

### Roadmap Evolution

- Phase 5 added: Peer-to-peer mesh with Tailscale auto-discovery and SSH-triggered activation

### Blockers/Concerns

- [Research]: arboard Wayland feature flags need verification against current docs before Phase 3
- [Research]: xclip `-loops 0` background-fork behavior needs empirical testing on target Ubuntu
- [Research]: wl-copy clipboard ownership lifetime (does process need to stay running?) needs testing
- [Research]: Whether Claude Code/Codex/OpenCode use subprocess xclip or OSC 52 affects Phase 3/4 path

## Session Continuity

Last session: 2026-02-27
Stopped at: Completed 02-01-PLAN.md — TCP transport layer complete
Resume file: None
