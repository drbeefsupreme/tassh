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
    /// Remote host to connect to
    #[arg(long, env = "CSSH_REMOTE_HOST")]
    pub remote_host: String,

    /// Port to connect on
    #[arg(long, env = "CSSH_PORT", default_value = "34782")]
    pub port: u16,
}

#[derive(Debug, Parser)]
pub struct RemoteArgs {
    /// Port to listen on
    #[arg(long, env = "CSSH_PORT", default_value = "34782")]
    pub port: u16,
}
