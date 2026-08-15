# Slice 3 — The walking skeleton (PLAN)

> Planner's note: written incrementally per working rule 2 — the heading
> skeleton was on disk before the first `Read`. No production code and no
> tests are written by this document's author.

## Sources consulted (and deliberately not consulted)

**`SPEC.md`, by targeted line range only — never whole** (working rule 1):
§3.4 (625–674), §7.1–7.2 (1844–1892), §7.4 (1947–1996), §7.7–7.9
(2465–2543), §8 (2544–2809), §12.2 (3388–3415), §15 (3743–3855),
§16.1–16.3 (3858–4353), §16.4–16.7 (4354–4629), §16.8–16.10 (4630–4702),
§18.1–18.2 (5055–5221), Appendix B (5365–5410, 5475–5487, 5695–5771),
Named constants (5787–5881).

**Also read:** `CLAUDE.md` (whole), `PLAN.md` §1–§2 and §4–§9,
`STORIES.md` (S1, S2, S23, S24–S27, S29), `.spec-v2-clean-slate/rulings.md`
(index plus rulings 62–63 and the 60–80 locations),
`.slices/01-packets/PLAN.md` and `.slices/02-handshake/PLAN.md` (heading
structure, as format specs), `.slices/02-handshake/OPEN-QUESTIONS.md`
(§C, §D), `src/core/connection/mod.rs`, `src/core/mod.rs` (150–262),
`src/constants.rs` (grep), `src/error.rs` (grep),
`hiss-0.3.2/src/noise/datagram.rs` (the four methods slice 3 uses).

**Deliberately not consulted:** `.spec-v2-clean-slate/SPEC-v0.1-wire-historical.md`
and the v0.1 implementation at `5324ce5` — neither governs current
behaviour.

**No gate was run and none is claimed green.**

## 0. Cut recommendation — **cut it, with two amendments**

**Recommendation: cut slice 3 into 3a and 3b, on the maintainer's
boundary, with two amendments.**

| | Contents | Stories closed | Rough size |
|---|---|---|---|
| **3a — the connection core** | §7.1–7.2, §7.7–7.9, **§7.4's two seal paths + arming rule + install pin + the `Liveness` timer** (amendment 1), §8.2–8.7 codec with PADDING/PING/ACK/CLOSE, §15's frame + three post-mortem states + registry, §16.4's poll contract on `core::Connection`, §16.5's connection timer table and equal-deadline order, §16.7 plan-seal-commit | **S23**, plus a core-level S1 precursor | ≈ 1.6–2.0 k |
| **3b — the shell** | the driver, slice 2b's `Endpoint`/`Connecting`/staged handles, `Connection` handle + `closed()`, accessors, §16.2/§16.3/§16.8/§16.9/§16.10, ruling 53's two mechanisms, ruling 49's `slither::io` trace | **S1, S2, S26, S27 (`closed()` half), S29** | ≈ 1.6–2.0 k |

### 0.1 Why cut — the arguments that are evidence, not tidiness

1. **The test-author/implementer split (working rule 6) does not
   work across the combined slice, and works cleanly across the cut.**
   Rule 6 requires the story test to be written from the story and its
   spec section *first*, by someone who is not the implementer. For 3b's
   stories that test is an async paused-clock flow test that must
   *compile against a public API*. In a combined slice the shell's API
   does not exist when the shell's test author starts, so either the
   author blocks on the implementer (serialising the slice) or both are
   handed the same API sketch — which is precisely the
   "one author making both wrong in a mutually consistent way" that
   rule 6 exists to prevent. After a cut, 3a's surface is **frozen and
   green** before 3b's test author starts, and 3b's test author writes
   against a real, compilable, already-tested core. This is a mechanism,
   not a preference, and it is the strongest single argument.
2. **The scheduled seam review has a much better target.** `PLAN.md` §5
   charters a post-slice-3 review of "the poll contract's drain
   discipline, `Retired` ordering, waker registration under `RefCell`,
   cancel-safety of every `async fn`, drop order across handles". Every
   one of those five is 3b. Landing the review after a combined ~4 k-line
   slice makes the reviewer carry the frame codec, the replay window and
   the ratchet as noise. After the cut it reviews ~1.8 k lines of shell
   with a green core underneath it. **Note:** the review is chartered
   after "slice 3" — after the cut it belongs after **3b**, not after 3a.
3. **The dependency is strictly one-directional.** 3b needs 3a's
   `close()`, `ConnEvent::Closed`, and `ToEndpoint::Retired`. 3a needs
   nothing from 3b — §16.10 makes the core drivable with no kernel and
   `src/core/tests.rs` already drives two endpoint cores directly. There
   is no round trip to pay for.
4. **File-path disjointness (working rule 6, second half) becomes
   trivial.** 3a owns `src/core/connection/**` and nothing else; 3b owns
   `src/shell/**` and nothing else. In a combined slice, four concurrent
   agents would be contending for `src/core/tests.rs` and a new
   `src/shell/tests.rs` at once — which is exactly the shape of the slice
   2a accident (an implementer's stub overwrote 68 independently written
   tests because two briefs named one path).
5. **Interruptibility.** `PLAN.md` §8 risk 2 mitigates scope by ending
   each slice green. Slice 3 is the biggest in the plan and now also
   carries 2b; one interruption point over ~4 k lines is the weakest
   point in that mitigation.
6. **Precedent.** Slice 2's cut worked, and its planning artefact
   (`.slices/02-handshake/PLAN.md` §1.1) records that the narrowing was
   right on the spec's own terms.

### 0.2 The honest cost, stated plainly

**3a closes exactly one story — S23 — and leaves the headline story S1
open through it.** That is real and I am not going to dress it up. Three
things bound the cost:

- S1 was going to be open through slices 0, 1 and 2 regardless; 3a adds
  one boundary to a wait that is already four slices long.
- 3a can close a **core-level S1 precursor** on the existing two-endpoint
  paused-clock fixture: dial → install → `close(code, reason)` → the peer
  core surfaces `ConnEvent::Closed(PeerClosed { code, reason })` and ours
  surfaces `LocallyClosed`. That is S1's protocol content, minus its
  handles. It should be written and named as a precursor, **not** as S1
  — a name is not a pin (working rule 9), and calling it S1 would let 3b
  ship without the story test S1 actually asks for.
- 3a's definition of done is therefore **S23 plus Appendix B's "Frame
  layer" and "CLOSE" obligation lists**, which are spec obligations
  rather than stories. Slice 0 closed zero stories and slice 1 closed one
  partial, so this is within precedent — but it should be *stated* in
  3a's brief, so nobody discovers at review time that the slice looks
  story-thin.

### 0.3 Amendment 1 — §7.4's liveness half moves into 3a

`PLAN.md` §4 gives "§7.4 liveness / `seal_quiet`" to slice 7. **§15.2
requires `seal_quiet` to seal CLOSE**, so slice 3 cannot implement its own
headline frame without it. Full statement of the conflict is **C1**.
Recommended split of §7.4:

| §7.4 content | Slice |
|---|---|
| the two seal paths (`seal` marks `last_send`, `seal_quiet` does not) | **3a** |
| the death-clock arming rule (marking **or** ack-eliciting; arms on first, not re-armed, reset by every authenticated window-fresh receive) | **3a** |
| the install pin (both clocks at install, deadline **already armed**) | **3a** |
| the `Liveness` timer and `ConnectionLost::TimedOut` | **3a** |
| §7.5's keepalive, persistent keepalive, contested probe, `Contested` timer | **7** |

This is a ~60-line addition to 3a and it buys three things: §15.2's CLOSE
becomes implementable as written; §15.4's teardown matrix reaches **five
of eight rows** in slice 3 instead of three; and Appendix B's `closed()`
obligation — which names `TimedOut` explicitly — becomes reachable in 3b
rather than deferred to slice 7. Without it, 3b ships a `closed()` that
has never been observed to resolve on a death the application did not
cause, which is the case ruling 46 exists for.

### 0.4 Amendment 2 — sequence 3b internally, driver first

Ruling 53's seam is two mechanisms, and they are separable:

1. **command channel + oneshot** — `Endpoint`, `Connecting`, the three
   staged handles. This is slice 2b's deferred work and it exercises the
   driver's endpoint side only.
2. **`Rc<RefCell<ConnShell>>` + waker maps** — the `Connection` handle,
   `close()`, `closed()`, the four accessors.

Build 1 before 2 inside 3b. The endpoint side has a fully-tested core
underneath it (slice 2a) and no waker maps at all, so it validates the
driver's `select!` loop, the `Wire` seam and ruling 49's trace against
known-good protocol behaviour before any `RefCell` borrow discipline
exists to confound a failure. **Do not make this a third slice** — 1 and 2
share `driver.rs`, so they cannot be given to disjoint concurrent agents,
and a slice boundary that does not enable parallelism is only overhead.

### 0.5 If the maintainer declines the cut

The plan below is written so it can be executed as one slice. The single
change is that §10's agent table collapses from two waves to one, and
working rule 6's authorship split then requires the shell test author to
be given the §4 API digest of this plan as its contract — which is the
weaker arrangement, and the reason for the recommendation above.

## 1. Scope and inheritance

### 1.1 What slice 3 was handed

From `PLAN.md` §4's slice table, row 3:

> **3 — The walking skeleton.** §7.1–7.2 counter + replay window, §7.7
> ratchet, §7.9, §8 frame codec with CLOSE/PING/ACK only, §15 CLOSE +
> teardown matrix, §16.4 poll contract on both cores, §16.5 timer table,
> §16.7 plan-seal-commit, the driver, `closed()`. Stories closed: **S1,
> S23, S26, S27 (`closed()` half)**.

Plus, carried from the 2a/2b re-cut, **slice 2b**: the shell `Endpoint`,
`Connecting` and the three staged handles, which close the user-visible
halves of **S2** and **S29** (their core halves already pass in
`src/core/tests.rs`).

And `PLAN.md` §4's own warning, which is the reason this plan is long:

> **Slices 0–3 are the spine and 3 is the risk.** It is the first slice
> where both cores, the driver, and the handle seam exist together, and
> it is the layer three protocol-focused reviews never examined.

### 1.2 What exists at `61d867e`

| Area | State |
|---|---|
| `src/packet/` | complete — suite decl, three headers, mac1, §3.1 gate, golden vectors |
| `src/core/endpoint/` | complete for 2a — typestate, intro queue, §17.1 guard, §17.2–17.4 tables, §5.5 driving |
| `src/core/connection/mod.rs` | **138 lines**, deliberately minimal: session `Option`, §16.6 sub-seed, `handle_endpoint_event` (`Install`), `poll_output()` terminating in `Timeout(None)` |
| `src/shell/` | `mod.rs` is 9 lines; `wire.rs` is the `Wire` trait + `UdpSocket`. **No driver, no handles.** |
| `src/testutil/` | 1378 lines — `Network`, `FlakyWire`, `FlakyPolicy`, counting identity, two-endpoint paused-clock fixture |
| `src/varint.rs` | §8.1 done |

263 tests; all eight gates green on `61d867e`.

### 1.3 Two hand-forwards inherited

- **`.slices/02-handshake/PLAN.md` §7.2 is wrong and known to be.** It
  pins the §17.1 timestamp guard at `authenticate()`; §17.1 puts it at
  `read_identity()` and the implementation follows the spec. Nothing in
  this plan inherits the error. (Recorded as D4 in
  `.slices/02-handshake/OPEN-QUESTIONS.md`.)
- **Not slice 3's to fix:** §6.6/§6.7's `exempt_until` extends a guard
  pin past `HANDSHAKE_GIVEUP`, which breaks ruling 70's
  `TS_GUARD_ORPHAN_TTL == INTRO_TTL` alias. Slice 3 does not touch
  §6.6–6.8, so it is noted and left for slice 7.

### 1.4 Three carries from `.slices/02-handshake/OPEN-QUESTIONS.md`

- **§C: the §16.6 RNG sub-seed draw order is explicitly deferred to
  slice 3** — "no accessor; `connect()` mints index and draws sub-seed in
  one call". Closed here as **Q9** / **T9**.
- **§C, and this one interacts with Q1: slice 2a's core reuses
  `ToEndpoint::Retired` as S29's cancellation signal**, because "§16.4
  lists no cancel verb". That is recorded as a *plan derivation, not a
  ratified rule*, and its two tests say so in their own doc comments. So
  `Retired` currently carries **two** meanings — "this connection's
  teardown is complete" and "this pending was cancelled" — and **Q1**
  changes the first. Whoever answers Q1 must check the second does not
  move underneath slice 2a's tests. Flagged; not resolved here.
- **§D5: §16.4's Rust block is schematic, not literal.** `EndpointOutput`,
  `Install`, `EstablishedSession` and `Connection` are all generic over
  `<C: Handshake>` because the seal/open halves are
  `DatagramSend<IK>`/`DatagramRecv<IK>`. Every signature quoted from
  §16.4 in §3.8 below carries the same caveat.

---

## 2. Module map and file ownership

The tree follows `PLAN.md` §1's planned layout exactly — §8's codec lives
in `core/connection/frame.rs`, **not** in `packet/`, because `packet/` is
§2–§4's outer packet and is complete.

### 2.1 Slice 3a — files

| Path | Implements | New/grow | Owner |
|---|---|---|---|
| `src/core/connection/mod.rs` | §16.4's `core::Connection`, `ConnOutput`, `ConnEvent`; the drain | **grow** (138 → ~450) | **IMPL-A** |
| `src/core/connection/session.rs` | §7.1 counter, §7.2 replay window, §7.7 ratchet wiring, §7.9 exhaustion, §7.4's two seal paths + arming + install pin | new | **IMPL-A** |
| `src/core/connection/frame.rs` | §8.2 parse-then-apply, §8.3 table, §8.4 PADDING/PING/ACK/CLOSE layouts, §8.5 packing order, §8.7 classes | new | **IMPL-A** |
| `src/core/connection/close.rs` | §15.1–15.4: closing/draining, linger, reply rate, registry | new | **IMPL-A** |
| `src/core/connection/timers.rs` | §16.5's named timer table, equal-deadline order, min-deadline out | new | **IMPL-A** |
| `src/core/mod.rs` | re-exports for the new connection types | grow (small) | **IMPL-A** |
| `src/constants.rs` | nothing new expected — verify (§2.3 below) | grow (only if a gap is found) | **IMPL-A** |
| `src/core/tests.rs` | **existing 3152 lines** — 3a's core tests append here | grow | **TEST-A** |
| `tests/spec_frames.rs` | §8 conformance from the spec alone | new | **TEST-A** |
| `tests/spec_close.rs` | §15 conformance from the spec alone | new | **TEST-A** |

