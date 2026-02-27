//! Unified tassh daemon: IPC server, peer management, clipboard broadcast.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpStream, UnixListener, UnixStream};
use tokio::sync::{broadcast, mpsc, Mutex};
use tracing::{debug, info, warn};

use crate::clipboard::{watch_clipboard, ClipboardWriter};
use crate::display::DisplayManager;
use crate::ipc::{IpcMessage, StatusResponse};
use crate::peer::PeerRegistry;
use crate::pid_watcher::watch_pid;
use crate::protocol::Frame;
use crate::transport::{apply_keepalive, recv_frame, send_frame};

/// Default port for tassh daemon TCP connections.
pub const DEFAULT_PORT: u16 = 9877;

/// Path to the daemon Unix socket.
pub fn socket_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_owned());
    PathBuf::from(home).join(".tassh/daemon.sock")
}

/// Run the unified tassh daemon.
///
/// This function:
/// 1. Ensures single-instance via Unix socket
/// 2. Initializes display environment (Xvfb if headless)
/// 3. Starts clipboard watcher
/// 4. Listens for IPC messages (Connect, Disconnect, StatusRequest)
/// 5. Manages peer connections and clipboard broadcast
pub async fn run_daemon(port: u16) -> anyhow::Result<()> {
    let sock_path = socket_path();

    // Ensure ~/.tassh directory exists
    if let Some(parent) = sock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Single-instance check: try to connect to existing socket
    if UnixStream::connect(&sock_path).await.is_ok() {
        anyhow::bail!("tassh daemon is already running (socket exists and is live)");
    }

    // Remove stale socket from previous crash
    let _ = std::fs::remove_file(&sock_path);

    // Initialize display (force_xvfb=true for consistent behavior)
    let display_mgr = DisplayManager::detect_and_init(true).await?;
    info!("display initialized: {:?}", display_mgr.env);

    // Create peer registry and clipboard broadcast channel
    let (registry, clip_tx) = PeerRegistry::new();
    let registry = Arc::new(Mutex::new(registry));

    // Start clipboard watcher - converts local clipboard changes to broadcast
    let clip_tx_clone = clip_tx.clone();
    let (watch_tx, mut watch_rx) = mpsc::channel::<Frame>(16);
    let clipboard_handle = tokio::spawn(async move {
        if let Err(e) = watch_clipboard(watch_tx).await {
            warn!("clipboard watcher error: {e}");
        }
    });

    // Bridge clipboard watcher (mpsc) to broadcast channel
    let broadcast_handle = tokio::spawn(async move {
        while let Some(frame) = watch_rx.recv().await {
            let _ = clip_tx_clone.send(Arc::new(frame));
        }
    });

    // Start TCP listener for incoming peer connections (we also act as receiver)
    let tcp_registry = registry.clone();
    let tcp_display_env = display_mgr.env;
    let tcp_clip_tx = clip_tx.clone();
    let tcp_handle = tokio::spawn(async move {
        if let Err(e) = run_tcp_server(port, tcp_registry, tcp_display_env, tcp_clip_tx).await {
            warn!("TCP server error: {e}");
        }
    });

    // Bind Unix socket for IPC
    let listener = UnixListener::bind(&sock_path)?;
    info!("daemon listening on {}", sock_path.display());

    // Handle graceful shutdown
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;

    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, _)) => {
                        let reg = registry.clone();
                        let clip_tx = clip_tx.clone();
                        tokio::spawn(async move {
                            handle_ipc_connection(stream, reg, clip_tx, port).await;
                        });
                    }
                    Err(e) => warn!("IPC accept error: {e}"),
                }
            }
            _ = sigterm.recv() => {
                info!("SIGTERM received, shutting down");
                break;
            }
            _ = tokio::signal::ctrl_c() => {
                info!("Ctrl-C received, shutting down");
                break;
            }
        }
    }

    // Cleanup
    clipboard_handle.abort();
    broadcast_handle.abort();
    tcp_handle.abort();
    display_mgr.shutdown().await;
    let _ = std::fs::remove_file(&sock_path);

    Ok(())
}

