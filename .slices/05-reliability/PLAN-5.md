# PLAN-5 — Slice 5: Reliability (§12 ACK, §13 loss/RTT/PTO, §14 NewReno)

Planner working notes + binding plan. HEAD at planning time: `d39ed9b`.
Stories closed: **S12** (full, over `FlakyWire` with loss), **S28**.

> STATUS: DRAFT IN PROGRESS — sections appended as they are researched.

## §0. Provenance of facts in this document

(file:line citations for everything asserted)

## §1. Cut recommendation

## §2. The binding API contract

## §3. Module map and file ownership

## §4. The algorithms, stated precisely

## §5. The sent-packet map

## §6. The five debts

### 6.1 Ruling 128 — the post-death drain
### 6.2 Ruling 113 — the FIN flag in `on_ack_range`
### 6.3 Ruling 129 — the positive control for `reset()` after `finish()`
### 6.4 Slice 4a's three watermark tests + §10.4 scope rule
### 6.5 `credit_frames_precede_the_stream_fill_in_a_packet` — slice 7, not ours

## §7. Story-to-test mapping

## §8. The unstated-scope hunt

## §9. Conflicts — reported, not resolved

## §10. Open questions, ranked by cost of a wrong answer

## §11. Risk register
