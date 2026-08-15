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

