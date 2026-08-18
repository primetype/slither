# Round 41 material — recorded during round 40 (2026/08/17–18)

Everything below was reported by a round-40 agent or found at
integration, deliberately not resolved. Sources: the slice integration
commits on `main` (`96f7ef0`, `5806f12`, `f6710d8`, `58e81f2`,
`22b7481`, `65af633`) and the agents' final reports. Nothing here blocks
release gates today — all nine are green at `65af633`.

## Needs a ruling or a maintainer line

1. **Spec is silent on the shipped keepalive announce-gate.** Slice 7b's
   F1 fix gates both keepalives on §7.3's budget
   (`keepalive_can_leave()`, `mod.rs`), pinned in `tests_livelock.rs`;
   no SPEC sentence states it. Ruling 249's recorded deferral — the
   documentary twin of the `Pto` gate it ratified. **[RESOLVED
   2026/08/18 — ruling 265, and the measurement found more than a
   documentary gap: the immortal-park state, constructed at core and
   shell, fixed by the death-clock backstop.]**
2. **§7.7's straggler tolerance has an unstated scope** (R40-C author,
   measured): the one-epoch-back key is reachable only within
   `REPLAY_WINDOW` (2 048) counters of a boundary — 32× tighter than the
   sentence reads at the production epoch (same-epoch packets 3 000
   counters back are refused by §7.2, not the ratchet). Rule 8's shape;
   worth a §7.7 clause. **[RESOLVED 2026/08/18 — ruling 256:
   delivery-scope clause + Appendix B companion; the verb corrected —
   the retained key opens the whole preceding epoch, §7.2 bounds
   *delivery*.]**
3. **Coalescing granularity in the [40, 49) both-owed window** (ruling
   250 implemented the amended sentence's letter: all-or-nothing over
   the owed set). Greedy is strictly better by one 9 B frame there. **[RESOLVED
   2026/08/18 — ruling 258: all-or-nothing ratified; the 9–18 B fund
   nothing below the 30 B minimum datagram.]**
4. **Where an owed pure ACK rides when the probe coalesces** (R40-B
   author, rule 8): §8.5 packs an owed ACK first *within* a packet;
   nothing says *which* packet beside a coalescing probe. **[RESOLVED
   2026/08/18 — ruling 258: behind, never aboard; §12.4 states the
   exception.]**
5. **May STREAM fill ride the cwnd-exempt probe?** §8.5 permits it in
   any packet; §14.5's exemption rationale ("at most 18 B of piggyback")
   forbids it by implication. The implementation does not do it; the
   scope is unstated. **[RESOLVED 2026/08/18 — ruling 258: no fill; the
   18 B bound is the exemption's proof; §8.5's collision sentence
   corrected.]**
6. **`tests_path.rs`'s second conflict header** (`CONTRACT-7b.md`
   `!elicits` vs §8.7's broader condition) still reads "REPORTED AND NOT
   RESOLVED"; the code follows §8.7 and its test is green. One line.
   **[RESOLVED 2026/08/18 at `ae71823` — dated resolution appended.]**

## Code defects, reported and reproducible

7. **`streams.rs:1077`** `debug_assert!(false, "stream_payload_room
   over-promised")` fires at `Pair::seeded(0xE5B0000C)`, one 2 KiB uni
   stream, `FlakyPolicy::lossy(0.5).with_delay(20 ms, 0)` (R40-A author).
   Benign in release (the arm defers via `return_chunk`), panics debug
   drivers. No existing test reaches it — rule 13's shape. **[RESOLVED
   2026/08/18 — ruling 257: one-predicate fix, deterministic
   regression, `extends_to_end` scope noted at the query.]**
8. **The pump loop's `contested.is_pending()` offer disjunct is
   unreachable** (predates ruling 250; reported in place at the R40-B
   merge). Its comment reads as a live mechanism. **[RESOLVED 2026/08/18 at
   `ae71823` — unreachable proven both halves (0 hits in >37 000
   iterations + single-writer mechanism); disjunct deleted.]**

## Smaller residue

9. `copy_work()` summed over live halves is not monotone across half
   retirement (documented at the `Streams` level; no test depends on
   it). Decide whether the connection-level meter should survive
   retirement. **[RESOLVED 2026/08/18 — ruling 263: it accumulates.]**
10. Ruling 253's "small-to-large can invert which copy survives" names a
    mechanism the landed gap-filling merge does not have (stored bytes
    always win there); the code doc states the §9.5 non-guarantee as
    ruled. Cosmetic unless someone re-reads 253 as describing the code.
    **[RESOLVED 2026/08/18 — ruling 264: dated addendum on 253.]**
11. `tests_livelock.rs`'s fixed-point bound still says `cap = 64` — much
    looser than needed post-249/254 (mechanism comment already corrected
    at `96f7ef0`). **[RESOLVED 2026/08/18 at `ae71823` —
    `FIXED_POINT_CAP = 4`, separating shape scores 25.]**
12. Testing conventions worth writing down where authors look: session
    ids are wall-clock-fed — never golden-pin one; the `Tap` records
    **before** the loss draw — a tapped datagram is not a delivered one
    (both from R40-D's author). **[RULED 2026/08/18 — ruling 264(vi):
    lands in the comment slice, `testutil` module docs.]**

## Standing from before round 40 (still open)

13. Ruling 248's eight API/doc decisions (round 39) — untouched.
    **[RESOLVED 2026/08/18 — ruling 259: all eight disposed; items
    (i)–(vii) landed at `99cdc56`/`f148140`; (viii)'s Config knob ships
    in its own slice.]**
14. The 2026-08-17 survey's doc-staleness items not consumed by round 40:
    README/CHANGELOG/SECURITY/TODO predate the rewrite;
    `core/endpoint/mod.rs:34–41` (+3 files) claim the proven-LIVE branch
    "still returns Stale" while `staged.rs:619–665` implements it;
    `recovery.rs` path_gen prose; ~20 slice-scoped "absence" comments;
    CLAUDE.md rule 13's fixture claim (send_failure exists since ruling
    49); `Identity` trait absent from SPEC.md; `.claude/worktrees/`
    leftovers (grown this round). **[RULED 2026/08/18 — ruling 264:
    top-level docs landed at `99cdc56`; in-code comments in the comment
    slice; worktree cleanup at round close.]**
15. Audit items 6–8 (coverage NONE list, hardening, the loopback p99
    anomaly) — untriaged Part A/B material. **[TRIAGED 2026/08/18 —
    rulings 260 (O53a/b), 261 (IntroError), 262 (driver arms); spin
    detector already resolved by 249(ii)+255; NAT cap by 264. Carried
    as test gaps, no ruling needed: the vacuous NewReno-reset test
    (measured: a no-op reset passes the suite), O13/guard-pins LRU
    flush, O43e budget independence, authenticate() idempotency, F2's
    one-token fix, the untested 2⁶²−1 offset bound (2.2b). The p99
    item's "0 % CPU" premise is refuted — samples were idle-window
    only; re-scoped to "split the phase-C timer" when next measured.]**
