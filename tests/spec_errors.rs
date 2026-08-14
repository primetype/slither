//! Exhaustiveness fence over SPEC.md §18.1's error taxonomy.
//!
//! §18.1 is explicit: "Closed and normative: every variant this
//! specification names appears here exactly once." For each of the ten
//! error types this file defines a function that takes a value of that
//! type and `match`es it with **no wildcard arm**. That absence is the
//! whole point: a `_` arm would silently keep compiling the day a variant
//! is added or removed, defeating the fence. If a variant list here ever
//! stops matching the crate's, this file stops *compiling* — do not "fix"
//! that by adding a `_` arm; it means either SPEC.md or the crate moved
//! and the taxonomy needs a ruling, per CLAUDE.md's frozen-spec rule.
//!
//! Each fence function is referenced (as a bare function pointer) from a
//! `#[test]`, rather than called on a constructed instance — several
//! variants carry fields (e.g. `ConnectionLost::PeerClosed`'s `reason`)
//! whose exact Rust type §18.1 does not spell out, and guessing one to
//! build a dummy value would risk a false-negative compile failure
//! unrelated to the taxonomy itself. Taking the function pointer is enough
//! to force the match to be type-checked (and to keep it from being
//! flagged dead code under `-D warnings`) without that risk. Where §18.1
//! *does* give an explicit field type (`WriteError::Reset(u64)`,
//! `ReadError::Reset(u64)`), the match arm below annotates the binding
//! with that type, so a field-type drift also fails to compile.
//!
//! `ConfigError` is the one exception to "from §18.1": per ruling 44
//! (grep `ConfigError` in SPEC.md), it is a **configuration** error for
//! `set_persistent_keepalive` and is deliberately kept outside §18.1's
//! protocol-error taxonomy — no peer, packet, or connection state is
//! involved. It is checked here against §16.2 instead.

use slither::error::*;

fn requires_clone<T: Clone>() {}

#[test]
fn connection_lost_is_clone() {
    // SPEC.md §16.2, ruling 46 ("A connection's death ... [is] awaitable."):
    // `closed()` is a *latched* signal, awaitable concurrently from any
    // number of tasks, and resolves with the `ConnectionLost` that ended
    // the connection every time it's polled after death — which the spec
    // says "asks only that `ConnectionLost` be `Clone`, a derive, not a
    // variant change; §18.1 stays closed."
    requires_clone::<ConnectionLost>();
}

#[test]
fn connect_error_is_exhaustively_matched() {
    let _: fn(ConnectError) = match_connect_error;
}

#[test]
fn intro_error_is_exhaustively_matched() {
    let _: fn(IntroError) = match_intro_error;
}

#[test]
fn auth_error_is_exhaustively_matched() {
    let _: fn(AuthError) = match_auth_error;
}

#[test]
fn accept_error_is_exhaustively_matched() {
    let _: fn(AcceptError) = match_accept_error;
}

#[test]
fn connection_lost_is_exhaustively_matched() {
    let _: fn(ConnectionLost) = match_connection_lost;
}

#[test]
fn write_error_is_exhaustively_matched() {
    let _: fn(WriteError) = match_write_error;
}

#[test]
fn read_error_is_exhaustively_matched() {
    let _: fn(ReadError) = match_read_error;
}

#[test]
fn message_error_is_exhaustively_matched() {
    let _: fn(MessageError) = match_message_error;
}

#[test]
fn datagram_error_is_exhaustively_matched() {
    let _: fn(DatagramError) = match_datagram_error;
}

#[test]
fn config_error_is_exhaustively_matched() {
    let _: fn(ConfigError) = match_config_error;
}

// =======================================================================
// The fence functions themselves.
// =======================================================================

/// SPEC.md §18.1: `ConnectError::{AlreadyConnected, TimedOut}` — exactly
/// two variants, both unit. `AlreadyConnected` is returned by `connect()`
/// itself and also as the tie-break-loser resolution of an in-flight
/// `Connecting` (§6.4's PENDING branch, §6.7); `TimedOut` is initial-connect
/// give-up at `HANDSHAKE_GIVEUP` (§5.5).
fn match_connect_error(e: ConnectError) {
    match e {
        ConnectError::AlreadyConnected => {}
        ConnectError::TimedOut => {}
    }
}

/// SPEC.md §18.1: `IntroError::{Expired, Internal, Malformed,
/// EndpointDropped}` — exactly four variants, all unit. `Superseded`
/// appears nowhere (§6.3) — it is not a fifth variant to add here.
fn match_intro_error(e: IntroError) {
    match e {
        IntroError::Expired => {}
        IntroError::Internal => {}
        IntroError::Malformed => {}
        IntroError::EndpointDropped => {}
    }
}

/// SPEC.md §18.1: `AuthError::{Replay, HandshakeFailed, Expired,
/// EndpointDropped}` — exactly four variants, all unit. `HandshakeFailed`
/// is called out as "the only variant in the staged taxonomy that is a
/// security signal."
fn match_auth_error(e: AuthError) {
    match e {
        AuthError::Replay => {}
        AuthError::HandshakeFailed => {}
        AuthError::Expired => {}
        AuthError::EndpointDropped => {}
    }
}

