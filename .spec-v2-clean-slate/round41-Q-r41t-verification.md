# Round 41, report Q — slice R41-T verification (mutation runners + adversarial review)

*Compiled by the integrator from the three verification agents' reports.
Stage 2: two independent mutation runners (Sonnet), blind to how the
authors built their own mutants, each in a worktree detached at the
package commit. Stage 3: one adversarial reviewer (Fable fork) over the
combined three-package tree, rule-12 lens ("what state does the clearing
argument assume") and rule-9 lens ("no assertion the collapsed
implementation satisfies for free").*

## Runner A — five mutants against `15a7bdf` (base verified `980cd13`)

Baseline: all 5 new/rewritten tests green. Final restore: green, tree
clean between every step.

| Mutant | Expected | Observed |
|---|---|---|
| `NewReno::reset` body no-op | cwnd test RED | both roam tests RED (`15600 vs 12000`; `6000 vs u64::MAX`) |
| only `ssthresh = u64::MAX` deleted | split | exact split: cwnd ok, ssthresh RED |
| shared thread-local budget (reset-on-arm semantics) | O43e test RED | RED (`Some((588,196)) vs Some((107,196))`); full suite 757/758 — **sole observer** |
| `<=` → `<` in the STREAM ceiling filter | exactly-at RED, one-past ok | exact split (code 1 vs 2) |
| `.filter` clause removed | one-past RED, exactly-at ok | exact split (code 2 vs 1) |

Fairness note on the shared-budget mutant: the runner's first
(additive-arm) attempt collateral-failed 6 single-connection tests; it
revised to reset-on-arm semantics — indistinguishable from correct
per-instance behaviour for every single-connection test — and only the
two-session test observes it. That is the fair mutant, and it is caught.

## Runner B — six mutants against `e4e3705` (base verified `980cd13`)

Baseline green; final restore green; tree clean throughout.

| Mutant | Expected | Observed | Sole observer? |
|---|---|---|---|
| `pinned()` drops `exempt_until` | O13 test RED | RED at the survived-the-flush assert | yes (755/756) |
| `evict_if_over_cap` early-return | O13 test RED | RED at "the flush did not happen" precondition | yes |
| `age_deadline` drops exemption term | O13 test RED | RED (75 s = HANDSHAKE_GIVEUP delta visible in the two instants) | no — the routing give-up test also observes (pre-existing) |
| `Proven` arm deleted | idempotency RED | RED: second call `Expired` | yes |
| `Proven` arm redoes a provider DH, still `Ok` | RED on dh-count | compiled; got past the `Ok` expect, RED at `zero incremental DH: left 1 right 0` | yes |
| M4c: `oldest_unconsumed(Some(key))` → `(None)` | per-source-cap RED | RED (`evicted the wrong number: 4 vs 3`) | no — the parked-decision TTL test also observes (pre-existing) |

The M4c row is the audit's F2 finding closed: at `980cd13` the named
test could not observe this mutant; after the two-token fixture fix it
does.

## Review (Fable fork, combined tree `15a7bdf`+`e4e3705`+`b25c75b`)

**Verdict: GO, unconditioned.** No blocking findings. Two nits:
- **N1** — the O43e behavioural half's premise (A still holds unsent
  bytes) was true by arithmetic but unasserted. Applied at integration
  as its own commit (`80dedfa`), openly, not folded into package A's.
- **N2** — ruling 268's rewrite should name an **admission-driven**
  flood. Adopted in the ruling's SPEC text.

Cleared, with each clearing argument's assumed state named in the
review: the O43e inversion (strictly stronger than the briefed
direction — funding the exhausted core makes legal pump-spend
observationally identical to wrongful shared-counter spend); the O13
record()-fill (same eviction path any flood must drive; both horizon
sides pinned; no fresh-15 s demotion window exists in the code); the
offset-ceiling observable (structural parse precedes flow checks;
codes 0x01 vs 0x02 verified); the idempotency DH counter
(`CountingProvider` counts exactly `DhProvider::dh`, the seam a
re-driven `ss` must cross; the tuple half compares against
fixture-predicted values, excluding coincidental match); the NewReno
pair (the ranged-ACK helper's loss-avoidance argument cross-validated
by the sibling test deliberately triggering the threshold it avoids);
C's harness (matching rule self-checked by the printed length
histogram; not part of `cargo test`; round41-N's numbers reconcile
with its own quoted runs). Diff hygiene: every commit inside its
partition; new tests paused-clock and seeded; real-clock code confined
to the example.
