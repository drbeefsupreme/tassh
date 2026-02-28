mod cli;
mod clipboard;
mod daemon;
mod display;
mod ipc;
mod peer;
mod pid_watcher;
mod protocol;
mod setup;
mod transport;

use std::time::Duration;

use clap::Parser;
use cli::Commands;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = cli::Cli::parse();

    match cli.command {
        Commands::Local(args) => {
            // Parse `host:port` or bare host from --remote.
            let (remote_host, port) = parse_remote(&args.remote, args.port);

            let (tx, rx) = tokio::sync::mpsc::channel::<protocol::Frame>(16);

            // Spawn clipboard watcher — polls local clipboard, wraps PNG bytes in Frame,
            // sends over mpsc channel to the transport client.
            let watch_handle = tokio::spawn(async move {
                if let Err(e) = clipboard::watch_clipboard(tx, None, None).await {
                    tracing::error!("clipboard watcher error: {e}");
                }
            });

            // Run transport client and handle Ctrl-C for clean shutdown.
            tokio::select! {
                result = transport::client(&remote_host, port, rx) => {
                    if let Err(e) = result {
                        eprintln!("transport error: {e}");
                        std::process::exit(1);
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    tracing::info!("Ctrl-C received, shutting down");
                }
            }

            // Abort the watcher if still running (e.g. after Ctrl-C).
            watch_handle.abort();
        }
        Commands::Remote(args) => {
            // Use explicit --bind if provided; otherwise signal auto-detection.
            let bind_addr = args.bind.as_deref().unwrap_or("auto").to_owned();

            // Always force Xvfb so SSH sessions can read the clipboard via
            // ~/.tassh/display, even on machines with a Wayland/X11 desktop.
            let display_mgr = match display::DisplayManager::detect_and_init(true).await {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("display init error: {e}");
                    std::process::exit(1);
                }
            };

            // Verify required clipboard tools are installed before accepting connections.
            if let Err(e) = clipboard::check_clipboard_tools(&display_mgr.env).await {
                eprintln!("clipboard tool check failed: {e}");
                display_mgr.shutdown().await;
                std::process::exit(1);
            }

            // Set up SIGTERM handler for clean shutdown.
            let mut sigterm =
                match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("failed to install SIGTERM handler: {e}");
                        display_mgr.shutdown().await;
                        std::process::exit(1);
                    }
                };

            // Run server loop; handle SIGTERM and Ctrl-C for clean shutdown.
            tokio::select! {
                result = transport::server(&bind_addr, args.port, display_mgr.env) => {
                    if let Err(e) = result {
                        eprintln!("server error: {e}");
                    }
                }
                _ = sigterm.recv() => {
                    tracing::info!("SIGTERM received, shutting down");
                }
                _ = tokio::signal::ctrl_c() => {
                    tracing::info!("Ctrl-C received, shutting down");
                }
            }

            // Shutdown: kill Xvfb (if any) and remove ~/.tassh/display.
            display_mgr.shutdown().await;
        }
        Commands::Daemon(args) => {
            if let Err(e) = daemon::run_daemon(args.port).await {
                eprintln!("daemon error: {e}");
                std::process::exit(1);
            }
        }
        Commands::Notify(args) => {
            // Fast fire-and-forget IPC to daemon.
            // MUST exit quickly — LocalCommand blocks SSH session.
            if let Err(e) = send_notify(&args).await {
                // Log at debug level only — don't fail the SSH connection.
                tracing::debug!("notify failed: {e}");
            }
            // Always exit 0 — notify failure should not break SSH.
        }
        Commands::Status => {
            if let Err(e) = run_status().await {
                eprintln!("status error: {e}");
                std::process::exit(1);
            }
        }
        Commands::Inject(args) => {
            if let Err(e) = send_inject(&args).await {
                eprintln!("inject error: {e}");
                std::process::exit(1);
            }
        }
        Commands::Setup { target } => {
            let result = match target {
                cli::SetupTarget::Local(args) => setup::run_setup_local(&args),
                cli::SetupTarget::Remote(args) => setup::run_setup_remote(&args),
                cli::SetupTarget::Daemon(args) => setup::run_setup_daemon(&args),
            };
            if let Err(e) = result {
                eprintln!("setup error: {e}");
                std::process::exit(1);
            }
        }
    }
}

