//! Wire protocol framing types.

/// Magic bytes that identify a tassh frame.
/// 0xC5 is non-ASCII (prevents confusion with text), 0x53 is 'S'.
pub const MAGIC: [u8; 2] = [0xC5, 0x53];

/// Protocol version.
pub const VERSION: u8 = 1;

/// Frame type identifier for PNG payloads.
pub const FRAME_TYPE_PNG: u8 = 0x01;

/// Header length in bytes: 2 magic + 1 version + 1 type + 4 length.
const HEADER_LEN: usize = 8;

/// Errors that can occur when serializing or deserializing a [`Frame`].
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("invalid magic bytes")]
    InvalidMagic,

    #[error("unsupported protocol version: {0}")]
    UnsupportedVersion(u8),

    #[error("frame data too short")]
    TooShort,

    #[error("frame payload exceeds maximum size ({0} bytes)")]
    TooLarge(usize),

    #[error("payload length mismatch: header says {expected} bytes, got {actual}")]
    LengthMismatch { expected: usize, actual: usize },
}

/// The display environment the remote side is running in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayEnvironment {
    Wayland,
    X11,
    Xvfb,
    Headless,
}

/// A framed message on the wire.
///
/// Wire layout (big-endian):
/// ```text
/// [0..2]  MAGIC  (2 bytes)
/// [2]     VERSION (1 byte)
/// [3]     frame_type (1 byte)
/// [4..8]  payload length as u32 (4 bytes, big-endian)
/// [8..]   payload (variable)
/// ```
#[derive(Debug)]
pub struct Frame {
    pub frame_type: u8,
    pub payload: Vec<u8>,
}

impl Frame {
    /// Construct a PNG frame.
    pub fn new_png(payload: Vec<u8>) -> Self {
        Frame {
            frame_type: FRAME_TYPE_PNG,
            payload,
        }
    }

    /// Serialize this frame to bytes.
    ///
    /// Returns [`FrameError::TooLarge`] if the payload exceeds `u32::MAX` bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>, FrameError> {
        let len = self.payload.len();
        if len > u32::MAX as usize {
            return Err(FrameError::TooLarge(len));
        }
        let mut out = Vec::with_capacity(HEADER_LEN + len);
        out.push(MAGIC[0]);
        out.push(MAGIC[1]);
        out.push(VERSION);
        out.push(self.frame_type);
        out.extend_from_slice(&(len as u32).to_be_bytes());
        out.extend_from_slice(&self.payload);
        Ok(out)
    }

    /// Deserialize a frame from bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Frame, FrameError> {
        if data.len() < HEADER_LEN {
            return Err(FrameError::TooShort);
        }

        if data[0] != MAGIC[0] || data[1] != MAGIC[1] {
            return Err(FrameError::InvalidMagic);
        }

        if data[2] != VERSION {
            return Err(FrameError::UnsupportedVersion(data[2]));
        }

        let frame_type = data[3];
        let expected_len = u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize;

        let available = data.len() - HEADER_LEN;
        if available != expected_len {
            return Err(FrameError::LengthMismatch {
                expected: expected_len,
                actual: available,
            });
        }

        Ok(Frame {
            frame_type,
            payload: data[HEADER_LEN..].to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_png_frame() {
        // Use the PNG magic bytes as payload to test byte fidelity
        let payload = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        let frame = Frame::new_png(payload.clone());
        let encoded = frame.to_bytes().expect("to_bytes failed");
        let decoded = Frame::from_bytes(&encoded).expect("from_bytes failed");
        assert_eq!(decoded.frame_type, FRAME_TYPE_PNG);
        assert_eq!(decoded.payload, payload);
    }

    #[test]
    fn round_trip_empty_payload() {
        let frame = Frame::new_png(vec![]);
        let encoded = frame.to_bytes().expect("to_bytes failed");
        assert_eq!(encoded.len(), HEADER_LEN);
        let decoded = Frame::from_bytes(&encoded).expect("from_bytes failed");
        assert_eq!(decoded.payload, &[] as &[u8]);
    }

    #[test]
    fn round_trip_large_payload() {
        let payload = vec![0xAB; 65536]; // 64 KB
        let frame = Frame::new_png(payload.clone());
        let encoded = frame.to_bytes().expect("to_bytes failed");
        assert_eq!(encoded.len(), HEADER_LEN + 65536);
        let decoded = Frame::from_bytes(&encoded).expect("from_bytes failed");
        assert_eq!(decoded.payload, payload);
    }

    #[test]
    fn rejects_invalid_magic() {
        let mut data = vec![0x00u8; HEADER_LEN];
        data[0] = 0xFF; // wrong magic
        data[1] = 0xFF;
        data[2] = VERSION;
        let result = Frame::from_bytes(&data);
        assert!(matches!(result, Err(FrameError::InvalidMagic)));
    }

    #[test]
    fn rejects_unsupported_version() {
        let mut data = vec![0x00u8; HEADER_LEN];
        data[0] = MAGIC[0];
        data[1] = MAGIC[1];
        data[2] = 0xFF; // bad version
        data[3] = FRAME_TYPE_PNG;
        // length = 0
        let result = Frame::from_bytes(&data);
        assert!(matches!(result, Err(FrameError::UnsupportedVersion(0xFF))));
    }

    #[test]
    fn rejects_truncated_header() {
        let data = vec![0xC5u8, 0x53, 0x01]; // only 3 bytes — less than HEADER_LEN
        let result = Frame::from_bytes(&data);
        assert!(matches!(result, Err(FrameError::TooShort)));
    }

    #[test]
    fn rejects_truncated_payload() {
        // Build a valid header claiming 100 bytes of payload, but only provide 10
        let mut data = Vec::with_capacity(HEADER_LEN + 10);
        data.push(MAGIC[0]);
        data.push(MAGIC[1]);
        data.push(VERSION);
        data.push(FRAME_TYPE_PNG);
        data.extend_from_slice(&100u32.to_be_bytes()); // claims 100 bytes
        data.extend_from_slice(&[0xABu8; 10]); // only 10 bytes present
        let result = Frame::from_bytes(&data);
        assert!(matches!(
            result,
            Err(FrameError::LengthMismatch {
                expected: 100,
                actual: 10
            })
        ));
    }

    #[test]
    fn display_environment_equality() {
        assert_eq!(DisplayEnvironment::X11, DisplayEnvironment::X11);
        assert_ne!(DisplayEnvironment::X11, DisplayEnvironment::Wayland);
        assert_ne!(DisplayEnvironment::Xvfb, DisplayEnvironment::Headless);
    }
}
