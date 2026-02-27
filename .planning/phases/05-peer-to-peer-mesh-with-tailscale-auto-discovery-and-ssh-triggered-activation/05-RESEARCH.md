# Phase 5: Peer-to-Peer Mesh with SSH-Triggered Activation - Research

**Researched:** 2026-02-27
**Domain:** SSH LocalCommand integration, Unix socket IPC, process lifecycle tracking, multi-peer state management in Tokio/Rust
**Confidence:** MEDIUM-HIGH

---

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**SSH Integration**
- Use SSH `LocalCommand` in `~/.ssh/config` to detect new SSH connections
- LocalCommand notifies local tassh daemon with hostname and SSH process PID
- tassh probes remote host for daemon on connect
- If daemon found, add to active clipboard targets
- Discovery is on-demand (only when SSH connects), not periodic

**Connection Lifecycle**
- Single connection per unique remote host (regardless of SSH session count)
- Watch SSH process PID to detect session end
- Disconnect from host only when ALL SSH sessions to that host are closed
- Auto-reconnect with backoff if connection drops while SSH session still active

**Daemon Architecture**
- Single `tassh daemon` command (replace separate `local`/`remote` subcommands)
- Role auto-detected per connection based on SSH direction
- Machine initiating SSH becomes sender for that connection
- Machine being SSH'd to becomes receiver for that connection
- Bidirectional flow supported: if A SSHs to B AND B SSHs to A, clipboard flows both ways

**Clipboard Behavior**
- All nodes watch their clipboard for changes (even headless with Xvfb)
- When clipboard changes, send to all SSH-connected peers where this node is sender
- Existing clipboard read/write logic (arboard, xclip, wl-copy) unchanged

**User Feedback**
- Silent operation by default (no desktop notifications)
- `tassh status` command shows active connections
- No proactive notifications on connect/disconnect

### Claude's Discretion
- Port selection (fixed vs configurable with sensible default)
- Exact LocalCommand syntax and PID tracking implementation
- Process watching mechanism details
- tassh setup command modifications for ~/.ssh/config

### Deferred Ideas (OUT OF SCOPE)

None — discussion stayed within phase scope
</user_constraints>

---

## Summary

Phase 5 transforms tassh from a manually-configured two-machine relay into an automatic mesh: when a user runs `ssh somehost`, the local tassh daemon is notified, probes the remote for a tassh daemon, and establishes a clipboard sync connection if one is found. The user never manually runs `tassh local --remote somehost` again.

The key technical components are: (1) SSH `LocalCommand` in `~/.ssh/config` to trigger a tiny notify script when any SSH connection opens, (2) a long-running `tassh daemon` listening on a Unix socket for these notifications, (3) TCP probing of the remote host's tassh port to detect presence, (4) `pidfd_open()` (Linux 5.3+, available on all target Ubuntu versions) to watch SSH process PIDs and know when all sessions to a host close, and (5) multi-peer state management replacing the current single-connection transport.

The largest refactor is replacing the separate `local`/`remote` subcommands with a unified `daemon` subcommand that auto-detects role per connection. Existing clipboard and display code is unchanged. The `setup` subcommand gains an additional step: writing a `LocalCommand` stanza into `~/.ssh/config`.

