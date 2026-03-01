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

    let stream = match tokio::time::timeout(
        Duration::from_secs(5),
        UnixStream::connect(&socket_path),
    )
    .await
    {
        Ok(Ok(s)) => s,
        Ok(Err(_)) => {
            println!("daemon not running");
            return Ok(());
        }
        Err(_) => {
            return Err(anyhow::anyhow!("timed out connecting to daemon (5s)"));
        }
    };

    let msg = ipc::IpcMessage::StatusRequest;
    let mut json = serde_json::to_vec(&msg)?;
    json.push(b'\n');

    let (reader, mut writer) = stream.into_split();
    writer.write_all(&json).await?;

    // Read response with timeout so we don't hang if the daemon is stuck.
    let mut buf_reader = tokio::io::BufReader::new(reader);
    let mut response_line = String::new();
    tokio::time::timeout(
        Duration::from_secs(5),
        tokio::io::AsyncBufReadExt::read_line(&mut buf_reader, &mut response_line),
    )
    .await
    .map_err(|_| anyhow::anyhow!("timed out waiting for daemon response (5s)"))??;

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
