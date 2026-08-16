//! §13 — loss recovery: the RTT estimator, the sent-packet map, ack-based
//! loss detection and the probe timeout.
//!
//! RFC 9002's shape throughout, over **one** packet-number space: a
//! connection has exactly one session (§7.8), so §7.1's counter is the
//! packet number and there is nothing to key a second space on.
//!
//! # Frames, never packets
//!
//! §13.5: *"A lost packet is never retransmitted as a packet: its
//! still-needed frames are re-framed into new packets under fresh counters
//! (§8.7), STREAM ranges split or merged freely."* [`SentPacket`] therefore
//! holds frame **identities** ([`SentFrame`]) and not bytes, and a
//! retransmission creates a *new* entry under a *new* counter while the old
//! one waits to be acknowledged or declared lost.
//!
//! # What this module does not do
//!
//! It never touches streams and never touches the congestion controller.
//! [`Recovery::on_ack`] returns an [`AckOutcome`] and the **caller** applies
//! it, which is what keeps §12.5's processing, §8.7's retransmission classes
//! and §14's controller in three separately testable places.

use std::collections::BTreeMap;
use std::ops::Range;
use std::time::{Duration, Instant};

use crate::constants;

use super::frame::Ack;
use super::stream_id::Dir;
use super::streams::StreamRef;

/// One ack-eliciting packet in flight. §13.5's record, exactly.
#[derive(Debug, Clone)]
pub(crate) struct SentPacket {
    /// §7.1's counter — slither's packet number. There is one space (§7.8).
    pub(crate) counter: u64,
    /// When it was sealed.
    pub(crate) time_sent: Instant,
    /// **The full datagram length**: `DATA_HEADER_LEN + ciphertext + tag`,
    /// i.e. `Transmit::data.len()` (**ruling 136**). Feeds
    /// `bytes_in_flight` (§14.5).
    ///
    /// §14.5 derives `INITIAL_WINDOW` *"at `MAX_DATAGRAM` = 1200"*, and
    /// `MAX_DATAGRAM` is the datagram: a window expressed in datagram units
    /// must be spent in datagram units. Counting plaintext instead
    /// under-counts by 30 bytes per packet — a standing overshoot that grows
    /// with the window and that no functional test can see.
    pub(crate) size: u64,
    /// §14.5's flag, recorded at send by **us** and read by `on_ack`.
    ///
    /// Recorded per packet rather than consulted at ack time because a peer
    /// controls ACK arrival timing and could otherwise choose when the
    /// predicate is evaluated (§14.5's hostile-peer paragraph).
    pub(crate) app_limited: bool,
    /// **[ruling 137]** §14.6's path-generation stamp. **Held at 0 until
    /// slice 7's roaming exists**, and asserted so.
    ///
    /// It lands now because §14.6's single recovery-period marker cannot
    /// serve §13.6's four fences: `recovery_start` is *also* set by every
    /// ordinary congestion event, so reusing it for the RTT fence would
    /// suppress RTT sampling after every normal loss episode — silently, and
    /// permanently on a lossy path. §14.6 names quinn's path-generation
    /// stamping in the same breath, which is the mechanism that works.
    pub(crate) path_gen: u32,
    /// §13.5's "frame identities aboard".
    ///
    /// **May be empty**: a bare-PING PTO probe is ack-eliciting (§8.3) and
    /// so is tracked (§13.5 excludes only *non*-ack-eliciting packets, and
    /// §17.5's caveat says in terms that both exempt probes are inserted and
    /// counted), but it carries nothing that re-queues.
    pub(crate) frames: Vec<SentFrame>,
}

