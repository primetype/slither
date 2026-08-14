# Security-fix pass notes — rulings 31 & 32 + directed fixes (2026/08/14)

Target: `SPEC-v2-DRAFT.md` (draft v4, clean at pass start).
Scope: ruling 31 (replacement basis), ruling 32 (PENDING branch at
`accept()`), SECV4-11 (delete `Install.initial` + SECV4-12 note 5),
SECV4-8 (§6.5 NAT honesty sentence), SECV4-9 (replacement-churn honesty
sentence). Wire-free: no size, header, frame type, error code, or
constant moves. §7.4/§7.5/§7.7/§1.1/§13.3 and the timer/constant tables
belong to a **later pass** and are untouched here; `DEAD_TIMEOUT` is
referenced **by name** only (its 15 s → 25 s change is that later pass).

## Checklist

- [x] **31a** — `replacement_basis: Option<Timestamp>` added to the
  connection-scoped state (§16/§17), referenced from §6.4.
- [x] **31b** — §6.4 admission amended: proven-LIVE replaces only if
  basis is `Some(t)` and candidate timestamp > t; else
  `AcceptError::Stale` (+ §18.1 `Stale` wording).
- [x] **31c** — §6.7 tie-break winner-side drop records the loser's
  timestamp; §6.6 step 4 / trailing record-free sentence and §17.1's
  "record on full admission only" phrasing amended (post-`ss`, so
  "only key-holders write guard entries" preserved).
- [x] **31d** — every false "the guard bars replays" claim rewritten:
  §6.4 bullet, §17.1 new sentence + honesty parenthetical, §5.4 LIVE
  bullet, Appendix B obligation (split into the two cases).
- [x] **31e** — §6.8 zombie/restart story updated for the
  ≤ `DEAD_TIMEOUT` delay path.
- [x] **32a** — §6.4 gains the PENDING branch (cancel the in-flight
  pending, its `Connecting` resolves `Err(ConnectError::AlreadyConnected)`,
  install fresh as responder; no tie-break).
- [x] **32b** — Appendix B obligation: `read_identity()` → `connect()` →
  `accept()` on one static ⇒ exactly one connection.
- [x] **32c** — consistency: §16.1, §5.4 responder rule, §6.6 step 2.
- [x] **D-11** — `Install.initial` deleted (§16.4 struct + bullet, §6.6
  step 4, §6.7); `ConnEvent::Established` comment loses "first".
- [x] **D-8** — §6.5 NAT simultaneous-open honesty sentence
  (documentation form).
- [x] **D-9** — replacement-churn honesty sentence (§6.4/§6.9).
- [x] **Final** — marker counts + wire-constant verification.

## Progress log

