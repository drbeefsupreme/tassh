# Project Research Summary

**Project:** codex-screenshot-ssh (cssh)
**Domain:** Clipboard bridge daemon — unidirectional image sync over Tailscale/TCP
**Researched:** 2026-02-27
**Confidence:** MEDIUM (no external web access during research; training data only; stack crate versions verified via cargo search)

## Executive Summary

`codex-screenshot-ssh` is a focused, single-purpose Linux daemon that bridges clipboard images from a local Ubuntu machine to a headless remote Ubuntu machine over Tailscale, so that CLI tools like Claude Code and Codex can receive screenshots via Ctrl-V. No general-purpose tool exists for this exact use case: OSC 52 does not support images, `lemonade` and `clipper` are text-only, and `clipcat`/CopyQ are full clipboard managers rather than lightweight bridges. This project is genuinely novel in its combination of image-aware clipboard watching, PNG-over-TCP transport, headless X11 display management (Xvfb), and DISPLAY propagation to SSH sessions.

The recommended approach is a single Rust binary (`cssh`) with two subcommand modes: `cssh local` (clipboard watcher + TCP sender) and `cssh remote` (TCP listener + Xvfb manager + clipboard writer). The local side polls the clipboard, deduplicates via content hash, frames PNG bytes with a 4-byte length prefix, and sends over Tailscale TCP. The remote side manages an Xvfb instance for headless display, writes received images to the X11 clipboard via xclip, and publishes the DISPLAY variable to a file that SSH sessions source. Both daemons run as systemd user services with `Restart=on-failure`. This layered architecture is well-understood and the dependency ordering is clear: display management must precede clipboard write, which must precede transport, which must precede deduplication.

The dominant risks are all in the X11/display layer, not the networking layer. X11 clipboard ownership requires the process to stay alive serving SelectionRequest events — a process that writes to the clipboard and exits immediately causes silent empty pastes. Xvfb lifecycle management (stale lock files, zombie processes) is a common failure mode. DISPLAY propagation to SSH sessions is the hardest debugging scenario: the daemon appears healthy but Ctrl-V produces nothing. These risks are well-documented and have known mitigations. The Tailscale transport layer is comparatively simple and low-risk.

---

## Key Findings

### Recommended Stack

The stack is solidly Rust-native with strategic use of subprocess delegation for clipboard I/O. `arboard` (3.6.1) handles clipboard reading on the local side because it abstracts X11/Wayland without requiring the developer to manage the X11 event loop. For the remote clipboard write, the recommendation is to shell out to `xclip` or `wl-copy` rather than using arboard — these tools correctly handle X11 selection ownership (they background-fork and serve SelectionRequest events). Tokio (1.49.0) provides the async runtime for TCP. `clap` (4.5.60) provides CLI subcommand parsing. The `image` crate (0.25.9) handles PNG encoding and validation. `sha2`/xxHash handles content hashing. `tracing` + `anyhow` provide logging and error handling.

**Core technologies:**
- `arboard` 3.6.1: clipboard read on local side — actively maintained, handles X11 event loop internally
- `tokio` 1.49.0: async TCP for both sender and listener — ecosystem standard
- `clap` 4.5.60: CLI subcommand derive macros — idiomatic Rust CLI
- `image` 0.25.9: PNG encode/validate — needed for format normalization and magic byte verification
- `sha2` / `xxhash`: content hashing for dedup — hash decoded pixel data, not PNG bytes (PNG encoding is non-deterministic)
- `tracing` + `anyhow`: structured logging and error propagation
- `xclip` / `wl-copy` (subprocess): remote clipboard write — these handle X11 selection ownership correctly
- `Xvfb` (subprocess): headless X11 display — managed by daemon as an owned child process

**Explicitly rejected:** The `clipboard` crate (abandoned), `copypasta` (text-only), `serde`+JSON for wire protocol (overkill — length-prefixed frames are simpler), `arboard` for remote write (does not handle X11 selection ownership for long-lived content).

See `.planning/research/STACK.md` for full rationale and version details.

### Expected Features

**Must have (table stakes):**
- Clipboard watch on local machine (Wayland event-driven via `wl-paste --watch` + X11 polling fallback)
- Image MIME type detection — filter to `image/png` only before sending
- PNG-encoded TCP transport with 4-byte length-prefixed framing
- Remote clipboard write via `wl-copy` (Wayland) or `xclip -selection clipboard -t image/png` (X11)
- Xvfb spawn and management for headless remote
- Content hash dedup (hash decoded pixels, not PNG bytes) to suppress re-sends on poll interval
- Reconnection with exponential backoff (1s → 2s → ... → 30s cap)
- Systemd user service units for both `cssh-local` and `cssh-remote`
- Single binary with `cssh local` / `cssh remote` subcommands
- Basic logging via stderr + `tracing`

