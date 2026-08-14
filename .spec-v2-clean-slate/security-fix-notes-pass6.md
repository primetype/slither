# Pass 6 — rulings 50 and 51 (both wire-free)

Target: `SPEC-v2-DRAFT.md` (draft v6, 5242 lines at start).
Rollback: `.spec-v2-clean-slate/SPEC-DRAFT-v6-post-pass5-frozen.md`.
Method: targeted `grep -n` → `Read` with offset/limit → `Edit`. The file
was never read whole.

## Landscape found before editing

- Drop-semantics enumeration lives at the **end of §16.2** (lines
  3916–3928), not §16.3; §16.3 (from 3930) is "Driver and handle
  lifetimes" and already carries ruling 49's `Wire` trait block. Ruling
  50's normative block therefore goes in §16.3 (as instructed), with a
  pointer sentence added to §16.2's enumeration and a clause in §16.1.
- §16.1 line 3679: `connect()` to a static with a live `Connection`
  **or an in-flight outbound connect** returns `AlreadyConnected`.
- §5.4 line 752: the three-valued LIVE / PENDING / NONE responder state.
  PENDING = "an in-flight outbound initiation exists and no established
  connection".
- §6.4 lines 1288–1329: ruling 35's PENDING branch. Fires on "If an
  in-flight outbound initiation exists for the proven static".
- §6.5 line 1367 + §17.4 line 4471: the hint set is *the pending
  tables' dialled addresses*; the probed set is pending outbound
  remotes only.
- §17.3 line 4456: the `pending-index → connection` table, plus the
  "a datagram that routes by index but fails to open touches nothing"
  corollary.
- §7.4 lines 1862–1876: the install pin — deadline armed at install,
  `last_send = last_authenticated_recv`, so a half-open session "emits
  nothing at all: it is reaped in silence" at `DEAD_TIMEOUT`.
- §9.8 lines 2911–2928 **already** carried an overflow policy marked
  `[RATIFIED 2026/08/14]` that ends "the choice needs the ruling" — a
  leftover open question that ruling 51 closes.
- §9.6 lines 2823–2845: the receiver-emitted reset, whose retained frame
  identity is pinned as `{ stream_id, error_code = 0, final_size }`.
- §15.3's error-code registry: `0x00` NO_ERROR … `0x05`
  FINAL_SIZE_ERROR, `0x06`–`0x0f` "transport-reserved; **never sent**",
  `≥ 0x10` application via `close()`.

## Ruling 50 — dropping a `Connecting` cancels the attempt

Applied at:
- **§16.3**, new block after the handle-lifetime paragraph: the
  normative cancel rule, the cancel-on-drop rationale, the honest
  consequence (the peer's half-open session is not told; dies at
  `DEAD_TIMEOUT` per §7.4's install pin = ruling 39's reap case), the
  two declined alternatives, and the PENDING-interaction paragraph.
- **§16.2**, drop-semantics enumeration: one sentence naming
  `Connecting` and pointing at §16.3.
- **§16.1**, after the `AlreadyConnected` clause: in-flight means live;
  a dropped `Connecting` returns the static to NONE so an immediate
  redial succeeds.
- **§5.5** step 6: parenthetical that a dropped `Connecting` ends the
  retransmit train before `HANDSHAKE_GIVEUP`.
- **Appendix B**, shell-surface block: the paused-clock obligation.

### The PENDING interaction — verified, and what I found

Question asked: can a cancelled pending leave a stale PENDING entry that
ruling 35's tie-break comparison could later consult?

Finding: **no, provided cancellation empties the pending tables**, which
is exactly how the spec already defines every PENDING-consulting rule.
All three consumers of "is this static PENDING?" read the *same* pending
tables:

1. §6.4's PENDING branch (ruling 35) tests "an in-flight outbound
   initiation exists for the proven static";
2. §6.5's routing hint set is defined in §17.4 as "the pending tables'
   dialled addresses";
3. §5.4's responder rule defines PENDING as "an in-flight outbound
   initiation exists".

There is no separate per-static PENDING flag that could survive the
pending's removal, so the fix is closed by construction once the ruling
says the pending and its index are dropped. I made that explicit in the
new §16.3 block rather than editing §6.4 (§6.4 needs no new rule; it
needs the guarantee that the thing it tests is gone).

Two further checks, both clean:
- **No `Connecting` is left to resolve.** §6.4's tie-break-loser branch
  resolves the cancelled pending's `Connecting` with
  `Err(ConnectError::AlreadyConnected)`. After a drop there is no future
  to resolve, and the ruling adds no variant — §18.1 stays closed.
- **The timestamp guard is untouched.** A cancelled dial authenticated
  nothing, so it writes nothing to §17.1 — identical to a
  `HANDSHAKE_GIVEUP` expiry. Ruling 37's post-mortem pin is unaffected.

Ordering caveat made normative in the text: an implementation MUST order
the cancellation ahead of any endpoint verb issued after the drop
returns, otherwise an immediate redial could still see `AlreadyConnected`.
The spec states no FIFO guarantee for the shell→driver path anywhere, so
this is stated as an observable requirement (and pinned in Appendix B)
rather than by appeal to channel mechanics.

## Ruling 51 — an unclaimed uni stream fails loudly at `MESSAGE_RECV_MAX`

Applied at:
- **§9.8**, third bullet: the normative programming-error statement for
  mixing `recv_message()` and `accept_uni()` on one connection, with the
  two safe patterns (bidi alongside messages; in-band tagging).
- **§9.8**, the overflow-policy paragraph: broadened trigger, the error
  code stated as a rule, the declined alternatives, and the removal of
  the dangling "the choice needs the ruling".
- **Appendix B**, streams block: the `open_uni` + message-mode-receiver
  obligation (sender gets `WriteError::Reset`, not a permanent stall).

