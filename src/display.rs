//! Display environment detection and Xvfb lifecycle management.
//!
//! [`DisplayManager::detect_and_init`] detects the current display environment and,
//! if running headless, spawns Xvfb and publishes the chosen display string to
//! `~/.cssh/display` for SSH sessions that source it.

#![allow(dead_code)]

use std::os::unix::io::RawFd;
use std::sync::Arc;

use anyhow::{anyhow, Context};
use tokio::sync::Mutex;

use crate::protocol::DisplayEnvironment;

/// Manages the display environment for the remote daemon.
///
/// Created via [`DisplayManager::detect_and_init`]. Call [`DisplayManager::shutdown`] on exit.
pub struct DisplayManager {
    /// The kind of display environment detected (Wayland, X11, or Xvfb).
    pub env: DisplayEnvironment,
    /// The display identifier string, e.g. `":1"` or a Wayland socket name.
    pub display_str: String,
    /// Handle to the Xvfb child process, if we spawned one.
    ///
    /// Wrapped in Arc<Mutex<>> so the auto-restart background task can swap in a new child.
    xvfb_child: Option<Arc<Mutex<Option<tokio::process::Child>>>>,
}

impl DisplayManager {
    /// Detect the current display environment and initialise accordingly.
    ///
    /// Detection order (when `force_xvfb` is false):
    /// 1. `$WAYLAND_DISPLAY` set and non-empty → [`DisplayEnvironment::Wayland`]
    /// 2. `$DISPLAY` set and non-empty → [`DisplayEnvironment::X11`]
    /// 3. Neither → headless path: clean stale locks, spawn Xvfb, publish `~/.cssh/display`
    ///
    /// When `force_xvfb` is true, skip Wayland/X11 detection and always spawn Xvfb.
    /// This is used by `cssh remote` so that SSH sessions can read the clipboard
    /// via the published `~/.cssh/display` file, even on machines with a desktop session.
    pub async fn detect_and_init(force_xvfb: bool) -> anyhow::Result<Self> {
        if !force_xvfb {
            // 1. Try Wayland
            if let Ok(wd) = std::env::var("WAYLAND_DISPLAY") {
                if !wd.is_empty() {
                    tracing::info!("display: Wayland ({})", wd);
                    return Ok(Self {
                        env: DisplayEnvironment::Wayland,
                        display_str: wd,
                        xvfb_child: None,
                    });
                }
            }
            tracing::debug!("display: WAYLAND_DISPLAY not set, checking DISPLAY");

            // 2. Try X11
            if let Ok(d) = std::env::var("DISPLAY") {
                if !d.is_empty() {
                    tracing::info!("display: X11 ({})", d);
                    return Ok(Self {
                        env: DisplayEnvironment::X11,
                        display_str: d,
                        xvfb_child: None,
                    });
                }
            }
            tracing::debug!("display: DISPLAY not set, entering headless path");
        } else {
            tracing::info!("display: force_xvfb=true, skipping Wayland/X11 detection");
        }

        // 3. Headless — clean stale locks, then spawn Xvfb
        tracing::info!("display: headless, cleaning stale Xvfb lock files");
        clean_stale_xvfb_locks().await;

        tracing::info!("display: headless, spawning Xvfb");
        let (child, display_str) = spawn_xvfb().await?;

        // Set DISPLAY in this process's environment so child processes inherit it.
        // SAFETY: This is called once at startup before any threads read DISPLAY.
        #[allow(deprecated)]
        unsafe {
            std::env::set_var("DISPLAY", &display_str);
        }
        tracing::info!("Xvfb spawned on display {}", display_str);

        // Publish to ~/.cssh/display
        publish_display(&display_str)
            .await
            .context("failed to write ~/.cssh/display")?;

        let child_handle = Arc::new(Mutex::new(Some(child)));
        let child_for_monitor = Arc::clone(&child_handle);

        // Background task: monitor Xvfb and auto-restart with exponential backoff.
        let display_str_clone = display_str.clone();
        tokio::spawn(async move {
            monitor_xvfb(child_for_monitor, display_str_clone).await;
        });

        Ok(Self {
            env: DisplayEnvironment::Xvfb,
            display_str,
            xvfb_child: Some(child_handle),
        })
    }

