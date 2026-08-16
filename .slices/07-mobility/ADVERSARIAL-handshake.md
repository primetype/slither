# Adversarial review — handshake, identity, replay, staged accept ladder

**Lens:** SPEC §3.1 (mac1), §6 (staged accept ladder), §17.1 (replay/guard).
**Reviewer:** adversarial protocol reviewer (slice 7).
**Base:** working tree of `/Users/nicolasdiprima/work/primetype/slither`, `HEAD = ebc1663`
("Rulings 204-207: API surface ratified, and ruling 203's fix brief"). Not a worktree —
read the working copy directly; no other agent believed to hold the tree.
**Status:** COMPLETE. Seven findings, ranked. Three sub-areas returned nothing
and say so.

---

## 0. Method / what I read

Spec, targeted: §3.1 (543–614), §4.1–4.4 (699–771), §6.1–6.2 (1045–1186),
§6.3 (1187–1344), §6.4 (1345–1554), §6.5 (1555–1618), §6.6–6.7 (1619–1787),
§6.9 (1856–2021), §17.1 (5909–6072), §5.3 (804–820).
Code: `src/packet/{mac,handshake,header}.rs`, `src/core/endpoint/*`,
`src/shell/staged.rs`, `src/config.rs`, `src/identity.rs`, `src/lib.rs`.
Rulings: grep for mac1 / intro queue / §17.1 / guard.

**Spec ground already established (not findings, context for them):**
- §4.3 states outright that mac1 is not a secret authenticator; the cookie
  tier (`0x05`) is deferred to §19. So target 2 is *known and priced*, and
  the review's job there is whether the accounting in §6.9 is complete, not
  whether the gap exists.
- §6.9 prices the whole ladder and states a 2-DH per-packet ceiling.
- §6.1 states the claimed-static hazard explicitly and at length.

That means for targets 1–3 the interesting question is **scope**, per working
rule 8: what does the stated construction fail to bound?

---

## 1. Findings, ranked by severity

| # | Finding | Kind | Confidence |
|---|---|---|---|
| H1 | §6.9's "2 DH ceiling" is contradicted by §6.6 step 4; the tie-break admission is **4 DH** on one replayed packet, and it destroys a dial | spec-internal conflict, **needs a ruling** | high |
| H2 | `AuthError::Replay` is a third-party-forgeable accusation against a **proven** static, undocumented | missing documentation obligation | high |
| H3 | `SoftwareIdentity`'s RNG — source of every handshake ephemeral — has no OS default and no test-only warning; the endpoint RNG has both | API shape + §16.6 scope gap | high / med |
| M1 | §6.1 names three unauthenticated quantities; every warning covers one | scope narrowing (rule 8) | high / med |
| M2 | Intro-queue knobs unvalidated; `max_per_source ≥ cap` silently deletes the only occupant-shaped defence; `0` silently disables accepts | API hazard | high / med |
| L1 | Ruling 77's stated bound ("the LRU still evicts") does not reach a `Claimed`-pinned entry | rationale names an inapplicable mechanism (rules 11/12) | high |
| L2 | §17.1's honesty clause overstates the orphan flush: the code evicts one, then stalls | honesty clause too pessimistic | high |
| L3 | `Timestamp::succ` saturates silently; §17.2's scope names generations, not processes | degenerate case + scope gap | high / low impact |

**Nothing found in:** mac1 itself, the pre-AEAD gate, msg2 completion ordering,
the shell's introduction queue, staged cancel-safety. See §2.

---

### H1 — §6.9's "maximum cost of any single attacker packet is 2 DH" is contradicted by §6.6 step 4. The real figure on that path is **4 DH**, and the packet also destroys a dial. **Needs a ruling; I am not resolving it (working rule 3).**

**The conflict.** §6.9 (SPEC.md:1868) prices the replay row:

> replayed genuine msg1 reaching the tie-break path | **2 DH** (`es` + `ss`) …
> before then it survives the guard and **can cancel our pending** (§6.7's
> honesty clause), **at the same 2 DH**

and then states an absolute (SPEC.md:1888):

> **The ceiling, explicitly:** the maximum cost of any single attacker packet
> is **2 DH** — on the responder side and, via the forged-msg2 row, on the
> initiator side too

