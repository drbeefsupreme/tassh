//! Unified tassh daemon: IPC server, peer management, clipboard broadcast.

use std::collections::{HashMap, HashSet};
use std::ffi::CStr;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpStream, UnixListener, UnixStream};
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio::time::timeout;
use tracing::{debug, info, warn};

use crate::clipboard::{watch_clipboard, ClipboardWriter};
use crate::display::DisplayManager;
use crate::ipc::{IpcMessage, StatusResponse};
use crate::peer::PeerRegistry;
use crate::pid_watcher::watch_pid;
use crate::protocol::{DisplayEnvironment, Frame};
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
    let original_display = std::env::var("DISPLAY").ok();
    let original_wayland_display = std::env::var("WAYLAND_DISPLAY").ok();

    // Ensure ~/.tassh directory exists
    if let Some(parent) = sock_path.parent() {
        std::fs::create_dir_all(parent)?;
        // Drop stale exported display from previous runs before we initialize a new one.
        let _ = std::fs::remove_file(parent.join("display"));
    }

    // Single-instance check: try to connect to existing socket
    if UnixStream::connect(&sock_path).await.is_ok() {
        anyhow::bail!("tassh daemon is already running (socket exists and is live)");
    }

    // Remove stale socket from previous crash
    let _ = std::fs::remove_file(&sock_path);

    // Always require Xvfb here so SSH sessions can source ~/.tassh/display and
    // paste images reliably into remote CLI tools.
    let display_mgr = DisplayManager::detect_and_init(true)
        .await
        .map_err(|e| anyhow::anyhow!("display init failed (Xvfb required): {e}"))?;
    info!("display initialized: {:?}", display_mgr.env);

    // Choose clipboard watcher env:
    // - Prefer the original desktop env captured at process start.
    // - If absent (e.g. headless/user service without imported env), fall back to
    //   the daemon display so watcher startup does not fail.
    let original_display = original_display.filter(|v| !v.is_empty());
    let original_wayland_display = original_wayland_display.filter(|v| !v.is_empty());
    let (watcher_display, watcher_wayland_display) =
        if original_display.is_some() || original_wayland_display.is_some() {
            (original_display, original_wayland_display)
        } else {
            match display_mgr.env {
                DisplayEnvironment::Wayland => (None, Some(display_mgr.display_str.clone())),
                DisplayEnvironment::X11 | DisplayEnvironment::Xvfb => {
                    (Some(display_mgr.display_str.clone()), None)
                }
                DisplayEnvironment::Headless => (None, None),
            }
        };

    info!(
        "clipboard watcher env: DISPLAY={:?}, WAYLAND_DISPLAY={:?}",
        watcher_display, watcher_wayland_display
    );

    // Create peer registry and clipboard broadcast channel.
    let (registry, clip_tx) = PeerRegistry::new();
    let registry = Arc::new(Mutex::new(registry));

    // Start clipboard watcher - converts local clipboard changes to broadcast.
    let clip_tx_clone = clip_tx.clone();
    let (watch_tx, mut watch_rx) = mpsc::channel::<Frame>(16);
    let clipboard_handle = tokio::spawn(async move {
        if let Err(e) = watch_clipboard(watch_tx, watcher_display, watcher_wayland_display).await {
            warn!("clipboard watcher error: {e}");
        }
    });

    // Bridge clipboard watcher (mpsc) to broadcast channel.
    let broadcast_handle = tokio::spawn(async move {
        while let Some(frame) = watch_rx.recv().await {
            let _ = clip_tx_clone.send(Arc::new(frame));
        }
    });

    // Start TCP listener for incoming peer connections (we also act as receiver)
    let tcp_registry = registry.clone();
    let tcp_display_env = display_mgr.env;
    let tcp_display_str = display_mgr.display_str.clone();
    let tcp_clip_tx = clip_tx.clone();
    let tcp_handle = tokio::spawn(async move {
        if let Err(e) = run_tcp_server(
            port,
            tcp_registry,
            tcp_display_env,
            tcp_display_str,
            tcp_clip_tx,
        )
        .await
        {
            warn!("TCP server error: {e}");
        }
    });

    // Bind Unix socket for IPC
    let listener = UnixListener::bind(&sock_path)?;
    info!("daemon listening on {}", sock_path.display());

    // Discover existing SSH sessions and probe their remotes
    let discovery_registry = registry.clone();
    let discovery_clip_tx = clip_tx.clone();
    tokio::spawn(async move {
        discover_existing_ssh_sessions(discovery_registry, discovery_clip_tx, port).await;
    });

    // Periodically retry peers that still have active SSH sessions but no daemon connection.
    let reconcile_registry = registry.clone();
    let reconcile_clip_tx = clip_tx.clone();
    let reconcile_handle = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(2));
        loop {
            ticker.tick().await;
            refresh_peer_liveness(
                reconcile_registry.clone(),
                reconcile_clip_tx.clone(),
                port,
                false,
            )
            .await;
        }
    });

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

    // Cleanup - abort tasks and shutdown with timeout
    clipboard_handle.abort();
    broadcast_handle.abort();
    tcp_handle.abort();
    reconcile_handle.abort();

    // Timeout display shutdown to prevent hang on SIGTERM
    if tokio::time::timeout(Duration::from_secs(2), display_mgr.shutdown())
        .await
        .is_err()
    {
        warn!("display shutdown timed out, forcing exit");
    }
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
        Ok(IpcMessage::Connect {
            hostname,
            port: _ssh_port,
            ssh_pid,
        }) => {
            debug!("IPC: Connect to {hostname} (ssh_pid={ssh_pid})");
            handle_connect(&hostname, ssh_pid, registry, clip_tx, port).await;
        }
        Ok(IpcMessage::Disconnect { hostname, ssh_pid }) => {
            debug!("IPC: Disconnect from {hostname} (ssh_pid={ssh_pid})");
            handle_disconnect(&hostname, ssh_pid, registry).await;
        }
        Ok(IpcMessage::StatusRequest) => {
            // Refresh liveness so status does not stay "syncing" after a remote daemon exits.
            refresh_peer_liveness(registry.clone(), clip_tx.clone(), port, true).await;
            let reg = registry.lock().await;
            let response = StatusResponse {
                peers: reg.list_peers(),
            };
            drop(reg);
            let json = serde_json::to_string(&response).unwrap_or_default();
            let stream = reader.into_inner();
            let _ = write_response(stream, &json).await;
        }
        Ok(IpcMessage::InjectFrame { png_bytes }) => {
            let kb = png_bytes.len() / 1024;
            debug!("IPC: InjectFrame ({kb} KB)");
            let _ = clip_tx.send(Arc::new(Frame::new_png(png_bytes)));
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

/// Refresh peer liveness for active SSH sessions.
async fn refresh_peer_liveness(
    registry: Arc<Mutex<PeerRegistry>>,
    clip_tx: broadcast::Sender<Arc<Frame>>,
    port: u16,
    check_connected: bool,
) {
    let idle_connected_hosts = {
        let reg = registry.lock().await;
        reg.connected_hosts_without_sessions()
    };

    for hostname in idle_connected_hosts {
        info!("cleaning up idle connected peer {hostname} with zero SSH sessions");
        let mut reg = registry.lock().await;
        if let Some(peer) = reg.get_mut(&hostname) {
            if let Some(close_tx) = peer.close_tx.take() {
                drop(close_tx);
            }
            peer.connected = false;
            peer.connecting = false;
        }
    }

    let hosts = {
        let reg = registry.lock().await;
        reg.hosts_with_sessions()
    };

    let mut probe_jobs = tokio::task::JoinSet::new();
    for hostname in hosts {
        let (connected, connecting) = {
            let reg = registry.lock().await;
            match reg.get(&hostname) {
                Some(peer) => (peer.connected, peer.connecting),
                None => continue,
            }
        };

        if connecting {
            continue;
        }

        if connected && !check_connected {
            continue;
        }

        probe_jobs.spawn(async move {
            let reachable =
                probe_remote_with_timeout(&hostname, port, Duration::from_millis(400)).await;
            (hostname, connected, reachable)
        });
    }

    while let Some(job) = probe_jobs.join_next().await {
        let (hostname, connected, reachable) = match job {
            Ok(values) => values,
            Err(e) => {
                warn!("liveness probe task failed: {e}");
                continue;
            }
        };

        if reachable {
            if connected {
                continue;
            }

            let should_connect = {
                let mut reg = registry.lock().await;
                if let Some(peer) = reg.get_mut(&hostname) {
                    if peer.connected || peer.connecting || peer.session_count == 0 {
                        false
                    } else {
                        peer.connecting = true;
                        peer.probe_failed = false;
                        true
                    }
                } else {
                    false
                }
            };

            if should_connect {
                let reg_clone = registry.clone();
                let tx_clone = clip_tx.clone();
                let hostname_for_connect = hostname.clone();
                tokio::spawn(async move {
                    info!("daemon found on {hostname_for_connect}:{port}, connecting");
                    start_peer_connection(&hostname_for_connect, port, reg_clone, tx_clone).await;
                });
            }
            continue;
        }

        let mut reg = registry.lock().await;
        if let Some(peer) = reg.get_mut(&hostname) {
            if connected {
                info!("status: probe failed for {hostname}, marking disconnected");
                // Drop sender so connection task exits promptly if still running.
                if let Some(close_tx) = peer.close_tx.take() {
                    drop(close_tx);
                }
                peer.connected = false;
            }
            peer.connecting = false;
            peer.probe_failed = true;
        }
    }
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
        debug!("pid {ssh_pid} already watched for {hostname}, ignoring duplicate notify");
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

    // If not already connected or connecting, probe and connect
    if need_connect {
        // Check and set connecting flag atomically
        let should_connect = {
            let mut reg = registry.lock().await;
            let peer = reg.get_or_create(&hostname_owned);
            if peer.connecting || peer.connected {
                false
            } else {
                peer.connecting = true;
                true
            }
        };

        if should_connect {
            let reg_clone = registry.clone();
            let hostname_for_probe = hostname_owned.clone();
            tokio::spawn(async move {
                if probe_remote(&hostname_for_probe, port).await {
                    info!("daemon found on {hostname_for_probe}:{port}, connecting");
                    start_peer_connection(&hostname_for_probe, port, reg_clone, clip_tx).await;
                } else {
                    debug!("no daemon on {hostname_for_probe}:{port}");
                    // Mark as probed but no daemon found
                    let mut reg = reg_clone.lock().await;
                    if let Some(peer) = reg.get_mut(&hostname_for_probe) {
                        peer.probe_failed = true;
                        peer.connecting = false;
                    }
                }
            });
        }
    }
}

/// Handle a Disconnect IPC message (explicit, rare - usually we detect via pidfd).
async fn handle_disconnect(hostname: &str, ssh_pid: u32, registry: Arc<Mutex<PeerRegistry>>) {
    handle_pid_exit(hostname, ssh_pid, registry).await;
}

/// Called when an SSH process exits (detected via pidfd).
async fn handle_pid_exit(hostname: &str, ssh_pid: u32, registry: Arc<Mutex<PeerRegistry>>) {
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
    probe_remote_with_timeout(hostname, port, Duration::from_secs(3)).await
}

/// Probe a remote host with a caller-provided timeout.
async fn probe_remote_with_timeout(hostname: &str, port: u16, timeout: Duration) -> bool {
    match tokio::time::timeout(timeout, TcpStream::connect(format!("{hostname}:{port}"))).await {
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

            let (mut reader, mut writer) = stream.into_split();

            // Create close channel
            let (close_tx, mut close_rx) = mpsc::channel::<()>(1);

            // Update registry - only keep this connection if sessions still exist.
            let keep_connection = {
                let mut reg = registry.lock().await;
                if let Some(peer) = reg.get_mut(hostname) {
                    if peer.session_count == 0 {
                        peer.connecting = false;
                        peer.close_tx = None;
                        false
                    } else {
                        peer.connected = true;
                        peer.connecting = false;
                        peer.probe_failed = false;
                        peer.close_tx = Some(close_tx);
                        true
                    }
                } else {
                    false
                }
            };

            if !keep_connection {
                debug!("dropping connection to {hostname} because no active SSH sessions remain");
                return;
            }

            // Subscribe to clipboard broadcast
            let mut clip_rx = clip_tx.subscribe();
            let hostname_owned = hostname.to_owned();
            let registry_for_cleanup = registry.clone();

            // Buffer for detecting remote close (we don't expect data, just EOF)
            let mut read_buf = [0u8; 1];

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
                        // Monitor for remote disconnect - read returns 0 (EOF) or error
                        read_result = tokio::io::AsyncReadExt::read(&mut reader, &mut read_buf) => {
                            match read_result {
                                Ok(0) => {
                                    info!("remote {hostname_owned} closed connection (EOF)");
                                    break;
                                }
                                Err(e) => {
                                    info!("remote {hostname_owned} connection error: {e}");
                                    break;
                                }
                                Ok(_) => {
                                    // Unexpected data from remote, ignore
                                    debug!("unexpected data from {hostname_owned}");
                                }
                            }
                        }
                    }
                }
                // Connection ended - update registry to reflect disconnected state
                let mut reg = registry_for_cleanup.lock().await;
                if let Some(peer) = reg.get_mut(&hostname_owned) {
                    peer.connected = false;
                    peer.connecting = false;
                    peer.close_tx = None;
                    info!("connection to {hostname_owned} closed, marked disconnected");
                }
            });
        }
        Err(e) => {
            warn!("failed to connect to {addr}: {e}");
            // Clear connecting flag on failure
            let mut reg = registry.lock().await;
            if let Some(peer) = reg.get_mut(hostname) {
                peer.connecting = false;
            }
        }
    }
}