    /// Shut down cleanly: kill Xvfb (if running) and remove `~/.cssh/display`.
    pub async fn shutdown(self) {
        if let Some(child_handle) = self.xvfb_child {
            let mut guard = child_handle.lock().await;
            if let Some(mut child) = guard.take() {
                let _ = child.kill().await;
                tracing::debug!("Xvfb process killed");
            }
        }
        let display_path = display_file_path();
        let _ = std::fs::remove_file(&display_path);
        tracing::info!("Xvfb stopped, {} removed", display_path.display());
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Spawn a fresh Xvfb process using `-displayfd` for automatic display number selection.
///
/// Returns `(child, display_str)` where `display_str` is e.g. `":3"`.
async fn spawn_xvfb() -> anyhow::Result<(tokio::process::Child, String)> {
    // Create a plain Unix pipe (no O_CLOEXEC so the write end is inherited by Xvfb).
    let (read_fd, write_fd) = create_pipe().context("failed to create pipe for -displayfd")?;

    let write_fd_str = write_fd.to_string();
    let child = tokio::process::Command::new("Xvfb")
        .args(["-displayfd", &write_fd_str, "-screen", "0", "1x1x24"])
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow!("Xvfb not found. Install with: apt install xvfb")
            } else {
                anyhow!("failed to spawn Xvfb: {e}")
            }
        })?;

    // Close write end in parent — otherwise read_fd will never see EOF if Xvfb exits.
    // SAFETY: write_fd is a valid fd we just created via libc::pipe.
    unsafe { libc::close(write_fd) };

    // Read the display number written by Xvfb to the fd (e.g. "3\n").
    let display_num = read_displayfd(read_fd)
        .await
        .context("failed to read display number from Xvfb -displayfd")?;

    let display_str = format!(":{display_num}");
    Ok((child, display_str))
}

/// Create a plain `pipe(2)` (without `O_CLOEXEC`) so the write end is inherited by child processes.
///
/// Returns `(read_fd, write_fd)`.
fn create_pipe() -> anyhow::Result<(RawFd, RawFd)> {
    let mut fds: [libc::c_int; 2] = [0; 2];
    // SAFETY: fds is a valid 2-element array.
    let ret = unsafe { libc::pipe(fds.as_mut_ptr()) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        return Err(anyhow!("libc::pipe failed: {err}"));
    }
    Ok((fds[0], fds[1]))
}

/// Read the display number from the pipe read-end written by Xvfb via `-displayfd`.
///
/// Xvfb writes e.g. `"3\n"` once it has chosen a display number.
async fn read_displayfd(read_fd: RawFd) -> anyhow::Result<u32> {
    use tokio::io::AsyncReadExt;

    // Wrap the raw fd in an async file handle.
    // SAFETY: read_fd is a valid fd; we own it exclusively.
    let std_file = unsafe { <std::fs::File as std::os::unix::io::FromRawFd>::from_raw_fd(read_fd) };
    let mut async_file = tokio::fs::File::from_std(std_file);

    let mut buf = String::new();
    async_file
        .read_to_string(&mut buf)
        .await
        .context("read from -displayfd pipe failed")?;

    let trimmed = buf.trim();
    trimmed
        .parse::<u32>()
        .map_err(|_| anyhow!("Xvfb wrote unexpected display number: {:?}", trimmed))
}