### Judgment call 1 — the reset error code is **0** (`NO_ERROR`)

The spec has **no** application-independent reserved code space usable
here. §15.3's registry is the **CLOSE** registry: `0x00`–`0x05` are
transport codes with CLOSE meanings, `0x06`–`0x0f` are explicitly
"transport-reserved; **never sent**", and `≥ 0x10` is application space
reachable only through `close()`. Minting a new code for RESET_STREAM
would mean (a) amending that registry — a wire-visible code table, on
the protected list — and (b) changing the bytes this case already emits.

The already-ratified text pins the code twice: §9.8's overflow policy
("emits RESET_STREAM with error code 0, the one receiver-emitted reset")
and §9.6's retained frame identity
`{ stream_id, error_code = 0, final_size }`. Choosing anything else
moves an emitted byte. So the fix keeps **0**, and the "defined error
code" the ruling asks for is defined *by §9.6/§9.8* as the
receiver-emitted reset code rather than by a new registry entry. The
loud-failure property the ruling demands is fully delivered:
`WriteError::Reset(0)` in place of a permanent stall.

Consequence recorded in the spec text: the sender cannot tell this reset
from a peer's own `reset(0)` or a dropped `SendStream` (§16.2). See
NEEDS A RULING below if the maintainer wants it distinguishable.

### Judgment call 2 — the trigger keeps the "`recv_message()` claim
pending" discriminator

The ruling's literal form is "a uni stream that reaches
`MESSAGE_RECV_MAX` while still unclaimed must be reset". Applied
blanket, that **breaks a legitimate, ordinary pattern**: §16.4's pull
model retains "peer-opened streams awaiting `accept(dir)`" and names
this *backpressure by retention*. A receiver that uses `accept_uni()`
only, and is merely slow to call it (busy handling the previous stream),
would have a perfectly good stream reset out from under it the moment
the sender filled the 256 KiB initial window — a spurious failure, and a
departure from every comparable design (QUIC stacks stall such a sender
and never reset).

What I applied instead, which closes the ruling's own concrete
deadlock while preserving lazy accept:

- the trigger is **any** unclaimed uni stream (the ratified "oldest"
  restriction is dropped — a stream behind a slower one could evade it)
- that is window-full without a FIN,
- evaluated while a `recv_message()` claim is pending **and** at the
  instant such a claim is made.

Under ruling 51's scenario (B loops on `recv_message()`) a claim is
pending essentially always, and it fires at the latest on B's next
`recv_message()` call. Under the lazy-`accept_uni()` receiver no claim is
ever pending and retention still governs. The discriminator is already
core-visible: the ratified text used it.

Flagged for the maintainer under NEEDS A RULING — this is the one place
I did not apply a ruling to the letter, deliberately and visibly.

### Declined alternatives recorded in the spec text

- a **per-connection uni mode fixed at configuration**: makes mixing
  unrepresentable, but removes the legitimate chat-plus-file-transfer
  connection;
- a **wire-level mode signal** on the stream: closes it at the root, but
  is **wire-affecting** — it moves bytes and turns the golden-wire pin
  red.

## NEEDS A RULING

1. **A distinguishable reset code for the message-mode overflow.** If
   the sender should be able to tell "the receiver was in message mode
   and you overran the message bound" from an ordinary peer reset, a
   code must be minted in §15.3's reserved transport space
   (`0x06`–`0x0f`) *and* that entry's "never sent" wording relaxed for
   RESET_STREAM. That is wire-visible content and a protected table, so
   it is not mine to take. Applied as code 0 in the meantime; the change
   would be one varint value in one emitted frame.
2. **The blanket-vs-guarded trigger** (judgment call 2). If the
   maintainer wants the literal blanket form, the guard clause is one
   sentence to delete — but §16.4's backpressure-by-retention for
   unaccepted uni streams should be amended in the same breath, since
   the two would then disagree.

## Verification (end of pass)

- `[MAINTAINER]`: **0**.
- `[RATIFIED`: 37 → **39** (+2, exactly the two genuinely new normative
  blocks: §16.3's *"Dropping a `Connecting` cancels the attempt"*
  (ruling 50) and §9.8's *"Mixing the two receive modes on one
  connection is a programming error"* (ruling 51). The overflow policy
  keeps its **existing** marker, re-stamped in place as
  `[RATIFIED 2026/08/14; amended 2026/08/14 — ruling 51]`, so it adds
  none. The §16.1 / §16.2 / §5.5 cross-references and the Appendix B
  obligations carry no marker — they are pointers into the two blocks.)
- Diff vs the frozen copy: **9 hunks**, all intended — §5.5 step 6,
  §9.8 (×2), §16.1, §16.2's drop enumeration, §16.3, and Appendix B
  (×3, one of them the block header gaining ruling 50). 5242 → 5402
  lines. No hunk falls inside Appendix A, §6.6, §6.7, §17.1, §18.1, or
  any wire/constants table.
- Code fences: 52 (even ⇒ balanced).
- Wire constants re-grepped and unmoved: `IK_MSG1_LEN` 174,
  `IK_MSG2_LEN` 81, `INIT_PACKET_LEN` 196, `RESP_PACKET_LEN` 107,
  `MAX_DATAGRAM` 1200, `MAX_PLAINTEXT` 1170, `VERSION` 0x01,
  `PROLOGUE b"slither\x01"`, `DEAD_TIMEOUT` 25 s, `KEEPALIVE_TIMEOUT`
  10 s, `MESSAGE_RECV_MAX` 262 144.
- "15 s": still exactly 7, none touched (incl. §6.9's `1024 / 15 s`).
- Code fences balanced; protected sections (Appendix A, §6.6, §6.7,
  §17.1, §18.1, wire/constants tables) untouched.
