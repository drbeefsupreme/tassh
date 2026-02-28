//! Per-host peer connection state management.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;

use crate::protocol::Frame;

/// Registry of all connected peers, keyed by hostname.
pub struct PeerRegistry {
    peers: HashMap<String, PeerState>,
    /// Broadcast channel for clipboard frames - all peers subscribe
    clip_tx: broadcast::Sender<Arc<Frame>>,
}

/// State for a single connected peer.
pub struct PeerState {
    /// Number of active SSH sessions to this host
    pub session_count: usize,
    /// Number of active inbound tassh TCP connections from this host
    pub inbound_connections: usize,
    /// Whether clipboard TCP connection is established and active
    pub connected: bool,
    /// True if a connection attempt is in progress (prevents races)
    pub connecting: bool,
    /// True if we probed and no daemon was found on remote
    pub probe_failed: bool,
    /// Set of SSH PIDs currently tracked (for dedup with ControlMaster)
    pub watched_pids: std::collections::HashSet<u32>,
    /// Task handles for active PID watchers (so we can abort on shutdown)
    pub pid_watcher_handles: Vec<JoinHandle<()>>,
    /// Sender to close the peer connection (drop to signal disconnect)
    pub close_tx: Option<mpsc::Sender<()>>,
}

impl PeerRegistry {
    /// Create a new registry with a broadcast channel for clipboard frames.
    pub fn new() -> (Self, broadcast::Sender<Arc<Frame>>) {
        let (clip_tx, _) = broadcast::channel::<Arc<Frame>>(16);
        let registry = Self {
            peers: HashMap::new(),
            clip_tx: clip_tx.clone(),
        };
        (registry, clip_tx)
    }

    /// Get or create peer state for a hostname.
    pub fn get_or_create(&mut self, hostname: &str) -> &mut PeerState {
        self.peers
            .entry(hostname.to_owned())
            .or_insert_with(|| PeerState {
                session_count: 0,
                inbound_connections: 0,
                connected: false,
                connecting: false,
                probe_failed: false,
                watched_pids: std::collections::HashSet::new(),
                pid_watcher_handles: Vec::new(),
                close_tx: None,
            })
    }

    /// Get peer state by hostname (immutable).
    pub fn get(&self, hostname: &str) -> Option<&PeerState> {
        self.peers.get(hostname)
    }

    /// Get peer state by hostname (mutable).
    pub fn get_mut(&mut self, hostname: &str) -> Option<&mut PeerState> {
        self.peers.get_mut(hostname)
    }

    /// Remove peer entry entirely.
    pub fn remove(&mut self, hostname: &str) -> Option<PeerState> {
        self.peers.remove(hostname)
    }

    /// List all peers for status command.
    /// Returns peers with active outbound SSH sessions or inbound tassh sessions.
    pub fn list_peers(&self) -> Vec<crate::ipc::PeerInfo> {
        self.peers
            .iter()
            .filter_map(|(hostname, state)| {
                let total_sessions = state.session_count + state.inbound_connections;
                if total_sessions == 0 {
                    return None;
                }
                Some(crate::ipc::PeerInfo {
                    hostname: hostname.clone(),
                    connected: state.connected || state.inbound_connections > 0,
                    no_daemon: state.probe_failed && state.inbound_connections == 0,
                    session_count: total_sessions,
                })
            })
            .collect()
    }

    /// List peers that still have active SSH sessions.
    pub fn hosts_with_sessions(&self) -> Vec<String> {
        self.peers
            .iter()
            .filter(|(_, state)| state.session_count > 0)
            .map(|(hostname, _)| hostname.clone())
            .collect()
    }

    /// List connected peers with no active SSH sessions.
    pub fn connected_hosts_without_sessions(&self) -> Vec<String> {
        self.peers
            .iter()
            .filter(|(_, state)| state.connected && state.session_count == 0)
            .map(|(hostname, _)| hostname.clone())
            .collect()
    }

    /// List all known peer keys.
    pub fn hostnames(&self) -> Vec<String> {
        self.peers.keys().cloned().collect()
    }

    /// Get a subscriber to the clipboard broadcast channel.
    pub fn subscribe_clipboard(&self) -> broadcast::Receiver<Arc<Frame>> {
        self.clip_tx.subscribe()
    }
}

impl Default for PeerRegistry {
    fn default() -> Self {
        Self::new().0
    }
}