**The hard rule.** `src/core/tests.rs`, `tests/spec_frames.rs` and
`tests/spec_close.rs` belong to **TEST-A and to nobody else**. `IMPL-A`
must not create, stub, touch or reformat any of the three. `src/core/mod.rs`
already declares `#[cfg(test)] mod tests;` — IMPL-A changes nothing about
that declaration and creates no file behind it. This is the slice-2a
accident, and it is the one rule in this plan with no exception.

### 2.2 Slice 3b — files

| Path | Implements | New/grow | Owner |
|---|---|---|---|
| `src/shell/mod.rs` | module wiring, the `LocalSet` contract in its docs | grow (9 → ~40) | **IMPL-B** |
| `src/shell/driver.rs` | §16.3's `!Send` actor: `select!` over wire recv / command channel / timer; both cores; the drain discipline; ruling 49's `slither::io` trace | new | **IMPL-B** |
| `src/shell/endpoint.rs` | §16.2 `Endpoint`, `EndpointBuilder`, `connect`, `accept`, `Connecting` (ruling 62 handle, ruling 50 cancel-on-drop) | new | **IMPL-B** |
| `src/shell/staged.rs` | §6.2's `Intro → Claimed → Proven` handles, drop = silent reject | new | **IMPL-B** |
| `src/shell/connection.rs` | §16.2 `Connection` handle, `close()`, `closed()` latch, the four accessors, the notification slots (declared, filled in slice 7) | new | **IMPL-B** |
| `src/testutil/mod.rs` | the `LocalSet` two-endpoint driver harness | grow | **IMPL-B** |
| `src/lib.rs` | public re-exports of the shell types | grow (small) | **IMPL-B** |
| `tests/story_lifecycle.rs` | S1, S26, S27(`closed()`), S29 as paused-clock flow tests | new | **TEST-B** |
| `tests/story_dial.rs` | S2 (shell half) as a paused-clock flow test | new | **TEST-B** |
| `tests/spec_shell.rs` | Appendix B's "The shell surface" obligations reachable in slice 3 | new | **TEST-B** |

**The hard rule, again.** `tests/story_lifecycle.rs`, `tests/story_dial.rs`
and `tests/spec_shell.rs` belong to **TEST-B alone**. IMPL-B creates none
of them.

**One shared-path hazard to resolve before dispatch.** `src/testutil/mod.rs`
is listed under IMPL-B because the driver harness is production-shaped
plumbing, but TEST-B will want to *use* it. That is fine (use is not
ownership) provided TEST-B's brief names the harness's API and TEST-B does
not edit the file. If TEST-B needs a fixture capability IMPL-B did not
build, it goes back to the orchestrator — **it does not add it**. An
alternative is to give `testutil` to TEST-B and have IMPL-B consume it;
either assignment works, **but the brief must state which**, and the two
briefs must agree.

### 2.3 Constants — verified present, nothing new expected

Checked against `src/constants.rs` at `61d867e`. Every constant slice 3
needs already exists: `REPLAY_WINDOW` (2048), `REKEY_EPOCH_MSGS` (65 536),
`MAX_EPOCH_JUMP` (2), `FRAME_PADDING`/`FRAME_PING`/`FRAME_ACK`/
`FRAME_CLOSE` and the full §8.3 type set including
`FRAME_STOP_SENDING_RESERVED`, `STREAM_OFF`/`_LEN`/`_FIN`,
`CLOSE_REASON_MAX` (256), `CLOSE_LINGER` (5 s),
`CLOSE_REPLY_MIN_INTERVAL` (1 s), `MAX_ACK_RANGES` (64),
`SHELL_LATENESS_BOUND` (250 ms), `APPLICATION_ERROR_BASE` (0x10),
`MAX_PLAINTEXT` (1170), `MAX_DATAGRAM` (1200).

Two identifiers §15.3's registry needs are **not** in the constants file
as named codes — the registry is a table of six wire error codes
(`0x00` `NO_ERROR` … `0x05` `FINAL_SIZE_ERROR`, `0x06`
`MESSAGE_OVERFLOW`) and only `APPLICATION_ERROR_BASE` is named. Slice 3
introduces the registry as a type. See **U3**.

### 2.4 The ratchet is already wired — S23 is mostly a test obligation

`src/packet/suite.rs` already stamps
`fn into_datagram(transport, epoch_size) ->
transport.into_datagram_with_epoch(epoch_size)`, and both
`src/core/endpoint/mod.rs:652` and `src/core/endpoint/staged.rs:533` call
it with `Self::epoch_size()`. hiss 0.3.2's `into_datagram_with_epoch`
implements §7.7 exactly: the send half "advances its key eagerly as its
monotonic counter crosses each boundary; the receive half keeps the
current and the immediately-preceding epoch keys", with commit-only-after-
verify and `MAX_EPOCH_JUMP`-bounded look-ahead.

**So slice 3's §7.7 work is: pin it, and do not chase epochs.** The
implementation obligation is a negative one. The REKEY test vector
(`REKEY(0³²) = 25ce5d37…4c58`) is hiss's to satisfy; slither's test
asserts the **epoch boundary is observable at counter 65 536** and that
nothing in slither's code inspects or manipulates an epoch. See **T1**.

### 2.5 The hiss surface slice 3 rides, exactly

From `hiss-0.3.2/src/noise/datagram.rs`, the four methods the connection
core uses and nothing else:

```rust
impl DatagramSend<Proto> {
    pub fn encrypt_next(&mut self, ad: &[u8], plaintext: &[u8], output: &mut [u8])
        -> Result<(u64, usize), HandshakeError>;
    pub fn next_counter(&self) -> u64;
    pub fn session_id(&self) -> &SessionId;
}
impl DatagramRecv<Proto> {
    pub fn decrypt_at(&mut self, counter: u64, ad: &[u8], ciphertext: &[u8], output: &mut [u8])
        -> Result<usize, HandshakeError>;
    pub fn session_id(&self) -> &SessionId;
}
```

Three properties the implementer must rely on rather than re-implement,
quoted from hiss's own docs:

- `next_counter` — "This is what lets a packet header that carries the
  counter — and is then fed back in as the seal's associated data — be
  constructed *before* the seal, without mirroring hiss-owned state. …
  A failed seal leaves it unchanged." **This is what makes §16.7's
  plan-seal-commit implementable**: the Data header (which contains the
  counter, and is the AD) is built in `plan`, the seal is `seal`, and
  neither mutates recovery state.
- `encrypt_next` — "Errors on `u64::MAX` (nonce exhaustion) … On any error
  the counter does **not** advance and nothing is written." That is
  §7.9's `NonceExhausted` trigger and §16.7's "on seal failure nothing
  moved", for free.
- `decrypt_at` — "On a `DecryptionFailed` error … `output` holds
  **unauthenticated** bytes that must not be read." The replay check must
  therefore come **after** a successful `decrypt_at`, never around it.

`SessionId` is hiss's type and slither's `src/` does not mention it
anywhere yet, while §16.2 requires `Connection::session_id() -> SessionId`.
See **U2**.

---

## 3. Spec digest — the connection core

*Everything in this section is quoted or paraphrased from `SPEC.md` with
the section number, so the implementer and the test author can work from
this plan without re-reading the spec. Line ranges are given so a
targeted `Read` can confirm any quote.*

### 3.1 §7.1 — the counter is the packet number (1844–1858)

> "The hiss `DatagramSend` counter **is** the packet number: monotonic,
> hiss-owned, never caller-chosen, simultaneously the AEAD nonce and the
> epoch selector. … It rides in cleartext because the receiver decrypts
> with it (§3.4)."
>
> "Packet-number spaces are **per direction, one per connection for its
> whole life** … the counter runs from 0 at establishment and never
> restarts … Every seal — payload, control, keepalive — burns the next
> counter."

Consequences slice 3 must honour:

- The connection core never chooses a counter. `seal` returns
  `(counter, bytes)` (§16.7) and that counter is the packet number used
  by the sent-packet map (slice 5) and the ACK (slice 5).
- There is **no second identifier**. Do not add a shell-side packet id.
- CLOSE, PING and keepalive all burn counters. A slice-3 test that counts
  counters must expect the CLOSE to consume one.

### 3.2 §7.2 — the anti-replay window (1859–1892)

| Constant | Value |
|---|---|
| `REPLAY_WINDOW` | **2048** bits, RFC 6479 sliding bitmap, `[u64; 32]`, 256 B per connection |

> "The window tracks a greatest authenticated counter plus the 2048-bit
> bitmap. The replay check is strictly **post-AEAD**: check-then-mark only
> after `decrypt_at` authenticates. A duplicate, or a counter more than
> 2048 behind the greatest, is dropped after decryption **without
> delivery**. The window's greatest advances only on authenticated
> counters."
>
> "**Liveness and roaming are driven only by packets that are both
> authenticated and window-marked** (fresh). No replayed packet ever moves
> the endpoint or refreshes liveness."
>
> "**[RATIFIED 2026/08/14]** The ACK record stays **fused** to this
> window (§12.2): the window is the single received-packet record — reuse,
> don't duplicate."

**What slice 3 owes slice 5.** Because the window *is* the ACK record,
slice 3's window type must be able to answer §12.2's derivation queries,
not merely `bool`. Slice 3 does not build the ACK frame's ranges, but if
the window is written as a private `bool` oracle it will be rewritten in
slice 5. See §12.2 digest at 3393–3410 (read in §3.9 below) and open
question **Q4**.

Ordering the implementer must not invert (this is the whole of §7.2's
value): **AEAD first, then window check, then window mark, then deliver.**
A window check before `decrypt_at` would let an off-path forger who
observed a cleartext counter poison the window.

### 3.3 §7.7 — the epoch ratchet (2465–2513) — **this is S23**

| Constant | Value |
|---|---|
| `REKEY_EPOCH_MSGS` | 65 536 (2¹⁶) messages per epoch |
| `MAX_EPOCH_JUMP` | 2 (hiss-fixed) |

> "Transport keys ratchet forward on a counter-derived schedule
> (`into_datagram_with_epoch`) … A message sealed at `counter` belongs to
> epoch `counter / REKEY_EPOCH_MSGS`; each direction ratchets
> independently; the counter is **never reset** by the ratchet; `2⁶⁴ − 1`
> is reserved for the `Rekey()` transform. Epoch `e`'s key is Noise §11.3
> `Rekey()` applied `e` times: `Rekey(k) = ENCRYPT(k, 2⁶⁴ − 1, empty,
> zeros[32])[0..32]`. The ChaCha20-Poly1305 vector, pinned by test:
> `REKEY(0³²) =
> 25ce5d37df19f3783185f2ffd5ab17fa3397c212f02d62fb1733e0b875b74c58`."
>
> "The receiver retains the current and immediately preceding epoch keys
> (straggler tolerance: one epoch back); anything older is refused, its
> key ratcheted away."
>
> "**Epoch death is subsumed by liveness.** … **Implementations must not
> chase epochs.**"

**The ratchet is hiss's, not slither's.** The crypto rule in `CLAUDE.md`
is absolute: every Noise operation flows through `hiss`. Slice 3's job is
(a) to construct the transport pair with `into_datagram_with_epoch` so
the schedule exists at all, (b) to **not** add any epoch-chasing recovery
path, and (c) to pin the REKEY vector and the epoch boundary by test. The
`Rekey()` arithmetic itself is hiss's; if hiss does not expose the epoch
schedule at the surface §7.7 names, that is a finding, not a licence to
reimplement it — see **Q1**.

### 3.4 §7.8 — one session per connection (2514–2527)

> "A connection has **exactly one session** for its whole life (§5.4). No
> transport state ever crosses a handshake: a completed handshake installs
> a **new connection** — fresh cipher states, replay window, counters,
> streams, flow-control ledgers, recovery and congestion state, liveness
> clocks, and indices … There is no survival matrix, no re-queue rule, and
> no rekey seam: session-scoped and connection-scoped are the same scope."

This is a **negative** requirement and it is the cheapest thing in slice 3
to get wrong by helpfulness. `core::Connection` must have no "re-install"
path, no session-swap, and `handle_endpoint_event(Install)` must remain
**exactly once** (§16.4). Slice 2a already made it one-shot; slice 3 must
not soften it while adding the transport pair.

### 3.5 §7.9 — nonce exhaustion (2528–2543)

> "Sealing at counter `2⁶⁴ − 1` is refused by hiss (§2.1). A seal failure
> is never silent and can never strand frames (plan-seal-commit, §16.7):
> it moves the connection to `ConnectionLost::NonceExhausted` —
> **connection death, with no rekey escape**."

Unreachable in practice (≈ 58 000 years at 10⁷ packets/s), so it is
tested by **injection**, not by counting. See test-design note **T7**.

### 3.6 §8 — the frame layer (2544–2809)

§8.1's varints are already `src/varint.rs` (slice 0). Slice 3 implements
§8.2–§8.7 for **CLOSE, PING and ACK only**, plus PADDING (see **U1**).

**§8.2 parse-then-apply (2569–2606) — the rule that shapes the codec:**

> "A sealed Data packet's plaintext is a concatenation of frames, parsed
> to the end of the plaintext (the AEAD gives the exact length; there is
> no packet-level length prefix). An empty plaintext is the keepalive and
> never reaches this layer (§7.5)."
>
> "**Parse the whole plaintext first, then apply.**"
>
> - "**Structural failure** — an unknown frame type, a truncated frame, a
>   varint overrunning the plaintext, a length field overrunning the
>   plaintext, a non-final extends-to-end frame (§8.4), or any per-frame
>   structural error case below — is a **signalled death**. **[RATIFIED
>   2026/08/14]** Nothing from the packet is applied (no ACK scheduling,
>   no state change beyond the already-performed replay mark), one trace
>   fires on `slither::frames`, and the connection emits CLOSE with
>   `PROTOCOL_VIOLATION` (the existing `0x01`, §15.3) and enters the
>   closing state (§15.2), surfacing `ConnectionLost::ProtocolViolation
>   { code }` (§18.1)."
> - "**Semantic violation** — a structurally valid frame whose application
>   would break protocol state … is a protocol violation by an
>   authenticated peer: the connection emits CLOSE with the matching error
>   code and enters the closing state (§15.2)."

