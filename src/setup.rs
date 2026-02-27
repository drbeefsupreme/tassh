//! tassh setup — generates systemd user service unit files and orchestrates systemctl.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context};

use crate::cli::{SetupLocalArgs, SetupRemoteArgs};

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

fn home_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/root".to_owned()))
}

fn binary_path() -> PathBuf {
    home_dir().join(".cargo/bin/tassh")
}

fn unit_dir() -> PathBuf {
    home_dir().join(".config/systemd/user")
}

// ---------------------------------------------------------------------------
// Unit file generators
// ---------------------------------------------------------------------------

fn tassh_local_unit(remote: &str, port: u16) -> String {
    let bin = binary_path();
    // If remote already contains a colon it may be host:port — split and use those values.
    let exec_start = if let Some(colon) = remote.rfind(':') {
        let host = &remote[..colon];
        let embedded_port = remote[colon + 1..].parse::<u16>().unwrap_or(port);
        format!(
            "{} local --remote {} --port {}",
            bin.display(),
            host,
            embedded_port
        )
    } else {
        format!(
            "{} local --remote {} --port {}",
            bin.display(),
            remote,
            port
        )
    };

    format!(
        "[Unit]\n\
         Description=tassh local clipboard relay\n\
         After=network.target\n\
         \n\
         [Service]\n\
         ExecStart={exec_start}\n\
         Restart=always\n\
         RestartSec=5\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n"
    )
}

fn tassh_remote_unit(bind: &str, port: u16) -> String {
    let bin = binary_path();
    let exec_start = format!("{} remote --bind {} --port {}", bin.display(), bind, port);

    format!(
        "[Unit]\n\
         Description=tassh remote clipboard relay\n\
         After=network.target\n\
         \n\
         [Service]\n\
         ExecStart={exec_start}\n\
         Restart=always\n\
         RestartSec=5\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n"
    )
}

// ---------------------------------------------------------------------------
// Shell snippet
// ---------------------------------------------------------------------------

fn shell_snippet() -> &'static str {
    "# tassh: auto-export DISPLAY in SSH sessions\n\
     if [ -n \"$SSH_CONNECTION\" ] && [ -f \"$HOME/.tassh/display\" ]; then\n\
         . \"$HOME/.tassh/display\"\n\
     fi"
}

// ---------------------------------------------------------------------------
// systemctl helper
// ---------------------------------------------------------------------------

fn run_systemctl(args: &[&str]) -> anyhow::Result<()> {
    let status = Command::new("systemctl")
        .args(args)
        .status()
        .with_context(|| format!("failed to run systemctl {}", args.join(" ")))?;

    if !status.success() {
        bail!(
            "systemctl {} exited with status {}",
            args.join(" "),
            status
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Core setup orchestration
// ---------------------------------------------------------------------------

fn run_setup(service_name: &str, unit_content: &str) -> anyhow::Result<()> {
    let dir = unit_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create {}", dir.display()))?;

    let unit_path = dir.join(service_name);
    std::fs::write(&unit_path, unit_content)
        .with_context(|| format!("failed to write {}", unit_path.display()))?;
    println!("Wrote {}", unit_path.display());

    run_systemctl(&["--user", "daemon-reload"])?;
    run_systemctl(&["--user", "enable", service_name])?;
    run_systemctl(&["--user", "start", service_name])?;

    // loginctl enable-linger: failure is a warning, not a fatal error.
    let linger_status = Command::new("loginctl")
        .arg("enable-linger")
        .status();
    match linger_status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            eprintln!(
                "warning: loginctl enable-linger exited with status {} — \
                 linger may need to be enabled manually",
                s
            );
        }
        Err(e) => {
            eprintln!(
                "warning: loginctl enable-linger failed: {e} — \
                 linger may need to be enabled manually"
            );
        }
    }

    println!();
    println!("Add the following snippet to your shell profile (~/.bashrc or ~/.zshrc):");
    println!();
    println!("{}", shell_snippet());
    println!();
    println!(
        "To follow logs: journalctl --user -u {} -f",
        service_name
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Install tassh-local.service (clipboard watcher → sends frames to remote).
pub fn run_setup_local(args: &SetupLocalArgs) -> anyhow::Result<()> {
    let bin = binary_path();
    if !bin.exists() {
        bail!(
            "binary not found at {}. Run `cargo install --path .` first.",
            bin.display()
        );
    }
    run_setup(
        "tassh-local.service",
        &tassh_local_unit(&args.remote, args.port),
    )
}

/// Install tassh-remote.service (receives frames → writes clipboard).
pub fn run_setup_remote(args: &SetupRemoteArgs) -> anyhow::Result<()> {
    let bin = binary_path();
    if !bin.exists() {
        bail!(
            "binary not found at {}. Run `cargo install --path .` first.",
            bin.display()
        );
    }
    run_setup(
        "tassh-remote.service",
        &tassh_remote_unit(&args.bind, args.port),
    )
}
