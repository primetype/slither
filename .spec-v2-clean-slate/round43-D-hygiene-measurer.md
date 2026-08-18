# GAPSLICE-D — Registry hygiene: measured evidence and drafted proposals

Measurer D. Base commit: `b072afd` (verified by `git log --oneline -1` as first act,
working rule 14 — no reset needed).

REPORT-ONLY: nothing but this file is committed. Scratch test code is written,
run, pasted here, and reverted.

---

## 0. Base verification (rule 14)

```
$ git log --oneline -1
b072afd Ruling 271: the record — §12.4's emission point, §16.5's drain order
$ git rev-parse --abbrev-ref HEAD
worktree-wf_a5f79863-6bc-4
```

Base is the briefed commit. No reset was required.

---

## Summary

| Item | Finding |
|---|---|
| **1. `mod owed`** | **7 of 8 answered.** Six outright stale (rulings 83, 84, 85, 65 and slices 4–7 settled them); entry 6 has its substance discharged elsewhere but its literal sentence is still true. Entry 5 stays, pending a ruling. |
| **1.2 `VIOLATION_WHILE_CLOSING`** | **Measured: the violation is IGNORED.** The packet emitted is §15.2's ordinary linger reply — the original CLOSE verbatim, byte-for-byte identical to what a *benign* packet produces — under the ≤ 1/s cap. No second `Closed`, no `PROTOCOL_VIOLATION`, linger unmoved. **But §8.2 states the contrary rule with no scope clause** → C1. |
| **2. O13/O53a/O53b** | **Not numbering drift — dangling pointers.** Appendix B has never carried an O-number in any revision. The ids belong to `audit/G-obligations-trend.md`, which **does not exist** anywhere in the repo or its history. Recommend citing by bold title plus a provenance note. |
| **3. SECV5-5/6/8** | 5 and 6 **found** (sites located, comment lines drafted); **SECV5-8 NOT discharged** — and the two tests that look like they discharge it pass on the broken build → C3. |

**Six conflicts reported, none resolved** (§4). **Nothing was applied**: this
commit adds only this file.

## 1. `mod owed` — the 8 entries at `src/core/connection/tests.rs:3065-3151`

### 1.1 Entry-by-entry verification (rule 11: open the artefact each claim is about)

The block is `#[allow(dead_code)] mod owed` at `src/core/connection/tests.rs:3065`
(the `mod owed {` line), running to `:3150`. Eight `pub const … : () = ();` items:

| # | Item | line |
|---|------|------|
| 1 | `ESTABLISHED_HAS_NO_INSTANT` | 3079 |
| 2 | `LIVENESS_EXACT_INSTANT` | 3091 |
| 3 | `CLOSE_RATE_CLOCK_ORIGIN` | 3099 |
| 4 | `RETIRED_WITHOUT_A_SESSION` | 3109 |
| 5 | `VIOLATION_WHILE_CLOSING` | 3120 |
| 6 | `SEAL_VERSUS_SEAL_QUIET` | 3132 |
| 7 | `PACKING_ORDER` | 3140 |
| 8 | `OVERSIZE_DATA_PACKET` | 3150 |

(line numbers from `grep -n "pub const .*: () = ();" src/core/connection/tests.rs`)

---

#### 1 — `ESTABLISHED_HAS_NO_INSTANT` (:3079) — **STALE, discharge**

*Claim:* "`Connection::established(sub_seed, session)` has no argument to pin
[the install instant] to … §7.4's install pin is **unstated** for the
constructor §16.4 gives `accept()`."

*Artefact opened:* `src/core/connection/mod.rs:354-407`.

```rust
pub(crate) fn established(
    now: Instant,
    seed: impl Into<ConnSeed>,
    session: EstablishedSession<C>,
    role: Role,
) -> Self {
```

The signature **now takes `now: Instant` first**, and its own doc comment
(`mod.rs:350-353`) states the resolution: *"`now` is the install instant. §7.4
pins both liveness clocks to it and starts the death deadline **already armed**
— the accept path has no later event to read that instant from."* The pin lands
at `mod.rs:405` (`conn.install(now, session, role, true)`) →
`session.rs:410-414` (`Session::install` → `Liveness::pinned_at_install(now)`)
→ `session.rs:290-293`, which sets `last_authenticated_recv: now` and
`last_send: now`. The spec side of the claim (§7.4, `SPEC.md:2641-2643`) is
unchanged and is what the code implements.

The item is answered by the code as it stands. **Discharge.**

---

#### 2 — `LIVENESS_EXACT_INSTANT` (:3091) — **STALE, discharge (+ a second stale site)**

*Claim:* "§7.4 states the death condition as
`now − last_authenticated_recv > DEAD_TIMEOUT` — strictly greater — [while]
§16.5 says an armed deadline `D` fires 'no earlier than `D`' … At exactly
`anchor + DEAD_TIMEOUT` the two readings differ."

*Artefact opened:* `SPEC.md:2626-2628`.

> The connection is dead when
> `now − last_authenticated_recv >= DEAD_TIMEOUT` **[AMENDED 2026/08/15 —
> ruling 85; was `>`]** **and** at least one **arming** send has occurred …

The disagreement the item records **was resolved by ruling 85 on 2026/08/15**:
§7.4 now reads `>=`, which is exactly §16.5's "fires no earlier than `D`". There
is no longer a contested instant. The code agrees — `session.rs:311` arms the
deadline at `self.last_authenticated_recv + constants::DEAD_TIMEOUT`, with no
epsilon.

**Second stale site, same claim.** The doc comment on the test the item names,
`src/core/connection/tests.rs:1162-1166`, still carries the pre-amendment text
verbatim:

> **Spec gap, deliberately not pinned here.** §7.4 states the death
> condition as `now − last_authenticated_recv > DEAD_TIMEOUT` —
> strictly greater — while §16.5 says an armed deadline `D` fires "no
> earlier than `D`".

Discharging the `owed` item without touching this leaves the same withdrawn
claim asserted in the file — working rule 4's shape. Both sites are listed in
§1.3.

*Follow-on note (not a conflict, a coverage observation):* with the spec settled
at `>=`, the exact instant `t + DEAD_TIMEOUT` is now pinnable, and
`a_half_open_session_is_reaped_in_silence_and_not_one_nanosecond_early`
(`tests.rs:1169`) still brackets it at `−1 ns` / `+1 ns` only. Tightening the
late assertion from `t + DEAD_TIMEOUT + NS` to `t + DEAD_TIMEOUT` is now
spec-supported. That is a test change, outside this report-only task; flagged
for whoever owns the follow-up slice.

---


#### 3 — `CLOSE_RATE_CLOCK_ORIGIN` (:3099) — **STALE, discharge (+ a second stale site)**

*Claim:* "§15.2 says 'emit CLOSE and enter closing … Replies are capped at one
CLOSE per second' without saying whether the emitted CLOSE is itself the first
item under the cap."

*Artefact opened:* `SPEC.md:5210-5217`.

> Replies are capped at one CLOSE per second, and **the opening CLOSE is not a
> reply** **[RATIFIED 2026/08/15 — ruling 83]**: the rate clock is unset at
> `close()`, so the first reply owed to an inbound packet is sent at once.

The spec now says it explicitly. **Ruling 83 ratified the answer.** The code
implements it: `src/core/connection/close.rs:99-104`, `Closing::new` sets
`last_reply: None`, and `Closing::reply` (`close.rs:118-127`) treats `None` as
due. And it is **pinned by two tests**, so this is not merely unstated-and-
implemented:

- `src/core/connection/close.rs:157`
  `the_first_reply_is_not_rate_limited_by_the_opening_close` — `Closing::new(now, …)`
  then `closing.reply(now) == Some(close_frame())` at the *same instant*.
- `src/core/connection/tests.rs:2692` `a_call_that_sends_nothing_burns_no_counter`,
  which feeds at `t + 1 ms` after `close(t, …)` and asserts one transmit, with
  the comment naming ruling 83.

**Second stale site.** The doc comment on the flood test,
`src/core/connection/tests.rs:2169-2171`, still says *"whether the local CLOSE
itself starts the rate clock, and at two seconds both readings agree"* and
points at `owed::CLOSE_RATE_CLOCK_ORIGIN`. The `base = t + Duration::from_secs(2)`
offset in `the_linger_replies_at_most_once_per_second_under_a_flood`
(`tests.rs:2178`) is the hedge that reading bought; it is now merely inert, not
wrong. Both sites are listed in §1.3.

---

#### 4 — `RETIRED_WITHOUT_A_SESSION` (:3109) — **STALE, discharge**

*Claim:* "Ruling 81 puts 'any teardown before a session exists' in the no-linger
class, so `Closed` is followed by `Retired` in the same drain. But §17.3 mints a
session index at the handshake … §16.4 gives `Retired` no other shape and
nothing says what it carries here."

*Artefact opened (rule 11 — the claim is about a **ruling**, so the ruling is what
must be read):* `.spec-v2-clean-slate/rulings.md:1691-1698`.

