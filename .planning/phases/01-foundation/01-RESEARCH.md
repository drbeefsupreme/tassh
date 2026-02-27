# Phase 1: Foundation - Research

**Researched:** 2026-02-27
**Domain:** Rust CLI binary structure, custom wire-protocol framing, unit testing with in-memory I/O
**Confidence:** HIGH

---

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**Wire format:**
- Header: 2-byte magic + 1-byte version + 1-byte frame type + u32 length (big-endian) + payload
- Big-endian (network byte order) for all multi-byte fields
- Frame type field: 0x01 = PNG, other values reserved for future use (heartbeat, control)
- u32 length prefix — max ~4 GB per frame, more than enough for screenshots

**Frame & protocol types:**
- `DisplayEnvironment` enum defines all variants upfront: Wayland, X11, Xvfb, Headless — stable across all phases
- `Frame` struct carries serialization methods (`to_bytes()` / `from_bytes()`) — self-contained, protocol logic lives with the type
- Custom error enum for frame parsing (e.g., `FrameError::InvalidMagic`, `FrameError::UnsupportedVersion`, `FrameError::TooLarge`)

**CLI experience:**
- Terse, technical tone — like ripgrep or fd. Short help text, errors show what failed and why, nothing extra
- Phase 1 stubs: `cssh local` and `cssh remote` print resolved config and exit cleanly
- Config via CLI flags with environment variable fallbacks (e.g., `--port` / `CSSH_PORT`). No config file for v1
- Default port: random high port in unregistered range (Claude picks specific value)

**Project structure:**
- Single crate with domain-based flat modules: `src/protocol.rs`, `src/cli.rs`, `src/transport.rs`, `src/clipboard.rs`, `src/display.rs`
- Rust 2021 edition, no MSRV policy
- Tokio async runtime

### Claude's Discretion

- Frame metadata content (whether to include timestamp, dimensions, content hash alongside PNG bytes — or keep payload as raw PNG only)
- Specific default port number (high unregistered range)
- Exact magic byte values
- Internal error handling strategy beyond the protocol layer

### Deferred Ideas (OUT OF SCOPE)

None — discussion stayed within phase scope
</user_constraints>

---

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| SRVC-01 | Single binary with `cssh local`, `cssh remote`, `cssh status` subcommands | clap 4 derive macro: `#[derive(Parser)]` on root struct + `#[derive(Subcommand)]` enum; Phase 1 only needs `local` and `remote` stubs |
| XPRT-02 | Images are framed as length-prefixed PNG payloads | Custom header (2-byte magic + 1-byte version + 1-byte type + u32 BE length) + payload; serialize/deserialize with stdlib `to_be_bytes()` / `from_be_bytes()`; test with `io::Cursor` |
</phase_requirements>

---

## Summary

Phase 1 is a pure scaffolding and framing phase. There is no networking, no system calls, no display management. The deliverables are: a `cargo new --bin` project that compiles with two CLI subcommand stubs and a protocol module containing `Frame`, `DisplayEnvironment`, and `FrameError` — verified by a unit test that writes and reads back a framed PNG payload byte-for-byte.

The technical domain has two independent parts: (1) CLI structure with clap 4's derive API and environment variable fallbacks, and (2) binary framing with pure-stdlib big-endian I/O. Both are well-understood, stable, and require no unusual dependencies beyond clap, thiserror, and tokio.

The key architectural decision is that `Frame::to_bytes()` / `Frame::from_bytes()` are synchronous and work on `Vec<u8>` / `&[u8]` slices — they are not async and have no I/O. This keeps the protocol module testable in isolation. Async I/O (feeding frames into a `TcpStream`) is Phase 2.

**Primary recommendation:** Scaffold with `cargo new --bin cssh`, add clap 4 + thiserror + tokio, define `src/protocol.rs` with Frame/FrameError/DisplayEnvironment, and add a `#[test]` round-trip in the same file. Do not introduce any I/O adapter layer in Phase 1.

