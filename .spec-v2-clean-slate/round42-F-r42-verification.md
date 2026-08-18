# Round 42, report F — R42 slice verification (mutation runner + review)

*Compiled by the integrator. Stage 2: one Sonnet runner, blind to how
the tests were built, worktree at `069d7a0`. Stage 3: one Fable-fork
adversarial reviewer over the integrated tree.*

## Runner — four mutants at `ceiling_for` (recv.rs:693-701)

Baseline: story_reassembly 4/4, tests_reassembly_credit 6/6 green.

| Mutant | Expected | Observed |
|---|---|---|
| flat revert (ignore window) | 8/16 MiB stories RED, others green | as expected, PLUS `the_honest_hole_shape_survives_at_a_raised_window` RED — a third survival-shaped observer the catalogue under-predicted; full lib: these the only reds |
| ceiling deleted (usize::MAX) | abuse floods + floor RED, stories green | exact; full lib adds TWO pre-R42 legacy observers (recv.rs two-sided test; tests_reassembly's exactly-1024 pin) — the ceiling's existence is not pinned by the new suite alone |
| floor dropped (derived term only) | default-window floor test RED | exact — dies at 1024 where it must survive |
| off-by-one (+1 dropped) | report honestly | GREEN everywhere — by design; the tests pin the property, not the formula, as the test file itself states |

Restore verified clean; final green re-run pasted in the runner's output.

## Review — GO, three findings, none blocking

- **F1 (medium, landed `de50def`)**: nothing tied `STREAM_FILL_QUANTUM`
  to `REASSEMBLY_MIN_CONFORMING_FRAME`; a later quantum reduction would
  silently exit the theorem with nothing going red. Now a compile-time
  guard beside the quantum.
- **F2 (low, landed `de50def`)**: the residue edge (sub-P packet-tail
  frames outside the hypothesis) stated honestly in `ceiling_for`'s
  rustdoc, with its two bounding facts.
- **F3 (info)**: the bench's loss counter can print small negatives at
  window edges (dg_in > dg_out); cosmetic.

Cleared with assumed states: the proof sketch (adjacency-merge verified
at the code; chunks = maximal runs = holes+1; overlap swallowed; the
W/P+1 slack absorbs the sub-P final-frame edge); the ring (no copy
added — the old path already did `slice().to_vec()`; drain is
head-advance; the identity debug_asserted with the two discard states
excluded); the author's bounds recomputed exact, the burst window made
loud by `traversed()`; integration hygiene exact; the reviewer re-ran
`sweep-rtt` itself: 1.44/2.58/4.80/9.21/17.53, monotone, the kill cell
completing. Record inputs verified at HEAD: §10.5:4345's "the sender
cannot compute it" needed the value/standing split; six flat-as-ceiling
SPEC sites listed and swept under ruling 270.
