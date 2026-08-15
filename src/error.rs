//! The error taxonomy — `SPEC.md` §18.1, plus the one type it excludes.
//!
//! # The taxonomy is closed
//!
//! §18.1 names **nine** error types and every variant each may hold. That
//! set is closed by process: a new variant is a ratification decision, not
//! a patch release. **Ruling 72 is what that decision looks like** —
//! `ConnectError::Local` and `IntroError::Local` were added *because* the
//! taxonomy could not say "our own key hardware failed", and they were
//! added before release precisely because §18.1's closure makes the same
//! amendment a breaking change afterwards. Ruling 78 then added
//! `AuthError::Local`, which ruling 72 had missed — and missing it was the
//! worse half, because the variant a local fault fell through to there is
//! the taxonomy's one **security signal**.
//!
//! **[RATIFIED 2026/08/15 — ruling 79]** None of the three `Local`
//! variants carries the provider's error, and none can: these types are
//! `Clone + PartialEq + Eq + Send + Sync` and
//! [`Identity::Error`](crate::identity::Identity::Error) is bounded on none
//! of them — deliberately, since an enclave-backed provider is `!Send`.
//! The detail is not lost, it is **re-addressed**: the party who can *act*
//! gets the variant, the party who *diagnoses* gets §18.2's `slither::io`
//! trace, carrying the provider's own error and the verb that met it. This
//! is the third question that split has settled here, after rulings 49 and
//! 59, and it is now the general rule — when a fault is real but the
//! application cannot act on the detail, the detail is a trace, not a
//! variant. Nine of the ten types here are
//! therefore *exhaustive*
//! Rust enums, which says the same thing in the type system — a consumer
//! who matches without a `_` arm gets a **compile error** the day a variant
//! lands, and for a transport that is the loud failure worth having. A
//! wildcard arm would silently route a new error into a branch written for
//! the old ones, which is strictly worse than a build break.
//!
//! [`WriteError`] is the sole exception (ruling 61): §19 explicitly
//! reserves a `Stopped` variant for the deferred STOP_SENDING round, so
//! that type demonstrably *will* gain one and carries
//! `#[non_exhaustive]` as the reservation. **The attribute is not a
//! licence** — it does not authorise adding `Stopped` before that round is
//! ratified, and it must not be copied onto the other nine. A reviewer who
//! "fixes" the inconsistency by spreading it has undone a ruling.
//!
//! # The tenth type
//!
//! [`ConfigError`] is **not** part of §18.1. It exists because §16.2's
//! `set_persistent_keepalive` returns `Result<(), ConfigError>`, and
//! ruling 44 defines it while stating that it *"deliberately sits outside
//! §18.1's protocol-error taxonomy, which stays closed: no peer, no
//! packet, and no connection state is involved."* Its presence here is a
//! matter of where the file lives, not an opening of the taxonomy: a
//! rejected keepalive interval is a caller's configuration mistake,
//! observable nowhere on the wire.
//!
//! # What is deliberately absent
//!
//! There is **no `SlitherError` umbrella enum** — a top-level union would
//! re-open the taxonomy through the back door and give every consumer a
//! second way to match. There is **no `Result<T>` alias** — ten error
//! types, and an alias would have to pick a favourite. The
//! `std::io::Error` conversions and the wire-code ↔ variant mapping live
//! in the modules that need them, not here.

/// Why an outbound dial did not produce a connection. §18.1.
///
/// Exactly three variants (ruling 72). There is no `EndpointDropped`: by
/// ruling 62 a `Connecting` future is a *handle* — it changes protocol
/// state when dropped — so the driver cannot stop beneath one, and the
/// variant would describe an unreachable state.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConnectError {
    /// A connection to this static already exists.
    #[error("a connection to this static already exists")]
    AlreadyConnected,
    /// The dial was retried until `HANDSHAKE_GIVEUP` and gave up.
    #[error("the initial connect gave up after HANDSHAKE_GIVEUP")]
    TimedOut,
    /// **Our own** [`Identity::open`] failed — a locked or
    /// biometrics-gated enclave, a hardware fault, a provider that is
    /// momentarily unavailable. §18.1, **ruling 72**.
    ///
    /// The fault is *ours*, not the peer's, and S21 treats it as
    /// **expected** rather than exceptional. It is reported only when the
    /// dial ends having **never got a single msg1 onto the wire** — see
    /// the note on `ConnectError` in the endpoint core; a dial whose first
    /// attempt failed and whose second succeeded is not a local failure at
    /// all, and one that transmitted and was not answered is a genuine
    /// [`TimedOut`](ConnectError::TimedOut).
    ///
    /// **Ruling 79**: the provider's own error rides §18.2's `slither::io`
    /// trace, not this variant — see the [module docs](self).
    ///
    /// [`Identity::open`]: crate::identity::Identity::open
    #[error("our own identity provider failed to open")]
    Local,
}