§6.6 step 4 (SPEC.md:1652-1655) prices the *only* branch that cancels a pending:

> **Admit** (the loser side) — … our own pending cancelled (§6.7), a responder
> index minted (§17.3), and msg2 written (`ee`, `se`, **+2 DH**).

Two `es`/`ss` plus `ee`/`se` is four. The two sections disagree about the cost of
the same event, and §6.9's figure is the one stated as a **ceiling**.

**The code agrees with §6.6, not §6.9.** `src/core/endpoint/routing.rs:404-414`
carries its own cost table:

```
/// | tag death (step 1) | 2 — `es`, `ss` |
/// | guard rejection (step 2) | 2 |
/// | winner-side drop (step 3) | 2 |
/// | admission (step 4) | **4** — `+ee`, `+se` |
```

and step 4's `write_msg2` is at `routing.rs:484`, immediately before the pending
is removed at `routing.rs:509`. There is no route to the cancellation that skips
the `ee`/`se`.

**The concrete sequence, and it needs no spoofing at all.**
1. Our application calls `connect(A, V)`. V's static row goes `Pending`
   (`mint_pending`), so V is a pending outbound remote from that instant
   (ruling 178/187).
2. An attacker holding **one** passively captured msg1 from V sends it **from its
   own address** — any address that is not `A`. `is_hinted(src)` is false
   (`routing.rs:187`), so it takes `park_initiation` at 0 DH and surfaces as an
   ordinary `Intro`.
3. The application — following §6.5's own *"Applications that dial SHOULD also
   drain `accept()`"* — calls `read_identity()`. That is the app's 1 DH.
4. `read_identity` (`staged.rs:245`) finds the claim **is** a pending outbound
   remote and runs `intercept_parked_intro` → `internal_tiebreak`
   (`routing.rs:387`). From here the application has chosen nothing:
   - step 1 `complete()` = `ss`, +1 DH — verifies, the bytes are genuine;
   - step 2 the guard passes **vacuously** (§17.1: for a static we only ever
     dialled *"we hold no entry at all"*);
   - step 3 `wins_tiebreak` is false whenever our static sorts **above** V's —
     half the peer population, and §6.9's row already scopes the primitive that
     way;
   - step 4 `write_msg2` = `ee` + `se`, **+2 DH**.
5. Our pending is cancelled (`routing.rs:509-519`), a session is installed
   anchored at the **attacker's** address (`anchor: src`, `routing.rs:556`), and
   per ruling 191 the application's `Connecting` resolves **`Ok(Connection)`** —
   a connection to an address the attacker chose and cannot open, which dies at
   `DEAD_TIMEOUT` (25 s).

**Cost:** 4 DH for one attacker packet, one destroyed dial, and a 25-second dead
connection handed to the application as a success. Per dial: once (step 4
consumes the pending and the static becomes LIVE, so the interception cannot fire
again for it). Repeatable across dials, and §6.7 notes a captured
`HANDSHAKE_GIVEUP` train is ≈18 distinct single-use initiations.

**A second, smaller clause of §6.9 is wrong the same way.** The same row says
*"on the staged path the same replay costs only what the application chooses to
probe."* On this route the application chooses **1** DH and gets **4**: the other
three follow automatically from `read_identity()`, and §18.1's taxonomy
deliberately hands the app `IntroError::Internal` — *"the application learns no
identity and makes no decision"* — so it does not even learn what it bought.

**What is *not* in dispute.** The primitive itself is documented precisely, in
§6.7's honesty clause: *"a captured msg1 that arrives while we hold a pending to
its sender passes `ss`, passes the guard vacuously, and — if our static is the
larger — cancels our pending and installs a session the replayer cannot
complete."* Ruling 35 bounded it by key ordering and that bound holds in the
code. **The defect is the accounting, in a section whose whole job is the
accounting, and in the one sentence of it phrased as an absolute.** Ruling 177
is the precedent for the shape and even for the section: *"§6.9's sentence is a
cost summary written loosely and is corrected."*

