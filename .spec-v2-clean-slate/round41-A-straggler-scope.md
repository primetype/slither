# Round 41 item 2 — §7.7 straggler tolerance: unstated scope

Verification agent A. Read-only over the repo; all writes under scratchpad/round41/.

**Claim under test** (from `.spec-v2-clean-slate/round41-material.md` item 2):
§7.7's "retains the current and immediately preceding epoch keys (straggler
tolerance: one epoch back)" has an unstated scope — the previous-epoch key is
reachable only within `REPLAY_WINDOW` (2048) counters of an epoch boundary,
i.e. 32× tighter than the sentence reads at `REKEY_EPOCH_MSGS` = 65 536,
because same-epoch packets more than `REPLAY_WINDOW` counters behind the
highest-seen counter are refused by §7.2's replay window, not by the ratchet.

---

## 1. The spec text

### §7.7 — the straggler sentence (SPEC.md:3187–3189)

```
3187	The receiver retains the current and immediately preceding epoch keys
3188	(straggler tolerance: one epoch back); anything older is refused, its key
3189	ratcheted away.
```

Surrounding constants (SPEC.md:3169–3172): `REKEY_EPOCH_MSGS` = 65 536 (2^16)
messages per epoch; `MAX_EPOCH_JUMP` = 2 (hiss-fixed). Epoch membership
(SPEC.md:3176): "A message sealed at `counter` belongs to epoch
`counter / REKEY_EPOCH_MSGS`". §7.7 says nothing anywhere in its 66 lines
about the replay window, `REPLAY_WINDOW`, or a counter-distance bound on the
straggler.

### §7.2 — the rule it interacts with (SPEC.md:2150–2156)

```
2150	- The window tracks a greatest authenticated counter plus the 2048-bit
2151	  bitmap. The replay check is strictly **post-AEAD**: check-then-mark only
2152	  after `decrypt_at` authenticates. A duplicate, or a counter more than
2153	  2048 behind the greatest, is dropped after decryption **without
2154	  delivery**. The window's greatest advances only on authenticated
2155	  counters; hiss's `MAX_EPOCH_JUMP` and commit-and-cap bound forged-counter
2156	  cost upstream of it (§2.1).
```

Note two things that matter for the whole item:
* the drop is **after decryption** — so "opens" and "is accepted" are two
  different events, and the claim's phrase "refused by §7.2's replay window,
  not by the ratchet" is about the *second*;
* §7.2's own sizing bullet already reasons across the two constants
  (SPEC.md:2166–2169): "2048 … is ~20 ms of reordering memory at 1 Gbps line
  rate and ~200 ms at 100 Mbps, **comfortably inside one ratchet epoch
  (65 536)**".

### §2.1 hiss-facts table — the scope IS stated, but not in §7.7 (SPEC.md:542)

```
542	| Straggler tolerance is exactly one epoch back | the reordering budget (replay window, 2048) sits far inside one epoch (65 536) (§7.2) |
```

This row makes exactly the claim's arithmetic argument, one table away from
§7.7 and in the opposite rhetorical direction: it offers 2048 ≪ 65 536 as the
*reason* one-epoch tolerance is sufficient. So the interaction is not absent
from SPEC.md; what is absent is any statement **inside §7.7** that the
one-epoch sentence therefore over-states the reachable range. See §7 VERDICT:
this moves "unstated" to PARTIAL.

### Appendix B obligation (SPEC.md:7293–7298)

```
7293	- **Straggler tolerance** (§7.7): a packet from the immediately preceding
7294	  epoch opens after the receiver commits to the new one; a packet from
7295	  **two** epochs back is refused without key derivation. The refusal is
7296	  the separating assertion — a build that never rekeys opens the e−2
7297	  straggler happily, where boundary-invisibility alone is satisfied for
7298	  free by the build in which nothing ever happens.
```

The obligation is written entirely in the verb **"opens"** — an AEAD-surface
predicate — and never in terms of delivery or window acceptance. That wording
is *correct* for what it pins and is exactly why the obligation can be
satisfied without ever touching the scope question.

