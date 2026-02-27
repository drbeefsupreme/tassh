use clap::{Parser, Subcommand};

/// cssh — clipboard screenshot relay
#[derive(Debug, Parser)]
#[command(name = "cssh", about = "clipboard screenshot relay")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Run as local daemon (watches clipboard, sends frames)
    Local(LocalArgs),
    /// Run as remote daemon (receives frames, writes clipboard)
    Remote(RemoteArgs),
    /// Show daemon status
    Status,
}

#[derive(Debug, Parser)]
pub struct LocalArgs {
    /// Remote host to connect to.
    /// Accepts a Tailscale hostname or IP (e.g. `100.x.x.x` or `my-machine`).
    /// Port can be appended as `host:port` to override the default.
    #[arg(long, env = "CSSH_REMOTE_HOST")]
    pub remote: String,

    /// Port to connect on
    #[arg(long, env = "CSSH_PORT", default_value = "9877")]
    pub port: u16,
}

#[derive(Debug, Parser)]
pub struct RemoteArgs {
    /// Port to listen on
    #[arg(long, env = "CSSH_PORT", default_value = "9877")]
    pub port: u16,

    /// Address to bind the server to.
    /// If not provided, auto-detects the Tailscale IPv4 address via `tailscale ip -4`.
    #[arg(long, env = "CSSH_BIND")]
    pub bind: Option<String>,
}