/// Run the TCP server that accepts incoming connections (we act as receiver).
async fn run_tcp_server(
    port: u16,
    registry: Arc<Mutex<PeerRegistry>>,
    display_env: crate::protocol::DisplayEnvironment,
    display_str: String,
    _clip_tx: broadcast::Sender<Arc<Frame>>,
) -> anyhow::Result<()> {
    // Resolve Tailscale IP for binding
    let bind_ip = resolve_tailscale_ip().await?;
    let listener = tokio::net::TcpListener::bind(format!("{bind_ip}:{port}")).await?;
    info!("TCP server listening on {bind_ip}:{port}");

    loop {
        let (stream, peer_addr) = listener.accept().await?;
        let peer_ip = peer_addr.ip();
        let peer_host = resolve_inbound_peer_key(peer_ip, registry.clone()).await;
        info!("accepted connection from {peer_addr} (peer={peer_host})");

        {
            let mut reg = registry.lock().await;
            let peer = reg.get_or_create(&peer_host);
            peer.inbound_connections += 1;
            peer.probe_failed = false;
        }

        if let Err(e) = apply_keepalive(&stream) {
            warn!("failed to set keepalive: {e}");
        }

        let (mut reader, writer) = stream.into_split();
        let mut clip_writer = ClipboardWriter::new(display_env, Some(display_str.clone()));
        let registry_for_cleanup = registry.clone();
        let peer_host_for_cleanup = peer_host.clone();

        tokio::spawn(async move {
            // Keep write half alive so connected peers do not see immediate EOF.
            let _writer_guard = writer;
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

            let mut reg = registry_for_cleanup.lock().await;
            if let Some(peer) = reg.get_mut(&peer_host_for_cleanup) {
                peer.inbound_connections = peer.inbound_connections.saturating_sub(1);
            }
        });
    }
}

