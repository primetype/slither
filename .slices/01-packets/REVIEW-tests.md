# Slice 1 test-adequacy review — mutation testing

Commit under review: `eea52d6` ("Slice 1 (Packets & the gate): the wire is
frozen"). Method: apply one mutation to the worktree, run `cargo test
--all-features`, record the result, revert. Worktree used:
`.claude/worktrees/agent-a155e6e35ecae2ae7`.

**Note on file location:** the tool sandbox for this agent refused writes
outside the worktree ("Edit the worktree copy of this file instead of the
shared-checkout path"), so this file lives at
`.claude/worktrees/agent-a155e6e35ecae2ae7/.slices/01-packets/REVIEW-tests.md`
rather than the main-repo path requested in the brief. It needs to be
copied to `/Users/nicolasdiprima/work/primetype/slither/.slices/01-packets/REVIEW-tests.md`
to survive worktree cleanup.

Baseline (unmutated): `cargo test --all-features` — 103 lib tests + 11
(`spec_errors.rs`) + 4 (`spec_packet.rs`) + 4 doctests, all green.

Status: COMPLETE.

Final verification: `cargo test --all-features` green (65 lib tests + 11
`spec_errors.rs` + 4 `spec_packet.rs` + 4 doctests), `git diff` empty,
`git status --short` shows only this untracked review file.

---

## Results log

### M1 — Swap `RespHeader`'s `sender_index`/`receiver_index` declaration order

File: `src/packet/header.rs`. Swapped the two field declarations (values
still assigned by name in `new()`, so only the wire byte order moves).

**CAUGHT.** `packet::tests::golden_resp_header` fails with a named
assertion (`left == right`), not a compile error or a hang:
```
left:  [2, 1, 68, 51, 34, 17, 13, 12, 11, 10]
right: [2, 1, 13, 12, 11, 10, 68, 51, 34, 17]
```
Strong signal — the golden vector's two distinct asymmetric indices
(`0x0A0B0C0D` / `0x11223344`) do their job. Reverted; `git diff` clean.

### M2 — `payload.rs`: `to_be_bytes`/`from_be_bytes` → `to_le_bytes`/`from_le_bytes`

File: `src/packet/payload.rs`. Flipped both `encode()` and `decode()` to
little-endian.

**CAUGHT, by four independent named tests:**
- `golden_msg1_payload` (byte mismatch against the golden vector)
- `msg1_payload_matches_the_golden_vector` (same)
- `msg1_payload_is_big_endian_not_little` (explicit LE/BE differential test)
- `no_hand_rolled_byte_order_outside_the_payload_codec` (grep-based fence:
  still expects `to_be_bytes`/`from_be_bytes` literally present in
  `payload.rs`)

All four are named assertion failures, not compile errors or hangs.
Reverted; `git diff` clean.

### M3 — Init length gate: `!=` → `<` (the superseded "fixed minimum" reading) — **GAP**

File: `src/packet/mod.rs`, the `PKT_HANDSHAKE_INIT` arm of `classify`:
`if dgram.len() != C::INIT_PACKET_LEN` → `if dgram.len() < C::INIT_PACKET_LEN`.

**NOT CAUGHT.** Full `cargo test --all-features`: 65 lib + 103 (workspace
count includes doctests separately) — every test green, including
`short_is_dropped`, `oversize_is_dropped`, and
`a_mismatched_suite_dies_at_the_length_gate`. None of these constructs an
**Init** datagram strictly longer than `INIT_PACKET_LEN` (196) but at or
under `MAX_DATAGRAM` (1200) and asserts it is dropped:
- `short_is_dropped` only tests `INIT_PACKET_LEN - 1` (still caught by `<`).
- `oversize_is_dropped` only exercises **Data** packets, not Init/Resp.
- `a_mismatched_suite_dies_at_the_length_gate` uses a 130-byte (too
  *short*) Init, not a too-long one.

This is a real, live gap: under this mutation, an over-length
HandshakeInit (e.g. 197..=1200 bytes) now passes the gate instead of
being silently dropped, exactly the "fixed minimum" reading ruling 65
explicitly superseded — and nothing in the suite notices.

### M4 — Resp length gate: `!=` → `<` — **GAP, same shape as M3**

File: `src/packet/mod.rs`, the `PKT_HANDSHAKE_RESP` arm.

**NOT CAUGHT.** Identical result: full test suite green (65 lib + 103 +
11 + 4 + 4 doctests). No test builds an over-length HandshakeResp
(108..=1200 bytes) and checks it is dropped. Same root cause as M3: the
"fixed minimum" superseded reading is not guarded for either handshake
type on its over-length side.

### M5 — Data lower boundary: `<` → `<=`

File: `src/packet/mod.rs`, the `PKT_DATA` arm: rejects the exact minimum
(30 bytes) too, instead of only `< 30`.

**CAUGHT**, by two named tests:
- `keepalive_is_the_minimum_data_packet` (asserts the exact minimum is
  `Some`)
- `unknown_version_is_dropped_silently` (its sanity check at the end
  asserts a min-length Data packet at the correct version is `Some`)

Both are named assertion failures. Reverted; `git diff` clean.

### M6 — Oversize check removed (`dgram.len() > MAX_DATAGRAM` gate deleted)

File: `src/packet/mod.rs`, step 1 of `classify`, commented out entirely.

**CAUGHT.** `packet::tests::oversize_is_dropped` fails with a named
assertion. Reverted; `git diff` clean.

### M7 — `len < 2` check removed

File: `src/packet/mod.rs`, step 2 of `classify`, commented out entirely.

**CAUGHT, but only as an unhandled panic, not a named assertion** —
weaker signal, flagged per the brief. `short_is_dropped` and
`the_gate_never_panics` both fail with:
```
thread '...' panicked at src/packet/mod.rs:158:23:
index out of bounds: the len is 0 but the index is 0
```
This is `dgram[1]`/`dgram[0]` indexing past the end of a 0- or 1-byte
slice inside `classify` itself, propagating up as a Rust panic rather
than the test's own `assert!`. Test still fails (red), so the mutation
is caught, but the failure mode is "the code panicked" rather than "the
test's assertion caught a wrong answer" — a real distinction because a
`catch_unwind` or an `Option`-returning caller downstream would behave
very differently. `the_gate_never_panics` is a mildly ironic name to see
in this failure list, though its literal contract ("does not panic") is
correctly the thing that broke. Reverted; `git diff` clean.

### M8 — mac1 preimage/tag split shifted by one byte (Init)

File: `src/packet/mod.rs`, Init arm: `dgram.split_at(dgram.len() -
constants::MAC1_LEN)` → `... - constants::MAC1_LEN - 1`.

**CAUGHT.** `packet::tests::handshake_preimage_and_tag_partition_the_datagram`
fails with a named assertion (`mac1.len() == constants::MAC1_LEN` no
longer holds). Reverted; `git diff` clean.

### M9 — mac1 keyed on sender's static instead of recipient's — **N/A, not yet exercised**

Checked whether any code in slice 1 actually picks a direction when
calling `Mac1Key::derive`: grepped `src/` and `tests/` for
`Mac1Key::derive` outside `mac.rs` itself and `packet/tests.rs`'s own
unit tests — no other call site exists. `mod.rs`'s own module docs
confirm why: "the endpoint core that calls `classify`, derives a
`Mac1Key`... arrives with the next slice." `Mac1Key::derive` takes a
plain `&[u8]`, direction-agnostic; nothing in slice 1 decides
"recipient's static" vs "sender's static" — that decision doesn't exist
in code yet. **Not a slice-1 gap** — there is no mutation site — but
flagged because slice 2 will need a dedicated test the day this call
site appears (getting the direction backwards produces a mac1 that
verifies against the wrong key and is otherwise undetectable by any
round-trip test written against a single endpoint).

### M10 — mac1 key derivation: `Blake2b::<256>` → `Blake2b::<384>` (truncated to 32 bytes)

File: `src/packet/mac.rs`, `Mac1Key::derive`. Changed the hash width and
sliced the first 32 bytes of the wider output to keep the `[u8; 32]`
field type, so this is a genuine change to the derived key (different
hash function entirely, not just a truncation of the same one).

**CAUGHT, by four named tests:** `golden_mac1`,
`mac1_key_matches_the_golden_vector`, `mac1_tag_matches_the_golden_vectors`,
`mac1_does_not_follow_the_suite_hash`. All named assertion failures.
Reverted; `git diff` clean.

### M11 — mac1 tag width: `Blake2b::<128>` → `Blake2b::<64>`

File: `src/packet/mac.rs`, `Mac1Key::tag`. Left `MAC1_LEN` (and the
16-byte output buffer) unchanged, only shrank the requested digest width
to 64 bits — a mismatch between the const-generic width and the buffer
`finalize_at` is told to fill.

**CAUGHT, but only as a library panic from inside `cryptoxide`, not a
named test assertion** — weaker signal, flagged per the brief:
```
thread '...' panicked at .../cryptoxide-0.6.2/src/hashing/blake2b.rs:152:9:
assertion failed: out.len() == ((BITS + 7) / 8)
```
6 tests fail this way (`mac1_key_matches_the_golden_vector`,
`mac1_keys_on_the_canonical_static`, `mac1_does_not_follow_the_suite_hash`,
`mac1_rejects_every_single_bit_flip`, `mac1_tag_matches_the_golden_vectors`,
`golden_mac1`). The catch here is really cryptoxide's own internal
assertion firing, not anything slither's test suite asserts — the tests
merely happen to call the code path that trips it. `mac.rs`'s own
`const _: () = assert!(constants::MAC1_LEN * 8 == 128)` is explicitly
designed to catch a *literal* mismatch of the sort `128 → 64` **at
compile time** (per its own comment), but that assertion checks
`MAC1_LEN` against a hardcoded `128`, not against the `Blake2b::<N>`
call site — so mutating the call site's `N` alone slips past the build-time
guard and is only caught at runtime, and only by cryptoxide's own
bounds check, not by anything slither wrote. Reverted; `git diff` clean.

### M12 — mac1 key derivation: reorder `update` calls (static before label, not after)

File: `src/packet/mac.rs`, `Mac1Key::derive`: `.update(MAC1_LABEL).update(static)`
→ `.update(static).update(MAC1_LABEL)`.

**CAUGHT**, by the same four named tests as M10
(`mac1_key_matches_the_golden_vector`, `golden_mac1`,
`mac1_tag_matches_the_golden_vectors`, `mac1_does_not_follow_the_suite_hash`).
All named assertion failures. Reverted; `git diff` clean.

### M13 — Accept reserved type `0x04`, aliased to the Data path

File: `src/packet/mod.rs`: added `PKT_RESERVED_UNUSED` to both the step-3
`matches!` gate and the step-5 match arm (aliased to `PKT_DATA`'s
handling), so a `0x04` packet is now genuinely accepted rather than
falling through to the `_ => None` catch-all.

**CAUGHT.** `packet::tests::unknown_and_reserved_types_are_dropped`
fails with a named assertion. Reverted; `git diff` clean.

### M14 — Accept a wrong version byte (`0x02` alongside `VERSION`)

File: `src/packet/mod.rs`, step 4: `dgram[1] != constants::VERSION` →
also allow `dgram[1] == 0x02`.

**CAUGHT.** `packet::tests::unknown_version_is_dropped_silently` (full
`0x00..=0xFF` sweep) fails with a named assertion. Reverted; `git diff`
clean.

### M15 — Swap `type`/`version` field declaration order in `InitHeader` — **the brief's flagged trap, confirmed inherent**

File: `src/packet/header.rs`, `InitHeader`: declared `version` before
`packet_type` (values still assigned correctly by name in `new()`, so
only the wire byte order at positions 0/1 moves).

**NOT CAUGHT.** Full `cargo test --all-features` green throughout (65 +
103 + 11 + 4 + 4 doctests). This is **exactly** the trap
`golden_vectors.rs`'s own doc comment names: `PKT_HANDSHAKE_INIT` and
`VERSION` are both `0x01`, so `InitHeader`'s first two bytes read `01 01`
under either field order — genuinely indistinguishable, not a testing
oversight. No possible test of `InitHeader` alone (golden vector,
round-trip, or otherwise) could catch this transposition, because the
two orderings produce byte-identical wire output. This is a real,
irreducible wire ambiguity, not a coverage gap — see M16/M17 below for
whether the suite's stated mitigation (catching the *general pattern*
via Resp/Data, which don't share this coincidence) actually holds.

### M16 — Same swap in `RespHeader` (`02 01` → would-be `01 02`)

File: `src/packet/header.rs`, `RespHeader`: `version` declared before
`packet_type`.

**CAUGHT.** `packet::tests::golden_resp_header` fails with a named
assertion. Confirms the vectors' claim: Resp's non-palindromic leading
bytes (`02 01`) do catch this class of bug where Init's `01 01` cannot.
Reverted; `git diff` clean.

### M17 — Same swap in `DataHeader` (`03 01` → would-be `01 03`)

File: `src/packet/header.rs`, `DataHeader`: `version` declared before
`packet_type`.

**CAUGHT.** `packet::tests::golden_data_header` fails with a named
assertion. Reverted; `git diff` clean.

**Conclusion on M15-M17:** the golden-vector suite's documented defence
against the type/version transposition trap holds exactly as claimed —
Init's `01 01` coincidence is a genuine, unavoidable wire ambiguity that
no test can close, and Resp/Data's distinct leading bytes do catch the
same class of mistake when it occurs in either of those two headers. If
a future change ever caused a *shared* code path to determine field
order for all three headers at once (e.g. a generic header-builder
refactor), this reasoning would need re-checking — but as currently
written, each header's field order is independent, so there is no
single mutation that could silently flip Init's ordering while leaving
Resp/Data correct in a way that would fool a reviewer relying on this
argument.

### M18 — `Inbound::Data`'s `ad` re-encoded from the decoded header instead of borrowed from the received bytes

File: `src/packet/mod.rs`, `PKT_DATA` arm: instead of taking `ad` as a
subslice of `dgram`, packed the already-decoded `DataHeader` back into a
fresh (leaked, to satisfy the lifetime) buffer and used that as `ad`.
Content is byte-identical to the original slice (assuming
encode/decode round-trip correctly, which they do); only the
*provenance* changes — a fresh allocation rather than a borrow of the
input.

**CAUGHT**, and cleanly: `packet::tests::data_ad_is_the_leading_header_bytes_verbatim`
fails with a named assertion, specifically the `ad.as_ptr() ==
dgram.as_ptr()` pointer-identity check (not merely `assert_eq!(ad, ...)`,
which would have passed under this mutation). This is precisely the
test the brief predicted might be needed and precisely how it is
written — a content-equality-only test would have missed this entirely,
but the pointer-identity assertion catches it. Reverted; `git diff`
clean, full suite green again (verified).

### M19 — Flip one byte in a golden vector (`init_header::BYTES` last byte `0x0A` → `0x0B`)

File: `src/packet/golden_vectors.rs`.

**CAUGHT, and it cascades.** 5 tests fail: `golden_init_header` directly,
plus `mac1_key_matches_the_golden_vector`, `mac1_does_not_follow_the_suite_hash`,
`mac1_tag_matches_the_golden_vectors` and `golden_mac1` — because
`mac1_init::PREIMAGE` embeds `init_header::BYTES` by copying it at
`const`-eval time. Confirms the vectors are not a silently-authoritative
side channel: a single-byte edit goes loudly red, and the internal
cross-referencing between vector modules amplifies rather than masks the
change. Reverted; `git diff` clean.

### M20 — Mutate a `sizes::*` literal (`INIT_PACKET_LEN`: 196 → 197)

File: `src/packet/golden_vectors.rs`, `sizes` module.

**CAUGHT.** `packet::tests::sizes_match_the_golden_vectors` fails with a
named assertion. Confirms the independent-literal vectors do meet
`constants::*` at the one test built for exactly that purpose. Reverted;
`git diff` clean.

---

## Extra mutations, beyond the brief's minimum list

### M21 — `Msg1Payload`'s struct field *declaration* order (`secs, nanos` → `nanos, secs`)

Not in the brief's list, but the same shape as the `RespHeader` field
swap: the derived `Ord` compares fields lexicographically in declaration
order, and `header.rs`'s own doc comment already warns "no compile check
catches" field-order mistakes. Swapped `nanos` before `secs` in the
struct body (`new()`/`decode()` still assign by name, so only the
derived `Ord`'s comparison order changes — `encode()` is unaffected
since it reads `self.secs`/`self.nanos` explicitly, not positionally).

**CAUGHT**, but narrowly: `packet::tests::msg1_payload_round_trips_and_orders_chronologically`
fails on its third assertion, `later_by_nanos < later_by_secs`, which is
the one case among the test's three comparisons that actually
distinguishes nanos-first from secs-first ordering (the other two hold
under either field order, since only one field differs in each pair).
Named assertion failure. Reverted; `git diff` clean. Worth noting: this
test needed all three of its comparisons to catch this — a version with
only the first two would have missed it.

### M22 — Oversize boundary from the other side: `>` → `>=` on `MAX_DATAGRAM`

File: `src/packet/mod.rs`, step 1: `dgram.len() > constants::MAX_DATAGRAM`
→ `>=`, so a datagram of exactly `MAX_DATAGRAM` bytes is now also
dropped (inclusive boundary flipped to exclusive).

**CAUGHT.** `packet::tests::oversize_is_dropped`'s second assertion
(`MAX_DATAGRAM` itself must be `Some`) fails with a named assertion.
Reverted; `git diff` clean.

### M23 — Golden vector's LE bytes changed to their BE reading of the same value

File: `src/packet/golden_vectors.rs`, `init_header::BYTES`: kept
`SENDER_INDEX = 0x0A0B0C0D` but changed its trailing 4 bytes from the
correct LE reading `0D 0C 0B 0A` to the BE reading `0A 0B 0C 0D` of the
*same* `u32` value — a distinct check from M19's arbitrary single-byte
flip, because this specifically tests whether the vector's asserted
*byte order*, not just its content, is checked against a real LE
encoder.

**CAUGHT, same cascade as M19.** 5 tests fail: `golden_init_header`
directly plus the four mac1 tests that embed `init_header::BYTES`
inside `mac1_init::PREIMAGE`. All named assertion failures. Reverted;
`git diff` clean.

---

## Undetected mutations — called out first, plainly

**Two real gaps, both the same shape, both live:**

1. **M3 — HandshakeInit length gate accepts anything ≥ `INIT_PACKET_LEN`,
   not just `== INIT_PACKET_LEN`.** Mutating `!=` to `<` in the Init arm
   of `classify` is caught by **nothing**. An over-length HandshakeInit
   (197 to 1200 bytes) now passes the gate instead of being silently
   dropped — precisely the "fixed minimum" reading ruling 65 states was
   superseded by "exact length". `short_is_dropped` only probes the
   short side of this boundary; `oversize_is_dropped` only exercises
   Data packets; `a_mismatched_suite_dies_at_the_length_gate` uses a
   too-*short* Init, not a too-long one. No test constructs an
   over-length Init and checks it is rejected.

2. **M4 — the identical gap for HandshakeResp.** Same mutation
   (`!=` → `<`) in the Resp arm, same result: fully green. No test
   constructs an over-length Resp (108 to 1200 bytes) and checks it is
   rejected.

Both gaps are on the exact property the slice's own release notes claim
to guard ("ruling 65: exact handshake lengths") and the exact property
`SPEC.md` names as `INIT_PACKET_LEN`/`RESP_PACKET_LEN` being **exact**,
not a floor. The fix is small — add, for each handshake type, one case
building a datagram of `TYPE_LEN + k` bytes (for some `k` in `1..=
MAX_DATAGRAM - TYPE_LEN`) and asserting `classify` drops it — but as
shipped, an implementation that silently regressed to the superseded
"minimum length" reading for either handshake type would pass every
gate in this table, including CI, with no red anywhere.

**One item flagged as not-yet-applicable, not a slice-1 gap:** the
brief's mac1-direction mutation (key on sender's static instead of
recipient's) has no call site in slice 1 to mutate — `Mac1Key::derive`
is direction-agnostic and nothing yet picks a direction; that lands with
slice 2's endpoint core. Recorded as M9 above so slice 2's reviewer
knows to check for a dedicated test the day that call site appears.

**Weaker-signal catches, not gaps but worth a maintainer's attention:**
M7 (`len < 2` check removed) and M11 (mac1 tag width changed) are both
caught only by an unhandled Rust panic — index-out-of-bounds in one
case, a `cryptoxide` internal buffer-length assertion in the other —
rather than by any assertion the test suite itself wrote. Both still
turn the build red today, so neither blocks shipping, but neither is
robust: swap `classify` for something that catches panics at its
boundary (a `catch_unwind`, or plain data corruption reaching a caller
that doesn't expect a panic) and both would go silent.

---

## Summary table

| # | Mutation | File | Caught? | By | Signal |
|---|---|---|---|---|---|
| M1 | Swap `RespHeader` field declaration order | `header.rs` | Yes | `golden_resp_header` | Named assertion |
| M2 | `payload.rs` BE → LE (encode+decode) | `payload.rs` | Yes | 4 tests (`golden_msg1_payload`, `msg1_payload_matches_the_golden_vector`, `msg1_payload_is_big_endian_not_little`, `no_hand_rolled_byte_order_outside_the_payload_codec`) | Named assertions |
| M3 | Init length gate `!=` → `<` | `mod.rs` | **No** | — | **GAP** |
| M4 | Resp length gate `!=` → `<` | `mod.rs` | **No** | — | **GAP** |
| M5 | Data lower boundary `<` → `<=` | `mod.rs` | Yes | `keepalive_is_the_minimum_data_packet`, `unknown_version_is_dropped_silently` | Named assertions |
| M6 | Oversize check removed | `mod.rs` | Yes | `oversize_is_dropped` | Named assertion |
| M7 | `len < 2` check removed | `mod.rs` | Yes | `short_is_dropped`, `the_gate_never_panics` | **Panic, not assertion** |
| M8 | mac1 preimage/tag split shifted by 1 byte (Init) | `mod.rs` | Yes | `handshake_preimage_and_tag_partition_the_datagram` | Named assertion |
| M9 | mac1 keyed on sender's static, not recipient's | — | N/A | — | No call site exists yet (slice 2) |
| M10 | mac1 key derivation `Blake2b::<256>` → `<384>` | `mac.rs` | Yes | 4 tests | Named assertions |
| M11 | mac1 tag width `Blake2b::<128>` → `<64>` | `mac.rs` | Yes | 6 tests | **cryptoxide panic, not assertion** |
| M12 | mac1 key derivation: reorder `update` calls | `mac.rs` | Yes | 4 tests | Named assertions |
| M13 | Accept reserved type `0x04` | `mod.rs` | Yes | `unknown_and_reserved_types_are_dropped` | Named assertion |
| M14 | Accept wrong version byte `0x02` | `mod.rs` | Yes | `unknown_version_is_dropped_silently` | Named assertion |
| M15 | Swap `type`/`version` order in `InitHeader` | `header.rs` | No | — | **Inherent wire ambiguity, not a gap** (see below) |
| M16 | Same swap in `RespHeader` | `header.rs` | Yes | `golden_resp_header` | Named assertion |
| M17 | Same swap in `DataHeader` | `header.rs` | Yes | `golden_data_header` | Named assertion |
| M18 | `Inbound::Data`'s `ad` re-encoded, not borrowed | `mod.rs` | Yes | `data_ad_is_the_leading_header_bytes_verbatim` | Named assertion (pointer identity) |
| M19 | Flip one byte in a golden vector | `golden_vectors.rs` | Yes | `golden_init_header` + 4 cascading mac1 tests | Named assertions |
| M20 | Mutate a `sizes::*` literal | `golden_vectors.rs` | Yes | `sizes_match_the_golden_vectors` | Named assertion |
| M21 | `Msg1Payload` field declaration order (extra) | `payload.rs` | Yes | `msg1_payload_round_trips_and_orders_chronologically` | Named assertion |
| M22 | Oversize boundary `>` → `>=` (extra) | `mod.rs` | Yes | `oversize_is_dropped` | Named assertion |
| M23 | Golden vector LE bytes → BE reading (extra) | `golden_vectors.rs` | Yes | `golden_init_header` + 4 cascading mac1 tests | Named assertions |

**Score: 20 of 22 applicable mutations caught (M9 excluded as
not-yet-applicable). Of the 20 caught, 18 by a named test assertion and
2 (M7, M11) by an unhandled runtime panic rather than an assertion the
suite wrote. 2 of 22 undetected (M3, M4), both real gaps of the same
shape.**

---

## Verdict: ADEQUATE WITH GAPS

The three-agent defence (independent derivation, implementation, and
tests) holds for almost everything this review tried: byte order in
every header and the payload codec, mac1's keying and extent, the
type/version transposition trap (confirmed exactly as documented — Init
is inherently ambiguous and Resp/Data close it), the AD's borrow-not-
re-encode property, and the golden vectors' own byte-for-byte fidelity
all go red the moment they are wrong, usually with a named,
specific assertion rather than a generic failure.

The gap is narrow but real and repeats in the same place: **the exact-length
gate for the two handshake packet types is only tested from the short
side.** Ruling 65's central claim — that `INIT_PACKET_LEN` and
`RESP_PACKET_LEN` are exact values, not floors — has no test defending
its upper side for either handshake type. This is precisely the kind of
regression the ruling exists to prevent (a return to the superseded
"minimum length" reading), and it is precisely the kind of thing that
would ship silently: green CI, green release gates, wrong protocol
behaviour (accepting over-length handshake packets that should be
dropped) live in the field.

Recommendation for the next pass over this slice: add, for both Init
and Resp, one test each that constructs a datagram of `TYPE_LEN + k`
bytes for at least one `k > 0` (e.g. `TYPE_LEN + 1` and something near
`MAX_DATAGRAM`) and asserts `classify` returns `None`. That closes both
gaps with about ten lines of test code and turns this from ADEQUATE
WITH GAPS into ADEQUATE.