## 2. The code

### 2a. The order on the receive path — AEAD first, window second

`Session::open` (`src/core/connection/session.rs:569–626`) is the whole of it:

```
577	        let plaintext_len = ciphertext.len().checked_sub(constants::AEAD_TAG_LEN)?;
...
588	        // AEAD first. On failure `scratch` holds unauthenticated bytes,
589	        // which is why nothing below reads it on that path.
590	        let written = C::open(
591	            &mut self.established.open,
592	            counter, ad, ciphertext, scratch.as_mut_slice(),
593	        ).ok()?;                                      // ← line 597 is the `.ok()?`
599	        // Only now: the window check, and the mark.
600	        if !self.replay.check_and_mark(counter) {
611	            tracing::debug!(target: "slither::replay", counter,
614	                greatest = ?self.replay.greatest(), ...);
617	            return None;
618	        }
622	        self.liveness.on_authenticated_fresh_recv(now);
```

**Answer to (a): the previous-epoch key is exercised BEFORE the window is
consulted.** An old-epoch packet outside the window is decrypted — successfully,
under the retained previous-epoch key — and *then* dropped by the window. The
ratchet never gets the chance to refuse it, because it is not old enough for the
ratchet to refuse.

This is the only `C::open` call site in the crate
(`grep -rn 'C::open' src` → `src/core/connection/session.rs:590` alone), and the
only `check_and_mark` call site outside tests
(`src/core/connection/session.rs:600`). `handle_datagram`
(`src/core/connection/mod.rs:534–577`) returns on `open() == None` at line 576
and cannot distinguish the two causes; its doc at
`src/core/connection/mod.rs:526–528` already enumerates the two conjuncts as
separate ("`session.open(..)` returned `Some` — the **AEAD tag verified**" /
"the replay window **marked** it").

### 2b. Who owns the 2048-counter window: slither

`ReplayWindow` — `src/core/connection/session.rs:74–209`. `REPLAY_WINDOW` is
declared at `src/constants.rs:159` and used only there
(`src/core/connection/session.rs:44,112,139,173,181,190,223`).

The refusal condition is `src/core/connection/session.rs:137–149`:

```
137	            Some(greatest) => {
138	                let offset = greatest - counter;
139	                if offset == 0 || offset > constants::REPLAY_WINDOW as u64 {
140	                    return false;
141	                }
```

Note the deliberate off-by-one, documented at
`src/core/connection/session.rs:52–64`: the greatest lives in its own field, so
the bitmap's 2048 bits cover offsets **1..=2048** and *"`greatest − 2048` is
accepted (it is exactly 2048 behind, not *more than*), and `greatest − 2049` is
dropped."* Pinned two-sided by
`the_window_edge_is_two_sided` (`src/core/connection/session.rs:670–686`). This
choice is load-bearing for the arithmetic in §3 below.

### 2c. Who owns key retention: hiss, not slither

slither's `Handshake::open` is a pass-through to `DatagramRecv::decrypt_at`
(`src/packet/suite.rs:427–435`), and the trait doc already states the split
(`src/packet/handshake.rs:186–192`): *"Takes `&mut` because the §7.7 epoch
ratchet commits on success. **No replay rejection happens here** … §7.2's window
is the caller's duty and is strictly post-AEAD."*

