# Slice 2a — queued for the maintainer

> Written overnight while the maintainer is away. **Nothing here blocked
> progress**: each item below has a provisional decision recorded, chosen
> to be the most defensible reading and — where it affects behaviour —
> **test-pinned**, so reversing it is an edit to a named test rather than
> an excavation. Items are ordered by how much a wrong answer costs.
>
> None of these touch a wire byte. The wire froze in slice 1 and nothing
> in slice 2a moves it.

## A. Unstated scopes found by the independent test author

All four are CLAUDE.md working rule 8's shape — *a stated construction
with an unstated scope*. This is now the eighth through eleventh instance
across three slices.

### A1. A second `read_identity()` on an already-`Claimed` chain

**The gap.** §6.1's dagger note covers *pre-read* and *frozen* entries
only. §16.4's core verb takes `&mut self` and an `IntroId` — so unlike
§6.2's `self`-consuming handle, **it can be called twice**, and the spec
does not say what happens.

Three readings, all defensible, all observably different:
1. cached, 0 DH on the second call;
2. a second `es`, 1 DH more;
3. an error.

**Cost of getting it wrong:** reading 2 breaks §6.1's 1/2/4 DH ladder,
which is a security property. Reading 3 is the only one that cannot
silently inflate DH cost.

**Provisional:** whatever the implementer chose, pinned by test. Resolve
in the morning; the ladder tests make the cost visible either way.

### A2. Does a `Malformed` `read_identity()` destroy the entry?

**The gap.** Unstated. If the entry survives, is it *consumed* —
unsupersedable, non-evictable, holding a per-source slot — or does it
revert to byte-replaceable? The second is **the identity-straddling
surface freeze-on-carry exists to close** (§6.3), so this is not a
housekeeping question.

The test author avoided depending on it (every consumed-tier test uses
real msg1 bytes). **The implementation cannot avoid choosing.**

**Provisional:** destroy the entry — a structurally unreadable msg1 is
not a chain anyone can continue, and retaining it hands an attacker a
per-source slot for the price of malformed bytes.

### A3. Ruling 70 named the constant but not the clock's origin

§17.1 ages orphans on a `TS_GUARD_ORPHAN_TTL` timer and separately
defines LRU "use" as the successful record. It never says whether the
aging interval runs **from the last successful admission** or **from the
pin release**. These differ for every connection that outlives its
admission.

My ruling 70 named the interval and did not notice it had two possible
origins — the same defect one level down, in the fix for it.

**Provisional:** from pin release. An entry pinned by a live connection
is exempt from aging (§17.1), so the aging clock cannot meaningfully
start before the exemption ends. The test collapses the two instants and
pins the *duration* without picking a side, so either answer keeps it
green.

### A4. §16.5's equal-deadline ordering list is read as exhaustive

It orders exactly one endpoint pair — give-up over retransmit.
Intro-expiry vs retransmit, and orphan-aging vs give-up, are therefore
unordered, **even though their outputs differ** (`HandshakeFailed` vs
nothing). The tests assert no ordering for those.

**Provisional:** leave unordered and untested. If the list is meant to be
exhaustive it should say so; if not, the two open pairs need an order.

## B. Missing observability the plan promised tests for

### B1. `replacement_basis` (§17.4) is written and never read

Slice 2a writes it; §16.4 exposes no reader; the plan promises two tests
for it that therefore **cannot exist**. The plan's own argument is the
case for fixing it: writing it wrong now is undetectable until slice 7,
where it will look like a slice-7 bug.

**Provisional:** add a `pub(crate)` accessor so the tests can exist.
Cheap, invisible to the public API, and it converts a latent slice-7
mystery into a slice-2a assertion.

### B2. The hint set — same shape

Populated, never consulted, no reader. Same provisional: `pub(crate)`
accessor.

## C. Things deliberately not tested, recorded so they are not mistaken for coverage

