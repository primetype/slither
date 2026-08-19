//! The event log — one ordered stream of JSON lines on stdout.
//!
//! # Why one log and not three
//!
//! Three producers write timeline events: the scenario body (stage
//! transitions, application I/O), [`crate::wire::ObservedWire`] (one event
//! per datagram copy) and [`crate::trace`]'s `tracing` subscriber (§18.2's
//! five targets). The UI animates them on one axis, so they have to share
//! one monotonic `seq` and one clock reading. Separate logs merged later
//! would put the merge — and therefore the ordering bug — in JavaScript.
//!
//! # Every timestamp is virtual
//!
//! `t_us` is microseconds since the scenario's first instant, read from
//! `tokio::time::Instant::now()` under a runtime built with
//! `start_paused(true)`. So a 5-second retransmit wait is `5_000_000`
//! here and roughly a millisecond of anybody's wall clock. The page says
//! so out loud; nothing in the UI should present `t_us` as wall time.

use std::sync::{Arc, Mutex};

use tokio::time::Instant;

/// A cheap shared handle on the log. Clones write to the same stream.
#[derive(Clone)]
pub struct Log(Arc<Mutex<Inner>>);

struct Inner {
    origin: Instant,
    seq: u64,
    pkt: u64,
    counters: Counters,
}

/// The running totals the summary event reports.
///
/// Kept here rather than in the wire because the `end` event wants them
/// after the wire has been dropped along with the endpoint it was moved
/// into.
#[derive(Clone, Copy, Default)]
pub struct Counters {
    /// Datagrams the protocol handed to a wire (copies not counted).
    pub sent: u64,
    /// Copies that reached the peer's inbox.
    pub delivered: u64,
    /// Copies the demo's loss draw destroyed.
    pub lost: u64,
    /// Extra copies the demo's duplication draw created.
    pub duplicated: u64,
    /// Sends swallowed by a demo-side blackhole (`Ok`, nothing delivered).
    pub blackholed: u64,
}

impl Log {
    /// Start the clock. The first event emitted lands at `t_us` 0.
    pub fn start() -> Log {
        Log(Arc::new(Mutex::new(Inner {
            origin: Instant::now(),
            seq: 0,
            pkt: 0,
            counters: Counters::default(),
        })))
    }

    /// The next datagram ordinal, unique across **both** wires.
    ///
    /// Per-wire ordinals were the first shape and they collide: `a`'s
    /// packet 1 and `b`'s packet 1 are different datagrams, so any UI
    /// keying an animation on the id alone draws one dot for two packets.
    /// Numbering from the log removes the trap rather than documenting it.
    pub fn next_pkt(&self) -> u64 {
        let mut inner = self.0.lock().expect("the demo is single-threaded");
        let id = inner.pkt;
        inner.pkt += 1;
        id
    }

    /// Emit one event. `body` is the comma-separated tail of the object,
    /// already JSON — everything after `"kind"`.
    ///
    /// Written straight to stdout rather than buffered: under WASI this is
    /// one `fd_write` per line, the Worker's `ConsoleStdout.lineBuffered`
    /// splits on the newline, and a run that panics mid-way still shows
    /// the page everything that happened before the panic.
    pub fn emit(&self, kind: &str, body: &str) {
        let mut inner = self.0.lock().expect("the demo is single-threaded");
        let seq = inner.seq;
        inner.seq += 1;
        let t_us = Instant::now()
            .saturating_duration_since(inner.origin)
            .as_micros();
        drop(inner);
        if body.is_empty() {
            println!(r#"{{"seq":{seq},"t_us":{t_us},"kind":"{kind}"}}"#);
        } else {
            println!(r#"{{"seq":{seq},"t_us":{t_us},"kind":"{kind}",{body}}}"#);
        }
    }

    /// Virtual microseconds since the origin, for a caller that needs the
    /// number inside a body it is building.
    pub fn now_us(&self) -> u128 {
        let inner = self.0.lock().expect("the demo is single-threaded");
        Instant::now()
            .saturating_duration_since(inner.origin)
            .as_micros()
    }

    /// Mutate the running totals.
    pub fn count(&self, f: impl FnOnce(&mut Counters)) {
        let mut inner = self.0.lock().expect("the demo is single-threaded");
        f(&mut inner.counters);
    }

    /// Read the running totals.
    pub fn counters(&self) -> Counters {
        self.0.lock().expect("the demo is single-threaded").counters
    }
}

/// Render `s` as a JSON string literal, quotes included.
///
/// Hand-rolled because the alternative is `serde_json` in a payload that
/// ships over the network to a browser. The escape set is RFC 8259's: the
/// two mandatory characters, the five shorthands, and `\u00XX` for every
/// remaining control character.
pub fn jstr(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::jstr;

    #[test]
    fn escapes_the_mandatory_two_and_the_control_range() {
        assert_eq!(jstr(r#"a"b\c"#), r#""a\"b\\c""#);
        assert_eq!(jstr("\n\r\t"), r#""\n\r\t""#);
        assert_eq!(jstr("\u{1}"), "\"\\u0001\"");
        // Not escaped: DEL is not in the mandatory set, and non-ASCII
        // rides as UTF-8.
        assert_eq!(jstr("é"), "\"é\"");
    }
}