/// §8.7's three classes, as the identities the map holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SentFrame {
    /// §8.7 `ranges`.
    ///
    /// **[ruling 113]** `fin` is **carried, not inferred**. Inferring it as
    /// `range.end == final_size` is exact for every frame this
    /// implementation emits today and wrong in general, because §8.7 lets a
    /// retransmission split, merge or coalesce ranges freely — so a frame
    /// ending at the final size need not have carried the FIN.
    Stream {
        /// The stream.
        r: StreamRef,
        /// The byte range aboard.
        range: Range<u64>,
        /// Whether this frame carried the FIN.
        fin: bool,
    },
    /// §8.7 `regenerate` — the identity only; the value is re-read at
    /// retransmission time.
    ResetStream {
        /// The stream.
        r: StreamRef,
    },
    /// §8.7 `regenerate`: MAX_DATA.
    MaxData,
    /// §8.7 `regenerate`: MAX_STREAM_DATA.
    MaxStreamData {
        /// The stream.
        r: StreamRef,
    },
    /// §8.7 `regenerate`: MAX_STREAMS_BIDI / MAX_STREAMS_UNI.
    MaxStreams {
        /// The space whose cumulative limit was granted.
        dir: Dir,
    },
}

/// §13's per-connection recovery state.
pub(crate) struct Recovery {
    /// §13.5's map, keyed by §7.1's counter. A `BTreeMap` because §12.5's
    /// intersecting processing is a range query per ACK block.
    sent: BTreeMap<u64, SentPacket>,
    rtt: RttEstimator,
    largest_acked: Option<u64>,
    /// §13.2's `Loss` deadline. `None` when no survivor is inside the
    /// threshold.
    loss_time: Option<Instant>,
    /// §13.3's anchor: the last ack-eliciting **send**.
    last_ack_eliciting: Option<Instant>,
    pto_count: u32,
    /// Maintained incrementally; `debug_assert`ed equal to the map sum.
    bytes_in_flight: u64,
    /// **[ruling 137, live since slice 7]** §14.6's path generation, held
    /// **here** and nowhere else.
    ///
    /// The connection stamps it onto every [`SentPacket`] at seal time and
    /// [`on_roam`](Self::on_roam) is the only thing that moves it, so the
    /// stamp and the two fences that read it cannot drift apart — which is
    /// the whole failure mode a second copy on the connection would
    /// reintroduce.
    path_gen: u32,
}

/// What one ACK's processing produced, handed to the caller so the caller —
/// not this module — touches streams and the controller.
#[derive(Debug, Default)]
pub(crate) struct AckOutcome {
    /// Frame identities on newly acknowledged packets, in ascending counter
    /// order.
    pub(crate) acked: Vec<SentFrame>,
    /// Frame identities on newly declared-lost packets, ascending.
    pub(crate) lost: Vec<SentFrame>,
    /// One entry per newly acknowledged packet, for §14's `on_ack`:
    /// `(sent_time, bytes, app_limited)`.
    ///
    /// A tuple rather than a named struct because `CONTRACT-5a.md` §2.2
    /// writes it as one and two blind authors compile against that text.
    pub(crate) ack_events: Vec<(Instant, u64, bool)>,
    /// `Some` iff a loss episode occurred: §14.3's **once per episode**
    /// congestion event.
    ///
    /// An `Option` rather than a `Vec` **is** the structural enforcement of
    /// §14.3's *"once per loss episode (after the full lost-packet scan),
    /// never once per lost packet"*.
    pub(crate) congestion: Option<CongestionEvent>,
}

/// §14.3's congestion event, raised once per loss episode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CongestionEvent {
    /// The **earliest** lost packet's send time — §14.3's fence.
    pub(crate) sent_time: Instant,
    /// §14.4's verdict, computed inside §13.2's walk.
    pub(crate) is_persistent: bool,
    /// Total bytes of the lost packets.
    pub(crate) lost_bytes: u64,
}

/// The largest exponent `2^n` may take before `PTO_BACKOFF_CAP` binds.
///
/// **Derived from the constant, never transcribed.** `PTO_BACKOFF_CAP` is
/// **64 — the multiplier, not the exponent** (`constants.rs`, and SPEC's
/// table writes it "2⁶"). v0.1 wrote `1u32 << pto_count.min(PTO_BACKOFF_CAP)`
/// with its own `PTO_BACKOFF_CAP = 6`; copying that idiom with slither's
/// constant shifts by up to 64, which is undefined behaviour on `u32`.
const PTO_MAX_EXPONENT: u32 = constants::PTO_BACKOFF_CAP.trailing_zeros();