Two things the implementer will otherwise get wrong:

1. **"No state change beyond the already-performed replay mark."** The
   replay mark happens *before* frame parsing and is **kept** even when the
   packet is structurally rejected. So the order in `handle_datagram` is:
   AEAD open → replay check → replay mark → parse whole plaintext → (on
   structural failure: trace, CLOSE(`PROTOCOL_VIOLATION`), closing) →
   otherwise apply.
2. **Parse-then-apply is literal.** The parser produces a `Vec<Frame>` (or
   a two-pass validation) before any handler runs. A streaming
   parse-and-apply loop is a spec violation that no slice-3 test with only
   CLOSE/PING/ACK will catch — see test-design note **T2**.

**§8.3 the frame table (2607–2630)** — reproduced whole, because slice 3
must reject everything it does not implement and the table is the
authority on what exists at all:

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
| `0x1c` | CLOSE | error_code, reason_len, reason | no | linger rule (§15.2) | §15 |
| `0x30`/`0x31` | DATAGRAM | [length (0x31 only)], data | yes | never | §11 |

> "`0x05` is *reserved*, not implemented: like any unknown type, receiving
> it is a structural failure — CLOSE with `PROTOCOL_VIOLATION` (§8.2)."

**§8.4, the three frames slice 3 builds:**

- **PADDING (`0x00`)** — "a single `0x00` byte, no fields; any number may
  appear anywhere. Not ack-eliciting, never retransmitted, no error
  cases."
- **PING (`0x01`)** — "the type byte alone. Ack-eliciting; never
  retransmitted (a lost PING is superseded by the next probe). No error
  cases."
- **ACK (`0x02`)** —
  ```
  type(0x02) ‖ largest(varint) ‖ ack_delay(varint, µs)
             ‖ range_count(varint) ‖ first_range(varint)
             ‖ range_count × [ gap(varint) ‖ range(varint) ]
  ```
  > "Structural errors (§8.2's structural class): `range_count` >
  > `MAX_ACK_RANGES` (64); any range descending below counter zero.
  > Semantic no-op (frame ignored whole, traced): `largest` above the
  > highest counter this session has sealed (§12.5)."
- **CLOSE (`0x1c`)** —
  ```
  type(0x1c) ‖ error_code(varint) ‖ reason_len(varint) ‖ reason(reason_len B)
  ```
  | Constant | Value |
  |---|---|
  | `CLOSE_REASON_MAX` | 256 B |

  > "One frame type — no transport/application split and no
  > offending-frame-type field. `reason` SHOULD be UTF-8 but is carried as
  > bytes. Not ack-eliciting; never loss-retransmitted — the linger's
  > reply rule is its reliability (§15.2). Structural error: `reason_len`
  > > 256 (§8.2). `close()` truncates its `reason` to `CLOSE_REASON_MAX`
  > at the handle (§16.2) — an implementation must not be able to
  > *produce* the over-length case it must kill on receipt."

**§8.5 coalescing and packing order (2763–2774):**

> "Within a packet the sender packs in this order: the ACK first (if
> owed), then control frames (credit grants, RESET_STREAM, CLOSE), then
> STREAM and DATAGRAM fill, then PING last if a probe still owes
> ack-eliciting content. At most one extends-to-end frame (¬LEN STREAM, or
> `0x30` DATAGRAM) per packet, in final position."

Slice 3 implements the ordering skeleton with the three frames it has:
ACK, then CLOSE, then PING last. The round-robin STREAM fill is slice 4.
**The packer's shape must be written so slice 4 inserts a stage, not
rewrites it** — see the module map's note on `packer.rs`.

**§8.6 the per-seal bound (2776–2781):**

> "hiss caps any one sealed message at `MAX_MESSAGE_LEN` = 65 535 B of
> ciphertext. Every slither seal is at most `MAX_PLAINTEXT` + 16 = 1186 B."

`MAX_PLAINTEXT` is already in `src/constants.rs` (slice 0). The packer
budgets against it.

**§8.7 retransmission classes and ack-eliciting (2783–2809):**

> "A packet is **ack-eliciting** iff it contains at least one
> ack-eliciting frame (§8.3's column). Only ack-eliciting packets enter
> the sent-packet map (§13.5); pure-ACK packets, CLOSE packets, and
> keepalives are never tracked and never occupy the congestion window
> (§14.5)."
>
> "Loss recovery retransmits **frames, never packets** (§13.5). Three
> classes: ranges … regenerate … **never** (PADDING, PING, ACK, DATAGRAM,
> CLOSE)."

Every frame slice 3 implements is in the **never** class. That is
convenient and it is also a trap: it means slice 3 can ship an
ack-eliciting classifier that is never exercised on a `regenerate` or
`ranges` frame. Pin the classifier as a **table-driven function over the
whole §8.3 table**, including the types slice 3 does not build, so slice 4
and 5 inherit a tested classifier rather than a two-arm match. See **T3**.

### 3.7 §15 — CLOSE and the connection lifecycle (3743–3855) — **this is S1's second half**

| Constant | Value |
|---|---|
| `CLOSE_LINGER` | 5 s |
| close-reply rate | ≤ 1 CLOSE per second |
| `CLOSE_REASON_MAX` | 256 B |

> "Only the **authenticated, in-seal** CLOSE exists — nothing
> unauthenticated can kill a connection; the reserved cleartext close
> packet type (`0x04`) stays dead." (§15.1)

**§15.2 semantics, the four rows slice 3 implements:**

> - "**Local close** — `close(error_code, reason)` (the handle truncates
>   `reason` to `CLOSE_REASON_MAX`, §8.4): emit CLOSE (sealed
>   `seal_quiet`, §7.4) and enter **closing** for `CLOSE_LINGER`. The
>   closing state retains **the seal capability, the receive cipher
>   states, and the replay window** (all stream, flow-control, recovery,
>   and congestion state may drop immediately …): a reply is owed only to
>   an **authenticated, window-fresh** inbound packet — never to a packet
>   that merely routed by `receiver_index` … — and is sent **to the
>   session's endpoint address** (the closing state does not roam; never
>   to the triggering packet's source). Replies are capped at one CLOSE
>   per second; at linger expiry (`CloseLinger` timer, §16.5), drop all
>   state. … A CLOSE **received** while closing moves the connection to
>   the reply-free draining behaviour below: two closing endpoints go
>   quiet rather than ping-ponging replies at 1 Hz for the linger."
> - "**Receiving an authenticated CLOSE**: surface
>   `ConnectionLost::PeerClosed { code, reason }`, emit **nothing**, hold
>   a brief drain for the same `CLOSE_LINGER` (discarding late packets, no
>   replies), then drop all state."
> - "**Protocol violations by the authenticated peer** … get a signalled
>   death instead of a silent one: emit CLOSE with the matching code, then
>   linger as for a local close. … The locally surfaced error is
>   `ConnectionLost::ProtocolViolation { code }` (§18.1) — a dedicated
>   variant, not `LocallyClosed`."

There are therefore **three distinct post-mortem states**, not two, and
they behave differently:

| State | Entered by | Emits on inbound | Timer | Local surface |
|---|---|---|---|---|
| **closing** | local `close()`, last-handle drop, protocol violation | ≤ 1 CLOSE/s reply, to the **session's endpoint address** | `CloseLinger` 5 s | `LocallyClosed` / `ProtocolViolation { code }` |
| **draining** | peer's CLOSE received | **nothing** | `CloseLinger` 5 s | `PeerClosed { code, reason }` |
| **closing→draining** | a CLOSE arrives while closing | nothing thereafter | the *existing* `CloseLinger`? or a fresh one? — **Q2** | unchanged |

**§15.3 error-code registry (3805–3818)** — reproduced whole; slice 3
introduces the type and slices 4–6 fill in the reachable codes:

| Code | Name | Meaning |
|---|---|---|
| `0x00` | `NO_ERROR` | graceful close |
| `0x01` | `PROTOCOL_VIOLATION` | a semantic violation with no more specific code |
| `0x02` | `FLOW_CONTROL_ERROR` | advertised credit exceeded (§10.5) |
| `0x03` | `STREAM_LIMIT_ERROR` | cumulative stream limit exceeded (§10.4) |
| `0x04` | `STREAM_STATE_ERROR` | a frame for a stream its sender could not touch (§8.4) |
| `0x05` | `FINAL_SIZE_ERROR` | final-size disagreement (§9.5, §9.6) |
| `0x06` | `MESSAGE_OVERFLOW` | ruling 52: the receiver-emitted overflow reset; carried in RESET_STREAM's `error_code`, **never in CLOSE** |
| `0x07`–`0x0f` | reserved | transport-reserved; never sent |
| ≥ `0x10` | application | application-defined codes via `close()` |

**§15.4 the teardown matrix (3819–3854)** — the eight rows, with which
slice owns each:

| Cause | Transmitted | Local surface | Peer's view | Slice |
|---|---|---|---|---|
| liveness — 25 s without an authenticated fresh receive (§7.5) | nothing | `TimedOut` | its own liveness fires ≈ symmetrically | **7** (§7.4) — see **C1** |
| contested — probe unanswered at `KEEPALIVE_TIMEOUT` after transmission | the probe's one PING | `TimedOut` | asymmetric | **7** |
| nonce exhaustion (§7.9) | nothing | `NonceExhausted` | liveness | **3** |
| local `close(code, reason)` / last-handle drop (§16.2) | CLOSE, then ≤ 1 reply/s for 5 s | `LocallyClosed` | `PeerClosed { code, reason }` | **3** |
| peer's CLOSE received | nothing (drain only) | `PeerClosed { code, reason }` | (it closed) | **3** |
| protocol violation by the peer (§8.2, §15.2) | CLOSE(code), linger | `ProtocolViolation { code }` | `PeerClosed { code, reason }` | **3** |
| replaced — an `Intro` proving this connection's static was accepted | nothing on the old connection | a fresh `Intro`, then `Replaced` at its `accept()` | (it reconnected) | **2a core / 3 shell** |
| endpoint dropped — every handle gone (§16.3) | nothing | — (the driver stops) | liveness, ≤ 25 s | **3** — this is S26 |

> "`ConnectionLost::EndpointDropped` is the answer a surviving verb call
> receives when the driver has stopped mid-flight (§18.1) — it is a
> handle-side observation, not a teardown cause of its own."

### 3.8 §16.4 — the poll contract on both cores (4354–4534)

> "**every mutating call** (`handle_datagram`, `handle_timeout`, verb
> calls, stream/datagram/message operations, `connect`) **is followed by
> draining `poll_output()` to the terminal `Timeout(Option<Instant>)`**,
> which is simultaneously the drain sentinel and the next-deadline
> announcement — a driver cannot forget to drain."

The `core::Connection` surface, as §16.4 states it:

```rust
impl core::Connection {
    fn handle_datagram(&mut self, now: Instant, src: SocketAddr, datagram: &[u8]);
    fn handle_timeout(&mut self, now: Instant);                        // idempotent
    fn handle_endpoint_event(&mut self, now: Instant, ev: Install);    // Install only
    fn open(&mut self, dir: Dir) -> Result<StreamId, StreamsExhausted>;
    fn write(&mut self, now: Instant, id: StreamId, data: &[u8]) -> Result<usize, WriteError>;
    fn finish(&mut self, id: StreamId) -> Result<(), WriteError>;
    fn reset(&mut self, now: Instant, id: StreamId, error_code: u64);
    fn read(&mut self, id: StreamId, buf: &mut [u8]) -> Result<Option<usize>, ReadError>;
    fn send_message(&mut self, now: Instant, msg: &[u8]) -> Result<(), MessageError>;
    fn send_datagram(&mut self, now: Instant, data: &[u8]) -> Result<(), DatagramError>;
    fn close(&mut self, now: Instant, code: u64, reason: &[u8]);
    fn accept(&mut self, dir: Dir) -> Option<StreamId>;
    fn recv_message(&mut self) -> Option<Vec<u8>>;
    fn recv_datagram(&mut self) -> Option<Vec<u8>>;
    fn poll_output(&mut self) -> ConnOutput;
}

enum ConnOutput {
    Transmit(Transmit),
    Event(ConnEvent),
    ToEndpoint(ToEndpoint),
    Timeout(Option<Instant>),                    // terminal
}
enum ToEndpoint {
    Retired { our_index: u32 },                  // teardown: drop the index route (MUST)
}
```

`ConnEvent` has fourteen variants (4431–4445). Slice 3 can *emit* only
`Established` and `Closed(ConnectionLost)`; the rest are slices 4–7.

**Slice 3 adds only `Closed(ConnectionLost)`, and that is deliberate.**
`src/core/connection/mod.rs`'s own doc states the precedent:

> "The variants §16.4 lists that slice 2a cannot yet construct — every
> `ConnEvent` but `Established` — are absent rather than stubbed: an
> uninhabited variant is a claim about the protocol, and these will each
> arrive with the section that defines them."

`ConnOutput` and `ConnEvent` are `pub(crate)`, so growing them is not a
breaking change and working rule 8's "declare the list whole" instinct
does **not** apply here. Follow the precedent. (The `pub` types that *are*
frozen — `ConnectionLost` and the rest of §18.1 — are already complete in
`src/error.rs`; ruling 61 makes every one but `WriteError` exhaustive, so
they cannot grow.)

The four rules of §16.4 that slice 3 is the first slice able to violate:

1. > "**`accept()` returns a fully established connection — never followed
   > by an `Install`.** `Install` targets only a `connect()`-created
   > connection awaiting completion, **exactly once** … Emitting a
   > symmetry `Install` after `accept()` (double-install) and waiting for
   > one that never comes are both excluded."
2. > "**`HandshakeFailed` never reaches `core::Connection`**: the shell
   > resolves `Connecting` with `Err(ConnectError::TimedOut)` and drops
   > the never-established pending core."
