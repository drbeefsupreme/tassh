# Roadmap: codex-screenshot-ssh

## Overview

Four phases deliver a working clipboard bridge. Phase 1 establishes the shared protocol types and binary scaffold everything else depends on. Phase 2 builds and validates the TCP transport layer in isolation. Phase 3 implements the hard parts — headless display management and clipboard I/O on both sides — where most of the X11 pitfalls live. Phase 4 wires all components into the two subcommand pipelines, adds systemd packaging, and validates the full end-to-end paste workflow with real CLI tools.

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

- [x] **Phase 1: Foundation** - Binary scaffold with clap subcommands, wire protocol types, and framing logic (completed 2026-02-27)
- [ ] **Phase 2: Transport** - TCP sender and receiver with reconnect and length-prefixed framing
- [ ] **Phase 3: Display and Clipboard** - Xvfb lifecycle management, local clipboard reading, remote clipboard writing
- [ ] **Phase 4: Integration and Packaging** - Full pipeline wiring, systemd service units, shell snippet, E2E validation

## Phase Details

### Phase 1: Foundation
**Goal**: The project compiles as a single binary with two subcommand stubs, shared protocol types exist, and framing/deframing logic is tested in isolation
**Depends on**: Nothing (first phase)
**Requirements**: SRVC-01, XPRT-02
**Success Criteria** (what must be TRUE):
  1. `cssh local` and `cssh remote` subcommands exist and print usage without crashing
  2. `Frame` struct and `DisplayEnvironment` enum are defined and usable from both subcommand modules
  3. A length-prefixed PNG frame can be written and read back with byte-perfect fidelity in a unit test
  4. `cargo build` and `cargo test` pass with no warnings
**Plans:** 1/1 plans complete
Plans:
- [ ] 01-01-PLAN.md — Scaffold Cargo project with CLI subcommands and implement wire protocol framing with tests

### Phase 2: Transport
**Goal**: Arbitrary byte frames reliably traverse a TCP connection from local to remote, with automatic reconnection on failure
**Depends on**: Phase 1
**Requirements**: XPRT-01, XPRT-03
**Success Criteria** (what must be TRUE):
  1. Running `cssh local` and `cssh remote` on two machines, a test payload sent from local arrives intact on remote
  2. Killing the remote process and restarting it causes the local sender to reconnect without manual intervention
  3. TCP keepalive and write timeouts are configured so a silently dropped connection is detected within a reasonable window
  4. Partial write and partial read are handled correctly — `write_all` and `read_exact` used throughout
**Plans**: TBD

### Phase 3: Display and Clipboard
**Goal**: The remote daemon correctly manages a display environment (Wayland, X11, or Xvfb) and clipboard images written to the remote clipboard survive after the write operation; the local daemon reads new clipboard images reliably
**Depends on**: Phase 2
**Requirements**: DISP-01, DISP-02, DISP-03, DISP-04, CLRD-01, CLRD-02, CLRD-03, CLRD-04, CLWR-01, CLWR-02, CLWR-03, CLWR-04, CLWR-05
**Success Criteria** (what must be TRUE):
  1. On a headless remote, `cssh remote` spawns Xvfb, and `echo $DISPLAY` in a new SSH session shows the correct display value after sourcing `~/.cssh/display`
  2. After `cssh remote` starts with Xvfb, Ctrl-V in xclip produces the last image written — content persists because selection ownership is maintained
  3. On a local machine with a screenshot on the clipboard, the local clipboard watcher emits the image bytes (verified via log output) without polling the clipboard when it has not changed
  4. Stale Xvfb lock files from a previous crash do not prevent the remote daemon from starting cleanly
  5. Local auto-detects Wayland vs X11 and uses the correct clipboard reading path without manual configuration
**Plans**: TBD

### Phase 4: Integration and Packaging
**Goal**: Taking a screenshot on the local machine and pressing Ctrl-V inside Claude Code, Codex, or OpenCode on the remote SSH session shows the image — both daemons run as systemd user services
**Depends on**: Phase 3
**Requirements**: SRVC-02, SRVC-03, E2E-01, E2E-02, E2E-03
**Success Criteria** (what must be TRUE):
  1. Ctrl-V in Claude Code on the remote shows `[Image #1]` within a few seconds of taking a screenshot locally
  2. Ctrl-V in Codex on the remote shows the screenshot image
  3. Ctrl-V in OpenCode on the remote shows the screenshot image
  4. `systemctl --user start cssh-local` and `systemctl --user start cssh-remote` start both daemons and they survive across reboots
  5. Adding the provided shell snippet to `.bashrc` on the remote causes `$DISPLAY` to be set automatically in new SSH sessions
**Plans**: TBD

## Progress

**Execution Order:**
Phases execute in numeric order: 1 → 2 → 3 → 4

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Foundation | 1/1 | Complete   | 2026-02-27 |
| 2. Transport | 0/? | Not started | - |
| 3. Display and Clipboard | 0/? | Not started | - |
| 4. Integration and Packaging | 0/? | Not started | - |
