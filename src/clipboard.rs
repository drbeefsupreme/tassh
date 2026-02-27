//! Clipboard read/write operations.
//!
//! - [`watch_clipboard`] — local side: polls the system clipboard for new images, sends PNG bytes
//!   over an mpsc channel when content changes.
//! - [`ClipboardWriter`] — remote side: writes received PNG bytes to the system clipboard using
//!   the correct subprocess for the detected display environment.
//! - [`check_clipboard_tools`] — called at daemon startup to verify required tools are present.

use std::io::Cursor;

use anyhow::anyhow;
use image::{ImageFormat, RgbaImage};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::protocol::{DisplayEnvironment, Frame};

// ---------------------------------------------------------------------------
// Local side: watch_clipboard
// ---------------------------------------------------------------------------

/// Watch the system clipboard for new images.
///
/// Runs forever (until the sender is closed or an error occurs). On each new distinct image
/// (detected via SHA-256 content hash) the PNG-encoded bytes are sent over `tx`.
///
/// **Startup-skip behaviour:** the image present on the clipboard at startup is recorded
/// (its hash is stored) but NOT sent. This prevents re-sending a stale screenshot from a
/// previous session on restart.
///
/// # Errors
///
/// Returns immediately if no display server is found (`WAYLAND_DISPLAY` and `DISPLAY` are
/// both unset/empty) or if the clipboard backend fails to initialise.
pub async fn watch_clipboard(tx: tokio::sync::mpsc::Sender<Frame>) -> anyhow::Result<()> {
    // --- Display auto-detection (CLRD-04) ---
    if let Ok(wd) = std::env::var("WAYLAND_DISPLAY") {
        if !wd.is_empty() {
            tracing::info!("clipboard: using Wayland ({})", wd);
        } else {
            check_x11_display()?;
        }
    } else {
        check_x11_display()?;
    }

    // --- Initialise arboard inside spawn_blocking (X11 backend is !Send + blocking I/O) ---
    let mut clipboard =
        tokio::task::spawn_blocking(|| arboard::Clipboard::new())
            .await
            .map_err(|e| anyhow!("spawn_blocking panicked: {e}"))??;

    // Provide actionable error if backend failed to connect.
    // (arboard surfaces this as an error from Clipboard::new(), already propagated above.)

    // --- Startup-skip (CLRD-01): record initial hash without sending ---
    let mut last_hash: Option<[u8; 32]> = None;

    {
        // We need to move `clipboard` into the blocking task. Because arboard is !Send we
        // must keep it on the same thread for all calls. We'll use a dedicated blocking
        // task via a channel to serialise all clipboard access.
    }

    // arboard's X11/Wayland backend is !Send, so we cannot move it across await points.
    // Instead, run the entire polling loop inside a single spawn_blocking call that parks
    // itself with std::thread::sleep between polls.
    let result = tokio::task::spawn_blocking(move || {
        // Startup-skip: sample whatever is on the clipboard now.
        if let Ok(img) = clipboard.get_image() {
            let hash = content_hash(&img.bytes);
            last_hash = Some(hash);
            tracing::debug!(
                "clipboard: startup snapshot recorded ({}x{}), not sending",
                img.width,
                img.height
            );
        } else {
            tracing::debug!("clipboard: no image on clipboard at startup");
        }

        // Polling loop
        loop {
            std::thread::sleep(std::time::Duration::from_millis(500));

            match clipboard.get_image() {
                Ok(img) => {
                    let hash = content_hash(&img.bytes);
                    if Some(hash) == last_hash {
                        tracing::debug!("clipboard: image unchanged (hash match), skipping");
                        continue;
                    }

                    // New image — encode to PNG
                    match rgba_to_png(img.width as u32, img.height as u32, &img.bytes) {
                        Ok(png_bytes) => {
                            let kb = png_bytes.len() / 1024;
                            tracing::info!(
                                "clipboard image captured ({} KB), sending",
                                kb
                            );
                            // Wrap in Frame and send over the channel (blocking, since we're in a blocking thread).
                            if tx.blocking_send(Frame::new_png(png_bytes)).is_err() {
                                // Receiver dropped — caller shut down, exit cleanly.
                                tracing::debug!("clipboard: tx closed, stopping watch loop");
                                return Ok::<(), anyhow::Error>(());
                            }
                            last_hash = Some(hash);
                        }
                        Err(e) => {
                            tracing::warn!("clipboard: failed to encode PNG: {e}");
                        }
                    }
                }
                Err(_) => {
                    tracing::debug!("clipboard: no image on clipboard");
                }
            }
        }
    })
    .await
    .map_err(|e| anyhow!("clipboard watch task panicked: {e}"))?;

    result
}

/// Check that `$DISPLAY` is set and non-empty; return a clear error if not.
fn check_x11_display() -> anyhow::Result<()> {
    match std::env::var("DISPLAY") {
        Ok(d) if !d.is_empty() => {
            tracing::info!("clipboard: using X11 ({})", d);
            Ok(())
        }
        _ => Err(anyhow!(
            "No display server found. Set WAYLAND_DISPLAY or DISPLAY."
        )),
    }
}

// ---------------------------------------------------------------------------
// Remote side: ClipboardWriter
// ---------------------------------------------------------------------------

