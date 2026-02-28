# tassh

A PNG clipboard bridge for [Tailscale](https://tailscale.com/) + SSH.

Take a screenshot on one node and paste it into apps on another node over SSH.

## How it works (daemon mode)

`tassh` uses a single daemon per node.

1. `tassh daemon` runs on each node.
2. On SSH connect, SSH `LocalCommand` runs `tassh notify` on the source node.
3. The source daemon tracks SSH sessions and connects to destination daemon(s) automatically.
4. Clipboard PNG frames are forwarded over TCP on the Tailscale network.
5. Destination node writes frames into clipboard via `xclip` (daemon mode always starts Xvfb for reliable remote paste).

```
[Screenshot] -> tassh daemon -> TCP/Tailscale -> tassh daemon -> remote clipboard -> Ctrl-V
```

## Prerequisites

- [Rust](https://rustup.rs/) (build/install)
- [Tailscale](https://tailscale.com/) (node-to-node network path)
- OpenSSH client/server
- Clipboard tools:
  - `xclip` and `xvfb` (required; daemon always uses Xvfb for remote paste reliability)
  - `wl-paste` from `wl-clipboard` (optional; used for clipboard watching if your local session uses Wayland — `wl-copy` is **not** used in the default daemon path)

## Install

On each node:

```bash
git clone https://github.com/drbeefsupreme/tassh.git
cd tassh
cargo install --path .
```

## Setup (recommended)

Run on each node where you want automatic behavior:

```bash
tassh setup daemon
```

`tassh setup daemon` launches an interactive wizard and does all of this:

- Writes `~/.config/systemd/user/tassh-daemon.service`
- Runs:
  - `systemctl --user daemon-reload`
  - `systemctl --user enable tassh-daemon.service`
  - `systemctl --user start tassh-daemon.service`
- Tries `loginctl enable-linger` so user services survive logout
- Prompts to add this SSH stanza to `~/.ssh/config` (or shows it for manual merge if you already use `LocalCommand`):

```sshconfig
# tassh: notify daemon of SSH connections
Host *
    PermitLocalCommand yes
    LocalCommand tassh notify --host %h --port %p --ssh-pid $PPID
```

- Prompts to source `~/.tassh/display` in your shell profiles (`.zshrc`, `.zprofile`, `.bashrc`) for SSH sessions

Use `--yes` to skip all prompts and accept defaults non-interactively.

## Usage

1. Keep `tassh-daemon.service` running on participating nodes.
2. Start an SSH session as usual.
3. Take a screenshot on the source node.
4. Paste on the destination node (`Ctrl-V`) in apps that accept image paste.

Check status at any time:

```bash
tassh status
```

## Systemd and logs

Check daemon logs:

```bash
journalctl --user -u tassh-daemon.service -f
```

## Troubleshooting

Verify daemon socket + status:

```bash
tassh status
```

Verify display is exported in SSH session:

```bash
echo "$DISPLAY"
```

Verify clipboard has image data:

```bash
xclip -selection clipboard -t image/png -o | file -
```

## License

[MIT](LICENSE)