const _: () = assert!(constants::PTO_BACKOFF_CAP.is_power_of_two());
const _: () = assert!(PTO_MAX_EXPONENT == 6);

impl Recovery {
    /// Nothing in flight, no samples, no backoff.
    pub(crate) fn new() -> Self {
        Self {
            sent: BTreeMap::new(),
            rtt: RttEstimator::new(),
            largest_acked: None,
            loss_time: None,
            last_ack_eliciting: None,
            pto_count: 0,
            bytes_in_flight: 0,
            path_gen: 0,
        }
    }

    /// §14.6's current path generation — what a fresh seal is stamped with.
    pub(crate) fn path_gen(&self) -> u32 {
        self.path_gen
    }

    /// Record one **ack-eliciting** packet (§13.5).
    ///
    /// Arms nothing: the caller re-reads [`loss_deadline`](Self::loss_deadline)
    /// and [`pto_deadline`](Self::pto_deadline) after every mutating call.
    pub(crate) fn on_sent(&mut self, packet: SentPacket) {
        // **[ruling 137, discharged by slice 7]** The `debug_assert_eq!` that
        // pinned `path_gen` at 0 stood here, as the tripwire that would fire
        // the moment the field went live. It has: §7.3's roaming stamps the
        // connection's current generation at seal time, and its removal is
        // the marker that this fence is now load-bearing rather than
        // reserved.
        self.last_ack_eliciting = Some(packet.time_sent);
        self.bytes_in_flight += packet.size;
        let counter = packet.counter;
        let previous = self.sent.insert(counter, packet);
        debug_assert!(
            previous.is_none(),
            "§7.1: a counter is sealed once, so the map cannot collide at {counter}"
        );
        self.debug_check_in_flight();
    }

