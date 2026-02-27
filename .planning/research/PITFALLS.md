# Domain Pitfalls: Clipboard Bridge over SSH/Tailscale

**Domain:** Clipboard bridge daemon — X11/Wayland clipboard, Xvfb, TCP, systemd
**Researched:** 2026-02-27
**Confidence:** MEDIUM (training data; external verification tools unavailable during this session)

---

## Critical Pitfalls

Mistakes that cause rewrites, silent data loss, or production hangs.

---

### Pitfall 1: X11 Clipboard Ownership — The Process Must Stay Alive

**What goes wrong:**
In X11, clipboard ownership is NOT stored in a shared buffer. The owner process holds the selection and must respond to `SelectionRequest` events from any application that wants to read it. When the process that called `XSetSelectionOwner` exits, the clipboard becomes empty. If your remote daemon writes data to the clipboard and then drops the X11 connection (or the process exits), every downstream consumer (xclip, Claude Code, Codex) gets nothing.

**Why it happens:**
Developers assume clipboard = shared memory. It is not. It is a request/response protocol. The selection owner must run an X11 event loop indefinitely after claiming ownership, serving `SelectionRequest` events until it receives `SelectionClear` (ownership taken by another client).

**Consequences:**
- Ctrl-V produces empty paste silently
- Only reproducible under timing conditions (e.g., daemon processes a write and returns before the next Ctrl-V)
- Extremely hard to debug because xclip itself may claim ownership briefly then exit, causing the same symptom

**Prevention:**
- In Rust (x11rb or x11-clipboard crate): after `SetSelectionOwner`, run a dedicated event loop thread that handles `SelectionRequest` events and responds with `SelectionNotify`
- Use the `x11-clipboard` crate which handles this loop internally — do NOT use raw `xclip` subprocess calls and assume clipboard persists
- If using `xclip` as a subprocess: xclip has a `-loops N` flag and a background mode; understand what it does before using it
- Keep the daemon process alive as the selection owner; do not write and exit

**Warning signs:**
- Paste works once immediately after write, fails 100ms later
- `xclip -o -sel clipboard` returns empty after daemon is "done"
- xclip subprocess exits immediately (check with `ps`)

**Phase:** Addressed in the remote clipboard writer implementation (Phase: Core clipboard write logic).

---

### Pitfall 2: X11 INCR Protocol — Large Images Silently Truncated

