# PLAN-7b — remediation slice

Base commit: `23d0409` ("Ruling 208: the roam proof becomes an unforgeable
challenge (WIRE CHANGE)"). Verified by `git log --oneline -1` as the first
command of this planning session (working rule 14). Working tree clean.

Inputs read: `CLAUDE.md`; `ADVERSARIAL-{amplification,handshake,liveness}.md`;
rulings 203, 207, 208, 209; targeted reads of `SPEC.md` §7.3/§7.5/§16.6/§17.5/§19
and of `src/core/connection/{mod,mobility,frame,recv}.rs`,
`src/core/endpoint/mod.rs`, `src/shell/driver.rs`, `Cargo.toml`.
**`SPEC.md` was never read whole** (working rule 1) — `grep -n` plus
`sed`-ranged reads only.

**This plan writes no Rust and touches no file outside
`.slices/07b-remediation/`.**

Companion: `CONTRACT-7b.md` — the binding API and wire contract the blind
agents build against.

---

## Open questions

Twenty. **Four block dispatch** (Q17, Q13, Q18, Q14) — an agent cannot
start without them. The rest can be ruled in parallel with the work or
deferred, and each says which.

Working rule 3 governs throughout: where the reviews, the rulings and the
spec disagree, I report and recommend; I do not resolve.

---

### 🔴 Q17 (BLOCKING, and the largest) — `SPEC.md` still argues the ACK proof in ~24 places. Who sweeps it, and does the sweep precede dispatch?

CLAUDE.md's hard rule is *"the code must match the spec — never the other
way round."* Ruling 208 changes the code. Until `SPEC.md` is swept, the two
disagree, and **the blind test author writes from the spec.**

`grep -n` for the rationale rather than the token (working rule 4) finds it
at: **280, 293, 948-950, 1830, 1850, 1980, 2003-2017, 2069, 2097, 2127,
2140-2168, 2202-2217, 2245, 2345, 2375, 4282, 4399, 5619, 6146, 6175, 6383,
6857, 7016.** Sections §7.3, §7.4, §7.5, §12, §14.6, §16.5, §17.1, §17.5,
§19, §20's constant table, and §20's test-obligation list.

Some are token swaps. **Several are load-bearing prose that argues the
position 208 reverses**, which is precisely the defect working rule 4 was
written after:

- **2161-2168** — the rationale ruling 208 itself identifies as the defect.
- **2202-2217** — *"an ACK cannot be manufactured without the key"*, and
  *"`validation_floor` is wire-free: it adds no frame type"*. Ruling 208
  adds two. This paragraph exists to argue **against** the thing that was
  just ratified, and a token sweep leaves it standing.
- **2172** — *"the budget always admits something … and what it admits is
  enough to elicit the ACK that ends it."* This is the **no-deadlock
  proof**, and `mod.rs:1958-1966` cites it by line as the justification for
  the whole `owe_elicit` stage. Ruling 182's tiebreak — *follow the
  statement some other proof depends on* — applies: this sentence has a
  dependent, so it must be rewritten deliberately, not swept.
- **6383** — see Q18.

**Recommendation: the maintainer sweeps `SPEC.md` and commits it *before*
either blind agent is briefed, and the brief names that commit.** This is
working rule 14's exact failure: slice 4a's test author was briefed on a
`CONTRACT` that was uncommitted at its cut, reconstructed an API for ten
minutes, and guessed two semantics wrong. Here the stale document is
`SPEC.md` itself — the one document every agent is told is authoritative.

**What it turns on:** whether the maintainer is willing to sweep ~24 sites
before dispatch, or would rather the contract be declared to **override**
the spec for this slice with the sweep following. I recommend against the
second: the contract is 7b's document, `SPEC.md` is the project's, and a
slice that ships code contradicting the ratified spec inverts the hard
rule for however long the gap lasts.

---

### 🔴 Q13 (BLOCKING) — ruling 208 says the challenge is *"drawn from the endpoint RNG"*, and `Connection` cannot reach the endpoint RNG

Working rule 11: *a ruling's rationale must name a mechanism that exists —
check it against the code, not only the spec.*

`Endpoint::rng` is a private `ChaCha20Rng` on the endpoint core
(`endpoint/mod.rs:153`). The challenge must be drawn at
`Amplification::arm`, and both arming sites are inside `Connection` —
`commit_roam` and `established`. `commit_roam` runs from
`Connection::handle_datagram`, deep in the connection core, with no
endpoint in scope and no route to one. **The mechanism the ruling names is
not reachable from the code that needs it.**

This is rulings 87 and 89's shape exactly: correct in conclusion, justified
by a factoring the frozen code does not have.

**Recommendation: the challenge is drawn from a `ChaCha20Rng` seeded from
§16.6's per-connection sub-seed**, which `Connection` already holds
(`mod.rs:148`) and which is **drawn today and never used**. §16.6 states
the sub-seed is *"drawn even while unused, so later connection-side
randomness cannot perturb the endpoint's draw order"* — it was designed for
exactly this, and *"one root seed reproduces the whole system"* still holds,
so ruling 208's intent (endpoint-RNG lineage, per-connection freshness,
reproducible under a test seed) is satisfied in full.

**What it turns on:** whether *"the endpoint RNG"* meant the object or the
lineage. If the maintainer means the object, the alternative is a
challenge pre-drawn by the endpoint and handed down at connection creation
— which cannot supply a **fresh** value per roam, and ruling 208 requires
per-arming freshness. I believe the sub-seed is the only design that
satisfies the ruling as written.

**Second-order consequence, and it is the good news:** this makes
`Amplification::set_floor` unnecessary and deletable. It exists only
because the msg1-anchor arming precedes the session install and the floor
is unreadable until then; a challenge has no such dependency.

---

### 🔴 Q18 (BLOCKING) — §19's deferred-features table already contains `PATH_CHALLENGE / PATH_RESPONSE`, with **different semantics**

`SPEC.md:6383`:

> | PATH_CHALLENGE / PATH_RESPONSE | the QUIC-faithful upgrade of §7.3's
> anti-amplification budget: **explicit** address validation **before**
> roam commit, where §7.3's `validation_floor` validates implicitly and
> *after* (ruling 168) — two frame types from the reserved space plus a
> second reset seam alongside the roam seam (§7.3, §14.6) |

Ruling 208 states the opposite: *"The 3× budget, `AMPLIFICATION_FACTOR`,
the **arming triggers**, and the held-not-dropped discipline all stand."*
Arming triggers standing means **the roam still commits first** and the
challenge is still the *disarm* proof — validation remains **after** the
commit. Ruling 208 therefore adopts §19's *frames* and explicitly declines
§19's *sequencing*.

**Recommendation: keep ruling 208 as ratified (validate after commit) and
amend §19's row.** Moving validation before the commit is a much larger
change — it needs a probing/alternate-path state, it changes what §14.6's
reset seam keys on, and ruling 208 does not authorise it. Ratifying the
frames without the sequencing is coherent; §19's row is now the stale text.

**What it turns on:** whether the maintainer intended 208 to *close* §19's
row or to *partially satisfy* it. If it closes the row, the row is deleted
and the "before roam commit" design is abandoned. If it partially
satisfies it, the row must be rewritten to name what remains deferred —
otherwise a future reader implements the deferred design over the top of
the shipped one. **I recommend the second: rewrite, do not delete**, and
say the remaining deferred item is pre-commit path probing.

---

### 🔴 Q14 (BLOCKING) — where do the two frames sit in §7.3's priority list and §8.5's packing order?

