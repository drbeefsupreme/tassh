# tassh

A PNG clipboard bridge for [Tailscale](https://tailscale.com/). Take a screenshot on your local machine and paste it into any application on a remote server over SSH.

Useful for pasting screenshots into AI coding tools like Claude Code, Codex, and OpenCode running on remote machines, or any workflow where you need clipboard images available over SSH.

## How it works

1. **Local daemon** (`tassh local`) watches your clipboard for new screenshots
2. When a screenshot is detected, it sends the PNG over TCP to the remote
3. **Remote daemon** (`tassh remote`) receives the image and writes it to an X11 clipboard (via Xvfb)
4. SSH sessions source `~/.tassh/display` to access that clipboard
5. Ctrl-V in any application reads the image from the clipboard

```
[Screenshot] → tassh local → TCP/Tailscale → tassh remote → Xvfb clipboard → Ctrl-V
```

## Prerequisites

- [Rust](https://rustup.rs/) (for building)
- [Tailscale](https://tailscale.com/) (for networking between machines)
- **Remote machine:** `xclip` and `Xvfb` (`apt install xclip xvfb`)

## Install

On **both** machines (local and remote):

```bash
git clone https://github.com/drbeefsupreme/tassh.git
cd tassh
cargo install --path .
```

## Setup

On the **local** machine (where you take screenshots):

```bash
tassh setup local --remote <tailscale-ip-of-remote>
```

On the **remote** machine (where you SSH into):

```bash
tassh setup remote --bind <tailscale-ip-of-remote>
```

This creates systemd user services that start automatically and persist across reboots.

Add the DISPLAY snippet to your shell profile on the remote:

```bash
# For zsh:
cat >> ~/.zshrc << 'EOF'

# tassh: auto-export DISPLAY in SSH sessions
if [ -n "$SSH_CONNECTION" ] && [ -f "$HOME/.tassh/display" ]; then
    . "$HOME/.tassh/display"
fi
EOF

# For bash:
cat >> ~/.bashrc << 'EOF'

# tassh: auto-export DISPLAY in SSH sessions
if [ -n "$SSH_CONNECTION" ] && [ -f "$HOME/.tassh/display" ]; then
    . "$HOME/.tassh/display"
fi
EOF
```

Then open a **new SSH session** for it to take effect.

## Usage

1. Take a screenshot on your local machine (Flameshot, PrtScn, Snipping Tool, etc.)
2. SSH into the remote machine
3. Press Ctrl-V in any application -- the screenshot appears

## Troubleshooting

Check service logs:

```bash
journalctl --user -u tassh-local -f   # on local
journalctl --user -u tassh-remote -f  # on remote
```

Verify DISPLAY is set in your SSH session:

```bash
echo $DISPLAY
```

Verify clipboard has image data:

```bash
xclip -selection clipboard -t image/png -o | file -
```

## License

[MIT](LICENSE)