    /// §12.5's processing. `highest_sealed` is `next_counter() - 1`.
    ///
    /// Returns [`AckOutcome::default`] — a total no-op — when `ack.largest >
    /// highest_sealed` (§12.5's **ignore whole**, with a trace) and when
    /// nothing in the ACK is newly acknowledged.
    ///
    /// # Bounded intersecting processing
    ///
    /// §12.5: *"a received ACK is intersected with the sender's in-flight
    /// set, never materialised into the counters its ranges imply — a
    /// wire-legal ACK whose 64 ranges span millions of counters costs
    /// O(in-flight × range_count), never a multi-megabyte expansion."* Each
    /// block is one `BTreeMap::range` query, so the cost is
    /// O(in-flight + range_count · log n) and a 2⁴⁰-counter span costs the
    /// same as a 1-counter one.
    pub(crate) fn on_ack(&mut self, now: Instant, ack: &Ack, highest_sealed: u64) -> AckOutcome {
        if ack.largest > highest_sealed {
            // §12.5's **deliberate divergence** from RFC 9000 §13.1's
            // SHOULD-treat-as-PROTOCOL_VIOLATION: under bounded intersecting
            // processing the forged-future ACK is already harmless, so the
            // no-op keeps the failure local. The packet's other frames still
            // apply (§8.2's parse/apply split) — which is the caller's job,
            // and is why this returns rather than killing.
            tracing::debug!(
                target: "slither::frames",
                largest = ack.largest,
                highest_sealed,
                "an ACK above the highest counter sealed is ignored whole"
            );
            return AckOutcome::default();
        }

        // ── intersect, never materialise ────────────────────────────────
        let mut newly: Vec<u64> = Vec::new();
        for block in ack.ranges_desc() {
            newly.extend(self.sent.range(block).map(|(counter, _)| *counter));
        }
        // The blocks descend and do not overlap, so this is a reversal
        // rather than a sort in every case §12.1 admits.
        newly.sort_unstable();

        // §13.1: "newly acknowledged" is load-bearing. If this ACK's
        // `largest` was already acknowledged by an earlier ACK there is
        // **no** sample, even when the frame newly acknowledges others.
        //
        // §13.1's second clause — *"and at least one newly acknowledged
        // packet is ack-eliciting"* — is **vacuous and is not implemented**
        // (ruling 138): §13.5 never inserts a non-ack-eliciting packet, so
        // every packet reachable here is one. Building the tracking to
        // evaluate it would be machinery for a condition that is always
        // true.
        // §12.5: "Duplicate acknowledgment of a counter is a no-op." An ACK
        // that resolves nothing resolves nothing — including `pto_count`,
        // which §13.3 resets on a **newly** acknowledged packet.
        if newly.is_empty() {
            return AckOutcome::default();
        }
        let sample_from = newly
            .last()
            .filter(|counter| **counter == ack.largest)
            .copied();

        let mut outcome = AckOutcome::default();
        // §13.6's RTT fence needs the *sampled packet's* generation, and the
        // packet is consumed by the loop below, so it is carried out of it.
        let mut sample_path_gen = None;
        for counter in &newly {
            let packet = self
                .sent
                .remove(counter)
                .expect("the counter came from this map and nothing removed it since");
            self.bytes_in_flight -= packet.size;
            if sample_from == Some(*counter) {
                sample_path_gen = Some(packet.path_gen);
            }
            outcome
                .ack_events
                .push((packet.time_sent, packet.size, packet.app_limited));
            outcome.acked.extend(packet.frames);
        }

        if let Some(counter) = sample_from {
            let (sent_at, _, _) = *outcome
                .ack_events
                .last()
                .expect("newly is non-empty, so at least one event was pushed");
            debug_assert_eq!(counter, ack.largest);
            // **[ruling 172]** §13.6's first `path_gen` fence: *"no RTT
            // sample"* from a packet sent on the old path. Its round trip
            // measures a path this connection is no longer on, and §13.1
            // keeps the estimator across a roam only *"as a prior, not a
            // fact"* — feeding it an old-path sample would make it a fact
            // again. `min_rtt` has just been re-seeded to `None`, so a
            // pre-roam sample would additionally *define* the new floor from
            // the old path.
            if sample_path_gen == Some(self.path_gen) {
                self.rtt.sample(
                    now.saturating_duration_since(sent_at),
                    Duration::from_micros(ack.ack_delay),
                );
            }
        }

        // §13.3: "`pto_count` resets to 0 whenever **any** packet is newly
        // acknowledged" — not only when the probe's own packet is.
        self.pto_count = 0;
        self.largest_acked = Some(match self.largest_acked {
            Some(previous) => previous.max(ack.largest),
            None => ack.largest,
        });

        let (lost, congestion) = self.detect_lost(now, &newly);
        outcome.lost = lost;
        outcome.congestion = congestion;
        self.debug_check_in_flight();
        outcome
    }

    /// §13.2's timer firing: the same walk with no new acknowledgement.
    pub(crate) fn on_loss_timeout(&mut self, now: Instant) -> AckOutcome {
        let (lost, congestion) = self.detect_lost(now, &[]);
        self.debug_check_in_flight();
        AckOutcome {
            lost,
            congestion,
            ..AckOutcome::default()
        }
    }

    /// §13.3's timer firing. Increments `pto_count`, saturating at the
    /// exponent whose multiplier is `PTO_BACKOFF_CAP`, and returns nothing:
    /// the **caller** builds the probe from §13.4's rule.
    ///
    /// **[ruling 139(a)]** The increment is at the *firing*, before the
    /// probe is built — RFC 9002's point, and the one that stays right in
    /// slice 7, where §7.3's anti-amplification budget can stop a probe
    /// leaving and an increment tied to transmission would stall the
    /// backoff at an unvalidated address.
    pub(crate) fn on_pto_timeout(&mut self) {
        self.pto_count = self.pto_count.saturating_add(1).min(PTO_MAX_EXPONENT);
    }