3. > "**`Retired` is a MUST**: every terminal `ConnOutput` — a
   > `Closed(ConnectionLost)` event, or the completion of the close
   > linger — is followed **within the same drain** by
   > `ToEndpoint::Retired`, and the shell delivers it to
   > `handle_connection_event` **before** releasing the connection's
   > shell-side bookkeeping (else the index route and the guard-entry pin
   > leak for the endpoint's life). The all-handles-dropped case is
   > exempt (the driver simply stops)."
4. > "**Output ordering within one drain preserves generation order** — a
   > transmit and the event it caused come out in that order. Normative;
   > tests and logs depend on it."

Rule 3 is the single densest correctness obligation in the slice and it is
what the post-slice-3 seam review is chartered to examine (`PLAN.md` §5
names "`Retired` ordering" explicitly). See **T5**, **Q1** and **C4**.

### 3.9 §16.5 — time and timers (4535–4596)

> "**`now: Instant` is an explicit argument on every mutating call**; the
> cores never read a clock. … `poll_output` takes no `now`."
>
> "**Named timers, single min-deadline out.** The connection core's timer
> table: `Keepalive`, `PersistentKeepalive`, `Liveness`, `Loss`, `Pto`,
> `AckDelay`, `CloseLinger`, `Contested`."
>
> "**`handle_timeout` is idempotent**: each due timer is stopped before its
> logic runs, so spurious or repeated calls no-op. For `Loss`/`Pto` the
> idempotency additionally rests on synchronous sealing (§16.7)."

**Equal-deadline priorities, normative and — since ruling 76 — declared
exhaustive:**

> "**[RATIFIED 2026/08/15 — ruling 76]** This list is **exhaustive** …
> The governing principle …: **a terminal outcome precedes a routine one,
> and state removal precedes emission.** At the endpoint that gives, in
> order — (1) handshake **give-up**, (2) **intro expiry**, (3)
> **guard-orphan aging**, (4) **retransmit**. … Per connection, loss
> detection beats PTO and exactly one of the two fires per evaluation;
> teardown collection (liveness, `CloseLinger` expiry, then `Contested`)
> precedes keepalive evaluation … `AckDelay` fires after the loss/PTO
> evaluation at the same instant …; and `PersistentKeepalive` is
> evaluated last."

The connection's total order, written out, is therefore:

1. `Liveness` (slice 7)
2. `CloseLinger` expiry ← **slice 3**
3. `Contested` (slice 7)
4. `Loss` **xor** `Pto` (slice 5) — exactly one per evaluation
5. `AckDelay` (slice 5)
6. `Keepalive` (slice 7)
7. `PersistentKeepalive` (slice 7)

Slice 3 arms exactly one of these — `CloseLinger` — but **must build the
ordering as the ordering**, i.e. a single `next_deadline()` that consults
the named timers in the ratified order, not an `Option<Instant>` field
called `close_linger_deadline`. Otherwise slice 5 and 7 re-derive the
order from prose and one of them gets it wrong. See **T6**.

**The lateness bound:**

| Parameter | Value |
|---|---|
| `L` (shell lateness bound) | 250 ms |

> "Every armed deadline `D` fires no earlier than `D` and no later than
> `D + L`. `L` is a **conformance parameter of the shell, not of the
> protocol**: the cores expose exact deadlines, and a shell may batch or
> tick provided it honours `L`."

`L` binds the **driver**, which slice 3 builds. It is the first slice
where `L` is testable at all. See **T8**.

### 3.10 §16.6 — RNG (4597–4614)

> "The endpoint core owns one seeded RNG … Every index, jitter draw, and —
> via the forced increment — timestamp draw comes from it. At connection
> creation the endpoint draws a 32-byte **sub-seed** for the connection
> core (drawn even while unused, so later connection-side randomness
> cannot perturb the endpoint's draw order). One root seed reproduces the
> whole system."
>
> "Session and pending indices MUST be unpredictable to an off-path
> observer … so the config-supplied seed is a **test-only facility**."

Slice 2a already draws the sub-seed and stores it; slice 3 is the first
slice that **uses** it. `.slices/02-handshake/OPEN-QUESTIONS.md` §C lists
the draw order as deliberately untested — slice 3 closes that gap, because
a wrong draw order now becomes observable as a diverging reproduction from
one root seed. See **T9**.

### 3.11 §16.7 — plan-seal-commit (4615–4629)

> "Packetisation is **plan, seal, commit**: build the packet plan, seal
> it, and **only on seal success** commit the recovery transition
> (dequeue, mark transmitted, clear the pending ACK, `on_sent`, arm
> timers). On seal failure nothing moved — a seal error can never strand
> frames outside both the pending set and the loss tracker; the only
> reachable seal failure is nonce exhaustion, which is terminal (§7.9).
> Seal returns `(counter, bytes)`. Sealing — commit included — executes
> **within the mutating call that triggers it** (`handle_timeout`,
> `handle_datagram`, the application surface), never lazily inside
> `poll_output()`; this is what makes `Loss`/`Pto` idempotency real."

This is a **structural** requirement on the connection core, and it is
easiest to satisfy at slice 3 when there is almost nothing to commit —
which is exactly why it belongs here rather than in slice 5 where the
commit list is long. The three-phase function is:

```text
plan(now)   -> Option<PacketPlan>     // pure; consults timers, pending frames, budgets
seal(plan)  -> Result<(u64, Vec<u8>), SealError>   // hiss DatagramSend
commit(plan, counter, now)            // dequeue, clear pending ACK, on_sent, arm timers
```

`poll_output()` then only *hands out* the already-sealed `Transmit`. An
implementation that seals inside `poll_output()` will pass every slice-3
test — nothing in slice 3 retransmits — and fail in slice 5. See **T4**.

### 3.12 §12.2 — what §7.2's window owes the ACK derivation (3393–3410)

*(Read targeted, only to size the window's API — the ACK frame itself is
slice 5.)*

> "An ACK is derived from the replay window's snapshot (greatest + bitmap,
> §7.2) — the single received-packet record; there is no second tracker.
> Because the 2048-bit worst case (alternating) no longer fits one packet,
> construction emits ranges **newest-first, descending**, truncating at
> `MAX_ACK_RANGES` pairs or at packet capacity, whichever binds."
>
> | Constant | Value |
> |---|---|
> | `MAX_ACK_RANGES` | 64 |
>
> "A received ACK with `range_count` > 64 is malformed — a structural
> failure of §8.2's class."

So the window's slice-3 API must expose **`greatest` and the bitmap** in a
form a descending range walk can consume, not merely
`fn seen(&self, counter: u64) -> bool`. That is the whole of slice 3's
obligation to slice 5, and it is one method signature. See **Q4**.

### 3.13 §7.4 — the part slice 3 cannot avoid (1947–1996)

The slice table gives §7.4 to slice 7. **It cannot all wait**, because
§15.2 seals CLOSE with `seal_quiet` and `seal_quiet` is §7.4's:

> "The liveness clock is driven by **application intent only**. Two seal
> paths, identical on the wire (same sealed-Data packet, same counter
> increment):
> - **`seal`** marks `last_send`: packets carrying at least one
>   first-transmission STREAM frame or DATAGRAM frame (fresh application
>   sends), and the keepalive (§7.5).
> - **`seal_quiet`** does not touch `last_send` — the **quiet set**: pure
>   ACKs, PTO probes, retransmissions, the credit frames …, RESET_STREAM,
>   and CLOSE."
>
> "A send **arms** the death deadline if **either** it is a marking send …
> **or** it carries any ack-eliciting frame … the deadline arms on the
> *first* arming send after a receive, is **not** re-armed by subsequent
> sends of either kind, and is reset by every authenticated, window-fresh
> receive (§7.2)."
>
> "**At install the clock is pinned, and it is pinned *armed*.** A newly
> installed session (§5.4) sets both `last_authenticated_recv` and
> `last_send` to the install instant and starts with the death deadline
> **already armed** … which is what makes 'a half-open session is reaped
> by liveness in 25 s' a fact rather than an implementation choice."

This is reported as conflict **C1** and carried into the cut
recommendation: I propose slice 3 take §7.4's **two seal paths, the
arming rule, the install pin, and the `Liveness` timer**, and leave slice
7 §7.5's keepalive, persistent keepalive and contested probe. The reasons
are in §7 below.

---

## 4. Spec digest — the shell

### 4.1 §16.1 — the object model (3858–3907)

```text
Endpoint                                   // socket + demux; owns the accept queue
├── connect(addr, static) → Connecting     // Future → Connection
├── accept().await → Intro → Claimed → Proven → Connection    (§6)
└── Connection                             // one Noise session + the frame layer
      ├── SendStream / RecvStream          // per-stream handles (bidi pairs)
      ├── send_message / recv_message      // §9.8 sugar
      ├── send_datagram / recv_datagram    // §11
      ├── acked / SendStream::acked        // delivery confirmation (§16.2)
      └── closed / notified                // the awaitables (§16.2)
```

**One connection per remote static, endpoint-wide** (ratified):

> "`connect()` to a static with a live `Connection` or an in-flight
> outbound connect returns `ConnectError::AlreadyConnected`; an
> authenticated inbound initiation whose proven static matches a live
> connection is, by definition, a **replacement of that connection,
> admitted via `accept()`** (§5.4, §6.4) — never a second concurrent
> connection."
>
> "An outbound connect is 'in flight' only while its `Connecting` lives:
> dropping that future cancels the attempt, the static returns to NONE,
> and an immediate `connect()` to it succeeds (§16.3, ruling 50)."
>
> "A **staged chain in progress is deliberately not in `connect()`'s
> list**, and cannot be: until `authenticate()` the chain's static is
> merely claimed, and §6.1 forbids keying anything durable on an unproven
> claim."

The static→state map (NONE / PENDING / LIVE) is **endpoint-core state**
that slice 2a partly built; slice 3's shell must not maintain a second
copy. See **U6**.

### 4.2 §16.2 — the shell surface (3909–4155)

The `Endpoint` surface:

```rust
impl Endpoint {
    pub fn builder() -> EndpointBuilder;                 // identity, socket/Wire, Config
    pub async fn accept(&self) -> Option<Intro>;         // None = endpoint closed
    pub fn connect(&self, remote: SocketAddr, remote_static: PublicKey)
        -> Result<Connecting, ConnectError>;
}
```

Note that **`connect` is not `async`**: it returns
`Result<Connecting, ConnectError>` synchronously, and `Connecting` is the
future. `AlreadyConnected` therefore arrives *before* any await. This is
awkward against ruling 53's "endpoint verbs are a command channel plus
oneshot": a synchronous `connect()` cannot round-trip to the driver.
Reported as **C2**, with a provisional decision in **Q5**.

The `Connection` surface (slice 3 builds the marked subset):

```rust
impl Connection {
    pub async fn open_bi(&self)  -> Result<BiStream, ConnectionLost>;      // slice 4
    pub async fn open_uni(&self) -> Result<SendStream, ConnectionLost>;    // slice 4
    pub async fn accept_bi(&self)  -> Result<BiStream, ConnectionLost>;    // slice 4
    pub async fn accept_uni(&self) -> Result<RecvStream, ConnectionLost>;  // slice 4
    pub async fn send_message(&self, msg: &[u8]) -> Result<(), MessageError>;   // slice 6
    pub async fn recv_message(&self) -> Result<Vec<u8>, ConnectionLost>;        // slice 6
    pub fn send_datagram(&self, data: &[u8]) -> Result<(), DatagramError>;      // slice 6
    pub async fn recv_datagram(&self) -> Result<Vec<u8>, ConnectionLost>;       // slice 6
    pub async fn acked(&self) -> Result<(), ConnectionLost>;                    // slice 5
    pub async fn close(&self, code: u64, reason: &[u8]);                   // ← SLICE 3
    pub fn set_persistent_keepalive(&self, interval: Option<Duration>)
        -> Result<(), ConfigError>;                                        // slice 7
    pub async fn closed(&self) -> ConnectionLost;                          // ← SLICE 3
    pub async fn notified(&self) -> Result<Notification, ConnectionLost>;  // slice 7
    pub fn remote_static(&self) -> PublicKey;                              // ← SLICE 3
    pub fn remote_address(&self) -> SocketAddr;                            // ← SLICE 3
    pub fn session_id(&self) -> SessionId;                                 // ← SLICE 3
    pub fn is_established(&self) -> bool;                                  // ← SLICE 3
}
```

> "`close()` resolves once the CLOSE frame is sealed and the closing state
> is entered (§15.2), and truncates `reason` at `CLOSE_REASON_MAX` (§8.4)."

**`closed()` (ruling 46) — the S27 half slice 3 owes:**

> "**`closed()`** resolves with the `ConnectionLost` that ended the
> connection, for whatever reason — every row of §15.4's teardown matrix.
> It is a **latched** signal, not a queue: cancel-safe, awaitable
> concurrently from any number of tasks, and after death it resolves
> immediately and for ever — which asks only that `ConnectionLost` be
> `Clone`, a derive, not a variant change; §18.1 stays closed. On a
> healthy connection it never resolves, which is what makes it the
> `select!` arm of a long-running loop."

Four testable properties in that one paragraph, and all four are cheap to
get wrong: **latched** (not one-shot), **concurrent from any number of
tasks**, **cancel-safe**, **resolves immediately and for ever after
death**. See **T10**.

`notified()` is slice 7's, but the plan must not let slice 3 build a
`closed()` that makes `notified()` impossible. §16.2's retention rule —
"**Retention is one slot per kind**" with the oldest-`from`/newest-`to`
merge for `AddressMoved` — means the shell's per-connection bookkeeping
needs a notification-slots field from the start even if nothing fills it.
Declaring the empty struct now costs nothing; discovering in slice 7 that
`closed()` was built as the only shell-side event path costs a rewrite.

**Drop semantics (4138–4154) — this is S26 and S29:**

> "Dropping a staged object is a silent reject (§6.2). Dropping a
> `Connecting` **cancels the outbound attempt** … Dropping the last handle
> to a `Connection` performs `close(NO_ERROR, "")` — the graceful teardown
> of §15.2 … Dropping a `SendStream` without `finish()` resets it with
> error code 0. Dropping a `RecvStream` abandons the receive half …
> **Dropping every handle stops the driver and every connection dies
> silently — nothing transmitted (§15.4).**"

The last two sentences are in tension and the tension is the whole of
S26. Reported as **C3**.

### 4.3 §16.3 — driver and handle lifetimes (4156–4353)

> "The shell is **one `!Send` driver task**, spawned with
> `tokio::task::spawn_local` (a `LocalSet` is required), owning the socket
> for both receive and send and owning both sans-io cores. **Do not add
> `Send` bounds to the actor path** … `Endpoint`, staged objects,
> `Connection`, and stream handles are thin clients over the driver's
> state; connections do not send on socket clones. … The driver lives
> while any handle lives; dropping every handle stops it, and every
> session dies silently with it."

**Ruling 53's seam split (4169–4200)** — already ruled, not slice 3's to
revisit; slice 3 implements it:

| Surface | Mechanism |
|---|---|
| Endpoint verbs — `connect`, `accept`, and §6.2's three staged verbs | command channel + oneshot reply |
| Connection data path — `write`, `read`, `open_*`, `accept_*`, `send_message`, `recv_*`, `close`, `acked`, `notified` | shared cell (`Rc<RefCell<_>>`) with the driver, plus the blocked-readers / blocked-writers waker maps of §16.8 |
| Accessors — `remote_static`, `remote_address`, `session_id`, `is_established` | reads of that same shared cell (§16.8, unchanged) |

> "**Every mutating borrow ends by marking the connection dirty and waking
> the driver**, which drains `poll_output()` to `Timeout` and performs the
> I/O — §16.4's drain-after-every-mutating-call contract is untouched, it
> is merely not always the driver that made the call."
>
> "Each data-path verb is written **once**, as `poll_*(&mut self, cx) ->
> Poll<_>`; §16.2's `async fn` is then `poll_fn(|cx| self.poll_*(cx,
> ..)).await`, and §16.11's `AsyncRead`/`AsyncWrite` is the *same
> function* with its error mapped."

**Ruling 62 — a `Connecting` *is* a handle (4213–4230):**

> "The driver lives while a `Connecting` lives, and dropping the last
> `Connecting` — with no `Endpoint` and no `Connection` outstanding —
> stops it. … a `Connecting` **owns an in-flight protocol attempt** … while
> a `closed()` future owns nothing and merely observes. A future that
> changes protocol state when dropped is a handle; one that does not, is
> not."
>
> "The consequence is that **`ConnectError` needs no `EndpointDropped`** …
> The asymmetry with `IntroError`, `AuthError` and `AcceptError` — which
> all carry `EndpointDropped` — is therefore correct and not an omission."

`closed()` is explicitly **not** a handle:

> "They are not a reason to keep a connection alive: a `closed()` future
> is not a handle, and holding one while dropping every `Connection` still
> stops the driver and still kills the session silently."

**Ruling 50 — dropping a `Connecting` cancels (4232–4245), the S29 half:**

> "§5.5's retransmit train stops, the pending and its pending-index entry
> are dropped (§17.3), its dialled address leaves §6.5's hint set with
> them (§17.4), and the static leaves **PENDING** for **NONE** (§5.4).
> Nothing is transmitted … A subsequent `connect()` to that same static
> therefore **succeeds** … An implementation **MUST** order the
> cancellation ahead of any endpoint verb the application issues after the
> drop returns, so an immediate redial cannot observe the corpse;
> Appendix B pins that on the paused clock."

The **MUST** in that last sentence is the hardest ordering requirement in
the shell, because `Drop` is synchronous and the endpoint verbs are a
channel round-trip. If cancellation is delivered as a *message* on the
same command channel the redial uses, FIFO gives the ordering for free —
and that is the only construction that does. See **Q6** and **T11**.

**The `Wire` trait (4295–4322)** — already built in slice 0. Four
normative properties, restated because the driver is the first consumer:
application-supplied via `Endpoint::builder()`; **not `Send`**, and no
`Send` bound may be added to it or to its futures; `&self` on both methods
because one driver task owns the seam; `testutil::FlakyWire` is a `Wire`.

**A failing `send_to` is traced, not acted on (4324–4352), ruling 49:**

> "When `send_to` returns an `Err`, the driver **MUST** trace it against
> the connection whose datagram it was, under §18.2's operator contract,
> carrying the destination address and the underlying error. It does
> **not** kill the connection, resolve any verb with an error, or produce
> a `Notification`."

This is a **driver** obligation and slice 3 builds the driver, so slice 3
owes it — including the "against the connection whose datagram it was"
part, which means the driver's send path must carry a connection id
alongside the `Transmit`. `EndpointOutput::Transmit(Transmit)` (§16.4)
carries no connection id, and `ConnOutput::Transmit(Transmit)` is already
scoped to a connection. Reported as **U7**.

### 4.4 §16.8 — the no-blocking invariant (4630–4644)

> "Every driver→handle delivery is a bounded channel with a non-blocking
> policy or a oneshot reply that cannot block the driver. The accessors are
> **synchronous reads of a shared cell the driver updates** — never driver
> round-trips. Per-stream wakers key the shell's blocked-readers/
> blocked-writers maps (the quinn pattern) … The driver never performs a
> blocking send toward a handle. The shell is deadlock-free by
> construction. One class is exempt from the non-blocking drop policy by
> prohibition: **reliable data is never droppable** — stream bytes,
> messages, and claim-pending receive state stay in the core until the
> application takes them (§16.4's pull model, §10.6); the bounded channels
> carry signals and wakes, not reliable payloads."

### 4.5 §16.9 — early sends (4646–4663)

> "Queued work before establishment is **ordinary work**: the connection
> core exists from `connect()`, and early stream opens, writes, messages,
> and datagrams land in ordinary stream/queue state, pumping when a session
> installs — delivered exactly once after establishment, lost if the
> connect fails (the failure surfaces through `Connecting`). There is no
> special pre-establishment mechanism."
>
> "**Stream identity before establishment.** … **stream IDs are assigned
> at establishment**: pre-establishment handles hold core-internal indices,
> no frame is emitted before install (nothing sends until a session
> exists), and `id()` returns `None` until the connection is established
> (§16.2). On install the core maps its internal indices onto the parity
> the outcome dictates, in open order."

Slice 3 has no streams, so the *stream-ID* half is slice 4's. But the
**first** sentence binds slice 3: `close()` on a not-yet-established
connection must be ordinary work, not a special case. What does it do? The
spec does not say, and it is reachable in one line
(`let c = ep.connect(..)?; drop(c);` is cancel, but
`select!`-then-`close()` on a `Connection` that has not resolved is not
constructible — the handle does not exist until `Connecting` resolves).
Recorded as **U8**, likely a non-issue but stated so it is not discovered
in slice 4.

### 4.6 §16.10 — kernel-free drivability (4665–4701)

> "The whole protocol is drivable without a kernel: two endpoints over
> `testutil::FlakyWire` on tokio's **paused clock**, with every timer — the
> 5 s/10 s/15 s/25 s/25 ms/90 s family included — resolving in virtual
> time. New behaviour gets a paused-clock flow test, not a sleep
> (Appendix B)."
>
> "**[RATIFIED 2026/08/14 — ruling 60]** … These three names
> [`testutil::Network`, `testutil::FlakyWire`, `testutil::FlakyPolicy`]
> are **contract**, on the same terms as §18.2's trace targets: renaming
> or dropping one is a protocol revision."

All three exist (slice 0). Slice 3 is the first slice where the fixture
carries a **driver** rather than being poked directly, so the fixture will
need a `LocalSet`-based harness. That harness is new `testutil` surface
and it is **not** in ruling 60's contract list — so it may be named
freely, but it must not shadow the three contract names. See the module
map.

---

## 5. Story-to-test mapping

Every row is a **paused-clock** test. `PLAN.md` §4: *"a slice is not
finished when its code compiles, but when its stories are paused-clock
tests that pass."*

### 5.1 Slice 3a

| Story | Test | File | What it drives |
|---|---|---|---|
| **S23** — a long-lived connection rekeys itself without the user noticing | `s23_epoch_ratchets_without_handshake` | `src/core/tests.rs` | Two connection cores over the existing two-endpoint fixture. Drive the send counter across the `REKEY_EPOCH_MSGS` boundary (hiss's `set_counter_for_test` is `#[cfg(test)]` **inside hiss** and so unavailable — drive it by sealing, or via a test-only `epoch_size`; see **Q7**). Assert: packets either side of the boundary both open; **no handshake packet is emitted** across it (the `Network` tap sees zero `PKT_HANDSHAKE_INIT`/`_RESP`); **no `ConnEvent` at all** is emitted; the counter does **not** reset (§7.7). |
| **S23** (straggler half) | `s23_one_epoch_back_opens_two_back_does_not` | `src/core/tests.rs` | A packet from epoch *e−1* arriving after epoch *e* committed still opens; one from *e−2* does not, and its failure is an ordinary decryption failure with **no epoch-specific recovery** (§7.7: "implementations must not chase epochs"). |
| **S1 precursor** *(named a precursor, not S1)* | `core_close_round_trip_precursor_s1` | `src/core/tests.rs` | Dial → install → `close(code, reason)`; the peer core surfaces `ConnEvent::Closed(PeerClosed { code, reason })`, ours `LocallyClosed`. **This is not S1** — S1 needs handles, and naming this S1 would let 3b ship without the story test S1 actually asks for (working rule 9: a name is not a pin). |

### 5.2 Slice 3b

| Story | Test | File | What it drives |
|---|---|---|---|
| **S1** — a user can open a connection and close it | `s1_dial_then_close_both_sides_observe` | `tests/story_lifecycle.rs` | `connect(addr, static)` → `Connecting` → `Connection`; `close(code, reason)` resolves once the CLOSE is sealed; the peer surfaces `ConnectionLost::PeerClosed { code, reason }` with the same code and reason; our side sees `LocallyClosed`. **Assert the 4-DH initiator cost** (`CountingIdentity`/`DhCounter` already exist) — S1's `Cost:` line is part of the story. Assert `reason` is truncated at `CLOSE_REASON_MAX` when over-long. |
| **S2** (shell half) — dial a peer that never answers | `s2_no_answer_gives_timed_out_at_giveup` | `tests/story_dial.rs` | `Connecting` resolves `Err(ConnectError::TimedOut)` at `HANDSHAKE_GIVEUP` (90 s) — **"not before and not never"**, and both halves of that clause must be asserted (see **T12**). §5.5's retransmit schedule is already pinned in slice 2a's core tests; 3b asserts only that the shell surfaces the give-up. |
| **S26** — dropping every handle tears everything down cleanly | `s26_last_handle_drop_stops_the_driver` | `tests/story_lifecycle.rs` | Drop the `Endpoint` while a `Connection` lives: the driver **keeps running** and the connection stays usable. Then drop the `Connection`: the driver stops, the `LocalSet` completes, nothing leaks. Assert both drop orders. |
| **S26** (ruling 62's clause) | `s26_a_connecting_alone_keeps_the_driver_alive` | `tests/story_lifecycle.rs` | With no `Endpoint` and no `Connection`, a live `Connecting` keeps the driver running; dropping it stops it. And the negative: holding a `closed()` future while dropping every `Connection` does **not** keep it alive (§16.3, 4209–4211). |
| **S26** (the wire clause) | `s26_last_handle_drop_transmits_nothing` | `tests/story_lifecycle.rs` | §15.4's endpoint-dropped row: **nothing transmitted.** The `Network` tap records zero datagrams after the drop. This is the assertion that separates the row from the `close()` row, which does transmit. See **C3**. |
| **S27** (`closed()` half) | `s27_closed_resolves_with_no_verb_in_flight` | `tests/story_lifecycle.rs` | Appendix B verbatim: *"Park a task on `closed()` and nothing else — no `read`, no `recv_message`, no send — and drive each teardown row in turn."* Rows reachable in slice 3 (with amendment 1): `LocallyClosed`, `PeerClosed`, `ProtocolViolation`, `TimedOut`. **The unreached rows are named in the file's module doc**, so the gap is coverage rather than a claim (**C5**). |
| **S27** (the latch) | `s27_closed_is_latched_and_concurrent` | `tests/story_lifecycle.rs` | Appendix B: *"a second `closed()` after death resolves immediately and with the same value, a `closed()` first awaited **after** the death resolves too, and several concurrent `closed()` futures all resolve."* Three separate assertions; the third needs ≥ 3 concurrent tasks (**T10**). |
| **S29** (shell half) | `s29_cancel_then_immediate_redial` | `tests/story_lifecycle.rs` | Appendix B verbatim: dial a peer that never answers; drop the `Connecting` at *t* = 5 s **through `tokio::time::timeout`, so the test is the idiom**; assert no further msg1 leaves after the drop, and an **immediate** `connect()` to the same static returns `Ok` — **with no advance of the clock between the drop and the redial**, which is what pins the cancellation ahead of the next endpoint verb. |
| **S29** (NONE routing) | `s29_after_cancel_the_static_routes_as_none` | `tests/story_lifecycle.rs` | An initiation from that peer after the drop takes the ordinary staged accept and never §6.4's PENDING branch. |
| **S29** (no accumulation) | `s29_retry_loop_replaces_rather_than_accumulates` | `tests/story_lifecycle.rs` | N cycles of dial → cancel → redial leave the responder with exactly **one** connection per cycle: 4 DH and one `ConnectionLost::Replaced` per cycle, bounded by §16.1. **May not be fully reachable in slice 3** — it needs the shell's `Replaced` delivery. If it is not, the file says so and names the slice that owes it. |

**Not reachable in slice 3, each to be *named* in the test file rather
than silently absent:** Appendix B's notification-retention obligations
(slice 7); its delivery-confirmation / `acked()` obligations (slice 5);
the "traffic resumes when the seam heals" half of the send-failure
obligation (needs data frames, slice 4) — though its **trace** half is
slice 3's by ruling 49; S29's peer-side half-open reap at `DEAD_TIMEOUT`
(reachable **iff** amendment 1 lands — attempt it).

---

## 6. The unstated-scope hunt (candidate rulings)

Working rule 8: *a stated construction with an unstated or contradicted
scope* is this project's most productive defect class — eleven instances
across three slices, none of them a wrong value. §8's frame table, §15's
teardown matrix and §16.5's timer table are all **lists**, and a list in
this spec reads as exhaustive whether or not it says so. Ten candidates,
ranked by what a wrong answer costs.

### U1 — "CLOSE/PING/ACK only" excludes PADDING, and cannot

`PLAN.md` §4's slice-3 row reads "§8 frame codec with **CLOSE/PING/ACK
only**". §8.2 makes an unknown frame type a **structural failure** — CLOSE
with `PROTOCOL_VIOLATION`. PADDING (`0x00`) is a *known* type of which
"any number may appear anywhere" (§8.4), not ack-eliciting, no error
cases. A codec that implements the list literally therefore **kills a
connection on receipt of a legal PADDING byte.**

*Provisional:* slice 3 implements **four** frames — PADDING, PING, ACK,
CLOSE. PADDING is two lines and the alternative is a wire-visible bug. The
list is understated, not wrong. (Plan-level rather than spec-level, but it
changes what slice 3 builds, so it is a decision the maintainer should
make rather than an implementer.)

*Second-order, and easy to miss:* the same reading forces slice 3 to
short-circuit the **empty plaintext** before the frame parser. §3.4:
*"An **empty plaintext** (16-byte tag-only ciphertext; a 30-byte datagram)
is the **keepalive** — it bypasses the frame layer entirely and is the
only non-frame plaintext (§7.5)."* §8.2 repeats it. Keepalive *sending* is
slice 7; keepalive *receiving* must not parse as zero frames and must not
be an error — and retrofitting the short-circuit in slice 7 means editing
the slice-3 receive path.

### U2 — §16.2 returns a `SessionId` the spec never defines

`SessionId` appears in `SPEC.md` **exactly once** — line 3941,
`pub fn session_id(&self) -> SessionId;` — and nowhere else: no
definition, no width, no home section, no row in the Named-constants
table. `src/` does not mention it at all. hiss defines
`hiss::noise::SessionId`, reachable from both `DatagramSend::session_id()`
and `DatagramRecv::session_id()`, documented as "the same value on both
halves and on the peer".

The ambiguity is real, because slither has a second plausible referent:
the session **index** (`our_index`/`peer_index`, a nonzero `u32`, §17.3),
which is what an operator correlating a packet capture would reach for.

*Provisional:* re-export hiss's `SessionId`. It is the only candidate the
spec's own name matches; it is peer-agreed, which an index is not (the two
directions carry different indices); and it costs one `pub use`. Raised as
a candidate ruling because the accessor a consumer will log is an API
commitment, and changing `session_id()` later is a breaking change.

### U3 — §15.3's registry has names but no identifiers

Ruling 63 named thirteen constants "the spec fixes but never names", and
took `APPLICATION_ERROR_BASE` out of §15.3 — leaving `NO_ERROR`,
`PROTOCOL_VIOLATION`, `FLOW_CONTROL_ERROR`, `STREAM_LIMIT_ERROR`,
`STREAM_STATE_ERROR`, `FINAL_SIZE_ERROR` and `MESSAGE_OVERFLOW` as table
rows only. The consolidated constants table compresses all seven into one
row — "wire error codes | 0x00–0x06 + ≥ 0x10 application; 0x07–0x0f
reserved" — which is exactly ruling 63's second recurring shape: *a
compressed range with no identifier a reader can grep for*.

*Provisional:* slice 3 introduces them as a Rust type (`CloseCode`, or
`pub const ERROR_*: u64`), and the plan proposes adding them to the
Named-constants table under ruling 63's own closing instruction: *"a new
constant SHOULD enter the table with an identifier, not only a value."*
Low cost either way; raised because ruling 63 says to.

### U4 — what does a **closing** connection do with a received frame stream?

§15.2 says the closing state retains "the seal capability, the receive
cipher states, and the replay window", and that a reply is owed to an
authenticated, window-fresh inbound packet. It also requires that "a CLOSE
**received** while closing moves the connection to the reply-free draining
behaviour" — so the frame stream **must** be parsed at least far enough to
detect a CLOSE. It says nothing about the rest: are ACK, MAX_DATA, STREAM
applied? Is a structural failure while closing another
`PROTOCOL_VIOLATION`?

The stated construction is a retention **list**, therefore exhaustive:
stream, flow-control, recovery and congestion state "may drop
immediately", so there is nothing left for a STREAM or MAX_DATA frame to
be applied *to*.

*Provisional:* while closing or draining, **parse** the frame stream and
apply **nothing but CLOSE detection**; a structural failure while closing
is ignored (we are already dying, with a code, and a second CLOSE carrying
a different code would fight the ≤ 1/s reply rule's intent). This is the
reading the retention list forces, and it is worth writing down because
"parse and apply, minus the frames whose state is gone" is the natural
implementation and is subtly different.

*Consequence:* this makes **U9 vacuous**.

### U5 — the linger clock when closing becomes draining

§15.2: "A CLOSE **received** while closing moves the connection to the
reply-free draining behaviour below", and that behaviour is "hold a brief
drain for **the same** `CLOSE_LINGER`". Unstated: does the drain restart
at the CLOSE's arrival, or does the original `CloseLinger` deadline stand?

*Provisional:* the **original deadline stands**. §16.5's governing
principle is "state removal precedes emission", and a restart lets an
authenticated peer hold our post-mortem state open indefinitely by
re-CLOSEing at 4.9 s — a cheap unbounded hold against state §17.5 exists
to bound. This is **Q2**.

### U6 — who owns the static→state map, and can it be read synchronously?

§16.1 asserts the one-connection-per-static invariant; §5.4 defines
NONE/PENDING/LIVE; slice 2a built the pending tables and the static map in
the **endpoint core**. §16.2's `connect()` is **not `async`**, so
`AlreadyConnected` must be answerable with no driver round-trip — while
§16.3's ruling-53 table puts `connect` under "command channel + oneshot
reply". Full statement as **C2**; provisional decision at **Q5**.

### U7 — `EndpointOutput::Transmit` carries no connection id, but ruling 49's trace needs one

§18.2: `slither::io` carries "`Wire::send_to` failures, **against the
connection whose datagram it was**, with the destination address and the
underlying `io::Error`". §16.4's `EndpointOutput::Transmit(Transmit)`
carries `{ to, data }` and nothing else, so the driver cannot attribute a
failed msg1/msg2/retransmit send to a connection.
`ConnOutput::Transmit` is fine — already scoped to one connection.

The scope is unstated a second way: a **msg1 retransmit** belongs to a
pending that *does* have a `ConnectionId`
(`core::Endpoint::connect()` returns one), but a **msg2** and a
**tie-break msg2** are sent for a chain that has no connection until
`accept()` returns.

*Provisional:* trace endpoint transmits under `slither::io` with the
destination address and, where the endpoint core knows one, the
`ConnectionId`; do **not** change `EndpointOutput`'s shape this slice —
it is a `pub` type and slice 2a's tests pin it. Raised because §18.2
promises an attribution the type cannot carry, and slice 7's roaming will
want it too.

### U8 — §16.9's "ordinary work" and a `close()` before establishment

§16.9 makes pre-establishment work ordinary. `close()` on a
not-yet-established connection is not constructible *through the shell*
(no `Connection` handle exists until `Connecting` resolves), but it **is**
constructible on the core, since `core::Endpoint::connect()` hands back a
`core::Connection` immediately. With `session == None` there is no seal
capability, so no CLOSE can be emitted.

*Provisional:* a silent local teardown — `Closed(LocallyClosed)`,
`Retired`, nothing transmitted, **no linger** (there is nothing to linger
for). Consistent with §15.4's endpoint-dropped row and with ruling 50's
"an attempt that never completed has no session to close and no wire
signal to send". Cheap to state; invisible to get wrong until slice 4.

### U9 — §8.2's "one trace fires": one per what?

Rendered vacuous by **U4**'s provisional decision — the first structural
failure enters closing, and closing applies nothing, so at most one fires
per connection. Recorded so the reasoning is on the record rather than
rediscovered.

### U10 — do the other connection timers survive entry into closing?

§16.5 lists eight named connection timers. §15.2 drops stream,
flow-control, recovery and congestion state — which disarms `Loss`, `Pto`
and `AckDelay` by removing what they act on — but says nothing about
`Liveness`, `Keepalive`, `PersistentKeepalive` or `Contested`. If
`Liveness` survives, a closing connection that sits out its linger can
produce a **second** `Closed`; `closed()` is latched on the first, so the
second is either silently dropped or a bug, depending on who wrote it.

*Provisional:* entering closing or draining **disarms every timer but
`CloseLinger`**, and `Closed(_)` is emitted **exactly once** per
connection, with a `debug_assert!` to that effect. This matters more than
it looks: it is the invariant `closed()`'s latch rests on.

---

## 7. Conflicts found (reported, not resolved)

Working rule 3: *when two statements conflict, do not default to the
code-like rule.* Three times in this project the prose held the correct
intent and the formal rule held the bug. Each conflict states both sides
and a provisional reading; **none is resolved here.**

### C1 — §15.2 needs `seal_quiet`, which the slice plan gives to slice 7

- **§15.2 (spec):** "emit CLOSE (sealed `seal_quiet`, §7.4) and enter
  **closing**".
- **`PLAN.md` §4 (plan):** "§7.4 liveness / `seal_quiet`" is **slice 7**;
  "§15 CLOSE + teardown matrix" is **slice 3**.

Slice 3 cannot implement its headline frame as specified without a §7.4
concept. This is a **plan/spec** conflict, not a spec-internal one, so it
is settled by moving a boundary rather than by ruling on the wire.

*Provisional:* amendment 1 (§0.3) — the two seal paths, the arming rule,
the install pin and the `Liveness` timer move into 3a; §7.5's keepalive,
persistent keepalive and contested probe stay in slice 7.

*Second-order evidence that this is the right cut:* §7.4's install pin is
what makes "a half-open session is reaped by liveness in 25 s" a fact, and
**five** things already rest on it — §6.7, §17.1, §15.4's endpoint-dropped
row, Appendix B's S29 obligation, and Appendix B's `closed()` obligation.
Leaving it in slice 7 leaves five obligations unreachable across four
slices.

### C2 — `connect()` is synchronous in §16.2 and a round-trip in §16.3

- **§16.2 (3915–3916):**
  `pub fn connect(&self, remote, remote_static) -> Result<Connecting, ConnectError>;`
  — **not `async`**. `ConnectError::AlreadyConnected` is therefore
  returned before any await.
- **§16.3's ruling-53 table (4178):** "Endpoint verbs — `connect`,
  `accept`, and §6.2's three staged verbs | command channel + oneshot
  reply." A oneshot reply cannot be read from a non-`async` function
  without blocking, and §16.8 forbids blocking on the driver seam.
- **§16.1 (3889–3892)** adds a third constraint: "An outbound connect is
  'in flight' only while its `Connecting` lives" — so the PENDING/LIVE
  test `connect()` performs must observe driver-owned state **at the
  instant of the call**.

Neither side is obviously the bug. §16.2's signature is load-bearing: an
`async connect()` would make `timeout(d, connect(..))` ambiguous about
what is being timed out, and ruling 50's whole idiom depends on the
`Connecting` being the only future. Ruling 53's table is load-bearing
because §6.2 requires DH on the driver task — but **`connect()` performs
no DH**: §6.1's initiator costs are paid when msg1 is built, on the
driver; `core::Endpoint::connect()` merely mints the pending.

*Provisional (**Q5**):* `connect()` is a **synchronous send** on the
command channel plus a **synchronous read of a shared cell** for the
NONE/PENDING/LIVE test, returning `Connecting` immediately. Both texts are
satisfied: the verb still lands on the driver (the channel message starts
the protocol work), and the error the signature promises synchronously
comes from the same shared cell §16.8 already mandates for accessors.
**Report:** ruling 53's table lists `connect` beside the staged verbs,
which genuinely *are* `async`; `connect` is not, and the table does not
say so.

### C3 — §16.2's drop semantics: `close(NO_ERROR, "")` versus "dies silently"

Within one paragraph (4143–4154):

> "Dropping the last handle to a `Connection` performs
> `close(NO_ERROR, "")` — the graceful teardown of §15.2"

and

> "Dropping every handle stops the driver and every connection dies
> silently — nothing transmitted (§15.4)."

§15.4's matrix keeps them as two rows — "local `close(code, reason)` /
**last-handle drop** (§16.2)" transmits a CLOSE; "endpoint dropped —
**every handle gone** (§16.3)" transmits nothing — so the spec is
self-consistent **iff** "the last handle to a `Connection`" and "every
handle" are different sets. They usually are: the first is the last handle
to *one* connection while other handles keep the driver alive; the second
is the last handle to *anything*.

**The conflict is where the two coincide**: dropping the last `Connection`
when it is also the last handle in the process. Does it transmit a CLOSE?
A synchronous `Drop` cannot await the driver, and the driver is about to
stop.

*Provisional:* **nothing is transmitted.** §15.4's endpoint-dropped row is
unconditional; ruling 50 takes the same position for the analogous
`Connecting` case ("an attempt that never completed has no session to
close and no wire signal to send"); the peer's cost is bounded at
`DEAD_TIMEOUT`, which that row already accepts. But this is exactly S26's
`⚠ CHECK` — "drop-order sensitive and the opposite of the obvious guess" —
and it deserves a ruling rather than an implementer's choice, because the
observable difference is a peer waiting 25 s.

### C4 — §16.4's `Retired`-within-the-same-drain versus §15.2's linger

The densest one, and the seam review's chartered target.

- **§16.4 (4522–4528):** "every terminal `ConnOutput` — a
  `Closed(ConnectionLost)` event, **or** the completion of the close
  linger — is followed **within the same drain** by
  `ToEndpoint::Retired`, and the shell delivers it to
  `handle_connection_event` **before** releasing the connection's
  shell-side bookkeeping".
- **`ToEndpoint::Retired`'s own contract (4447):** "teardown: drop the
  index route (**MUST**)".
- **§15.2:** the closing state must, for `CLOSE_LINGER` = 5 s,
  **receive** authenticated inbound packets — to reply ≤ 1/s and to detect
  the peer's CLOSE. Receiving requires the `receiver_index → Connection`
  route to be **live**. And "the linger's reply rule is CLOSE's only
  reliability mechanism".

Taken literally, `Closed(LocallyClosed)` at `close()` is followed in the
same drain by `Retired`, the index route dies, and the five-second linger
is unreachable — deleting CLOSE's only reliability mechanism.

Two readings:

1. **`Closed` is emitted only at the end of the post-mortem.** The "or" in
   §16.4 then describes one moment twice. But §16.2 says `close()`
   resolves "once the CLOSE frame is sealed", and `closed()` resolves
   "whenever the connection ends" — an application would wait 5 s to learn
   about a death it caused.
2. **`Closed` is emitted at the death; `Retired` follows the *final*
   terminal moment.** The "or" then enumerates two different paths: a
   connection dying without a linger (liveness, nonce exhaustion,
   replaced, a structural failure before a session exists) emits `Closed`
   then `Retired` in one drain; a connection that closes emits `Closed`
   now and `Retired` at linger expiry.

*Provisional: reading 2.* Working rule 3 says the prose has held the
correct intent three times; here §15.2's prose makes a substantive
protocol claim ("CLOSE's only reliability mechanism") and §16.4's "within
the same drain" is the code-like rule. **Report; do not silently pick** —
and note that reading 2 requires §16.4's sentence to be **amended**,
because under it not every terminal `ConnOutput` is followed by `Retired`
within the same drain. This is **Q1**, ranked first.

### C5 — Appendix B's `closed()` obligation names rows slice 3 cannot reach

Appendix B (5706–5717) requires driving *each* teardown row in turn:
`PeerClosed`, `TimedOut` (both the silence case **and** the contested
verdict), `Replaced`, `ProtocolViolation`, `LocallyClosed`. The contested
verdict is slice 7 (§7.5); `Replaced` needs the endpoint→shell replacement
wiring.

Not a spec conflict — a **coverage** conflict between an obligation
written for the finished protocol and a slice plan that delivers it in
pieces. Reported because the failure mode is a slice-3 test file that
*looks* like it discharges Appendix B and does not.

*Provisional:* the test file's module doc lists every row, marks the ones
slice 3 reaches, and names the slice that owes each of the rest. Slice 7's
plan inherits the list.

---

## 8. Open questions, ranked by cost of a wrong answer

Shape borrowed from `.slices/02-handshake/OPEN-QUESTIONS.md`. Each carries
a **provisional decision chosen to be the most defensible reading**, so
that reversing it later is an edit to a named test rather than an
excavation.

### Q1 — when does `Retired` fire? *(cost: highest — a deleted reliability mechanism, or a permanent route leak)*

**The question.** §16.4 puts `Retired` in the same drain as
`Closed(ConnectionLost)`; §15.2 needs the index route alive for
`CLOSE_LINGER` afterwards. See **C4** for the full statement.

**Cost of each wrong answer.** Fire early and CLOSE's linger reply — "CLOSE's
only reliability mechanism" — never happens, and the peer waits
`DEAD_TIMEOUT` after every clean close. Fire never, or fire only on some
paths, and "the index route and the guard-entry pin leak for the
endpoint's life" — §16.4's own words.

**Provisional decision.** `Closed(_)` is emitted when the connection dies;
`Retired` is emitted when its **state is actually dropped** — at
`CloseLinger` expiry for the closing and draining paths, and within the
same drain as `Closed` for every path with no linger (liveness, nonce
exhaustion, `Replaced`, a pre-session teardown). Pinned by **T5**, which
is one named test; reversing this is an edit to that test.

**Needs a maintainer ruling** — it requires amending §16.4's sentence.

### Q2 — does a CLOSE received while closing restart the linger? *(cost: an authenticated peer can hold post-mortem state open indefinitely)*

**Provisional decision.** No — the **original `CloseLinger` deadline
stands**. Reasoning at **U5**. Pinned by a test that closes at *t*, sends
a peer CLOSE at *t* + 4.9 s, and asserts all state is dropped at *t* + 5 s
and not at *t* + 9.9 s.

### Q3 — is `Closed(_)` emitted exactly once, and does closing disarm the other timers? *(cost: a second, contradictory `ConnectionLost` behind the latch)*

**Provisional decision.** Yes and yes: entering closing or draining
disarms every timer but `CloseLinger`; `Closed(_)` is emitted exactly once
per connection and the core `debug_assert!`s it. Reasoning at **U10**.
This is the invariant `closed()`'s latch rests on, and it is cheap now and
expensive to retrofit once slice 5 and 7 arm five more timers.

### Q4 — what shape is the replay window's public surface? *(cost: a slice-5 rewrite of the one structure §12.2 forbids duplicating)*

**The question.** §7.2 and §12.2 make the replay window the *single*
received-packet record. If slice 3 exposes it as
`fn seen(&self, counter: u64) -> bool`, slice 5 cannot derive an ACK from
it and will build the second tracker the spec forbids.

**Provisional decision.** The window exposes, at minimum:

```rust
fn greatest(&self) -> Option<u64>;
/// Descending, newest-first ranges of received counters, per §12.2.
/// The iterator is the ACK derivation's input; slice 5 adds the
/// MAX_ACK_RANGES / packet-capacity truncation on top of it.
fn ranges_desc(&self) -> impl Iterator<Item = RangeInclusive<u64>> + '_;
```

Slice 3 tests `ranges_desc` directly (it is a pure function of the bitmap)
even though nothing consumes it yet. This is the cheapest possible way to
stop the fusion rule being violated by accident.

### Q5 — how does a synchronous `connect()` satisfy ruling 53? *(cost: either an API change or a §16.8 violation)*

**Provisional decision.** As in **C2**: a synchronous shared-cell read for
the NONE/PENDING/LIVE test, plus a synchronous non-blocking send on the
command channel that starts the protocol work. `Connecting` is returned
immediately and is the only future. **Report to the maintainer** that
ruling 53's table groups `connect` with verbs that are `async` when it is
not.

### Q6 — what mechanism makes ruling 50's cancellation-ordering MUST true? *(cost: the redial idiom every consumer writes returns `AlreadyConnected`)*

**The question.** §16.3: "An implementation **MUST** order the cancellation
ahead of any endpoint verb the application issues after the drop returns."
`Drop` is synchronous; the driver is not.

**Provisional decision.** `Connecting::drop` does **two** things
synchronously: it writes the static back to NONE in the **same shared
cell** `connect()` reads, and it enqueues the cancellation command. The
driver then performs the protocol-side teardown (pending, pending-index,
hint-set entry) when it next runs. Under Q5's answer the ordering MUST is
then **structurally true** rather than a discipline — an immediate redial
reads the cell the drop just wrote — and the FIFO order of the command
channel keeps the driver-side work ordered too.

*Rejected alternative, recorded so it is not re-proposed:* a
cancellation flag the driver polls. It makes the MUST depend on the driver
being scheduled between the drop and the redial, which on a paused clock
with no await between them **it is not** — this is exactly why Appendix B
specifies "with no advance of the clock between the drop and the redial".

### Q7 — how is the epoch boundary tested without 65 536 seals? *(cost: S23 is either untested or slow)*

**The question.** hiss's `set_counter_for_test` is `#[cfg(test)]`
**inside hiss**, so slither cannot reach it. `REKEY_EPOCH_MSGS` is a
slither constant handed to `into_datagram_with_epoch` as `epoch_size`.

**Provisional decision.** Two options, and I recommend the second:

1. Actually perform ~65 540 seals of a minimal frame. Correct, and on a
   paused clock it is CPU-bound rather than wall-clock-bound — but it is
   ≈ 131 k AEAD operations per test and will show up in `cargo test`.
2. Make `epoch_size` **test-configurable** through `Config` (it is
   already threaded as `Self::epoch_size()`), defaulting to
   `REKEY_EPOCH_MSGS`, and assert separately — by a constant test — that
   the default **is** `REKEY_EPOCH_MSGS`. The boundary behaviour and the
   constant are then pinned independently, which is the stronger
   arrangement.

Option 2 introduces a test-only configuration knob on a security-relevant
schedule, so it wants the same treatment §16.6 gives the RNG seed:
documented as test-only, or feature-gated. **Flag for a ruling** — it is
the same shape §16.6 already ruled on for the seed.

### Q8 — does the last-handle drop transmit a CLOSE when it is also the last handle in the process? *(cost: the peer waits 25 s, or does not)*

**Provisional decision.** Nothing is transmitted; §15.4's endpoint-dropped
row is unconditional. Full reasoning at **C3**. **Needs a ruling** — S26
already carries a `⚠ CHECK` for being drop-order sensitive and the
opposite of the obvious guess.

### Q9 — is the §16.6 sub-seed draw order pinned? *(cost: silent divergence of every seeded test in slices 4+)*

**Provisional decision.** Yes — slice 3 adds a golden-sequence test over
the endpoint's minted indices from a fixed root seed across N `connect()`s
and M `accept()`s. `.slices/02-handshake/OPEN-QUESTIONS.md` §C records the
draw order as deliberately untested; slice 3 is the first slice where a
wrong order is *reachable*, and a golden sequence is the only assertion
that separates a correct draw order from a plausible wrong one (**T9**).

### Q10 — is `SessionId` hiss's, or slither's? *(cost: a breaking change to an accessor consumers log)*

**Provisional decision.** Re-export `hiss::noise::SessionId`. Reasoning at
**U2**. Ruling requested.

### Q11 — does slice 3 implement PADDING? *(cost: a legal packet kills a connection)*

**Provisional decision.** Yes, and the empty-plaintext keepalive
short-circuit too. Reasoning at **U1**. This is a plan-scope decision, not
a spec question.

### Q12 — does §7.4's liveness half move into slice 3? *(cost: five obligations unreachable for four slices)*

**Provisional decision.** Yes — amendment 1, §0.3. Reasoning at **C1**.
**Needs a maintainer decision**, because it moves a slice boundary.

---

## 9. Test-design notes (working rule 9)

> *A bound is only a test if the degenerate case violates it.* An upper
> bound the collapsed implementation satisfies for free asserts nothing.
> Slice 2a shipped two tests named for properties they did not pin.

For each property below: **what the broken version does**, and the side of
the assertion that separates them.

### T1 — the epoch ratchet (S23)

- **Broken version:** `into_datagram()` with no epoch at all — a plain
  datagram pair whose keys never ratchet.
- **What the obvious test asserts:** "packets before and after counter
  65 536 both open." The broken version **passes**: a non-ratcheting pair
  opens everything.
- **Assert from the separating side:** a packet from epoch *e−2* **must
  fail to open**. Only a ratcheting receiver refuses it. Pair it with the
  straggler assertion (*e−1* **does** open), which separates a correct
  ratchet from a naive "current epoch only" build. Neither assertion alone
  is a pin; the pair is.

### T2 — parse-then-apply (§8.2)

- **Broken version:** a streaming parse-and-apply loop.
- **What the obvious test asserts:** "an unknown frame type produces
  `ProtocolViolation`." The broken version passes — it reaches the unknown
  type and dies there too.
- **Assert from the separating side:** a single packet whose plaintext is
  **[valid CLOSE frame][unknown type `0x7f`]**. The correct
  implementation surfaces `ConnectionLost::ProtocolViolation { code }`;
  the streaming one applies the CLOSE first and surfaces
  `PeerClosed { .. }`. Two different variants from one packet — decisive,
  and constructible with only slice 3's frames.

### T3 — the ack-eliciting classifier (§8.7)

- **Broken version:** `matches!(frame, Frame::Ping)` — a two-arm match.
- **Why no flow test separates them:** every frame slice 3 builds is in
  the "never" retransmission class, and PING is the only ack-eliciting one
  among them. The classifier is correct-by-accident for the whole slice.
- **Assert from the separating side:** write the classifier as a pure
  function of the **type code** — `fn is_ack_eliciting(ty: u64) -> bool` —
  and unit-test it against **all twelve rows of §8.3**, including the
  types slice 3 cannot construct (`MAX_DATA` true, `ACK` false,
  `RESET_STREAM` true, `DATAGRAM` true, `PADDING` false, `CLOSE` false…).
  Slices 4–6 then inherit a tested classifier instead of re-deriving it.

### T4 — plan-seal-commit (§16.7)

- **Broken version:** sealing lazily inside `poll_output()`.
- **What the obvious test asserts:** "`close()` produces a
  `ConnOutput::Transmit`." Passes either way.
- **Assert from the separating side, two ways:**
  1. After `close(now, code, reason)` and **before any `poll_output()`
     call**, the session's `next_counter()` has already advanced. A lazy
     implementation has not sealed. (Needs a `#[cfg(test)]` or
     `pub(crate)` accessor for the next counter — name it in the brief.)
  2. Call `handle_timeout(now)` **twice** with no drain in between, at a
     due `CloseLinger` reply moment, and assert exactly **one** reply
     transmit results. §16.5's idempotency "rests on synchronous sealing";
     a lazy implementation produces two.

### T5 — `Retired` (§16.4, Q1)

- **Broken versions:** (a) never emitted; (b) emitted in the same drain as
  `Closed`, killing the linger.
- **Assert from the separating side:** drive a local `close()` at *t*,
  then at *t* + 1 s feed an authenticated, window-fresh packet and assert
  **a CLOSE reply transmit is produced** *and* **`Retired` has not yet
  been emitted**. At *t* + 5 s assert `Retired` is emitted, exactly once,
  carrying the right `our_index`. Version (a) fails the second half,
  version (b) fails the first. A count-only assertion ("exactly one
  `Retired`") passes version (b) and pins nothing.

### T6 — the timer table and its ordering (§16.5)

- **Broken version:** a bare `close_linger_deadline: Option<Instant>` on
  the connection. Slice 3 arms one timer, so **every** ordering satisfies
  every flow test.
- **Assert from the separating side:** make the timer table a data
  structure with its own unit tests —
  `struct Timers { keepalive, persistent_keepalive, liveness, loss, pto, ack_delay, close_linger, contested: Option<Instant> }`
  plus `fn next(&self) -> Option<Instant>` and
  `fn due(&self, now: Instant) -> Option<TimerKind>` returning the
  **highest-priority** due timer. Then arm `Liveness` and `CloseLinger` at
  the *same* instant in a unit test and assert `Liveness` wins; arm
  `Loss` and `Pto` together and assert exactly one fires; arm `AckDelay`
  with `Loss` and assert `Loss` first. None of §7.5's or §13's machinery
  needs to exist for this — the ordering is testable as data, and slices 5
  and 7 then inherit a ratified order they cannot re-derive wrongly.

### T7 — nonce exhaustion (§7.9)

- **Broken versions:** the seal error is swallowed; or the commit happens
  before the seal.
- **Why the obvious test is impossible:** ~1.8 × 10¹⁹ seals, and hiss's
  `set_counter_for_test` is internal to hiss.
- **Assert from the separating side:** a `#[cfg(test)]` seal-failure
  injection point in `session.rs`, and then assert **three** things:
  `Closed(NonceExhausted)` is emitted; **nothing is transmitted**; and
  **nothing moved** — the counter is unchanged and no state was committed
  (§16.7's "on seal failure nothing moved"). The third is the only place
  in slice 3 where commit-after-seal is observable at all, so it is worth
  the injection seam.

### T8 — the lateness bound `L` (§16.5)

- **Broken versions:** a driver that ticks on a fixed interval (fires
  late); a driver that recomputes the deadline and fires **early**.
- **What the obvious test asserts:** "the linger expiry is observed no
  later than 5 s + 250 ms." The early-firing implementation passes.
- **Assert from the separating side:** assert **both** bounds — the expiry
  is observed at ≥ 5 s **and** ≤ 5 s + `SHELL_LATENESS_BOUND`. This is
  slice 1's one-sided-boundary lesson (`LEN` and `LEN−1` tested, `LEN+1`
  not), and §16.5 states both halves: "fires **no earlier than** `D` and
  no later than `D + L`."

### T9 — the §16.6 sub-seed draw order

- **Broken version:** the sub-seed is drawn lazily, or not at all.
- **Why nothing in slice 3 notices:** the connection core consumes no
  randomness this slice, so every observable value is identical.
- **Assert from the separating side:** a **golden sequence** — from one
  fixed root seed, run a fixed script of N `connect()`s and M `accept()`s
  and pin the exact sequence of minted indices as a byte array. A build
  that omits the per-connection sub-seed draw produces a different
  sequence at the second connection. This is the only assertion that
  separates them, and it also guards slices 4+ when the connection core
  starts drawing.

### T10 — `closed()`'s latch (S27)

- **Broken versions:** a `oneshot` (resolves once; a second `closed()`
  hangs); a `Notify` (a `closed()` created *after* the death never fires);
  a broadcast channel with a bounded buffer (a late subscriber misses it).
- **What the obvious test asserts:** "`closed()` resolves with
  `PeerClosed`." All three broken versions pass.
- **Assert from the separating side, three separate assertions:**
  1. **Three** concurrent `closed()` futures all resolve with the same
     value — kills the oneshot.
  2. A `closed()` created **after** the death resolves **immediately** —
     kills the `Notify` and the bounded broadcast.
  3. Dropping a `closed()` future and creating another still resolves —
     cancel-safety.

### T11 — cancellation ordering (S29, ruling 50)

- **Broken version:** `Drop` sets a flag the driver notices when next
  scheduled.
- **Assert from the separating side, two halves:**
  1. **No clock advance and no `await` between the drop and the redial** —
     Appendix B specifies exactly this. On the paused clock the driver is
     not scheduled in that gap, so the flag-based implementation returns
     `AlreadyConnected` and the shared-cell implementation returns `Ok`.
  2. **No msg1 leaves after the drop** — record the tap count at the drop,
     advance 30 s (six `RETRANSMIT_BASE` intervals), assert the count is
     unchanged. "No msg1 was observed in the next instant" is the
     degenerate version and passes on a train that is merely between
     retransmits.

### T12 — S2's give-up: "not before and not never"

- **Broken versions:** resolves at the first retransmit that gets no
  answer (before); never resolves (never).
- **Assert from the separating side:** at `HANDSHAKE_GIVEUP − 1 ms`, poll
  the `Connecting` and assert it is **still `Pending`**; then advance and
  assert it resolves `Err(ConnectError::TimedOut)` within
  `SHELL_LATENESS_BOUND`. The "still pending just before" half is the one
  a single-sided test omits, and S2's own wording — "not before and not
  never" — asks for both.

### T13 — the CLOSE linger's reply rule (§15.2)

- **Broken versions:** replies to every inbound packet; replies to the
  **triggering packet's source**; replies to a packet that merely routed
  by `receiver_index`.
- **Assert from the separating side, three assertions:**
  1. Ten authenticated, window-fresh inbound packets within one second
     produce **exactly one** reply — and at least one, which separates it
     from "never replies".
  2. Inject an authenticated packet from a **different source address**
     and assert the reply goes to the **session's endpoint address**
     (§15.2: "the closing state does not roam; never to the triggering
     packet's source"). A build that replies to the source passes the
     count test and fails only this one.
  3. Inject a packet that routes by `receiver_index` but **fails the
     AEAD** and assert **no reply at all** — this is the off-path forger
     §15.2 names explicitly.

### T14 — the replay window is strictly post-AEAD (§7.2)

- **Broken version:** check-then-mark **before** `decrypt_at`, or marking
  the window on a decryption failure.
- **What the obvious test asserts:** "a duplicate is dropped." Passes
  either way.
- **Assert from the separating side:** send a packet at counter *c* with a
  **corrupted tag**; then send the *genuine* packet at counter *c*. The
  genuine packet **must be delivered**. A pre-AEAD-marking build has
  already burned *c* and drops it. This single test is the whole of §7.2's
  ordering rule.

### T15 — the replay window's lower edge (§7.2)

- **Broken version:** an off-by-one on "more than 2048 behind the
  greatest".
- **Assert from both sides:** `greatest − 2048` is **accepted**;
  `greatest − 2049` is **dropped**. Testing only the dropped side leaves
  the whole window collapsible to a much smaller one with nothing red.
  (Slice 1's lesson: `LEN` and `LEN−1` were tested and `LEN+1` was not.)

### T16 — the two seal paths (§7.4, if amendment 1 lands)

- **Broken version:** `seal_quiet` is an alias for `seal`.
- **Why no slice-3 flow test notices:** slice 3's only seals are CLOSE
  replies, which are all `seal_quiet`, so `last_send` is never read.
- **Assert from the separating side:** a unit test on the session that
  seals a CLOSE and asserts `last_send` is **unchanged**, then (once slice
  4 has STREAM) seals a stream frame and asserts it **advances**. Slice 3
  can only assert the first half — so **state in the test file that the
  second half is owed by slice 4**, rather than letting a half-assertion
  read as a pin.

## 10. Agent briefs and sequencing

### 10.1 Blocking decisions — needed **before** any agent is dispatched

| # | Decision | Why it blocks |
|---|---|---|
| **B1** | The cut (§0) — 3a/3b, or one slice | Decides how many agents and in how many waves |
| **B2** | **Q12 / C1** — does §7.4's liveness half move into 3a? | Changes 3a's file list, its timer table, and whether `TimedOut` is reachable in 3b |
| **B3** | **Q1 / C4** — when does `Retired` fire? | It is a core state-machine shape, not a detail; both the implementer and the test author need the same answer, and they are forbidden to confer |
| **B4** | **Q11 / U1** — four frames, not three, plus the empty-plaintext short-circuit | Changes the codec's scope |
| **B5** | **Q5 / C2** — `connect()`'s synchronicity | Decides `Endpoint`'s internal shape |
| **B6** | **Q7** — how S23's boundary is tested | Decides whether `Config` gains a test-only `epoch_size` |

**B3 in particular must be settled before the briefs are written**, not
during the slice. Working rule 6 forbids the implementer and the test
author from converging on an answer between them; if the answer is not in
both briefs, they will converge anyway or diverge silently.

### 10.2 Wave 1 — slice 3a

| Agent | Owns (writes) | Reads (does not write) | Brief must carry |
|---|---|---|---|
| **TEST-A** | `src/core/tests.rs` (append), `tests/spec_frames.rs`, `tests/spec_close.rs` | `SPEC.md` §7.1–7.9, §8, §15, §16.4–16.7 by the line ranges in §3; `STORIES.md` S23 | The §3 digest **and nothing from `src/core/connection/`**. It writes tests from the spec and the story, before the implementation exists. The §9 test-design notes are binding on it. |
| **IMPL-A** | `src/core/connection/{mod,session,frame,close,timers}.rs`, `src/core/mod.rs` | the same spec sections; the resolutions of B2–B4, B6 | It **declares** `#[cfg(test)] mod tests;` where needed and **creates no test file**. The §6 unstated-scope provisionals and the §7 conflict provisionals are its instructions, each labelled as provisional so it flags rather than improvises if one is wrong. |

TEST-A and IMPL-A run **concurrently** and their paths are disjoint. The
reconciliation step (slice 1's `RECONCILIATION.md` pattern) runs after
both.

### 10.3 Wave 2 — slice 3b *(after 3a is green on the full gate table)*

| Agent | Owns (writes) | Reads | Brief must carry |
|---|---|---|---|
| **TEST-B** | `tests/story_lifecycle.rs`, `tests/story_dial.rs`, `tests/spec_shell.rs` | `STORIES.md` S1/S2/S26/S27/S29; `SPEC.md` §16.1–16.3, §16.8–16.10; Appendix B 5705–5770; the `testutil` harness API | §5.2's story table, §9's notes T10–T13, and the honest list of rows it cannot reach (**C5**). It must **name** every unreached obligation in a module doc. |
| **IMPL-B** | `src/shell/{mod,driver,endpoint,staged,connection}.rs`, `src/testutil/mod.rs`, `src/lib.rs` | the same spec sections; ruling 53, 49, 50, 62; the resolutions of B3, B5 | §4's digest, §0.4's internal sequencing (endpoint side first, connection side second), and the provisionals for Q5, Q6, Q8. |

**The one shared-path hazard** is `src/testutil/mod.rs` (see §2.2). The
briefs must state that IMPL-B owns it and TEST-B only consumes it — or
swap them — but they must say the same thing.

### 10.4 Build order inside each wave

**3a (IMPL-A):**

1. `timers.rs` — the named table and the ratified equal-deadline order,
   with its own unit tests. It has no dependencies and everything else
   arms into it.
2. `session.rs` — the seal/open wrapper over `EstablishedSession`, the
   replay window, §16.7's plan/seal/commit split, `next_counter()`
   plumbing, and (if B2 lands) the two seal paths + arming + install pin.
3. `frame.rs` — the codec: encode/decode for PADDING, PING, ACK, CLOSE;
   the §8.3 type-code classifier; parse-then-apply; §8.5's packing order
   with its ACK→control→fill→PING stages present even where empty.
4. `close.rs` — the three post-mortem states, the linger, the reply rate,
   the registry.
5. `mod.rs` — wire them into `handle_datagram` / `handle_timeout` /
   `close` / `poll_output`, and the `Closed` + `Retired` discipline.

**3b (IMPL-B):**

1. `driver.rs` skeleton — the `select!` over `Wire::recv_from`, the
   command channel, and the timer; the drain-to-`Timeout` discipline for
   both cores; ruling 49's `slither::io` trace on send failure.
2. `endpoint.rs` + `staged.rs` — slice 2b's deferred handles, the command
   channel + oneshot mechanism, `Connecting` (ruling 62 handle semantics,
   ruling 50 cancel-on-drop).
3. `connection.rs` — the shared cell, the `closed()` latch, the four
   accessors, `close()`, and the (empty) notification slots.
4. `testutil` harness — the `LocalSet` two-endpoint fixture.

### 10.5 What the briefs must **not** do

- Give two concurrent agents one path. This is working rule 6 and it has
  already cost this project 68 tests.
- Tell either agent to "update the expectation" if a golden-wire or
  size/constant test goes red. Slice 3 should move **no** wire byte — the
  only new bytes are inside the AEAD, and the golden vectors pin
  handshake packets. **A red golden test in slice 3 means the Data-header
  or seal path was touched wrongly**, and it is a ruling request.
- Ask either agent to resolve **C1–C5**. They report.

---

## 11. Exit criteria

### 11.1 The gate table — every slice ends here, not on `cargo test`

| Gate | Command | Bar |
|------|---------|-----|
| Compiles | `cargo build --all-features --all-targets` | clean build |
| Format | `cargo fmt --all --check` | no diff |
| Lints | `cargo clippy --all-features --all-targets -- -D warnings` | zero warnings |
| Docs | `cargo doc --no-deps` and `--all-features`, `RUSTDOCFLAGS=-D warnings` | no broken intra-doc links |
| Tests | `cargo test` **and** `cargo test --all-features` | all pass |
| Wire pins | the golden-wire and size/constant tests | byte-identical |
| MSRV | `cargo +1.96 check --all-features --all-targets` | passes |
| Supply chain | `cargo deny check` | clean |

*This planner has run none of these and claims none of them green.*

### 11.2 Slice 3a is done when

- S23's two tests pass on the paused clock, and the `s23_*` pair asserts
  from the separating side (**T1**).
- Appendix B's **Frame layer** list is discharged for the four frames
  slice 3 builds, and the type-code classifier is unit-tested against all
  twelve §8.3 rows (**T3**).
- Appendix B's **CLOSE** list is discharged: linger semantics, the ≤ 1
  reply/s cap under flood, replies only to authenticated window-fresh
  inbound and only to the session address, state dropped at 5 s, the
  receive side surfacing `PeerClosed` and never replying, mutual close
  draining reply-free, and violation ⇒ CLOSE with the matching registry
  code (**T13**).
- `Retired` is pinned by **T5** under whichever reading B3 settles.
- The timer table's equal-deadline order is unit-tested as data (**T6**).
- The replay window is pinned post-AEAD (**T14**) and at both edges
  (**T15**), and exposes §12.2's derivation surface (**Q4**).
- The sub-seed golden sequence exists (**T9**).
- Every conflict and unstated scope this slice met is written up in
  `.slices/03-skeleton/OPEN-QUESTIONS.md`, in the shape of
  `.slices/02-handshake/OPEN-QUESTIONS.md`.

### 11.3 Slice 3b is done when

- **S1, S2, S26, S27 (`closed()` half) and S29** are paused-clock tests
  that pass, per §5.2.
- Appendix B's **shell surface** obligations reachable in slice 3 are
  discharged, and every unreachable one is **named** with the slice that
  owes it (**C5**).
- No `Send` bound exists anywhere on the actor path — pinned by a
  compile-time test in the shape of `wire.rs`'s existing
  `a_wire_need_not_be_send`.
- The driver honours `SHELL_LATENESS_BOUND` at both ends (**T8**).
- The cancellation-ordering MUST is pinned with **no clock advance**
  between the drop and the redial (**T11**).
- `closed()`'s latch is pinned by all three separating assertions
  (**T10**).
- The post-slice-3 **seam review** (`PLAN.md` §5) is scheduled against
  **3b's** commit, with its five chartered targets: drain discipline,
  `Retired` ordering, waker registration under `RefCell`, cancel-safety of
  every `async fn`, drop order across handles.

### 11.4 Hand-forward to slice 4

- The frame codec's packing order (§8.5) has its ACK / control / fill /
  PING stages present; slice 4 inserts the round-robin STREAM fill as a
  stage rather than rewriting the packer.
- `ConnEvent` grows by the variants slice 4 can construct — the precedent
  in `src/core/connection/mod.rs` is that a variant arrives with the
  section that defines it.
- The replay window's `ranges_desc` (**Q4**) is slice 5's ACK input; slice
  5 adds truncation, not a second tracker (§12.2's fusion rule).
- §7.4's remaining half — §7.5's keepalive, persistent keepalive and the
  contested probe — stays in slice 7 whether or not amendment 1 lands.
- **Noted for slice 7, not slice 3's to fix:** §6.6/§6.7's `exempt_until`
  extends a guard pin past `HANDSHAKE_GIVEUP`, which breaks ruling 70's
  `TS_GUARD_ORPHAN_TTL == INTRO_TTL` alias. Slice 3 does not touch
  §6.6–6.8.