/// Resolve an inbound peer registry key from socket IP, preferring existing host keys.
async fn resolve_inbound_peer_key(peer_ip: IpAddr, registry: Arc<Mutex<PeerRegistry>>) -> String {
    if let Some(existing) = existing_peer_key_for_ip(registry.clone(), peer_ip).await {
        return existing;
    }

    if let Some(reverse_host) = reverse_dns_lookup(peer_ip).await {
        let normalized = normalize_peer_hostname(&reverse_host);

        // Reuse an existing key if reverse DNS gave a different variant (short/FQDN).
        let known_hosts = {
            let reg = registry.lock().await;
            reg.hostnames()
        };
        let normalized_short = normalized.split('.').next().unwrap_or(normalized.as_str());
        for known in known_hosts {
            let known_norm = normalize_peer_hostname(&known);
            let known_short = known_norm.split('.').next().unwrap_or(known_norm.as_str());
            if known_norm == normalized
                || known_short == normalized
                || known_norm == normalized_short
                || known_short == normalized_short
            {
                return known;
            }
        }

        return normalized;
    }

    peer_ip.to_string()
}

async fn existing_peer_key_for_ip(
    registry: Arc<Mutex<PeerRegistry>>,
    peer_ip: IpAddr,
) -> Option<String> {
    let known_hosts = {
        let reg = registry.lock().await;
        reg.hostnames()
    };

    for host in known_hosts {
        if host_resolves_to_ip(&host, peer_ip).await {
            return Some(host);
        }
    }
    None
}