    /// The `Loss` deadline, or `None`.
    pub(crate) fn loss_deadline(&self) -> Option<Instant> {
        self.loss_time
    }

    /// The `Pto` deadline.
    ///
    /// `None` iff the sent map is empty — §13.3's precondition, *"the `Pto`
    /// timer is armed only while at least one ack-eliciting packet is in the
    /// sent map"*. Without it an idle connection self-sustains a probe train
    /// at ~20 packets/s against the 10 s keepalive cadence, which §13.3
    /// names as the failure the precondition exists to prevent.
    pub(crate) fn pto_deadline(&self) -> Option<Instant> {
        if self.sent.is_empty() {
            return None;
        }
        let anchor = self.last_ack_eliciting?;
        let multiplier = 1u32 << self.pto_count.min(PTO_MAX_EXPONENT);
        debug_assert!(multiplier <= constants::PTO_BACKOFF_CAP);
        let interval = self
            .rtt
            .pto_interval()
            .checked_mul(multiplier)
            .unwrap_or(Duration::MAX);
        anchor.checked_add(interval)
    }

    /// §14.5's sum over the map — ack-eliciting packets only.
    pub(crate) fn bytes_in_flight(&self) -> u64 {
        self.bytes_in_flight
    }

    /// Whether anything is in flight.
    pub(crate) fn is_empty(&self) -> bool {
        self.sent.is_empty()
    }

    /// §13.1's estimator.
    pub(crate) fn rtt(&self) -> &RttEstimator {
        &self.rtt
    }

    /// §13.6's roam seam. Keeps the map — *"ACKs for packets in flight to
    /// the old address still resolve, and `bytes_in_flight` remains
    /// consistent with the retained map"* — and re-seeds `min_rtt` on the
    /// next sample, because §13.1 requires it to be **allowed to rise** or
    /// an old short path pins the PTO floor under a new long one.
    ///
    /// It also advances §14.6's path generation, which is **ruling 172's
    /// half of the 2/2 fence split**: from here on, a `SentPacket` stamped
    /// with an older generation feeds **no RTT sample** and takes **no part
    /// in §14.4's persistent-congestion walk**. The other half —
    /// §14.3's congestion event and §14.5's `app_limited` growth — is fenced
    /// by `recovery_start`, which [`NewReno::reset`] sets to the same
    /// instant.
    ///
    /// `recovery_start` cannot serve these two: it is *also* set by every
    /// ordinary congestion event, so reusing it would suppress RTT sampling
    /// after every normal loss episode — silently, and permanently on a
    /// lossy path.
    ///
    /// **What still happens to a pre-roam packet**: it resolves for loss and
    /// retransmission normally, its frames re-queue by §8.7's class, and it
    /// leaves `bytes_in_flight` when acked or declared lost. The fences
    /// suppress *feedback*, never *recovery*.
    ///
    /// [`NewReno::reset`]: super::congestion::NewReno::reset
    pub(crate) fn on_roam(&mut self, now: Instant) {
        let _ = now;
        // Saturating for the reason the connection's roam commit states: a
        // wrap to 0 would alias the initial generation and un-fence the
        // oldest packets in the map.
        self.path_gen = self.path_gen.saturating_add(1);
        self.rtt.reseed_min_rtt();
    }

    // ═══════════════════════════════════════════════════════════════════
    // §13.2's walk
    // ═══════════════════════════════════════════════════════════════════