**Confidence: high** on the arithmetic and the reachability (both are stated by
the spec and by the code's own doc table). **Not resolved** — which text governs
is a maintainer decision.

**Files:** `SPEC.md:1868`, `SPEC.md:1888`, `SPEC.md:1652-1655`;
`src/core/endpoint/routing.rs:404-414`, `:484`, `:509`, `:556`;
`src/core/endpoint/staged.rs:245-249`.

---

### H2 — `AuthError::Replay` is a third-party-forgeable accusation delivered against a **proven** static, and nothing in the crate says so

Documentation obligation #2 (`src/lib.rs:83-87`) is scoped to the **claimed**
static: *"Denylisting on it lets an attacker claim any public key in order to get
its owner banned."* `AuthError::Replay` arrives one rung later, **after the `ss`
has succeeded** — so the static the application is holding is genuinely proven,
which is exactly what makes the variant look like trustworthy evidence.

It is not. It is fully attacker-controlled.

**Sequence.** Peer V has a live connection to us that we accepted, so §17.1 holds
a guard entry for V with `greatest = T_g`, pinned for the connection's life. An
attacker passively captured V's retransmit train — §6.7: ≈18 initiations
`T_1 < … < T_18`, all genuine and all mac1-valid because mac1 keys on **our**
public static.

The attacker replays `T_1` from any address. It parks at 0 DH; the app probes
(1 DH, claimed = V); `authenticate()` runs `complete()` (+1 DH) — **the tail tag
verifies, because the bytes are V's** — then `guard.admits(V, T_1)` is false
(`src/core/endpoint/staged.rs:491`) and the app gets `Err(AuthError::Replay)`
attributable to a static the `ss` just proved. Repeatable forever with 17 spare
initiations, at 2 DH of ours per replay (which §6.9 does price correctly).

An application that treats `Replay` as misbehaviour — the natural reading of a
variant §18.1 files among the security signals, and the natural thing to log or
rate-limit on — punishes V, who has done nothing. This is documentation
obligation #2's exact failure mode, one rung above where the obligation stops.

**The crate already reasons about this hazard for the neighbouring variant.**
`src/error.rs:154-164`, on `AuthError::Local`: routing a local fault to
`HandshakeFailed` *"does not merely misattribute a local fault to the peer, it
**reports the peer as an attacker**, and teaches an operator to distrust the one
variant that must stay trustworthy."* `Replay` sits directly above it in the same
enum with a one-line doc (`src/error.rs:142-144`) and no such note — and it is
**more** dangerous than `HandshakeFailed`, because `HandshakeFailed` leaves the
static merely claimed while `Replay` hands over a proven one.

**Fix shape (not resolved here):** a paragraph on `AuthError::Replay` stating
that the variant means *this initiation is not fresh*, never *this peer
misbehaved*, and that a replayer can produce it at will from a single captured
packet. §17.1's honesty clause already contains the reasoning; it has simply
never been carried to the error type the application actually sees.

**Confidence: high** on reachability; the sequence uses only mechanisms §17.1 and
§6.7 describe.

**Files:** `src/error.rs:141-150`, `src/lib.rs:83-87`,
`src/core/endpoint/staged.rs:486-496`.

---

### H3 — the RNG that produces **every handshake ephemeral** has no OS default and no test-only warning; the far less critical one has both

`EndpointBuilder::rng_seed` (`src/shell/endpoint.rs:357-373`) carries a
`# This is a test-only facility` heading, cites §16.6, calls a caller-chosen seed
**security-relevant**, and defaults to `getrandom::fill` when unset
(`endpoint.rs:405-409`). What it seeds is §16.6's endpoint RNG: indices, jitter,
connection sub-seeds.

`SoftwareIdentity<S, R = ChaCha20Rng>` (`src/identity.rs:151`) takes `R` as a
**mandatory constructor argument with no OS-entropy path anywhere in the crate**,
and `R` is the source of:
- the static scalar itself, in `generate()` (`identity.rs:185-194`), and
- every `open()` sub-seed, hence the `EphemeralOnly<ChaCha20Rng>` behind **every
  handshake ephemeral, initiator and responder** (`identity.rs:210-216`).

Its doc frames determinism as a **feature** — *"a seeded parent still makes the
whole sequence reproducible"* (`identity.rs:149-150`) — with no counterpart
warning, and none of `lib.rs`'s five documentation obligations covers it.

**Why the shape invites the mistake.** The type default is `R = ChaCha20Rng`. A
caller writing `SoftwareIdentity::<Suite>::generate(…)` must produce a
`ChaCha20Rng`, and the two-keystroke way to do that is `from_seed(…)`; reaching
for an OS source means changing the type parameter, which the default exists to
avoid having to do. The `R: CryptoRng` bound does not help — `ChaCha20Rng` is a
CSPRNG *given a good seed*, and the bound says nothing about the seed.
`testutil` (`src/testutil/mod.rs:976`) does exactly the seeded thing, which is
correct there and is also the example a consumer will copy.

**Impact.** With `generate()` on a predictable `R`: the static private key is
recoverable — total compromise. With `from_scalar()` and a predictable `R`: the
initiator's ephemeral private key is recoverable, so `es = DH(e_i, S_r)` is
computable from **public data alone**, which decrypts msg1's static field and its
timestamp. That directly voids §5.3's stated guarantee — *"Noise confidentiality
level 2 … opaque to any passive observer — no clock-skew fingerprinting"* — and
forward secrecy with it. (`ss` still blocks a full transcript break, so this is
identity + metadata exposure and loss of forward secrecy, not immediate session
compromise.)

**The scope defect, in working rule 8's terms.** §16.6 introduces the
security-relevance rule and bounds it to *"the endpoint core owns one seeded
RNG"*. The identity's RNG is a second seeded RNG, is more security-critical, and
falls outside every stated rule — and the implementation mirrors that scope
exactly: it defaults and warns on the one §16.6 names, and does neither on the
one it does not. §16.6's own closing line — *"One root seed reproduces the whole
system"* — is already false of this crate, which needs two independent seeds
(`testutil/mod.rs:976` and `:1263`).

**Confidence: high** on the API shape and the missing default; **medium** on how
often a real consumer trips it (an attentive one reaches for `OsRng` unprompted).

**Files:** `src/identity.rs:141-156`, `:185-194`, `:210-216`;
`src/shell/endpoint.rs:357-373`, `:405-409`; `SPEC.md:5691-5707`; `src/lib.rs:42-124`.

---

### M1 — §6.1 names **three** unauthenticated quantities; every warning in the crate covers only **one**

§6.1 (SPEC.md:1110-1113): *"Nothing durable may be keyed on the claimed static,
**the source address**, or **`sender_index`** — no map insertion, no rate-limit
bucket, no unbounded logging."* Three quantities, one rule.

Then the hazard paragraph (SPEC.md:1145-1160) discusses **only** the claimed
static. So does documentation obligation #2 (`src/lib.rs:83-87`). So does
`Claimed`'s `Debug`, which suppresses the claimed static *specifically* because
printing it *"invites exactly the logging that documentation obligation #2 warns
about"* (`src/shell/staged.rs:227-229`).

Meanwhile `Intro::source()` and `Intro::sender_index()`
(`src/shell/staged.rs:79-102`) carry careful docs about *live reads* and ruling
71, and **no warning at all** — and `Intro`'s `Debug` prints both
(`staged.rs:156-163`).

The source address is a **worse** key than the claimed static, not a better one:
it costs the attacker 0 DH rather than 1, requires no knowledge of anyone's
public key, and has no return-routability proof whatsoever at stage 0. Under
precisely the flood §6.3's honesty clause describes — many distinct source ports,
mac1-valid, 68 packets/second sustaining full occupancy — the first thing an
operator reaches for is a source-address denylist, and a spoofed source turns
that into the third-party ban obligation #2 exists to prevent.

This is working rule 8's shape exactly: a stated construction (three quantities)
whose subsequent scope silently narrows to one, in four separate places.

**Confidence: high** that the asymmetry exists; **medium** on severity — it is a
documentation gap, but it is the gap in the one document class this crate treats
as a deliverable.

**Files:** `SPEC.md:1110-1113`, `SPEC.md:1145-1160`; `src/lib.rs:83-87`;
`src/shell/staged.rs:70-102`, `:156-163`.

---

### M2 — the introduction-queue knobs are unvalidated, and one of them is the only occupant-shaped defence in the protocol

`Config::with_intro_queue_cap(usize)` and `Config::with_intro_max_per_source(usize)`
(`src/config.rs:101-113`) take bare `usize`, store it, and validate nothing. The
crate has a `ConfigError` in its taxonomy and re-exports it (`src/lib.rs:189`);
neither builder uses it.

Three reachable configurations, none of which produces an error, a trace or an
event:

1. **`with_intro_queue_cap(0)` or `with_intro_max_per_source(0)` silently
   disables all inbound accepts.** `arrive()` reaches
   `entries.len() (0) >= cap (0)`, `oldest_unconsumed` returns `None`, and every
   arrival is `Arrival::Dropped` (`src/core/endpoint/intro_queue.rs:247-271`).
   `park_initiation` drops `Dropped` on the floor (`endpoint/mod.rs:670`), so
   `IntroReady` is never emitted and `accept()` never resolves. A typo becomes a
   permanent, silent, invisible total denial of service to oneself.
2. **`with_intro_max_per_source(n)` with `n ≥ intro_queue_cap` deletes the
   per-source bound.** §6.3's honesty clause rests on it: *"filling the queue
   needs ≥ 256 distinct sources"* — that number is exactly
   `cap / max_per_source`. Raise the second knob to the first and one source IP
   can hold every slot. §6.3's own closing sentence: *"until the deferred
   cookies/mac2 round (§19), the per-source cap is the only occupant-shaped
   defence."*
3. **The spec's own text invites raising it.** §6.3: *"Distinct initiators behind
   one NAT present distinct ports, hence distinct keys; a same-4-tuple collision
   is a rebind of the same flow"* — an operator serving many clients behind one
   NAT reads that, observes them sharing a /64 cap of 4, and raises the knob. The
   motivation is legitimate; nothing tells them what they are giving up.

The right shape is the one `with_epoch_size` already uses one screen below
(`config.rs:115-134`): a doc block naming what the value is load-bearing for.
`INTRO_TTL` being deliberately non-configurable (`config.rs:54-56`) is correct
and shows the judgement was applied to that constant and not to these two.

**Confidence: high** on the behaviour (traced through `arrive`); **medium** on
severity — every case requires the operator to act.

**Files:** `src/config.rs:101-113`; `src/core/endpoint/intro_queue.rs:217-298`;
`src/core/endpoint/mod.rs:657-672`; `SPEC.md:1293-1343`.

---

### L1 — ruling 77's stated fallback bound names a mechanism that does not reach the state it describes (working rule 11)

Ruling 77 narrowed a `Claimed` pin so it cannot restart §17.1's orphan clock, and
bounded the residual exposure like this (`rulings.md:1468-1470`, carried into
§17.1 at SPEC.md:6002-6004):

> The record is **retained**, not destroyed — mildly protective — and
> `TS_GUARD_ORPHAN_CAP` with admission-only recency **still evicts, so memory is
> capped.**

It does not. `evict_if_over_cap` filters `!entry.pinned(now)`
(`src/core/endpoint/guard.rs:484`), and `pinned()` is
`self.pins > 0 || exempt_until > now` (`guard.rs:165-167`). A `Claimed` pin
increments `pins` (`guard.rs:344`). `age_deadline` likewise returns `None`
outright while `pins > 0` (`guard.rs:191`). So an entry held by a merely-claimed
chain is exempt from **both** the LRU and the aging sweep — the code is correct
against §17.1's bullet (*"pinned — never evicted"*, which names a staged
mid-state), and it is ruling 77's **rationale sentence** that is inapplicable.