async fn host_resolves_to_ip(hostname: &str, peer_ip: IpAddr) -> bool {
    if hostname.parse::<IpAddr>().ok() == Some(peer_ip) {
        return true;
    }

    match tokio::net::lookup_host((hostname, 0)).await {
        Ok(addrs) => addrs.into_iter().any(|addr| addr.ip() == peer_ip),
        Err(_) => false,
    }
}

fn normalize_peer_hostname(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('.');
    normalize_ssh_host(trimmed).unwrap_or_else(|| trimmed.to_ascii_lowercase())
}

async fn reverse_dns_lookup(peer_ip: IpAddr) -> Option<String> {
    tokio::task::spawn_blocking(move || reverse_dns_lookup_blocking(peer_ip))
        .await
        .ok()
        .flatten()
}

fn reverse_dns_lookup_blocking(peer_ip: IpAddr) -> Option<String> {
    let mut host_buf = [0_i8; libc::NI_MAXHOST as usize];

    let rc = match peer_ip {
        IpAddr::V4(addr) => {
            let sockaddr = libc::sockaddr_in {
                sin_family: libc::AF_INET as u16,
                sin_port: 0,
                sin_addr: libc::in_addr {
                    s_addr: u32::from_ne_bytes(addr.octets()),
                },
                sin_zero: [0; 8],
            };

            // SAFETY: sockaddr points to a valid initialized sockaddr_in for this call.
            unsafe {
                libc::getnameinfo(
                    (&sockaddr as *const libc::sockaddr_in).cast::<libc::sockaddr>(),
                    std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
                    host_buf.as_mut_ptr(),
                    host_buf.len() as libc::socklen_t,
                    std::ptr::null_mut(),
                    0,
                    libc::NI_NAMEREQD,
                )
            }
        }
        IpAddr::V6(addr) => {
            let sockaddr = libc::sockaddr_in6 {
                sin6_family: libc::AF_INET6 as u16,
                sin6_port: 0,
                sin6_flowinfo: 0,
                sin6_addr: libc::in6_addr {
                    s6_addr: addr.octets(),
                },
                sin6_scope_id: 0,
            };

            // SAFETY: sockaddr points to a valid initialized sockaddr_in6 for this call.
            unsafe {
                libc::getnameinfo(
                    (&sockaddr as *const libc::sockaddr_in6).cast::<libc::sockaddr>(),
                    std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t,
                    host_buf.as_mut_ptr(),
                    host_buf.len() as libc::socklen_t,
                    std::ptr::null_mut(),
                    0,
                    libc::NI_NAMEREQD,
                )
            }
        }
    };

    if rc != 0 {
        return None;
    }

    // SAFETY: getnameinfo wrote a NUL-terminated hostname into host_buf on success.
    let host = unsafe { CStr::from_ptr(host_buf.as_ptr()) }
        .to_string_lossy()
        .trim()
        .to_owned();
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

/// Resolve the local Tailscale IPv4 address.
async fn resolve_tailscale_ip() -> anyhow::Result<String> {
    let output = timeout(
        Duration::from_secs(5),
        tokio::process::Command::new("tailscale")
            .args(["ip", "-4"])
            .output(),
    )
    .await
    .map_err(|_| {
        anyhow::anyhow!("tailscale ip -4 timed out after 5 s - is Tailscale running?")
    })??;
    let ip = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if ip.is_empty() {
        anyhow::bail!("tailscale ip -4 returned empty - is Tailscale running?");
    }
    Ok(ip)
}

/// Discover existing SSH sessions on daemon startup.
/// Scans for running ssh processes and probes their remote hosts.
async fn discover_existing_ssh_sessions(
    registry: Arc<Mutex<PeerRegistry>>,
    clip_tx: broadcast::Sender<Arc<Frame>>,
    port: u16,
) {
    let self_aliases = discover_local_aliases().await;

    // Get list of ssh processes with their command lines
    let output = match tokio::process::Command::new("pgrep")
        .args(["-a", "ssh"])
        .output()
        .await
    {
        Ok(o) => o,
        Err(e) => {
            debug!("pgrep failed: {e}");
            return;
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut hosts_to_probe: HashMap<String, HashSet<u32>> = HashMap::new();

    for line in stdout.lines() {
        // Expected format: "PID ssh [options] destination [command ...]"
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }

        let ssh_pid = match parts[0].parse::<u32>() {
            Ok(pid) => pid,
            Err(_) => continue,
        };

        if !is_ssh_client_command(parts[1]) {
            continue;
        }

        if let Some(hostname) = extract_ssh_destination_host(&parts[2..]) {
            if self_aliases.contains(&hostname) {
                debug!("startup: skipping self destination {hostname}");
                continue;
            }
            hosts_to_probe.entry(hostname).or_default().insert(ssh_pid);
        }
    }

    if hosts_to_probe.is_empty() {
        debug!("no existing SSH sessions found");
        return;
    }

    let total_sessions: usize = hosts_to_probe.values().map(|pids| pids.len()).sum();
    info!(
        "found {total_sessions} existing SSH sessions across {} hosts, probing for daemons",
        hosts_to_probe.len()
    );

    // Probe each host and track discovered ssh pids for accurate session counts.
    for (hostname, pids) in hosts_to_probe {
        let reg = registry.clone();
        let tx = clip_tx.clone();
        let pids: Vec<u32> = pids.into_iter().collect();
        tokio::spawn(async move {
            let mut new_pids = Vec::new();

            // Check and set connecting flag atomically
            let should_connect = {
                let mut r = reg.lock().await;
                let peer = r.get_or_create(&hostname);

                for pid in &pids {
                    if peer.watched_pids.insert(*pid) {
                        peer.session_count += 1;
                        new_pids.push(*pid);
                    }
                }

                if peer.connecting || peer.connected {
                    false
                } else {
                    peer.connecting = true;
                    true
                }
            };

            for ssh_pid in new_pids {
                let reg_clone = reg.clone();
                let hostname_for_watcher = hostname.clone();
                tokio::spawn(async move {
                    watch_pid(ssh_pid).await;
                    debug!("startup ssh pid {ssh_pid} exited for {hostname_for_watcher}");
                    handle_pid_exit(&hostname_for_watcher, ssh_pid, reg_clone).await;
                });
            }

            if !should_connect {
                debug!("startup: {hostname} already connecting/connected, skipping");
                return;
            }

            if probe_remote(&hostname, port).await {
                info!("startup: daemon found on {hostname}:{port}, connecting");
                start_peer_connection(&hostname, port, reg, tx).await;
            } else {
                debug!("startup: no daemon on {hostname}:{port}");
                let mut r = reg.lock().await;
                if let Some(peer) = r.get_mut(&hostname) {
                    peer.probe_failed = true;
                    peer.connecting = false;
                }
            }
        });
    }
}

fn is_ssh_client_command(cmd: &str) -> bool {
    let basename = cmd.rsplit('/').next().unwrap_or(cmd);
    basename == "ssh"
}

fn ssh_option_takes_value(arg: &str) -> bool {
    matches!(
        arg,
        "-b" | "-c"
            | "-D"
            | "-E"
            | "-e"
            | "-F"
            | "-I"
            | "-i"
            | "-J"
            | "-L"
            | "-l"
            | "-m"
            | "-O"
            | "-o"
            | "-p"
            | "-Q"
            | "-R"
            | "-S"
            | "-W"
            | "-w"
    )
}

fn extract_ssh_destination_host(args: &[&str]) -> Option<String> {
    let mut idx = 0;
    while idx < args.len() {
        let arg = args[idx];
        if arg == "--" {
            idx += 1;
            break;
        }

        if arg.starts_with('-') {
            // Short options like -p/-o may carry an inline value (-p22, -oProxyCommand=...),
            // in which case they don't consume the following token.
            let short = &arg[..arg.len().min(2)];
            let consumes_next =
                ssh_option_takes_value(arg) || (arg.len() == 2 && ssh_option_takes_value(short));
            idx += if consumes_next { 2 } else { 1 };
            continue;
        }

        return normalize_ssh_host(arg);
    }

    while idx < args.len() {
        let arg = args[idx];
        if !arg.starts_with('-') {
            return normalize_ssh_host(arg);
        }
        idx += 1;
    }

    None
}

fn normalize_ssh_host(raw: &str) -> Option<String> {
    let unquoted = raw.trim_matches(|c| c == '\'' || c == '"');
    let without_user = unquoted.rsplit('@').next().unwrap_or(unquoted);
    let without_brackets = without_user
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(without_user);
    let host = without_brackets
        .split(':')
        .next()
        .unwrap_or(without_brackets)
        .trim()
        .to_ascii_lowercase();

    if host.is_empty() || host.contains('/') {
        return None;
    }

    if host
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        Some(host)
    } else {
        None
    }
}

async fn discover_local_aliases() -> HashSet<String> {
    let mut aliases = HashSet::new();
    aliases.insert("localhost".to_owned());
    aliases.insert("127.0.0.1".to_owned());

    if let Ok(ip) = resolve_tailscale_ip().await {
        aliases.insert(ip.to_ascii_lowercase());
    }

    if let Ok(output) = tokio::process::Command::new("hostname").output().await {
        let hostname = String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_ascii_lowercase();
        if !hostname.is_empty() {
            aliases.insert(hostname.clone());
            if let Some(short) = hostname.split('.').next() {
                aliases.insert(short.to_owned());
            }
        }
    }

    aliases
}
