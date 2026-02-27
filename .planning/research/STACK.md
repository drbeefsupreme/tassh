# Stack Research: codex-screenshot-ssh

**Researched:** 2026-02-27
**Confidence:** HIGH (versions verified against crates.io)

## Recommended Stack

### Core Dependencies

| Crate | Version | Purpose | Confidence |
|-------|---------|---------|------------|
| `arboard` | 3.6.1 | Cross-platform clipboard read/write (images + text) | HIGH |
| `tokio` | 1.49.0 | Async runtime for TCP networking | HIGH |
| `clap` | 4.5.60 | CLI argument parsing with derive macros | HIGH |
| `image` | 0.25.9 | PNG encode/decode, image format handling | HIGH |
| `sha2` | 0.10.x (stable) | Content hashing for dedup | HIGH |
| `tracing` | 0.1.44 | Structured logging | HIGH |
| `anyhow` | 1.0.102 | Error handling | HIGH |

### Clipboard-Specific

| Crate | Version | Purpose | Confidence |
|-------|---------|---------|------------|
| `wl-clipboard-rs` | 0.9.3 | Direct Wayland clipboard access (alternative to shelling out to wl-copy) | MEDIUM |
| `x11-clipboard` | 0.9.3 | Direct X11 clipboard access | MEDIUM |
| `x11rb` | 0.13.2 | Low-level X11 bindings (for XFIXES clipboard change events) | MEDIUM |

### Rationale

**arboard vs subprocess shelling:**
- arboard wraps platform clipboard APIs natively
- However, for the *remote writer* side, arboard may not handle the X11 selection ownership model correctly (the process must stay alive to serve clipboard requests)
- **Recommendation:** Use arboard for the *local reader* (simple, just grab current clipboard). For the *remote writer*, shell out to `xclip -selection clipboard -target image/png -i` or `wl-copy --type image/png` — these tools handle the ownership/event-loop correctly
- This avoids the biggest pitfall (clipboard going empty when the process exits)

**tokio vs async-std:**
- tokio is the ecosystem standard, better maintained, more middleware
- The TCP server/client pattern is straightforward with tokio::net
- No need for async-std's alternative approach

**image crate:**
- Needed for PNG validation and potential format conversion
- The clipboard may provide RGBA bytes; we need to encode to PNG for wire transfer
- On the remote side, we may need to provide raw RGBA bytes to xclip

**Subprocess management (Xvfb):**
- Use `tokio::process::Command` for spawning and managing Xvfb
- No dedicated crate needed — standard process management is sufficient
- Key: use process groups for clean shutdown

### What NOT to Use

| Crate/Approach | Why Not |
|----------------|---------|
| `clipboard` crate | Abandoned, doesn't support images |
| `copypasta` crate | Text-only, no image support |
| Custom X11 protocol impl | x11rb exists; don't reinvent |
| `serde` + JSON for wire protocol | Overkill — length-prefixed PNG frames are simpler and faster |
| `arboard` for remote writer | Doesn't handle X11 selection ownership correctly for long-lived clipboard content |
| `nix` for process management | `std::process` + `tokio::process` are sufficient for Xvfb lifecycle |

### CLI Structure

```
cssh local <remote-host> [--port 9737]     # Watch local clipboard, send to remote
cssh remote [--port 9737] [--display :99]  # Listen for images, write to clipboard
cssh status                                # Check daemon health
```

Use `clap` derive macros for clean subcommand definitions.

### Systemd Integration

No crate needed — provide `.service` unit files:
- `cssh-local.service` (user service, runs on local machine)
- `cssh-remote.service` (user service, runs on remote machine)
- Use `Type=simple`, `Restart=on-failure`
- Remote service needs `Environment=DISPLAY=:99` or dynamic detection

---
*Versions verified: 2026-02-27 via `cargo search`*