### 31a — done
- `§17.4` gains a new final paragraph **"The replacement basis."**: the
  `static → connection` map's entries carry `replacement_basis:
  Option<Timestamp>`; `Some(t)` = establishing msg1's timestamp when we
  were responder (staged accept, re-homed candidate, tie-break loser's
  admit), `None` when we dialled; written once at install, never
  updated, dies with the connection; §6.4 is the only reader; explicitly
  distinguished from the evictable endpoint-global guard (§17.1);
  12 B, inside §17.5's per-connection term.
- `§16.1`'s object table, `Endpoint` "Owns" row: added "the
  per-connection replacement basis (§17.4)" after "the timestamp guard".

### 31b + 32a (§6.4 body) — done
- `§6.4` header marker `[RATIFIED 2026/08/14]` →
  `[RATIFIED 2026/08/14, amended 2026/08/14]`.
- Third bullet retitled "**The §16.1 guard — a proven-LIVE admission
  replaces only against a newer basis.**": replacement conditioned on
  basis `Some(t)` ∧ candidate > t; otherwise `AcceptError::Stale`, live
  connection untouched. The false "the guard is what bars a stale or
  replayed candidate from ever reaching this admission" clause is
  **deleted** from this bullet.
- New bullet "**What basis and guard bar, stated honestly.**" —
  SECV4-2's restatement: guard bars ≤-greatest, basis bars
  not-strictly-newer-than-establishing; neither distinguishes a withheld
  genuine retransmit; such a candidate destroys nothing until accepted;
  a `None` basis refuses outright (the dialled-connection case).
- New bullet "**PENDING at admission.**" (ruling 32) — cancels the
  in-flight pending, `Connecting` resolves
  `Err(ConnectError::AlreadyConnected)`, install proceeds as responder,
  tie-break explicitly does not run; names the
  `read_identity()`→`connect()`→`accept()` ordering it closes and why
  §6.5's interception cannot fire there.
- "**Stale.**" bullet extended with the basis-failure case.
- `§18.1`: `AcceptError::Stale` gains the basis-failure case;
  "no `AlreadyConnected`" sentence now covers the PENDING branch;
  `ConnectError::AlreadyConnected` gains the racing-`accept()`
  cancellation as a second producer.

### 31c (+ part of 32c, part of D-11) — done
- `§6.6` step 2: guard parenthetical softened (a replayed msg1's shape
  "once the guard holds an entry for that static"); added the converse
  clause — PENDING is **not** exclusive to the tie-break path, the
  NONE-then-`connect()` ordering lands on §6.4's PENDING branch (32c).
- `§6.6` step 3: winner-side drop now records the inbound's timestamp.
- `§6.6` step 4: "(admission = check-and-record on full admission only)"
  → "**recorded** as a full admission"; `Install { initial: true }` →
  `Install`; sets the connection's basis to `Some(t)` (responder).
- `§6.6` trailing paragraph: winner-side drop is no longer "record-free"
  — it records the loser's timestamp but writes **no** basis (winner is
  the initiator ⇒ basis `None`).
- `§6.7` winner bullet: "nothing recorded" → timestamp **is** recorded;
  key-holder-write invariant explicitly preserved (post-`ss`); rationale
  = denies a later replay its vacuous guard pass; basis stays `None`.
- `§6.7` new paragraph "**What a replay can still do here, stated
  honestly.**" (SECV4-4's restatement — judgment call, see final report):
  forgery cannot cancel a pending, a replay can until the guard holds an
  entry; bounded (single-use, winner-side record, session dies at
  `DEAD_TIMEOUT` — referenced **by name**).
- `§6.7` tie-break completion: `Install { initial: true }` → `Install`.

### 31d (+ 32b, part of 32c) — done
- `§17.1` opening paragraph: admission list now names the fourth,
  non-admitting write (the tie-break winner's, §6.7); "record made only
  on full admission" → "on a full admission or on that authenticated
  winner-side drop"; the "only key-holders write guard entries"
  invariant restated and preserved. The false sentence ("a stale or
  replayed initiation can never carry a strictly-greater timestamp") is
  **replaced** by a new paragraph: guard alone is not what makes §6.4
  safe, it bars older-or-equal and passes vacuously on an unseen static;
  safety is guard **plus** basis; withheld-newer replay still surfaces
  as an unaccepted `Intro`.
- `§17.1` honesty-clause marker → `[RATIFIED 2026/08/14, amended
  2026/08/14]`; its parenthetical no longer claims "its replays still
  die at the guard" — split into older-or-equal (dies) vs
  strictly-newer-withheld (survives the guard, measured against the
  basis, refused outright where we dialled).
- `§5.4` marker → `[... the ratchet-only ruling; amended 2026/08/14.]`;
  LIVE bullet retitled "**candidate replacement**" and its false guard
  claim replaced by the two-test statement (guard + basis, neither
  distinguishing a withheld genuine retransmit); PENDING bullet gains
  the §6.4-PENDING-branch cross-reference (32c).
- Appendix B: the local-state routing obligation lost "a replayed one
  additionally dies at the guard"; **new** obligation splitting replay
  into older-or-equal (dies at the guard) and withheld-newer (surfaces,
  destroys nothing until accepted), with an explicit instruction not to
  let the test stand on the first case; **new** replacement-basis
  obligation (dialled ⇒ `None` ⇒ `Stale`; accepted ⇒ `Some(t)`;
  winner records + keeps `None`, loser installs `Some(t)`); **new**
  PENDING-branch obligation (32b).

### 31e — done
- `§6.8`: the existing paragraph's "dies at liveness within 15 s" → "dies
  at liveness" (no bare timer value left in the clause I touched; the
  later pass owns §7.5's numbers).
- New paragraph "**Which of the two shapes a restart takes depends on
  the basis**": accepted-zombie ⇒ `Some(t)` ⇒ replaces at the first
  `accept()` (restarted peer's fresh wall-clock read is strictly newer,
  §5.3, barring a backwards clock jump); dialled-zombie ⇒ `None` ⇒
  `AcceptError::Stale`, restart resolves anyway delayed by at most
  **`DEAD_TIMEOUT`** (named, not valued) as the zombie dies at liveness,
  the static drops to NONE, and the peer's ~5 s retransmit takes the
  ordinary fresh accept; delay ruled acceptable.

### 32c (§16.1) — done
- `§16.1` marker → `[RATIFIED 2026/08/14, clause flipped per the
  ratchet-only ruling; amended 2026/08/14.]`; new sentences: a staged
  chain in progress is deliberately **not** in `connect()`'s list and
  cannot be (§6.1 forbids durable keying on an unproven claim); the
  invariant is held at the other end by §6.4's PENDING branch, so
  `read_identity()`→`connect()`→`accept()` yields exactly one connection
  and no static is ever LIVE and PENDING at once.

### D-11 (SECV4-11 + SECV4-12 note 5) — done
- `§16.4` struct: `struct Install { session: EstablishedSession, initial:
  bool }` + its two-line trailing comment → `struct Install { session:
  EstablishedSession }`.
- `§16.4` explanatory bullet: `initial: true`/`initial: false` prose
  replaced — `Install` targets a `connect()`-created connection
  "**exactly once**", carries the session and nothing else, one install
  per connection with the rekey swap deleted, so no discriminator is
  carried.
- `§16.4` `ConnEvent::Established` comment: "first install" → "the
  install" (SECV4-12 note 5).
- `§16.4` marker → `[RATIFIED 2026/08/14, amended 2026/08/14]`.
- `§6.6` step 4 and `§6.7` mentions already converted under 31c.
- Swept: no `initial` field reference remains anywhere in the document.

### D-8 (SECV4-8) — done
- `§6.5`: new paragraph after the false-negative clause (before the
  ratified membership-timing-oracle paragraph, which is untouched):
  the false negative does **not** self-heal through retransmission (a
  rewritten source port sends every retransmit from that same port);
  the `read_identity()` interception is the only backstop and it is
  application-driven; two NAT-rewritten simultaneous dialers both fail
  at `HANDSHAKE_GIVEUP`; self-limiting because retries de-synchronise;
  "Applications that dial SHOULD also drain `accept()`." Documentation
  form only — the eager-`es`-on-connect mechanism variant was **not**
  taken (it needs a ruling).

### D-9 (SECV4-9) — done
- `§6.9`: "**Two** rate-honesty notes" → "**Three**"; third note added at
  the end of that paragraph — the protocol paces replacement not at all,
  a key-holder can mint strictly-greater initiations at line rate, each
  auto-accepted one costs a teardown + 4 DH + §17.5 churn; applications
  SHOULD rate-limit replacing accepts per static; blast radius confined
  by §16.1's proven-static keying (a key-holder harms only itself).
  The 20 ms-gate mechanism alternative was **not** taken.

### Follow-on consistency edits inside the same clauses
- `§6.9` DoS table, "replayed genuine msg1 reaching the tie-break path"
  row: "dies at the timestamp guard" → dies at the guard *once an entry
  exists*, before then it survives and can cancel our pending (§6.7's
  honesty clause), at the same 2 DH (the cost column is unchanged).
- `§6.9` DoS table, "genuine replacement initiation from a key-holding
  peer" row: admission is now "basis-gated and application-gated
  (§6.4, §17.4)".
- `§17.4`: basis sized as one `Timestamp` (`TIMESTAMP_LEN`, §5.2), not a
  bare byte count.

### Final verification (all green)
- `[MAINTAINER]` markers: **exactly 2** — line 156 (§1.3's mention) and
  line 3161 (Appendix A.1). None added, none removed.
- `[RATIFIED` occurrences: 29 (28 markers + §1.3's prose mention),
  unchanged from the pass start. Five markers carry
  `amended 2026/08/14`: §5.4 (446), §6.4 (768), §16.1 (2553),
  §16.4 (2657), §17.1's honesty clause (2943).
- Wire constants verified untouched: `IK_MSG1_LEN` 174, `IK_MSG2_LEN`
  81, `INIT_PACKET_LEN` 196, `RESP_PACKET_LEN` 107, `MAX_DATAGRAM`
  1200, `MAX_PLAINTEXT` 1170, `VERSION` 0x01,
  `PROLOGUE b"slither\x01"` — in §2.3, §3.1, §3.5, §5.1 and the
  consolidated constants table.
- `DEAD_TIMEOUT` value unchanged (15 s in §5.7 and the constants table);
  both new references (§6.7, §6.8) name it, never its value.
- Untouched as directed: §1.1, §7.4, §7.5, §7.6, §7.7, §13.3, the
  timer/constant tables, and the preamble revision block.
- Sections edited this pass: §5.4, §6.4, §6.5, §6.6, §6.7, §6.8, §6.9,
  §16.1, §16.4, §17.1, §17.4, §18.1, Appendix B.

### Handover to the later `DEAD_TIMEOUT` pass (15 s → 25 s)
Bare "15 s" occurrences that derive from `DEAD_TIMEOUT` and will need
sweeping (this pass deliberately left them; it removed only the one in
§6.8 that it was rewriting). Line numbers as of this pass's end:
945 (§6.7 "mutually dark for 15 s"), 1193, 1227 (§7.4), 1262 (§7.5),
2446 (§15.2), 2519 and 2525 (§15.4's matrix), 2895 (§16.5's timer
family), 2947 and 2963 (§17.1's honesty clause, twice), plus the value
rows themselves at 557 (§5.7), 1239 (§7.5) and 3441 (constants table).
Not `DEAD_TIMEOUT`: 643/652/693/741/3444 are `INTRO_TTL` (also 15 s).
