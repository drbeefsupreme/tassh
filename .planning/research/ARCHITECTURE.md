# Architecture Patterns

**Domain:** Clipboard bridge daemon (image-only, unidirectional, over Tailscale/TCP)
**Researched:** 2026-02-27
**Confidence:** MEDIUM — primary sources unavailable in this session; architecture derived from project constraints in PROJECT.md, well-established patterns for clipboard daemons, Xvfb lifecycle management, and TCP framing. Flag specific library APIs for verification.

---

## Recommended Architecture

Two subcommand modes in a single binary (`cssh local` and `cssh remote`), each a small async Tokio daemon. No shared runtime state between modes.

```
LOCAL MACHINE                          REMOTE MACHINE (headless or display)
─────────────────────────────          ──────────────────────────────────────
 ┌─────────────────────────┐            ┌──────────────────────────────────┐
 │      cssh local          │            │         cssh remote               │
 │                          │            │                                    │
 │  ┌─────────────────┐    │            │  ┌────────────────────────────┐   │
 │  │ ClipboardWatcher │    │            │  │    DisplayManager           │   │
 │  │                  │    │            │  │  (detect Wayland/X11/none) │   │
 │  │ polls arboard or │    │            │  │  spawn Xvfb if headless    │   │
 │  │ wl-paste/xclip   │    │            │  │  export DISPLAY env var    │   │
 │  └────────┬─────────┘    │            │  └───────────────┬────────────┘   │
 │           │ ImageEvent   │            │                  │ DISPLAY ready  │
 │  ┌────────▼─────────┐    │            │  ┌───────────────▼────────────┐   │
 │  │  HashFilter      │    │            │  │    ClipboardWriter          │   │
 │  │ (skip duplicates)│    │            │  │  (wl-copy / xclip / Xvfb)  │   │
 │  └────────┬─────────┘    │            │  └───────────────▲────────────┘   │
 │           │ new image    │            │                  │ Vec<u8> PNG    │
 │  ┌────────▼─────────┐    │            │  ┌───────────────┴────────────┐   │
 │  │  Framer          │    │  TCP/TLS   │  │    Deframer                 │   │
 │  │ [4-byte len][PNG]│────┼────────────┼─▶│ read len, read body        │   │
 │  └──────────────────┘    │  Tailscale │  └────────────────────────────┘   │
 │                          │            │                                    │
 └─────────────────────────┘            └──────────────────────────────────┘
```

---

## Component Boundaries

### Local side (`cssh local`)

| Component | Responsibility | Communicates With |
|-----------|---------------|-------------------|
| `ClipboardWatcher` | Polls system clipboard for new PNG/image content; detects change via hash comparison | `HashFilter` |
| `HashFilter` | Computes SHA-256 (or xxhash) of raw PNG bytes; drops duplicates; stores last-seen hash | `Framer` |
| `Framer` | Encodes `[u32 big-endian length][PNG bytes]` into a byte stream; handles backpressure | `TcpSender` |
| `TcpSender` | Maintains TCP connection to remote over Tailscale; implements reconnect loop with exponential backoff | Network |

### Remote side (`cssh remote`)

| Component | Responsibility | Communicates With |
|-----------|---------------|-------------------|
| `TcpListener` | Accepts a single connection from the trusted local machine; re-listens on disconnect | `Deframer` |
| `Deframer` | Reads length prefix, reads exactly N bytes; validates PNG magic bytes before passing on | `ClipboardWriter` |
| `DisplayManager` | Detects display environment at startup; spawns and owns Xvfb process if headless; exports `DISPLAY` env var into process environment | `ClipboardWriter` |
| `ClipboardWriter` | Writes image bytes to clipboard using the correct backend (wl-copy, xclip, or xclip against Xvfb display) | System clipboard tools |
| `SessionEnvPublisher` | Writes `DISPLAY=:N` (and optionally `WAYLAND_DISPLAY`) to a known file (e.g., `/run/user/UID/cssh-env`) so SSH sessions can source it | Filesystem |