| Item | Why not | Lands in |
|---|---|---|
| Freeze-on-carry (§6.3 rule 6) | needs §6.5's eager read | slice 7 |
| `IntroError::Internal` | same | slice 7 |
| `TS_GUARD_ORPHAN_CAP` LRU eviction | needs 1024 *authenticated* statics = 1024 real P-256 handshakes | later, if ever |
| §16.6 RNG sub-seed draw order | no accessor; `connect()` mints index and draws sub-seed in one call | slice 3 |
| First-draw index collision | needs the seed→draw mapping, which the plan does not declare | — |

**Two tests pin plan derivations, not ratified rules**, and say so in
their own doc comments: `a_wholly_consumed_queue_drops_the_arrival`
(§6.3 states the *per-source* all-consumed drop and never the global one)
and the two `Retired`-as-cancel tests (§16.4 lists no cancel verb, so
S29's core-side cancellation reuses `ToEndpoint::Retired`).

Both are honest guesses that the spec does not settle. **If either is
wrong, the test is the thing that changes** — which is the point of
having written them down.

---

## D. Raised by the implementer

### D1. §18.1 has no variant for a *local* provider failure — ranked first

`Identity::open()` is fallible. `ConnectError` (2 variants) and
`IntroError` (4) cannot express "our own key hardware failed". §18.1's
taxonomy is closed (ruling 61 reserved `Stopped` on `WriteError` and
nothing else), so the implementer could not invent one.

**What it does today:** `connect`/retransmit skip the attempt and the dial
ends at the defined `TimedOut`; `read_identity` returns
`IntroError::Malformed` and leaves the chain parked. `EndpointDropped` was
considered and rejected as worse — an application would tear down its
whole accept loop over a transient enclave lock.

**Why this is the top item.** **A local hardware fault is currently
reported as if the peer's msg1 were malformed.** That is a wrong
attribution, not merely a coarse one, and it lands precisely on S21 — the
Secure Enclave story, where `open()` failing because the device is locked
or the key is biometrics-gated is an *expected* runtime condition, not a
defect. An operator debugging it is told the remote peer is sending
garbage.

It is also §18.2's shape twice over: the party that can fix the problem
(the local host) is handed evidence pointing at the peer. Ruling 49 and
ruling 59 both turned on exactly that.

**Provisional:** as implemented, because the alternative is inventing a
variant in a closed taxonomy. **Recommend** adding
`IntroError::Local`/`ConnectError::Local` (or a `slither::io`-style trace
obligation, which needs no variant and no API change) — but that is a
§18.1 amendment and yours to rule.

### D2. Orphan-aging origin — the two agents disagree, and I think the test author is right

Extends A3. The implementer ages from **last admission**; the test author
argued **pin release**. The implementer, having seen neither the other's
file nor its reasoning, wrote: *"their textual case is better — §17.1
defines orphans as dead-connection entries, so one cannot age as an orphan
before it is one."*

It did not implement pin-release because doing so needs an `orphaned_at`
set when pins hit zero, threading `now` into `unpin()`, which is adjacent
to mitigation (iii) and it declined to touch that unruled.

**The two collapse in every flow slice 2a can reach**, so no test
distinguishes them today and nothing is currently wrong. They diverge for
any connection that outlives its admission — i.e. every real one.

**Provisional:** last admission (as built). **Recommend** pin release.

### D3. `authenticate()` on a still-parked chain

The spec does not say what the core does when `authenticate()` is called
on a chain that never had `read_identity()` run. Implemented as:
**advance through the missing `es`**, because §6.1 prices the verb at 2 DH
*cumulative*, so the total is exactly the ratified cost and no error need
be invented. Reasonable; unstated. Pairs with A1.

### D4. §17.1's pin moment — plan and spec disagreed; spec won

Plan §7.2 pinned at `authenticate()`. §17.1 says pinned "while a staged
mid-state exists" and explicitly addresses the "merely claimed until
`authenticate()`" case, which puts it at `read_identity()`. Implemented at
`read_identity()`, per the spec. **No ruling needed** — recorded because
the plan is now wrong at §7.2 and should be corrected rather than left to
mislead slice 3.

### D5. §16.4's Rust block is schematic, not literal

`EndpointOutput` / `Install` / `EstablishedSession` / `Connection` must be
generic over the suite — the seal and open halves are
`DatagramSend<IK>` / `DatagramRecv<IK>`. Made `<C: Handshake>`. Forced by
the type system; recorded so §16.4's block is not read as a literal
signature by a later slice.

### D6. `SoftwareIdentity` is bound to `Curve = P256`

hiss exposes no generic private-key import seam and `PrivateKey` is not
`Clone`, so per-handshake re-import is forced, and re-import is
curve-specific. Cipher and hash stay generic. A second *curve* identity
needs its own type. Constraint from upstream, not a choice.

---

## E. Confirmed by mutation testing (28 mutations, isolated worktree)

**Verdict: ADEQUATE WITH GAPS.** Everything else — the DH ladder, both
ruling-69 eviction sites *independently*, every cap boundary in both
directions, the guard's strict-inequality admission, mitigation (i)
including the exact release-order bug, rulings 70 and 71, and the poll
contract's drain and min-deadline invariants — is caught by specific
named assertions, often from several angles.

Two gaps were found. **One is now closed**; the other is A4 and is yours.

### E1. Jitter presence and per-attempt freshness — CLOSED

Dropping the retransmit jitter entirely was undetected: the bounds test
asserted no interval *exceeds* `RETRANSMIT_BASE + RETRANSMIT_JITTER_MAX`,
which a constant-interval core satisfies for free. Closed by
`the_retransmit_jitter_is_drawn_afresh_for_every_attempt`, with two
assertions — and the second is the test author's own addition and the
better half: a core that draws the offset **once** and reuses it passes
"at least one interval exceeds base" and fails "not all intervals equal".
A single draw hoisted out of the attempt builder leaves every pending on
an identical phase, which is most of what the jitter is for.

Both mutations verified caught, each by that test alone.

### E2. The give-up / retransmit tie-break at an equal instant — **this is A4**

Reversing "give-up beats a same-instant retransmit" is undetected, and
the test file's own comment already concedes the case cannot be forced
without controlling the jitter draw. **This is not a test gap.** It is
§16.5's equal-deadline ordering list, which orders exactly one endpoint
pair and leaves intro-expiry vs retransmit and orphan-aging vs give-up
unordered — though their outputs differ (`HandshakeFailed` vs nothing).

Per working rule 8, that list reads as exhaustive. If it is meant to be,
it should say so; if not, the two open pairs need an order. Until then
no test can legitimately pin the behaviour, because the spec does not
say what it is.

## F. Verified by re-mutation, not asserted

Every fix below was confirmed by re-applying its bug and watching the
named test fail — the tree was committed first, then reverted after.

| Property | Mutation | Caught by |
|---|---|---|
| Fresh ephemeral per handshake | frozen sub-seed in `Identity::open()` | `every_retransmit_mints_a_fresh_index_and_a_fresh_ephemeral`, **alone** |
| §17.1 pin over-release | unconditional `unpin` in `drop_pending` | `cancelling_a_dial_does_not_release_a_pin_it_never_took` |
| §17.1 record survives a revert | empty `still_pinned` branch | `authenticate_then_reject_clears_the_record_even_when_another_chain_pins_the_entry` |
| Jitter present | `span = 1` | `the_retransmit_jitter_is_drawn_afresh_for_every_attempt` |
| Jitter drawn per attempt | constant non-zero jitter | same test, second assertion |

The ephemeral result is the one that matters: **before** the test author's
fix, that mutation was caught only *incidentally* by six unrelated tests
and **not** by the test named for it. A name is not a pin.