/// Handle a single IPC connection (one message, one response, then close).
async fn handle_ipc_connection(
    stream: UnixStream,
    registry: Arc<Mutex<PeerRegistry>>,
    clip_tx: broadcast::Sender<Arc<Frame>>,
    port: u16,
) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();

    if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
        return;
    }

    match serde_json::from_str::<IpcMessage>(&line) {
        Ok(IpcMessage::Connect { hostname, port: _ssh_port, ssh_pid }) => {
            debug!("IPC: Connect to {hostname} (ssh_pid={ssh_pid})");
            handle_connect(&hostname, ssh_pid, registry, clip_tx, port).await;
        }
        Ok(IpcMessage::Disconnect { hostname, ssh_pid }) => {
            debug!("IPC: Disconnect from {hostname} (ssh_pid={ssh_pid})");
            handle_disconnect(&hostname, ssh_pid, registry).await;
        }
        Ok(IpcMessage::StatusRequest) => {
            let reg = registry.lock().await;
            let response = StatusResponse { peers: reg.list_peers() };
            drop(reg);
            let json = serde_json::to_string(&response).unwrap_or_default();
            let stream = reader.into_inner();
            let _ = write_response(stream, &json).await;
        }
        Err(e) => {
            warn!("invalid IPC message: {e}");
        }
    }
}

/// Write a response back on the Unix socket.
async fn write_response(mut stream: UnixStream, json: &str) -> std::io::Result<()> {
    stream.write_all(json.as_bytes()).await?;
    stream.write_all(b"\n").await?;
    Ok(())
}

/// Handle a Connect IPC message: probe remote, start connection if daemon found.
async fn handle_connect(
    hostname: &str,
    ssh_pid: u32,
    registry: Arc<Mutex<PeerRegistry>>,
    clip_tx: broadcast::Sender<Arc<Frame>>,
    port: u16,
) {
    let mut reg = registry.lock().await;
    let peer = reg.get_or_create(hostname);

    // Check if this PID is already being watched (ControlMaster scenario)
    if peer.watched_pids.contains(&ssh_pid) {
        debug!("pid {ssh_pid} already watched for {hostname}, incrementing session count only");
        peer.session_count += 1;
        return;
    }

    peer.session_count += 1;
    peer.watched_pids.insert(ssh_pid);

    let need_connect = !peer.connected;
    let hostname_owned = hostname.to_owned();
    drop(reg);

    // Start PID watcher for this SSH session
    let reg_clone = registry.clone();
    let hostname_for_watcher = hostname_owned.clone();
    tokio::spawn(async move {
        watch_pid(ssh_pid).await;
        debug!("ssh pid {ssh_pid} exited for {hostname_for_watcher}");
        handle_pid_exit(&hostname_for_watcher, ssh_pid, reg_clone).await;
    });

    // If not already connected, probe and connect
    if need_connect {
        let reg_clone = registry.clone();
        let hostname_for_probe = hostname_owned.clone();
        tokio::spawn(async move {
            if probe_remote(&hostname_for_probe, port).await {
                info!("daemon found on {hostname_for_probe}:{port}, connecting");
                start_peer_connection(&hostname_for_probe, port, reg_clone, clip_tx).await;
            } else {
                debug!("no daemon on {hostname_for_probe}:{port}");
            }
        });
    }
}

/// Handle a Disconnect IPC message (explicit, rare - usually we detect via pidfd).
async fn handle_disconnect(
    hostname: &str,
    ssh_pid: u32,
    registry: Arc<Mutex<PeerRegistry>>,
) {
    handle_pid_exit(hostname, ssh_pid, registry).await;
}

/// Called when an SSH process exits (detected via pidfd).
async fn handle_pid_exit(
    hostname: &str,
    ssh_pid: u32,
    registry: Arc<Mutex<PeerRegistry>>,
) {
    let mut reg = registry.lock().await;
    if let Some(peer) = reg.get_mut(hostname) {
        peer.watched_pids.remove(&ssh_pid);
        peer.session_count = peer.session_count.saturating_sub(1);

        if peer.session_count == 0 {
            info!("all SSH sessions to {hostname} closed, disconnecting");
            // Signal connection task to close
            if let Some(close_tx) = peer.close_tx.take() {
                drop(close_tx); // Dropping the sender signals the receiver
            }
            peer.connected = false;
            // Abort any remaining PID watchers
            for handle in peer.pid_watcher_handles.drain(..) {
                handle.abort();
            }
        }
    }
}