Reachable: an attacker sends a mac1-valid msg1 naming a public static that
already holds a guard entry; each `read_identity()` the app performs takes a
`Claimed` pin held until the chain expires at `INTRO_TTL`, and a new chain every
14 s holds it continuously.

**Direction of the error is safe** — retaining a guard entry *preserves* replay
protection, which is what the guard is for — and the memory claim is true for a
different reason than the one given: §17.5's ceiling bounds the pinned tier at
`≤ connections + pendings + mid-states`, and mid-states are bounded by
`INTRO_QUEUE_CAP`. So this is not an exploit. It is a ratified bound resting on a
mechanism that does not reach the state it describes — CLAUDE.md rule 11's exact
shape, and rule 12's too (the lemma is true; it is true of the *unpinned* tier,
which is not the state under discussion). Worth correcting in the record rather
than leaving for the next reader to rely on.

**Confidence: high** (three lines of code, all quoted).

**Files:** `src/core/endpoint/guard.rs:165-167`, `:190-199`, `:341-353`, `:480-505`;
`SPEC.md:5994-6007`; `.spec-v2-clean-slate/rulings.md:1468-1470`; `SPEC.md:6165-6180` (§17.5).

---

### L2 — §17.1's honesty clause overstates the orphan-flush attack; the code stalls it after one eviction