/// Scan `/tmp/.X{N}-lock` for N in 0..100. For each lock file whose PID is dead,
/// remove the lock and the corresponding Unix socket.
async fn clean_stale_xvfb_locks() {
    for n in 0u32..100 {
        let lock_path = format!("/tmp/.X{n}-lock");
        let socket_path = format!("/tmp/.X11-unix/X{n}");

        let pid_str = match tokio::fs::read_to_string(&lock_path).await {
            Ok(s) => s,
            Err(_) => {
                tracing::debug!("display: /tmp/.X{n}-lock not present, skipping");
                continue;
            }
        };

        let pid = match pid_str.trim().parse::<libc::pid_t>() {
            Ok(p) if p > 0 => p,
            _ => {
                tracing::debug!("display: /tmp/.X{n}-lock has unparseable PID, skipping");
                continue;
            }
        };

        // SAFETY: kill(pid, 0) is POSIX; returns 0 if process is alive, -1 with ESRCH if dead.
        let alive = unsafe { libc::kill(pid, 0) == 0 };

        if !alive {
            let _ = tokio::fs::remove_file(&lock_path).await;
            let _ = tokio::fs::remove_file(&socket_path).await;
            tracing::info!(
                "removed stale Xvfb lock: {} (dead PID {})",
                lock_path,
                pid
            );
        } else {
            tracing::debug!("display: /tmp/.X{n}-lock PID {pid} is alive, not removing");
        }
    }
}

/// Write `export DISPLAY={display_str}\n` to `~/.cssh/display`, creating the directory if needed.
async fn publish_display(display_str: &str) -> anyhow::Result<()> {
    let path = display_file_path();
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let content = format!("export DISPLAY={display_str}\n");
    tokio::fs::write(&path, &content)
        .await
        .with_context(|| format!("failed to write {}", path.display()))?;
    tracing::debug!("published display: {}", path.display());
    Ok(())
}

/// Returns the path `$HOME/.cssh/display`, falling back to `/root` if `$HOME` is unset.
fn display_file_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    std::path::PathBuf::from(home).join(".cssh").join("display")
}

/// Background task that monitors Xvfb and auto-restarts it on unexpected exit.
///
/// Exponential backoff: 2s, 4s, 8s, 16s, 32s. Gives up after 5 failed attempts
/// and calls `std::process::exit(1)`.
async fn monitor_xvfb(child_handle: Arc<Mutex<Option<tokio::process::Child>>>, display_str: String) {
    let mut attempts: u32 = 0;
    const MAX_ATTEMPTS: u32 = 5;

    loop {
        // Wait for the current child to exit.
        let status = {
            let mut guard = child_handle.lock().await;
            if let Some(ref mut child) = *guard {
                match child.wait().await {
                    Ok(status) => status,
                    Err(e) => {
                        tracing::error!("Xvfb wait() error: {e}");
                        break;
                    }
                }
            } else {
                // Child was taken by shutdown() — exit gracefully.
                tracing::debug!("Xvfb monitor: child was taken, exiting monitor task");
                break;
            }
        };

        tracing::warn!("Xvfb exited unexpectedly (status: {status:?})");
        attempts += 1;

        if attempts >= MAX_ATTEMPTS {
            tracing::error!("Xvfb failed {MAX_ATTEMPTS} times in a row, giving up");
            std::process::exit(1);
        }

        let backoff_secs = 2u64.pow(attempts);
        tracing::info!(
            "Xvfb restart attempt {attempts}/{MAX_ATTEMPTS} in {backoff_secs}s"
        );
        tokio::time::sleep(tokio::time::Duration::from_secs(backoff_secs)).await;

        // Re-spawn Xvfb.
        match spawn_xvfb().await {
            Ok((new_child, new_display)) => {
                if new_display != display_str {
                    tracing::warn!(
                        "Xvfb restarted on different display {} (was {})",
                        new_display,
                        display_str
                    );
                }
                // Set DISPLAY again in case it changed (best-effort).
                #[allow(deprecated)]
                unsafe {
                    std::env::set_var("DISPLAY", &new_display);
                }
                if let Err(e) = publish_display(&new_display).await {
                    tracing::warn!("Failed to re-publish display: {e}");
                }
                tracing::info!("Xvfb restarted on display {new_display}");

                let mut guard = child_handle.lock().await;
                *guard = Some(new_child);
                // Reset attempt counter on success.
                attempts = 0;
            }
            Err(e) => {
                tracing::error!("Failed to restart Xvfb (attempt {attempts}): {e}");
            }
        }
    }
}
