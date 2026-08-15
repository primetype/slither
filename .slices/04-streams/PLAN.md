# Slice 4 — streams and flow control — PLAN

> Planner's note: written incrementally per working rule 2 — the heading
> skeleton was on disk before the first `Read` of any spec or source file.
> No production code and no tests are written by this document's author.

*Written against HEAD `da114c5`, clean, 456 tests green. Slices 0, 1, 2a,
3a, 3b closed. Wire frozen at slice 1; no byte moves in slice 4.*

## Sources consulted (and deliberately not consulted)

**`SPEC.md`, by targeted line range only — never whole** (working rule 1):
§8.3–§8.7 (2633–2835), §9.1–§9.7 (2838–3011), §9.8 first 6 lines only
(3012–3017, to know the boundary), §10.1–§10.7 (3167–3337), §15.3
(3838–3851), §16.2 (3942–4041, 4120–4216), §16.4 (4435–4644), §16.8
(4733–4747), §16.9–§16.10 (4749–4804), §18.1 (5236–5275), Appendix B
(5580–5632, 5820–5860). Plus `grep -n` sweeps for `Dir`, `StreamId`,
`StreamsExhausted`, `SendStream`, `RecvStream`, `BiStream`, and the frame
type constants.

**Also read:** `CLAUDE.md` (whole), `PLAN.md` §4–§5, `STORIES.md`
S12/S13/S14/S17 (and S15/S16 to know the boundary),
`.spec-v2-clean-slate/rulings.md` (index, plus rulings 16–22, 51–58,
81–82, and round 15 in full), `.slices/03-skeleton/PLAN.md` §0 (as a
format spec), `.slices/03-skeleton/FIXES-3b.md` §4–§5,
`.slices/03-skeleton/FIXES-3b-round2.md` (F5, F8, F9, F10),
`src/core/connection/mod.rs`, `src/core/connection/frame.rs`,
`src/constants.rs`, `src/error.rs`, `src/shell/shared.rs`,
`src/shell/connection.rs` (by grep and targeted read).

**Deliberately not consulted:** §9.8's body (140 lines, slice 6), §9.9,
§11, §12, §13, §14, `.spec-v2-clean-slate/SPEC-v0.1-wire-historical.md`,
and the v0.1 implementation at `5324ce5`.

**No gate was run and none is claimed green.**

---

## §0. Cut recommendation — **cut it: 4a (core) / 4b (shell + stories)**

**Recommendation: cut slice 4 into 4a and 4b along the core/shell seam.**

| | Contents | Stories closed | Rough size (impl + tests) |
|---|---|---|---|
| **4a — streams and flow control in `core::Connection`** | §9.1 ids and the four spaces · §9.2 implicit opening + the four closed-stream watermarks · §9.3–§9.4 both halves · §9.5 STREAM semantics and reassembly · §9.6 the **sender-emitted** RESET_STREAM · §9.7 lifecycle and GC · §10 entire (both credit levels, the re-grant formula, retirement true-up, MAX_STREAMS, violations, the §10.6 reassembly bound) · §8.3/§8.4's six new frames in the codec · §8.5's STREAM fill stage · §16.9's index→id remap at install · the six new `ConnEvent`s | **none** — three named **precursors** (S12p, S13p, S17p) at core level | ≈ 2.2 k + 1.3 k |
| **4b — the shell stream surface** | `SendStream` / `RecvStream` / `BiStream` (type + `split`/`join`, **no `AsyncRead`/`AsyncWrite`** — those are slice 8) · `Connection::{open_bi, open_uni, accept_bi, accept_uni}` · ruling 53's `poll_*`-once data path · §16.8's per-`StreamId` blocked-reader/blocked-writer waker maps · §16.2's drop semantics for both half-handles · the four story tests | **S12 (loss-free), S13, S14, S17** | ≈ 0.7 k + 1.1 k |

Ruling 90's `mint_pending`/`start_attempt` split is **separate work already
in flight** and is not planned here; see §1.

### §0.1 The decisive question, asked again — and it answers the same way

Round 12 cut slice 3 on one mechanical argument: **working rule 6's
test-author/implementer split cannot function across a combined slice,
because the story test must compile against a public API that does not
exist when its author starts.** That argument is not automatically
transferable — "slice 2 and slice 3 were both cut" is not evidence — so
here it is re-derived for slice 4, and then the three ways it could have
failed to transfer.

S12, S13, S14 and S17 are all *user-facing* capability stories. Every one
of their tests is an async paused-clock flow test over `testutil::Network`
that calls `Connection::open_bi()`, `SendStream::write()`,
`RecvStream::read()`, `SendStream::reset()`. **None of those exist today.**
In a combined slice they come into existence at the same time as the core
they sit on, written by agents who must agree on a surface neither has
built. That is the slice-3b shape exactly.

Three ways it might not have transferred, checked:

