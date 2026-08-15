# Slice 4a — implementation report (IMPL-S)

Status: IN PROGRESS (skeleton written before first Read, per working rule 2).

## 1. What I built, per module

_(pending)_

## 2. Contract / ruling points that were hard to satisfy

_(pending)_

## 3. Conflicts found and NOT resolved

_(pending)_

## 4. Things I believe are wrong

_(pending)_

## 5. Deliberately not built — the seam I left

_(pending)_

## 6. Decisions forced on me that the contract did not cover

_(pending)_

## 7. Gate output

_(pending)_

## Reading log (append-only, as I go)

- Read `CONTRACT-4a.md` (232 lines) in full. Binding API surface noted:
  `StreamId`/`Dir` public, `Opener`/`Space`/`StreamRef`/`StreamsExhausted`
  `pub(crate)`; nine `Connection` verbs; six new `ConnEvent` variants keyed
  by `StreamRef`.
- Read `rulings.md` Round 17 (lines 2044–2468), rulings 93–107 in full.
  Key deltas from the plan: ruling 98 (seal table — §7.4 governs; RESET_STREAM
  is quiet, STREAM retransmission is quiet), ruling 93 (true-up = highest
  stream-level limit ever advertised).
- Read `PLAN.md` §2–§5 (237–725), §6 (726–796), §8 (861–1290), §9 (1291–1527),
  §12 (1783–1935).
- Read the existing tree: `src/core/connection/{mod,frame}.rs` in full,
  `session.rs` seal surface, `src/core/mod.rs` (Role/Install/Transmit),
  `src/constants.rs` §8.3/§8.4/§10 blocks, `src/error.rs`
  `WriteError`/`ReadError`, `src/lib.rs` re-export block.
- **Contract amendment received mid-work** (§4a, ruling 93's two tombstone
  mechanisms). Re-read `CONTRACT-4a.md` lines 170–254. Design updated before
  any code was written: `recv_abandoned` per-half tombstone **plus** watermark
  advance only where the abandonment fully closes the stream.

