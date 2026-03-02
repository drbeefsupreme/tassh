//! tassh setup — generates systemd user service unit files and orchestrates systemctl.

use std::io::{BufRead, IsTerminal, Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{bail, Context};

use crate::cli::SetupDaemonArgs;

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
// Interactive wizard helpers
// ---------------------------------------------------------------------------

/// Print a yes/no prompt and return the user's answer.
/// Returns `false` when stdin is not a TTY (piped/redirected). Use `--yes` for non-interactive use.
fn prompt_yes_no(question: &str, default_yes: bool) -> bool {
    if !std::io::stdin().is_terminal() {
        return false;
    }

    let hint = if default_yes { "Y/n" } else { "y/N" };
    print!("{question} [{hint}] ");
    std::io::stdout().flush().ok();

    let mut input = String::new();
    match std::io::stdin().read_line(&mut input) {
        Ok(_) => match input.trim().to_lowercase().as_str() {
            "y" | "yes" => true,
            "n" | "no" => false,
            _ => default_yes,
        },
        Err(_) => default_yes,
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
// Public entry points
// ---------------------------------------------------------------------------

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

    // -----------------------------------------------------------------------
    // Systemd service (mandatory — this is what `setup daemon` is for).
    // -----------------------------------------------------------------------
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

    // loginctl enable-linger (warning only on failure or timeout).
    // Spawn the child and wait with a 10-second timeout so that an
    // unresponsive logind/D-Bus cannot hang `tassh setup daemon` indefinitely.
    match Command::new("loginctl").arg("enable-linger").spawn() {
        Err(e) => eprintln!(
            "warning: loginctl enable-linger failed to start: {e} — \
             linger may need to be enabled manually"
        ),
        Ok(mut child) => {
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(child.wait());
            });
            match rx.recv_timeout(Duration::from_secs(10)) {
                Ok(Ok(s)) if s.success() => {}
                Ok(Ok(s)) => eprintln!(
                    "warning: loginctl enable-linger exited with status {} — \
                     linger may need to be enabled manually",
                    s
                ),
                Ok(Err(e)) => eprintln!(
                    "warning: loginctl enable-linger failed: {e} — \
                     linger may need to be enabled manually"
                ),
                Err(_) => eprintln!(
                    "warning: loginctl enable-linger timed out after 10 s — \
                     linger may need to be enabled manually"
                ),
            }
        }
    }

    // -----------------------------------------------------------------------
    // SSH config — optional, prompt the user.
    // -----------------------------------------------------------------------
    println!();
    setup_ssh_config(args.yes)?;

    // -----------------------------------------------------------------------
    // Shell snippets — optional, prompt the user.
    // -----------------------------------------------------------------------
    println!();
    setup_shell_snippets(args.yes);

    println!();
    println!("To follow logs: journalctl --user -u {} -f", service_name);

    Ok(())
}

fn setup_ssh_config(yes: bool) -> anyhow::Result<()> {
    let ssh_config_path = home_dir().join(".ssh/config");

    // If the stanza is already present, nothing to do.
    if ssh_config_path.exists() {
        let content = std::fs::read_to_string(&ssh_config_path)?;
        if content.contains("# tassh:") {
            println!("SSH config already contains the tassh stanza — skipping.");
            return Ok(());
        }

        // Existing LocalCommand directives: we cannot safely append.
        if content.contains("LocalCommand") {
            println!("WARNING: ~/.ssh/config already contains LocalCommand directives.");
            println!("Please manually add the following to ~/.ssh/config:");
            println!("{}", ssh_config_stanza());
            return Ok(());
        }
    }

    let do_it = yes || prompt_yes_no("Add the tassh LocalCommand hook to ~/.ssh/config?", true);

    if do_it {
        if ssh_config_path.exists() {
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&ssh_config_path)?;
            writeln!(file, "{}", ssh_config_stanza())?;
            println!("Appended LocalCommand stanza to ~/.ssh/config");
        } else {
            std::fs::create_dir_all(ssh_config_path.parent().unwrap())?;
            std::fs::write(&ssh_config_path, ssh_config_stanza())?;
            println!("Created ~/.ssh/config with LocalCommand stanza");
        }
    } else {
        println!("Skipped SSH config update.");
        println!("To set it up manually, add the following to ~/.ssh/config:");
        println!("{}", ssh_config_stanza());
    }

    Ok(())
}

fn setup_shell_snippets(yes: bool) {
    let do_it = yes
        || prompt_yes_no(
            "Add the DISPLAY export hook to your shell profiles (.zshrc, .zprofile, .bashrc)?",
            true,
        );

    if do_it {
        install_shell_snippets();
    } else {
        println!("Skipped shell profile update.");
        println!(
            "To set it up manually, add the following to ~/.zshrc, ~/.zprofile, or ~/.bashrc:"
        );
        println!();
        println!("{}", shell_snippet());
    }
}
