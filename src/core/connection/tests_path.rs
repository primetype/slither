//! Path-validation behaviour tests (ruling 208).
//!
//! Written blind to the implementers, from `CONTRACT-7b.md`, rulings 208 /
//! 210 / 212 and `SPEC.md` §7.3. Each test names the *broken build* it is
//! meant to catch (working rule 9: a bound is only a test if the degenerate
//! case violates it).
//!
//! Facts pinned from the ruling record (read before any source):
//!
//! - 208: `PATH_CHALLENGE` `0x1a` / `PATH_RESPONSE` `0x1b`, 8 opaque bytes,
//!   both ack-eliciting, per-arming, never reused across armings.
//! - 208: `validation_floor` and `on_ack_covering` **lose their security
//!   role** — replaced, not supplemented; leaving them is an attacker bypass.
//! - 210(b): challenge RNG is §16.6's per-connection sub-seed, not the
//!   endpoint RNG (`Connection` cannot reach it). `set_floor` is deleted.
//! - 210(c): the challenge closes A1 (off-path/peer forgery) but NOT A1b
//!   (on-path relay).
//! - 210(d): no existing wire byte moves; a red golden vector is a stop.
//! - 210 hazard: `on_ack_coverage` feeds **two** floors — §7.3's
//!   amplification budget and §7.5's contested-probe floor (ruling 176).
//!   Remove the amplification role only.
//! - 212(c): challenge/response rank immediately after CLOSE, **above** the
//!   contested probe; the pump's early return may not block it.
//! - 212(d): retransmission class `never` + standing obligation; mismatched
//!   `PATH_RESPONSE` is a semantic no-op (not `PROTOCOL_VIOLATION`); at most
//!   one outstanding response, overwritten not queued; obligation kept
//!   across a roam.
//! - 212: `AMPLIFICATION_FACTOR` is still 3.
//!
//! Skeleton (filled in below):
//!
//! 1. Forged ACK no longer validates
//! 2. `PATH_CHALLENGE` / `PATH_RESPONSE` round trip
//! 3. Challenge entropy / per-arming freshness
//! 4. Contested-probe floor still fed (`on_ack_coverage`, ruling 176)
//! 5. Ruling 212(c) — challenge outranks contested probe
//! 6. Mismatched response is a no-op; at most one outstanding
//! 7. Amplification budget arithmetic (factor 3)