**What goes wrong:**
X11 properties have a maximum size (typically 256KB, bounded by the X server's `maxRequestLength`). For images larger than this, the ICCCM specifies the INCR (incremental transfer) protocol: the owner signals the transfer size and then delivers data in chunks as the receiver drains each chunk. Most naive clipboard implementations ignore INCR entirely — they respond to `SelectionRequest` with the full data and the X server silently drops data exceeding the limit, or the request fails.

**Why it happens:**
Screenshots can easily exceed 1MB. The INCR protocol is complex and most tutorials omit it. The `x11-clipboard` Rust crate handles INCR on the read side but developers must verify write-side INCR support as well.

**Consequences:**
- PNG images larger than ~256KB paste as garbage or empty
- Failure is silent — no error returned to the requester
- Works fine in testing with small screenshots, breaks in production with 1080p+ screens

**Prevention:**
- Verify whether the chosen Rust clipboard library handles INCR on the owner side (serving large selections)
- The `x11-clipboard` crate (uses xcb internally) handles INCR; verify via source or tests
- If rolling custom X11 code: implement INCR owner loop — check `maxRequestLength`, chunk data, handle `PropertyNotify` events from receiver
- Consider compressing or downsizing images before clipboard write if INCR handling is uncertain
- Test explicitly with a 3840x2160 screenshot (~8MB uncompressed, ~500KB PNG)

**Warning signs:**
- Small screenshots paste correctly, large ones paste as empty or broken
- X11 error logs showing `BadLength` or `BadValue`

**Phase:** Addressed in image encoding/clipboard write phase. Add explicit large-image test.

---

### Pitfall 3: Wayland Clipboard Requires a Compositor — No Headless Path

**What goes wrong:**
On Wayland, clipboard access requires an active Wayland compositor connection. `wl-copy` and `wl-paste` will fail with "cannot connect to Wayland display" or "wl_display_connect failed" on headless machines or SSH sessions without a running compositor. Unlike X11 (where Xvfb provides a full fake display), there is no lightweight headless Wayland compositor equivalent that is universally available and stable.

**Why it happens:**
Wayland's security model ties clipboard access to the compositor. The `wlr-data-control-unstable-v1` protocol (used by wl-clipboard) requires a wlroots-based compositor. On GNOME/KDE Wayland, the compositor is the graphical session — not available over SSH.

**Consequences:**
- Remote machines running Wayland with no active GUI session cannot use `wl-copy` path
- Fallback to X11+Xvfb is required on headless remotes regardless of the machine's normal display server
- Detection logic (`WAYLAND_DISPLAY` env var) gives false positives: the var may be set but the compositor unreachable from the SSH session

**Prevention:**
- Do NOT rely solely on `WAYLAND_DISPLAY` being set for detection — attempt an actual connection test
- For headless remotes: always use the Xvfb+xclip path, regardless of what the display vars say
- If the remote has an active GUI Wayland session (rare for headless servers): wl-copy works, but verify `WAYLAND_DISPLAY` is accessible from the SSH session user's environment
- Consider: headless remote = Xvfb path is default; only use Wayland path if user explicitly configures it

**Warning signs:**
- `wl-copy: compositor does not support wl-data-control protocol`
- `wl-copy: failed to connect to a Wayland compositor`
- WAYLAND_DISPLAY set but wl-copy fails

**Phase:** Display environment detection and fallback logic (Phase: Remote daemon environment detection).

---

### Pitfall 4: Xvfb Display Number Collisions and Stale Lock Files

**What goes wrong:**
Xvfb uses lock files in `/tmp/.X<N>-lock` and unix socket `/tmp/.X11-unix/X<N>` to claim a display number. If a previous Xvfb instance crashed or was killed without cleanup, these files persist. A new Xvfb startup attempt for `:99` will fail with "Server is already active for display 99" even though nothing is listening. Alternatively, probing for a free display number without proper locking creates a race condition between concurrent daemon startups.

**Why it happens:**
Xvfb does not auto-clean its lock files on crash. System reboots clean `/tmp` (on most distros), but Xvfb crashes within a session leave stale locks. Display number selection (find-first-free) is not atomic.

**Consequences:**
- Daemon fails to start with cryptic Xvfb error, clipboard unavailable
- Systemd service restart loop if Xvfb can't claim its display
- After machine suspend/resume, stale locks from previous session

**Prevention:**
- Use a fixed, project-specific display number (e.g., `:99`) rather than auto-discovery
- On daemon startup: check for stale lock file (lock file exists but no process with that PID is running), remove it, then start Xvfb
- Stale lock check: read `/tmp/.X99-lock` → get PID → `kill -0 <PID>` → if ESRCH (no such process), remove lock and socket, then proceed
- Add this cleanup to the systemd `ExecStartPre` script or as a startup routine in the daemon
- Set `DISPLAY=:99` explicitly in systemd service file — do not rely on environment inheritance

**Warning signs:**
- `Fatal server error: Server is already active for display 99`
- Xvfb fails on second daemon start after unclean shutdown
- `/tmp/.X99-lock` exists but `ps aux | grep Xvfb` shows nothing

**Phase:** Xvfb lifecycle management (Phase: Headless display management).

---

### Pitfall 5: Xvfb Zombie Processes and Signal Handling

**What goes wrong:**
If the remote daemon spawns Xvfb as a child process (`Command::new("Xvfb")`) and then exits without waiting on the child, Xvfb becomes a zombie (defunct) process. Worse, if the daemon panics or is killed with SIGKILL, Xvfb continues running as an orphan owned by init/systemd — and the next daemon start cannot claim the display number because Xvfb is already running. Double-start produces two Xvfb processes fighting over `:99`.

**Why it happens:**
Rust's `std::process::Child` drops without waiting if not explicitly `.wait()`ed. SIGKILL cannot be caught for cleanup. Orphaned Xvfb inherits the lock file and remains.

**Consequences:**
- Display number occupied, daemon restart fails
- Memory/resource leak from accumulated Xvfb orphans
- Inconsistent state: daemon thinks it owns the display, but Xvfb was from a previous run

**Prevention:**
- Spawn Xvfb with a process group, kill entire group on daemon exit
- Register a `ctrlc` or signal handler (using `signal-hook` crate) to send SIGTERM to Xvfb child before daemon exit
- In systemd service: use `KillMode=control-group` so systemd kills all processes in the service cgroup, including Xvfb children
- On daemon startup: check if Xvfb is already running for `:99` before spawning a new one (check lock file PID is alive); reuse if healthy, kill and restart if stale
- Use `std::process::Child::kill()` + `wait()` in Drop implementation of an Xvfb manager struct

**Warning signs:**
- Multiple `Xvfb :99` entries in `ps aux`
- `[Xvfb] <defunct>` in process list
- Xvfb memory growth over multiple daemon restarts

**Phase:** Xvfb lifecycle management (Phase: Headless display management).

---

### Pitfall 6: DISPLAY Environment Variable Not Propagated to SSH Sessions

**What goes wrong:**
The remote daemon sets `DISPLAY=:99` in its own environment and exports it. But CLI tools (Claude Code, Codex, OpenCode) running in separate SSH sessions do not inherit this variable — they get whatever `DISPLAY` (if any) the SSH server provides, which on a headless machine is typically empty. When the user presses Ctrl-V in Claude Code, it invokes `xclip -o` which fails silently because `DISPLAY` is unset.

**Why it happens:**
SSH sessions do not inherit the environment of other running processes. Each SSH session gets a fresh environment from PAM/profile. `DISPLAY` is only set if X11 forwarding is enabled (which it isn't here) or if the user's shell profile sets it.

**Consequences:**
- Clipboard bridge works (daemon writes to Xvfb clipboard), but Ctrl-V produces nothing
- `xclip -o` fails with "Can't open display" silently
- This is the hardest class of bug to debug because the daemon appears healthy

**Prevention:**
- Write `DISPLAY=:99` (or whatever display was chosen) to a known file, e.g., `/run/user/<UID>/cssh-display` or `/tmp/cssh-display`
- Document that users must add `export DISPLAY=$(cat /tmp/cssh-display)` to their SSH session `~/.bashrc` or `~/.zshrc`
- Alternatively: provide a shell wrapper that reads the display file and sets DISPLAY before invoking xclip
- Systemd socket activation or a small helper `cssh env` subcommand that emits `export DISPLAY=:99` for eval
- Consider: use `~/.config/environment.d/` drop-ins (systemd user environment) — these ARE inherited by systemd user services but NOT by SSH sessions directly
- Test the full path: start daemon → open new SSH session → check `echo $DISPLAY` → run `xclip -o -sel clipboard`

**Warning signs:**
- `xclip: Can't open display: (null)`
- `Error: Can't open display` from any X11 tool in SSH session
- Daemon is running but Ctrl-V produces no paste

**Phase:** System integration and DISPLAY propagation (Phase: SSH session environment setup).

---

## Moderate Pitfalls

---

### Pitfall 7: TCP Partial Writes and the Length-Prefix Protocol

**What goes wrong:**
TCP is a stream protocol — `write()` can return having sent fewer bytes than requested. A length-prefixed frame protocol (4-byte length header + N bytes of PNG data) breaks if the sender does not loop until all bytes are written, or if the receiver reads exactly N bytes but TCP delivers them in multiple chunks. This produces corrupted frames: the receiver reads a partial PNG, decodes garbage, writes garbage to clipboard.

**Why it happens:**
Rust's `TcpStream::write()` maps to the OS `send()` syscall, which can return short. Developers assume `write_all()` is always available but forget to use it. On the read side, `read_exact()` exists but must be used deliberately.

**Consequences:**
- Silent data corruption — the PNG header may parse but pixel data is wrong
- Intermittent failures that only appear under network load or when large images are sent
- Hard to reproduce locally (loopback never short-writes)

**Prevention:**
- Always use `write_all()` on the send side — it loops internally until all bytes sent
- Always use `read_exact()` on the receive side for both the header and the body
- Validate PNG magic bytes (first 8 bytes: `\x89PNG\r\n\x1a\n`) after receive before writing to clipboard
- Add a checksum (CRC32 or xxHash) as a trailer to detect corruption without full PNG decode
- Test over an actual Tailscale connection, not just loopback

**Warning signs:**
- Clipboard contains broken PNG (gray blocks, partial image)
- Image decode errors from clipboard reader
- Failures only under high bandwidth usage

**Phase:** TCP transport layer (Phase: Core transport implementation).

---

### Pitfall 8: TCP Connection Keepalive and Silent Disconnects

**What goes wrong:**
Tailscale connections can silently drop (NAT timeout, Tailscale reconnect, machine sleep/wake). A TCP connection that was established hours ago may appear open to both sides but is actually dead — writes block indefinitely or return errors only after the kernel's TCP timeout (up to 2 hours by default). The local daemon sends a new screenshot and blocks forever waiting for the ACK that will never come.

**Why it happens:**
TCP does not have application-level heartbeats by default. The kernel's TCP keepalive is disabled by default and has a 2-hour idle timeout even when enabled. Tailscale's tunneled connections can silently go away when the underlying path changes.

**Consequences:**
- Daemon hangs on write, stops processing new screenshots
- User takes screenshot, nothing arrives at remote — no error, no feedback
- Requires daemon restart to recover

**Prevention:**
- Enable TCP keepalive on the socket: `TcpStream::set_keepalive()` or using `socket2` crate with `set_tcp_keepalive()` with a short idle time (e.g., 30 seconds) and short interval (10 seconds)
- Set a write timeout: `TcpStream::set_write_timeout(Some(Duration::from_secs(10)))` — fail fast rather than hang
- Implement reconnection loop: on any write error, close connection, wait (exponential backoff), reconnect
- The remote side should also detect dead connections: if no data for X seconds, close and wait for new connection
- Use a connection health check: send a 0-length keepalive frame periodically (or use the length-prefix protocol with a special type byte for heartbeat)

**Warning signs:**
- Screenshot taken, no paste available after ~30 seconds
- No error logged, daemon appears healthy
- Connection established once then never sends data again after sleep/wake

**Phase:** TCP transport layer, reconnection logic (Phase: Connection resilience).

---

### Pitfall 9: Image Format Assumptions — RGBA vs RGB vs Pre-multiplied Alpha

**What goes wrong:**
The local clipboard on Ubuntu (via GNOME screenshot tool) stores images as `image/png` in the clipboard. However, when reading via xclip or wl-paste, you get raw bytes that may be PNG-encoded, or may be raw pixel data depending on the target atom requested. If the code requests the wrong target (`image/png` vs `image/bmp` vs `image/x-bmp` vs raw `PIXMAP`), it gets a different format. Additionally, PNG images may have an alpha channel (RGBA) — when the remote re-encodes for clipboard or display, dropping the alpha channel incorrectly produces color shifts.

**Why it happens:**
X11 clipboard offers multiple targets for the same data. The requester must ask for the right one. The default `TARGETS` atom lists available formats; naive code picks the first one which may not be PNG.

**Consequences:**
- CLI tools receive wrong image format, fail to display
- Color distortion (pre-multiplied alpha producing darkened images)
- Claude Code/Codex may reject non-PNG formats

**Prevention:**
- Always request `image/png` explicitly as the clipboard target — verify this target is available before requesting
- On the local side: check available targets via `TARGETS` atom first, select `image/png` specifically
- When re-encoding: use the `image` crate's `DynamicImage`, preserve alpha if present, encode as PNG with alpha intact
- Test with a screenshot that has transparency (e.g., a window with rounded corners on a transparent background)
- Validate round-trip: hash the PNG before send, hash after receive and clipboard-write, verify match

**Warning signs:**
- Image in clipboard is wrong color tone (greenish, darkened)
- CLI tool refuses to display the pasted image
- Image crate decode error on receive side

**Phase:** Image encoding/format handling (Phase: Clipboard read and image encoding).

---

### Pitfall 10: Systemd Service — DISPLAY and User Environment Not Inherited

**What goes wrong:**
A systemd user service (`systemctl --user`) does not inherit the login session's environment variables. `DISPLAY`, `WAYLAND_DISPLAY`, `DBUS_SESSION_BUS_ADDRESS`, and `XDG_RUNTIME_DIR` are all absent. Xvfb invocations in a systemd service have no DISPLAY to reference, and X11 tools called from the service fail. For system services (`systemctl` without `--user`), it's even worse — no user session environment at all.

**Why it happens:**
Systemd services start in a clean environment. Login environment variables are only available in PAM sessions (interactive logins, graphical sessions), not in systemd units.

**Consequences:**
- Daemon starts successfully per systemd, but Xvfb never gets DISPLAY propagated
- X11 calls in service fail silently
- `journalctl` shows the service as active but no clipboard writes happen

**Prevention:**
- In systemd unit file: set `Environment=DISPLAY=:99` explicitly — do not rely on inherited env
- For Xvfb: set `Environment=DISPLAY=:99` in the unit that starts Xvfb, and the same in the clipboard daemon unit
- Use `EnvironmentFile=` to load from a file if display number is dynamic
- For user services: `systemctl --user` units can inherit from `~/.config/environment.d/` files — put `DISPLAY=:99` there
- Test by examining `systemctl --user show-environment` before and after service start
- For `XDG_RUNTIME_DIR`: for user services, systemd sets this automatically; for system services, it must be computed (`/run/user/<UID>`)

**Warning signs:**
- `systemctl --user status cssh-remote` shows active but clipboard doesn't work
- `journalctl --user -u cssh-remote` shows X11 "Can't open display" errors
- Service works when run manually but not under systemd

**Phase:** Systemd service file authoring (Phase: Service packaging).

---

### Pitfall 11: Systemd Restart Policy and Xvfb Dependency Ordering

**What goes wrong:**
If the remote daemon and Xvfb are separate systemd units, the daemon may start before Xvfb is ready (Xvfb takes 100-500ms to initialize). If the daemon starts, attempts to connect to the X display, fails, and does not retry — it exits and systemd marks it failed. With `Restart=on-failure`, it may restart faster than Xvfb becomes ready, hitting a restart limit and entering a failed state permanently.

**Why it happens:**
`After=xvfb.service` in the unit file ensures ordering but does NOT guarantee readiness — it only means Xvfb was started, not that it is accepting connections. There is no built-in readiness probe for X11 servers in systemd.

**Consequences:**
- Daemon enters failed state at boot, requires manual `systemctl reset-failed`
- Race condition only reproducible at boot, not in testing

**Prevention:**
- In the daemon: implement retry loop for X11 connection (retry up to 10 times with 500ms sleep)
- OR: use `ExecStartPre` with a script that loops `xdpyinfo -display :99` until it succeeds (with timeout)
- Use `Type=notify` or `Type=forking` appropriately — or simpler: `Type=simple` with retry logic inside the binary
- Set `RestartSec=2` and `StartLimitIntervalSec=30` with `StartLimitBurst=5` to allow for boot-time retries without infinite loops
- Consider: manage Xvfb inside the daemon binary itself (spawn as child) rather than as a separate service — eliminates the ordering problem

**Warning signs:**
- Works on second `systemctl start` but not first
- `systemctl status` shows "start-limit-hit" after boot
- Works manually but fails at boot

**Phase:** Systemd service authoring (Phase: Service packaging).

---

### Pitfall 12: Content Hashing — Hash Collisions vs. Hash of What?

**What goes wrong:**
The project requirement includes "content hashing avoids re-sending duplicate images." The pitfall is: what exactly is being hashed? If the screenshot tool captures the same screen region but includes a timestamp in the image metadata, the PNG bytes differ even though the visual content is identical — hash check fails, duplicate is sent. Conversely, if hashing only the raw pixel data (not the PNG encoding), encoding variations produce the same hash for visually identical images but may miss actual changes.

Additionally: hash comparison requires storing state. If the local daemon restarts, it loses the last hash and re-sends the last image on startup (minor but potentially confusing).

**Why it happens:**
PNG encoding is not deterministic — even the same pixel data can produce different PNG bytes depending on compression level, metadata (timestamps, software tags), and filter choices.

**Prevention:**
- Hash the decoded pixel data, not the PNG bytes — decode to RGBA, hash the raw bytes
- Use a fast hash (xxHash or Blake3, not SHA256 — this runs on every screenshot)
- Store last hash in memory (per session) — on daemon restart, re-send last image once (acceptable behavior)
- Document: duplicate suppression is best-effort, not guaranteed across restarts

**Warning signs:**
- Same screenshot triggers multiple sends (hash not matching due to metadata variation)
- CPU spike when hashing large images (wrong hash algorithm)

**Phase:** Local daemon clipboard watcher (Phase: Clipboard monitoring and deduplication).

---

## Minor Pitfalls

---

### Pitfall 13: Security — Binding TCP on 0.0.0.0 Instead of Tailscale Interface

**What goes wrong:**
If the remote daemon binds the TCP listener on `0.0.0.0:PORT`, it is reachable from any network interface, including public internet interfaces. The clipboard data (screenshot images) becomes accessible to anyone who can reach that port.

**Prevention:**
- Bind only on the Tailscale interface IP: obtain the `100.x.x.x` address from `tailscale ip` or bind to the interface name via `socket2`
- Or: bind on `100.0.0.0/8` range by resolving the machine's Tailscale IP at startup
- Document this as a security requirement — binding on Tailscale IP is not optional
- Test: verify the port is not reachable from the public internet interface

**Phase:** TCP listener setup (Phase: Core transport implementation).

---

### Pitfall 14: Large Screenshot Memory Pressure

**What goes wrong:**
A 4K screenshot is ~32MB uncompressed (3840x2160x4 bytes). The pipeline decodes it from clipboard (32MB), encodes to PNG (~2-5MB), buffers it in the TCP send buffer, receives it on remote (another 5MB buffer), decodes to verify (32MB again), and writes to X11 clipboard (holds 32MB for lifetime of selection ownership). Under high screenshot frequency, this causes significant heap pressure.

**Prevention:**
- PNG encode once, keep only the encoded form in memory — do not keep decoded pixel data after encoding
- On the remote side: store only the PNG bytes for clipboard serving, not decoded pixels
- Consider a maximum image size limit (configurable) — reject or downscale images above a threshold
- Avoid buffering entire image in memory on receive before verifying; streaming would be ideal but is complex with the INCR protocol
- Use `Vec::with_capacity()` with the frame length to avoid reallocations

**Warning signs:**
- OOM killer activity after several large screenshots
- RSS memory of daemon grows monotonically

**Phase:** Image handling pipeline (Phase: Clipboard read and image encoding).

---

### Pitfall 15: xclip vs xsel vs xclip -sel clipboard vs PRIMARY Selection Confusion

**What goes wrong:**
X11 has three selections: `PRIMARY` (mouse-selection, middle-click paste), `SECONDARY` (rarely used), and `CLIPBOARD` (Ctrl-C/Ctrl-V). `xclip` defaults to `PRIMARY` unless `-selection clipboard` is specified. Reading/writing to the wrong selection means Ctrl-V never sees the data.

**Prevention:**
- Always pass `-selection clipboard` (xclip) or `--clipboard` (xsel) explicitly — never rely on defaults
- In Rust X11 code: use `CLIPBOARD` atom (not `PRIMARY`) as the selection target
- Test: write to clipboard, verify with `xclip -o -selection clipboard` (not just `xclip -o`)

**Phase:** Clipboard write implementation (Phase: Core clipboard write logic).

---

### Pitfall 16: Rust Clipboard Crates — Maintenance Status

**What goes wrong:**
The Rust clipboard ecosystem has fragmentation. `clipboard` crate is largely unmaintained. `x11-clipboard` is lower-level but active. `arboard` is cross-platform and actively maintained (supports X11, Wayland, macOS, Windows) and handles the event loop internally. Choosing an unmaintained crate means working around bugs without upstream fixes.

**Prevention:**
- Use `arboard` for clipboard read/write — it handles X11 event loop, INCR protocol, and Wayland via wl-clipboard
- Verify `arboard` version and last commit date before committing to it
- Fallback: `x11-clipboard` crate for X11-only if `arboard` has issues; handle event loop manually
- Avoid: `clipboard` crate (last updated 2019), raw `xclip` subprocess (subprocess lifetime issues)

**Phase:** Library selection (Phase: Project setup and dependency selection).

---

## Phase-Specific Warnings

| Phase Topic | Likely Pitfall | Mitigation |
|-------------|---------------|------------|
| Clipboard read (local) | Wrong selection (PRIMARY vs CLIPBOARD) | Always specify CLIPBOARD selection explicitly |
| Image encoding | PNG non-determinism breaks hash dedup | Hash decoded pixels, not PNG bytes |
| TCP transport | Partial writes, silent disconnects | `write_all()`, `read_exact()`, write timeout, keepalive |
| Remote clipboard write | Selection owner dies, clipboard empty | Keep daemon alive as X11 event loop owner |
| Large image transfer | INCR protocol truncation | Verify library handles INCR; test with 4K screenshots |
| Xvfb startup | Stale lock files, zombie orphans | Startup cleanup routine, signal handler, KillMode=control-group |
| DISPLAY propagation | SSH sessions don't inherit DISPLAY | Write DISPLAY to file; document user profile setup |
| Wayland detection | WAYLAND_DISPLAY set but compositor unreachable | Test connection, not just env var; default to Xvfb on headless |
| Systemd ordering | Daemon starts before Xvfb ready | Retry loop in daemon; ExecStartPre readiness check |
| Security | Listener on 0.0.0.0 | Bind to Tailscale interface IP only |

---

## Sources

- X11 ICCCM specification: clipboard ownership model, selection protocol, INCR protocol (training data — MEDIUM confidence, well-established standard)
- Wayland wlr-data-control protocol: compositor dependency for clipboard access (training data — MEDIUM confidence)
- Xvfb man page: lock file locations, display number management (training data — MEDIUM confidence)
- Linux TCP socket API: `SO_KEEPALIVE`, `TCP_KEEPIDLE`, partial write behavior (training data — HIGH confidence, POSIX standard)
- systemd unit file documentation: `KillMode`, `Environment=`, ordering vs readiness (training data — MEDIUM confidence)
- arboard crate: Rust clipboard abstraction with X11/Wayland support (training data — MEDIUM confidence; verify current maintenance status before use)
- x11-clipboard crate: lower-level X11 clipboard with INCR support (training data — MEDIUM confidence)

**Note:** All external research tools (WebSearch, WebFetch, Brave API) were unavailable during this research session. All findings are based on training data (knowledge cutoff August 2025). Confidence is MEDIUM rather than HIGH for ecosystem-specific claims. Recommend verifying `arboard` and `x11-clipboard` crate maintenance status via crates.io before library selection.