hiss 0.3.2, resolved from `Cargo.toml:101,199`, vendored at
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/hiss-0.3.2`:

* `RecvRatchet` — `src/noise/datagram.rs:295–305`: fields `epoch_size`,
  `current_epoch`, `current_key`, `prev_key: Option<[u8;32]>`
  ("The key for `current_epoch − 1`, or `None` while `current_epoch == 0`").
* The **previous-epoch branch** — `src/noise/datagram.rs:350–360`:

```
353	        if msg_epoch < self.current_epoch {
354	            if msg_epoch + 1 == self.current_epoch
355	                && let Some(prev) = self.prev_key.as_ref()
356	            {
357	                return Ci::decrypt(prev, counter, ad, ciphertext, output);
358	            }
359	            return Err(HandshakeError::DecryptionFailed);
360	        }
```

  **Answer to (b): the previous-epoch key is reachable iff
  `msg_epoch + 1 == current_epoch` — a pure epoch predicate with no
  counter-distance term and no state change on this path.** Every packet of the
  whole preceding epoch reaches `Ci::decrypt` under `prev`, however far back.
* `current_epoch` advances **only** on a *future*-epoch packet whose tag
  verifies — commit-and-cap, `src/noise/datagram.rs:372–408`, with
  `steps > MAX_EPOCH_JUMP` refused before any key derivation
  (`src/noise/datagram.rs:373–375`). On success `prev_key` becomes
  `key(msg_epoch − 1)` and everything older is zeroized
  (`src/noise/datagram.rs:391–401`).

So the split is: **hiss decides whether a packet *opens*; slither decides whether
it is *delivered*.** The straggler sentence in §7.7 is about the first; the
32× bound the claim asserts lives entirely in the second.

Epoch size: `Config::DEFAULT_EPOCH_SIZE = REKEY_EPOCH_MSGS` (`src/config.rs:90`),
overridable test-only by `with_epoch_size` (`src/config.rs:167–185`, ruling 82).

## 3. Arithmetic and mechanism — derived, then measured

### 3a. The invariant that couples the two mechanisms

Let `E` = epoch size, `W` = `REPLAY_WINDOW` = 2048, `g` = `ReplayWindow::greatest`,
`e = g / E`, `r = g mod E` (how far past the boundary the receiver has advanced).

**`current_epoch == g / E` at all times** (given `g` is `Some`). Both are the
maximum over the *same* set — the packets whose AEAD tag verified:

* `g` advances on every successful open: `check_and_mark` is called
  unconditionally after `C::open` returns `Ok`
  (`src/core/connection/session.rs:597,600`), and its `counter > greatest` arm
  assigns unconditionally (`src/core/connection/session.rs:128–136`);
* `current_epoch` advances only on a *future*-epoch packet that verifies
  (`~/.cargo/.../hiss-0.3.2/src/noise/datagram.rs:390–398`), which is exactly
  the subset of those opens that raise the maximum counter past a boundary;
* epoch is monotone in counter, so `max epoch = epoch(max counter)`.

### 3b. The condition for a previous-epoch packet to be *delivered*

A packet at counter `c` with `c / E == e − 1`:

1. **opens** iff `msg_epoch + 1 == current_epoch` and `prev_key` is present —
   `hiss-0.3.2/src/noise/datagram.rs:353–358`. *No counter-distance term.*
   Every counter of the whole preceding epoch opens.
2. **is delivered** iff additionally `g − c ≤ W` and its bit is unset —
   `src/core/connection/session.rs:137–149`.

The newest counter in epoch `e − 1` is `eE − 1 = g − r − 1`, i.e. **`r + 1`**
behind the greatest. So *something* from the previous epoch is still deliverable
iff `r + 1 ≤ W`, i.e.

> **`r ≤ 2047`** — the previous-epoch key can deliver nothing once the
> receiver's greatest has advanced **`REPLAY_WINDOW` = 2048 counters past the
> boundary** (`g ≥ eE + 2048`).

And at a given `r ≤ W`, the number of previous-epoch counters that can still be
delivered is `W − r` (those in `[g − W, eE − 1]`): **2048 at the boundary,
falling to 1 at `r = 2047`, 0 from `r = 2048` on.**

**Off-by-one check.** The figure is 2048 and not 2047 *only because* of the
deliberate index offset at `src/core/connection/session.rs:52–64` — the greatest
lives outside the bitmap, so offsets 1..=2048 are covered and "exactly 2048
behind" is accepted. An implementation putting the greatest at bit 0 would give
2047. The value is therefore exactly `REPLAY_WINDOW`, and the coincidence is
structural, not accidental.

**The 32× figure.** `REKEY_EPOCH_MSGS / REPLAY_WINDOW = 65 536 / 2048 = 32`,
exactly, under both natural framings:
* fraction of the previous epoch ever deliverable: at best `2048 / 65 536 = 1/32`
  (only its newest 2048 counters, and only at `r = 0`);
* fraction of the current epoch during which the previous key is still useful:
  `2048 / 65 536 = 1/32`.

### 3c. MEASURED — a probe crate outside the repo

Read-only w.r.t. the repository: a throwaway crate at
`…/scratchpad/round41/probe` with a **path dependency** on slither
(`features = ["test-util"]`); nothing in the repo was written to. Knob epoch
`E = 3000` (ruling 82's `with_epoch_size`), so `E > W + 1` and the two limits
separate. Two full-size datagrams are held from the fabric (`FlakyPolicy::drop_at`,
the `hold_one` construction from `tests/spec_rekey.rs:303–348`), the sender is
pumped to put the receiver's greatest at a chosen distance, then each held packet
is injected and delivery is observed.

```
cargo test --release --test probe -- --nocapture