/// Why a parked introduction could not be taken up. §18.1.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IntroError {
    /// The parked introduction outlived `INTRO_TTL`.
    #[error("the parked introduction outlived INTRO_TTL")]
    Expired,
    /// The initiation belonged to a pending outbound dial and was consumed
    /// by it.
    #[error("the initiation belonged to a pending outbound dial and was consumed")]
    Internal,
    /// The introduction's msg1 is structurally unreadable.
    ///
    /// **The peer's bytes are at fault and the verdict is definitive**: 1
    /// DH is spent, the chain is **discarded**, and its stage-0 slot is
    /// freed (§6.1). Retaining it would hand an attacker a per-source slot
    /// for the price of unreadable bytes. Contrast
    /// [`Local`](IntroError::Local), which is the opposite in every way
    /// that matters (ruling 72).
    #[error("the introduction's msg1 is structurally unreadable")]
    Malformed,
    /// **Our own** provider failed — [`Identity::open`], or the responder
    /// machine it feeds, would not build. §18.1, **ruling 72**.
    ///
    /// **0 DH has been spent and the chain is left parked**, so a retry
    /// can still succeed: a locked enclave is transient, and S21 treats it
    /// as expected. Reporting this as
    /// [`Malformed`](IntroError::Malformed) would tell the application the
    /// remote peer sent garbage — evidence pointing at the wrong party,
    /// which an application may reasonably act on by denylisting or
    /// alerting.
    ///
    /// **Ruling 79**: the provider's own error rides §18.2's `slither::io`
    /// trace, not this variant — see the [module docs](self).
    ///
    /// [`Identity::open`]: crate::identity::Identity::open
    #[error("our own identity provider failed to open")]
    Local,
    /// The endpoint driver stopped.
    #[error("the endpoint driver stopped")]
    EndpointDropped,
}

/// Why an introduction failed to authenticate its peer. §18.1.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    /// The timestamp guard rejected this initiation as a replay.
    #[error("the timestamp guard rejected this initiation as a replay")]
    Replay,
    /// The handshake failed to authenticate.
    ///
    /// This is a **security signal**, and deliberately carries no detail:
    /// a caller learns that authentication failed, never why.
    #[error("the handshake failed to authenticate")]
    HandshakeFailed,
    /// The parked introduction outlived `INTRO_TTL`.
    #[error("the parked introduction outlived INTRO_TTL")]
    Expired,
    /// **Our own** provider failed, exactly as [`IntroError::Local`].
    /// §18.1, **ruling 78**.
    ///
    /// `authenticate()` may drive a skipped `read_identity()` (ruling 75),
    /// so it can meet the same local fault. Ruling 72 did not reach this
    /// type, and the omission was the worse half of the defect it fixed:
    /// without this variant a locked enclave surfaced as
    /// [`HandshakeFailed`](AuthError::HandshakeFailed) — which does not
    /// merely misattribute a local fault to the peer, it **reports the peer
    /// as an attacker**, and teaches an operator to distrust the one
    /// variant that must stay trustworthy.
    ///
    /// **Ruling 79**: the provider's own error rides §18.2's `slither::io`
    /// trace, not this variant — see the [module docs](self).
    #[error("our own identity provider failed to open")]
    Local,
    /// The endpoint driver stopped.
    #[error("the endpoint driver stopped")]
    EndpointDropped,
}

