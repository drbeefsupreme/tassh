# Phase 4: Integration and Packaging - Research

**Researched:** 2026-02-27
**Domain:** systemd user services, clap nested subcommands, shell environment detection, E2E clipboard validation
**Confidence:** HIGH

## Summary

Phase 4 is predominantly a wiring and packaging phase. The clipboard bridge machinery (transport, display management, clipboard read/write) is entirely built. The work here is: (1) add a `cssh setup` nested subcommand that writes systemd unit files and runs `systemctl --user` commands, (2) produce a shell snippet for DISPLAY sourcing, and (3) manually validate the E2E paste workflow in Claude Code, Codex, and OpenCode.

The biggest technical constraint is that systemd user services do not inherit shell environment variables, and ExecStart must use an absolute path. Since `cargo install --path .` puts the binary at `~/.cargo/bin/cssh`, the unit file's ExecStart must hardcode that path (or expand `$HOME` at setup time, writing the literal path into the unit file). This is the central design decision for the setup subcommand.

The E2E requirements (E2E-01 through E2E-03) are all manual-only: Claude Code, Codex, and OpenCode all use `xclip -selection clipboard -t image/png -o` under the hood on X11. This means the existing clipboard bridge (which writes via `xclip`) already feeds directly into what these tools read. No new clipboard protocol work is needed; the E2E validation is a manual smoke test on real hardware.

**Primary recommendation:** Keep the `setup` subcommand simple — expand `$HOME` at setup time to write literal absolute paths into unit files; no runtime variable expansion tricks needed. E2E validation is manual-only; no automated test tooling is appropriate for this phase.

---

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**Systemd service behavior:**
- Restart=always with ~5-second delay — keeps clipboard bridge running unattended
- Auto-start on login via loginctl enable-linger + WantedBy=default.target
- Logs go to systemd journal only — `journalctl --user -u cssh-local` / `cssh-remote`
- Separate unit files: `cssh-local.service` and `cssh-remote.service` (not template units)

**Shell snippet design:**
- Detect SSH sessions via `$SSH_CONNECTION` — only source DISPLAY when set
- Support bash and zsh (single compatible snippet for .bashrc and .zshrc)
- Print snippet for user to copy-paste — don't auto-modify rc files
- Export DISPLAY only (not WAYLAND_DISPLAY) — remote is headless with Xvfb

**Connection configuration:**
- Remote address passed via CLI flag: `cssh local --remote 100.x.y.z:port`
- Fixed default port (e.g., 9737) — overridable with `--port` flag
- Remote binds to Tailscale interface only: `cssh remote --bind 100.x.y.z`
- User specifies bind address explicitly via `--bind` flag

**Setup subcommand:**
- `cssh setup local --remote 100.x.y.z` and `cssh setup remote --bind 100.x.y.z`
- Copies unit files to `~/.config/systemd/user/`, enables services, starts immediately
- Prints shell snippet for user to add to their rc file
- Binary installed via `cargo install --path .` on each machine

### Claude's Discretion

- Exact default port number choice
- Unit file ordering/dependency details (e.g., After=network.target)
- Exact shell snippet formatting and comments
- E2E validation test approach and tooling
- Error messages and setup output formatting

### Deferred Ideas (OUT OF SCOPE)

None — discussion stayed within phase scope
</user_constraints>

