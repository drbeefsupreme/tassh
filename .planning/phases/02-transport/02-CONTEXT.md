# Phase 2: Transport - Context

**Gathered:** 2026-02-27
**Status:** Ready for planning

<domain>
## Phase Boundary

TCP sender and receiver that reliably move arbitrary byte frames from the local daemon to the remote daemon over Tailscale, with automatic reconnection on failure. Framing logic already exists from Phase 1. Clipboard reading/writing and display management are separate phases.

</domain>

<decisions>
## Implementation Decisions

### Connection configuration
- Remote address specified via `--remote` CLI flag on `cssh local` (no config file)
- Default port: 9877 (override with `--remote host:port`)
- `cssh remote` binds to Tailscale interface only — not exposed on other networks
- Named flag `--remote` rather than positional argument

### Timeout tuning
- TCP keepalive detects silently dropped connections within ~30 seconds
- Write operations timeout after 10 seconds — treat as dead connection, trigger reconnect
- All timeout values hardcoded — no configurable flags

### Logging & feedback
- Silent by default — no output unless RUST_LOG is set
- Verbosity controlled via standard RUST_LOG env var (tracing/env_logger)
- Human-readable log format (e.g. `INFO cssh::transport: connected to 100.x.x.x:9877`)
- Connection state changes (connected/disconnected/reconnecting) logged at WARN level — visible even with default silent behavior

### Reconnect behavior
- Retry forever — daemon never gives up (matches long-running service design)
- Exponential backoff: start at 1s, double each attempt, cap at 30s
- Jitter: random 0-25% added to each backoff interval
- Backoff resets immediately on successful reconnection

### Claude's Discretion
- Async runtime choice (tokio vs std threads)
- Internal connection state machine design
- TCP socket options beyond keepalive and write timeout
- Error types and internal error handling patterns

</decisions>

<specifics>
## Specific Ideas

No specific requirements — open to standard approaches

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 02-transport*
*Context gathered: 2026-02-27*
