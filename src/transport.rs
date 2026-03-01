//! TCP transport layer helpers.
//!
//! Provides helpers for frame framing over TCP and TCP keepalive configuration.

use std::{io, time::Duration};

use socket2::{SockRef, TcpKeepalive};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpStream,
    },
    time::timeout,
};

use crate::protocol::{Frame, FrameError};

/// Header length for a tassh frame: 2 magic + 1 version + 1 type + 4 length.
const HEADER_LEN: usize = 8;

/// Errors that can arise in the transport layer.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("write timed out (10 s deadline exceeded)")]
    WriteTimeout,

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
/// Returns [`TransportError::ConnectionClosed`] when the peer has closed the
/// connection cleanly (EOF on the 8-byte header read).
pub async fn recv_frame(reader: &mut OwnedReadHalf) -> Result<Frame, TransportError> {
    let mut header = [0u8; HEADER_LEN];
    match reader.read_exact(&mut header).await {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
            return Err(TransportError::ConnectionClosed);
        }
        Err(e) => return Err(TransportError::Io(e)),
    }

    let payload_len = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;
    let mut payload = vec![0u8; payload_len];
    reader.read_exact(&mut payload).await.map_err(|e| {
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
