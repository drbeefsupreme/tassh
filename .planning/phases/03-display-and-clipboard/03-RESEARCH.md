# Phase 3: Display and Clipboard - Research

**Researched:** 2026-02-27
**Domain:** Xvfb lifecycle management, X11/Wayland clipboard read (local) and write (remote), RGBA-to-PNG encoding, display environment detection
**Confidence:** HIGH (core patterns verified from official docs and crate docs; subprocess behavior confirmed from man pages)

---

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**Clipboard watching behavior:**
- Event-driven clipboard change notifications where the OS supports them, with polling fallback
- Deduplicate by content hash — skip sending if image is identical to last sent
- Log non-image clipboard changes (debug aid) but only send images
- On startup, ignore whatever is currently on the clipboard — only send newly captured images

**Error and fallback behavior:**
- Missing clipboard tools (xclip, wl-paste, wl-copy): fail immediately with actionable error message including install command
- Display detection: try Wayland first, fall back to X11; if neither works, fail with clear error showing what was checked
- Stale Xvfb lock files: auto-clean after verifying no process owns them, log what was cleaned
- Xvfb crash: auto-restart with exponential backoff, give up after N failures and exit with error

**Display file format:**
- `~/.cssh/display` is a sourceable shell script: `export DISPLAY=:99`
- DISPLAY value only — no PID, timestamp, or other metadata
- Auto-create `~/.cssh/` directory if it doesn't exist
- Remove `~/.cssh/display` on graceful daemon shutdown to prevent stale values

**Logging and feedback:**
- Quiet by default, `-v` flag for verbose output
- Normal mode logs: daemon started, connected to remote, image sent/received (with size in KB), display detected, errors
- All log lines include timestamps
- Verbose mode adds: clipboard check details, display detection steps, Xvfb management details

### Claude's Discretion

- Specific clipboard change notification mechanism per platform
- Polling fallback interval
- Content hashing algorithm
- Xvfb display number selection strategy
- Exponential backoff parameters for Xvfb restart
- Exact log format and logging library choice

### Deferred Ideas (OUT OF SCOPE)

None — discussion stayed within phase scope
</user_constraints>

---

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| CLRD-01 | Local daemon watches system clipboard for new image content | arboard `get_image()` in polling loop + hash dedup; `wayland-clipboard-listener` for event-driven Wayland path |
| CLRD-02 | Local daemon supports Wayland clipboard reading (wl-paste) | arboard with `wayland-data-control` feature OR subprocess `wl-paste --type image/png`; both verified |
| CLRD-03 | Local daemon supports X11 clipboard reading (arboard/xclip) | arboard default Linux backend is X11; no additional feature flag required |
| CLRD-04 | Local daemon auto-detects display environment (Wayland vs X11) | Check `$WAYLAND_DISPLAY` env var first, then `$DISPLAY`; runtime detection at startup |
| CLWR-01 | Remote daemon writes received images to system clipboard | Subprocess dispatch: `xclip` for X11/Xvfb, `wl-copy` for Wayland; both support `image/png` MIME type |
| CLWR-02 | Remote daemon supports X11 clipboard writing via xclip | `xclip -selection clipboard -t image/png -i` piped with PNG bytes; xclip forks to background and serves selection ownership |
| CLWR-03 | Remote daemon supports Wayland clipboard writing via wl-copy | `wl-copy --type image/png` piped with PNG bytes; wl-copy forks to background by default |
| CLWR-04 | Remote daemon auto-detects display environment at runtime | Same detection as CLRD-04 plus headless fallback triggering Xvfb spawn |
| CLWR-05 | Remote daemon maintains X11 selection ownership | xclip and wl-copy both fork to background by default, serving SelectionRequest events until replaced; confirmed in man pages |
| DISP-01 | Remote daemon spawns Xvfb when no display server is available | `tokio::process::Command::new("Xvfb").args([":N", "-screen", "0", "1x1x24"])` with `-displayfd` for auto display selection |
| DISP-02 | Remote daemon cleans up stale Xvfb lock files on startup | Check `/tmp/.X{N}-lock` existence + verify PID inside is dead; delete if stale; log what was cleaned |
| DISP-03 | Remote daemon publishes DISPLAY to `~/.cssh/display` | Write `export DISPLAY=:N\n` to `~/.cssh/display`; create dir if absent; remove on shutdown |
| DISP-04 | Remote daemon manages Xvfb lifecycle (clean shutdown on exit) | SIGTERM handler via `tokio::signal::unix::signal(SignalKind::terminate())`; call `child.kill().await` then delete `~/.cssh/display` |
</phase_requirements>

---

## Summary

Phase 3 has three distinct sub-problems that can be designed and implemented semi-independently: (1) local clipboard reading, (2) remote clipboard writing, and (3) Xvfb lifecycle management on the remote.

