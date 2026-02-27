# Phase 4: Integration and Packaging - Context

**Gathered:** 2026-02-27
**Status:** Ready for planning

<domain>
## Phase Boundary

Wire all components from phases 1-3 into working `cssh local` and `cssh remote` pipelines, package as systemd user services with a setup subcommand, provide a shell snippet for DISPLAY sourcing, and validate the full screenshot-to-paste workflow with Claude Code, Codex, and OpenCode.

</domain>

<decisions>
## Implementation Decisions

### Systemd service behavior
- Restart=always with ~5-second delay — keeps clipboard bridge running unattended
- Auto-start on login via loginctl enable-linger + WantedBy=default.target
- Logs go to systemd journal only — `journalctl --user -u cssh-local` / `cssh-remote`
- Separate unit files: `cssh-local.service` and `cssh-remote.service` (not template units)

### Shell snippet design
- Detect SSH sessions via `$SSH_CONNECTION` — only source DISPLAY when set
- Support bash and zsh (single compatible snippet for .bashrc and .zshrc)
- Print snippet for user to copy-paste — don't auto-modify rc files
- Export DISPLAY only (not WAYLAND_DISPLAY) — remote is headless with Xvfb

### Connection configuration
- Remote address passed via CLI flag: `cssh local --remote 100.x.y.z:port`
- Fixed default port (e.g., 9737) — overridable with `--port` flag
- Remote binds to Tailscale interface only: `cssh remote --bind 100.x.y.z`
- User specifies bind address explicitly via `--bind` flag

### Setup subcommand
- `cssh setup local --remote 100.x.y.z` and `cssh setup remote --bind 100.x.y.z`
- Copies unit files to `~/.config/systemd/user/`, enables services, starts immediately
- Prints shell snippet for user to add to their rc file
- Binary installed via `cargo install --path .` on each machine

### Claude's Discretion
- Exact default port number choice
- Unit file ordering/dependency details (e.g., After=network.target)
- Exact shell snippet formatting and comments
- E2E validation test approach and tooling
- Error messages and setup output formatting

</decisions>

<specifics>
## Specific Ideas

- Setup should be a one-command experience: run `cssh setup local --remote <addr>`, service is running, paste shell snippet, done
- The systemd units should bake in the addresses from setup so the user never edits unit files manually

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 04-integration-and-packaging*
*Context gathered: 2026-02-27*
