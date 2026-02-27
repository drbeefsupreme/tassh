# codex-screenshot-ssh

## What This Is

A Rust clipboard bridge that auto-syncs screenshot images from a local Ubuntu machine to a remote Ubuntu machine over Tailscale. It enables seamless Ctrl-V image pasting into CLI AI tools (Claude Code, Codex, OpenCode) running on remote SSH sessions, as if the tools were running locally.

## Core Value

Ctrl-V on the remote machine pastes the local screenshot into the CLI tool — no extra steps, no file juggling, just seamless image pasting over SSH.

## Requirements

### Validated

(None yet — ship to validate)

### Active

- [ ] Local daemon watches system clipboard for new image content
- [ ] Images are sent over TCP to remote machine via Tailscale network
- [ ] Remote daemon receives images and writes them to system clipboard
- [ ] Remote handles Wayland display environments (wl-copy)
- [ ] Remote handles X11 display environments (xclip)
- [ ] Remote handles headless environments by managing its own Xvfb
- [ ] Ctrl-V in Claude Code on remote shows the local screenshot as [Image #1]
- [ ] Ctrl-V in Codex on remote shows the local screenshot
- [ ] Ctrl-V in OpenCode on remote shows the local screenshot
- [ ] Content hashing avoids re-sending duplicate images
- [ ] Both daemons can run as systemd services
- [ ] Single Rust binary with subcommands (cssh local, cssh remote)

### Out of Scope

- Text clipboard syncing — only images, to keep it focused
- Bidirectional sync (remote → local) — not needed for the screenshot use case
- Non-Tailscale networking — Tailscale provides encryption and identity, no need for custom auth
- Image display in the terminal — the CLI tools handle display, this tool just bridges the clipboard
- macOS or Windows support — both machines are Ubuntu

## Context

- Local machine: Ubuntu with Ghostty terminal, default Ubuntu screenshot tool
- Remote machine: Ubuntu, accessed via plain SSH over Tailscale
- Remote may or may not have a display server (headless or with X11/Wayland)
- CLI tools (Claude Code, Codex, OpenCode) read clipboard images via system tools (xclip, wl-paste) when user presses Ctrl-V
- Over SSH, the remote clipboard is disconnected from the local clipboard — that's the gap this tool bridges
- Tailscale provides stable hostnames and encrypted networking between machines

## Constraints

- **Language**: Rust — user preference
- **Networking**: Tailscale TCP only — both machines are on the same tailnet
- **Protocol**: Simple length-prefixed PNG frames — no need for complex serialization
- **Display fallback**: Must manage Xvfb lifecycle on headless remotes and expose DISPLAY for SSH sessions

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Single binary with subcommands | Simpler distribution — one cargo install, two modes | — Pending |
| TCP over Tailscale (no custom auth) | Tailscale already handles encryption + identity between machines | — Pending |
| Auto-sync (daemon) over on-demand | User wants seamless experience — no manual trigger needed | — Pending |
| Xvfb for headless clipboard | xclip needs a display server; Xvfb is lightweight and reliable | — Pending |
| Image-only (no text) | Focused scope — text clipboard over SSH already works fine | — Pending |

---
*Last updated: 2026-02-27 after initialization*