---

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| clap | 4.5.x | CLI argument parsing and subcommand dispatch | De facto standard for Rust CLIs; derive API generates help text automatically; `env` feature handles `CSSH_PORT`-style fallbacks |
| thiserror | 2.0.x | Derive `std::error::Error` on `FrameError` | Zero-cost abstraction; doesn't appear in public API; generates `Display` from `#[error("...")]` attributes; correct choice for library-like protocol code |
| tokio | 1.49.x | Async runtime | Locked decision; `#[tokio::main]` macro needed even for stubs so the binary is async-ready from the start |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| std::io::Cursor | stdlib | In-memory reader/writer for unit tests | Used in the framing round-trip test instead of a real file or socket |
| u32::to_be_bytes() / from_be_bytes() | stdlib | Big-endian encoding of the length field | Preferred over `byteorder` crate for simple single-field reads — stdlib since Rust 1.32 |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| thiserror | anyhow | anyhow is for application error propagation, not typed protocol errors; `FrameError::InvalidMagic` needs to be matchable by callers |
| stdlib BE methods | byteorder crate | byteorder adds a dependency and ergonomic trait methods; stdlib suffices for a 4-byte header; use byteorder only if reading many fields gets verbose |
| clap derive | clap builder API | Builder is more verbose; derive is idiomatic for new Rust CLI projects in 2025 |

**Installation:**
```toml
[dependencies]
clap = { version = "4", features = ["derive", "env"] }
thiserror = "2"
tokio = { version = "1", features = ["full"] }
```

---

## Architecture Patterns

### Recommended Project Structure

```
cssh/
├── Cargo.toml
└── src/
    ├── main.rs          # #[tokio::main], clap parse, subcommand dispatch
    ├── cli.rs           # Cli struct, Commands enum, per-subcommand arg structs
    ├── protocol.rs      # Frame, FrameError, DisplayEnvironment — NO I/O, pure data
    ├── transport.rs     # (stub) future TCP framing adapter
    ├── clipboard.rs     # (stub) future clipboard read/write
    └── display.rs       # (stub) future display env detection
```

Stub modules (`transport.rs`, `clipboard.rs`, `display.rs`) are created as empty files with a module comment and a `#![allow(dead_code)]` attribute to satisfy `cargo build` with no warnings.

### Pattern 1: Clap Subcommand Dispatch

**What:** Root `Cli` struct with `#[command(subcommand)]` field; `Commands` enum variants map 1:1 to subcommands; each variant optionally holds a struct with its flags.

**When to use:** Any time a binary has two or more distinct modes of operation.

**Example:**
```rust
// Source: https://docs.rs/clap/latest/clap/_derive/_tutorial/index.html
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "cssh", about = "clipboard screenshot relay", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run as local daemon (watches clipboard, sends frames)
    Local(LocalArgs),
    /// Run as remote daemon (receives frames, writes clipboard)
    Remote(RemoteArgs),
}

#[derive(Parser)]
struct LocalArgs {
    #[arg(long, env = "CSSH_REMOTE_HOST")]
    remote_host: String,

    #[arg(long, env = "CSSH_PORT", default_value = "34782")]
    port: u16,
}

#[derive(Parser)]
struct RemoteArgs {
    #[arg(long, env = "CSSH_PORT", default_value = "34782")]
    port: u16,
}
```

### Pattern 2: Frame as Self-Contained Serializable Type

**What:** `Frame` owns its payload as `Vec<u8>` and exposes `to_bytes(&self) -> Vec<u8>` and `from_bytes(data: &[u8]) -> Result<Frame, FrameError>`. No async, no I/O. The transport layer in Phase 2 wraps these.

**When to use:** Whenever a protocol type needs to cross a crate/module boundary — keeping serde logic inside the type makes testing trivial.

**Example:**
```rust
// Source: design derived from stdlib + project decisions in CONTEXT.md
const MAGIC: [u8; 2] = [0xCS, 0x53]; // "CS" = cssh; Claude to pick exact bytes
const VERSION: u8 = 1;
const FRAME_TYPE_PNG: u8 = 0x01;

pub struct Frame {
    pub frame_type: u8,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn to_bytes(&self) -> Vec<u8> {
        let len = self.payload.len() as u32;
        let mut buf = Vec::with_capacity(8 + self.payload.len());
        buf.extend_from_slice(&MAGIC);
        buf.push(VERSION);
        buf.push(self.frame_type);
        buf.extend_from_slice(&len.to_be_bytes());
        buf.extend_from_slice(&self.payload);
        buf
    }

    pub fn from_bytes(data: &[u8]) -> Result<Frame, FrameError> {
        if data.len() < 8 {
            return Err(FrameError::TooShort);
        }
        if &data[0..2] != MAGIC {
            return Err(FrameError::InvalidMagic);
        }
        if data[2] != VERSION {
            return Err(FrameError::UnsupportedVersion(data[2]));
        }
        let frame_type = data[3];
        let len = u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize;
        if data.len() < 8 + len {
            return Err(FrameError::TooShort);
        }
        Ok(Frame {
            frame_type,
            payload: data[8..8 + len].to_vec(),
        })
    }
}
```

### Pattern 3: thiserror for Protocol Errors