§7.3's priority list (SPEC:2225-2232) is `CLOSE, contested probe, pure
ACKs, PTO, keepalives, rtx, new data`. §8.5's packing order is the
codec-level sibling, realised as `Packing`'s `Stage { Ack, Control, Fill,
Ping }` (`frame.rs:871`). Ruling 208 places the new frames in neither, and
working rule 8 reads both lists as exhaustive.

**Recommendation:**

- **§7.3 priority: `PATH_CHALLENGE` at position 2, immediately after
  CLOSE** — ahead of the contested probe. Under a scarce budget the
  challenge is the **only** output that can end the scarcity; anything
  ranked above it spends budget the escape is waiting for, which is ruling
  171's own argument for the contested probe, applied to the thing that
  outranks it. `PATH_RESPONSE` beside pure ACKs (position 3-4): it costs us
  nothing and unblocks the peer.
- **§8.5 / `Stage`: both in `Stage::Control`.** Not `Stage::Ping` — `Ping`
  is last, and under a room clamped near 39 bytes, last is nowhere.

**What it turns on:** whether the challenge outranks the contested probe.
The case against: ruling 171 says a pending probe *"outranks all other
output"* and `pump_packets` enforces it by **returning early**
(`mod.rs:1875-1879`), so a challenge below it never gets built while a mark
is pending — and a pending mark on an unvalidated address is exactly the
state F1's unbounded variant lives in. The case for is above. **These two
rulings collide in the one state both were written for**, and I do not
think either text resolves it.

---

### Q1 — ruling 208 closes A1, but **A1b is untouched, and the reviews say so in different words**

`ADVERSARIAL-amplification.md` finding **1b**: an on-path attacker A, with
no key at all, rewrites one genuine peer→us packet's source to victim V; we
roam to V; A captures our egress and **forwards our packet to the real
peer**; the peer answers; A rewrites the answer's source to V. Post-208
that answer is a `PATH_RESPONSE` carrying our 8 bytes. **It validates.**

The challenge proves *a packet we sent toward V reached the key holder*. It
does not prove *anything at V received it*. Those coincide only if nobody
between us and V can carry a packet — the assumption the attacker in the
rationale's own sentence violates. This is QUIC's known limitation too;
RFC 9000 does not claim path validation defeats an on-path attacker.

**Ruling 208 does not mention 1b, and its rationale — *"a peer that did not
receive at that address cannot guess it"* — is true and scoped.** That is
defect class 1: a stated construction with an unstated scope. Restating it
without the scope in the crate docs would repeat, inside the fix, the
defect being fixed.

**Recommendation: accept the residual and *state* it, in one sentence, in
§7.3's honesty-clause voice.** Something of the form: *the challenge proves
the key holder received at that address, or that something on the path
between us and it did; against an on-path attacker the budget is escapable
at the cost of relaying one packet per arming, and §7.3's ratio is the only
continuing cost.* The reviewer's own suggestion — keep the 3× ratio binding
on any address roamed to within the last N seconds — is the cheapest repair
and would restore a **continuing** cost against A1b. It is a real design
change and I am not proposing it for this slice.

**What it turns on:** whether v0.2 ships with an unstated on-path residual.
§6.9 and §17.1 already carry honesty clauses for exactly this purpose, and
this project's practice is to state exposures rather than let them be
found. **Not blocking** — it is one paragraph and can land at integration —
but it must not be forgotten, because the whole slice reads as "return
routability is now proven" if nobody writes it down.

---

### Q2 — A2: ratify charging msg2 to the budget, or amend §7.3's MUST?

msg2's 107 bytes go to the unvalidated msg1 anchor from the **endpoint**
(`staged.rs:753`, `routing.rs:543`) before the connection exists, and
`arm` starts `sent = 0`. Measured 3.55× against a normative MUST of 3.

**Recommendation: charge it.** `established()`'s `anchor_from_msg1` branch
calls `on_sent(RESP_PACKET_LEN)` immediately after arming — one site, no
signature change, no endpoint-side table (ruling 170 forbids one). Cost:
the responder's opening budget drops 588 → 481, which still admits a
39-byte challenge with room to spare.

**What it turns on:** whether the maintainer would rather amend §7.3 to
exclude the handshake response from the count. That is defensible — msg2 is
a fixed 107 bytes, not attacker-scalable — but it makes a normative MUST
say "3, except for one packet", and §7.3's whole value is that the ratio is
stated without exceptions. **Sub-question the implementer must answer
either way:** can msg2 be emitted more than once against a single arming?

---

### Q4 — F1: which fix, and I am declining half the review's suggestion

**Recommendation: two changes, and only one of them is the review's.**

1. **Core (the fix):** `sync_liveness_timer` arms `Keepalive` and
   `PersistentKeepalive` only when a keepalive can actually leave — the
   same predicate `transmit_keepalive` guards on, factored into one
   function used by both. `TimerKind::Liveness` is **not** suppressed.
2. **Shell (the detector):** `debug_assert!(deadline >= now)` in
   `Driver::deadline`.

**I am declining the review's *"clamp `Driver::deadline` to `now`"*
(working rule 5).** A clamp does not stop the spin: `sleep_until(max(d,
now))` completes immediately for exactly the same set of deadlines. It
would look like a second-line defence and be a no-op. An **assertion** is
the thing that pays, because a spin is invisible on the wire (ruling 141)
and unreachable from `FlakyWire`, which models a network and not a CPU
(working rule 13) — an assertion in the one function every drain passes
through converts the whole class into a test failure in every debug run.

**What it turns on:** whether suppressing the keepalive arming while held
is acceptable given that it leaves a connection announcing `Timeout(None)`
in the `armed == false` state — parked, not dying. I believe it is (that is
"held, not dropped" in the timer table) but it is a behaviour statement §7.5
does not currently make.

---

### Q19 — does ruling 208 reach §7.5's **contested probe** floor, which is the same `largest >= floor` shape?

`Connection::on_ack_coverage` (`mod.rs:1271-1306`) serves **two**
independent floors: §7.3's amplification floor and §7.5's contested probe
floor. Both clear on an ACK. Ruling 208's argument — an ACK is an assertion
by the key holder, not a proof — applies verbatim to the second.

**Recommendation: no. Ruling 208 reaches only §7.3's floor, and this should
be said rather than left inferred.** The contested mark's threat model is a
peer that **cannot produce the ACK** — §15.4's row is written for a peer
that *"has already restarted and holds nothing"*. An adversary who holds
the key **is** the peer, and a peer that answers its own probe is a peer
that is alive, which is all the probe asks. The forgery that breaks §7.3
is a non-event for §7.5.

**What it turns on:** nothing in the reviews raised it; I raise it because
rule 8 says a construction's scope is read as exhaustive, and 208's scope
is unstated here. **Its practical importance is the implementation
hazard**, which is in `CONTRACT-7b.md` §1.7 with a warning box: deleting
`on_ack_coverage` wholesale silently disables ruling 176's contested
machinery, and no wire test would notice.

---

### Q7 — F7: a pending contested mark suppresses the passive dance for up to `DEAD_TIMEOUT`, and §15.4 promises the opposite

The liveness reviewer reports this as a **conflict**, correctly, and does
not resolve it. §7.3 / ruling 171's priority order says the pending probe
outranks the keepalive (`transmit_keepalive`'s guard implements it).
§15.4's contested row promises *"a healthy peer is unaffected and keeps its
side for its own `DEAD_TIMEOUT`."* They differ exactly when the mark stays
pending longer than `DEAD_TIMEOUT − KEEPALIVE_TIMEOUT` — 15 s.

**Recommendation: defer the *behaviour*, but note that Q4 and Q14 both
move it.** Under Q14's recommendation the challenge outranks the pending
probe, so an unvalidated address escapes and the mark stops being pending;
under Q4's recommendation the connection parks instead of spinning. Both
shrink F7's reachable window without deciding it. **Deciding it needs a
ruling on which of §7.3's priority and §15.4's promise governs** — and it
is worth noting that §15.4's row is *prose about the peer's experience*
while ruling 171's order is a *formal rule*, which is the one shape this
project has resolved in both directions (nine times prose, once formal).

---

### Q3 — A4: the budget counts UDP payload; reflection is measured on the wire

28 B (v4) / 48 B (v6) of IP+UDP header per packet are outside the count.
One large packet in, many small packets out. The reviewer computes ~5.5×
v4 / ~6.7× v6 against a ratified factor of 3 — and notes ruling 203's fix
**deliberately** makes output smaller and more numerous. Ruling 208 adds a
39-byte challenge packet to the same regime.

**Recommendation: defer, and record the reason.** Changing the unit is a
§7.3 definitional change; it would need `MIN_WIRE_OVERHEAD`-style constants
that are IP-version dependent, which the core cannot always know. **Not
dispatched this slice.** Worth one line in §7.3 stating the unit is UDP
payload and the on-wire ratio is correspondingly higher — that costs
nothing and stops the next reviewer rediscovering it.

---

### Q5 — F3: is the fix "skip a fully-covered frame", or a byte-denominated reassembly bound?

**Recommendation: the skip, this slice.** It closes the amplification
exactly — the attack is *zero-progress* work, and after the skip `insert`'s
allocation is bounded by bytes that are **new**, hence by flow credit. It
is ~10 lines inside `Reassembly::insert` and needs no ruling on a constant.

**What it turns on:** F4 (Q6) proposes a byte-denominated bound over the
same structure. **If the maintainer wants that, doing it *with* F3 is much
cheaper than after** — same file, same function, one set of tests. If not,
the skip stands alone and is complete.

---

### Q6 — F4: §17.5's *"the credit term dominates"* is arithmetically false

160 peer-opened streams × 1 024 chunks = 163 840 chunks ⇒ ~8-11 MB of
metadata against 320 KB of charged credit. `REASSEMBLY_CHUNKS_MAX` bounds
the **count**, not the **cost**, so it changes the multiplier from the
"25-50×" the parenthetical names to ~10× — the same defect at a smaller
constant. Working rule 11 at the specification level.

**Recommendation: amend §17.5's row this slice (prose, maintainer-owned);
defer the byte-denominated bound.** The prose is wrong today and cheap to
fix; the bound is a design change with a constant that needs ratifying.
**But see Q5** — if the bound is wanted, it is much cheaper now.

---

### Q8 — H1: §6.9's *"2 DH ceiling"* vs §6.6 step 4's 4 DH

§6.9:1888 states an **absolute**: *"the maximum cost of any single attacker
packet is 2 DH."* §6.6:1652-1655 prices the tie-break admit at `es + ss +
ee + se` = **4**, and `routing.rs:404-414` carries its own table saying 4.
Reachable with **no spoofing at all** and it destroys a dial: the
application gets `Ok(Connection)` to an attacker-chosen address that dies
at `DEAD_TIMEOUT`. The same row's *"the same replay costs only what the
application chooses to probe"* is wrong the same way — the app chooses 1
and gets 4.

**Recommendation: correct §6.9 to 4 DH.** The code, §6.6 and §6.7's honesty
clause all agree; §6.9 is a cost summary written loosely, and **ruling 177
is the precedent for the shape and for the section**. No code change.

**What it turns on:** nothing much — this is the clearest of the twenty.
It is here because it needs a ruling and no agent may edit `SPEC.md`.

---

### Q9 — H2: does `lib.rs`'s documentation-obligation list grow to six?

`AuthError::Replay` is forgeable by a third party from one captured msg1,
against a static the `ss` has **proven**. Documentation obligation #2
(`lib.rs:83-87`) stops at the *claimed* static — one rung below.

**Recommendation: widen obligation #2 rather than add a sixth.** The
hazard is identical (a third party gets a peer banned); only the rung
differs. A sixth obligation invites the reading that #2 no longer covers
the claimed case. Plus the variant-level paragraph on `AuthError::Replay`
itself, which is unconditional and needs no ruling.

**What it turns on:** whether the five obligations are a numbered contract
consumers cite. If they are, renumbering is worse than appending.

---

### Q10 — H3: may the ratified API surface gain an OS-entropy constructor three days after ruling 204?

`SoftwareIdentity<S, R = ChaCha20Rng>` is the source of the static scalar
and **every handshake ephemeral on both roles**, takes `R` as a mandatory
argument, has **no OS-entropy path anywhere in the crate**, and documents
determinism as a feature. `EndpointBuilder::rng_seed` — far less critical —
has both a test-only warning and a `getrandom::fill` default.

**Recommendation: add `SoftwareIdentity::generate_os()` /
`from_scalar_os()`**, seeding `ChaCha20Rng` from `getrandom::fill`,
mirroring `endpoint.rs:405-409` exactly. Additive, no dependency, no
breakage; ruling 204 ratified the surface as *reviewed*, not as *closed*.
**A doc that says "use an OS RNG" when the crate offers no OS path is not a
fix**, which is why I do not recommend documentation alone.

Do **not** remove or deprecate the `R` parameter — `testutil` depends on
the seeded form and §16.6's reproducibility rests on it.

**What it turns on:** whether ruling 204 froze the surface. If it did, this
becomes documentation-only and the asymmetry stands.

---

### Q11 — M2: should `with_intro_queue_cap` / `with_intro_max_per_source` validate?

`0` on either **silently disables all inbound accepts, permanently and
invisibly**. `max_per_source ≥ cap` deletes the only occupant-shaped
defence in the protocol — §6.3's *"filling the queue needs ≥ 256 distinct
sources"* is exactly `cap / max_per_source` — and §6.3's own NAT paragraph
gives an operator a legitimate reason to raise it.

**Recommendation for this slice: documentation only.** A doc block naming
what each value is load-bearing for, in the shape `with_epoch_size` already
uses one screen below (`config.rs:115-134`). Zero API cost, available now,
and it closes the *"nothing tells them what they are giving up"* half.

**What it turns on:** whether the builders should return `Result` (a
breaking signature change on a ratified surface) or clamp with a
`tracing::warn!` (silent-ish behaviour change). Both are real options; both
are bigger than this slice, and neither is needed to close the
documentation gap.

---

### Q12 — three maintainer-owned text corrections (L1, L2, L3b)

- **L1 —** ruling 77's *"`TS_GUARD_ORPHAN_CAP` … **still evicts, so memory
  is capped**"* (rulings.md:1468-1470, carried to SPEC:6002-6004) does not
  reach a `Claimed`-pinned entry: `evict_if_over_cap` filters
  `!entry.pinned(now)` and a `Claimed` pin increments `pins`. Direction is
  **safe**; the memory claim is true for a *different* reason (§17.5's
  pinned-tier ceiling). **Working rule 4(b) applies**: a correction to 77
  inherits the duty to address its **reasoning**, not merely its sentence —
  ruling 175 did exactly that wrong and ruling 188 caught it.
- **L2 —** §17.1's honesty clause prices the orphan flush at ~1024
  evictions; the code stalls at **one** (steady state 1023). Safe
  direction, but an honesty clause that overstates a live attack is wrong,
  and §6.9/§17.1's honesty clauses are load-bearing here.
- **L3b —** §17.2's *"It survives across connection generations"* is silent
  on process restarts, which `last_init_timestamp` does not survive
  (`endpoint/mod.rs:166`). Consequence bounded at ≤ ~115 s. One sentence.

**Recommendation: rule all three as text corrections; none needs code.**
Not blocking.

---

### Q15 — should `Intro`'s `Debug` stop printing `source` and `sender_index`?

`Claimed`'s `Debug` already suppresses the claimed static *because*
printing it *"invites exactly the logging that documentation obligation #2
warns about"* (`staged.rs:227-229`). `Intro`'s `Debug` prints the other two
§6.1 quantities (`staged.rs:156-163`), and the source address is the
**worse** key — 0 DH, no key knowledge, no routability proof.

**Recommendation: document this slice, do not change `Debug`.** The
argument for suppressing is real and symmetric; the argument against is
that `Intro`'s `Debug` is the operator's only view of a stage-0 arrival and
`Claimed` suppresses a *secret-ish* value where `Intro` would suppress
*diagnostics*. Genuinely two-sided, and it is not what this slice is for.

---

### Q16 — the brief says the golden-wire vectors *"will go red by design"*. On this design they should not.

Working rule 5: my brief tells me something that looks wrong, so I say so.

Ruling 208 **adds two frame type codes and moves no existing byte.** The
handshake wire, the packet headers and every existing frame encoding are
untouched. Adding `0x1a`/`0x1b` cannot change a byte of any existing golden
vector, and `IK_MSG1_LEN` / `IK_MSG2_LEN` / `INIT_PACKET_LEN` /
`RESP_PACKET_LEN` / `VERSION` / `PROLOGUE` are all unmoved.

**So a red wire test in this slice is the stop signal in full force, not
the licensed exception.** The one legitimate expectation change I have
found is A2's: the responder's post-install budget counters move from
`(0, 196)` to `(107, 196)`. That is a **counter**, not a wire byte.

**What it turns on:** whether the maintainer knows of a golden vector that
enumerates the frame-code table or asserts a "no unknown types" property.
If one exists, it moves and my analysis is incomplete. **If none exists, I
recommend the brief's licence be withdrawn before dispatch** — an
implementer told in advance that red wire tests are expected is an
implementer who will update one that mattered.

---

### Q20 — CLAUDE.md working rules 6 and 15 give opposite answers for `#[cfg(test)] mod <new>;`