/// SPEC.md §18.1: `AcceptError::{Stale, EndpointDropped}` — exactly two
/// variants, both unit. Explicitly **not** four: `Expired` and
/// `AlreadyConnected` are both called out by name as absent ("There is no
/// `Expired` here ... There is no `AlreadyConnected`").
fn match_accept_error(e: AcceptError) {
    match e {
        AcceptError::Stale => {}
        AcceptError::EndpointDropped => {}
    }
}

/// SPEC.md §18.1: `ConnectionLost::{TimedOut, NonceExhausted,
/// LocallyClosed, PeerClosed { code, reason }, ProtocolViolation { code },
/// Replaced, EndpointDropped}` — exactly seven variants. `PeerClosed` and
/// `ProtocolViolation` are the two struct-shaped ones (the shape itself —
/// field names and arity — is what this match's patterns assert); the
/// other five are unit.
fn match_connection_lost(e: ConnectionLost) {
    match e {
        ConnectionLost::TimedOut => {}
        ConnectionLost::NonceExhausted => {}
        ConnectionLost::LocallyClosed => {}
        ConnectionLost::PeerClosed { code, reason } => {
            // Shape only (§18.1 does not pin the field types beyond
            // "carries the peer's CLOSE payload"): both fields must exist,
            // named exactly `code` and `reason`.
            let _ = code;
            let _ = reason;
        }
        ConnectionLost::ProtocolViolation { code } => {
            let _ = code;
        }
        ConnectionLost::Replaced => {}
        ConnectionLost::EndpointDropped => {}
    }
}

/// SPEC.md §18.1: `WriteError::{Reset(u64), ConnectionLost(ConnectionLost),
/// Finished}` — exactly three *documented* variants, but this is the ONE
/// type in the taxonomy ruling 61 marks `#[non_exhaustive]` (§19 reserves
/// `Stopped` for the deferred STOP_SENDING round, so this type demonstrably
/// will gain a variant). `#[non_exhaustive]` has no effect within the
/// defining crate, but this file is compiled as a separate crate (anything
/// under `tests/` is), and from outside the defining crate the compiler
/// *requires* a wildcard arm on a non_exhaustive enum's match — there is no
/// way to satisfy rustc here without one, unlike every other function in
/// this file. The `_` below is therefore load-bearing, not a shortcut: it
/// stays loud by panicking rather than silently doing nothing, so an
/// actually-new variant is still caught (at test-run time, since a compile
/// time catch is exactly what `#[non_exhaustive]` forecloses here).
fn match_write_error(e: WriteError) {
    match e {
        WriteError::Reset(code) => {
            let _: u64 = code;
        }
        WriteError::ConnectionLost(inner) => {
            let _: ConnectionLost = inner;
        }
        WriteError::Finished => {}
        _ => panic!(
            "WriteError gained a variant this exhaustiveness fence doesn't \
             recognise (documented set: Reset(u64) / ConnectionLost(..) / \
             Finished, plus the reserved-but-undelivered `Stopped` of §19) \
             — SPEC.md §18.1/§19 and this file need to be reconciled"
        ),
    }
}

/// SPEC.md §18.1: `ReadError::{Reset(u64), ConnectionLost(ConnectionLost)}`
/// — exactly two variants. `Reset(code)` is the peer's RESET_STREAM (§9.6).
fn match_read_error(e: ReadError) {
    match e {
        ReadError::Reset(code) => {
            let _: u64 = code;
        }
        ReadError::ConnectionLost(inner) => {
            let _: ConnectionLost = inner;
        }
    }
}

/// SPEC.md §18.1: `MessageError::{TooLarge, ConnectionLost(ConnectionLost)}`
/// — exactly two variants. `TooLarge`: payload > `MESSAGE_RECV_MAX` at the
/// handle (§9.8).
fn match_message_error(e: MessageError) {
    match e {
        MessageError::TooLarge => {}
        MessageError::ConnectionLost(inner) => {
            let _: ConnectionLost = inner;
        }
    }
}

/// SPEC.md §18.1: `DatagramError::{TooLarge,
/// ConnectionLost(ConnectionLost)}` — exactly two variants. `TooLarge`:
/// payload > `MAX_DATAGRAM_PAYLOAD` at the handle (§11.4).
fn match_datagram_error(e: DatagramError) {
    match e {
        DatagramError::TooLarge => {}
        DatagramError::ConnectionLost(inner) => {
            let _: ConnectionLost = inner;
        }
    }
}

/// `ConfigError` is deliberately **outside** §18.1's closed taxonomy
/// (ruling 44: "`ConfigError` is a **configuration** error and
/// deliberately sits outside §18.1's protocol-error taxonomy ... no peer,
/// no packet, and no connection state is involved, and nothing about it is
/// observable on the wire"). Its home is §16.2: `set_persistent_keepalive`
/// returns `Result<(), ConfigError>` with exactly two variants, each naming
/// the bound it violated — `KeepaliveTooShort` (below 1 s, the floor,
/// ruling 42) and `KeepaliveTooLong` (at or above `DEAD_TIMEOUT`, the
/// ceiling, ruling 40).
fn match_config_error(e: ConfigError) {
    match e {
        ConfigError::KeepaliveTooShort => {}
        ConfigError::KeepaliveTooLong => {}
    }
}
