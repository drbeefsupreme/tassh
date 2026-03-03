//! IPC message types for Unix socket communication between `tassh notify` and `tassh daemon`.

use serde::{Deserialize, Serialize};

/// Messages sent to the daemon over the Unix socket.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum IpcMessage {
    /// Sent by LocalCommand when SSH connects to a host.
    Connect {
        /// Remote hostname (from %h token)
        hostname: String,
        /// SSH port (from %p token, typically 22)
        port: u16,
        /// PID of the SSH process (from $PPID in LocalCommand)
        ssh_pid: u32,
    },
    /// Sent by LocalCommand on SSH disconnect (optional, primary detection is via pidfd).
    Disconnect { hostname: String, ssh_pid: u32 },
    /// Sent by `tassh status` CLI invocation.
    StatusRequest,
    /// Sent by hidden `tassh inject` command for deterministic E2E fan-out tests.
    InjectFrame {
        /// PNG bytes to broadcast to all connected peers.
        /// Annotated with serde_bytes so serde_json encodes as base64 (~33% overhead)
        /// instead of a JSON integer array (~300% overhead).
        #[serde(with = "serde_bytes")]
        png_bytes: Vec<u8>,
    },
}

/// Response to a StatusRequest.
#[derive(Debug, Serialize, Deserialize)]
pub struct StatusResponse {
    pub peers: Vec<PeerInfo>,
}

/// Information about a connected peer.
#[derive(Debug, Serialize, Deserialize)]
pub struct PeerInfo {
    pub hostname: String,
    /// True if clipboard TCP connection is active
    pub connected: bool,
    /// True if probe found no daemon on remote
    pub no_daemon: bool,
    pub session_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_message_round_trip() {
        let msg = IpcMessage::Connect {
            hostname: "myhost.example.com".to_string(),
            port: 22,
            ssh_pid: 12345,
        };
        let json = serde_json::to_string(&msg).expect("serialize failed");
        // Verify serde tag produces {"type":"Connect",...}
        assert!(json.contains(r#""type":"Connect""#), "json={json}");
        assert!(
            json.contains(r#""hostname":"myhost.example.com""#),
            "json={json}"
        );
        assert!(json.contains(r#""port":22"#), "json={json}");
        assert!(json.contains(r#""ssh_pid":12345"#), "json={json}");

        let decoded: IpcMessage = serde_json::from_str(&json).expect("deserialize failed");
        match decoded {
            IpcMessage::Connect {
                hostname,
                port,
                ssh_pid,
            } => {
                assert_eq!(hostname, "myhost.example.com");
                assert_eq!(port, 22);
                assert_eq!(ssh_pid, 12345);
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn disconnect_message_round_trip() {
        let msg = IpcMessage::Disconnect {
            hostname: "peer.tailnet".to_string(),
            ssh_pid: 99,
        };
        let json = serde_json::to_string(&msg).expect("serialize failed");
        assert!(json.contains(r#""type":"Disconnect""#), "json={json}");

        let decoded: IpcMessage = serde_json::from_str(&json).expect("deserialize failed");
        match decoded {
            IpcMessage::Disconnect { hostname, ssh_pid } => {
                assert_eq!(hostname, "peer.tailnet");
                assert_eq!(ssh_pid, 99);
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn status_request_round_trip() {
        let msg = IpcMessage::StatusRequest;
        let json = serde_json::to_string(&msg).expect("serialize failed");
        assert!(json.contains(r#""type":"StatusRequest""#), "json={json}");

        let decoded: IpcMessage = serde_json::from_str(&json).expect("deserialize failed");
        assert!(matches!(decoded, IpcMessage::StatusRequest));
    }

    #[test]
    fn status_response_empty_peers() {
        let resp = StatusResponse { peers: vec![] };
        let json = serde_json::to_string(&resp).expect("serialize failed");
        let decoded: StatusResponse = serde_json::from_str(&json).expect("deserialize failed");
        assert!(decoded.peers.is_empty());
    }

    #[test]
    fn inject_frame_round_trip_base64() {
        let png_bytes: Vec<u8> = vec![137, 80, 78, 71, 13, 10, 26, 10, 0, 1, 2, 3];
        let msg = IpcMessage::InjectFrame {
            png_bytes: png_bytes.clone(),
        };
        let json = serde_json::to_string(&msg).expect("serialize failed");
        // serde_bytes with serde_json produces a base64 string, not a JSON integer array
        assert!(json.contains(r#""type":"InjectFrame""#), "json={json}");
        // A base64 string starts with '"', not '['
        assert!(
            !json.contains('['),
            "expected base64 string but got integer array: json={json}"
        );
        let decoded: IpcMessage = serde_json::from_str(&json).expect("deserialize failed");
        match decoded {
            IpcMessage::InjectFrame { png_bytes: decoded_bytes } => {
                assert_eq!(decoded_bytes, png_bytes);
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn status_response_with_peers() {
        let resp = StatusResponse {
            peers: vec![
                PeerInfo {
                    hostname: "alpha.tailnet".to_string(),
                    connected: true,
                    no_daemon: false,
                    session_count: 2,
                },
                PeerInfo {
                    hostname: "beta.tailnet".to_string(),
                    connected: false,
                    no_daemon: true,
                    session_count: 1,
                },
            ],
        };
        let json = serde_json::to_string(&resp).expect("serialize failed");
        let decoded: StatusResponse = serde_json::from_str(&json).expect("deserialize failed");
        assert_eq!(decoded.peers.len(), 2);
        assert_eq!(decoded.peers[0].hostname, "alpha.tailnet");
        assert!(decoded.peers[0].connected);
        assert!(!decoded.peers[0].no_daemon);
        assert_eq!(decoded.peers[0].session_count, 2);
        assert_eq!(decoded.peers[1].hostname, "beta.tailnet");
        assert!(!decoded.peers[1].connected);
        assert!(decoded.peers[1].no_daemon);
        assert_eq!(decoded.peers[1].session_count, 1);
    }
}
