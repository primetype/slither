//! §11's datagram queues — the unreliable path's only state.
//!
//! Two bounded queues and two counters, and every one of those four lives
//! **in the connection core**: §11.3 says so in terms — *"Both queues,
//! their eviction discipline, and the drop counters live in the connection
//! core, not the shell (§16.4) — the bound is protocol state, not a
//! delivery detail."*
//!
//! The discipline is **drop-oldest with the newest always accepted**. There
//! is deliberately no "queue full, try later": that answer would contradict
//! §16.2's `send_datagram` *never waits* and S15's *"drops oldest under
//! pressure rather than blocking"*, and it is not representable here —
//! [`push_send`](Datagrams::push_send) and [`push_recv`](Datagrams::push_recv)
//! return *whether an eviction happened*, never *whether the new item was
//! taken*.
//!
//! Bounded by **count**, not bytes (§11.3): 64 × 1169 B ≈ 73 KiB per queue
//! per connection, and a tiny-datagram flood cannot turn a byte budget into
//! a million queue entries.

use std::collections::VecDeque;

use crate::constants;

/// §11.5's counters — **two**, one per queue.
///
/// One counter cannot answer the operator question the counters exist for:
/// *is my application over-producing, or is my peer over-sending?* (ruling
/// 156b — §11.5's singular sits four words from its own plural.)
///
/// **They are not a delivery-loss counter.** A datagram lost on the wire
/// increments nothing: §11.1 promises no delivery and §8.7 puts DATAGRAM in
/// the `never` class, so there is no loss event to count. Only
/// queue-overflow evictions are counted here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct DatagramDrops {
    /// Send-queue evictions: this application produced faster than the
    /// congestion window drained.
    pub(crate) send: u64,
    /// Receive-queue evictions: the peer produced faster than this
    /// application claimed.
    pub(crate) recv: u64,
}

/// §11.3's two bounded queues.
#[derive(Debug, Default)]
pub(crate) struct Datagrams {
    send: VecDeque<Vec<u8>>,
    recv: VecDeque<Vec<u8>>,
    drops: DatagramDrops,
}

impl Datagrams {
    /// Queue a datagram for transmission, evicting the oldest if the queue
    /// is at [`DATAGRAM_SEND_QUEUE`](constants::DATAGRAM_SEND_QUEUE).
    ///
    /// Returns whether an eviction occurred. The counter is incremented and
    /// the drop traced **here**, because `drops` is this type's field: a
    /// caller-side count would be one more place to forget one, and §11.5
    /// makes the trace core behaviour rather than a shell detail.
    pub(crate) fn push_send(&mut self, data: Vec<u8>) -> bool {
        let evicted = Self::push(&mut self.send, data, constants::DATAGRAM_SEND_QUEUE);
        if evicted {
            self.drops.send += 1;
            trace_drop("send", self.drops.send);
        }
        evicted
    }

    /// Queue an arrived datagram, on [`push_send`](Self::push_send)'s terms —
    /// **except that the trace is rate-limited**, because this side's rate is
    /// the peer's to choose. See [`traces_eviction`].
    pub(crate) fn push_recv(&mut self, data: Vec<u8>) -> bool {
        let evicted = Self::push(&mut self.recv, data, constants::DATAGRAM_RECV_QUEUE);
        if evicted {
            self.drops.recv += 1;
            if traces_eviction(self.drops.recv) {
                trace_drop("recv", self.drops.recv);
            }
        }
        evicted
    }

    /// Take the oldest queued datagram for the §8.5 fill.
    pub(crate) fn pop_send(&mut self) -> Option<Vec<u8>> {
        self.send.pop_front()
    }

    /// Put a datagram back at the **front** — §14.5's refused packet.
    ///
    /// The admission gate runs after the plaintext is packed, so a packet
    /// the congestion window declines must put back exactly what building it
    /// took out. Returning it to the back instead would reorder the queue
    /// against a peer that never asked for ordering — harmless on the wire
    /// (§11.1 promises none) but wrong as an undo, and it would let a
    /// repeatedly-refused datagram cycle to the front of the eviction queue
    /// and be dropped ahead of newer ones.
    pub(crate) fn unpop_send(&mut self, data: Vec<u8>) {
        self.send.push_front(data);
    }