**Local clipboard reading** is cleanly handled by arboard 3.6.1, which supports both X11 (default) and Wayland (via the `wayland-data-control` feature flag). The `get_image()` method returns `ImageData` with raw RGBA bytes; these must be PNG-encoded using the `image` crate before hashing and transmission. For event-driven Wayland watching, `wl-paste --watch` subprocess is the most universal approach (avoids compositor-specific protocol dependencies). On X11, polling with arboard is the correct fallback — XFixes events in Rust are not reliable enough to depend on without a display window. The startup-skip behavior (ignore current clipboard at launch) is implemented by recording the initial hash without sending it.

**Remote clipboard writing** uses subprocess dispatch: `wl-copy --type image/png` on Wayland, `xclip -selection clipboard -t image/png -i` on X11/Xvfb. Both tools **fork to the background by default** and hold X11/Wayland selection ownership until the next clipboard write arrives or the daemon exits — this directly satisfies CLWR-05 without any custom event loop. The cssh remote process must keep a handle to the previous clipboard child and kill it when a new image arrives, to prevent accumulating background processes.

**Xvfb management** uses `tokio::process::Command` with the `-displayfd` flag for automatic display number selection (supported since X server 1.13 / ~2012), avoiding manual lock-file scanning. Stale lock file cleanup is required on startup before spawning Xvfb. SIGTERM handling (via `tokio::signal::unix`) enables clean shutdown: kill Xvfb child, remove `~/.cssh/display`.

**Primary recommendation:** Use arboard 3.6.1 with `wayland-data-control` feature for local reading; subprocess dispatch (xclip/wl-copy) for remote writing; `tokio::process::Command` with `-displayfd` for Xvfb; `sha2` for content hashing; `image` crate for RGBA-to-PNG encoding.

---

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `arboard` | 3.6.1 | Local clipboard image read (X11 + Wayland) | 1Password-maintained; only Rust clipboard crate supporting both X11 and Wayland images with a single API; `get_image()` returns typed `ImageData` |
| `image` | 0.25.x | RGBA-to-PNG encoding; PNG bytes for wire + xclip input | De facto standard for image I/O in Rust; `RgbaImage::from_raw` + `write_to(Cursor, Png)` pattern is idiomatic |
| `sha2` | 0.10.x | SHA-256 content hash for deduplication | RustCrypto standard; integrates with `digest` trait; no extra dep needed (`sha2` re-exports `Digest`) |
| `tokio::process::Command` | (tokio 1.x, already in Cargo.toml) | Spawn Xvfb and clipboard writer subprocesses | Already a dependency; async `.spawn()` + `.kill().await` is the correct pattern |
| `tokio::signal::unix` | (tokio 1.x, already in Cargo.toml) | SIGTERM handler for clean Xvfb shutdown | Already a dependency; `signal(SignalKind::terminate())` + `tokio::select!` pattern |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `wl-clipboard-rs` | 0.9.x | Direct Wayland clipboard paste (no subprocess) | Alternative to `wl-paste` subprocess for Wayland reads; heavier dependency but avoids subprocess latency |
| `tracing` + `tracing-subscriber` | (already in Cargo.toml) | Structured logging with timestamps and verbosity | Already present; use `tracing::debug!` for verbose clipboard-check logs |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| arboard for local reads | `wl-paste`/`xclip` subprocess | Subprocess adds ~50ms latency per poll; arboard is in-process; use arboard for local |
| arboard for remote writes | arboard `set_image()` | arboard drops clipboard when `Clipboard` instance is dropped — unsafe for daemon use; subprocess (xclip/wl-copy) holds ownership in background |
| `sha2` | `xxhash` / `blake3` | sha2 is already in project stack research; both alternatives are faster but sha2 is sufficient for screenshot-rate dedup |
| `tokio::process::Command` for Xvfb | `std::process::Command` | std::process is synchronous; tokio::process integrates with the async runtime for `.kill().await` and `select!` |
| `-displayfd` flag | Manual lock file scan | `-displayfd` is simpler and avoids race conditions; supported on all modern Ubuntu Xvfb versions |

**Installation (new deps only — tokio, tracing already present):**
```toml
arboard = { version = "3", features = ["wayland-data-control"] }
image = { version = "0.25", default-features = false, features = ["png"] }
sha2 = "0.10"
```

---

## Architecture Patterns

### Recommended Project Structure

```
src/
├── main.rs          # subcommand dispatch (already exists)
├── cli.rs           # CLI args (already exists)
├── protocol.rs      # Frame, DisplayEnvironment (already exists)
├── transport.rs     # TCP framing (already exists, Phase 2)
├── clipboard.rs     # ClipboardReader (local) + ClipboardWriter (remote)  ← Phase 3
└── display.rs       # DisplayManager: detect(), spawn_xvfb(), publish()   ← Phase 3
```

### Pattern 1: ClipboardReader — arboard with startup-skip