HELD P counter=961 epoch=0  Q counter=962 epoch=0
PHASE 1 (across the boundary): g=3010 epoch=1  g-P=2049  g-Q=2048
PHASE 1 RESULT: prev-epoch P(2049 behind)=[]  prev-epoch Q(2048 behind)=[502]
PHASE 2 (inside one epoch): g=5060 epoch=1  g-R=2049  g-S=2048
PHASE 2 RESULT: same-epoch R(2049 behind)=[]  same-epoch S(2048 behind)=[602]
epoch of R/S = 1 / 1, epoch of g2 = 1
test straggler_edge ... ok
```

Reading it: **a straggler from the immediately preceding epoch behaves at the
window edge exactly as a same-epoch packet does** — 2048 behind is delivered,
2049 behind is silently dropped, in both cases. The epoch contributes nothing to
the cutoff; §7.2's window is the whole of it. (Phase 1's P and Q are one counter
apart and in the same epoch as each other, so the only variable between the
delivered and the refused case is the distance.)

## 4. The ruling-251 tests — what is and is not pinned

`tests/spec_rekey.rs` (872 lines) holds five tests; `tests/story_rekey.rs` (414
lines) holds one.

| Test | Line | What it actually asserts |
|---|---|---|
| `rk1_the_rekey_of_zeros_vector` | `tests/spec_rekey.rs:424` | the `REKEY(0³²)` constant, computed test-only via `cryptoxide`. Nothing about windows. |
| `rk2_crossing_several_epoch_boundaries_is_invisible` | `tests/spec_rekey.rs:499` | a stream is byte-exact across boundaries; no handshake, no event. |
| `rk3_the_two_directions_ratchet_independently` | `tests/spec_rekey.rs:607` | each direction's epoch advances on its own counter. |
| `rk4_a_packet_two_epochs_back_does_not_open` | `tests/spec_rekey.rs:691` | a held packet, injected once B has committed `stale_epoch + MAX_EPOCH_JUMP`, produces **no** delivery; then a current-epoch oracle does. **Explicitly disclaims the window as the cause**: `tests/spec_rekey.rs:686–689` — *"§7.2's replay window cannot produce this silence … the distance from B's highest opened counter is under 60 — far inside `REPLAY_WINDOW`'s 2 048."* |
| `rk5_a_packet_one_epoch_back_still_opens` | `tests/spec_rekey.rs:822` | at `EPOCH = 16` (`tests/spec_rekey.rs:107`), a straggler exactly one epoch back **is delivered** (`tests/spec_rekey.rs:862–869`). At that epoch size the distance is a handful of counters, so the window is never in play. |
| `s23_a_long_lived_connection_rekeys_itself_without_the_user_noticing` | `tests/story_rekey.rs:223` | S23 at the production constant: byte-exactness, counter monotonicity, zero handshakes, zero extra DH. |

**No test pins the scope.** Nothing anywhere in the repository asserts that a
*previous-epoch* straggler more than `REPLAY_WINDOW` behind is refused. What
exists instead:

* **Prose, in two places, recording exactly this finding** — written by the
  round-40 test author and never promoted to SPEC.md:
  * `tests/spec_rekey.rs:801–820`, a heading *"Why this lives at the knob and
    cannot be repeated at 65 536"*, with the measured table
    (1 000 → opens, 3 000 → no, 5 000 → no) and the conclusion *"the retained
    previous-epoch key is used only for packets within `REPLAY_WINDOW` counters
    of a boundary"*;
  * `tests/story_rekey.rs:384–405`, the same table and the same sentence, plus
    *"An assertion here would be red on a conforming build, which is a flake and
    not a pin."*
  Note both tables are measurements of **same-epoch** distances (the comment says
  *"with the packet and the receiver's commit inside one epoch throughout"*), so
  they establish the window's behaviour, not the composite. §3c above measures
  the composite.
* **The window edge itself, two-sided, at unit level**:
  `the_window_edge_is_two_sided`, `src/core/connection/session.rs:670–686` —
  `greatest − 2048` accepted, `greatest − 2049` refused; and
  `advancing_the_greatest_slides_the_window`, `src/core/connection/session.rs:688–700`.
  Also `the_derivation_never_reaches_past_the_windows_edge`,
  `src/core/connection/tests_ack.rs:688–707`.

So the Appendix B obligation (`SPEC.md:7293–7298`) is **discharged as written** —
it is phrased in "opens", and `rk4`/`rk5` pin opening — while the delivery scope
is pinned nowhere.

## 5. Refusal attribution — and it *is* observable

### The code path for a same-epoch packet 3000 counters back

1. `classify::<C>` accepts it as `Inbound::Data` (`src/core/connection/mod.rs:547`).
2. `C::open` → `DatagramRecv::decrypt_at` → `msg_epoch == current_epoch`, so it
   is decrypted under the **committed current key** with no state change
   (`hiss-0.3.2/src/noise/datagram.rs:346–348`). It is a genuine packet, so the
   tag verifies and `open` returns `Ok`.
3. `check_and_mark` computes `offset = 3000 > 2048` and returns `false`
   (`src/core/connection/session.rs:138–141`).
4. slither emits `tracing::debug!(target: "slither::replay", counter, greatest, …)`
   (`src/core/connection/session.rs:611–616`) and returns `None`
   (`src/core/connection/session.rs:617`).
5. `handle_datagram` returns at `src/core/connection/mod.rs:576`: no delivery, no
   liveness refresh, no roam, no ACK-record mark, no error to the application.

**So the refusal is the window-mark check, on a packet that decrypted
successfully.** The claim's attribution is correct — and it is correct for a
*previous-epoch* straggler too, because step 2's prev-key branch
(`hiss-0.3.2/src/noise/datagram.rs:353–358`) also returns `Ok`.

### Is it observable? Yes — at exactly one surface

* **API surface: indistinguishable.** Both are `open() -> None` → an early
  `return` in `handle_datagram`. `Session::open`'s own doc says so
  (`src/core/connection/session.rs:559–565`): *"`None` is a silent drop — a
  forgery, a straggler more than one epoch back, a duplicate, or a counter beyond
  the window's tail. §7.2 gives all of those one behaviour, and this function's
  caller cannot tell them apart, deliberately."*
* **Operator surface: different.** A window refusal emits `slither::replay`
  (§18.2, `SPEC.md:6901` — *"replay-window rejections"*); an AEAD failure emits
  **nothing**, by the deliberate comment at
  `src/core/connection/session.rs:601–606`.

**MEASURED** (same probe run, tracing subscriber at TRACE; full log in
`…/scratchpad/round41/probe-output.txt`):

```
--- injecting P (prev epoch, 2049 behind) ---
DEBUG slither::replay: a received packet was rejected by the replay window counter=961 greatest=Some(3010)
--- end P ---
...
--- injecting P again (TWO epochs back, 5045 behind) ---
--- end P-again; delivered=[] ---            ← no slither::replay event at all
```

The *same held packet* produces the `slither::replay` event when it is one epoch
back and out of the window, and **no event** when it is two epochs back. That is
the mechanical proof of the attribution: at 2049 behind, hiss's retained
previous-epoch key **opened it** (the event is unreachable except through
`C::open(..).ok()?` at `src/core/connection/session.rs:590–597`), and slither's
window threw it away.

A corollary worth recording: the operator-visible consequence of the scope is
that **ordinary post-boundary reordering shows up under `slither::replay`, not as
a decryption failure** — so an operator watching that target sees straggler loss
attributed to replay, which is exactly right and exactly not what §7.7's sentence
would lead them to expect.

## 6. Draft material

### 6a. Three candidate §7.7 clauses

All three insert **mid-line at `SPEC.md:3189`**: after *"anything older is
refused, its key ratcheted away."* and before *"The ratchet is **forward rotation
only, not healing**…"* — inside the retention sentence's own paragraph, because
that is where the reader forms the belief (working rule 4a: the defect is in the
sentence being read, not somewhere else).

**Option A — minimal, states the bound and nothing else.**

> Retention is not reach. The retained key opens **any** counter of the
> preceding epoch, but §7.2 binds after it: a packet more than
> `REPLAY_WINDOW` counters behind the greatest authenticated counter is
> dropped post-AEAD without delivery. The previous-epoch key therefore
> delivers only while the receiver's greatest is within `REPLAY_WINDOW`
> counters of the boundary — 2048 of `REKEY_EPOCH_MSGS`' 65 536, one part
> in 32.

*Tradeoff:* smallest possible amendment and provably true, but leaves a reader to
work out for themselves why the sentence above it says "one epoch".

**Option B — A, plus the attribution an operator needs.**

> Retention is not reach. The retained key opens **any** counter of the
> preceding epoch, but §7.2 binds after it: a packet more than
> `REPLAY_WINDOW` counters behind the greatest authenticated counter is
> dropped post-AEAD without delivery — so the previous-epoch key delivers
> only while the receiver's greatest is within `REPLAY_WINDOW` counters of
> the boundary, one part in 32 of `REKEY_EPOCH_MSGS`. Such a straggler is
> refused by the **window**, not by the ratchet: it appears under
> `slither::replay` (§18.2), never as a decryption failure. That asymmetry
> is the only surface on which the two refusals differ.

*Tradeoff:* adds the one fact a post-mortem actually needs (measured in §5) at the
cost of pulling §18.2 into a crypto section; it also hard-couples §7.7 to a trace
target, which §18.2 already calls *"operator-visible contract"*.

**Option C — A, plus what it means for a conformance test (feeds Appendix B).**

> Retention is not reach. The retained key opens **any** counter of the
> preceding epoch, but §7.2 binds after it, so the previous-epoch key
> delivers only within `REPLAY_WINDOW` counters of the boundary. The
> tolerance is stated in epochs because epochs are what the key schedule
> retains; the **deliverable** set is bounded by §7.2 and is never wider
> than `REPLAY_WINDOW`. A conformance test for straggler tolerance must
> therefore be built at an epoch size below `REPLAY_WINDOW` — at
> `REKEY_EPOCH_MSGS` the delivered case is unreachable, and an assertion
> there would be red on a conforming build.

*Tradeoff:* the only option that repairs the Appendix B obligation's blind spot
(§4) and that tells the next test author why `rk5` must live at the knob — but it
puts test methodology into §7.7 rather than into Appendix B, where a companion
edit would be the tidier home.

**Companion Appendix B edit (pairs with any option, recommended with A or B).**
At `SPEC.md:7293–7298`, after *"a packet from **two** epochs back is refused
without key derivation"*, add:

> Both pins are about **opening**, and must be built at an epoch size below
> `REPLAY_WINDOW`: §7.2's window is what refuses a preceding-epoch packet
> further back than that, and at `REKEY_EPOCH_MSGS` it refuses every one
> of them.

**Consistency sweep the amendment must carry (working rule 4).** `SPEC.md:542`
— the §2.1 hiss-facts row — already argues this arithmetic in the opposite
direction (*"the reordering budget (replay window, 2048) sits far inside one
epoch (65 536)"*). It is not wrong, but it is the second place a reader forms a
belief about straggler reach, and it currently reads as *reassurance* that one
epoch is generous. Whichever clause lands, that row should be re-read in the same
pass; `SPEC.md:2166–2169` (§7.2's sizing bullet, *"comfortably inside one ratchet
epoch"*) is the third and is consistent as written. No other text in SPEC.md
argues from straggler reach (`grep -n -i straggler SPEC.md` → 27, 542, 3188,
7293, 7297; `grep -n -i reorder SPEC.md` → 542, 2167, 3496, 4064, 6297, 7203 —
27 is the ruling-251 amendment-table row, 3496/4064 are frame idempotence,
6297 is the `Wire` contract and 7203 is stream reassembly), so nothing downstream
depends on the loose reading — which is the tiebreak working rule 3 asks for:
**no proof rests on "one epoch back", while §12.2's ACK fusion rests on the
window being the single record.**

### 6b. Could code widen the straggler window to match the sentence? No.

**Is it even wire-visible?** No. The Data header is
`type(1) ‖ version(1) ‖ receiver_index(4) ‖ counter(8)`
(`src/packet/header.rs:83–103`) — **there is no epoch field**; the counter *is*
the epoch selector, derived identically at both ends. Straggler reach is a
receiver-local delivery policy: widening it moves no wire byte and breaks no
golden vector. It is ratified behaviour all the same, so it is still a ruling.

Three ways it could be widened, and why each is refused:

1. **Enlarge `REPLAY_WINDOW`.** Out of scope by instruction (ratified constant),
   and independently unattractive: §7.2 fuses the window to the ACK record
   (`SPEC.md:2171–2172`), so the bitmap is also the ACK bitmap — 2048 bits =
   256 B/connection today, and matching one production epoch would need 65 536
   bits = 8 KB per connection per direction, plus pressure on `MAX_ACK_RANGES`.
2. **A second, epoch-scoped acceptance record for stragglers.** Directly
   forbidden: *"The ACK record stays **fused** to this window (§12.2): the window
   is the single received-packet record — reuse, don't duplicate"*
   (`SPEC.md:2171–2172`), and slither has a test whose stated mutation target is
   exactly this second tracker
   (`the_derivation_never_reaches_past_the_windows_edge`,
   `src/core/connection/tests_ack.rs:684–687`). A packet delivered but not in the
   window is also un-ACKable, which manufactures the spurious retransmission
   §7.2's own failure analysis is at pains to bound.
3. **Deliver old-epoch packets without marking them.** This is not a widening,
   it is the removal of replay protection for everything older than the window:
   §7.2's *"No replayed packet ever moves the endpoint, refreshes liveness, or
   funds the budget"* (`SPEC.md:2157–2160`, ruling 169) becomes unenforceable for
   precisely the class being widened.

And the widening buys nothing measurable. §7.2 sizes 2048 as *"~20 ms of
reordering memory at 1 Gbps line rate and ~200 ms at 100 Mbps"*
(`SPEC.md:2166–2168`); a datagram 2049+ counters late is outside any reordering
budget the protocol claims to keep, epoch boundary or not — the boundary is not a
reordering event, it is a key change that happens to coincide with one counter.
**Recommendation: no code change; amend the clause.**

## 7. VERDICT

| Sub-claim | Verdict | Decisive citation |
|---|---|---|
| **Reachability bound** — "the one-epoch-back key is reachable only within `REPLAY_WINDOW` (2048) counters of an epoch boundary" | **PARTIAL** | The *numeric* bound is exact and measured: prev-epoch packet 2048 behind delivered, 2049 behind dropped (`probe-output.txt`, PHASE 1: `g=3010 g-P=2049 g-Q=2048` → `P=[] Q=[502]`). The *verb* is wrong: the key is **reached and successfully used** for every counter of the preceding epoch — `hiss-0.3.2/src/noise/datagram.rs:353–358` has no distance term — what is bounded is **delivery**, at `src/core/connection/session.rs:137–141`. Any clause that lands should say "delivers", not "is reachable". |
| **The 32× figure** | **VERIFIED** | `REKEY_EPOCH_MSGS / REPLAY_WINDOW = 65 536 / 2048 = 32` exactly (`src/constants.rs:150`, `src/constants.rs:159`), under both framings (§3b). The 2048 — rather than 2047 — is itself exact, and only because the greatest sits outside the bitmap so offsets 1..=2048 are covered: `src/core/connection/session.rs:52–64`, pinned two-sided by `the_window_edge_is_two_sided`, `src/core/connection/session.rs:670–686`. |
| **Refusal attribution** — a same-epoch (or prev-epoch) packet >2048 back is refused by §7.2's window, not by the ratchet | **VERIFIED** | The order is AEAD-then-window at `src/core/connection/session.rs:590–600`, and the measurement separates the two: the *same held packet* logs `slither::replay counter=961 greatest=Some(3010)` when one epoch back and out of window, and logs **nothing** when two epochs back (`probe-output.txt`). The event is unreachable except through a successful `C::open`. |
| **"Unstated" scope** (the framing of the round-41 item) | **PARTIAL** | Unstated **in §7.7** — confirmed, §7.7 (`SPEC.md:3167–3231`) never names `REPLAY_WINDOW`. But the arithmetic is stated at `SPEC.md:542` (§2.1's hiss-facts row) and the consequence is written out twice in-tree, at `tests/spec_rekey.rs:801–820` and `tests/story_rekey.rs:384–405`. The finding is a **relocation and promotion**, not a discovery. |

**Conflict report (working rule 3).** There is **no contradiction** between §7.7
and §7.2 to adjudicate: the two sentences are about different predicates —
§7.7's is about which key **opens**, §7.2's about what is **delivered** — and both
are literally true of the code as written. What §7.7 has is working rule 8's
defect: *a stated construction with an unstated scope*. The gloss
"(straggler tolerance: one epoch back)" invites a delivery reading that the
protocol does not provide at the ratified epoch size, and nothing in the section
bounds it. Recorded here rather than resolved.

**One thing the round-41 item does not say, and should.** The Appendix B
obligation at `SPEC.md:7293–7298` is phrased entirely in *"opens"* and is
therefore **correctly discharged** by `rk4`/`rk5` — it is not defective, it is
narrow. The gap is that nothing, in spec or tests, pins the delivery side. §3c's
probe is the shape such a test would take: two adjacent held packets from the
preceding epoch, the receiver's greatest placed exactly 2048 past one of them,
one delivered and one not. It needs an epoch size in `(REPLAY_WINDOW, …)` —
3000 was used here — and about 5 000 pumped datagrams with virtual time
advancing, which is well inside a paused-clock test's budget (5.1 s release,
this machine).

**Preferred clause: Option A, with the Appendix B companion.** It is the smallest
true statement, it needs no new cross-section coupling, and the companion is what
stops the next test author re-deriving the measurement a third time.

---

### Artefacts

* This file.
* `…/scratchpad/round41/probe-output.txt` — the full probe run, including both
  `slither::replay` events and their absence.
* `…/scratchpad/round41/probe/` — the throwaway measurement crate (path
  dependency on slither with `features = ["test-util"]`; **nothing in the
  repository was written to**, verified: the probe's own `target/` and
  `Cargo.lock` live under the scratchpad).