1. **"The API is spec-given this time, so the test author is not
   guessing."** Partly true and it is the strongest counter-argument:
   §16.2 gives `open_bi`/`open_uni`/`accept_bi`/`accept_uni` and both
   half-handles with full signatures, where slice 3b's `Connecting` ladder
   had to be designed. **But it does not survive contact with the
   detail.** §16.2's surface is under-determined in exactly the places
   the story tests touch: `StreamId`, `Dir` and `StreamsExhausted` are
   used and **never defined anywhere in `SPEC.md`** (§8, item H13 —
   ruling 89's shape, three times over), `BiStream`'s home slice is
   contradicted between `PLAN.md` and §16.2 (conflict C7), and
   `open()`'s return type is contradicted between §16.4 and §16.9
   (conflict C1). A test author handed only the spec cannot write
   `let (mut tx, mut rx) = conn.open_bi().await?.split();` without three
   decisions that are not in the spec. After the cut, those three
   decisions are **made, compiled and green** in 4a before 4b's test
   author starts, and the test author reads them off a real crate.
2. **"4b is only ~700 lines of implementation; a slice for that is
   overhead."** True about the line count and wrong about the risk. 4b
   is the layer the round-15 seam review just found four defects in, two
   of them unreachable from all 451 tests **by construction** (working
   rule 13). 4b's specific content is per-`StreamId` waker maps under
   `RefCell` (F10's exact site, now with a map keyed by a value the
   application controls), cancel-safety of four new `async fn`s, and
   §16.2's drop semantics — where dropping a `SendStream` *emits a wire
   frame* (`reset(0)`) and dropping a `RecvStream` *mutates the
   flow-control ledger*. Drop-order and cancel-safety are precisely the
   fault class `FlakyWire` cannot express. Small and risky is an argument
   **for** giving it its own review boundary, not against.
3. **"The dependency might be two-directional, so the cut buys nothing."**
   Checked: it is not. 4a needs nothing from 4b — §16.10 makes the core
   drivable with no kernel, and `src/core/connection/tests.rs` (3 105
   lines) already drives cores directly on the paused clock. 4b needs
   4a's six verbs and six events, all of which §16.4 pins by name. One
   integration point, one direction.

### §0.2 Two further arguments from evidence

4. **File-path disjointness becomes trivial, and does not otherwise.**
   Working rule 6's second half is absolute. A combined slice puts four
   concurrent agents — core implementer, core test author, shell
   implementer, story test author — around `src/core/connection/`, a new
   `src/core/connection/tests_streams.rs`, `src/shell/`, and
   `tests/story_streams.rs` at once, with the core test author and the
   shell implementer both wanting to touch `src/core/connection/mod.rs`
   (the former to add `mod tests_streams;`, the latter to add verbs).
   That is the slice-2a accident's shape. Cut, each phase has two agents
   and a clean partition (§6).
5. **The wrong-sizing is itself evidence.** `PLAN.md` sizes slice 4 at
   ≈ 3 k lines. Section §6's module map totals ≈ 5.3 k. The gap is not
   padding: §10 alone carries two credit levels, a re-grant formula, a
   cumulative-count limit with two emission triggers, an absolute-not-
   additive retirement true-up, and a reassembly-fragment bound with its
   own kill path — five independently testable mechanisms that
   `PLAN.md`'s row compresses into the two words "§10 flow control".
   Slice 4 is the largest slice in the plan as written, larger than the
   slice the plan itself calls "the risk".

### §0.3 The cut I considered and rejected: streams | flow control

The obvious alternative is to cut by subsystem — 4a = §9, 4b = §10.
**Reject it.** §10 is not layered on top of §9; it is threaded through it:

- §8.4's STREAM error list contains `FLOW_CONTROL_ERROR`, so the frame
  path cannot be finished without the ledger.
- §10.6 makes credit **the receiver's buffer commitment**, so the
  reassembler's bound *is* a flow-control quantity. A §9-only reassembler
  is unbounded, which is the one thing §10.6 exists to forbid; §10 would
  then be retrofitted into every receive path rather than added beside it.
- §9.6's RESET_STREAM true-up and §9.7's retirement true-up are §10.3
  rules living inside §9 sections.

A subsystem cut produces a first half that is knowingly wrong and a second
half that edits all of it. The layer cut produces a first half that is
right and a second half that sits on top. That difference is the argument.

### §0.4 The honest cost

**4a closes zero stories.** Slice 3a closed one and said so plainly;
slice 0 closed none. This is within precedent but it must be *in 4a's
brief*, not discovered at review. Three things bound the cost:

- 4a closes the **Appendix B §9/§10 obligation list** (§13), which is
  eleven distinct obligations — more spec surface than any slice since 1.
- 4a should close three **named precursors**, written as core-level
  paused-clock tests and named as precursors, never as the stories:
  `s12_precursor_*`, `s13_precursor_*`, `s17_precursor_*`. Working rule 9:
  a name is not a pin, and calling one of them S12 would let 4b ship
  without the handle-level test the story actually asks for.
- The cut is short. 4a's surface is spec-pinned by §16.4, so unlike
  slice 3 there is no surface to design at the boundary.

### §0.5 What the cut does *not* fix, and must be stated in both briefs

Slice 4 has **no ACK semantics** (§12 is slice 5) and **no loss recovery**
(§13 is slice 7). Neither cut changes that. Its consequences are worked
out in §12 below; the headline is that a **send half can never reach
`DataRecvd` in slice 4**, so `ConnEvent::StreamFinished` never fires, the
watermark advances only on the receive side, and MAX_STREAMS
replenishment is reachable for peer-opened **uni** streams and not for
peer-opened **bidi** streams. This is a fact about the slice boundary, not
a defect, but if it is not written into both briefs an implementer will
discover it as a mystery three days in.

---

## §1. Assumptions carried in

1. **Ruling 90 has landed.** `core::Endpoint::connect()` is two verbs —
   `mint_pending` (no DH, callable synchronously from the shell) and
   `start_attempt` (builds msg1 on the driver, §6.2) — and
   `ShellState::statics` / `StaticSlot` / `claim_static` /
   `release_static` in `src/shell/shared.rs` are **gone**, the shell
   reading `core::Endpoint`'s own static map instead.

   **Verified against the working tree, not assumed** (working rule 11: a
   rationale must name a mechanism that exists — rulings 87 and 89 both
   failed this, one day apart, and 87's failure *is* ruling 90). At the
   time of writing the tree is dirty with exactly this work:
   `src/core/endpoint/mod.rs:386` `fn mint_pending`,
   `:450` `fn start_attempt`, `src/shell/driver.rs:836`
   `.start_attempt(now(), id)`, and `src/shell/shared.rs` carrying
   `StaticSlot`/`claim_static`/`release_static` **only in prose that
   records their removal**. `git diff --stat` at that moment: six files,
   +354 / −404, all of them endpoint or shell.

   Slice 4 plans around it and **touches none of it**. If it has not been
   committed when 4a starts, 4a is unaffected (it lives entirely under
   `src/core/connection/**`); **4b is affected**, because
   `src/shell/shared.rs` and `src/shell/driver.rs` are on both work
   items' paths. Sequencing request in §14.3.

   *Two housekeeping notes for whoever integrates.* The tree also carries
   an untracked `tests/zz_scratch_keyorder.rs`, which is not slice 4's
   and should be removed or committed before anyone runs a mutation pass
   — **working rule 10**: mutation testing reverts with `git checkout
   <file>` and silently discards uncommitted work. And
   `src/core/tests.rs` is modified by ruling 90's work, so it is *not* a
   free path for anyone else until that lands.
2. **No wire byte moves.** Every frame slice 4 adds is already in §8.3's
   ratified table and already has a constant in `src/constants.rs`
   (`FRAME_RESET_STREAM`, `FRAME_STREAM_BASE`/`_MAX`, `FRAME_MAX_DATA`,
   `FRAME_MAX_STREAM_DATA`, `FRAME_MAX_STREAMS_BIDI`/`_UNI`,
   `STREAM_OFF`/`_LEN`/`_FIN`), as are all five flow-control constants
   and `REASSEMBLY_CHUNKS_MAX`. `src/constants.rs` needs **no new
   constant** for slice 4 except possibly the round-robin quantum, which
   §8.5 makes implementation-defined and which therefore must *not* go in
   the pinned constants table. A red `spec_constants.rs` in slice 4 means
   someone moved a wire byte.
3. **`frame.rs` already knows the six frame types exist.**
   `is_ack_eliciting()` and `retransmission()` already classify all of
   them (`src/core/connection/frame.rs:379–431`), and `Packing`'s doc
   comment already reserves `Stage::Fill` for slice 4's STREAM fill
   (`frame.rs:435–470`). Slice 4 adds parse/encode arms and a fill verb;
   it does not restructure the codec.
4. **Slice 3a's four-frame codec, the replay window, §7.4's liveness half,
   §15's CLOSE and post-mortem states, and §16.4's drain contract are
   frozen and correct**, as amended by `FIXES-3b.md` and
   `FIXES-3b-round2.md`. In particular `deadline()`'s position in
   `Driver::run` is load-bearing (FIXES-3b §4) and `Wakers::take_all` /
   `resolve_slot` exist so that no waker is invoked under a live borrow
   (FIXES-3b-round2 F10). **4b's per-stream waker maps must obey the same
   rule**, and that is stated as a 4b acceptance criterion in §11.
5. **`STREAMS_BLOCKED`, `DATA_BLOCKED` and `STREAM_DATA_BLOCKED` do not
   exist** (§10.4: "stays deferred with the rest of the BLOCKED family,
   §19"). A blocked sender in slice 4 is silent and waits.

## §2. Spec extract — §9 streams

Quoted so the implementer and test author need not re-open `SPEC.md`.
Line numbers are against `SPEC.md` at HEAD `da114c5`.

### §9.1 Stream identifiers (2838–2862)

> A stream ID is a varint. Its two low bits tag the stream; the remaining
> 60 bits are `index`, a per-space monotonically allocated counter from 0:
>
> | Bit | Meaning |
> |---|---|
> | `0x01` | opener: 0 = the connection initiator, 1 = the acceptor |
> | `0x02` | direction: 0 = bidirectional, 1 = unidirectional |
>
> This yields four independent ID spaces (initiator/acceptor × bidi/uni),
> QUIC's encoding verbatim. […]
>
> **Role stability.** The opener bit refers to the roles of the
> connection's **establishment**: the connection initiator is the dialler,
> or under simultaneous open the tie-break winner (§6.7). Parity is fixed
> at establishment and never changes for the connection's life.

Consequences for slice 4:
- `StreamId` is `varint(index << 2 | dir_bit << 1 | opener_bit)`. Note the
  table gives **bit masks** (`0x01`, `0x02`) on the *encoded* value, so
  `index = id >> 2`. 60 bits of index ⇒ max index `2^60 - 1`; the varint
  itself is §2's 62-bit varint, so **the id-space ceiling (2^62) and the
  index ceiling (2^60) are different numbers** — see §8 hunt item H1.
- "Role stability" is the tie to §6.7 simultaneous open. Slice 3 already
  establishes a role at handshake completion; slice 4 must **read** it,
  never re-derive it from who sent INIT.

### §9.2 Implicit opening (2864–2887)

> There is no OPEN frame. A frame referencing stream `N` of a space opens
> `N` and every lower-numbered not-yet-open stream of that space, subject
> to the cumulative limit (§10.4) — opening past it is
> `STREAM_LIMIT_ERROR`. The first STREAM or RESET_STREAM frame is the open.
>
> **The closed-stream watermark.** Each of the four spaces keeps, alongside
> its open set, the **highest fully-closed stream index** (§9.7). A STREAM
> or RESET_STREAM frame naming an index **at or below the watermark and not
> currently open** is a **no-op — processed as acknowledged, never
> re-opened**; implicit opening applies only to indices *above* the
> watermark. This tombstone is what makes exactly-once delivery real: a
> receive half frees at read-to-final (§9.7) and a sugar stream frees the
> instant its message surfaces (§9.8), both *before* the sender can know
> (only our ACK tells it), so a single lost ACK makes the peer's routine
> PTO retransmission re-name the freed stream — and without the watermark
> that retransmission would re-open it, restart the reassembler, re-pin the
> final size, and surface the same message twice (or fire a phantom
> `StreamOpened` for a finished stream). The watermark is monotone, lives
> for the connection's life, and costs one index per space. (An index at or
> below the watermark that is not open is necessarily a *closed* stream:
> implicit opening opened everything at or below the watermark when the
> watermark stream was first named.)

Consequences:
- The watermark is **per space** — four of them — and the parenthetical is
  the invariant that makes the "not currently open" test sufficient.
- "processed as acknowledged" means the frame must **not** be an error and
  must **not** suppress the packet's ACK. It is a silent drop that still
  counts as received.
- Note the watermark is described as "highest fully-closed stream **index**"
  while §9.7 defines full closure per *stream*. For remote-opened spaces a
  bidi stream has two halves; the watermark advances only when **both**
  are freed. See H2.

### §9.3 The send half (2889–2902)

> ```
> Ready ──write──▶ Send ──STREAM+FIN sent──▶ DataSent ──all ACKed──▶ DataRecvd (terminal)
>    │                │                          │
>    └────────────────┴──────reset()────────────▶ ResetSent ──RESET ACKed──▶ ResetRecvd (terminal)
> ```
>
> At the terminals the send half's state is freed (§9.7). The six-state
> diagram is exposition, not an implementation mandate: an implementation
> collapses it (quinn-proto's shape: `Ready` / `DataSent { finish_acked }` /
> `ResetSent`, with the terminals represented by removal).

### §9.4 The receive half (2904–2915)

> ```
> Recv ──STREAM+FIN──▶ SizeKnown ──all bytes──▶ DataRecvd ──app read all──▶ DataRead (terminal)
>    │                     │
>    └──RESET_STREAM───────┴──▶ ResetRecvd ──app read reset──▶ ResetRead (terminal)
> ```
>
> The receive half buffers arriving ranges and delivers the **contiguous
> prefix** to the application as it becomes available; a FIN pins the final
> size; the terminals free the state. The same collapse note applies
> (`Recv { size: Option<u64> }` / `ResetRecvd { size, error_code }`).

### §9.5 STREAM frame semantics (2917–2943)

> Each STREAM frame is a labelled byte range `(stream_id, offset, data)` of
> a per-stream logical byte sequence — not a self-contained message. […]
>
> - Ranges arrive in any order and may **overlap** (retransmission
>   re-framing, §8.7): a receiver delivers each byte exactly once; a byte
>   received twice with differing values is undefined behaviour of the
>   sender (an honest sender never produces it) and the receiver may keep
>   either.
> - **FIN pins the final size** as the frame's end offset
>   (`offset + data length`). Receiving data beyond a pinned final size,
>   a FIN pinning a size below already-received data, or two pins that
>   disagree ⇒ `FINAL_SIZE_ERROR` (CLOSE, §8.2).
> - Retransmitted stream bytes consume no new flow-control credit (§10.7);
>   data beyond advertised credit ⇒ `FLOW_CONTROL_ERROR`.
> - An empty STREAM frame with FIN is a valid end-of-stream marker; an
>   empty frame without FIN and without data is valid and a no-op
>   (tolerated, never emitted).
> - Reassembly memory is bounded twice over: by advertised credit (the span
>   a receiver must cover, §10.6) and by the reassembly-fragment mandate of
>   §10.6 — per-stream reassembly state MUST be O(advertised credit) and
>   MUST NOT scale with the number of received frames.

### §9.6 RESET_STREAM semantics (2945–2988)

> `reset(error_code)` abandons a send half abruptly: pending and in-flight
> data for the stream stop being retransmitted, and RESET_STREAM
> `{ stream_id, error_code, final_size }` is emitted (regenerated until
> acknowledged), where `final_size` is the number of bytes the stream would
> have carried (the end offset of the highest byte sent, or 0 if none),
> **truing up the receiver's connection-level flow-control accounting**:
> the receiver counts the full `final_size` against `MAX_DATA` consumption
> exactly as if the bytes had arrived (§10.1) […]. The receive half
> surfaces `ReadError::Reset(error_code)` (§18.1), discards its reassembly
> buffer, and closes when the application observes the reset. A RESET_STREAM
> for an already-FIN-complete receive half is a valid no-op if the final
> sizes agree, `FINAL_SIZE_ERROR` otherwise.
>
> The true-up runs only **after** the §8.4 `FLOW_CONTROL_ERROR` check that
> `final_size` does not exceed the advertised limits, so it releases
> exactly the credit the asserted bytes had already consumed and can never
> manufacture more; and when the receive half is retired, the same
> `final_size` counts as **consumed** for connection-level credit-advance
> (§10.3).
>
> **The receiver-emitted reset (the §8.4 exception).** In exactly one case
> the *receiver* of a uni stream emits RESET_STREAM — the message-mode
> overflow of §9.8. […] No flow-control true-up runs in this direction […]
> **Its delivery is reliable independent of the retired half** […] retained
> in the connection's regenerate set and re-emitted on loss **until
> acknowledged** […]

Slice 4 builds the **sender-emitted** reset only. The receiver-emitted
reset belongs to §9.8 (slice 6) — but see §12 for the seam it leaves.

### §9.7 Lifecycle and garbage collection (2990–3010)

> Stream state is freed eagerly:
>
> - a **send half** frees when every byte up to the final size, FIN
>   included, is acknowledged (`DataRecvd`), or when its RESET_STREAM is
>   acknowledged (`ResetRecvd`);
> - a **receive half** frees when the application has read to the final
>   size (`DataRead`), or has observed the reset (`ResetRead`), or — for an
>   abandoned handle — when the final size is reached with no reader
>   (§16.2);
> - a stream is **fully closed** when its halves (one for uni, two for
>   bidi) are freed; full closure is what earns the peer a MAX_STREAMS
>   credit (§10.4).
>
> The allocator never reuses a stream ID; churn is bounded by the free-list
> pattern (state lives only for open streams). Freeing is what advances the
> closed-stream watermark (§9.2) […]. Retiring a receive half also trues up
> connection-level credit for its unread bytes (§10.3).

## §3. Spec extract — §10 flow control

### §10.1 The model (3167–3185)

> Two levels, both receiver-driven, both expressed as **absolute byte
> offsets** (a limit says "you may send up to offset X", never "X more
> bytes"):
>
> - **Stream level**: each stream's data is bounded by the peer's
>   advertised per-stream limit (initially `INITIAL_MAX_STREAM_DATA`,
>   raised by MAX_STREAM_DATA).
> - **Connection level**: the **sum over all streams** of the highest
>   received offset (the final size, once pinned) — where a reset stream
>   contributes its trued-up `final_size` (§9.6) — is bounded by the peer's
>   connection limit (initially `INITIAL_MAX_DATA`, raised by MAX_DATA). A
>   sender respects both limits; whichever is tighter binds.
>
> Limits advance monotonically: a received credit frame applies as
> monotone-max, so duplicates and reordering are naturally idempotent
> (§8.4).

### §10.2 Initial values (3187–3202)

> | Constant | Value |
> |---|---|
> | `INITIAL_MAX_DATA` | 1 048 576 B (1 MiB) |
> | `INITIAL_MAX_STREAM_DATA` | 262 144 B (256 KiB) |
> | `INITIAL_MAX_STREAMS_BIDI` | 32 (cumulative) |
> | `INITIAL_MAX_STREAMS_UNI` | 128 (cumulative — higher for message traffic, §9.8) |
> | `STREAMS_CREDIT_BATCH` | 8 |
>
> […] initial windows are protocol constants, identical in both directions
> and all stream spaces; later credit is receiver policy. The values ship
> **ratified-but-revisitable**, gated on the Appendix B window-constants
> throughput validation […]

### §10.3 Advancing credit (3204–3244)

> […] with `WINDOW` the level's window (`INITIAL_MAX_STREAM_DATA` for a
> stream, `INITIAL_MAX_DATA` for the connection), the **prospective limit**
> is `bytes_read + WINDOW`, and the receiver emits MAX_STREAM_DATA or
> MAX_DATA when `prospective_limit − last_advertised ≥ WINDOW/2` […]
> Consumption, not arrival, drives credit: an unread buffer earns nothing.
> Credit frames are ack-eliciting […] yet liveness-neutral (§7.4) — […]
> they are **non-marking**, sealed via `seal_quiet`, leaving `last_send`
> untouched so they defer no keepalive; and, being ack-eliciting, they
> **arm the death clock** like any other ack-eliciting send (§7.4). […]
>
> **Retirement advances connection credit.** When a receive half is retired
> for any reason […] **all of its bytes up to its final size count as
> consumed for connection-level credit-advance**, exactly as if the
> application had read them; stream-level credit is simply never re-granted
> for a retired stream […] The true-up is **absolute, not additive**: it
> advances the stream's contribution to the connection-level consumed count
> **to** its `final_size` — a monotone bring-to-final, idempotent with
> bytes already counted by reads (§10.1's per-stream absolute sum) — and
> never adds `final_size` on top of them […]

**Load-bearing for the implementer:** the connection-level consumed count
is *not* a scalar accumulator you add to. §10.3 says it is a **per-stream
absolute sum**: `consumed = Σ_streams contribution(s)`, and retirement
sets `contribution(s) := final_size(s)`. A scalar `consumed += n` cannot
be made idempotent under a read-then-retire sequence. The natural
implementation keeps, per live receive half, `counted_consumed` (bytes
already folded into the connection scalar) and folds only the delta; on
retirement it folds `final_size − counted_consumed`. That is the same
arithmetic, expressed so the absolute rule is enforced structurally.

### §10.4 Stream limits — cumulative credit (3246–3267)

> - The limit counts **streams ever opened** in a space; opening stream
>   index `i` requires cumulative limit > `i`.
> - The receiver grants +1 as it **fully closes a peer-opened stream** of
>   the space (§9.7) — closing streams we opened must not inflate the
>   peer's allowance (RFC 9000 §4.6's scope) — batching advertisements:
>   emit MAX_STREAMS when ≥ `STREAMS_CREDIT_BATCH` (8) grants are
>   unadvertised, **or** when the peer's remaining allowance drops to ≤ 8.
>   Receipt of MAX_STREAMS surfaces `ConnEvent::StreamsAvailable { dir }`
>   to wake blocked openers (§16.4).
> - Opening beyond the limit ⇒ `STREAM_LIMIT_ERROR` ⇒ CLOSE (§8.2).
> - `STREAMS_BLOCKED` stays deferred with the rest of the BLOCKED family
>   (§19).

### §10.5 Violations (3269–3274)

> A peer exceeding advertised credit — stream or connection level — is a
> protocol violation: CLOSE with `FLOW_CONTROL_ERROR`. A peer opening
> beyond a stream limit: CLOSE with `STREAM_LIMIT_ERROR`. There is no
> tolerance band; the limits are exact (§8.2's semantic class).

### §10.6 Credit is the buffer commitment (3276–3328)

> The advertised credit **is** the receiver's buffer commitment: a receiver
> only advertises what it will buffer until read. […] an in-credit packet
> always has buffer room **by construction**, so no delivered-but-shed
> state can exist, and a beyond-credit packet is a violation (§10.5), not a
> shed. […] the supporting check: **no non-stream, non-datagram frame can
> force unbounded buffering** — ACK processing is bounded intersecting
> (§12.5), credit frames apply as O(1) monotone-max, PING/PADDING are O(1),
> RESET_STREAM *frees* state, and CLOSE enters the linger. […]
>
> **The second bound — reassembly fragments.** […] The mandate: **per-stream
> reassembly state MUST be O(advertised credit) and MUST NOT scale with the
> number of received frames.** Two implementations are admissible: (a) a
> span-allocated buffer plus a received-bitmap […]; or (b) the default —
> received ranges are coalesced on insert, and a stream whose stored
> discontiguous ranges would exceed `REASSEMBLY_CHUNKS_MAX` (= 1024) after
> coalescing is a protocol violation: CLOSE with `PROTOCOL_VIOLATION`
> (§15.3; quinn's defragment-plus-hard-fail shape). […]
>
> | Constant | Value |
> |---|---|
> | `REASSEMBLY_CHUNKS_MAX` | 1024 stored discontiguous ranges per stream |
>
> **Consumption, defined.** Consumption is **the application taking bytes
> out of the connection core** — a `read()` draining the contiguous prefix,
> a message or datagram claimed by its verb (§16.4), or a retirement
> true-up (§10.3). No unbounded intermediate queue may exist between core
> and handle […] and reliable stream or message data MUST NOT be droppable
> under the shell's non-blocking delivery policy (§16.8) — §16.4's pull
> model is what makes both properties implementable.

### §10.7 Exemptions (3330–3336)

> - **DATAGRAM frames are flow-control-exempt** […]
> - **Retransmissions of the same stream bytes consume no new credit** —
>   credit accounts the stream's offset high-water mark, not bytes on the
>   wire.

## §4. Spec extract — §8.3/§8.4 frames added by this slice

### §8.3 table rows added (2643–2649)

> | Type | Frame | Fields (all varints) | Ack-eliciting | Retransmission | Home |
> |---|---|---|---|---|---|
> | `0x04` | RESET_STREAM | stream_id, error_code, final_size | yes | regenerate | §9.6 |
> | `0x05` | (reserved: STOP_SENDING) | — | — | — | §19 |
> | `0x08`–`0x0f` | STREAM | stream_id, [offset], [length], data; OFF = 0x04, LEN = 0x02, FIN = 0x01 | yes | ranges | §9.5 |
> | `0x10` | MAX_DATA | max | yes | regenerate | §10.3 |
> | `0x11` | MAX_STREAM_DATA | stream_id, max | yes | regenerate | §10.3 |
> | `0x12` | MAX_STREAMS_BIDI | max (cumulative) | yes | regenerate | §10.4 |
> | `0x13` | MAX_STREAMS_UNI | max (cumulative) | yes | regenerate | §10.4 |
>
> `0x05` is *reserved*, not implemented: like any unknown type, receiving it
> is a structural failure — CLOSE with `PROTOCOL_VIOLATION` (§8.2).

### §8.4 RESET_STREAM (`0x04`) — 2679–2700

> ```
> type(0x04) ‖ stream_id(varint) ‖ error_code(varint) ‖ final_size(varint)
> ```
>
> […] Semantic violations: a `stream_id` naming a stream the sender of the
> frame could not send on (their receive-only half) ⇒ `STREAM_STATE_ERROR`
> — with exactly one exception, the message-mode overflow reset of §9.8
> […]; a `final_size` below the receiver's highest-received offset, or
> conflicting with an already-pinned final size ⇒ `FINAL_SIZE_ERROR`; a
> `final_size` that would push stream- or connection-level consumption above
> the advertised limit ⇒ `FLOW_CONTROL_ERROR`, checked **before** the
> §9.6/§10.3 credit true-up, with checked or saturating `u64` arithmetic
> mandated (an unchecked sum wraps for large `final_size` values and
> silently re-opens the window). A RESET_STREAM naming an index at or below
> the space's closed-stream watermark and not currently open is a no-op —
> ACKed, never re-opened (§9.2).

### §8.4 STREAM (`0x08`–`0x0f`) — 2702–2728

> ```
> type(0x08 | OFF(0x04) | LEN(0x02) | FIN(0x01))
>      ‖ stream_id(varint)
>      ‖ [ offset(varint)   if OFF ]
>      ‖ [ length(varint)   if LEN ]
>      ‖ data(length bytes, or to the end of the plaintext if ¬LEN)
> ```
>
> | Constant | Value |
> |---|---|
> | `STREAM_OFF` / `STREAM_LEN` / `STREAM_FIN` | 0x04 / 0x02 / 0x01 (bits of the type byte) |
>
> OFF absent ⇒ offset 0. LEN absent ⇒ the data extends to the end of the
> plaintext, and the frame must be the packet's final frame. FIN marks the
> data's end offset as the stream's final size (an empty FIN-only frame is
> valid). Semantics in §9.5. Structural errors: `length` overrunning the
> plaintext; a ¬LEN frame that is not final; `offset + length` exceeding
> 2⁶² − 1. Semantic violations: data beyond stream or connection credit ⇒
> `FLOW_CONTROL_ERROR`; a `stream_id` the peer could not send on ⇒
> `STREAM_STATE_ERROR`; opening a stream beyond the cumulative limit ⇒
> `STREAM_LIMIT_ERROR`; data beyond, or a FIN conflicting with, a pinned
> final size ⇒ `FINAL_SIZE_ERROR`. A frame naming an index at or below the
> space's closed-stream watermark and not currently open is a no-op —
> ACKed, never re-opened (§9.2). All offset arithmetic (`offset + length`,
> final-size and credit comparisons) is checked or saturating.

### §8.4 MAX_DATA / MAX_STREAM_DATA — 2730–2747

> ```
> type(0x10) ‖ max(varint)
> type(0x11) ‖ stream_id(varint) ‖ max(varint)
> ```
>
> Absolute-offset credit grants (§10). Monotone-max on receipt: a value not
> above the current limit is a valid no-op […]. Semantic violations:
> MAX_STREAM_DATA for a stream the *receiver of the frame* cannot send on ⇒
> `STREAM_STATE_ERROR`; likewise — QUIC's rule — MAX_STREAM_DATA for a
> stream in a space the frame's receiver opens that the receiver has not yet
> opened (credit frames never open streams; §9.2's implicit opening is for
> STREAM and RESET_STREAM only). Credit for a fully-closed stream (at or
> below the watermark, §9.2) is a valid no-op.

### §8.4 MAX_STREAMS_BIDI / MAX_STREAMS_UNI — 2749–2757

> ```
> type(0x12|0x13) ‖ max(varint, cumulative stream count)
> ```
>
> Cumulative-count credit for the corresponding space (§10.4). Monotone-max
> on receipt. Ack-eliciting; regenerated. Structural error: `max` > 2⁶⁰
> (unrepresentable as a stream index) — §8.2's structural class.

### §8.5 Coalescing and packing order (2789–2800)

> Within a packet the sender packs in this order: the ACK first (if owed),
> then control frames (credit grants, RESET_STREAM, CLOSE), then STREAM and
> DATAGRAM fill, then PING last if a probe still owes ack-eliciting content.
> At most one extends-to-end frame (¬LEN STREAM, or `0x30` DATAGRAM) per
> packet, in final position. Within the STREAM fill, streams with pending
> data are served **round-robin** — one quantum per stream per fill pass,
> the quantum size implementation-defined — which is what makes the
> no-head-of-line-blocking contract real under contention (§9.8).

### §8.7 Retransmission classes (2809–2834)

> - **ranges** (STREAM): the lost packet's stream ranges return to the
>   pending set and are re-framed on fresh counters — split, merged, or
>   coalesced with new data freely; only still-un-ACKed sub-ranges are
>   resent.
> - **regenerate** (MAX_DATA, MAX_STREAM_DATA, MAX_STREAMS_BIDI/UNI,
>   RESET_STREAM): the lost frame's *identity* re-queues, and the
>   retransmission carries the **freshest current value** […]. (For
>   RESET_STREAM the values are fixed at reset time; it re-emits until
>   acknowledged or the stream state is discarded […])

**Slice-boundary note.** §8.7's classes are *loss-recovery* rules and
loss recovery is §13 — slice 7. Slice 4 must produce the **state** those
classes act on (a pending-range set per send half, a regenerate set of
frame identities) but must not implement detection or the retransmit
trigger. See §12 for the exact seam.

## §5. Spec extract — §16.9 early sends, §16.4 poll contract deltas

### §16.9 Early sends (4749–4766)

> Queued work before establishment is **ordinary work**: the connection core
> exists from `connect()`, and early stream opens, writes, messages, and
> datagrams land in ordinary stream/queue state, pumping when a session
> installs — delivered exactly once after establishment, lost if the connect
> fails (the failure surfaces through `Connecting`). There is no special
> pre-establishment mechanism.
>
> **Stream identity before establishment.** Wire stream IDs encode opener
> parity, which is fixed only at establishment (a tie-break loss makes the
> dialler the acceptor — §6.7, §9.1), so **stream IDs are assigned at
> establishment**: pre-establishment handles hold core-internal indices, no
> frame is emitted before install (nothing sends until a session exists),
> and `id()` returns `None` until the connection is established (§16.2). On
> install the core maps its internal indices onto the parity the outcome
> dictates, in open order — the on-wire IDs are identical whichever
> resolution the race takes.

### §16.4 — the parts slice 4 lands (4491–4526)

The core `Connection` API surface slice 4 must complete:

```rust
fn open(&mut self, dir: Dir) -> Result<StreamId, StreamsExhausted>;
fn write(&mut self, now: Instant, id: StreamId, data: &[u8]) -> Result<usize, WriteError>;
fn finish(&mut self, id: StreamId) -> Result<(), WriteError>;
fn reset(&mut self, now: Instant, id: StreamId, error_code: u64);
fn read(&mut self, id: StreamId, buf: &mut [u8]) -> Result<Option<usize>, ReadError>;
fn accept(&mut self, dir: Dir) -> Option<StreamId>;   // claim a peer-opened stream
```

and the `ConnEvent` variants slice 4 must start emitting:

```rust
StreamOpened { dir: Dir },                   // signal: claim via accept(dir)
StreamsAvailable { dir: Dir },               // MAX_STREAMS credit arrived (§10.4)
StreamReadable { id: StreamId },
StreamWritable { id: StreamId },             // stream/connection credit arrived for a blocked writer
StreamFinished { id: StreamId },             // send half fully acknowledged
StreamReset { id: StreamId, error_code: u64 },
```

Governing prose (4587–4598):

> **The pull model is uniform.** The core **retains** what it has not
> handed over: reassembled-but-unclaimed incoming uni streams, queued
> received datagrams, and peer-opened streams awaiting `accept(dir)`. The
> receive-side `ConnEvent`s are **signals**, not payload carriers — the
> shell wakes the matching blocked verb, and the verb claims through the
> core (`accept`, `recv_message`, `recv_datagram`, `read`). […] This is what
> §10.6's no-unbounded-intermediate-queue rule and §16.8's
> no-drop-for-reliable-data rule rest on: nothing reliable ever sits in a
> droppable shell channel.

and (4632–4636):

> **`Timeout(None)`** = drained and no deadline armed […]
> **Output ordering within one drain preserves generation order** — a
> transmit and the event it caused come out in that order. Normative;
> tests and logs depend on it.

**Note for the shell author.** `open()` returns
`Result<StreamId, StreamsExhausted>` in §16.4, but §16.9 says `id()`
returns `None` until established. These are reconciled in §9 below
(conflict C1): the core-level `open()` cannot return a wire `StreamId`
before install. Do not resolve it silently.

## §6. Module map and file ownership

**Working rule 6 is absolute: no two concurrent agents may name the same
path.** In slice 2a an implementer's placeholder stub overwrote 68
independently written tests because two briefs named
`src/core/tests.rs`. The tables below are the partition; a brief that
deviates from one has a race in it.

**A second, separate constraint** — flagged because it invalidated two
files in slice 3's plan: `tests/*.rs` are **integration tests** and can
reach only slither's *public* API. `src/core` is `pub(crate)`
(`src/lib.rs:127`). **No test of `core::Connection`'s stream machinery
can live in `tests/`.** All of 4a's tests are in-crate.

### §6.1 Phase 4a — files

| Path | New? | Owner | Contents |
|---|---|---|---|
| `src/core/connection/stream_id.rs` | new | **4a-impl** | `StreamId`, `Dir`, `Opener`, the four `Space`s; encode/decode against §9.1's two-bit tag; `index()`, `dir()`, `opener()`; the 2⁶⁰ index ceiling |
| `src/core/connection/send.rs` | new | **4a-impl** | §9.3's send half collapsed to `Ready`/`DataSent{fin_acked}`/`ResetSent`; the write buffer, the pending-range set, the un-ACKed retention set, `finish()`, `reset()`, the `on_ack_range` / `on_lost_range` entry points (defined and unit-tested here, **wired to the wire in slice 5**) |
| `src/core/connection/recv.rs` | new | **4a-impl** | §9.4's receive half; the coalescing reassembler; `REASSEMBLY_CHUNKS_MAX`; final-size pinning; contiguous-prefix `read()`; `ReadError::Reset` surfacing |
| `src/core/connection/streams.rs` | new | **4a-impl** | the four-space table, open sets, the four closed-stream watermarks, §9.2 implicit opening, §9.7 GC and full-closure detection, §16.9's internal-index→wire-id remap at install, the `accept(dir)` claim queue |
| `src/core/connection/flow.rs` | new | **4a-impl** | §10 entire: the two credit ledgers, §10.3's re-grant trigger and the absolute-not-additive retirement true-up, §10.4's cumulative limits with both emission triggers, §10.5's two violations |
| `src/core/connection/frame.rs` | **edit** | **4a-impl** | parse/encode arms for the six new frames; `Stage::Fill`'s `stream()` verb and the round-robin fill; the extends-to-end rule |
| `src/core/connection/mod.rs` | **edit** | **4a-impl** | the six `core::Connection` verbs; the six new `ConnEvent` variants; `mod` declarations, **including `#[cfg(test)] mod tests_streams;` — declared here, file created by nobody but 4a-test** |
| `src/core/connection/tests_streams.rs` | new | **4a-test** | every 4a test. **4a-impl creates nothing at this path** (working rule 6). |
| `src/constants.rs` | **edit** | **4a-impl** | nothing expected (§1.2); if a `const` is genuinely needed it is *not* added to the pinned `spec_constants` table without a ruling |

`src/core/connection/tests.rs` (3 105 lines, slice 3a's test author's
file) is **not** on either 4a agent's path. If a slice-3a test needs
adjusting because a `ConnEvent` enum gained variants, that is an
integration edit made by whoever integrates 4a, **after** both agents
finish — never concurrently.

### §6.2 Phase 4b — files

| Path | New? | Owner | Contents |
|---|---|---|---|
| `src/shell/stream.rs` | new | **4b-impl** | `SendStream`, `RecvStream`, `BiStream`; `poll_write`/`poll_finish`/`poll_read` written **once** as `poll_*` per ruling 53; `id() -> Option<StreamId>`; both `Drop` impls |
| `src/shell/connection.rs` | **edit** | **4b-impl** | `open_bi`, `open_uni`, `accept_bi`, `accept_uni` as `poll_fn` over `poll_*` |
| `src/shell/shared.rs` | **edit** | **4b-impl** | per-`StreamId` blocked-reader / blocked-writer waker maps in `ConnCell`; the `StreamsAvailable` and `StreamOpened` waker sets |
| `src/shell/driver.rs` | **edit** | **4b-impl** | routing the six new `ConnEvent`s to wakers — **using `take_all` and waking outside the borrow** (F10) |
| `src/shell/mod.rs` | **edit** | **4b-impl** | `pub use` of the three handle types and `StreamId`/`Dir` |
| `src/lib.rs` | **edit** | **4b-impl** | re-exports |
| `tests/story_streams.rs` | new | **4b-test** | S12 (loss-free), S13, S14, S17 |
| `tests/spec_streams.rs` | new | **4b-test** | public-API-reachable §9/§10 obligations: two-sided boundaries on the handle surface, drop semantics, cancel-safety |

**`src/error.rs` is on nobody's path.** §18.1 is closed (ruling 61) and
`WriteError`/`ReadError` already carry exactly the variants slice 4 needs
(`Reset(u64)`, `ConnectionLost(_)`, `Finished`). `StreamsExhausted` is a
**core-internal** type and must not reach the public error surface — see
H13.

### §6.3 The one shared-path hazard, named

`src/shell/shared.rs` and `src/shell/driver.rs` are edited by **4b-impl**
*and* by **ruling 90's separate work item**. That is a two-work-item
collision on two files, and it is the same shape as the slice-2a
accident even though the agents are in different slices. **4b must not
start until ruling 90's split has landed and been committed.** Stated
again in §14 as a request.

### §6.4 Where the round-robin quantum lives

§8.5: *"one quantum per stream per fill pass, the quantum size
implementation-defined"*. It is therefore **not** a wire constant and must
not enter `src/constants.rs`'s pinned table — a value in that table is
asserted by `tests/spec_constants.rs`, which would turn an
implementation-defined choice into a wire pin by accident. Put it in
`frame.rs` as a private `const` with a comment naming §8.5.

## §7. Story-to-test mapping

Stories are quoted from `STORIES.md`. Every test below is
`#[tokio::test(start_paused = true)]` over `testutil::Network` +
`FlakyWire`; **no `sleep`** (§16.10, working rule "new behaviour gets a
paused-clock flow test, not a sleep").

### §7.1 The four stories (phase 4b, `tests/story_streams.rs`)

| Story | Accepts (quoted) | Test | What the broken build does |
|---|---|---|---|
| **S12** (loss-free half) | *"open a stream, write, the peer reads the same bytes in the same order with no gaps or duplicates, `finish()` delivers the FIN, the reader observes end-of-stream. Survives loss, reordering and duplication on the path."* | `s12_a_user_can_stream_over_a_reordering_duplicating_path` | see §11.1 |
| **S13** | *"concurrent streams are independent; loss on one does not stall another. Stream IDs carry the initiator/responder parity fixed at S4."* | `s13_a_stalled_stream_does_not_block_a_concurrent_one` + `s13_stream_ids_carry_establishment_parity` | see §11.2 |
| **S14** | *"reset a stream; the peer surfaces `ReadError::Reset(code)`; other streams and the connection are unaffected."* | `s14_a_reset_stream_leaves_the_connection_and_its_siblings_alive` | see §11.3 |
| **S17** | *"flow-control credit bounds unacknowledged data per stream and per connection; a reader that stops reading stalls its own stream, not the connection, and the sender learns rather than buffering without limit."* | `s17_a_slow_reader_stalls_its_own_stream_only` + `s17_the_sender_resumes_when_the_reader_drains` | see §11.4 |

**S12's name must say `loss_free`.** `PLAN.md` gives slice 4 "S12
(lossless)" and slice 5 "S12 (full, over `FlakyWire`)", and the story text
demands loss survival, which slice 4 cannot deliver (§12). A test named
`s12_a_user_can_stream` would be a name that is not a pin (working rule
9) — it would let slice 5 ship without the loss half. Recommended name:
`s12_loss_free_a_user_can_stream_over_a_reordering_duplicating_path`.

**"Loss-free" is not "impairment-free", and the test must exploit that.**
Reordering and duplication are handled by the reassembler and the §7.2
replay window, **not** by loss recovery. So slice 4's S12 runs over a
`FlakyPolicy` with **reorder and duplicate on, loss off** — a materially
stronger test than the phrase "lossless" suggests, and the one that
actually exercises §9.5's overlap rule. Deterministic under its seed
(§16.10, ruling 60).

### §7.2 The three precursors (phase 4a, `src/core/connection/tests_streams.rs`)

Named `*_precursor_*` so they can never be mistaken for the stories.

| Precursor | Content | Closed by |
|---|---|---|
| `s12_precursor_two_cores_exchange_a_finished_stream` | two `core::Connection`s over the in-crate two-core fixture: `open` → `write` → `finish` → peer `accept(dir)` → `read` to `Ok(None)`; bytes identical, FIN observed | S12 in 4b |
| `s13_precursor_two_streams_reassemble_independently` | stream A's frames withheld while stream B's are delivered; B readable, A not; then A delivered and both complete | S13 in 4b |
| `s17_precursor_the_credit_ledger_stalls_and_resumes` | fill the stream window, assert `write` returns `Ok(0)`/blocked at the core level, drain the reader, assert MAX_STREAM_DATA emitted at exactly the §10.3 trigger, assert the writer unblocks | S17 in 4b |

There is deliberately **no S14 precursor**: S14's protocol content
(RESET_STREAM out, `ReadError::Reset` in) is covered by the §9.6
obligation tests in the same file, and a precursor would duplicate them
without adding a claim.

### §7.3 Obligation tests (both phases)

Appendix B's eleven §9/§10 obligations (§13) map as:

| Obligation | Phase | File |
|---|---|---|
| reassembly under reorder/overlap/duplication; FIN pinning; every `FINAL_SIZE_ERROR` case | 4a | `tests_streams.rs` |
| the closed-stream tombstone (receive-side; see §13 for the deferred half) | 4a | `tests_streams.rs` |
| flow-control stall-and-resume both levels; the §10.3 re-grant formula | 4a | `tests_streams.rs` |
| `FLOW_CONTROL_ERROR` on over-credit data | 4a | `tests_streams.rs` |
| discard-credit: observed resets and abandoned halves true up | 4a (ledger) + 4b (handle drop) | both |
| the §8.4 bound check runs *before* the true-up; no `u64` wrap | 4a | `tests_streams.rs` |
| the reassembly-fragment bound and `REASSEMBLY_CHUNKS_MAX` kill | 4a | `tests_streams.rs` |
| MAX_STREAMS replenishment: batch-8, low-allowance, peer-opened-only | 4a | `tests_streams.rs` |
| `STREAM_LIMIT_ERROR`, two-sided at the limit | 4a | `tests_streams.rs` |
| §8.5 packing order and one-extends-to-end (extended to STREAM) | 4a | `tests_streams.rs` |
| §16.2 drop semantics for both half-handles | 4b | `spec_streams.rs` |

## §8. The unstated-scope hunt (working rule 8)

*A stated construction with an unstated or contradicted scope.* Thirteen
instances across four slices, none of them a wrong value. Slice 4's spec
range is unusually rich in lists and tables — §8.3's frame table, §9.1's
id table, §10.2's constants table, §10.5's violation list, §10.6's
consumption list, §8.4's per-frame error lists — so this was hunted
deliberately, list by list, asking of each: *what bounds this, and does
the text say?*

Items marked **[candidate ruling]** need a maintainer decision. Items
marked **[resolved in-spec]** are recorded because an implementer will
otherwise re-derive them wrongly; the spec does answer them, and where it
does the answer is quoted.

---

### H1 — §9.1's index ceiling vs the varint's value ceiling **[resolved in-spec]**

§9.1: *"the remaining 60 bits are `index`"*. §8.1's varint holds 62 bits.
2 + 60 = 62, so `index = id >> 2` can never exceed 2⁶⁰ − 1 for any
*decodable* `stream_id`; there is no reachable "index too large" case on
the STREAM path, and no check is needed there. The ceiling is reachable
only on MAX_STREAMS, where §8.4 states it: *"Structural error: `max` >
2⁶⁰"*.

Note the boundary is **`>`, not `≥`**: `max = 2⁶⁰` is legal, because
§10.4's *"opening stream index `i` requires cumulative limit > `i`"*
makes `max = 2⁶⁰` exactly the limit that admits the largest representable
index, 2⁶⁰ − 1. Test it two-sided (slice 1's lesson): `2^60` accepted,
`2^60 + 1` structural.

### H2 — "fully closed" is a **local**, per-endpoint predicate **[resolved in-spec, but easy to get wrong]**

§9.7: *"a stream is **fully closed** when its halves (one for uni, two for
bidi) are freed"*. For a locally-opened uni stream this endpoint holds
one half (send); the peer holds one half (receive). The two ends
therefore reach "fully closed" at **different moments and for different
reasons**, and that is correct: the watermark is a receiver-side
tombstone and MAX_STREAMS credit is granted only for **peer-opened**
streams (§10.4). An implementer who models "fully closed" as a shared
property of a stream will build a synchronisation that does not exist.

### H3 — the watermark exists in all **four** spaces, but does different work in two of them **[candidate ruling — low cost]**

§9.2: *"**Each of the four spaces** keeps, alongside its open set, the
highest fully-closed stream index"*. Its stated purpose — no-op'ing a
peer's late STREAM/RESET_STREAM retransmission — applies only to the two
**peer-opened** spaces. For the two spaces *we* open, the only late frame
a peer can legitimately send is a **credit** frame, and §8.4 covers that
separately: *"Credit for a fully-closed stream (at or below the watermark,
§9.2) is a valid no-op."* So all four watermarks are load-bearing, but two
of them serve §8.4's rule rather than §9.2's. The construction is stated
for four spaces; its rationale is written for two. Not a defect — but
state it, or an implementer will maintain two watermarks and be surprised
by a `STREAM_STATE_ERROR` where §8.4 promised a no-op.

**Interacts with H4.**

### H4 — which check wins: §9.2's watermark no-op, or §8.4's `STREAM_STATE_ERROR`? **[candidate ruling — medium cost, security-relevant]**

§9.2 says a STREAM or RESET_STREAM frame *"naming an index at or below the
watermark and not currently open is a **no-op** — processed as
acknowledged, never re-opened"*. §8.4 says *"a `stream_id` the peer could
not send on ⇒ `STREAM_STATE_ERROR`"*. For a **locally-opened uni** stream
the peer can *never* send STREAM — so a STREAM frame naming a
fully-closed local uni index satisfies **both** rules, which give opposite
answers (silent ACK vs. kill the connection). The spec orders neither.

The same collision exists for the reverse half of a locally-opened bidi
stream once that half is freed, and for RESET_STREAM.

**Provisional (most defensible, test-pinnable):** the **space/direction
legality check runs first**. The watermark answers *"which index"*; the
state error answers *"who may send"*. A frame the peer could never
legally send at any index is not a late retransmission of anything, and
ordering the watermark first would delete a violation check for exactly
the streams an attacker can most cheaply name. Pin it with
`a_stream_frame_on_a_closed_local_uni_space_is_a_state_error`; reversing
the ruling is an edit to that one test's expectation.

### H5 — §8.4's per-frame error lists are unordered **[candidate ruling — medium cost]**

§8.4's STREAM entry lists four semantic violations —
`FLOW_CONTROL_ERROR`, `STREAM_STATE_ERROR`, `STREAM_LIMIT_ERROR`,
`FINAL_SIZE_ERROR` — as a set, with no evaluation order. A single crafted
frame can trip several at once (e.g. index above the cumulative limit
*and* data beyond credit). All four kill the connection, so the only
observable difference is **the error code on the wire** — which is
exactly what Appendix B's obligations assert on, and what a peer's
operator reads. A list read as exhaustive (working rule 8) is still not a
list read as *ordered*.

**Provisional order, from cheapest-and-most-specific to most-general:**

1. `STREAM_STATE_ERROR` — is this peer allowed to send this frame at all?
   (H4 puts it first regardless.)
2. watermark no-op — is this frame inert?
3. `STREAM_LIMIT_ERROR` — may this index exist?
4. `FINAL_SIZE_ERROR` — is this frame consistent with what we already know
   about this stream?
5. `FLOW_CONTROL_ERROR` — does it fit in the credit we advertised?

Rationale for putting flow control **last**: §8.4 and §9.6 both mandate
that the credit bound check runs *before* any true-up, and both mandate
checked/saturating arithmetic; making it the final gate means the ledger
is consulted exactly once per frame, after the frame is known to be
otherwise legal. Rationale for `FINAL_SIZE_ERROR` before
`FLOW_CONTROL_ERROR`: a frame contradicting a pinned final size is a
statement about a stream we already fully understand, and answering it
with a credit code would mislead.

RESET_STREAM's list orders itself partially — §8.4 states
`FLOW_CONTROL_ERROR` is *"checked **before** the §9.6/§10.3 credit
true-up"* — but says nothing about `FINAL_SIZE_ERROR` vs
`FLOW_CONTROL_ERROR`. Same provisional: final size first.

### H6 — is the "≤ 8" low-allowance threshold `STREAMS_CREDIT_BATCH`, or a second unnamed constant? **[candidate ruling — low cost, not separable by test]**

§10.4: *"emit MAX_STREAMS when ≥ `STREAMS_CREDIT_BATCH` (8) grants are
unadvertised, **or** when the peer's remaining allowance drops to ≤ 8."*
The first threshold is a **named** constant; the second is a **literal**.
§10.2's table declares exactly one constant of value 8. Either they are
the same constant written two ways, or the second is an unnamed magic
number with no home in the constants table and no revisitability note.

**This one cannot be separated by a test** — both readings produce
identical behaviour at today's values — which makes it a ruling question
rather than a test-design question, and worth stating for exactly that
reason (the inverse of working rule 9: a difference no test can see needs
the ruling *more*, not less).

**Provisional:** one constant. Use `STREAMS_CREDIT_BATCH` in both places
in the code, so that if the maintainer later revisits the value both
triggers move together — which is the behaviour the batching rationale
implies (a batch of 8 and a headroom of 8 are the same "one batch of
slack").

### H7 — are MAX_STREAMS_BIDI/UNI non-marking? §10.3 says "credit frames" but its subject is the other two **[candidate ruling — medium cost, lands in slice 4]**

§10.3's subject line is the MAX_STREAM_DATA / MAX_DATA re-grant rule.
Inside it: *"**Credit frames** are ack-eliciting (loss recovery
regenerates them with the freshest value, §8.7) yet liveness-neutral
(§7.4) — and *liveness-neutral* carries ruling 33's exact sense, both
halves of it: they are **non-marking**, sealed via `seal_quiet`, leaving
`last_send` untouched so they defer no keepalive; and, being
ack-eliciting, they **arm the death clock**."*

§10.4 defines two more credit frames and repeats none of this. So: does
"credit frames" mean the two frames §10.3 is about, or all four? **A
stated construction with an unstated scope**, and it is decided *in slice
4* because the two seal paths landed in 3a and every new frame must
choose one.

**Provisional: all four are non-marking.** The rationale §10.3 gives —
*"A connection whose only output is credit is not thereby exempt from
dying"* — is a statement about credit as a class, and a connection whose
only output is MAX_STREAMS has exactly the same problem. Test-pinnable
(§11.6).

**And the omission's other half, which no section states at all:** §10.3
tells us credit frames are non-marking and §3.4/§7.4 tell us keepalives
are; **nothing says what RESET_STREAM and STREAM use.** By omission they
take the ordinary marking `seal`. Slice 4 must therefore publish this
table, because no spec section contains it:

| Frame added by slice 4 | Seal path | Ack-eliciting | Source |
|---|---|---|---|
| STREAM `0x08`–`0x0f` | `seal` (marking) | yes | §8.3 + omission from §10.3 |
| RESET_STREAM `0x04` | `seal` (marking) | yes | §8.3 + omission from §10.3 |
| MAX_DATA `0x10` | **`seal_quiet`** | yes | §10.3, explicit |
| MAX_STREAM_DATA `0x11` | **`seal_quiet`** | yes | §10.3, explicit |
| MAX_STREAMS_BIDI `0x12` | **`seal_quiet`** (provisional) | yes | H7 |
| MAX_STREAMS_UNI `0x13` | **`seal_quiet`** (provisional) | yes | H7 |

A packet mixing a credit frame with a STREAM frame is **marking** — the
marking property belongs to the seal, not the frame, and one marking
frame makes the packet marking. Nothing states that either; it follows
from `seal`/`seal_quiet` being a per-seal choice.

### H8 — §10.6's memory arithmetic is done at the wrong level **[candidate ruling — high cost; also conflict C5]**

§10.6's mandate is **per stream**: *"per-stream reassembly state MUST be
O(advertised credit) and MUST NOT scale with the number of received
frames."* Its worked example is **per connection**: *"(a) a span-allocated
buffer plus a received-bitmap (a **1 MiB** span costs 1 MiB + 128 KiB,
frame-count-independent…)"*. 1 MiB is `INITIAL_MAX_DATA`, the connection
window. The per-stream window is `INITIAL_MAX_STREAM_DATA` = 256 KiB.

The gap is not cosmetic. Take admissible implementation (a) literally at
the level the mandate names, and a receiver eagerly allocates 256 KiB per
open receive half. `INITIAL_MAX_STREAMS_UNI` is 128, so 128 concurrent
peer-opened uni streams allocate **32 MiB** — while `INITIAL_MAX_DATA`
guarantees at most **1 MiB** can ever be occupied. That is a 32×
amplification of exactly the kind §10.6 cites §11.3 against, produced by
following §10.6's own admissible option.

§10.6's per-stream bound is real and necessary; what is missing is that
the **connection-level** credit is the bound that protects memory, and
that the two must be reconciled by never allocating ahead of arrival.

**Provisional:** slice 4 implements option **(b)** — coalesce-on-insert
with `REASSEMBLY_CHUNKS_MAX` = 1024 — and allocates **lazily**, so total
buffered stream bytes across the connection are bounded by the advertised
connection credit. Both bounds then hold simultaneously. Pin it with a
test that opens many streams and asserts total buffered bytes, not
per-stream capacity (§11.7).

### H9 — §10.6's "Consumption, defined" list, and the two things that bound an unclaimed stream **[resolved in-spec, worth writing down]**

*"Consumption is **the application taking bytes out of the connection
core** — a `read()` draining the contiguous prefix, a message or datagram
claimed by its verb (§16.4), or a retirement true-up (§10.3)."* Read as
exhaustive. A peer-opened stream **never claimed** via `accept(dir)` is
therefore never consumed — intentional; §16.4 calls it *"backpressure by
retention"*. Two independent bounds keep that safe, and both should be
written into 4a's brief because §10.6's memory argument depends on them:

- **byte bound**: the connection window, 1 MiB, because §10.1 makes the
  connection level a sum over *all* streams — including unclaimed ones.
- **count bound**: the cumulative MAX_STREAMS limit, because credit is
  granted only on **full closure** and an unclaimed stream never closes.

So 128 unclaimed uni streams cannot buffer 32 MiB: the connection window
binds first (*"whichever is tighter binds"*, §10.1). This is the argument
H8's provisional depends on.

### H10 — an empty, FIN-less STREAM frame: no-op, or an open? **[candidate ruling — medium cost; also conflict C4]**

§9.2: *"The first STREAM or RESET_STREAM frame is the open."*
§9.5: *"an empty frame without FIN and without data is valid and a
**no-op** (tolerated, never emitted)."*

For a frame naming a not-yet-open index above the watermark these
disagree: §9.2 opens the stream (and every lower one, and charges the
cumulative limit); §9.5 calls the frame a no-op. slither never emits such
a frame, so this is reachable only from a foreign or hostile peer — where
the difference is whether a peer can consume its whole MAX_STREAMS
allowance with 128 one-byte frames carrying no data.

**Provisional: it opens.** §9.2's rule is about the *frame*, §9.5's
"no-op" is about the *data* (it delivers no bytes, pins no final size,
consumes no credit). Reading §9.5 as suppressing the open would make the
open set depend on a payload property §9.2 never mentions, and would make
a legitimate zero-length write on an already-open stream and a
stream-creating frame indistinguishable in the codec. The consumed
allowance is bounded and self-inflicted on the peer's side.

### H11 — `StreamOpened { dir }` carries no id, and implicit opening opens many at once **[candidate ruling — low cost]**

§16.4: `StreamOpened { dir: Dir }` — *"signal: claim via `accept(dir)`"*.
§9.2: a frame naming index `N` *"opens `N` and every lower-numbered
not-yet-open stream of that space"*. A peer whose first frame names index
5 opens six streams in one packet. How many `StreamOpened` events?

The spec says nothing. Because the event carries no id it is genuinely
ambiguous whether it means "one stream is claimable" or "claiming is
possible".

**Provisional: one event per newly-opened stream.** §16.4's contract is
that the receive-side events are signals the shell translates into
wakeups, and `accept(dir)` returns **one** stream per call. Emitting one
event for six streams means the shell must loop `accept()` until `None`
on every wake or lose five streams — a lost-wakeup shape, and one that
would only fail under exactly the reordering the tests inject. One event
per stream makes the count checkable and is what §9.2's own rationale
assumes when it worries about *"a phantom `StreamOpened` for a finished
stream"* (singular, per stream).

### H12 — `STREAM_LIMIT_ERROR`'s boundary is stated; test it two-sided **[resolved in-spec]**

§10.4: *"opening stream index `i` requires cumulative limit > `i`"*. With
`INITIAL_MAX_STREAMS_BIDI` = 32 the legal indices are 0…31. Slice 1's
lesson was a one-sided boundary (`LEN` and `LEN−1` tested, `LEN+1` not).
Test index 31 accepted **and** index 32 ⇒ `STREAM_LIMIT_ERROR`, and do the
same at 128/127 for uni.

### H13 — three symbols used by the spec and defined nowhere in it **[candidate ruling — ruling 89's exact shape, ×3]**

`grep -n` over all of `SPEC.md`:

- **`Dir`** — 4 occurrences (4491, 4500, 4514, 4515). Never defined. No
  variants named anywhere; `Dir::` appears zero times.
- **`StreamId`** — 11 occurrences, all in §16.2/§16.4 signatures and one
  in §16.8. §9.1 describes *"a stream ID is a varint"* but never mints
  the type, never says what accessors it has, and never states whether it
  is public. **§16.2's `SendStream::id(&self) -> Option<StreamId>` makes
  it public whether or not the spec says so.**
- **`StreamsExhausted`** — 1 occurrence (4491). Never defined, and **not
  in §18.1's error taxonomy**, which ruling 61 declares closed.

Ruling 89 is the precedent: `SessionId` *"appeared exactly twice in this
specification and was never defined"*, and the ruling that fixed it also
carried a rationale that named a mechanism that was not there
(`Handshake::Seal` had no bounds) — working rule 11. So this class costs
real work when it is discovered late, and it is being reported early on
purpose.

**Provisionals:**
- `Dir` is `pub enum Dir { Bi, Uni }`, public (it appears in the public
  `open_bi`/`open_uni` split only implicitly, but `StreamOpened` and
  `accept(dir)` are core-level; §16.2's shell surface does **not** take a
  `Dir` argument anywhere — the four verbs are named for it). Keeping
  `Dir` `pub(crate)` is therefore also viable and is the smaller public
  surface. **Recommend `pub(crate)`** unless 4b finds a public signature
  that needs it.
- `StreamId` is **public**, forced by `SendStream::id()`. Minimum surface:
  `Copy + Eq + Ord + Hash + Debug + Display`, plus `index() -> u64`,
  `dir() -> Dir`, `initiated_by_connection_initiator() -> bool`, and a
  `u64` round-trip. It must **not** expose a constructor that lets an
  application mint an id for a stream it does not own.
- `StreamsExhausted` stays **`pub(crate)`**. §18.1 is closed (ruling 61)
  and §16.2's shell verbs never surface it: `open_bi`/`open_uni` *"wait
  for MAX_STREAMS allowance when the cumulative limit is exhausted"*, so
  the shell converts the core error into a park, and no public error
  variant is added. Making it public would breach a closed taxonomy for
  a condition the public API is specified never to return.

### H14 — §8.5's packing order vs the one-extends-to-end rule: unreachable, and the implementer must know *why* **[resolved by argument — record the argument]**

§8.5 says *"then PING last if a probe still owes ack-eliciting content"*
and, one sentence earlier, *"At most one extends-to-end frame (¬LEN
STREAM, or `0x30` DATAGRAM) per packet, **in final position**."* Both
cannot hold in a packet that has a ¬LEN STREAM frame and owes a PING.

It is **unreachable**: a STREAM frame is ack-eliciting (§8.3), so a packet
containing one never "still owes ack-eliciting content" and never appends
a PING. Same for DATAGRAM. Recorded rather than dropped because working
rule 12's lesson is that a clean argument about the wrong state proves
nothing — this argument assumes *"owes ack-eliciting content"* means
"contains no ack-eliciting frame yet", which is what §8.7 defines
ack-eliciting to mean. If a later slice redefines a probe as needing a
PING *specifically*, the contradiction becomes reachable and this note is
where to look.

### H15 — §10.3's re-grant formula needs `last_advertised` seeded to the constant, not zero **[resolved in-spec, high-value test target]**

*"the **prospective limit** is `bytes_read + WINDOW`, and the receiver
emits MAX_STREAM_DATA or MAX_DATA when `prospective_limit −
last_advertised ≥ WINDOW/2`"*. At connection start `bytes_read = 0` and
the peer's limit is the protocol constant, which was **never sent on the
wire** (§10.2: no negotiation). So `last_advertised` must be initialised
to `INITIAL_MAX_STREAM_DATA` / `INITIAL_MAX_DATA`, giving
`prospective − last_advertised = 0` and no spurious grant. Initialise it
to 0 and every stream emits a MAX_STREAM_DATA the instant it opens. A
one-line bug with a wire-visible effect; §11.5 pins it.

### H16 — §10.2's table has five rows and four of them are the same kind of thing **[candidate ruling — low cost; the "grouping does not justify every row" shape]**

§10.2 is titled *"Initial values — protocol constants, no negotiation"*
and its lead sentence is *"slither has no negotiation surface at all, so
**initial windows** are protocol constants, identical in both directions
and all stream spaces; **later credit is receiver policy**."* Then:

| Constant | Value |
|---|---|
| `INITIAL_MAX_DATA` | 1 048 576 B |
| `INITIAL_MAX_STREAM_DATA` | 262 144 B |
| `INITIAL_MAX_STREAMS_BIDI` | 32 |
| `INITIAL_MAX_STREAMS_UNI` | 128 |
| **`STREAMS_CREDIT_BATCH`** | **8** |

The first four are initial windows: **wire-relevant**, because both ends
must assume the same value with nothing negotiated, so changing one is a
wire change. `STREAMS_CREDIT_BATCH` is not an initial window and not
wire-relevant at all — it is when a receiver *chooses* to advertise, which
by §10.2's own next clause is **"receiver policy"**. Two peers running
different batch values interoperate perfectly; two peers running different
`INITIAL_MAX_DATA` values corrupt each other's accounting immediately.

Why it matters concretely, and why it is not pedantry: the four windows
are pinned by `tests/spec_constants.rs` as **wire pins**, and `CLAUDE.md`
says a red wire-pin test *"needs a ruling, not an updated expectation."*
Putting a local policy knob in the same table gives it the same
protection, so a future tuning change to the batch size looks like a wire
change and gets a ruling round it does not need. It also implies the
reverse — that a consumer could reasonably expect the batch to be
observable — which it is not.

**Provisional:** keep the value at 8 and keep the constant where it is
(moving it is not worth a wire-pin churn), but **mark it in the code and
in `spec_constants.rs` as receiver policy, not a wire constant**, so the
next person to touch it knows which kind of change they are making. §10.2
already says which kind it is; only the table's grouping disagrees.

### H17 — §10.5 is titled "Violations" and lists two of the three §10 violations **[candidate ruling — ruling 64's exact shape]**

§10.5, in full:

> A peer exceeding advertised credit — stream or connection level — is a
> protocol violation: CLOSE with `FLOW_CONTROL_ERROR`. A peer opening
> beyond a stream limit: CLOSE with `STREAM_LIMIT_ERROR`. There is no
> tolerance band; the limits are exact (§8.2's semantic class).

§10.6, the very next section, defines a **third** §10 violation:

> a stream whose stored discontiguous ranges would exceed
> `REASSEMBLY_CHUNKS_MAX` (= 1024) after coalescing is a protocol
> violation: CLOSE with `PROTOCOL_VIOLATION` (§15.3).

A section named "Violations", inside the chapter that defines all three,
enumerating two. **Ruling 64 said "two things this rule does not reach"
when there were three**; this is the same defect at the same arity.
Working rule 8's warning that *a list in the spec is read as exhaustive
whether or not it says so* applies directly: an implementer building §10's
violation handling from §10.5 builds two of three, and the missing one is
the memory-safety bound.

There is a second, subtler asymmetry inside it. §10.5 insists *"There is
no tolerance band; the limits are exact"* — true of both violations it
lists, and **not** true of the third: `REASSEMBLY_CHUNKS_MAX` is
explicitly a *tolerance*, admissible implementation (a) makes the ceiling
*"unreachable"* entirely, and §10.6 ships it *"ratified-but-revisitable"*.
So the section's closing claim is exactly the claim the omitted violation
would contradict. That is what makes the omission hard to see — the
sentence reads as complete because it is internally consistent.

**Provisional:** slice 4 implements all three and its tests treat the
§10 violation set as three-membered. If §10.5 is amended, the sentence to
watch is the "no tolerance band" one — **working rule 4: grep for the
rationale, not only the token.** A fix that adds a third bullet and leaves
that sentence standing ships a self-contradicting section, which is the
failure working rule 4 was written from.

*(§10.7's "Exemptions" list was checked the same way and **is**
exhaustive: the only two frames that carry application bytes are STREAM
and DATAGRAM, DATAGRAM is exempted and retransmitted stream bytes are
exempted, and every other frame type carries no stream data to account
for. Recorded so the next reader does not re-derive it.)*

---

## §9. Conflicts found — reported, not resolved (working rule 3)

**Working rule 3: when two statements conflict, do not default to the
code-like rule — five times in this project the prose held the correct
intent and the formal rule held the bug.** Each conflict below is stated
with *both* sides and a provisional that can be reversed by editing one
named test. None is resolved here.

---

### C1 — `open()` returns a `StreamId` that §16.9 says does not exist yet

**Side A (the formal rule), §16.4 line 4491:**
> `fn open(&mut self, dir: Dir) -> Result<StreamId, StreamsExhausted>;`

and `write`, `finish`, `reset`, `read` are all keyed by `StreamId`.

**Side B (the prose), §16.9:**
> Wire stream IDs encode opener parity, which is fixed only at
> establishment […] so **stream IDs are assigned at establishment**:
> pre-establishment handles hold core-internal indices […] and `id()`
> returns `None` until the connection is established (§16.2).

§16.9 is a `[RATIFIED]` behavioural rule about early sends; §16.4's
signature list is introduced with *"the types are normative in shape; an
implementation may rename"* (§16.2, carried). **Both cannot be
implemented literally**: a `connect()`-created connection can be written
to before install (that is §16.9's whole point), so `open()` must return
something, and it cannot be a wire `StreamId`.

**Why this is not the obvious "just make it `Option`":** the danger is a
core that returns an internal index *typed as* `StreamId` and remaps it
at install, leaving every handle holding a stale key. That build passes a
pre-establishment test and a post-establishment test and fails only the
one that opens before install and writes after — the mutually-consistent
failure working rule 6 exists to catch.

**Provisional:** the core's stream verbs are keyed by an opaque
`pub(crate) struct StreamRef` that is **stable across install**, and
`core::Connection::stream_id(&self, r: StreamRef) -> Option<StreamId>`
is the §16.9 accessor the shell's `id()` reads. §16.4's signature list
then needs the same amendment ruling 71 made for the stage-0 accessors —
a list that omitted a member, invisible until someone built against it.
Reversing this is an edit to
`an_early_opened_stream_keeps_its_handle_across_install`.

**Candidate ruling.** This changes five §16.4 signatures.

---

### C2 — §10.3 lists "handle abandoned" as a retirement, §16.2 says abandonment does not retire, and §16.2's own promise is false under §16.2's own mechanism

This is the highest-cost conflict in the slice.

**Side A, §10.3 (the list):**
> When a receive half is retired for any reason — read to its final size,
> reset observed, **handle abandoned (§16.2)**, surfaced as a message
> (§9.8), or final size reached with no reader (§9.7) — **all of its
> bytes up to its final size count as consumed for connection-level
> credit-advance**.

Read plainly: abandonment *is* a retirement, and the true-up runs then.

**Side B, §16.2 (the mechanism):**
> Dropping a `RecvStream` abandons the receive half: arrivals for it are
> discarded and stream-level credit is never again advanced (a sender
> that keeps pushing stalls at the stream window), **the half closes when
> the pinned final size or a reset arrives** (§9.7), and **on that
> retirement** its bytes up to the final size count as consumed at the
> connection level (§10.3) — **an abandoned stream never wedges the
> connection window.**

Read plainly: abandonment *arms* a later retirement; the true-up runs
only if the peer eventually FINs or resets.

**The two readings differ, and Side B contradicts its own final clause.**
Under Side B, consider the reachable case: the application drops a
`RecvStream` mid-transfer while the sender still has more than
`INITIAL_MAX_STREAM_DATA` to send. We stop advancing stream credit, so
the sender stalls at the stream window — §16.2 says so explicitly. A
stalled sender does not send FIN (its FIN comes after its data) and has
no reason to reset. So no final size is ever pinned, no retirement ever
happens, no true-up ever runs, and that stream's bytes hold connection
credit **for the connection's life**. Four such streams at 256 KiB wedge
the entire 1 MiB connection window. **"An abandoned stream never wedges
the connection window" is false under the mechanism the same sentence
describes.**

There is no escape hatch: `STOP_SENDING` — the frame that would tell the
sender to stop — is deferred to §19 (§9.9, §10.4), so slither cannot ask.

Note also the **second** wedge, on a different resource: full closure is
what earns the peer a MAX_STREAMS grant (§10.4). Under Side B an
abandoned peer-opened stream never fully closes, so an application that
drops `RecvStream`s permanently starves the peer's cumulative stream
allowance — 32 dropped bidi handles and the peer can never open another
stream.

And §10.3's own rationale is on Side A's side:
> Without this rule, MAX_DATA is an absolute limit advanced only by
> reads, and cumulative discarded or reset bytes march the connection
> into a permanent send stall after `INITIAL_MAX_DATA` with no error and
> no timer — **reachable in honest operation by any application that
> cancels streams.**

That is a description of the exact failure Side B reintroduces.

**Provisional (Side A): abandonment retires the half immediately.** On
drop, the receive half is freed, its index is tombstoned at the
watermark, and the connection-level true-up advances that stream's
contribution to its **highest received offset** (its final size is
unknown and unknowable; the high-water mark is the only defensible
value, and §10.3's true-up is *"absolute, not additive"* and *"monotone
bring-to-final"*, which a high-water mark satisfies). Subsequent STREAM
frames for that index are inert by §9.2's watermark rule — ACKed, never
re-opened, no credit consumed. The sender still stalls at the stream
window, which §16.2 accepts and which no slither frame can cure, but the
**connection** window and the **stream allowance** are both released.

Reversing this is an edit to
`a_dropped_recv_stream_releases_connection_credit_at_once`.

**Working rule 3's shape for the sixth time**, and note which side is
which: §10.3's list is the *formal* rule and §16.2's sentence is the
*prose*, and here the **list** is right and the **prose mechanism** is
wrong — while the prose *promise* in the same sentence agrees with the
list. I am reporting it rather than picking, but I want the asymmetry on
record, because it is the first instance in this project where the prose
contradicts itself within one sentence.

**Candidate ruling. High cost:** the wrong answer is a silent,
unrecoverable connection stall in an application that cancels streams —
which is every application that uses a timeout.

---

### C3 — the reassembly bound's level (H8)

§10.6's mandate is per-stream; its cost arithmetic is per-connection.
Restated in full at **H8**. Reversing the provisional is an edit to
`buffered_bytes_stay_within_the_connection_window_across_many_streams`.

**Candidate ruling. High cost** if answered by allocating eagerly at the
per-stream window: a peer opens 128 uni streams and costs the receiver
32 MiB with 1 MiB of credit. That is a remote memory-amplification
vector, and §10.6 is the section that exists to close exactly that class.

---

### C4 — §9.2 opens on any STREAM frame; §9.5 calls the empty FIN-less frame a no-op

Restated at **H10**. Reversing is an edit to
`an_empty_finless_stream_frame_opens_its_stream`.

---

### C5 — §9.2's watermark vs §8.4's `STREAM_STATE_ERROR` ordering

Restated at **H4**. Reversing is an edit to
`a_stream_frame_on_a_closed_local_uni_space_is_a_state_error`.

---

### C6 — the same peer frame is a kill or a no-op depending on **local read timing**

§9.6:
> A RESET_STREAM for an already-FIN-complete receive half is a valid
> no-op if the final sizes agree, `FINAL_SIZE_ERROR` otherwise.

§9.7 frees the receive half at `DataRead` — *"the application has read to
the final size"* — and §9.2 then makes any frame naming that index inert.
So a peer's RESET_STREAM with a **disagreeing** final size is:

- `FINAL_SIZE_ERROR`, killing the connection, if it arrives before the
  application's last `read()`;
- a silent no-op if it arrives after.

**This is almost certainly intended** — the watermark exists precisely so
late frames are inert, and §9.2 says so at length. But it is nowhere
stated, and it matters twice over: a test author who writes
`a_disagreeing_reset_after_fin_is_a_final_size_error` without pinning the
read timing has written a **flaky** test, and a security reviewer reading
§9.6 alone will believe the check is unconditional.

**No provisional needed** — both behaviours follow from rules slice 4 must
implement anyway. What is needed is that both tests exist and both name
the timing in their names:
`a_disagreeing_reset_before_the_final_read_is_a_final_size_error` and
`a_disagreeing_reset_after_the_half_is_freed_is_inert`.

---

### C7 — `BiStream`'s home slice: `PLAN.md` says 8, §16.2 says 4

**`PLAN.md` §4, slice 8 row:**
> all of §3 above: `compat/{io,stream,codec,tower}.rs`, **`BiStream`**,
> the `io::Error` conversions, the no-prefetch pin

**§16.2 (and ruling 55):**
> `pub async fn open_bi(&self) -> Result<BiStream, ConnectionLost>;
> // .split() → the pair`

Slice 4 owns `open_bi`/`accept_bi`. It cannot own them and not own their
return type. Either slice 4 returns a `(SendStream, RecvStream)` tuple —
contradicting §16.2 and ruling 55, and forcing a **breaking public API
change** in slice 8 — or `BiStream` lands in slice 4.

**Provisional:** `BiStream` the **type**, with `split()` and `join()`,
lands in **slice 4**; its `AsyncRead`/`AsyncWrite` **impls** and the
`compat/` modules stay in slice 8. `PLAN.md`'s slice-8 row means the
*composability* of `BiStream`, not its existence. This is the same shape
as round 12's §7.4 finding — a plan/spec boundary error, settled by
moving the boundary, not by changing the spec — and the cost is ~40 lines.

**Plan decision, not a wire ruling**, but it changes a `PLAN.md` row and
so is the maintainer's.

---

### C8 — a slice-boundary conflict, not a spec conflict: Appendix B's tombstone obligation cannot be fully discharged in slice 4

Appendix B:
> **The closed-stream tombstone** (§9.2): free a stream (read-to-final
> and sugar-surfaced), **drop the ACK, and let the peer's PTO
> retransmission re-name it** […]

"Drop the ACK" needs §12 (slice 5); "PTO retransmission" needs §13
(slice 7). Slice 4 can exercise the identical receive-path code by
injecting the duplicate STREAM frame directly through `FlakyPolicy`'s
duplication, which is strictly a *stronger* stimulus (it arrives with no
delay). **Recommendation:** slice 4 writes the injected-duplicate test and
records the loss-driven variant as **owed to slice 7**, in slice 4's exit
note. Flagged here because an obligation quietly marked done by a weaker
test is how a slice ships a gap.

## §10. Open questions, ranked by cost of a wrong answer

Ranked by *what a wrong answer costs*, not by how interesting it is. Each
provisional is chosen to be the most defensible reading **and**
test-pinnable, so reversing it is an edit to one named test — except Q4,
where no test can separate the readings and that is stated.

| # | Question | Cost of a wrong answer | Provisional | Reversing edits |
|---|---|---|---|---|
| **Q1** | **C2** — does dropping a `RecvStream` retire the half at once, or only when a FIN/reset arrives? | **Highest.** Wrong ⇒ a silent, permanent connection-window stall and a permanently starved MAX_STREAMS allowance, in any application that cancels a stream. No error, no timer, no recovery. §10.3's own rationale calls this failure *"reachable in honest operation."* | Retire at once; true up to the highest received offset; tombstone the index | `a_dropped_recv_stream_releases_connection_credit_at_once` |
| **Q2** | **C3/H8** — is the reassembly buffer allocated at the per-stream window or lazily under the connection window? | **High.** Wrong ⇒ 32× remote memory amplification (128 uni × 256 KiB against 1 MiB of credit), from following §10.6's own admissible option (a) | Option (b), coalesce-on-insert, lazy allocation; assert the **connection-wide** buffered total | `buffered_bytes_stay_within_the_connection_window_across_many_streams` |
| **Q3** | **C1** — what does `core::Connection::open()` return before establishment? | **High.** Wrong ⇒ handles hold stale keys across install; the failing case is "open early, write late", which passes both of the obvious tests | Opaque `StreamRef`, stable across install, plus a `stream_id(r) -> Option<StreamId>` accessor | `an_early_opened_stream_keeps_its_handle_across_install` |
| **Q4** | **H4/H5** — the evaluation order of §8.4's semantic checks | **Medium.** Wrong ⇒ the wrong error code on the wire, and in one case (H4) a violation check deleted entirely for closed local uni spaces | State/legality → watermark → limit → final size → flow control | `a_stream_frame_on_a_closed_local_uni_space_is_a_state_error` and one per adjacent pair |
| **Q5** | **H7** — are MAX_STREAMS_BIDI/UNI sealed `quiet`? | **Medium.** Wrong ⇒ a connection whose only output is stream credit defers its keepalive and dies (or the reverse: never defers and beacons needlessly). Wire-invisible, timer-visible, and slice 7 will be blamed for it | All four credit frames are non-marking | `a_max_streams_only_packet_does_not_defer_the_keepalive` |
| **Q6** | **C7** — does `BiStream` land in slice 4 or slice 8? | **Medium**, and it is a *release* cost rather than a correctness one: the wrong answer is a breaking public-API change between two shipped slices | Type + `split`/`join` in slice 4; `AsyncRead`/`AsyncWrite` and `compat/` in slice 8 | n/a — a `PLAN.md` row |
| **Q7** | **H11** — one `StreamOpened` per opened stream, or one per packet? | **Medium.** Wrong ⇒ a lost-wakeup that only manifests when a peer's first frame names a high index, i.e. under exactly the reordering the tests inject | One per newly-opened stream | `an_implicit_open_of_six_streams_emits_six_stream_opened_events` |
| **Q8** | **H10/C4** — does an empty FIN-less STREAM frame open its stream? | **Low–medium.** Reachable only from a foreign or hostile peer; the difference is whether such frames consume the peer's own MAX_STREAMS allowance | It opens | `an_empty_finless_stream_frame_opens_its_stream` |
| **Q9** | **H13** — public surface of `StreamId`, and are `Dir` / `StreamsExhausted` public? | **Low–medium**, but **irreversible after release**: a public type is a semver commitment | `StreamId` public and minimal; `Dir` `pub(crate)` unless 4b needs it public; `StreamsExhausted` `pub(crate)` (§18.1 is closed, ruling 61) | `spec_errors.rs`'s taxonomy test |
| **Q10** | **H6** — is the "≤ 8" low-allowance threshold `STREAMS_CREDIT_BATCH`? | **Low today, latent.** Both readings behave identically at the current value; the cost appears only when the constant is revisited — which §10.2 says it will be | One constant, named in both places | **none — not separable by test.** This is why it needs the ruling |
| **Q11** | **C8** — may slice 4 mark Appendix B's tombstone obligation discharged by an injected duplicate rather than a PTO retransmission? | **Low if written down, medium if not.** An obligation silently downgraded is how a gap ships | Discharge the receive-path half now; record the loss-driven variant as owed to slice 7 | n/a — an exit-note decision |

**Q1 and Q2 should be answered before 4a starts.** Both change the shape
of `recv.rs` and `flow.rs`, not just a branch inside them. Q3 should be
answered before 4a freezes its surface, because 4b compiles against it.
Q4–Q11 can be taken as provisionals and revisited at integration.

## §11. Test-design notes (working rule 9)

**A bound is only a test if the degenerate case violates it.** This
matters more in slice 4 than in any slice so far, for a reason with
evidence behind it: the round-15 seam review **mutated §16.4's central
MUST and 454 of 454 tests stayed green**. Flow control is made almost
entirely of bounds that a collapsed implementation satisfies for free.

The canonical trap, stated so nobody writes it: **"credit never goes
negative" is true of an implementation that never grants any credit.**
So is "buffered bytes never exceed the connection window", of an
implementation that buffers nothing. So is "no stream exceeds
`REASSEMBLY_CHUNKS_MAX` ranges", of an implementation that coalesces
everything into one range by dropping the gaps.

For every property below: **what the broken build does**, and **which
assertion separates it**. Both agents' briefs should carry this section.

---

### §11.1 S12 — bytes arrive intact, ordered, exactly once

*Broken build A:* delivers the contiguous prefix but silently drops the
overlapping region of a duplicated frame's tail — the bytes are "already
received" by offset but the range bookkeeping is off by the overlap.
*Broken build B:* re-delivers duplicated bytes, so the reader sees more
bytes than were written.

*Separating assertions:*
- the received byte **vector equals** the sent one — not "length matches",
  not "no gaps". Use a payload whose content is a function of its offset
  (e.g. `b[i] = (i % 251) as u8`) so an off-by-N shift is visible; a
  payload of repeated `0xAA` passes under a shift.
- **total bytes read == total bytes written**, asserted separately from
  content, so duplication and truncation fail differently.
- `read()` returns `Ok(None)` **exactly once** and only after the last
  byte, and a subsequent `read()` still returns `Ok(None)` rather than
  hanging or erroring.
- the payload must exceed one packet's `MAX_PLAINTEXT` (1170 B) by a
  wide margin — **at least 64 KiB** — or fragmentation, the offset field
  and the round-robin fill are all untested. A single-packet S12 test is
  a test of nothing in §9.5.

### §11.2 S13 — independence, and the parity claim

*Broken build:* one shared reassembly buffer keyed by offset rather than
by stream, or a fill loop that drains stream A completely before touching
stream B. Both make S13's "independent" false while every byte still
arrives.

*Separating assertions:*
- with stream A's frames **withheld** by the wire and stream B's
  delivered, `B.read()` returns data **while `A.read()` is pending**.
  Asserting only "both eventually complete" passes the head-of-line
  build.
- the round-robin fill: write to two streams concurrently, capture the
  wire with `Tap`, and assert that **some packet carries frames for both
  streams** *and* that neither stream's frames form an unbroken prefix of
  the capture. Working rule 9's own example is the warning here: "these
  two packets differ" passed a core reusing one ephemeral. **"Both
  streams made progress" passes a strictly-sequential fill** if the
  measurement is taken at the end. Measure interleaving, not completion.
- parity (`STORIES.md` S13: *"Stream IDs carry the initiator/responder
  parity fixed at S4"*): assert the **dialler's** stream ids have
  `id & 0x01 == 0` and the **acceptor's** have `id & 0x01 == 1`, on the
  same connection, in the same test. Asserting only "the two sides' ids
  differ" passes a build that allocates from one shared counter.

### §11.3 S14 — reset

*Broken build:* the reset tears down the connection, or the peer surfaces
`ConnectionLost` rather than `ReadError::Reset(code)`, or the sibling
stream's credit is released along with the reset one's.

*Separating assertions:*
- `ReadError::Reset(code)` with the **exact code** the sender passed, not
  any reset. Use a code that is not 0 and not any registry value —
  §15.3 reserves `≥ 0x10` for applications, so use e.g. `0x2a`. A test
  using code 0 cannot distinguish "the peer's code" from "the drop
  default" (§16.2: dropping a `SendStream` resets with code 0).
- a **sibling** stream on the same connection completes a full
  write/read round trip **after** the reset, and the connection's
  `closed()` future is still pending.
- the connection-level credit **released** by the reset equals
  `final_size`, asserted by observing that a subsequent write of exactly
  that many bytes on another stream succeeds where it would otherwise
  block. "Credit was released" is unobservable; "the next write fits"
  is.

### §11.4 S17 — backpressure

This is where the degenerate builds are most seductive.

*Broken build A (grants nothing):* `write` blocks at the initial window
and never resumes. Passes "credit never goes negative", passes "the
sender does not buffer without limit", passes "the reader's stream
stalls".
*Broken build B (grants unconditionally):* every read emits a credit
frame. Passes "the sender resumes", passes "the connection is not
stalled".
*Broken build C (one shared ledger):* stream-level and connection-level
credit collapsed into one number. Passes both of the above.

*Separating assertions:*
- **Stall side:** with the reader **not** reading, the sender's `write`
  is `Pending` after exactly `INITIAL_MAX_STREAM_DATA` bytes have been
  accepted — assert the **byte count at which it first blocks**, not
  merely that it blocks. Build A blocks at the same place; build B never
  blocks. So this assertion alone separates B only.
- **Resume side:** after the reader drains `N` bytes, the sender accepts
  **more** — and specifically, accepts more **only once
  `N ≥ WINDOW/2`**. Assert *both* directions: draining
  `WINDOW/2 − 1` bytes emits **no** MAX_STREAM_DATA, and draining
  `WINDOW/2` emits exactly one. This is the assertion that kills build B,
  and it is the §10.3 formula's only real pin.
- **Two-level separation (kills build C):** open **two** streams. Stall
  reader 1 at its stream window; assert stream 2 still accepts data.
  Then stall enough streams that the **connection** window binds; assert
  that a *fresh* stream, with a full stream window of its own, now
  blocks. A single-stream S17 test cannot tell the two levels apart, and
  the story explicitly asks for both (*"per stream and per connection"*).
- **"The sender learns rather than buffering without limit"**: assert the
  sender's `write` returns `Poll::Pending` — not that it returns `Ok(n)`
  with a smaller `n` — and, at the core level, that the send half's
  buffered-but-unsent byte count has a ceiling. Ruling 56 is the pin:
  *"`poll_write` accepts only what flow-control credit admits, so
  accepted bytes are already in send state and there is no shell
  buffer."* A build that accepts everything into a shell-side `Vec` and
  drips it out passes every observable behaviour of S17 and violates
  §10.6. **Assert the acceptance boundary, not the delivery.**

### §11.5 The §10.3 re-grant trigger (H15)

*Broken build:* `last_advertised` initialised to 0, so every stream emits
a MAX_STREAM_DATA on open.

*Separating assertion:* with a `Tap` on the wire, open a stream, write one
byte, read it, and assert **zero** MAX_STREAM_DATA frames were sent. A
test that only checks "a grant eventually arrives" passes the broken
build with flying colours.

### §11.6 Non-marking credit (H7/Q5)

*Broken build:* MAX_STREAMS sealed with the marking `seal`.

*Separating assertion:* drive a connection to a state where its **only**
output is a MAX_STREAMS frame, then assert `last_send` is unchanged
across it while the death-clock deadline **is** armed. Both halves —
ruling 33's *"both halves of it"* — because a build that makes credit
neither marking nor ack-eliciting also passes the first half.

### §11.7 The reassembly bound (H8/Q2)

*Broken build A (the collapsed one):* stores every received range without
coalescing, so the ceiling is never reached because the ranges are
counted after a merge that never happens — or, worse, coalesces by
**overwriting gaps**, keeping one range and losing data.
*Broken build B:* eager per-stream allocation at 256 KiB.

*Separating assertions:*
- feed one-byte frames at offsets 0, 2, 4, … and assert the connection
  dies with `PROTOCOL_VIOLATION` at **exactly** the 1025th stored range
  — two-sided: 1024 stored ranges is **alive**, 1025 is dead. A one-sided
  "it eventually dies" passes a build with any lower ceiling, including
  one that dies at 2.
- feed frames that *should* coalesce (offsets 0, 1, 2, …, arriving in
  reverse) and assert the connection **survives** 4096 of them — this is
  the assertion that separates "coalesces" from "has a low ceiling".
- for B: open 64 uni streams, deliver one byte to each, and assert
  **process-visible buffered bytes** stay bounded by the connection
  window — measured through a `pub(crate)` accessor on the core, not
  inferred. If no such accessor exists the property is untestable and one
  should be added; an untestable MUST is what §10.6 became last time.

### §11.8 The `u64` arithmetic mandate

§8.4 mandates checked or saturating arithmetic *"(an unchecked sum wraps
for large `final_size` values and silently re-opens the window)"*.

*Separating assertion:* a RESET_STREAM with `final_size = u64::MAX` (or
`2^62 − 1`, the varint ceiling) must produce `FLOW_CONTROL_ERROR`, **not**
a wrapped ledger that then accepts more data. Assert the error *and* that
a subsequent in-credit write on another stream is still correctly bounded
— because a build that wraps and then errors for an unrelated reason
passes the first assertion alone. Run this test in **release** as well as
debug: `debug_assert`-based overflow checks compile out, and this is
precisely the case FIXES-3b §4 shows costs its bug only in release.

### §11.9 Two-sided boundaries throughout (slice 1's lesson)

Every limit in slice 4 gets both sides:

| Bound | Alive | Dead |
|---|---|---|
| cumulative bidi limit (32) | index 31 | index 32 ⇒ `STREAM_LIMIT_ERROR` |
| cumulative uni limit (128) | index 127 | index 128 ⇒ `STREAM_LIMIT_ERROR` |
| stream credit (262 144) | offset+len == limit | one byte past ⇒ `FLOW_CONTROL_ERROR` |
| connection credit (1 048 576) | sum == limit | one byte past ⇒ `FLOW_CONTROL_ERROR` |
| `REASSEMBLY_CHUNKS_MAX` (1024) | 1024 ranges | 1025 ⇒ `PROTOCOL_VIOLATION` |
| MAX_STREAMS `max` (§8.4) | `2^60` | `2^60 + 1` ⇒ structural |
| final size | FIN at the exact high-water offset | FIN one byte below ⇒ `FINAL_SIZE_ERROR` |

### §11.10 4b-specific: the fault class `FlakyWire` cannot express (working rule 13)

Two of the four round-15 findings were unreachable from all 451 tests
**by construction**, because `FlakyWire` models a network and not a
socket. 4b's content is full of the same class, and no amount of
flow-testing over `FlakyWire` will reach it:

- **Drop while blocked.** Drop a `SendStream` whose `write` future is
  parked on flow-control credit. §16.2 says the drop *"resets it with
  error code 0"* — so a drop must emit a wire frame from a `Drop` impl
  that cannot await the driver. Assert the peer sees
  `ReadError::Reset(0)`.
- **Drop the `RecvStream` half of a `BiStream` while the `SendStream`
  half lives.** Two independent lifetimes over one core stream.
- **Cancel-safety.** Drop a `read()` future mid-poll and re-issue it;
  assert **no byte is lost** and none is delivered twice. Repeat for
  `write()`, `open_bi()` and `accept_uni()`. FIXES-3b-round2 F10's
  precedent applies: park under an `InlineWaker` that re-polls from
  inside `wake()` and assert the **positive** outcome, since the failure
  mode is a swallowed panic inside the driver task, not an observable
  crash.
- **The per-`StreamId` waker maps must not grow without bound.**
  FIXES-3b finding 3 was `Driver::waiting` growing 132.3 bytes per
  cancelled `accept()` forever. A map keyed by `StreamId` on a
  connection that opens and closes streams for hours is the same shape
  with a larger multiplier. Assert map size after N open/close cycles.

## §12. What slice 4 must NOT build

Explicit exclusions, each with the **seam** — the shape slice 4 must leave
behind so the later slice adds rather than rewrites. §10.6's *"credit is
the buffer commitment"* touches every one of them, so each entry says
where.

### §12.1 §9.8 — messages (slice 6)

**Do not build:** `send_message`, `recv_message`, the sugar-stream
allocator, `MESSAGE_RECV_MAX` enforcement, the receiver-emitted overflow
RESET_STREAM, `MESSAGE_OVERFLOW` (`0x06`), or the "which mode consumes
this stream" arbitration.

**Read §9.8 only far enough to know the boundary** — it is ~140 lines and
working rule 1's context-exhaustion warning names it by name.

**Seams to leave:**
- A uni receive half must be **claimable by exactly one consumer**, and
  the claim must be a distinct step from the arrival. §16.4 already
  requires this (`accept(dir)` claims; the core retains until claimed).
  Build the claim queue as a queue of *unclaimed peer-opened halves*, not
  as "the newest one"; §9.8 will add a second claim verb drawing from
  **the same supply**.
- §10.3: *"Sugar-consumed streams never earn stream-level credit (§9.8);
  their reads still earn connection-level credit."* So the credit ledger
  must be able to answer *"does this half earn stream-level credit?"* per
  half. In slice 4 the answer is always yes. Make it a field, not a
  constant — the alternative is threading a mode flag through `flow.rs`
  in slice 6.
- §9.6's **receiver-emitted** reset is retained in a connection-level
  regenerate set that outlives the stream state (§8.7's carve-out). Slice
  4's sender-emitted reset lives in the *stream's* state. Do not merge
  them into one structure keyed by stream — slice 6 needs one that is not.

### §12.2 §9.9 — STOP_SENDING (out of scope entirely)

**Do not build** frame `0x05` in any form. §8.3: *"`0x05` is *reserved*,
not implemented: like any unknown type, receiving it is a structural
failure — CLOSE with `PROTOCOL_VIOLATION`."* That behaviour is **already
correct** in slice 3a's codec (`frame.rs:352`, the `other =>
Err(Structural::UnknownType(other))` arm) and must stay correct — a test
asserting it should exist and is cheap.

`WriteError::Stopped` is reserved and must not be added (§18.1, ruling
61: `WriteError` is `#[non_exhaustive]` **for this reason**).

**Seam:** there is none, and that is the point. Slice 4 must not invent a
private mechanism for "tell the sender to stop" — C2's provisional is the
correct answer to the pressure that would otherwise create one.

### §12.3 §11 — datagrams (slice 6)

**Do not build** `0x30`/`0x31`, the 64-entry receive queue, the drop
counter, or `send_datagram`.

**Seam:** `Packing`'s `Stage::Fill` is shared between STREAM and DATAGRAM
(§8.5: *"then STREAM and DATAGRAM fill"*). Slice 4 adds the fill stage;
slice 6 adds a second contributor to it. **The one-extends-to-end-frame
rule is a property of the stage, not of STREAM** — put the check in
`Packing`, not in the stream fill loop, or slice 6 will duplicate it.
§10.7: DATAGRAM is **flow-control-exempt**, so `flow.rs` must never be
consulted on the datagram path; keep the ledger's entry points keyed by
stream so that is structurally true.

### §12.4 §12 — ACK semantics (slice 5)

**Do not build** ACK generation, the delayed-ACK timer, ACK ranges from
the replay window, or ACK application. The ACK **codec** already exists
(slice 3a) and stays codec-only.

**This is the largest exclusion and it has consequences slice 4 must state
rather than discover:**

| §9/§10 rule | Needs ACK? | Slice 4 status |
|---|---|---|
| send half frees at `DataRecvd` (§9.7) | **yes** | unreachable |
| send half frees at `ResetRecvd` (§9.7) | **yes** | unreachable |
| `ConnEvent::StreamFinished` (§16.4) | **yes** | never fires |
| `SendStream::acked()` (ruling 47) | **yes** | **not built** — it is `PLAN.md` slice 5 already |
| RESET_STREAM regenerate-until-acked (§9.6) | **yes** | emit once; the regenerate set exists and is not driven |
| watermark advance for **peer-opened uni** | no | **reachable** — one half, freed by read-to-final |
| watermark advance for **peer-opened bidi** | yes (our send half) | unreachable |
| watermark advance for **locally-opened** | yes | unreachable |
| MAX_STREAMS replenishment, **uni** | no | **reachable** |
| MAX_STREAMS replenishment, **bidi** | yes | unreachable |
| every §10 credit rule | no | **fully reachable** |
| every §9.5 receive rule | no | **fully reachable** |

**Seam:** define `on_ack_range(stream, range)` and `on_lost_range(stream,
range)` on the send half **in slice 4**, unit-test them directly in-crate
(they are `pub(crate)`), and leave them uncalled from the datagram path.
Slice 5 wires §12's ACK application to them and nothing in `send.rs`
changes. **This is what makes the GC, watermark and `StreamFinished`
logic testable in slice 4 without ACKs on the wire** — and it must be
done this way, because the alternative is slice 4 shipping that logic
untested and slice 5 discovering it.

**One consequence to write into 4a's brief:** with no ACK processing, the
send half's un-ACKed retention set **never drains**, so a long-lived
connection retains every byte it ever sent. That is a real, temporary,
known condition closed by slice 5. It is not a licence to skip the
retention set — the set is what §8.7's `ranges` class acts on — and it is
not a licence to free on send, which would be a collapsed implementation
that makes slice 5's tests pass for free.

### §12.5 §13 — loss recovery (slice 7)

**Do not build** the sent-packet map, packet/time-threshold detection,
PTO, or the probe. **Do build** the two structures §8.7 names, because
they are stream state and not recovery state:

- the send half's **pending-range set** (`ranges` class), so a returned
  range can be re-framed;
- the connection's **regenerate set** of frame identities (RESET_STREAM,
  and the four credit frames), so a lost identity re-queues with the
  freshest value.

Slice 4 populates both and drives neither.

### §12.6 §14 — congestion control (slice 7)

**Do not build** cwnd, slow start, ABC, recovery periods, or persistent
congestion. **Seam:** slice 4's fill loop is bounded by flow control
only. Slice 7 inserts a second bound; write the fill loop so the "how
many bytes may I send right now" question has **one** call site.

### §12.7 §10.6's reach across all of these — stated once

*"the supporting check: **no non-stream, non-datagram frame can force
unbounded buffering** — ACK processing is bounded intersecting (§12.5),
credit frames apply as O(1) monotone-max, PING/PADDING are O(1),
RESET_STREAM *frees* state, and CLOSE enters the linger."*

Slice 4 owns two clauses of that check and must not break the others:

- **"credit frames apply as O(1) monotone-max"** — a MAX_STREAM_DATA for
  a stream we have not opened must be `STREAM_STATE_ERROR` (§8.4), and
  one at or below the watermark a **no-op**; under no circumstance may it
  allocate stream state. §8.4 is explicit: *"credit frames never open
  streams"*. A receiver that lazily created a stream entry on receiving
  credit for it would hand a peer unbounded allocation with four bytes
  per stream — and it is the natural implementation if the ledger is
  keyed by a map with an `entry().or_default()`.
- **"RESET_STREAM *frees* state"** — the receive half's reassembly buffer
  is dropped when the reset is *applied*, not when the application
  observes it. §9.6: *"The receive half surfaces `ReadError::Reset`,
  **discards its reassembly buffer**, and closes when the application
  observes the reset."* Three distinct moments; the discard is the first.
  Holding the buffer until observation would let a peer pin 1 MiB behind
  an application that never reads.

## §13. Appendix B obligations touched

Appendix B is non-normative but is the checklist a slice is measured
against. The §9/§10 block (SPEC.md ~5588–5632); **bold = in slice 4**:

> **Streams, flow control, messages, datagrams.**
> - **Stream reassembly under reordering, overlap, and duplication; FIN
>   final-size pinning; every `FINAL_SIZE_ERROR` case (§9.5, §9.6).**
> - **The closed-stream tombstone** (§9.2): free a stream (read-to-final
>   and sugar-surfaced), drop the ACK, and let the peer's PTO
>   retransmission re-name it — no re-open, no phantom `StreamOpened`, no
>   second surfacing of the same message; the watermark holds for the
>   connection's life (§9.2).
> - **Flow-control stall-and-resume at both levels; the §10.3 re-grant
>   formula** (MAX_STREAM_DATA/MAX_DATA emitted exactly when the read
>   offset advances ≥ WINDOW/2 past the last advertisement); **violation ⇒
>   CLOSE with `FLOW_CONTROL_ERROR`** (§10).
> - **Discard-credit** (§10.3): abandoned handles, observed resets, and
>   sugar-surfaced streams true up connection credit — a stream-cancelling
>   application never wedges MAX_DATA; the §8.4 bound check rejects a
>   `final_size` beyond the advertised limit *before* any true-up (no
>   credit inflation, no `u64` wrap).
> - **The reassembly-fragment bound** (§10.6): a one-byte-frames-at-
>   even-offsets flood stays O(credit) or dies at `REASSEMBLY_CHUNKS_MAX`
>   with `PROTOCOL_VIOLATION`; the defragmentation cost is measured by the
>   throughput gate below.
> - **MAX_STREAMS replenishment** (batching at 8, low-allowance emission,
>   peer-opened streams only) and **`STREAM_LIMIT_ERROR`** (§10.4).
> - Message sugar […] — *slice 6*
> - The unclaimed uni stream fails loudly (§9.8, ruling 51) — *slice 6*
> - DATAGRAM […] — *slice 5/6 per `PLAN.md`*

**Two obligations in that list are only partly reachable in slice 4** and
this must be stated in the slice's own exit note rather than discovered
later:

- *The closed-stream tombstone.* Its full shape ("drop the ACK, let the
  peer's PTO retransmission re-name it") needs §12 ACK semantics and §13
  PTO — slices 5 and 7. In slice 4 the watermark is testable by
  **injecting the retransmission directly** (`Tap`/`FlakyWire`
  duplication of the STREAM frame after the receive half retires), which
  exercises the identical code path without a PTO. Do that, and mark the
  loss-driven variant as owed to slice 7.
- *Discard-credit for reset-observed and abandoned-handle streams* is
  fully reachable in slice 4. *Sugar-surfaced* is slice 6.

From the §16.2 block, one obligation **partially** lands here — the
`SendStream::acked()` shape (5838–5847). `acked()` resolves on
`ConnEvent::StreamFinished`, which fires when a send half is **fully
acknowledged**, and that requires §12. See conflict C2 / open question
Q1: slice 4 cannot honestly deliver `acked()`, and should not pretend to.

## §14. Notes back to the maintainer

### §14.1 Summary

- **Cut recommendation: yes — 4a (core) / 4b (shell + stories).** The
  argument is rule 6's mechanism re-derived for slice 4, plus the three
  ways it could have failed to transfer, all checked (§0.1).
- **15 candidate rulings** (H3, H4, H5, H6, H7, H8, H10, H11, H13, H16,
  H17, and C1, C2, C7, plus the C8 exit-note decision). Of these, **C2 is
  the one I would answer first** and **H13 is the one that will cost most
  if left**: three symbols the spec uses and never defines, one of which
  (`StreamId`) is forced public by §16.2. **H17 is the cheapest to fix and
  the easiest to ship a gap through** — §10.5 enumerates two of §10's
  three violations, ruling 64's arity exactly.
- **8 conflicts reported, none resolved** (C1–C8). C2 is the serious one.
- **Zero gates run**; none claimed.

### §14.2 One thing in the brief I want to flag rather than do silently

The brief says slice 4's scope is *"§9.1–9.7 ids, implicit open, both
halves, RESET_STREAM, GC; §10 flow control; §16.9 early sends +
id-at-establishment"* and lists the shell surface
`SendStream`/`RecvStream`/`BiStream`. **`BiStream` is assigned to slice 8
by `PLAN.md`'s own table** (conflict C7). I have not resolved it; I have
planned for the type in slice 4 and the `AsyncRead`/`AsyncWrite` impls in
slice 8, and flagged it as a `PLAN.md` row that is the maintainer's to
change. Doing this silently either way would have left a breaking public
API change between two shipped slices.

I also want to be explicit about a boundary the brief did not mention:
**`SendStream::acked()` is in §16.2's handle surface and cannot be built
in slice 4** (it resolves on `StreamFinished`, which needs §12).
`PLAN.md` already assigns ruling 47's `acked()` to slice 5, so there is
no conflict — but §16.2's `impl SendStream` block lists it beside
`write`/`finish`/`reset`/`id`, and a 4b implementer working from §16.2
will build a stub for it. **4b's brief should say the block is delivered
minus `acked()`**, or the stub will be written and will lie.

### §14.3 Two sequencing requests

1. **4b must not start until ruling 90's split has landed and been
   committed.** Both work items edit `src/shell/shared.rs` and
   `src/shell/driver.rs`. That is a two-work-item path collision of the
   same shape as the slice-2a accident, and "different slices" does not
   make it safe. 4a is unaffected (it lives entirely under
   `src/core/connection/`) and can run concurrently with ruling 90's
   work.
2. **Answer Q1 and Q2 before 4a starts.** Both change the shape of
   `recv.rs` and `flow.rs` rather than a branch inside them. Q3 must be
   answered before 4a's surface freezes, since 4b compiles against it.

### §14.4 What I need from you

| # | Need | Why now |
|---|---|---|
| 1 | **Q1 / C2** — does dropping a `RecvStream` retire the half at once? | Highest-cost wrong answer in the slice; changes `recv.rs`'s shape. §16.2's sentence contradicts itself and §10.3 |
| 2 | **Q2 / C3 / H8** — reassembly allocation level | Remote memory amplification if answered by §10.6's own option (a) at the level §10.6's mandate names |
| 3 | **Q3 / C1** — `core::Connection::open()`'s pre-establishment return | Freezes 4a's surface; five §16.4 signatures |
| 4 | **Q6 / C7** — `BiStream`'s home slice | A `PLAN.md` row; avoids a breaking API change between slices |
| 5 | **H13** — `StreamId` public shape; `Dir` and `StreamsExhausted` visibility | `StreamId` is public and irreversible after release; `StreamsExhausted` would breach §18.1's closed taxonomy if made public |
| 6 | Confirmation that **4a closing zero stories** is accepted, and that its definition of done is *Appendix B's §9/§10 list plus three named precursors* | Slice 3a set the precedent and said so in its brief; this should be in 4a's brief too, not discovered at review |
| 7 | Confirmation of **C8** — Appendix B's tombstone obligation discharged by injected duplication now, loss-driven variant owed to slice 7 | An obligation quietly downgraded is how a gap ships |
| 8 | **H17** — §10.5 lists two of §10's three violations | Cheapest fix on this list and the easiest to ship a gap through; ruling 64's arity exactly. If amended, working rule 4 applies: the *"no tolerance band"* sentence is the rationale that contradicts the omitted row |
| 9 | **H16** — `STREAMS_CREDIT_BATCH` sits in a wire-constants table but is receiver policy | Wire-pin protection applied to a knob that is not on the wire; costs a needless ruling round the first time it is tuned |

### §14.5 A note on how I checked, in the spirit of working rules 11 and 12

Where a claim in this plan rests on the **code** and not only the spec, I
opened the file: `frame.rs`'s existing `is_ack_eliciting`/`retransmission`
classification of all six new frame types (`frame.rs:379–431`),
`Packing`'s reserved `Stage::Fill` and its doc comment naming slice 4
(`frame.rs:435–470`), the frame-type and flow-control constants already
present in `src/constants.rs` (166–263, and the `const _` assertions at
513–533), the error-code registry in `src/constants.rs:430–448`, and the
absence of `StreamsExhausted` from `src/error.rs`. Ruling 87's rationale
described a core factoring that did not exist and ruling 89's described a
hiss method that slither's abstraction could not reach; both cost real
work. I do not want a plan to be the third.

Where a claim rests on an **argument about state**, I have said what
state the argument assumes — H14 explicitly (the packing-order
contradiction is unreachable *because* `ack-eliciting` means what §8.7
says, and the note records where to look if that changes), and H9 (the
memory bound holds *because* the connection window binds before the
per-stream windows sum). Working rule 12's lesson is that a true lemma
about the wrong state proves nothing, and the failure mode is a clean
argument attached to a wrong verdict.