**What:** On the local side, initialize arboard `Clipboard`, record the initial image hash without sending it, then poll in a loop. On each tick, call `get_image()`, PNG-encode the result, compute SHA-256; only forward if hash differs from last seen.

**When to use:** Always for the local watcher. Poll interval: 500ms (Claude's discretion — balances responsiveness vs CPU).

```rust
// src/clipboard.rs
use arboard::Clipboard;
use sha2::{Sha256, Digest};
use image::{RgbaImage, ImageFormat};
use std::io::Cursor;
use tokio::time::{sleep, Duration};

pub async fn watch_clipboard(tx: tokio::sync::mpsc::Sender<Vec<u8>>) -> anyhow::Result<()> {
    let mut clipboard = Clipboard::new()?;
    let mut last_hash: Option<[u8; 32]> = None;

    // Record initial hash without sending — satisfies "ignore current clipboard on startup"
    if let Ok(img) = clipboard.get_image() {
        last_hash = Some(hash_rgba(&img.bytes));
    }

    loop {
        sleep(Duration::from_millis(500)).await;

        match clipboard.get_image() {
            Ok(img) => {
                let hash = hash_rgba(&img.bytes);
                if Some(hash) != last_hash {
                    last_hash = Some(hash);
                    // Encode RGBA bytes to PNG
                    let png = rgba_to_png(img.width as u32, img.height as u32, &img.bytes)?;
                    let kb = png.len() / 1024;
                    tracing::info!("clipboard image captured ({kb} KB), sending");
                    let _ = tx.send(png).await;
                }
            }
            Err(_) => {
                // No image on clipboard — log at debug only
                tracing::debug!("clipboard: no image");
            }
        }
    }
}

fn hash_rgba(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

fn rgba_to_png(width: u32, height: u32, rgba: &[u8]) -> anyhow::Result<Vec<u8>> {
    let img = RgbaImage::from_raw(width, height, rgba.to_vec())
        .ok_or_else(|| anyhow::anyhow!("invalid RGBA dimensions"))?;
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)?;
    Ok(buf)
}
```

**Source:** arboard docs: `get_image()` returns `ImageData { width, height, bytes: Cow<[u8]> }` in RGBA order. image crate: `RgbaImage::from_raw` + `write_to` pattern.

### Pattern 2: ClipboardWriter — subprocess dispatch with background hold

**What:** On the remote side, after receiving a PNG frame, dispatch based on `DisplayEnvironment`. Spawn `xclip` or `wl-copy` with PNG bytes piped to stdin. Both tools fork to background by default, holding selection ownership. Kill the previous clipboard child before spawning the next one.

**When to use:** Always on the remote side. Never use arboard for remote writes (drops clipboard on drop).

```rust
// src/clipboard.rs — remote side
use tokio::process::Command;
use tokio::io::AsyncWriteExt;
use crate::protocol::DisplayEnvironment;

pub struct ClipboardWriter {
    current_child: Option<tokio::process::Child>,
    display: DisplayEnvironment,
}

impl ClipboardWriter {
    pub fn new(display: DisplayEnvironment) -> Self {
        Self { current_child: None, display }
    }

    pub async fn write(&mut self, png_bytes: &[u8]) -> anyhow::Result<()> {
        // Kill previous clipboard-holding process before replacing
        if let Some(mut child) = self.current_child.take() {
            let _ = child.kill().await;
        }

        let mut child = match &self.display {
            DisplayEnvironment::Wayland => {
                Command::new("wl-copy")
                    .args(["--type", "image/png"])
                    .stdin(std::process::Stdio::piped())
                    .spawn()?
            }
            DisplayEnvironment::X11 | DisplayEnvironment::Xvfb => {
                let display_str = if let DisplayEnvironment::Xvfb = &self.display {
                    std::env::var("DISPLAY").unwrap_or_else(|_| ":99".to_string())
                } else {
                    std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string())
                };
                Command::new("xclip")
                    .args(["-display", &display_str,
                           "-selection", "clipboard",
                           "-t", "image/png", "-i"])
                    .stdin(std::process::Stdio::piped())
                    .spawn()?
            }
            DisplayEnvironment::Headless => {
                anyhow::bail!("no display available for clipboard write");
            }
        };

        if let Some(stdin) = child.stdin.take() {
            let mut stdin = stdin;
            stdin.write_all(png_bytes).await?;
            // Drop stdin — signals EOF to xclip/wl-copy; they then fork to background
        }

        // Store child handle — don't .wait() or .kill() yet; it serves clipboard requests
        self.current_child = Some(child);
        tracing::info!("clipboard write: {} KB", png_bytes.len() / 1024);
        Ok(())
    }
}
```

**Critical note:** Do NOT call `.wait()` on the child after writing. Both xclip and wl-copy fork to the background after receiving stdin EOF and serve clipboard requests until another app claims ownership. Holding the `Child` handle prevents zombie accumulation and allows explicit kill on next write.

**Source:** xclip man page: "default action is to silently wait in the background for X selection requests until another application takes ownership." wl-copy man page: "by default, wl-copy forks and serves data requests in the background."

### Pattern 3: DisplayManager — Xvfb with -displayfd

**What:** Detect display environment at startup. If headless, spawn Xvfb using `-displayfd` flag to auto-select a free display number. Read the chosen display number from the fd. Set `DISPLAY` env var in the daemon process. Write `~/.cssh/display` for SSH sessions.

**When to use:** Remote side startup, before ClipboardWriter is constructed.

```rust
// src/display.rs
use tokio::process::Command;
use std::os::unix::io::FromRawFd;
use crate::protocol::DisplayEnvironment;

pub struct DisplayManager {
    pub env: DisplayEnvironment,
    pub display_str: String,          // e.g. ":99"
    xvfb_child: Option<tokio::process::Child>,
}

impl DisplayManager {
    pub async fn detect_and_init() -> anyhow::Result<Self> {
        // 1. Try Wayland
        if let Ok(wd) = std::env::var("WAYLAND_DISPLAY") {
            if !wd.is_empty() {
                tracing::info!("display: Wayland ({})", wd);
                return Ok(Self { env: DisplayEnvironment::Wayland,
                                 display_str: wd, xvfb_child: None });
            }
        }

        // 2. Try X11
        if let Ok(d) = std::env::var("DISPLAY") {
            if !d.is_empty() {
                tracing::info!("display: X11 ({})", d);
                return Ok(Self { env: DisplayEnvironment::X11,
                                 display_str: d, xvfb_child: None });
            }
        }

        // 3. Headless — spawn Xvfb with -displayfd
        tracing::info!("display: headless, spawning Xvfb");
        let (read_fd, write_fd) = create_pipe()?;

        // Clean stale lock files before spawning
        clean_stale_xvfb_locks().await;

        let child = Command::new("Xvfb")
            .args(["-displayfd", &write_fd.to_string(),
                   "-screen", "0", "1x1x24"])
            .spawn()?;

        // Read display number written by Xvfb to the fd
        let display_num = read_displayfd(read_fd).await?;
        let display_str = format!(":{display_num}");

        // Set DISPLAY in this process's environment
        std::env::set_var("DISPLAY", &display_str);
        tracing::info!("Xvfb spawned on display {display_str}");

        // Publish to ~/.cssh/display
        publish_display(&display_str).await?;

        Ok(Self { env: DisplayEnvironment::Xvfb,
                  display_str, xvfb_child: Some(child) })
    }

    /// Called on SIGTERM — kills Xvfb and removes ~/.cssh/display
    pub async fn shutdown(mut self) {
        if let Some(mut child) = self.xvfb_child.take() {
            let _ = child.kill().await;
        }
        let display_path = display_file_path();
        let _ = std::fs::remove_file(&display_path);
        tracing::info!("Xvfb stopped, {} removed", display_path.display());
    }
}

async fn clean_stale_xvfb_locks() {
    // Check /tmp/.X{N}-lock for N in 0..=255
    // If lock file exists but PID inside is dead (kill -0 fails), remove it
    for n in 0u8..=99 {
        let lock = format!("/tmp/.X{n}-lock");
        if let Ok(pid_str) = tokio::fs::read_to_string(&lock).await {
            let pid: u32 = pid_str.trim().parse().unwrap_or(0);
            if pid > 0 {
                // kill -0 checks if process is alive without signaling
                let alive = unsafe { libc::kill(pid as i32, 0) == 0 };
                if !alive {
                    let _ = tokio::fs::remove_file(&lock).await;
                    // Also remove socket
                    let _ = tokio::fs::remove_file(format!("/tmp/.X11-unix/X{n}")).await;
                    tracing::info!("removed stale Xvfb lock: {lock} (dead PID {pid})");
                }
            }
        }
    }
}

async fn publish_display(display_str: &str) -> anyhow::Result<()> {
    let path = display_file_path();
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let content = format!("export DISPLAY={display_str}\n");
    tokio::fs::write(&path, content).await?;
    Ok(())
}

fn display_file_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    std::path::PathBuf::from(home).join(".cssh").join("display")
}
```

**Note on `-displayfd`:** The `-displayfd N` flag causes Xvfb to write the chosen display number (as a decimal string followed by newline) to file descriptor N. This requires creating a pipe before `spawn()` and passing the write end's fd number as the argument. The write fd must be inherited by the child process (not set as `FD_CLOEXEC`). Tokio's `Command` inherits open fds by default on Linux.

**Stale lock file cleanup:** Read the PID from `/tmp/.X{N}-lock`, check liveness with `kill(pid, 0)` returning `ESRCH` if dead. This requires `libc` crate (already transitively available, or add explicitly).

### Pattern 4: SIGTERM handler with select!

**What:** Run the main server loop and SIGTERM handler concurrently. On SIGTERM, call `DisplayManager::shutdown()` then exit.

**When to use:** Remote daemon (`cssh remote`) startup.

```rust
// In main.rs — remote arm
use tokio::signal::unix::{signal, SignalKind};

Commands::Remote(args) => {
    let display = display::DisplayManager::detect_and_init().await?;
    let mut sigterm = signal(SignalKind::terminate())?;

    tokio::select! {
        result = transport::server(&bind_addr, args.port, &display) => {
            if let Err(e) = result { eprintln!("server error: {e}"); }
        }
        _ = sigterm.recv() => {
            tracing::info!("SIGTERM received, shutting down");
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Ctrl-C received, shutting down");
        }
    }

    display.shutdown().await;
}
```

**Source:** tokio docs: `signal(SignalKind::terminate())` — SignalKind::terminate() is SIGTERM on Unix. `recv()` is cancel-safe for use in `select!`.

### Pattern 5: Xvfb crash auto-restart with backoff

**What:** Wrap Xvfb spawn in a retry loop. On crash (child exits with non-zero), wait with exponential backoff and re-spawn. Give up after N failures (user decision: Claude's discretion — recommend 5 attempts, 2s/4s/8s/16s/32s).

**When to use:** Inside `DisplayManager` when `env == Xvfb`. Monitor via `child.wait()` in a background task.

```rust
// Background task monitoring Xvfb health
tokio::spawn(async move {
    let mut attempts = 0u32;
    let max_attempts = 5;
    loop {
        let status = xvfb_child.wait().await;
        tracing::warn!("Xvfb exited: {status:?}");
        attempts += 1;
        if attempts >= max_attempts {
            tracing::error!("Xvfb failed {max_attempts} times, giving up");
            std::process::exit(1);
        }
        let backoff = Duration::from_secs(2u64.pow(attempts));
        tracing::info!("restarting Xvfb in {backoff:?}");
        tokio::time::sleep(backoff).await;
        // re-spawn Xvfb ...
    }
});
```

### Anti-Patterns to Avoid

- **Using arboard `set_image()` on remote:** arboard drops clipboard when `Clipboard` is dropped. The daemon's loop will replace clipboard contents on the next receive, losing the previous image. Use subprocess instead.
- **Calling `.wait()` on the clipboard subprocess immediately:** Causes the process to exit before serving paste requests. Keep the `Child` handle alive until the next image arrives.
- **Not specifying `-t image/png` to xclip or `--type image/png` to wl-copy:** CLI tools request `image/png` MIME type specifically. Without the MIME type declaration, pastes return empty or wrong data.
- **Xvfb with large screen size:** `Xvfb :N -screen 0 1920x1080x24` allocates ~6MB of RAM unnecessarily. Use `1x1x24` — clipboard operations need no screen space.
- **Skipping the startup-ignore hash:** Without recording the initial clipboard hash at startup, the daemon sends the current clipboard contents immediately on first poll, even if it's an old screenshot from a previous session.
- **Not removing `~/.cssh/display` on shutdown:** SSH sessions sourcing this file will get a stale DISPLAY value pointing at a dead Xvfb, causing all clipboard operations to silently fail.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| RGBA-to-PNG encoding | Manual PNG byte construction | `image` crate `write_to(Cursor, Png)` | PNG has checksums, DEFLATE compression, color space metadata — 500+ lines to do correctly |
| Clipboard image read | Custom X11/Wayland protocol impl | arboard `get_image()` | Handles ICCCM selection transfers, INCR protocol for large images, compositor negotiation |
| Wayland clipboard events | Custom `wl_data_device` listener | `wl-paste --watch` subprocess OR `wayland-clipboard-listener` crate | Protocol is compositor-dependent; subprocess approach is universal |
| Display number selection | Manual `/tmp/.X11-lock` scan | Xvfb `-displayfd` | Avoids race conditions; Xvfb's own scan is authoritative |
| Content hashing | Rolling hash or CRC32 | `sha2` `Sha256` | SHA-256 is collision-resistant; screenshot data can be adversarial in edge cases |

**Key insight:** The hard part of clipboard on Linux is the event model (apps must serve paste requests). Delegating this to xclip/wl-copy subprocesses completely eliminates the complexity — they implement the X11 selection protocol and Wayland data-control protocol correctly.

---

## Common Pitfalls

### Pitfall 1: arboard drops clipboard on drop

**What goes wrong:** If `arboard::Clipboard` is created, `set_image()` is called, and then the Clipboard instance is dropped, other apps get empty data on paste.

**Why it happens:** On Linux, the clipboard owner must stay alive to serve `SelectionRequest` events. arboard's X11 backend spawns a background thread to serve these, but it stops when the Clipboard is dropped.

**How to avoid:** Use arboard only for reads (`get_image`). Use xclip/wl-copy subprocess for writes. Both tools handle the ownership loop in their own process.

**Warning signs:** Paste returns nothing immediately after the `cssh remote` process handles a frame.

### Pitfall 2: wl-copy stays in foreground without knowing it

**What goes wrong:** Code expects wl-copy to background-fork and move on, but `child.wait()` is called (blocking until wl-copy exits, which never happens until another app claims the clipboard).

**Why it happens:** wl-copy forks to background by default, but if `kill_on_drop(true)` is set on the Command, the child is killed when the handle is dropped.

**How to avoid:** Do NOT set `kill_on_drop(true)` on clipboard writer subprocesses. Keep the `Child` handle alive. Kill it explicitly only when the next image arrives.

**Warning signs:** Remote daemon hangs after receiving first frame; clipboard content does appear but daemon stops processing new frames.

### Pitfall 3: Xvfb stale lock file prevents startup

**What goes wrong:** A previous cssh remote crash left `/tmp/.X99-lock` with a dead PID. New Xvfb spawn fails with "server already active for display :99".

**Why it happens:** Xvfb does not clean up its own lock files on crash (only on clean exit).

**How to avoid:** At startup, scan lock files, verify PID liveness with `kill(pid, 0)`, remove stale ones before spawning. Log what was cleaned.

**Warning signs:** `Xvfb :99 -screen 0 1x1x24` exits immediately with error code; `/tmp/.X99-lock` exists.

### Pitfall 4: DISPLAY not set in SSH session

**What goes wrong:** SSH session has no `DISPLAY` env var. `xclip` invoked by Claude Code / Codex / OpenCode fails with "Can't open display". User presses Ctrl-V but sees no image.

**Why it happens:** SSH sessions don't inherit the daemon's environment. The cssh remote daemon sets `DISPLAY` for itself but not for new SSH logins.

**How to avoid:** `~/.cssh/display` (sourced in `~/.bashrc` or `~/.zshrc`) exposes DISPLAY to SSH sessions. This is documented in Phase 4 (shell snippet), but the file must be correctly written in Phase 3.

**Warning signs:** `echo $DISPLAY` in SSH session returns empty; `xclip -o` fails with display error.

### Pitfall 5: arboard `wayland-data-control` feature not enabled for Wayland

**What goes wrong:** arboard is added without the feature flag. On Wayland, it falls back to X11 via XWayland (if available) or fails with "can't open display".

**Why it happens:** Wayland support is opt-in in arboard and uses the `wlr-data-control` or `ext-data-control` protocol extension, which not all compositors support.

**How to avoid:** `arboard = { version = "3", features = ["wayland-data-control"] }`. On non-supporting compositors, arboard auto-falls-back to X11 — still correct behavior.

**Warning signs:** Clipboard operations fail on pure Wayland (no XWayland); Gnome/KDE Wayland sessions work but Sway sessions don't (different compositor protocol support).

### Pitfall 6: Xvfb -displayfd fd inheritance

**What goes wrong:** Pipe write end is created with `FD_CLOEXEC` set. Child process (Xvfb) cannot write to it. Daemon hangs waiting to read the display number.

**Why it happens:** Some fd-creation APIs (e.g., `pipe2` with `O_CLOEXEC`) set close-on-exec by default. Tokio's `Command` inherits fds but does not clear `FD_CLOEXEC` for custom fds.

**How to avoid:** Create the pipe with `nix::unistd::pipe()` (no `O_CLOEXEC`) or use `libc::pipe()` directly. Alternatively, use `nix::fcntl::open` with explicit flags. Close the write end in the parent after `spawn()`.

**Warning signs:** Daemon hangs on the read from the displayfd pipe; Xvfb spawns and runs but no display number is received.

---

## Code Examples

### RGBA to PNG encoding (verified from docs.rs/image)

```rust
use image::{RgbaImage, ImageFormat};
use std::io::Cursor;

fn rgba_to_png(width: u32, height: u32, rgba_bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
    let img = RgbaImage::from_raw(width, height, rgba_bytes.to_vec())
        .ok_or_else(|| anyhow::anyhow!("RGBA dimensions mismatch: {}x{} != {} bytes",
                                        width, height, rgba_bytes.len()))?;
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), ImageFormat::Png)?;
    Ok(out)
}
```

### SHA-256 content hash (verified from docs.rs/sha2)

```rust
use sha2::{Sha256, Digest};

fn content_hash(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
```

### SIGTERM handler (verified from docs.rs/tokio)

```rust
use tokio::signal::unix::{signal, SignalKind};

let mut sigterm = signal(SignalKind::terminate()).expect("SIGTERM handler");
tokio::select! {
    _ = sigterm.recv() => { /* shutdown */ }
    _ = tokio::signal::ctrl_c() => { /* shutdown */ }
    result = main_loop() => { /* normal exit */ }
}
```

### xclip write with MIME type (verified from Ubuntu man page)

```bash
# Write PNG bytes to X11 clipboard with image/png MIME type
# xclip will fork to background and serve paste requests
echo -n "$PNG_BYTES" | xclip -display :99 -selection clipboard -t image/png -i
```

```rust
// Rust equivalent
let mut child = tokio::process::Command::new("xclip")
    .args(["-display", ":99", "-selection", "clipboard", "-t", "image/png", "-i"])
    .stdin(std::process::Stdio::piped())
    .spawn()?;
if let Some(mut stdin) = child.stdin.take() {
    stdin.write_all(png_bytes).await?;
}
// Do NOT await child — it runs in background serving paste requests
```

### wl-copy write (verified from Arch man page)

```rust
let mut child = tokio::process::Command::new("wl-copy")
    .args(["--type", "image/png"])
    .stdin(std::process::Stdio::piped())
    .spawn()?;
if let Some(mut stdin) = child.stdin.take() {
    stdin.write_all(png_bytes).await?;
}
// wl-copy forks to background by default — do NOT await
```

### Stale lock file detection

```rust
use std::fs;

fn is_process_alive(pid: u32) -> bool {
    // POSIX: kill(pid, 0) returns 0 if process exists, -1 with ESRCH if not
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

async fn clean_stale_lock(display_num: u8) -> bool {
    let lock_path = format!("/tmp/.X{display_num}-lock");
    let socket_path = format!("/tmp/.X11-unix/X{display_num}");

    if let Ok(content) = tokio::fs::read_to_string(&lock_path).await {
        if let Ok(pid) = content.trim().parse::<u32>() {
            if !is_process_alive(pid) {
                let _ = tokio::fs::remove_file(&lock_path).await;
                let _ = tokio::fs::remove_file(&socket_path).await;
                tracing::info!("removed stale Xvfb lock (display :{display_num}, dead PID {pid})");
                return true;
            }
        }
    }
    false
}
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| arboard X11-only (default) | arboard with `wayland-data-control` feature | arboard 3.x | Wayland support without subprocess; compositor support required |
| Manual display number scanning | Xvfb `-displayfd` | X server 1.13 (2012) | Atomic free-display selection; no race conditions |
| `image::png::PNGEncoder` (old API) | `DynamicImage::write_to(cursor, Png)` | image 0.24+ | Simpler; single method; handles all pixel formats |
| `sha2::Sha256::new().update().finalize()` | `Sha256::digest(bytes)` one-liner | sha2 0.10 | Convenience method; same result |
| xclip `-selection clipboard` (implicit MIME) | xclip `-t image/png` (explicit MIME) | N/A — always required | Tools that read `image/png` specifically get the right data |
| `std::process::Command` for subprocess | `tokio::process::Command` | Phase 2 decision | Async-safe; `.kill().await` in select! context |

**Deprecated/outdated:**
- `clipboard` crate: Abandoned; text-only; do not use
- `copypasta` crate: Text-only; no image support
- `arboard` without `wayland-data-control` on Wayland desktops: Falls back to XWayland silently; enable the feature

---

## Open Questions

1. **libc dependency for kill(pid, 0) in lock file cleanup**
   - What we know: `libc::kill` is the standard POSIX way to check process liveness
   - What's unclear: Whether `libc` is already a transitive dep (likely via tokio/nix) or needs explicit addition
   - Recommendation: Add `libc = "0.2"` explicitly to Cargo.toml; it's a near-universal Rust dep and adds negligible build cost

2. **Pipe creation for -displayfd without FD_CLOEXEC**
   - What we know: Standard `pipe2` with `O_CLOEXEC` will not be inherited by Xvfb child
   - What's unclear: Whether `nix::unistd::pipe()` (without cloexec) or `libc::pipe()` is already available
   - Recommendation: Use `libc::pipe()` directly (same libc dep as above); or use `nix = "0.29"` for safer abstraction; verify fd is inherited after spawn

3. **xclip availability on Ubuntu remote**
   - What we know: xclip is not installed by default on Ubuntu Server; must be apt-installed
   - What's unclear: Whether the user's remote machine has xclip; fail-fast with install hint is the locked decision
   - Recommendation: Check for xclip with `which xclip` at daemon startup; emit `apt install xclip` hint if missing

4. **wl-clipboard availability on Ubuntu Wayland remote**
   - Same as above but for wl-copy/wl-paste: `apt install wl-clipboard`
   - Recommendation: Same fail-fast check at startup

5. **arboard Clipboard::new() thread-safety on Tokio**
   - What we know: arboard's X11 backend spawns its own thread; `Clipboard` is `!Send`
   - What's unclear: Whether `get_image()` can be called from an async context directly or needs `spawn_blocking`
   - Recommendation: Wrap arboard calls in `tokio::task::spawn_blocking` to avoid blocking the async executor; this is the correct pattern for any `!Send` + blocking I/O

---

## Discretion Recommendations

These are areas left to Claude's discretion by the user.

### Polling fallback interval

**Recommendation: 500ms**

Rationale: 500ms gives ~2 polls/second, which is responsive enough for interactive screenshot workflows. 250ms would double CPU usage for minimal UX gain. 1000ms would feel sluggish (1s delay between screenshot and send).

### Content hashing algorithm

**Recommendation: SHA-256 via `sha2` crate**

Rationale: Already in project stack research. `Sha256::digest(bytes)` is a one-liner. Collision risk for content dedup is negligible. Faster alternatives (xxhash, blake3) would require new dependencies for no meaningful benefit at screenshot rates.

### Xvfb display number selection strategy

**Recommendation: Use `-displayfd` flag**

Rationale: Delegates free-display selection to Xvfb itself. Atomically chosen. Avoids TOCTOU races from manual scanning. Supported on all Ubuntu versions from 12.04 onward (X server 1.13+).

### Xvfb restart exponential backoff parameters

**Recommendation: 5 attempts, starting at 2s, doubling (2s, 4s, 8s, 16s, 32s = ~62s total wait)**

Rationale: 5 attempts is enough to handle transient failures (port conflicts, temp filesystem issues) without hanging indefinitely. Backoff starting at 2s gives the system time to recover.

### Logging library choice

**Recommendation: Continue with `tracing` + `tracing-subscriber` (already in Cargo.toml)**

For verbose mode: implement via `tracing::debug!` / `tracing::trace!` calls gated by log level. The `-v` flag adds `RUST_LOG=debug` or sets the tracing subscriber filter to `debug` level programmatically.

---

## Sources

### Primary (HIGH confidence)

- [arboard docs.rs Clipboard struct](https://docs.rs/arboard/latest/arboard/struct.Clipboard.html) — `get_image()` return type, ImageData structure
- [arboard docs.rs ImageData](https://docs.rs/arboard/latest/arboard/struct.ImageData.html) — `width`, `height`, `bytes: Cow<[u8]>` in RGBA order
- [arboard GitHub 1Password/arboard](https://github.com/1Password/arboard) — `wayland-data-control` feature; Wayland protocol requirements (wlr-data-control / ext-data-control)
- [wl-clipboard Arch man page](https://man.archlinux.org/man/wl-clipboard.1.en) — wl-copy forks to background by default; `--foreground` overrides; `--type image/png` MIME support
- [xclip Ubuntu man page](https://manpages.ubuntu.com/manpages/jammy/man1/xclip.1.html) — default `-silent` (background fork); `-loops 0` (serve indefinitely); `-t` MIME type flag
- [tokio signal docs.rs](https://docs.rs/tokio/latest/tokio/signal/) — `signal(SignalKind::terminate())`, `recv()` cancel safety
- [tokio process Child docs.rs](https://docs.rs/tokio/latest/tokio/process/struct.Child.html) — `.kill().await`, `kill_on_drop`

### Secondary (MEDIUM confidence)

- [image crate docs.rs](https://docs.rs/image/latest/image/) — `RgbaImage::from_raw`, `write_to(Cursor, ImageFormat::Png)` pattern; verified from crates.io page
- [sha2 docs.rs](https://docs.rs/sha2/latest/sha2/) — `Sha256::digest()` one-liner; `Digest` trait re-export
- [Xvfb man page x.org](https://www.x.org/releases/X11R7.6/doc/man/man1/Xvfb.1.xhtml) — `-displayfd` option (supported since X server 1.13); `-screen 0 WxHxD` syntax
- [X11 clipboard write blog (jameshunt.us)](https://jameshunt.us/writings/x11-clipboard-management-foibles/) — X11 selection ownership model; xclip background behavior confirmation
- [wayland-clipboard-listener crates.io](https://crates.io/crates/wayland-clipboard-listener) — event-driven Wayland clipboard watching; `WlClipboardPasteStream` API

### Tertiary (LOW confidence)

- WebSearch consensus: arboard `Clipboard::new()` is not `Send` on Linux (X11 backend); needs `spawn_blocking` — needs empirical verification against current arboard source
- WebSearch consensus: Xvfb stale lock file format contains PID as decimal string — consistent with multiple sources but not verified against Xvfb source code directly

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — versions confirmed from crates.io; arboard, image, sha2 APIs verified from docs.rs
- Architecture: HIGH — xclip/wl-copy background fork behavior confirmed from official man pages; tokio signal API confirmed from docs.rs
- Pitfalls: HIGH for clipboard-drop and MIME-type pitfalls (confirmed from docs); MEDIUM for fd inheritance (common pattern but not empirically tested in this project)

**Research date:** 2026-02-27
**Valid until:** 2026-08-27 (arboard 3.x is stable; tokio 1.x is LTS; xclip/wl-copy man page behavior is stable POSIX/Wayland protocol behavior)
