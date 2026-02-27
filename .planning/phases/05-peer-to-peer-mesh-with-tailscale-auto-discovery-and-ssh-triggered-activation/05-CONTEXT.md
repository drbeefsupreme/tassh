# Phase 5: SSH-triggered Auto-connection - Context

**Gathered:** 2026-02-27
**Status:** Ready for planning

<domain>
## Phase Boundary

Evolve tassh from manual connection to automatic SSH-triggered connections. When user SSHs to a host, tassh automatically detects and connects to the remote daemon if present. Bidirectional clipboard sync between any connected nodes. Tailscale provides the encrypted transport layer but requires no explicit configuration (no tags).

</domain>

<decisions>
## Implementation Decisions

### SSH Integration
- Use SSH `LocalCommand` in `~/.ssh/config` to detect new SSH connections
- LocalCommand notifies local tassh daemon with hostname and SSH process PID
- tassh probes remote host for daemon on connect
- If daemon found, add to active clipboard targets
- Discovery is on-demand (only when SSH connects), not periodic

### Connection Lifecycle
- Single connection per unique remote host (regardless of SSH session count)
- Watch SSH process PID to detect session end
- Disconnect from host only when ALL SSH sessions to that host are closed
- Auto-reconnect with backoff if connection drops while SSH session still active

### Daemon Architecture
- Single `tassh daemon` command (replace separate `local`/`remote` subcommands)
- Role auto-detected per connection based on SSH direction
- Machine initiating SSH becomes sender for that connection
- Machine being SSH'd to becomes receiver for that connection
- Bidirectional flow supported: if A SSHs to B AND B SSHs to A, clipboard flows both ways

### Clipboard Behavior
- All nodes watch their clipboard for changes (even headless with Xvfb)
- When clipboard changes, send to all SSH-connected peers where this node is sender
- Existing clipboard read/write logic (arboard, xclip, wl-copy) unchanged

### User Feedback
- Silent operation by default (no desktop notifications)
- `tassh status` command shows active connections
- No proactive notifications on connect/disconnect

### Claude's Discretion
- Port selection (fixed vs configurable with sensible default)
- Exact LocalCommand syntax and PID tracking implementation
- Process watching mechanism details
- tassh setup command modifications for ~/.ssh/config

</decisions>

<specifics>
## Specific Ideas

- "Ideally the user installs tassh locally and on the remote. When an SSH session is started, the local tassh checks for a tassh daemon on the remote. If so it connects."
- Tailscale stays as network layer for encryption and NAT traversal, but no Tailscale-specific API calls or tags
- LocalCommand chosen over shell wrappers for reliability across all shells and SSH invocation methods

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 05-peer-to-peer-mesh-with-tailscale-auto-discovery-and-ssh-triggered-activation*
*Context gathered: 2026-02-27*