---

## Data Flow

### Forward path (local screenshot → remote clipboard)

```
1. [User presses screenshot key on local Ubuntu]
   Ubuntu screenshot tool writes PNG to system clipboard

2. ClipboardWatcher (polling loop, ~500ms interval)
   Reads clipboard image bytes via arboard or wl-paste/xclip subprocess
   → Option<Vec<u8>>

3. HashFilter
   SHA-256(bytes) == last_hash?  →  DROP
   SHA-256(bytes) != last_hash?  →  UPDATE last_hash, FORWARD bytes

4. Framer
   Prepend [u32 big-endian: len(bytes)]
   Concatenate with bytes
   → Write to TcpStream

5. TcpSender (Tokio async)
   Buffered write to Tailscale TCP socket
   Flush after each frame

6. [Network: Tailscale encrypted TCP]

7. TcpListener (remote, Tokio)
   Accept connection (one at a time)

8. Deframer
   Read 4 bytes → u32 len
   Read exactly len bytes → body
   Validate PNG magic: [0x89, 0x50, 0x4E, 0x47, ...]  →  reject malformed
   → Vec<u8>

9. ClipboardWriter
   Dispatch on DisplayEnvironment:
     Wayland  → spawn wl-copy, pipe bytes to stdin
     X11      → spawn xclip -selection clipboard -t image/png, pipe bytes to stdin
     Headless → spawn xclip -display :N -selection clipboard -t image/png, pipe bytes to stdin
              (where :N is the Xvfb display managed by DisplayManager)

10. [CLI tool on remote (Claude Code / Codex / OpenCode)]
    User presses Ctrl-V
    Tool reads clipboard via wl-paste or xclip subprocess
    Receives PNG bytes
    Renders as inline image in terminal
```

### Xvfb startup path (headless remote)

```
1. cssh remote starts
2. DisplayManager::detect()
   Check $WAYLAND_DISPLAY set and socket exists → Wayland mode, done
   Check $DISPLAY set and X server reachable (xdpyinfo) → X11 mode, done
   Neither → Headless mode

3. Headless mode:
   Find free display number N (try :1, :2, ... until lock file absent)
   spawn Xvfb :N -screen 0 1x1x24 &
   Wait for X server ready (poll xdpyinfo -display :N, max 5s)
   Store display_num = N in DisplayManager

4. SessionEnvPublisher
   Write "DISPLAY=:N\n" to /run/user/$(id -u)/cssh.env
   (SSH sessions source this file from ~/.bashrc or ~/.zshrc)

5. On cssh remote shutdown (SIGTERM):
   Kill Xvfb child process
   Remove /run/user/.../cssh.env
```

---

## How CLI Tools Read Clipboard Images

**Confidence: MEDIUM** — inferred from terminal/TUI conventions; not verified against source code of each tool.

Claude Code, Codex, and OpenCode are TUI applications running in a terminal. When the user presses Ctrl-V:

1. The TUI reads from the terminal's paste buffer (bracketed paste, OSC 52, or direct system clipboard read).
2. On Linux, the most common path is a subprocess call to `wl-paste --type image/png` (Wayland) or `xclip -selection clipboard -t image/png -o` (X11).
3. The tool receives raw PNG bytes on stdout, detects the PNG magic header, and renders inline.

**Implication for architecture:** The remote clipboard must hold the image in the system clipboard's `image/png` MIME type (not just raw bytes). Both `wl-copy --type image/png` and `xclip -selection clipboard -t image/png` handle this correctly when piped PNG bytes.

**Critical:** For headless (Xvfb) mode, the SSH session environment must have `DISPLAY=:N` pointing at the Xvfb display. Without this, `xclip` invoked by the CLI tool will fail silently or error. The `SessionEnvPublisher` component solves this by writing the env to a file that SSH sessions source.

