# Roadmap: codex-screenshot-ssh

## Overview

Four phases deliver a working clipboard bridge. Phase 1 establishes the shared protocol types and binary scaffold everything else depends on. Phase 2 builds and validates the TCP transport layer in isolation. Phase 3 implements the hard parts — headless display management and clipboard I/O on both sides — where most of the X11 pitfalls live. Phase 4 wires all components into the two subcommand pipelines, adds systemd packaging, and validates the full end-to-end paste workflow with real CLI tools.

Phase 5 evolves tassh from manual connection to automatic SSH-triggered activation: when a user SSHs to a host with tassh installed, clipboard sync starts automatically.

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

- [x] **Phase 1: Foundation** - Binary scaffold with clap subcommands, wire protocol types, and framing logic (completed 2026-02-27)
- [x] **Phase 2: Transport** - TCP sender and receiver with reconnect and length-prefixed framing (completed 2026-02-27)
- [x] **Phase 3: Display and Clipboard** - Xvfb lifecycle management, local clipboard reading, remote clipboard writing (completed 2026-02-27)
- [x] **Phase 4: Integration and Packaging** - Full pipeline wiring, systemd service units, shell snippet, E2E validation (completed 2026-02-27)
- [ ] **Phase 5: SSH-triggered Activation** - Unified daemon, SSH LocalCommand integration, automatic peer discovery (in progress)

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
- [x] 01-01-PLAN.md — Scaffold Cargo project with CLI subcommands and implement wire protocol framing with tests

### Phase 2: Transport
**Goal**: Arbitrary byte frames reliably traverse a TCP connection from local to remote, with automatic reconnection on failure
**Depends on**: Phase 1
**Requirements**: XPRT-01, XPRT-03
**Success Criteria** (what must be TRUE):
  1. Running `cssh local` and `cssh remote` on two machines, a test payload sent from local arrives intact on remote
  2. Killing the remote process and restarting it causes the local sender to reconnect without manual intervention
  3. TCP keepalive and write timeouts are configured so a silently dropped connection is detected within a reasonable window
  4. Partial write and partial read are handled correctly — `write_all` and `read_exact` used throughout
**Plans:** 1/1 plans complete
Plans:
- [x] 02-01-PLAN.md — TCP transport with server/client, keepalive, reconnect, and loopback integration test

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
**Plans:** 3/3 plans complete
Plans:
- [x] 03-01-PLAN.md — Add Phase 3 dependencies and implement DisplayManager (Xvfb lifecycle, stale lock cleanup, display file) (completed 2026-02-27)
- [x] 03-02-PLAN.md — Implement clipboard reading (local arboard watcher) and clipboard writing (remote xclip/wl-copy subprocess dispatch)
- [x] 03-03-PLAN.md — Wire display and clipboard into daemon main loop with SIGTERM/Ctrl-C clean shutdown

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
**Plans:** 2/2 plans complete

Plans:
- [x] 04-01-PLAN.md — Add `cssh setup` subcommand with systemd unit file generation, service orchestration, and shell snippet
- [x] 04-02-PLAN.md — E2E validation checkpoint: verify screenshot paste in Claude Code, Codex, and OpenCode

### Phase 5: SSH-triggered Activation
**Goal**: When user SSHs to a host with tassh installed, clipboard sync starts automatically without manual `tassh local --remote` invocation
**Depends on**: Phase 4
**Requirements**: SSH-01, SSH-02, SSH-03, SSH-04, MESH-01, MESH-02, MESH-03, MESH-04, MESH-05, MESH-06
**Success Criteria** (what must be TRUE):
  1. Running `ssh remotehost` triggers automatic clipboard connection if remote has tassh daemon
  2. `tassh status` shows active peer connections with session counts
  3. Multiple SSH sessions to the same host share a single clipboard connection
  4. Closing all SSH sessions to a host disconnects the clipboard sync
  5. SSH works normally (no delay, no errors) when remote has no tassh daemon
**Plans:** 1/4 plans executed

Plans:
- [ ] 05-01-PLAN.md — Add Phase 5 dependencies (async-pidfd, serde, serde_json) and create IPC types + PID watcher modules
- [ ] 05-02-PLAN.md — Implement PeerRegistry and daemon core with IPC server, remote probing, clipboard broadcast
- [ ] 05-03-PLAN.md — Wire CLI subcommands (daemon, notify, status) and add setup daemon with SSH config
- [ ] 05-04-PLAN.md — E2E validation checkpoint: verify SSH-triggered clipboard sync

## Progress

**Execution Order:**
Phases execute in numeric order: 1 -> 2 -> 3 -> 4 -> 5

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Foundation | 1/1 | Complete   | 2026-02-27 |
| 2. Transport | 1/1 | Complete    | 2026-02-27 |
| 3. Display and Clipboard | 3/3 | Complete    | 2026-02-27 |
| 4. Integration and Packaging | 2/2 | Complete   | 2026-02-27 |
| 5. SSH-triggered Activation | 1/4 | In Progress|  |
