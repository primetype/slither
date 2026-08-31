# slither — protocol specification, wire version 1 (first release)

> **Status: RATIFIED 2026/08/14. Wire version 1 (`VERSION` `0x01`).**
>
> **[Stamped 2026/08/16 — ruling 242.]** Every section header of this
> document carried a `DRAFT 2026/08/13` marker and this block read
> *"DRAFT v6 … unratified"* for two days after the protocol was ratified,
> while `CLAUDE.md` described the same file as *"the ratified protocol for
> slither's wire — version 1 … Ratified 2026/08/14"*. The markers were
> simply never swept.
>
> **Amended after ratification. None of it has been released** — v0.2 is
> unpublished and `bubble-engine` is the only consumer — which is the
> standing reason each amendment was affordable. *Appendix B's bullets are
> unnumbered and are cited by their bold title (ruling 274). The
> `O13`/`O53a`/`O53b` ids that rulings 260, 268 and 270 used were the
> round-41 audit's private numbering (`audit/G-obligations-trend.md`,
> never committed and no longer extant), positional over Appendix B as of
> ratification; they resolve to the **post-mortem pin**, the
> **ACK-loss-burst simulation**, and the **window-constants throughput
> sanity check** respectively.*
>
> | Ruling | Where | What moved |
> |---|---|---|
> | **208** | §8.3, §8.4, §7.3 | **the wire**: `PATH_CHALLENGE` `0x1a` and `PATH_RESPONSE` `0x1b`, nine bytes each. **No existing byte moved** — every golden vector is byte-identical. |
> | 221 | §13.4, §8.3 | a PTO probe to an unvalidated address carries the challenge, not a bare PING |
> | 227 | §16.11.1 (new) | the nine-variant `io::ErrorKind` table, replacing a slash with no rule for choosing |
> | 229 | §16.11, Appendix B | the adapter face list gains `incoming`; `Endpoint` gains `poll_accept` |
> | 232 | Appendix B | the composability obligations, which two documents had claimed were already there |
> | 238 | §16.11.1 | the `WriteError` match is exhaustive; the fallback arm hid the future rather than guarding it |
> | 241 | §9.8, §9.9 | the peer-reset latch is scoped to unidirectional streams |
> | 249 | §13.3, §13.4, §13.6, §16.5 | the `Pto` deadline is announced only while §7.3's budget admits a probe — the livelock the roam seam could construct; the driver's past-deadline guard goes release-mode |
> | 250 | §7.3, §14.5, §15.4 | the probe coalesces the owed path frames, and §7.3's arithmetic holds at pump time, not the arming instant; two dead flag references swept |
> | 251 | §7.7, Appendix B | §7.7's obligations exist: the boundary, the straggler window, S23 — and the `REKEY(0³²)` vector pin has a named home |
> | 252 | §6.4, §6.5, §6.8 | drain `accept()` is every application's obligation, not the dialler's — a lost msg2 is only closed by the next accept (S34) |
> | 253 | §10.6 | coalesce-on-insert gains its work bound, O(credit · log credit), and capacity stays the arrived span |
> | 254 | §13.3, §14.4, the constants tables | `PTO_BACKOFF_CAP` 2⁶ → 2³: the ladder fits inside `DEAD_TIMEOUT`'s window; survival envelope stated |
> | 256 | §7.7, Appendix B | retention is not reach: the previous-epoch key delivers only within `REPLAY_WINDOW` counters of the boundary — one part in 32 — and the straggler pins must be built below it |
> | 258 | §7.3, §8.5, §12.4, §14.5 | what rides the contested probe's packet: coalescing is all-or-nothing over the owed set; the owed ACK rides behind, not aboard; no fill — the 18 B bound is the exemption's proof; §8.5's collision sentence scoped to the PTO probe |
> | 259 | §10.2, §16.2, §16.4, §16.11 | ruling 248's listings: `SendCreditAvailable` enters both `ConnEvent` lists with the counting rule; `join` is fallible in §16.11 as ruling 120 ratified; (viii)'s config-raisable receive windows enter §10.2 (2026/08/18) |
> | 260 | Appendix B | Appendix B's **ACK-loss-burst simulation** discharged by measurement (spurious ≤ 1.55 %, envelope 2.5 %); the **window-constants throughput sanity check**'s quinn bar ruled untestable, its no-stall clause pinned in virtual time |
> | 261 | §6.3, §18.1, §18.2 | `IntroError::Evicted` splits cap-pressure from TTL; the eviction events enter `slither::policy` |
> | 262 | §16.4 | the cores expose `next_deadline(&self)`: the deadline is read, never popped — the driver's destroy-arms disappear (a reentrant inline-executor consumer could reach them and silently lose a datagram in release) |
> | 264 | §6.3, §16.4 | the NAT'd-population clause on the per-source cap, and the `Identity` seam defined where its bound is used |
> | 265 | §7.5, §13.6, §16.5, Appendix B | neither keepalive deadline is announced while §7.3's budget or a pending mark holds it, and a vetoed keepalive announces the death clock — the measured immortal-park ends in death or recovery; §16.5's "both wait" was two timers and is four |
> | 267 | §6.1 | `authenticate()` is idempotent on a `Proven` chain — same peer, same timestamp, 0 DH; the symmetric clause `read_identity()` already had |
> | 268 | Appendix B | Appendix B's **post-mortem pin** obligation — its flush parenthetical becomes admission-driven: the authenticate-then-drop flood it named mints no entries, by mitigation (i)'s own design |
> | 269 | §10.2 | the window-limited cap is ≈ window/(2 × RTT) — §10.3's half-window re-grant is the factor; the 259(viii) clause read window/RTT until the benchmark measured it (`bench-vs-tcp-2026-08.md`) |
> | 270 | §10.2 kinds, §10.5, §10.6, §17.5, Appendix B | the reassembly ceiling derives from the advertised credit — max(`REASSEMBLY_CHUNKS_MAX`, W/`REASSEMBLY_MIN_CONFORMING_FRAME`+1); a conforming full-frame sender inside its credit can no longer be killed by loss; the flood still dies 512× above the ceiling; the **window-constants throughput sanity check**'s gate recorded run |
| 271 | §12.4, §16.5 | the ACK emission point: due stays every-2nd, emission coalesces to the receive-drain boundary (at most one per drain per `ACK_COALESCE_MAX` — 32, new, named; gap the sole exception); §16.5's drain-before-deadline order becomes normative. **No wire byte moves; the wire mix does**: ACK-only datagrams 33.6 % → 3.4 %, bulk ~88 → 108–115 MiB/s default, 129–135 raised |
| 272 | §6.3, §17.5 | the stage-0 per-entry figure becomes the measured ≈ 484 B / ≈ 496 KB at the cap (was ≈ 220 B / ≈ 225 KB — a stated construction whose scope excluded the entry's own fields); pinned by a core unit test |
| 273 | §8.2 | the structural-failure consequence is scoped to a live connection: while closing/draining a violation is ignored entirely, and at most one structural trace fires per connection — what the code always did, now a rule |
| 274 | amendment table | rulings 260/268/270's `O13`/`O53a`/`O53b` were dangling pointers into a never-committed audit file; the rows now cite Appendix B by bold title, with the provenance recorded above |
| 275 | §18.2, STORIES.md §S22 | WARN ratified as the level of §18.2's failure events; S22's anchor line gains §4, §6.1 (clause 4's whole mechanism was unanchored); the `mod owed` registry discharged and the SECV5-5/6/8 pins written |
| 282 | §5.7, §7.4, §7.5, §16.2, §16.4, §16.5, §18.1, Appendix B, constants tables | an endpoint-wide validated `TimingProfile` may vary established-session liveness; the v1 10 s / 25 s constants remain the exact default, every other timer remains fixed, and no wire field or negotiation is added |
| 283 | §5.7, §7.5, Appendix B | ruling 282's one-loss validation counts two shell-lateness allowances because each successive keepalive firing can independently be late; defaults and wire remain unchanged |
| 284 | §5.7, §7.5, §13.3, §16.4, §16.5, §17.5, Appendix B | every deadline derivation checks all `Duration` intermediates and the final `Instant + Duration`; a sum beyond the platform horizon remains logically enabled where state requires but is unreachable and unannounced — never panicked, wrapped, saturated, or replaced by an earlier timeout; later state changes recompute it; constructor-time profile rejection remains, and representable/default/wire behaviour is unchanged |
> This document is the complete specification of the slither protocol at
> **wire version 1 — the first released wire**. It supersedes all prior
> slither wire and specification text **wholesale**: `SPEC.md` (2026/07/16
> and 2026/07/17) and `SPEC-v2.md` (the phase-1 draft) are **pre-release
> drafts** — no released software ever spoke them — with **zero
> authority**, not incorporated by reference, and not compatible with this
> wire. There is no on-wire transition mechanism, because there is nothing
> released to transition from (§1.1). On ratification the DRAFT markers
> flip to RATIFIED and the code must match this file; a later change to
> either is a protocol revision, not an edit. **No maintainer flag
> remains open.** Appendix A.1's no-fallback gate was the last one, and
> it closed when the staged read shipped in hiss 0.3.2 and Appendix A
> was reconciled against the shipped API (§1.3, Appendix A); every other
> flag was ruled in the 2026/08/14 walkthrough. The flag token appeared
> nowhere in this document, and a zero count was part of the ratification
> check. *(**Amended 2026/08/16 by ruling 208**: the count rose to **one**,
> carrying the bracketed marker `FLAGGED FOR RULING`, at its home in §7.3
> and reconciled in §1.3. **Ruling 215 closed it the same day and the
> count is zero again.** Both entries stand as written so the history
> stays legible: the check worked, which is the only evidence that a
> zero count means anything.)*
> The two [OPEN] markers draft v1 carried are
> resolved (§6.4's re-home rule, §15.2's violation surface); **no [OPEN]
> markers remain**. British English, Oxford comma, dates `YYYY/MM/DD`.
>
> **Round-1 revisions applied (draft v1 → v2, 2026/08/13).** All 58 round-1
> review findings, resolved in eight clusters (clusters A and F are
> **partially superseded** by the draft-v4 ratchet-only ruling below):
> **A** — the CONTINUATION/CONTINUED restart-vs-rekey machinery redesigned
> over a three-valued LIVE/PENDING/NONE local state: flag routing gated on
> LIVE, CONTINUED computed (never hardcoded), the `Replaced` teardown
> deferred to `accept()`, mid-state-carrying entries frozen, §6.4's
> candidate-flag discard deleted (§5.4, §6.3–6.8).
> **B** — flow control made a real memory bound: retired receive halves
> credit the connection level, the re-grant formula corrected, a second
> reassembly-fragment bound added, consumption defined (§9–§10, §16.8).
> **C** — an anti-amplification budget (`AMPLIFICATION_FACTOR` = 3) binding
> all output to unvalidated addresses, and pre-roam packets fenced from the
> fresh path's congestion controller (§7.3, §13, §14).
> **D** — exactly-once stream delivery pinned by a per-space closed-stream
> watermark (§9.2, §9.7, §8.4).
> **E** — recovery aligned with RFC 9002: PTO armed only with ack-eliciting
> packets in flight, persistent congestion at the un-backed-off PTO, no
> window growth during recovery (§13, §14).
> **F** — the liveness anchor corrected to receive-keyed death, a
> persistent-keepalive floor, and idle sessions rekeying via the keepalive
> (§7.4–7.6).
> **G** — the core/shell seam completed: closing-state retention, stream IDs
> assigned at establishment, a uniform pull model with claim verbs, the
> wake-event set closed (§15.2, §16).
> **H** — post-AEAD structural frame failure is a signalled death (CLOSE
> with `PROTOCOL_VIOLATION`), `ConnectionLost::ProtocolViolation { code }`
> added, mutual close drains reply-free (§3.1, §8.2, §15, §18.1).
>
> **Final editorial pass (draft v2 → v3, 2026/08/14):** RAC-1, RAC-2,
> RBH-1, RBH-2 — four wire-free re-review fixes (§6.3/§6.9, §7.3,
> §9.6/§9.8/§8.7, §10.3). No wire byte, constant value, frame type, or
> error code moved.
>
> **Walkthrough revisions (draft v3 → v4, 2026/08/14).** Every maintainer
> flag walked 1:1 and ruled (the rulings log:
> `.spec-v2-clean-slate/rulings.md`); all rulings applied in one batch
> revision. The headline rulings: **ratchet-only rekey** — the periodic DH
> re-handshake is deleted; the §7.7 epoch ratchet is the only rekey; a
> connection has exactly one session, and a new handshake from an
> already-live static replaces the connection via `accept()` (§5.4, §6.4,
> §7.6 deleted, §7.8 collapsed); **no version bump** — nothing ever
> shipped, so this wire is the first: `VERSION = 0x01`,
> `PROLOGUE = b"slither\x01"` (§1.1); and, consequent on ratchet-only,
> the **CONTINUATION/CONTINUED flags are deleted** — the msg1 payload is
> the 12-byte timestamp alone, msg2 carries no payload, and the reference
> handshake packets are 196 B and 107 B (§2.3, §5.2). Clusters A and F of
> the round-1 revisions are partially superseded by the ratchet-only
> ruling; their surviving halves are re-ratified inline. Ratification of
> the whole document follows the Appendix A.1 resolution and a focused
> adversarial re-review of the restructured §§5–7.
>
> **Security-fix revisions (draft v4 → v6, 2026/08/14).** Successive
> adversarial re-reviews of the restructured §§5–7 returned findings; the
> maintainer ruled them and they were applied in four passes. Every pass
> is
> **wire-free**: no packet size, header layout, frame type, error variant,
> or constant value moved except `DEAD_TIMEOUT`, which ruling 34 re-values
> deliberately.
> **Pass 1 — rulings 31–32.** The replacement basis becomes explicit
> state: a connection records the basis its admission was decided against,
> and a strictly-newer withheld replay is measured against that basis
> rather than against the guard alone (§6.4, §6.7, §17.4). And `accept()`
> grows its **PENDING** branch, so an initiation arriving against a static
> we are ourselves dialling routes through the simultaneous-open tie-break
> instead of a second, independent admission (§6.5–§6.8).
> **Pass 2 — rulings 33–34, plus three directed fixes.**
> **Ruling 33** — the liveness death clock arms on **any ack-eliciting
> send** as well as on a marking send. The two triggers are independent and
> either suffices; the clock is still armed once per receive and never
> re-armed, so probe trains still cannot defer death. Without this, a
> connection whose only output was quiet-set-but-ack-eliciting could pour
> unacknowledged traffic into a black hole with nothing ever arming the
> clock (§7.4, §7.5, §13.3, §7.3, §7.7).
> **Ruling 34** — **`DEAD_TIMEOUT` rises 15 s → 25 s**, redefined as
> 2 × `KEEPALIVE_TIMEOUT` + 5 s grace. At the old value a *single* lost
> keepalive killed a healthy idle connection; the new value buys explicit
> one-lost-keepalive tolerance, and only two consecutive losses end an idle
> session (§5.7, §7.4, §7.5, and every derived figure across §§6, 13, 15,
> 16, 17 and the constants table). `KEEPALIVE_TIMEOUT` is unchanged at
> 10 s, and `INTRO_TTL` — which happens to share the superseded value — is
> deliberately untouched, including the flood arithmetic derived from it
> (§6.3, §6.9).
> **Directed fixes:** §1.1's old-dev-binary note is restated on the correct
> mechanism — the two wires are separated by a **suite difference** in
> mac1's static keying, not by structure, since their packet sizes are
> identical; §7.7's epoch-death justification is rebuilt on the congestion
> controller's ACK-driven admission gate, which is what actually excludes
> the drift, with liveness as the unconditional backstop; and §7.3 is
> re-attached to `ConnEvent::AddressMoved`, the core event that feeds its
> observability surfaces.
> One coincidence is **flagged and deliberately not acted on**:
> `PERSISTENT_KEEPALIVE`'s 25 s default now sits exactly on the liveness
> floor, with zero margin (§5.7). Idle liveness is sustained by the 10 s
> keepalive dance, not by the persistent beacon, so this is not a defect;
> whether to raise the default is a maintainer call. *(Ruled in pass 3b
> below — ruling 38: the beacon is inert at every admissible interval and
> the default stays 25 s. Ruled again in pass 3c — ruling 40: the
> inertness was the bound's fault, not the knob's; the bound is a
> ceiling, the default drops to 10 s, and the coincidence disappears with
> it.)*
>
> **Security-fix revisions (draft v5 → v6, 2026/08/14).** A second
> adversarial re-review returned findings against the pass-1/pass-2 text
> itself; the maintainer ruled them and they are being applied in three
> passes, all **wire-free** — no packet size, header layout, frame type,
> error variant, timer or constant value moves, the one exception being
> `PERSISTENT_KEEPALIVE`'s recommended default and its handle-side
> validation rule (ruling 40, pass 3c), neither of which is on the wire.
> **Pass 3a — rulings 35 and 37, plus two directed fixes.**
> **Ruling 35** — `accept()`'s **PENDING** branch runs the tie-break
> comparison. Pass 1 gave that branch an unconditional cancel-and-install
> justified on local state, but the tie-break is a **two-sided** agreement
> over the pair of statics and no path may opt out of it unilaterally:
> with both sides on the `read_identity()` → `connect()` → `accept()`
> ordering, both installed as responder over each other's msg1 and the
> pair went mutually dark. The branch now compares: tie-break loser
> cancels its pending and installs as responder (the branch as written);
> tie-break winner returns `AcceptError::Stale`, keeps its pending, and
> records the candidate's timestamp exactly as §6.7's winner side does
> (§5.4, §6.4, §6.6, §6.9, §16.1, §18.1).
> **Ruling 37** — the captured-initiation replay is **bounded honestly**.
> §6.7's flat "each captured initiation is single-use" was false: §17.1's
> own orphan aging and LRU eviction recycle the guard entry that makes it
> single-use. The claim is now qualified per initiation and per surviving
> guard entry, and §17.1 **pins** entries written by a tie-break admission
> or a winner-side record for `HANDSHAKE_GIVEUP` past the death of the
> connection they belong to — a per-static timestamp, not a session, still
> under the 1024-entry cap (§6.7, §17.1). The alternative mitigation
> *refuse to cancel a pending on a vacuous guard pass* is **declined and
> recorded as declined** in §6.7: it breaks genuine first contact.
> **Directed fixes:** §17.1's honesty clause is corrected for a static we
> only ever **dialled** — no guard entry is ever written for one, so there
> is nothing to pin and nothing to evict, the guard bars nothing, the
> `None` basis carries the whole protection, and the spurious-`Intro`
> primitive is indefinite rather than one-shot (§17.1, §6.4); and the
> ordering of the guard's **record** against the basis check is pinned —
> the record reverts on `AcceptError::Stale`, with the tie-break winner's
> record as the one deliberate exception (§6.4, §17.1's mitigation (i)).
> **Pass 3b — rulings 36 and 38, plus three directed fixes.**
> **Ruling 36** — the **contested-connection probe**. §6.8's "a restart
> resolves, delayed by at most `DEAD_TIMEOUT`" rested on an unstated
> premise: that nothing authentic still reaches the zombie. An attacker
> who harvests genuine peer→us Data while dropping it — so our replay
> window never advances past it — can inject one harvested packet every
> less than `DEAD_TIMEOUT` from anywhere off-path, resetting the zombie's
> liveness clock forever and roaming the session to itself, while every
> genuine reconnect is refused against the `None` basis and `connect()`
> reports `ConnectError::AlreadyConnected`: not a delay but a permanent
> wedge, escapable only by an application `close()`. A basis-`None`
> refusal in §6.4 now
> marks the connection **contested** — an ack-eliciting PING, and an ACK
> covering it required within `KEEPALIVE_TIMEOUT` on pain of
> `ConnectionLost::TimedOut` — because withheld genuine Data can reset a
> receive clock but can never acknowledge a packet sent *after* the
> harvest. (**Ruling 41, pass 4, amends the predicate**: what must be
> acknowledged is any counter at or above a recorded **probe floor**, not
> the packet that carried the PING, and concurrent marks collapse into
> one. The security argument is unchanged; the predicate it rests on is
> the one that had to move.) The refusal itself is unchanged and a live peer keeps its
> connection (§6.4, §6.8, §7.4, §7.5, §14.5, §17.4). The root-cause fix,
> a 12-byte timestamp in **msg2** making the basis `Some(t)` on both
> sides, is **considered and declined as wire-affecting** and recorded as
> declined in §17.4: the probe is a mitigation, not a closure.
> **Ruling 38** — `PERSISTENT_KEEPALIVE` is **documented as inert**, and
> nothing moves. §5.7's floor sentence was inverted (the floor rejects
> intervals *below* `DEAD_TIMEOUT`, i.e. short ones, and its job is to
> keep a marking beacon from outpacing the death clock); with it corrected
> the derivation follows in §7.5 — the 10 s passive dance drags
> `last_send` forward and pushes the beacon's deadline with it while a
> connection is receiving, and once it is not, death at
> `last_authenticated_recv + DEAD_TIMEOUT` strictly precedes the beacon's
> `last_send + I ≥ last_authenticated_recv + DEAD_TIMEOUT` for every
> interval the floor admits. The default stays **25 s**, the floor stays
> `DEAD_TIMEOUT`-relative, and the knob is retained; re-basing the floor
> on `KEEPALIVE_TIMEOUT` and deleting the knob are both **declined and
> recorded as declined**. This closes the coincidence the v4 → v5 block
> flagged as an open maintainer call.
> *(Partly **superseded by ruling 40**, pass 3c below. This entry stands
> as written so the history stays legible: its derivation is sound, but
> its diagnosis was inverted — the floor **sentence** it "corrected" was
> the half that was already right, and the floor **rule** was the half
> that was wrong.)*
> **Directed fixes:** the death clock's state **at install** is pinned —
> a new session starts with `last_authenticated_recv` and `last_send` at
> the install instant and the deadline **already armed**, so a half-open
> session dies at 25 s in silence instead of living forever under the
> reading that left it unarmed, which is what §6.7, §17.1 and §15.4
> already assume (§7.4); the one-lost-keepalive tolerance is restated as
> **unidirectional** — a simultaneous bidirectional loss over a single
> interval still ends an idle connection at 25 s (§5.7, §7.5); and §10.3
> carries both halves of ruling 33's redefinition, credit frames being
> non-marking **and** death-clock-arming (§10.3). §7.4 also states the
> general principle the probe is the exception to: the death clock is
> driven by **mere authenticated receipt, not acknowledged progress**, and
> every liveness question in this document inherits that.
> With pass 3b every ruling the maintainer issued against the re-review
> is applied, and the wire is byte-identical to draft v5's.
> **Pass 3c — rulings 39 and 40**, two rulings issued against the pass-3b
> text itself, and the last of this round. Wire-free like the rest: no
> packet byte, header layout, frame type, error variant, packet size or
> timer value moves except `PERSISTENT_KEEPALIVE`'s **recommended
> default** and its **validation rule**, neither of which is on the wire.
> **Ruling 39** — the **idle-from-install drop stands**, and the dance's
> scope is stated instead of derived. Pass 3b's pin — `last_send` =
> `last_authenticated_recv` = the install instant, deadline armed there —
> is confirmed as ruled: a connection that never carries traffic is
> reaped at `DEAD_TIMEOUT` and the application redials. What was missing
> was the scope around it, and §7.5 (reflected in §5.7) now says it
> plainly: the passive dance is **automatic for any connection that has
> carried traffic**, since one exchange in either direction puts `R > S`
> on the receiver and the loop is self-sustaining from there — a sparse
> request/response application stays connected with no opt-in at all, and
> only the never-carried-anything case dies at install + 25 s. The
> consequence is stated where the ruling is: after that death only a side
> that can still **reach** the other can restart the connection, so a
> peer behind a NAT — its binding gone with the connection — must either
> be the dialler or hold the binding open with the persistent beacon.
> Making **all** keepalive opt-in, deleting the automatic dance, is
> **considered and declined**: it silently breaks sparse-traffic
> applications that do not opt in (§5.7, §7.4, §7.5, Appendix B).
> **Ruling 40** — the persistent-keepalive bound **is a ceiling, and this
> reverses part of ruling 38**. §5.7 and §7.5 justified the bound with a
> sentence describing a ceiling — *reject an interval so long that the
> beacon could not keep a connection alive on its own* — while the rule
> was written as a **floor**: reject intervals *below* `DEAD_TIMEOUT`.
> Ruling 38 diagnosed the sentence as the inverted half and rewrote it to
> match the floor; that diagnosis was itself wrong, and pass 3c corrects
> it — **the sentence was right and the rule was wrong**. A floor at
> `DEAD_TIMEOUT` is exactly what made the knob unreachable, since a
> beacon firing every 25 s cannot sustain a 25 s death timer. As ruled:
> `set_persistent_keepalive` now **rejects an interval at or above
> `DEAD_TIMEOUT`**, and the recommended default moves **25 s → 10 s**,
> matching `KEEPALIVE_TIMEOUT` for one-lost-beacon tolerance inside 25 s
> (2 × 10 + 5). The spec now also says why the beacon reaches where the
> passive rule cannot — it fires **unconditionally**, requiring no
> `R > S`, so it sustains a *mutually idle* link (the state ruling 39
> reaps) and holds a NAT binding open. The beacon **stays in the marking
> set**: short intervals need no special case, because arming *enables*
> death and never defers it — a beacon fired into a void still dies at
> `R + DEAD_TIMEOUT` — so the once-declined alternative of excluding
> beacons from the marking set is **unnecessary rather than declined**.
> Ruling 38's `I`/`S`/`R` derivation is **retained** and re-framed as the
> proof that the old bound was inverted; of its two declined
> alternatives, re-basing on `KEEPALIVE_TIMEOUT` is **superseded** by the
> ceiling and deleting the knob **still stands as declined** (§5.7, §7.5,
> §16.2, Appendix B, and the consolidated constants table).
> `DEAD_TIMEOUT` remains 25 s and `KEEPALIVE_TIMEOUT` remains 10 s.
>
> **Pass 4 — rulings 41–43, plus six directed fixes.** Two independent
> adversarial reviews of draft v6 found the same **blocker** in ruling
> 36's contested-connection probe; these rulings close it and clean up
> around it. Wire-free, and no error variant is added: the contested
> death keeps firing the existing `ConnectionLost::TimedOut`.
> **Ruling 41 — the probe matches a counter high-water mark, not a
> packet.** The old text required "an ACK covering the packet that
> carried that PING", which §8.7 makes unsatisfiable whenever that one
> packet is lost — PING is in the *never*-retransmit class, so the peer's
> replay window keeps a permanent gap there and every ACK it derives
> carries the gap. A fully live peer answering every PTO retry was still
> killed at the deadline. It is a **security** defect and not merely a
> liveness one, because §6.4's timestamp guard is vacuous for a peer we
> only ever dial and reverts on `AcceptError::Stale`, so a single
> captured msg1 is replayable forever and each replay was an independent
> trial at the forward loss rate — inverting the property §6.4 and §17.4
> claim for a `None` basis. As ruled: at mark time the connection records
> a **probe floor**, the counter the next seal will use
> (`DatagramSend::next_counter()`, Appendix A.2), and the mark clears on
> **any ACK covering any counter at or above that floor** — so PTO
> retries now *rescue* the connection. Concurrent marks **collapse**:
> one floor and one deadline per connection, a refusal arriving while
> contested creates no second mark and, explicitly, does **not** re-arm
> the deadline (re-arming would let an attacker's Intro supply postpone
> the verdict indefinitely). The security proof holds in its new form —
> every counter at or above the floor was sealed after the harvest, and a
> peer cannot ACK a packet we never sent. Two alternatives are recorded
> as declined: making PING retransmittable (it would reshape §8.7 for a
> frame §13.4's probe trains also use) and clearing on any post-mark ACK
> (an ACK already in flight at mark time proves nothing about the
> present) (§7.5, §6.3, §6.9, §13.5, §15.4, §16.5, §17.5, Appendix B).
> **Ruling 42 — the beacon gets a floor as well as a ceiling.** Ruling
> 40 replaced the floor with a ceiling and left no lower bound at all, so
> `set_persistent_keepalive(1 ms)` was conformant: 1000 packet/s on a
> beacon §14.5 exempts from congestion control, when §13.3 already
> condemns 20 packet/s, and with RTT sampling suppressed because
> keepalives never enter the sent map. The admissible range is now
> **[1 s, `DEAD_TIMEOUT`)** — reject below 1 s, reject at or above 25 s
> — with the recommended default unchanged at **10 s** (§5.7, §7.5,
> §16.2, Appendix B, and the consolidated constants table).
> **Ruling 43 — bound the probe honestly.** §7.5 claimed the probe was
> bounded by "nothing an attacker controls"; an attacker controls the
> supply of **Intros** that provoke refusals, so the claim was false and
> is replaced by the bound ruling 41's collapse actually delivers: **at
> most one probe per `KEEPALIVE_TIMEOUT` per connection**, however many
> Intros arrive. *(Superseded in round 30 — **ruling 175**: that
> replacement was false too. The collapse suppresses only refusals landing
> while a mark is **outstanding**, and a live peer clears the mark in
> ~1 RTT, so the true rate is `min(refusal rate, 1/RTT)`. The bound is
> restated honestly in §7.5, §6.9 and §6.3, and **no cooldown is added** —
> every re-mark records a fresh floor, so the security half is untouched.)*
> The probe stays exempt from §14.5's congestion
> *admission* gate — a cwnd-blocked probe would convert congestion into a
> liveness verdict — but is now explicitly **counted** in the sent map
> and `bytes_in_flight`, which keeps §17.5's cwnd bound true; §6.3's
> "established connections keep running regardless" is reconciled the
> same way (§6.3, §7.5, §14.5, §17.5).
> The directed fixes: §15.4 gains its own **contested** teardown row
> (correct 10 s deadline, correct trigger, correct asymmetry, same
> `TimedOut` variant); §16.5's closed timer table gains **`Contested`**,
> one deadline per connection, armed at the probe's transmission; §6.9
> and §17.5 account the probe's send and state cost; §7.3's exemption
> list, stale since ruling 36, names the contested probe; §7.4's and
> §6.9's unconditional spoofed-address and "ratio < 1" claims are
> qualified for a configured beacon (the 3× budget is the operative
> bound); ruling 39's reap is restated as a **receive**-within-25 s rule
> with the connect-ahead-of-use trap spelled out and an application
> obligation attached; the probe's mark, transmission and verdict join
> `slither::policy` (§18.2); and a probe that §7.3's budget defers leaves
> the mark **pending** rather than failed, with a mark on a
> closing/draining connection a no-op.
> With pass 4 the round closes.

slither is a Noise-over-UDP transport: mutually authenticated, encrypted
**streams, messages, and datagrams** between two peers, WireGuard-shaped
below (a cheap mac1 DoS gate, fresh-ephemeral handshake retransmission, an
anti-replay sliding window, endpoint roaming, and the keepalive/liveness
timers) and QUIC-shaped above (one unified frame layer inside every
sealed packet: STREAM fragments, ACK ranges, flow-control credit, an
unreliable DATAGRAM frame, and a CLOSE frame; RFC 9002 loss detection and
NewReno congestion control). All session cryptography flows through `hiss`
(Noise **IK** via the `noise!` macro and its datagram transport); the one
raw primitive is mac1's keyed BLAKE2b from `cryptoxide`.

## 1. Status, scope, and the first wire

### 1.1 The first wire

Wire version 1 is the **first released slither wire**. **[RATIFIED
2026/08/14]** No released software ever spoke the superseded pre-release
drafts (`SPEC.md`, `SPEC-v2.md`), so there is no on-wire migration and no
version cut — this wire simply claims the first version number. The
version byte is `VERSION = 0x01` in every packet header, and the Noise
prologue is `PROLOGUE = b"slither\x01"` (§5.1): an unknown version or
packet type is a silent drop (§3.1), and a version-confused peer that
somehow passed classification still fails cryptographically, because the
version is bound into the Noise transcript via the prologue. **No version
negotiation exists or is reserved for.** Version 1 means exactly this
specification; if there is ever a version 2, it is a deliberate break
with its own prologue. Negotiation is permanently out of scope for a
mutually-authenticated pair protocol — both ends are configured, not
discovered. (The alternative — a reserved negotiation surface — buys
nothing for configured peers and is a standing parsing liability;
declined.)

**Accepted note — old dev binaries.** The pre-release drafts happened to
use the same version byte and prologue, and — for the reference suite —
the same packet sizes: `IK_MSG1_LEN` / `IK_MSG2_LEN` /
`INIT_PACKET_LEN` / `RESP_PACKET_LEN` are 174 / 81 / 196 / 107 on both.
**Nothing structural separates the two wires**, so the separation is
cryptographic, and the reason is a **suite difference**, stated with its
scope:

- **P-256 (the suite the old wire had).** The pre-release wire was
  **P-256-only**, and it keyed mac1 over the 33-byte compressed SEC1
  static where this wire keys over the 65-byte canonical uncompressed
  form (§2.4, §4.4). mac1 therefore does not verify across the two: every
  cross-wire handshake packet is a silent drop at the mac1 gate, before
  any DH.
- **Any other suite.** The question does not arise — the old wire has no
  counterpart at all, so its packets differ in length from a new
  endpoint's and die at §3.1's classification gate instead.

Either way no session forms between the two wires; and were one somehow
to form, the old frame grammar inside the seal fails structurally and
dies as a signalled death under §8.2, applying nothing from the packet.
Accepted, because **nothing was ever released**: the exposure is confined
to unreleased development builds of the superseded drafts.

**One pre-release addition to this wire.** **[RATIFIED 2026/08/16 —
rulings 208, 210(d)]** Recorded here because §1.3's freeze is exactly what
makes it a ruling rather than drift. Ruling 208 adds two frame types —
`PATH_CHALLENGE` (`0x1a`) and `PATH_RESPONSE` (`0x1b`), §8.3 — replacing
the address-validation proof §7.3 carried under ruling 168. It is ratified
as a wire change because **nothing has shipped**: no third party has ever
seen a slither datagram, so the price is at its lifetime minimum today and
rises permanently at first publication. The freeze exists to stop casual
drift, not to make the protocol unfixable before it ships. **Its scope is
exact, and the exactness is why it is affordable: two new type codes, and
not one existing byte moved.** Every golden wire vector stays
byte-identical under it. A red wire vector while implementing this change
is therefore §1.3's stop signal in full force — it means the change went
further than it was ratified to go, not that an expectation needs
updating.

### 1.2 Scope

This specification covers the whole protocol: the crypto suite seam and
`channel!` (§2), the packet grammar (§3), mac1 (§4), the handshake (§5),
the staged accept and initiation routing (§6), the session layer —
counter, replay, roaming, liveness, the epoch ratchet (§7), the unified
frame layer (§8), streams (§9), flow control (§10), datagrams (§11), the ACK
frame and policy (§12), loss recovery (§13), congestion control (§14), CLOSE
and the connection lifecycle (§15), the object model and the sans-io cores
(§16), endpoint-global state (§17), errors and observability (§18), and the
deferral list (§19). Appendix A records the gating hiss dependencies;
Appendix B the test obligations; the final table consolidates every named
constant.

The old Leg 1/Leg 2 split is dead. There is one frame layer inside the seal:
every sealed packet's plaintext is a frame stream, DATAGRAM is just another
frame type, and one packet can coalesce an ACK, a STREAM fragment, a credit
grant, and an unreliable datagram. The old whole-message DATA frame, its
sequence space, its dedup floor, and the 1159-byte message cap are gone —
fragmentation and ordering are the STREAM frame's offset field (§9), and
"reliable message" is API sugar over short unidirectional streams (§9.8).

### 1.3 Ratification discipline and carried rulings

The code matches the spec, never the other way round. All golden wire
vectors freeze **for the first time** at this wire (the prologue, the
msg1 payload, and the canonical-encoding ruling of §4.4 fix the handshake
and mac1 bytes), and then hold under the standing test discipline: any
change that moves a wire byte turns a pinned test red, and such a red
means "this needs a ruling", not "update the expectation" (Appendix B).

The maintainer-flagged rulings carried from the superseded pre-release
drafts were **re-ratified individually** in the 2026/08/14 walkthrough —
one-connection-per-static (§16.1), the staged type names (§6.2), the
intro-queue DoS posture (§6.3), own-bytes-on-consume and the `Superseded`
removal (§6.3), the guard-eviction mitigations (§17.1), the
membership-oracle restatement (§6.5, in reduced form), the `accept()`
re-home model (§6.4), and the two-core sans-io shape (§16.4) — each
carrying its **[RATIFIED 2026/08/14]** marker at its home section. Two
did not: the instant swap-cut is **moot** (its home, §7.6, is deleted by
the ratchet-only ruling), and the no-fallback split-read gate
(Appendix A.1) was **pending** while it was worked jointly with hiss.
That gate is now **closed**: the staged read shipped in hiss 0.3.2, the
DH ladder it rests on is pinned by test, and Appendix A is reconciled
against the shipped API (**RECONCILED 2026/08/14**). **No maintainer
flag remained open in this document at ratification** — the marker token
appeared nowhere in it, and a zero count was the ratification check.

**[AMENDED 2026/08/16 — ruling 208, then CLOSED the same day by ruling
215.]** *The count went to one and back to zero, and saying so is the
point of keeping the check.* **The bracketed marker `FLAGGED FOR RULING`
now appears nowhere in this document, and 0 is again the expected answer**
— §7.3's entry is `[RATIFIED … ruling 215]`. The record of the call is
kept below rather than deleted, because a check that has never once fired
is indistinguishable from a check that does not work. The call was:
§7.3's
scarce-budget priority order and §7.5's pending contested probe were
written for the same state and do not compose — a probe that *returns*
from the send pass suppresses every rank below it, including the
`PATH_CHALLENGE` that would end the scarcity. **Ruling 215 resolved it in
favour of the third available resolution: the ranks were right and the
send pump's early return was the defect** — it emits the probe and
continues building on the same pass — and, since ruling 250, prefers one
packet: the probe coalesces the owed path frames when pump-time room
admits the coalesced size, the probe-plus-challenge arithmetic re-checked
against the **remaining** room rather than the arming-instant 90 B
floor. An earlier ruling (212(c)) lifted the challenge
above the probe instead and was reversed, because §7.5 proves a delayed
probe *"would silently convert congestion into a liveness verdict"* while
a delayed challenge only prolongs a cap.

**Zero** is the expected count again. A bracketed marker appearing without
a ruling is the same signal a red wire vector is.

## 2. Crypto suites and `channel!`

### 2.1 The hiss contract

slither rides hiss 0.3.x: the IK handshake is declared through the `noise!`
macro, and the data path rides the datagram transport
(`DatagramSend`/`DatagramRecv`). slither never touches curve or AEAD
primitives itself (the one exception is §4's mac1). The following hiss facts
are load-bearing for the wire; each row states what hiss fixes and how the
wire respects it.

| hiss fixes | the wire respects it by |
|---|---|
| The send counter is hiss-owned, strictly monotonic from 0, and never caller-chosen; it is returned by each seal | the counter is **transmitted verbatim** in the Data header (§3.4); slither never mints a second packet number (§7.1) |
| The counter is the AEAD nonce; the receiver must know it before it can decrypt | the counter rides in **cleartext** in the header — non-negotiable (§3.4) |
| The receive half is stateless with respect to ordering and replay: it opens any counter, any number of times | slither owns 100 % of replay protection — the RFC 6479 window, consulted strictly **after** the AEAD authenticates (§7.2) |
| The epoch ratchet: a message at `counter` belongs to epoch `counter / epoch_size`; both ends must pass the identical epoch size | `REKEY_EPOCH_MSGS = 65 536` is a protocol constant, not a knob (§7.7) |
| `MAX_EPOCH_JUMP = 2` (hiss-fixed): a counter more than two epochs ahead is refused without deriving any key; committed keys advance only after the AEAD tag verifies (commit-and-cap) | forged far-future counters are bounded upstream of the replay window and can never desynchronise the receiver; epoch death is subsumed by liveness (§7.7) |
| Straggler tolerance is exactly one epoch back | the reordering budget (replay window, 2048) sits far inside one epoch (65 536) (§7.2) |
| `MAX_MESSAGE_LEN = 65 535` bounds any one sealed message (ciphertext incl. tag) | every slither seal is ≤ `MAX_PLAINTEXT` + 16 = 1186 B, far under the cap (§8.6) |
| The counter value `2⁶⁴ − 1` is reserved for the `Rekey()` transform; sealing at it is refused | the usable counter space is `0 ..= 2⁶⁴ − 2`; exhaustion is the terminal `ConnectionLost::NonceExhausted` (§7.9) |
| The two directions have independent counters; a fresh handshake builds a fresh transport counting from 0 | packet-number spaces are **per direction**, and a connection has exactly one session for its life (§7.1, §7.8) |

### 2.2 `channel!` — what varies, what the wire sees

`channel!` (a `macro_rules` macro — no proc-macro) stamps the `hiss::noise!`
IK invocation with a caller-chosen `<Curve, Cipher, Hash>` triple plus the
`Channel`/`Protocol` implementation. The IK token block and the handshake
payload declaration (`[12]` on msg1; msg2 declares no payload — §5.2) are
hardcoded in the macro. The reference suite is
**`P256 / ChaChaPoly / Blake2b`**, and its Noise protocol name —
`Noise_IK_P256_ChaChaPoly_BLAKE2b` — is pinned by test.

**There are exactly two patterns: `IK` and `IKpsk1`.** `channel_psk!` is the
second stamp — the same triple, the same hardcoded `[12]` payload, and a
token block differing only by a **trailing `psk`** on msg1
(`-> e, es, s, ss, psk [12]`). It exists for one requirement the `known` set
cannot serve: admitting a **stranger** under a secret carried out of band, an
in-person pairing ceremony being the motivating case. The pre-shared key is
supplied at the two staged points — the dial and §6.1's stage 2 — never held
by the endpoint, so a responder selects it **using the claimed static it has
just paid one `es` for**. That ordering is the whole reason the pattern is
`IKpsk1` and not a psk0 shape: the identity is revealed *before* the `psk`
token, so an unenrolled stranger is rejected at **1 DH**, with the peer named,
rather than the 2 DH a lookup at the `psk` token would cost.

Three things about the second pattern are consequences rather than choices.
Its `psk` token **puts no bytes on the wire**, so §2.3's derivation holds for
it exactly as written and all four sizes equal the reference suite's. Its
protocol name is `Noise_IKpsk1_<curve>_<cipher>_<hash>` — a different name
seeding a different initial handshake hash, which is the only separation the
two patterns get and the only one they need. And **endpoints remain
monomorphic per suite**: a psk channel is a *separate endpoint*, on its own
port, never a second pattern multiplexed onto one socket — which is also what
makes "stop presenting the pairing window" a lifetime rather than a flag.
**[AMENDED 2026/08/23 — ruling 280: `IK is the only pattern` is lifted; hiss
0.4.1's trailing-`psk` staged read is the capability the Deferred row was
waiting on.]**

Wire-visible consequences of the suite are the handshake message sizes
(point encodings) and, in principle, the AEAD tag size; everything above
the seal — the data-packet header, the frame layer, and every constant in
§§8–15 — is suite-independent. **There is no suite identifier on the
wire.** Endpoints are monomorphic per suite: the shell type is
`Endpoint<C: Channel>`, and §16.2's `Endpoint` and §16.4's
`core::Endpoint<I: Identity>` are the same parameterisation viewed from the
shell and the core (`I`'s provider is the suite's DH provider; those
sections elide the parameters). A
mismatched-suite packet dies silently: at the length gate or at mac1 when
the suites differ in curve — the same fate as garbage — and, for a
same-curve sibling suite (`P256 / AesGcm / Blake2b` against the reference
suite, the first such pair), at msg1's first AEAD open on the staged
ladder: the lengths coincide (§2.3 — `PK` is the only per-suite quantity)
and §4.1's mac1 key carries no suite, so both gates pass and §6.9's
mac1-valid rows price the spend. Either way nothing installs. The version
byte does not encode the suite. **[AMENDED 2026/08/20 — ruling 279: the
pre-DH death was stated for every suite pair and is true only when the
curves differ.]**

### 2.3 Per-suite derived sizes

**[RATIFIED 2026/08/14 — ruling 68]** With `PK` = `Curve::PUBLIC_KEY_SIZE`
and `TAG` = `AEAD_TAG_LEN` = **16**, *fixed for every suite* — the AEAD
tag length does **not** follow the suite, exactly as §4.4 fixes mac1 at
keyed-BLAKE2b rather than following the suite's Hash. **`PK` is the only
per-suite quantity in the formulas below.** This is what keeps §3.5's
`MAX_PLAINTEXT` a flat 1 200 − 14 − 16 and the data-path overhead a flat
30 B; were `TAG` per-suite, both would be per-suite too, which §3.5
denies. Nothing real is foreclosed: every AEAD Noise defines —
ChaCha20-Poly1305 and AES-GCM alike — has a 16-byte tag.

```
MSG1_LEN        = PK + (PK + TAG) + (MSG1_PAYLOAD_LEN + TAG)   (e ‖ enc_s ‖ enc_payload)
MSG2_LEN        = PK + TAG                                     (e ‖ the empty payload's tag)
INIT_PACKET_LEN = 6  + MSG1_LEN + 16                           (InitHeader ‖ msg1 ‖ mac1)
RESP_PACKET_LEN = 10 + MSG2_LEN + 16                           (RespHeader ‖ msg2 ‖ mac1)
```

Reference-suite values (P-256: `PK` = 65; `MSG1_PAYLOAD_LEN` = 12 — §5.2,
test-pinned). `AEAD_TAG_LEN` appears in the table below for reference and
is **not** a reference-suite value: per ruling 68 it is 16 on every suite:

| Constant | Value |
|---|---|
| `IK_MSG1_LEN` | **174** (= 65 + 81 + 28) |
| `IK_MSG2_LEN` | **81** (= 65 + 16) |
| `INIT_PACKET_LEN` | **196** |
| `RESP_PACKET_LEN` | **107** |
| `AEAD_TAG_LEN` | 16 |

### 2.4 The canonical static encoding

The **canonical encoding of a static public key is the `AsRef<[u8]>` octets
of `Curve::PublicKey`** — for P-256, the 65-byte uncompressed SEC1 storage
form (`0x04 ‖ X ‖ Y`, normalisation enforced by hiss regardless of the
encoding a key was parsed from); for X25519, the raw 32 bytes. One encoding,
three uses: mac1 is keyed over it (§4), the simultaneous-open tie-break
compares it (§6.7), and identity maps and accept policies key on it. Within
a suite all canonical encodings are equal-length, so the tie-break compares
the `as_ref()` octets directly as unsigned octet strings — the only bound
required is `Curve::PublicKey: AsRef<[u8]>`, expressed as a slither-side
`where` clause; `Ord` is **not** required (Appendix A.3). The stated
consequence: P-256 mac1 keying moves from the pre-release drafts' 33-byte
compressed form to the 65-byte uncompressed form — fixed at the first
golden freeze (§4.4), with no shipped bytes to move.

## 3. Packet grammar

### 3.1 Packet types and version

| Constant | Value | Meaning |
|---|---|---|
| `VERSION` | `0x01` | protocol version byte; unknown ⇒ silent drop |
| `PKT_HANDSHAKE_INIT` | `0x01` | initiator's IK msg1 |
| `PKT_HANDSHAKE_RESP` | `0x02` | responder's IK msg2 |
| `PKT_DATA` | `0x03` | sealed transport datagram |
| `0x04` | reserved | **unused** — the cleartext close packet concept is dead (CLOSE is a frame, §15); never emitted, silently dropped |
| `0x05` | reserved | cookie reply / mac2 (§19) — never emitted, silently dropped |
| `0x06..` | reserved | future — never emitted, silently dropped |

Every packet opens with `type: u8, version: u8`. **[RATIFIED 2026/08/14 —
ruling 64]** **All multi-byte header fields are little-endian.** That is
exactly three fields across the whole grammar — `sender_index`,
`receiver_index` and `counter` — because everything else in every header
is a single byte or an opaque octet string, and octet strings have no
byte order. Little-endian for two reasons: it matches **WireGuard**, whose
header integers are little-endian and whose posture §3.4 already adopts
for the clear counter; and — **for the reference suite** — it makes the
`counter` on the wire byte-identical to the ChaCha20-Poly1305 nonce Noise
derives from it, which big-endian would leave as its byte-reverse (§3.4).
That second reason is suite-specific and the rule is not: Noise encodes
the ChaChaPoly nonce little-endian but the AES-GCM nonce big-endian, so a
future AEAD inverts the coincidence without disturbing the rule, which
rests on WireGuard's shape.

**Three** things this rule does **not** reach, all of which stay as they
are.

1. §8.1's varints are byte-identical to RFC 9000 §16 and therefore
   big-endian. The mixed reading is not observable: the frame layer rides
   **inside** the AEAD and never appears in the same cleartext as a header.
2. §5.2's msg1 payload timestamp is `ts_secs(8, BE) ‖ ts_nanos(4, BE)` —
   big-endian **deliberately**, because §5.3's strictly-greater test is an
   ordering, and a big-endian `ts_secs` orders correctly compared as an
   octet string. Unlike the varints this one *is* observable beside the
   header, since the responder decrypts msg1 while still holding the
   header bytes: it is a considered exception, not an oversight.
3. §2.4's canonical static comparison (§6.7's tie-break) is a lexicographic
   comparison of equal-length **octet strings**, not an integer encoding,
   so no byte order applies to it at all.

**[RATIFIED 2026/08/14 — ruling 65]** The pre-AEAD length gate is
**exact** for the two fixed-size handshake packets and a **range** for
Data:

| Type | Accepted length |
|---|---|
| `PKT_HANDSHAKE_INIT` | exactly `INIT_PACKET_LEN` (196) |
| `PKT_HANDSHAKE_RESP` | exactly `RESP_PACKET_LEN` (107) |
| `PKT_DATA` | `DATA_HEADER_LEN + AEAD_TAG_LEN` (30) ≤ len ≤ `MAX_DATAGRAM` (1200) |

Exactness on the handshake types is load-bearing, not tidiness. §4.1
defines mac1's preimage as "all packet bytes preceding the tag", so a
variable length would move the tag and leave the preimage extent
undefined; and because mac1's key is derived from **public** data (§4.3),
any third party could pad an initiation and recompute a valid mac1,
appending trailing bytes that neither Noise's AEAD nor any secret covers.
Fixing the length forecloses that malleability and resolves the preimage
to a constant per type (§4.1).

A datagram outside its type's accepted length,
longer than `MAX_DATAGRAM`, or bearing an unknown type or version is
silently dropped before any further work. This pre-AEAD gate is the
**only** silent-drop tier for malformed traffic: a packet that fails here
may genuinely be corruption, so nothing is signalled; a packet that passes
the AEAD and then fails structurally is a peer bug or an attack, and gets a
signalled death (§8.2).

### 3.2 HandshakeInit (`0x01`) — 196 bytes (reference suite)

```
type(1) ‖ version(1) ‖ sender_index(4)                       ← InitHeader, 6 B
        ‖ hiss IK msg1(174) ‖ mac1(16)
```

- `sender_index` — the initiator's random nonzero `u32` index (§17.3).
- msg1's encrypted tail carries the 12-byte payload `timestamp(12)`
  (§5.2); nothing of it appears in the header.
- `mac1` — keyed on the **responder's** static (the recipient, §4); its
  preimage is all packet bytes preceding the tag.

### 3.3 HandshakeResp (`0x02`) — 107 bytes (reference suite)

```
type(1) ‖ version(1) ‖ sender_index(4) ‖ receiver_index(4)   ← RespHeader, 10 B
        ‖ hiss IK msg2(81) ‖ mac1(16)
```

- `sender_index` — the responder's random nonzero `u32` index.
- `receiver_index` — the initiator's index this response answers.
- msg2 carries no payload (§5.2): its encrypted tail is the empty
  payload's AEAD tag alone.
- `mac1` — keyed on the **initiator's** static (the recipient, §4).

### 3.4 Data (`0x03`) — 14-byte header ‖ ciphertext

```
type(1) ‖ version(1) ‖ receiver_index(4) ‖ counter(8)        ← DataHeader, 14 B
        ‖ ciphertext(plaintext + 16)
```

- **The 14 header bytes are the AEAD associated data, verbatim.** The
  counter in the AD is redundant (it is already the nonce), but harmless,
  and "AD = the header, verbatim" is the simplest possible rule.
  Authenticating `receiver_index`, `type`, and `version` is stronger than
  WireGuard (whose data AEAD uses empty AAD) at zero cost: header tampering
  is tag-detectable.
- `receiver_index` — the recipient's session index; routes the packet to a
  session (and thus a key) before decryption (§17.3).
- `counter` — exactly the value the seal returned: the hiss-owned monotonic
  send counter, which is simultaneously the AEAD nonce, the packet number
  (§7.1), and the epoch selector (§7.7). Little-endian per §3.1, so under
  the reference suite these eight bytes **are** the nonce's trailing eight
  bytes: Noise builds the ChaChaPoly nonce as `32 zero bits ‖ LE64(counter)`,
  so the wire bytes and nonce bytes `[4, 12)` coincide exactly rather than
  by reversal. (Noise's AES-GCM nonce rule is big-endian, so the
  coincidence is a reference-suite property; the `u64`-LE rule is not.)
  Full 8 bytes, in clear, no
  truncation and no header protection in this version — the WireGuard
  posture; truncated packet numbers and header protection are deferred
  metadata levers (§19). Appendix A.2's `next_counter()` accessor has
  **shipped**, and naming the counter before the seal is the operative
  mechanism — the header is the AEAD associated data, so it must be built
  *before* sealing. The mirror-and-assert interim (mirror the expected next
  counter, `debug_assert_eq!` it against each seal's returned value) is
  retained here only as the **fallback shape**, not as the mechanism to
  implement; were it used, the mirrored value would feed **only** the AD
  construction and never be fed back to hiss.
- **There is no cleartext length field**: the AEAD gives the exact
  plaintext length, and the frame parser runs to the end of it (§8.2).
- An **empty plaintext** (16-byte tag-only ciphertext; a 30-byte datagram)
  is the **keepalive** — it bypasses the frame layer entirely and is the
  only non-frame plaintext (§7.5).

### 3.5 Sizes and caps

| Constant | Value |
|---|---|
| `INIT_HEADER_LEN` | 6 |
| `RESP_HEADER_LEN` | 10 |
| `DATA_HEADER_LEN` | 14 |
| `MAX_DATAGRAM` | 1200 |
| `MAX_PLAINTEXT` | 1170 (= `MAX_DATAGRAM` − 14 − 16) |
| session index | random nonzero `u32`, re-drawn per §17.3 |

Per-packet overhead on the data path is 14 + 16 = 30 B. Oversize receive
(> `MAX_DATAGRAM`) is a silent drop. Oversize *send* has no single rule:
stream data fragments across packets by construction (§9.5), and each
sending surface enforces its own cap at the handle — `DatagramError::TooLarge`
above `MAX_DATAGRAM_PAYLOAD` (§11.4), `MessageError::TooLarge` above
`MESSAGE_RECV_MAX` (§9.8).

## 4. mac1 — the DoS gate

### 4.1 Construction

```
key  = BLAKE2b-256(MAC1_LABEL ‖ recipient_static_canonical)
mac1 = keyed-BLAKE2b-128(key, all packet bytes preceding the tag)
```

| Constant | Value |
|---|---|
| `MAC1_LABEL` | `b"slither mac1"` |
| `MAC1_LEN` | 16 |

**[RATIFIED 2026/08/14 — ruling 66]** Both BLAKE2b invocations are
**plain**: no salt and no personalisation (absent / all-zero), matching
WireGuard's plain keyed BLAKE2s. Domain separation is by **concatenation**
— `MAC1_LABEL` is a prefix on the key preimage — and never by the
primitive's personalisation parameter. The key preimage is therefore
`MAC1_LABEL.len() + STATIC_PUBLIC_LEN` = 12 + 65 = **77 bytes** for the
reference suite. This is stated because a personalised BLAKE2b changes
every output byte, and the golden freeze makes that permanent.

Because §3.1 fixes the handshake packets at exact lengths (ruling 65),
"all packet bytes preceding the tag" resolves to a constant extent:

| Packet | mac1 preimage | mac1 occupies |
|---|---|---|
| HandshakeInit | `[0, 180)` = `INIT_PACKET_LEN − MAC1_LEN` | `[180, 196)` |
| HandshakeResp | `[0, 91)` = `RESP_PACKET_LEN − MAC1_LEN` | `[91, 107)` |
| Data | — no mac1 | — |

`recipient_static_canonical` is the recipient's static public key in the
canonical encoding of §2.4 — for the reference suite, the 65-byte
uncompressed SEC1 form. On a HandshakeInit the recipient is the responder;
on a HandshakeResp the recipient is the initiator.

### 4.2 Verification order

mac1 is verified **before any curve or DH work**. A garbage flood, a
wrong-key packet, or a wrong-curve-suite packet dies at one keyed hash and
never reaches the DH provider; a same-curve sibling suite's packet is
mac1-valid — §4.1's key preimage carries no suite — and costs what §6.9
prices for any mac1-valid initiation from a knower of the static (§2.2).
This is the floor of the staged-accept cost
ladder (§6.2). **[AMENDED 2026/08/20 — ruling 279]**

### 4.3 What mac1 is not

mac1 is **not** a secret authenticator: its key is derived from public
data, so anyone who knows the recipient's static can mint mac1-valid
packets. It is an anti-amplification and cheap-reject gate; the real
authentication is the Noise handshake underneath. The cookie/mac2 second
tier (packet type `0x05`) against floods of *valid-looking* packets remains
reserved (§19).

### 4.4 Suite genericity

**[RATIFIED 2026/08/14]** mac1 is **fixed keyed-BLAKE2b for every suite** —
it does not follow the suite's Hash. mac1 is a keyed hash over public data,
not session cryptography, so it falls under the raw-primitive rule (taken
from `cryptoxide` directly), and WireGuard's fixed-BLAKE2s sets the
precedent; making it follow the suite Hash would demand a keyed-hash mode
from every hiss Hash and buy nothing. Together with the no-suite-byte rule
(§2.2) this forecloses any future multi-suite endpoint on one socket —
mismatched suites die silently (pre-DH when the curves differ; at the
first AEAD open for a same-curve sibling, §2.2 — **[AMENDED 2026/08/20 —
ruling 279]**), which is acceptable for
mutually-configured peers. The keying encoding is the canonical encoding
(§2.4): a generic formulation inheriting `AsRef` silently would have been a
trap under a frozen wire; here it is a deliberate ruling, and the resulting
P-256 change (33-byte compressed → 65-byte uncompressed keying, relative to
the pre-release drafts) is costless — no shipped bytes existed, and the
first golden freeze pins the canonical form (§1.1, §1.3). The
alternative — pinning the compressed form per curve — would preserve the
pre-release drafts' mac1 bytes at the price of a second, per-curve encoding
rule alongside the canonical one.

## 5. Handshake

### 5.1 Prologue

```
PROLOGUE = b"slither\x01"        (fixed; identical for every handshake)
```

The prologue carries the wire version, binding it into the Noise
transcript: a peer that mis-classifies the version does not merely parse
garbage, it fails the handshake cryptographically (§1.1).

### 5.2 Handshake payloads

```
msg1 payload (12 B, encrypted in msg1's tail):
    ts_secs(8, BE) ‖ ts_nanos(4, BE)
msg2: no payload (msg2's encrypted tail is the empty payload's tag alone)
```

| Constant | Value |
|---|---|
| `TIMESTAMP_LEN` | 12 |
| `MSG1_PAYLOAD_LEN` | 12 |

The payload sits inside msg1's encrypted tail, after `e, es, s, ss`
(authenticated against the initiator's proven static): encrypted,
authenticated, and forgeable only by key-holders — who can only hurt
themselves (§5.4, §17.1). msg2 declares no payload; its tail's tag still
authenticates the full transcript (the cipher is fully keyed after
`e, ee, se`), so a tampered msg2 fails completion exactly as before.

### 5.3 The initiation timestamp

The timestamp is the wall clock (`secs_since_unix_epoch ‖ nanos`), the one
wall-clock read in the protocol (§16.5), forced **strictly greater** than
the previous timestamp this endpoint emitted — endpoint-global, across all
connections and connection generations (§17.2) — so a retransmit is always
admissible even when the coarse clock has not advanced, and a close-and-
reconnect still emits strictly greater.

**The exact confidentiality guarantee (carried).** The msg1 payload has
Noise confidentiality level 2: it is encrypted to the responder's static
key, so it is opaque to any passive observer — no clock-skew
fingerprinting — and authenticated (a tampered ciphertext fails the tail's
AEAD tag, failing the handshake). It is **not** forward-secret against a
later compromise of the responder's static key; acceptable, because the
plaintext is a wall-clock reading, not a secret.

### 5.4 One session per connection — restart is structural

**[RATIFIED 2026/08/14 — the ratchet-only ruling; amended
2026/08/14.]** There is no periodic
DH re-handshake in this protocol (§7.6 is deleted); the §7.7 epoch
ratchet is the **only** rekey. Every completed handshake therefore
establishes a **fresh connection with fresh transport state on both
sides** — sessions and connections are 1:1, and no stream, flow-control,
recovery, or congestion state ever crosses a handshake (§7.8). The old
wire's restart bug — rekey and restart cryptographically
indistinguishable at msg1, producing silent, *confirmed*, undetectable
data loss when a restarted peer's from-zero identifiers were swallowed as
duplicates and acknowledged — is dead **by construction**, not by flag:
there is no state-carrying handshake for a restart to masquerade as, so
no flag bits, no CONTINUATION/CONTINUED machinery, and no
restart-detection surface exist. The handshake payload is the timestamp
alone (§5.2).

**Responder rule.** The local state for the proven static — always
consulted **post-`ss`**, at the point each path proves the static: the
tie-break's routing (§6.6) or the staged chain's admission (§6.4) — is
exactly one of **three** values:
**LIVE** — an established connection exists; **PENDING** — **a pending
exists for that static** and no established connection (the
simultaneous-open state; §16.1 forbids both at once); or **NONE** —
neither.
**[AMENDED 2026/08/16 — ruling 178]** *PENDING means a pending exists,
not that a datagram is in flight.* This value read "an in-flight
outbound initiation exists", which ruling 90's
`mint_pending`/`start_attempt` split left ambiguous — a **minted**
pending has nothing in flight yet — and the predicate is now stated
positively: **membership in the pending tables** (§17.3, §17.4), from
the instant `connect()` mints the pending until it completes, gives up
or is cancelled (§16.3, ruling 50). Everywhere this document says an
"in-flight outbound pending" it means an entry in those tables; there is
no separate per-static flag, and all three readers of "is this static
PENDING?" — §6.4's branch, §6.5's hint set, and this rule — consult the
same tables. The alternative reading routes an initiation arriving in
the mint-to-send window down the **NONE** row, which installs a second
session and leaves both sides mutually dark until their effective dead
timeouts — precisely the divergence ruling 35 exists to prevent.

- **LIVE** — the initiation is a **candidate replacement**: it parks as
  an ordinary
  `Intro`, frozen with its paid mid-state where one was paid (§6.3), and
  surfaces through the staged accept. The live connection keeps running
  untouched; `accept()` of that `Intro` performs the
  `ConnectionLost::Replaced` teardown as the act that installs the
  replacement (§6.4). A withheld or replayed initiation left unaccepted
  costs one parked `Intro` and nothing else. Two tests gate the
  admission: the per-static greatest-timestamp guard (§17.1) rejects
  anything older than or equal to what we have already admitted for that
  static, and the connection's replacement basis (§17.4) rejects anything
  not strictly newer than the initiation that established it — and
  rejects everything where we dialled, because then we hold no
  initiation of that peer's to measure against. Neither test can tell a
  withheld genuine retransmit from a fresh one (§6.4).
- **PENDING** — the simultaneous open: the deterministic tie-break
  decides (§6.6–§6.7); no teardown either way. A static that is PENDING
  only because a `connect()` raced a chain already staged against it
  reaches the **same comparison** by a different route — §6.4's PENDING
  branch applies §6.7's ordering itself: the tie-break loser cancels its
  pending and installs as responder, the tie-break winner returns
  `AcceptError::Stale`, keeps its pending, and records the candidate's
  timestamp. The comparison is two-sided, so no admission path opts out
  of it (§6.4).
- **NONE** — the ordinary fresh accept via the staged path (§6.1).

A restarted peer needs no protocol help: its fresh initiation is simply a
replacement (LIVE row) or a fresh accept (NONE row), and its zombie
counterpart-connection dies at the replacing `accept()` or at liveness
(§7.5). **No routing outcome can merge two transport-state generations,
because no handshake carries state across.**

### 5.5 Initiator behaviour

1. Draw a random nonzero `sender_index` (re-draw rule §17.3) and a fresh
   strictly-greater timestamp; build msg1 over a fresh ephemeral with the
   12-byte payload (§5.2); append mac1; send to the dialled address.
2. Arm a retransmit at `RETRANSMIT_BASE` + uniform jitter ≤
   `RETRANSMIT_JITTER_MAX` (5 s + U[0, 333 ms]). **Every retransmit is a
   completely fresh initiation** — new ephemeral, new random index, new
   strictly-greater timestamp. The interval is fixed, not exponential —
   WireGuard's shipped shape, kept for simplicity.
3. **One completion attempt per retransmit interval.** The pending's
   attempt is *taken* on the first **length-correct, index-matching,
   mac1-valid** msg2; a second msg2 in the same interval is dropped. A
   failed completion (bad crypto)
   spends the attempt — the next scheduled retransmit refreshes it — so a
   guessed-index or mac1-invalid msg2 can never spend anything, and no
   forged-msg2 volume can induce initiations faster than the retransmit
   schedule. (The residual exposure — an on-path, index-observing forger
   beating the genuine msg2 each interval — is documented in §6.9.)
4. **The msg2 source address is deliberately ignored.** Completion
   requires an index match, not an address match; the initiator anchors
   the session at the dialled address, and the peer roams in on its first
   authenticated data packet (§7.3). (The responder has no dialled
   address: it anchors at the msg1 source — a peer-supplied address,
   send-gated by §7.3's anti-amplification budget; §5.6.)
5. On completion: session live — our receiver index = our
   `sender_index`, the peer's = the response's `sender_index`.
6. Give up at `HANDSHAKE_GIVEUP` (90 s): `Connecting` resolves
   `Err(ConnectError::TimedOut)` — the only handshake failure the
   application ever sees. (Every initiation is a `connect()`; there are
   no internal rekey initiations — §5.4.) The train also ends **early**
   if the application drops the `Connecting`, which cancels the attempt
   outright (§16.3, ruling 50).

### 5.6 Responder shape

The responder's processing is the staged DH-cost ladder — mac1 (0 DH) →
`es` (1 DH, recovers the *claimed* static) → `ss` (1 DH, proves possession
and decrypts the timestamp) → `ee`, `se` (+2 DH, msg2) — driven by the
application through the staged accept, with one internal exception: the
simultaneous-open tie-break (§6.6–§6.7). §6 is its normative home. The
per-static greatest-timestamp guard (strictly greater, else drop — §17.1)
admits at `authenticate()` on the staged path and at the tie-break's
admit step, in both cases post-`ss`, so only key-holders can write guard
entries.

The responder anchors an accepted session at the initiation's msg1 source
address — the one place a peer-supplied address becomes a send target
before any authenticated data has arrived from it. That anchor arms
§7.3's anti-amplification budget: until the address is **validated**,
output to it is capped at `AMPLIFICATION_FACTOR` × the authenticated,
window-fresh bytes received from it (§7.3). **[AMENDED 2026/08/16 —
rulings 168, 169; the predicate superseded 2026/08/16 — ruling 208]**
Validation is a predicate, not a mood: the anchor draws an 8-byte
challenge and owes a `PATH_CHALLENGE` (`0x1a`) to that address, and the
address validates — and the budget disarms — at the first authenticated,
window-fresh packet from it carrying the matching `PATH_RESPONSE`
(`0x1b`) (§7.3, §8.3). The earlier predicate — an ACK covering a
`validation_floor` counter — is superseded: an ACK's fields are
assertions by whoever holds the key, and at a peer-supplied address the
key holder is the party whose claim is in question (§7.3).

### 5.7 Timer values and their derivations

| Timer | Value | Derivation |
|---|---|---|
| `RETRANSMIT_BASE` + jitter | 5 s + U[0, 333 ms] | WireGuard Rekey-Timeout + jitter, cross-checked in the paper, the kernel, and wireguard-go; fixed-interval is WireGuard's shipped shape |
| `HANDSHAKE_GIVEUP` | 90 s | WireGuard Rekey-Attempt-Time |
| `KEEPALIVE_TIMEOUT` | 10 s | WireGuard Keepalive-Timeout; the v1/default profile's passive cadence and contested verdict (§7.5) |
| `DEAD_TIMEOUT` | 25 s | 2 × `KEEPALIVE_TIMEOUT` + 5 s grace — the v1/default profile's receive-anchored death timeout and one-lost-keepalive tolerance, *in one direction* (below); the only idle killer (§7.5) |
| `PERSISTENT_KEEPALIVE` | 10 s default; admissible range **[1 s, effective dead timeout)** | `KEEPALIVE_TIMEOUT`, so the v1/default profile tolerates one lost beacon inside `DEAD_TIMEOUT` (2 × 10 + 5); a recommended default, per-connection `Option<Duration>`, off by default; the handle rejects an interval **below 1 s** — the floor, ruling 42 — and **at or above the connection's effective dead timeout** — the ceiling, rulings 40 and 282 (§7.5) |

**Effective established-session liveness (rulings 282 and 283).** `Config` may
carry one endpoint-wide, immutable, validated `TimingProfile`. Let
`K_eff` be its passive-keepalive interval and `D_eff` its
receive-anchored dead-peer timeout. The profile is stamped onto every
connection born from that endpoint, through both outbound `connect()`
and inbound `accept()`; it cannot vary by peer or be changed on a live
connection. `Config::default()` carries the exact v1 profile:
`K_eff = KEEPALIVE_TIMEOUT` (10 s) and `D_eff = DEAD_TIMEOUT` (25 s).
The public constants remain unchanged and continue to pin those defaults.

A profile is constructible only when `K_eff >= 1 s` and, with checked
`Duration` arithmetic,

```text
D_eff > 2 × K_eff + K_INITIAL_RTT + 2 × SHELL_LATENESS_BOUND.
```

Overflow rejects the profile. Construction also rejects a dead timeout
that cannot be added to the constructor's current monotonic `Instant`.
That is early validation, not a lifetime proof about every later anchor:
all deadline derivations also follow §16.5's checked, unreachable-deadline
rule. The strict relation admits one lost keepalive under the initial-RTT
assumption and leaves enough room for both successive keepalive firings
to consume the shell's permitted lateness without making an arrival
exactly at the verdict instant win by event-loop ordering (ruling 283).
Each firing re-anchors the next deadline from its actual send instant, so
both lateness allowances are necessary. The two values move only the
coupled established-session rules: passive keepalive and the contested
verdict both use `K_eff`; receive-anchored death and every liveness
backstop both use `D_eff`; and the persistent-beacon ceiling is `D_eff`.
Where §§7.4–7.5 and their cross-references say *effective keepalive* or
*effective dead timeout*, these are the values meant.

Nothing is negotiated or carried on the wire. Peers with different
profiles remain wire-compatible but can reach different liveness
verdicts; a deployment that depends on common loss or failover behaviour
MUST configure the common profile out of band. Handshake retransmission
and give-up, introduction and timestamp-guard retention, ACK/PTO
recovery, close linger, and shell lateness remain the fixed v1 constants.
In particular, `HANDSHAKE_GIVEUP` is not application dial patience:
dropping `Connecting` remains the early-cancel mechanism (§16.3).

`DEAD_TIMEOUT` is deliberately **two** keepalive periods plus grace, not
one. At a single keepalive period plus grace, one lost keepalive killed a
healthy idle connection: the 10 s to the keepalive plus 5 s of grace
leaves no room for even one retransmit of it. At
2 × `KEEPALIVE_TIMEOUT` + 5 s the tolerance is explicit — one lost
keepalive is survivable. Its shape is **unidirectional**, and the text
says so rather than implying more: the survivable case is one keepalive
lost *in one direction*, where the surviving direction's keepalive keeps
the loser's clock fed. A *simultaneous bidirectional* loss — one loss
event, both keepalives, the same interval — is not tolerated at all and
still ends an idle connection at 25 s, because the passive rule is
one-shot per receive and keepalives are never retransmitted (§7.5, §8.7).

**What the dance covers, and what it does not (ruling 39).** The passive
keepalive dance is **automatic for any connection that has ever carried
traffic**. One exchange in either direction leaves the receiving side
with `last_authenticated_recv > last_send`, which is exactly the dance's
entry condition, and each keepalive it sends puts the *other* side in
that state in turn — so the loop is self-sustaining once entered, and a
sparse request/response application (a request every 60 s, say) stays
connected with no opt-in whatsoever. The single case the dance does not
cover is a connection that has had **no authenticated receive at all**
since install:
with `last_send` pinned equal to `last_authenticated_recv` at the install
instant (§7.4) the entry condition is false from the start, the dance
never begins, and such a connection is reaped at install + `D_eff`
(25 s under the v1/default profile). The rule is stated in terms of
*receiving* deliberately,
because that is what it is: the death deadline is armed at install and
is reset only by an authenticated receive, so sending is useful here
only instrumentally, by provoking the peer's passive keepalive. That
reaping is intended, not a gap — §7.5 states the
consequence, the trap it sets for connect-ahead-of-use applications, and
the alternative that was declined.

One coincidence used to follow here, and it is now gone. At the old 25 s
default, `PERSISTENT_KEEPALIVE` sat **exactly on** the bound that
constrains it, which ruling 38 read as a *floor* rejecting intervals
**below** `DEAD_TIMEOUT` — 25 ≥ 25 held with zero margin — and from that
reading ruling 38 derived that the beacon could never fire at any
admissible interval, documenting the knob as inert. **Ruling 40 reverses
that.** The sentence ruling 38 "corrected" was the half that was already
right: the bound's job is only to reject an interval so long that the
beacon could not keep a connection alive on its own, and that describes a
**ceiling**. The *rule* was the half that was wrong — written as a floor,
and a floor at `DEAD_TIMEOUT` is precisely what made the knob
unreachable, since a beacon firing every 25 s cannot possibly sustain a
25 s death timer. As ruled and later generalized by ruling 282:
`set_persistent_keepalive` rejects an interval **at or above `D_eff`**
(§7.5, §16.2), and the v1 recommended default moved 25 s → **10 s**,
matching `KEEPALIVE_TIMEOUT` so that one lost beacon is still tolerated
inside the 25 s default deadline
(2 × 10 + 5 — the same arithmetic that sizes `DEAD_TIMEOUT` itself).
Ruling 38's derivation is **retained** in §7.5, re-framed as the proof
that the old bound was inverted rather than as a claim that the knob is
permanently inert.

**And ruling 42 restores a floor underneath it.** Replacing the floor
with a ceiling left the interval unbounded below, which is its own
defect: nothing in the text stopped `set_persistent_keepalive(1 ms)`, a
conformant configuration emitting a thousand packets a second on a
beacon §14.5 exempts from the congestion window, when §13.3 already
condemns a 20-packet-per-second cadence as defeating §16.5's timer
economy. The admissible range is therefore **[1 s, `D_eff`)**: the
handle rejects an interval **below 1 s** as well as one at or above the
connection's effective dead timeout. The floor is deliberately far
below the 10 s default —
it forecloses the degenerate configurations, not the useful short ones —
and §7.5 records why 1 s and not something larger.

The beacon remains a **marking** send, and admitting short intervals is
safe precisely because arming **enables** death and never defers it
(§7.4): a beacon fired into a void still dies at
`last_authenticated_recv + D_eff`. What the beacon buys, and the
passive dance cannot, is that it fires **unconditionally** on its own
timer — it does not require `last_authenticated_recv > last_send` — so it
sustains a *mutually idle* link, one that never entered the dance because
neither side ever had traffic to send, which is exactly the state ruling
39 now reaps; and it holds a NAT binding open (§7.5).

There is no DH-rekey trigger at all — neither by message count
(WireGuard's 2⁶⁰) nor by time (WireGuard's Rekey-After-Time): the epoch
ratchet is the only rekey, refreshing keys by count with no handshake
(§5.4, §7.7).

## 6. Staged accept and initiation routing

The responder's staged DH costs become an application-driven typestate,
and inbound initiations route between that typestate and the endpoint's
one internal path, the simultaneous-open tie-break (§6.6–§6.7).
Requirements: every application-visible admission — fresh accept and
replacement alike — is decided by the application at `accept()` (there
are no rekeys, so nothing bypasses it except the tie-break); the 0-DH
drop of an unwanted `Intro` survives; the per-attacker-packet cost is
bounded and stated (§6.9); the 1/2/4 cumulative DH costs are preserved on
the application-visible path; and stage-0-only parking survives, with the
one bounded carried-mid-state exception (§6.5 step 3). Routing keys on
the one-connection-per-static invariant (§16.1).

### 6.1 The typestate and its DH costs

**[RATIFIED 2026/08/14]** The type names `Intro`, `Claimed`, and `Proven`
are final — each is honest about the security state it represents.

| Stage | Cumulative responder cost | Visible to the application | Automatic (non-policy) rejections |
|---|---|---|---|
| `Intro` — length-gated, classified, mac1-verified, parked | 1 keyed hash, **0 DH** | source address, `sender_index` | wrong length (§3.1 — exact for handshakes), unknown type/version, bad mac1 — all silent, before the queue |
| `read_identity()` → `Claimed` | **1 DH** (`es`)† | the **claimed** static | structurally unreadable msg1 (`Malformed`) |
| `authenticate()` → `Proven` | **2 DH** (+ `ss`) | possession proven; the initiation timestamp | tail-tag failure (`HandshakeFailed`); timestamp replay (`Replay` — the guard is not policy) |
| `accept()` → `Connection` | **4 DH** (+ `ee`, `se`; msg2 sent) | an established connection | — |

† A hinted-source entry may arrive with its identity **pre-read** (§6.5
step 3): the 1 DH was charged once, at the eager read, and
`read_identity()` on such an entry returns the cached claimed static at 0
incremental DH. The cumulative table is unchanged either way. The same
holds for every **frozen** mid-state-carrying entry (eager-demoted —
§6.3): its staged accessors return cached results at 0 incremental DH,
and a later same-source initiation parks as a *new* entry that pays its
own ladder.

The table prices `accept()`'s fast path; a **re-homed** `accept()` (§6.4)
adds the admitted candidate's `es` + `ss` on top of the 4 — an
application-driven spend.

**[RATIFIED 2026/08/15 — rulings 74 and 75]** The core's verbs (§16.4)
are keyed by `IntroId` and take `&mut self`, so unlike §6.2's
`self`-consuming handles they **can** be called out of order or twice.
The handle typestate makes both unreachable from an application; these
rules are the core's, and they are chosen so that **no route can perturb
the cumulative cost above**:

- **`read_identity()` is idempotent.** A second call on an already
  `Claimed` or `Proven` chain returns the revealed static and pays **0
  DH** — it opens no provider. The ladder therefore holds under any number
  of calls (ruling 74).
- **`authenticate()` advances a still-parked chain**, driving the skipped
  `es` itself and landing on exactly **2 DH cumulative**. §6.1 prices
  stages cumulatively, so the permissive answer costs precisely the
  ratified amount and no error need be invented (ruling 75).
  **And it is idempotent once the chain is `Proven`** **[AMENDED
  2026/08/18 — ruling 267]**: a second `authenticate()` on the same
  `IntroId` returns the same peer static and the same timestamp and pays
  **0 DH** — the guarantee the block's opening sentence implies and
  `read_identity()`'s bullet states explicitly, now stated for this verb
  too. The ladder holds at 2 DH cumulative however many times either
  verb is called.
- **A structurally unreadable msg1 discards the chain.** `read_identity()`
  returning `IntroError::Malformed` is definitive — 1 DH is spent, the
  entry is destroyed, and its stage-0 slot is freed. This is what
  distinguishes it from `IntroError::Local` (§18.1, ruling 72), where
  *our own* provider failed, the chain is **left parked**, and a retry can
  still succeed. Retaining a malformed chain would hand an attacker a
  per-source slot for the price of unreadable bytes.

Dropping the object at any stage is a **silent reject**: no msg2, nothing
transmitted, the slot freed. The claimed static at `Claimed` is
attacker-choosable (reaching it requires no secret), and the same
discipline applies one stage earlier: `source()` and `sender_index()` are
exposed at 0 DH and are equally attacker-chosen. **Nothing durable may be
keyed on the claimed static, the source address, or `sender_index`** — no
map insertion, no rate-limit bucket, no unbounded logging. Proof of
possession arrives only at `authenticate()`, where a forged claim of a
real static dies at the msg1 tail's AEAD tag.

**Rejection is the application's, and slither records nothing.**
**[RATIFIED 2026/08/14 — ruling 48]** The clause above binds **slither's
own state** and is unchanged. It says nothing about the application's
state, and the distinction is the whole of this ruling: **slither
provides no ban list, no deny list, no reputation store, and keeps no
such state** — nothing durable is written about a peer at any stage,
before or after 2 DH. What the staged typestate gives an application is
the **freedom to reject at any stage, for any reason**, including
consulting a list the application itself owns and stores. Dropping the
object is that rejection: silent, transmitting nothing, at whatever stage
the application has learned enough to decide.

The stage it decides at is a **cost**, and the cost is the one already
priced in the table above.

| Reject at | Cost | What is known |
|---|---|---|
| `Intro` | **0 DH** | source address and `sender_index` — nothing about identity |
| `Claimed` | **1 DH** | the **claimed** static: an unauthenticated assertion |
| `Proven` | **2 DH** | the **proven** static: possession is established |

There is therefore **no 0-DH reject-by-identity**: the claimed static is
not knowable at `Intro` — msg1's static field is encrypted, and reading
it *is* the `es` — so an application that wants to decide on identity
pays 1 DH to learn a claim, or 2 to learn a fact. That is not an
implementation limit to be optimised away; it is what the IK handshake
costs, and it is why the ladder exists.

**The hazard, stated once, because it is a real trade and not a
footnote.** A list keyed on a **claimed** static bans on an assertion
anyone can make: reaching `Claimed` requires no secret, so an attacker
who knows a third party's public static can send initiations claiming it
and get that third party banned by the very list meant to protect the
application — a denial of service the victim cannot detect and the
operator will read as the victim misbehaving. Deciding on a **proven**
static (2 DH, after `authenticate()`) is immune: a forged claim dies at
the msg1 tail's AEAD tag and never reaches the decision. The application
that prefers the 1-DH decision is trading exactly that immunity for one
saved DH under load, and it may be the right trade — an unauthenticated
claim is still a fine key for a cheap early filter when the consequence
of a wrong answer is one dropped initiation rather than a durable
record. **The trade is the application's to make, and slither takes no
position beyond pricing it.** What slither will not do is make the trade
on the application's behalf, or remember the outcome.

### 6.2 The staged verbs

```rust
impl Intro {                                                  // 0 DH so far
    pub fn source(&self) -> SocketAddr;
    pub fn sender_index(&self) -> u32;
    pub async fn read_identity(self) -> Result<Claimed, IntroError>;  // 1 DH (0 on pre-read)
}   // drop = silent reject
impl Claimed {                                                // 1 DH; identity CLAIMED
    pub fn claimed_static(&self) -> &PublicKey;               // never `remote_static()`
    pub async fn authenticate(self) -> Result<Proven, AuthError>;     // +1 DH; guard admits here
}   // drop = silent reject
impl Proven {                                                 // 2 DH; possession proven
    pub fn peer_static(&self) -> &PublicKey;
    pub fn timestamp(&self) -> Timestamp;
    pub async fn accept(self) -> Result<Connection, AcceptError>;     // +2 DH (re-home adds 2)
}   // drop = silent reject
```

Each stage's `async` is a driver round-trip; the DH costs land on the
driver task (§16.3). The staged error types are §18.1's; the verbs they
serve are summarised there. Dropping the object at any stage is the
application's rejection — the only rejection there is, and slither keeps
no record of it (ruling 48, §6.1).

### 6.3 The stage-0 queue

| Constant | Value | Notes |
|---|---|---|
| `INTRO_QUEUE_CAP` | **1024** slots, endpoint-wide | configurable in `Config` |
| `INTRO_MAX_PER_SOURCE` | **4** chains per source IP (per /64 for IPv6) — the sum of unconsumed stage-0 entries and consumed chains | configurable |
| `INTRO_TTL` | **15 s** after the entry's last refresh | ≈ 3 retransmit intervals; the flood hold-cost bound |

The accept queue parks **stage-0 state only** — the raw 196-byte msg1 plus
the source address and the entry's book-keeping. **[AMENDED 2026/08/19 —
ruling 272]** This sentence read *"≈ 220 B per entry; worst case ≈ 225 KB
at the default cap"*, an estimate whose stated construction ("msg1 plus
the source address" — which really is ≈ 220 B) silently excluded seven
further `IntroEntry` fields, dominated by a 96 B chain-state slot reserved
inline in every entry including the parked ones that never use it. The
measured figure at the ratifying commit is **≈ 484 B per entry** (288 B
struct + the 196 B msg1 heap allocation, map-slot overhead excluded), i.e.
**≈ 496 KB at the default cap** — pinned by a core unit test. One bounded
exception stands: an eager-demoted entry carries its already-paid
mid-state (§6.5 step 3). The bound is post-mac1 — a higher bar
than WireGuard's pre-mac1 4096-slot ring.

**[RATIFIED 2026/08/14]** The flood posture below — evict-oldest overflow,
the per-source cap, and the 15 s TTL — is a ruled posture: each is cheap,
none is load-bearing for correctness, and together they change the flood
exposure from a holdable reservation to a per-packet race (honesty clause
below).

- **Dedup key: the full source `SocketAddr` alone; replace-with-newest**,
  and replacement refreshes the deadline. **[RATIFIED 2026/08/14]** (This
  supersedes any `(addr, sender_index)` key: every retransmit is a
  completely fresh initiation with a new random index — §5.5 — so an
  index-bearing key would never match across retransmits, defeating dedup.
  The key is stage-0-only; a peer whose address changes mid-attempt
  recovers by convergence, and post-establishment mobility is roaming's
  job — §7.3.)
  Distinct initiators behind one NAT present distinct ports, hence distinct
  keys; a same-4-tuple collision is a rebind of the same flow, for which
  newest-wins is correct. Replacement initiations from established peers
  park here like any other (§5.4): dedup's replace-with-newest and the
  evict-oldest guarantee give a retransmitting genuine peer the same
  per-packet race as any fresh initiator.
- **Per-source cap**: at most `INTRO_MAX_PER_SOURCE` chains per source IP,
  counting the **sum of unconsumed stage-0 entries and consumed chains**.
  An arrival that would exceed the cap replaces that IP's oldest
  **unconsumed** entry — oldest **by last refresh**, as in evict-oldest
  below (ruling 69); eviction operates on the unconsumed tier only;
  consumed chains are DH-paid, and non-evictable for it: **app-held**
  after `read_identity()`, or **endpoint-frozen** for a
  mid-state-carrying entry (freeze-on-carry, below), which the
  application may never have held — and if the source's whole
  allowance is held by consumed chains, the arrival is dropped. Frozen
  carried entries remain counted under this cap and expire at
  `INTRO_TTL` like any entry (honesty clause below).
  `read_identity()` is net-zero for its source's count (−1 unconsumed,
  +1 consumed). Initiators sharing one public IP share one allowance —
  the cap key is the source IP, so a NAT'd population competes for
  `INTRO_MAX_PER_SOURCE` slots; the constant is marked `configurable`,
  and `Config::with_intro_max_per_source` is the operator's remedy
  **[AMENDED 2026/08/18 — ruling 264]**.
- **Overflow: evict-oldest.** **[RATIFIED 2026/08/15 — ruling 69]** A full
  queue evicts the oldest unconsumed entry **by last refresh — the same
  clock `INTRO_TTL` runs on**, never by original park time. A same-source
  retransmit that replaces an entry's bytes therefore makes it young
  again. This is what the guarantee two bullets above actually requires:
  under park-time ordering a genuine peer retransmitting for 14 s holds
  the *oldest* park time in the queue and is evicted first, while every
  attacker's freshly-parked entry is younger — the precise opposite of
  "the same per-packet race as any fresh initiator". One age key serves
  both expiry and eviction. In favour of the arrival: a genuine initiation
  always obtains a slot — except where its own source's allowance is
  wholly held by consumed chains (the per-source drop above; honesty
  clause below) — and an attacker must win a per-packet race against
  the genuine peer's ~5 s retransmit rather than hold a reservation.
  Consumed chains are never evicted by overflow.
- **Expiry** is silent eviction at `INTRO_TTL`; staged verbs on an expired
  attempt return `IntroError::Expired` (`AuthError::Expired` at that
  stage). An attempt evicted by overflow (above) returns
  `IntroError::Evicted` instead, while the bounded eviction record holds
  its id **[AMENDED 2026/08/18 — ruling 261]**. A consumed chain's mid-state expires 15 s after the initiation
  that fed it. `accept()` alone is exempt — the re-home rule (§6.4) means a
  chain's age never fails an `accept()`, only the absence of any parked
  initiation does.

**Consumption and supersession — own-bytes-on-consume.** An entry is
**unconsumed** until `read_identity()` — and only if it carries no
mid-state (below). While unconsumed, a newer
initiation from the same source transparently replaces its bytes and
refreshes its TTL: same `IntroId`, newest bytes, accessors reflect the
newest bytes at call time, no second surfacing. The moment
`read_identity()` runs, the chain owns its bytes and its `IntroId`: the
source's stage-0 slot is freed, a subsequent initiation parks as a new
entry, and a consumed `Claimed`/`Proven` chain can **never** be superseded
by any later packet — an unauthenticated mac1-valid packet cannot clobber
DH-paid work.

**Mid-state-carrying entries are consumed on arrival — freeze-on-carry.**
Any stage-0 entry that carries a paid mid-state — eager-demoted (§6.5
step 3) — is **consumed from the moment it
parks**: its bytes and its `IntroId` are frozen, a later same-source
initiation parks as a *new* entry subject to the per-source cap, and the
staged accessors never straddle two initiations. An unauthenticated
mac1-valid packet can therefore never byte-replace an entry whose cached
mid-state claims an identity, so `read_identity()` and
`authenticate()` can never report an identity for bytes that identity
never sent. (The minimal-state alternative — replacement replaces such an
entry wholesale, clearing the identity-already-read tag so
`read_identity()` re-pays its 1 DH — is declined: freeze-on-carry closes
an identity-straddling injection surface, and Appendix B's
DH-cost pins assume it.)

**[RATIFIED 2026/08/14]** **`Superseded` appears in no error enum**:
under own-bytes-on-consume no verb can observe supersession, so the variant
is unreachable and deleted rather than retained.

**Mid-states are live key material.** A mid-state — post-`read_identity`,
or carried by an eager-demoted entry — lives inside the endpoint core keyed
by `IntroId`. It holds the endpoint's static provider and the `es`-derived
keys (≈ 0.5–1 KB); bounded by the queue cap and the TTL (§17.5).

**Honesty clause — the queue-occupancy exposure.** mac1's key is public
data (§4.3), so minting mac1-valid initiations costs an attacker only
bandwidth, and no spoofing capability is needed to occupy slots (distinct
source ports are distinct sources). The caps buy: one source is bounded to
4 chains total (consumed and unconsumed together); filling the queue needs
≥ 256 distinct sources; evict-oldest makes full occupancy a per-packet
race, not a reservation; and the TTL prices sustained full occupancy at
≈ 68 packets/second (1024 / 15 s). The denial, while sustained, is
endpoint-wide **for inbound accepts — replacements of established
connections included**, since under the ratchet-only design replacement
initiations park in this queue like any other (§5.4); established
connections themselves keep running regardless — they hold no queue
slot. That last clause needs one qualification, and it is stated here
rather than left to §7.5: an established connection **we dialled**
(`replacement_basis` = `None`) does feel a parked `Intro` for its own
static, because an `accept()` that **admits** that `Intro` and then
refuses it on the basis rule marks the connection **contested** and puts
it on a `K_eff` watch (§6.4, §7.5, rulings 177 and 282 — an
un-admitted candidate, including a walk that exhausts, marks nothing). A
live connection
answers the probe and survives; what it costs is one ack-eliciting PING
per mark, with no second PING and no re-armed deadline for any refusal
landing **while that mark is outstanding** (ruling 41's collapse).
**[AMENDED 2026/08/16 — ruling 175]** *This passage previously claimed
"one such probe per `KEEPALIVE_TIMEOUT` however many Intros the attacker
supplies" and "at most once per 10 s"; both are false.* The collapse
covers only the outstanding-mark window, and a live peer clears the mark
in about one RTT, so the honest rate is `min(refusal rate, 1/RTT)`. So
the claim as amended: established connections hold no
queue slot and are not denied by occupancy, and the only thing a flood
can do to a dialled one is ask it — once per mark, over an
application-paced `accept()` rate — to prove it is
still there, which a live connection does by answering. One occupancy surface is
sharper than the rest and is stated precisely: freeze-on-carry parks
**endpoint-frozen, non-evictable** consumed slots that no application
ever held, so an attacker that spoofs a hint-set source (the dialled
address of an in-flight outbound connect — §6.5) and claims unknown
statics mints up to
`INTRO_MAX_PER_SOURCE` (4) frozen stage-0 slots for that source IP
(/64 for IPv6), at 1 DH each, each held for up to `INTRO_TTL` — and
while the /64's whole allowance is frozen, a genuine **new** peer
sharing it is denied a stage-0 slot until expiry (the evict-oldest
guarantee does not reach the consumed tier). The exposure is bounded
by the per-source-both-tiers cap and the TTL, is open only while an
outbound connect is in flight to that address, touches no established
connection, and requires spoofing that specific dialled address;
genuine peers on any other
source are unaffected. This is not
WireGuard-equivalent exposure (WireGuard's ring is a transient work queue
backed by the under-load cookie gate); until the deferred cookies/mac2
round (§19), the per-source cap is the only occupant-shaped defence.

### 6.4 `accept()` re-homes to the freshest parked initiation

**[RATIFIED 2026/08/14, amended 2026/08/14]** The staged chain proves
*identity*; `accept()`
commits to the *peer*, never to the specific initiation inspected.
Fresh-ephemeral
retransmission re-mints the initiator's index every ~5 s and a msg2
answering a superseded initiation is ignored (§5.5), so a `Proven` chain's
own initiation goes stale in about one retransmit interval — far inside a
human-in-the-loop accept decision. The rule:

- **Fast path.** If the chain's own initiation is still the freshest
  parked for its source, `accept()` proceeds on it at the table's price.
- **Re-home.** Otherwise the endpoint replays the parked candidates for
  the chain's source in order of **park time, newest first** (park time,
  not msg1 timestamp — the timestamp is encrypted and unknown until
  `es` + `ss`). For each candidate it runs `es` + `ss`; a candidate is
  admitted only if **all three** hold: the read yields the **same** proven
  static, the tail tag verifies, and the timestamp guard admits its
  strictly-greater timestamp. msg2 is written for the
  admitted candidate. A failing candidate is discarded and the next-newest
  tried, until admission or exhaustion. The per-source cap bounds the walk
  at four `es` + `ss` pairs. **[AMENDED 2026/08/16 — ruling 177]**
  **Exhaustion — every candidate tried and none admitted — returns
  `AcceptError::Stale`**, and it is a distinct case from "nothing
  parked": it is reachable precisely *because* initiations are parked.
  It is named in the `Stale` list below rather than left to this
  sentence's "or exhaustion", and it marks **nothing**: the contested
  mark below requires an **admitted** candidate, so a walk that reaches
  no admission buys an attacker no probe (§6.9, §7.5).
- **The §16.1 guard — a proven-LIVE admission replaces only against a
  newer basis.** At admission, fast path or re-home: if a LIVE connection
  exists for the proven static, the `accept()` **is** the replacement
  (§5.4) — but **only if** that connection's replacement basis (§17.4) is
  `Some(t)` and the candidate's timestamp is **strictly greater than
  `t`**. Then the install fires `ConnectionLost::Replaced` on the old
  connection and the replacement takes its place: the teardown executes
  exactly at the act that commits it, never earlier, so a withheld or
  replayed replacement initiation left unaccepted costs one parked
  `Intro` and nothing else (§15.4). Otherwise — basis `None` (we dialled
  this connection, so we hold no initiation timestamp of the peer's to
  measure a replacement against) or a candidate not strictly newer than
  `t` — `accept()` returns `AcceptError::Stale` and the live connection
  is untouched. **One exception to "untouched", and it is the point of
  ruling 36:** a refusal of an **admitted candidate** against a
  **`None`** basis marks that connection
  **contested**, which records a **probe floor** (the counter the next
  seal will use), sends an ack-eliciting PING on it, and requires an ACK
  covering **any counter at or above that floor** within the connection's
  `K_eff` of the probe's transmission, on pain of
  `ConnectionLost::TimedOut` — the mechanism, its bound, why an ACK
  rather than a receive is what the question needs, and why the predicate
  is a high-water mark rather than that one packet (ruling 41) are
  specified in §7.5.
  **[RATIFIED 2026/08/16 — ruling 177]** *"Admitted" is load-bearing and
  is stated here because this is the narrower of two texts that
  disagreed.* The mark requires a candidate that reached admission —
  the **same** proven static, a **verifying tail tag**, and the
  timestamp guard passed — so it is a **key-holder** primitive, priced
  in §7.5's security argument as *"an attacker's replay supply buys
  refusals"*. The refusals that reach **no** admission — nothing parked,
  and a re-home walk that exhausts — return `Stale` and mark nothing;
  an attacker able to park only mac1-valid rubbish therefore cannot
  provoke a probe with no key material at all, which is what §6.9's DoS
  table already prices those rows at (*0 DH … one bounded queue slot*,
  never a probe).
  **[RATIFIED 2026/08/16 — ruling 179]** *And one carve-out, stated here
  because this is where the mark is taken.* A mark against a connection
  already **closing or draining** (§15.2) is a **no-op** — no floor, no
  PING, no deadline. A closing connection stays in §17.4's map for its
  linger, which is what makes the `Retired` event and the guard pin
  necessary, so this rule as written would otherwise take the mark and
  leave §7.5 to undo it. §7.5 states the same carve-out from the state's
  side; a rule enforced only in the section that *describes* the state
  and not in the section that *enters* it is a rule that gets missed.
  The connection's protocol state is otherwise unchanged and the refusal
  itself stands either way: a live peer answers — the probe, a PTO retry,
  or any later packet will do — and keeps its connection. Refusing again
  while the connection is already contested adds nothing: no second mark,
  no second PING, and no re-armed deadline (§7.5) — that collapse covers
  refusals landing while the mark is **outstanding**, and once the mark
  clears the next admitted refusal is a full second mark with a fresh
  floor (ruling 175). A refusal against a `Some(t)` basis marks nothing — there
  the basis can decide, and a genuine reconnect carries a strictly greater
  timestamp. There is no `AcceptError::AlreadyConnected`: a
  basis-passing proven-LIVE accept is a replacement, so the variant is
  unreachable and deleted (§18.1; `ConnectError::AlreadyConnected`
  remains for `connect()`, §16.1).
- **What basis and guard bar, stated honestly.** The timestamp guard
  (§17.1) rejects any candidate whose timestamp is ≤ the greatest this
  endpoint has admitted for that static — a set that is **empty for every
  peer we only ever dial**, since all of §17.1's write sites are post-`ss`
  reads of an inbound msg1 and a `connect()` completed by msg2 writes
  none, so against such a peer the guard bars nothing at all and every
  candidate passes it vacuously, however old. The basis rule above
  additionally rejects any candidate not strictly newer than the
  initiation that established the live connection — and on precisely
  those dialled connections the basis is `None` and refuses every
  candidate, which is what carries the protection the guard cannot.
  Neither
  distinguishes a **withheld genuine retransmit** from a fresh one: every
  retransmit is a completely fresh initiation carrying a strictly greater
  timestamp (§5.5), so an initiation captured off the wire and injected
  later is indistinguishable from the peer reconnecting. Such a candidate
  destroys nothing until the application accepts it; applications that
  auto-accept replacements accept that residual. A candidate presented
  against a `None` basis is refused outright — which is what keeps a
  passively captured msg1 from destroying a connection we dialled, the
  case in which we hold no timestamp of that peer's at all.
- **Ordering — the guard's record against the basis check.** The guard is
  *check **and** record* (§17.1), and its record lands **first** on both
  paths: at `authenticate()` on the fast path, and at candidate admission
  on the re-home walk. The basis rule above runs after it, and can still
  refuse. The order is pinned by making the record **conditional on the
  accept**: a guard record made for a candidate whose `accept()` then
  returns `AcceptError::Stale` is **reverted**, leaving the guard exactly
  as it was before the call. This is §17.1's mitigation (i) revert — until
  now stated only for a dropped chain — extended to a refused accept, so
  that an initiation authenticated but never accepted never advances
  endpoint-global replay state by any route, and two conformant
  implementations agree on the guard's contents after a refusal. (An
  implementation may equivalently defer the record until the basis check
  has passed; the observable state is identical, and only the observable
  state is normative.) **One exception, and it is deliberate:** the
  tie-break-**winner** case of the PENDING branch below returns `Stale`
  and **keeps** its record — that record *is* the point of the branch
  (§6.7's winner-side record), the write that denies a later replay of
  that same initiation the vacuous guard pass it would otherwise enjoy.
  No other `Stale` leaves a record behind.
- **PENDING at admission.** If **a pending exists for the proven
  static**, `accept()` applies **§6.7's comparison** over the
  same ordered pair of statics. **[AMENDED 2026/08/16 — ruling 178]**
  *The predicate is **membership in the pending tables** (§17.3), not
  whether a datagram has left the host.* This clause read "an in-flight
  outbound initiation exists", which ruling 90's
  `mint_pending`/`start_attempt` split made ambiguous — a **minted**
  pending has no initiation in flight — and ruling 91 recorded the
  ambiguity as open. It closes here on the table-membership reading,
  because a minted pending is a declared intent to dial: if a peer's
  initiation arriving in that window took §5.4's **NONE** row instead,
  we would install as responder and `start_attempt` would *then* fire,
  giving two sessions, two key sets, both msg2s dropped and both sides
  mutually dark until their effective dead timeouts — exactly the
  divergence ruling 35 exists to prevent. The mechanism was already
  verified in round 8: all
  three readers of "is this static PENDING?" consult the same pending
  tables that ruling 50's cancellation empties, and there is no separate
  per-static flag anywhere.
  - **The peer's static is smaller** — we would be the tie-break *loser*:
    the `accept()` **cancels** the pending — the pending and its index are
    dropped and its `Connecting` resolves
    `Err(ConnectError::AlreadyConnected)` — and the accept proceeds as an
    ordinary fresh install with this endpoint as responder.
  - **Our static is smaller** — we would be the tie-break *winner*:
    `accept()` returns `AcceptError::Stale`, the pending is **left in
    place**, and the candidate's timestamp is **recorded** in the guard
    exactly as §6.7's winner-side record does (§17.1) — the candidate
    authenticated post-`ss`, so the write is a key-holder write like every
    other. Our own outbound completes normally and the peer installs as
    responder.

  The comparison is **not** optional on this path. It is a two-sided
  agreement evaluated over the pair of statics, never over local state, and
  a side that opts out unilaterally reasons only about itself: if we
  installed as responder here regardless, our own in-flight msg1 would
  still reach the peer, which reaches the *same* comparison by §6.6's
  internal route, loses, and installs as responder over *our* msg1. Two
  distinct sessions, two key sets, each side's msg2 dropped by the other,
  mutually dark until their effective dead timeouts (§7.5) — and both ends
  would compute the same stream-ID parity, contradicting §6.7's parity
  consequence. When
  both applications use the `read_identity()` → `connect()` → `accept()`
  ordering, both sides take this branch and the divergence is certain, not
  a coin flip.

  This is the branch that closes the
  `read_identity()` → `connect()` → `accept()` ordering — the static was
  NONE when the chain was staged and became PENDING before it was
  accepted, so §6.5's interception, which fires when a *parked* `Intro`'s
  claim turns out to be a pending outbound remote, cannot fire on a chain
  the application already holds. Without this branch §16.1's
  one-connection-per-static invariant would be broken by two ordinary
  API calls in the wrong order; with the comparison it holds on **both**
  outcomes — the loser side installs the responder session in place of the
  pending it cancelled, and the winner side installs **no second
  connection** at all, keeping only the one its own outbound completes.
  Either way that ordering against one static yields exactly one
  connection.
- **Stale.** **[AMENDED 2026/08/16 — ruling 177 adds the fourth case.]**
  `accept()` returns `AcceptError::Stale` in exactly four situations, and
  this list is the complete enumeration:
  1. no initiation for that static is currently parked;
  2. the **re-home walk exhausts** — candidates *were* parked and every
     one of them failed the three-part admission test above. This is a
     distinct case from (1), reachable precisely when initiations are
     parked, and it was previously missing from this list while §6.9
     supplied it only in passing (*"before it returns `Stale`"*), leaving
     the path with no stated return value;
  3. an **admitted** candidate fails the basis rule above — the only one
     of the four that can mark the connection contested;
  4. the static is PENDING and we are the tie-break winner.

  The application SHOULD
  re-accept when the peer's next initiation surfaces as a new `Intro` —
  the error-prompted case of §6.5's loop obligation (ruling 252), which
  also covers the lost-msg2 case in which no error ever prompts it. On
  the tie-break-winner case there is nothing to re-accept: our own
  outbound is completing that connection, and its `Connecting` resolves
  in the ordinary way. Only case (3) can touch a live connection
  (§7.5's contested mark, and only where the basis is `None`); (1) and
  (2) leave it untouched, and (4) has no live connection to touch —
  it keeps its pending and writes the winner-side guard record described
  above.

### 6.5 The routing rule

Inbound `HandshakeInit` processing in the endpoint core:

1. **Stage 0 (always):** length gate, classify, mac1 verify. Failure is a
   silent drop. Cost: one keyed hash.
2. **Hint check (no DH):** the *hint set* is the dialled addresses of all
   **pendings** — **nothing else** (established
   connections' addresses are not hints; a LIVE static's initiation takes
   the ordinary staged path below). **[AMENDED 2026/08/16 — ruling 178]**
   *"All pendings" is membership in the pending tables (§17.3), not "all
   pendings whose msg1 has left":* a pending minted by `connect()`
   contributes its dialled address from the instant it is minted, so the
   eager path is reachable for a peer whose initiation crosses ours
   inside the `mint_pending`/`start_attempt` window (ruling 90). The
   clause previously read "in-flight outbound initiations". If `src` ∉
   hint set → park at stage 0
   (§6.3) and surface an `Intro`.
3. **Eager path (`src` ∈ hint set):** the endpoint immediately runs the
   split intro read (1 DH, `es` — Appendix A.1) and inspects the claimed
   static:
   - claimed ∈ *pending outbound remotes* → the **internal tie-break**
     (§6.6). The packet never touches the accept queue and the
     application never sees it.
   - claimed ∉ pending outbound remotes → the raw packet is **demoted**
     to the stage-0 queue under the §6.3 rules, **carrying its paid
     mid-state**, tagged identity-already-read: a peer sharing a source
     with a dialled address still surfaces as an `Intro`, and
     `read_identity()` on it returns the cached claim at 0 incremental DH.
4. **`read_identity()` interception (the backstop):** when a parked
   `Intro`'s claimed static turns out to be a pending outbound remote,
   the endpoint performs the same internal tie-break and
   `read_identity()` returns `Err(IntroError::Internal)` — the
   application learns no identity and makes no decision.

The probed set is pending outbound remotes **only**: a PENDING static's
initiation **must** enter the internal path — that is where the
tie-break runs (§6.6, §6.7) — while LIVE- and NONE-state claims are
parked or demoted to the staged path, where a proven-LIVE `accept()` is
the replacement (§5.4, §6.4). The hint's one job is to catch the
crossing msg1 of a simultaneous open cheaply; its false negative (the
peer dialling out from an address other than the one we dialled) parks
the msg1 as an ordinary `Intro` and self-heals at the `read_identity()`
interception.

The false negative does **not** self-heal through retransmission — a peer
dialling from a rewritten source port sends every retransmit from that
same port (§5.5) — so the `read_identity()` interception is the only
backstop, and it is application-driven: two peers that dial
simultaneously, are both NAT-port-rewritten, and never probe inbound
intros will both fail at `HANDSHAKE_GIVEUP` and must retry. The failure
is self-limiting in practice, because the two retries are scheduled by
the applications and so de-synchronise. **Every application SHOULD treat
`accept()` as a loop for the lifetime of its endpoint — diallers and
responders alike.** **[AMENDED 2026/08/17 — ruling 252; this sentence
read "applications that dial", a scope narrower than the hazard.]** For a
dialler the reason is the paragraph above. For a responder it is the lost
msg2: msg2 is never retransmitted (§5.5 — every retransmit is a fresh
initiation), so one lost msg2 leaves this side holding a LIVE,
never-confirmed connection while the peer re-offers a fresh `Intro` every
~5 s until `HANDSHAKE_GIVEUP`; no error ever prompts a retry — the first
`accept()` **succeeded** — and only the next `accept()` closes the gap,
as the §16.1 replacement (§6.4): the unconfirmed connection dies
`Replaced` and the new chain completes. The same loop is what makes
§6.8's *"restart needs no machinery of its own"* true — the machinery it
does need is the application still listening (S34).

**The membership-timing oracle, restated.** **[RATIFIED 2026/08/14, in
reduced form.]** The probed set is the **pending-outbound-remote set
only** — an in-flight-dial oracle, not a configuration oracle, and no
established connection is probed — exposed both as timing (the
tie-break's extra `ss`) and as an explicit API discriminator
(`IntroError::Internal` versus a `Claimed` at one DH less). Statics are
public data, and the window is the lifetime of an in-flight `connect()`;
the exposure is accepted.

### 6.6 The internal tie-break completion

The endpoint's one internal admission path — reached only when the
claimed static is a pending outbound remote (§6.5 step 3, or the step-4
interception). It runs on the already-paid mid-state, in this order:

1. **Tag** — `complete()` (`ss`, +1 DH): a forged claim of a pending
   static dies here at the msg1 tail's AEAD tag. A failure leaves the
   in-flight outbound pending untouched — nothing unauthenticated can
   reach the tie-break.
2. **Guard** — the per-static greatest-timestamp guard (§17.1): strictly
   greater, or the initiation dies here (the shape of a replayed genuine
   msg1, once the guard holds an entry for that static — §6.7's honesty
   clause states what happens before it does). LIVE and NONE statics are
   unreachable in this path: §6.5
   routes them to the staged accept, where a proven-LIVE `accept()` is
   the replacement (§5.4, §6.4). The converse does **not** hold —
   PENDING is not exclusive to this path: a chain staged while its static
   was NONE and accepted after a `connect()` made that static PENDING
   reaches §6.4's PENDING branch instead. That branch is a **different
   route to the same comparison**, not an exemption from it: it applies
   §6.7's ordering over the identical pair of statics and reaches the
   identical conclusion — loser cancels its pending and installs as
   responder (steps 3–4 below), winner keeps its pending, refuses the
   `accept()` with `AcceptError::Stale`, and records the candidate's
   timestamp exactly as step 3's winner side does. The two routes differ
   only in what carries the mid-state and in who observes the refusal;
   they can never disagree, which is what keeps two crossing initiations
   convergent no matter which route each side takes.
3. **Tie-break** — §6.7 decides the race. Our static smaller ⇒ we are the
   winner: the authenticated inbound is silently dropped, its timestamp
   **recorded** (§6.7), and our own outbound completes normally. The
   peer's static smaller ⇒ proceed as responder.
4. **Admit** (the loser side) — only on passing all of the above is the
   strictly-greater timestamp **recorded** as a full admission, our own
   pending cancelled (§6.7), a
   responder index minted (§17.3), and msg2 written (`ee`, `se`, +2 DH).
   The admission completes the connection as an `Install`
   (§16.4), resolving its `Connecting` exactly as a msg2 completion
   would (§6.7), and sets that connection's replacement basis to
   `Some(t)` for the admitted timestamp `t` (§17.4) — on this branch we
   are the responder.

A failure at steps 1–2 is a silent drop with a trace (`slither::policy`),
the pending untouched, nothing recorded. A step-3 winner-side drop is
likewise silent and traced, but it is **not** record-free: the winner
records the loser's timestamp (§6.7). It writes no basis — the winner is
the connection initiator, so its basis stays `None` (§17.4).

### 6.7 Simultaneous open — the deterministic tie-break

When an inbound initiation's claimed static matches a peer to whom we hold
an in-flight outbound pending, the race is resolved by a tie-break
evaluated identically on both sides, never by completion order (each
side's inbound handshake always completes first locally; "whichever
completes first" would install different key sets on the two sides and go
mutually dark for 25 s): **the peer with the lexicographically smaller
static public key is the winning initiator.** The comparison is over the
**canonical static encoding** (§2.4) as unsigned octet strings — always
equal-length within a suite, and identical to the mac1-keying and
identity-map octets, so the comparison is over the `as_ref()` bytes
directly and needs no `Ord` bound (Appendix A.3).

**The tie-break runs only on an authenticated inbound — after `ss`
succeeds.** A match detected at `es` selects the tie-break path but
decides nothing: an `ss` failure is a silent drop with the pending
untouched — **a forgery cannot cancel a pending**. Only when `ss` and
the guard pass (§6.6's order) is it applied:

- **Our static is smaller** ⇒ we are the winning initiator: the
  authenticated inbound is silently dropped (mid-state discarded, no
  msg2) and our own outbound completes normally — but its timestamp
  **is** recorded in the guard (§17.1). The packet authenticated
  post-`ss`, so the write is a key-holder write like every other
  (§17.1's invariant is preserved); recording it is what denies a later
  replay of that same initiation the vacuous guard pass it would
  otherwise enjoy against an endpoint that has admitted nothing from this
  peer. The winning connection's replacement basis stays `None`: we are
  its initiator, and the recorded timestamp belongs to the guard, not to
  the connection (§17.4). §6.4's PENDING branch reaches this same
  winner-side outcome by the staged route — the only visible difference
  is that the refusal is reported to the application as
  `AcceptError::Stale` rather than dropped silently, because there the
  application is holding the chain.
- **The peer's static is smaller** ⇒ we cancel our pending now — post-`ss`,
  on the authenticated inbound (the pending and its index dropped; no
  give-up, no error) — and the tie-break admits and writes msg2 as

  > **[RATIFIED 2026/08/16 — ruling 191] The loser's *dial* resolves
  > differently on the two routes, and the difference is forced.** The
  > winner-side note above says the only visible difference between the
  > routes is `AcceptError::Stale` versus a silent drop; on the **loser**
  > side there is a second one. By this internal route the pending is
  > **promoted in place** — the same connection receives
  > `Install { role: Responder }`, so the application's own `Connecting`
  > resolves **`Ok(Connection)`**. By §6.4's staged route the pending is
  > dropped and its `Connecting` resolves
  > **`Err(ConnectError::AlreadyConnected)`**, with a *new* connection
  > minted for the accepted initiation.
  > Neither is a defect, because the routes differ in **which call owns
  > the resulting handle**: on the staged route the application drove
  > `read_identity()` → `authenticate()` → `accept()`, and `accept()`
  > returns the connection, so the dial has nothing left to deliver; on
  > this route §6.6 guarantees the application never sees an accept, so
  > the `Connecting` is the **only** handle that can carry it. Each route
  > delivers exactly one connection and they differ only in which verb
  > delivers it — but an application writes code against that difference,
  > so it is stated rather than left to be discovered.
  responder (§6.6 step 4).

**What a replay can still do here, stated honestly.** A forgery cannot
cancel a pending. A **replay** of a genuine msg1 can, until the guard
holds an entry for that static: replay and first transmission are
indistinguishable when we have never admitted an initiation from that
peer, so a captured msg1 that arrives while we hold a pending to its
sender passes `ss`, passes the guard vacuously, and — if our static is
the larger — cancels our pending and installs a session the replayer
cannot complete. The cost is bounded, and the bound is **conditional —
stated here as such rather than as an absolute**. The admission records
the timestamp, so a given captured initiation is single-use **per
initiation, and only for as long as that initiation's guard entry
survives §17.1's orphan aging and LRU eviction**. Two qualifications
follow and neither may be dropped:

- **Per initiation, not per capture.** Every retransmit is a completely
  fresh initiation with a strictly greater timestamp (§5.5), so a
  captured `HANDSHAKE_GIVEUP`-long retransmit train is a *set* of
  distinct initiations — one per ~5 s interval, ≈ 18 of them — each
  separately single-use, and the guard's monotonic rule spends them in
  timestamp order.
- **Only while the entry survives §17.1.** The guard entry is what makes
  an initiation single-use; §17.1's own orphan aging (`TS_GUARD_ORPHAN_TTL`)
  and LRU eviction recycle that entry, and a recycled entry re-arms the
  replay. This is why §17.1 **pins guard entries written by a tie-break
  admission or a winner-side record for `HANDSHAKE_GIVEUP` past the death
  of the connection they belong to**: without that pin the bound
  evaporates on exactly the schedule an attacker controls, and the
  vacuous-pass window reopens against a peer we have already admitted.

The winner-side record above closes the same hole from the other
direction, and the resulting session carries nothing and dies at its
effective dead timeout (§7.5). It is strictly weaker than a capture-capable
attacker's baseline ability to drop our handshake outright.

**One mitigation is declined, and recorded as declined so it is not
re-proposed:** *refuse to cancel a pending on a vacuous guard pass*. It
would close this window, and it breaks genuine first contact. On a true
simultaneous open between two peers that have never spoken, **both**
guards are empty and both passes are vacuous; both sides would refuse to
cancel, both would keep their own pending, both would install as
initiator, and the pair would go mutually dark — precisely the
divergence the tie-break exists to prevent. The vacuous pass is not
separable from first contact, so it stays.

Both sides compare the same ordered pair and reach complementary
conclusions, so exactly one session — the winner's msg1, the loser's
msg2 — is constructed, and both sides hold it. A connecting
(never-established) connection that loses the tie-break is completed by
the tie-break's `Install` (§16.4), which resolves its
`Connecting` exactly as a msg2 completion would — connect resolution is
**edge-triggered exactly once** per connection lifecycle. Equal statics
cannot occur (`connect()` to our own static is out of scope under §16.1).
Queued sends are unaffected by either outcome: they live in the connection
core's stream state and pump on whichever session installs (§16.9).

**Stream-parity consequence.** The tie-break winner is the **connection
initiator** for the life of the connection: stream-ID parity (§9.1) is
fixed by this outcome at establishment and never changes thereafter.

### 6.8 Restart handling, summarised

Restart needs no machinery of its own (§5.4) — beyond an application
still draining `accept()` (§6.5, ruling 252): a restarted *peer*
reconnects, its initiation parks as an ordinary `Intro` at our LIVE
static, the live (now-zombie) connection keeps running untouched, and the
`Replaced` teardown fires only at the replacing `accept()` (§6.4); a
restarted *self* holds no state at all — the peer's zombie connection to
us receives nothing it can open, dies at liveness (§7.5), and
the application reconnects, surfacing at our end as an ordinary fresh
`Intro` (or being replaced at its end if we reconnect first).

**Which of the two shapes a restart takes depends on the basis** (§17.4).
Where the zombie is a connection we **accepted**, its basis is `Some(t)`
and the restart replaces at the first `accept()`: the restarted peer
reads a fresh wall clock, so its initiation is strictly newer than the
one that established the connection (§5.3), barring a backwards clock
jump. Where the zombie is a connection we **dialled**, its basis is
`None` and no initiation can replace it — `accept()` returns
`AcceptError::Stale` (§6.4) and the `Intro` is refused, because we hold
no initiation of that peer's against which a captured msg1 could be
distinguished from a genuine reconnect. The restart still resolves with
no machinery, delayed by at most `D_eff` — **but that bound rests
on a premise, and the premise must be named**: it holds when nothing
authentic is still reaching the zombie. Absent an attacker it does: the
zombie receives nothing it can open, so it dies at liveness; the static
drops to NONE; and the peer's next initiation — its retransmit train
re-mints one every ~5 s (§5.5), and its application retries beyond that —
takes the ordinary fresh accept. That bounded delay on connections we
dialled is the price of refusing an off-path replay the same power, and
it is ruled acceptable.

**Against an attacker the premise is false, and the contested probe is
what restores the bound.** §7.4's clock is driven by mere authenticated
receipt, so an adversary who harvested genuine peer→us Data — dropping it
so our replay window never advanced past it — can inject one harvested
packet every less than `D_eff` from anywhere off-path and keep the
zombie's liveness clock reset forever, roaming the session to itself in
the process. Nothing then dies: every genuine reconnect is refused
against the `None` basis, `connect()` reports
`ConnectError::AlreadyConnected` (§16.1), and the pair is wedged for as
long as the attacker keeps dripping. The
**contested-connection probe** (ruling 36, §7.5) is the answer: the same
refusal that would leave the wedge in place now demands an ACK covering a
counter sealed *after* the doubt arose — the probe floor of ruling 41,
which any post-mark packet's ACK satisfies — which no harvested traffic
can supply, so the zombie dies within its `K_eff` of the probe and the
restart resolves after all. The delay is then bounded by the
application's next `accept()` rather than by `D_eff`; under the v1/default
profile the `Intro` is still parked when it comes (§6.3's `INTRO_TTL`).

**[AMENDED 2026/08/16 — ruling 171]** *That bound rests on a premise this
paragraph establishes and never joined to it, which is the failure this
section's own method exists to prevent.* The attacker described above
**roams the session to itself** as it drips, so the probe is aimed at an
address §7.3 has just marked unvalidated and re-armed the budget for —
and if the budget admitted the owed ACK ahead of the probe, the probe
would never leave, `Contested` would never fire, and the zombie would be
immortal after all. Two rules close it, and neither is optional here:
§7.3's priority order puts a **pending contested probe ahead of every
other class of output except CLOSE** at an unvalidated address (rulings
171 and 186 — and a connection sending CLOSE is not a zombie anyone needs
to reap, so the carve-out cannot weaken this argument), and §7.3's
**challenge** means an off-path injector cannot keep the address
unvalidated in the first place, since it spoofs a source it does not
receive at and therefore never sees the eight bytes it would have to echo
(rulings 168, 208). *"Dies within `K_eff` of the probe"* is therefore
true, and true for a named reason. **[AMENDED 2026/08/16 —
ruling 208]** The reason survives the change of mechanism intact, and it
is worth saying why it survives: this attacker is **off-path at the
address it names**, which is the one adversary both the superseded ACK
predicate and the challenge defeat. The class ruling 208 exists to close —
a *connected peer* naming a victim's address — is not this attacker, and
does not reach this argument.

### 6.9 DoS accounting

Per attacker packet; the mac1 verification (one keyed hash) is already
paid by us in every row. Stimulus 196 B (HandshakeInit); msg2 107 B.

| Packet class | Our cost beyond the mac1 hash |
|---|---|
| mac1-invalid garbage / wrong key / wrong suite (curve differs — a same-curve sibling suite is mac1-valid and prices as the rows below; ruling 279) | 0 |
| mac1-valid, src ∉ hint set, `Intro` left or dropped unprobed | **0 DH**, one bounded queue slot (≈ 484 B — ruling 272's measured figure; this row read ≈ 220 B, the raw-bytes estimate) |
| mac1-valid, src ∉ hint set, application probes identity then drops | 1 DH — an application-chosen spend |
| mac1-valid, src spoofed into the hint set (a dialled address of an in-flight connect), claimed static unknown | **1 DH** — the `es` paid once at the eager read and carried through the demotion (§6.5 step 3) |
| forged claim of a pending-outbound static | 2 DH (`es` + `ss`), dies at the tail tag with the pending untouched (§6.7); a forged claim of any other static is an application-chosen spend (the staged rows above) |
| replayed genuine msg1 reaching the tie-break path | **2 DH** (`es` + `ss`), dying at the timestamp guard once an entry exists for that static — before then it survives the guard and can cancel our pending (§6.7's honesty clause), at the same 2 DH; on the staged path the same replay costs only what the application chooses to probe — the sustained 2-DH primitive is confined to the in-flight-dial window **and, within it, to the half of the peer population whose static sorts below ours** (§6.7's key ordering; the pending survives the replay outright on the other half) |
| genuine replacement initiation from a key-holding peer | staged like any other: DH only as the application probes; + 2 DH (`ee` + `se`) only per `accept()` — replacement admission is basis-gated and application-gated (§6.4, §17.4) |
| authenticated peer sends a violating frame stream (credit breach, limit breach, stream-state or final-size violation, or a post-AEAD structural failure — §8.2) | no DH: one parse + one CLOSE seal + the 5 s linger (§15.2) |
| authenticated peer floods packets during our linger | ≤ 1 CLOSE reply per second, to authenticated window-fresh inbound only (§15.2) |
| mac1-valid, index-matching forged msg2 (initiator side) | **2 DH** (`ee` + `se`), dies at msg2's tag; spends that interval's completion attempt (§5.5) |

**Key ordering bounds the replay-cancels-a-pending primitive.** Ruling
35 put that primitive back inside §6.7's key-ordering bound on *every*
route into it. Both admission paths — §6.6's internal tie-break and
§6.4's PENDING branch — now cancel a pending only when the replayed
initiation's static sorts **below** ours, so against any given peer the
primitive either exists for the whole in-flight-dial window or does not
exist at all, decided by a comparison the attacker cannot influence
(it holds no key and cannot choose the statics). The secondary gain is
population-scale: §6.4's PENDING branch previously cancelled the pending
**unconditionally**, which made the primitive key-order-independent on
that route and so roughly doubled its reach across a population of peers
— every peer was vulnerable on the `read_identity()` → `connect()` →
`accept()` ordering, where now only the half that sorts below us is.

**The ceiling, explicitly:** the maximum cost of any single attacker
packet is **2 DH** — on the responder side and, via the forged-msg2 row,
on the initiator side too — and only 1 of those is reachable without
holding a hint-set address, observing our 32-bit index (on-path in
practice), or the application choosing to spend.

**Three rate-honesty notes** complete the table. The eager `es` of §6.5 is
**rate-ungated** per spoofed hint-set source: the §6.3 caps bound *state*,
never *work* — mac1-valid initiations from a spoofed hint-set address (a
dialled address of an in-flight connect; the exposure window is that
dial's lifetime) cost 1 DH each, every time, until the deferred cookie
tier (§19) prices
them. Each such unknown-static demotion also parks **frozen** (consumed
on arrival, §6.3), occupying a non-evictable consumed slot, so the
per-source cap — not evict-oldest — is what bounds this occupant
surface until the cookie tier: ≤ `INTRO_MAX_PER_SOURCE` (4) frozen
stage-0 slots per /64 at 1 DH each, `INTRO_TTL`-expiring, denying a
genuine new peer of that /64 a slot while held (§6.3's honesty
clause). A re-home walk's candidates are **attacker-fillable**:
mac1-valid rubbish parked for a chain's source can cost a legitimate
`accept()` up to four wasted `es` + `ss` pairs before it returns `Stale`
— application-driven and cap-bounded, but attacker-provoked. And
**the protocol paces replacement not at all**: a key-holding peer
can present a fresh, strictly-greater initiation as fast as it can send,
and an auto-accepting application replaces its own connection on each,
paying a full teardown plus 4 DH per accept and §17.5's state churn with
it. Applications that auto-accept replacements SHOULD rate-limit
replacing accepts per static; the protocol's only guarantee is the one
§16.1 gives — every replacement keys on the *proven* static, so the
damage is confined to the connection to that static and a key-holder
harms nobody but itself.

One initiator-side exposure completes the accounting: a mac1-valid,
index-matching but cryptographically invalid msg2 spends the initiator's
completion attempt for that interval (§5.5). This is an on-path
(index-observing) capability — off-path requires guessing a 32-bit
index — and its full mitigation is foreclosed by hiss's consuming state
machines; the attempt refreshes at the next scheduled retransmit, so a
sustained on-path forger denies the handshake for at most the 90 s
give-up and can never induce initiations faster than the schedule.

A re-homed `accept()` pays the inspected chain's `es` + `ss` plus up to
four candidates' `es` + `ss` plus the admitted one's `ee` + `se` —
app-driven, post-authentication, bounded by the
per-source cap; not attacker amplification.

**The contested-connection probe's cost, accounted here rather than only
at §7.5.** An `accept()` that **admits** a candidate against a live
connection whose `replacement_basis` is `None`, and refuses it on the
basis rule, marks that connection contested and sends one ack-eliciting
PING on it (§7.5, rulings 36/41/43). **[AMENDED 2026/08/16 — ruling
177]** *This sentence previously read "a refused `accept()`", which
scoped the mark to **any** refusal and put it a whole authentication
tier below §6.4's rule.* §6.4's narrower rule governs: the mark requires
an **admitted** candidate — same proven static, verifying tail tag,
timestamp guard passed — so a walk that exhausts without admitting
anything, or a chain with nothing parked, returns `Stale` and marks
**nothing**. An attacker who can park only mac1-valid rubbish therefore
buys no probe at all: this table already prices those rows at *0 DH …
one bounded queue slot*, and that price is correct as written.
The refusals are
`accept()` calls, but the *Intros* that provoke them are
attacker-suppliable — that is exactly what §6.3's queue exists for — so
this is an attacker-adjacent cost and belongs in this table's reasoning.
**[AMENDED 2026/08/16 — ruling 175]** *The bound previously stated here
— "at most one `MAX_DATAGRAM`-bounded packet per `K_eff` per
live connection … no matter how many Intros arrive" — was false, and
§7.5 stated the same false thing.* Ruling 41's collapse suppresses only
refusals landing **while a mark is outstanding**; a live peer ACKs in
about one RTT and clears the mark, after which the next refusal is a
full second mark. The honest cost is **one `MAX_DATAGRAM`-bounded packet
per mark, one mark per uncontested refusal, marks never overlapping**,
so the per-connection rate is `min(refusal rate, 1/RTT)` — the refusal
rate being the application's own `accept()` rate over an attacker-fed
Intro supply. No cooldown is imposed (§7.5): the security half is
unaffected, because every re-mark records a fresh floor. The packet is
exempt from the congestion
admission gate but counted in the sent map (§13.5, §14.5), and it is
capped by §7.3's budget at an unvalidated address like everything else —
where, being a **pending contested probe**, it outranks every other
class of output but CLOSE (§7.3, rulings 171 and 186).
The state is likewise one slot regardless of the rate: a single optional
`(probe_floor, deadline)` per connection, never a list (§17.5).

**No amplification.** The accept path replies 107 B (msg2) to a 196 B
stimulus — ratio < 1 — and no path in this specification emits bytes **to
an address that has not authenticated**: no cookie replies, no error
packets, the `0x04` packet type is never emitted, CLOSE exists only
inside the seal and its linger replies only to authenticated,
window-fresh inbound (§15.2), every staged rejection is local and silent,
and a peer-supplied anchor or roam target is send-capped by the
anti-amplification budget until it is validated — §7.3's **challenge**
predicate, a `PATH_RESPONSE` echoing eight bytes we sent to that address
after the anchor or the roam, which a spoofed source can never produce
because it never receives them (§7.3, rulings 168, 208).

**One qualification, since ruling 40 made the beacon reachable.** The
`ratio < 1` above is a property of the *accept path itself*, and it
holds unconditionally. It is not, by itself, the bound on everything an
accepted session may emit toward a msg1 source that has not yet
validated: an application that configures a persistent keepalive (§7.5)
turns a half-open session anchored at a replayed initiation's spoofed
source into a beacon aimed at that source, which §7.4's "a replayed
initiation never turns us into a keepalive source aimed at a spoofed
address" states without that qualification and which is corrected there.
The **operative bound in that case is §7.3's budget, not the msg2
ratio**: total output to the unvalidated address may not exceed
`AMPLIFICATION_FACTOR` (3) × the authenticated, window-fresh bytes
received from it,
so the worst case is 3 × 196 B = 588 B for one replayed msg1 — a ratio
of 3, the maximum the budget permits anywhere in this document, and
reached only where the application opted the beacon on. **[AMENDED
2026/08/16 — ruling 168; predicate superseded 2026/08/16 — ruling 208]**
That 588 B bound survives the budget's disarm condition, and the premise
is named rather than assumed: **nothing receives at the spoofed source**,
so nothing there can echo the eight challenge bytes, the address never
validates, and the cap therefore binds this case for the session's whole
life — exactly as the superseded permanent-cap text claimed for every
case. Note which premise now carries it. Under ruling 168 the premise was
*"the spoofed source is not the peer"*, and that premise was the defect:
it is a statement about **who holds the key**, and the key holder can mint
any ACK field it likes. The challenge does not ask who the sender is; it
asks **who received**, and a source nobody receives at fails that question
whoever holds the key. The honest statement is therefore: **no path to an
address that has not answered a challenge exceeds the 3× budget, and the
accept path with no beacon configured stays far under it at 0.55×.** The
scope qualifier matters: once an address *has* validated — a
`PATH_RESPONSE` matching a challenge we drew after the anchor, which only
a party that received at that address can produce — the budget disarms and
the ratio stops being an amplification figure at all, because there is no
longer a third party to reflect toward. Every attacker-relevant case in
this table is a case that never validates. The
alternative of suppressing the beacon entirely while an address is
unvalidated — the beacon's NAT-holding job being arguably meaningless
before the address has answered a challenge (§7.3; *"before return
routability is proved"* is how this read before ruling 210(c), and it
overclaims what any challenge proves) — would restore the ratio outright
and is **left open** rather than adopted here, because it is a
behavioural change to the beacon and the budget already bounds the
exposure.

## 7. Session layer

### 7.1 The counter is the packet number

The hiss `DatagramSend` counter **is** the packet number: monotonic,
hiss-owned, never caller-chosen, simultaneously the AEAD nonce and the
epoch selector. ACKs reference it directly (§12); there is no second
identifier, so a retransmission is never ambiguous (frames are
retransmitted, never packets — §13.5) and there is no Karn's problem. It
rides in cleartext because the receiver decrypts with it (§3.4).

Packet-number spaces are **per direction, one per connection for its
whole life**: a connection has exactly one session (§5.4), so the counter
runs from 0 at establishment and never restarts — there is no
re-handshake, no second counter space, and no make-before-break demux.
Every seal — payload, control, keepalive — burns the next counter.

### 7.2 The anti-replay window

| Constant | Value |
|---|---|
| `REPLAY_WINDOW` | **2048** bits (an RFC 6479 sliding bitmap, `[u64; 32]`, 256 B per connection) |

- The window tracks a greatest authenticated counter plus the 2048-bit
  bitmap. The replay check is strictly **post-AEAD**: check-then-mark only
  after `decrypt_at` authenticates. A duplicate, or a counter more than
  2048 behind the greatest, is dropped after decryption **without
  delivery**. The window's greatest advances only on authenticated
  counters; hiss's `MAX_EPOCH_JUMP` and commit-and-cap bound forged-counter
  cost upstream of it (§2.1).
- **Liveness, roaming, and §7.3's amplification budget are driven only by
  packets that are both
  authenticated and window-marked** (fresh). No replayed packet ever moves
  the endpoint, refreshes liveness, or funds the budget.
  **[AMENDED 2026/08/16 — ruling 169]** The budget is named here because
  §7.3 previously funded itself on the broader *authenticated* class and
  excluded only the unauthenticated, which on the literal text let a
  replayed packet replenish a security counter. Three readers, one class,
  and this is the complete list of what reads it.
- Sizing: 2048 sits above boringtun's 1024 and below the kernel's 8192;
  it is ~20 ms of reordering memory at 1 Gbps line rate and ~200 ms at
  100 Mbps, comfortably inside one ratchet epoch (65 536), with margin for
  any future truncated-packet-number scheme.

**[RATIFIED 2026/08/14]** The ACK record stays **fused** to this window (§12.2):
the window is the single received-packet record — reuse, don't duplicate.
The failure mode under congestion control is bounded and benign: ACK
fidelity is capped at 2048 counters, so only an ACK-loss burst longer than
the window's time-width causes delivered-but-unreported packets, which
surface as spurious retransmissions (streams dedup by offset, §9.5) and at
worst one spurious congestion event absorbed by the recovery-period rule
(§14.3). A reviewer could reasonably demand the decoupled QUIC-style range
tracker now; it is instead the flagged upgrade when sustained >100 Mbps
per connection matters (nothing on the wire changes for it — the ACK
encoding is already range-based), and the Appendix B ACK-loss-burst
simulation quantifies the exposure before ratification hardens (§19).

### 7.3 Roaming

An **authenticated, fresh, window-marked** Data packet whose source
differs from the session's current endpoint moves the endpoint to the new
source. Nothing unauthenticated, and no replayed packet, ever moves it.
**Handshake packets never roam a live session** — an accepted initiation
*anchors* a new or replacement session at its msg1 source, which is not
roaming. Observability is the `remote_address()` accessor plus the
`slither::roam` trace target (§18.2); at the core level the connection
emits `ConnEvent::AddressMoved`, which is what updates `remote_address()`
and fires the trace (§16.4, §16.8). Roaming additionally resets the congestion
controller with the pre-roam flight fenced off (§14.6) — new wiring
relative to all prior slither: a new path
carries no continuity evidence for the old window.

**The anti-amplification budget.** **[RATIFIED 2026/08/14, amended 2026/08/14; amended 2026/08/16 — rulings 168, 169, 170, 171]** Roaming moves the
endpoint on one authenticated packet, and an accepted initiation anchors
at its msg1 source (§5.6) — in both cases a peer-supplied address becomes
a send target with no return-routability proof, while §13.4 and §14.5
exempt whole output classes (PTO probes, the contested-connection probe,
pure ACKs, CLOSE, keepalives)
from the congestion window. Unchecked, that is a reflector. The rule:
whenever a session's endpoint address changes (a roam) or is first
anchored from a msg1 source, the address is **unvalidated** and a
send-side budget arms — total bytes sent to the address MUST NOT exceed
`AMPLIFICATION_FACTOR` (= 3) × total bytes **received from it,
authenticated, and window-fresh** on this session, both counters resetting
at each such
address change. **[RATIFIED 2026/08/16 — ruling 169]** Authenticated and
window-fresh means the packet's AEAD tag verified **and** §7.2's replay
window marked it as not-yet-seen — §7.2's *authenticated and
window-marked* class, the same class that is the only thing permitted to
move the endpoint or refresh liveness, and the class this section's own
opening sentence already names (the anchoring initiation qualifies, its
handshake tail tags having verified at admission). Three kinds of bytes
MUST NOT replenish the budget, and the list is exhaustive in both
directions — nothing outside the authenticated, window-fresh class ever
funds it: merely-received bytes; unauthenticated or undecryptable
datagrams claiming the address; and **replayed duplicates of packets
already counted**. The third is the one the earlier text admitted by
accident, by citing the broader *authenticated* class and then excluding
only the unauthenticated: a keyless on-path attacker who could refund the
budget by re-injecting bytes we have already counted would inflate our
send allowance toward an address of its choosing, which is the reflector
this rule exists to prevent. No argument has ever been offered for
letting duplicates fund a security counter.

**The scope is one session.** **[RATIFIED 2026/08/16 — ruling 170;
restated for ruling 208's state]** Both byte counters, and the
**outstanding challenge** below, are **per unvalidated address, per
session**. They live in `core::Connection` beside the roam
seam (§13.6, §14.6) — the only place they are implementable, and the only
place §17.5's per-connection state census budgets them; there is no
endpoint-side per-address table. The residual is stated here rather than
left to inference: `N` sessions anchored or roamed to the **same** address
carry `N` independent budgets, so a peer holding `N` sessions against one
victim address multiplies the reflector by `N`. That is routine under
NAT, and routine in §6.9's threat model. It is bounded by the number of
sessions the application accepts — the endpoint's governing scale in §6.9
and §17.5 — and it is the price of putting the counters where the roam
seam is.

**Disarming: an unforgeable challenge, echoed from the address.**
**[RATIFIED 2026/08/16 — rulings 208, 210; this supersedes ruling 168's
`validation_floor` entirely — the two are alternatives, never
complements, and leaving the ACK predicate in place beside this one would
be an unlocked bypass around it]** The unvalidated state **ends**, and the
text says how. At each address change — a roam, or the first anchor from a
msg1 source — the connection, in the same act that arms the budget, draws
a challenge and owes it to the new address:

> `challenge` = **8 opaque bytes**, drawn from the connection core's
> **per-connection sub-seed** (§16.6) — *not* from the endpoint RNG,
> which `core::Connection` cannot reach (ruling 210(b)). The connection
> owes a `PATH_CHALLENGE` (`0x1a`, §8.3/§8.4) carrying those bytes to the
> new address. **One challenge per arming, never reused across armings**;
> a fresh one is drawn at every address change.

The address becomes **validated**, and the budget **disarms**, at the
first **authenticated, window-fresh packet from that address** carrying a
`PATH_RESPONSE` (`0x1b`) whose eight bytes equal the challenge
outstanding for the current arming. A `PATH_RESPONSE` that does not match
— stale, drawn from a prior arming, or invented — validates nothing; it
is **not** a structural error and is otherwise ignored (§8.4). Until the
match arrives the 3× cap binds all output exactly as stated above. At
validation the two byte counters and the outstanding challenge are freed
and the cap no longer applies to that address; they are **re-armed, with
a fresh challenge drawn, at the next address change** — §13.6 lists this
among the roam seam's per-connection resets.

**The arming triggers are unchanged, and so is the ordering.** The
endpoint still **commits** the roam on the authenticated, window-fresh
packet that carries it (this section's opening rule), and validates
**afterwards**, with the budget binding in the interval. slither does not
probe a candidate path and switch on success — that is QUIC's model, it
needs a second path's worth of state, and §19 keeps it deferred. Refused
output is still **held, not dropped**.

A `PATH_RESPONSE` matching the challenge can only have been minted by a
party that **received the packet we sent to that address after the
change** — which is what the budget needs, with no state beyond eight
bytes and one flag per connection. The bytes are unguessable: eight bytes
from a CSPRNG stream the peer never observes, redrawn at every arming, so
a party that did not receive at the address has one chance in 2⁶⁴ per
attempt, cannot accumulate attempts across armings, and cannot mine them
offline. If nothing at the new address ever answers, nothing is validated
and the session dies by liveness inside 25 s — unconditionally, since any
ack-eliciting output we aim at the address arms the death clock by
itself, even where nothing marking is sent (§7.4); `PATH_CHALLENGE` is
itself ack-eliciting (§8.3), so the very packet that asks the question
arms the clock on the answer. There is no deadlock in
either direction: the budget always admits *something* (the anchoring or
roaming packet funds 3× its own size, and 3× the *smallest* packet that
can arm the budget — §7.5's 30-byte keepalive, 90 B — still admits a
packet carrying the challenge, which costs 14 B of header, 9 B of frame
and a 16 B tag), and what it admits is enough to
elicit the `PATH_RESPONSE` that ends it.

That last clause is a claim about the **sender**, and it is only true of a
sender that **sizes** its output to the room the budget admits. A sender
that builds a full-size packet, finds it refused, and holds it whole has
made the escape unreachable while satisfying every word of the
*held-not-dropped* rule — the defect ruling 203 found under the
superseded predicate, which this design does not repair and does not
weaken. Sizing to `min(MAX_DATAGRAM, room)` is what makes the no-deadlock
argument above a fact rather than an aspiration.

**What the challenge proves, and what it does not.** **[RATIFIED
2026/08/16 — ruling 210(c)]** Stated plainly, because the failure this
whole change corrects was a sound argument read past its scope. The
challenge defeats the adversary ruling 208 was written for: **a peer that
never received at the address it named.** That peer holds the session keys
and can therefore mint any plaintext field it likes — which is exactly why
an ACK's `largest` was never a proof — but it cannot echo eight bytes it
has never seen. The challenge does **not** defeat an **on-path attacker
able to carry packets to and from the real peer**. Such an attacker names
a victim's address, relays our `PATH_CHALLENGE` onward to the peer,
relays the peer's `PATH_RESPONSE` back, and the address validates — and
it *should*, because at that point packets aimed there genuinely do reach
the peer, which is all any return-routability check can ask. This is the
ceiling of the mechanism rather than a gap in slither's use of it; QUIC's
path validation has the same ceiling (RFC 9000 §9.3). **"Return
routability is now proven" is therefore false as an unqualified
sentence**, and writing it that way would reproduce, inside this fix, the
unstated-scope defect the fix exists to correct: an argument sound against
one adversary, read as sound against all. What is proven is narrower and
is enough for what this budget is for — **the address is not a pure
reflector**, because something there is talking back.

**Why the literal permanent cap could not stand, recorded so the
reversal is not re-litigated.** The budget arms on *every* first anchor
from a msg1 source, so **every responder-side connection begins
unvalidated**. Under a cap that never ends, an accepting endpoint could
never send more than 3× what it receives for the connection's entire
life — and a peer downloading a file replies with ACKs only, ~40 B per
~2400 B sent (§12.4's delayed ACK), funding ~120 B of budget against
2400 B of demand. **An endpoint that accepts connections could never
serve one.** The superseded text's reassurance — that *"a genuine peer
clears it within about one round trip, because its own authenticated
traffic funds the budget continuously"* — is true only of a **symmetric**
request/response exchange and false of every asymmetric transfer, which
is what made the permanence look harmless; and "clears it" is transition
language for a transition the old rule did not define.

**What is still never lifted.** The **3× ratio itself** is never raised,
never lowered, never configurable, and never waived. For as long as an
address is unvalidated it binds **all** output to it, **explicitly
including the §14.5 and §13.4 congestion-window exemptions** — those
exemptions are scoped to cwnd, never to this budget — and it caps every
byte any accrued window could discharge there (§14.5). It is the
amplification factor QUIC accepts (RFC 9000 §8.2/§9.3), and it forces an
attacker to pay a third of any flood it reflects, removing the reflection
incentive at zero protocol machinery. What ends is the **unvalidated
state**, not the ratio — and RFC 9000 is now cited for what it says: QUIC
binds that limit *until the address is validated*, and validates it by a
return-routability proof, which is exactly the shape adopted here.

**The declination this section recorded, and its reversal.** **[RATIFIED
2026/08/16 — rulings 208, 210(a)]** This passage is kept rather than
deleted: the alternative now adopted is one this section explicitly
declined, and a declination that vanishes when it is reversed teaches
nothing.

(1) *An N-authenticated-packets-over-1-RTT validation unlock* — **still
declined**, on a ground ruling 208 does not disturb: N authenticated
packets can be **replayed** at us by an off-path attacker, so the scheme
carries the very reflection property it was meant to remove, and it costs
more state than either alternative.

(2) *Explicit `PATH_CHALLENGE`/`PATH_RESPONSE` validation* — **declined
by ruling 168, adopted by ruling 208.** The declination's stated ground
was **wire cost**: two new frame types, wire-affecting, golden-wire pin
red. Nothing else was held against it. That ground has been weighed again
against a fact the declination never considered — **nothing has shipped**
(§1.1) — so the cost is at its lifetime minimum today and rises
permanently at first publication. Re-proposing a declined idea normally
wastes a round; this reversal is legitimate because it answers the
declination's own reason rather than stepping around it. What §19 still
defers is the *other* half of the QUIC model — validation **before** roam
commit — not the frames.

**The sentence that made the declination look safe, quoted because it is
the whole defect.** Against `PATH_CHALLENGE` the superseded text argued
that an ACK covering a counter we chose after the address change *"cannot
be manufactured without the key"*. Every word of that is true. **The peer
has the key.** The comparison was drawn throughout against an *off-path
attacker*, while §7.3's roaming threat model **is the peer** — the peer is
the party that tells us where to send — and the paragraph never asked the
question. An ACK is `{ largest, ack_delay, first_range, ranges }`, four
plaintext integers under AEAD: `largest` is not evidence of receipt, it is
an **assertion by whoever holds the key**. So a connected peer announced a
move to a victim address, waited for one sealed packet, and returned a
forged ACK spoofed from the victim — **two small packets**, after which
the budget was gone and reflection at the victim was unbounded, defeating
this section's own stated purpose (*"forces an attacker to pay a third of
any flood it reflects"*) at O(1) where the pre-168 rule charged that third
continuously. The attacker needs source-address spoofing, but it needed
that to fake the move at all: ruling 168 added no requirement, it removed
the ongoing cost.

The superseded proof had **scoped itself out of this in its own words**.
It argued that an ACK's coverage derives from the peer's replay window
(§12.2), *"which cannot contain a counter the peer never received, and an
**attacker** holds only packets we sealed before the floor."* That is
sound — against a **third party**. It says nothing about the peer, and the
word "attacker" sitting in the middle of it is what disguised the gap.
This is the spec's most productive defect class — a stated construction
with an unstated scope (working rule 8) — occurring inside the proof of
the ruling that reversed a declination. It is recorded at length so the
reversal is a ruling and not an excavation, and so that the same reading
is applied to the replacement: see *what the challenge proves, and what it
does not*, above.

**Priority within a scarce budget.** **[RATIFIED 2026/08/16 — ruling
171]** The budget binds all output and cannot be waived, so when it
admits less than is owed, *something* must yield, and the order is
normative rather than left to queue order:

1. **CLOSE** (§15.2) — **[RATIFIED 2026/08/16 — ruling 186]**.
2. **A pending contested probe** (§7.5) — ahead of all other output to an
   unvalidated address.
3. **`PATH_RESPONSE`** (§8.3) — **[RATIFIED 2026/08/16 — ruling 208]**.
4. **`PATH_CHALLENGE`** (§8.3) — **[RATIFIED 2026/08/16 — ruling 208]**.
5. Pure ACKs.
6. PTO probes (§13.4).
7. Keepalives — passive and persistent (§7.5).
8. Retransmissions (§13.5).
9. New application data — STREAM and DATAGRAM fill (§8.5).

**Why the two path frames rank where they do, and why their order
relative to each other is not load-bearing.** They are the *only* output
whose delivery **ends** the scarcity every other rank is competing
inside; every class below them is contending for a budget that a single
round trip removes, so ranking them under the queue they unblock inverts
means and ends. They are placed under the contested probe rather than
above it because the probe's deadline is a **liveness verdict** that a
delay converts into a death (§7.5, and the argument quoted below), where a
delayed challenge only prolongs a cap. Between themselves the order is
free: each costs 9 bytes of frame, so a packet carrying **both** costs
14 B of header + 18 B of frames + a 16 B tag = 48 B, and the smallest
budget any arming can produce is 3 × the 30-byte keepalive that armed it =
90 B (§7.5, ruling 203's arithmetic). They therefore never contend with
one another **at the arming instant** — **[AMENDED 2026/08/17 — ruling
250]** the floor is a property of the budget when it is armed, and by pump
time the budget may have been spent down by whatever left since, so the
guarantee the pump keeps is checked against the **remaining** room when
the packet is built (ruling 207(c)'s seam), never asserted from the floor
— and `PATH_RESPONSE` is listed first only because answering an obligation
before raising one is the conventional reading.

**[RATIFIED 2026/08/16 — ruling 215, closing this section's one open
flag. §1.3's expected flag count returns to zero.]** An interaction
between ranks 2 and 4 that neither ruling 171 nor ruling 208 anticipated.
**The ranks above are correct as written and do not move; the send pump's
early return is the defect.**

A **pending** contested probe is realised in the send pump as an **early
return** — the pump emits the probe and stops, so nothing ranked below it
is built on that pass. Rank 4 is below it. An address that is
simultaneously **unvalidated** and holding a **pending** probe therefore
never builds its `PATH_CHALLENGE` while the probe is pending, and the two
states co-occur by construction rather than by coincidence: §6.8's
attacker roams the session to a fresh source *and* is the reason the mark
was taken, which is the one scenario both rulings were written for. Both
rulings govern what an unvalidated address may send under a scarce budget;
neither knew about the other.

**Where the defect lives is itself unsettled, and that is why this is a
flag and not an erratum.** Nothing in §7.5 or in ruling 171 mandates an
early *return*; they mandate a **rank**. The early return is the send
pump's realisation of that rank, and it is a strictly stronger reading —
rank 2 outranking rank 4 does not mean rank 4 is never built, only that it
yields when the budget cannot hold both, and here the budget can: probe
and challenge together cost 14 B of header + 1 B of PING + 9 B of
challenge + a 16 B tag = **40 B** — one coalesced packet, one header, one
tag — inside the 90 B floor at arming, and checked against the remaining
room at pump time (ruling 250).
Three resolutions were available — coalesce the challenge into the probe's
own packet, lift the challenge above the probe, or hold that the priority
order was never an early return and the pump is simply wrong.

**Ruling 215 takes the third, and it concedes nothing.** Lifting the
challenge above the probe was ruled first (212(c)) and **reversed**: it
would demote the probe below a frame whose delay merely prolongs a cap,
while §7.5 proves the probe's own delay *"would silently convert
congestion into a liveness verdict"* — an argument that transfers verbatim
to the budget, and which nothing in the challenge's case answers. The
ranks above therefore stand exactly as written, and **the send pump may
emit the probe and continue building on the same pass**. **[AMENDED
2026/08/17 — ruling 250]** The pump prefers **one packet**: the probe
coalesces the owed path frames when the remaining room at pump time admits
the coalesced size — 40 B with one 9 B path frame owed (the packet this
section's own arithmetic has priced all along), 49 B with both, which a
roam constructs (§13.6 keeps the owed `PATH_RESPONSE` and re-draws the
challenge at one instant) — and emits the bare 31 B PING otherwise, the
path frames
following at their rank when room next admits them; when room admits
neither, nothing is emitted and no `Pto` deadline is announced (§13.3,
ruling 249). One packet, one counter, one sent-map entry — ruling 221's
deletion of the dedicated-packet machinery is not resurrected. "The budget
holds both and always does" was a universal over **armed** budgets with an
unstated scope; the pump-time room check is the guarantee the
implementation can keep.

**Coalescing is all-or-nothing over the owed set, and the "otherwise"
above is that rule.** **[AMENDED 2026/08/18 — ruling 258]** The three
sizes are indexed by what is *owed*, not by what fits: with both path
frames owed, a remaining room of 40–48 B admits one of them and the pump
nonetheless emits the bare 31 B PING, leaving **both** to ranks 3 and 4.
The alternative — packing whichever frames fit — buys one 9 B frame in a
9 B-wide window, at the cost of a per-frame room check that must also
reserve the PING's own byte, since §8.5 packs the PING after the path
frames and a probe packet with no PING is not a probe. One invariant,
checked once, is worth more than nine bytes recovered on a window this
narrow. (The residue either choice leaves is unusable in both readings:
40 − 31 = 9 and 48 − 40 = 8 are each below §3.4's 30 B minimum datagram,
so no later packet is funded by the bytes saved.)

The probe's place is the load-bearing one, and §7.5 already makes the
argument exactly once, for the congestion gate: *"a probe the gate could
delay past its own deadline would silently convert congestion into a
liveness verdict."* The argument transfers verbatim to the budget. The
attack it forecloses: an adversary holding harvested peer→us Data injects
one small packet just under `D_eff` **from a fresh source each
time**, which refreshes liveness, roams the session (re-arming the
counters at that one packet's bytes), and leaves too little budget for
the probe to win against the ACK also owed — making the zombie the probe
exists to reap immortal, and `Contested` never fire. **[AMENDED
2026/08/16 — ruling 208]** The disarm rule independently defuses that
attack's *engine*, since an injector spoofing a source it does not receive
at never sees the challenge and so cannot echo it, and cannot keep the
address unvalidated for free; the priority rule stands anyway, because an
attacker who *can* keep an address unvalidated must still not be able to
starve the verdict — and ruling 208 widens that residual class rather than
narrowing it, because an on-path relay **can** keep answering. Note also
that the engine's defusal is a claim about this attacker only: the
once-flagged rank-2/rank-4 interaction above was a way the verdict could
be starved with **no attacker at all** — closed by ruling 215 and
resolved into coalescing by ruling 250, not by anything in this
paragraph.

**Why CLOSE outranks even the probe. [RATIFIED 2026/08/16 — ruling
186]** §16.5 states the governing principle for exactly this tie: *"a
terminal outcome precedes a routine one."* A connection that is closing
has no use for the probe's verdict — the probe exists to decide whether to
reap a connection, and one that is leaving has answered that question
already. §7.5 points the same way from the other side: a contested mark
taken on an already-closing connection is a **no-op**, so the two states
barely co-exist, and where a mark taken while live survives into closing
its verdict is moot. A CLOSE the budget will not admit, by contrast, costs
the peer a full `D_eff` to learn what one small packet would have
told it at once. Both are small and both are cwnd-exempt, so the ordering
is free in the common case and decides only the scarce-budget case, which
is what this rule is for.

| Constant | Value |
|---|---|
| `AMPLIFICATION_FACTOR` | 3 (× authenticated, window-fresh bytes received, per unvalidated address, per session) |

### 7.4 The liveness model — `seal` versus `seal_quiet`

The liveness clock is driven by **application intent only**. Two seal
paths, identical on the wire (same sealed-Data packet, same counter
increment):

- **`seal`** marks `last_send`: packets carrying at least one
  first-transmission STREAM frame or DATAGRAM frame (fresh application
  sends), and the keepalive (§7.5).
- **`seal_quiet`** does not touch `last_send` — the **quiet set**: pure
  ACKs, PTO probes, retransmissions, the credit frames (MAX_DATA,
  MAX_STREAM_DATA, MAX_STREAMS_BIDI/UNI), RESET_STREAM, CLOSE, and **the
  contested-connection probe's PING (§7.5)**.

  **[RATIFIED 2026/08/16 — ruling 190] Which half of this rule decides,
  and why the list is not the answer.** The marking side above is a
  **closed characterisation** — application intent — and the quiet side is
  an **enumeration**, which a reader takes as exhaustive whether or not it
  says so. When a new frame appears, those two shapes disagree: it fails
  the characterisation (so it is quiet) and is absent from the list (so it
  is undefined). **The characterisation decides; this list is illustrative
  of the classes that arise, not a definition.** Every send not covered by
  the marking rule is quiet.
  The contested PING is named explicitly because slice 7 introduced it and
  it **must** be quiet: a marking probe would drag `last_send` forward and
  suppress the very passive keepalive whose absence the probe exists to
  diagnose. Found by a blind test author told (ruling 182) to *quote* this
  section rather than re-derive it — who then found the frame it needed
  missing from the list it was told to quote.

Ack-eliciting and liveness-marking remain independent axes: credit frames
are ack-eliciting (they need loss recovery — §8.7) yet do not touch
`last_send`, so they never defer a keepalive. They do arm the death clock
— marking governs *when we send*, arming governs *when we may die*, and
the two sets deliberately differ.

**The liveness anchor is the receive clock, armed by intent or by
ack-eliciting output.** The connection is dead when
`now − last_authenticated_recv >= D_eff`, the connection's effective
dead timeout (§5.7), **[AMENDED 2026/08/15 — ruling 85; was `>`]**
**and** at least one
**arming** send has occurred since that last authenticated receive. A send
arms the death deadline if **either** it is a marking send (a `seal` —
fresh application intent, or the keepalive) **or** it carries any
ack-eliciting frame, whether or not it marks; the two triggers are
independent and either alone suffices. Equivalently: the deadline arms on
the *first* arming send after a receive, is **not** re-armed by subsequent
sends of either kind, and is reset by every authenticated, window-fresh
receive (§7.2). A sender writing into a black hole therefore dies
`D_eff` after its last authenticated receive — 25 s under the
v1/default profile — no matter how often, or how quietly, it writes. The
send clock never defers death; it only enables it.

**At install the clock is pinned, and it is pinned *armed*.** A newly
installed session (§5.4) sets both `last_authenticated_recv` and
`last_send` to the install instant and starts with the death deadline
**already armed**: the handshake is the arming event, so the rule's
second conjunct holds from install onward and no subsequent send is
needed to enable it. Two consequences, both intended. A session that
receives nothing after install dies at install + `D_eff` whether or not
the application ever sends — 25 s under the v1/default profile — which
is what makes "a half-open session is reaped by liveness" a fact rather
than an implementation choice (§6.7, §17.1, and §15.4's
endpoint-dropped row all rest on it), and without the pin an
implementation that started the clock unarmed would hold such a session
**forever**, since §7.6 is deleted and liveness is the only reaper. And
with `last_send` equal to
`last_authenticated_recv`, §7.5's passive rule — *received since its last
marking send* (§7.5, ruling 182) — is false until the first authenticated
receive, so a half-open
session emits nothing at all: it is reaped in silence, and a replayed
initiation never turns us into a keepalive source aimed at a spoofed
address — **unless the application has configured a persistent keepalive
on that connection**, whose beacon is unconditional by design (§7.5) and
therefore does fire into the unvalidated anchor. That case is not a hole
but a smaller guarantee: the beacon's output to an unvalidated address
stays capped by §7.3's anti-amplification budget at 3× the authenticated,
window-fresh bytes received, so the session emits at most 588 B for the
replayed
196 B and then goes quiet until the address validates — §7.3's
**challenge** predicate (rulings 168, 208), which a spoofed source cannot
satisfy because nothing there receives the challenge to echo, so here
"until" means "never" and the 588 B is the whole
budget the case ever gets. §6.9 states the
resulting ratio in full. The claim above is exact for the default
configuration — the beacon is off unless asked for — and this sentence
is what makes it exact rather than merely usually true.

Arming on ack-eliciting output closes the hole the marking-only rule left:
a connection whose output is entirely quiet-set yet ack-eliciting — credit
frames, retransmissions, PTO probes — would otherwise pour unacknowledged
traffic into a black hole with nothing ever arming the clock, and sit
undetected for as long as it kept trying. An unending PTO train still must
not *defer* death: probes now arm the deadline, but no send ever re-arms
it (§13.3). An immediate ACK must not suppress the keepalive dance (a pure
ACK is neither marking nor ack-eliciting, so it neither defers nor arms).
A packet coalescing fresh application frames with control frames marks the
clock (it carries fresh intent) and arms it.

**What the death clock measures — and the one thing it does not.** The
clock is driven by **mere authenticated receipt**, never by acknowledged
progress. Any authenticated, window-fresh packet resets it (§7.2),
including one the peer sealed long ago that an attacker captured,
withheld, and injected later from anywhere off-path: it is genuine, so it
opens; the window never advanced past it, so it is fresh; and it therefore
buys our side's zombie another full `D_eff` — and, because roaming keys
on authenticated receipt too, moves the session to the injector's
address (§7.3). Every "is this peer still there?" question in
this document inherits that: liveness answers *something authentic
arrived*, not *the peer is still there and still talking to us*. The
**contested-connection probe** (§7.5) is the one place the spec
deliberately demands the stronger signal instead — an ACK covering a
counter we sealed *after* the doubt arose, which nothing withheld can
ever forge — and it is asked exactly where mere receipt is known to be
forgeable (§6.4's basis-`None` refusal, §6.8).

**Terminology, since ruling 33 splits what one word used to cover.**
"**Liveness-neutral**" everywhere in this document means **non-marking** —
sealed via `seal_quiet`, leaving `last_send` untouched, so it defers no
keepalive and re-arms no persistent keepalive. It has never meant, and
after ruling 33 emphatically does not mean, "does not arm the death
clock": every ack-eliciting member of the quiet set (credit frames,
retransmissions, PTO probes) is liveness-neutral **and** arming.

### 7.5 Keepalive and liveness timers

| Term | v1/default profile | Effective rule |
|---|---|---|
| passive keepalive / contested verdict | `KEEPALIVE_TIMEOUT` = 10 s | `K_eff` from the endpoint's `TimingProfile` |
| receive-anchored death / liveness backstops | `DEAD_TIMEOUT` = 25 s | `D_eff` from the same profile |
| `PERSISTENT_KEEPALIVE` | 10 s recommended; per-connection `Option<Duration>`, off by default | admissible range **[1 s, `D_eff`)** |

- **The keepalive is the empty plaintext** (§3.4) — the cheapest possible
  liveness beacon, bypassing the frame layer, sealed via `seal`. Its
  classification, explicit: the keepalive is a **marking** send. Passive
  rule: a side that has received since its last **marking** send, and has
  not made a **marking** send for
  `K_eff`, sends a keepalive.
  **[AMENDED 2026/08/16 — ruling 182]** *Both conjuncts read `S` =
  `last_send`, **marking sends only** (§7.4) — this rule previously said
  "has not sent", and wire traces diverge from the first non-marking send
  onward.* The formal definition at ruling 40's derivation below governs,
  and here — unusually for this document — the **formal rule carried the
  intent and the prose carried the bug**. The reason is decisive rather
  than reflexive: the beacon's soundness proof below rests on *"every
  send that can establish `S > R` is a marking send, so the death clock
  is armed there (§7.4)"*. Under the prose reading a **non-marking** send
  blocks the dance — "has not sent" becomes false — **without arming the
  death clock**, and the proof collapses into exactly the immortal
  half-open session SECV5-2 was applied to prevent. When two statements
  conflict, follow the one some other proof depends on. *(Which
  individual sends are marking is **§7.4's** classification, quoted, not
  re-derived here — a PTO probe's class in particular is §7.4's answer.)*
- **Persistent keepalive** is a per-connection opt-in beacon, for NAT
  holding and for mutually idle links. Its trigger is WireGuard's: it
  fires when no marking send has occurred for the configured interval,
  and re-arms from every marking send. `set_persistent_keepalive`
  **rejects an interval below 1 s, and an interval at or above
  `D_eff`**, at the handle: the admissible range is **[1 s, `D_eff`)**.
  **[RATIFIED 2026/08/14, amended 2026/08/14]** The upper bound is a
  **ceiling**, and its job is the one this document has always stated for
  it: to reject an interval so long that the beacon could not keep a
  connection alive on its own. The recommended default is **10 s** —
  `KEEPALIVE_TIMEOUT`, which leaves one-lost-beacon tolerance inside the
  25 s deadline (2 × 10 + 5, the arithmetic that sizes `DEAD_TIMEOUT`
  itself). Under a custom profile, choosing `I = K_eff` is the
  corresponding one-loss choice: rulings 282 and 283 leave the required
  initial-RTT and two-firing shell-lateness margin inside `D_eff`. Ruling
  40 below records why the bound was briefly written as a floor instead,
  and why that is now reversed. The flag's
  other half — the idle-rekey rule, under which the keepalive consulted
  `REKEY_AGE` — is moot: there is no DH rekey to consult (§5.4, the
  ratchet-only ruling). The anchor correction itself
  (§7.4) is a forced fix, not a flagged call.

  **Why the beacon reaches where the passive rule cannot.** The passive
  rule is *conditional*: it fires only when `R > S`, i.e. only in
  response to a receive. It therefore sustains any link that has *entered*
  the loop — one exchange suffices and the loop then feeds itself — but
  it can never start one where neither side has anything to send. The
  beacon is *unconditional*: it fires on its own timer and asks nothing
  of `R`, so it is the one mechanism that sustains a **mutually idle**
  link — one that never entered the dance because neither side ever had
  traffic, exactly the state ruling 39 reaps — and the one mechanism that
  holds a NAT binding open on a cadence the application picks. The two
  compose: a beacon
  arriving at the peer establishes `R > S` there, so the peer's passive
  rule answers within `K_eff`, and that answer resets the
  beaconing side's own `R`. One side opting in is thus enough to keep the
  pair alive: at the 10 s default the pair settles into a 10 s ping-pong
  — the beacon arrives, and the peer answers at once, its own
  `K_eff` having already elapsed — so a single lost beacon
  costs one extra interval and lands the next answer at 20 s, 5 s inside
  the deadline, while two consecutive losses end the connection. That is
  the same one-loss tolerance the dance itself has, and the reason the
  default is `KEEPALIVE_TIMEOUT` rather than anything larger. With a
  custom profile, `I = K_eff` is the equivalent selection. A peer that
  must remain reachable while idle — one behind a NAT that cannot redial
  — sets it.

  **The beacon stays in the marking set**, and no special case is needed
  to make short intervals safe. Arming **enables** death; it never defers
  it (§7.4). A beacon fired into a void arms a deadline it cannot reset,
  so a connection whose entire output is beacons still dies at
  `R + D_eff`, exactly as if it had sent nothing. Marking is
  therefore harmless at every admissible interval, and the alternative
  once recorded here — admitting short intervals while excluding
  persistent keepalives from the marking set — is **unnecessary rather
  than declined**: the ceiling admits short intervals directly, leaving
  the marking set whole and the keepalive a single class.

  **Ruling 40 — the bound was inverted, and ruling 38's own derivation is
  the proof.** Ruling 38 established the arithmetic correctly and drew
  the wrong conclusion from it; the arithmetic is retained here for that
  reason. Let `I` be the configured
  interval, `S` = `last_send` (marking sends only), and `R` =
  `last_authenticated_recv`. The beacon fires at `S + I` and re-arms from
  every marking send; receives do not reset it. Whenever `R > S` the
  passive keepalive above fires at `S + K_eff`, and being itself a
  marking send it drags `S` forward in `K_eff` steps and pushes the
  beacon's deadline along with it — while the dance runs, the beacon
  never fires. The only state that blocks the dance is `S > R`; but every
  send that can establish `S > R` is a marking send, so the death clock
  is armed there (§7.4) and death arrives at `R + D_eff`, while the
  beacon's deadline is `S + I > R + I ≥ R + D_eff` for every
  `I ≥ D_eff` — **strictly after death**. That is a sound proof of a
  narrow fact: *an interval at or above `D_eff` is inert*.
  Ruling 38 read it as a property of the knob and documented the knob as
  a permanent no-op. Ruling 40 reads it as what it is — a proof that the
  **bound** was inverted, because a floor at `D_eff` admits
  precisely and only the intervals the derivation shows can never fire.
  Making the bound a ceiling (`I < D_eff`) breaks that inequality
  exactly where it needs breaking. The beacon is useful precisely when
  `S + I < R + D_eff`; under a floor that is unreachable by
  construction, since `S ≥ R` in the blocking state gives
  `S + I ≥ S + D_eff ≥ R + D_eff` for every admissible
  `I`, with no configuration escaping it. Under a ceiling it is reachable
  — and it holds outright in the state ruling 39 reaps, a connection idle
  from install, where `S = R` at the install instant (§7.4) makes the
  beacon's deadline `R + I < R + D_eff`. There the beacon fires
  while the connection is still alive, the peer's passive rule answers
  it, and the answer resets `R` before `R + D_eff` arrives — so
  the knob does the job it was always described as doing.
  **What moves:** the bound becomes a ceiling, and the recommended
  default becomes 10 s. **What does not:** `DEAD_TIMEOUT` stays 25 s,
  `KEEPALIVE_TIMEOUT` stays 10 s, the beacon stays marking, the knob
  stays per-connection and off by default, and no wire byte changes.

  Ruling 38's two recorded alternatives are updated rather than dropped,
  so neither is re-proposed. Re-basing the bound on `KEEPALIVE_TIMEOUT`
  is **superseded**: the ceiling admits every interval that re-basing was
  reaching for, 10 s included, without a second constant to keep in step.
  Deleting the knob outright **still stands as declined**, and for a
  stronger reason than ruling 38 could give: the knob now does something
  no other mechanism in this document does.

  **Ruling 42 — the beacon needs a floor as well as a ceiling.** Ruling
  40 replaced the floor with a ceiling and, in doing so, removed the
  lower bound altogether: `set_persistent_keepalive(Some(1 ms))` was
  conformant. That is a real defect and not a theoretical one. The
  beacon is exempt from the congestion window (§14.5), so a
  millisecond interval is an unthrottled 1000 packet/s emitter that no
  gate slows; §13.3 already condemns a 20 packet/s cadence as defeating
  §16.5's timer economy, and this is fifty times that. It also
  **suppresses RTT sampling**: §13.1 takes a sample only when `largest`
  is newly acked, and beacons are not ack-eliciting and never enter the
  sent map, so a connection whose entire output is beacons feeds the
  estimator nothing while filling the path. The admissible range is
  therefore **[1 s, `D_eff`)** — reject below 1 s, reject at or above
  the effective dead timeout (25 s under the v1/default profile), and
  retain 10 s as the v1 recommended value. **Why 1 s and not more:** the
  floor's job is to foreclose the degenerate configurations, not to
  second-guess an application that knows its NAT. A 1 s beacon is one
  packet per second per connection, a rate any of this document's other
  mechanisms can already reach; below that the knob stops being a
  liveness beacon and becomes a load generator. **What moves:** only the
  lower bound. `DEAD_TIMEOUT` stays 25 s, `KEEPALIVE_TIMEOUT` stays
  10 s, the default stays 10 s, the beacon stays marking, and no wire
  byte changes.

  **What the admissible band does and does not promise.** The range is
  wide on purpose, but only the lower part of it carries the tolerance
  the default advertises. A beacon at `I` must land, be answered, and
  have the answer arrive before `R + D_eff`; §16.5's lateness
  bound `L` (250 ms) applies independently to both successive beacon
  timers, and the answer costs a round trip. At `I = K_eff`, one lost
  beacon leaves the next one firing no later than `2 × K_eff + 2 × L`;
  the profile's strict validation relation leaves more than
  `K_INITIAL_RTT` after that second firing before death at `D_eff`.
  Equality is
  therefore the intended one-loss case — under the v1/default profile,
  10 s then 20 s, with 5 s before the 25 s verdict. At `I > K_eff`, the
  full default one-loss margin no longer follows: near the top of the
  band (within `L` plus one RTT of `D_eff`) even an unlost beacon cannot
  complete in time. Those intervals remain **admissible and are not
  rejected** — the handle enforces a range, not a loss model, and an
  application choosing a larger interval on a lossless link gets exactly
  what it asked for — but it must not infer the profile's one-loss
  guarantee. This is why the recommendation follows `K_eff`, not the
  largest admissible value.
- **Neither keepalive's deadline is announced while a keepalive cannot
  leave — and a vetoed keepalive arms the death clock.** **[RATIFIED
  2026/08/18 — ruling 265]** `Keepalive` and `PersistentKeepalive`
  (§16.5) are armed only while §7.3's budget admits the 30-byte empty
  plaintext **and** no contested mark is pending — rank 2 outranks rank
  7 (§7.3), so a keepalive may not spend budget the probe is waiting
  for. Both deadlines are functions of `last_send`, and a held keepalive
  is held without moving `last_send`, so arming from it regardless puts
  the deadline at an instant already passed: the shell's `sleep_until`
  returns immediately and the one `!Send` driver every connection
  shares spins (§16.3). The gate is on the announcement, not the state:
  `last_send`, the passive rule's debt and §7.4's arming bit are
  untouched; both holds lift only on an authenticated, window-fresh
  receive, which recomputes both deadlines — no dedicated re-arm
  machinery exists, and no future instant is predictable while the hold
  stands, so arming nothing is right and arming later is wrong.

  **`Liveness` is not the backstop here — measured, not argued.** §7.4
  disarms the death clock at the very receive that sets the passive
  rule's debt, and §7.3's rank-5 pure ACKs — cwnd-exempt, sized to the
  whole remaining room — can spend the refunded budget before anything
  arms, so the suppressed state can otherwise reach `Timeout(None)` with
  a keepalive owed and every timer dark: a connection that neither talks
  nor dies. The rule that closes it: **while a keepalive is owed and
  vetoed, the connection announces the representable
  `last_authenticated_recv + D_eff`** — the death clock's own anchor, so
  the parked state ends in death at the effective timeout (25 s under the
  v1/default profile) or in recovery at the first qualifying receive,
  which re-funds the budget, resumes the keepalive, and moves death out by
  its own rule. If the sum lies beyond the platform clock horizon, the
  death obligation remains logically enabled but §16.5 permits no
  fabricated earlier announcement.
- **Liveness** (`D_eff`) keys on the receive clock (§7.4): a
  connection that has *armed* the clock since its last authenticated
  receive — by a marking send **or** by any ack-eliciting send — and then
  receives nothing authenticated for `D_eff` is dead
  (`ConnectionLost::TimedOut`). It is **the only idle killer** — an idle
  session sustained by the keepalive dance keeps receiving, so it lives
  indefinitely *while the dance survives the path*. The residual is named,
  not hidden — and it is **narrower than "two consecutive losses"**.
  Under the v1/default profile, 25 s = 2 × `KEEPALIVE_TIMEOUT` + 5 s
  grace, and the tolerance is one lost keepalive **in one direction**:
  if A's keepalive at t = 10 is lost but B's arrives, A has received
  since its last marking send, keepalives again at t = 20, and lands
  inside B's deadline with 5 s to spare. A
  **simultaneous bidirectional** loss — one loss *event*, two packets, the
  same interval — is not tolerated at all: both sides then hold
  `last_send` > `last_authenticated_recv`, so the passive rule's first
  conjunct is false on both; the rule is one-shot per receive and
  keepalives are never retransmitted (§8.7 files them in the *never*
  class and they never enter the sent map); neither side ever sends again,
  and both die at t = 25. A brief **bidirectional** outage spanning a
  single keepalive interval — a flapping path, a NAT rebind, an interface
  hiccup — therefore ends an otherwise healthy idle connection, and
  slither never reconnects on its own — re-establishment is the
  application's, by a fresh `connect()` (§16.2). Applications that need an
  idle session to survive a lossier path should carry their own reconnect,
  not expect a longer timeout.

  **Ruling 39 — the dance's scope, and what the idle-from-install drop
  costs.** The dance is **automatic for any connection that has carried
  traffic**, and the document states that here rather than leaving it to
  be derived. One exchange in either direction leaves the receiving side
  with `R > S`, which is the passive rule's entry condition; the
  keepalive it then sends puts the far side into that state in turn, and
  the loop is self-sustaining from there. A sparse request/response
  application — one request a minute, nothing in between — therefore
  stays connected with no opt-in at all, needing neither the beacon nor
  an application-level ping. The beacon exists for the cases the dance
  does not reach; it is not a general requirement.

  There is exactly one such case, and its precise statement is **a
  connection with no authenticated receive since install** — not "a
  connection that has sent nothing". With `last_send` pinned equal to
  `last_authenticated_recv` at the install instant and the deadline armed
  there (§7.4), the entry condition is false from the start and stays
  false, so the connection dies at install + `D_eff` in silence. That is
  ruled as intended — a connection that never carries traffic is reaped
  at the effective timeout and the application redials — and it is what
  makes "a half-open session is reaped by liveness" a fact (§6.7,
  §17.1, §15.4). The v1/default value is 25 s.

  **Stating it as a receive rule is not pedantry; two consequences follow
  that a send rule would hide.** First, **a late first send does not
  save the connection**. A dialling side whose application sends its
  first request one second before `D_eff` still has
  `last_authenticated_recv` at the install instant, so it dies at
  `D_eff` unless the peer's answer completes inside that second; if that
  first request is lost, the sender gets at most the recovery time that
  remains and then dies with data still queued. The usable window for a
  first exchange **shrinks as the connection ages**, from `D_eff` at
  install down to one round trip.
  Second, the flows this reaps are **ordinary, not degenerate**:
  connect-ahead-of-use (dial at process start to hide handshake latency
  from the first user action), human-in-the-loop (dial, then wait for an
  operator to type), and responder-first-silence (a server with nothing
  to say until asked, whose client stalls past `D_eff`). In each case both
  sides emit nothing at all — the reap is silent by §7.4's own design —
  so `ConnectionLost::TimedOut` is indistinguishable from a real path
  failure, which is precisely the complaint this section levels at the
  declined all-opt-in alternative below, and it is fair against the
  retained design too for the first `D_eff` of a connection's life.

  **The application rule that follows, stated normatively.** An
  application that establishes a connection ahead of its first use MUST
  either carry an exchange within `D_eff` of install or configure
  a persistent keepalive on it (§7.5's beacon, §16.2). Those are the two
  mechanisms; there is no third, and no protocol default rescues a
  connection that uses neither. This is the one place where slither's
  "no built-in reconnect" posture reaches an application that has done
  nothing wrong, so the requirement is spelled out rather than derived.

  **The cost of that drop, stated where the ruling is.** After the death,
  only a side that can still *reach* the other can restart the
  connection: the timing-out side surfaces `ConnectionLost::TimedOut`
  (§15.4), slither never reconnects on its own, and re-establishment is a
  fresh `connect()` by whichever side has a route (§16.2). For a peer
  behind a NAT the binding dies with the connection and inbound
  initiations stop arriving, so that peer must either be the **dialler**
  on every re-establishment or hold its binding open with the persistent
  beacon above — which is precisely the case that beacon exists for
  (ruling 40). A peer that is neither the dialler nor beaconing is
  unreachable until it dials.

  The alternative — **make all keepalive opt-in**, deleting the automatic
  dance so that only a configured beacon sustains a connection — was
  **considered and declined**, recorded so it is not re-proposed. It
  silently breaks every sparse-traffic application that does not opt in:
  a request/response peer with a 60 s idle gap works under this
  specification under the v1/default profile and would begin dying at
  25 s instead, with nothing on the wire distinguishing that from a real
  path failure and no diagnosis available to the application beyond
  reconnecting harder.
- Keepalives are admitted to the replay window (they appear
  opportunistically in ACK ranges; `ack_delay = 0` when the window's
  largest was not frame-seen — §12.3) but never reach recovery, never
  count as ack-eliciting, and never enter the congestion window (§14.5).
- **PING** is not a keepalive: it is the ack-eliciting PTO probe (§13.4)
  and the carrier of the contested-connection probe below, sealed via
  `seal_quiet` — liveness-neutral, i.e. non-marking; being
  ack-eliciting it still *arms* the death clock (§7.4, §13.3). Two
  signals, two masters. **[ruling 221]** It is not the *only* thing a PTO
  can carry: on an unvalidated address §13.4's probe carries
  `PATH_CHALLENGE` instead, which is ack-eliciting in its own right, so no
  PING is owed beside it.
- **The contested-connection probe** (ruling 36). When §6.4 refuses an
  `accept()` because the live connection's `replacement_basis` is `None`,
  that connection is marked **contested**. Marking does three things, in
  order: it records
  the mark's **probe floor** — the counter the next seal on that
  connection will use, `DatagramSend::next_counter()` (Appendix A.2) — it
  sends an ack-eliciting **PING** (§8.3) — ordinarily the first packet at
  or above that floor, though the floor is what binds and it is recorded
  at the mark whether or not the PING is the very next seal — and it arms
  a `K_eff` deadline **at that PING's transmission**. The mark clears on
  **any ACK covering any counter at or above the probe floor** — the
  probe's own counter, or any later one. If such an ACK arrives before
  the deadline the mark clears
  and nothing else happens — the refusal stands and the basis rule is
  untouched; a genuinely live peer simply answered, and it does not
  matter *which* post-mark packet it answered. If none arrives by the
  deadline the connection dies with `ConnectionLost::TimedOut` (§15.4's
  contested row — no new variant, no wire signal, nothing transmitted),
  its static drops to NONE, and the parked `Intro` takes an ordinary
  fresh `accept()` on the application's next attempt.

  **One mark per connection.** A connection is contested or it is not:
  the state retains one `(probe_floor, deadline)`, never a set. The
  deadline may be unreachable at the platform clock horizon (§16.5), but
  the mark and its floor remain armed. A
  refusal that lands while the connection is **already** contested is
  **not** a second mark — it leaves the existing floor and the existing
  deadline exactly where they are, and sends no second PING. It does
  **not** re-arm the deadline, and that is a security property, not an
  optimisation: re-arming on each refusal would hand the attacker — who
  supplies the Intros that cause refusals (§6.3) — a way to postpone the
  verdict indefinitely by dripping one captured initiation in just under
  every `K_eff`, which is precisely the zombie the probe
  exists to reap. The verdict lands on the clock set by the *first*
  refusal, whatever arrives after it.

  **Ruling 41 — the predicate is a counter high-water mark, not a packet
  identity.** The clause this replaces required "an ACK covering the
  packet that carried that PING", and that predicate is unsatisfiable
  whenever the probe packet itself is lost: §8.7 files PING in the
  **never**-retransmit class — a lost PING is superseded by the next
  probe — so the peer never receives that counter, its replay window
  carries a permanent gap there, and every ACK it derives from that
  window (§12.2) carries the gap too. A fully live peer answering every
  PTO retry would still be killed at the deadline, falsifying this
  section's own claim that a genuinely live peer simply answered. Under
  the high-water mark the PTO retries **rescue** the connection instead
  of being irrelevant to it: the retry is sealed at a counter above the
  floor (every subsequent seal is), so the ACK that covers the retry
  clears the mark. Any post-mark traffic does — an application Data
  packet, a §13.4 probe train, a retransmission — which is the correct
  behaviour, because what the mark asks is not "did you hear this packet"
  but "have you acknowledged anything I sealed after the harvest".

  **Why an ACK rather than a receive.** The refusal exists because a
  `None` basis cannot tell a genuine reconnect from a replayed initiation
  (§6.4, §17.4). §7.4's mere-receipt weakness lets an attacker who
  harvested genuine peer→us Data — dropping it, so our replay window never
  advances past it — keep the resulting zombie's receive clock alive
  indefinitely by injecting one harvested packet every less than
  `D_eff` from anywhere off-path, roaming the session to itself as
  it goes; meanwhile every genuine reconnect is refused and `connect()`
  reports `ConnectError::AlreadyConnected` (§16.1), so without this probe
  the pair is wedged permanently and the only escape is to `close()` a
  connection that looks healthy from every protocol-visible angle.
  Withheld genuine Data can reset a receive clock; it can **never**
  produce an ACK covering the probe floor. Every counter at or above the
  floor is one this endpoint seals *after* the mark — therefore after the
  harvest — and an ACK's coverage is derived from the peer's replay
  window (§12.2), which cannot contain a counter the peer never received.
  A peer cannot acknowledge a packet we never sent, and the attacker
  holds only packets we sent *before* the floor. That is why widening the
  predicate from one packet to a high-water mark costs the attacker
  nothing it did not already lack: it is the *floor*, not the identity of
  a single carrier, that does the security work. This is the one liveness
  question in the document that demands acknowledged progress instead of
  authenticated receipt (§7.4), and it is asked exactly where mere
  receipt is known to be forgeable.

  **Why the old predicate was a security defect and not merely a
  liveness one.** §6.4's timestamp guard is empty for every peer we only
  ever dial, so every candidate initiation passes it vacuously however
  old, and §6.4's ordering clause reverts the guard record on
  `AcceptError::Stale`. One passively captured msg1 is therefore
  replayable against us forever, and each replay was an independent trial
  at the forward loss rate: lose the one probe packet and the connection
  died even though the peer was answering. At a 0.5 % forward loss rate
  a thousand replays kill with near certainty, which **inverts** the very
  property §6.4 and §17.4 claim for a `None` basis — that a captured msg1
  cannot destroy a connection we dialled. The high-water mark restores
  it: an attacker's replay supply buys refusals, and refusals now buy at
  most one collapsed mark whose verdict any post-mark ACK clears.

  **Two alternatives, declined, recorded so neither is re-proposed.**
  (1) **Make PING retransmittable.** This would satisfy the old predicate
  by ensuring the carrier eventually lands, but it changes §8.7's
  retransmission semantics for a frame the §13.4 probe trains also use —
  a PTO probe whose whole design is that loss is absorbed by the *next*
  probe would acquire a retransmission identity and a sent-map lifetime
  it does not want. A liveness question must not reshape the loss
  recovery machinery. (2) **Clear the mark on any post-mark ACK,**
  without a floor. This is simpler to state and wrong: an ACK already in
  flight when the mark is taken was minted by the peer *before* the
  harvest window closed, so it proves nothing about the present, and
  accepting it would weaken the after-the-harvest proof to an
  after-the-mark-arrival coincidence. The floor is what makes the proof
  a statement about counters rather than about arrival order.

  **Cost, and the honest bound (rulings 43 and 175).** Two drafts have
  bounded this probe and both were wrong. The first bounded it by "the
  application's own accept rate and nothing an attacker controls";
  ruling 43 corrected the false half — refusals are provoked by
  **Intros**, and an attacker supplies Intros, which is what the queue in
  §6.3 exists for — and replaced it with *"at most one packet per
  `KEEPALIVE_TIMEOUT` per connection … a bound the attacker cannot
  move."* **[AMENDED 2026/08/16 — ruling 175]** *That replacement is also
  false, and by a factor of about a thousand.* The collapse rule
  suppresses only refusals landing **while the mark is outstanding**, and
  on a **live** connection the peer ACKs in about one RTT, which clears
  the mark; the next refusal then lands uncontested and is a full second
  mark, with its own PING and its own fresh floor. The real rate is
  `min(refusal rate, 1/RTT)` — on a 10 ms LAN path up to ~100 probes/s,
  not 0.1/s. Both texts said the same thing and both were wrong the same
  way, which is why review passed it: not a conflict between two
  statements, but one unstated scope agreed upon in two places.

  The bound, stated honestly: **at most one probe per mark, at most one
  mark per uncontested refusal, and marks cannot overlap** — one floor
  and one deadline per connection, never a list (§17.5). The refusal rate
  is the application's own `accept()` rate; the **Intro** supply that
  provokes those refusals is the attacker's, and no sentence here should
  be read as claiming otherwise. The endpoint-wide total is the
  per-connection rate times the number of live connections the
  application holds.

  **No cooldown is added**, and the omission is deliberate rather than an
  oversight: a cooldown would leave a genuine second doubt unprobed for
  its duration, trading a cost bound for a security hole. The security
  half is untouched either way, and this is what makes the honest bound
  affordable — **every re-mark records a fresh floor**, so each probe
  still demands acknowledged progress *after* the doubt that raised it.
  A connection answering 100 probes a second is a connection answering,
  which is precisely the verdict the probe asks for. This is a **cost**
  defect, not a security one.

  **Congestion: exempt from the gate, counted in the map.** The probe is
  admitted regardless of the congestion window (§14.5) — a probe the gate
  could delay past its own deadline would silently convert congestion
  into a liveness verdict, so that exemption is correct and must not be
  removed. The exemption is from **admission**, never from
  **accounting**: the probe is ack-eliciting, so §13.5 inserts it into
  the sent-packet map like any other ack-eliciting packet and its size
  counts in `bytes_in_flight`. This is what keeps §17.5's memory bound
  honest, and it is also what makes the mark clearable by an ordinary
  ACK — an untracked packet would be a packet loss recovery could not
  reason about. Like every other exempt class the probe remains bound by
  §7.3's anti-amplification budget at an unvalidated address.

  **When the probe cannot be sent, and when it must not be.** The
  deadline is armed at the probe's **transmission**, not at the mark, so
  a probe that §7.3's budget will not yet admit leaves the mark
  **pending** rather than failed — the endpoint sends it, **unless the
  mark has already cleared**, and arms, at
  the first instant the budget allows. A connection may not be killed by
  a question that was never asked. That same instant is when the
  application-visible `Contested` signal fires (§16.4, §16.2, ruling 46):
  the mark-pending gap emits nothing, so the signal never announces a
  countdown that is not yet running. A contested mark taken on a connection
  already closing or draining (§15.2) is a **no-op**: that connection is
  already leaving, and the parked `Intro` will meet no live static —
  §6.4 carries the same carve-out at the point the mark is *taken*
  (ruling 179).

  **The pending mark's other two exits.** **[RATIFIED 2026/08/16 —
  ruling 176]** The mark-pending state had one stated entry and one
  stated exit; two more are reachable and are stated now.

  - **An ACK covering the floor arrives while the mark is still
    pending.** This is ordinary, not exotic: the floor is *the counter
    the next seal will use*, so **any** post-mark seal — a keepalive, a
    retransmission, a pure ACK, an application Data packet — lands at or
    above it, and the peer's ACK of that seal clears the mark before the
    probe was ever admitted. **Clearing a pending mark cancels the
    pending probe and emits nothing.** The *"mark-pending gap emits
    nothing"* principle §16.4 already states for the gap governs its
    exit too. Both halves matter: on the literal earlier text
    `ContestCleared` fired *"when the mark clears"* unconditionally, so
    it could fire with **no preceding `Contested`** — the unmatched
    notification that ruling 46 deleted `under_probe: bool` to prevent
    (§16.4 now states the matching rule) — and the send rule *"the
    endpoint sends it, and arms, at the first instant the budget
    allows"* carried no condition, so a **stray probe** would go out and
    arm a `K_eff` verdict deadline for a mark that no longer exists. That
    deadline would then be uncancellable by §16.5's disarm
    rule, which disarms on an ACK covering a floor that has *already*
    been satisfied.
  - **The connection roams again while the mark is still pending.** The
    pending mark is left **intact with its floor unchanged**: the
    counter space is never reset (§7.7), so the floor stays meaningful
    across the seam, and the roam changes only the probe's budget
    prospects, not the question it asks. §13.6 lists it among the roam
    seam's per-connection outcomes for exactly this reason (ruling 173).

  Under the v1/default profile the probe's 10 s `K_eff` deadline is
  shorter than `INTRO_TTL` (§6.3), and the peer's retransmit train
  re-mints an initiation every ≈ 5 s (§5.5), so the `Intro` that provoked
  the probe is still parked — or has been refreshed by a newer one —
  when the verdict lands. A custom profile does not stretch either fixed
  admission timer: if its `K_eff` outlives `INTRO_TTL` or the peer's
  fixed handshake train, the parked candidate may be gone when the
  verdict lands. That is an intentional consequence of keeping
  admission and handshake timing outside `TimingProfile`, not an implied
  extension of either timer.

### 7.6 [deleted 2026/08/14 — the ratchet-only ruling]

The periodic DH re-handshake this section specified — `REKEY_AGE`
(120 s), `REJECT_AGE` (180 s), the `NeedsRekey` signal, the silent swap,
the instant swap-cut, and make-before-break demux — is **deleted**: the
§7.7 epoch ratchet is the only rekey, and a new handshake from an
already-live static is a connection replacement via `accept()` (§5.4,
§6.4). The swap-cut's maintainer flag is moot with the section. The
section number is retained to keep §7's numbering stable.

### 7.7 The epoch ratchet

| Constant | Value |
|---|---|
| `REKEY_EPOCH_MSGS` | 65 536 (2¹⁶) messages per epoch |
| `MAX_EPOCH_JUMP` | 2 (hiss-fixed) |

Transport keys ratchet forward on a counter-derived schedule
(`into_datagram_with_epoch`), retiring ageing key material by rotation. A
message sealed at `counter` belongs to epoch `counter / REKEY_EPOCH_MSGS`;
each direction ratchets independently; the counter is **never reset** by
the ratchet; `2⁶⁴ − 1` is reserved for the `Rekey()` transform. Epoch `e`'s
key is Noise §11.3 `Rekey()` applied `e` times:
`Rekey(k) = ENCRYPT(k, 2⁶⁴ − 1, empty, zeros[32])[0..32]`. The
ChaCha20-Poly1305 vector, pinned by a test-only computation in slither's
own suite **[AMENDED 2026/08/17 — ruling 251]** (via `cryptoxide`, the
golden-wire philosophy: a fixed vector over constants is not session
cryptography — and a both-sides-hiss boundary test cannot pin it, because
a wrong `Rekey()` agrees with itself):
`REKEY(0³²) = 25ce5d37df19f3783185f2ffd5ab17fa3397c212f02d62fb1733e0b875b74c58`.
The receiver retains the current and immediately preceding epoch keys
(straggler tolerance: one epoch back); anything older is refused, its key
ratcheted away. Retention is not reach **[AMENDED 2026/08/18 — ruling
256]**: the retained key opens **any** counter of the preceding epoch,
but §7.2 binds after it — a packet more than `REPLAY_WINDOW` counters
behind the greatest authenticated counter is dropped post-AEAD without
delivery. The previous-epoch key therefore delivers only while the
receiver's greatest is within `REPLAY_WINDOW` counters of the boundary:
2048 of `REKEY_EPOCH_MSGS`' 65 536, one part in 32. The ratchet is
**forward rotation only, not healing**:
post-compromise healing within a connection **does not exist** — an
exfiltrated session key decrypts its direction until the application
reconnects, and healing is application reconnect policy (the TLS 1.3
KeyUpdate model; WireGuard's contrary choice — a periodic DH
re-handshake — is noted and overridden by the ratchet-only ruling, §5.4).

**Epoch death is subsumed by liveness.** A peer more than two epochs ahead
is permanently unopenable (refused without key derivation — a generic
decryption failure at the hiss surface). No dedicated detection or
recovery path exists or may be added — and the reason is congestion
control, not arithmetic. The condition is not silence but **unopened**
messages: the receiver's committed epoch advances only on messages that
open, so the drift needs a sender whose forward direction is black-holed
while the reverse direction keeps both sides alive, pushing ≥ 196 608
unopened seals (`MAX_EPOCH_JUMP` × `REKEY_EPOCH_MSGS`, plus the epoch in
progress) ahead of the receiver's commit. Elapsed time alone does not
exclude that, and `DEAD_TIMEOUT`'s rise to 25 s widens the window rather
than narrowing it: 196 608 seals in 25 s is ≈ 7.9 k packets/second, ≈ 75
Mbps at `MAX_DATAGRAM` — an ordinary LAN rate, not an orders-of-magnitude
gap. What actually excludes it is the ACK-driven admission gate: with the
forward direction black-holed no ACKs arrive, so cwnd collapses to
`MINIMUM_WINDOW` and §14.5 stops the sender within about one round trip.
The drift cannot accumulate. And if the condition somehow arises anyway,
liveness closes it **unconditionally** at the longer timeout: nothing
opens, `last_authenticated_recv` stops advancing, and the traffic
producing the drift is ack-eliciting by construction, so the death clock
is armed whether or not that sender ever marked (§7.4) — the session dies
at its effective dead timeout with no epoch-specific machinery.
**Implementations must not chase epochs.**

**The epoch size is config-supplied for tests, `REKEY_EPOCH_MSGS`
otherwise.** **[RATIFIED 2026/08/15 — ruling 82]** The boundary is
otherwise unreachable in a test: it takes 65 536 seals to cross, and the
counter setter that would shortcut it is `#[cfg(test)]` **inside hiss**,
so no consumer can reach it. A configurable epoch therefore pins the
boundary *behaviour* and a separate constant test pins the *value* —
independently, which is the stronger arrangement, since a single test
crossing a real boundary would pass just as well against a wrong
constant. The schedule is security-relevant (it bounds how much traffic
one key seals), so this carries **§16.6's test-only rule verbatim**: a
production endpoint uses `REKEY_EPOCH_MSGS`, and a build that accepts a
caller-chosen epoch must be feature-gated or documented as such.

### 7.8 One session per connection — nothing survives a handshake

A connection has **exactly one session** for its whole life (§5.4). No
transport state ever crosses a handshake: a completed handshake installs
a **new connection** — fresh cipher states, replay window, counters,
streams, flow-control ledgers, recovery and congestion state, liveness
clocks, and indices — and the connection it replaces (if any) dies whole
at the replacing `accept()` (`ConnectionLost::Replaced`, §6.4) or at
liveness. There is no survival matrix, no re-queue rule, and no rekey
seam: session-scoped and connection-scoped are the same scope. Data
un-ACKed at a replacement is lost with the old connection and is the
application's to re-send on the new one — exactly the reconnect
semantics of every fresh `connect()`.

### 7.9 Nonce exhaustion

Sealing at counter `2⁶⁴ − 1` is refused by hiss (§2.1). A seal failure is
never silent and can never strand frames (plan-seal-commit, §16.7): it
moves the connection to `ConnectionLost::NonceExhausted` — **connection
death, with no rekey escape** (there is no DH re-handshake to mint a
fresh counter space — §5.4). The bound is unreachable in practice: the
usable space is `0 ..= 2⁶⁴ − 2` ≈ 1.8 × 10¹⁹ seals per direction — at a
sustained 10⁷ packets/s that is ≈ 1.8 × 10¹² seconds, over 58 000
years — and the epoch ratchet bounds per-key AEAD volume long before
counts matter (65 536 messages × ≤ 1 186 B ≈ 78 MB per epoch key, §7.7).
The wire's varints additionally cap ACK-referenced counters at 2⁶² − 1
(§8.1) — ≈ 4.6 × 10¹⁸ packets, still over 14 000 years at 10⁷ packets/s;
both bounds are stated for completeness, neither is reachable at any
physical send rate.

## 8. The frame layer

### 8.1 Varint encoding

All frame-body integer fields — the frame type byte included — are QUIC
variable-length integers, byte-identical to RFC 9000 §16. The top two bits
of the first byte select the total length; the remaining bits, big-endian
across the encoding, are the value:

| Prefix | Length | Usable bits | Maximum value |
|---|---|---|---|
| `00` | 1 byte | 6 | 63 |
| `01` | 2 bytes | 14 | 16 383 |
| `10` | 4 bytes | 30 | 1 073 741 823 |
| `11` | 8 bytes | 62 | 2⁶² − 1 (4 611 686 018 427 387 903) |

A sender emits the minimal encoding; a receiver accepts any length (a
non-minimal encoding is valid, as in QUIC). The cleartext packet header is
**not** varint — fixed widths there (u32 index, u64 counter) keep
classification and AD construction trivial. Stated consequence: varints
cap at 2⁶² − 1, so ACK `largest` (§12.1), stream offsets and final
sizes (§9.5), **and the CLOSE `error_code` (§15.1)** cap there too. A
connection would need > 4.6 × 10¹⁸ packets
to reach the ACK bound — over 14 000 years at 10⁷ packets/s (§7.9) —
unreachable, but the bound is explicit.

**The CLOSE code is the one entry an application controls**, and it is
the one the list omitted. **[AMENDED 2026/08/15 — ruling 86]** §16.2's
`close(code: u64, reason)` accepts a `u64`, so a caller can hand it a
value no varint encodes. It is **capped at 2⁶² − 1 where `reason` is
truncated** — at the producing side, per §8.4's rule that "an
implementation must not be able to *produce* the over-length case it
must kill on receipt" — and not refused: `close()` stays infallible,
because §15.2's teardown is a path an application must be able to take
unconditionally. No application code in §15.3's registry is within
10¹⁷ of the cap, so nothing legitimate is reshaped by it.

### 8.2 The frame stream: parse-then-apply

A sealed Data packet's plaintext is a concatenation of frames, parsed to
the end of the plaintext (the AEAD gives the exact length; there is no
packet-level length prefix). An empty plaintext is the keepalive and never
reaches this layer (§7.5).

**Parse the whole plaintext first, then apply.** Two failure classes,
strictly distinguished:

- **Structural failure** — an unknown frame type, a truncated frame, a
  varint overrunning the plaintext, a length field overrunning the
  plaintext, a non-final extends-to-end frame (§8.4), or any per-frame
  structural error case below — is a **signalled death**. **[RATIFIED 2026/08/14]**
  Nothing from the packet is applied (no ACK scheduling, no state change
  beyond the already-performed replay mark), one trace fires on
  `slither::frames`, and the connection emits CLOSE with
  `PROTOCOL_VIOLATION` (the existing `0x01`, §15.3) and enters the
  closing state (§15.2), surfacing
  `ConnectionLost::ProtocolViolation { code }` (§18.1).
  **[AMENDED 2026/08/19 — ruling 273]** This consequence is scoped to a
  **live** connection. While closing or draining, a structural failure is
  ignored entirely — no second CLOSE, no additional trace, no further
  event; the packet's arrival is answered, if at all, only by §15.2's
  rate-capped linger reply, byte-identical to a benign packet's. Two
  facts force the scope: §15.2's retention list is exhaustive and keeps
  no frame-apply machinery, and the semantic violation class is
  unreachable while closing by construction, since post-mortem
  processing applies no frame. A corollary the scope buys: **at most one
  structural trace fires per connection, ever** — which is the answer to
  "one trace per what?". The reasoning is
  population, not tidiness: after the AEAD tag verifies, corruption is
  excluded (2⁻¹²⁸) and version skew is excluded by design (one version,
  no negotiation — §1.1), so a structurally invalid frame stream is a
  peer bug or a deliberate violation — the same population the semantic
  class below already closes on. The silent-drop alternative leaves a
  buggy peer retransmitting its malformed frame for ever, both liveness
  clocks fresh (the packets *are* received and window-marked), with no
  error and no operator signal — an unbounded livelock. The silent drop
  survives **only** pre-AEAD, at §3.1's length/type/version gate, where
  corruption is genuinely possible. This is a behaviour change with no
  wire change (the code already exists in the registry); the flagged call
  is confirming the population.
- **Semantic violation** — a structurally valid frame whose application
  would break protocol state (a flow-control breach §10.5, a stream-limit
  breach §10.5, a stream-state error, a final-size violation §9.5) — is a
  protocol violation by an authenticated peer: the connection emits CLOSE
  with the matching error code and enters the closing state (§15.2).

### 8.3 The frame table

QUIC's type numbers are reused verbatim where the concept is shared; the
gaps are harmless (the type is a varint).

| Type | Frame | Fields (all varints) | Ack-eliciting | Retransmission | Home |
|---|---|---|---|---|---|
| `0x00` | PADDING | — | no | never | §8.4 |
| `0x01` | PING | — | yes | never | §13.4 |
| `0x02` | ACK | largest, ack_delay, range_count, first_range, (gap, range)* | no | never | §12 |
| `0x04` | RESET_STREAM | stream_id, error_code, final_size | yes | regenerate | §9.6 |
| `0x05` | (reserved: STOP_SENDING) | — | — | — | §19 |
| `0x08`–`0x0f` | STREAM | stream_id, [offset], [length], data; OFF = 0x04, LEN = 0x02, FIN = 0x01 | yes | ranges | §9.5 |
| `0x10` | MAX_DATA | max | yes | regenerate | §10.3 |
| `0x11` | MAX_STREAM_DATA | stream_id, max | yes | regenerate | §10.3 |
| `0x12` | MAX_STREAMS_BIDI | max (cumulative) | yes | regenerate | §10.4 |
| `0x13` | MAX_STREAMS_UNI | max (cumulative) | yes | regenerate | §10.4 |
| `0x1a` | PATH_CHALLENGE | data (8 opaque bytes, **not** a varint) | yes | never — see §8.7 | §7.3 |
| `0x1b` | PATH_RESPONSE | data (8 opaque bytes, **not** a varint) | yes | never — see §8.7 | §7.3 |
| `0x1c` | CLOSE | error_code, reason_len, reason | no | linger rule (§15.2) | §15 |
| `0x30`/`0x31` | DATAGRAM | [length (0x31 only)], data | yes | never | §11 |

**[RATIFIED 2026/08/16 — ruling 208]** `0x1a`/`0x1b` are QUIC's own code
points for these two frames, adopted verbatim so a reader who knows QUIC
needs no lookup, and they were unused in slither. They are the **only**
addition ruling 208 makes to this wire: no existing type code, field
order or packet layout moves, and every golden vector stays byte-identical
(§1.1, ruling 210(d)). Their payload is eight **opaque** bytes — never
re-encoded, never interpreted, compared for equality and nothing else.
That is consistent with §8.1 rather than an exception to it: §8.1 governs
*integer* fields, and an opaque byte string is what STREAM's `data` and
CLOSE's `reason` already are. Encoding the challenge as a varint would be
a defect and not a style choice, because §8.1 admits **non-minimal
encodings**, so one challenge value would have several valid encodings and
"the responder returns exactly what it received" — the whole security
property — would stop being checkable by comparing bytes.

`0x05` is *reserved*, not implemented: like any unknown type, receiving it
is a structural failure — CLOSE with `PROTOCOL_VIOLATION` (§8.2). The
retransmission classes are §8.7's.

### 8.4 Frame layouts and error cases

**PADDING (`0x00`)** — a single `0x00` byte, no fields; any number may
appear anywhere. Not ack-eliciting, never retransmitted, no error cases.

**PING (`0x01`)** — the type byte alone. Ack-eliciting; never
retransmitted (a lost PING is superseded by the next probe). No error
cases.

**ACK (`0x02`)**

```
type(0x02) ‖ largest(varint) ‖ ack_delay(varint, µs)
           ‖ range_count(varint) ‖ first_range(varint)
           ‖ range_count × [ gap(varint) ‖ range(varint) ]
```

Semantics and policy in §12. Structural errors (§8.2's structural class):
`range_count` > `MAX_ACK_RANGES` (64); any range descending below counter
zero. Semantic no-op (frame ignored whole, traced): `largest` above the
highest counter this session has sealed (§12.5).

**RESET_STREAM (`0x04`)**

```
type(0x04) ‖ stream_id(varint) ‖ error_code(varint) ‖ final_size(varint)
```

Abrupt termination of the sender's stream (§9.6). Ack-eliciting; on loss,
regenerated. Semantic violations: a `stream_id` naming a stream the
sender of the frame could not send on (their receive-only half) ⇒
`STREAM_STATE_ERROR` — with exactly one exception, the message-mode
overflow reset of §9.8, in which the *receiver* of a uni stream emits
RESET_STREAM as its abandonment signal (§9.6); a `final_size` below the
receiver's
highest-received offset, or conflicting with an already-pinned final size
⇒ `FINAL_SIZE_ERROR`; a `final_size` that would push stream- or
connection-level consumption above the advertised limit ⇒
`FLOW_CONTROL_ERROR`, checked **before** the §9.6/§10.3 credit true-up,
with checked or saturating `u64` arithmetic mandated (an unchecked sum
wraps for large `final_size` values and silently re-opens the window). A
RESET_STREAM naming an index at or below the space's closed-stream
watermark and not currently open is a no-op — ACKed, never re-opened
(§9.2).

**STREAM (`0x08`–`0x0f`)**

```
type(0x08 | OFF(0x04) | LEN(0x02) | FIN(0x01))
     ‖ stream_id(varint)
     ‖ [ offset(varint)   if OFF ]
     ‖ [ length(varint)   if LEN ]
     ‖ data(length bytes, or to the end of the plaintext if ¬LEN)
```

| Constant | Value |
|---|---|
| `STREAM_OFF` / `STREAM_LEN` / `STREAM_FIN` | 0x04 / 0x02 / 0x01 (bits of the type byte) |

OFF absent ⇒ offset 0. LEN absent ⇒ the data extends to the end of the
plaintext, and the frame must be the packet's final frame. FIN marks the
data's end offset as the stream's final size (an empty FIN-only frame is
valid). Semantics in §9.5. Structural errors: `length` overrunning the
plaintext; a ¬LEN frame that is not final; `offset + length` exceeding
2⁶² − 1. Semantic violations: data beyond stream or connection credit ⇒
`FLOW_CONTROL_ERROR`; a `stream_id` the peer could not send on ⇒
`STREAM_STATE_ERROR`; opening a stream beyond the cumulative limit ⇒
`STREAM_LIMIT_ERROR`; data beyond, or a FIN conflicting with, a pinned
final size ⇒ `FINAL_SIZE_ERROR`. A frame naming an index at or below the
space's closed-stream watermark and not currently open is a no-op —
ACKed, never re-opened (§9.2). All offset arithmetic (`offset + length`,
final-size and credit comparisons) is checked or saturating.

**[RATIFIED 2026/08/15 — ruling 97]** **These checks are a set, and their
evaluation order is normative**, because one crafted frame can trip
several and all of them end the connection — so the only observable
difference is the error code on the wire, which is what Appendix B
asserts on and what a peer's operator reads:

1. **`STREAM_STATE_ERROR`** — may this peer send this frame at all?
2. **the watermark no-op** (§9.2) — is this frame inert?
3. **`STREAM_LIMIT_ERROR`** — may this index exist?
4. **`FINAL_SIZE_ERROR`** — is it consistent with what we already know?
5. **`FLOW_CONTROL_ERROR`** — does it fit the credit we advertised?

Legality precedes the watermark because the two collide and disagree: for
a **locally-opened uni** stream the peer may never send STREAM at all, so
a frame naming a fully-closed local-uni index satisfies both rules, one
answering "silently ACK" and the other "end the connection". The
watermark answers *which index*; the state error answers *who may send*.
A frame the peer could never legally send at any index is not a late
retransmission of anything, and ordering the watermark first would delete
this check for exactly the indices an attacker can most cheaply name. The
check is decidable without consulting the stream table — §9.1's id
encodes direction and opener parity, and with the role fixed at install
(ruling 106) it is a total function of the id — so a freed stream cannot
confuse it.

Flow control runs **last** so the ledger is consulted exactly once per
frame, after the frame is known otherwise legal; this is the same
ordering §9.6 and §10.3 already require when they put the credit bound
check before any true-up. `FINAL_SIZE_ERROR` precedes it because a frame
contradicting a pinned final size is a statement about a stream we
already fully understand, and answering it with a credit code would
mislead. `STREAM_LIMIT_ERROR` precedes the open it would authorise, so a
frame naming an index above the limit performs **no** implicit opening
and emits **no** `StreamOpened` (ruling 99) — the order is what bounds
that event burst, not merely what names the error.

**MAX_DATA (`0x10`)** / **MAX_STREAM_DATA (`0x11`)**

```
type(0x10) ‖ max(varint)
type(0x11) ‖ stream_id(varint) ‖ max(varint)
```

Absolute-offset credit grants (§10). Monotone-max on receipt: a value not
above the current limit is a valid no-op (duplicates and reordering are
idempotent). Ack-eliciting; on loss, regenerated with the freshest value,
never byte-retransmitted. Semantic violations: MAX_STREAM_DATA for a
stream the *receiver of the frame* cannot send on ⇒ `STREAM_STATE_ERROR`;
likewise — QUIC's rule — MAX_STREAM_DATA for a stream in a space the
frame's receiver opens that the receiver has not yet opened (credit
frames never open streams; §9.2's implicit opening is for STREAM and
RESET_STREAM only). Credit
for a fully-closed stream (at or below the watermark, §9.2) is a valid
no-op.

**MAX_STREAMS_BIDI (`0x12`)** / **MAX_STREAMS_UNI (`0x13`)**

```
type(0x12|0x13) ‖ max(varint, cumulative stream count)
```

Cumulative-count credit for the corresponding space (§10.4). Monotone-max
on receipt. Ack-eliciting; regenerated. Structural error: `max` > 2⁶⁰
(unrepresentable as a stream index) — §8.2's structural class.

**PATH_CHALLENGE (`0x1a`) / PATH_RESPONSE (`0x1b`)** **[RATIFIED
2026/08/16 — ruling 208]**

```
type(0x1a) ‖ data(8 B, opaque)          — the challenge
type(0x1b) ‖ data(8 B, opaque)          — the same 8 bytes, echoed
```

Nine bytes each, fixed: no length prefix and no varint anywhere, so
neither frame can be truncated into a different valid frame and neither
needs a bound check beyond "eight bytes remain". §7.3 is their normative
home and the only thing that emits or consumes them. Both are
**ack-eliciting** (§8.3), which is load-bearing twice over: it is what
puts the challenge into the sent map so §7.3's death clock arms on it
(§7.4), and it is what makes the response elicit the peer's own ACK.

Structural error (§8.2's structural class, ⇒ CLOSE with
`PROTOCOL_VIOLATION`): fewer than 8 bytes remain in the plaintext after
the type byte. That is the **only** structural error either frame has.
In particular a `PATH_RESPONSE` whose eight bytes match no outstanding
challenge is **not** an error — it is a **semantic no-op**: it validates
nothing, it is traced, and the frame is otherwise ignored, exactly as
§8.4 treats an ACK whose `largest` exceeds anything sealed. The
distinction is deliberate and is a security property rather than
leniency: a mismatched response is what an off-path attacker's guess
looks like, what a response from a superseded arming looks like after a
second roam, and what a duplicate looks like after the address has
already validated. Killing the connection on it would hand any off-path
party that can guess a frame boundary a **remote kill primitive**
requiring no key, which is a strictly worse defect than the one this
mechanism exists to fix.

A received `PATH_CHALLENGE` obliges a `PATH_RESPONSE` carrying its eight
bytes verbatim. The obligation is unconditional and is **not** gated on
the challenge being one we expected, on the source having roamed, or on
anything else: the responder does not interpret the bytes, and a
responder that filtered them would be answering a question it cannot see
the point of. It is bounded — one outstanding response, overwritten by a
newer challenge rather than queued, since the newer one is the only one
whose answer can still validate anything (§17.5 budgets one, never a
list).

**CLOSE (`0x1c`)**

```
type(0x1c) ‖ error_code(varint) ‖ reason_len(varint) ‖ reason(reason_len B)
```

| Constant | Value |
|---|---|
| `CLOSE_REASON_MAX` | 256 B |

One frame type — no transport/application split and no offending-frame-type
field. `reason` SHOULD be UTF-8 but is carried as bytes. Not
ack-eliciting; never loss-retransmitted — the linger's reply rule is its
reliability (§15.2). Structural error: `reason_len` > 256 (§8.2).
`close()` truncates its `reason` to `CLOSE_REASON_MAX` at the handle
(§16.2) — an implementation must not be able to *produce* the over-length
case it must kill on receipt.

**DATAGRAM (`0x30`/`0x31`)**

```
type(0x30) ‖ data(to the end of the plaintext)          — must be final
type(0x31) ‖ length(varint) ‖ data(length B)
```

Unreliable payload (§11). Ack-eliciting; **never** retransmitted.
Structural errors: `length` overrunning the plaintext; a `0x30` frame
that is not the packet's final frame. A receiver-side oversize case is
unrepresentable (§11.4).

### 8.5 Coalescing and packing order

Frames-to-packets is many-to-many: one packet carries many frames, and
one stream's bytes span many packets. Within a packet the sender packs in
this order: the ACK first (if owed), then control frames — **[AMENDED
2026/08/16 — ruling 208]** `PATH_RESPONSE` and `PATH_CHALLENGE` **first
among the control frames**, then credit grants, RESET_STREAM, CLOSE —
then STREAM and DATAGRAM fill, then PING last
among **length-prefixed** frames if a
probe still owes ack-eliciting content. At most one extends-to-end frame
(¬LEN STREAM, or `0x30` DATAGRAM) per packet, in final position.

**[RATIFIED 2026/08/16 — ruling 208]** *This order and §7.3's priority
order answer different questions, and the two must not be read as one
rule.* §8.5 decides **byte placement inside a packet whose size is already
settled**; §7.3 decides **which class of output gets a scarce budget at
all**. So §7.3 ranking the path frames above a pure ACK is not in tension
with the ACK being packed first here: a packet that carries both carries
both, and nine bytes of path frame plus an ACK fit together in any packet
either could travel in. What the placement above buys is the case where
the two orders *could* diverge — a packet shrunk to the budget's admitted
room. Packing the path frames ahead of every other control frame is what
keeps the frame that **ends** the scarcity inside the packet the scarcity
allowed, rather than trimmed out of it by a credit grant.

**[RATIFIED 2026/08/16 — ruling 181]** *Two frames were told to be last;
the two senses are different and the parser forces the separation.* An
extends-to-end frame carries **no length prefix** — it is defined as
running to the end of the packet — so its final position is
**structural**: nothing *can* follow it, because anything that did would
be parsed as part of it. That claim is not negotiable. PING's "last" is
**ordinal**, a placement preference among length-prefixed frames, and a
one-byte frame's position carries no semantics. Therefore: **PING is
packed immediately before the extends-to-end frame**, and "PING last"
reads as *last among length-prefixed frames*. A sender may equally emit
the datagram in its `0x31` LEN form and keep PING physically last — both
parse identically and the choice is the sender's — but the ¬LEN form
**must never be followed by anything**. Slice 7 makes the collision
routine, since the **PTO** probe (§13.4) emits its PING into a packet
that may already carry an extends-to-end frame. **[AMENDED 2026/08/18 —
ruling 258]** The **contested** probe (§7.5) does not: it is built ahead
of the pump's ordinary packets and carries only the PING and §7.3's owed
path frames (§14.5, §16.5), so the collision cannot arise there.

Within
the STREAM fill, streams with pending data are served **round-robin** —
one quantum per stream per fill pass, the quantum size
implementation-defined — which is what makes the no-head-of-line-blocking
contract real under contention (§9.8).

### 8.6 The per-seal bound

hiss caps any one sealed message at `MAX_MESSAGE_LEN` = 65 535 B of
ciphertext. Every slither seal is at most `MAX_PLAINTEXT` + 16 = 1186 B —
far inside the cap; the frame layer never approaches it, and no future
batching may exceed it.

### 8.7 Retransmission classes and ack-eliciting

A packet is **ack-eliciting** iff it contains at least one ack-eliciting
frame (§8.3's column). Only ack-eliciting packets enter the sent-packet
map (§13.5); pure-ACK packets, CLOSE packets, and keepalives are never
tracked and never occupy the congestion window (§14.5).

Loss recovery retransmits **frames, never packets** (§13.5). Three
classes:

- **ranges** (STREAM): the lost packet's stream ranges return to the
  pending set and are re-framed on fresh counters — split, merged, or
  coalesced with new data freely; only still-un-ACKed sub-ranges are
  resent.
- **regenerate** (MAX_DATA, MAX_STREAM_DATA, MAX_STREAMS_BIDI/UNI,
  RESET_STREAM): the lost frame's *identity* re-queues, and the
  retransmission carries the **freshest current value** — never the stale
  bytes. (For RESET_STREAM the values are fixed at reset time; it re-emits
  until acknowledged or the stream state is discarded — with one
  carve-out: the receiver-emitted overflow reset of §9.6/§9.8 retires
  its receive half at the moment of emission, so its identity is
  retained at the connection level and re-emitted until acknowledged;
  the discard termination never applies to it.)
- **never** (PADDING, PING, ACK, DATAGRAM, CLOSE, PATH_CHALLENGE,
  PATH_RESPONSE): loss is absorbed by the
  next ACK, the next probe, the unreliability contract, or the linger
  reply rule respectively — and, for the two path frames, by a **standing
  obligation** rather than by loss recovery at all. **[RATIFIED
  2026/08/16 — ruling 208]** The distinction matters, because "never
  retransmitted" would otherwise read as "sent once and lost forever",
  which §7.3's no-deadlock argument cannot survive. `PATH_CHALLENGE` is
  **owed for as long as the arming lasts**: the sender re-emits it — with
  the **same** eight bytes, since ruling 208 fixes one challenge per
  arming — whenever §7.3's budget admits a packet and the address is still
  unvalidated, and stops owing it the instant the address validates or the
  next address change draws a fresh challenge. Re-emitting the same bytes
  to the same address leaks nothing: the secret is *who receives them*,
  not how many copies were sent. `PATH_RESPONSE` is owed on receipt of a
  challenge and discharged by one emission; if it is lost, the peer's
  still-standing challenge asks again and a fresh response is owed. Neither
  frame is ever re-queued by loss detection, so neither appears in §13.5's
  retransmission path.

## 9. Streams

### 9.1 Stream identifiers

A stream ID is a varint. Its two low bits tag the stream; the remaining
60 bits are `index`, a per-space monotonically allocated counter from 0:

| Bit | Meaning |
|---|---|
| `0x01` | opener: 0 = the connection initiator, 1 = the acceptor |
| `0x02` | direction: 0 = bidirectional, 1 = unidirectional |

This yields four independent ID spaces (initiator/acceptor ×
bidi/uni), QUIC's encoding verbatim. The opener bit is mandatory anyway —
both sides open streams unprompted, and parity is the only handshake-free
collision avoidance — and the direction bit is what makes the message
primitive cheap (§9.8): a uni stream lets the receiver allocate no
send-half state and expect no reverse FIN. **[RATIFIED 2026/08/14]** The four-space
choice trades slither-minimalism for QUIC congruence (a one- or two-space
collapse would save nothing real — the two bits are already paid for in
the varint — but a reviewer may prefer the smaller conceptual surface);
see also §10.2's constants flag (the two halves of one ruling).

**Role stability.** The opener bit refers to the roles of the
connection's **establishment**: the connection initiator is the dialler,
or under simultaneous open the tie-break winner (§6.7). Parity is fixed
at establishment and never changes for the connection's life.

### 9.2 Implicit opening

There is no OPEN frame. A frame referencing stream `N` of a space opens
`N` and every lower-numbered not-yet-open stream of that space, subject to
the cumulative limit (§10.4) — opening past it is `STREAM_LIMIT_ERROR`.
The first STREAM or RESET_STREAM frame is the open.

**The closed-stream watermark.** Each of the four spaces keeps, alongside
its open set, the **highest fully-closed stream index** (§9.7). A STREAM
or RESET_STREAM frame naming an index **at or below the watermark and not
currently open** is a **no-op — processed as acknowledged, never
re-opened**; implicit opening applies only to indices *above* the
watermark. This tombstone is what makes exactly-once delivery real: a
receive half frees at read-to-final (§9.7) and a sugar stream frees the
instant its message surfaces (§9.8), both *before* the sender can know
(only our ACK tells it), so a single lost ACK makes the peer's routine
PTO retransmission re-name the freed stream — and without the watermark
that retransmission would re-open it, restart the reassembler, re-pin the
final size, and surface the same message twice (or fire a phantom
`StreamOpened` for a finished stream). The watermark is monotone, lives
for the connection's life, and costs one index
per space. (An index at or below the watermark that is not open is
necessarily a *closed* stream: implicit opening opened everything at or
below the watermark when the watermark stream was first named.)

### 9.3 The send half

Conceptual states (RFC 9000 §3.1's shape):

```
Ready ──write──▶ Send ──STREAM+FIN sent──▶ DataSent ──all ACKed──▶ DataRecvd (terminal)
   │                │                          │
   └────────────────┴──────reset()────────────▶ ResetSent ──RESET ACKed──▶ ResetRecvd (terminal)
```

At the terminals the send half's state is freed (§9.7). The six-state
diagram is exposition, not an implementation mandate: an implementation
collapses it (quinn-proto's shape: `Ready` / `DataSent { finish_acked }` /
`ResetSent`, with the terminals represented by removal).

### 9.4 The receive half

```
Recv ──STREAM+FIN──▶ SizeKnown ──all bytes──▶ DataRecvd ──app read all──▶ DataRead (terminal)
   │                     │
   └──RESET_STREAM───────┴──▶ ResetRecvd ──app read reset──▶ ResetRead (terminal)
```

The receive half buffers arriving ranges and delivers the **contiguous
prefix** to the application as it becomes available; a FIN pins the final
size; the terminals free the state. The same collapse note applies
(`Recv { size: Option<u64> }` / `ResetRecvd { size, error_code }`).

### 9.5 STREAM frame semantics

Each STREAM frame is a labelled byte range `(stream_id, offset, data)` of
a per-stream logical byte sequence — not a self-contained message. The
offset field alone reconstructs order, decoupled from packet arrival
order and packet numbers; fragmentation is not a special case (a large
write is consecutive ranges across as many packets as needed), and there
is no message-size ceiling beyond flow control. Rules:

- Ranges arrive in any order and may **overlap** (retransmission
  re-framing, §8.7): a receiver delivers each byte exactly once; a byte
  received twice with differing values is undefined behaviour of the
  sender (an honest sender never produces it) and the receiver may keep
  either.
- **FIN pins the final size** as the frame's end offset
  (`offset + data length`). Receiving data beyond a pinned final size,
  a FIN pinning a size below already-received data, or two pins that
  disagree ⇒ `FINAL_SIZE_ERROR` (CLOSE, §8.2).
- Retransmitted stream bytes consume no new flow-control credit (§10.7);
  data beyond advertised credit ⇒ `FLOW_CONTROL_ERROR`.
- An empty STREAM frame with FIN is a valid end-of-stream marker; an
  empty frame without FIN and without data is valid and a no-op
  (tolerated, never emitted). **[RATIFIED 2026/08/15 — ruling 100]** The
  no-op is about the frame's *data*, not about §9.2's open: such a frame
  naming a not-yet-open index above the watermark **does open it**, and
  every lower one, and charges the cumulative limit. It delivers no
  bytes, pins no final size and consumes no credit — that is all "no-op"
  claims. Reading it as suppressing the open would make the open set
  depend on a payload property §9.2 never mentions, and would make a
  zero-length write on an open stream indistinguishable in the codec from
  a stream-creating frame. Reachable only from a foreign or hostile peer,
  whose own allowance it spends (§10.4).
- Reassembly memory is bounded twice over: by advertised credit (the span
  a receiver must cover, §10.6) and by the reassembly-fragment mandate of
  §10.6 — per-stream reassembly state MUST be O(advertised credit) and
  MUST NOT scale with the number of received frames.

### 9.6 RESET_STREAM semantics

`reset(error_code)` abandons a send half abruptly: pending and in-flight
data for the stream stop being retransmitted, and RESET_STREAM
`{ stream_id, error_code, final_size }` is emitted (regenerated until
acknowledged), where `final_size` is **the end offset of the highest byte
this endpoint has actually transmitted for the stream, or 0 if none**
(**[RATIFIED 2026/08/15 — ruling 111]**: bytes `write()` has accepted but
that have never been sealed onto the wire are *not* counted — this
sentence previously read "the number of bytes the stream would have
carried", which disagrees with its own parenthetical the moment a
`reset()` follows a `write()` that congestion control has not yet
released, i.e. in ordinary operation),
**truing up the receiver's connection-level flow-control accounting**:
the receiver counts the full `final_size` against `MAX_DATA` consumption
exactly as if the bytes had arrived (§10.1), so both ends agree on
consumed credit even though the tail never arrives. The receive half
surfaces `ReadError::Reset(error_code)` (§18.1), discards its reassembly
buffer, and closes when the application observes the reset. A RESET_STREAM
for an already-FIN-complete receive half is a valid no-op if the final
sizes agree, `FINAL_SIZE_ERROR` otherwise.

The true-up runs only **after** the §8.4 `FLOW_CONTROL_ERROR` check that
`final_size` does not exceed the advertised limits, so it releases
exactly the credit the asserted bytes had already consumed and can never
manufacture more; and when the receive half is retired, the same
`final_size` counts as **consumed** for connection-level credit-advance
(§10.3).

**The receiver-emitted reset (the §8.4 exception).** In exactly one case
the *receiver* of a uni stream emits RESET_STREAM — the message-mode
overflow of §9.8. Its `final_size` field carries the receiver's highest
received offset and is informational: the stream's sender, on receiving
it, stops (re)transmitting the stream, frees its send half (un-ACKed
ranges dropped; the freed half counts toward full closure, §9.7), and
surfaces `WriteError::Reset(error_code)` to a blocked or subsequent
writer. No flow-control true-up runs in this direction — the frame
releases the *sender's* obligation, not the receiver's credit.
**Its delivery is reliable independent of the retired half**: emitting
it retires the receive half at once (§9.8), but the reset's frame
identity `{ stream_id, error_code = MESSAGE_OVERFLOW, final_size }` is
retained in
the connection's regenerate set and re-emitted on loss **until
acknowledged** — §8.7's "stream state is discarded" termination does
not apply to this frame (the retained identity is a few words of
connection state, not stream state). Without this retention a single
lost reset re-strands the sender for good: its PTO retransmissions
are no-op'd and ACKed below the closed-stream watermark (§9.2), so
liveness never fires, while the send half stays wedged at the stream
window — the exact stall the reset exists to cure.

### 9.7 Lifecycle and garbage collection

Stream state is freed eagerly:

- a **send half** frees when every byte up to the final size, FIN
  included, is acknowledged (`DataRecvd`), or when its RESET_STREAM is
  acknowledged (`ResetRecvd`);
- a **receive half** frees when the application has read to the final
  size (`DataRead`), or has observed the reset (`ResetRead`), or — for an
  abandoned handle — **at the moment of abandonment** (§16.2, ruling 93:
  it does not wait for a final size that a sender stalled at the stream
  window has no reason to send);
- a stream is **fully closed** when its halves (one for uni, two for
  bidi) are freed; full closure is what earns the peer a MAX_STREAMS
  credit (§10.4).

The allocator never reuses a stream ID; churn is bounded by the free-list
pattern (state lives only for open streams). Freeing is what advances the
closed-stream watermark (§9.2): a fully-closed stream leaves no per-stream
state behind, only the per-space tombstone that keeps late
retransmissions naming it inert. Retiring a receive half also trues up
connection-level credit for its unread bytes (§10.3).

### 9.8 Messages — sugar over auto-managed uni streams

**[RATIFIED 2026/08/14]** The reliable-unordered message primitive is **API sugar
over unidirectional streams — there is no DATA frame type and no second
reliability engine.** Two reliability paths (whole-message retransmit
alongside range retransmit) were the single largest avoidable surface in
the old design; one-stream-per-message preserves the
no-head-of-line-blocking contract (messages on distinct streams never
stall each other), and exactly-once surfacing rests on offset
reassembly **plus the closed-stream watermark** (§9.2 — a freed message
stream cannot be re-opened by a late retransmission) — the old seq-dedup
machinery is gone. The costs accepted:
per-message overhead is a STREAM header (≈ 3–6 B versus the old 11-byte
DATA header — a wash or better), stream-state churn per message (bounded
by §9.7's GC), and messages bounded by the initial stream window. The
alternative — a distinct reliable-message frame — preserves the tiny-RPC
micro-optimum but doubles the retransmission surface forever.

- `Connection::send_message(bytes)`: allocates the next outbound uni
  stream, writes the whole payload, sets FIN, and garbage-collects the
  stream when the FIN'd range is fully acknowledged. No stream handle
  surfaces — so `Connection::acked()` (§16.2, rulings 47/54) is how an
  application awaits that acknowledgement, the send verb itself having
  resolved as soon as the payload entered send state. Payloads above
  `MESSAGE_RECV_MAX` are rejected at the handle
  (`MessageError::TooLarge`) — a larger sugar send could stall forever
  against a sugar-consuming receiver, which never extends credit.
- `Connection::recv_message()`: treats each incoming uni stream as one
  message, surfacing the payload only when reassembly is complete (FIN
  and all bytes), then frees the stream.
- `accept_uni()`/`open_uni()` remain available for incremental streams.
  The two receive verbs draw from the same incoming-uni supply: a stream
  claimed by `accept_uni()` leaves message consideration;
  `recv_message()` surfaces the oldest fully-reassembled unclaimed uni
  stream. Which mode consumes a given stream is the receiving
  application's choice, **invisible on the wire**.

**Mixing the two receive modes on one connection is a programming
error.** **[RATIFIED 2026/08/14 — ruling 51]** Normative, and stated
here because the shared supply above is exactly what makes it true: the
wire carries **no discriminator** between "this uni stream is a message"
and "this uni stream is a stream", so the **receiver's verb choice
alone** decides how a stream is interpreted and a **sender has no way to
signal intent**. A receiver that runs `recv_message()` and `accept_uni()`
against the same connection gets a nondeterministic assignment —
`accept_uni()` can claim a stream the sender meant as a message, and a
message-mode claim can consume one the sender meant as an incremental
stream — and no implementation can repair it, because the information
needed was never transmitted. Two patterns are safe, and one of them is
almost always what the application wanted: use **bidi streams for
streaming alongside messages** (`open_bi`/`accept_bi` draw from a
separate supply, §9.1, and never collide with the message sugar), or
**tag in band** — carry everything as messages and put the application's
own discriminator in the payload. Note the asymmetry that invites the
mistake: it is exactly `open_uni()` + `send_message()` that collides;
`open_bi()` alongside messages is safe. When the rule is broken the
failure is **defined and loud**, not silent — the overflow policy below —
but it is a defined failure, not a working configuration.

| Constant | Value |
|---|---|
| `MESSAGE_RECV_MAX` | = `INITIAL_MAX_STREAM_DATA` (262 144 B) |

The bound is structural: the receiver never extends per-stream credit for
a sugar-consumed stream, so a message is bounded by the initial stream
window. Larger transfers use real streams.

**The overflow policy — an unclaimed stream fails loudly, it does not
stall.** **[RATIFIED 2026/08/14; amended 2026/08/14 — ruling 51]** A uni
stream that consumes its
full initial window without pinning a final size can never complete as a
message (`MESSAGE_RECV_MAX` = the initial window, and message-consumed
streams never earn more credit); left alone it stalls the sender at the
window **for ever** — no error, no timeout, and keepalives still flowing
in both directions, so liveness never fires — while its buffered bytes
hold connection credit on both ends (§10.3). Note where the bound is
*not* enforced: `MESSAGE_RECV_MAX` is checked on the **send** side of
`send_message()` only, so `open_uni()` bypasses that check entirely, and
a legitimate incremental stream written against a message-mode receiver
is precisely this case.

The rule: an **unclaimed** uni stream — neither claimed by
`accept_uni()`, nor surfaceable by `recv_message()`, which it cannot be,
having no FIN — that reaches `MESSAGE_RECV_MAX` **at its highest received
offset, with no final size pinned**, is **reset** by the receiver, so its
sender learns through
`WriteError::Reset(MESSAGE_OVERFLOW)` (§18.1) instead of stalling. The
**[AMENDED 2026/08/16 — ruling 164]** *"which it cannot be, having no
FIN" was arguing as an established fact the very thing ruling 153 had to
add as an independent clause.* It is true only of a sender that has not
yet sent its FIN — which is exactly the case this rule is for — and it
does **not** establish that the predicate is safe, because a conforming
`send_message` of precisely `MESSAGE_RECV_MAX` bytes reaches the bound at
the same instant. What makes the predicate safe is ruling 153's other
half: `send_message` **carries the FIN on its final data frame**, so
reaching the bound implies the final size is already pinned and the rule
above cannot fire. A reader who takes the parenthetical as the argument
will build the version that resets its own protocol's largest legal
message.

The receive half retires (its bytes count as consumed at the connection
level, §10.3) and the receiver emits RESET_STREAM (`0x04`, §8.3, §8.4 —
**no new frame type**) with **error code `MESSAGE_OVERFLOW` = `0x06`**,
the one receiver-emitted reset (§9.6), retained and
regenerated until acknowledged despite the retired half (§9.6, §8.7) —
so the sender's stream frees instead of wedging, even when the reset
itself is lost. The receiver **MUST** trace what it emitted, under
§18.2's `slither::frames` (ruling 59): the sender learns by error code,
and without the trace the end that actually chose the conflicting mode
learns nothing at all.

**[RATIFIED 2026/08/14 — ruling 52]** *The code is distinguishable, and
that is the point.* This reset first shipped carrying `0` (`NO_ERROR`),
which a sender could not tell from the peer's application calling
`reset(0)` or from a dropped `SendStream` — leaving ruling 51's "fail
loudly" only half loud, since the one thing the sender needed to learn
was *which* hazard it had hit. `MESSAGE_OVERFLOW` is therefore minted
from §15.3's transport-reserved range, and §15.3's *"never sent"* gains
this single named exception. **This is not a layout change:** it is a
new *value* in the existing `error_code` varint of an existing frame, in
a case that previously could not arise — no header moves, no frame type
is added, no size changes, and the golden-wire vectors (which pin
handshake bytes) are untouched. A sender receiving it knows it wrote an
unbounded `open_uni()` stream to a receiver consuming in message mode,
which is precisely the §9.8 mixing hazard, and can say so in a log
rather than reporting an anonymous reset.

**When the check runs, and why it is guarded.** The receiver runs it
while a `recv_message()` claim is pending, and at the instant such a
claim is made: the application is then demonstrably in message mode,
which is the only evidence the receiver has that nobody will ever
`accept_uni()` this stream. It applies to **every** unclaimed window-full
stream, not merely the oldest — a stream sitting behind a slower one must
not evade it. The guard is deliberate, and the unguarded form is worse:
a receiver in *stream* mode that is merely slow to call `accept_uni()` is
exercising §16.4's backpressure-by-retention, and resetting its stream
the moment the sender filled the initial window would break an ordinary
lazy accept loop. Streams claimed by `accept_uni()` are untouched: real
streams extend credit normally.

**One consequence, retired.** **[SUPERSEDED 2026/08/14 by ruling 52.]**
This paragraph previously recorded that the reset carried `0`
(`NO_ERROR`, §15.3), leaving the sender unable to tell it from a peer's
own `reset(0)` or from a dropped `SendStream` (§16.2), and argued that
minting a code in §15.3's reserved transport space was not worth it.
Ruling 52 overturned exactly that judgement: the code **is** minted
(`MESSAGE_OVERFLOW` = `0x06`, above), because the one fact the sender
needed was *which* hazard it had hit. Recorded here rather than deleted
so the earlier position is not re-proposed as new.

*Declined alternatives.* (a) **Keep extending stream credit past the
message bound and surface the stream as a stream** — avoids the
receiver-reset carve-out at the price of handing the application a mode
it never asked for. (b) **A per-connection uni mode fixed at
configuration** — makes the mixing error unrepresentable, but it also
removes the legitimate connection that carries chat messages *and* a file
transfer. (c) **A wire-level mode signal on the stream** — closes the
ambiguity at its root, and is **wire-affecting**: it moves bytes and
turns the golden-wire pin red, so it is not available on this wire line.

### 9.9 STOP_SENDING

Reserved (`0x05`), deferred (§19). Until it ships there is no wire signal
for "stop transmitting this stream to me"; an uninterested receiver drops
its handle and discards arrivals (§16.2), and the sender runs to FIN or
resets.

## 10. Flow control

### 10.1 The model

Two levels, both receiver-driven, both expressed as **absolute byte
offsets** (a limit says "you may send up to offset X", never "X more
bytes"):

- **Stream level**: each stream's data is bounded by the peer's
  advertised per-stream limit (initially `INITIAL_MAX_STREAM_DATA`,
  raised by MAX_STREAM_DATA).
- **Connection level**: the **sum over all streams** of the highest
  received offset (the final size, once pinned) — where a reset stream
  contributes its trued-up `final_size`
  (§9.6) — is bounded by the peer's connection limit (initially
  `INITIAL_MAX_DATA`, raised by MAX_DATA). A sender respects both limits;
  whichever is tighter binds.

Limits advance monotonically: a received credit frame applies as
monotone-max, so duplicates and reordering are naturally idempotent
(§8.4).

### 10.2 Initial values — protocol constants, no negotiation

**[RATIFIED 2026/08/14]** slither has no negotiation surface at all, so initial
windows are protocol constants, identical in both directions and all
stream spaces; later credit is receiver policy. The values ship
**ratified-but-revisitable**, gated on the Appendix B window-constants
throughput validation (with §9.1's four-space choice, the two halves of
one ruling):

| Constant | Value |
|---|---|
| `INITIAL_MAX_DATA` | 1 048 576 B (1 MiB) |
| `INITIAL_MAX_STREAM_DATA` | 262 144 B (256 KiB) |
| `INITIAL_MAX_STREAMS_BIDI` | 32 (cumulative) |
| `INITIAL_MAX_STREAMS_UNI` | 128 (cumulative — higher for message traffic, §9.8) |
| `STREAMS_CREDIT_BATCH` | 8 — **receiver policy, not a wire constant** (ruling 103) |

**[RATIFIED 2026/08/15 — ruling 103]** **The table's five rows are two
kinds of thing, and only the first four are wire constants.** The four
initial windows are unnegotiated, so both ends must assume the same value
and a divergence corrupts the peer's accounting immediately; changing one
is a wire change. `STREAMS_CREDIT_BATCH` is when a receiver *chooses* to
advertise — this section's own "later credit is receiver policy" — and
two peers running different batch values interoperate perfectly. Grouping
them in one table gives the policy knob the wire pins' protection, so a
future tuning change looks like a wire change and draws a ratification
round it does not need.

`REASSEMBLY_CHUNKS_MAX` — and, since ruling 270,
`REASSEMBLY_MIN_CONFORMING_FRAME` beside it (§10.6) — is a **third** kind:
receiver policy like
the batch, but externally *observable*, since a peer that fragments past
one receiver's ceiling is killed and past another's is not (§10.5). The
values and their locations do not move — that would be wire-pin churn for
nothing — but the code and `spec_constants.rs` mark which kind each is.

**The two receive windows are config-raisable; the constants are the
defaults.** **[AMENDED 2026/08/18 — ruling 259(viii)]** A receiver may
advertise *more* than `INITIAL_MAX_DATA` / `INITIAL_MAX_STREAM_DATA`,
and an endpoint config may say so once, statically, for every connection
it mints. Ruling 247(a) is what this answers: the ratified pair caps one
stream at ≈ `INITIAL_MAX_STREAM_DATA / (2 × RTT)` **[AMENDED 2026/08/18
— ruling 269]** — measured 1.4 MiB/s at 100 ms, 0.44–0.57 × `W/RTT`
across 20–100 ms; the factor is §10.3's half-window re-grant (a
window-limited sender spends `W/2`, then waits a round trip), and this
sentence read `/ RTT` until the benchmark measured it
(`bench-vs-tcp-2026-08.md`) — and before this a consumer had no way to
buy more.

**Nothing on the wire changes shape.** The knob is not negotiation and
is not a sixth row in the table above: the *initial* values are still
protocol constants, still unnegotiated, still identical in both
directions, and still what a peer assumes before any credit frame
arrives. A raise is therefore **said**, not assumed — carried by the
MAX_DATA and MAX_STREAM_DATA frames §10.3 already emits, at values §8.4
already admits. A receiver that widens its own ledger without emitting
one has changed nothing its peer can observe.

Three bounds, and they are the whole of the validation: **it raises
only** — a configured window below its ratified constant is refused
rather than clamped, because lowering re-opens §17.5's memory ceiling,
§9.8's message bound, and the assumption a peer makes before the first
credit frame; **`stream ≤ connection`**, the relation the constants
already hold; and **both ≤ 2⁶² − 1**, the largest absolute offset a
§8.1 varint carries.

**`MESSAGE_RECV_MAX` does not move with it.** §9.8's bound is checked on
the **send** side, and a sender cannot know what its receiver
configured; a locally raised message bound would emit a payload that a
default peer resets with `MESSAGE_OVERFLOW`. It stays equal to
`INITIAL_MAX_STREAM_DATA` — the constant, not the configured window —
so §9.8's bound (*messages bounded by the initial stream window*) is
true of every conforming sender against every receiver, whatever either
has configured.

The memory consequence is the operator's, knowingly: §17.5's
per-connection receive commitment is the **connection** window, so
raising the pair multiplies that term.

### 10.3 Advancing credit

The re-grant rule, concrete (RFC 9000 §4.2's shape): with `WINDOW` the
level's window (the advertised stream or connection window — the
ratified constant by default, ruling 259(viii)'s configured raise
otherwise), the **prospective limit** is
`bytes_read + WINDOW`, and the receiver emits MAX_STREAM_DATA or MAX_DATA
when `prospective_limit − last_advertised ≥ WINDOW/2` — that is, when the
read offset has advanced at least half a window since the last
advertisement. Consumption, not arrival, drives credit: an unread
buffer earns nothing. Credit frames are ack-eliciting (loss recovery
regenerates them with the freshest value, §8.7) yet liveness-neutral
(§7.4) — and *liveness-neutral* carries ruling 33's exact sense, both
halves of it: they are **non-marking**, sealed via `seal_quiet`, leaving
`last_send` untouched so they defer no keepalive; and, being
ack-eliciting, they **arm the death clock** like any other ack-eliciting
send (§7.4). A connection whose only output is credit is not thereby
exempt from dying. Sugar-consumed streams never earn stream-level credit
(§9.8); their reads still earn connection-level credit.

**Retirement advances connection credit.** When a receive half is retired
for any reason — read to its final size, reset observed, handle abandoned
(§16.2), or surfaced as a message (§9.8) — **all of its bytes up to its
final size count as consumed
for connection-level credit-advance**, exactly as if the application had
read them; stream-level credit is simply never re-granted for a retired
stream (there is no stream to grant to).
**[AMENDED 2026/08/16 — ruling 154]** This list had a **fifth** entry,
"final size reached with no reader (§9.7)", and **§9.7 — the section it
cited — does not contain it**: a receive half frees on three triggers
there, and arrival with no reader is not one. Had it been real, a complete
but unclaimed message stream would retire and true up connection credit at
the FIN, *before* `recv_message()` claimed it — contradicting §10.6's
"message and datagram payloads stay accounted inside the core … until the
handle takes them" and §16.4's backpressure-by-retention. Three statements,
at most two of which could hold; retention until claimed is the rule. Without this rule, MAX_DATA is
an absolute limit advanced only by reads, and cumulative discarded or
reset bytes march the connection into a permanent send stall after
`INITIAL_MAX_DATA` with no error and no timer — reachable in honest
operation by any application that cancels streams. The true-up applies
only **after** the §8.4 `FLOW_CONTROL_ERROR` bound check, with checked or
saturating `u64` arithmetic (§9.6): for every retirement that has a
pinned final size it releases exactly the credit the bytes had consumed,
and no retirement can be driven above the **advertised** limit — a peer
cannot manufacture credit, only waste its own window. The
true-up is **absolute, not additive**: it advances the stream's
contribution to the connection-level consumed count **to** its
`final_size` — a monotone bring-to-final, idempotent with bytes
already counted by reads (§10.1's per-stream absolute sum) — and
never adds `final_size` on top of them; a read-then-retire sequence
therefore counts each byte exactly once, and a receiver can never
over-advance its own MAX_DATA past its buffer commitment.

**[RATIFIED 2026/08/15 — ruling 93]** **Abandonment is the one retirement
with no final size, and it trues up to the stream window.** Dropping a
`RecvStream` (§16.2) retires the half **immediately** — freed, tombstoned
at §9.2's watermark, trued up in the same step — rather than arming a
retirement for a FIN or reset that a stalled sender has no reason to
send. Its final size is unknown and unknowable, so the value is the
**highest stream-level limit this endpoint ever advertised** for that
half: seeded to the advertised stream window (§10.2 —
`INITIAL_MAX_STREAM_DATA` unless config raised it, ruling 259(viii))
and grown by the re-grant above, never the constant as a fixed value. That is the least
upper bound on what the peer could have sent without a
`FLOW_CONTROL_ERROR`, and it is ≥ the highest received offset, so the
bring-to-final stays monotone.

The highest *received* offset is the wrong value and its failure is
silent: the sender charges its connection window by highest offset
**sent**, so bytes lost or still in flight are already charged there and
absent here, and the difference is leaked for the connection's life. The
memory argument lands on the same value and is the stronger one — §10.6
makes credit the buffer commitment, what was committed to that half was
its stream window, and freeing the half releases exactly that. Releasing
less than the memory actually freed is the wedge this rule exists to
close. Over-advancing is bounded at one stream window per abandoned half,
and abandonment requires a claimed stream, which §10.4 bounds.

The sender still stalls at the **stream** window — §16.2 says so, and
with `STOP_SENDING` deferred (§19) no slither frame can cure it — but the
**connection** window and the **cumulative stream allowance** are both
released, which is what "never wedges the connection window" requires.

### 10.4 Stream limits — cumulative credit

**[RATIFIED 2026/08/14]** MAX_STREAMS frames exist, diverging from a fixed-cap
design, because messages-as-streams (§9.8) makes stream churn the common
case and the limit is **cumulative-count** (QUIC's model): a fixed
cumulative cap would kill the connection after N messages, and a
"concurrent" cap requires both ends to agree on close timing — exactly
the ambiguity the credit model exists to avoid. The cost is two frame
types and a replenishment rule.

- The limit counts **streams ever opened** in a space; opening stream
  index `i` requires cumulative limit > `i`.
- The receiver grants +1 as it **fully closes a peer-opened stream** of
  the space (§9.7) — closing streams we opened must not inflate the
  peer's allowance (RFC 9000 §4.6's scope) — batching
  advertisements: emit MAX_STREAMS when ≥ `STREAMS_CREDIT_BATCH` (8)
  grants are unadvertised, **or** when the peer's remaining allowance
  drops to ≤ `STREAMS_CREDIT_BATCH` — **one constant, both triggers**
  (ruling 102: a batch of 8 and a headroom of 8 are the same batch of
  slack, and no test can separate the readings at today's value, so the
  two must move together when it is revisited). Receipt of MAX_STREAMS
  surfaces
  `ConnEvent::StreamsAvailable { dir }` to wake blocked openers (§16.4).
- Opening beyond the limit ⇒ `STREAM_LIMIT_ERROR` ⇒ CLOSE (§8.2).
- `STREAMS_BLOCKED` stays deferred with the rest of the BLOCKED family
  (§19).

### 10.5 Violations

**§10 defines three violations, and this section lists all of them.**

A peer exceeding advertised credit — stream or connection level — is a
protocol violation: CLOSE with `FLOW_CONTROL_ERROR`. A peer opening
beyond a stream limit: CLOSE with `STREAM_LIMIT_ERROR`. For these two
there is no tolerance band; the limits are exact (§8.2's semantic class),
because both are **accounting** limits that the sender computes for
itself from what this endpoint advertised, so a peer that exceeds one has
either miscounted or is probing.

**[RATIFIED 2026/08/15 — ruling 104]** The third is defined in §10.6: a
stream whose stored discontiguous ranges would exceed §10.6's
credit-derived ceiling (floor `REASSEMBLY_CHUNKS_MAX` — ruling 270) after
coalescing is a protocol violation, CLOSE
with `PROTOCOL_VIOLATION` (§15.3). **It is a tolerance and not an exact
limit** — the ceiling's *value* is now computable from the advertised
window and a published constant **[AMENDED 2026/08/18 — ruling 270]**, but
a sender still cannot compute its own *standing* against it, which depends
on this receiver's coalescing and on the arrival order the *network*
produced;
§10.6 ships the value revisitable, and its admissible implementation (a)
makes the ceiling unreachable entirely. It is the memory-safety bound of
the three, and the one an implementer building §10's violation handling
from this section alone has historically missed.

*The paragraph above and the two-sentence claim before it must be read
together, and were amended together.* This section previously enumerated
two of three and closed on "there is no tolerance band; the limits are
exact" — a claim true of exactly the two it listed and false of the one
it omitted, which is what made the omission hard to see: the section read
as complete because it was internally consistent with its own gap.

### 10.6 Credit is the buffer commitment

**[RATIFIED 2026/08/14]** The advertised credit **is** the receiver's buffer
commitment: a receiver only advertises what it will buffer until read.
This replaces the superseded draft's `shed_mask`/`RECV_BUFFER` receive
backpressure wholesale — the problem that machinery solved (bounded
receive memory without acknowledging undelivered data) is solved
principledly: an in-credit packet always has buffer room **by
construction**, so no delivered-but-shed state can exist, and a
beyond-credit packet is a violation (§10.5), not a shed. DATAGRAM frames
need no shed logic — dropping an unreliable datagram at a full queue is
legitimate (§11.5), and acknowledging its packet is honest (an ACK
confirms packet arrival; datagram delivery was never promised). The
removal is sound only because flow control is now load-bearing for memory
safety; the supporting check: **no non-stream, non-datagram frame can
force unbounded buffering** — ACK processing is bounded intersecting
(§12.5), credit frames apply as O(1) monotone-max, PING/PADDING are
O(1), RESET_STREAM *frees* state, and CLOSE enters the linger. What
survives from the removed design is its invariant: liveness and roaming
are driven only by authenticated, window-marked packets (§7.2).

**The second bound — reassembly fragments.** **[RATIFIED 2026/08/14]** Byte credit
bounds the *span* a receiver must cover, not the *number of stored
discontiguous ranges* inside it: one-byte STREAM frames at offsets
0, 2, 4, … would store ~512 000 ranges within 1 MiB of credit, inflating
real memory 25–50× over the advertised commitment — the exact
amplification argument §11.3 uses against byte-bounded datagram queues,
applied to the reliable path. The mandate: **per-stream reassembly state
MUST be O(advertised credit) and MUST NOT scale with the number of
received frames.** Two implementations are admissible: (a) a
span-allocated buffer plus a received-bitmap (a 1 MiB span costs
1 MiB + 128 KiB, frame-count-independent; the ceiling below is then
unreachable), or (b) the default — received ranges are coalesced on
insert, and a stream whose stored discontiguous ranges would exceed
`REASSEMBLY_CHUNKS_MAX` (= 1024) after coalescing is a protocol
violation: CLOSE with `PROTOCOL_VIOLATION` (§15.3; quinn's
defragment-plus-hard-fail shape). The ceiling value ships
ratified-but-revisitable, gated on the Appendix B
defragmentation/throughput check.

**[AMENDED 2026/08/18 — ruling 270]** **The ceiling is derived from the
advertised credit, and `REASSEMBLY_CHUNKS_MAX` is its floor.** A receiver
tolerates `max(REASSEMBLY_CHUNKS_MAX, W / P + 1)` stored discontiguous
ranges per stream, where `W` is the stream window it advertises and `P` is
`REASSEMBLY_MIN_CONFORMING_FRAME` (1024 B — receiver policy, observable,
the same third kind as the ceiling itself). The flat value was **stricter
than this section's own mandate**, which is already stated as *O(advertised
credit)*, and the strictness is what killed honest peers: a stream's credit
and its tolerated hole count were two constants that did not scale
together, so at a raised window a sender **inside its credit**, on a path
losing packets in the pattern a saturated receive socket produces, exceeded
the second while obeying the first. The property the derivation buys — and
the reason the divisor is packet-scale rather than 2 — is this: **a peer
that never exceeds its advertised credit and whose STREAM frames each carry
at least `P` bytes cannot cross the ceiling under any loss or reordering
pattern.** Stored ranges are maximal runs, since they coalesce on
*adjacency* and not merely on overlap, so each is at least one frame wide
except the partially-read front one, and disjoint ranges of at least `P`
bytes inside a `W`-byte span number at most `W / P`. **The disposition does
not change.** Crossing the ceiling is still CLOSE with `PROTOCOL_VIOLATION`
(§10.5's third violation), and this section's own worked flood — one-byte
frames at offsets 0, 2, 4, …, some `W / 2` ranges — sits **512×** above the
derived ceiling and still dies there. That boundary is the point of the
change and not a casualty of it. This is the revisit the paragraph above
reserved: the Appendix B defragmentation/throughput check was run at a
raised window and came back **fatal rather than slow**.

**[AMENDED 2026/08/17 — ruling 253]** **The mandate above bounds state;
this clause bounds work.** Coalesce-on-insert's total copy work per
stream MUST be O(that stream's advertised credit · log credit) — every
stored byte is
copied O(log) times across its lifetime (the small-to-large discipline),
never once per bridging frame. Without the work bound a peer alternating
bridging inserts buys receiver work three orders of magnitude past its
wire bytes while staying inside flow credit — the lever lives exactly in
the gap between a state bound and a work bound, and the state mandate
alone says nothing about it. The evidence is an adversarial
alternating-bridging workload asserted from the separating side — the
pre-253 whole-span merge fails it (an Appendix B obligation) — with the
throughput gate guarding the honest path; the gate alone is not the
evidence, because the whole-span merge passes it.

| Constant | Value |
|---|---|
| `REASSEMBLY_CHUNKS_MAX` | 1024 stored discontiguous ranges per stream — since ruling 270, the **floor** of the credit-derived ceiling |
| `REASSEMBLY_MIN_CONFORMING_FRAME` | 1024 B — the divisor: tolerance = max(floor, `W / P` + 1) (ruling 270) |

**[RATIFIED 2026/08/15 — ruling 94]** **slither implements (b), and
allocates lazily — and the level this mandate is stated at is not the
level its own worked example computes.** The mandate is *per stream*; the
example is *per connection* — "a 1 MiB span" is `INITIAL_MAX_DATA`, while
the per-stream window is `INITIAL_MAX_STREAM_DATA` = 256 KiB. Take (a)
literally at the level the mandate names and a receiver eagerly allocates
256 KiB per open receive half: with `INITIAL_MAX_STREAMS_UNI` = 128, that
is **32 MiB** allocated against 1 MiB of credit — a 32× amplification of
exactly the class this section exists to close, produced by following
this section's own admissible option.

Both bounds are real and they are reconciled by never allocating ahead of
arrival — within ruling 253's accounting bound, stated for option (b):
allocated **capacity** stays ≈ the arrived span, its per-stream ceiling
≈ the advertised credit; shrink-at-quiescence and capped growth both
qualify (the ruling's admissible mechanisms), a bare doubling policy
holding ~1.5 × credit does not, and the capacity observable below is
what enforces this. The connection-level credit is what protects memory; the
per-stream bound is what keeps any one stream's metadata proportional.
Buffering only what has arrived makes total buffered bytes across **all**
streams bounded by the advertised connection credit, and (b)'s
coalesce-on-insert keeps each stream's range count bounded independently.
An implementation of (a) that allocates the span up front satisfies the
letter of the per-stream mandate and breaches the memory guarantee the
mandate is for; it is admissible only if its allocation is also bounded
connection-wide.

*A test for this must assert allocated **capacity**, not bytes received.*
An eager per-stream allocator receives few bytes and passes a
bytes-received assertion for free.

**Consumption, defined.** Consumption is **the application taking bytes
out of the connection core** — a `read()` draining the contiguous prefix,
a message or datagram claimed by its verb (§16.4), or a retirement
true-up (§10.3). No unbounded intermediate queue may exist between core
and handle: message and datagram payloads stay accounted inside the core
(or its flow-control ledger) until the handle takes them, and reliable
stream or message data MUST NOT be droppable under the shell's
non-blocking delivery policy (§16.8) — §16.4's pull model is what makes
both properties implementable.

### 10.7 Exemptions

- **DATAGRAM frames are flow-control-exempt** (they are
  congestion-controlled instead, §11.3).
- **Retransmissions of the same stream bytes consume no new credit** —
  credit accounts the stream's offset high-water mark, not bytes on the
  wire.

## 11. Datagrams

### 11.1 Contract

The DATAGRAM frame (§8.4) is the unreliable path: no delivery promise, no
ordering promise, no retransmission, **no sequence identity at all**
(applications needing one embed their own). Datagrams are

- **ack-eliciting**: the carrying packet is tracked and acknowledged — an
  ACK confirms the packet arrived, not that the datagram was delivered to
  the application (§11.5);
- **congestion-controlled**: they count in flight and the admission gate
  applies (§14.5), but loss never retransmits them;
- **flow-control-exempt**: they consume no MAX_DATA credit (§10.7).

### 11.2 Size bound

| Constant | Value |
|---|---|
| `MAX_DATAGRAM_PAYLOAD` | 1169 B (= `MAX_PLAINTEXT` − 1: a type-`0x30` frame's one type byte, data to the end of the plaintext) |

A datagram never spans packets.

### 11.3 Queues

| Constant | Value |
|---|---|
| `DATAGRAM_SEND_QUEUE` | 64 datagrams |
| `DATAGRAM_RECV_QUEUE` | 64 datagrams |

Both queues are bounded by **count**, discipline **drop-oldest with the
newest always accepted** (the arriving or newly-sent datagram always
enters; the oldest queued is evicted to make room). Count-not-bytes is a
deliberate divergence from quinn's byte bounds (1.25 MB receive / 1 MiB
send), argued: the worst case is crisp — 64 × 1169 B ≈ 73 KiB per queue
per connection — the entry count is bounded (a byte bound admits
allocation-churn amplification from a tiny-datagram flood: 1.25 MB of
one-byte datagrams is 1.25 million queue entries), and datagrams only
arrive from the authenticated peer. The constants are
ratified-but-revisitable. Both queues, their eviction discipline, and the
drop counters live **in the connection core**, not the shell (§16.4) —
the bound is protocol state, not a delivery detail.

### 11.4 Oversize rules

- **Send**: a payload > `MAX_DATAGRAM_PAYLOAD` returns
  `DatagramError::TooLarge` at the handle, before any queue.
- **Receive**: an oversized DATAGRAM frame is **impossible by
  construction** — a frame's data lies inside one sealed packet's
  plaintext ≤ `MAX_PLAINTEXT`, and a length field overrunning the
  plaintext is a structural failure (§8.2) — so no receiver oversize rule
  exists; the connection-fatal case in quinn's model is unrepresentable
  here.

### 11.5 Drops are counted

Every queue-overflow drop — send-side eviction and receive-side
eviction — increments a counter surfaced on the `slither::frames` trace
target (§18.2); the counters are core state (§11.3, §16.4), so the trace
is core behaviour, not a shell detail. A silent drop is a known
operability weakness of the
precedent and is deliberately not copied.

## 12. ACK

### 12.1 Range semantics

The ACK frame (layout §8.4) acknowledges received packet counters as
descending ranges, RFC 9000 §19.3.1 semantics in varints:

- the first block covers `largest − first_range ..= largest`;
- for each subsequent `(gap, range)` pair, with `prev_smallest` the
  smallest counter of the preceding block: the block's largest is
  `prev_smallest − gap − 2`, and the block covers
  `block_largest − range ..= block_largest`;
- a block descending below counter zero is structural failure (§8.4).

`ack_delay` is raw microseconds as a varint — no exponent scaling (there
is no negotiation to carry an exponent, and immediate or 25 ms delays fit
1–4 bytes). No ECN counts exist (§19).

### 12.2 Derivation — fused to the replay window

An ACK is derived from the replay window's snapshot (greatest + bitmap,
§7.2) — the single received-packet record; there is no second tracker.
Because the 2048-bit worst case (alternating) no longer fits one packet,
construction emits ranges **newest-first, descending**, truncating at
`MAX_ACK_RANGES` pairs or at packet capacity, whichever binds — the
dropped oldest ranges are exactly the ones prior ACKs most likely already
carried.

| Constant | Value |
|---|---|
| `MAX_ACK_RANGES` | 64 — the cap on `range_count` (the `(gap, range)` pairs; at most 65 blocks including the first) |

A received ACK with `range_count` > 64 is malformed — a structural
failure of §8.2's class (nothing from the packet applied; CLOSE with
`PROTOCOL_VIOLATION`).

### 12.3 `ack_delay`

Measured from the arrival of the packet bearing `largest` to the emission
of the ACK, in microseconds. When the window's largest counter was not
frame-seen (a keepalive's counter, §7.5), `ack_delay = 0`. The RTT
estimator subtracts the peer's `ack_delay` capped at `MAX_ACK_DELAY`
(§13.1).

### 12.4 Delayed-ACK policy

**[RATIFIED 2026/08/14]** The old immediate-ACK-per-packet policy is replaced by
QUIC's default; congestion control now consumes ACK timing, and streams
make 1:1 ACK traffic a real reverse-path cost, while 25 ms is already the
PTO formula's assumption — the change is self-consistent. The cost is one
more named timer and slightly laggier RTT samples. **[AMENDED 2026/08/18
— ruling 271]** This paragraph ended *"immediate-ACK remains the
conservative fallback if the Appendix B timing obligations disappoint"*,
written when the emission point was the every-2nd trigger itself, one
step up from immediate. Ruling 271 moved emission to the drain boundary,
so the ladder now has two steps back: the pre-271 emission point (build
the ACK inside the receive that crossed the threshold — ruling 271's
control run, measured at 33.6 % of all wire datagrams and −30 %
throughput) is the conservative fallback, and per-packet immediate-ACK
is a step further behind it. Both are cadence regressions, not
correctness fixes; the timing obligations themselves did not move.

| Constant | Value |
|---|---|
| `MAX_ACK_DELAY` | 25 ms |
| `ACK_COALESCE_MAX` | 32 |

- **[AMENDED 2026/08/18 — ruling 271]** An ACK becomes **due** after every
  **2nd** ack-eliciting packet. It is **emitted** at the earliest of: any
  outgoing packet built for another reason, which it rides (third bullet
  below); the end of the receiver's current **receive drain**;
  `ACK_COALESCE_MAX` unacknowledged ack-eliciting packets; or the `AckDelay`
  timer, armed at `MAX_ACK_DELAY` on receipt of the first unacknowledged
  ack-eliciting packet.

  A **receive drain** is one pass of the shell's event loop over everything
  the substrate has already delivered; it ends when no datagram is ready
  (§16.5). A sans-io core cannot observe it and does not have to — the drain
  boundary reaches the core as an `AckDelay` armed at `now`, which by §16.5
  is due at `now` and therefore fires on the first pass with nothing else
  ready. The **normative reading**: within one receive drain, in-order
  traffic draws at most **one** ACK emission per `ACK_COALESCE_MAX`
  ack-eliciting packets — for any burst under the valve, exactly one, at
  the boundary. Distinct drains may share an `Instant` (a paused or coarse
  clock makes this ordinary); the bound is per drain, not per clock
  reading. The out-of-order rule (next bullet) is the **sole exception**
  that emits outside this cadence: its job is a loss signal, and
  coalescing it would blunt §13.2's detection.

  This replaces *"an ACK is owed after every 2nd ack-eliciting packet …
  whichever first"*, under which the ACK was built inside the receive that
  crossed the threshold. Because the driver delivers one datagram per loop
  turn, that put **one ACK-only datagram on the wire for every two data
  datagrams** — 33.6 % of all wire traffic, measured, against quinn's 1.68 %
  for the same workload, and ≈3.69 µs of a 14.11 µs per-data-datagram budget
  on both sides combined (`round42-G`, `round42-H`). Neither `MAX_ACK_DELAY`
  nor `ACK_ELICITING_PER_ACK` changes value or job; only the emission point
  moved.
- An ACK is owed **immediately** on out-of-order arrival: an ack-eliciting
  packet whose counter is not exactly one greater than the window's
  previous greatest (it opens, fills, or sits inside a gap). The first
  ack-eliciting packet of a session has no previous greatest, so the rule
  applies vacuously and yields an immediate ACK — harmless, and it seeds
  the peer's RTT estimate early.
- An owed ACK rides the next outgoing packet (packing order §8.5); if none
  is pending, a standalone ACK packet is generated. Pure-ACK packets are
  sealed `seal_quiet` (§7.4), are not ack-eliciting (no ACK-of-ACK loops),
  are never tracked for loss, and bypass the congestion window (§14.5).
  (**[Ruling 271]** This is now the *first* of §12.4's emission triggers
  rather than a convenience: a due ACK riding a packet that already exists
  is the whole reason coalescing does not delay a bidirectional flow.)
  **[Scope — the one packet this does not name.]** **[AMENDED 2026/08/18
  — ruling 258]** The contested-connection probe (§7.5, §7.3) is built
  before the pump's ordinary packets and carries only the PING and the
  owed path frames (§16.5). An owed ACK does **not** ride it; it rides
  the first ordinary packet behind it, in the same pass, and waits for
  the budget where §7.3 ranks it (rank 5, under the probe and both path
  frames). Two reasons, and the second is load-bearing: the probe exists
  to ask one question and is sized to it, and §14.5's exemption for *"the
  probe's packet as built"* is earned by a piggyback bounded at 18 B,
  which an ACK frame — up to `MAX_ACK_RANGES` ranges — is not.

### 12.5 Processing a received ACK

- **Bounded intersecting processing**: a received ACK is intersected with
  the sender's in-flight set, never materialised into the counters its
  ranges imply — a wire-legal ACK whose 64 ranges span millions of
  counters costs O(in-flight × range_count), never a multi-megabyte
  expansion.
- An ACK whose `largest` exceeds the highest counter this session has
  sealed is **ignored whole** — the frame applies as a no-op with a trace;
  the packet's other frames still apply (§8.2's parse/apply split). This
  is a **deliberate divergence** from RFC 9000 §13.1's
  SHOULD-treat-as-`PROTOCOL_VIOLATION`: under bounded intersecting
  processing the forged-future ACK is already harmless, and the no-op
  keeps the failure local.
- Newly acknowledged packets clear their frames from the in-flight set and
  feed recovery (§13.2) and the congestion controller (§14.2). Duplicate
  acknowledgment of a counter is a no-op.

## 13. Loss recovery

Per connection (a connection has exactly one session — §7.8), over the
ack-eliciting sent-packet map. RFC 9002's shape throughout.

### 13.1 RTT estimation (RFC 9002 §5)

The estimator keeps `latest_rtt`, `smoothed_rtt`, `rttvar`, and `min_rtt`.
An ACK yields an RTT sample when its `largest` is newly acknowledged and
at least one newly acknowledged packet is ack-eliciting.
**[CLARIFIED 2026/08/15 — ruling 138]** The second clause is **vacuous in
slither and MUST NOT be implemented**: §13.5 says non-ack-eliciting
packets are never inserted into the sent-packet map, so every packet an
ACK can newly acknowledge is ack-eliciting. The wording is RFC 9002
§5.1's, carried across from a design that tracks both kinds. It is
recorded rather than deleted because an implementer reading this
paragraph as exhaustive would otherwise build the non-ack-eliciting
tracking *in order to evaluate a condition that is always true*. Note
also that "newly acknowledged" is load-bearing in the first clause: if
this ACK's `largest` was already acknowledged by an earlier ACK, there is
**no** sample, even when the frame newly acknowledges other packets.
First sample:
`smoothed_rtt = latest_rtt`, `rttvar = latest_rtt / 2`,
`min_rtt = latest_rtt`. Later samples: `min_rtt = min(min_rtt, latest)`;
the peer's `ack_delay`, capped at `MAX_ACK_DELAY`, is subtracted only when
doing so does not push the sample below `min_rtt`; then
`rttvar = ¾ · rttvar + ¼ · |smoothed_rtt − adjusted|` and
`smoothed_rtt = ⅞ · smoothed_rtt + ⅛ · adjusted`. Before any sample the
estimator seeds from `K_INITIAL_RTT` with `rttvar = K_INITIAL_RTT / 2`.

| Constant | Value |
|---|---|
| `K_INITIAL_RTT` | 333 ms |
| `K_GRANULARITY` | 1 ms |

The RTT estimator is **connection-scoped**: it survives roaming
(a path property), but as a **prior**, not a fact — it seeds the new
path's first PTO and is corrected by the next sample (§14.6).
One exception to `min_rtt`'s monotonicity: on a roam (§7.3), `min_rtt` is
**re-seeded from the first post-roam sample** — it MUST be allowed to
rise, or an old short path pins the PTO floor under a new long one and
manufactures spurious (cwnd-exempt, budget-bound) probes for the
connection's remaining life.

### 13.2 Ack-based loss detection (RFC 9002 §6.1)

A tracked packet is declared lost when a later packet in its space has
been acknowledged **and** either:

- **packet threshold**: it is `K_PACKET_THRESHOLD` = 3 or more counters
  below the largest acknowledged; or
- **time threshold**: it was sent
  `loss_delay = max(9/8 · max(smoothed_rtt, latest_rtt), K_GRANULARITY)`
  or more before the acknowledgment arrived.
  **[AMENDED 2026/08/16 — ruling 141]** This read "more than
  `loss_delay`", a strict `>`, which contradicts the arming rule two
  sentences below: the `Loss` timer arms at `time_sent + loss_delay`, so
  at the firing instant a packet's age *equals* `loss_delay` and a strict
  comparison declares nothing. The walk then re-arms at the same instant
  and the driver spins — ruling 131's "a timer that fires and declares
  nothing", reached by a second route in this same section. The
  comparison is inclusive, as RFC 9002 §6.1.2 has it.

Survivors inside the threshold arm the `Loss` timer at
`time_sent + loss_delay` (**minimum across those survivors**).
**[AMENDED 2026/08/15 — ruling 131]** This parenthetical read "minimum
across in-flight packets", which ranges over the whole sent-packet map
including entries **above** `largest_acked` that this walk does not judge
at all. The two readings diverge whenever anything newer than
`largest_acked` is outstanding — the ordinary case during a transfer —
and the wide one arms a timer that fires and declares nothing, or, if an
implementation then acts on it, declares recent packets lost. The
subject of the sentence is the survivors; the parenthetical now agrees
with it, as RFC 9002 §6.1.2 does. Lost packets'
frames re-queue by retransmission class (§8.7); the lost packet's bytes
leave `bytes_in_flight`, and the loss feeds the congestion controller once
per episode (§14.3).

### 13.3 Probe timeout (RFC 9002 §6.2)

```
PTO = smoothed_rtt + max(4 · rttvar, K_GRANULARITY) + MAX_ACK_DELAY
```

anchored at the last ack-eliciting send, doubled per consecutive
unanswered probe (`2^pto_count`), capped at `PTO_BACKOFF_CAP` = 2³
**[AMENDED 2026/08/17 — ruling 254]**.
`pto_count` resets to 0 whenever any packet is newly acknowledged. The
probe train is ended by liveness (`D_eff` — under symmetric loss and,
since the anchor is the receive clock, under asymmetric loss too, §7.4).
The cap is sized to the v1/default 25 s window, not to overflow
**[AMENDED 2026/08/17 — ruling 254]**: at the inherited 2⁶ the later
rungs could not fire inside 25 s at any warm RTT, silently converting the
train's tail from probing into waiting — measured at 50 % sustained loss,
transfers timed out at 2⁶ that complete at 2³, at zero observed
honest-path cost (every virtual-time budget in the suite sits at ≤ 3
doublings). At 2³ the whole ladder fits inside the v1 `DEAD_TIMEOUT` and
liveness still decides. A shorter custom profile may end the train sooner;
ruling 282 deliberately does not retune PTO. The survival envelope the v1
profile buys, stated: under sustained random loss the probe cadence never
thins beyond 8 × PTO, so completion degrades gracefully toward the
liveness verdict rather than cliffing — an Appendix B obligation pins
the ladder shape and a ≥ 30 % completion floor at 50 % loss (the audit's
E5a/E5b). The ending is unconditional, and that is
ruling 33's doing: a probe is ack-eliciting, so the *first* probe arms the
death deadline even when the connection has marked nothing since its last
receive, and no later probe re-arms it. A probe train therefore always
terminates within `D_eff` of the last authenticated receive; it can
neither defer death nor run in a black hole unobserved.

**The `Pto` timer is armed only while at least one ack-eliciting packet
is in the sent map** (RFC 9002 §6.2.1) **and while §7.3's amplification
budget admits a probe datagram** **[AMENDED 2026/08/17 — ruling 249]**;
when the map empties it is disarmed, and when the `Loss` timer is armed it
takes precedence (§16.5).
Without the first precondition an idle connection self-sustains a probe
train — PTO fires, the bare PING is ack-eliciting, the peer ACKs,
`pto_count` resets, the timer re-arms — at ~20 packets/s against the 10 s
keepalive cadence, defeating §16.5's timer economy. Without the second, a
saturated backoff at a closed budget re-arms itself in the past forever:
the anchor moves only at an ack-eliciting send, the increment advances the
deadline only until `pto_count` saturates, and a firing that can emit
nothing changes neither — so the one driver every connection shares spins
until `D_eff` (ruling 249's measured livelock, reachable from any
roam, which zeroes the budget, §13.6). The second precondition gates the
**announcement**, not the state: sent map, `pto_count` and anchor are
untouched while the budget is closed, the connection's `Timeout` falls to
the next representable armed timer — ordinarily `Liveness` at the latest,
or `None` if every logical deadline is beyond the clock horizon — and the
deadline is announced again at the authenticated, window-fresh receive that
refunds the budget (§7.2, §7.3; every receive recomputes the `Timeout`, so
no dedicated re-arm machinery exists). This is §16.4's `Contested` principle
— a deadline is never announced for output that cannot leave — applied to
`Pto`.

| Constant | Value |
|---|---|
| `K_PACKET_THRESHOLD` | 3 |
| time threshold | 9⁄8 |
| `PTO_BACKOFF_CAP` | 2³ |

### 13.4 Probe content

A firing PTO sends one ack-eliciting packet: pending retransmittable
frames oldest-first if any exist, else a bare PING. The one-packet,
pending-oldest-first-else-PING content rule is a **deliberate
simplification** of RFC 9002 §6.2.4, which sends new data before old and
up to two datagrams.

**[RATIFIED 2026/08/16 — ruling 221]** There is a **third** case, and the
two-case rule above was read as exhaustive because §8 reads every list
that way. On an **unvalidated** address the probe carries §7.3's
`PATH_CHALLENGE` — the packet is built for the PTO, and §8.7's standing
obligation puts the challenge on it exactly as it does on any other packet
the budget admits. No PING is owed beside it: `PATH_CHALLENGE` is
ack-eliciting (§8.3), so the "else" arm never fires. This is the **only**
mechanism by which a lost challenge is asked again — §8.7 keeps both path
frames out of loss recovery, so on a connection with nothing else to say
the PTO is the sole timer that can re-ask, and without it §8.7's
*"sent once and lost forever"* is what the implementation does. The probe
remains subject to §7.3's budget (below), which is what bounds the
re-offer: while the budget has no room for the 39-byte challenge datagram
the `Pto` deadline is not announced at all **[AMENDED 2026/08/17 — ruling
249]** (§13.3) — nothing fires and nothing is emitted, and the session
still dies at `D_eff` as §7.3 intends.

Probes are sealed
`seal_quiet` (liveness-neutral, §7.4) **and** exempt from the congestion
admission gate (§14.5) — two independent properties of the same send, for
different reasons (a partitioned session must still die; a black-holed
path must stay probeable). Probes are **not** exempt from §7.3's
anti-amplification budget on an unvalidated address.

### 13.5 Frames, never packets

A lost packet is never retransmitted as a packet: its still-needed frames
are re-framed into new packets under fresh counters (§8.7), STREAM ranges
split or merged freely. The sent-packet map holds, per ack-eliciting
counter: send time, the frame identities aboard (stream ranges, credit
frame identities, RESET_STREAM, PING, DATAGRAM markers), and the packet's
**size in bytes** — the `size` field feeding `bytes_in_flight` (§14.5).
Non-ack-eliciting packets are never inserted.

### 13.6 What resets when — the roam seam

Roaming is the **only** recovery seam: there is no rekey, and a
connection's one session lives as long as the connection (§7.8).

**Roaming** (§7.3): the sent map is **kept** — ACKs for packets in flight
to the old address still resolve, and `bytes_in_flight` remains consistent
with the retained map; loss detection continues undisturbed, and PTO
**state** carries across untouched — though its deadline is announced
again only once the zeroed budget re-admits a probe (§13.3, ruling 249).
The
congestion controller resets with the **pre-roam flight fenced off**
(§14.6): packets sent before the roam still resolve for loss and
retransmission, but feed no congestion event, no persistent-congestion
walk, no RTT sample, and no `app_limited` window growth — the
roam-triggering path break must not be read as fresh-path congestion (the
break *is* §14.4's predicate: two far-apart losses with nothing acked
between; unfenced, every roam would start at `MINIMUM_WINDOW` instead of
`INITIAL_WINDOW`). **[AMENDED 2026/08/16 — ruling 172]** Those four
fences are **not** all read from one stamp: `recovery_start` (set to the
roam instant, never cleared) fences the congestion event and the
`app_limited` growth, and `path_gen` fences the RTT sample and the
persistent-congestion walk. §14.6 states the assignment in full and why
the two mechanisms are behaviourally indistinguishable here. The RTT
estimator is treated as suspect-but-kept, with
`min_rtt` re-seeded from the first post-roam sample (§13.1).
(Consequence: immediately after a roam,
`bytes_in_flight` may exceed the fresh initial window; the admission gate
then blocks new sends until old-path packets are acknowledged or declared
lost — a bounded stall of at most one loss-detection/PTO cycle once §7.3's
budget re-admits a probe, which the first authenticated receive at the new
address funds; until then the `Pto` deadline is suppressed (§13.3, ruling
249), and the path stays probeable by the PTO exemption within that
budget.)

**Every per-connection reset on this seam, in one list.**
**[RATIFIED 2026/08/16 — ruling 173]** This section's title claims a
scope, and a list read as exhaustive had better be one — the previous
text covered recovery and congestion state only, while §7.3 mandates a
reset on the *same* seam that it never mentioned. An implementer
building `on_roam()` from the old list would touch the controller and the
sent map and let the amplification budget carry the **old** address's
credit to the new one, funding sends to a fresh attacker-supplied address
with credit earned from the genuine peer — the reflector §7.3 exists to
prevent, reconstructed out of a missing line. Narrowing the title instead
was considered and declined: the omission is invisible until someone
builds against it, so the list is the thing that has to exist.

| On a roam | What happens | Where it is ruled |
|---|---|---|
| congestion controller | **reset** to `INITIAL_WINDOW` / `ssthresh = u64::MAX` | §14.6 |
| `recovery_start` | **set to the roam instant** — never cleared | §14.6, ruling 139(b) |
| path generation (`path_gen`) | **incremented** — the pre-roam stamp is what fences the RTT sample and the persistent-congestion walk | §14.6, ruling 172 |
| amplification byte counters (sent, received) | **reset to zero**, and the address becomes unvalidated | §7.3 |
| the outstanding **challenge** | **re-drawn** at the roam — eight fresh bytes from the connection's §16.6 sub-seed; any earlier challenge is discarded and a `PATH_RESPONSE` echoing it validates nothing thereafter | §7.3, §16.6, rulings 208, 210(b) |
| an outstanding **`PATH_RESPONSE` obligation** (one we owe the peer) | **kept** — it answers the peer's question about *its* path, which our endpoint moving does not change; like all output it is sent to the new endpoint, and it is capped by the re-armed budget like everything else | §7.3, §8.4, ruling 208 |
| sent-packet map | **kept** — in-flight ACKs still resolve, `bytes_in_flight` stays consistent | §13.5, above |
| PTO / loss detection | **state undisturbed** — sent map, `pto_count`, anchor and `loss_time` carry across, nothing is reset; the `Pto` **announcement** is budget-gated (§13.3, ruling 249), so it is suppressed from the roam — which zeroes the budget, four rows up — until the first qualifying receive | §13.3, §13.4 |
| `Keepalive` / `PersistentKeepalive` announcements | **suppressed** — the roam zeroes the budget four rows up, so §7.5's announce-gate withholds both deadlines until the first qualifying receive, the death-clock backstop announcing in their stead (ruling 265); `last_send`, the passive debt and §7.4's arming bit carry across untouched | §7.5, §7.3 |
| RTT estimator | **kept as a prior** (suspect-but-kept); `min_rtt` re-seeded from the first post-roam sample | §13.1 |
| pending contested mark and its `probe_floor` | **kept intact, floor unchanged** — a roam changes the pending probe's budget prospects, not the question it asks | §7.5, ruling 176 |
| `Contested` deadline, once armed | **undisturbed** — it is armed at the probe's transmission and disarmed only by an ACK covering `probe_floor` | §16.5, §7.5 |
| §7.2's anti-replay window | **not reset** — stated out loud rather than left clean-by-construction: one session has one never-reset counter space (§7.7), so a roam cannot rewind it and a replayed packet stays a replay across the seam | §7.2, §7.7 |
| the session keys, the counter space, stream and credit state | **untouched** — roaming is a path change, not a session change (§7.8) | §7.7, §7.8 |

## 14. Congestion control

### 14.1 The controller seam

Congestion control sits behind a small trait (quinn-proto's shape), wired
at the three existing recovery mutation points — packet sent, packets
newly acknowledged, loss episode:

```rust
trait Controller {
    fn on_sent(&mut self, now: Instant, bytes: u64);
    fn on_ack(&mut self, now: Instant, sent_time: Instant, bytes: u64, app_limited: bool);
    fn on_congestion_event(&mut self, now: Instant, sent_time: Instant,
                           is_persistent: bool, lost_bytes: u64);   // once per loss episode
    fn window(&self) -> u64;
}
```

NewReno is the one v1 implementation; CUBIC and BBR are pure additions
behind the trait later (§19). Nothing congestion-related appears on the
wire — the wire is deliberately CC-agnostic.

### 14.2 NewReno

| Constant | Value |
|---|---|
| `INITIAL_WINDOW` | 12 000 B (= min(10 × 1200, max(2 × 1200, 14 720)) — RFC 9002 §7.2 at `MAX_DATAGRAM` = 1200) |
| `MINIMUM_WINDOW` | 2 400 B (= 2 × 1200) |
| `LOSS_REDUCTION_FACTOR` | 0.5 |

`ssthresh` starts at `u64::MAX`. **Slow start** (cwnd < ssthresh): cwnd
grows by the bytes newly acknowledged. **Congestion avoidance**: integer
appropriate-byte-counting — accumulate acknowledged bytes and add one
`MAX_DATAGRAM` to cwnd each time the accumulator exceeds cwnd (one MTU per
RTT, no floating point).

### 14.3 The recovery period — one cut per episode

On a congestion event (any loss of an ack-eliciting packet):
`cwnd = max(cwnd × 0.5, MINIMUM_WINDOW)`, `ssthresh = cwnd`, and the
recovery period starts at the event. Subsequent congestion events for
packets **sent before** the recovery period started are ignored — one
loss burst produces exactly one window cut. Symmetrically,
**acknowledgments of packets sent before the recovery period started do
not grow the window** (RFC 9002 §7.3.2) — the same
`sent_time ≤ recovery_start` test gates both the event and the growth;
without it, the pre-cut flight's ACKs keep inflating cwnd through the
recovery they triggered. `on_congestion_event` fires
once per loss episode (after the full lost-packet scan), never once per
lost packet.

### 14.4 Persistent congestion

Computed inside the loss-detection walk (§13.2), not bolted on: if two
ack-eliciting packets sent more than
`persistent_period = PTO × PERSISTENT_CONGESTION_THRESHOLD` apart are both
lost with **no packet acknowledged between them**, and **a prior RTT
sample exists** (the pre-sample `K_INITIAL_RTT` phase never triggers it),
the controller collapses: `cwnd = MINIMUM_WINDOW`, slow start effectively
restarts. `persistent_period` evaluates the §13.3 PTO formula **with
`pto_count = 0`** (RFC 9002 §7.6.1): the backoff is deliberately excluded
so the period is a property of the path, not of the probe count — with
the backoff included, the threshold would run up to 2³× too long and
persistent congestion would never trigger under exactly the sustained
loss it exists to detect. Packets sent before a roam are excluded from
the walk (§13.6, §14.6).

| Constant | Value |
|---|---|
| `PERSISTENT_CONGESTION_THRESHOLD` | 3 |

### 14.5 `bytes_in_flight` and the admission gate

`bytes_in_flight` is the sum of the `size` fields over the sent-packet map
(§13.5) — ack-eliciting packets only. The send-side admission gate:

```
send permitted  iff  bytes_in_flight + candidate_size ≤ cwnd
```

**Exemptions**, exhaustively:

- **PTO probes** (§13.4) — a black-holed path with a full window must
  stay probeable; the exemption is orthogonal to, and coexists with, the
  probe's liveness-neutral `seal_quiet`.
- **The contested-connection probe** (§7.5) — same reasoning, one step
  sharper: the probe carries its own `K_eff` deadline, and a
  gate that could delay it past that deadline would turn a congestion
  answer into a liveness verdict. As with the PTO probe, the exemption is
  from **admission only**: the probe is ack-eliciting, so §13.5 records
  it in the sent map and its bytes count in `bytes_in_flight` like any
  other tracked packet (ruling 43). Exempting it from the gate without
  counting it would have put a packet in flight that loss recovery could
  not see and §17.5's cwnd bound did not cover. **[AMENDED 2026/08/17 —
  ruling 250]** The exemption covers the probe's packet **as built**: a
  probe that coalesces the owed `PATH_RESPONSE`/`PATH_CHALLENGE` (§7.3)
  remains exempt — the packet exists because the probe demanded it, the
  piggyback adds at most 18 B of frames, and gating the merged packet
  would starve the challenge at collapsed cwnd exactly where a roam makes
  it owed. (The dedicated path-frame packet the coalescing replaces was
  cwnd-gated; its work now rides the exempt probe.) **Scope of "as
  built".** **[AMENDED 2026/08/18 — ruling 258]** The probe's packet
  carries the PING and the owed `PATH_RESPONSE`/`PATH_CHALLENGE`, and
  **nothing else** — no STREAM fill, no DATAGRAM, no credit grant, no
  ACK. That is what makes the 18 B bound above a fact rather than a
  hope, and it is the whole of what this exemption was widened to cover.
  §8.5 orders the frames a packet carries; it does not decide which
  packet a frame rides in, and its permission for a STREAM fill to sit
  beside a PING is about the **PTO** probe (§13.4), which is built in
  the ordinary pump pass and does carry fill. Application data that is
  ready while a probe is pending leaves in the packet **behind** the
  probe, on the same pass (§7.3, ruling 250(i)), where this section's
  gate applies to it normally.
- **Non-ack-eliciting control packets** — pure ACKs, CLOSE, keepalives —
  are never tracked in flight and never gated.

DATAGRAM frames are congestion-controlled: they count in flight and the
gate applies (a queued datagram waits for window room), but loss never
retransmits them (§11.1).

**The exemptions are cwnd-scoped only.** On an unvalidated address — a
fresh roam target or msg1-source anchor — §7.3's anti-amplification
budget binds **all** output, the exempt classes above included: probes,
pure ACKs, CLOSE, and keepalives are free of the congestion window, never
of the budget. **[AMENDED 2026/08/16 — rulings 168, 171]** Two
consequences follow and are stated here so this list is not read as the
whole story. First, "unvalidated" is a **state with an exit**: §7.3's
**challenge** disarms the budget when the address echoes it (**[AMENDED
2026/08/16 — ruling 208]**, superseding the `validation_floor` ACK
predicate), and these exemptions then face no cap at all. Second, while
the budget *is* armed and admits less than is owed, §7.3's priority order
decides which exempt class goes first — a **pending contested probe**
ahead of everything **but CLOSE** (ruling 186; this clause read "ahead of
everything" and did not carry ruling 186's amendment across — §7.3's list
is the normative one), because a probe the budget could delay past its own
deadline would convert a scarce budget into a liveness verdict, which is
the same argument that earns it the cwnd exemption above. `PATH_RESPONSE`
and `PATH_CHALLENGE` rank immediately below the probe and above the pure
ACK; the once-flagged interaction between the probe's rank and the
challenge's was closed by ruling 215 and resolved into coalescing by
ruling 250 — the probe carries the owed path frames when room admits —
read the order in §7.3, not here.

**`app_limited`** (quinn's mechanism, pinned): the send path maintains an
application-limited flag — set when the sender runs out of queued data
with cwnd headroom remaining — and **records it onto each sent packet**;
`on_ack` reads the acknowledged packet's recorded flag, and when it is
set the controller does not grow the window on that acknowledgment — idle
connections earn no phantom window. The hostile-peer reasoning, stated: a
peer controls ACK arrival timing and can therefore choose *when* the
predicate is consulted, but the flag is recorded at send time by *us*, so
ACK-timing games cannot un-set it; the dangerous direction is a window
that grows while the application is idle and later discharges as a burst
— the recorded-per-packet form bounds it, and §7.3's budget caps what any
accrued window can emit at an unvalidated address.

### 14.6 The roam reset seam

**[RATIFIED 2026/08/14, in reduced form — the rekey seam is deleted with
§7.6; the roam seam stands exactly as ratified.]** The controller resets
to initial state (cwnd = `INITIAL_WINDOW`, ssthresh = `u64::MAX`) on the
one seam; the RTT estimator survives it as a prior (§13.1):

- **roaming** — on the authenticated address move (§7.3): a new path, no
  continuity evidence; the sent map is *kept* (§13.6) but the window is
  not. **The recovery-period marker is set to the roam instant — not
  cleared** — and packets sent before the roam are fenced from the fresh
  controller (§13.6): they resolve for loss and retransmission but feed
  no congestion event, no persistent-congestion walk, no RTT sample, and
  no `app_limited` growth (RFC 9000 §9.4's per-path separation; quinn's
  path-generation stamping).
  **[AMENDED 2026/08/15 — ruling 137]** *One marker cannot serve those
  four fences and the text must not be read as saying it does.* The
  recovery-period marker serves the congestion event and `app_limited`
  growth, because §14.3 already gates both on it. It **cannot** serve the
  RTT fence: `recovery_start` is also set by every ordinary congestion
  event, so an implementation reusing it would suppress RTT sampling
  after every normal loss episode — silently, and permanently on a lossy
  path. The mechanism that works is the one this bullet already names:
  **path-generation stamping**. `SentPacket` therefore carries a `u32`
  path generation from slice 5 onward, held at 0 until roaming exists.
  **[AMENDED 2026/08/16 — ruling 172]** *The split is 2/2, and the four
  fences are assigned individually here because three texts previously
  gave three answers.* This clause read *"§13.6's fences read that stamp
  rather than the recovery marker"* — plural and unqualified, i.e. all
  four on the stamp — which reversed its own opening two sentences.
  Ruling 137's enumeration, for its part, assigned three of the four and
  silently dropped the persistent-congestion walk. The assignment, in
  full, is the one the code shipped:

  | Fence on a pre-roam packet | Read by |
  |---|---|
  | no congestion event | `recovery_start`, set to the roam instant (§14.3 already gates it) |
  | no `app_limited` window growth | `recovery_start`, same reason |
  | no RTT sample | `path_gen` — `recovery_start` cannot serve it |
  | no persistent-congestion walk | `path_gen` — ruling 137's missing fourth assignment |

  The two readings are **behaviourally identical**, and the reason is
  worth recording so the 2/2 split is not re-litigated as a hybrid that
  reintroduces ruling 137's silent failure: escaping the
  `recovery_start` fence would require `recovery_start` to be
  *clearable*, and ruling 139(b) forbids exactly that — *"`recovery_start`
  is **not** cleared"*; it is only ever assigned `Some(now)` and moves
  **monotonically forward**. Every pre-roam packet therefore satisfies
  `sent_time ≤ roam_instant ≤ every later recovery_start`, so its
  in-recovery test stays true permanently. The defect was
  **documentation, not behaviour** — but all three texts have to say the
  one thing the code does, because §14.6 is where a blind test author
  reads the fences and asserts against them. This is conservative per RFC 9002/quinn
  precedent (a fresh
  controller per path); a reviewer could argue for keeping cwnd across a
  same-NAT port rebind — declined here for want of evidence the path is
  the same. One stated divergence: RFC 9000 §9.4 also resets the RTT
  estimator on a path change; keeping it as a prior is a deliberate
  slither choice (roaming here is the same peer moving, not adversarial
  migration), with the consequence that a roam onto a slower path briefly
  runs on stale `smoothed_rtt` — corrected by the first post-roam sample,
  with `min_rtt` re-seeded so the PTO floor may rise (§13.1).

### 14.7 Explicitly out

No pacing (no sub-RTT wakeups in the v1 shell; a 12 KB initial window
bounds bursts adequately), no ECN (wire and socket work), no CUBIC/BBR
(trait-additive later). All deferred with pointers in §19.

## 15. CLOSE and the connection lifecycle

### 15.1 The CLOSE frame

**[RATIFIED 2026/08/14, amended 2026/08/14]** Deliberate teardown gets a wire signal — without one, a
clean disconnect costs the peer 25 s of liveness wait. The semantics are
minimal by design: no close-ACK, no handshake, no QUIC
transport/application split, a 5 s linger instead of QUIC's 3×PTO
closing/draining bookkeeping. The QUIC-faithful alternative is more state
for little gain at slither's scale. Only the **authenticated, in-seal**
CLOSE exists — nothing unauthenticated can kill a connection; the
reserved cleartext close packet type (`0x04`) stays dead.

| Constant | Value |
|---|---|
| `CLOSE_LINGER` | 5 s |
| close-reply rate | ≤ 1 CLOSE per second |
| `CLOSE_REASON_MAX` | 256 B |

### 15.2 Semantics

- **Local close** — `close(error_code, reason)` (the handle truncates
  `reason` to `CLOSE_REASON_MAX`, §8.4): emit CLOSE (sealed
  `seal_quiet`, §7.4) and enter **closing** for `CLOSE_LINGER`. The
  closing state retains **the seal capability, the receive cipher
  states, and the replay window** (all stream, flow-control, recovery,
  and congestion state may drop immediately — so data unacknowledged at
  `close()` is never retransmitted and data still queued behind the
  congestion window is never sent at all; ruling 47's `acked()` (§16.2)
  is how an application waits for delivery **before** closing, and this
  clause is deliberately unchanged by it): a reply is owed only to an
  **authenticated, window-fresh** inbound packet — never to a packet
  that merely routed by `receiver_index`, which an off-path forger who
  observed the cleartext index could mint — and is sent **to the
  session's endpoint address** (the closing state does not roam; never
  to the triggering packet's source). Replies are capped at one CLOSE
  per second, and **the opening CLOSE is not a reply**
  **[RATIFIED 2026/08/15 — ruling 83]**: the rate clock is unset at
  `close()`, so the first reply owed to an inbound packet is sent at
  once. The cap governs *replies*, and the reply exists so a peer that
  **lost** the opening CLOSE learns of the death — anchoring the clock
  on that opening CLOSE would delay recovery by up to a second in
  exactly the case the mechanism is for. At linger expiry
  (`CloseLinger` timer, §16.5), drop all
  state. The linger's reply rule
  is CLOSE's only reliability mechanism — CLOSE is not ack-eliciting and
  is never retransmitted by loss detection. A CLOSE **received** while
  closing moves the connection to the reply-free draining behaviour
  below: two closing endpoints go quiet rather than ping-ponging replies
  at 1 Hz for the linger.
- **Receiving an authenticated CLOSE**: surface
  `ConnectionLost::PeerClosed { code, reason }`, emit **nothing**, hold a
  brief drain for the same `CLOSE_LINGER` (discarding late packets, no
  replies), then drop all state. **[AMENDED 2026/08/15 — ruling 133, CORRECTED
  2026/08/16 — ruling 146]** *Received stream state is retained for the
  drain — **on both paths**, not only this one.* Ruling 133 read the
  bullet above as licensing a **closing** endpoint to free stream and
  flow-control state at once, on the argument that calling `close()` while
  a receive half holds unread bytes *is* a decision to discard them. That
  argument is sound and the rule it produced was not: `send_settled`
  reports an **absent** half as settled, so freeing the closer's state
  makes `Connection::acked()` answer `Ok(())` over bytes that were never
  acknowledged — precisely the misreport ruling 47 exists to prevent. The
  "may" above stays permissive and slither declines it: both paths retain
  until the linger expires, `read` and `accept_*` behave identically
  whoever closed (§16.2, ruling 128), and `acked()` is honest by
  construction. The no-linger deaths — liveness (§7.4), nonce
  exhaustion (§7.9), `Replaced` (§5.4), endpoint dropped — retain
  nothing, and the consequence is stated rather than left to be found: a
  receiver killed by `D_eff` mid-transfer cannot drain, which is
  honest, because a path that produced no CLOSE produced no finished
  sender either.
- **Protocol violations by the authenticated peer** (§8.2's semantic
  class — `FLOW_CONTROL_ERROR`, `STREAM_LIMIT_ERROR`,
  `STREAM_STATE_ERROR`, `FINAL_SIZE_ERROR` — and its post-AEAD
  structural class, `PROTOCOL_VIOLATION`) get a signalled death instead
  of a silent one: emit CLOSE with the matching code, then linger as for
  a local close. **[RATIFIED 2026/08/14 — one ruling, two homes with
  §8.2.]** The locally surfaced error is
  `ConnectionLost::ProtocolViolation { code }` (§18.1) — a dedicated
  variant, not `LocallyClosed`: "I closed" and "the peer's misbehaviour
  forced a close" are opposite causes with opposite operational
  responses (peer reputation, allow-list demotion, alerting), and
  §18.1's own precedent is that a security signal earns its own variant.
  The addition is enum-level only; the CLOSE frame and the on-wire code
  registry are unchanged.

### 15.3 Error-code registry

| Code | Name | Meaning |
|---|---|---|
| `0x00` | `NO_ERROR` | graceful close |
| `0x01` | `PROTOCOL_VIOLATION` | a semantic violation with no more specific code |
| `0x02` | `FLOW_CONTROL_ERROR` | advertised credit exceeded (§10.5) |
| `0x03` | `STREAM_LIMIT_ERROR` | cumulative stream limit exceeded (§10.4) |
| `0x04` | `STREAM_STATE_ERROR` | a frame for a stream its sender could not touch (§8.4) |
| `0x05` | `FINAL_SIZE_ERROR` | final-size disagreement (§9.5, §9.6) |
| `0x06` | `MESSAGE_OVERFLOW` | **[RATIFIED 2026/08/14 — ruling 52; predicate completed 2026/08/16 — ruling 164]** the receiver-emitted overflow reset of §9.6/§9.8: an unclaimed uni stream reached `MESSAGE_RECV_MAX` **at its highest received offset, with no final size pinned**, while a `recv_message()` claim was pending. The second clause is not decoration: without it this row describes a rule that resets a *conforming* maximum-size message whose FIN is still in flight. Carried in RESET_STREAM's `error_code`, never in CLOSE |
| `0x07`–`0x0f` | reserved | transport-reserved; never sent |
| ≥ `0x10` | application | application-defined codes via `close()` |

### 15.4 The teardown matrix

Every way a connection dies, what is transmitted (mostly: nothing), and
what each side observes:

| Cause | Transmitted | Local surface | Peer's view |
|---|---|---|---|
| liveness — `D_eff` without an authenticated fresh receive (25 s under the v1/default profile; §7.5) | nothing | `ConnectionLost::TimedOut` | its own liveness fires according to its local profile |
| **contested** — a contested-connection probe unanswered: the connection's `K_eff` (10 s under the v1/default profile) after the probe's **transmission** with no ACK covering its probe floor, and authenticated receives may well have been arriving throughout (§7.5, §6.4, rulings 36/41/282) | the probe's packet — the PING, plus the owed path frames when room admits (ruling 250) — at the mark, or at the first instant §7.3's budget admits it, which is also when the deadline arms and when `Contested` is emitted (§7.5, §16.4), and where a pending probe **outranks all other output but CLOSE** to that address (§7.3, rulings 171 and 186 — this cell read "all other output" and did not carry ruling 186's amendment across; §7.3's list is the normative one, and the once-flagged interaction with ruling 208's `PATH_CHALLENGE` was closed by ruling 215 and resolved into coalescing by ruling 250: the probe carries the owed path frames when room admits); **nothing** at the verdict, and **nothing at all** if the mark clears while still pending, which cancels the probe (§7.5, ruling 176) | `ConnectionLost::TimedOut` — the same variant, no new one | **asymmetric.** A healthy peer is unaffected and keeps its side for its own `D_eff`; the peer this case is aimed at has already restarted and holds nothing, and its parked `Intro` is accepted next |
| nonce exhaustion (§7.9) | nothing | `ConnectionLost::NonceExhausted` | liveness |
| local `close(code, reason)` / last-handle drop (§16.2) | CLOSE, then ≤ 1 reply/s for 5 s | `ConnectionLost::LocallyClosed` | `PeerClosed { code, reason }` |
| peer's CLOSE received | nothing (drain only) | `ConnectionLost::PeerClosed { code, reason }` | (it closed) |
| protocol violation by the peer — semantic or post-AEAD structural (§8.2, §15.2) | CLOSE(code), linger | `ConnectionLost::ProtocolViolation { code }` | `PeerClosed { code, reason }` |
| replaced — an `Intro` proving this connection's static was **accepted**; the teardown fires at the replacing `accept()` (§5.4, §6.4) | nothing on the old connection | a fresh `Intro` first, then `ConnectionLost::Replaced` at its `accept()` | (it reconnected; its new connection proceeds) |
| endpoint dropped — every handle gone (§16.3) | nothing | — (the driver stops) | liveness, ≤ its local `D_eff` |

`ConnectionLost::EndpointDropped` is the answer a surviving verb call
receives when the driver has stopped mid-flight (§18.1) — it is a
handle-side observation, not a teardown cause of its own.

**The two `TimedOut` rows are distinct causes sharing one variant, and
that is deliberate.** The liveness row is the plain idle death: `D_eff`
(25 s under the v1/default profile), no authenticated fresh receive, and
both ends reach it at about the same moment when their local profiles
match because both are watching the same silence. The contested row is
neither — it fires at `K_eff` (10 s under the v1/default profile), it
fires *while* authenticated receives may still be arriving (that is the
whole point: the receives are the attacker's replayed harvest, §7.5), and
the peer sees nothing at all.
The application gets one variant because from its side the fact is the
same one — this connection stopped being usable and re-establishment is
a fresh `connect()` (§16.2) — and because a new variant would be wire-
visible policy in an enum §18.1 keeps deliberately small. Operators who
need to tell them apart have `slither::policy`, which carries the mark,
the probe and the verdict (§18.2). The application's only synchronous
hint is the `AcceptError::Stale` that provoked the mark: it does not say
that a live connection is now on a `K_eff` watch, and the spec states
that rather than implying otherwise.

## 16. Object model and the sans-io core

### 16.1 The object model

```text
Endpoint                                   // socket + demux; owns the accept queue
├── connect(addr, static) → Connecting     // Future → Connection
├── accept().await → Intro → Claimed → Proven → Connection    (§6)
└── Connection                             // one Noise session + the frame layer
      ├── SendStream / RecvStream          // per-stream handles (bidi pairs)
      ├── send_message / recv_message      // §9.8 sugar
      ├── send_datagram / recv_datagram    // §11
      ├── acked / SendStream::acked       // delivery confirmation (§16.2)
      └── closed / notified                // the awaitables (§16.2)
```

| Object | Owns |
|---|---|
| `Endpoint` | socket + demux (`receiver_index → Connection`, pending-index table), the stage-0 intro queue + staged-accept verbs, the hint set + the internal tie-break (§6.6–§6.7), all initiator pendings, the timestamp guard, the per-connection replacement basis (§17.4), index minting, the root RNG |
| staged objects (`Intro → Claimed → Proven`) | one inbound initiation's graduated state; each verb is a driver round-trip; drop = silent reject at every stage |
| `Connection` | one Noise session (seal/open, replay window, roaming, liveness, the epoch ratchet), the unified frame layer, recovery + congestion controller, connection-level flow control, the streams table, the datagram queues, the CLOSE lifecycle |
| stream handles | per-stream state: send buffer + un-ACKed ranges + FIN, or reassembler + credit ledger; borrow the connection core through the shell |

**One connection per remote static, endpoint-wide.** **[RATIFIED
2026/08/14, clause flipped per the ratchet-only ruling; amended
2026/08/14.]**
`connect()` to a static with a live `Connection` or an in-flight outbound
connect returns `ConnectError::AlreadyConnected`; an authenticated
inbound initiation whose proven static matches a live connection is, by
definition, a **replacement of that connection, admitted via
`accept()`** (§5.4, §6.4) — never a second concurrent connection. Every
routing rule keys on this invariant; without it, "which connection does
this initiation replace" has no answer.
An outbound connect is "in flight" only while its `Connecting` lives:
dropping that future cancels the attempt, the static returns to NONE, and
an immediate `connect()` to it succeeds (§16.3, ruling 50) — so this
clause never strands an application behind an attempt it has abandoned.
A **staged chain in progress is deliberately not in `connect()`'s list**,
and cannot be: until `authenticate()` the chain's static is merely
claimed, and §6.1 forbids keying anything durable on an unproven claim.
The invariant is held at the other end of that race instead — a proven
static that is PENDING at `accept()` runs §6.7's comparison (§6.4's
PENDING branch): as tie-break loser the `accept()` cancels the pending
and installs in its place; as tie-break winner it installs **no second
connection** at all, returning `AcceptError::Stale` and leaving the
pending to complete. Either way `read_identity()` → `connect()` →
`accept()` on one static still yields exactly one connection, and no
static is ever LIVE and PENDING at once.
WireGuard's model is identical (one `wg_peer` per static). `connect()` to
our own static is out of scope under this rule, which is what makes the
tie-break's equal-statics case unrepresentable (§6.7). This forecloses
multi-connection-per-peer for this wire line.

### 16.2 Shell surface

`TimingProfile` lives at `slither::config::TimingProfile`; it is not
added to the prelude. Construction validates the complete pair before an
endpoint can own it, and configuration accepts only that value object:

```rust
impl TimingProfile {
    pub fn try_new(
        passive_keepalive: Duration,
        dead_timeout: Duration,
    ) -> Result<Self, TimingProfileError>;
}

#[non_exhaustive]
pub enum TimingProfileError {
    // the concrete reasons are §5.7's floor, strict relation, and overflow
}

impl Config {
    pub fn with_timing_profile(self, profile: TimingProfile) -> Self;
}
```

`TimingProfileError` is a dedicated, non-exhaustive construction error.
It does not widen the exhaustive `ConfigError`, and
`with_timing_profile` is infallible because its argument is already
valid. `Config::default()` supplies the 10 s / 25 s v1 profile (§5.7).
The profile is endpoint-wide and immutable; this surface deliberately
offers neither a per-peer override nor a setter on `Connection`.

```rust
impl Endpoint {
    pub fn builder() -> EndpointBuilder;                 // identity, socket/Wire, Config
    pub async fn accept(&self) -> Option<Intro>;         // None = endpoint closed
    pub fn connect(&self, remote: SocketAddr, remote_static: PublicKey)
        -> Result<Connecting, ConnectError>;             // Connecting: Future<Output = Result<Connection, ConnectError>>
}
// staged verbs: §6.2

impl Connection {
    pub async fn open_bi(&self)  -> Result<BiStream, ConnectionLost>;   // .split() → the pair
    pub async fn open_uni(&self) -> Result<SendStream, ConnectionLost>;
    pub async fn accept_bi(&self)  -> Result<BiStream, ConnectionLost>; // .split() → the pair
    pub async fn accept_uni(&self) -> Result<RecvStream, ConnectionLost>;
    pub async fn send_message(&self, msg: &[u8]) -> Result<(), MessageError>;
    pub async fn recv_message(&self) -> Result<Vec<u8>, ConnectionLost>;
    pub fn send_datagram(&self, data: &[u8]) -> Result<(), DatagramError>;
    pub async fn recv_datagram(&self) -> Result<Vec<u8>, ConnectionLost>;
    pub async fn acked(&self) -> Result<(), ConnectionLost>;   // delivery confirmation (rulings 47, 54)
    pub async fn close(&self, code: u64, reason: &[u8]);
    pub fn set_persistent_keepalive(
        &self,
        interval: Option<Duration>,
    ) -> Result<(), ConfigError>;
    /// **[RATIFIED 2026/08/16 — ruling 189]** The configured beacon
    /// interval, or `None` when the beacon is off.
    pub fn persistent_keepalive(&self) -> Option<Duration>;
    // awaitables (ruling 46):
    pub async fn closed(&self) -> ConnectionLost;              // resolves when this connection ends
    pub async fn notified(&self) -> Result<Notification, ConnectionLost>;   // claim one notification
    // accessors (synchronous shared-cell reads, §16.8):
    pub fn remote_static(&self) -> PublicKey;
    pub fn remote_address(&self) -> SocketAddr;
    pub fn session_id(&self) -> SessionId;   // hiss's, re-exported (ruling 89)
    pub fn is_established(&self) -> bool;
}

/// `SessionId` is **`hiss::noise::SessionId`, re-exported**, not a
/// slither type. **[RATIFIED 2026/08/15 — ruling 89]** It appeared
/// exactly twice in this specification and was never defined. hiss
/// derives it from the handshake hash, both peers of a session produce
/// the same value, and its own documentation states it is a *public*
/// channel-binding value meant for out-of-band comparison — which is
/// precisely what an application logs it for, and what a
/// short-authentication-string check needs. Minting a slither wrapper
/// would add a type that must be kept equal to hiss's by hand, for no
/// gain. It is reachable from the seal half slither already holds
/// (`DatagramSend::session_id`), so nothing is captured at install.
/// Note its `Eq` is **not** constant-time, by hiss's deliberate choice;
/// it carries no secret and must not be used to compare one.

/// The application-facing notification set — deliberately **not**
/// `core::ConnEvent` (§16.4). Non-exhaustive: a later wire line may add
/// a kind without a breaking change.
#[non_exhaustive]
pub enum Notification {
    AddressMoved { from: SocketAddr, to: SocketAddr },   // §7.3 roam committed
    Contested,        // §7.5's contested-connection probe went out
    ContestCleared,   // that mark cleared — an ACK covered the probe floor
}

impl SendStream {
    pub async fn write(&mut self, buf: &[u8]) -> Result<usize, WriteError>;
    pub async fn finish(&mut self) -> Result<(), WriteError>;
    pub async fn acked(&mut self) -> Result<(), WriteError>;   // delivery confirmation (ruling 47)
    pub fn reset(&mut self, error_code: u64);
    pub fn id(&self) -> Option<StreamId>;   // see the id() note below (ruling 116)
}
impl RecvStream {
    pub async fn read(&mut self, buf: &mut [u8]) -> Result<Option<usize>, ReadError>;
        // Ok(None) = FIN reached, all data delivered — NOT a reset (ruling 121)
    pub fn id(&self) -> Option<StreamId>;   // see the id() note below (ruling 116)
}
impl BiStream {                                  // ruling 96; join's shape is ruling 120
    pub fn split(self) -> (SendStream, RecvStream);
    pub fn join(send: SendStream, recv: RecvStream)
        -> Result<Self, (SendStream, RecvStream)>;   // Err = not the same stream
    pub fn id(&self) -> Option<StreamId>;
}
```

**[RATIFIED 2026/08/15 — ruling 116]** *What `id()`'s `Option` holds
open.* It was annotated "None before establishment (§16.9)", and ruling
116 removes that cause: no application handle exists before
establishment, so a handle's id is known from birth. The `Option`
**stays**, and the handle **caches** its id. Caching is not an
optimisation — the core's `stream_id(r)` is deliberately *not* monotone,
because a fully closed stream's entry is freed (§9.7), so an uncached
`id()` would answer `None` again after the stream ended. That would
contradict `remote_static()`'s keeps-answering property for no gain.
The `Option` is retained because removing it is a breaking change to save
a `match`, and re-adding it when a later line publishes a
pre-establishment route would be breaking again.

**[RATIFIED 2026/08/15 — ruling 121]** *`Ok(None)` means the FIN, and
only the FIN.* A stream the peer reset surfaces `Err(ReadError::Reset)`,
and that outcome is **sticky at the handle**: every subsequent `read`
re-reports it. Without stickiness an application that logs the reset and
retries its loop reads a clean end-of-stream on the next call, and §9.6's
abandoned data is presented as a complete transfer — data loss reported
as success.

**[RATIFIED 2026/08/15 — ruling 119]** *An empty `buf` is `Ok(Some(0))`,
not a wait.* `read` with a zero-length buffer returns immediately without
consulting the connection, mirroring ruling 110's rule that a zero-length
`write` is `Ok(0)`. `Pending` keeps exactly one meaning at this surface:
*there is work to wait for*.

**[RATIFIED 2026/08/15 — ruling 124]** *A stream handle reports the fate
of its own stream. Precedence, for every verb on all three handles:*

1. **this handle's terminal state** — `Ok(None)` / `Err(Reset(code))` on
   a receive half; `Ok(())` for a repeated `finish()` and `Err(Finished)`
   for anything after `finish()` or `reset()` on a send half;
2. **the connection's death** — `Err(ConnectionLost)`;
3. **the empty-buffer short-circuit** (rulings 110, 119);
4. **the connection itself**, which may block.

The order of 1 over 2 is the substantive part. A stream that reached EOF
**completed**, and the connection dying afterwards does not un-complete
it: answering `ConnectionLost` to a reader that already took every byte
and the FIN reports a failure about a success, which is ruling 121's
misreport with its sign flipped. The connection's own fate is `closed()`'s
to report. §16.11 makes this load-bearing rather than tidy — `AsyncRead`
requires a sticky end-of-file, so a connection that dies after the FIN
would otherwise surface a spurious `io::Error` to `read_to_end`. The
order of 2 over 3 follows from what the empty-buffer rules are *for*:
they exist to avoid blocking, and a dead connection does not block, so
answering `Ok(0)` there would report success on a corpse.

`open_bi`/`open_uni` wait for MAX_STREAMS allowance when the cumulative
limit is exhausted (§10.4), woken by `StreamsAvailable` (§16.4); `write`
waits for stream and connection credit
(§10.1); `send_message` waits for stream allowance, then behaves per §9.8.
`send_datagram` never waits (drop-oldest queue, §11.3). `finish()`
resolves when the FIN is accepted into the stream's send state (errors
surface as `WriteError`) — **not** when the peer has it, which is what
ruling 47's two `acked()` verbs below are for; `close()` resolves once the
CLOSE frame is sealed and the closing state is entered (§15.2), and truncates `reason`
**[RATIFIED 2026/08/16 — ruling 189]** `persistent_keepalive()` is the
setter's reader, and it exists because ruling 44 made *"a rejected call
leaves the interval **unchanged**"* an acceptance criterion that **nothing
in this surface could observe**. Without it the obligation is testable
only by inferring the interval from beacon cadence across a long timing
window, bracketed from both sides so that neither an
upward nor a downward clamp survives — which a blind test author did, and
should not have had to. A configuration setter whose effect cannot be read
back is the defect; the getter is the fix. *Note the shape for Appendix
B's sweep before slice 9: ruling 44 was ratified with a test obligation
whose **observability was never checked** — working rule 11's cousin,
applied to an obligation rather than to a mechanism.*

`set_persistent_keepalive` admits exactly
the range **[1 s, `D_eff`)**: it rejects an interval **below 1 s**
(the floor — ruling 42, which keeps the beacon from becoming an
unthrottled load generator on a class §14.5 exempts from the congestion
window) and an interval **at or above the connection's `D_eff`** (the
ceiling — rulings 40 and 282, which reject an interval too long to keep
a connection alive at all). `None` disables the beacon and is always
accepted. The v1 recommended interval is 10 s; for a custom profile,
`K_eff` is the corresponding one-loss choice (§7.5).

**[RATIFIED 2026/08/14 — ruling 44]** *Rejection is a
`Result`, never a panic.* `set_persistent_keepalive` returns
`Result<(), ConfigError>` with `ConfigError::{KeepaliveTooShort,
KeepaliveTooLong}`, each naming the bound it violated. The setter is the
only fallible member of the handle surface, and it is fallible because
its argument is the only one an application routinely takes from
**outside the program** — a config file, a settings field, a remote
policy. Two consequences are normative. An out-of-range interval is a
**recoverable input error, not a programmer error**: an implementation
must not panic, debug-assert, or silently clamp, because a clamp would
hand back a beacon that does not do what was asked while reporting
success. And the setter is reachable across an FFI boundary, where an
unwinding panic is undefined behaviour rather than a crash the caller
can trap — so a panicking setter would make a hostile config file a
memory-safety problem in the host application, not merely a bad
interval. `ConfigError` is a **configuration** error and deliberately
sits outside §18.1's protocol-error taxonomy, which stays closed: no
peer, no packet, and no connection state is involved, and nothing about
it is observable on the wire.

**[RATIFIED 2026/08/14 — ruling 46]** *A connection's death, and the
events that matter to an application, are awaitable.* Before this ruling
the surface above could tell an application nothing except as the return
value of a verb it happened to be inside at the time: a process holding
an **idle** connection — the mobile case this protocol is built for —
learned of `TimedOut`, `Replaced`, or `PeerClosed` only at its next send,
and ruling 45's contested mark and §7.3's roams, both explicitly
described as application-visible, reached no application at all. Two
awaitables close that gap and are the whole of the application-facing
event surface.

- **`closed()`** resolves with the `ConnectionLost` that ended the
  connection, for whatever reason — every row of §15.4's teardown
  matrix. It is a **latched** signal, not a queue: cancel-safe, awaitable
  concurrently from any number of tasks, and after death it resolves
  immediately and for ever — which asks only that `ConnectionLost` be
  `Clone`, a derive, not a variant change; §18.1 stays closed. On a
  healthy connection it never resolves,
  which is what makes it the `select!` arm of a long-running loop.
- **`notified()`** claims **one** `Notification`, in the same **pull
  model** as `accept_bi`, `recv_message` and `recv_datagram` (§16.4) —
  but see ruling 118 below for how far the *post-death* half of that
  model reaches: the
  connection retains what has not been claimed and the verb hands over
  exactly one, so a notification is never dropped on the floor between an
  application's two visits. It resolves `Err(ConnectionLost)` once the
  connection has ended **and** its unclaimed notifications have been
  drained — a notification generated before the death is not lost to the
  death. It is cancel-safe: a dropped future has claimed nothing.

**[RATIFIED 2026/08/15 — ruling 118, AMENDED the same day by ruling
128]** *The drain-then-report rule is `notified()`'s alone as an
**ordering** rule; what it may not claim is that nothing survives.* At
the **closing** endpoint, §15.2 really does let `close()` drop stream,
recovery and congestion state immediately, and there is nothing to hand
over. Ruling 118 said that and stopped, and applying it to the
**receiving** endpoint was an error: §15.2's next bullet gives a peer
that receives an authenticated CLOSE a `CLOSE_LINGER` drain before it
drops state, so a stream that fully arrived is still there.

**[RATIFIED 2026/08/15 — ruling 128]** *Data that arrived before the
death survives it.* While the connection still holds a stream's received
state, `read` serves the buffered bytes and then the FIN's `Ok(None)`,
and `accept_bi`/`accept_uni` hand over streams opened before the death;
when nothing is left, both report `ConnectionLost`.
**[AMENDED 2026/08/16 — ruling 152]** *That enumeration was short:
`recv_message` and `recv_datagram` drain on the same terms.* Appendix B
ratifies an obligation the two-verb reading cannot satisfy —
`send_message(m)`, then `acked()`, then `close()`, then drop every handle,
and the peer must still receive `m` in full from `recv_message()` — where
the peer's driver processes the data and the CLOSE in one pass, which is
this ruling's own worked example. The *rationale* already described
messages: §16.2 names `send_message(msg); acked(); close()` as the idiom
`acked()` exists for; only the *list* omitted them. `recv_datagram` joins
by symmetry — nothing is promised for a datagram, but the asymmetry would
be a trap for exactly the reason given here, that a receiver woken after
the latch cannot win the race by being prompt.
**Parking is never
permitted on a dead connection** — nothing further can arrive, so a
`read` with no data and no FIN is an error rather than a wait. `closed()`
is unaffected and still resolves at the death: the connection *is* dead,
and what survives is only what already arrived. The case that forces this
is the ordinary one — a sender that writes, finishes and drops its
handles closes implicitly (§16.2, ruling 125), its peer's driver
processes the data and the CLOSE in one pass, and the peer's application
is woken **after** the latch is set. Without this rule that application
can never reach a stream that arrived in full, and it is not a race it
can win by being prompt. It is ruling 47's problem from the receiving
end, and `acked()` does not reach it: the peer's transport acknowledging
is not the peer's application claiming. A `Notification` remains
different in the way that matters — it is a fact about the connection,
complete in itself, and it survives the event it describes.

**Retention is one slot per kind**, which is what keeps the notification
state O(1) per connection and lets it need no queue bound at all
(§17.5). An unclaimed `AddressMoved` superseded by a further roam keeps
the **oldest unclaimed `from`** and the **newest `to`**, so the pair
always describes the net move since the application last looked.
`Contested` and `ContestCleared` are distinct kinds, and ruling 41's
collapse means at most one mark exists per connection at a time, so at
most one of each can ever be pending. Pending notifications of different
kinds are handed over in generation order (§16.4's ordering rule).

**Why this is not `core::ConnEvent`.** The core enum's remaining variants
— `Established`, `StreamOpened`, `StreamsAvailable`, `StreamReadable`,
`StreamWritable`, `StreamFinished`, `StreamReset`, `MessageReadable`,
`DatagramReadable`, `SendCreditAvailable`, `Closed` — are **already
served**: each is the wakeup behind a blocking verb (`Connecting`'s
resolution, `accept_bi`/`accept_uni`, `open_*`, `read`, `write`, ruling
47's `acked`, `read`'s `Reset`, `recv_message`, `recv_datagram`, a
blocked `send_message` — ruling 150 — and `closed()`). Publishing them a
second time would hand an application two ways to learn one fact and
invite the read-the-event-stream-instead-of-calling-the-verb style that
§10.6 and §16.8 exist to forbid — an event consumer that claims nothing
leaves the payload retained in the core for ever, which is precisely the
wedge the pull model is built to make impossible. `Notification` carries
only what **no verb can deliver**: a change to the connection itself.

**This is a shell change; the sans-io core is untouched.** The **shell
translates**: `ConnEvent::Closed(lost)` latches `closed()`;
`ConnEvent::AddressMoved`, `Contested` and `ContestCleared` fill the
notification slots. **[AMENDED 2026/08/18 — ruling 259]** The counting
rule, stated so the lists can be checked: the core enum has **fourteen**
variants — eleven verb-served above, three notification-fillers here —
and a variant added to `ConnEvent` must land in exactly one of the two
lists. No new core event, no new `poll_output` variant, no
timer, no state machine, and nothing on the wire. §16.4's "signals, not
payload carriers" framing holds unchanged at both layers: a
`Notification` is a signal about the connection, carries no application
payload, is not an instruction, and an application that never calls
`notified()` observes exactly the protocol behaviour it would have
observed without it.

**[RATIFIED 2026/08/14 — ruling 47]** *Delivery confirmation — an
application can wait for what it sent to be acknowledged before closing.*
`send_message()` and `finish()` resolve when the payload and the FIN are
accepted into send state, **not** when the peer has them, and §15.2 lets
`close()` drop stream, recovery and congestion state immediately. Compose
the two and message-then-close — the natural last act of a messaging
application, and of every clean shutdown, reachable even by accident
since dropping the last handle performs `close(NO_ERROR, "")` — loses its
tail at the path's loss rate, silently. The core already knows the
answer: `ConnEvent::StreamFinished { id }` fires when a send half is
fully acknowledged, and was simply never surfaced. Ruling 47 surfaces it,
in two verbs and no more.

- **`SendStream::acked()`** resolves once every byte written to that
  stream **and its FIN** are acknowledged by the peer's transport — the
  `StreamFinished` event, awaited. It is legal and expected **after**
  `finish()`, and never returns `WriteError::Finished` for that reason.
  It returns `Reset(code)` if the stream was reset before its data was
  acknowledged (a local reset, or the peer's §9.8 overflow reset), and
  `ConnectionLost` if the connection died first.
- **`Connection::acked()`** takes a **snapshot** — every byte handed to
  the connection at the instant of the call, across every stream,
  including the message streams §9.8 never surfaces a handle for — and
  resolves when each of those bytes is acknowledged **or abandoned by a
  reset** (§9.6: an abandoned byte is never acknowledged, and waiting on
  one would never terminate). Bytes written *after* the call do not
  extend it, so `acked()` terminates on a live connection even while a
  bulk stream is still being written. This is the verb behind
  `send_message(msg).await; acked().await; close(NO_ERROR, "").await`.

**[RATIFIED 2026/08/14 — ruling 54]** *This verb is named `acked()`, not
`flush()`.* It was `Connection::flush()` until §16.11's `AsyncWrite`
surface made the name unusable: `AsyncWrite::flush` on a `SendStream`
from the same object graph means something strictly **weaker** — bytes
accepted into send state — and a consumer reading two verbs named
`flush` on adjacent objects, one meaning *acknowledged by the peer* and
one meaning *nothing has been promised*, will use the wrong one exactly
where it matters. `acked()` is symmetric with `SendStream::acked()`
above, which already carried the right name, and §16.2's "the types are
normative in shape; an implementation may rename" permits the change.
The semantics are **unchanged in every respect** — snapshot scope,
reset-abandonment termination, and what an acknowledgement does and does
not promise all stand as ruling 47 wrote them.

**What an acknowledgement promises, and what it does not.** It is
**transport receipt**: the peer's slither received those bytes, admitted
them into its receive state, and said so in an ACK (§12). It is **not**
delivery to the peer's application — not `recv_message()` having
returned, not read, not processed, not stored, and not agreed to. An
application that needs any of those needs its **own** acknowledgement
message, and always did; slither's job here is only to remove the case
where the bytes never left. Nor is it a guarantee against a peer that
acknowledges and then dies: transport receipt is a statement about the
past, and no transport primitive can make it a statement about the
future.

**No wire change, and §18.1 stays closed.** Nothing here is transmitted
or observable to a peer: both verbs read state the recovery layer already
maintains. `SendStream::acked()` uses `WriteError`'s existing variants and
`Connection::acked()`
uses `ConnectionLost`; no error variant is added. §15.2 is deliberately
**unchanged** — `close()` still drops state immediately and CLOSE is
still not ack-eliciting — which is exactly why the confirmation belongs
in a verb the application calls when it cares, rather than in a slower
`close()` imposed on everyone.

The types are normative in shape; an implementation may rename.

**Drop semantics.** Dropping a staged object is a silent reject (§6.2).
Dropping a `Connecting` **cancels the outbound attempt**: the retransmit
train stops, the static leaves PENDING, and an immediate redial to it
succeeds instead of returning `AlreadyConnected` — the rule and its
consequences are §16.3's (ruling 50).
Dropping the last handle to a `Connection` performs
`close(NO_ERROR, "")` — the graceful teardown of §15.2 (the superseded
"drop = silent local teardown" rule carried onto the wire signal that now
exists). Dropping a `SendStream` without `finish()` resets it with error
code 0. **[RATIFIED 2026/08/15 — ruling 93]** Dropping a `RecvStream`
abandons the receive half: arrivals for it are discarded and stream-level
credit is never again advanced (a sender that keeps pushing stalls at the
stream window), and the half is **retired at once** — freed, tombstoned
at §9.2's watermark, and trued up to the stream window at the connection
level (§10.3) in the same step. An abandoned stream never wedges the
connection window and never starves the peer's cumulative stream
allowance.

*This sentence previously said the half "closes when the pinned final
size or a reset arrives", which contradicted the promise that closes it.*
A sender stalled at the stream window — the stall this same sentence
describes — sends no FIN, because its FIN follows its data, and has no
reason to reset; with `STOP_SENDING` deferred (§19) slither cannot ask.
So no final size was ever pinned, no retirement ever ran, and four
abandoned 256 KiB streams wedged the 1 MiB connection window for the
connection's life. §10.3's list, which names "handle abandoned" among the
retirements, held the correct rule.

*Two different mechanisms make later arrivals inert, and which one applies
depends on the space.* For a **peer-opened uni** stream the abandoned
receive half is the only half this endpoint holds, so freeing it makes the
stream **fully closed** (§9.7): the watermark advances, §9.2 makes every
later frame naming that index a no-op, and the peer earns its MAX_STREAMS
grant (§10.4). For a **bidi** stream the send half is still ours and still
live, so the stream is **not** fully closed, the watermark does **not**
advance, and the index remains in the open set — later STREAM frames for
it are neither implicit opens nor watermark no-ops. They are discarded by
§16.2's own rule ("arrivals for it are discarded"): ACKed, delivered
nowhere, consuming no further credit, because §10.3's true-up is absolute
and that stream's contribution is already at its maximum. The stream-level
`FLOW_CONTROL_ERROR` check still applies against the frozen advertised
limit, which is what keeps an abandoned half from becoming an unbounded
sink.

An implementation that relies on the watermark alone re-opens an abandoned
bidi receive half on the next frame — resurrecting freed state and
double-charging the cumulative limit — and one that relies on the
per-half tombstone alone never advances the watermark for uni and never
grants the peer its MAX_STREAMS credit. **Both are needed.** Dropping every handle stops the driver and every
connection dies silently — nothing transmitted (§15.4).

**Where those two rules coincide, nothing is transmitted.**
**[RATIFIED 2026/08/15 — ruling 88]** They are usually different sets:
"the last handle to a `Connection`" is one connection ending while other
handles keep the driver alive, and "every handle" is the process letting
go of everything. Dropping the last `Connection` **when it is also the
last handle in the process** is both at once, and §15.4's
endpoint-dropped row governs: no CLOSE is sealed. A synchronous `Drop`
cannot await the driver, and the driver is already stopping; ruling 50
takes the same position for the analogous `Connecting` case ("an
attempt that never completed has no session to close and no wire signal
to send"), and the peer's cost is bounded at its effective dead timeout,
which that row already accepts. This is S26's `⚠ CHECK` — drop-order
sensitive and
the opposite of the obvious guess — and it belongs in the rustdoc beside
S3a's.

### 16.3 Driver and handle lifetimes

The shell is **one `!Send` driver task**, spawned with
`tokio::task::spawn_local` (a `LocalSet` is required), owning the socket
for both receive and send and owning both sans-io cores. **Do not add
`Send` bounds to the actor path** — a DH provider is not required to be
`Send` (hardware statics). `Endpoint`, staged objects, `Connection`, and
stream handles are thin clients over the driver's state; connections do
not send on socket clones. The `Wire` trait seam (real socket or
`testutil::FlakyWire`)
is the driver's I/O boundary. The driver lives while any handle lives;
dropping every handle stops it, and every session dies silently with it.

**[AMENDED 2026/08/15 — ruling 115]** *The list above enumerates thin
clients, not driver-keeping handles, and the two sets are not equal.*
**Staged objects are in it and do not keep the driver alive** — ruling
62 makes a staged object's verb "a round-trip to a driver it does not
keep alive", which is why `IntroError`, `AuthError` and `AcceptError`
each carry `EndpointDropped` and `ConnectError` does not. What decides
membership is ruling 62's test — *a future or handle that changes
protocol state when dropped is a handle; one that does not, is not* —
and by that test `Endpoint`, `Connecting`, `Connection`, `SendStream`,
`RecvStream` and `BiStream` keep the driver alive, while staged objects
and a `closed()` future do not. A stream handle qualifies twice over: its
`Drop` emits RESET_STREAM or retires a receive half, and a `Drop` that
must put a frame on the wire needs a driver to put it there.

**How a handle reaches the core — split by cost.** **[RATIFIED
2026/08/14 — ruling 53]** This paragraph previously said "thin
**channel-backed** clients", which contradicted §16.8's "quinn pattern"
(per-stream waker maps, and accessors that are "synchronous reads of a
shared cell"). Those are two different mechanisms and only one can be
built. The seam is now split by what each surface costs:

| Surface | Mechanism |
|---|---|
| Endpoint verbs — `accept` and §6.2's three staged verbs | command channel + oneshot reply |
| `connect` | command channel **send only**, plus a synchronous read of the shared cell for the NONE/PENDING/LIVE test. **[AMENDED 2026/08/15 — ruling 87]** |
| Connection data path — `write`, `read`, `open_*`, `accept_*`, `send_message`, `recv_*`, `close`, `acked`, `notified` | shared cell (`Rc<RefCell<_>>`) with the driver, plus the blocked-readers / blocked-writers waker maps of §16.8 |
| Accessors — `remote_static`, `remote_address`, `session_id`, `is_established` | reads of that same shared cell (§16.8, unchanged) |

The endpoint verbs stay round-trips because §6.2 requires the DH costs to
land **on the driver task**, and they are rare and already `async` in
§16.2's signatures.

**`connect` is the exception, and ruling 53's table originally hid it**
by listing it beside verbs that genuinely are `async`.
**[AMENDED 2026/08/15 — ruling 87]** §16.2 declares
`pub fn connect(…) -> Result<Connecting, ConnectError>` — **not
`async`** — so `ConnectError::AlreadyConnected` is returned before any
await, and a oneshot reply cannot be read from it without blocking,
which §16.8 forbids. Nor does it need one: **`connect()` performs no
DH.** §6.1's initiator costs are paid when msg1 is built, on the driver;
the verb itself only mints the pending. So it sends its command and
returns, and the NONE/PENDING/LIVE test §16.1 requires "at the instant
of the call" is a synchronous read of the same shared cell §16.8 already
mandates for the accessors. Both texts are satisfied; neither signature
changes. The synchronous cell read is also what makes ruling 50's
cancellation-ordering MUST **structural** rather than a discipline —
`Connecting::drop` writes the static back to NONE in that cell, so an
immediate redial with no clock advance between them reads what the drop
just wrote. The data path does not: the driver is `!Send` and
single-threaded, so a shared cell costs a refcount and a borrow flag,
there is no lock and no contention to have. **Every mutating borrow ends
by marking the connection dirty and waking the driver**, which drains
`poll_output()` to `Timeout` and performs the I/O — §16.4's
drain-after-every-mutating-call contract is untouched, it is merely not
always the driver that made the call.

*Why this is load-bearing rather than a style preference.* Each data-path
verb is written **once**, as `poll_*(&mut self, cx) -> Poll<_>`; §16.2's
`async fn` is then `poll_fn(|cx| self.poll_*(cx, ..)).await`, and
§16.11's `AsyncRead`/`AsyncWrite` is the *same function* with its error
mapped. Under the channel form neither is available: every adapter in
§16.11 must box and store an in-flight future, `Unpin` becomes delicate
(a stored future has already copied a buffer that the next `poll_write`
may not pass again), and every write costs a round-trip and an
allocation. **Shell-only: no core type, no wire byte, no timer.**

Ruling 46's `closed()` and `notified()` are handle-borne like every other
verb and change no lifetime: they resolve `Err(ConnectionLost::
EndpointDropped)` when the driver stops under them, and the latch and the
notification slots live in the connection's shell-side bookkeeping —
released with it, after §16.4's `Retired` (which is what keeps a
post-mortem `closed()` answering from a `Connection` handle the
application still holds, rather than from a leaked route). They are not
a reason to keep a connection alive: a `closed()` future is not a handle,
and holding one while dropping every `Connection` still stops the driver
and still kills the session silently.

**A `Connecting` *is* a handle.** **[RATIFIED 2026/08/14 — ruling 62]**
The driver lives while a `Connecting` lives, and dropping the last
`Connecting` — with no `Endpoint` and no `Connection` outstanding — stops
it. This is the distinction against the `closed()` future above, and it
is a real one rather than a special case: a `Connecting` **owns an
in-flight protocol attempt** — a pending, its index, and §5.5's
retransmit train, which is why ruling 50 makes dropping it a
state-changing event — while a `closed()` future owns nothing and merely
observes. A future that changes protocol state when dropped is a handle;
one that does not, is not.
The consequence is that **`ConnectError` needs no `EndpointDropped`** and
§18.1 stays closed as written: a `Connecting` cannot outlive the driver,
so there is no state for such a variant to describe. The asymmetry with
`IntroError`, `AuthError` and `AcceptError` — which all carry
`EndpointDropped` — is therefore correct and not an omission: a staged
object's verb is a **round-trip to a driver it does not keep alive**, so
the driver can stop underneath it; an outbound attempt keeps its own
driver running.

**Dropping a `Connecting` cancels the attempt.** **[RATIFIED 2026/08/14 —
ruling 50]** A `Connecting` is a future, and dropping it **cancels the
outbound attempt immediately**: §5.5's retransmit train stops, the
pending and its pending-index entry are dropped (§17.3), its dialled
address leaves §6.5's hint set with them (§17.4), and the static leaves
**PENDING** for **NONE** (§5.4). Nothing is transmitted — an attempt that
never completed has no session to close and no wire signal to send, as in
§15.4's endpoint-dropped row. A subsequent `connect()` to that same
static therefore **succeeds** rather than returning
`ConnectError::AlreadyConnected`: §16.1's in-flight-outbound clause names
live attempts only, and a cancelled attempt is not one. An implementation
**MUST** order the cancellation ahead of any endpoint verb the
application issues after the drop returns, so an immediate redial cannot
observe the corpse; Appendix B pins that on the paused clock.

*Why cancel-on-drop.* It is Rust's convention for futures, so it is what
a consumer will assume — and here the assumption is near-universal in
practice, because `HANDSHAKE_GIVEUP` is 90 s and there is no configurable
connect deadline, so essentially every application writes
`timeout(5 s, endpoint.connect(...))` or a `select!`. Specifying anything
else would turn that idiom into a trap with **no escape**: the attempt
would hold the static PENDING for up to 90 s, every redial would return
`AlreadyConnected`, and — unlike a live connection, which the application
can always `close()` — there would be no handle left to close.

*What the peer is not told, stated honestly.* If the peer already
answered and installed a half-open session (it sent msg2, and we dropped
before or during completion), that session is **not** told. It dies by
liveness at its effective dead timeout (25 s under the v1 profile):
§7.4's install pin arms the death
deadline at install and sets `last_send` equal to
`last_authenticated_recv`, so a session that receives nothing after
install emits nothing at all and is reaped in silence — exactly ruling
39's reap case, and the same 25 s the endpoint-dropped row already rests
on. A msg2 racing the drop arrives after the pending index is gone: it
routes by index to nothing and is inert (§17.3's corollary), so no
session is installed on our side and no state is resurrected.

*Declined, so they are not re-proposed.* (a) **Send a CLOSE when msg2 has
already arrived.** More correct on the wire's own terms — it is exactly
the 25 s of liveness wait §15.1 exists to save the peer — but it requires
carrying completion state, keys, and an index through the drop path so
that a synchronous `Drop` can produce an authenticated packet, and the
reward is a peer-side latency saving in the one case liveness already
closes. (b) **Require an explicit cancel method and make drop a no-op or
a detach.** It fights the cancellation convention rather than serving it,
and it leaves the `timeout()` trap in place for precisely the
applications that never read this paragraph.

*The PENDING interaction, verified.* A cancelled attempt leaves **no**
PENDING entry anywhere the tie-break can consult. All three readers of
"is this static PENDING?" read the same pending tables the cancellation
empties: ruling 35's §6.4 branch fires on *a pending exists for the
proven static*, §6.5's hint set is defined as
*the pending tables' dialled addresses* (§17.4), and §5.4's responder
rule defines PENDING the same way. There is no separate per-static flag
that could outlive the pending. **[AMENDED 2026/08/16 — ruling 178]**
*This verification is what ruling 178 rests on, and it is stated in the
same words the three readers now use.* The §6.4 branch's clause read "an
in-flight outbound initiation exists" until that ruling; the
verification above was already about **table membership**, which is why
ruling 90's `mint_pending`/`start_attempt` split — a pending minted with
nothing yet in flight — resolves to PENDING rather than NONE. So an initiation from that peer arriving
after a cancelled dial takes §5.4's **NONE** row — the ordinary staged
accept — and never §6.7's comparison: there is no stale pending to lose a
tie-break to, and no `Connecting` left for §6.4's loser branch to resolve
with `AlreadyConnected` (no error variant is added; §18.1 stays closed).
Cancellation writes nothing to the timestamp guard (§17.1), because it
authenticated nothing — identical to a `HANDSHAKE_GIVEUP` expiry.

**The `Wire` trait.** **[RATIFIED 2026/08/14 — ruling 49]** The seam is
normative, not an implementation note — it is the crate's extension
point, and the whole of §16.10's kernel-free drivability rests on it:

```rust
pub trait Wire {
    /// Send `buf` to `addr`, returning the bytes written.
    async fn send_to(&self, buf: &[u8], addr: SocketAddr) -> std::io::Result<usize>;
    /// Receive one datagram into `buf`, returning its length and source.
    async fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)>;
}
```

with a blanket implementation for `tokio::net::UdpSocket`, so the default
case costs the application nothing. Four properties are normative. The
**application supplies it** — through `Endpoint::builder()` (§16.2) — so
an application that needs its own socket options, a dual-stack or
per-interface arrangement, a tunnel, or a simulator installs one without
forking the crate. It is **not required to be `Send`**, and no `Send`
bound may be added to it or to the futures its methods return: the
driver is a single `!Send` actor (above) and the same reasoning that
keeps a DH provider free of `Send` keeps a `Wire` free of it. It takes
`&self` on both methods because the one driver task owns the seam and
drives both directions from it; a `Wire` needs no interior handle
duplication and connections never send on socket clones. And
**`testutil::FlakyWire` is a `Wire`** — the in-memory implementation the
paused-clock flow tests ride (§16.10, Appendix B), which is why every
timer in this document is testable without a kernel, a port, or a sleep.

**A failing `send_to` is traced, not acted on.** **[RATIFIED 2026/08/14 —
ruling 49]** When `send_to` returns an `Err`, the driver **MUST** trace
it against the connection whose datagram it was, under §18.2's operator
contract, carrying the destination address and the underlying error. It
does **not** kill the connection, resolve any verb with an error, or
produce a `Notification`; §18.1's taxonomy stays closed and gains no I/O
variant. This is a deliberate design position, not an omission:

- **A failed send is not authoritative.** Liveness in this protocol is
  **receive-driven by ruling** (§7.4) — a connection dies because nothing
  authenticated arrived, never because something failed to leave. A local
  send error is a statement about this host's routing table at this
  instant, and the protocol has exactly one verdict path, on purpose.
- **`ENETUNREACH` is the signal that *precedes* a successful roam**, not
  one that follows a dead connection (§7.3). Wi-Fi drops, the interface
  goes away, sends fail for a few hundred milliseconds, cellular comes
  up, and the session continues at a new address with nothing lost.
  Killing the connection on the send error would convert the exact
  scenario the migration guarantee exists for into a teardown — it would
  delete the guarantee, not implement an error path.
- **The application already observes every one of these errors.** It
  supplies the `Wire`, so each `io::Error` crosses code it wrote, with
  its destination address in hand, before slither ever sees the `Err`. An
  application that wants to act on `EMSGSIZE`, to switch interfaces, or
  to give up early can do so at that seam — where the information is
  richest — and slither's obligation is the one thing the application
  cannot do from there: make the failure **explicable afterwards**, so an
operator reading a receive-driven timeout finds the burst of send
  failures that explains it instead of a bare timeout.

### 16.4 The two cores and the poll contract

**[RATIFIED 2026/08/14, amended 2026/08/14]** The protocol logic lives in
two pure state machines —
`core::Endpoint<I: Identity>` and `core::Connection` — with the str0m
single-`poll_output` contract: **every mutating call** (`handle_datagram`,
`handle_timeout`, verb calls, stream/datagram/message operations,
`connect`) **is followed by draining `poll_output()` to the terminal
`Timeout(Option<Instant>)`**, which is simultaneously the drain sentinel
**[AMENDED 2026/08/18 — ruling 262: each core additionally exposes
`next_deadline(&self) -> Option<Instant>`, returning exactly the value
the terminal `Timeout` carries — a read-only path to the announcement
half, so a driver collecting deadlines never pops (a popping read at
that position destroyed a queued datagram when a reentrant inline
consumer left a queue non-empty)]**
and the next-deadline announcement — a driver cannot forget to drain.
Connection→endpoint events fold into `ConnOutput::ToEndpoint` (one drain
loop, no second queue to forget). The generic `I: Identity` must reach the
endpoint core's type (the mid-state map is typed over `I::Provider`).

```rust
impl<I: Identity> core::Endpoint<I> {
    fn new(now: Instant, config: Config, identity: I, rng_seed: [u8; 32]) -> Self;
    fn connect(&mut self, now: Instant, remote: SocketAddr, remote_static: PublicKey)
        -> Result<(ConnectionId, core::Connection), ConnectError>;
    fn handle_datagram(&mut self, now: Instant, src: SocketAddr, datagram: &[u8]) -> Disposition;
    fn handle_timeout(&mut self, now: Instant);                        // idempotent
    fn handle_connection_event(&mut self, now: Instant, id: ConnectionId, ev: ToEndpoint);  // [ruling 80]
    fn poll_output(&mut self) -> EndpointOutput;                       // drain to Timeout
    fn next_deadline(&self) -> Option<Instant>;                        // the Timeout's value, read-only [ruling 262]
    // staged verbs (§6.2), by IntroId:
    fn read_identity(&mut self, id: IntroId) -> Result<PublicKey, IntroError>;
    fn authenticate(&mut self, now: Instant, id: IntroId)
        -> Result<(PublicKey, Timestamp), AuthError>;
    fn accept(&mut self, now: Instant, id: IntroId)
        -> Result<(ConnectionId, core::Connection), AcceptError>;
    fn reject(&mut self, now: Instant, id: IntroId);   // [ruling 80]
    // stage-0 accessors (§6.1, §6.3) — [RATIFIED 2026/08/15, ruling 71]:
    fn intro_source(&self, id: IntroId) -> Option<SocketAddr>;
    fn intro_sender_index(&self, id: IntroId) -> Option<u32>;
}
```

`core::Endpoint::new` retains `config`'s validated `TimingProfile` and
stamps it onto both connection birth paths: the connection returned by
`connect()` and the connection returned by `accept()`. The profile is
part of the connection's immutable construction state; `Install` neither
carries nor changes it. This symmetry is normative: configuring only the
initiator-shaped core would make one endpoint apply two liveness policies
depending on which peer happened to send msg1.

**The `Identity` seam.** **[AMENDED 2026/08/18 — ruling 264]** `I:
Identity` is the static-key and DH-provider abstraction (§2.4's
canonical encoding is what `public_static()` returns the octets of;
§17.5 is why `open()` is lazy):

```rust
pub trait Identity {
    type Suite: Handshake;
    type Provider: DhProvider<CurveOf<Self>>;
    type Error: core::error::Error + 'static;
    fn public_static(&self) -> &PublicKeyOf<Self>;
    fn open(&self) -> Result<(Self::Provider, PrivateKeyOf<Self>), Self::Error>;
}
```

`open()` mints the provider and static-key handle for **one** handshake,
called lazily — at `read_identity()` on the responder path, and once per
attempt on the initiator path — never at park, so an enclave-backed
static does not hold up to `INTRO_QUEUE_CAP` concurrent provider handles
for introductions nobody has inspected yet. No `Send` bound appears
anywhere on `Provider`, deliberately (§16.3, S21).

```rust
enum Disposition { ForConnection(ConnectionId), Done }

enum EndpointOutput {
    Transmit(Transmit),                          // msg1/msg2, retransmits, tie-break msg2
    IntroReady(IntroId, SocketAddr),
    ToConnection(ConnectionId, Install),
    HandshakeFailed(ConnectionId, ConnectError), // shell-only (below)
    Timeout(Option<Instant>),                    // terminal
}
struct Install { session: EstablishedSession, role: Role }
enum Role { Initiator, Responder }               // ruling 106; §6.7 fixes it, §9.1 reads it
struct EstablishedSession { /* the hiss transport pair (seal + open), our session
                               index, the peer's index, and the anchor address (§5.6) */ }
struct Transmit { to: SocketAddr, data: Vec<u8> }
```

```rust
impl core::Connection {
    fn handle_datagram(&mut self, now: Instant, src: SocketAddr, datagram: &[u8]);
    fn handle_timeout(&mut self, now: Instant);                        // idempotent
    fn handle_endpoint_event(&mut self, now: Instant, ev: Install);    // Install only
    // application surface (mirrors §16.2, core-shaped):
    fn open(&mut self, dir: Dir) -> Result<StreamRef, StreamsExhausted>;
    fn write(&mut self, now: Instant, r: StreamRef, data: &[u8]) -> Result<usize, WriteError>;
    fn finish(&mut self, r: StreamRef) -> Result<(), WriteError>;
    fn reset(&mut self, now: Instant, r: StreamRef, error_code: u64);
    fn read(&mut self, r: StreamRef, buf: &mut [u8]) -> Result<Option<usize>, ReadError>;
    fn stream_id(&self, r: StreamRef) -> Option<StreamId>;   // ruling 95; §16.9's accessor
    fn send_message(&mut self, now: Instant, msg: &[u8])
        -> Result<SendMessage, MessageError>;                // ruling 163
    // enum SendMessage { Sent, Blocked }  — NOT an error; §18.1 stays closed
    fn send_datagram(&mut self, now: Instant, data: &[u8]) -> Result<(), DatagramError>;
    fn close(&mut self, now: Instant, code: u64, reason: &[u8]);
    // claim verbs — the pull model (§10.6, §11.3, §9.8):
    fn accept(&mut self, dir: Dir) -> Option<StreamRef>;  // claim a peer-opened stream
    fn recv_message(&mut self, now: Instant) -> Option<Vec<u8>>;   // ruling 151        // claim the oldest complete unclaimed message
    fn recv_datagram(&mut self) -> Option<Vec<u8>>;       // claim the oldest queued datagram
    fn poll_output(&mut self) -> ConnOutput;
    fn next_deadline(&self) -> Option<Instant>;   // the Timeout's value, read-only [ruling 262]
}

enum ConnOutput {
    Transmit(Transmit),
    Event(ConnEvent),
    ToEndpoint(ToEndpoint),
    Timeout(Option<Instant>),                    // terminal
}
enum ConnEvent {
    Established,                                 // the install; the shell resolves Connecting
    StreamOpened { dir: Dir },                   // signal: claim via accept(dir); one per
                                                 // newly-opened stream, ruling 99
    StreamsAvailable { dir: Dir },               // MAX_STREAMS credit arrived (§10.4)
    StreamReadable { r: StreamRef },
    StreamWritable { r: StreamRef },             // stream/connection credit arrived for a blocked writer
    StreamFinished { r: StreamRef },             // send half fully acknowledged
    StreamReset { r: StreamRef, error_code: u64 },
    SendCreditAvailable,                         // connection MAX_DATA credit for a blocked
                                                 // message sender — companion to
                                                 // StreamWritable, ruling 150
    MessageReadable,                             // signal: claim via recv_message() (§9.8)
    DatagramReadable,                            // signal: claim via recv_datagram() (§11)
    AddressMoved { from: SocketAddr, to: SocketAddr },
    Contested,                                   // rulings 45/46: the probe went out (§7.5)
    ContestCleared,                              // an ACK covered the probe floor
    Closed(ConnectionLost),
}
enum ToEndpoint {
    Retired { our_index: u32 },                  // teardown: drop the index route (MUST)
}
```

- **`accept()` returns a fully established connection — never followed by
  an `Install`.** `Install` targets only a `connect()`-created connection
  awaiting completion, **exactly once**, resolving its
  `Connecting` — whether from msg2 completion or a lost tie-break's
  admission (§6.7). With the rekey swap deleted (§7.6) there is exactly
  one install per connection, so no discriminator distinguishes them and
  none is carried. Emitting a symmetry `Install`
  after `accept()` (double-install) and waiting for one that never comes
  are both excluded.
- **`Install` carries the role, and the connection core cannot derive
  it.** **[RATIFIED 2026/08/15 — ruling 106]** §6.7 fixes initiator-ness
  for the life of the connection and §9.1's stream-ID parity reads it —
  but §6.6 step 4 admits a peer that **dialled** as the *responder*, so a
  core inferring "I was created by `connect()`, therefore I am the
  initiator" is wrong on exactly that path. It fails silently: both ends
  still agree on every stream they open themselves, and disagree only on
  parity. It cannot be recovered afterwards from hiss — ruling 89 leaves
  `Handshake::Seal` an associated type with no bounds — so the endpoint,
  which is the only party that knows the tie-break's outcome, states it.
- **The core's stream verbs are keyed by `StreamRef`, not `StreamId`.**
  **[RATIFIED 2026/08/15 — ruling 95]** §16.9 assigns wire stream IDs at
  establishment, because parity is fixed only then, and makes `id()`
  return `None` until then — while §16.9's whole point is that a
  `connect()`-created connection is *writable before install*. So `open()`
  must return something, and it cannot be a wire `StreamId`. `StreamRef`
  is an opaque core-internal handle, **stable across install**;
  `stream_id(r)` is §16.9's accessor that the shell's `id()` reads, and
  it returns `None` until establishment.

  The trap this closes is not the missing `Option`. It is a core that
  returns an internal index *typed as* `StreamId` and remaps it at
  install, leaving every live handle holding a stale key — a build that
  passes a pre-establishment test and a post-establishment test and fails
  only "open early, write late".

  **The key type reaches eleven sites, not five**: `open`, `write`,
  `finish`, `reset`, `read`, `accept`, the new `stream_id`, and the four
  `ConnEvent`s that name a stream (`StreamReadable`, `StreamWritable`,
  `StreamFinished`, `StreamReset`). The events must key the same way the
  handles do or the shell cannot match a wakeup to a waker before
  establishment, which is precisely when §16.9 says work is in flight.
  *This ruling was first written naming five signatures; the other six
  were found by opening the file to edit it.* Ruling 71's shape, and the
  reason §16.4's lists get counted rather than read.
- **`Contested` and `ContestCleared` report the mark, and its
  resolution.** **[RATIFIED 2026/08/14 — ruling 45; emission moment and
  variant names fixed 2026/08/14 by ruling 46, per FAB-6]** §7.5's
  contested mark is **application-visible** — via the shell's
  `Notification` (§16.2), which is the surface an application actually
  sees; these two core events are what the shell translates.
  - **`Contested` is emitted at the probe's transmission**, not at the
    mark. That is the same instant §7.5 arms the connection's `K_eff`
    deadline, and it is deliberate: the two moments **can** separate —
    a probe §7.3's amplification budget will not yet admit leaves the
    mark *pending*, which is reachable exactly when the connection has
    just roamed to an unvalidated address, not an exotic corner for a
    mobility-first protocol — and an event fired at the mark would
    announce a countdown that is not running. Binding the event to the
    transmission makes it mean one checkable thing: *the question has
    been asked, and the clock on the answer is now running.* The
    mark-pending gap emits nothing; if the connection dies before the
    budget ever admits the probe, the death arrives as `Closed` like any
    other. `slither::policy` still traces all three moments — the mark,
    the transmission, the verdict (§18.2) — because an operator wants
    the gap and an application does not.
  - **`ContestCleared` is emitted when the mark clears** because an ACK
    covering the probe floor arrived. **[AMENDED 2026/08/16 — ruling
    176]** *And it is emitted **only where `Contested` was**.* The
    clause above read unconditionally, which made an unmatched
    notification reachable by an entirely ordinary route: the floor is
    the counter the *next* seal will use, so any post-mark seal lands at
    or above it, and the peer's ACK can clear a mark whose probe §7.3's
    budget never admitted — firing `ContestCleared` with no preceding
    `Contested`. That is precisely the mis-read ruling 46 deleted
    `under_probe: bool` to prevent, arriving by a different door.
    Clearing a **pending** mark therefore cancels the pending probe and
    emits nothing at all (§7.5): the mark-pending gap emits nothing, and
    so does its exit. It is a separate variant rather
    than ruling 45's `Contested { under_probe: bool }`, because that
    field read backwards: `under_probe: false` names the *pending* state
    to any reader skimming the enum, while it meant *cleared*, and the
    three real states (marked-pending, probing, cleared) do not map onto
    one bool at all. Two named variants carry the same information —
    a reconnect scheduler needs set, cleared, and `Closed`, no more —
    with nothing left to mis-read.
  - A mark that is never answered needs no third event: the connection's
    death already arrives as `Closed(ConnectionLost::TimedOut)`. Like
    every other event here both are a **signal, not a payload carrier**
    and not an instruction: the refusal stands either way, the basis
    rule is untouched, and an application that ignores them observes
    exactly the behaviour it would have observed without them. They
    exist because the two outcomes of a refusal are operationally
    opposite — *refused but healthy, keep using this connection* versus
    *refused and dying, prepare to redial* — and `AcceptError::Stale`
    alone cannot tell them apart, leaving a reconnect scheduler to
    guess. Each is emitted **at most once per mark**, and ruling 41's
    collapse means at most one mark exists per connection at a time, so
    both inherit that bound rather than needing one of their own. **No
    wire change**: nothing here is transmitted, observable to a peer, or
    reflected in any packet.
- **The pull model is uniform.** The core **retains** what it has not
  handed over: reassembled-but-unclaimed incoming uni streams, queued
  received datagrams, and peer-opened streams awaiting `accept(dir)`.
  The receive-side `ConnEvent`s are **signals**, not payload carriers —
  the shell wakes the matching blocked verb, and the verb claims through
  the core (`accept`, `recv_message`, `recv_datagram`, `read`). A
  reassembled-never-claimed uni stream holds its stream state and its
  MAX_STREAMS credit until claimed (backpressure by retention, §9.8);
  the 64-datagram receive queue and its drop counter are core state
  (§11.3). This is what §10.6's no-unbounded-intermediate-queue rule and
  §16.8's no-drop-for-reliable-data rule rest on: nothing reliable ever
  sits in a droppable shell channel.
- **`HandshakeFailed` never reaches `core::Connection`**: the shell
  resolves `Connecting` with `Err(ConnectError::TimedOut)` and drops the
  never-established pending core. `handle_endpoint_event` carries
  `Install` only.
- **`Retired` is a MUST**, and it fires when the connection's state is
  **actually dropped** — not when its death is announced.
  **[AMENDED 2026/08/15 — ruling 81]** The two moments coincide on every
  path with no post-mortem and are `CLOSE_LINGER` apart on every path
  with one:
  - **No linger** — liveness (§7.4), nonce exhaustion (§7.9), or
    `Replaced` (§5.4): `Closed(ConnectionLost)` is followed **within the
    same drain** by `ToEndpoint::Retired`.
  - **No session, so no `Retired`** — a teardown before a session is
    installed emits `Closed(ConnectionLost)` **alone**.
    **[AMENDED 2026/08/15 — ruling 84]** `Retired { our_index }` names
    the index route it exists to drop, and a connection that never
    installed a session never had one: there is nothing to retire and no
    leak the MUST prevents. Ruling 81's first draft listed this case,
    which is not constructible.
  - **Closing or draining** (§15.2): `Closed(ConnectionLost)` is emitted
    **at the death**, so `close()` and `closed()` resolve when §16.2 says
    they do rather than five seconds later; `Retired` follows the
    **`CloseLinger` expiry**. The linger must keep *receiving* — its
    reply is owed only to an authenticated, window-fresh inbound packet,
    and that reply rule is "CLOSE's only reliability mechanism" (§15.2) —
    while `Retired` drops the `receiver_index` route that receiving
    needs. Emitting it at the death would delete the mechanism.

  In both cases the shell delivers `Retired` to
  `handle_connection_event` **before** releasing the connection's
  shell-side bookkeeping (else the index route and the guard-entry pin
  leak for the endpoint's life). The all-handles-dropped case is exempt
  (the driver simply stops).
- **`Timeout(None)`** = drained and no representable deadline can be
  announced; a logically enabled deadline may still lie beyond the
  platform clock horizon. `Timeout(Some(d))` = drained, next representable
  deadline `d`. Identical semantics for both cores.
- **Output ordering within one drain preserves generation order** — a
  transmit and the event it caused come out in that order. Normative;
  tests and logs depend on it.

### 16.5 Time and timers

- **`now: Instant` is an explicit argument on every mutating call**; the
  cores never read a clock. The initiation timestamp (§5.3) is the one
  wall-clock read, behind a clock service injected in the endpoint
  config. `poll_output` takes no `now`.
- **Every monotonic deadline derivation uses checked arithmetic**
  **[RATIFIED 2026/08/31 — ruling 284]**, for both intermediate
  `Duration` calculations and the final addition to its anchor. If
  `anchor.checked_add(delay)` returns `None`, the deadline lies beyond the
  platform clock's representable horizon: the core retains any logically
  enabled timer state but does not announce or fire that deadline. It MUST
  NOT panic, wrap, clamp, saturate, substitute `now`, or fabricate any
  other earlier instant. Other representable deadlines still participate
  normally in the minimum. Every later state transition that re-derives
  the timer performs the checked calculation again. Exact-instant,
  ordering, and lateness rules are unchanged for representable deadlines.
- **Named timers, single min-deadline out.** The connection core's timer
  table: `Keepalive`, `PersistentKeepalive`, `Liveness`, `Loss`, `Pto`,
  `AckDelay`, `CloseLinger`, `Contested`. `Contested` is the
  contested-connection probe's deadline (§7.5): it is armed at the
  probe's **transmission** — never at the mark, which may wait on §7.3's
  budget — for the connection's `K_eff`, and it is disarmed by any ACK
  covering the mark's probe floor. It is **one deadline per connection,
  not one per refusal**: ruling 41 collapses concurrent marks into a
  single contested state, so a refusal arriving while `Contested` is
  armed neither re-arms it nor adds a second, and the named-timer model
  holds with no queue behind the name. **[AMENDED 2026/08/16 — rulings
  175, 176]** Two clarifications the timer model depends on. The collapse
  covers refusals arriving while the mark is **outstanding**; once it
  clears, the next admitted refusal is a fresh mark with a fresh floor
  and arms this timer again — the timer is one-at-a-time, not
  once-per-`K_eff` (ruling 175). And a mark that clears
  **while still pending** cancels its pending probe, so this timer is
  never armed for it (ruling 176) — which matters here because the
  disarm rule above keys on an ACK covering the floor, and a floor
  already satisfied cannot disarm a deadline armed after it. `Pto` is armed only while an
  ack-eliciting packet is in the sent map **and §7.3's budget admits a
  probe** (§13.3, ruling 249) — the probe is ack-eliciting and is in that
  map (§13.5), so `Pto` and `Contested` can be armed together; they are
  not independent in one respect: **four of the eight timers wait on
  §7.3's budget** **[AMENDED 2026/08/18 — ruling 265]** — `Contested` at
  the probe's **transmission** (§16.4), `Pto` at its **announcement**
  (§13.3), and `Keepalive` and `PersistentKeepalive` at theirs (§7.5),
  the last two additionally waiting on the absence of a **pending**
  contested mark, with the vetoed-keepalive state announcing the death
  clock in their stead (§7.5, ruling 265). The sizes asked differ —
  39 B for the probe's challenge datagram (§13.4), 30 B for §3.4's empty
  plaintext — but the predicate is one predicate; `Liveness` is armed by the first
  **marking or ack-eliciting** send after an authenticated, window-fresh
  receive, is not re-armed by later sends of either kind, and is disarmed
  and re-anchored by every such receive for the connection's `D_eff`
  (§7.4). `Keepalive`, `Contested`, and `Liveness` therefore read the
  immutable profile stamped at birth; `PersistentKeepalive` reads its
  own optional interval but validates it against that same `D_eff`. The
  endpoint core's deadline is the min over its representable pendings'
  retransmit/give-up deadlines, parked-intro expiries, and timestamp-guard
  orphan-aging deadlines (§17.1). Unreachable members remain in their
  owning state under the rule above.
- **`handle_timeout` is idempotent**: each due timer is stopped before its
  logic runs, so spurious or repeated calls no-op. For `Loss`/`Pto` the
  idempotency additionally rests on synchronous sealing (§16.7).
- **The drain-before-deadline order is normative.** **[RATIFIED 2026/08/18
  — ruling 271]** The shell's event loop MUST poll inbound datagrams
  **before** an expired deadline. A deadline already due when it is read
  therefore fires on the first pass on which no datagram is ready, which is
  what makes it expressible as *"the end of the receive drain"* (§12.4) and
  the only reason §12.4's coalescing needs no core API. Polling the deadline
  first is not a correctness failure and moves no wire byte a per-packet
  conformance check can see; it silently restores the pre-271 every-2nd
  emission — the deadline armed at `now` by the 2nd arrival fires before
  the 3rd is read — forfeiting the coalescing win with nothing red to show
  for it.
- **Equal-deadline priorities** (normative). **[RATIFIED 2026/08/15 —
  ruling 76]** This list is **exhaustive**: every pair of deadlines that
  can fall on one instant is ordered here, because §16.4 makes generation
  order normative and an unordered pair would make that claim hollow
  exactly where two timers collide. The governing principle, from which
  the endpoint's cases follow: **a terminal outcome precedes a routine
  one, and state removal precedes emission.** At the endpoint that gives,
  in order — (1) handshake **give-up**, (2) **intro expiry**, (3)
  **guard-orphan aging**, (4) **retransmit**. Give-up beating a
  same-instant retransmit is an instance of the principle rather than a
  special case. Per connection, loss detection beats PTO and exactly one of
  the two fires per evaluation; teardown collection (liveness,
  `CloseLinger` expiry, then `Contested`) precedes keepalive
  evaluation — a session already collected for teardown owes no
  keepalive, and `Liveness` beating `Contested` at the same instant is
  the harmless ordering, both being `ConnectionLost::TimedOut` (§15.4);
  `AckDelay` fires after the
  loss/PTO evaluation at the same instant (the owed ACK then rides any
  probe or retransmission that evaluation produced, §8.5); and
  `PersistentKeepalive` is evaluated last — any marking send the instant
  produced re-arms it (§7.5).
  **[AMENDED 2026/08/16 — ruling 174]** *Two relations were missing, and
  "exhaustive" is the same self-certifying scope claim as §13.6's title
  one section apart.* The relations above leave
  `{Liveness, CloseLinger, Contested}` unordered against
  `{Loss, Pto, AckDelay}`, and `{Loss, Pto, AckDelay}` unordered against
  `Keepalive`. Both are stated now, and the enum's declaration order in
  `timers.rs` — which froze this answer so slices 5 and 7 would not
  re-derive it from prose — is authoritative:
  - **Teardown collection precedes loss/PTO evaluation.** A connection
    collected this instant evaluates no loss and sends no probe; this one
    *is* an instance of the governing principle (a terminal outcome
    before a routine one, state removal before emission).
  - **`Loss`, `Pto` and `AckDelay` precede `Keepalive` evaluation.** This
    one does **not** follow from the principle and must be stated rather
    than derived: `AckDelay` before `Keepalive` is
    emission-before-emission, which the principle does not reach. The
    practical reason it is the right order is §7.5's — a keepalive
    evaluated after the instant's other output sees an accurate
    `last_send`.

  Only five of the eight timers are ever armed before slice 7; slice 7
  arms `Contested`, `Keepalive` and `PersistentKeepalive` for the first
  time, which is what makes **every** collision in those two groups newly
  reachable, and is why the gap had to close before a blind test author
  derived the opposite from this list and asserted it.

**The lateness bound.**

| Parameter | Value |
|---|---|
| `L` (shell lateness bound) | 250 ms |

> Every announced deadline `D` fires no earlier than `D` and no later than
> `D + L`. `L` is a **conformance parameter of the shell, not of the
> protocol**: the cores expose exact representable deadlines, and a shell
> may batch or tick provided it honours `L`.

### 16.6 RNG

The endpoint core owns one seeded RNG (constructor `[u8; 32]`;
config-supplied for tests, OS entropy otherwise). Every index, jitter
draw, and — via the forced increment — timestamp draw comes from it. At
connection creation the endpoint draws a 32-byte **sub-seed** for the
connection core (drawn unconditionally, so connection-side randomness can
never perturb the endpoint's draw order). One root seed
reproduces the whole system.

**[AMENDED 2026/08/16 — rulings 208, 210(b)]** *The sub-seed had no
consumer when it was specified; it has one now, and this is the seam that
decides where the draw happens.* §7.3's 8-byte address-validation
challenge is drawn from the **connection** core's sub-seed, at every
arming. It could not have come from the endpoint RNG whatever the wording
preferred: both arming sites — a roam, and the first anchor from a msg1
source — are inside `core::Connection`, and **`Connection` cannot reach
`Endpoint`'s RNG**. Ruling 208 as first written said "the endpoint RNG",
naming a path that does not exist; ruling 210(b) corrects it. The
unconditional draw above is what makes the correction free: the sub-seed
was already there, already reproducible from the root seed, and adding a
consumer perturbs no existing draw order, so every seeded test that passed
before this change still sees the same endpoint-side sequence.

The challenge inherits §16.6's security posture in full, and the
inheritance is load-bearing rather than incidental: it must be
**unpredictable to a party that has not received it**, for the same
reason session indices must be — so a config-supplied seed makes the
challenge predictable exactly as it makes indices predictable, and the
test-only rule below governs both.

Session and pending indices MUST be unpredictable to an off-path
observer — index unpredictability is load-bearing for §5.5's
on-path-only completion spend and §15.2's authenticated-only linger
reply — so the config-supplied seed is a **test-only facility**: a
production endpoint seeds from OS entropy, and a build that accepts a
caller-chosen seed is security-relevant and must be feature-gated or
documented as such.

### 16.7 Plan-seal-commit; sealing is synchronous

Packetisation is **plan, seal, commit**: build the packet plan, seal it,
and **only on seal success** commit the recovery transition (dequeue, mark
transmitted, clear the pending ACK, `on_sent`, arm timers). On seal
failure nothing moved — a seal error can never strand frames outside both
the pending set and the loss tracker; the only reachable seal failure is
nonce exhaustion, which is terminal (§7.9). Seal returns
`(counter, bytes)`. Sealing — commit included — executes **within the
mutating call that triggers it** (`handle_timeout`, `handle_datagram`,
the application surface), never lazily inside `poll_output()`; this is
what makes `Loss`/`Pto` idempotency real (a repeated `handle_timeout`
before a drain observes the deadline already advanced by the committed
probe).

### 16.8 The no-blocking invariant

Every driver→handle delivery is a bounded channel with a non-blocking
policy or a oneshot reply that cannot block the driver. The accessors are
**synchronous reads of a shared cell the driver updates** — never driver
round-trips. Per-stream wakers key the shell's blocked-readers/
blocked-writers maps (the quinn pattern): a stream verb that would wait
parks its waker under its `StreamRef` and is woken by the matching
`ConnEvent`. **[AMENDED 2026/08/15 — ruling 117]** This sentence said
`StreamId`, which ruling 95 had already made unbuildable when it keyed
the four stream-naming `ConnEvent`s by `StreamRef`; the token was not
swept. A `StreamId` is fixed by an opener parity that §6.7's tie-break
can **invert**, so a map keyed by it would need rekeying at every install
and a park that straddled one would look up a key that no longer exists.
`StreamRef` is the only key stable for the life of the stream, and it
stays the key even though ruling 116 removes the pre-establishment case
that first motivated it. The driver never performs a blocking send toward a handle.
The shell is deadlock-free by construction. One class is exempt from the
non-blocking drop policy by prohibition: **reliable data is never
droppable** — stream bytes, messages, and claim-pending receive state
stay in the core until the application takes them (§16.4's pull model,
§10.6); the bounded channels carry signals and wakes, not reliable
payloads.

### 16.9 Early sends

Queued work before establishment is **ordinary work**: the connection core
exists from `connect()`, and early stream opens, writes, messages, and
datagrams land in ordinary stream/queue state, pumping when a session
installs — delivered exactly once after establishment, lost if the connect
fails (the failure surfaces through `Connecting`). There is no special
pre-establishment mechanism.

**Stream identity before establishment.** Wire stream IDs encode opener
parity, which is fixed only at establishment (a tie-break loss makes the
dialler the acceptor — §6.7, §9.1), so **stream IDs are assigned at
establishment**: pre-establishment handles hold core-internal indices, no
frame is emitted before install (nothing sends until a session exists),
and `id()` returns `None` until the connection is established (§16.2). On
install the core maps its internal indices onto the parity the outcome
dictates, in open order — the on-wire IDs are identical whichever
resolution the race takes.

**[AMENDED 2026/08/15 — ruling 116]** *This section is a guarantee about
the **core**, and the v1 application surface publishes no route to the
window it describes.* §16.2 hands out a `Connection` only once the
session is installed, so an application cannot open or write a stream
before establishment, and it is not an oversight that it cannot. **The
window is crossed inside the core, and that crossing is real**: §6.7's
tie-break and §6.4's replacement both install a session underneath a
connection core that already holds queued sends, which is where "pump on
whichever session installs" earns its keep and where ruling 95's
`StreamRef` is exercised. What an application-visible route would buy is
**one task wake-up and not one round trip** — this section itself forbids
emitting a frame before install, so the handshake costs the same either
way — and it would cost the totality of `session_id()`, which §16.2
declares without an `Option` and ruling 89 derives from a handshake hash
that does not yet exist. Publishing such a route on a later wire line is
additive; it is deliberately not on this one.

### 16.10 Kernel-free drivability

The whole protocol is drivable without a kernel: two endpoints over
`testutil::FlakyWire` on tokio's **paused clock**, with every timer — the
5 s/10 s/15 s/25 s/25 ms/90 s family included — resolving in
virtual time. New behaviour gets a paused-clock flow test, not a sleep
(Appendix B). `FlakyWire` is a `Wire` (§16.3) and nothing else: the trait
is the whole of the substitution, which is why the same flow tests run
unchanged over a real `tokio::net::UdpSocket`, and why a `Wire` that
fails its sends is the only fixture ruling 49's trace obligation needs.

**The fixture surface, attested.** **[RATIFIED 2026/08/14 — ruling 60]**
This section named `FlakyWire` alone while the fixture it belongs to has
three parts, all of which a downstream crate already depends on by name:

- **`testutil::Network`** — the in-memory routing fabric. It owns the
  address→endpoint map and moves datagrams between `FlakyWire`s; it is
  what makes "two endpoints without a kernel" a single object rather
  than a test-local convention.
- **`testutil::FlakyWire`** — a `Wire` (§16.3) attached to a `Network` at
  one address.
- **`testutil::FlakyPolicy`** — the impairment applied to each datagram:
  loss, reordering, duplication, and **send failure**. It **MUST** be
  deterministic under a caller-supplied seed. A flow test that cannot be
  replayed byte-for-byte from its seed is not a regression test, and the
  loss-dependent behaviour in §13 and §7.5 is exactly where a
  once-in-a-thousand-runs failure would otherwise be unactionable.

**Send-failure injection is required, not optional.** Ruling 49 makes a
failing `send_to` a trace obligation, and Appendix B's obligation for it
is unreachable without a fixture that can fail a send. It belongs in
`FlakyPolicy` from the start: retrofitting it later would rewrite the
tests of every slice that had already ridden the fixture.

These three names are **contract**, on the same terms as §18.2's trace
targets: renaming or dropping one is a protocol revision, because a
consumer's test suite is built on them.

### 16.11 The composability surface

**[RATIFIED 2026/08/14 — rulings 55, 56, 57, 58]**
§16.2's verbs are the whole of the protocol surface. This section adds
**no verb and no state**: it fixes the shape in which those verbs meet
the async ecosystem, because three of the four decisions below are
guessable wrongly and one of them can reintroduce a hazard §10.6 exists
to forbid. Everything here is **shell-layer**: no core type, no wire
byte, no timer. An implementation may place it behind cargo features.

**A `Connection` is a multiplexer, so the byte-oriented object is a
stream.** That substitution is the whole of the mapping; the rest
follows.

```rust
impl tokio::io::AsyncWrite for SendStream {}
impl tokio::io::AsyncRead  for RecvStream {}

pub struct BiStream { /* SendStream + RecvStream */ }
impl BiStream {
    pub fn split(self) -> (SendStream, RecvStream);
    pub fn join(send: SendStream, recv: RecvStream)
        -> Result<Self, (SendStream, RecvStream)>;  // ruling 120; Err = not the
                                                    // same stream — [AMENDED
                                                    // 2026/08/18 — ruling 259]
}
impl tokio::io::AsyncRead  for BiStream {}
impl tokio::io::AsyncWrite for BiStream {}

impl From<ReadError>  for std::io::Error {}   // §16.11.1 — the full table
impl From<WriteError> for std::io::Error {}   // §16.11.1 — the full table
```

#### 16.11.1 The `io::ErrorKind` mapping

**[RATIFIED 2026/08/16 — ruling 227.]** Until this ruling, the two lines
above read `Reset → ConnectionReset` and `ConnectionLost → NotConnected /
BrokenPipe`. The first is unambiguous. The second was **a slash between
two kinds with no rule for choosing**, over a `ConnectionLost` with seven
variants, and said nothing about `WriteError::Finished` at all — working
rule 8's defect class, a stated construction with an unstated scope, in a
conversion an `AsyncRead`/`AsyncWrite` consumer meets on every error.

| Variant | read → `ErrorKind` | write → `ErrorKind` |
|---|---|---|
| `Reset(code)` | `ConnectionReset` | `ConnectionReset` |
| `Finished` | *(not a `ReadError`)* | `BrokenPipe` |
| `ConnectionLost(TimedOut)` | `TimedOut` | `TimedOut` |
| `ConnectionLost(NonceExhausted)` | `ConnectionAborted` | `BrokenPipe` |
| `ConnectionLost(LocallyClosed)` | `NotConnected` | `NotConnected` |
| `ConnectionLost(PeerClosed { .. })` | `ConnectionAborted` | `BrokenPipe` |
| `ConnectionLost(ProtocolViolation { .. })` | `ConnectionAborted` | `BrokenPipe` |
| `ConnectionLost(Replaced)` | `ConnectionAborted` | `BrokenPipe` |
| `ConnectionLost(EndpointDropped)` | `NotConnected` | `NotConnected` |

**The rule, stated so it can be judged rather than memorised:** on the
write side, a peer or transport that went away *under a writer* is
`BrokenPipe`; a connection *this side* never had or gave up is
`NotConnected`. This reads the original slash as a **variant** split
rather than a read/write direction split — which is how it was written,
the comment having sat on the `WriteError` line alone.

`TimedOut` is lifted out of both columns because `io::ErrorKind::TimedOut`
exists and a liveness-timeout death is exactly what it names; collapsing it
would make every death look alike to a consumer whose only view is
`io::Error`.

Two binding details, both directly testable:

- **The `WriteError` match is exhaustive, and deliberately so.**
  **[CORRECTED 2026/08/16 — ruling 238.]** This bullet read: *"`WriteError`
  is `#[non_exhaustive]` (ruling 61 reserves `Stopped`), **so** the
  conversion needs a `_ =>` arm … and must not be `unreachable!()`."* The
  conclusion was aimed at the right hazard and the *"so"* was false.
  `#[non_exhaustive]` is **inert inside the defining crate**, and the
  conversion can only live there (the orphan rule puts
  `impl From<WriteError> for io::Error` in slither or nowhere), so it never
  bites. `src/error.rs`'s `write_error_is_exhaustive_in_crate` already
  proves this.
  A `_ =>` arm therefore does not future-proof the conversion — it
  **hides** the future. An exhaustive match turns the day `Stopped` lands
  into a **compile error at the exact site that must be updated**, which is
  strictly stronger than silently mapping a new variant to `Other`, and it
  is what the original *"must not be `unreachable!()`"* clause was reaching
  for: a variant that cannot compile cannot panic.
- **The inner error is preserved.** Every arm constructs
  `io::Error::new(kind, err)`, so `e.into_inner().downcast::<ReadError>()`
  recovers the original including a reset's `u64` code. The `ErrorKind` is
  a lossy projection for ecosystem code; nothing is discarded.

- **[Ruling 55 — `open_bi`/`accept_bi` yield `BiStream`.]** §16.2's
  signatures return the duplex object rather than the
  `(SendStream, RecvStream)` tuple, and `.split()` recovers the tuple, so
  nothing is lost. The duplex object is what the ecosystem consumes: it
  is what makes `Framed<BiStream, C>` and `copy_bidirectional` work
  without an adapter. Shape change only; no semantic change.
- **[Ruling 56 — `poll_flush` is a no-op returning `Ready`.]** Bytes
  accepted by `poll_write` are already in send state, and `poll_write`
  accepts only what flow-control credit admits (§10.1), so there is no
  shell buffer to push. **`AsyncWrite::flush` is not delivery
  confirmation** — promising otherwise would make it a second, weaker
  `acked()` and put the two in competition. Rustdoc must say so at the
  impl.
- **[Ruling 57 — `poll_shutdown` is `finish()` *and then* `acked()`.]**
  The weaker reading (`finish()` alone) is prior art elsewhere and is
  **rejected here** for the reason ruling 47 exists: the natural last act
  of a transfer is `copy(..).await; shutdown().await`, and under the weak
  reading it loses its tail at the path's loss rate, silently — S28's bug
  reachable a second time, through the `AsyncWrite` surface, by an
  application that never touches `close()`. It cannot hang past the
  connection's own death: a dying connection resolves the wait in error.
- **[Ruling 58 — an adapter never claims ahead of its consumer.]**
  **Normative.** A `Stream` adapter over `recv_message`, `recv_datagram`,
  `accept_bi`/`accept_uni` or `notified()` claims **at most one item, and
  only from inside `poll_next`**. No prefetch, no read-ahead task, no
  intermediate queue. §16.4's pull model is precisely what keeps reliable
  data in the core until the application takes it (§16.8), and an adapter
  that claimed ahead of its consumer would rebuild the unbounded shell
  queue §10.6 forbids — while looking like an ordinary ergonomic
  convenience. This is the easiest way to get the composability layer
  wrong, and Appendix B pins it.

The `Stream`/`Sink` faces (`messages`, `datagrams`, `incoming_bi`,
`incoming_uni`, `notifications`, `incoming`, and the two sinks), the codec
constructors, and any `tower::Service` shapes are ordinary adapters over
§16.2's verbs under ruling 58 and are otherwise unconstrained by this
document.

**[ruling 229]** `incoming` — the `Stream` over `Endpoint::accept()` — was
missing from that list, which named seven faces, none of them the
endpoint's. **Working rule 8 reads a list as exhaustive whether or not it
says so**, and this is the eighth ruling of that shape. It is added rather
than the plan's eighth face being struck, because ruling 58 governs it
identically and nothing in this section wanted it excluded.

`Endpoint::accept()` is on ruling 53's **channel** side (§6.2 puts the DH
on the driver task), so it is the one adapter with no `poll_*` behind it.
An implementation must supply one — see ruling 229 — rather than boxing an
in-flight future inside the adapter, which is the cost the paragraph below
ruling 53 names as what that ruling exists to avoid.

**Termination. [RATIFIED 2026/08/16 — ruling 226]** The `Result`-carrying
faces **never end**: after the connection dies they yield
`Some(Err(ConnectionLost))` for as long as they are polled, and never
`None`. This is faithful to the verbs they wrap — the underlying `poll_*`
re-report the latched death indefinitely, and `ConnectionLost` is `Clone`
for exactly that reason — and it keeps the *reason* recoverable, which a
`None` destroys. A consumer wanting the terminating shape composes one
combinator; a consumer handed `None` cannot recover what killed it. The
rustdoc on every such adapter must say so, because a bare
`while let Some(_) = s.next().await` spins.

**The `!Send` boundary, stated so it is not discovered late.** §16.3's
driver is `!Send` by requirement, not by accident, and S21 is why. The
adapter surfaces above compose regardless: `tokio::io`'s traits and
combinators, `tokio_util::codec`, `futures`' `Stream`/`Sink`
combinators, and `tower::Service` itself carry no `Send` bound. What
does **not** compose is anything that spawns onto a work-stealing
executor — buffered/spawn-ready service layers, boxed-`Send` service
objects, hyper, and plain `tokio::spawn` on any slither handle
(`spawn_local` is the substitute). A `Send` façade over a driver on its
own thread would close that gap and is **deliberately not specified
here**: it re-crosses the core→shell seam with channels, and the handle
shapes above are already compatible with adding one later without a
breaking change.

## 17. Endpoint-global state

Four pieces of state are endpoint-core-global; none may be pushed into a
connection.

### 17.1 The timestamp guard

Per-remote-static greatest initiation timestamp (§5.3). Admission (check
**and** record) happens at `authenticate()` on the staged path, at a
re-homed `accept()`'s candidate admission (§6.4), and at the internal
tie-break's admit step (§6.6 step 4); one further write records **without
admitting** — the tie-break **winner**'s side, which authenticates the
loser's initiation post-`ss`, drops it, and records its timestamp anyway
(§6.7), reached either by §6.6's internal route or by §6.4's PENDING
branch, which refuses the `accept()` with `AcceptError::Stale` and keeps
the record. All four writes are post-`ss`, so **only key-holders write
guard entries**; the record is made on a full admission or on that
authenticated winner-side drop, and nowhere else. A record made for a
candidate whose `accept()` then returns `AcceptError::Stale` is
**reverted** — §6.4's ordering clause pins that, and the winner-side
record is its one deliberate exception.

The guard alone does **not** make the proven-LIVE replacement of §6.4
safe, and never did. It rejects any candidate whose timestamp is ≤ the
greatest this endpoint has admitted for that static — which kills an
older-or-equal replay — but a genuine retransmit withheld off the wire
carries a strictly greater timestamp by construction (§5.5) and passes,
and where we have admitted nothing for that static the check passes
vacuously. What makes the replacement safe is the **pair**: the guard
plus the per-connection replacement basis (§17.4), which refuses any
candidate not strictly newer than the initiation that established the
live connection, and refuses every candidate outright on a connection we
dialled, where no basis exists. A withheld-newer replay still reaches
the application — as an `Intro` that destroys nothing until it is
accepted (§6.4).

- An entry is **pinned** — never evicted — while a live `Connection`, an
  in-flight outbound pending, or a staged mid-state exists for its static.
  **[RATIFIED 2026/08/16 — ruling 187]** "In-flight outbound pending"
  here carries §5.4's definition without qualification: **membership in
  the pending tables**, so a **minted** pending pins from the instant
  `connect()` mints it, before `start_attempt` has sent anything. The pin
  exists so that the entry the attempt is *about to need* — to validate
  its msg2 or its replacement — survives until it arrives, and the
  narrower reading opens a window in which exactly that entry can be
  evicted, invisibly, until an eviction and a dial coincide. The
  extension is **self-bounding**: the pin lasts as long as the pending,
  and dropping a `Connecting` empties the pending tables synchronously
  (ruling 50), so no minted-and-abandoned pending holds an entry. Ruling
  37's `HANDSHAKE_GIVEUP` exemption and the 1024 cap are untouched.
  For a staged mid-state (whose static is merely claimed until
  `authenticate()`) the pin never *creates* an entry — a bounded
  exception to §6.1's nothing-durable rule, flipping a bit on an entry a
  key-holder already wrote, reverting on drop.
- **The pin outlives its connection by `HANDSHAKE_GIVEUP`.** An entry
  written by the internal tie-break's admit step (§6.6 step 4) or by a
  **winner-side record** (§6.7, including §6.4's PENDING branch when we
  are the tie-break winner) stays exempt from orphan aging **and** LRU
  eviction for `HANDSHAKE_GIVEUP` (90 s) after the connection it belongs
  to dies — the connection the admission installed, or, on the
  winner side, the connection our own outbound completed. If that
  outbound never completed, the 90 s runs from its own
  `HANDSHAKE_GIVEUP` expiry instead. Only when the extension lapses does
  the entry demote to an ordinary orphan and enter the LRU below.
  Without this the guard entry is exactly what §6.7's single-use bound
  rests on, and aging it out at `TS_GUARD_ORPHAN_TTL` (= `INTRO_TTL`,
  ruling 70) re-arms the
  replay it was written to stop. The rationale for the value and the
  cost: 90 s is long enough to cover an application's reconnect backoff
  — the interval over which a re-armed replay would actually meet a new
  pending to cancel; what is retained is a **per-static timestamp
  (≈ 45 B), not a session**, so nothing keyed, buffered, or
  connection-scoped is held open by it; the `TS_GUARD_ORPHAN_CAP` of
  1024 still bounds the tier the extension eventually feeds; and
  mitigation (i) below still holds, so an attacker cannot mint these
  entries by authenticating and dropping — every one of them costs a
  genuine tie-break admission or a genuine authenticated winner-side
  drop, both of which require the peer's key.
**[RATIFIED 2026/08/15 — ruling 73]** An orphan's `TS_GUARD_ORPHAN_TTL`
runs from the instant its **last pin is released**, not from its last
successful admission. §17.1 defines an orphan as a *dead-connection*
entry, so an entry cannot age **as an orphan** before it is one. Under
last-admission a connection outliving the TTL has `last_admitted` frozen
at accept time, so the moment it retires the entry is **already past its
deadline** and dies at the next sweep with **no orphan window at all** —
deleting the guard's replay protection precisely for the connections that
held it longest, and precisely when a captured initiation could be
replayed. Mitigation (iii) is untouched: LRU **recency** remains
admission-only; it is the **aging clock** that starts at release.

**[RATIFIED 2026/08/15 — ruling 77]** Only the release of a
**key-holder-proven** pin starts that clock — a live connection, an
in-flight outbound pending, or a `Proven` chain. A merely **`Claimed`**
chain's pin still bars eviction, as the bullet above requires, but does
**not** restart aging. Reaching `Claimed` costs 1 DH and proves nothing
(§6.1: the claimed static is attacker-choosable), so without this
narrowing anyone able to send a mac1-valid msg1 naming a static that
already holds an entry could restart that entry's clock at will and defer
mitigation (ii) indefinitely. The exposure is bounded — the record is
*retained*, not destroyed, and `TS_GUARD_ORPHAN_CAP` with admission-only
recency still evicts — but an unauthenticated party must not move a
timer. This is §6.1's "nothing durable may be keyed on the claimed
static" reaching one step further than §6.1 states it: not merely no new
durable state, but **no control over the lifetime of existing state**.

- All other entries (orphans — dead connections) live in a bounded LRU
  with timer aging:

| Constant | Value |
|---|---|
| `TS_GUARD_ORPHAN_CAP` | 1024 orphan entries (≈ 45 B each) |
| `TS_GUARD_ORPHAN_TTL` | **[RATIFIED 2026/08/15 — ruling 70]** `= INTRO_TTL` (15 s). An *alias*, not a second literal: one value, two names, so the two cannot drift. |

**Honesty clause and mitigations.** **[RATIFIED 2026/08/14, amended
2026/08/14]** Evicting an orphan
re-admits a replay of that static's last initiation: the replayed msg1 is
genuine, authenticates, and surfaces as a fresh `Intro` — or, if
accepted, a half-open session reaped by liveness in 25 s (WireGuard
accepts the same on responder restart). Pinning guarantees eviction never
touches an established connection's replacement protection. The clause is
completed rather than left incidental: eviction is attacker-triggerable on
demand (orphans require keys, but self-generated statics are free —
~1024 authenticate-then-drop chains flush the tier at ~2048 DH of our
cost); LRU order is adversarially optimal (longest-idle legitimate peers
evict first); and the observable consequence of a re-admitted replay is a
spurious **unaccepted** `Intro` attributed to a real peer at an
attacker-chosen address — never the destruction of a live connection.
Why never, stated for the **two cases separately**, because the guard
does different work in each:

- **A static we accepted.** We hold an entry, pinning protects it while
  the connection lives, so it is never evicted and an older-or-equal
  replay of it still dies at the guard. A strictly-newer withheld replay
  survives the guard, as it always has, and is then measured against that
  connection's replacement basis (§17.4, §6.4).
- **A static we only ever dialled.** We hold **no entry at all** — every
  §17.1 write site is a post-`ss` read of an *inbound* msg1, and a
  `connect()` completed by msg2 writes nothing — so there is nothing to
  pin, nothing to evict, and **the guard offers this case nothing**: every
  candidate passes it vacuously, however old. The whole of the replacement
  protection here is the **basis**, which is `None` on a connection we
  dialled and therefore refuses every candidate outright (§17.4, §6.4).
  The honest consequence is that a captured initiation from such a peer —
  arbitrarily old, from a long-dead session — surfaces as a spurious
  `Intro` attributed to that peer **indefinitely and repeatably**, not
  once: mitigation (i) below returns the guard to empty after each
  refusal, so the same single captured packet is replayable forever. Each
  replay costs one stage-0 slot and whatever DH the application chooses to
  spend (§6.9). This is the majority case for a client, which typically
  only ever dials.

A replacement in any case executes only at `accept()`; if the application
accepts one, the resulting half-open session dies at 25 s liveness and
leaks nothing, but
"peer is online" side-effects fire on a forgery. The ruled mitigations:
**(i) no-orphan-on-reject** — a static authenticated and then rejected
without ever being accepted writes no orphan (its record drops with the
chain; a pre-existing entry reverts), and the revert applies **whether
the rejection is a dropped chain or an `accept()` that returns
`AcceptError::Stale`** (§6.4's ordering clause pins this), so the
authenticate-then-drop flood cannot mint orphans by either route; the
sole record that survives a `Stale` is the tie-break winner's, which is
deliberate (§6.7). The cost, stated honestly, is that a replay of such
a never-accepted initiation can be re-authenticated later, surfacing only
as a fresh `Intro`. **(ii) Timer aging** — orphans age out on an
`TS_GUARD_ORPHAN_TTL` timer as well as the LRU cap. **(iii) LRU "use" is
admission only** — recency refreshes on a successful post-`ss` record,
never on a failed check, keeping the write path key-holder-only.

### 17.2 `last_init_timestamp`

The endpoint-global outbound monotonic forcing (§5.3). It survives across
connection generations to the same peer — close-and-reconnect still emits
strictly greater — so endpoint scope is the correct superset.

### 17.3 The index tables

`index → connection` (sessions) and `pending-index → connection`
(in-flight initiations). **Index minting draws a random nonzero `u32` and
re-draws while the value is present in *either* table** — drawn from the
endpoint RNG and off-path-unpredictable by requirement (§16.6) — required
because
a pending's msg1 index graduates into the session index on completion;
this closes the route-stealing collision. The corollary, stated: **a
datagram that routes by index but fails to open touches nothing** — not
liveness, not roaming, not the replay window (§7.2's decrypt-first
ordering guarantees it). It matters because a freed-then-re-drawn index
makes stale traffic land on the wrong connection routinely; such traffic
is inert.

### 17.4 The static map and the hint set

The `static → connection` map (§16.1's invariant made concrete) plus the
pending tables' dialled addresses (§17.3), which are §6.5's hint set —
the probed set for the internal tie-break is the pending outbound
remotes alone (§6.5). Established connections contribute no hints:
their initiations take the ordinary staged path (§5.4), so the endpoint
tracks no per-connection address (the connection core owns its own
endpoint address, §7.3).

**[RATIFIED 2026/08/16 — ruling 178]** **Membership in the pending
tables *is* the PENDING predicate**, and this section is where the
tables live, so it is stated here as well as at §5.4 and §6.4. An entry
counts from the instant `connect()` **mints** it — not from the instant
its msg1 leaves the host — and stops counting when the pending
completes, gives up, or is cancelled (§16.3, ruling 50). Ruling 90's
`mint_pending`/`start_attempt` split is what made *"an in-flight
outbound initiation exists"* ambiguous, and ruling 91 recorded the
ambiguity as open across §6.4, §6.5 and this section; it is closed on
the table-membership reading. There is no separate per-static flag: the
three readers of the predicate all read these tables, so an
implementation cannot diverge from this rule without inventing state
this document does not have.

**The replacement basis.** Each entry of the `static → connection` map
carries one further field, `replacement_basis: Option<Timestamp>`,
alongside the connection it names — connection-scoped state, endpoint-held
because the endpoint is where §6.4's admission decides. It is
`Some(t)` where `t` is the timestamp of the msg1 that **established** the
connection while we were the **responder** — a staged `accept()`, a
re-homed `accept()`'s admitted candidate, or the tie-break loser's admit
step (§6.6 step 4) — and `None` when we **dialled** (a `connect()`
completed by msg2, or a tie-break we won: neither teaches us any
timestamp of the peer's, because msg2 carries no payload, §5.2). It is
written once at install, never updated, and dies with the connection; a
replacement installs its own. §6.4's admission is the only reader.
The basis is not the timestamp guard (§17.1): the guard is
endpoint-global, evictable, and orphan-tiered, while the basis lives and
dies with one connection and cannot be flushed by an attacker's
authenticate-then-drop churn. One `Timestamp` per connection
(`TIMESTAMP_LEN`, §5.2), inside §17.5's per-connection term.

**What a `None` basis costs, stated correctly.** A `None` basis refuses
every replacement, and that refusal is what keeps a passively captured
msg1 from destroying a connection we dialled (§6.4). Its price is **not**
merely a bounded delay: absent an attacker the peer's restart resolves in
at most `D_eff` (§6.8), but an attacker who drips withheld genuine
Data into the zombie holds its liveness clock open indefinitely, and
against a `None` basis no reconnect can ever displace it — a permanent
wedge, not a delay (§6.8, §7.4). That is why ruling 36 attaches the
**contested-connection probe** to precisely this refusal (§7.5): the
basis rule is unchanged and still refuses, but the connection it protects
must now prove itself with an ACK covering a counter sealed after the
refusal — the probe floor of ruling 41, which any post-mark packet's ACK
satisfies — so a `None` basis can no longer shelter a zombie. Ruling 41
matters to *this* section specifically: because the guard is vacuous for
a dialled peer and reverts on `AcceptError::Stale`, the attacker's
captured msg1 is replayable without limit, so a per-refusal death
probability had to be zero for a live peer rather than merely small. A `Some(t)` basis needs
none of this — it can measure a candidate, so a genuine reconnect
displaces the zombie at the first `accept()`.

*Footnote, so it is not re-proposed without a fresh ruling.* The
root-cause fix — carrying a 12-byte timestamp in **msg2**, which would
make the basis `Some(t)` on both sides and delete the asymmetry entirely
— was **considered and declined as wire-affecting**: it moves
`IK_MSG2_LEN` and `RESP_PACKET_LEN` off their pinned values (§2.3, §3.3).
The contested probe is therefore a **mitigation**, not a closure: it
guarantees the zombie dies, not that a dialled connection can measure a
replacement.

### 17.5 State ceilings

The composite bound, in one place, so an application can size its accept
policy:

| State | Ceiling | Worst case |
|---|---|---|
| stage-0 entries + consumed chains | one budget of `INTRO_QUEUE_CAP` (1024) slots | ≈ 484 B each — 288 B struct + 196 B msg1 heap, measured (ruling 272; the pre-272 ≈ 220 B counted the raw bytes alone) — ≈ 496 KB |
| staged mid-states (consumed chains + carried pre-read entries) | ≤ `INTRO_QUEUE_CAP` | ≈ 0.5–1 KB live key material each, ≈ 1 MB — and each holds the endpoint's static provider: for a hardware/enclave static this is up to 1024 concurrent provider handles, an operationally scarce resource the TTL bounds in time |
| timestamp-guard map | `TS_GUARD_ORPHAN_CAP` (1024) orphans + pinned (≤ connections + pendings + mid-states) | ≈ 45 B each |
| established connections | **application-governed — unbounded by the protocol**, with the caveat below | per connection, the receive commitment is the advertised credit — ≤ the advertised connection window (`INITIAL_MAX_DATA`, 1 MiB, unless config raised it — ruling 259(viii), the operator's deliberate purchase) plus per-stream book-keeping and reassembly metadata bounded by §10.6's ceiling — `REASSEMBLY_CHUNKS_MAX` at the ratified window and `window / REASSEMBLY_MIN_CONFORMING_FRAME + 1` above it (ruling 270), ~40 B per stored range: ~40 KiB against 256 KiB of credit at the ratified window, ~320 KiB against 8 MiB at a raised one — under 4 % either way, which is what makes the credit term the dominant term rather than a 25–50× underestimate — plus the datagram queues (≈ 146 KiB, §11.3), the replay window (256 B), a sent map bounded by cwnd **plus the §14.5 admission exemptions in flight** (the one-packet PTO probe of §13.4 and, at most, one contested-connection probe — each ≤ `MAX_DATAGRAM`, so the overshoot is ≤ 2 400 B and never grows with the attack), the contested mark itself, one `(probe_floor, deadline)` state per connection whose concrete deadline may be unreachable under §16.5, **§7.3's amplification state — two byte counters, one 8-byte outstanding challenge and one validated flag (an `Option<[u8; 8]>` carries both), plus at most one 8-byte `PATH_RESPONSE` owed to the peer, overwritten by a newer challenge and never queued; per connection and never per address (rulings 170, 208)** — and ruling 46's notification slots (one per kind, §16.2 — O(1) by construction, which is why they need no queue bound here); the credit term dominates |

**The caveat on the sent map, stated because ruling 43 changed what it
covers.** "Bounded by cwnd" is exact for congestion-controlled output
and would be false if read as covering every packet in flight: §14.5
exempts the PTO probe and the contested-connection probe from the
**admission gate**, so both may be sent with the window already full.
Neither is exempt from **accounting** — both are ack-eliciting, so
§13.5 inserts them and their sizes feed `bytes_in_flight` — which is
what keeps this row's arithmetic honest, and it is why the exemption is
safe: the overshoot is a fixed small constant (§13.4 sends one probe per
firing, and ruling 41's collapse allows at most one contested probe per
connection), not a term an attacker can drive. Had the exempt probes
been left untracked, the sent map would have been bounded by cwnd only
in the sense that it did not contain the packets it was missing.

The contested state is **O(1) per connection** for the same reason. An
earlier draft let each refusal add a mark with its own deadline, which
would have made this row's per-connection term depend on how many Intros
an attacker chose to supply; ruling 41 collapses concurrent marks into a
single floor-and-deadline pair, so a connection under a refusal flood
holds exactly the same state as a connection refused once (§7.5, §16.5).

## 18. Errors and observability

### 18.1 The error taxonomy

Closed and normative: every variant this specification names appears here
exactly once; `Superseded` appears nowhere (§6.3), and
`AcceptError::AlreadyConnected` appears nowhere — a proven-LIVE
`accept()` is a replacement (§6.4), so the variant is unreachable and
deleted.

- **`ConnectError::{AlreadyConnected, TimedOut, Local}`** —
  `AlreadyConnected`: §16.1 — returned by `connect()` itself, and also
  the resolution of an in-flight `Connecting` cancelled by a racing
  `accept()` on the same proven static when §6.7's comparison makes us
  the tie-break **loser** (§6.4's PENDING branch; as tie-break winner the
  `Connecting` is untouched and the `accept()` reports `Stale` instead);
  `TimedOut`: initial-connect give-up at
  `HANDSHAKE_GIVEUP` (§5.5); **`Local`** **[RATIFIED 2026/08/15 — ruling
  72]**: *our own* `Identity::open()` failed — a locked or
  biometrics-gated enclave, a hardware fault, a provider that is
  momentarily unavailable. See the note below.
- **`IntroError::{Expired, Evicted, Internal, Malformed, Local, EndpointDropped}`**
  **[AMENDED 2026/08/18 — ruling 261]** —
  `Expired`: the parked entry outlived `INTRO_TTL` (§6.3); `Evicted`:
  the parked entry was displaced by §6.3's cap pressure — per-source or
  global overflow — before any verb consumed it. The eviction record is
  bounded at the queue's own cap, so a verb arriving after that many
  further evictions reports `Expired`; the §18.2 `intro_evicted` event
  is the unconditional signal and has no such window; `Internal`:
  the §6.5 interception — the initiation belonged to a pending outbound
  remote (a simultaneous open) and
  was consumed by the endpoint; the application learns no identity;
  `Malformed`: the msg1 read fails structurally — **the peer's bytes are
  at fault, and the chain is discarded** (1 DH is spent and the verdict is
  definitive); **`Local`** **[RATIFIED 2026/08/15 — ruling 72]**: *our own*
  provider failed, **the chain is left parked**, and a retry can still
  succeed; `EndpointDropped`: the driver stopped mid round-trip.

  **[RATIFIED 2026/08/15 — ruling 72]** `Local` exists because the two
  failures above are opposite in every way that matters and were
  previously indistinguishable. A structurally unreadable msg1 is the
  peer's fault and is final; a locked enclave is *ours* and is transient.
  Reporting the second as `Malformed` tells an application the remote peer
  sent garbage, and an application may reasonably act on that — stop
  retrying, denylist, alert an operator — over a condition S21 treats as
  **expected**, not exceptional. It is §18.2's recurring shape: the party
  who can fix the problem is handed evidence pointing at someone else.
  §18.1's closure exists to stop variants accreting after release; nothing
  has shipped, so this costs nothing now and would be a breaking change
  later.
- **`AuthError::{Replay, HandshakeFailed, Expired, Local, EndpointDropped}`** —
  `Replay`: the automatic guard failure (§17.1); `HandshakeFailed`: the
  tail-tag death of a forged claim — **the only variant in the staged
  taxonomy that is a security signal**; the rest are liveness and
  lifecycle; **`Local`** **[RATIFIED 2026/08/15 — ruling 78]**: as
  §18.1's `IntroError::Local`, since `authenticate()` may drive a skipped
  `read_identity()` (ruling 75) and can therefore meet the same local
  fault; `Expired`/`EndpointDropped` as above, at this stage.

  **[RATIFIED 2026/08/15 — ruling 78]** Ruling 72 did not reach this
  type, and the omission was the worse half of the defect it fixed:
  without `Local`, a locked or biometrics-gated enclave surfaced as
  `HandshakeFailed` — the one variant this taxonomy designates a
  **security signal**, deliberately detail-free because it means a forged
  claim died at the msg1 tail's AEAD tag. Reporting a device lock through
  it does not merely misattribute a local fault to the peer; it reports
  the peer as an attacker, and teaches an operator to distrust the single
  variant that must stay trustworthy.
- **`AcceptError::{Stale, EndpointDropped}`** —
  `Stale`: no initiation for the proven static is parked, **or** the
  admitted candidate fails the replacement-basis rule — the live
  connection's basis is `None`, or the candidate is not strictly newer
  than it (§6.4, §17.4; the
  application SHOULD re-accept on the peer's next `Intro`) — **or** the
  proven static is PENDING and §6.7's comparison makes us the tie-break
  **winner**, in which case the pending is left in place, the candidate's
  timestamp is recorded in the guard, and our own outbound completes the
  connection (§6.4's PENDING branch; nothing is left to re-accept).
  There is no `Expired` here: a chain's age never fails an `accept()`.
  There is no `AlreadyConnected`: a basis-passing proven-LIVE `accept()`
  is a replacement, and a PENDING static's `accept()` either cancels the
  pending (tie-break loser) or reports `Stale` (tie-break winner) rather
  than reporting a distinct already-connected condition (§6.4).
- **`ConnectionLost::{TimedOut, NonceExhausted,
  LocallyClosed, PeerClosed { code, reason },
  ProtocolViolation { code }, Replaced,
  EndpointDropped}`** — the teardown matrix (§15.4) maps each to its
  cause; `PeerClosed` carries the peer's CLOSE payload;
  `ProtocolViolation { code }` is the violation-triggered teardown
  (§15.2) — with `AuthError::HandshakeFailed`, the second security
  signal in the taxonomy, kept distinct from `LocallyClosed` because
  peer misbehaviour and application intent demand opposite operational
  responses; `Replaced` is the replacement teardown, fired at the
  replacing `accept()` (§5.4, §6.4);
  `EndpointDropped` is the surviving-handle observation of a stopped
  driver.
- **`WriteError::{Reset(u64), ConnectionLost(ConnectionLost),
  Finished}`** — `Reset`: the stream was reset — by the local
  application, or by the peer's message-mode overflow reset (§9.8, the
  one receiver-emitted RESET_STREAM);
  `Finished`: write-after-finish. (No `Stopped`: STOP_SENDING is
  deferred, §9.9; the variant is reserved for that round.)
- **`ReadError::{Reset(u64), ConnectionLost(ConnectionLost)}`** —
  `Reset(code)`: the peer's RESET_STREAM (§9.6).
- **`MessageError::{TooLarge, ConnectionLost(ConnectionLost)}`** —
  `TooLarge`: payload > `MESSAGE_RECV_MAX` at the handle (§9.8).
- **`DatagramError::{TooLarge, ConnectionLost(ConnectionLost)}`** —
  `TooLarge`: payload > `MAX_DATAGRAM_PAYLOAD` at the handle (§11.4).

The wire error codes (`0x00`–`0x06`, ≥ `0x10` application; `0x07`–`0x0f`
reserved) are §15.3's registry. *(`0x06` is ruling 52's
`MESSAGE_OVERFLOW`; this sentence and the Named-constants table both read
`0x00`–`0x05` until 2026/08/14, when ruling 52's application was found to
have updated §15.3 and §9.8 but neither of the two places that restate
them.)*

**[RATIFIED 2026/08/14 — ruling 61]** *`#[non_exhaustive]` goes only
where a variant is actually reserved.* The protocol-error taxonomy is
closed by process, and the type system should say the same thing wherever
that is true. So **`WriteError` alone among §18.1's protocol errors**
carries `#[non_exhaustive]` — §19 explicitly reserves `Stopped` for the
STOP_SENDING round, so that type demonstrably will gain a variant — and
every other error type in this section is exhaustive. A consumer
therefore matches without a `_` arm and gets a **compile error** the day
a variant is added, which for a transport is the loud failure worth
having: a wildcard arm would silently swallow a new error into a branch
written for the old ones. The cost is that adding a variant to any other
§18.1 type is a major version bump, which is the correct price and a
useful brake. (§16.2's `Notification` remains `#[non_exhaustive]` on its
own reasoning — it is a signal set a later wire line may extend, not an
error taxonomy. `ConfigError` and the non-exhaustive
`TimingProfileError` both sit outside §18.1 entirely, by rulings 44 and
282 respectively.)

### 18.2 Trace targets — the operator contract

| Target | Carries |
|---|---|
| `slither::policy` | guard rejections, internal tie-break outcomes (admissions, tag deaths, winner-side drops), the **intro-queue evictions** (per-source and global overflow, with the evicted id — ruling 261), and the **contested-connection probe's three events** — the mark (with its probe floor), the probe's transmission, and the verdict (cleared, or `TimedOut`) — §7.5 |
| `slither::replay` | replay-window rejections |
| `slither::frames` | the frame layer's violation CLOSEs (post-AEAD structural failures and semantic violations, §8.2), the **datagram queue-overflow drop counters** (§11.5), and the **message-mode overflow reset** we emit — the stream, its final size, and the mode conflict that caused it (§9.8, ruling 59) |
| `slither::roam` | endpoint moves (§7.3), and — **[AMENDED 2026/08/16 — ruling 208]** — the address-validation exchange at the same seam: the challenge drawn and sent at each arming, the matching `PATH_RESPONSE` that validates and disarms the budget, and a **mismatched** `PATH_RESPONSE`, which §8.4 makes a silent no-op and which is therefore visible *only* here. The last is the operative one for an operator: it is what an off-path guess, a superseded arming, or a duplicate after validation all look like, and a rate of it is the signal that a mechanism with no error surface is nevertheless being exercised |
| `slither::io` | `Wire::send_to` failures, against the connection whose datagram it was, with the destination address and the underlying `io::Error` — a trace obligation and nothing more: the protocol never acts on a send failure (§16.3, §7.4). **[RATIFIED 2026/08/15 — ruling 79]** Also **`Identity::open()` failures**, with the provider's own error and the verb that met it, behind the `Local` variants of §18.1 (rulings 72, 78) |

The targets are operator-visible contract: renaming or dropping one is a
protocol revision. **[RATIFIED 2026/08/19 — ruling 275]** The failure
events these rows carry — `io`'s `send_to` and `open()` failures,
`frames`' violation CLOSEs and the message-overflow reset — emit at
**WARN**: below INFO they would be filtered out of an ordinary production
subscriber, defeating the post-mortem the rows exist for. Counters and
non-failure events may sit lower.

**[RATIFIED 2026/08/14 — ruling 49]** `slither::io` exists to make one
specific post-mortem answerable. A connection that dies at its effective
dead timeout because the host's routing table was broken for that whole
window, and one that dies because the peer went away, are
indistinguishable in every other target — same variant, same matrix row,
same silence. The obligation costs nothing on a healthy path (`send_to`
does not fail) and is the difference between "the network was down, here
is the errno and the address" and a bare timeout. It adds **no** error
variant (§18.1 is closed) and changes no behaviour: an implementation
that traces and continues is conformant; one that tears down on a send
error is not.

**[RATIFIED 2026/08/14 — ruling 59]** *The message-mode overflow reset is
traced by the receiver, which is the only end that learns nothing else.*
§9.8's reset is asymmetric in a way rulings 51 and 52 left unaddressed:
the **sender** is told, precisely and by design — that is the whole of
ruling 52's `MESSAGE_OVERFLOW` — while the **receiver**, which is the end
whose verb choice actually caused the conflict, emits a RESET_STREAM and
continues with no error, no notification, and nothing in its API surface
to say what it just did. So the party that can fix the bug is the one
with no evidence of it. An implementation **MUST** therefore trace the
reset under `slither::frames`, naming the stream, its final size, and the
mode conflict. The reasoning is ruling 49's exactly: slither does not act
on the condition, and its obligation is the one thing the application
cannot do for itself — make the failure explicable afterwards. The
concrete post-mortem is an operator asking why transfers to this peer die
at exactly 256 KiB, who reads the **receiver's** log to find out. **No
API change, no notification, no new error variant** — §18.1 stays closed
— **and no wire change**: the RESET_STREAM was already being sent.

## 19. Out of scope and deferred

**[RATIFIED 2026/08/14]** The deferrals below are ratified as a block; each names
its future home.

| Deferred | Pointer |
|---|---|
| STOP_SENDING | frame `0x05` (§8.3, §9.9); `WriteError::Stopped` reserved with it |
| the BLOCKED family (`DATA_BLOCKED`, `STREAM_DATA_BLOCKED`, `STREAMS_BLOCKED`) | pure diagnostics; QUIC types `0x14`–`0x17` if adopted (§10) |
| pacing | needs sub-RTT shell wakeups; revisit with a finer event loop (§14.7) |
| ECN | wire (ACK ECN counts) + socket plumbing (§14.7) |
| CUBIC / BBR | pure additions behind the `Controller` trait (§14.1) |
| header protection | second per-packet crypto pass outside hiss; metadata hardening axis (§3.4) |
| packet-number truncation | the per-packet-overhead lever; safe against hiss's commit-and-cap, couples to the replay-window width (§3.4, §7.2) |
| PMTUD | `MAX_DATAGRAM` is fixed at 1200 (§3.5) |
| cookies / mac2 | packet type `0x05`; WireGuard's under-load model is the template; answers §6.3's occupancy exposure, §6.5's hint-set spoof, and §6.9's ungated eager-`es` rate (§4.3) |
| ~~PATH_CHALLENGE / PATH_RESPONSE~~ → **validation *before* roam commit** (QUIC's full path-migration model) | **[AMENDED 2026/08/16 — ruling 208. The frames are no longer deferred; half of this row shipped and the other half did not, so the row is narrowed rather than deleted.]** The **frames** now exist and are normative: `0x1a`/`0x1b`, §8.3/§8.4, and §7.3 validates a roamed address with them. What stays deferred is QUIC's *ordering*: probing a candidate path while continuing to send on the old one and **committing only on success**. §7.3 keeps its arming triggers — it commits the roam on the authenticated packet and validates **after**, with the budget binding in the interval — so the deferred work is a second path's worth of state (a second congestion controller and RTT estimate, §14.6) plus a second reset seam alongside the roam seam (§7.3, §13.6, §14.6), not two frame types |
| the range-tracker ACK | decouples ACK fidelity from the replay window; wire-compatible (§7.2, §12.2) |
| per-peer `ss` precomputation | needs a hiss seam or a bounded memoising provider; re-opens the DH-cost table (§6.1) |
| persistence | nothing in this specification survives a process restart by design (§5.4: restart is a replacement or a fresh accept, never a merge) |
| reflector / mDNS / probe ping-pong | discovery and hole-punching; packet types would come from the reserved space (§3.1) |
| bubble integration | slither stays independent: zero `bubble-*` dependencies |

## Appendix A — hiss dependencies *(RECONCILED 2026/08/14; non-normative)*

**Every gate in this appendix is satisfied by released hiss 0.3.2**
(with `hiss-macros` 0.3.1), on crates.io, all hiss gates green. This
appendix was written against 0.3.1 as a list of *requests*; it is
reconciled here against the **shipped** API, verified by reading the
hiss tree rather than taking the delivery report. A.1 was the one hard
gate; A.2's interim is superseded; A.3 resolved to documentation and
was delivered as executable pins.

**[AMENDED 2026/08/23 — ruling 280.]** The dependency floor is
**`hiss = "0.4.1"`**, and this sentence previously read *"the dependency
line stays `hiss = "0.3"`; the floor rises to `0.3.2` when slither's
code first calls the staged read."* It rose twice since: to 0.4.0 for
`AesGcm` (ruling 279) and to 0.4.1 for the trailing-`psk` staged read
this appendix's *Qualifying scope* paragraph now describes. `Cargo.toml`
is the authority for the pin; this appendix is non-normative and records
only what the gates were reconciled against.

**A.1 The split msg1 read — SHIPPED.** The API as built:

```rust
let (claimed_static, mid) = state.read_message_1_intro(&msg1)?;  // 1 DH: es
let (payload, responder) = mid.complete()?;                      // +1 DH: ss
```

The mid-state type is named `{Pattern}{Role}Msg{n}Intro` — for slither's
reference case, **`IKResponderMsg1Intro<CP>`**
(`hiss-macros/src/codegen.rs:1382`). It is owned, carries no lifetime,
re-supplies nothing, and is `#[must_use]`; it is deliberately **not**
`Clone`, so a parked chain cannot be forked. `read_message_1_intro`
exposes the **claimed static** at the `es` boundary — serving both
`read_identity()` and §6.5's eager inspection — and `complete()` performs
`ss` and returns the decrypted 12-byte msg1 payload alongside the
responder state, which is where §5.2's timestamp comes from. This is what
lets the `Claimed` stage **suspend** rather than merely decide: it is an
app-held object (human-in-the-loop rejection is a motivating use case),
parked across event-loop turns.

*Unretryability is compile-time, not a latch:* `complete(self)` consumes
the mid-state, so a failed `complete()` cannot be retried — the type
system forbids it rather than a runtime flag refusing it.

*Scrub is structural, not a runtime check:* the mid-state holds a
`SymmetricState` and provider keys, each of which scrubs in its own
`Drop`; there is no separate scrubbing step to test at runtime, and
`SymmetricState::drop` now zeroes `h` as well (see *Wart resolved*
below).

*The un-read tail* is a `[u8; MSG1_INTRO_TAIL]` owned array —
**28 bytes** for the reference case (the 12-byte payload plus its 16-byte
tag). The const is generated per pattern and **derived by subtraction
from the message-size const** rather than re-derived independently
(`codegen.rs:475-492`), so the tail and the size cannot drift apart.

*Qualifying scope* is narrower than "IK": the staged pair is generated
for **the first message whose token sequence ends `…, s, ss`,
optionally followed by a `psk`** (a declared payload may follow) —
`split_read_on`, `codegen.rs:1068-1070`. That includes IKpsk0 **and
IKpsk1**. One-way patterns (X, Xpsk0) also qualify, and there
`complete()` yields the transport directly — no slither impact.
**[AMENDED 2026/08/23 — ruling 280.]** This paragraph previously read
*"excludes IKpsk1, whose trailing `psk` would require the PSK re-supplied
mid-read and so break the mid-state's 'nothing re-supplied later'
contract; the `_with` lookup closure already serves per-peer PSK
selection there."* hiss 0.4.1 reverses **both** halves, and the rationale
is amended with the token. On the first: the contract was stated too
broadly. What the mechanism actually depends on is that **no bytes of
`message` are re-supplied** — that is what keeps the mid-state
self-contained and the input buffer unborrowed — and a `psk` is not
message bytes, so `complete(&psk)` takes it without touching the
property. On the second: `read_message_1_with`'s lookup closure is
emitted **at** the `psk` token, which is *after* the proving `ss`, so
rejecting an unenrolled stranger through it costs **2 DH**. Staged, the
claimed identity arrives after **1 DH** and the PSK is chosen only if
that identity survives. It did not "already serve" the requirement; it
served it at twice the price. That is what §2.2's second pattern climbs.

*The DH ladder is pinned by test*, not merely documented: a counting
provider asserts `dhs == 1` after `intro` and `dhs == 2` cumulatively
after `complete()`, and a third test pins the drop-to-reject path at
exactly 1 DH. That is §6.1's ladder — one DH to inspect, two to
authenticate — so slither's `reject()`, `INTRO_TTL` expiry and queue
eviction each cost one DH and never `ss`. Equivalence is pinned too:
byte-identical msg2 and session id against the one-shot read, and the
Cacophony IK vectors replay byte-identical through the split path.

*Mid-state size, measured* (reference suite, `EphemeralOnly<StdRng>`):
**784 B on P-256** (616/648 B on X25519), of which ~320 B is the provider
itself. The delta over the pre-read state slither already holds is only
**32 B** — not state-plus-message. At `INTRO_QUEUE_CAP` = 1024 parked
mid-states that is ≈ **0.77 MiB**, inside §6.3/§17.5's ≈ 0.5–1 KB
per-entry estimate, which therefore **stands unamended**.

*The rejected fallback, recorded so it is not re-proposed:* the
`read_message_1_with` Verify closure does hand the claimed static to a
closure between `es` and `ss`, but a `Claimed` built on it would decide
synchronously inside the read and re-pay `es` at `authenticate()` — a
3-DH accepted read that distorts the 1/2/4 DH-cost ladder (§6.1). There
is **no fallback path in this specification or in the code**.

**A.2 `DatagramSend::next_counter()` — SHIPPED**
(`src/noise/datagram.rs:244`). The accessor names the counter the next
seal will use, which §3.4 needs because the Data header is the AEAD
associated data and must be built **before** sealing. The normative
statement remains behavioural — the header carries exactly the counter
the seal used — but an implementation no longer mirrors hiss-owned state
to satisfy it. **The §3.4 mirror-and-assert interim is superseded**: it
was conformant while the accessor was absent and is retained in §3.4
only as the fallback shape, not as the operative mechanism.

**A.3 The canonical encoding — RESOLVED, and pinned by test.** No hiss
trait change was required and none was made: the canonical-encoding
ruling (§2.4) needs exactly one bound, `AsRef<[u8]>`, and slither
expresses it as its own `where C::PublicKey: AsRef<[u8]>` clause — **the
permanent mechanism, not an interim**. `Ord` is **not** required; the
§6.7 tie-break compares equal-length `as_ref()` octets directly as
unsigned octet strings. What was requested was a doc-only stability
promise; what shipped is stronger — every curve now carries a test
pinning its public-key encoding against authoritative vectors (RFC 7748
§6.1/§6.2 for X25519/X448, and for P-256 the length, the leading `0x04`
uncompressed tag, and that a compressed input normalises to the same 65
canonical bytes). mac1 keying, the tie-break and the golden vectors all
rest on that encoding, so an executable pin is worth more than a promise.

**Wart resolved.** `SymmetricState::drop` now zeroes `h` alongside `ck`,
as its comment had always promised. Pre-existing, affected every dropped
handshake; `h` is not secret, so this is defence in depth rather than a
fix to a leak.

**Non-item, closed — the msg2 payload.** The superseded draft's msg2
`[1]` payload would have needed no hiss change, since a `[N]` payload on
the final message was already supported. The ratchet-only ruling deleted
the msg2 payload entirely (§5.2), so the question is moot at both ends;
recorded in one line so the walkthrough's verification work is not
re-litigated. (A 12-byte timestamp in msg2 was separately **considered
and declined as wire-affecting** when fixing ruling 36 — that is a wire
decision, not a hiss dependency.)

## Appendix B — test obligations *(non-normative)*

The obligations this specification's normative clauses demand. All flow
tests run two endpoints over `testutil::FlakyWire` on tokio's paused
clock (§16.10); no test sleeps.

**Wire pins.**
- Golden vectors frozen **for the first time** at wire version 1 (the
  prologue, the 12-byte msg1 payload, mac1's canonical keying — §1.3),
  then held byte-identical.
- Compile-time size/constant asserts for every §3/§2.3 length and every
  frame-layout constant.
- DH-cost pins: 1 DH per rejected probe, 2 per authenticate, 4 total per
  accept (§6.1).

**Handshake and routing.**
- The staged-accept queue obligations (§6.3, carried): cap; addr-only
  dedup with replace-with-newest and one surfacing per source;
  evict-oldest overflow; the per-source cap counting consumed +
  unconsumed (`read_identity()` net-zero); TTL expiry;
  own-bytes-on-consume (a consumed chain is unsupersedable; a
  post-consumption initiation parks fresh); **freeze-on-carry** (a
  mid-state-carrying eager-demoted entry is
  consumed on arrival: a later mac1-valid same-source packet parks
  separately and can never straddle the frozen entry's accessors).
- Hint routing: eager tie-break entry; unknown-claim demotion with the
  carried mid-state (1 DH total for the class, `read_identity()` at 0
  incremental); interception → `IntroError::Internal`.
- **The local-state routing** (§5.4), over all three states: LIVE ⇒ the
  initiation parks as an ordinary `Intro`, the live connection untouched
  until the replacing `accept()` fires `Replaced` (a withheld or
  replayed initiation left unaccepted destroys nothing); PENDING ⇒ the
  tie-break comparison by **either** route — §6.6's internal path or
  §6.4's PENDING branch — reaching the same conclusion, no
  teardown either way (§6.6–§6.7); NONE ⇒ the ordinary staged accept.
  Restart end-to-end: a restarted peer's reconnect replaces via
  `accept()`, and its zombie counterpart dies at the replacement or at
  liveness — no state merge is representable (§5.4, §6.8).
- **Replay against a live connection — both cases, separately pinned**
  (§6.4, §17.1, §17.4): an **older-or-equal** replay dies at the
  timestamp guard and never surfaces to the application; a
  **withheld-newer** replay — a genuine retransmit captured off the wire
  and injected later — passes the guard, surfaces as an `Intro`,
  authenticates, and **destroys nothing until it is accepted**. The
  second case is the one the guard has never barred; the test must
  exercise it explicitly and not stand on the first.
- **The replacement basis** (§17.4): a connection we **dialled** carries
  basis `None`, and a proven-LIVE `accept()` against it returns
  `AcceptError::Stale` with the live connection untouched — the
  captured-msg1 injection test, run against a connection established by
  `connect()`; a connection we **accepted** carries `Some(t)`, a
  candidate with timestamp ≤ `t` returns `Stale`, and a strictly greater
  one replaces. The tie-break winner records the loser's timestamp in the
  guard (§6.7) and keeps basis `None`; the loser installs with `Some(t)`.
- **The PENDING branch at `accept()`** (§6.4, ruling 35): the branch runs
  §6.7's comparison, so **both orderings must be pinned separately** and
  a test that exercises only one is not sufficient.
  - *Peer's static smaller (we lose):* the pending and its index are
    dropped, its `Connecting` resolves
    `Err(ConnectError::AlreadyConnected)`, the accept installs as
    responder.
  - *Our static smaller (we win):* `accept()` returns
    `AcceptError::Stale`, the pending is **still in flight** afterwards
    and completes normally, and the candidate's timestamp **is** in the
    guard — the one `Stale` that leaves a record behind (§6.4's ordering
    clause).
  - *Convergence, the regression this ruling exists for:* two endpoints
    both driven `read_identity()` → `connect()` → `accept()` against each
    other, so **both** take the PENDING branch, must converge on **one**
    shared session with complementary roles and agreeing stream-ID parity
    (§9.1) — not two sessions with two key sets going mutually dark. Run
    it for both static orderings, and assert data flows in both
    directions rather than merely that a connection object exists.
  - §16.1's invariant on both outcomes: exactly one connection per
    static, and on the winner side **no second connection is installed**.
- Tie-break: both static orderings from both ends converging on one
  shared session; forgery-cannot-cancel (a forged claim of the dialled
  static dies at the tag with the pending untouched); **stream-ID parity
  fixed by the tie-break at establishment** (§6.7, §9.1).
- `accept()` re-home: fast path; newest-first re-home admission;
  `Stale`; the proven-LIVE replacement (`Replaced` fires exactly at the
  replacing `accept()`, never earlier); one completion attempt per
  interval (a forged msg2 spends it; the next scheduled retransmit
  refreshes it); `ConnectError::AlreadyConnected` at `connect()` (no
  `AcceptError` counterpart exists — §18.1).
- Guard pinning,
  no-orphan-on-reject, orphan timer aging, admission-only LRU refresh;
  index re-draw across both tables.
- **The post-mortem pin** (§17.1, ruling 37): an entry written by a
  tie-break admission or a winner-side record survives orphan aging
  **and** a full LRU flush (≈ 1024 distinct **admitted** statics — an
  admission-driven flood: admission is cap eviction's only driver, and
  an authenticate-then-drop flood mints no guard entries at all, being
  exactly what §17.1 mitigation (i)'s `GuardUndo` defeats **[AMENDED
  2026/08/18 — ruling 268]**) for
  `HANDSHAKE_GIVEUP` after its connection dies, then demotes to an
  ordinary orphan and ages normally. Paired with the replay it exists to
  stop: kill the connection, flush the tier, replay the captured
  initiation inside the 90 s and assert it dies at the guard; replay it
  after the 90 s and assert the documented re-admission — the bound is
  conditional and the test must pin **both** sides of the horizon
  (§6.7's qualified single-use claim).
- **No-record-on-`Stale`** (§6.4's ordering clause, SECV5-6): an
  `accept()` refused by the basis rule leaves the guard byte-identical to
  its pre-call contents, so a later genuine initiation with a timestamp
  between the two is still admitted; the tie-break-winner `Stale` is the
  single exception and **does** leave its record.
- **The dialled-only static** (§17.1's honesty clause, SECV5-5): against
  a peer we only ever `connect()`ed to, the guard holds **no** entry, so
  an arbitrarily old captured initiation passes it vacuously and
  surfaces as an `Intro` **repeatably** — assert it surfaces more than
  once from a single captured packet, and that the live connection is
  untouched every time because the basis is `None`.

**Frame layer.**
- Varint round-trips, boundary values, non-minimal-encoding acceptance
  (§8.1).
- Frame-table round-trips for every frame; unknown-type and every
  post-AEAD structural-failure case ⇒ nothing from the packet applied,
  CLOSE with `PROTOCOL_VIOLATION`, and
  `ConnectionLost::ProtocolViolation { code }` surfaced (§8.2); pre-AEAD
  gate failures stay silent (§3.1).
- Packing order and the one-extends-to-end-frame rule (§8.5).

**Streams, flow control, messages, datagrams.**
- Stream reassembly under reordering, overlap, and duplication; FIN
  final-size pinning; every `FINAL_SIZE_ERROR` case (§9.5, §9.6).
- **The closed-stream tombstone** (§9.2): free a stream (read-to-final
  and sugar-surfaced), drop the ACK, and let the peer's PTO
  retransmission re-name it — no re-open, no phantom `StreamOpened`, no
  second surfacing of the same message; the watermark holds for the
  connection's life (§9.2).
- Flow-control stall-and-resume at both levels; the §10.3 re-grant
  formula (MAX_STREAM_DATA/MAX_DATA emitted exactly when the read offset
  advances ≥ WINDOW/2 past the last advertisement); violation ⇒ CLOSE
  with `FLOW_CONTROL_ERROR` (§10).
- **Discard-credit** (§10.3): abandoned handles, observed resets, and
  sugar-surfaced streams true up connection credit — a stream-cancelling
  application never wedges MAX_DATA; the §8.4 bound check rejects a
  `final_size` beyond the advertised limit *before* any true-up (no
  credit inflation, no `u64` wrap).
- **The reassembly-fragment bound** (§10.6): a one-byte-frames-at-
  even-offsets flood stays O(credit) or dies at §10.6's credit-derived
  ceiling (ruling 270; `REASSEMBLY_CHUNKS_MAX` is its floor, and the flood
  sits 512× above the derived term at any window)
  with `PROTOCOL_VIOLATION`; the defragmentation cost is measured by the
  throughput gate below.
- **The reassembly work bound** (§10.6, ruling 253): an alternating
  bridging workload's total copy work stays O(credit · log credit),
  asserted from the separating side — the pre-253 whole-span merge fails
  it while passing the throughput gate, which is why the gate alone is
  not the evidence.
- MAX_STREAMS replenishment (batching at 8, low-allowance emission,
  peer-opened streams only) and
  `STREAM_LIMIT_ERROR` (§10.4).
- Message sugar: exactly-once surfacing, the 256 KiB bound at the handle,
  GC after full ACK (§9.8); the overflow reset (a FIN-less
  window-filling uni stream under a pending `recv_message()` is reset,
  the sender surfaces `WriteError::Reset`, connection credit trues up);
  the lost-reset case (the receiver-emitted RESET_STREAM is regenerated
  until acknowledged — dropping its first transmission still frees the
  sender, §9.6, §8.7).
- **The unclaimed uni stream fails loudly** (§9.8, ruling 51): the
  mixed-mode shape end to end. A opens a stream with `open_uni()` and
  writes past `MESSAGE_RECV_MAX` (262 144 B) while B **only ever** calls
  `recv_message()`; assert on the paused clock that A's write resolves
  `Err(WriteError::Reset(MESSAGE_OVERFLOW))` — the **distinguishable**
  code of ruling 52, not `0` and not a permanent stall — that B's
  connection credit trues up, and that keepalives kept both sides alive
  throughout, so nothing here is a liveness death in disguise. Assert
  the not-oldest case too: a second such stream behind a slower unclaimed
  one is reset on its own account. And assert the **negative**, which is
  what the guard buys: a receiver that uses `accept_uni()` only, and is
  slow to call it, is **not** reset — its window-full stream waits under
  §16.4's backpressure-by-retention and resumes when the accept lands.
- DATAGRAM: no retransmission on loss; queue-overflow drop-oldest with
  newest accepted; the drop counter emitted on `slither::frames` (§11).

**ACK, recovery, congestion.**
- Delayed-ACK policy timing: every-2nd due, per-drain coalescing bounded
  by `ACK_COALESCE_MAX`, the 25 ms timer, immediate on gap (§12.4).
- Window-2048 admit/duplicate/edge cases; ACK truncation newest-first at
  the cap and at packet capacity; over-cap received ACK ⇒ structural
  failure (CLOSE with `PROTOCOL_VIOLATION`, §8.2);
  above-highest-sealed ⇒ frame ignored whole (§7.2, §12).
- Loss/PTO staircase on the paused clock (packet threshold, time
  threshold, PTO doubling and cap, reset-on-ack) (§13).
- **PTO-disarm** (§13.3, amended by ruling 249): an idle connection with
  an empty sent map arms no `Pto` — no self-sustaining PING train — and a
  **non-empty** map under a closed §7.3 budget announces no `Pto` either:
  the announced `Timeout` falls to the next representable armed timer —
  ordinarily `Liveness`, or ruling 265's vetoed-keepalive backstop, at the
  latest **[AMENDED 2026/08/18 — ruling 265]** (the livelock separation —
  the pre-249 build announces a past deadline and spins the shared driver);
  `None` is permitted only at the platform clock horizon. The timer is
  re-announced at the receive that refunds the budget and re-arms with the
  next ack-eliciting send **the budget admits**.
- **Keepalive-disarm and the parked-state backstop** (§7.5, ruling 265):
  at a budget with no room for the 30-byte empty plaintext, and —
  separately — at a **pending** contested mark with room for the
  keepalive but not the probe, **neither** keepalive deadline is
  announced (the two holds must be separated: a build consulting only
  one passes the other's test), the announcement returns at the receive
  that lifts the hold, and the admissible path still fires and re-arms —
  a build that suppresses unconditionally satisfies every
  non-retrospection assertion and has deleted §7.5. And the backstop,
  both sides: a starved passive keepalive ends in **death or a send,
  never silence** (the premise read at the ACK instant, the verdict read
  past `D_eff`), while a peer that returns and re-funds the
  budget is **not** reaped — the mirror a build with an unconditional
  `Liveness` fails. Discharged: `tests_livelock.rs` (the gate),
  `tests_park.rs` and `story_park.rs` (the backstop).
- **The backoff ladder and the survival envelope** (§13.3, ruling 254):
  probe intervals under a black-holed path are **not all equal** and are
  **capped at 8 ×** the base PTO with the capped rung reached — both
  sides, so an always-at-base build and an uncapped build each fail; and,
  under the v1/default profile, at 50 % random loss a bounded transfer
  completes inside `DEAD_TIMEOUT` in ≥ 30 % of runs (the audit's E5a/E5b,
  discharged by the story suite).
- NewReno: slow start, congestion avoidance (ABC), recovery-period
  one-cut, **no cwnd growth from ACKs of pre-recovery-period packets**
  (§14.3), persistent congestion (3× the un-backed-off PTO —
  `pto_count = 0` — with the RTT-sample precondition)
  (§14.2–14.4).
- The cwnd admission gate incl. the PTO-probe exemption and
  non-ack-eliciting exemption (§14.5).
- **The epoch ratchet** (§7.7, ruling 251): seals cross the epoch
  boundary invisibly — a stream is byte-exact across it, with no
  handshake, no round trip and no application-visible event (S23); each
  direction ratchets independently; the counter is never reset. Pinned at
  **both triggers**: the config-supplied epoch (ruling 82's knob)
  crossing several boundaries on the paused clock, and the production
  constant (`REKEY_EPOCH_MSGS` = 65 536) crossing once — in the release
  run if debug-slow.
- **Straggler tolerance** (§7.7): a packet from the immediately preceding
  epoch opens after the receiver commits to the new one; a packet from
  **two** epochs back is refused without key derivation. Both pins are
  about **opening**, and must be built at an epoch size below
  `REPLAY_WINDOW`: §7.2's window is what refuses a preceding-epoch
  packet further back than that, and at `REKEY_EPOCH_MSGS` it refuses
  every one of them **[AMENDED 2026/08/18 — ruling 256]**. The refusal
  is the separating assertion — a build that never rekeys opens the e−2
  straggler happily, where boundary-invisibility alone is satisfied for
  free by the build in which nothing ever happens.
- The `REKEY(0³²)` vector (§7.7), pinned test-only in slither via
  `cryptoxide` (ruling 251).
- **One session per connection** (§7.8): a completed replacement
  handshake installs a fresh connection carrying nothing — fresh
  streams, credit, recovery, counters — and the old connection dies
  whole with `Replaced` at the replacing `accept()`.
- Roaming: CC reset with the pre-roam flight fenced (old-path losses
  fire no congestion event and no persistent-congestion collapse; the
  flight still resolves for retransmission), sent map kept, RTT kept as
  a prior with `min_rtt` re-seeded (§13.1, §13.6, §14.6).

**Liveness and amplification.**
- **The endpoint timing profile** (§5.7, §7.5, §16.2, rulings 282 and
  283): pin
  `Config::default()` to the exact public v1 constants, 10 s and 25 s.
  Pin every construction boundary independently: a passive interval
  below 1 s is rejected and 1 s is admitted; a dead timeout equal to
  `2 × K_eff + K_INITIAL_RTT + 2 × SHELL_LATENESS_BOUND` is rejected and
  the smallest representable value above it is admitted; overflowing checked
  arithmetic is rejected. Both connection birth paths — outbound
  `connect()` and inbound `accept()` — must receive the configured pair.
  With a short valid profile on the paused clock, pin passive keepalive
  and the contested verdict to `K_eff`, and receive-anchored death plus
  the vetoed-keepalive backstop to `D_eff`. Pin the persistent-beacon
  ceiling to that same `D_eff`: one representable instant below is
  accepted, equality is rejected, and rejection leaves the prior setting
  unchanged. Finally, connect endpoints with asymmetric valid profiles
  and assert wire interoperability while each endpoint keeps its own
  local deadlines. No test may make handshake, admission, ACK/PTO, close,
  or shell-lateness timing follow the profile.
- **The monotonic clock horizon** (§16.4–§16.5, ruling 284): drive every
  endpoint and connection timer family from an anchor close enough to the
  platform horizon that its final addition, or one of recovery's
  `Duration` intermediates, is unrepresentable. Assert no panic, wrap,
  clamp, saturation, fabricated earlier deadline, premature firing, or
  loss of the owning logical state. Other representable deadlines must
  remain ordered and announced normally, and a later state change must
  recompute from its new anchor. Repeat ordinary-anchor cases to pin exact
  default timing and wire behaviour unchanged. The families are handshake
  retransmit/give-up, intro expiry, guard orphan aging/exemption, passive
  and persistent keepalive, liveness and its veto backstop, delayed ACK,
  contested verdict, close linger, Loss, and PTO.
- **The liveness anchor** (§7.4): a sender writing into a black hole
  dies at `D_eff` after its last authenticated receive — the
  deadline arms on the first **marking or ack-eliciting** send after a
  receive and no send ever re-arms it, so neither a write loop nor a PTO
  train defers death (§13.3), and a connection whose entire output is
  quiet-set-but-ack-eliciting (credit frames, retransmissions, probes)
  dies on schedule rather than sitting undetected; a healthy receiving
  session never dies; the `PERSISTENT_KEEPALIVE` range rejection at the
  handle — the admissible band is `[1 s, D_eff)`, so under the
  v1/default profile
  `set_persistent_keepalive` accepts 1 s and 10 s and rejects both 500 ms
  and 25 s (§7.5, rulings 40 and 42), **each rejection returning
  `Err(ConfigError::…)` rather than panicking** — assert the `Err`, and
  assert the beacon's interval is *unchanged* after a rejected call, so a
  refused setter cannot silently clamp (ruling 44); an idle
  keepalive-sustained connection lives
  indefinitely **while the dance survives the path** — at
  2 × `KEEPALIVE_TIMEOUT` + 5 s one keepalive lost **in one direction** is
  tolerated, while a *simultaneous bidirectional* loss over a single
  interval ends the connection at 25 s (SECV5-8 — drop both directions'
  keepalive in the same interval and assert both sides fire `TimedOut`,
  since neither can re-fire the one-shot passive rule and keepalives are
  never retransmitted), with no handshake ever re-run and no built-in
  reconnect (§5.4, §7.5).
- **The clock is armed at install** (§7.4, SECV5-2): under the
  v1/default profile, install a session
  and drive **nothing** — no application send, no receive — and assert it
  dies with `ConnectionLost::TimedOut` at exactly install +
  `DEAD_TIMEOUT` on the paused clock, and that it transmits **nothing** in
  the interim (with `last_send` pinned to install time the passive rule
  never fires, so no keepalive escapes). The responder-side half-open
  shape is the one that matters: accept a replayed initiation whose
  initiator never speaks again, and assert reclamation rather than an
  immortal session — this is the obligation under §6.7's and §17.1's
  "reaped by liveness in 25 s".
- **The dance is automatic once traffic has flowed** (§7.5, ruling 39):
  under the v1/default profile,
  install a pair, exchange **one** application message in one direction
  only, then drive nothing further, and assert on the paused clock that
  both sides are still alive well past install + `DEAD_TIMEOUT` — the
  keepalive ping-pong sustains them with no `set_persistent_keepalive`
  call anywhere in the test. The companion is the negative already above:
  the same pair with **no** exchange at all dies at install + 25 s.
- **Connect-ahead-of-use is reaped, and the two escapes work** (§7.5,
  §5.7, ruling 39): under the v1/default profile, the reap is a
  *receive*-within-`DEAD_TIMEOUT` rule,
  not a *send* rule, and the tests must say so. (a) Install a pair, send
  the first application message at *t* = 24 s on the paused clock, and
  assert the sender still dies at *t* = 25 s unless the peer's answer
  lands inside that second — then drop that first message and assert it
  dies after roughly one PTO with data still queued. (b) The same shape
  with a persistent keepalive configured at install lives indefinitely.
  (c) The same shape with any exchange completed before *t* = 25 s lives
  indefinitely. This is the application-facing trap of ruling 39 and the
  obligation exists so an implementation cannot quietly turn the reap
  into a send-based rule that would pass the "drive nothing" test above
  while breaking the connect-early/use-later shape.
- **The beacon sustains a mutually idle link** (§7.5, ruling 40): under
  the v1/default profile, install
  a pair, call `set_persistent_keepalive(Some(10 s))` on **one** side
  only, drive no application traffic in either direction, and assert on
  the paused clock that both sides live indefinitely — the beacon fires
  unconditionally, the peer's passive rule answers it, and neither death
  deadline is ever reached. Assert also that dropping a single beacon
  kills neither side — the next answer lands at 20 s, 5 s inside the
  deadline — and that dropping two consecutive ones kills both at 25 s.
- **The keepalive interval is bounded above *and* below** (§7.5, §16.2,
  rulings 40, 42, and 282): under the v1/default profile, assert
  `set_persistent_keepalive` **accepts** every
  interval in `[1 s, DEAD_TIMEOUT)` — 10 s, the recommended default, and
  the floor value 1 s itself, which is admissible **inclusive** — and
  **rejects** everything outside it. Above: 25 s (`DEAD_TIMEOUT` exactly)
  and 30 s both fail at the handle. Below: 999 ms, 1 ms and
  `Duration::ZERO` all fail — ruling 42's floor, without which a beacon
  exempt from the congestion window (§14.5) is an unthrottled emitter.
  `None` (disable) is accepted at all times. The upper direction is the
  one ruling 38 had backwards; a test that pins the old floor at
  `DEAD_TIMEOUT` is the regression this obligation exists to catch, and
  a test asserting that 1 s is *rejected* is the mirror regression
  against over-reading ruling 42.
- **The contested-connection probe** (§7.5, §6.4, rulings 36, 41 and
  43): under the v1/default profile, with a connection we **dialled**
  (basis `None`) live, park an
  `Intro` for the same static and call `accept()`. Assert (a) it returns
  `AcceptError::Stale`, (b) a PING goes out on the live connection
  immediately, and (c) the two outcomes on the paused clock — an ACK
  covering the probe floor inside `KEEPALIVE_TIMEOUT` leaves the
  connection alive and the refusal standing, while silence fires
  `ConnectionLost::TimedOut` at the deadline, after which the parked
  `Intro` is accepted normally.
  - **The high-water-mark regression (ruling 41), the reason this
    obligation exists in its present form.** Run the same shape but
    **drop the probe PING itself**, and let the connection's ordinary
    PTO retry (§13.4) go out and be acknowledged inside the deadline.
    Assert the connection **lives**: the retry is sealed above the probe
    floor, so the ACK covering it clears the mark. A conformant
    implementation that matched the *packet* carrying the PING would
    kill a fully live peer here, because §8.7 never retransmits a PING
    and no ACK covering that counter can ever exist. Assert the same
    with an ordinary application Data packet in place of the PTO retry.
  - **The collapse (ruling 41).** While contested, deliver a **second**
    parked `Intro` and call `accept()` again. Assert it also returns
    `AcceptError::Stale`, that **no second PING** is emitted, and — the
    load-bearing assertion — that the connection still dies at the
    **original** deadline, not one `KEEPALIVE_TIMEOUT` later. Repeat with
    a refusal every second for the whole interval: the death instant must
    not move. A re-arming implementation lets an attacker's Intro supply
    postpone the verdict forever.
  - **The security case.** Feed the zombie **withheld genuine Data**
    (harvested with the window never advanced past it) every 5 s
    throughout, and assert it still dies at the probe deadline — a reset
    receive clock must not satisfy the probe, and no ACK the attacker
    holds can cover a counter at or above the floor, all of which were
    sealed after the harvest.
  - **Accounting and admission (ruling 43).** Assert the probe is sent
    with the congestion window full — the admission gate does not hold
    it — and that it nonetheless appears in the sent-packet map and in
    `bytes_in_flight` (§13.5, §14.5). Assert also that a probe held by
    §7.3's anti-amplification budget leaves the mark **pending** with no
    deadline armed, that the deadline arms at the eventual transmission,
    and that a contested mark taken on a closing or draining connection
    is a no-op (§6.4 carries that carve-out too — ruling 179).
  - **The pending mark's other two exits (ruling 176).** Hold the probe
    with the budget, then let an ACK covering the floor arrive — any
    post-mark seal's ACK will do, since the floor is the counter the next
    seal uses. Assert **no probe is ever sent**, **no deadline is ever
    armed**, and **no notification of any kind is emitted** — in
    particular no `ContestCleared`, which without this rule fires with no
    preceding `Contested`, and no stray probe arming a verdict deadline
    for a mark that no longer exists. Separately, **roam again while the
    mark is still pending** and assert the mark survives with its floor
    **unchanged**.
  - **Re-marking after a clear (ruling 175).** Clear a mark with an ACK,
    then deliver a fresh `Intro` and refuse again: assert a **second**
    PING goes out with a **fresh, strictly greater** floor and a newly
    armed deadline. An implementation that read the old
    "one probe per `KEEPALIVE_TIMEOUT`" bound as a rate limit fails this,
    and a genuine second doubt would go unprobed.
  - **The notification arrives at the probe's transmission** (§16.2,
    §16.4, rulings 45/46, FAB-6). Drive the mark-pending case above —
    roam to an unvalidated address so §7.3's budget holds the probe —
    and assert that `notified()` yields **nothing** while the mark is
    pending, yields `Notification::Contested` at the instant the probe
    goes out (the same instant the deadline arms, pinned together), and
    yields `ContestCleared` when an ACK covering the probe floor lands.
    Assert **at most one of each per mark** across a refusal storm (the
    ruling 41 collapse shape above emits no second `Contested`), and
    that the never-answered path emits no third notification — the death
    arrives on `closed()` as `ConnectionLost::TimedOut` instead.
- **The anti-amplification budget** (§7.3): a roam or msg1-source anchor
  caps all output — PTO probes, the contested-connection probe, pure
  ACKs, CLOSE, and keepalives
  included — at 3× **authenticated, window-fresh** bytes received until
  the address validates; a silent address dies by
  liveness having received at most 3× what it sent.
  - **The disarm, and the re-arm** (rulings 168, 170, 208). Assert the
    positive: a genuine roam validates the address and **disarms** the
    budget within ~1 RTT, at the first authenticated, window-fresh packet
    from the new address carrying a `PATH_RESPONSE` matching the
    challenge — after
    which output to it is no longer capped at all. Assert the negative
    from the side that separates it (working rule 9): with the response
    **withheld** — the peer answering with ordinary traffic and ACKs but
    never echoing the challenge — the cap must still bind. **That negative
    is the whole of ruling 208 and is the one assertion the superseded
    design fails**: an implementation still validating on an ACK covering
    a floor passes every other item in this list and fails only this one.
    Assert it in its adversarial form too: a peer that **forges an ACK**
    naming any counter it likes, spoofed from the new address, must not
    validate it — that is the two-packet O(1) reflector ruling 208 exists
    to close, and it is a wire-level assertion, not a unit-level one.
    Assert the challenge is **unguessable and per-arming**: two armings on
    one connection must not draw the same eight bytes, and a
    `PATH_RESPONSE` replaying the **previous** arming's bytes must
    validate nothing (working rule 9 again — an implementation that
    re-used one challenge for the connection's life passes a
    "validation happens" test and fails only this one). And assert
    the re-arm: roam again, a **fresh** challenge is drawn and the cap
    binds again, with no credit carried across from the old address
    (§13.6).
  - **A mismatched `PATH_RESPONSE` is a no-op, not a death** (ruling 208,
    §8.4). Deliver eight bytes matching nothing and assert the connection
    **survives**, validates nothing, and traces. The inverted
    implementation — `PROTOCOL_VIOLATION` on mismatch — is a remote kill
    primitive available to any off-path party, and it is the natural thing
    to write if the frame is treated like every other structural error in
    §8.2.
  - **The honesty clause is not testable and is stated so it is not
    mistaken for one** (ruling 210(c)). An on-path relay that carries the
    challenge to the real peer and the response back **does** validate the
    address, correctly. No test asserts otherwise; a test that did would be
    asserting a property the mechanism does not have. The load-bearing regression is the asymmetric transfer: an
    **accepting** endpoint serving a download, whose peer replies with
    ACKs only, must reach full send rate — under the superseded permanent
    cap it could never exceed 3× ~40 B per ~2400 B and could not serve at
    all.
  - **Replay does not fund it** (ruling 169). Re-inject a peer packet the
    replay window has already marked and assert the budget's received
    counter does **not** move; an implementation funding on the broader
    *authenticated* class passes every other assertion here and fails
    only this one.
  - **Per session** (ruling 170). Two connections anchored to the same
    peer address hold two independent budgets: exhausting one must not
    throttle the other, and funding one must not credit the other.
  - **Priority under scarcity** (ruling 171). With the budget admitting
    less than is owed, assert a **pending contested probe** goes out
    ahead of an ACK, a keepalive, a PTO probe, a retransmission and new
    application data — the starvation path in which the verdict is
    never reached is what this ordering exists to foreclose.

**CLOSE.**
- Linger semantics: one CLOSE emitted, ≤ 1 reply/s under inbound flood —
  replies only to authenticated, window-fresh inbound, sent to the
  session address — state dropped at 5 s; receive side surfaces
  `PeerClosed` and never replies (§15.2).
- Mutual close: a CLOSE received while closing drains reply-free — no
  1 Hz ping-pong (§15.2).
- Violation ⇒ CLOSE with the matching registry code and
  `ConnectionLost::ProtocolViolation { code }` locally (§15.2, §15.3).

**The shell surface (rulings 46, 47, 49, 50).**
- **`closed()` resolves on every death, with no verb in flight** (§16.2,
  §15.4). Park a task on `closed()` and nothing else — no `read`, no
  `recv_message`, no send — and drive each teardown row in turn:
  `PeerClosed` (the peer closes), `TimedOut` (silence past `D_eff`, and
  separately the contested verdict), `Replaced` (a
  replacing `accept()`), `ProtocolViolation` (a semantic violation),
  `LocallyClosed`. Assert the correct `ConnectionLost` arrives in each,
  on the paused clock, **without the application ever calling a verb** —
  the pre-ruling-46 surface would have hung for ever here. Assert it is
  latched: a second `closed()` after death resolves immediately and with
  the same value, a `closed()` first awaited *after* the death resolves
  too, and several concurrent `closed()` futures all resolve.
- **Notification retention and its O(1) bound** (§16.2). Roam twice
  without claiming, then `notified()` once: assert the single
  `AddressMoved` carries the **oldest unclaimed `from`** and the
  **newest `to`**. Assert a notification generated before the death is
  still claimable after it, and that `notified()` returns
  `Err(ConnectionLost)` only once the slots are drained. Assert
  cancel-safety: dropping a `notified()` future claims nothing, and the
  next call yields the same notification.
- **Delivery confirmation, the message-then-close obligation** (§16.2,
  §9.8, §15.2, ruling 47) — the acceptance shape for the "farewell
  message". Over FlakyWire with **loss on**, on endpoint A:
  `send_message(m).await`, then `acked().await`, then
  `close(NO_ERROR, "").await`, then drop every handle. Assert endpoint B
  **receives `m` in full** from `recv_message()`. Run it enough times
  that the pre-ruling-47 ordering (send, close, drop, no flush) fails
  the same assertion at the injected loss rate — the test's whole point
  is that the ordering with `acked()` does not. Assert the same for
  `SendStream::acked()`: `write` a payload, `finish()`, `acked().await`,
  close, and assert the peer read every byte and the FIN. Assert
  `acked()` after `finish()` does **not** return `WriteError::Finished`;
  that a reset before acknowledgement returns `Reset(code)` rather than
  hanging; that `acked()` **terminates** while an unrelated bulk stream
  is still being written (snapshot semantics — later bytes do not extend
  it); and that a stream reset inside the snapshot lets `acked()`
  resolve `Ok(())` rather than waiting on bytes that will never be
  acknowledged.
- **A send failure is traced, never acted on** (§16.3, §18.2, ruling
  49). Give the endpoint a `Wire` whose `send_to` returns
  `ENETUNREACH` for a bounded interval, then heals. Assert the
  connection **survives** — no teardown, no verb resolving with an
  error, no notification — that a `slither::io` trace was emitted per
  failed send carrying the destination address, and that traffic
  resumes when the seam heals. Then hold the failure past `D_eff` and
  assert the death is still the ordinary
  receive-driven `TimedOut` (§7.4) with the traces present to explain
  it. A `FlakyWire` that can fail sends is the fixture; the obligation
  is unreachable without one.
- **Dropping a `Connecting` frees the static** (§16.3, §16.1, ruling
  50) — the `timeout()` shape every application writes. On the paused
  clock, dial a peer that never answers, drop the `Connecting` at
  *t* = 5 s (through `tokio::time::timeout`, so the test is the idiom),
  and assert: no further msg1 leaves the endpoint after the drop (the
  §5.5 train is stopped, not merely unobserved), and an **immediate**
  `connect()` to the same static returns `Ok` rather than
  `ConnectError::AlreadyConnected` — with no advance of the clock
  between the drop and the redial, which is what pins the cancellation
  ahead of the next endpoint verb. Assert also that the static is
  routable as **NONE**: an initiation arriving from that peer after the
  drop takes the ordinary staged accept and never §6.4's PENDING branch.
  Then the peer-side half: let the peer answer, drop the `Connecting`
  after its msg2 is on the wire, and assert the peer's half-open session
  transmits nothing and dies at its `D_eff` (25 s under the v1/default
  profile) — ruling 39's reap case (§7.4), which is the cost this ruling
  accepts.

**The composability surface (§16.11, rulings 55–58, 226, 227).**

**[RATIFIED 2026/08/16 — ruling 232.]** Ruling 58 ends *"Pinned by
Appendix B"* and §16.11 says *"Appendix B pins it"*. **Until this entry,
Appendix B contained no composability obligation at all** — the appendix
ran wire pins, handshake, frames, streams, ACK, liveness, CLOSE, the
shell surface (rulings 46/47/49/50) and the validation gates, and none of
them reached §16.11. Two documents asserted a pin that did not exist, for
the invariant §16.11 itself calls *"the easiest way to get the
composability layer wrong"*. Working rule 11 in the maintainer's own text.

- **An adapter claims at most one item, and only inside `poll_next`**
  (ruling 58 — **normative**). The assertion must separate a prefetcher
  from a correct adapter, and the obvious one does not: *polling once and
  asserting one item arrived passes a prefetching adapter.* Send
  **three** items, poll the adapter **exactly once** (`Waker::noop()`),
  then assert the underlying verb — `recv_message`, `recv_datagram`,
  `accept_bi` — yields the **second** item **immediately**, on its first
  poll. A prefetcher has swallowed items two and three, so that call
  parks. Keep the adapter alive across the assertion, or a build that
  prefetches and hands back on drop passes.
- **The `io::ErrorKind` table, row by row** (§16.11.1, ruling 227), in
  both directions, plus: the split is **by variant, not by direction**
  (some rows agree across read and write, some differ — assert both
  sides, since neither alone separates a direction-split build from a
  shared-conversion one); `into_inner().downcast()` recovers the original
  error including a reset's `u64` code, which an `io::Error::from(kind)`
  build fails while satisfying every kind assertion; and the
  `#[non_exhaustive]` fallback is `Other` and **never** a panic.
- **`poll_flush` is `Ready` with bytes unacknowledged** (ruling 56), and
  still `Ready` *after the connection dies* — the second case is what
  makes "it touches nothing" observable, separating *Ready because there
  is nothing to do* from *Ready because it asked*.
- **`poll_shutdown` is `finish()` and then `acked()`** (ruling 57):
  `Pending` immediately after a write with no driver turn — where a
  `finish()`-alone build is `Ready` — resolving **in error** on the
  connection's death rather than waiting for ever, and idempotent across
  repeated polls.
- **The `Result`-carrying adapters never end** (ruling 226): poll
  **repeatedly** after the first error and assert `Some(Err(..))` every
  time. A single post-error poll passes the one-`bool` build that ends.
- **The empty-buffer and EOF conventions** (rulings 110, 119, 121): an
  empty `ReadBuf` consumes nothing and does not latch EOF; a drained but
  open stream is `Pending`, **never** a zero-length fill — that inversion
  makes `read_to_end` report a truncation as success; EOF is sticky and
  survives the connection's death; an empty write is `Ok(0)` while a
  blocked non-empty write parks.

**Post-implementation validation obligations.** **[AMENDED 2026/08/18 —
ruling 260: both were stated as pre-ratification gates; ratification
(2026/08/14) overtook them undischarged, so each is re-scoped to a
measured, pinned obligation — the constants stay ratified either way.]**
- **The ACK-loss-burst simulation** (the §7.2/D-5 obligation):
  **discharged by measurement 2026/08/18** (`tests/spec_ack_burst.rs`,
  ruling 260). A 2 MiB stream at 20 ms RTT under return-path bursts of
  2/8, 4/8 and 6/8 datagrams measured spurious retransmission at 0.11 %,
  0.43 % and 1.55 % of forward datagrams; the pinned regression envelope
  is 2.5 %. The structural finding recorded with the numbers: on a
  lossless forward path the fused window is one contiguous block, so
  every ACK carries the receiver's complete state — ACK loss costs
  feedback *timing*, not *information* — and §19's range-tracker remedy
  is not needed. The separation mutant (a window that forgets ACKed
  ranges) scores 50.9 % spurious at zero ACK loss and dies at the first
  burst severity.
- **The window-constants throughput sanity check** (the §10.2 and §10.6
  obligation): the wall-clock bar this stated — **within 20 % of quinn
  under its shipped defaults** — is machine-relative and is **not
  dischargeable as a test** (ruling 260); `benches/throughput.rs` remains
  the wall-clock instrument. What survives as the pinned obligation is
  the other clause, *"with no stall"*, held deterministically in virtual
  time with the §10.6 reassembly bound active: a 2 MiB transfer over a
  zero-delay wire completes in 0 ns of virtual time (bound at
  `K_GRANULARITY`), and over the 20 ms-RTT fabric at 6.45 MiB/s with the
  regression floor at half that (`tests/spec_ack_burst.rs`) — before the
  §10.2 constants and `REASSEMBLY_CHUNKS_MAX` ratify. **[Run 2026/08/18 —
  ruling 270]** Run at a raised window (8 MiB, ruling 259(viii)'s knob),
  the gate came back **fatal rather than slow**: the flat ceiling killed a
  conforming sender under one socket-buffer loss burst (`round42-C`). The
  credit-derived ceiling is the discharge, pinned red-to-green by
  `tests/story_reassembly.rs` and the `tests_reassembly_credit` property
  suite.

## Named constants *(consolidated; reference suite where suite-dependent)*

| Constant | Value | Home |
|---|---|---|
| `VERSION` | 0x01 | §3.1 |
| `PROLOGUE` | `b"slither\x01"` | §5.1 |
| `PKT_HANDSHAKE_INIT` / `PKT_HANDSHAKE_RESP` / `PKT_DATA` | 0x01 / 0x02 / 0x03 | §3.1 |
| reserved packet types | 0x04 (unused), 0x05 (cookie/mac2) | §3.1 |
| `INIT_HEADER_LEN` / `RESP_HEADER_LEN` / `DATA_HEADER_LEN` | 6 / 10 / 14 B | §3.2–3.4 |
| `MAC1_LABEL` / `MAC1_LEN` | `b"slither mac1"` / 16 B | §4.1 |
| `TIMESTAMP_LEN` / `MSG1_PAYLOAD_LEN` | 12 / 12 B (msg2 has no payload) | §5.2 |
| `IK_MSG1_LEN` / `IK_MSG2_LEN` | 174 / 81 B | §2.3 |
| `INIT_PACKET_LEN` / `RESP_PACKET_LEN` | 196 / 107 B | §2.3 |
| `AEAD_TAG_LEN` | 16 B | §2.3 |
| `MAX_DATAGRAM` / `MAX_PLAINTEXT` | 1200 / 1170 B | §3.5 |
| `REKEY_EPOCH_MSGS` / `MAX_EPOCH_JUMP` | 65 536 / 2 (hiss-fixed) | §7.7 |
| `REPLAY_WINDOW` | 2048 bits | §7.2 |
| frame types | 0x00, 0x01, 0x02, 0x04, 0x08–0x0f, 0x10–0x13, 0x1a/0x1b, 0x1c, 0x30/0x31; 0x05 reserved | §8.3 |
| `STREAM_OFF` / `STREAM_LEN` / `STREAM_FIN` | 0x04 / 0x02 / 0x01 | §8.4 |
| `INITIAL_MAX_DATA` | 1 048 576 B (1 MiB) | §10.2 |
| `INITIAL_MAX_STREAM_DATA` | 262 144 B (256 KiB) | §10.2 |
| `INITIAL_MAX_STREAMS_BIDI` / `_UNI` | 32 / 128 (cumulative) | §10.2 |
| `STREAMS_CREDIT_BATCH` | 8 | §10.4 — **receiver policy**, not a wire constant (ruling 103) |
| credit re-grant threshold | ½ window consumed | §10.3 |
| `MESSAGE_RECV_MAX` | = `INITIAL_MAX_STREAM_DATA` | §9.8 |
| `MAX_DATAGRAM_PAYLOAD` | 1169 B (= `MAX_PLAINTEXT` − 1) | §11.2 |
| `DATAGRAM_SEND_QUEUE` / `DATAGRAM_RECV_QUEUE` | 64 / 64 (count; drop-oldest, newest always accepted; ≈ 73 KiB worst case each) | §11.3 |
| `REASSEMBLY_CHUNKS_MAX` | 1024 — the credit-derived ceiling's **floor** (ruling 270) | §10.6 — receiver policy, **externally observable** (ruling 103) |
| `REASSEMBLY_MIN_CONFORMING_FRAME` | 1024 B — ceiling = max(floor, `W / P` + 1) | §10.6 — receiver policy, observable (ruling 270) |
| `CLOSE_REASON_MAX` | 256 B | §8.4 |
| `CLOSE_LINGER` / close-reply rate | 5 s / ≤ 1 per s | §15.1 |
| `MAX_ACK_RANGES` | 64 | §12.2 |
| ACK policy | every 2nd ack-eliciting **due**, coalesced per receive drain (ruling 271), `MAX_ACK_DELAY` cap, immediate on gap | §12.4 |
| `MAX_ACK_DELAY` | 25 ms | §12.4 / §13.3 |
| `ACK_COALESCE_MAX` | 32 — the coalescing valve (ruling 271) | §12.4 |
| `K_PACKET_THRESHOLD` / time threshold / `K_GRANULARITY` | 3 / 9⁄8 / 1 ms | §13.2 |
| `K_INITIAL_RTT` / `PTO_BACKOFF_CAP` | 333 ms / 2³ | §13.1 / §13.3 |
| `INITIAL_WINDOW` / `MINIMUM_WINDOW` | 12 000 / 2 400 B | §14.2 |
| `LOSS_REDUCTION_FACTOR` / `PERSISTENT_CONGESTION_THRESHOLD` | 0.5 / 3 | §14.2 / §14.4 |
| `RETRANSMIT_BASE` / `RETRANSMIT_JITTER_MAX` | 5 s / 333 ms | §5.5 |
| `HANDSHAKE_GIVEUP` | 90 s | §5.5 |
| `KEEPALIVE_TIMEOUT` / `DEAD_TIMEOUT` | 10 s / 25 s; the exact v1/default `TimingProfile` | §5.7, §7.5 |
| `PERSISTENT_KEEPALIVE` (default, per-connection) | 10 s; admissible range **[1 s, effective dead timeout)** — handle-rejected **below 1 s** (floor, ruling 42) and **at or above** the connection's `D_eff` (ceiling, rulings 40 and 282) | §7.5 |
| `AMPLIFICATION_FACTOR` | 3 (× authenticated, window-fresh bytes received, per unvalidated address, per session; disarmed by a `PATH_RESPONSE` echoing the arming's 8-byte challenge — rulings 168–170, superseded in the disarm predicate only by ruling 208) | §7.3 |
| `INTRO_QUEUE_CAP` / `INTRO_MAX_PER_SOURCE` / `INTRO_TTL` | 1024 / 4 / 15 s | §6.3 |
| `TS_GUARD_ORPHAN_CAP` | 1024 | §17.1 |
| `TS_GUARD_ORPHAN_TTL` | `= INTRO_TTL` (15 s) | §17.1 |
| `L` (shell lateness bound) | 250 ms | §16.5 |
| wire error codes | 0x00–0x06 + ≥ 0x10 application; 0x07–0x0f reserved | §15.3 |
| session index | nonzero u32, random, re-drawn across both tables | §17.3 |

### Constants the spec fixes but never names

**[RATIFIED 2026/08/14 — ruling 63]** Every value above is normative, but
thirteen of them are stated as prose, as a compressed range, or under a
one-letter alias, and therefore have **no identifier a reader can grep
for**. Two independent readers of this document — slice 0's planner and
its conformance-test author, working without sight of each other —
both stopped at the same gap, which is the evidence that it is one.

The identifiers below are **normative names for values this document
already fixes**. Nothing here changes a value, a byte, or a behaviour.

| Name | Value | Home | What the table says instead |
|---|---|---|---|
| `STATIC_PUBLIC_LEN` | 65 B | §2.4 | "the 65-byte uncompressed SEC1 storage form"; §2.3 writes it `PK` |
| `PKT_RESERVED_UNUSED` / `PKT_RESERVED_COOKIE` | 0x04 / 0x05 | §3.1 | "reserved packet types … 0x04 (unused), 0x05 (cookie/mac2)" |
| `FRAME_STOP_SENDING_RESERVED` | 0x05 | §8.3 | "(reserved: STOP_SENDING)" |
| `STREAM_FLAG_MASK` | 0x07 | §8.4 | the three flags are named; their union is not |
| `CREDIT_REGRANT_DIVISOR` | 2 | §10.3 | "credit re-grant threshold … ½ window consumed" |
| `ACK_ELICITING_PER_ACK` | 2 | §12.4 | "every 2nd ack-eliciting" |
| `K_TIME_THRESHOLD_NUM` / `_DEN` | 9 / 8 | §13.2 | "time threshold … 9⁄8" |
| `PTO_BACKOFF_CAP` | **8** | §13.3 | written "2³" |
| `CLOSE_REPLY_MIN_INTERVAL` | 1 s | §15.1 | "close-reply rate … ≤ 1 per s" |
| `SHELL_LATENESS_BOUND` | 250 ms | §16.5 | the spec calls it `L` |
| `PERSISTENT_KEEPALIVE_DEFAULT` / `_MIN` | 10 s / 1 s | §7.5 | one bare `PERSISTENT_KEEPALIVE`, range in prose |
| `APPLICATION_ERROR_BASE` | 0x10 | §15.3 | "≥ 0x10 application" |

**Two shapes recur, and naming them is worth more than the thirteen
entries.** Both are ways a specification can be complete and still
unimplementable without a guess.

1. **A ratio or rate stated in prose** — `½ window`, `every 2nd`, `9⁄8`,
   `≤ 1 per s`, `2³` — needs an identifier **and a stated unit**.
   `PTO_BACKOFF_CAP` proves the point: "2³" is a multiplier (8) in
   §13.3's sentence and reads as an exponent (3) in the table, and an
   implementer who guessed wrong would back off 8× too little with
   nothing red to show for it — more silently since ruling 254 lowered
   the cap from the inherited 2⁶: `1u32 << 8` is a legal 256 where
   `1u32 << 64` was undefined, so the compile-time pins
   (`PTO_BACKOFF_CAP == 1 << 3`, `PTO_MAX_EXPONENT == 3`) are
   load-bearing rather than belt-and-braces.
2. **A range stated in prose against one policy value** — the admissible
   `[1 s, D_eff)` for `PERSISTENT_KEEPALIVE` — becomes two identifiers
   and one live comparison in code. Name the **default** and the
   **floor**, and leave the ceiling as a comparison against the
   connection's effective dead timeout: a named ceiling would duplicate
   a value that now varies by endpoint and can drift from the policy
   actually governing that connection.

A future constant SHOULD be added to the table with an identifier, not
only a value, and a prose-stated ratio SHOULD carry its unit.
