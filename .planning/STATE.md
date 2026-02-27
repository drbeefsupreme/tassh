# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-27)

**Core value:** Ctrl-V on the remote machine pastes the local screenshot into the CLI tool — no extra steps, no file juggling
**Current focus:** Phase 1 — Foundation

## Current Position

Phase: 1 of 4 (Foundation)
Plan: 0 of ? in current phase
Status: Ready to plan
Last activity: 2026-02-27 — Roadmap created, requirements mapped to 4 phases

Progress: [░░░░░░░░░░] 0%

## Performance Metrics

**Velocity:**
- Total plans completed: 0
- Average duration: —
- Total execution time: 0 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| - | - | - | - |

**Recent Trend:**
- Last 5 plans: —
- Trend: —

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

### Pending Todos

None yet.

### Blockers/Concerns

- [Research]: arboard Wayland feature flags need verification against current docs before Phase 3
- [Research]: xclip `-loops 0` background-fork behavior needs empirical testing on target Ubuntu
- [Research]: wl-copy clipboard ownership lifetime (does process need to stay running?) needs testing
- [Research]: Whether Claude Code/Codex/OpenCode use subprocess xclip or OSC 52 affects Phase 3/4 path

## Session Continuity

Last session: 2026-02-27
Stopped at: Roadmap created — ready to begin planning Phase 1
Resume file: None