/// Why a staged accept did not complete. §18.1.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AcceptError {
    /// No initiation is parked for this static, or it fails the
    /// replacement basis.
    #[error("no initiation is parked for this static, or it fails the replacement basis")]
    Stale,
    /// The endpoint driver stopped.
    #[error("the endpoint driver stopped")]
    EndpointDropped,
}

/// Why a connection ended. §18.1.
///
/// This is the payload of a fan-out: `closed()` resolves for every holder,
/// every notification stream ends with it, and each of [`WriteError`],
/// [`ReadError`], [`MessageError`] and [`DatagramError`] embeds it. A
/// connection dies once and the same value is handed to N awaiting futures
/// and to every later verb call, so **`Clone` is a hard requirement, not a
/// convenience** — the alternative is an `Rc` in the public error type.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConnectionLost {
    /// No authenticated packet arrived for `DEAD_TIMEOUT`.
    #[error("no authenticated packet arrived for DEAD_TIMEOUT")]
    TimedOut,
    /// The send counter is exhausted.
    #[error("the send counter is exhausted")]
    NonceExhausted,
    /// Closed by this application.
    #[error("closed by this application")]
    LocallyClosed,
    /// Closed by the peer, with the code and reason it sent.
    ///
    /// `reason` is bounded at `CLOSE_REASON_MAX` bytes by the wire, so
    /// cloning it is bounded too.
    #[error("closed by the peer: code {code}")]
    PeerClosed {
        /// The application or transport error code the peer sent.
        code: u64,
        /// The peer's reason phrase, at most `CLOSE_REASON_MAX` bytes.
        reason: Vec<u8>,
    },
    /// Torn down locally after the peer violated the protocol.
    #[error("torn down after a protocol violation: code {code}")]
    ProtocolViolation {
        /// The transport error code sent to the peer in the CLOSE.
        code: u64,
    },
    /// Replaced by a newer connection from the same static.
    #[error("replaced by a newer connection from the same static")]
    Replaced,
    /// The endpoint driver stopped.
    #[error("the endpoint driver stopped")]
    EndpointDropped,
}

/// Why a stream write failed. §18.1.
///
/// The **one** non-exhaustive type in the crate (ruling 61): §19 reserves
/// `Stopped` for the STOP_SENDING round, so this type demonstrably will
/// gain a variant. `#[non_exhaustive]` is that reservation — it is not
/// permission to add the variant early.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum WriteError {
    /// The stream was reset, with the code that reset it.
    #[error("the stream was reset: code {0}")]
    Reset(u64),
    /// The connection ended.
    #[error(transparent)]
    ConnectionLost(#[from] ConnectionLost),
    /// A write was attempted after the stream was finished.
    #[error("write after finish")]
    Finished,
}

/// Why a stream read failed. §18.1.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReadError {
    /// The peer reset the stream, with the code it sent.
    #[error("the peer reset the stream: code {0}")]
    Reset(u64),
    /// The connection ended.
    #[error(transparent)]
    ConnectionLost(#[from] ConnectionLost),
}

/// Why a message verb failed. §18.1.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MessageError {
    /// The message exceeds `MESSAGE_RECV_MAX`.
    #[error("the message exceeds MESSAGE_RECV_MAX")]
    TooLarge,
    /// The connection ended.
    #[error(transparent)]
    ConnectionLost(#[from] ConnectionLost),
}

/// Why an unreliable-datagram verb failed. §18.1.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DatagramError {
    /// The datagram exceeds `MAX_DATAGRAM_PAYLOAD`.
    #[error("the datagram exceeds MAX_DATAGRAM_PAYLOAD")]
    TooLarge,
    /// The connection ended.
    #[error(transparent)]
    ConnectionLost(#[from] ConnectionLost),
}