/// Send a Connect notification to the daemon via Unix socket.
/// Returns quickly (200ms timeout) — must not block SSH session.
async fn send_notify(args: &cli::NotifyArgs) -> anyhow::Result<()> {
    let socket_path = daemon::socket_path();

    // Short timeout — if daemon isn't running or slow, bail fast.
    let stream = tokio::time::timeout(
        Duration::from_millis(200),
        UnixStream::connect(&socket_path),
    )
    .await??;

    let msg = ipc::IpcMessage::Connect {
        hostname: args.host.clone(),
        port: args.port,
        ssh_pid: args.ssh_pid,
    };

    let mut json = serde_json::to_vec(&msg)?;
    json.push(b'\n');

    // Split into owned halves so we can write.
    let (_, mut writer) = stream.into_split();

    tokio::time::timeout(Duration::from_millis(100), writer.write_all(&json)).await??;

    Ok(())
}

/// Inject a PNG frame into daemon broadcast for deterministic test fan-out.
async fn send_inject(args: &cli::InjectArgs) -> anyhow::Result<()> {
    let socket_path = daemon::socket_path();
    let png_bytes = tokio::fs::read(&args.png_file).await?;

    let stream = UnixStream::connect(&socket_path).await?;
    let msg = ipc::IpcMessage::InjectFrame { png_bytes };
    let mut json = serde_json::to_vec(&msg)?;
    json.push(b'\n');

    let (_, mut writer) = stream.into_split();
    writer.write_all(&json).await?;
    Ok(())
}

/// Query daemon status and print peer connections.
async fn run_status() -> anyhow::Result<()> {
    let socket_path = daemon::socket_path();

    let stream = match UnixStream::connect(&socket_path).await {
        Ok(s) => s,
        Err(_) => {
            println!("daemon not running");
            return Ok(());
        }
    };

    let msg = ipc::IpcMessage::StatusRequest;
    let mut json = serde_json::to_vec(&msg)?;
    json.push(b'\n');

    let (reader, mut writer) = stream.into_split();
    writer.write_all(&json).await?;

    // Read response.
    let mut buf_reader = tokio::io::BufReader::new(reader);
    let mut response_line = String::new();
    tokio::io::AsyncBufReadExt::read_line(&mut buf_reader, &mut response_line).await?;

    let response: ipc::StatusResponse = serde_json::from_str(&response_line)?;

    if response.peers.is_empty() {
        println!("daemon running, no active connections");
    } else {
        println!("Peers:");
        for peer in response.peers {
            // Status reflects actual clipboard sync state
            let status = if peer.connected {
                "syncing" // Clipboard TCP connection is active
            } else if peer.no_daemon {
                "no daemon" // Remote doesn't have tassh running
            } else {
                "probing" // Still checking for remote daemon
            };
            println!(
                "  {} -- {} ({} SSH session{})",
                peer.hostname,
                status,
                peer.session_count,
                if peer.session_count == 1 { "" } else { "s" }
            );
        }
    }

    Ok(())
}

/// Split a `host:port` string into its components.
/// If no colon is present the supplied `default_port` is used.
fn parse_remote(remote: &str, default_port: u16) -> (String, u16) {
    if let Some(colon) = remote.rfind(':') {
        let host = &remote[..colon];
        if let Ok(p) = remote[colon + 1..].parse::<u16>() {
            return (host.to_owned(), p);
        }
    }
    (remote.to_owned(), default_port)
}