§17.1 (SPEC.md:6024-6027): *"eviction is attacker-triggerable on demand … ~1024
authenticate-then-drop chains flush the tier at ~2048 DH of our cost."*

Trace it against `guard.rs` with the orphan tier full at `TS_GUARD_ORPHAN_CAP`:

1. `record()` inserts the attacker's entry **unpinned** → 1025 → `evict_if_over_cap`
   (`while count > CAP`) evicts **one** legitimate orphan → 1024.
2. `authenticate()` then pins it — `pin(&key, KeyHolder)` now succeeds because
   `record` created the entry `read_identity`'s pin could not
   (`staged.rs:506-515`) → 1023 unpinned.
3. The drop reverts and unpins; the entry goes → **1023** unpinned.
4. The next chain's `record()` takes the tier to 1024, which is **not `> 1024`**
   → no eviction. Steady state: 1023.

So the flood costs one legitimate orphan, not 1024. The divergence is in the
**safe** direction and needs no code change — but an honesty clause that
overstates a live attack is still wrong, and §6.9/§17.1's honesty clauses are
load-bearing documents in this project.

**Confidence: high.** **Files:** `src/core/endpoint/guard.rs:263-291`, `:480-505`;
`SPEC.md:6017-6030`.

---

### L3 — two degenerate cases in the timestamp path, both flagged rather than urged