> **Ruling 84 — no `Retired` without a session.** Ruling 81, one day old,
> listed "any teardown before a session exists" among the no-linger paths that
> MUST emit `ToEndpoint::Retired`. But `Retired { our_index }` names the index
> route it exists to drop, and a connection that never installed a session never
> had one. The case is **not constructible** … Correct: `Closed(ConnectionLost)`
> is emitted alone, there is nothing to retire, and the MUST protects against a
> leak that cannot occur.

Ruling 84 is the direct answer, and it corrects the very ruling 81 clause the
item quotes. The code matches, at two places:

- `src/core/connection/mod.rs:785-800` — the `close()` fast path, when
  `self.session.is_none()`, emits `Closed(LocallyClosed)` and returns **before**
  `drop_state`, with the comment *"No `Retired` either: it carries `our_index`, a
  **session** index this connection has never had."*
- `src/core/connection/mod.rs:2052-2056` — `drop_state` guards the emission with
  `if let Some(session) = self.session.take()`, so no path can emit a sessionless
  `Retired`.

**Discharge.**

---

#### 6 — `SEAL_VERSUS_SEAL_QUIET` (:3132) — **substance discharged; its literal scope claim is still true**

*Claim:* "T16: `seal_quiet` must not touch `last_send`, `seal` must. Slice 3a
seals **only** CLOSE … a build in which `seal_quiet` is a plain alias for `seal`
passes every test in this file. The separating assertion needs a
first-transmission STREAM or DATAGRAM frame, and arrives with slice 4."

*Measured, not reasoned.* I applied the exact mutant the item names — made
`seal_quiet` a plain alias for `seal` by flipping its `marking` argument in
`src/core/connection/session.rs:483` (`seal_inner(now, plaintext, false, …)` →
`true`) — and ran the suite.

```
$ cargo test --all-features
test core::connection::session::transport_tests::seal_quiet_does_not_mark_and_seal_does ... FAILED
test core::connection::tests_ack::policy_on_the_wire::a_pure_ack_packet_is_quiet_unelicited_and_untracked ... FAILED
test core::connection::tests_ack_cadence::a_coalesced_ack_moves_no_liveness_clock ... FAILED
test core::connection::tests_ack_cadence::an_ack_only_emission_cannot_defer_death ... FAILED
test core::connection::tests_park::a_starved_passive_keepalive_must_not_park_the_connection_forever ... FAILED
test core::connection::tests_roam::a_quiet_send_neither_advances_s_nor_suppresses_the_keepalive ... FAILED
test core::connection::tests_roam::a_marking_send_in_the_same_evaluation_suppresses_the_keepalive ... FAILED
test core::connection::tests_roam::an_owed_ack_is_emitted_before_the_keepalive_at_one_instant ... FAILED
test core::connection::tests_roam::the_passive_keepalive_arms_only_after_a_receive ... FAILED
test core::connection::tests_roam::the_passive_keepalive_sends_the_empty_plaintext_and_then_disarms ... FAILED
test core::connection::tests_streams::sealing::a_reset_stream_only_packet_does_not_mark_last_send ... FAILED
test result: FAILED. 766 passed; 11 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.49s
```

Mutant reverted with `git checkout -- src/core/connection/session.rs`.

**Eleven tests separate the alias today**, including `tests_streams::sealing::a_reset_stream_only_packet_does_not_mark_last_send`
— a frame-carrying separator of exactly the shape the item said slice 4 owed.
The substance ("do not let the absence of a red test here read as coverage") is
**discharged**.

**But the item's literal sentence is still true, and the discharge text must not
pretend otherwise.** Scoped to the file the item is written in:

```
$ cargo test --all-features --lib core::connection::tests::
test result: ok. 82 passed; 0 failed; 0 ignored; 0 measured; 695 filtered out; finished in 0.22s
```

Under the mutant, all 82 tests in `src/core/connection/tests.rs` still pass. The
separator lives in sibling files, not this one. So this entry is not stale in the
way 1–4 are: the **gap it names is closed**, the **sentence describing where it
is closed is what changed**. The drafted discharge in §1.3 says that rather than
"stale".

---

#### 7 — `PACKING_ORDER` (:3140) — **STALE, discharge**

*Claim:* "'ACK first (if owed), then control frames …, then STREAM and DATAGRAM
fill, then PING last' — slice 3a emits exactly one frame type, CLOSE, and never
coalesces. The order, and the one-extends-to-end-frame rule, are owed by slices
4 and 5."

Slices 4 and 5 landed. The owed coverage exists, in three places:

- `src/core/connection/tests_streams.rs:2421` — `mod packing`, holding
  `at_most_one_extends_to_end_frame_per_packet_and_it_is_last` (:2432 — asserts
  the count **and** the position), `the_stream_fill_serves_pending_streams_round_robin`
  (:2490), and `credit_frames_precede_the_stream_fill_in_a_packet` (:2566).
  That is the extends-to-end rule and the control-before-fill half.
- `src/core/connection/tests_contested.rs:1579` —
  `both_owed_path_frames_ride_the_probe_in_one_packet_in_spec_8_5_order`, whose
  own doc (:1568) states the ordering it pins: *"before `PATH_CHALLENGE`, PING
  last among length-prefixed frames"*. That is the **PING-last** half.
- `src/core/connection/tests_ack.rs:1643` — §12.4's *"an owed ACK rides the next
  outgoing packet (packing order §8.5)"*, the **ACK-first** half, with its
  mutation named in the doc.

Every clause the item lists is now pinned. **Discharge.**

---

#### 8 — `OVERSIZE_DATA_PACKET` (:3150) — **STALE, discharge**

*Claim:* "§3.1's pre-AEAD gate … states the gate for the handshake packet types.
Nothing in §7 or §8 says what an authenticated Data packet longer than
`MAX_DATAGRAM` does, and no test here pins one, because either answer (silent
drop, or open and parse) is defensible from the text."

*Artefact opened:* `SPEC.md:678-694`. §3.1's accepted-length table **has a
`PKT_DATA` row**:

> | `PKT_DATA` | `DATA_HEADER_LEN + AEAD_TAG_LEN` (30) ≤ len ≤ `MAX_DATAGRAM` (1200) |

and the paragraph below it (`SPEC.md:693-695`) reads *"A datagram outside its
type's accepted length, longer than `MAX_DATAGRAM`, or bearing an unknown type
or version is silently dropped before any further work."* §3.5 (`SPEC.md:778`)
says the same independently: *"Oversize receive (> `MAX_DATAGRAM`) is a silent
drop."*

