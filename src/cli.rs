use clap::{Parser, Subcommand};

/// tassh — clipboard screenshot relay
#[derive(Debug, Parser)]
#[command(name = "tassh", about = "clipboard screenshot relay")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Run unified clipboard relay daemon (auto-connects on SSH)
    Daemon(DaemonArgs),
    /// Notify daemon of SSH connection (called by LocalCommand, fast fire-and-forget)
    Notify(NotifyArgs),
    /// Show daemon status and active peer connections
    Status,
    /// Inject a PNG frame into daemon broadcast (hidden; used by E2E harness)
    #[command(hide = true)]
    Inject(InjectArgs),
    /// Install and configure tassh as a systemd user service
    Setup {
        #[command(subcommand)]
        target: SetupTarget,
    },
}

#[derive(Debug, Parser)]
pub struct DaemonArgs {
    /// Port for TCP connections (both listening and outbound)
    #[arg(long, env = "TASSH_PORT", default_value = "9877")]
    pub port: u16,
}

#[derive(Debug, Parser)]
pub struct NotifyArgs {
    /// Remote hostname (from SSH %h token)
    #[arg(long)]
    pub host: String,

    /// SSH port (from SSH %p token)
    #[arg(long, default_value = "22")]
    pub port: u16,

    /// PID of the SSH process (from $PPID in LocalCommand)
    #[arg(long)]
    pub ssh_pid: u32,
}

#[derive(Debug, Parser)]
pub struct InjectArgs {
    /// PNG file to broadcast to all connected peers.
    #[arg(long)]
    pub png_file: String,
}

#[derive(Debug, Subcommand)]
pub enum SetupTarget {
    /// Set up tassh-daemon.service and SSH config (recommended)
    Daemon(SetupDaemonArgs),
}

#[derive(Debug, Parser)]
pub struct SetupDaemonArgs {
    /// Port for daemon TCP connections
    #[arg(long, default_value = "9877")]
    pub port: u16,
}