    /// §13.2's loss detection, and §14.4's persistent-congestion verdict
    /// computed inside it rather than bolted on.
    ///
    /// `newly_acked` is what this evaluation acknowledged, ascending — the
    /// set §14.4's *"no packet acknowledged between them"* is checked
    /// against. It is empty on the `Loss`-timer path, where nothing was
    /// acknowledged by definition.
    fn detect_lost(
        &mut self,
        now: Instant,
        newly_acked: &[u64],
    ) -> (Vec<SentFrame>, Option<CongestionEvent>) {
        // Recomputed from scratch on every walk. A `loss_time` carried over
        // from a previous walk arms a timer for a packet that has since been
        // acknowledged or declared lost, which fires and declares nothing.
        self.loss_time = None;

        let Some(largest_acked) = self.largest_acked else {
            // §13.2's precondition: a packet is lost only when *a later
            // packet in its space has been acknowledged*. Before the first
            // ACK nothing is judged, and only the PTO can rescue the flight
            // (§13.3).
            return (Vec::new(), None);
        };

        let loss_delay = self.rtt.loss_delay();
        let mut lost: Vec<u64> = Vec::new();

        // **[ruling 131]** The walk ranges over packets at or below
        // `largest_acked` and **nothing above it**. §13.2's parenthetical
        // once read "minimum across in-flight packets", which includes
        // entries the walk does not judge at all; the two readings diverge
        // whenever anything newer than `largest_acked` is outstanding — the
        // ordinary case during a transfer — and the wide one arms a timer
        // that fires and declares nothing.
        for (counter, packet) in self.sent.range(..=largest_acked) {
            let by_count = largest_acked - counter >= constants::K_PACKET_THRESHOLD;
            // **[ruling 139(f), REVERSED 2026/08/16]** `>=`, not `>`.
            //
            // §13.2's prose says *"sent **more than** `loss_delay` before
            // the acknowledgment arrived"*, and 139(f) originally took that
            // literally over v0.1's `>=`. Its own next sentence overrides
            // it: the `Loss` timer arms at **`time_sent + loss_delay`**, so
            // at the firing instant the packet's age *equals* `loss_delay`
            // and a strict `>` is false — the walk the firing triggers
            // declares nothing and re-arms at the same instant. Not an edge
            // case: **every** firing.
            //
            // Under this core it is worse than a wasted wakeup. The re-arm
            // goes through `sync_recovery_timers`, so `poll_output` then
            // announces a deadline at or before `now`, the shell schedules
            // an immediate wakeup, and the pair spins. It is ruling 131's
            // defect — "a timer that fires and declares nothing" — reaching
            // the same section by a second route.
            let by_time = now.saturating_duration_since(packet.time_sent) >= loss_delay;

            if by_count || by_time {
                lost.push(*counter);
            } else if let Some(at) = packet.time_sent.checked_add(loss_delay) {
                self.loss_time = Some(match self.loss_time {
                    Some(current) => current.min(at),
                    None => at,
                });
            }
        }

        if lost.is_empty() {
            return (Vec::new(), None);
        }

        let is_persistent = self.persistent_congestion(&lost, newly_acked);

        let mut frames = Vec::new();
        let mut lost_bytes = 0u64;
        let mut earliest: Option<Instant> = None;
        for counter in &lost {
            let packet = self
                .sent
                .remove(counter)
                .expect("the counter came from this map and nothing removed it since");
            self.bytes_in_flight -= packet.size;
            lost_bytes += packet.size;
            earliest = Some(match earliest {
                Some(current) => current.min(packet.time_sent),
                None => packet.time_sent,
            });
            frames.extend(packet.frames);
        }

        let event = CongestionEvent {
            sent_time: earliest.expect("lost is non-empty"),
            is_persistent,
            lost_bytes,
        };
        (frames, Some(event))
    }