Rule 6: *"If the implementer needs a module to compile against, it declares
`#[cfg(test)] mod tests;` and **creates nothing** — the file is the test
author's alone."*

Rule 15: a file valid only once **both** agents' work exists belongs to the
**integrator**, because `Cargo.toml`'s `[[test]]` stanza naming a missing
file means cargo *"refuses to parse the manifest"* — **no gate can run at
all.**

`mod tests_path;` naming a missing file is a **compile error**, which is
the same failure one layer down: the implementer cannot run `cargo build`,
let alone the nine gates, while rule 7 forbids reporting a gate green
without running it. Rule 6 tells the implementer to create the condition
rule 15 exists to prevent.

**Recommendation: rule 15 governs, and rule 6's clause should be amended to
say so.** Rule 15 is the later and more specific lesson and its reasoning
is strictly stronger. In this slice I have assigned **both** the
`Cargo.toml` stanza and the `mod` declaration to the integrator; the
dispatch split below says how each agent's tree stays green without them.

---

## Scope

**In, and dispatched:** ruling 208 (wire + core), A2, A3, F1, F3, F5, H2,
M1, H3 (contingent on Q10), M2 (documentation half).

**In, but maintainer-owned text — no agent touches these:** Q17's `SPEC.md`
sweep, H1, L1, L2, L3b, Q18's §19 row, Q1's honesty sentence.