**(a) `Timestamp::succ` saturates instead of failing.** At
`secs == u64::MAX && nanos == 999_999_999`, `succ()` returns *itself*
(`src/core/mod.rs:136-145`, `secs.saturating_add(1)`). `draw_timestamp`
(`endpoint/mod.rs:360-368`) then emits a value **equal** to
`last_init_timestamp` on every subsequent initiation, and §5.3's strictly-greater
invariant is silently violated — every peer holding a guard entry refuses us as
`Replay` for ever. Unreachable in practice (year ≈584 billion) and I am not
asking for a behaviour change; it is listed because the code converts a
representability failure into a silent invariant violation with no
`debug_assert`, and working rule 9 asks what the degenerate case does.

**(b) §17.2's scope names generations, not processes.** §17.2 (SPEC.md:6075-6077):
*"It survives across connection generations to the same peer."*
`last_init_timestamp` is a plain field initialised to `None`
(`endpoint/mod.rs:166`, `:195`), so it does **not** survive a process restart —
which §17.2 neither says nor denies. Consequence: an endpoint whose wall clock
steps backwards across a restart (NTP step, VM snapshot restore, an RTC that
resets on battery loss) emits initiations *below* what its peers recorded, and
every peer holding a guard entry for it answers `AuthError::Replay` until that
entry ages out. Self-healing and bounded — the peer's connection dies at
`DEAD_TIMEOUT` (25 s), the entry then ages at `TS_GUARD_ORPHAN_TTL` (15 s), or
90 s more if a tie-break exemption is armed — so ≤ ~40 s typically and ≤ ~115 s
worst case. Worth one sentence in §17.2 because the scope statement is exactly
the shape working rule 8 warns about.

**Confidence: high** on both mechanisms; **low** on operational impact.

**Files:** `src/core/mod.rs:126-145`; `src/core/endpoint/mod.rs:160-168`, `:350-368`;
`SPEC.md:6073-6077`.

---

## 2. Sub-areas with nothing to report

Stated explicitly rather than padded.