---

## Patterns to Follow

### Pattern 1: Polling with hash deduplication (clipboard watcher)

**What:** No reliable push notification for clipboard changes exists on all Linux environments. Poll on a short interval (250–500ms); compute a hash of image bytes; only forward when hash changes.

**Why:** On X11, `xfixes` extension provides `XFixesSelectSelectionInput` events but requires an X window; not available headless. On Wayland, `wlr-data-control` protocol allows event-driven watching but is compositor-dependent. Polling is the only universal approach across X11, Wayland, and the local-side (which always has a display).

**When:** Always use on the local watcher. Keep poll interval configurable.

```rust
// Pseudocode structure
loop {
    let bytes = read_clipboard_image(); // arboard or subprocess
    if let Some(bytes) = bytes {
        let hash = xxhash(&bytes);
        if hash != state.last_hash {
            state.last_hash = hash;
            sender.send(bytes).await?;
        }
    }
    tokio::time::sleep(Duration::from_millis(POLL_INTERVAL_MS)).await;
}
```

### Pattern 2: Length-prefixed framing (TCP protocol)

**What:** Send `[u32 big-endian length (4 bytes)][PNG bytes (length bytes)]` over the TCP stream. Read exactly that many bytes on the remote side.

**Why:** TCP is a stream protocol; frames must be delimited. Length prefix is simpler than delimiter-based (no escaping) and appropriate for binary data. Big-endian is conventional for network byte order.

**When:** All TCP sends. Validate PNG magic on receipt before writing to clipboard.

```rust
// Framing (local)
let len = (bytes.len() as u32).to_be_bytes();
stream.write_all(&len).await?;
stream.write_all(&bytes).await?;

// Deframing (remote)
let mut len_buf = [0u8; 4];
stream.read_exact(&mut len_buf).await?;
let len = u32::from_be_bytes(len_buf) as usize;
let mut body = vec![0u8; len];
stream.read_exact(&mut body).await?;
```

### Pattern 3: Reconnect loop with exponential backoff (TcpSender)

**What:** The local sender wraps all TCP operations in a retry loop. On any IO error, wait (start 1s, double each failure, cap at 30s), then reconnect. Do not exit the process.

**Why:** Daemons must survive network blips, remote machine reboots, and Tailscale reconnections. The remote listener must also re-listen after a connection drop.

```rust
let mut backoff = Duration::from_secs(1);
loop {
    match connect_and_run(&addr).await {
        Ok(()) => { backoff = Duration::from_secs(1); }
        Err(e) => {
            tracing::warn!("connection failed: {e}, retrying in {backoff:?}");
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(Duration::from_secs(30));
        }
    }
}
```

### Pattern 4: Child process ownership (Xvfb lifecycle)

**What:** `DisplayManager` owns the `Child` handle from `tokio::process::Command`. Register a SIGTERM handler that kills the child before exit.

**Why:** Orphaned Xvfb processes block display numbers on restart. Xvfb lock files in `/tmp/.X{N}-lock` must be cleaned up.

```rust
// On startup
let child = Command::new("Xvfb")
    .args([":1", "-screen", "0", "1x1x24"])
    .spawn()?;

// On SIGTERM (tokio signal handler)
signal::ctrl_c().await?;
child.kill().await?;
std::fs::remove_file("/tmp/.X1-lock").ok();
```

### Pattern 5: Display environment detection order

**What:** Check in order: (1) `$WAYLAND_DISPLAY` is set and socket exists under `$XDG_RUNTIME_DIR`; (2) `$DISPLAY` is set and `xdpyinfo` succeeds; (3) neither — headless, spawn Xvfb.

**Why:** Wayland is preferred when available. X11 fallback handles older desktops. Headless handles server-only remotes.

---

## Anti-Patterns to Avoid

### Anti-Pattern 1: Assuming a single clipboard backend