**Scoped and deliberately not built:** F2.

**Deferred with a stated reason:** A4, F4's bound, F6, L3a, M2's validation
half, Q15's `Debug` change.

**Not in this slice at all:** anything from slice 8. This slice is
remediation; `PLAN-8.md` and `CONTRACT-8.md` (committed at `743be07`) are
untouched by it, and slice 8 resumes after 7b closes.

---

## F2 — scoped, not built

The cost, since the brief asks for it.

**The defect.** `Driver::handle_timeout` (`driver.rs:886-896`) calls
`core.handle_timeout(now)` on **every** connection and sets `dirty = true`
unconditionally, for **any** expired deadline anywhere. `serve()` then
drains all N and `deadline()` polls all N. §7.5's dance is self-sustaining
and unstoppable (ruling 39), so every connection that ever carried a byte
produces ≥ 1 timer event per `KEEPALIVE_TIMEOUT` forever ⇒ **O(N²/10) full
core drains per second on a wholly idle endpoint.** N = 1 000 ⇒ 10⁵/s.

**The fix shape.** `handle_timeout` consults each connection's announced
deadline before calling into it, and dirties only cores it actually called.
The deadline is already tracked per connection to compute the `min`, so the
data is there; what is missing is storing it rather than recomputing it.

**Cost estimate: 1 agent-slice, roughly half of it harness.** The code
change is perhaps 40 lines in `driver.rs` plus a per-connection
`Option<Instant>` cache and the invalidation discipline that keeps it
honest — and the invalidation is where the bugs are, because a stale cached
deadline is a **missed timer**, which is a silent liveness failure and
strictly worse than the quadratic it replaces.

**Why it is not dispatched here:**

1. **It shares `driver.rs` with F1.** F1 is the HIGH with an unbounded
   variant; two agents in one file is working rule 6's race, and one agent
   doing both in one slice makes the integrator's review of the HIGH harder
   for no reason.
2. **No test in the tree can fail on it.** `FlakyWire` runs two endpoints.
   A fixture that models everything a *network* does cannot express N = 1
   000 idle connections — **working rule 13 exactly**, and the seam review
   already lost two of four findings to this same gap. Building the fix
   without a harness that shows the quadratic means shipping an unverified
   performance claim.
3. **The harness is the real work.** An N-connection idle-endpoint bench,
   on the paused clock, asserting **core-drain count** rather than wall
   time (wall time on a paused clock measures nothing). That is a
   `testutil` addition and a design in its own right.

**Recommendation: F2 becomes its own slice, 7c, whose first deliverable is
the N-connection harness.** It is a HIGH with no wire consequence and no
security consequence, which makes it exactly the kind of work that should
wait for a fixture that can prove it.

---

## Triage of the three review reports

Every finding in all three reports appears below with a disposition. Nothing
is silently dropped.

### `ADVERSARIAL-amplification.md` (4 findings)

| # | Sev | Disposition |
|---|---|---|
| A1 / A1b | Critical | **Ruling 208 supersedes.** A1 (keyed peer forges the ACK) is closed by the challenge. **A1b (on-path relay) is *not* closed by ruling 208** — see open question Q1. In scope: implement 208; A1b's residual is a ratification question. |
| A2 | High | **In scope.** msg2's 107 B charged to nothing → 3.55× against a normative MUST of 3. Needs a ruling on *where* the charge lands (Q2). |
| A3 | Medium | **In scope.** `mod.rs:489`'s `match roamed` credits `on_recv` from any source on a non-live connection. Small, local, well-understood; the sibling guard 19 lines below is the model. |
| A4 | Low-Med | **DEFER, with a recorded reason.** The payload-vs-wire unit gap (IP+UDP headers uncounted, 28 B v4 / 48 B v6 per packet). Changing the unit is a spec change to §7.3's definition of the budget, it interacts with ruling 203's deliberately-smaller packets *and* with 208's small challenge packets, and it is arithmetic rather than a bypass. Raised as open question Q3 so the maintainer decides; **not** dispatched to an implementer this slice. |