---

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| SRVC-02 | Systemd user service unit files for both local and remote daemons | Unit file templates, `~/.config/systemd/user/` location, loginctl enable-linger |
| SRVC-03 | Shell snippet for .bashrc/.zshrc to auto-source DISPLAY on SSH login | `$SSH_CONNECTION` detection, sourcing `~/.cssh/display`, bash/zsh compatibility |
| E2E-01 | Ctrl-V in Claude Code on remote shows [Image #1] | Claude Code uses `xclip -selection clipboard -t image/png -o`; requires DISPLAY set |
| E2E-02 | Ctrl-V in Codex on remote shows screenshot image | Codex uses xclip for X11 clipboard reads; same requirement as E2E-01 |
| E2E-03 | Ctrl-V in OpenCode on remote shows screenshot image | OpenCode uses `xclip -selection clipboard -t image/png -o` on X11 |
</phase_requirements>

---

## Standard Stack

### Core

| Library / Tool | Version | Purpose | Why Standard |
|----------------|---------|---------|--------------|
| clap derive | 4 (already in Cargo.toml) | `cssh setup local/remote` nested subcommand | Already used; nested subcommands via `#[command(subcommand)]` on a field inside a variant |
| std::fs | stdlib | Write unit files to `~/.config/systemd/user/` | No external crate needed for file write |
| std::process::Command | stdlib | Run `systemctl --user`, `loginctl enable-linger` | Simple subprocess invocations; no crate needed |

### Supporting

| Tool | Purpose | When to Use |
|------|---------|-------------|
| `loginctl enable-linger` | Allow user services to survive across reboots without a logged-in session | Called once during `cssh setup` |
| `systemctl --user daemon-reload` | Pick up newly written unit files | Called after writing unit files |
| `systemctl --user enable` | Symlink service into default.target.wants | Called after daemon-reload |
| `systemctl --user start` | Start service immediately | Called after enable |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Hardcoding `~/.cargo/bin/cssh` at setup time | Detecting binary path at runtime | Hardcoding is simpler; setup expands `$HOME` once and writes literal path |
| `std::process::Command` for systemctl | `zbus` systemd D-Bus crate | D-Bus approach is over-engineered for a one-shot setup command |

**Installation:**
No new dependencies needed. This phase uses only the existing `clap` and standard library.

---

## Architecture Patterns

### Recommended Project Structure

```
src/
├── cli.rs           # Add Setup variant + SetupLocal/SetupRemote args
├── setup.rs         # New module: unit file generation + systemctl invocations
├── main.rs          # Add Commands::Setup arm
└── (existing modules unchanged)
```

### Pattern 1: Nested Subcommand with Clap Derive

**What:** `cssh setup local --remote ...` and `cssh setup remote --bind ...` via a two-level enum.

**When to use:** Anytime a subcommand itself has subcommands.

**Example:**
```rust
// In cli.rs — add to existing Commands enum:
#[derive(Debug, Subcommand)]
pub enum Commands {
    Local(LocalArgs),
    Remote(RemoteArgs),
    Status,
    /// Install systemd user services and print shell snippet
    Setup {
        #[command(subcommand)]
        target: SetupTarget,
    },
}

#[derive(Debug, Subcommand)]
pub enum SetupTarget {
    /// Set up the local daemon service
    Local(SetupLocalArgs),
    /// Set up the remote daemon service
    Remote(SetupRemoteArgs),
}

#[derive(Debug, Parser)]
pub struct SetupLocalArgs {
    /// Remote host or host:port to connect to
    #[arg(long)]
    pub remote: String,

    #[arg(long, default_value = "9877")]
    pub port: u16,
}

#[derive(Debug, Parser)]
pub struct SetupRemoteArgs {
    /// Tailscale address to bind on
    #[arg(long)]
    pub bind: String,

    #[arg(long, default_value = "9877")]
    pub port: u16,
}
```

### Pattern 2: Unit File Generation

**What:** Build the unit file content as a `String` with the absolute ExecStart path expanded at setup time.

**Key constraint:** systemd ExecStart must be an absolute path. `$HOME` is NOT expanded by systemd in ExecStart on user services by default (behavior varies by systemd version). The safe approach is to expand it in Rust at setup time.

**Example:**
```rust
// In setup.rs
use std::path::PathBuf;

fn binary_path() -> PathBuf {
    // cargo install --path . puts binary at $HOME/.cargo/bin/cssh
    dirs_home().join(".cargo").join("bin").join("cssh")
}

fn home_dir() -> PathBuf {
    // std::env::var("HOME") is reliable at setup time (user is logged in)
    PathBuf::from(std::env::var("HOME").expect("HOME not set"))
}

fn cssh_local_unit(remote: &str, port: u16) -> String {
    let bin = home_dir().join(".cargo").join("bin").join("cssh");
    format!(
        "[Unit]\n\
         Description=cssh local clipboard relay\n\
         After=network.target\n\
         \n\
         [Service]\n\
         ExecStart={bin} local --remote {remote} --port {port}\n\
         Restart=always\n\
         RestartSec=5\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        bin = bin.display(),
        remote = remote,
        port = port,
    )
}

fn cssh_remote_unit(bind: &str, port: u16) -> String {
    let bin = home_dir().join(".cargo").join("bin").join("cssh");
    format!(
        "[Unit]\n\
         Description=cssh remote clipboard relay\n\
         After=network.target\n\
         \n\
         [Service]\n\
         ExecStart={bin} remote --bind {bind} --port {port}\n\
         Restart=always\n\
         RestartSec=5\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        bin = bin.display(),
        bind = bind,
        port = port,
    )
}
```

### Pattern 3: Setup Orchestration

**What:** Write unit files, then run `systemctl --user` commands in sequence.

**Example:**
```rust
pub fn run_setup_local(args: &SetupLocalArgs) -> anyhow::Result<()> {
    let unit_dir = home_dir()
        .join(".config").join("systemd").join("user");
    std::fs::create_dir_all(&unit_dir)?;

    let unit_content = cssh_local_unit(&args.remote, args.port);
    let unit_path = unit_dir.join("cssh-local.service");
    std::fs::write(&unit_path, &unit_content)?;
    println!("Wrote {}", unit_path.display());

    for argv in &[
        vec!["--user", "daemon-reload"],
        vec!["--user", "enable", "cssh-local.service"],
        vec!["--user", "start", "cssh-local.service"],
    ] {
        let status = std::process::Command::new("systemctl")
            .args(argv)
            .status()?;
        if !status.success() {
            anyhow::bail!("systemctl {} failed", argv.join(" "));
        }
    }

    // Enable linger so service survives logout
    let status = std::process::Command::new("loginctl")
        .args(["enable-linger"])
        .status()?;
    if !status.success() {
        eprintln!("Warning: loginctl enable-linger failed (may need to run as root)");
    }

    // Print shell snippet
    println!("\nAdd this to ~/.bashrc or ~/.zshrc on the REMOTE machine:\n");
    println!("{}", display_shell_snippet());
    Ok(())
}
```

### Pattern 4: Shell Snippet for DISPLAY Sourcing

**What:** A POSIX-compatible snippet that sources `~/.cssh/display` only inside SSH sessions.

**Key facts:**
- `$SSH_CONNECTION` is set by sshd for all interactive and non-interactive SSH sessions (bash and zsh).
- `~/.cssh/display` is written by `DisplayManager::publish_display()` as `export DISPLAY=:N\n`.
- Snippet must work in both bash and zsh with a single copy-pasteable block.

**Example:**
```bash
# cssh: auto-export DISPLAY in SSH sessions
if [ -n "$SSH_CONNECTION" ] && [ -f "$HOME/.cssh/display" ]; then
    . "$HOME/.cssh/display"
fi
```

This is POSIX sh syntax and works identically in bash and zsh. The `.` (dot) builtin sources the file; it executes `export DISPLAY=:N` which is also POSIX-compatible.

### Anti-Patterns to Avoid

- **Expand `$HOME` in ExecStart at runtime:** Systemd does support `%h` specifier (home directory of the user running the unit) as of systemd 236+, but behavior is version-dependent. Safe to use `%h` on Ubuntu 22.04+ (systemd 249), but the simpler and guaranteed-correct approach is to expand at setup time.
- **Auto-modifying `.bashrc`:** Locked decision says print only; don't auto-modify.
- **Using `WAYLAND_DISPLAY` in snippet:** The remote is headless with Xvfb; DISPLAY is the only relevant variable.
- **Using template units (@) for cssh-local and cssh-remote:** Locked decision: separate named unit files.
- **Sourcing `.bashrc` from the unit file to pick up PATH:** Systemd user units don't source `.bashrc`. Use the absolute path in ExecStart instead.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Nested subcommands | Custom argv parsing | `clap` `#[command(subcommand)]` | Already in the project; trivial to extend |
| Unit file validation | Custom parser | Just write the string; systemd validates on daemon-reload | Unit files are trivial ini-style text |
| Subprocess management for systemctl | Custom D-Bus wiring | `std::process::Command` | One-shot commands; no streaming needed |

**Key insight:** This phase has no hard algorithmic problems. The complexity is purely in getting the sequencing right (write files → daemon-reload → enable → start → linger).

---

## Common Pitfalls

### Pitfall 1: ExecStart Path Not Absolute

**What goes wrong:** `systemctl --user start cssh-local` fails with "Executable ... not absolute path" or silently uses wrong binary.

**Why it happens:** systemd does not apply shell PATH expansion to ExecStart. `~`, `$HOME`, and relative paths do not work.

**How to avoid:** In `setup.rs`, use `std::env::var("HOME")` at setup time and write the full expanded path into the unit file. Verify the binary exists at that path before writing.

**Warning signs:** `systemctl --user status cssh-local` shows "failed" with ExecStart-related error message.

### Pitfall 2: Missing daemon-reload After Writing Unit File

**What goes wrong:** `systemctl --user enable cssh-local.service` returns "Unit file not found" or enables a stale version.

**Why it happens:** Systemd caches unit files; new files on disk are invisible until `daemon-reload`.

**How to avoid:** Always run `systemctl --user daemon-reload` before `enable` and `start`. This is the first systemctl call in the setup sequence.

### Pitfall 3: loginctl enable-linger Requires No Arguments for Current User

**What goes wrong:** `loginctl enable-linger` fails with permission error.

**Why it happens:** Without arguments, `loginctl enable-linger` enables linger for the current user, which requires no root. With a username argument, it requires root on some distros.

**How to avoid:** Call `loginctl enable-linger` with no arguments (enables for the invoking user). The setup command runs as the user who will own the service.

**Warning signs:** Non-zero exit code from loginctl; print a warning but don't abort setup.

### Pitfall 4: DISPLAY Not Set When CLI Tool Runs Ctrl-V

**What goes wrong:** Claude Code / Codex / OpenCode shows "No image found in clipboard" even though the bridge is running.

**Why it happens:** The SSH session that launched the CLI tool did not source `~/.cssh/display`, so DISPLAY is unset, and xclip fails silently.

**How to avoid:** The shell snippet must be added to `.bashrc` / `.zshrc` and the user must open a new SSH session (or source the rc file) after setup. Document this explicitly in the setup output.

**Warning signs:** `echo $DISPLAY` in the SSH session returns empty; xclip errors with "can't open display".

### Pitfall 5: xclip -loops 0 Background Fork Behavior (from STATE.md)

**What goes wrong:** The selection owner process (xclip) exits before Claude Code / Codex / OpenCode attempts to read from the clipboard.

**Why it happens:** `xclip -loops 0` forks to background and serves the selection indefinitely on some systems, but on others the behavior differs. (This is flagged as an unresolved research concern in STATE.md.)

**How to avoid:** Phase 3 chose to store the xclip subprocess handle without calling `.wait()` — this keeps the selection owner alive for the lifetime of the connection. E2E testing will confirm whether this is sufficient.

**Warning signs:** First Ctrl-V works, subsequent ones in the same session fail; or images paste correctly in xclip but not in Claude Code.

### Pitfall 6: Unit File WantedBy=default.target Symlink Not Created

**What goes wrong:** Service starts manually but not at login/boot.

**Why it happens:** `systemctl --user enable` must be run (not just start); it creates the symlink in `~/.config/systemd/user/default.target.wants/`.

**How to avoid:** Run `enable` before `start`. Check that the symlink exists after setup: `ls ~/.config/systemd/user/default.target.wants/`.

---

## Code Examples

### Unit File Written to Disk (cssh-remote.service)

```ini
[Unit]
Description=cssh remote clipboard relay
After=network.target

[Service]
ExecStart=/home/user/.cargo/bin/cssh remote --bind 100.64.0.1 --port 9877
Restart=always
RestartSec=5

[Install]
WantedBy=default.target
```

Note: `/home/user` is the literal home directory, expanded at setup time.

### Shell Snippet (printed to stdout by `cssh setup`)

```bash
# cssh: auto-export DISPLAY in SSH sessions
# Add to ~/.bashrc and/or ~/.zshrc on the remote machine:
if [ -n "$SSH_CONNECTION" ] && [ -f "$HOME/.cssh/display" ]; then
    . "$HOME/.cssh/display"
fi
```

The `.cssh/display` file content written by `DisplayManager::publish_display()`:
```
export DISPLAY=:1
```

### Setup Subcommand Flow (pseudocode)

```
cssh setup local --remote 100.x.y.z:9877
  1. Expand $HOME → absolute binary path
  2. Create ~/.config/systemd/user/ if absent
  3. Write cssh-local.service with literal ExecStart
  4. systemctl --user daemon-reload
  5. systemctl --user enable cssh-local.service
  6. systemctl --user start cssh-local.service
  7. loginctl enable-linger (no args = current user)
  8. Print shell snippet
  9. Print "Service started. Check status: journalctl --user -u cssh-local -f"

cssh setup remote --bind 100.x.y.z
  1-7. Same pattern for cssh-remote.service
  8. Print shell snippet
  9. Print status hint
```

### Confirming E2E works (manual test procedure)

```
On remote:
  1. journalctl --user -u cssh-remote -f    # watch logs
  2. Open new SSH session, confirm: echo $DISPLAY → :1

On local:
  3. Take screenshot (Flameshot, gnome-screenshot, etc.)
  4. journalctl --user -u cssh-local -f     # confirm "new image" log line

On remote:
  5. Open Claude Code
  6. Press Ctrl-V
  7. Confirm [Image #1] appears
```

---

## How CLI Tools Read Clipboard Images (E2E-01, E2E-02, E2E-03)

All three tools use the system clipboard via xclip on X11. This is confirmed by multiple GitHub issues:

**Claude Code (anthropics/claude-code):**
- Uses `xclip -selection clipboard -t image/png -o` to retrieve image data
- Requires `xclip` installed and `DISPLAY` set
- Source: GitHub issues #14725, #15031, #29204

**OpenCode (sst/opencode):**
- Uses `xclip -selection clipboard -t image/png -o` on X11
- Binary PNG data is read from xclip; the tool then processes it
- Source: GitHub issue #3816 (clipboard.ts implementation)

**Codex (openai/codex):**
- Uses xclip or wl-paste for clipboard reads on Linux
- Ctrl-V triggers clipboard image paste (Codex issue #2743)
- Exact command: xclip-based on X11, wl-paste-based on Wayland

**Implication for this phase:** The cssh remote daemon already writes via `xclip -selection clipboard -t image/png -i`, which is the counterpart to what Claude Code, OpenCode, and Codex read. No additional clipboard protocol work is needed. The E2E path is:
```
local screenshot → cssh local (arboard read) → TCP → cssh remote (xclip write)
               → [Claude Code/Codex/OpenCode] xclip -t image/png -o read → display
```
The bridge is complete. E2E validation is purely manual.

---

## State of the Art

| Old Approach | Current Approach | Impact |
|--------------|------------------|--------|
| Shipping shell scripts for service setup | `cargo install` + `cssh setup` subcommand | Single binary, no shell script distribution needed |
| Hardcoding unit files in /etc/systemd/system | User-space `~/.config/systemd/user/` | No root required; user-owned services |
| Template units (@.service) for parameterization | Separate named units with parameters baked in at setup | Simpler management; no `systemctl --user start foo@args.service` syntax |

**Deprecated/outdated:**
- `~/.pam_environment`: Deprecated in some distros (Ubuntu 22.04+ removed pam_env for user home); do not use for DISPLAY.
- PAM-based DISPLAY injection: Not applicable for headless Xvfb; the shell snippet approach is more portable.

---

## Open Questions

1. **Does `xclip -loops 0` keep selection alive long enough?**
   - What we know: Phase 3 stores the subprocess handle without `.wait()` to keep xclip alive
   - What's unclear: On target Ubuntu, does `xclip -loops 0` exit after the first paste or serve indefinitely?
   - Recommendation: E2E test should include pasting Ctrl-V multiple times (once is not sufficient). If it fails on second paste, switch to `xclip -loops -1` (infinite loops)
   - Source: STATE.md blocker "xclip `-loops 0` background-fork behavior needs empirical testing"

2. **Does loginctl enable-linger work without root on the target Ubuntu version?**
   - What we know: On Ubuntu 20.04+, `loginctl enable-linger` (no args) works without root for the current user
   - What's unclear: Whether the specific target machine has policykit configured to allow this
   - Recommendation: Run it and check exit code; print a clear warning if it fails rather than aborting setup

3. **Which port to use as default?**
   - Current code has 9877 (from Phase 2 decision); CONTEXT.md references 9737 as an example
   - Recommendation: Keep 9877 (already in code; changing would break existing running instances)

---

## Validation Architecture

Nyquist validation is not enabled in `.planning/config.json` (no `workflow.nyquist_validation` key). Section skipped.

---

## Sources

### Primary (HIGH confidence)
- Arch Wiki: systemd/User — unit file locations, loginctl enable-linger, WantedBy=default.target
- freedesktop.org systemd.exec man page — ExecStart absolute path requirement confirmed
- Claude Code GitHub issues #14725, #15031, #29204 — xclip usage for image clipboard reads confirmed
- OpenCode GitHub issue #3816 — X11 clipboard implementation via xclip confirmed

### Secondary (MEDIUM confidence)
- WebSearch + Arch Wiki cross-reference: `systemctl --user daemon-reload → enable → start` sequence
- WebSearch + multiple sources: `$SSH_CONNECTION` variable set by sshd for SSH session detection
- cargo book (doc.rust-lang.org): `cargo install` puts binary in `$HOME/.cargo/bin/`

### Tertiary (LOW confidence)
- OpenAI Codex issue #2743: Codex clipboard paste on Linux — confirmed Ctrl-V works but specific xclip command not documented in that issue; inferred from ecosystem patterns

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — clap already in project; std::fs and std::process::Command are stdlib; systemd patterns are well-documented
- Architecture: HIGH — unit file structure is stable; setup sequence is canonical; shell snippet is POSIX
- E2E clipboard tool behavior: HIGH — Claude Code and OpenCode use xclip confirmed by multiple GitHub issues with source code references
- Pitfalls: HIGH — ExecStart absolute path and daemon-reload sequencing are documented systemd requirements; xclip loops behavior is LOW (flagged in STATE.md as needing empirical testing)

**Research date:** 2026-02-27
**Valid until:** 2026-03-29 (stable domain; systemd user service patterns are not volatile)
