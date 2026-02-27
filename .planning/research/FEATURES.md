# Feature Landscape

**Domain:** Clipboard bridge daemon for image sync over SSH/Tailscale
**Researched:** 2026-02-27
**Confidence note:** Web access unavailable during this session. Findings draw on training-data knowledge of the tools named in the research brief (lemonade, clipper, clipcat, CopyQ, xclip, OSC 52). Confidence levels reflect this. No external verification was possible.

---

## Table Stakes

Features users expect. Missing = product feels incomplete or broken.

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| Clipboard watch on local machine | Core function — nothing works without it | Low | Poll interval or inotify/event-driven; xclip -selection clipboard or wl-paste |
| Image detection (not text) | Must distinguish image MIME types from text clipboard content | Low | Check MIME: `image/png`, `image/jpeg`, `image/bmp` before sending |
| Send image over TCP | Core transport — no TCP, no bridge | Medium | Length-prefixed framing to handle binary safely; Tailscale handles auth |
| Receive and write to remote clipboard | The other half of the bridge | Medium | wl-copy (Wayland) or xclip -selection clipboard (X11); requires DISPLAY |
| X11 and Wayland support on remote | Both display servers are in active use on Ubuntu | Medium | Detect WAYLAND_DISPLAY vs DISPLAY env vars; fall back gracefully |
| Headless remote support (Xvfb) | Remote may have no display server; xclip/wl-copy need one | High | Spawn Xvfb, set DISPLAY, export for SSH sessions to inherit |
| Duplicate suppression via content hash | Without this, every poll interval re-sends the same image | Low | Hash image bytes (SHA-256 or xxhash); store last-sent hash |
| Systemd service units | Daemons that die on reboot are not daemons | Low | Two .service files: cssh-local.service, cssh-remote.service |
| Single binary with subcommands | Users expect one thing to install | Low | `cssh local` and `cssh remote` subcommands |
| Reconnection on TCP disconnect | Network interruptions are normal; daemon should survive them | Medium | Retry loop with backoff on the local side; remote listens persistently |
| Basic logging | Without logs, debugging failures is impossible | Low | stderr + optional --log-file; log each image sent/received with timestamp |

---

## Differentiators

Features that set this product apart. Not universally expected, but materially valuable.

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| PNG-only wire format (not raw bitmap) | Keeps payload small; PNG is lossless and universally supported | Low | Re-encode to PNG before sending if source is BMP/JPEG; predictable format |
| Automatic DISPLAY export for SSH sessions | Most headless solutions require manual env setup; auto-export makes it zero-config | High | Write DISPLAY to a known file (e.g. /tmp/cssh-display); source in .bashrc or systemd env |
| Content-addressed dedup with configurable window | Smarter than simple last-hash; prevents re-sends across reconnects | Medium | Store last N hashes with timestamp; configurable TTL |
| Configurable polling interval | Different users have different latency tolerances | Low | --interval flag; default 500ms is a reasonable balance |
| Startup health check | On launch, verify remote is reachable before entering watch loop | Low | Single ping/test frame at startup; exit with clear error if remote unreachable |
| Structured log format (JSON) | Enables log aggregation and monitoring | Low | --log-format json flag; default to human-readable |
| Image size limit with configurable cap | Prevent accidentally sending huge images over the network | Low | --max-bytes flag; default 10MB; reject with log warning |
| Verbose/debug mode | Essential for development and user troubleshooting | Low | --verbose or RUST_LOG=debug |

---

## Anti-Features

Features to deliberately NOT build. Scope protection.