**A1b matters for what this slice *claims*.** Implementing 208 and writing
"return routability is now proven" in the crate docs would repeat the exact
defect 208 was ratified to fix: a stated construction with an unstated
scope (working rule 8). The documentation wording is in scope even though
the residual is not.

### `ADVERSARIAL-liveness.md` (7 findings)

| # | Sev | Disposition |
|---|---|---|
| F1 | **HIGH** | **In scope, top priority after 208.** The keepalive/beacon livelock — ruling 141's defect surviving in `sync_liveness_timer`. 100 % CPU; the `contested`-pending and `armed == false` variants are **unbounded**. Needs a ruling on *which* fix (Q4): re-arm at the instant the hold could lift, and/or clamp `Driver::deadline` to `now`. My recommendation: **both**, and the reviewer's framing supports it. |
| F2 | HIGH | **Scope-and-cost only, no implementation this slice.** Deliberate: see "F2 is scoped, not built" below. |
| F3 | MED-HIGH | **In scope.** Reassembly re-coalescing: O(span) per frame, zero flow credit, ~10⁴× CPU amplification. Local to `recv.rs`; no wire change; no ruling strictly required, but the *policy* choice needs one (Q5). |
| F4 | MED | **DEFER — spec defect, not a code defect.** §17.5's *"the credit term dominates"* is arithmetically false; metadata is ~10× credit. The fix is either a byte-denominated reassembly bound (a real design change), a smaller `REASSEMBLY_CHUNKS_MAX` (a constant change), or an amended §17.5 row (prose). Raised as Q6. **Note F3 and F4 touch the same structure** — if the maintainer wants a byte-denominated bound, doing it *with* F3 is much cheaper than after. |
| F5 | MED | **In scope — smallest item in the slice.** `warn!` per evicted datagram is a peer-driven log flood. Fix is rate-limiting/backoff while preserving §11.5's visibility requirement. Cheap, self-contained, `datagram.rs` only. |
| F6 | LOW-MED | **DEFER.** `Reassembly::read`'s `drain(..n)` is O(bytes remaining) per call. Bounded by delivered bytes × chunk/read ratio, and a well-behaved reader never sees it. The fix (a front-chunk cursor instead of `drain`) is genuinely small — flagged as an *optional* stretch item, see sequencing. |
| F7 | LOW | **DEFER to a ruling, no code.** A pending contested mark stops the passive dance for up to `DEAD_TIMEOUT`, killing a healthy peer. The reviewer correctly reports it as a **conflict between §7.3/ruling 171's priority order and §15.4's "a healthy peer is unaffected"** rather than resolving it. Q7. **F1's fix partially overlaps this** — see the sequencing note. |

### `ADVERSARIAL-handshake.md` (8 findings)

| # | Sev | Disposition |
|---|---|---|
| H1 | High | **Ruling only, no code.** §6.9's *"the maximum cost of any single attacker packet is 2 DH"* vs §6.6 step 4's 4 DH. The code and `routing.rs:404-414`'s own table agree with §6.6. A spec-internal conflict; the reviewer correctly reports rather than resolves (working rule 3). Q8. **Ruling 177 is the precedent for the shape *and* the section.** |
| H2 | High | **In scope — documentation.** `AuthError::Replay` is a third-party-forgeable accusation against a *proven* static; documentation obligation #2 stops at *claimed*. Fix is a doc paragraph on the variant plus (probably) a sixth documentation obligation. Q9 asks whether the obligation list grows. |
| H3 | High | **In scope — but it is an API addition, and the API was ratified 3 days ago (ruling 204).** Needs Q10 answered before an implementer can act. |
| M1 | Med | **In scope — documentation, bundled with H2.** §6.1 names three unauthenticated quantities (claimed static, **source address**, **`sender_index`**); every warning covers one. `Intro`'s `Debug` prints both of the uncovered two. Cheap, and it is the same author and the same files as H2. |
| M2 | Med | **In scope, narrowly — documentation only, no validation.** Q11: making the builders validate is a breaking signature change on a ratified API. The doc-block shape `with_epoch_size` already uses is available *today* at zero API cost and closes the "nothing tells them what they are giving up" half. |
| L1 | Low | **`rulings.md` correction — maintainer-owned, flagged not done.** Ruling 77's *"the LRU still evicts, so memory is capped"* does not reach a `Claimed`-pinned entry. Direction is safe; the memory claim is true for a different reason (§17.5's pinned-tier ceiling). Working rule 4(b) applies: correcting it must address 77's **reasoning**, not just its sentence. Q12. |
| L2 | Low | **Spec prose correction — maintainer-owned.** §17.1's honesty clause prices the orphan flush at ~1024 evictions; the code stalls after **one**. Divergence is in the safe direction. Bundled into Q12. |
| L3 | Low | **DEFER both.** (a) `Timestamp::succ` saturates at year ≈584 billion — a `debug_assert` is the whole fix and it is not worth an agent's slot; listed as an optional stretch item. (b) §17.2's *"survives across connection generations"* vs the field not surviving a process restart — one sentence of spec prose, maintainer-owned, bundled into Q12. |

**Three maintainer-owned files, one warning.** L1, L2, L3(b) and H1 all land
in `SPEC.md` / `rulings.md`, which **no agent in this slice may touch**.
They are listed so they are not lost, not so they are done here.

**F2 is scoped, not built, and here is the reasoning.** F2 is a real O(N²)
and its fix shape is clear (consult the per-connection deadline before
calling `handle_timeout`, dirty only cores actually called). But it is a
change to `Driver`'s central loop, it is the *same file and the same
function* F1's clamp touches, and F1 is the HIGH with an unbounded variant.
Landing both in one slice means one agent owns `driver.rs` for two
unrelated reasons, and the O(N²) fix has no test that fails today —
`FlakyWire` runs two endpoints, so the fixture cannot express N = 1 000
(working rule 13: **the fixture bounds the coverage**). Cost estimate and
the test-harness gap it needs are in `## F2 — scoped, not built` below.

---

## Sequencing

Five phases. Phase 0 is the maintainer's and **nothing dispatches until it
commits**.

### Phase 0 — the maintainer (blocking)

