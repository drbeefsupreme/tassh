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

`tassh setup daemon` does all of this:

- Writes `~/.config/systemd/user/tassh-daemon.service`
- Runs:
  - `systemctl --user daemon-reload`
  - `systemctl --user enable tassh-daemon.service`
  - `systemctl --user start tassh-daemon.service`
- Tries `loginctl enable-linger` so user services survive logout
- Adds this SSH stanza to `~/.ssh/config` (or asks you to merge manually if you already use `LocalCommand`):

```sshconfig
# tassh: notify daemon of SSH connections
Host *
    PermitLocalCommand yes
    LocalCommand tassh notify --host %h --port %p --ssh-pid $PPID
```

- Ensures your shell profiles source `~/.tassh/display` in SSH sessions

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

## Integration harness (Docker)

The repo includes a repeatable mesh E2E harness:

```bash
tests/daemon_mesh_e2e.sh
```

Requirements:

- Docker running locally

What it validates:

- First SSH session establishes daemon sync.
- Second SSH session to same node increments session count without extra daemon processes or extra peer TCP connections.
- Peer connection remains until the final SSH session exits.
- One node syncs to multiple peer nodes simultaneously.
- Frame fan-out reaches all connected peers.
- If SSH starts before remote daemon, sync comes up once remote daemon starts.

## CI

GitHub Actions workflow:

- `.github/workflows/daemon-mesh-e2e.yml`

Trigger:

- `pull_request` targeting `master`

The workflow builds `tassh` and runs `tests/daemon_mesh_e2e.sh`.

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