/// Writes PNG image bytes to the system clipboard via the appropriate subprocess.
///
/// Use [`check_clipboard_tools`] at daemon startup before constructing this.
pub struct ClipboardWriter {
    /// The running clipboard-holder subprocess from the previous write, if any.
    ///
    /// xclip and wl-copy must stay alive to service paste (SelectionRequest) events.
    /// We keep the handle here so we can kill it before spawning a new one.
    current_child: Option<tokio::process::Child>,

    /// Which display environment we're writing to.
    display: DisplayEnvironment,
}

impl ClipboardWriter {
    /// Create a new writer for the given display environment.
    pub fn new(display: DisplayEnvironment) -> Self {
        Self {
            current_child: None,
            display,
        }
    }

    /// Write `png_bytes` to the system clipboard.
    ///
    /// Kills any previous clipboard-holder subprocess, then spawns a new one, pipes the PNG
    /// data to its stdin, and stores the child handle **without** calling `.wait()` so it
    /// stays alive to serve paste requests.
    ///
    /// # Errors
    ///
    /// Returns an error if the subprocess cannot be spawned or stdin write fails.
    pub async fn write(&mut self, png_bytes: &[u8]) -> anyhow::Result<()> {
        // --- Kill previous clipboard holder (CLWR-05) ---
        if let Some(mut child) = self.current_child.take() {
            tracing::debug!("killing previous clipboard holder");
            let _ = child.kill().await;
        }

        // --- Dispatch by display environment (CLWR-04) ---
        let mut child = match self.display {
            DisplayEnvironment::Wayland => {
                // CLWR-03: wl-copy with explicit MIME type
                tokio::process::Command::new("wl-copy")
                    .args(["--type", "image/png"])
                    .stdin(std::process::Stdio::piped())
                    .spawn()
                    .map_err(|e| {
                        if e.kind() == std::io::ErrorKind::NotFound {
                            anyhow!("wl-copy not found. Install with: sudo apt install wl-clipboard")
                        } else {
                            anyhow!("failed to spawn wl-copy: {e}")
                        }
                    })?
            }
            DisplayEnvironment::X11 | DisplayEnvironment::Xvfb => {
                // CLWR-02: xclip with clipboard selection + MIME type
                // Pass -display explicitly from the current env var.
                let display_val =
                    std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string());
                tokio::process::Command::new("xclip")
                    .args([
                        "-selection",
                        "clipboard",
                        "-t",
                        "image/png",
                        "-i",
                        "-display",
                        &display_val,
                    ])
                    .stdin(std::process::Stdio::piped())
                    .spawn()
                    .map_err(|e| {
                        if e.kind() == std::io::ErrorKind::NotFound {
                            anyhow!("xclip not found. Install with: sudo apt install xclip")
                        } else {
                            anyhow!("failed to spawn xclip: {e}")
                        }
                    })?
            }
            DisplayEnvironment::Headless => {
                return Err(anyhow!("no display available for clipboard write"));
            }
        };

        // --- Pipe PNG data to stdin (CLWR-01) ---
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(png_bytes).await.map_err(|e| {
                anyhow!("failed to write PNG to clipboard subprocess stdin: {e}")
            })?;
            // Drop stdin — signals EOF so the subprocess knows we're done sending data.
        }

        tracing::info!("clipboard write: {} KB", png_bytes.len() / 1024);

        // --- Store child WITHOUT .wait() (CLWR-05) ---
        // xclip / wl-copy must stay alive to serve SelectionRequest events.
        self.current_child = Some(child);

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tool availability check
// ---------------------------------------------------------------------------

/// Check that the required clipboard tool is available for the given display environment.
///
/// Call this at daemon startup before constructing a [`ClipboardWriter`].
///
/// # Errors
///
/// Returns a descriptive error with an install hint if a required tool is missing.
pub async fn check_clipboard_tools(display: &DisplayEnvironment) -> anyhow::Result<()> {
    match display {
        DisplayEnvironment::X11 | DisplayEnvironment::Xvfb => {
            let status = tokio::process::Command::new("which")
                .arg("xclip")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .await
                .map_err(|e| anyhow!("failed to run which: {e}"))?;

            if !status.success() {
                return Err(anyhow!(
                    "xclip not found. Install with: sudo apt install xclip"
                ));
            }
            Ok(())
        }
        DisplayEnvironment::Wayland => {
            let status = tokio::process::Command::new("which")
                .arg("wl-copy")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .await
                .map_err(|e| anyhow!("failed to run which: {e}"))?;

            if !status.success() {
                return Err(anyhow!(
                    "wl-copy not found. Install with: sudo apt install wl-clipboard"
                ));
            }
            Ok(())
        }
        DisplayEnvironment::Headless => Err(anyhow!(
            "No display available for clipboard write. Run display detection first."
        )),
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Encode raw RGBA bytes to a PNG `Vec<u8>`.
fn rgba_to_png(width: u32, height: u32, rgba: &[u8]) -> anyhow::Result<Vec<u8>> {
    let expected = (width as usize) * (height as usize) * 4;
    if rgba.len() != expected {
        return Err(anyhow!(
            "rgba_to_png: byte count mismatch: expected {expected}, got {}",
            rgba.len()
        ));
    }

    let img = RgbaImage::from_raw(width, height, rgba.to_vec())
        .ok_or_else(|| anyhow!("rgba_to_png: RgbaImage::from_raw failed"))?;

    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
        .map_err(|e| anyhow!("rgba_to_png: PNG encoding failed: {e}"))?;

    Ok(buf)
}

/// Compute SHA-256 of a byte slice and return the digest as a fixed-size array.
fn content_hash(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