    /// §14.4, computed inside §13.2's walk.
    ///
    /// *"if two ack-eliciting packets sent more than `persistent_period =
    /// PTO × PERSISTENT_CONGESTION_THRESHOLD` apart are both lost with **no
    /// packet acknowledged between them**, and **a prior RTT sample
    /// exists**"*.
    ///
    /// `persistent_period` evaluates §13.3's formula with **`pto_count =
    /// 0`**: §14.4 says the backoff is deliberately excluded *"so the period
    /// is a property of the path, not of the probe count — with the backoff
    /// included, the threshold would run up to 2⁶× too long and persistent
    /// congestion would never trigger under exactly the sustained loss it
    /// exists to detect."*
    ///
    /// The `has_sample` guard is §14.4's own — *"the pre-sample
    /// `K_INITIAL_RTT` phase never triggers it"* — and without it a
    /// first-flight blackhole collapses the window on the initial-RTT guess.
    ///
    /// # Is the guard reachable in slice 5?
    ///
    /// It is close to vacuous but **not** vacuous, and the distinction is
    /// worth recording because the near-miss invites deleting it.
    ///
    /// The walk is reached from [`on_ack`](Self::on_ack) and from
    /// [`on_loss_timeout`](Self::on_loss_timeout), and both need
    /// `largest_acked`, so an ACK must have arrived. That ACK usually takes
    /// a sample — but only when its `largest` is itself newly acknowledged
    /// (§13.1, and ruling 138 on why the ack-eliciting clause is not tested
    /// separately). When our highest counter is a packet §13.5 never
    /// inserted — a standalone pure ACK, which §12.4 emits whenever one is
    /// owed and nothing else is pending — the peer's `largest` names it, it
    /// is not in the map, **no sample is taken**, and the walk below still
    /// runs on the counters underneath it.
    ///
    /// So the guard can fire on a first ACK in ordinary bidirectional
    /// traffic. It becomes load-bearing rather than incidental in slice 7,
    /// where §13.6's roam fence excludes pre-roam packets from sampling.
    fn persistent_congestion(&self, lost: &[u64], newly_acked: &[u64]) -> bool {
        if !self.rtt.has_sample() {
            return false;
        }
        let Some(period) = self
            .rtt
            .pto_interval()
            .checked_mul(constants::PERSISTENT_CONGESTION_THRESHOLD)
        else {
            return false;
        };

        let mut run_start: Option<Instant> = None;
        let mut previous: Option<u64> = None;
        for counter in lost {
            let packet = &self.sent[counter];
            // **[ruling 172]** §13.6's second `path_gen` fence, and §14.4
            // states the obligation without naming a mechanism: *"packets
            // sent before a roam are excluded from the walk."* Excluded, not
            // "not-lost" — the packet still resolves for retransmission
            // above; it simply takes no part in the verdict. A run that
            // straddles a roam would otherwise collapse the window on
            // evidence gathered from a path the connection has left, which
            // is the one direction §14.6's reset exists to prevent.
            if packet.path_gen != self.path_gen {
                continue;
            }
            let sent_at = packet.time_sent;
            // A run is broken by any counter between two lost packets that
            // was **acknowledged** rather than lost. A counter still in
            // flight breaks nothing: it is not acknowledged.
            let broken = match previous {
                None => true,
                Some(previous) => newly_acked
                    .iter()
                    .any(|acked| *acked > previous && *acked < *counter),
            };
            if broken {
                run_start = Some(sent_at);
            }
            if let Some(first) = run_start
                && sent_at.saturating_duration_since(first) > period
            {
                return true;
            }
            previous = Some(*counter);
        }
        false
    }

    fn debug_check_in_flight(&self) {
        debug_assert_eq!(
            self.bytes_in_flight,
            self.sent.values().map(|p| p.size).sum::<u64>(),
            "§14.5: bytes_in_flight is the sum of the map's sizes"
        );
    }
}

/// §13.1's estimator: `latest_rtt`, `smoothed_rtt`, `rttvar`, `min_rtt`.
///
/// Integer arithmetic throughout — §14.2's *"no floating point"* is stated
/// for the controller and holds here for the same reason: a transport's
/// timers must be reproducible across platforms.
#[derive(Debug, Clone)]
pub(crate) struct RttEstimator {
    /// §8-H3: `latest_rtt = ack_arrival − time_sent(largest_newly_acked)`.
    /// §13 names the quantity and never defines it; this is RFC 9002 §5.1's
    /// definition, and it is also the input to §13.2's
    /// `max(smoothed_rtt, latest_rtt)`.
    latest: Duration,
    /// `None` before the first sample.
    smoothed: Option<Duration>,
    rttvar: Duration,
    /// `None` before the first sample **and** after a roam re-seed (§13.1).
    min_rtt: Option<Duration>,
}