**What:** A dedicated `FrameError` enum with `#[derive(Debug, thiserror::Error)]` and `#[error("...")]` on each variant.

**Example:**
```rust
// Source: https://docs.rs/thiserror/latest/thiserror/
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("invalid magic bytes")]
    InvalidMagic,
    #[error("unsupported protocol version: {0}")]
    UnsupportedVersion(u8),
    #[error("frame data too short to be valid")]
    TooShort,
    #[error("frame payload exceeds maximum size")]
    TooLarge,
}
```

### Pattern 4: Unit Test with io::Cursor

**What:** Use `std::io::Cursor<Vec<u8>>` as an in-memory reader/writer to exercise frame round-trips without touching the filesystem.

**Example:**
```rust
// Source: https://doc.rust-lang.org/std/io/struct.Cursor.html
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_round_trip_png() {
        let payload = vec![0x89, 0x50, 0x4E, 0x47]; // PNG magic bytes
        let frame = Frame { frame_type: FRAME_TYPE_PNG, payload: payload.clone() };
        let bytes = frame.to_bytes();
        let decoded = Frame::from_bytes(&bytes).expect("round-trip failed");
        assert_eq!(decoded.frame_type, FRAME_TYPE_PNG);
        assert_eq!(decoded.payload, payload);
    }
}
```

### Anti-Patterns to Avoid

- **Async frame parsing:** `from_bytes` must not be async. Async I/O belongs in the transport layer (Phase 2), not in the data type.
- **Skipping stub modules:** Creating only `protocol.rs` and `cli.rs` and leaving out `transport.rs`/`clipboard.rs`/`display.rs` will require adding them in Phase 2 with possible module path churn. Create empty stubs now.
- **`features = ["full"]` only in binary:** `tokio = { version = "1", features = ["full"] }` is fine for a single-binary project with no lib crate. Don't add `tokio` to dev-dependencies separately.
- **`#[allow(unused)]` on the entire crate:** Use it per stub module or per item, not crate-wide.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| CLI argument parsing with help text | Manual `std::env::args()` parsing | clap 4 derive | Auto-generates `--help`, `--version`, error messages; env fallback via `#[arg(env)]` |
| Error type boilerplate | `impl Display for FrameError { ... }` by hand | thiserror `#[error("...")]` | 10+ lines of boilerplate per error variant; thiserror generates it correctly every time |
| Subcommand routing logic | `if args[1] == "local" { ... }` | clap `#[derive(Subcommand)]` | Type-safe match; help text per subcommand; unknown subcommand error handling |

**Key insight:** The stdlib provides everything needed for the binary framing logic (`to_be_bytes`, `from_be_bytes`, slicing). Don't add byteorder unless reading a complex multi-field header becomes verbose.

---

## Common Pitfalls

### Pitfall 1: clap `env` Feature Not Enabled

**What goes wrong:** `#[arg(env = "CSSH_PORT")]` compiles but silently ignores the env var, or compilation fails with a cryptic error about a missing method.

**Why it happens:** The `env` feature in clap is opt-in. `features = ["derive"]` alone is insufficient.

**How to avoid:** Use `features = ["derive", "env"]` in Cargo.toml from the start.

**Warning signs:** `cargo test` passes but `CSSH_PORT=9999 cssh local` shows the default port.

### Pitfall 2: Signed/Unsigned Mismatch on Length Field

**What goes wrong:** `payload.len()` is `usize` (64-bit on amd64), cast to `u32` silently truncates for payloads > 4 GB.

**Why it happens:** Rust does not warn on `as u32` truncation by default.

**How to avoid:** Add an explicit size guard in `to_bytes`: `assert!(self.payload.len() <= u32::MAX as usize)` or return a `FrameError::TooLarge` from a fallible `try_to_bytes`.

**Warning signs:** No compiler warning, test passes with small payloads, fails silently on large ones.

### Pitfall 3: `cargo build` Warns on Unused Stub Modules

**What goes wrong:** Empty stub files (`transport.rs`, `clipboard.rs`, `display.rs`) with no public items produce `dead_code` warnings, failing the "no warnings" success criterion.

**Why it happens:** Rust warns on any declared-but-unused items.

**How to avoid:** Add `#![allow(dead_code)]` at the top of each stub module, or add a placeholder public type (`pub struct Transport;`) to anchor the module.

**Warning signs:** `cargo build` output contains `warning: module ... is never used`.

### Pitfall 4: `#[tokio::main]` on a Synchronous Stub

**What goes wrong:** Phase 1 stubs do no async work, but `#[tokio::main]` requires `async fn main()`. If the developer writes `fn main()` without `async`, it won't compile.