/// Probe a remote host to check if a tassh daemon is running.
async fn probe_remote(hostname: &str, port: u16) -> bool {
    match tokio::time::timeout(
        Duration::from_secs(3),
        TcpStream::connect(format!("{hostname}:{port}")),
    )
    .await
    {
        Ok(Ok(_stream)) => true, // Connection succeeded = daemon present
        _ => false,              // Refused or timeout = no daemon
    }
}

/// Start a peer connection task (we become the sender to this remote).
async fn start_peer_connection(
    hostname: &str,
    port: u16,
    registry: Arc<Mutex<PeerRegistry>>,
    clip_tx: broadcast::Sender<Arc<Frame>>,
) {
    let addr = format!("{hostname}:{port}");

    match TcpStream::connect(&addr).await {
        Ok(stream) => {
            if let Err(e) = apply_keepalive(&stream) {
                warn!("failed to set keepalive for {addr}: {e}");
            }

            let (_reader, mut writer) = stream.into_split();

            // Create close channel
            let (close_tx, mut close_rx) = mpsc::channel::<()>(1);

            // Update registry
            {
                let mut reg = registry.lock().await;
                if let Some(peer) = reg.get_mut(hostname) {
                    peer.connected = true;
                    peer.close_tx = Some(close_tx);
                }
            }

            // Subscribe to clipboard broadcast
            let mut clip_rx = clip_tx.subscribe();
            let hostname_owned = hostname.to_owned();

            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        result = clip_rx.recv() => {
                            match result {
                                Ok(frame) => {
                                    if let Err(e) = send_frame(&mut writer, &frame).await {
                                        warn!("send to {hostname_owned} failed: {e}");
                                        break;
                                    }
                                }
                                Err(broadcast::error::RecvError::Lagged(n)) => {
                                    warn!("clipboard broadcast lagged by {n} frames for {hostname_owned}");
                                }
                                Err(broadcast::error::RecvError::Closed) => {
                                    break;
                                }
                            }
                        }
                        _ = close_rx.recv() => {
                            info!("closing connection to {hostname_owned}");
                            break;
                        }
                    }
                }
            });
        }
        Err(e) => {
            warn!("failed to connect to {addr}: {e}");
        }
    }
}

/// Run the TCP server that accepts incoming connections (we act as receiver).
async fn run_tcp_server(
    port: u16,
    _registry: Arc<Mutex<PeerRegistry>>,
    display_env: crate::protocol::DisplayEnvironment,
    _clip_tx: broadcast::Sender<Arc<Frame>>,
) -> anyhow::Result<()> {
    // Resolve Tailscale IP for binding
    let bind_ip = resolve_tailscale_ip().await?;
    let listener = tokio::net::TcpListener::bind(format!("{bind_ip}:{port}")).await?;
    info!("TCP server listening on {bind_ip}:{port}");

    loop {
        let (stream, peer_addr) = listener.accept().await?;
        info!("accepted connection from {peer_addr}");

        if let Err(e) = apply_keepalive(&stream) {
            warn!("failed to set keepalive: {e}");
        }

        let (mut reader, _writer) = stream.into_split();
        let mut clip_writer = ClipboardWriter::new(display_env);

        tokio::spawn(async move {
            loop {
                match recv_frame(&mut reader).await {
                    Ok(frame) => {
                        let kb = frame.payload.len() / 1024;
                        info!("received frame from {peer_addr}: {kb} KB");
                        if let Err(e) = clip_writer.write(&frame.payload).await {
                            warn!("clipboard write failed: {e}");
                        }
                    }
                    Err(crate::transport::TransportError::ConnectionClosed) => {
                        info!("peer {peer_addr} disconnected");
                        break;
                    }
                    Err(e) => {
                        warn!("connection error from {peer_addr}: {e}");
                        break;
                    }
                }
            }
        });
    }
}

/// Resolve the local Tailscale IPv4 address.
async fn resolve_tailscale_ip() -> anyhow::Result<String> {
    let output = tokio::process::Command::new("tailscale")
        .args(["ip", "-4"])
        .output()
        .await?;
    let ip = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if ip.is_empty() {
        anyhow::bail!("tailscale ip -4 returned empty - is Tailscale running?");
    }
    Ok(ip)
}