**What:** Using only arboard (which selects backend at compile time on some versions) without subprocess fallback.

**Why bad:** arboard's Linux backend is X11-only in older versions; Wayland support requires the `wayland` feature and compositor supporting `wlr-data-control`. A headless system has neither.

**Instead:** Use subprocess dispatch: detect environment at runtime, call `wl-paste`/`wl-copy` on Wayland, `xclip` on X11/Xvfb. This is less elegant but universally reliable. For the local watcher (which always has a display), arboard is fine.

**Confidence:** MEDIUM — arboard Wayland support status as of 2026 should be verified against current crate docs.

### Anti-Pattern 2: Writing clipboard bytes without specifying MIME type

**What:** Calling `xclip` or `wl-copy` without `-t image/png`.

**Why bad:** CLI tools request `image/png` MIME type specifically. Without declaring the MIME type, the clipboard entry may not be returned when queried as `image/png`.

**Instead:** Always pass `-t image/png` to xclip and `--type image/png` to wl-copy.

### Anti-Pattern 3: Holding clipboard via subprocess lifetime

**What:** Some clipboard tools (especially xclip) serve clipboard content only while the process is alive. Spawning and immediately dropping the child loses the clipboard contents.

**Why bad:** The CLI tool reads the clipboard after the subprocess exits — gets nothing.

**Instead:** Spawn clipboard writer with `stdin` piped, write bytes, then either (a) keep the child alive until replaced by next clipboard write, or (b) use a tool that forks into background (xclip does this by default when given `-loops 0`; verify wl-copy behavior). Explicitly: do not `.wait()` before the next clipboard write arrives.

**Confidence:** MEDIUM — xclip background fork behavior should be tested.

### Anti-Pattern 4: Ignoring PNG size limits

**What:** Forwarding arbitrarily large clipboard images (e.g., 4K screenshots can be 5–15MB).

**Why bad:** No flow control means memory pressure and potential TCP buffer stalls. A single large frame blocks the channel.

**Instead:** Enforce a max frame size (e.g., 20MB). Log and drop oversized images. Optionally add image resizing/compression as a later feature.

### Anti-Pattern 5: Running Xvfb with a large screen resolution

**What:** Using `Xvfb :1 -screen 0 1920x1080x24`.

**Why bad:** Xvfb allocates the full framebuffer in memory. At 1920x1080x24 that is ~6MB of RAM per Xvfb instance for no benefit — clipboard operations need no screen real estate.

**Instead:** Use `1x1x24`. The framebuffer is negligible and clipboard operations work identically.

---

## Component Build Order

Dependencies flow bottom-up. Build and test each layer before the next.

```
Layer 0 (Foundation — no dependencies)
  ├── Protocol types: Frame struct, DisplayEnvironment enum
  └── Error types: unified Error enum

Layer 1 (Display detection — depends on Layer 0)
  └── DisplayManager: detect() → DisplayEnvironment

Layer 2 (Clipboard I/O — depends on Layer 1)
  ├── ClipboardReader (local): read image bytes from system clipboard
  └── ClipboardWriter (remote): write image bytes, dispatch on DisplayEnvironment

Layer 3 (Transport — depends on Layer 0)
  ├── Framer: Vec<u8> → framed bytes
  └── Deframer: AsyncRead → Vec<u8> frames

Layer 4 (Network — depends on Layer 3)
  ├── TcpSender: reconnect loop, feeds Framer output to socket
  └── TcpReceiver: accept loop, feeds socket into Deframer

Layer 5 (Orchestration — depends on all layers)
  ├── HashFilter: stateful dedup
  ├── SessionEnvPublisher: write DISPLAY to /run/user/UID/cssh.env
  └── Signal handling: SIGTERM → kill Xvfb child

Layer 6 (CLI wiring — depends on Layer 5)
  ├── cssh local: ClipboardReader → HashFilter → Framer → TcpSender
  └── cssh remote: TcpReceiver → Deframer → ClipboardWriter (via DisplayManager)
```