**Why it happens:** The locked decision is to use Tokio from the start — the function signature must be `async fn main()`.

**How to avoid:** Always write `async fn main()` and annotate with `#[tokio::main]`. The stub body can be entirely synchronous code inside an async fn.

**Warning signs:** `error[E0308]: mismatched types` at `#[tokio::main]`.

### Pitfall 5: Magic Bytes Conflict with Valid UTF-8

**What goes wrong:** Choosing magic bytes that form valid UTF-8 makes it harder to detect accidental text data in the stream.

**Why it happens:** Aesthetic bias toward readable ASCII.

**How to avoid:** Use bytes above 0x7F for at least one of the two magic bytes (e.g., `[0xC5, 0x53]`). This guarantees the header can never be mistaken for ASCII text.

---

## Code Examples

Verified patterns from official sources:

### Cargo.toml Dependencies

```toml
# Source: https://crates.io/crates/clap, https://crates.io/crates/thiserror, https://crates.io/crates/tokio
[package]
name = "cssh"
version = "0.1.0"
edition = "2021"

[dependencies]
clap = { version = "4", features = ["derive", "env"] }
thiserror = "2"
tokio = { version = "1", features = ["full"] }
```

### main.rs Entry Point

```rust
// Source: https://tokio.rs/tokio/tutorial/hello-tokio
// Source: https://docs.rs/clap/latest/clap/_derive/_tutorial/index.html
use clap::Parser;

mod cli;
mod clipboard;
mod display;
mod protocol;
mod transport;

#[tokio::main]
async fn main() {
    let cli = cli::Cli::parse();
    match cli.command {
        cli::Commands::Local(args) => {
            println!("cssh local: remote={} port={}", args.remote_host, args.port);
        }
        cli::Commands::Remote(args) => {
            println!("cssh remote: port={}", args.port);
        }
    }
}
```

### DisplayEnvironment Enum

```rust
// src/protocol.rs
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisplayEnvironment {
    Wayland,
    X11,
    Xvfb,
    Headless,
}
```

### Frame Round-Trip Unit Test Pattern

```rust
// Source: https://doc.rust-lang.org/std/io/struct.Cursor.html
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_png_frame() {
        // Minimal 1x1 white PNG (37 bytes) — real PNG magic header
        let payload: Vec<u8> = vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A,
        ];
        let frame = Frame::new_png(payload.clone());
        let encoded = frame.to_bytes();

        // to_bytes output is: 2 magic + 1 version + 1 type + 4 length + N payload
        assert_eq!(encoded.len(), 8 + payload.len());

        let decoded = Frame::from_bytes(&encoded).expect("decode failed");
        assert_eq!(decoded.frame_type, FRAME_TYPE_PNG);
        assert_eq!(decoded.payload, payload, "payload must be byte-perfect");
    }

    #[test]
    fn rejects_invalid_magic() {
        let bad = vec![0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x04, 0x01, 0x02, 0x03, 0x04];
        assert!(matches!(Frame::from_bytes(&bad), Err(FrameError::InvalidMagic)));
    }

    #[test]
    fn rejects_unsupported_version() {
        let mut bytes = Frame::new_png(vec![1, 2, 3, 4]).to_bytes();
        bytes[2] = 0xFF; // corrupt version byte
        assert!(matches!(Frame::from_bytes(&bytes), Err(FrameError::UnsupportedVersion(0xFF))));
    }
}
```

---

## Discretion Recommendations

These are areas left to Claude's discretion by the user.

### Default Port Number

**Recommendation: `34782`**

Rationale: Falls in the IANA unregistered dynamic port range (49152–65535 is "private", but 1024–49151 contains many unregistered gaps). Port 34782 does not appear in the IANA service name registry and is not associated with any known software. Alternately, the full private range starts at 49152; `49201` is also a clean choice. Either works — pick one and hardcode it as the default.

### Magic Bytes

**Recommendation: `[0xC5, 0x53]`**

Rationale: `0xC5` is a non-ASCII byte (above 0x7F), ensuring the header can never be mistaken for ASCII or valid UTF-8 text. `0x53` is the ASCII code for 'S' (for "screenshot" or "ssh"). Together they form a distinctive 2-byte sequence with no known conflicts.

### Frame Metadata

**Recommendation: Keep payload as raw PNG bytes only (no timestamp, dimensions, or hash in Phase 1)**

Rationale: The locked wire format is already fixed at `magic + version + type + u32_length + payload`. Adding metadata would require a new frame type or embedding metadata inside the PNG payload, both of which are Phase 2+ concerns. The `Frame` struct should have no metadata fields in Phase 1.

