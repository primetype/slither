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