**Primary recommendation:** Build the IPC channel as a Unix socket at `~/.tassh/daemon.sock` (Tokio's `UnixListener`, no new deps). Watch SSH process lifetimes using `pidfd_open` via the `async-pidfd` crate (Linux 5.3+, poll-free). Track per-host peer state in an `Arc<Mutex<HashMap<String, PeerState>>>` shared across tasks.

---

## Standard Stack

### Core (no new deps required for IPC or process watching)

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `tokio::net::UnixListener` | tokio 1.x (already in Cargo.toml) | Unix socket IPC between LocalCommand and daemon | Already present, no new dep |
| `async-pidfd` | 0.1.x (new dep) | Poll-free watching of arbitrary SSH process PIDs | Uses Linux `pidfd_open` (5.3+), epoll-based, works on non-child pids |
| `serde` + `serde_json` | 1.x (new deps) | Serialize IPC messages over Unix socket | Clean message boundary, easy to evolve |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `dashmap` | 6.x | Lock-free concurrent HashMap for peer state | If contention on `Arc<Mutex<HashMap>>` becomes a problem; start with Mutex first |
| polling `/proc/<pid>/` | stdlib | Fallback PID watch if pidfd unavailable | Not needed — Ubuntu 20.04+ ships kernel 5.4+, pidfd supported |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `async-pidfd` | Poll `/proc/<pid>` every 500ms | Polling is wasteful and slow to detect; pidfd is event-driven and instantaneous |
| `async-pidfd` | SIGCHLD handler | SSH process is not a child of tassh daemon — SIGCHLD won't fire |
| Unix socket IPC | Named pipe / FIFO | Sockets support bidirectional streams and framing; pipes are one-way |
| Unix socket IPC | TCP localhost port | Adds port conflict risk; Unix sockets have simpler permissions model |
| `serde_json` | Bare newline-delimited strings | JSON allows structured messages; easy to add fields later |

**Installation (new deps to add to Cargo.toml):**
```bash
cargo add async-pidfd serde --features serde/derive serde_json
```

---

## Architecture Patterns

### Recommended Source File Layout

The current layout:
```
src/
├── cli.rs          # Subcommands: Local, Remote, Status, Setup
├── clipboard.rs    # watch_clipboard, ClipboardWriter, check_clipboard_tools
├── display.rs      # DisplayManager, Xvfb lifecycle
├── lib.rs          # pub mod re-exports
├── main.rs         # Dispatch to subcommands
├── protocol.rs     # Frame wire format
├── setup.rs        # systemd unit file generation
└── transport.rs    # server(), client(), send_frame, recv_frame
```

Phase 5 additions:
```
src/
├── daemon.rs       # NEW: single daemon entry point, peer state, IPC socket loop
├── ipc.rs          # NEW: IpcMessage types (NotifyConnect, NotifyDisconnect, StatusRequest)
├── peer.rs         # NEW: PeerState, per-host connection tracking
└── pid_watcher.rs  # NEW: wrap async-pidfd for watching SSH process PIDs
```

Retire: `Commands::Local` and `Commands::Remote` from `cli.rs` — replace with `Commands::Daemon`.

### Pattern 1: Unix Socket IPC (Daemon Notification Channel)

**What:** LocalCommand sends a JSON message to `~/.tassh/daemon.sock` and disconnects. Daemon reads the message and acts.

**When to use:** Any time SSH opens or the user runs `tassh status`.

**IPC message format:**
```rust
// src/ipc.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum IpcMessage {
    /// Sent by LocalCommand when SSH connects to a host.
    Connect {
        hostname: String,   // %h token from ssh_config
        port: u16,          // %p token from ssh_config (22 typically)
        ssh_pid: u32,       // $PPID from LocalCommand shell script
    },
    /// Sent by LocalCommand on SSH disconnect (if using exit hook — optional).
    Disconnect {
        hostname: String,
        ssh_pid: u32,
    },
    /// Sent by `tassh status` CLI invocation.
    StatusRequest,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StatusResponse {
    pub peers: Vec<PeerInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PeerInfo {
    pub hostname: String,
    pub connected: bool,
    pub session_count: usize,
}
```

**Daemon IPC server loop:**
```rust
// src/daemon.rs (excerpt)
// Source: tokio::net::UnixListener docs (docs.rs/tokio)
use tokio::net::UnixListener;

pub async fn run_ipc_server(
    socket_path: &std::path::Path,
    state: Arc<Mutex<PeerRegistry>>,
) -> anyhow::Result<()> {
    // Remove stale socket from previous run
    let _ = std::fs::remove_file(socket_path);
    let listener = UnixListener::bind(socket_path)?;
    loop {
        let (stream, _) = listener.accept().await?;
        let state = state.clone();
        tokio::spawn(async move {
            handle_ipc_connection(stream, state).await;
        });
    }
}
```

### Pattern 2: SSH LocalCommand Integration

**What:** A shell one-liner in `~/.ssh/config` notifies the daemon on every SSH connection.

**Configuration stanza written by `tassh setup daemon`:**
```
Host *
    PermitLocalCommand yes
    LocalCommand tassh notify --host %h --port %p --ssh-pid $PPID
```

**Key facts about LocalCommand (verified from man7.org):**
- Runs synchronously after connection established — daemon notification must be fast (non-blocking fire-and-forget or short timeout)
- `%h` = remote hostname as resolved, `%n` = original hostname given on CLI, `%p` = remote port
- `$PPID` in the shell is the PID of the SSH process itself (LocalCommand's parent)
- `PermitLocalCommand yes` is required — it is NOT the default

**Important caveat:** LocalCommand runs synchronously. `tassh notify` must connect to the Unix socket, send the message, and exit quickly. Do NOT do the remote probe inside LocalCommand — that belongs in the daemon.

### Pattern 3: PID Watching with pidfd

**What:** After receiving a `Connect` IPC message, the daemon watches the SSH process PID. When the process exits, decrement the session counter for that host. When counter reaches 0, close the clipboard connection.

**Why pidfd (not polling):**
- `waitpid()` only works on child processes — SSH is NOT a child of the daemon
- Polling `/proc/<pid>/` every N ms is racy and wastes CPU
- `pidfd_open()` + epoll provides event-driven, poll-free notification of any process exit
- Kernel requirement: Linux 5.3+ (Ubuntu 20.04 ships 5.4, Ubuntu 22.04 ships 5.15 — both supported)

**Implementation:**
```rust
// src/pid_watcher.rs
// Source: async-pidfd crate docs (docs.rs/async-pidfd)
use async_pidfd::AsyncPidFd;

/// Watch a PID and call `on_exit` when it exits.
/// Spawns a Tokio task that awaits the pidfd.
pub fn watch_pid(
    pid: u32,
    on_exit: impl FnOnce() + Send + 'static,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        match AsyncPidFd::from_pid(pid as libc::pid_t) {
            Ok(pidfd) => {
                // Awaiting the pidfd blocks until the process exits (epoll-based)
                let _ = pidfd.wait().await;
                on_exit();
            }
            Err(e) => {
                // Fallback: poll /proc/<pid> existence every 500ms
                tracing::warn!("pidfd_open failed for pid {pid}: {e}, falling back to polling");
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
                        on_exit();
                        break;
                    }
                }
            }
        }
    })
}
```

### Pattern 4: Per-Host Peer State (PeerRegistry)

**What:** Shared state tracking which hosts are connected and how many SSH sessions exist to each.

```rust
// src/peer.rs
use std::collections::HashMap;

pub struct PeerRegistry {
    /// hostname -> active state
    peers: HashMap<String, PeerState>,
}

pub struct PeerState {
    /// Number of active SSH sessions to this host
    pub session_count: usize,
    /// Whether clipboard TCP connection is established
    pub connected: bool,
    /// tokio task handles for active PID watchers
    pub pid_watcher_handles: Vec<tokio::task::JoinHandle<()>>,
    /// sender channel to the transport client task for this peer
    pub frame_tx: tokio::sync::mpsc::Sender<crate::protocol::Frame>,
}
```

**Session count rule (from CONTEXT.md):**
- On `Connect` IPC message: increment session count; if was 0, probe remote and start connection
- On SSH PID exit (from pidfd watcher): decrement session count; if reaches 0, close connection
- This ensures a single TCP connection per remote host regardless of SSH session count

### Pattern 5: Remote Daemon Probe

**What:** After receiving a `Connect` notification, the daemon attempts a TCP connection to `hostname:port` (default port — at Claude's discretion, recommend 9877 as currently used). If the connection succeeds, the remote has a daemon. If refused immediately, no daemon.

```rust
// In daemon.rs
use tokio::net::TcpStream;
use std::time::Duration;

async fn probe_remote(hostname: &str, port: u16) -> bool {
    // Short timeout — if daemon is running it will accept immediately
    match tokio::time::timeout(
        Duration::from_secs(3),
        TcpStream::connect(format!("{hostname}:{port}"))
    ).await {
        Ok(Ok(_stream)) => true,   // daemon present — stream will be upgraded
        _ => false,                // connection refused or timeout — no daemon
    }
}
```

**Recommended port:** Keep 9877 (already in use, already in systemd units). Configurable via `TASSH_PORT` env or `--port` flag (already present).

### Pattern 6: Clipboard Broadcasting (Multi-Peer)

**What:** When the local clipboard changes, send to all connected peers where this node is the sender (i.e., where this node initiated SSH).

The current `watch_clipboard` produces frames on a single `mpsc::Sender<Frame>`. For multi-peer, use a `tokio::sync::broadcast` channel:

```rust
// In daemon.rs initialization
let (clip_tx, _) = tokio::sync::broadcast::channel::<Arc<protocol::Frame>>(16);

// Each peer connection subscribes:
let mut clip_rx = clip_tx.subscribe();
tokio::spawn(async move {
    while let Ok(frame) = clip_rx.recv().await {
        let _ = send_frame(&mut writer, &frame).await;
    }
});
```

`Arc<Frame>` avoids cloning the PNG payload for each subscriber.

### Anti-Patterns to Avoid

- **Doing the remote probe inside LocalCommand:** LocalCommand is synchronous and blocks the SSH terminal until it returns. The probe can take seconds. Do it in the daemon asynchronously.
- **One TCP connection per SSH session:** CONTEXT.md is explicit — single connection per unique hostname. Track this in `PeerRegistry`.
- **Polling for PID exit:** `waitpid()` won't work (non-child). Polling `/proc` every 500ms introduces up to 500ms lag and wastes CPU. Use `pidfd_open`.
- **Removing stale sockets without error handling:** If `~/.tassh/daemon.sock` exists from a crashed previous run, `UnixListener::bind` will fail. Always `remove_file` before bind (ignoring ENOENT).
- **Multiple daemon instances:** Daemon must check for a running instance at startup (try-connect to socket; if success, another daemon is running — exit). Single-instance enforcement via the socket itself.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Non-child process exit detection | Custom SIGCHLD handler or `/proc` polling loop | `async-pidfd` | waitpid requires parent-child relationship; pidfd works for any process; polling is racy |
| IPC message framing | Custom length-prefix protocol over Unix socket | `serde_json` newline-delimited over `BufReader<UnixStream>` | Newline-delimited JSON is trivial to frame and debug |
| Single-instance daemon enforcement | PID file + lock file | The Unix socket itself (bind fails if in use) | Socket bind is atomic; PID files can be stale |

**Key insight:** The Unix socket already enforces single-instance semantics. If a second `tassh daemon` starts and the socket exists and is live, the bind fails. This is simpler than PID files and doesn't require cleanup on clean shutdown (well — you do need `remove_file` on exit, but bind-failure detection is free).

---

## Common Pitfalls

### Pitfall 1: LocalCommand Blocking SSH Session
**What goes wrong:** If `tassh notify` takes more than a second or two, the user's SSH session appears to hang on connect. This is especially bad if the daemon isn't running.
**Why it happens:** LocalCommand is synchronous. SSH waits for it to finish before giving the user a shell.
**How to avoid:** `tassh notify` should connect to the daemon socket with a very short timeout (100-200ms). If the daemon is not running or doesn't respond, fail silently and exit 0. Never fail the LocalCommand — it should not prevent SSH from working.
**Warning signs:** Users report SSH taking 3+ seconds to connect.

### Pitfall 2: pidfd_open Permission Errors
**What goes wrong:** `pidfd_open` for an SSH process started by the same user should succeed. However, if the SSH PID has already exited by the time the daemon processes the `Connect` message (very fast SSH sessions), `pidfd_open` returns `ESRCH`.
**Why it happens:** Race between IPC delivery and process exit.
**How to avoid:** Handle `ESRCH` gracefully — log a warning and treat as if the session already ended (decrement count immediately). Don't crash.
**Warning signs:** Spurious "connection" entries in `tassh status` that never go away.

### Pitfall 3: Stale Unix Socket
**What goes wrong:** If `tassh daemon` crashes, `~/.tassh/daemon.sock` remains. Next start fails with "address already in use".
**Why it happens:** Unix sockets are filesystem entries; they persist after process death unlike TCP ports.
**How to avoid:** At startup, always `remove_file` the socket path before binding. Optionally try-connect first to detect a genuinely live daemon vs a stale socket.
**Warning signs:** `tassh daemon` exits immediately with bind error.

### Pitfall 4: SSH PID vs. SSH Process Family
**What goes wrong:** On some systems, the SSH session spawns child processes. The PID passed in `$PPID` from LocalCommand is the master SSH process. But that process may have a different lifetime than expected (e.g., ControlMaster multiplexing).
**Why it happens:** With SSH ControlMaster, multiple sessions share one process; the PID watcher would see a single long-lived SSH master process PID.
**How to avoid:** This phase does NOT need to handle ControlMaster specially — the per-host session counter handles it correctly. If three sessions share one ControlMaster PID, all three will send the same PID in their `Connect` notification. The daemon will increment the counter three times for that host but watch the same PID. When the master exits, all sessions end. The session count just needs to be decremented by the watcher exit, not necessarily 1:1 with PIDs.
**Warning signs:** Session count goes to 0 while SSH sessions are still alive.

### Pitfall 5: `tassh notify` Run Without Daemon
**What goes wrong:** If the user SSHes to a host before starting `tassh daemon`, the notify call fails. This must not break SSH.
**Why it happens:** LocalCommand always runs.
**How to avoid:** `tassh notify` exits 0 on any error (socket not found, connection refused, timeout). Log to stderr at DEBUG level only if `TASSH_LOG` env is set.

### Pitfall 6: broadcast Channel Lag
**What goes wrong:** If one peer's TCP connection is slow and the broadcast receiver lags, the broadcast channel's buffer fills and newer messages are dropped for slow receivers.
**Why it happens:** `broadcast::Receiver::recv()` returns `RecvError::Lagged` when the receiver is behind.
**How to avoid:** Use `recv().await` with lagged-handling — log the drop, continue. PNG payloads should use content-hash dedup (already in clipboard watcher) to avoid resending identical images anyway.

---

## Code Examples

### Daemon startup: stale socket cleanup + single instance check
```rust
// Source: tokio::net::UnixListener (docs.rs/tokio)
async fn start_daemon(socket_path: &Path) -> anyhow::Result<()> {
    // Try to connect — if succeeds, another daemon is running
    if UnixStream::connect(socket_path).await.is_ok() {
        anyhow::bail!("tassh daemon is already running");
    }
    // Remove stale socket (ignore error — may not exist)
    let _ = std::fs::remove_file(socket_path);
    let listener = UnixListener::bind(socket_path)?;
    // ... run IPC loop
    Ok(())
}
```

### IPC message send (from `tassh notify`)
```rust
// Source: tokio::net::UnixStream + serde_json
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;

async fn send_notify(socket_path: &Path, msg: &IpcMessage) -> anyhow::Result<()> {
    let mut stream = tokio::time::timeout(
        std::time::Duration::from_millis(200),
        UnixStream::connect(socket_path),
    ).await??;
    let mut json = serde_json::to_vec(msg)?;
    json.push(b'\n');  // newline delimiter
    stream.write_all(&json).await?;
    Ok(())
}
```

### IPC message receive (in daemon)
```rust
// Source: tokio::io::BufReader + serde_json
use tokio::io::{AsyncBufReadExt, BufReader};

async fn handle_ipc_connection(stream: UnixStream, state: Arc<Mutex<PeerRegistry>>) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line).await.unwrap_or(0) == 0 { return; }
    match serde_json::from_str::<IpcMessage>(&line) {
        Ok(msg) => process_ipc_message(msg, state).await,
        Err(e) => tracing::warn!("invalid IPC message: {e}"),
    }
}
```

### ~/.ssh/config stanza (written by `tassh setup daemon`)
```
Host *
    PermitLocalCommand yes
    LocalCommand tassh notify --host %h --port %p --ssh-pid $PPID
```

### PID watcher with pidfd fallback
```rust
// Source: async-pidfd (docs.rs/async-pidfd), /proc polling as fallback
pub async fn wait_for_pid_exit(pid: u32) {
    use async_pidfd::AsyncPidFd;
    if let Ok(pidfd) = AsyncPidFd::from_pid(pid as libc::pid_t) {
        let _ = pidfd.wait().await;
        return;
    }
    // Fallback: /proc polling (500ms interval)
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        if !std::path::Path::new(&format!("/proc/{pid}")).exists() { break; }
    }
}
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Manual `tassh local --remote HOST` | SSH-triggered via LocalCommand | Phase 5 | Zero-configuration after setup |
| Single remote (`server()` is single-client) | Multi-peer broadcast | Phase 5 | Any number of SSH-connected peers |
| Separate `local`/`remote` binaries/commands | Unified `tassh daemon` | Phase 5 | Simpler deployment model |
| No peer discovery | TCP probe on SSH connect | Phase 5 | Automatic: works when both have daemon, gracefully no-ops otherwise |

**Deprecated/outdated in Phase 5:**
- `Commands::Local(LocalArgs)` — replaced by `Commands::Daemon`
- `Commands::Remote(RemoteArgs)` — replaced by `Commands::Daemon`
- `transport::server()` and `transport::client()` — not retired, but called differently; server becomes multi-accept; client is spawned per connected peer
- `tassh-local.service` and `tassh-remote.service` systemd units — replaced by a single `tassh-daemon.service`

---

## Open Questions

1. **Port selection for daemon**
   - What we know: Port 9877 is currently used and baked into systemd units and CLI defaults
   - What's unclear: Should Phase 5 keep 9877 or change? The CONTEXT.md marks this as Claude's discretion.
   - Recommendation: Keep 9877. It is already in use, already in `TASSH_PORT` env, and changing it would break existing Phase 4 installations. No reason to change.

2. **ControlMaster multiplexing behavior**
   - What we know: Multiple SSH connections to same host may share one master process PID when ControlMaster is enabled
   - What's unclear: When LocalCommand fires for each `ssh` invocation under a ControlMaster, does `$PPID` point to the master or to a mux client process?
   - Recommendation: Design session_count to be robust to duplicate PID notifications (deduplicate PIDs per host in PeerRegistry — if the same PID is already watched, don't start a second watcher for it, but still increment the session count OR track PID watch vs. session count separately).

3. **tassh setup daemon vs. existing tassh setup local/remote**
   - What we know: Phase 4 users have `tassh-local.service` and `tassh-remote.service` installed
   - What's unclear: Does `tassh setup daemon` need to remove old services?
   - Recommendation: Have `tassh setup daemon` print a note asking users to disable old services: `systemctl --user disable --now tassh-local.service tassh-remote.service`. Don't automate removal to avoid surprising users.

4. **SSH config collision with existing PermitLocalCommand**
   - What we know: Users may already have `PermitLocalCommand yes` or conflicting `LocalCommand` entries
   - What's unclear: How to handle this safely in `tassh setup daemon`
   - Recommendation: `tassh setup daemon` should append to `~/.ssh/config` under a `# tassh` comment block, and print a warning if existing `PermitLocalCommand` or `LocalCommand` directives are found. Do not modify existing entries.

---

## Sources

### Primary (HIGH confidence)
- `man7.org/linux/man-pages/man5/ssh_config.5.html` - LocalCommand, PermitLocalCommand, available tokens (%h, %p, %n, %r)
- `man7.org/linux/man-pages/man2/pidfd_open.2.html` - pidfd_open syscall, kernel 5.3 requirement, poll/epoll support, non-child process monitoring
- `docs.rs/tokio/latest/tokio/net/struct.UnixListener.html` - UnixListener API, bind, accept, cancel safety

### Secondary (MEDIUM confidence)
- `docs.rs/async-pidfd` - async-pidfd crate API, AsyncPidFd::from_pid, wait() method
- `github.com/orbstack/pidfd-rs` - Real-world Tokio + pidfd usage pattern
- Kernelnewbies.org Linux 5.3 - Confirms pidfd first appeared in 5.3
- Ubuntu kernel lifecycle (ubuntu.com) - Ubuntu 20.04 ships 5.4, Ubuntu 22.04 ships 5.15 — both support pidfd

### Tertiary (LOW confidence)
- Community forum posts about SSH LocalCommand + PPID watching patterns - consistent with man page documentation

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - tokio UnixListener is already in the project; async-pidfd is well-documented; serde is ubiquitous
- Architecture: MEDIUM-HIGH - patterns are well-established but phase refactor scope is significant
- Pitfalls: MEDIUM - most are derived from man page caveats and known Linux process management constraints; ControlMaster behavior is LOW (needs empirical testing)

**Research date:** 2026-02-27
**Valid until:** 2026-03-27 (stable domain — SSH, Unix sockets, pidfd are stable kernel ABIs)