impl Default for RttEstimator {
    fn default() -> Self {
        Self::new()
    }
}

impl RttEstimator {
    /// Seeded from `K_INITIAL_RTT`, with `rttvar = K_INITIAL_RTT / 2`.
    pub(crate) fn new() -> Self {
        Self {
            latest: Duration::ZERO,
            smoothed: None,
            rttvar: constants::K_INITIAL_RTT / 2,
            min_rtt: None,
        }
    }

    /// §13.1. `ack_delay` is the peer's raw report; the cap and the
    /// `min_rtt` guard are applied here.
    pub(crate) fn sample(&mut self, latest: Duration, ack_delay: Duration) {
        self.latest = latest;

        let Some(smoothed) = self.smoothed else {
            // First sample.
            self.smoothed = Some(latest);
            self.rttvar = latest / 2;
            self.min_rtt = Some(latest);
            return;
        };

        // §13.1: `min_rtt = min(min_rtt, latest)`, except immediately after
        // a roam, where it is re-seeded and **MUST be allowed to rise** —
        // otherwise an old short path pins the PTO floor under a new long
        // one and manufactures spurious probes for the connection's life.
        let min_rtt = match self.min_rtt {
            Some(current) => current.min(latest),
            None => latest,
        };
        self.min_rtt = Some(min_rtt);

        // *"the peer's `ack_delay`, capped at `MAX_ACK_DELAY`, is subtracted
        // **only when doing so does not push the sample below `min_rtt`**"*.
        let capped = ack_delay.min(constants::MAX_ACK_DELAY);
        let adjusted = if latest >= min_rtt + capped {
            latest - capped
        } else {
            latest
        };

        // §13.1's `|smoothed_rtt − adjusted|`.
        let deviation = smoothed.abs_diff(adjusted);
        self.rttvar = self.rttvar * 3 / 4 + deviation / 4;
        self.smoothed = Some(smoothed * 7 / 8 + adjusted / 8);
    }

    /// `K_INITIAL_RTT` before any sample.
    pub(crate) fn smoothed_rtt(&self) -> Duration {
        self.smoothed.unwrap_or(constants::K_INITIAL_RTT)
    }

    /// `K_INITIAL_RTT / 2` before any sample.
    pub(crate) fn rttvar(&self) -> Duration {
        self.rttvar
    }

    /// The smallest sample seen since the last re-seed.
    pub(crate) fn min_rtt(&self) -> Option<Duration> {
        self.min_rtt
    }

    /// §14.4's precondition: *"a prior RTT sample exists"*.
    pub(crate) fn has_sample(&self) -> bool {
        self.smoothed.is_some()
    }

    /// §13.2's `max(9/8 · max(smoothed_rtt, latest_rtt), K_GRANULARITY)`.
    ///
    /// The `max` with `latest_rtt` is load-bearing: on a rising path
    /// `smoothed_rtt` lags, and a threshold built from it alone declares
    /// stragglers lost that are merely late.
    pub(crate) fn loss_delay(&self) -> Duration {
        let base = self.smoothed_rtt().max(self.latest);
        (base * 9 / 8).max(constants::K_GRANULARITY)
    }

    /// §13.3's formula **with `pto_count = 0`** — §14.4 uses exactly this.
    pub(crate) fn pto_interval(&self) -> Duration {
        self.smoothed_rtt()
            + (self.rttvar * 4).max(constants::K_GRANULARITY)
            + constants::MAX_ACK_DELAY
    }

    /// §13.1's roam clause: `min_rtt` MUST be allowed to rise, so the next
    /// sample re-seeds it rather than being clamped by the old path's floor.
    ///
    /// The estimator itself survives the roam — §13.1 keeps it *"as a prior,
    /// not a fact"*.
    pub(crate) fn reseed_min_rtt(&mut self) {
        self.min_rtt = None;
    }
}
