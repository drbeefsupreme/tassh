# Phase 1: Foundation - Context

**Gathered:** 2026-02-27
**Status:** Ready for planning

<domain>
## Phase Boundary

Single binary (`cssh`) with `local` and `remote` subcommand stubs, shared wire protocol types (`Frame`, `DisplayEnvironment`), and length-prefixed framing logic tested in isolation. No networking, no clipboard, no display management — those are later phases.

</domain>

<decisions>
## Implementation Decisions

### Wire format
- Header: 2-byte magic + 1-byte version + 1-byte frame type + u32 length (big-endian) + payload
- Big-endian (network byte order) for all multi-byte fields
- Frame type field: 0x01 = PNG, other values reserved for future use (heartbeat, control)
- u32 length prefix — max ~4 GB per frame, more than enough for screenshots

### Frame & protocol types
- `DisplayEnvironment` enum defines all variants upfront: Wayland, X11, Xvfb, Headless — stable across all phases
- `Frame` struct carries serialization methods (`to_bytes()` / `from_bytes()`) — self-contained, protocol logic lives with the type
- Custom error enum for frame parsing (e.g., `FrameError::InvalidMagic`, `FrameError::UnsupportedVersion`, `FrameError::TooLarge`)

### CLI experience
- Terse, technical tone — like ripgrep or fd. Short help text, errors show what failed and why, nothing extra
- Phase 1 stubs: `cssh local` and `cssh remote` print resolved config and exit cleanly
- Config via CLI flags with environment variable fallbacks (e.g., `--port` / `CSSH_PORT`). No config file for v1
- Default port: random high port in unregistered range (Claude picks specific value)

### Project structure
- Single crate with domain-based flat modules: `src/protocol.rs`, `src/cli.rs`, `src/transport.rs`, `src/clipboard.rs`, `src/display.rs`
- Rust 2021 edition, no MSRV policy
- Tokio async runtime

### Claude's Discretion
- Frame metadata content (whether to include timestamp, dimensions, content hash alongside PNG bytes — or keep payload as raw PNG only)
- Specific default port number (high unregistered range)
- Exact magic byte values
- Internal error handling strategy beyond the protocol layer

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

*Phase: 01-foundation*
*Context gathered: 2026-02-27*
