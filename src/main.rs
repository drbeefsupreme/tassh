mod cli;
mod clipboard;
mod display;
mod protocol;
mod transport;

use clap::Parser;
use cli::Commands;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = cli::Cli::parse();

    match cli.command {
        Commands::Local(args) => {
            // Parse `host:port` or bare host from --remote.
            let (remote_host, port) = parse_remote(&args.remote, args.port);

            let (tx, rx) = tokio::sync::mpsc::channel::<protocol::Frame>(16);
            // tx will be used by the clipboard watcher in Phase 3.
            let _tx = tx;

            if let Err(e) = transport::client(&remote_host, port, rx).await {
                eprintln!("transport error: {e}");
                std::process::exit(1);
            }
        }
        Commands::Remote(args) => {
            // Use explicit --bind if provided; otherwise signal auto-detection.
            let bind_addr = args.bind.as_deref().unwrap_or("auto").to_owned();

            if let Err(e) = transport::server(&bind_addr, args.port).await {
                eprintln!("transport error: {e}");
                std::process::exit(1);
            }
        }
        Commands::Status => {
            println!("status: not yet implemented");
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