So the premise is false twice over: the gate is **not** stated only for the
handshake types, and the second answer the item calls defensible ("open and
parse") is excluded — the drop is *pre*-AEAD, so "an **authenticated** Data
packet longer than `MAX_DATAGRAM`" is not a reachable state.

The code matches, at `src/packet/mod.rs:146-150`, where the size cap is step 1
of `classify`, ahead of the type byte:

```rust
// 1. Oversize. Type-independent, so it comes first (§3.5).
if dgram.len() > constants::MAX_DATAGRAM {
    return None;
}
```

and it is pinned **two-sided** (working rule 9's shape) at
`src/packet/tests.rs:135` `oversize_is_dropped`: `MAX_DATAGRAM + 1` is `None`,
`MAX_DATAGRAM` exactly is `Some`. The item's "no test here pins one" was true of
`tests.rs`; the pin belongs at the gate, and it is there.

**Discharge.**

---

### 1.2 `VIOLATION_WHILE_CLOSING` — measured current behaviour

The item claims: "Whether a second violation re-signals (a second CLOSE with a
different code, against the 1 Hz reply rule's intent) or is ignored is
`PLAN.md` U4's open question, not a spec rule."

I drove the state rather than reasoning about it.

#### The scratch test

Appended to `src/core/connection/tests.rs`, run, then reverted with
`git checkout -- src/core/connection/tests.rs`. Core-level only, explicit
`Instant`s, no shell, no sleep. **Case B is the load-bearing one** — without a
benign control, case A's transmit reads as a re-signal when it is not.

```rust
mod scratch_measure_violation_while_closing {
    use super::*;

    const APP_CODE: u64 = APPLICATION_ERROR_BASE + 7;

    /// Report what a drain contained, or that it was silent.
    fn describe(f: &mut Fixture, d: &Drained, label: &str) {
        let n = d.transmits().len();
        if n == 0 {
            println!("  {label}: NO transmit");
        } else {
            let tx = d.one_transmit();
            let close = one_close(&mut f.peer, &tx.data);
            println!(
                "  {label}: {n} transmit, CLOSE code=0x{:02x} reason={:?}",
                close.code,
                String::from_utf8_lossy(&close.reason)
            );
        }
        println!("     closed events: {:?}", d.closed());
        println!("     retired: {:?}", d.retired());
    }

    #[test]
    fn measure() {
        let t = t0();

        println!("\n=== BASELINE: structural violation on a LIVE connection ===");
        {
            let mut f = established_at(t);
            let d = f.deliver(t, &vi(0x7f));
            describe(&mut f, &d, "violation on live");
            println!("     deadline == t + CLOSE_LINGER: {}",
                     d.deadline == Some(t + CLOSE_LINGER));
        }

        println!("\n=== A: close(APP_CODE,\"bye\"), then a structural violation at t+2s ===");
        {
            let mut f = established_at(t);
            let d0 = f.close(t, APP_CODE, b"bye");
            describe(&mut f, &d0, "the opening close");

            let at = t + Duration::from_secs(2);
            let d = f.deliver(at, &vi(0x7f));
            describe(&mut f, &d, "violation while closing");
            println!("     deadline unmoved at t+CLOSE_LINGER? {}",
                     d.deadline == Some(t + CLOSE_LINGER));
        }

        println!("\n=== B: control — close(APP_CODE), then a BENIGN packet at t+2s ===");
        {
            let mut f = established_at(t);
            let _ = f.close(t, APP_CODE, b"bye");
            let at = t + Duration::from_secs(2);
            let d = f.deliver(at, &padding(1));
            describe(&mut f, &d, "benign while closing");
        }

        println!("\n=== C: three violations across the reply interval ===");
        {
            let mut f = established_at(t);
            let _ = f.close(t, APP_CODE, b"bye");
            let at = t + Duration::from_secs(2);
            let d1 = f.deliver(at, &vi(0x7f));
            describe(&mut f, &d1, "violation #1");
            let d2 = f.deliver(at + Duration::from_millis(10), &vi(0x7f));
            describe(&mut f, &d2, "violation #2 (+10ms)");
            let d3 = f.deliver(at + CLOSE_REPLY_MIN_INTERVAL, &vi(0x7f));
            describe(&mut f, &d3, "violation #3 (+1s)");
        }

        println!("\n=== D: violation while DRAINING (peer CLOSE received while closing) ===");
        {
            let mut f = established_at(t);
            let _ = f.close(t, APP_CODE, b"bye");
            let at = t + Duration::from_secs(2);
            let dc = f.deliver(at, &close_frame(0x00, b""));
            describe(&mut f, &dc, "peer CLOSE while closing");
            let d = f.deliver(at + Duration::from_millis(10), &vi(0x7f));
            describe(&mut f, &d, "violation while draining");
        }

        println!("\n=== E: does the linger still expire normally after a violation? ===");
        {
            let mut f = established_at(t);
            let _ = f.close(t, APP_CODE, b"bye");
            let _ = f.deliver(t + Duration::from_secs(2), &vi(0x7f));
            let d = f.timeout(t + CLOSE_LINGER);
            describe(&mut f, &d, "at t+CLOSE_LINGER");
        }
    }
}
```

#### Its output

```
$ cargo test --all-features --lib scratch_measure -- --nocapture

=== BASELINE: structural violation on a LIVE connection ===
  violation on live: 1 transmit, CLOSE code=0x01 reason=""
     closed events: [ProtocolViolation { code: 1 }]
     retired: []
     deadline == t + CLOSE_LINGER: true

=== A: close(APP_CODE,"bye"), then a structural violation at t+2s ===
  the opening close: 1 transmit, CLOSE code=0x17 reason="bye"
     closed events: [LocallyClosed]
     retired: []
  violation while closing: 1 transmit, CLOSE code=0x17 reason="bye"
     closed events: []
     retired: []
     deadline unmoved at t+CLOSE_LINGER? true

=== B: control — close(APP_CODE), then a BENIGN packet at t+2s ===
  benign while closing: 1 transmit, CLOSE code=0x17 reason="bye"
     closed events: []
     retired: []

=== C: three violations across the reply interval ===
  violation #1: 1 transmit, CLOSE code=0x17 reason="bye"
     closed events: []
     retired: []
  violation #2 (+10ms): NO transmit
     closed events: []
     retired: []
  violation #3 (+1s): 1 transmit, CLOSE code=0x17 reason="bye"
     closed events: []
     retired: []

=== D: violation while DRAINING (peer CLOSE received while closing) ===
  peer CLOSE while closing: NO transmit
     closed events: []
     retired: []
  violation while draining: NO transmit
     closed events: []
     retired: []

=== E: does the linger still expire normally after a violation? ===
  at t+CLOSE_LINGER: NO transmit
     closed events: []
     retired: [286331153]
test core::connection::tests::scratch_measure_violation_while_closing::measure ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 777 filtered out; finished in 0.19s
```

#### What the code actually does today — **the violation is ignored**

Read against the baseline and the control, the answer is unambiguous:

| Question the `owed` item asks | Measured answer |
|---|---|
| Is a **second CLOSE** emitted? | A packet is emitted, but it is **not a re-signal** — see below |
| With a **different code**? | **No.** `code=0x17`, the *original* `APP_CODE`, `reason="bye"` verbatim. The baseline shows a real structural signal carries `0x01` `PROTOCOL_VIOLATION`; nothing here does |
| Or is it **ignored**? | **Ignored.** |
| Second `Closed` event? | **No** — `closed events: []` |
| Linger extended? | **No** — deadline unmoved at `t + CLOSE_LINGER`, and E confirms it expires there with `Retired` |

**Case B is what settles it.** A *benign* padding packet at the same instant
produces the **identical** output — `1 transmit, CLOSE code=0x17 reason="bye"`.
So the transmit in case A is §15.2's ordinary linger reply owed to any
authenticated, window-fresh inbound packet, **not** a response to the violation.
The violation contributes nothing.

**Case C** confirms the packet is governed by the ordinary ≤ 1/s reply cap and
not by any violation-specific path: violation #2 at +10 ms is silent, #3 at
+1 s replies again — exactly `CLOSE_REPLY_MIN_INTERVAL`.

**Case D** extends the answer past what the item asked: once a peer CLOSE has
moved the connection to **draining**, a violation is **fully silent** — no
transmit at all, matching §15.2's reply-free draining behaviour.

So the code implements U4's provisional decision exactly: *"a structural failure
while closing is ignored"*.

#### The mechanism, named and opened (working rule 11)

The behaviour is not incidental — it is written down, at
`src/core/connection/mod.rs:1917-1929`:

```rust
/// Apply a received packet to a **closing** or **draining** connection.
///
/// §15.2's retention list is exhaustive … the only thing the frame stream
/// is still read for is the peer's CLOSE.
///
/// A structural failure here is ignored: we are already dying, with a
/// code, and a second CLOSE carrying a different one would fight the
/// reply rule it would have to travel under.
fn apply_post_mortem(&mut self, now: Instant, received: Received) {
    let peer_closed = match &received {
        Received::Frames(frames) => frames.iter().any(|f| matches!(f, Frame::Close(_))),
        Received::Keepalive | Received::Structural(_) => false,
    };
```

Three facts follow that matter to the ruling, and only the first is obvious:

1. **The violation is parsed and detected, then discarded — not skipped.**
   `handle_datagram` runs `frame::parse` unconditionally
   (`mod.rs:622-626`) and captures the failure as `Received::Structural(error)`.
   `apply_post_mortem` then matches it into the same arm as
   `Received::Keepalive` and drops it. This is U4's *"parse the frame stream and
   apply nothing but CLOSE detection"*, implemented literally.
2. **No trace fires.** The live path traces at `mod.rs:1719-1722`
   (*"structural failure in the frame stream; closing"*); `apply_post_mortem`
   traces nothing at all. A peer that floods garbage for the whole 5 s linger is
   invisible to `slither::` tracing. This is the one respect in which the
   "ignore" answer is lossier than it first appears, and it is the strongest
   argument for the amendment being *narrower* than "ignore" — see option (2b).
3. **U9 is discharged by construction.** `.slices/03-skeleton/PLAN.md:1412`
   records U9 (*"§8.2's 'one trace fires': one per what?"*) as *"rendered vacuous
   by U4"*. Fact 2 is why: closing traces nothing, so at most one structural
   trace can ever fire per connection. That reasoning is now checkable against
   the code rather than assumed.

#### `PLAN.md U4` — does it exist?

**It does not, and the citation is wrong in a way worth fixing rather than
deleting.**

```
$ grep -n "U4\|U-4" PLAN.md
(no output)
$ grep -nE "\bU[0-9]+\b|\bU-[0-9]+\b" PLAN.md
(no output)
```

The repo-root `PLAN.md` has **no U-numbered items at all**, so the citation
cannot be resolved as written — and this is quoted twice, at
`src/core/connection/tests.rs:3117` (the `owed` item) and at
`src/core/connection/tests.rs:2584` (the test's doc comment: *"`PLAN.md` U4 asks
what a *closing* connection does with a structurally invalid frame stream"*).

The question **does** exist, at a different path:

```
$ grep -rn "U4\b" --include="*.md" .
.slices/03-skeleton/PLAN.md:1325:### U4 — what does a **closing** connection do with a received frame stream?
.slices/03-skeleton/PLAN.md:1412:Rendered vacuous by **U4**'s provisional decision — the first structural
.slices/03-skeleton/IMPLEMENTATION.md:33:- U4 — closing/draining parses the frame stream but applies nothing but
.slices/04-streams/PLAN-4b.md:918:**U4 — `ShellState::handles` enumerates three handle kinds and 4b adds
```

`.slices/03-skeleton/PLAN.md:1325-1348` is the real U4, and its provisional
decision is exactly what I measured:

> *Provisional:* while closing or draining, **parse** the frame stream and apply
> **nothing but CLOSE detection**; a structural failure while closing is ignored
> (we are already dying, with a code, and a second CLOSE carrying a different
> code would fight the ≤ 1/s reply rule's intent).

Note the collision: `.slices/04-streams/PLAN-4b.md:918` also has a **U4**, about
`ShellState::handles`. So "`PLAN.md` U4" is ambiguous even once the path is
fixed; the citation needs the slice directory in it. Whatever the maintainer
rules on the substance, the citation should become
**`.slices/03-skeleton/PLAN.md` U4** at both sites.

#### Drafted ruling options

**First, the finding that makes this a spec question rather than a plan
question.** §8.2's structural rule (`SPEC.md:3385-3394`) reads:

> **Structural failure** — … is a **signalled death**. **[RATIFIED 2026/08/14]**
> Nothing from the packet is applied …, one trace fires on `slither::frames`,
> and the connection emits CLOSE with `PROTOCOL_VIOLATION` … and enters the
> closing state (§15.2), surfacing `ConnectionLost::ProtocolViolation { code }`.

**It carries no scope clause.** Nothing in §8.2 — structural *or* semantic
class — says "on an established connection". Read literally, as working rule 8
says a stated construction must be read, §8.2 requires the re-signal, and the
code does not do it. So the honest framing for the maintainer is not "the plan
left this open" but **"§8.2 states a construction whose scope is unstated, and
the implementation has silently assumed the narrow one for six slices."** That
is the same defect class as rulings 71, 64 and 68.

*(Corollary worth stating: the **semantic** class is unreachable while closing
**by construction**, because `apply_post_mortem` applies no frame at all, so no
semantic check runs. Only the structural class is live here. A ruling that scopes
§8.2 should say so for both classes anyway, or the next reader re-derives it.)*

---

**Option A — re-signal. §8.2 applies unconditionally; a structural violation
while closing emits a second CLOSE with `PROTOCOL_VIOLATION`.**

*Draft text:*

> **Ruling NNN — §8.2's structural class re-signals while closing.** A
> structural failure is a signalled death regardless of lifecycle state. A
> closing connection that receives one emits CLOSE with `PROTOCOL_VIOLATION`
> in place of its linger reply, under the ≤ 1/s cap, and the surfaced cause is
> **not** re-emitted (§15.2's `Closed`-once rule stands).

*What it costs, measured:*

- It contradicts nothing in §15.2 **only** if the second CLOSE travels under the
  existing reply cap — otherwise it is a new unbounded send path in the linger.
  Case C shows the cap is currently the sole gate on that packet.
- It requires deciding **which** CLOSE the *next* reply carries: the original
  (`0x17 "bye"` in my run) or the violation's (`0x01 ""`). `Closing` stores
  exactly one `Close` (`src/core/connection/close.rs:88-104`), so this is a real
  state change, not a code-path change.
- The `Closed` event has **already been delivered** to the application
  (case A: `LocallyClosed` at `t`). Slice 3's **Q3**
  (`.slices/03-skeleton/PLAN.md:1628`) fixes `Closed(_)` as emitted exactly once,
  so the re-signal is wire-only and invisible to the API — the peer learns of a
  violation the local application never hears about.
- It hands an authenticated peer a way to **overwrite our stated close reason**
  on the wire after we chose it.

*Code's current answer:* **not this.** Measured `code=0x17 reason="bye"`,
identical to the benign control.

---

**Option B — ignore. §8.2's structural class is scoped to a connection that is
not already closing or draining.**

*Draft text:*

> **Ruling NNN — §8.2's structural class is scoped to a live connection.** §8.2
> states the structural failure's consequence without bounding the state it
> applies in; the bound is **established, not closing and not draining**. A
> closing or draining connection parses the frame stream **only** to detect the
> peer's CLOSE (§15.2's retention list is exhaustive, so nothing else has state
> to be applied to); a structural failure there is discarded. It emits no second
> CLOSE, does not change the CLOSE the linger replies with, does not re-emit
> `Closed`, and does not move the `CloseLinger` deadline. The inbound packet is
> still authenticated and window-fresh, so §15.2's ordinary linger reply is owed
> for it exactly as for any other packet, under the same ≤ 1/s cap — the reply
> is a consequence of the packet arriving, never of its contents.

*Code's current answer:* **this, exactly**, at
`src/core/connection/mod.rs:1929` (`apply_post_mortem`), and the rationale is
already written in that function's doc comment. Ratifying B is a **zero-line
behaviour change**; it converts an undocumented assumption into a rule.

*Two sub-variants, and they differ only in observability:*

- **B1 — ignore entirely (what the code does today).** No trace fires while
  closing. Consequence: a peer that floods malformed frames for the whole 5 s
  linger produces **no `slither::frames` output at all**. It also makes U9
  (`.slices/03-skeleton/PLAN.md:1412`, *"§8.2's 'one trace fires': one per
  what?"*) true by construction — at most one structural trace per connection,
  ever.
- **B2 — ignore the signal, keep the trace.** §8.2's *"one trace fires"* survives
  into the closing state; only the CLOSE and the `ConnectionLost` are scoped out.
  Closes B1's observability hole. **Cost:** the trace is not rate-capped the way
  the reply is (case C: the *reply* is capped at 1/s, the parse is not), so a
  flooding peer produces one `warn!` per packet for 5 s. Only the genuine
  authenticated peer can do this, so it is bounded and not an off-path log
  amplifier — but it is a `warn!`-level flood from a connection already dying,
  and it re-opens U9's "one per what?".

**My recommendation: B1.** It is what the code does, what the slice-3 plan
provisionally decided, and what §15.2's retention list forces. B2's observability
gain is real but small — the operator already has the *first* structural trace if
the violation is what caused the close, and `LocallyClosed`/`ProtocolViolation`
already names the cause — while its cost re-opens a question U9 currently closes.
If the maintainer wants B2's visibility, the cheaper form is a **single**
`debug!` on the first discarded structural failure per linger, which keeps "one
trace fires" well-defined; that needs a latch in `Closing` and is a real code
change, so it should be ruled on deliberately rather than folded into a scoping
amendment.

---

### 1.3 Drafted discharge text for the stale entries

Seven of the eight are answered. **Six are outright stale** (1, 2, 3, 4, 7, 8);
**one (6) has had its substance discharged but its literal sentence is still
true**, and its replacement text says so rather than claiming staleness.
**Entry 5 stays** until the maintainer rules, with its citation corrected either
way.

One line each, to replace the item's closing sentence (the `///` doc bodies
above them stay as the record of *why* the question was asked — deleting them
loses the reasoning, which is working rule 4(b)'s point about a reversal
inheriting the duty to address the original argument):

| # | Item | Drafted discharge line |
|---|------|------------------------|
| 1 | `ESTABLISHED_HAS_NO_INSTANT` | **[DISCHARGED]** `Connection::established` now takes `now: Instant` first (`connection/mod.rs:354`) and pins both clocks through `install` → `Liveness::pinned_at_install` (`session.rs:290`). §7.4's install pin is stated for the accept path. |
| 2 | `LIVENESS_EXACT_INSTANT` | **[DISCHARGED — ruling 85, 2026/08/15]** §7.4's predicate became `>=` (`SPEC.md:2627`), which is §16.5's "no earlier than `D`". There is no contested instant; the exact deadline is now pinnable. |
| 3 | `CLOSE_RATE_CLOCK_ORIGIN` | **[DISCHARGED — ruling 83, 2026/08/15]** §15.2 now states *"the opening CLOSE is not a reply … the rate clock is unset at `close()`"* (`SPEC.md:5210`). Pinned at `close.rs:157` and `tests.rs:2692`. |
| 4 | `RETIRED_WITHOUT_A_SESSION` | **[DISCHARGED — ruling 84]** Ruling 81's "any teardown before a session exists" was withdrawn: the case is not constructible, `Closed` is emitted alone, and no `Retired` is owed (`connection/mod.rs:793-799`, `:2052`). |
| 5 | `VIOLATION_WHILE_CLOSING` | **stays** — see §1.2. Citation fix required either way: `PLAN.md` U4 → **`.slices/03-skeleton/PLAN.md` U4** (`PLAN.md` has no U-items; `PLAN-4b.md` has a different U4). |
| 6 | `SEAL_VERSUS_SEAL_QUIET` | **[DISCHARGED elsewhere — slice 4 delivered the separator]** The alias mutant now reddens 11 tests, incl. `tests_streams::sealing::a_reset_stream_only_packet_does_not_mark_last_send` and six in `tests_roam`. It still passes all 82 tests **in this file**, so the warning above remains accurate about *this file* and is no longer a gap in the crate. |
| 7 | `PACKING_ORDER` | **[DISCHARGED]** Slices 4–5 landed it: extends-to-end and control-before-fill in `tests_streams.rs:2421` (`mod packing`), PING-last in `tests_contested.rs:1579`, ACK-first in `tests_ack.rs:1643`. |
| 8 | `OVERSIZE_DATA_PACKET` | **[DISCHARGED — ruling 65]** §3.1's table has a `PKT_DATA` row (`30 ≤ len ≤ MAX_DATAGRAM`, `SPEC.md:682`) and §3.5 states the silent drop (`:778`). The drop is pre-AEAD (`packet/mod.rs:147`), so an *authenticated* oversize Data packet is unreachable. Two-sided pin at `packet/tests.rs:135`. |

**Two follow-on edits outside `mod owed`**, both carrying the same withdrawn
claims and both invisible to a grep for the item names (working rule 4 —
*grep for the rationale, not only the token*):

1. `src/core/connection/tests.rs:1162-1166` — the doc on
   `a_half_open_session_is_reaped_in_silence_and_not_one_nanosecond_early` still
   states the `>` vs "no earlier than" gap that ruling 85 closed. Replace with a
   note that ruling 85 settled it at `>=` (and, if the follow-up slice takes it,
   tighten the assertion to the exact instant).
2. `src/core/connection/tests.rs:2169-2171` — the doc on
   `the_linger_replies_at_most_once_per_second_under_a_flood` still says *"at two
   seconds both readings agree"*. Ruling 83 removed the second reading; the
   `t + 2 s` offset is now an inert hedge, not a correctness requirement.

Additionally, `src/core/connection/tests.rs:2584` carries the same
`PLAN.md` U4 miscitation as entry 5 and needs the same path fix.

**All of §1.3 is a draft. No source file was edited for it.**

## 2. Appendix B O-citations (O13 / O53a / O53b) — not drift, dangling pointers

### 2.1 Where the citations are

**Working rule 5 first: the brief's premise is not what I found, and the
difference matters to the ruling.**

The brief says *"Appendix B's bullets are unnumbered and have shifted (rulings
265/268 added bullets)"*, framing this as **numbering drift**. It is not drift.
The O-numbers were **never in `SPEC.md` at all**, and the document that defined
them is **no longer in the repository**.

**(i) All three citations are in the amendment table, and nowhere else in the
spec.**

```
$ grep -nE "\bO13\b|\bO53" SPEC.md
34:> | 260 | Appendix B | O53a discharged by measurement (spurious ≤ 1.55 %, envelope 2.5 %); O53b's quinn bar ruled untestable, the no-stall clause pinned in virtual time |
40:> | 268 | Appendix B | O13's flush parenthetical becomes admission-driven — the authenticate-then-drop flood it named mints no entries, by mitigation (i)'s own design |
42:> | 270 | §10.2 kinds, §10.5, §10.6, §17.5, Appendix B | … O53b's gate recorded run |
```

**(ii) Appendix B itself contains no O-number.** Appendix B runs from
`SPEC.md:7370` to `:7986`; scanning that whole range:

```
$ sed -n '7370,7990p' SPEC.md | grep -nE "\bO[0-9]+[a-z]?\b"
(no output)
```

Its structure is bold group headings (`**Wire pins.**`, `**Handshake and
routing.**`, …) over plain `-` bullets. There is no numbering to have shifted.

**(iii) The numbers are the *audit's* ids, and the audit says so in as many
words.** `.spec-v2-clean-slate/round41-H-audit-triage.md:23-27`:

> Note on the numbering: `O13`/`O43e`/`O53a`/`O53b` are the **audit's own**
> ids, defined in `audit/G-obligations-trend.md`. **They do not appear in
> `SPEC.md`**; the obligations themselves live in Appendix B (from
> `SPEC.md:7090`).

**(iv) `audit/G-obligations-trend.md` does not exist — anywhere.**

```
$ find . -name "G-obligations-trend.md" -not -path "./target/*"
(no output)
$ git ls-files | grep -i obligation
(no output)
$ git log --all --oneline --diff-filter=A -- "*G-obligations-trend.md"
(no output)
```

Not in the working tree, not tracked, and never added in any reachable commit.

**So the actual defect is this: `SPEC.md` — RATIFIED — cites three identifiers
from a working-directory audit artefact that was never committed and is now
gone.** The citations are unresolvable from the repository by anyone who was not
in round 41. That is a strictly worse failure than shifted numbering, and it is
working rule 11's generalised form: *a citation is a claim about the cited text*,
and here the cited text cannot be opened at all.

*(The brief's "shifted" instinct is right about one thing, just at a different
level: the triage note's own line references have drifted badly. It cites
Appendix B as starting at `SPEC.md:7090` and quotes O13 at `SPEC.md:7178-7183`;
at `b072afd` Appendix B starts at **7370**, so those line numbers are ~280 lines
stale and land inside §17/§18 instead. Line-number citations into `SPEC.md` decay
every round — which is itself an argument for option (b) below.)*

### 2.2 What each currently points at, positionally, vs what it meant

**The numbering was positional over Appendix B's top-level `- ` bullets, and it
was fixed at the ratification commit.** Measured across every commit that touches
`SPEC.md` (46 of them), reporting only the commits where a count changes:

```
commit     date         tot  pin  ack  thr  subject
2274981    2026-08-14    53   13   52   53  Make SPEC.md the ratified wire-v2 spec ...
6aa1d58    2026-08-16    59   13   58   59  Rulings 232-235: Agent C's blind return
c21869f    2026-08-17    64   13   63   64  Round 40: rulings 249-254 ...
8c29808    2026-08-18    65   13   64   65  Rulings 262 and 265 ...
```

(`tot` = top-level bullets in Appendix B; `pin`/`ack`/`thr` = the 1-indexed
position of the post-mortem-pin, ACK-loss-burst and throughput-sanity bullets.)

At **`2274981`, the ratification commit**, Appendix B had exactly **53** bullets
— which is why the highest cited number is 53. Today it has **65**.

**Resolution of each citation:**

| Citation | Meant (ratification bullet) | Current position | Current anchor | Status |
|---|---|---|---|---|
| **O13** | #13 — *"**The post-mortem pin** (§17.1, ruling 37)"* | **#13** | `SPEC.md:7458` | **Still resolves — by luck.** All 12 bullets added since were inserted *after* #13, so nothing shifted it. One insertion in the first twelve breaks it silently. |
| **O53a** | #52 — *"**The ACK-loss-burst simulation** (the §7.2/D-5 gate)"* | **#64** | `SPEC.md:7957` | **Broken.** Off by 12 (and off by one even at ratification — see below). |
| **O53b** | #53 — *"**The window-constants throughput sanity check** (the §10.2 and §10.6 gate)"* | **#65** | `SPEC.md:7969` | **Broken.** Off by 12. |

**What bullet #53 is today:** *"**`closed()` resolves on every death, with no verb
in flight** (§16.2, ...)"* (`SPEC.md:7824`) — an unrelated §16.2 shell obligation.
So a reader resolving "O53a" positionally at `b072afd` lands on the wrong
obligation and gets no signal that they have.

**One honest gap I cannot close.** At ratification the two post-implementation
bullets were **#52 and #53**, not "53a and 53b". Ruling 260's own text binds the
letters unambiguously to the obligations — *"O53a ran (`tests/spec_ack_burst.rs`)"*
for the ACK-loss-burst, *"O53b's 'within 20 % of quinn'"* for the throughput bar —
so **which obligation each letter names is certain**. What is *not* recoverable is
why the pair was numbered off a single `53`: whether the audit numbered the
two-bullet "**Post-implementation validation obligations**" *group* as one item, or
simply mis-indexed by one. `audit/G-obligations-trend.md` is the only document
that could settle it, and it does not exist. Recorded as unresolved rather than
guessed (working rule 11: a citation is a claim about a text, and this text cannot
be opened).

**Structural check, for the amendment's benefit:** the ratification bullets are
still in their original relative order today — the mapping from ratification index
to current index is strictly monotonic, verified over all 53. So a renumbering has
a well-defined answer; nothing was reordered, only inserted.

The 12 bullets added since ratification, in current order:

| today # | anchor text (opening words) | added by |
|---|---|---|
| 24 | *The reassembly work bound* (§10.6, ruling 253) | ruling 253 |
| 33 | *Keepalive-disarm and the parked-state backstop* (§7.5, ruling 265) | ruling 265 |
| 34 | *The backoff ladder and the survival envelope* (§13.3, ruling 254) | ruling 254 |
| 37 | *The epoch ratchet* (§7.7, ruling 251) | ruling 251 |
| 38 | *Straggler tolerance* (§7.7) | rulings 251/256 |
| 39 | *The `REKEY(0-32)` vector* (§7.7) | ruling 251 |
| 58 | *An adapter claims at most one item, and only inside `poll_next`* (ruling 58) | ruling 232 |
| 59 | *The `io::ErrorKind` table, row by row* (§16.11.1, ruling 227) | ruling 232 |
| 60 | *`poll_flush` is `Ready` with bytes unacknowledged* (ruling 56) | ruling 232 |
| 61 | *`poll_shutdown` is `finish()` and then `acked()`* (ruling 57) | ruling 232 |
| 62 | *The `Result`-carrying adapters never end* (ruling 226) | ruling 232 |
| 63 | *The empty-buffer and EOF conventions* (rulings 110, 119, 121) | ruling 232 |

*(The ACK-loss-burst bullet also fails a text-similarity match against its
ratification ancestor, because ruling 260 rewrote it wholesale from a gate into a
discharged measurement. It is the same obligation — same title, same §7.2/D-5
reference — not a new one.)*

### 2.3 Option (a): stamp stable O-numbers into Appendix B

**An amendment to Appendix B.** Numbers become part of the ratified text, are
assigned once, and are **never reused and never renumbered** — a new bullet takes
the next free number wherever it is inserted, so position and identity stop being
the same thing.

*Assignment, anchored to current text — this is the whole of it:*

1. The 53 bullets that existed at `2274981` keep their ratification index as
   **O1 ... O53**, in their current order (verified monotonic, §2.2). In
   particular:
   - **O13** = *"**The post-mortem pin** (§17.1, ruling 37)"* — `SPEC.md:7458`.
     Unchanged; ruling 268's citation keeps working.
   - **O52** = *"**The ACK-loss-burst simulation** (the §7.2/D-5 obligation)"* —
     `SPEC.md:7957`.
   - **O53** = *"**The window-constants throughput sanity check** (the §10.2 and
     §10.6 obligation)"* — `SPEC.md:7969`.
2. The 12 post-ratification bullets listed in §2.2 take **O54 ... O65** in their
   current order: today #24 to O54, #33 to O55, #34 to O56, #37 to O57, #38 to
   O58, #39 to O59, #58 to O60, #59 to O61, #60 to O62, #61 to O63, #62 to O64,
   #63 to O65.
3. **The `O53a`/`O53b` problem.** Under (1) the ACK-burst obligation becomes
   **O52**, which *invalidates every existing citation*: rulings 260, 268 and 270
   in `rulings.md`, three rows of `SPEC.md`'s own amendment table, and ~14
   references through `tests/spec_ack_burst.rs`. Two sub-choices:
   - **(a-i)** Accept the break: stamp O52/O53 and add a note to the amendment
     table — *"ruling 260's `O53a`/`O53b` are O52 and O53 under Appendix B's
     stamped numbering."* Cost: every historical citation needs that note to
     resolve.
   - **(a-ii)** Preserve the labels: stamp the two bullets literally as **`O53a`**
     and **`O53b`**, number the rest O1–O51 and O54–O65, leaving **O52
     unallocated**. Cost: one permanently unused number and one irregular pair.
     Benefit: **no existing citation breaks**, in the spec, the rulings record or
     the test suite. If (a) is chosen at all, this is the sub-choice to take.

### 2.4 Option (b): replace the three citations with section+quote references

**No amendment to Appendix B.** The three citations in `SPEC.md`'s amendment
table are rewritten to name each obligation by its **bold title**, which is
already unique and already in the ratified text, and Appendix B is never numbered.

*Concrete replacement text for the three rows:*

| line | current fragment | replacement |
|---|---|---|
| `SPEC.md:34` | `O53a discharged by measurement ...; O53b's quinn bar ...` | `Appendix B's **ACK-loss-burst simulation** discharged by measurement ...; the **window-constants throughput sanity check**'s quinn bar ruled untestable, its no-stall clause pinned in virtual time` |
| `SPEC.md:40` | `O13's flush parenthetical becomes admission-driven ...` | `Appendix B's **post-mortem pin** obligation — its flush parenthetical becomes admission-driven ...` |
| `SPEC.md:42` | `... O53b's gate recorded run` | `... the **window-constants throughput sanity check**'s recorded run` |

The bold titles are unique within Appendix B (verified), so each resolves by a
single grep for the title text, with no ambiguity and no position dependence.

**Note the precedent already in the file.** Appendix B does **not** number its
bullets, but it *does* carry stable inline identifiers on selected bullets
already — the **`SECV5-N`** tags, written into the bullet text itself:

```
7472:- **No-record-on-`Stale`** (§6.4's ordering clause, SECV5-6): ...
7477:- **The dialled-only static** (§17.1's honesty clause, SECV5-5): ...
7648:- **The clock is armed at install** (§7.4, SECV5-2): ...
```

These are position-independent, survive insertion, and are already cited from
code (`src/core/connection/session.rs:268`). So the file's own established
convention for "this obligation needs a durable name" is **an inline tag on the
bullets that need one**, not a numbering of all of them.

### 2.5 Recommendation

**Option (b), with one addition borrowed from (a).**

*Why (b) over (a):*

1. **The numbers were never the spec's.** They were a working audit's private
   ids, and the audit document said so itself. Stamping them now ratifies a
   foreign artefact's indexing scheme into a frozen document, inverting the
   direction of authority this project maintains everywhere else.
2. **(a) makes position and identity permanently diverge.** After a few more
   insertions Appendix B reads `... O21, O22, O57, O23 ...`, and every future
   reader must be told the numbers are not an order. That is a standing tax on a
   *non-normative* appendix.
3. **(a) forces a bad choice at (3) above** — break ~20 live citations across
   three artefacts, or ratify a permanent hole at O52 plus an irregular
   `53a`/`53b` pair. Both are worse than the problem being solved.
4. **The bold titles are already unique, already ratified, and already stable**
   through every amendment so far. Ruling 260 rewrote the ACK-burst bullet's body
   completely and left its title intact — exactly the robustness a citation needs,
   and precisely what the positional number failed to provide.
5. **Line numbers decay and titles do not.** Demonstrated inside this report: the
   triage document's `SPEC.md:7090` and `:7178-7183` references are ~280 lines
   stale after one round, while *"The post-mortem pin"* still finds its bullet on
   the first grep.

*The addition from (a):* fix the amendment-table rows **and** record the
provenance, because the record is currently unresolvable by anyone who was not in
round 41. One line, in the table's preamble or in the ruling that settles this:

> Appendix B's bullets are unnumbered and are cited by their bold title. The
> `O13`/`O53a`/`O53b` ids appearing in rulings 260, 268 and 270 were the round-41
> audit's private numbering (`audit/G-obligations-trend.md`, never committed and
> no longer extant), positional over Appendix B as of the ratification commit
> `2274981`, when it held 53 bullets. They resolve to the **post-mortem pin**, the
> **ACK-loss-burst simulation**, and the **window-constants throughput sanity
> check** respectively.

That is the piece neither option supplies on its own, and the one that stops the
next reader repeating this investigation. Rulings 260/268/270 and
`tests/spec_ack_burst.rs` then keep their `O53a`/`O53b` text unchanged — they
become historical labels with a documented resolution rather than dangling
pointers.

**Cost of (b): three table-row edits plus one provenance note. No Appendix B
change, no test change, no ruling rewrite.**

## 3. SECV5-5 / SECV5-6 / SECV5-8 traceability

**Tag inventory, verified.** Every `SECV5` occurrence in the crate:

```
$ grep -n "SECV5" SPEC.md
2735:  half-open session SECV5-2 was applied to prevent. When two statements
7472:- **No-record-on-`Stale`** (§6.4's ordering clause, SECV5-6): an
7477:- **The dialled-only static** (§17.1's honesty clause, SECV5-5): against
7643:  interval ends the connection at 25 s (SECV5-8 — drop both directions'
7648:- **The clock is armed at install** (§7.4, SECV5-2): install a session

$ grep -rn "SECV5" src/ tests/
src/core/connection/session.rs:268:    /// no death (`armed` is false): SECV5-2's **immortal half-open
src/core/connection/tests_ack_cadence.rs:724:/// half-open session alive forever — SECV5-2's immortal session.
```

So **SECV5-2 is traced twice and SECV5-5/6/8 are traced nowhere** — which is
the gap. Two brief corrections, neither material:

- The brief gives SECV5-8 at `:7639`; the tag is at **`:7643`**. It is not its
  own bullet — it is a parenthetical inside the **"The liveness anchor" (§7.4)**
  bullet, which starts at **`SPEC.md:7625`**. That matters for the fix: SECV5-8
  is a *clause*, and the bullet around it is discharged elsewhere, so a single
  "this bullet is covered" comment would over-claim.
- The existing SECV5-2 traces are **prose mentions inside doc comments**, not a
  formal marker syntax. The drafts below follow that house style rather than
  inventing one.

---

### 3.1 SECV5-5 — the dialled-only static (`SPEC.md:7477-7482`)

**Obligation, quoted in full:**

> - **The dialled-only static** (§17.1's honesty clause, SECV5-5): against a peer
>   we only ever `connect()`ed to, the guard holds **no** entry, so an
>   arbitrarily old captured initiation passes it vacuously and surfaces as an
>   `Intro` **repeatably** — assert it surfaces more than once from a single
>   captured packet, and that the live connection is untouched every time because
>   the basis is `None`.

**Substance FOUND — three clauses of four.** Searched by mechanism (`dialled`,
`basis`, `greatest`, `AcceptError::Stale`, `Contested`), not by tag.

| clause | discharge site | verdict |
|---|---|---|
| the guard holds **no** entry for a dialled-only static | `src/core/tests.rs:2195` `a_dialled_static_holds_no_guard_entry` | **pinned.** Uses two frozen clocks with the dialler 10 000 s *ahead*, so a build that recorded its own outbound timestamp refuses the peer's genuine older msg1 — a real separator, not a vacuous one |
| an arbitrarily old captured initiation passes vacuously and surfaces as an `Intro` | `src/core/endpoint/routing.rs:1379` `a_dialled_live_rows_stale_reverts_its_record_and_marks_contested` — `let captured = lone_msg1(...)` taken **before** the dial, then fed at `:1410` | **pinned** |
| the live connection is untouched, because the basis is `None` | same test (`:1418-1421`, `accept` → `Err(AcceptError::Stale)`), plus `src/core/tests.rs:3258` `connect_records_a_none_basis`, which separates `Some(None)` from `None` explicitly | **pinned** |
| **surfaces `Intro` *repeatably* — "more than once from a single captured packet"** | — | **NOT pinned** |

**The gap is exactly working rule 9's shape.** `routing.rs:1379` feeds `captured`
**once**. A build that consumed the captured packet on first use — one that let
the refused chain's teardown poison a replay of the same bytes — passes every
assertion in that test. The obligation says *"assert it surfaces more than
once"* precisely because surfacing once does not separate the builds. The
mechanism is in fact correct (the refusal calls `discard_chain`, which reverts
the provisional record, so the second replay passes the guard vacuously again —
`src/core/endpoint/staged.rs:673-676`, `:690-694`), so this is a **missing
assertion over working code**, not a defect. Cheap to close: a second
`large.feed(t, addr(8, 4011), &captured)` in the same body, asserting a second
`Intro` and a second `Stale` with the connection still alive.

*(Cross-check against the nearest candidate, so the gap is not merely unfound:
`tests/story_keepalive.rs:1218`
`s11_a_second_refusal_while_contested_neither_re_notifies_nor_re_arms` does
produce two refusals, but its own comment says* "Two independent restarts of B's
static: each parks its own admitted candidate" *— two distinct packets, not one
replayed. It does not discharge this clause.)*

### 3.2 SECV5-6 — no-record-on-`Stale` (`SPEC.md:7472-7476`)

**Obligation, quoted in full:**

> - **No-record-on-`Stale`** (§6.4's ordering clause, SECV5-6): an `accept()`
>   refused by the basis rule leaves the guard byte-identical to its pre-call
>   contents, so a later genuine initiation with a timestamp between the two is
>   still admitted; the tie-break-winner `Stale` is the single exception and
>   **does** leave its record.

**Substance FOUND.** The mechanism is `discard_chain` on every refusing arm and
`keep_winner_side_record` on exactly one — `src/core/endpoint/staged.rs:673-676`
(basis `Some(_)`, not strictly newer), `:690-694` (basis `None`), against
`:698-701` (PENDING winner) and its implementation at `:896`.

| clause | discharge site | verdict |
|---|---|---|
| the tie-break-winner `Stale` **keeps** its record (the single exception) | `src/core/endpoint/tests.rs:2017` `the_pending_branch_winner_returns_stale_and_is_the_one_stale_that_keeps_its_record`, part (a) | **pinned**, and its doc names the mutation: *"applying §6.4's ordering clause uniformly and reverting here too"* |
| every other refusal leaves the guard byte-identical | same test, part (b) — the deliberate contrast in one body, *"a core that never reverts anything would satisfy the first half alone, so the pin is the two halves disagreeing"* | **pinned** |
| an `accept()` **refused by the basis rule** reverts | `src/core/endpoint/routing.rs:1379` — asserts `greatest()` is `Some` after `authenticate()` and `None` after the basis-refused `accept()` | **pinned** |
| a later genuine initiation with a timestamp **between** the two is still admitted | `src/core/tests.rs:1950` `authenticate_then_reject_restores_a_prior_value` | **pinned in substance, on a neighbouring path** — see below |

**One honest qualification.** The "timestamp between the two is still admitted"
clause is pinned only via `reject()`, not via a basis-refused `accept()`. The
two reach `discard_chain` by different arms
(`reject` → `staged.rs` teardown; basis refusal → `staged.rs:674`/`:692`), so a
build that reverted correctly on one and not the other is not fully separated by
the existing suite. `routing.rs:1379` covers the accept-arm revert but asserts
only `greatest() == None`, i.e. the *empty* case, not the *restored-to-a-prior-
value* case. This is a narrower gap than SECV5-5's — the arms share
`discard_chain` — and I flag it rather than assert it is covered (working rule
12: a true lemma about a neighbouring path proves nothing about this one).

### 3.3 SECV5-8 — simultaneous bidirectional keepalive loss (`SPEC.md:7643`)

**Obligation, quoted in context.** SECV5-8 is a parenthetical inside the
**"The liveness anchor" (§7.4)** bullet, which begins at `SPEC.md:7625`:

> … an idle keepalive-sustained connection lives indefinitely **while the dance
> survives the path** — at 2 × `KEEPALIVE_TIMEOUT` + 5 s one keepalive lost **in
> one direction** is tolerated, while a *simultaneous bidirectional* loss over a
> single interval ends the connection at 25 s (**SECV5-8 — drop both directions'
> keepalive in the same interval and assert both sides fire `TimedOut`**, since
> neither can re-fire the one-shot passive rule and keepalives are never
> retransmitted), with no handshake ever re-run and no built-in reconnect
> (§5.4, §7.5).

**Substance PARTIALLY found. The obligation as written is NOT discharged — this
is a conflict, reported not resolved (working rule 3).**

| clause | discharge site | verdict |
|---|---|---|
| the passive rule is **one-shot**: it fires once and disarms, and cannot re-fire without a further receive | `src/core/connection/tests_roam.rs:212` `the_passive_keepalive_sends_the_empty_plaintext_and_then_disarms` — after the keepalive, `timer(TimerKind::Keepalive)` is `None`, *"S has moved to `now`, so R > S is false again"* | **pinned**, at the core |
| the passive rule arms only on `R > S` | `src/core/connection/tests_roam.rs:192` `the_passive_keepalive_arms_only_after_a_receive` | **pinned** |
| **drop both directions' keepalive in the same interval; assert both sides fire `TimedOut`** | — | **NOT FOUND** |

**What I searched, so the absence is evidence and not just a failure to look.**
`grep` over `tests/` for `block_path`, `heal_path`, `partition`, `heal`,
`TimedOut`, `one-shot`, `never retransmitted`, and a read of every test name in
`tests/story_keepalive.rs`. Results:

- **`tests/story_keepalive.rs` has no loss injection at all.** Its own header
  (`:95-97`) says so: *"`FlakyPolicy::lossy` is invisible to every public counter
  …, so no test here uses it. Loss is `block_path` or a dropped endpoint."* It
  then uses neither.
- **The only bidirectional blocks in the suite are `tests/story_compat.rs:464-465`
  and `:629-630`** (`s31_shutdown_resolves_in_error_when_the_connection_dies_first`,
  `s31_flush_is_a_no_op_and_never_delivery_confirmation`). Both block the path
  **permanently** and hold the peer as `let _keep_peer = cb;` without asserting
  on it.

**Why the S31 tests do not discharge it, stated as the mutant they fail to
kill.** A permanent bidirectional blackhole kills the connection in *any* build,
including one whose passive keepalive re-fires every interval — with no path,
even an immortally-armed keepalive reaches nobody. SECV5-8's entire content is
that **one interval** of bidirectional loss is fatal, and that is a claim about
the *one-shot* property, observable only if the path is healed afterwards. The
degenerate build satisfies the existing tests for free, which is working rule 9's
definition of a bound that is not a test.

`Network::heal_path` exists (`src/testutil/mod.rs:539`) and is used by nine tests,
so the fixture can express this — unlike working rule 13's cases, this is **not**
a harness gap. It is an unwritten test.

**Recommended shape** (for whoever writes it, not written here — this is a
report-only task): establish, exchange one datagram so the passive dance is
running, `block_path` both directions, advance one `KEEPALIVE_TIMEOUT` so each
side's one-shot keepalive fires into the blackhole and disarms, `heal_path` both
directions, then assert **both** `ca` and `cb` resolve `ConnectionLost::TimedOut`
at install-relative `DEAD_TIMEOUT`. The heal is what separates it from the S31
tests, and the two-sided assertion is what separates it from a build that reaps
only the dialler.

### 3.4 Drafted comment lines

Following the house style already set by the two SECV5-2 traces — the tag named
in prose inside the doc comment at the discharge site, not a bespoke marker.

**SECV5-5** — add to the doc comment of
`a_dialled_live_rows_stale_reverts_its_record_and_marks_contested`, at
`src/core/endpoint/routing.rs:1378` (immediately above `#[test]`):

```rust
/// **Appendix B's dialled-only static, SECV5-5** (`SPEC.md:7477`): the
/// captured initiation passes the guard vacuously and the `None` basis
/// leaves the live connection untouched. The obligation's *"surfaces
/// **repeatably** — more than once from a single captured packet"* clause
/// is **not** asserted here: `captured` is fed once.
```

Paired with a supporting line on the guard half, at `src/core/tests.rs:2194`
(above `#[test] fn a_dialled_static_holds_no_guard_entry`):

```rust
/// Appendix B's **dialled-only static** (SECV5-5, `SPEC.md:7477`): this is
/// its *"the guard holds **no** entry"* half; the refusal half is
/// `endpoint::routing`'s `a_dialled_live_rows_stale_reverts_its_record_and_marks_contested`.
```

**SECV5-6** — add to the doc comment of
`the_pending_branch_winner_returns_stale_and_is_the_one_stale_that_keeps_its_record`,
at `src/core/endpoint/tests.rs:2016` (immediately above `#[test]`):

```rust
/// **Appendix B's no-record-on-`Stale`, SECV5-6** (`SPEC.md:7472`): both
/// halves — the winner's exception in (a), the ordinary revert in (b). The
/// basis-refused `accept()` arm is covered by `endpoint::routing`'s
/// `a_dialled_live_rows_stale_reverts_its_record_and_marks_contested`; the
/// *"a timestamp **between** the two is still admitted"* clause is asserted
/// only on the `reject()` path, at `core::tests`'
/// `authenticate_then_reject_restores_a_prior_value`.
```

**SECV5-8** — **no discharge site exists, so there is no comment to add.** The
honest artefact is a placeholder recording the gap. If the maintainer wants it
tracked in code rather than only here, the natural home is the existing `owed`
registry pattern, as a new item in `tests/story_keepalive.rs` (which is where the
test belongs):

```rust
/// **Appendix B SECV5-8 is NOT discharged** (`SPEC.md:7643`, inside the
/// "liveness anchor" bullet at `:7625`).
///
/// *"Drop both directions' keepalive in the same interval and assert both
/// sides fire `TimedOut`."* The one-shot property it rests on is pinned at
/// the core (`core::connection::tests_roam::the_passive_keepalive_sends_the_empty_plaintext_and_then_disarms`),
/// but no flow test drops both directions for **one interval** and heals.
/// The two permanent bidirectional blocks in `story_compat` (S31) do not
/// separate it: with the path never healed, a build whose passive keepalive
/// re-fires every interval dies too. `Network::heal_path` exists, so this is
/// an unwritten test, not a fixture gap.
pub const SECV5_8_BIDIRECTIONAL_KEEPALIVE_LOSS: () = ();
```

## 4. Conflicts found (rule 3)

Reported, not resolved. Ordered by what a maintainer has to decide.

**C1 — §8.2 states the structural-failure consequence with no scope clause, and
the code has assumed the narrow scope for six slices.** `SPEC.md:3385-3394` makes
a structural failure a signalled death — CLOSE with `PROTOCOL_VIOLATION`, plus
`ConnectionLost::ProtocolViolation` — and never says "on an established
connection". `apply_post_mortem` (`src/core/connection/mod.rs:1929`) ignores it
while closing or draining, which I **measured** (§1.2). On the literal text the
code is wrong; on the retention list in §15.2 the code is right. Working rule 8's
exact shape. Two drafted options in §1.2; my recommendation is **B1** (scope §8.2
to a live connection — a zero-line behaviour change that ratifies what is already
there). *Not resolved here.*

**C2 — `SPEC.md`, RATIFIED, cites three identifiers from a document that does not
exist.** `O13`/`O53a`/`O53b` at `SPEC.md:34`, `:40`, `:42` were the round-41
audit's private ids from `audit/G-obligations-trend.md` — never committed, absent
from the working tree and from all reachable history. Appendix B contains no
O-number anywhere. Two drafted options in §2; recommendation **(b)** plus a
provenance note. *Not resolved here.*

**C3 — SECV5-8's obligation is not discharged, and the tests that look like they
discharge it do not.** `SPEC.md:7643` requires *"drop both directions' keepalive
in the same interval and assert both sides fire `TimedOut`"*. No test heals the
path, so the one-shot property the obligation exists to pin is never separated;
the two permanent bidirectional blocks in `tests/story_compat.rs` pass on a build
whose keepalive re-fires every interval. `Network::heal_path` exists, so this is
an unwritten test, not a fixture gap. Full argument in §3.3.

**C4 — a dangling citation, quoted twice in shipped source.** `PLAN.md` U4 does
not exist; `PLAN.md` has no U-numbered items at all. The real question is
`.slices/03-skeleton/PLAN.md:1325`, and the short form is ambiguous anyway
because `.slices/04-streams/PLAN-4b.md:918` defines a *different* U4. Cited at
`src/core/connection/tests.rs:3117` and `:2584`. Fix in §1.3 regardless of how
C1 is ruled.

**C5 — two doc comments still assert claims that rulings 83 and 85 withdrew.**
`src/core/connection/tests.rs:1162-1166` states §7.4's death predicate as `>`
(ruling 85 made it `>=` on 2026/08/15); `:2169-2171` states that whether the
opening CLOSE starts the reply clock is open (ruling 83 ratified that it does not,
same day). Both are invisible to a grep for the `owed` item names — working rule
4's *"grep for the rationale, not only the token"*. Listed in §1.3.

**C6 — two Appendix B obligations are pinned in substance but not in the shape
the obligation names.** SECV5-5's *"surfaces **repeatably** — more than once from
a single captured packet"* is never asserted (`routing.rs:1379` feeds the packet
once), and SECV5-6's *"a timestamp **between** the two is still admitted"* is
asserted only on the `reject()` path, not on the basis-refused `accept()` path.
Both are missing assertions over working code, not defects. §3.1, §3.2.

---

### Where I departed from the brief, and why (working rule 5)

**(a) The brief's count.** It says *"the 7 believed stale"*. I found **six**
outright stale (entries 1, 2, 3, 4, 7, 8). Entry **6** (`SEAL_VERSUS_SEAL_QUIET`)
is a seventh whose *substance* is discharged but whose **literal sentence remains
true**: I ran the alias mutant and 11 tests went red, none of them in the file
the item is written in, where all 82 still pass (§1.1). Writing it up as "stale"
would have put a false statement in the registry in the act of cleaning the
registry. Its drafted discharge line says what is actually true.

**(b) The brief's framing of item 2.** It describes *"Appendix B O-numbering
drift … bullets are unnumbered and have shifted"*. The bullets were never
numbered, in any revision; the numbers are a vanished audit document's. I report
that rather than answering the question as posed, because the two framings lead
to different rulings — "drift" suggests option (a) (restore the numbering),
whereas the actual provenance is the strongest argument *against* it (§2.5).

**(c) Working rule 9 and the mutants.** This task commits no test, so there is no
committed test to mutate. I ran mutants anyway, as *evidence for the staleness
claims* — that is what makes them measurements rather than readings:
- `seal_quiet` → marking (`session.rs:483`): **11 red**, pasted in §1.1, reverted.
- The `VIOLATION_WHILE_CLOSING` scratch harness carries its own separating
  control **inside** it — case **B**, the benign packet. Without B, case A's
  transmit reads as a re-signal; with B, the two are identical and the violation
  is proved to contribute nothing. Case C (the ≤ 1/s cap) is the second
  separator. Reverted.

## 5. Gates (rule 7)

Run on the final tree — the only change is `GAPSLICE-D-REPORT.md`; both mutants
and the scratch test were reverted with `git checkout --` before these ran.

```
$ git status --short
?? GAPSLICE-D-REPORT.md
```

**Format** — `cargo fmt --all` then the check:

```
$ cargo fmt --all --check
(no output, exit 0)
```

**Lints:**

```
$ cargo clippy --all-features --all-targets -- -D warnings
    Checking slither v0.2.0 (/Users/.../worktrees/wf_a5f79863-6bc-4)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.41s
(zero warnings)
```

**Tests:**

```
$ cargo test --all-features
test result: ok. 777 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.67s
test result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.44s
test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s
test result: ok. 112 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
...
$ cargo test --all-features 2>&1 | grep -cE "^test result: ok"
26
$ cargo test --all-features 2>&1 | grep -E "^test result" | awk -F'[ ;]' '{p+=$4} END {print p}'
1116
```

**26 test binaries, all `ok`; 1116 passed, 0 failed, 1 ignored.** Identical to the
suite at `b072afd` — this commit adds no test and changes no source file.

*(The two lines matching `failed` in the raw output are test **names** —
`a_failed_decryption_never_burns_its_counter` and
`a_failed_completion_spends_the_attempt_and_the_next_retransmit_refreshes_it` —
both reported `... ok`.)*

**Gates not run**, because this commit touches no Rust source and no manifest:
docs, release tests, MSRV, `cargo deny`. The wire pins run under `cargo test` and
are inside the 1116 above.

---

## 6. What a maintainer has to decide

| # | Decision | My recommendation | Cost if taken |
|---|---|---|---|
| 1 | §8.2's structural class while closing — re-signal or ignore (**C1**) | **Ignore, variant B1** — scope §8.2 to a live connection | Spec text only; zero code change |
| 2 | Discharge the seven answered `owed` entries (§1.3) | Take all seven, with entry 6's wording as drafted | 7 one-line edits + 2 stale doc comments + 3 citation fixes |
| 3 | Appendix B's O-citations (**C2**) | **Option (b)** — cite by bold title, plus a provenance note | 3 table rows + 1 note; no Appendix B change |
| 4 | SECV5-5/6 traceability comments (§3.4) | Add as drafted | 3 doc-comment insertions |
| 5 | SECV5-8's missing test (**C3**) | Write it; shape given in §3.3 | One new flow test in `tests/story_keepalive.rs` |
| 6 | SECV5-5's repeatability and SECV5-6's between-timestamp assertions (**C6**) | Close both; cheap | ~2 assertions each, in existing test bodies |

**Nothing in this report was applied.** The only file this commit adds is the
report itself; `git status` above is the evidence.
