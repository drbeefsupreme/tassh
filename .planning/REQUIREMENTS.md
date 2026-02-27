# Requirements: codex-screenshot-ssh

**Defined:** 2026-02-27
**Core Value:** Ctrl-V on the remote machine pastes the local screenshot into the CLI tool — no extra steps, no file juggling

## v1 Requirements

### Transport

- [ ] **XPRT-01**: Local daemon connects to remote daemon over TCP via Tailscale
- [ ] **XPRT-02**: Images are framed as length-prefixed PNG payloads
- [ ] **XPRT-03**: Connection automatically reconnects with exponential backoff on loss

### Clipboard Reading (Local)

- [ ] **CLRD-01**: Local daemon watches system clipboard for new image content
- [ ] **CLRD-02**: Local daemon supports Wayland clipboard reading (wl-paste)
- [ ] **CLRD-03**: Local daemon supports X11 clipboard reading (arboard/xclip)
- [ ] **CLRD-04**: Local daemon auto-detects display environment (Wayland vs X11)

### Clipboard Writing (Remote)

- [ ] **CLWR-01**: Remote daemon writes received images to system clipboard
- [ ] **CLWR-02**: Remote daemon supports X11 clipboard writing via xclip
- [ ] **CLWR-03**: Remote daemon supports Wayland clipboard writing via wl-copy
- [ ] **CLWR-04**: Remote daemon auto-detects display environment at runtime
- [ ] **CLWR-05**: Remote daemon maintains X11 selection ownership (process stays alive to serve clipboard requests)

### Headless Display Management

- [ ] **DISP-01**: Remote daemon spawns Xvfb when no display server is available
- [ ] **DISP-02**: Remote daemon cleans up stale Xvfb lock files on startup
- [ ] **DISP-03**: Remote daemon publishes DISPLAY to ~/.cssh/display for SSH sessions
- [ ] **DISP-04**: Remote daemon manages Xvfb lifecycle (clean shutdown on exit)

### CLI / Service

- [ ] **SRVC-01**: Single binary with `cssh local`, `cssh remote`, `cssh status` subcommands
- [ ] **SRVC-02**: Systemd user service unit files for both local and remote daemons
- [ ] **SRVC-03**: Shell snippet for .bashrc/.zshrc to auto-source DISPLAY on SSH login

### End-to-End

- [ ] **E2E-01**: User takes screenshot on local machine, Ctrl-V in Claude Code on remote shows [Image #1]
- [ ] **E2E-02**: User takes screenshot on local machine, Ctrl-V in Codex on remote shows the image
- [ ] **E2E-03**: User takes screenshot on local machine, Ctrl-V in OpenCode on remote shows the image

## v2 Requirements

### Transport

- **XPRT-04**: Content hash dedup — don't re-send identical screenshots
- **XPRT-05**: Configurable image size limits

### CLI / Service

- **SRVC-04**: Structured logging via tracing crate with configurable levels
- **SRVC-05**: Health check shows connection status, display environment, last image timestamp

### Display

- **DISP-05**: Auto-detect and reuse existing display server instead of spawning Xvfb

## Out of Scope

| Feature | Reason |
|---------|--------|
| Text clipboard syncing | Only images; text clipboard already works over SSH |
| Bidirectional sync (remote → local) | Not needed for the screenshot use case |
| Non-Tailscale networking / custom auth | Tailscale provides encryption and identity |
| Image display in terminal | CLI tools handle display; this tool bridges the clipboard |
| macOS or Windows support | Both machines are Ubuntu |
| Multiple simultaneous remote targets | Single remote is sufficient for the use case |
| OSC 52 clipboard protocol | Text-only; doesn't support images |

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| XPRT-01 | Phase 2 | Pending |
| XPRT-02 | Phase 1 | Pending |
| XPRT-03 | Phase 2 | Pending |
| CLRD-01 | Phase 3 | Pending |
| CLRD-02 | Phase 3 | Pending |
| CLRD-03 | Phase 3 | Pending |
| CLRD-04 | Phase 3 | Pending |
| CLWR-01 | Phase 3 | Pending |
| CLWR-02 | Phase 3 | Pending |
| CLWR-03 | Phase 3 | Pending |
| CLWR-04 | Phase 3 | Pending |
| CLWR-05 | Phase 3 | Pending |
| DISP-01 | Phase 3 | Pending |
| DISP-02 | Phase 3 | Pending |
| DISP-03 | Phase 3 | Pending |
| DISP-04 | Phase 3 | Pending |
| SRVC-01 | Phase 1 | Pending |
| SRVC-02 | Phase 4 | Pending |
| SRVC-03 | Phase 4 | Pending |
| E2E-01 | Phase 4 | Pending |
| E2E-02 | Phase 4 | Pending |
| E2E-03 | Phase 4 | Pending |

**Coverage:**
- v1 requirements: 22 total
- Mapped to phases: 22
- Unmapped: 0

---
*Requirements defined: 2026-02-27*
*Last updated: 2026-02-27 after roadmap creation*