### Internal Error Handling Beyond Protocol Layer

**Recommendation: Use `anyhow::Result` in `main.rs` for application-level errors, and typed `FrameError` in `protocol.rs` for protocol-level errors**

Rationale: This is the standard Rust error handling split — thiserror for library-like code (protocol), anyhow for application code (main). However, since anyhow is an additional dependency and Phase 1 main.rs is a stub, it is acceptable to use `eprintln!` + `std::process::exit(1)` in Phase 1 and defer anyhow to Phase 2 when real error propagation is needed.

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| clap 3 builder API | clap 4 derive API | 2022 | Derive is now idiomatic; builder still works but verbose |
| thiserror 1.x | thiserror 2.x | 2024 | thiserror 2 supports `#[error(transparent)]` more broadly; API is backward-compatible |
| tokio 0.x | tokio 1.x (stable) | 2021 | tokio 1 is stable and has LTS releases; `features = ["full"]` is safe for binary crates |
| `byteorder` crate for BE reads | `u32::from_be_bytes()` stdlib | Rust 1.32 (2018) | No external dependency needed for simple fixed-width integer parsing |

**Deprecated/outdated:**
- `structopt`: Merged into clap 3+ as the derive API; do not use `structopt` in new projects
- `failure` crate: Replaced by `thiserror` + `anyhow`; do not use
- `byteorder` for simple reads: Still maintained but not needed when stdlib suffices

---

## Open Questions

1. **Exact magic bytes — finalize before code is written**
   - What we know: Must be 2 bytes, big-endian order, not valid ASCII text
   - What's unclear: Whether to choose something "meaningful" (e.g., initials of project) or purely random
   - Recommendation: Use `[0xC5, 0x53]` as recommended above; finalize in Wave 0 before `protocol.rs` is written

2. **Default port — confirm no conflict with Tailscale or local services**
   - What we know: 34782 is not in the IANA registry
   - What's unclear: Whether the developer's Tailscale setup uses any ports in this range
   - Recommendation: Default to 34782; document that `--port` / `CSSH_PORT` overrides it

3. **`cssh status` subcommand — include in Phase 1 stub or defer?**
   - What we know: SRVC-01 requires `cssh local`, `cssh remote`, `cssh status`; Phase 1 success criteria mention only `local` and `remote`
   - What's unclear: Whether adding a `status` stub now (vs. Phase 4) causes any harm
   - Recommendation: Add `status` as a stub variant in `Commands` in Phase 1 (zero cost, avoids a refactor in Phase 4). Print "status: not yet implemented" and exit 0.

---

## Sources

### Primary (HIGH confidence)

- [clap docs.rs derive tutorial](https://docs.rs/clap/latest/clap/_derive/_tutorial/index.html) — derive macro structure, `#[command(subcommand)]`, Parser/Subcommand pattern
- [thiserror docs.rs](https://docs.rs/thiserror/latest/thiserror/) — `#[derive(Error)]`, `#[error("...")]`, `#[from]` semantics
- [std::io::Cursor docs](https://doc.rust-lang.org/std/io/struct.Cursor.html) — in-memory reader/writer for unit tests
- [tokio.rs tutorial](https://tokio.rs/tokio/tutorial/hello-tokio) — `#[tokio::main]` usage
- [tokio docs.rs runtime](https://docs.rs/tokio/latest/tokio/runtime/index.html) — runtime flavors and macro behavior

### Secondary (MEDIUM confidence)

- [rust.code-maven.com clap env vars](https://rust.code-maven.com/clap/clap-and-environment-variables.html) — `features = ["env"]` requirement verified against official clap crate feature list
- [crates.io clap](https://crates.io/crates/clap) — version 4.5.x confirmed current; 4.5.60 as of search date
- [crates.io thiserror](https://crates.io/crates/thiserror) — version 2.0.17 confirmed current
- [crates.io tokio](https://crates.io/crates/tokio) — version 1.49.0 confirmed current

### Tertiary (LOW confidence)

- [thepacketgeek.com custom protocol](https://thepacketgeek.com/rust/tcpstream/create-a-protocol/) — general protocol framing patterns in Rust; not verified against official docs but consistent with stdlib approach

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — versions confirmed from crates.io; derive API verified from official docs
- Architecture: HIGH — patterns derived from locked user decisions + official clap/tokio docs
- Pitfalls: HIGH for clap env feature and tokio::main; MEDIUM for magic byte design (general principle, not Rust-specific)

**Research date:** 2026-02-27
**Valid until:** 2026-09-27 (clap 4 and tokio 1 are stable; thiserror 2 is stable; stdlib patterns are permanent)