**Should have (differentiators):**
- DISPLAY export to `/run/user/<UID>/cssh.env` for SSH session sourcing
- Startup health check (ping remote before entering watch loop)
- Configurable poll interval (default 500ms)
- Image size limit with configurable cap (default 10–20MB)
- Verbose/debug mode via `RUST_LOG=debug`
- Structured JSON logging option (`--log-format json`)

**Defer to v2+:**
- Structured JSON logging (add after basic logging works)
- Health check subcommand (add after core flow is stable)
- DISPLAY auto-export via PAM/`~/.pam_environment` (complex; document manual workaround first)
- Content-addressed dedup with configurable TTL window (simple last-hash is sufficient for MVP)

**Anti-features (explicitly out of scope):** text clipboard sync, bidirectional sync, TLS/custom auth, macOS/Windows support, GUI, plugin system, clipboard format negotiation.

See `.planning/research/FEATURES.md` for full analysis including existing tool comparison.

### Architecture Approach

Two independent async Tokio daemons in one binary, sharing protocol types and framing logic via a library module. The local daemon is a linear pipeline: ClipboardWatcher → HashFilter → Framer → TcpSender (with reconnect loop). The remote daemon has a startup branch: DisplayManager detects environment (Wayland/X11/headless) and optionally spawns Xvfb before the main loop begins; then: TcpListener → Deframer → ClipboardWriter. Components are layered with clear dependencies, enabling bottom-up build and test. The wire protocol is deliberately minimal: `[u32 big-endian length][PNG bytes]`, with PNG magic byte validation on receipt.

**Major components:**
1. `ClipboardWatcher` (local) — polls system clipboard, emits image bytes on change
2. `HashFilter` (local) — SHA-256/xxHash of decoded pixels, drops duplicates, stateful per session
3. `Framer` / `Deframer` (shared) — length-prefixed binary framing for TCP stream
4. `TcpSender` (local) — reconnect loop with exponential backoff, Tailscale endpoint
5. `TcpListener` (remote) — accepts single connection, re-listens on disconnect
6. `DisplayManager` (remote) — detects Wayland/X11/headless at startup, spawns and owns Xvfb child
7. `ClipboardWriter` (remote) — dispatches to `wl-copy` or `xclip` based on display environment
8. `SessionEnvPublisher` (remote) — writes `DISPLAY=:N` to `/run/user/<UID>/cssh.env`

**Build order:** Protocol types → DisplayManager → ClipboardReader/Writer → Framer/Deframer → TcpSender/Receiver → HashFilter + signal handling → CLI wiring.

See `.planning/research/ARCHITECTURE.md` for component diagrams, code patterns, and data flow.

### Critical Pitfalls

1. **X11 clipboard ownership — process must stay alive** — The clipboard does not persist after the writing process exits. Never write and exit. For the remote, use `xclip` (which background-forks) or keep a Rust X11 event loop alive as the selection owner. Test: write to clipboard, wait 200ms, then Ctrl-V — content must still be there.

2. **Xvfb stale lock files and zombie orphans** — If Xvfb crashes without cleanup, `/tmp/.X99-lock` blocks restart. If the daemon exits without killing Xvfb, the orphan holds the display number. Fix: startup cleanup routine (check lock file PID is alive via `kill -0`, remove stale lock), signal handler (SIGTERM kills Xvfb child), systemd `KillMode=control-group`.

3. **DISPLAY not propagated to SSH sessions** — The daemon's environment does not transfer to user SSH sessions. `xclip` called by Claude Code fails silently with "Can't open display." Fix: write `DISPLAY=:99` to `/run/user/<UID>/cssh.env`; document that users source this from `~/.bashrc`. Test the full path: daemon running → new SSH session → `echo $DISPLAY` → `xclip -o`.

4. **X11 INCR protocol — large images silently truncated** — X11 has a per-request size limit (~256KB). Images larger than this require INCR (incremental transfer) protocol. Verify that the chosen Rust clipboard library handles INCR on the owner side. Test with a 3840x2160 screenshot.

5. **Wayland false-positive detection** — `WAYLAND_DISPLAY` being set does not guarantee a compositor is reachable from the SSH session. Test actual connection, not just the env var. On headless remotes, default to Xvfb+xclip regardless of display vars.

Additional moderate pitfalls: TCP partial writes (use `write_all()`/`read_exact()` always), silent disconnects after sleep/wake (enable TCP keepalive + write timeout), PNG non-determinism breaking hash dedup (hash decoded pixels not PNG bytes), systemd service missing `Environment=DISPLAY=:99` (set explicitly in unit file).

