# Phase 2: Transport - Research

**Researched:** 2026-02-27
**Domain:** Tokio async TCP — framing, keepalive, write timeout, reconnection with exponential backoff
**Confidence:** HIGH

## Summary

Phase 2 wires up the full TCP transport layer: a listener on the remote side and a connecting client with auto-reconnect on the local side. The framing protocol (Frame struct, magic bytes, length-prefix) was built in Phase 1. Phase 2's job is to stream those frames reliably across a TCP connection, detect dead connections quickly, and reconnect automatically.

The project already uses Tokio (version 1, features = ["full"]) and thiserror, so the async runtime choice is settled. The transport implementation is straightforward Tokio I/O: `TcpListener::bind`, `TcpStream::connect`, `AsyncReadExt::read_exact`, `AsyncWriteExt::write_all`, and `tokio::time::timeout` for write deadlines. TCP keepalive configuration requires the `socket2` crate (Tokio's `TcpStream` exposes no native keepalive API). Exponential backoff with jitter is a handful of lines using `rand::thread_rng()` — no external retry crate needed. Logging uses `tracing` + `tracing-subscriber` with `EnvFilter` keyed to `RUST_LOG`.

**Primary recommendation:** Use raw Tokio I/O primitives directly — no codec/framing library needed since the protocol is already implemented. Add `socket2` for keepalive and `rand` for jitter. Inline the backoff loop rather than reaching for a retry crate.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

- Remote address specified via `--remote` CLI flag on `cssh local` (no config file)
- Default port: 9877 (override with `--remote host:port`)
- `cssh remote` binds to Tailscale interface only — not exposed on other networks
- Named flag `--remote` rather than positional argument
- TCP keepalive detects silently dropped connections within ~30 seconds
- Write operations timeout after 10 seconds — treat as dead connection, trigger reconnect
- All timeout values hardcoded — no configurable flags
- Silent by default — no output unless RUST_LOG is set
- Verbosity controlled via standard RUST_LOG env var (tracing/env_logger)
- Human-readable log format (e.g. `INFO cssh::transport: connected to 100.x.x.x:9877`)
- Connection state changes (connected/disconnected/reconnecting) logged at WARN level
- Retry forever — daemon never gives up
- Exponential backoff: start at 1s, double each attempt, cap at 30s
- Jitter: random 0-25% added to each backoff interval
- Backoff resets immediately on successful reconnection

### Claude's Discretion

- Async runtime choice (tokio vs std threads) — Tokio already in Cargo.toml, use it
- Internal connection state machine design
- TCP socket options beyond keepalive and write timeout
- Error types and internal error handling patterns

### Deferred Ideas (OUT OF SCOPE)

None — discussion stayed within phase scope
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| XPRT-01 | Local daemon connects to remote daemon over TCP via Tailscale | `TcpListener::bind(tailscale_ip:port)` on remote; `TcpStream::connect(remote:port)` on local with auto-reconnect loop |
| XPRT-03 | Connection automatically reconnects with exponential backoff on loss | Inline backoff loop with `rand::thread_rng().gen_range()`; jitter is 0–25% of current interval; `tokio::time::sleep` for wait |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| tokio | 1 (already in Cargo.toml) | Async runtime, TcpListener, TcpStream, time::timeout, time::sleep | Already chosen; full-featured async I/O |
| socket2 | 0.5 | TCP keepalive configuration (TcpKeepalive struct + SockRef) | Tokio's TcpStream has no native keepalive API; socket2 is the standard solution |
| rand | 0.8 | Jitter calculation in backoff loop | Minimal dep; `thread_rng().gen_range()` is idiomatic |
| tracing | 0.1 | Structured logging macros (`info!`, `warn!`, `debug!`) | Already implied by CONTEXT.md decision; ecosystem standard |
| tracing-subscriber | 0.3 | EnvFilter (RUST_LOG), human-readable fmt output | Pairs with tracing; `EnvFilter::from_default_env()` reads RUST_LOG |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| thiserror | 2 (already in Cargo.toml) | Custom error types for transport layer | Define `TransportError` wrapping io::Error and protocol errors |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| socket2 for keepalive | tokio-io-timeout crate | socket2 is lower-level/more control; tokio-io-timeout easier but less precision |
| Inline backoff loop | tokio-retry2 or backoff crate | Inline is simpler for forever-retry with custom jitter; crates add complexity without benefit here |
| tracing + tracing-subscriber | env_logger | tracing is richer and already the standard for Tokio apps; supports future structured logging |

**Installation:**
```bash
cargo add socket2 rand tracing tracing-subscriber --features tracing-subscriber/env-filter
```

## Architecture Patterns

### Recommended Project Structure
```
src/
├── transport.rs        # All TCP transport logic (this phase)
│   ├── server()        # Remote: TcpListener loop, accept connections
│   ├── client()        # Local: connect + reconnect loop
│   ├── send_frame()    # Write a Frame to an OwnedWriteHalf with timeout
│   ├── recv_frame()    # Read a Frame from an OwnedReadHalf (header then payload)
│   └── keepalive()     # Configure socket2 keepalive on a TcpStream
├── protocol.rs         # Frame, FrameError (Phase 1, done)
├── cli.rs              # Args (needs --remote flag update)
└── main.rs             # Wire server/client into subcommands
```

### Pattern 1: Remote — Bind to Tailscale IP, Accept Loop

**What:** Resolve the Tailscale IP at runtime (via `tailscale ip -4` subprocess or hardcoded from `--bind` flag), bind to `tailscale_ip:port`, loop accepting connections, spawn a task per connection.

**When to use:** `cssh remote` subcommand startup.

```rust
// Source: https://docs.rs/tokio/latest/tokio/net/struct.TcpListener.html
use tokio::net::TcpListener;

let listener = TcpListener::bind(format!("{}:{}", bind_addr, port)).await?;
tracing::info!("listening on {}", listener.local_addr()?);

loop {
    let (socket, peer) = listener.accept().await?;
    tracing::info!("accepted connection from {}", peer);
    apply_keepalive(&socket)?;
    tokio::spawn(handle_connection(socket));
}
```

**Binding to Tailscale interface only:** Pass the Tailscale IP (100.x.x.x) as bind address. The daemon will only accept connections arriving on that interface. Simpler and more portable than `SO_BINDTODEVICE` (Linux-only).

### Pattern 2: Local — Connect with Reconnect Loop

**What:** Loop forever: attempt connect, on success enter send loop, on any error log and backoff, then retry.

**When to use:** `cssh local` subcommand startup.

```rust
// Source: https://tokio.rs/tokio/tutorial/io
use tokio::net::TcpStream;
use tokio::time::{sleep, Duration};

let mut backoff_secs: f64 = 1.0;
const CAP_SECS: f64 = 30.0;

loop {
    match TcpStream::connect(&remote_addr).await {
        Ok(stream) => {
            tracing::warn!("connected to {}", remote_addr);
            apply_keepalive(&stream)?;
            backoff_secs = 1.0; // reset on success
            if let Err(e) = run_send_loop(stream, &mut frame_rx).await {
                tracing::warn!("connection lost: {}", e);
            }
        }
        Err(e) => {
            tracing::warn!("connect failed: {}; retrying in {:.1}s", e, backoff_secs);
        }
    }
    let jitter = rand::thread_rng().gen_range(0.0..backoff_secs * 0.25);
    sleep(Duration::from_secs_f64(backoff_secs + jitter)).await;
    backoff_secs = (backoff_secs * 2.0).min(CAP_SECS);
}
```

### Pattern 3: Frame Send with Write Timeout

**What:** Wrap `write_all` with `tokio::time::timeout`. On timeout, treat as dead connection and return error to trigger reconnect.

**When to use:** Inside the send loop every time a frame is written.

```rust
// Source: https://docs.rs/tokio/latest/tokio/time/fn.timeout.html
use tokio::time::{timeout, Duration};
use tokio::io::AsyncWriteExt;

async fn send_frame(writer: &mut OwnedWriteHalf, frame: &Frame) -> Result<(), TransportError> {
    let bytes = frame.to_bytes()?;
    timeout(Duration::from_secs(10), writer.write_all(&bytes))
        .await
        .map_err(|_| TransportError::WriteTimeout)?
        .map_err(TransportError::Io)
}
```

### Pattern 4: Frame Receive (read_exact for header, then payload)

**What:** Read the fixed 8-byte header with `read_exact`, parse payload length, then `read_exact` the payload. This is safe because the protocol has a fixed-size header.

**When to use:** Inside the receive loop on the remote side.

```rust
// Source: https://docs.rs/tokio/latest/tokio/net/struct.TcpStream.html
use tokio::io::AsyncReadExt;

async fn recv_frame(reader: &mut OwnedReadHalf) -> Result<Frame, TransportError> {
    let mut header = [0u8; 8]; // HEADER_LEN from protocol.rs
    reader.read_exact(&mut header).await
        .map_err(|e| if e.kind() == io::ErrorKind::UnexpectedEof {
            TransportError::ConnectionClosed
        } else {
            TransportError::Io(e)
        })?;
    let payload_len = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;
    let mut payload = vec![0u8; payload_len];
    reader.read_exact(&mut payload).await.map_err(TransportError::Io)?;
    // Combine and parse via Frame::from_bytes
    let mut full = header.to_vec();
    full.extend_from_slice(&payload);
    Frame::from_bytes(&full).map_err(TransportError::Frame)
}
```

### Pattern 5: TCP Keepalive via socket2

**What:** After accepting or establishing a connection, configure keepalive so a silently dropped connection is detected within ~30 seconds.

**When to use:** Apply to every TcpStream immediately after connect/accept.

```rust
// Source: https://docs.rs/socket2/latest/socket2/struct.TcpKeepalive.html
use socket2::{SockRef, TcpKeepalive};
use std::time::Duration;

fn apply_keepalive(stream: &tokio::net::TcpStream) -> Result<(), io::Error> {
    let ka = TcpKeepalive::new()
        .with_time(Duration::from_secs(10))      // idle before probes start
        .with_interval(Duration::from_secs(5))   // between probes
        .with_retries(3);                        // probes before giving up
    let sock_ref = SockRef::from(stream);
    sock_ref.set_tcp_keepalive(&ka)
}
```

With `time=10s`, `interval=5s`, `retries=3`: detection happens within 10 + 3*5 = 25 seconds — well within the 30-second window.

### Pattern 6: Stream Splitting for Concurrent Read+Write

**What:** Use `TcpStream::into_split()` to get `OwnedReadHalf` and `OwnedWriteHalf`. These can be moved to separate tokio tasks.

**When to use:** If the remote side needs concurrent read and write on the same connection (Phase 2 is send-only in one direction, so splitting may not be needed yet — but use it if the architecture calls for it).

```rust
// Source: https://docs.rs/tokio/latest/tokio/net/tcp/struct.OwnedReadHalf.html
let (mut read_half, mut write_half) = stream.into_split();
// OwnedReadHalf: implements AsyncRead + AsyncReadExt (read_exact available)
// OwnedWriteHalf: implements AsyncWrite + AsyncWriteExt (write_all available)
```

Use `split()` (borrow-based, zero-cost) if both halves stay in the same task.
Use `into_split()` (Arc-based) if halves go to separate tasks.

### Pattern 7: Tracing / Logging Setup

```rust
// Source: https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/index.html
// In main() before subcommand dispatch:
tracing_subscriber::fmt()
    .with_env_filter(
        tracing_subscriber::EnvFilter::from_default_env()
    )
    .init();
```

This reads `RUST_LOG`. Silent by default (no output unless RUST_LOG is set). With `RUST_LOG=info`, human-readable lines like `INFO cssh::transport: connected to 100.x.x.x:9877` appear. Connection state changes at `warn!` level will always be visible when `RUST_LOG=warn` or higher.

### Anti-Patterns to Avoid

- **Binding remote to 0.0.0.0:** Exposes the port on all network interfaces. Bind to the Tailscale IP (100.x.x.x) explicitly.
- **Not checking `read_exact` UnexpectedEof separately:** `read_exact` returns `UnexpectedEof` when the peer closes cleanly. This is a normal connection-closed signal, not a hard error. Must be matched and handled as `TransportError::ConnectionClosed` rather than an unexpected error.
- **Forgetting write timeout:** A TCP connection over Tailscale can be silently dropped (VM suspend, NAT timeout). Without a write timeout, `write_all` will hang indefinitely. The 10-second write timeout is mandatory.
- **Backoff without jitter:** Pure exponential backoff causes retry storms when the remote restarts and many clients reconnect simultaneously. Jitter spreads reconnects.
- **Using `split()` (borrow) and moving halves to separate tasks:** Won't compile — borrow split halves can't be `'static`. Use `into_split()` for cross-task use.
- **Using `io::split()` unnecessarily:** Adds `Arc<Mutex<_>>` overhead. Prefer `into_split()` for TcpStream specifically.
- **Not resetting backoff on reconnect:** If backoff is not reset on successful connection, the next reconnect after the first successful connection will start from the capped value.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| TCP keepalive config | Custom ioctl/setsockopt wrappers | socket2 `TcpKeepalive` + `SockRef` | socket2 is the official Rust abstraction; handles platform differences |
| Jitter backoff | Custom iterator | `rand::thread_rng().gen_range()` inline | 3 lines; no abstraction needed for a simple loop |
| Frame codec | Implement `Decoder`/`Encoder` for tokio-util | Direct `read_exact` + `write_all` | Protocol is already implemented as `Frame`; no codec layer needed |
| Write timeout | Wrapping stream in a custom type | `tokio::time::timeout(dur, write_all(...))` | One line; no wrapper needed |

**Key insight:** For a simple, hand-rolled framing protocol with a fixed header, `read_exact` + `write_all` is simpler and more debuggable than codec frameworks. Codec frameworks (tokio-util `Framed`) add value for complex, variable-format protocols.

## Common Pitfalls

### Pitfall 1: `read_exact` Hangs on Partial Write

**What goes wrong:** If a write timeout fires after writing part of a frame (e.g. header written, payload not), the connection is dropped. On the next connection (if reconnecting), the remote may try to parse the partial frame and hang in `read_exact` waiting for bytes that will never come.

**Why it happens:** TCP has no built-in message boundaries. A partial frame in flight leaves the protocol state machine in an ambiguous position.

**How to avoid:** On any write error or timeout, drop the connection immediately. On the remote side, drop the connection and treat a new `accept()` as a clean slate. Do not try to recover mid-frame state.

**Warning signs:** Remote-side `read_exact` hangs indefinitely; no timeout configured on the read path.

### Pitfall 2: `read_exact` Returns `UnexpectedEof` on Clean Close

**What goes wrong:** When the local side disconnects (process killed, reconnecting), the remote's `read_exact` returns `Err(UnexpectedEof)`. If this is treated as a hard error rather than a normal disconnect signal, the server task panics or logs spurious errors.

**Why it happens:** `read_exact` returning `UnexpectedEof` is how Tokio signals that the peer closed the connection before sending the expected bytes. It is not a bug.

**How to avoid:** Match `io::ErrorKind::UnexpectedEof` in the receive loop and treat it as `ConnectionClosed` — log at INFO and wait for the next `accept()`.

### Pitfall 3: No Write Timeout — Silent Connection Hang

**What goes wrong:** If Tailscale drops a connection silently (VM suspended, keepalive not fast enough), `write_all` never returns. The local daemon freezes indefinitely.

**Why it happens:** Tokio's `write_all` has no built-in deadline. The kernel buffers the bytes, the socket never signals error, and the future never resolves.

**How to avoid:** Always wrap `write_all` in `tokio::time::timeout(Duration::from_secs(10), ...)`. On `Elapsed`, treat as a dead connection and trigger reconnect.

### Pitfall 4: Binding Remote to Wrong Address

**What goes wrong:** Binding to `0.0.0.0` exposes the port on all interfaces including the public IP, not just Tailscale.

**Why it happens:** Easy default, but violates the security requirement.

**How to avoid:** Resolve the Tailscale IP at startup and bind to `tailscale_ip:port`. The simplest approach: require the user to configure `--bind` with their Tailscale IP, or run `tailscale ip -4` as a subprocess and parse the output.

### Pitfall 5: Port Conflict Between Phase 1 and Phase 2 Decisions

**What goes wrong:** The Phase 1 scaffold used port 34782. CONTEXT.md specifies port 9877 as the new default. The CLI's `default_value` must be updated in this phase.

**Why it happens:** Port was decided during Phase 1 planning (STATE.md: "[01-01]: Port 34782 chosen"), but CONTEXT.md for Phase 2 overrides this to 9877.

**How to avoid:** Phase 2 planning must include a task to update `cli.rs` default port from 34782 to 9877 to match CONTEXT.md.

### Pitfall 6: Backoff Not Resetting After Reconnect

**What goes wrong:** After a reconnect, backoff stays at 30s. The next disconnection waits 30s even if it's a transient failure.

**Why it happens:** Forgetting to reset `backoff_secs = 1.0` when connect succeeds.

**How to avoid:** Immediately set `backoff_secs = 1.0` at the top of the `Ok(stream)` branch before entering the send loop.

## Code Examples

### Complete Keepalive Setup (verified pattern)
```rust
// Source: https://docs.rs/socket2/latest/socket2/struct.TcpKeepalive.html
use socket2::{SockRef, TcpKeepalive};
use std::io;
use std::time::Duration;

fn apply_keepalive(stream: &tokio::net::TcpStream) -> io::Result<()> {
    let ka = TcpKeepalive::new()
        .with_time(Duration::from_secs(10))
        .with_interval(Duration::from_secs(5))
        .with_retries(3);
    SockRef::from(stream).set_tcp_keepalive(&ka)
}
```

### Write with Timeout (verified pattern)
```rust
// Source: https://docs.rs/tokio/latest/tokio/time/fn.timeout.html
use tokio::time::{timeout, Duration};
use tokio::io::AsyncWriteExt;

async fn timed_write_all(
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    buf: &[u8],
) -> Result<(), TransportError> {
    timeout(Duration::from_secs(10), writer.write_all(buf))
        .await
        .map_err(|_| TransportError::WriteTimeout)? // Elapsed
        .map_err(TransportError::Io)                // io::Error
}
```

### Jitter Backoff (inline, no crate)
```rust
// Source: https://oneuptime.com/blog/post/2026-01-25-exponential-backoff-jitter-rust/view
use rand::Rng;

let mut backoff: f64 = 1.0;
const CAP: f64 = 30.0;

// After each failed attempt:
let jitter = rand::thread_rng().gen_range(0.0..backoff * 0.25);
tokio::time::sleep(Duration::from_secs_f64(backoff + jitter)).await;
backoff = (backoff * 2.0).min(CAP);

// On successful connection:
backoff = 1.0;
```

### tracing-subscriber Init
```rust
// Source: https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/index.html
tracing_subscriber::fmt()
    .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
    .init();
```

### Frame Receive (read_exact pattern)
```rust
// Source: https://docs.rs/tokio/latest/tokio/net/struct.TcpStream.html
use tokio::io::AsyncReadExt;
use std::io;

async fn recv_frame(
    reader: &mut tokio::net::tcp::OwnedReadHalf,
) -> Result<Frame, TransportError> {
    const HEADER_LEN: usize = 8;
    let mut header = [0u8; HEADER_LEN];

    match reader.read_exact(&mut header).await {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
            return Err(TransportError::ConnectionClosed);
        }
        Err(e) => return Err(TransportError::Io(e)),
    }

    let payload_len =
        u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;
    let mut payload = vec![0u8; payload_len];
    reader.read_exact(&mut payload).await.map_err(TransportError::Io)?;

    let mut full = header.to_vec();
    full.extend_from_slice(&payload);
    Frame::from_bytes(&full).map_err(TransportError::Frame)
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `tokio::io::split()` (Arc+Mutex) for cross-task use | `TcpStream::into_split()` (Arc only, no Mutex) | Tokio 1.x | Lower overhead for owned halves |
| Manual keepalive via unsafe setsockopt | `socket2::SockRef::from(&stream).set_tcp_keepalive()` | socket2 0.4+ | Safe, ergonomic, cross-platform |
| `env_logger` for RUST_LOG | `tracing-subscriber` with `EnvFilter` | ~2021 onward | Structured spans + events; async-aware |

**Deprecated/outdated:**
- `tokio::io::split()` for TcpStream: Still works but use `into_split()` for TcpStream specifically — no Mutex overhead
- `tokio_core::net::TcpStream`: Old tokio 0.x API; completely superseded

## Open Questions

1. **How does `cssh remote` discover its Tailscale IP to bind to?**
   - What we know: Must bind to Tailscale IP (100.x.x.x), not 0.0.0.0
   - What's unclear: Should this be a required `--bind` flag, or discovered automatically via `tailscale ip -4` subprocess?
   - Recommendation: Add optional `--bind` flag with sensible error message if no Tailscale interface found; auto-detect via `std::process::Command::new("tailscale").arg("ip").arg("-4")` as fallback

2. **Port default: 9877 (CONTEXT.md) vs 34782 (existing cli.rs)**
   - What we know: CONTEXT.md says 9877 is the new default. cli.rs currently has 34782.
   - What's unclear: Was 34782 a placeholder? Yes — STATE.md says it was chosen in Phase 1 as a high unregistered port to avoid conflicts.
   - Recommendation: Phase 2 planning must include a task to update the default port to 9877 in cli.rs.

3. **Does Phase 2 need a receiver on the remote side, or just plumbing?**
   - What we know: Success criteria say "a test payload sent from local arrives intact on remote"
   - What's unclear: Is the remote receiver just printing/logging frames (stub), or calling into clipboard code?
   - Recommendation: Remote receiver in Phase 2 should be a stub that logs received frames. Clipboard integration is Phase 3.

## Sources

### Primary (HIGH confidence)
- https://docs.rs/tokio/latest/tokio/net/struct.TcpStream.html — TcpStream API, available socket options
- https://docs.rs/tokio/latest/tokio/net/struct.TcpSocket.html — TcpSocket for pre-connect config
- https://docs.rs/tokio/latest/tokio/net/tcp/struct.OwnedReadHalf.html — OwnedReadHalf API
- https://docs.rs/tokio/latest/tokio/net/tcp/struct.OwnedWriteHalf.html — OwnedWriteHalf + write_all
- https://docs.rs/tokio/latest/tokio/time/fn.timeout.html — timeout function signature and semantics
- https://docs.rs/socket2/latest/socket2/struct.TcpKeepalive.html — TcpKeepalive API, platform support
- https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/index.html — fmt subscriber + EnvFilter setup
- https://tokio.rs/tokio/tutorial/io — read_exact, write_all, stream splitting patterns

### Secondary (MEDIUM confidence)
- https://oneuptime.com/blog/post/2026-01-25-exponential-backoff-jitter-rust/view — jitter implementation with rand (Jan 2026)
- https://docs.rs/tokio-retry2/latest/tokio_retry2/ — ExponentialBackoff with jitter (verified against crate docs)
- https://users.rust-lang.org/t/tokio-asyncreadext-read-exact-method-hangs-indefinately/123618 — read_exact hang pitfall (community verified)

### Tertiary (LOW confidence)
- None identified — all key claims verified against official docs

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all crates verified against official docs.rs
- Architecture: HIGH — patterns derived from official Tokio tutorial and docs
- Pitfalls: HIGH — read_exact UnexpectedEof and write timeout behavior verified from official sources and community discussion

**Research date:** 2026-02-27
**Valid until:** 2026-03-29 (30 days — tokio and socket2 are stable APIs)