- **mac1 itself (`src/packet/mac.rs`) — nothing.** It matches §4.1 line for line:
  plain `Blake2b::<256>` for the key, plain `Blake2b::<128>::new_keyed` for the
  tag, length-check before compare, and `const _: () = assert!(MAC1_LEN * 8 ==
  128)` tying the const-generic literal to the constant. The variable-time
  comparison is correct and correctly justified (the key is derived from public
  data). Target 2 asked me to "attack the gap" left by the absent cookie tier:
  the gap is real, but §4.3 states it outright, §3.1's ruling-65 exact-length
  gate closes the padding malleability it would otherwise enable, and §6.9 prices
  every consequence. I found nothing to add beyond H1's accounting defect.
- **The pre-AEAD gate and packet classification — nothing.**
  `handle_datagram` (`endpoint/mod.rs:600-654`) runs `classify` → `our_mac1.verify`
  → `route_initiation`, in that order, with 0 DH before the hint check.
- **msg2 completion ordering — nothing.** `complete_initiation`
  (`endpoint/mod.rs:688-767`) sets `attempt_spent` strictly **after** the mac1
  check, so §5.5's *"a guessed-index or mac1-invalid msg2 can never spend
  anything"* holds as written.
- **The shell's `IntroReady` queue — nothing.** I checked it for the unbounded
  shell queue §10.6 forbids; `prune_ready` (`src/shell/driver.rs:374-381`)
  retains only ids still present in the core, so `ready` is bounded by
  `INTRO_QUEUE_CAP`, and `prune_waiting` is its dual.
- **The staged typestate's cancel-safety and reject-on-drop — nothing.** Every
  stage keeps its object alive across the await and marks `consumed` only after
  the reply lands (`src/shell/staged.rs:126-145`, `:195-214`, `:275-288`), so a
  cancelled future frees the slot instead of orphaning it. `Claimed`'s `Debug`
  deliberately hides the claimed static.
- **`Claimed::claimed_static()` naming — nothing to add.** §6.2 ratifies the name
  (*"never `remote_static()`"*) and the code honours it. There is no *type-level*
  separation — `claimed_static()` and `peer_static()` both return
  `&PublicKeyOf<I>`, so a helper taking `&PublicKey` accepts either without
  complaint — but the spec ratifies the name and is silent on the type, and
  `authenticate()` consuming `self` already forces an explicit `.clone()` before
  a claimed static can outlive its stage. I do not think a newtype is worth
  proposing; recorded so the next reviewer knows it was considered.

---

---

## 3. Raw notes

### `src/packet/mac.rs` — clean
Matches §4.1 exactly. `Blake2b::<256>` keyed derive, `Blake2b::<128>::new_keyed`
tag, `const _: () = assert!(MAC1_LEN * 8 == 128)` ties the literal to the
constant. `verify` folds the whole difference (variable-time by design, and
correctly justified — the key is public). Length check `candidate.len() !=
MAC1_LEN` first. No finding.

### `src/core/endpoint/intro_queue.rs` — arrival order
`arrive()` = dedup → per-source cap → global cap → park. Step 3's "unreachable
when step 2 evicted" holds (`len ≤ cap` invariant ⇒ after an eviction
`len ≤ cap−1`). `evicted` is a single `Option`, so a double eviction would
silently drop one entry's guard-undo; it is genuinely unreachable.
Evict-oldest reads `age_key()` = `refreshed_at`, one clock, ruling 69 honoured.
`consume()` removes the `by_addr` row and leaves `per_source` — net-zero, by
construction.

### `src/config.rs`
`with_intro_queue_cap` / `with_intro_max_per_source` take a bare `usize` with
**no validation whatsoever** — see finding on config knobs below.

### `src/core/endpoint/guard.rs` — the LRU flush is weaker than §17.1 claims
`evict_if_over_cap` runs `while unpinned_count > TS_GUARD_ORPHAN_CAP`, and is
called only from `record()`. Trace the documented
authenticate-then-drop flush against the code with the tier full at 1024:

1. `record()` inserts the attacker's entry unpinned → 1025 → evicts **one**
   legitimate orphan → 1024.