    /// The oldest queued datagram's bytes, to size a frame before
    /// committing to it.
    pub(crate) fn peek_send(&self) -> Option<&[u8]> {
        self.send.front().map(Vec::as_slice)
    }

    /// §16.4's `recv_datagram`: claim the oldest arrived datagram.
    pub(crate) fn pop_recv(&mut self) -> Option<Vec<u8>> {
        self.recv.pop_front()
    }

    /// Whether anything is waiting to go out — feeds
    /// `Connection`'s "is there output owed" question.
    pub(crate) fn has_send(&self) -> bool {
        !self.send.is_empty()
    }

    /// §11.5's counters, for the `#[cfg(test)]` accessor.
    pub(crate) fn drops(&self) -> DatagramDrops {
        self.drops
    }

    /// §15.2: `close()` drops state immediately, and §11.1 promises nothing
    /// about a queued datagram — so the send queue is **discarded**, never
    /// flushed into the CLOSE packet.
    pub(crate) fn discard_send(&mut self) {
        self.send.clear();
    }

    /// The one place the bound is written.
    fn push(queue: &mut VecDeque<Vec<u8>>, data: Vec<u8>, bound: usize) -> bool {
        debug_assert!(bound > 0, "§11.3's queues admit at least one datagram");
        let evicted = queue.len() >= bound;
        if evicted {
            queue.pop_front();
        }
        queue.push_back(data);
        evicted
    }
}

/// Whether the `count`-th eviction on a queue earns a record: **1, 2, 4, 8,
/// 16, …**
///
/// **[F5]** §11.5 requires that the drop be **visible** and says nothing
/// about the **rate** — working rule 8's shape, and the two are separable.
/// A peer that sends datagrams this application never claims produces one
/// eviction per datagram past `DATAGRAM_RECV_QUEUE`, so a record per
/// eviction hands the peer a log-write amplifier: ~31 B on the wire buys a
/// formatted structured record on our disk, at the peer's chosen rate,
/// forever, per connection. Every record carries the same message text, so
/// a backend that de-duplicates on the message does not save us either.
///
/// Powers of two keep §11.5's visibility whole — the **first** eviction is
/// still reported the instant it happens, and the cumulative total rides
/// every record, so an operator loses no information about magnitude — and
/// bound the records at `log₂(n)`: a peer that forces a billion evictions
/// buys thirty lines.
///
/// No constant and no configuration: §11.5 names neither, and inventing a
/// knob for a bound nothing tunes is scope this finding does not carry.
///
/// `pub(crate)` for one reason: the backoff is otherwise **untestable**.
/// Its only other effect is a `tracing` record, the crate has no
/// subscriber-capture dev-dependency, and [`Datagrams::drops`] counts every
/// eviction whether or not it was traced — so without this seam a test can
/// assert the counter (unchanged by this fix) and nothing else, which is
/// working rule 9's "a name is not a pin" in advance.
pub(crate) fn traces_eviction(count: u64) -> bool {
    count.is_power_of_two()
}

/// §11.5's obligation: *"A silent drop is a known operability weakness of
/// the precedent and is deliberately not copied."*
///
/// `warn!` and not `debug!`: a default subscriber sits at `INFO`, so a
/// lower level would leave the drop silent for exactly the operator §11.5
/// is written for. The cumulative counter rides every record, because
/// §11.5 asks for the *counter* and there is no public accessor for it
/// (§16.2's list is exhaustive — working rule 8).
///
/// **The two callers differ, deliberately.** `push_send`'s rate is the
/// application's own — it is not an attack surface, and an application
/// over-producing wants to hear about it every time. `push_recv`'s rate is
/// the peer's, so it goes through [`traces_eviction`]. Rate-limiting both
/// would spend §11.5's visibility where nothing threatens it.
fn trace_drop(queue: &'static str, count: u64) {
    tracing::warn!(
        target: "slither::frames",
        queue,
        drops = count,
        "datagram queue full; oldest evicted (§11.3)"
    );
}