1. Rule **Q17, Q13, Q18, Q14** — the four that block.
2. Sweep `SPEC.md` for ruling 208 (Q17's ~24 sites), amend §19's row
   (Q18), place the two frames in §7.3's priority list and §8.5 (Q14).
3. Rule the rest, or defer them explicitly.
4. Amend `CONTRACT-7b.md` where a ruling contradicts it — **this file is
   binding on the blind agents and must be true at their cut.**
5. **Commit all of it.** Then name that commit in every brief.

> Working rule 14, both halves. Slice 4a's author reconstructed an API for
> ten minutes because `CONTRACT-4a.md` was uncommitted at its cut; slice
> 7's third author was briefed at one commit and cut from another that
> contained the implementation it existed to be blind to, and caught it on
> its **first command**. Every brief names its base commit; every agent
> verifies its base as its first act and reports what it sees.

### Phase 1 — implementation and tests, in parallel, mutually blind

Agents **I1, I2, T1, T2**. Disjoint paths, enumerated below.

### Phase 2 — integration

The integrator lands the rule-15 files, resolves collisions, runs the
gates. Details below.

### Phase 3 — the residual text

Q1's honesty sentence, and any `rulings.md` entries the phase-1 agents
generate. Maintainer-owned.

### Phase 4 — close

Nine gates on the exact commit. Then slice 8 resumes from `743be07`.

**Why ruling 208 goes first inside phase 1 rather than F1.** F1 is the
HIGH, but 208 is what moves the API every other item builds against
(`Amplification::arm`'s signature, `on_ack_coverage`'s halves,
`Connection`'s new RNG field). Serialising them would idle three agents; so
they run in parallel with 208's API frozen by `CONTRACT-7b.md` §1 rather
than by 208 landing first. **That is the contract's whole job.**

---

## Blind dispatch split

Four agents. **Every path below is owned by exactly one of them.** Working
rule 6: in slice 2a an implementer's placeholder stub destroyed 68
independently-written tests because two briefs named `src/core/tests.rs`,
and the finish order decided who won.

### I1 — implementer, connection core and shell

**Owns, exclusively:**

```
src/constants.rs
src/core/connection/frame.rs
src/core/connection/mobility.rs
src/core/connection/mod.rs
src/shell/driver.rs
```

**Builds:** ruling 208 (`CONTRACT-7b.md` §1), A2 (§2), A3 (§3), F1 (§4).

**Also owns — the expired-premise migration.** The signature changes in §1.3
stop these compiling: `src/core/connection/tests_roam.rs`,
`tests_contested.rs`, `tests_sizing.rs`, `tests.rs`, and any other existing
`tests_*.rs` that references `validation_floor` / `Amplification::floor` /
`set_floor` / `on_ack_covering`. Plus the two closed-list tables in its own
files (`CONTRACT-7b.md` §1.1 items 1 and 2) — including
`frame.rs`'s `ack_eliciting_matches_the_whole_of_table_8_3`, whose doc
claims *"all twelve rows"* of §8.3 and now needs fourteen. **Add the rows;
do not relax the test** — its own doc says a two-arm classifier is
*"correct by accident"* without it.

> **This is a deliberate exception to rule 6 and it is the split's one soft
> spot.** The tree must compile for the implementer to run a single gate
> (rule 7), and these are *existing* tests whose premise moved — not
> acceptance tests for new behaviour, which are T1's and T2's alone.
> **Three constraints make it safe:**
> 1. **Narrow or invert, never delete** (process-memo defect class 3): the
>    mutation each test was built to catch is usually still live by a new
>    route. *"An ACK below the floor proves nothing"* becomes *"a
>    PATH_RESPONSE with the wrong bytes proves nothing"*, which is a
>    **better** test after the change.
> 2. **I1's report must list every migrated assertion**, old text and new,
>    with one line on why the new one is not weaker.
> 3. **The integrator diffs every one of them** against constraint 1. A
>    migration that turns an assertion into a tautology is the failure mode,
>    and it is invisible in a green CI run.

**Must NOT touch:** anything below, and in particular **must not create**
`src/core/connection/tests_path.rs`, `tests_livelock.rs`, or any `tests/`
file. Must not add the `mod` declarations for them (see integrator).

### I2 — implementer, periphery and documentation

**Owns, exclusively:**

```
src/core/connection/recv.rs
src/core/connection/datagram.rs
src/error.rs
src/identity.rs
src/lib.rs
src/shell/staged.rs
src/config.rs
```

**Builds:** F3 (`CONTRACT-7b.md` §5), F5 (§6), H2 + M1 (§7), H3 (§8,
contingent on Q10), M2's documentation half (Q11).

**Disjointness verified against I1:** F3 is entirely inside
`Reassembly::insert`; F5 entirely inside `trace_drop`. Frame *application*
dispatch is at `mod.rs:1439-1470`, which is I1's — I2 needs no line of it.
`lib.rs` carries the documentation obligations only; ruling 208 adds no
public item to it.

**If I2 finds it needs a line in one of I1's files, it stops and reports.**
It does not edit. That report is a planning defect and I want to hear about
it, not have it papered over.

### T1 — test author, ruling 208 (blind to I1 and I2)

**Owns, exclusively:**

```
src/core/connection/tests_path.rs          (new)
tests/story_path.rs                        (new)
tests/spec_constants.rs                    (additions only)
src/core/connection/testfix.rs             (additions only)
```

**Writes from `CONTRACT-7b.md` §1, §2, §3, §9 and the spec sections quoted
into its brief. It does not read `src/core/connection/mod.rs`,
`frame.rs` or `mobility.rs`.**

Coverage it owes: the frame codec (encode/decode/`encoded_len`/
ack-eliciting, and the 7/8/**9**-byte boundary — both sides, per slice 1's
one-sided-boundary defect); challenge generation per arming and
non-reuse across armings; `from_anchor` gating on the response; the
wrong-bytes negative; the smallest-budget escape (roam on a 30-byte
keepalive, challenge fits, address validates in one round trip); A2's
charged counters; A3's closing-connection negative.

**`testfix.rs` is T1's** because the raw `path_challenge_frame` builder is
a test fixture and I1 does not need one — I1 has `Frame::PathChallenge`
directly. **I1 and I2 are forbidden to touch `testfix.rs`.**

### T2 — test author, remediation findings (blind to I1 and I2)

**Owns, exclusively:**

```
src/core/connection/tests_livelock.rs      (new)
src/core/connection/tests_reassembly.rs    (new)
```

**Writes from `CONTRACT-7b.md` §4, §5, §6.**

Coverage it owes: F1's **deadline-in-the-past** assertion in both variants
(budget-refused and `contested`-pending), plus the second assertion that
the `Liveness` deadline is *still* announced — **two builds must fail, not
one**; F3's zero-progress insert; F5's backoff.

> **T2's brief must carry `CONTRACT-7b.md` §4.3 verbatim.** A blind author
> asked to test a livelock will reach for "assert the CPU spins", and on
> tokio's paused clock that is an infinite loop that never advances virtual
> time — a **hanging** test, which reads as CI flake rather than as a
> finding. §4.3 gives the testable statement instead.

### Disjointness table

| path | owner |
|---|---|
| `src/constants.rs` | I1 |
| `src/core/connection/frame.rs` | I1 |
| `src/core/connection/mobility.rs` | I1 |
| `src/core/connection/mod.rs` | I1 |
| `src/shell/driver.rs` | I1 |
| existing `src/core/connection/tests*.rs` | I1 (migration only) |
| `src/core/connection/recv.rs` | I2 |
| `src/core/connection/datagram.rs` | I2 |
| `src/error.rs`, `src/identity.rs`, `src/lib.rs`, `src/config.rs`, `src/shell/staged.rs` | I2 |
| `src/core/connection/tests_path.rs`, `tests/story_path.rs`, `tests/spec_constants.rs`, `src/core/connection/testfix.rs` | T1 |
| `src/core/connection/tests_livelock.rs`, `tests_reassembly.rs` | T2 |
| **`Cargo.toml`**, **the `mod` declarations** | **integrator** |

---

## Integrator-owned files (working rule 15)

Two, and the second is a **new instance of rule 15 in this project**.

### 1. `Cargo.toml`'s `[[test]]` stanza for `tests/story_path.rs`

The known case. Cargo **refuses to parse a manifest** whose `[[test]]`
names a missing file, so an implementer landing it early leaves a tree on
which **no gate can run at all** — while rule 7 forbids reporting a gate
green without running it, and creating a placeholder test file is the
slice-2a accident that destroyed 68 tests.

`Cargo.toml` already carries the precedent verbatim from slice 7 (ruling
194's comment above the `story_keepalive` / `story_mobility` stanzas),
including the reason it is not cosmetic: **there is no `autotests = false`,
so cargo auto-discovers the target without its `required-features` and the
feature-less `cargo test` gate fails on `unresolved import
slither::testutil` until the stanza exists.**

```toml
# Slice 7b. Integrator's (working rule 15) — T1's file, cut after I1's tree.
[[test]]
name = "story_path"
required-features = ["test-util"]
```

### 2. The `#[cfg(test)] mod …;` declarations in `src/core/connection/mod.rs`

```rust
#[cfg(test)] mod tests_path;        // T1
#[cfg(test)] mod tests_livelock;    // T2
#[cfg(test)] mod tests_reassembly;  // T2
```

`mod.rs` is I1's file, but **these three lines are not I1's to write** —
a `mod` declaration naming a missing file is a compile error, which is
`Cargo.toml`'s failure one layer down: I1 could not run `cargo build`, let
alone the nine gates.

**This is where CLAUDE.md rules 6 and 15 collide** (Q20). Rule 6 says the
implementer declares the module and creates nothing; rule 15 says a file
valid only once both agents' work exists is the integrator's. **Rule 15
governs here**, and the recommendation in Q20 is that rule 6's clause be
amended to say so.

**How each agent's tree stays green without them:** T1 and T2 add the
declaration *in their own worktrees only*, and **do not commit it** — their
deliverable is the test file. The integrator adds the three lines once,
against a tree that contains all four agents' work. Each brief must say
this in terms, because an agent that commits a one-line `mod` declaration
into `mod.rs` has silently taken a write on I1's file.

### 3. Everything that requires seeing both halves

- Diffing I1's migrated assertions against defect class 3 (see I1 above).
- Any collision between T1's `testfix.rs` additions and I1's needs.
- The `slither::frames` trace strings for the two new frame types, if T1
  and I1 disagree on wording.

---

## Gates

The full nine-row table from CLAUDE.md, on the exact closing commit. Not
`cargo test` alone (*"every slice ends on the full table"*), and working
rule 7: **paste the command and its output** — a gate is not green because
it ought to be.

| Gate | Command |
|------|---------|
| Compiles | `cargo build --all-features --all-targets` |
| Format | `cargo fmt --all --check` |
| Lints | `cargo clippy --all-features --all-targets -- -D warnings` |
| Docs | `cargo doc --no-deps` / `--all-features`, `RUSTDOCFLAGS=-D warnings` |
| Tests | `cargo test` **and** `cargo test --all-features` |
| Release tests | `cargo test --release --all-features` |
| Wire pins | the golden-wire and size/constant tests |
| MSRV | `cargo +1.96 check --all-features --all-targets` |
| Supply chain | `cargo deny check` |

**Two slice-specific gate notes.**

1. **The wire-pin row is a stop signal, not an expected red.** See Q16 —
   ruling 208 moves no existing byte. The one legitimate expectation change
   is A2's counter movement `(0, 196)` → `(107, 196)`, which is not a wire
   byte.
2. **The release-test row matters more than usual this slice.** F1's
   detector is a `debug_assert!`, which is **compiled out** under
   `--release`. `cargo test --release --all-features` therefore does *not*
   exercise it — that is correct and intended, but it means the debug rows
   are the ones that carry F1's whole class, and a green release row proves
   nothing about it.

**Commit before mutating** (working rule 10): mutation testing reverts with
`git checkout <file>`, which discards *any* uncommitted work in that file.
Two accessors were lost that way mid-review.
