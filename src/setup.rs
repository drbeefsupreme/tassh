//! tassh setup — generates systemd user service unit files and orchestrates systemctl.

use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context};

use crate::cli::{SetupDaemonArgs, SetupLocalArgs, SetupRemoteArgs};

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
    r#"# tassh: auto-export DISPLAY in SSH sessions
if [ -n "$SSH_CONNECTION" ] && [ -f "$HOME/.tassh/display" ]; then
    . "$HOME/.tassh/display"
fi
"#
}

fn ensure_shell_snippet(shell: &str) -> anyhow::Result<bool> {
    let path = home_dir().join(shell);
    let marker = "# tassh: auto-export DISPLAY in SSH sessions";

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(&path)
        .with_context(|| format!("failed to open {}", path.display()))?;

    // Lock the file for the whole read/check/append sequence to avoid duplicate
    // snippet writes from concurrent setup invocations.
    // SAFETY: `flock` is called with a valid file descriptor and constant flags.
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error())
            .with_context(|| format!("failed to lock {}", path.display()));
    }

    let mut content = String::new();
    file.read_to_string(&mut content)
        .with_context(|| format!("failed to read {}", path.display()))?;

    if content.contains(marker) {
        return Ok(false);
    }

    let mut to_append = String::new();
    if !content.is_empty() && !content.ends_with('\n') {
        to_append.push('\n');
    }
    to_append.push('\n');
    to_append.push_str(shell_snippet());

    file.seek(SeekFrom::End(0))
        .with_context(|| format!("failed to seek {}", path.display()))?;
    file.write_all(to_append.as_bytes())
        .with_context(|| format!("failed to write {}", path.display()))?;
    file.flush()
        .with_context(|| format!("failed to flush {}", path.display()))?;
    Ok(true)
}

fn install_shell_snippets() {
    println!();
    println!("Ensuring SSH display hook in shell profiles:");
    for shell in [".zshrc", ".zprofile", ".bashrc"] {
        match ensure_shell_snippet(shell) {
            Ok(true) => println!("  updated ~/{shell}"),
            Ok(false) => println!("  unchanged ~/{shell}"),
            Err(e) => eprintln!("warning: failed to update ~/{shell}: {e}"),
        }
    }
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
        bail!("systemctl {} exited with status {}", args.join(" "), status);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Core setup orchestration
// ---------------------------------------------------------------------------

fn run_setup(service_name: &str, unit_content: &str) -> anyhow::Result<()> {
    let dir = unit_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;

    let unit_path = dir.join(service_name);
    std::fs::write(&unit_path, unit_content)
        .with_context(|| format!("failed to write {}", unit_path.display()))?;
    println!("Wrote {}", unit_path.display());

    run_systemctl(&["--user", "daemon-reload"])?;
    run_systemctl(&["--user", "enable", service_name])?;
    run_systemctl(&["--user", "start", service_name])?;

    // loginctl enable-linger: failure is a warning, not a fatal error.
    let linger_status = Command::new("loginctl").arg("enable-linger").status();
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

    install_shell_snippets();
    println!();
    println!("To follow logs: journalctl --user -u {} -f", service_name);

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

// ---------------------------------------------------------------------------
// Daemon setup helpers
// ---------------------------------------------------------------------------

fn tassh_daemon_unit(port: u16) -> String {
    let bin = binary_path();
    let exec_start = format!("{} daemon --port {}", bin.display(), port);

    format!(
        "[Unit]\n\
         Description=tassh daemon (SSH-triggered clipboard relay)\n\
         After=network.target\n\
         \n\
         [Service]\n\
         ExecStart={exec_start}\n\
         Restart=always\n\
         RestartSec=5\n\
         TimeoutStopSec=3\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n"
    )
}

fn ssh_config_stanza() -> String {
    r#"
# tassh: notify daemon of SSH connections
Host *
    PermitLocalCommand yes
    LocalCommand tassh notify --host %h --port %p --ssh-pid $PPID
"#
    .to_string()
}

/// Install tassh-daemon.service and configure SSH LocalCommand.
pub fn run_setup_daemon(args: &SetupDaemonArgs) -> anyhow::Result<()> {
    let bin = binary_path();
    if !bin.exists() {
        bail!(
            "binary not found at {}. Run `cargo install --path .` first.",
            bin.display()
        );
    }

    // Write systemd unit.
    let service_name = "tassh-daemon.service";
    let dir = unit_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;

    let unit_path = dir.join(service_name);
    std::fs::write(&unit_path, tassh_daemon_unit(args.port))
        .with_context(|| format!("failed to write {}", unit_path.display()))?;
    println!("Wrote {}", unit_path.display());

    run_systemctl(&["--user", "daemon-reload"])?;
    run_systemctl(&["--user", "enable", service_name])?;
    run_systemctl(&["--user", "start", service_name])?;

    // loginctl enable-linger (warning only on failure).
    let linger_status = Command::new("loginctl").arg("enable-linger").status();
    match linger_status {
        Ok(s) if s.success() => {}
        Ok(s) => eprintln!(
            "warning: loginctl enable-linger exited with status {} — \
             linger may need to be enabled manually",
            s
        ),
        Err(e) => eprintln!(
            "warning: loginctl enable-linger failed: {e} — \
             linger may need to be enabled manually"
        ),
    }

    // Handle SSH config.
    let ssh_config_path = home_dir().join(".ssh/config");

    if ssh_config_path.exists() {
        let content = std::fs::read_to_string(&ssh_config_path)?;
        if content.contains("# tassh:") {
            println!();
            println!("SSH config already contains tassh stanza.");
        } else if content.contains("LocalCommand") {
            println!();
            println!("WARNING: ~/.ssh/config already contains LocalCommand directives.");
            println!("Please manually add the following to ~/.ssh/config:");
            println!("{}", ssh_config_stanza());
            println!();
        } else {
            // Safe to append.
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&ssh_config_path)?;
            use std::io::Write;
            writeln!(file, "{}", ssh_config_stanza())?;
            println!("Appended LocalCommand stanza to ~/.ssh/config");
        }
    } else {
        // Create new SSH config.
        std::fs::create_dir_all(ssh_config_path.parent().unwrap())?;
        std::fs::write(&ssh_config_path, ssh_config_stanza())?;
        println!("Created ~/.ssh/config with LocalCommand stanza");
    }

    install_shell_snippets();
    println!();

    println!("To follow logs: journalctl --user -u {} -f", service_name);

    Ok(())
}
