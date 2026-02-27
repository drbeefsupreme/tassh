mod cli;
mod clipboard;
mod display;
mod protocol;
mod setup;
mod transport;

use clap::Parser;
use cli::Commands;

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
                if let Err(e) = clipboard::watch_clipboard(tx).await {
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
            let mut sigterm = match tokio::signal::unix::signal(
                tokio::signal::unix::SignalKind::terminate(),
            ) {
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
        Commands::Status => {
            println!("status: not yet implemented");
        }
        Commands::Setup { target } => {
            let result = match target {
                cli::SetupTarget::Local(args) => setup::run_setup_local(&args),
                cli::SetupTarget::Remote(args) => setup::run_setup_remote(&args),
            };
            if let Err(e) = result {
                eprintln!("setup error: {e}");
                std::process::exit(1);
            }
        }
    }
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