See `.planning/research/PITFALLS.md` for full pitfall inventory with warning signs and phase mapping.

---

## Implications for Roadmap

Based on the architecture's layered dependency order and pitfall severity mapping, the following phase structure is recommended.

### Phase 1: Foundation — Project Setup and Protocol Types

**Rationale:** All other components depend on shared protocol types, error types, and the framing protocol. Building this first establishes the contract every other component satisfies. No pitfalls here; this is pure Rust module structure.
**Delivers:** Binary scaffold with clap subcommands, `Frame` struct, `DisplayEnvironment` enum, unified error types, framing/deframing logic, and a test harness for the wire protocol.
**Addresses:** Single binary requirement, wire protocol definition, PNG format normalization.
**Avoids:** Protocol drift between sender and receiver by defining types once.
**Research flag:** None — standard Rust project patterns, skip research-phase.

### Phase 2: Transport Layer — TCP Send and Receive with Reconnect

**Rationale:** The transport layer has no dependency on clipboard or display logic, making it independently testable. Establishing the TCP pipeline early enables integration testing of all later components over actual Tailscale connections.
**Delivers:** `TcpSender` with exponential backoff reconnect loop, `TcpListener` with re-listen on disconnect, length-prefixed framing in both directions, PNG magic byte validation on receipt, TCP keepalive and write timeout configuration.
**Implements:** Framer, Deframer, TcpSender, TcpReceiver components.
**Avoids:** Pitfall 7 (partial writes — use `write_all()`/`read_exact()`), Pitfall 8 (silent disconnects — keepalive + write timeout), Pitfall 13 (bind to Tailscale interface IP, not 0.0.0.0).
**Research flag:** None — TCP patterns are well-established; standard Tokio docs sufficient.

### Phase 3: Headless Display Management — Xvfb Lifecycle

**Rationale:** DisplayManager is a prerequisite for ClipboardWriter on the remote side. This is the highest-risk subsystem (lock files, zombies, DISPLAY propagation) and should be isolated and stabilized before being coupled to clipboard write logic. Solving it here prevents it from contaminating Phase 4 debugging.
**Delivers:** `DisplayManager` with Wayland/X11/headless detection, Xvfb spawn with stale-lock cleanup, child process ownership with SIGTERM handler, `SessionEnvPublisher` writing DISPLAY to `/run/user/<UID>/cssh.env`.
**Avoids:** Pitfall 3 (Wayland false-positive — test connection, not env var), Pitfall 4 (stale lock files — startup cleanup), Pitfall 5 (zombie Xvfb — signal handler + `KillMode=control-group`), Pitfall 6 (DISPLAY not in SSH sessions — publish to file).
**Research flag:** Needs verification — confirm `tokio::process::Command` signal handling API for SIGTERM propagation to child process groups; verify xclip behavior with Xvfb display at `:99`.

### Phase 4: Clipboard I/O — Read and Write

**Rationale:** With transport (Phase 2) and display management (Phase 3) proven, clipboard read and write can be implemented and tested end-to-end. This is where the X11 selection ownership pitfall must be addressed.
**Delivers:** `ClipboardWatcher` (local, via arboard or subprocess dispatch), `HashFilter` with pixel-level dedup, `ClipboardWriter` (remote, via subprocess xclip/wl-copy dispatch), MIME type enforcement (`-t image/png`), image size limit enforcement, PNG round-trip validation.
**Addresses:** Clipboard watch, image detection, dedup, remote clipboard write from FEATURES.md.
**Avoids:** Pitfall 1 (X11 selection ownership — use xclip background-fork; verify with timing test), Pitfall 2 (INCR protocol — verify library handles large images; test with 4K screenshot), Pitfall 9 (wrong image format/target atom), Pitfall 12 (PNG non-determinism — hash pixels not bytes), Pitfall 15 (PRIMARY vs CLIPBOARD selection — always specify CLIPBOARD).
**Research flag:** Needs verification — test `xclip` background-fork behavior with `-loops 0`; verify arboard Wayland feature flag requirements; test wl-copy `--type image/png` persistence after process exit.

### Phase 5: Integration and Systemd Packaging

**Rationale:** With all components implemented, wire them into the two subcommand pipelines and package as systemd services. This phase is low-risk but requires attention to environment variable propagation in systemd context.
**Delivers:** `cssh local` full pipeline (ClipboardWatcher → HashFilter → Framer → TcpSender), `cssh remote` full pipeline (TcpListener → Deframer → ClipboardWriter via DisplayManager), `cssh-local.service` and `cssh-remote.service` unit files, manual DISPLAY sourcing documentation.
**Addresses:** Systemd service units, reconnection, single binary — all table stakes from FEATURES.md.
**Avoids:** Pitfall 10 (systemd missing DISPLAY — set `Environment=DISPLAY=:99` explicitly in unit file), Pitfall 11 (daemon starts before Xvfb ready — retry loop or ExecStartPre readiness check), Pitfall 6 (SSH DISPLAY propagation — document `source ~/.run/user/.../cssh.env` in README).
**Research flag:** None — systemd unit file patterns are well-documented; standard patterns apply.