---

## Scalability Considerations

This is a two-machine personal tool. Scalability is not a concern. The following table documents what breaks if scope changes, purely for defensive planning:

| Concern | Current (2 machines) | At 10+ machines | Notes |
|---------|---------------------|-----------------|-------|
| TCP connections | Single sender→receiver | Fan-out needed | Out of scope |
| Image throughput | ~1 image every few seconds | Still fine | Screenshots are infrequent |
| Xvfb instances | One per machine | One per machine | Fine |
| Auth | Tailscale identity (ACL) | Still Tailscale | No app-layer auth needed |

---

## Single Binary vs Separate Binaries Decision

**Recommendation: Single binary with subcommands** (`cssh local`, `cssh remote`).

Rationale:
- `cargo install codex-screenshot-ssh` installs one binary to `~/.cargo/bin/cssh`
- Both local and remote machines run the same release artifact
- Systemd service files reference the same path on both machines
- No confusion about which binary goes where
- Subcommand dispatch is idiomatic Rust (clap with subcommands)

The two modes share zero runtime state — they are effectively separate programs that happen to share protocol types, error types, and the framing/deframing logic. Factor shared code into a `lib` crate within the same workspace or as private modules; subcommands import from there.

---

## Systemd Integration Points

Both daemons are user-level systemd services (`~/.config/systemd/user/`).

**Local (`cssh-local.service`):**
- `Type=simple`
- `ExecStart=/path/to/cssh local --remote <tailscale-hostname>:PORT`
- `Restart=always`, `RestartSec=5`
- Requires display: `Environment=DISPLAY=%env{DISPLAY}` or rely on user session environment

**Remote (`cssh-remote.service`):**
- `Type=simple`
- `ExecStart=/path/to/cssh remote --port PORT`
- `Restart=always`, `RestartSec=5`
- No display required at start — DisplayManager detects at runtime
- After start: `SessionEnvPublisher` writes `/run/user/%U/cssh.env`

---

## SSH Session Environment Exposure

**Problem:** SSH sessions don't inherit the `DISPLAY` set by the cssh-remote daemon. The CLI tool's xclip/wl-paste calls will fail unless `DISPLAY` is set in the SSH session.

**Solution:** `SessionEnvPublisher` writes `export DISPLAY=:N` to a well-known path. SSH users source it.

```bash
# In ~/.bashrc or ~/.zshrc on remote:
if [ -f /run/user/$(id -u)/cssh.env ]; then
  source /run/user/$(id -u)/cssh.env
fi
```

This is a one-time setup step users perform on the remote machine. Document it in README.

**Alternative:** Use `pam_env` or `~/.pam_environment` to set `DISPLAY` system-wide for the user — less fragile but requires understanding of PAM configuration. Keep as a v2 improvement.

---

## Sources

**Confidence note:** Web search and WebFetch were unavailable during this research session. All findings are derived from:

1. Project constraints documented in `.planning/PROJECT.md` (HIGH confidence — first-party)
2. Author training knowledge of Linux clipboard architecture, X11/Wayland display protocols, Xvfb, Tokio async patterns, and TCP framing conventions (MEDIUM confidence — training data, verify specific API details against current crate docs)
3. Well-established patterns for daemon architecture (MEDIUM confidence)

**Verify before implementing:**
- arboard current Wayland support status (feature flags, minimum compositor requirements) — check https://docs.rs/arboard
- wl-copy `--type` flag behavior (does it background-fork or hold foreground?) — test manually
- xclip background fork behavior with `-loops 0` — test manually
- Current Tokio signal handling API for SIGTERM on Linux — check https://docs.rs/tokio/latest/tokio/signal/
- Whether Claude Code / Codex / OpenCode use OSC 52, bracketed paste, or direct xclip/wl-paste subprocess for clipboard reads — inspect each tool's source or documentation