| Anti-Feature | Why Avoid | What to Do Instead |
|--------------|-----------|-------------------|
| Text clipboard syncing | Scope creep; SSH already handles text paste reasonably; adds complexity | Document that text is out of scope; keep focus |
| Bidirectional sync (remote → local) | Not needed for the screenshot workflow; doubles implementation complexity | One-directional only: local → remote |
| Clipboard history / manager | That is CopyQ's job; this is a bridge, not a manager | Users who want history use CopyQ separately |
| TLS / custom auth layer | Tailscale already provides encryption and identity; layering TLS wastes effort | Document Tailscale as the security layer |
| Non-Tailscale networking (arbitrary IPs) | Opens auth questions and security surface this tool is not designed for | Tailscale-only; document the constraint clearly |
| macOS / Windows support | Both machines are Ubuntu; cross-platform adds build complexity | Linux-only; document explicitly |
| Image compression/transcoding options | Adds UI complexity; PNG is good enough for screenshots | PNG only on the wire |
| Web UI or GUI | Daemons don't need GUIs; CLI tools are the consumers | CLI-only; systemd logs surface status |
| Plugin system | Over-engineering for a focused tool | Hard-code the behaviors that matter |
| Clipboard format negotiation | Adds protocol complexity; PNG is sufficient for the use case | Fixed format: PNG |

---

## Feature Dependencies

```
Xvfb management → Remote clipboard write (X11 path)
  (X11 path requires a DISPLAY; headless remotes have none)

Content hash dedup → Clipboard watch loop
  (Hashing only meaningful inside the watch loop)

Reconnection logic → TCP transport
  (Reconnect is a property of the TCP client, not the watch loop)

Systemd service → All daemon features
  (Systemd is the packaging layer, not a functional feature)

Image size limit → Send path
  (Size check happens before framing and sending)

DISPLAY export for SSH → Xvfb management
  (Only useful if Xvfb is running; must export the same DISPLAY Xvfb used)
```

---

## Existing Tools: Feature Inventory

Analysis of named tools from the research brief. Confidence: MEDIUM (training data, unverified against current repos).

### OSC 52
- **What it does:** Terminal escape sequence that writes to the local clipboard from a remote shell
- **Strengths:** Zero infrastructure, works over any SSH session, no daemon needed
- **Critical limitation:** Binary data (images) are NOT supported by most terminal emulators via OSC 52; it is designed for base64-encoded text. Ghostty has OSC 52 support but typically for text only.
- **Why not sufficient:** Cannot reliably bridge image/png clipboard content to SSH remote

### lemonade (github.com/pocke/lemonade)
- **What it does:** TCP-based clipboard bridge; wraps pbcopy/pbpaste/xdg-open over RPC
- **Strengths:** Handles text clipboard over SSH; simple daemon model
- **Limitations:** Text-focused; no first-class image MIME type handling; Go, not Rust; no Xvfb management; no systemd integration built-in
- **Why not sufficient for this project:** No image support; no headless display management

### clipper (github.com/wincent/clipper)
- **What it does:** Clipboard access for local and remote tmux sessions via Unix socket
- **Strengths:** Reliable for text; integrates with tmux workflows
- **Limitations:** Text-only; requires tmux; no image support; macOS-centric history
- **Why not sufficient:** Text-only; tmux dependency; no image MIME

### clipcat (github.com/xrelkd/clipcat)
- **What it does:** Clipboard manager daemon for X11/Wayland on Linux; gRPC-based
- **Strengths:** Proper X11/Wayland abstraction; event-driven clipboard watching (not polling); clipboard history; supports multiple MIME types including images
- **Limitations:** Full clipboard manager scope (history, search); overkill for bridge-only use; gRPC adds complexity; no SSH transport built-in
- **Why informative:** Its X11/Wayland abstraction layer is the right approach for the remote-side clipboard write

### CopyQ
- **What it does:** Advanced clipboard manager with GUI, history, scripting
- **Strengths:** Supports image MIME types; cross-platform
- **Limitations:** GUI application; not a daemon bridge; no SSH transport
- **Why not sufficient:** Wrong product category

### xclip / wl-copy (direct forwarding)
- **What it does:** CLI tools that read/write clipboard; can be used with SSH forwarding
- **Strengths:** Universally available; scriptable
- **Limitations:** Require DISPLAY; xclip requires X11; wl-copy requires Wayland; no persistent daemon; no dedup; manual invocation
- **Why informative:** These are the write primitives this project wraps; not alternatives

---

## Clipboard Watching: Polling vs Event-Driven