/// Why a configuration value was rejected. §16.2, ruling 44.
///
/// Outside §18.1 by that ruling — a configuration error, not a protocol
/// one: no peer, no packet, no connection state, nothing observable on the
/// wire. Exhaustive, like the taxonomy proper.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// The persistent-keepalive interval is below the 1 s floor.
    #[error("the persistent-keepalive interval is below the 1 s floor")]
    KeepaliveTooShort,
    /// The persistent-keepalive interval is at or above `DEAD_TIMEOUT`.
    #[error("the persistent-keepalive interval is at or above DEAD_TIMEOUT")]
    KeepaliveTooLong,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as _;

    fn _assert_clone<T: Clone>() {}
    fn _assert_send_sync<T: Send + Sync + 'static>() {}

    /// The exhaustiveness fence for `WriteError`, and the reason it lives
    /// **here** rather than beside the other nine in `tests/spec_errors.rs`.
    ///
    /// `WriteError` is the one type carrying `#[non_exhaustive]` (ruling
    /// 61 — §19 reserves `Stopped` for the STOP_SENDING round). That
    /// attribute has **no effect within the defining crate** and full
    /// effect outside it, so an integration test in `tests/` — a separate
    /// crate — is *forced* to write a wildcard arm. The wildcard then
    /// silently absorbs any variant added later, which is precisely the
    /// event the fence exists to catch. An out-of-crate exhaustiveness
    /// fence over a `#[non_exhaustive]` enum cannot work, by
    /// construction; `tests/spec_errors.rs` covers the other nine, where
    /// it does work, and defers this one here.
    ///
    /// Found by mutation testing at slice 0: an eleventh variant added to
    /// `WriteError` passed `cargo build`, `cargo test`, and the
    /// out-of-crate test named for this exact check.
    ///
    /// **Do not add a `_` arm.** Failing to compile is the whole point:
    /// when `Stopped` lands, this match is a deliberate stop so that §18.1
    /// and §19 are reconciled on purpose rather than by a wildcard.
    #[test]
    fn write_error_is_exhaustive_in_crate() {
        fn fence(e: WriteError) {
            match e {
                WriteError::Reset(code) => {
                    let _: u64 = code;
                }
                WriteError::ConnectionLost(inner) => {
                    let _: ConnectionLost = inner;
                }
                WriteError::Finished => {}
            }
        }
        // Call it, so the fence is live code and not merely a coercion.
        fence(WriteError::Reset(0));
        fence(WriteError::ConnectionLost(ConnectionLost::TimedOut));
        fence(WriteError::Finished);
    }

    /// `ConnectionLost` is fanned out to every holder of a dead
    /// connection, so it must be cloneable. A future payload that is not
    /// would break this silently at the point of use; here it breaks at
    /// compile time.
    #[test]
    fn connection_lost_is_clone() {
        _assert_clone::<ConnectionLost>();
        // Clone propagates to the four types that embed it.
        _assert_clone::<WriteError>();
        _assert_clone::<ReadError>();
        _assert_clone::<MessageError>();
        _assert_clone::<DatagramError>();
        // And the heap-carrying variant clones for real, not just by type.
        let lost = ConnectionLost::PeerClosed {
            code: 0x10,
            reason: b"goodbye".to_vec(),
        };
        assert_eq!(lost.clone(), lost);
    }

    /// Errors are plain data, so they may cross a thread boundary.
    ///
    /// This does **not** violate the no-`Send` rule: that rule is about
    /// the actor path (the `Wire`, the DH provider, the handles), not
    /// about error payloads a consumer will want to put on a channel.
    #[test]
    fn error_types_are_send_and_sync() {
        _assert_send_sync::<ConnectError>();
        _assert_send_sync::<IntroError>();
        _assert_send_sync::<AuthError>();
        _assert_send_sync::<AcceptError>();
        _assert_send_sync::<ConnectionLost>();
        _assert_send_sync::<WriteError>();
        _assert_send_sync::<ReadError>();
        _assert_send_sync::<MessageError>();
        _assert_send_sync::<DatagramError>();
        _assert_send_sync::<ConfigError>();
    }

    /// One instance of every variant, for the display-string check.
    ///
    /// This is a *sample*, not a taxonomy fence — the closed-taxonomy
    /// compile fence is authored separately, from the spec.
    fn every_variant() -> Vec<Box<dyn std::error::Error>> {
        vec![
            Box::new(ConnectError::AlreadyConnected),
            Box::new(ConnectError::TimedOut),
            Box::new(ConnectError::Local),
            Box::new(IntroError::Expired),
            Box::new(IntroError::Internal),
            Box::new(IntroError::Malformed),
            Box::new(IntroError::Local),
            Box::new(IntroError::EndpointDropped),
            Box::new(AuthError::Replay),
            Box::new(AuthError::HandshakeFailed),
            Box::new(AuthError::Expired),
            Box::new(AuthError::Local),
            Box::new(AuthError::EndpointDropped),
            Box::new(AcceptError::Stale),
            Box::new(AcceptError::EndpointDropped),
            Box::new(ConnectionLost::TimedOut),
            Box::new(ConnectionLost::NonceExhausted),
            Box::new(ConnectionLost::LocallyClosed),
            Box::new(ConnectionLost::PeerClosed {
                code: 0x10,
                reason: b"bye".to_vec(),
            }),
            Box::new(ConnectionLost::ProtocolViolation { code: 0x01 }),
            Box::new(ConnectionLost::Replaced),
            Box::new(ConnectionLost::EndpointDropped),
            Box::new(WriteError::Reset(7)),
            Box::new(WriteError::ConnectionLost(ConnectionLost::TimedOut)),
            Box::new(WriteError::Finished),
            Box::new(ReadError::Reset(7)),
            Box::new(ReadError::ConnectionLost(ConnectionLost::TimedOut)),
            Box::new(MessageError::TooLarge),
            Box::new(MessageError::ConnectionLost(ConnectionLost::TimedOut)),
            Box::new(DatagramError::TooLarge),
            Box::new(DatagramError::ConnectionLost(ConnectionLost::TimedOut)),
            Box::new(ConfigError::KeepaliveTooShort),
            Box::new(ConfigError::KeepaliveTooLong),
        ]
    }

    /// Every `Display` string is non-empty, starts lower-case and does not
    /// end in a full stop — the `thiserror` house style, which `-D
    /// warnings` cannot enforce.
    #[test]
    fn display_strings_are_non_empty_and_lowercase_initial() {
        for e in every_variant() {
            let s = e.to_string();
            assert!(!s.is_empty(), "empty display string for {e:?}");
            assert!(
                !s.ends_with('.'),
                "display string ends in a full stop: {s:?}"
            );
            let first = s.chars().next().expect("non-empty");
            assert!(
                !first.is_uppercase(),
                "display string starts upper-case: {s:?}"
            );
        }
    }

    /// `?` lifts a `ConnectionLost` into each of the four types that embed
    /// it, and `#[error(transparent)]` forwards `Display` through
    /// unchanged.
    #[test]
    fn embedded_connection_lost_converts() {
        fn w() -> Result<(), WriteError> {
            Err(ConnectionLost::TimedOut)?
        }
        fn r() -> Result<(), ReadError> {
            Err(ConnectionLost::TimedOut)?
        }
        fn m() -> Result<(), MessageError> {
            Err(ConnectionLost::TimedOut)?
        }
        fn d() -> Result<(), DatagramError> {
            Err(ConnectionLost::TimedOut)?
        }

        assert_eq!(
            w().unwrap_err(),
            WriteError::ConnectionLost(ConnectionLost::TimedOut)
        );
        assert_eq!(
            r().unwrap_err(),
            ReadError::ConnectionLost(ConnectionLost::TimedOut)
        );
        assert_eq!(
            m().unwrap_err(),
            MessageError::ConnectionLost(ConnectionLost::TimedOut)
        );
        assert_eq!(
            d().unwrap_err(),
            DatagramError::ConnectionLost(ConnectionLost::TimedOut)
        );

        // `transparent` means the wrapper adds no message of its own.
        let inner = ConnectionLost::TimedOut.to_string();
        assert_eq!(w().unwrap_err().to_string(), inner);
        assert_eq!(r().unwrap_err().to_string(), inner);
        assert_eq!(m().unwrap_err().to_string(), inner);
        assert_eq!(d().unwrap_err().to_string(), inner);

        // `transparent` also forwards `source()` — so a wrapper reports
        // the *inner* error's source, which for a leaf `ConnectionLost` is
        // `None`. The chain is not lengthened by the wrapper; that is the
        // point of the attribute.
        assert!(ConnectionLost::TimedOut.source().is_none());
        assert!(w().unwrap_err().source().is_none());

        // A non-transparent variant likewise has no source.
        assert!(WriteError::Finished.source().is_none());
    }
}
