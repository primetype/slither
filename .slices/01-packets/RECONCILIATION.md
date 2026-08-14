# Slice 1 — reconciliation of the independent readings

> Written by the orchestrator, after both agents finished and before any
> golden vector is frozen. Its purpose is narrow: to record whether two
> readings of `SPEC.md` that never saw each other **agree byte for byte**,
> and to say plainly which bytes are covered by that agreement and which
> are not covered by anything.

## The readings being reconciled

| # | Reading | Produced by | Saw |
|---|---|---|---|
| A | `DERIVATION.md` (1 067 lines) | derivation agent | `SPEC.md` bounded ranges + ruling 64 only. **Forbidden** `src/`, `tests/`, `PLAN.md`, `.slices/`, git history |
| B | `src/packet/*` (927 lines) | implementer | `SPEC.md`, `.slices/01-packets/PLAN.md`, rulings 64–67. **Never saw A** |
| C | Python `hashlib.blake2b` | implementer, out of band | independent BLAKE2b implementation, no shared code with `cryptoxide` |

A and B were launched from separate briefs with an explicit prohibition
on reading each other. C exists because ruling 66's parameterisation
(plain, no salt, no personalisation) changes every mac1 byte and had no
second source otherwise.

## Verdict: no disagreement on any hand-derivable byte

| Item | A (derivation) | B (implementation) | Agree |
|---|---|---|---|
| LE encoding rule | `0x0A0B0C0D → 0D 0C 0B 0A` (L83) | `0x11223344 → 44 33 22 11` | ✅ same rule |
| `counter` LE | `counter=1 → 01 00 …` (L1056) | `0x0102030405060708 → 0807060504030201` | ✅ |
| `InitHeader` | `type@0 ‖ version@1 ‖ sender_index@2` | `010144332211` | ✅ |
| `RespHeader` | sender **then** receiver, `@2` and `@6` | `02014433221188776655` | ✅ |
| `DataHeader` | `receiver_index@2 ‖ counter@6`, no sender | `0301ddccbbaa0807060504030201` | ✅ |
| mac1 preimage, Init | `[0, 180)`, tag `[180, 196)` (L359) | `split_at(len − MAC1_LEN)` ⇒ `[0, 180)` | ✅ |
| mac1 preimage, Resp | `[0, 91)`, tag `[91, 107)` (L360) | ⇒ `[0, 91)` | ✅ |
| mac1 key preimage | 77 B = 12 ‖ 65 (L1061) | `update(MAC1_LABEL).update(static)` | ✅ |
| `MAC1_LABEL` | `73 6C 69 74 68 65 72 20 6D 61 63 31` (L316) | `constants::MAC1_LABEL` | ✅ |
| `PROLOGUE` | `73 6C 69 74 68 65 72 01`, 8 B (L1064) | `constants::PROLOGUE` | ✅ |
| msg1 payload | `ts_secs(8, BE) ‖ ts_nanos(4, BE)` | `0102030405060708090a0b0c` | ✅ BE |
| Packet arithmetic | `6+174+16=196`, `10+81+16=107` | `const _: () = assert!` in `header.rs` | ✅ |

**mac1's output bytes additionally agree with reading C** — Python
`hashlib.blake2b`, `digest_size=32` over the 77-byte key preimage, then
`digest_size=16, key=key` over 180-byte and 91-byte preimages, byte-identical
to `cryptoxide`. Ruling 66 is confirmed by an implementation that shares no
code with the one slither ships. It also holds *by construction*: cryptoxide's
BLAKE2b exposes no salt or personalisation parameter at all.

## What this agreement does and does not cover

**Covered — frozen on two independent readings.** Every offset, every
length, both constant bytes per header, all three LE integer encodings,
both mac1 preimage extents, the mac1 key preimage shape, `PROLOGUE`,
`MAC1_LABEL`, the msg1 payload's big-endian pair, and the Data header's
identity with the AEAD associated data. This is 6 of Init's 196 bytes,
10 of Resp's 107, and all 14 of a Data header.

**Not covered by any reading — and honestly, nothing can cover them.**
Every byte downstream of a P-256 scalar multiplication or a BLAKE2b
compression: the two ephemerals, all four Noise ciphertext/tag blocks, the
Data ciphertext and tag, and mac1's 16 output bytes. These are snapshots.
A snapshot is only a golden vector if it is reproducible, which requires
the two static scalars, both ephemeral scalars, the timestamp, both
indices, the counter and the plaintext to be pinned as **literal byte
arrays, never as an RNG seed** — P-256 keygen is rejection-sampled, so a
`rand_core` bump would silently change the vector, and `Cargo.lock` is
deliberately uncommitted (A, §8).

## Rulings this slice's independence produced

Neither agent was asked to review the spec. All four findings below came
out of agents doing their assigned job and hitting text that would not
support it.

| Ruling | Found by | Class |
|---|---|---|
| 65 — exact handshake lengths | **A and the planner, independently** | three normative statements disagreed |
| 66 — mac1's BLAKE2b is plain | A | a stated construction with an unstated parameter |
| 67 — the gate emits nothing | planner | a committed non-spec file asserting a spec obligation the spec lacks |
| 64's three self-corrections | A | a ruling's *rationale* unreviewed by ratifying its *rule* |

Ruling 65 is the load-bearing one: A and the planner reached it from
different directions, and **both refused to resolve it** under working
rule 3. Convergent refusal is the strongest signal this process has
produced.

## Open

- Reading D (the independent test author) is still running. Its tests are
  written against the plan's declared API without sight of `src/packet/`;
  a compile failure at integration is a signal, not a defect to paper over.
- One API amendment was circulated to D mid-flight rather than left to be
  discovered: `Msg1Payload::new(secs, nanos)` plus `pub(crate)` fields.
- **Gate 6 (wire pins) is not yet green and is not claimed to be.**
  `golden_vectors.rs` and `tests.rs` are stubs by design. What is pinned
  today: 103 size/constant assertions, plus compile-time assertions in
  `constants.rs`, `header.rs`, `mac.rs`, `payload.rs` and every `channel!`
  expansion. The vectors freeze after D integrates.