| Approach | How | Pros | Cons | Confidence |
|----------|-----|------|------|------------|
| Polling with xclip | `xclip -selection clipboard -t TARGETS -o` on interval | Simple; works everywhere | CPU overhead; latency = poll interval | HIGH |
| Polling with wl-paste | `wl-paste --watch` or interval poll | Wayland-native | wl-paste --watch is event-driven, not polling | HIGH |
| X11 event-driven (XFIXES) | Subscribe to XFixesSelectSelectionInput events | Low latency; no CPU waste | Requires X11 dev libs in Rust (x11rb crate) | MEDIUM |
| Wayland event-driven | wl-paste --watch blocks until clipboard changes | True event-driven; no polling | Wayland only | HIGH |
| inotify | Not applicable to clipboard (not a file) | N/A | N/A | HIGH |

**Recommendation for this project:** Use `wl-paste --watch` on Wayland (event-driven, low latency) and polling via `xclip` on X11. The local machine runs Ghostty on Ubuntu which likely uses Wayland; polling at 250-500ms is acceptable for local daemon.

---

## Image Format Considerations

| Format | Wire Use | Notes | Confidence |
|--------|----------|-------|------------|
| PNG | Recommended | Lossless; universal; reasonable size for screenshots | HIGH |
| JPEG | Avoid on wire | Lossy; screenshots with text degrade badly | HIGH |
| BMP | Avoid on wire | Uncompressed; enormous; no benefit | HIGH |
| WebP | Not necessary | More complex decode; no benefit over PNG for this use case | MEDIUM |
| Raw RGBA | Avoid | No compression; larger than PNG | HIGH |

**Decision:** Re-encode any source format to PNG before transmission. MIME type on remote clipboard should be set to `image/png`.

---

## Security Considerations

| Concern | Risk | Mitigation | Confidence |
|---------|------|------------|------------|
| Clipboard data in transit | Images may contain sensitive content | Tailscale encrypts all traffic; no additional TLS needed | HIGH |
| Unauthenticated TCP listener on remote | Any process on the tailnet could push content | Bind listener to Tailscale interface IP only (not 0.0.0.0); Tailscale ACLs provide identity | MEDIUM |
| Large payload DoS | Malicious or accidental huge image exhausts memory | Enforce max-bytes limit before accepting full frame | MEDIUM |
| Clipboard injection from network | Malicious content written to clipboard | Out of scope for this tool; document Tailscale trust model | LOW |
| Xvfb socket permissions | Other users could attach to Xvfb display | Set DISPLAY to :99 or higher; document that this is a single-user tool | MEDIUM |

---

## Service Management Features

| Feature | Standard in Category | Complexity | Notes |
|---------|---------------------|------------|-------|
| systemd unit files | Yes — expected | Low | WantedBy=default.target or multi-user.target; Restart=on-failure |
| Auto-start on login | Yes — expected | Low | systemctl --user enable cssh-local.service |
| Restart on failure | Yes — expected | Low | Restart=on-failure; RestartSec=5 in unit file |
| Log via journald | Yes — expected | Low | systemd captures stdout/stderr automatically |
| Reconnection with backoff | Yes — expected | Medium | Exponential backoff in TCP client retry loop |
| Health/status command | Nice to have | Low | `cssh status` subcommand that checks if daemon is running |
| Graceful shutdown on SIGTERM | Yes — expected | Low | tokio signal handling; flush in-flight send before exit |

---

## MVP Recommendation

Prioritize for initial working version:

1. Local clipboard watch (Wayland event-driven + X11 polling fallback)
2. PNG encoding and length-prefixed TCP send to Tailscale IP
3. Remote TCP listener → write to clipboard (wl-copy / xclip)
4. Xvfb spawn and DISPLAY management on headless remote
5. Content hash dedup (avoid re-sending same image)
6. Systemd service units for both daemons

Defer to post-MVP:
- Structured JSON logging: add after basic logging works
- Health check subcommand: add after core flow is stable
- Image size limit: add after core flow is stable (low risk short-term)
- DISPLAY auto-export to SSH sessions: complex; document manual workaround first

---

## Sources

- Domain knowledge from training data (August 2025 cutoff)
- Project requirements from `.planning/PROJECT.md`
- Tools surveyed: lemonade, clipper, clipcat, CopyQ, xclip, wl-copy, OSC 52
- Web access unavailable during research session; no external URLs verified
- Confidence: MEDIUM overall (training data only; no live verification)
