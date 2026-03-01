//! TCP transport layer (Phase 2)
//!
//! Provides [`server`] (listening end) and [`client`] (connecting end with auto-reconnect),
//! plus helpers for frame framing over TCP and TCP keepalive configuration.

use std::{io, time::Duration};

use socket2::{SockRef, TcpKeepalive};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpListener, TcpStream,
    },
    time::timeout,
};
use tracing::{info, warn};

use crate::clipboard::ClipboardWriter;
use crate::protocol::{DisplayEnvironment, Frame, FrameError};

/// Header length for a tassh frame: 2 magic + 1 version + 1 type + 4 length.
const HEADER_LEN: usize = 8;

/// Errors that can arise in the transport layer.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("write timed out (10 s deadline exceeded)")]
    WriteTimeout,

    #[error("read timed out (30 s deadline exceeded)")]
    ReadTimeout,

    #[error("connection closed by peer")]
    ConnectionClosed,

    #[error("frame error: {0}")]
    Frame(#[from] FrameError),
}

/// Apply TCP keepalive to a connected stream.
///
/// Configuration:
/// - Idle time before first probe: 10 s
/// - Interval between probes: 5 s
/// - Number of probes: 3
///
/// Detection latency: ≤ 10 + 5*3 = 25 s, which satisfies the ≤ 30 s requirement.
pub fn apply_keepalive(stream: &TcpStream) -> io::Result<()> {
    let keepalive = TcpKeepalive::new()
        .with_time(Duration::from_secs(10))
        .with_interval(Duration::from_secs(5))
        .with_retries(3);
    SockRef::from(stream).set_tcp_keepalive(&keepalive)
}

/// Serialize and write a [`Frame`] to the writer.
///
/// A 10-second write timeout is applied; if the deadline is exceeded
/// [`TransportError::WriteTimeout`] is returned and the caller should reconnect.
pub async fn send_frame(writer: &mut OwnedWriteHalf, frame: &Frame) -> Result<(), TransportError> {
    let bytes = frame.to_bytes()?;
    timeout(Duration::from_secs(10), writer.write_all(&bytes))
        .await
        .map_err(|_| TransportError::WriteTimeout)?
        .map_err(TransportError::Io)
}

/// Read one [`Frame`] from the reader.
///
/// A 30-second read timeout is applied to both the header read and the payload
/// read. If the deadline is exceeded [`TransportError::ReadTimeout`] is returned
/// and the caller should close the connection.
///
/// Returns [`TransportError::ConnectionClosed`] when the peer has closed the
/// connection cleanly (EOF on the 8-byte header read).
pub async fn recv_frame(reader: &mut OwnedReadHalf) -> Result<Frame, TransportError> {
    let mut header = [0u8; HEADER_LEN];
    match timeout(Duration::from_secs(30), reader.read_exact(&mut header)).await {
        Ok(Ok(_)) => {}
        Ok(Err(e)) if e.kind() == io::ErrorKind::UnexpectedEof => {
            return Err(TransportError::ConnectionClosed);
        }
        Ok(Err(e)) => return Err(TransportError::Io(e)),
        Err(_) => return Err(TransportError::ReadTimeout),
    }

    let payload_len = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;
    let mut payload = vec![0u8; payload_len];
    timeout(Duration::from_secs(30), reader.read_exact(&mut payload))
        .await
        .map_err(|_| TransportError::ReadTimeout)?
        .map_err(|e| {
            if e.kind() == io::ErrorKind::UnexpectedEof {
                TransportError::ConnectionClosed
            } else {
                TransportError::Io(e)
            }
        })?;

    let mut full = Vec::with_capacity(HEADER_LEN + payload_len);
    full.extend_from_slice(&header);
    full.extend_from_slice(&payload);

    Frame::from_bytes(&full).map_err(TransportError::Frame)
}

/// Auto-detect the local Tailscale IPv4 address by running `tailscale ip -4`.
async fn resolve_tailscale_ip() -> Result<String, TransportError> {
    let output = tokio::process::Command::new("tailscale")
        .args(["ip", "-4"])
        .output()
        .await?;
    let ip = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    Ok(ip)
}

/// Run as the remote (server) side.
///
/// Binds a TCP listener on `bind_addr:port` and accepts one connection at a time.
/// For each accepted connection, received frames are written to the system clipboard
/// using [`ClipboardWriter`] with the given `display_env`.
///
/// If `bind_addr` is `"auto"` (set by main.rs when `--bind` is not provided),
/// the Tailscale IPv4 address is auto-detected.
pub async fn server(
    bind_addr: &str,
    port: u16,
    display_env: DisplayEnvironment,
) -> Result<(), TransportError> {
    let resolved = if bind_addr == "auto" {
        resolve_tailscale_ip().await?
    } else {
        bind_addr.to_owned()
    };

    let listener = TcpListener::bind(format!("{resolved}:{port}")).await?;
    let addr = listener.local_addr()?;
    info!("listening on {addr}");

    loop {
        let (stream, peer) = listener.accept().await?;
        warn!("accepted connection from {peer}");
        apply_keepalive(&stream)?;

        let (mut reader, _writer) = stream.into_split();
        // Create a ClipboardWriter per connection so each connection gets a fresh writer.
        let mut writer = ClipboardWriter::new(display_env, None);
        loop {
            match recv_frame(&mut reader).await {
                Ok(frame) => {
                    let kb = frame.payload.len() / 1024;
                    info!("received frame: {kb} KB payload, writing to clipboard");
                    if let Err(e) = writer.write(&frame.payload).await {
                        warn!("clipboard write failed: {e}");
                    }
                }
                Err(TransportError::ConnectionClosed) => {
                    warn!("client disconnected");
                    break;
                }
                Err(e) => {
                    warn!("connection error: {e}");
                    break;
                }
            }
        }
    }
}

/// Run as the local (client) side.
///
/// Connects to `remote_addr:port` and reads frames from `frame_rx`.
/// On any send error or connection failure the client reconnects using
/// exponential backoff with jitter (initial 1 s, maximum 30 s).
pub async fn client(
    remote_addr: &str,
    port: u16,
    mut frame_rx: tokio::sync::mpsc::Receiver<Frame>,
) -> Result<(), TransportError> {
    let addr = format!("{remote_addr}:{port}");
    let mut backoff: f64 = 1.0;

    loop {
        match TcpStream::connect(&addr).await {
            Ok(stream) => {
                warn!("connected to {addr}");
                if let Err(e) = apply_keepalive(&stream) {
                    warn!("failed to set keepalive: {e}");
                }
                backoff = 1.0;

                let (_reader, mut writer) = stream.into_split();
                loop {
                    match frame_rx.recv().await {
                        Some(frame) => {
                            if let Err(e) = send_frame(&mut writer, &frame).await {
                                warn!("connection lost: {e}");
                                break;
                            }
                        }
                        None => {
                            // Channel closed — sender dropped, nothing left to do.
                            return Ok(());
                        }
                    }
                }
            }
            Err(e) => {
                warn!("connect failed: {e}; retrying in {backoff:.1}s");
            }
        }

        // Exponential backoff with jitter.
        let jitter = rand::random::<f64>() * backoff * 0.25;
        tokio::time::sleep(Duration::from_secs_f64(backoff + jitter)).await;
        backoff = (backoff * 2.0).min(30.0);
    }
}