2. `authenticate()` then pins it (`pin(&key, KeyHolder)` now succeeds, because
   `record` created the entry `read_identity`'s `pin` could not) → 1023 unpinned.
3. The drop reverts + unpins → entry removed → 1023 unpinned.
4. The **next** chain's `record()` takes the tier to 1024, which is **not
   `> 1024`** → no eviction. Steady state: 1023.

So the flood evicts exactly **one** orphan and then stalls, not the ~1024 that
§17.1's honesty clause prices ("~1024 authenticate-then-drop chains flush the
tier at ~2048 DH of our cost"). The divergence is in the **safe** direction —
the code is stronger than the spec's stated exposure — so it is not a defect to
fix, but §17.1's honesty clause overstates a live attack, and an honesty clause
that overstates is still wrong. Logged in §2 as a spec-accuracy note.

### `src/core/endpoint/routing.rs` + `staged.rs` — paths traced, no divergence found
- `handle_datagram` Init arm: `classify` (§3.1 exact-length gate) → `our_mac1.verify`
  → `route_initiation`. 0 DH before the hint check. Correct.
- `complete_initiation` (msg2): index lookup → mac1 → **then** `attempt_spent = true`.
  So §5.5's "a guessed-index or mac1-invalid msg2 can never spend anything" holds:
  the spend is strictly after mac1.
- `internal_tiebreak`: tag → guard → tie-break → admit, and every early exit
  `return`s with the pending untouched. `write_msg2` is attempted **before** the
  pending is cancelled. Matches §6.6.
- `Timestamp::decode(&[u8; MSG1_PAYLOAD_LEN])` takes a fixed-size array, so there
  is no short-payload path.
- `wins_tiebreak` = `self.our_static() < peer_static` over `[u8]`'s `Ord`. §6.7.

### Attacks I traced and found already priced in the spec (not findings)
- **mac1 is forgeable by anyone holding our public static** — §4.3 says so.
- **Queue occupancy flood** — §6.3 honesty clause; the caps and evict-oldest work
  as described, and evict-oldest genuinely favours the arrival.
- **Eager `es` is rate-ungated per spoofed hint-set source** — §6.9's rate-honesty
  note. `demote`'s `Arrival::Dropped` arm even comments that the `es` is spent for
  nothing.
- **Replayed genuine msg1 cancels our pending / re-homes a dial to a spoofed
  address** — §6.9 row 7 plus §6.7's honesty clause, bounded by key ordering
  (ruling 35). `internal_tiebreak` step 4 sets `anchor_from_msg1: true` (ruling 200),
  which arms §7.3's budget on exactly this path.
- **Captured msg1 for a dialled-only static is replayable forever** — §17.1's
  honesty clause states it in as many words ("indefinitely and repeatably").

### Candidate: §6.9's 2-DH ceiling vs. `internal_tiebreak`'s own 4-DH admit row
`src/core/endpoint/routing.rs:404-414` documents the internal tie-break's costs
as `tag death 2 / guard rejection 2 / winner-side drop 2 / **admission 4**`, and
step 4 (`write_msg2`, `routing.rs:484`) is the branch that cancels our pending.
§6.9 prices the same event at *"the same 2 DH"* and then states a ceiling:
*"the maximum cost of any single attacker packet is **2 DH**"*. Needs §6.6/§6.7
cross-check before it is a finding.

### `SoftwareIdentity`'s RNG vs. `EndpointBuilder::rng_seed` — the asymmetry
`EndpointBuilder::rng_seed` (`src/shell/endpoint.rs:357-373`) has a
"# This is a test-only facility" heading and defaults to `getrandom::fill`
(`endpoint.rs:405-409`). It seeds §16.6's endpoint RNG: indices, jitter,
sub-seeds.
`SoftwareIdentity<S, R = ChaCha20Rng>` (`src/identity.rs:151`) takes `R` as a
**required constructor argument with no OS default at all**, and `R` is the
source of (a) the static scalar in `generate()` and (b) every `open()` sub-seed,
hence **every handshake ephemeral on both roles**. Its doc frames
reproducibility as a feature: *"a seeded parent still makes the whole sequence
reproducible."* No test-only warning, and none of `lib.rs`'s five documentation
obligations covers it.