### Phase 6: Hardening and Polish (post-MVP)

**Rationale:** These features add robustness and observability but do not block the core workflow.
**Delivers:** Startup health check (`cssh status`), structured JSON logging, configurable poll interval and size limit, verbose/debug mode, DISPLAY auto-export improvement (PAM or `environment.d/`).
**Addresses:** All differentiator features from FEATURES.md.
**Research flag:** None — all are additive features on a working base.

### Phase Ordering Rationale

- **Foundation first:** Protocol types and framing must be shared across both sides; defining them in Phase 1 prevents protocol drift.
- **Transport before clipboard:** Transport is independently testable (can send arbitrary bytes); this provides an integration test harness for Phase 4.
- **Xvfb before clipboard write:** ClipboardWriter depends on DisplayManager having run detection and potentially spawned Xvfb. Isolating Xvfb lifecycle in its own phase prevents the highest-risk subsystem from contaminating clipboard debugging.
- **Clipboard last among core phases:** Only after transport and display are proven does it make sense to add clipboard I/O, because clipboard failures are the hardest to diagnose (silent empty pastes, timing-dependent ownership loss).
- **Systemd last:** Packaging is only meaningful when all components work in isolation; systemd adds environment constraints that complicate debugging.

---

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | Crate versions verified via `cargo search` on 2026-02-27; arboard, tokio, clap, image are all current and actively maintained |
| Features | MEDIUM | Web access unavailable; findings from training data (August 2025 cutoff); no live verification of lemonade/clipper/clipcat current state |
| Architecture | MEDIUM | Patterns are well-established (TCP framing, polling dedup, subprocess dispatch); specific API details (arboard Wayland flags, tokio signal API) need verification against current docs before implementation |
| Pitfalls | MEDIUM | X11/Wayland pitfalls are from ICCCM/Wayland specs (stable standards, HIGH confidence); crate-specific behavior (xclip background-fork, arboard INCR handling) needs empirical testing |

**Overall confidence:** MEDIUM — sufficient to begin implementation with verification steps built into each phase.

### Gaps to Address

- **arboard Wayland support status:** Feature flags and minimum compositor requirements changed between versions. Verify against https://docs.rs/arboard before using for local clipboard read.
- **xclip background-fork behavior:** Whether `xclip -loops 0` reliably holds selection ownership after writing needs empirical testing on target Ubuntu version. If it does not, need a Rust-level X11 event loop.
- **wl-copy persistence:** Whether `wl-copy --type image/png` holds clipboard ownership after process exit (or requires process to stay running) must be tested on Wayland.
- **Claude Code / Codex clipboard read mechanism:** Whether these tools use OSC 52, bracketed paste, or subprocess xclip/wl-paste for clipboard reads affects whether the Xvfb+xclip path is sufficient or if a Wayland path is required even on headless remotes.
- **INCR protocol handling:** Whether `arboard` or `xclip` (as a write target) correctly handles the X11 INCR protocol for images >256KB must be verified with a large screenshot before considering Phase 4 complete.

---

## Sources

### Primary (HIGH confidence)
- `.planning/PROJECT.md` — project requirements and constraints (first-party)
- `cargo search` results — crate versions verified 2026-02-27

### Secondary (MEDIUM confidence)
- Training data knowledge of X11 ICCCM specification — clipboard ownership, selection protocol, INCR protocol
- Training data knowledge of Wayland `wlr-data-control-unstable-v1` protocol
- Training data knowledge of Xvfb man page — lock file locations, display number management
- Training data knowledge of Linux TCP socket API — `SO_KEEPALIVE`, partial write behavior
- Training data knowledge of systemd unit file behavior — `KillMode`, `Environment=`, ordering vs readiness
- Training data knowledge of arboard, x11-clipboard, tokio, clap crates (knowledge cutoff August 2025)

### Tertiary (LOW confidence — needs live verification)
- Clipboard behavior of Claude Code, Codex, OpenCode — inferred from TUI conventions; not verified against source
- xclip `-loops 0` background-fork behavior — referenced from documentation; needs empirical test
- wl-copy process lifetime and clipboard ownership — referenced from usage patterns; needs empirical test

---
*Research completed: 2026-02-27*
*Ready for roadmap: yes*
