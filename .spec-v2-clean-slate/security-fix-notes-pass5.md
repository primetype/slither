# Pass 5 — rulings 46–49 (applied 2026/08/14)

Baseline: 4879 lines; `[MAINTAINER]` 0; `[RATIFIED` 31; "15 s" 7; fences 50.
Final:    5242 lines; `[MAINTAINER]` 0; `[RATIFIED` 37; "15 s" 7; fences 52.

Rollback: `.spec-v2-clean-slate/SPEC-DRAFT-v6-post-ruling45-frozen.md`.

## Ruling 46 — application-facing awaitables; repair of ruling 45

- §16.1 object-model tree: two new lines (`flush / SendStream::acked`,
  `closed / notified`).
- §16.2 code block, `impl Connection`: `closed()` and `notified()` under an
  `// awaitables (ruling 46):` comment; new `#[non_exhaustive] enum
  Notification { AddressMoved { from, to }, Contested, ContestCleared }`.
- §16.2 prose: `[RATIFIED 2026/08/14 — ruling 46]` block — the latched
  `closed()`; the pull-model `notified()`; **one retention slot per kind**
  (AddressMoved coalesces to oldest-unclaimed `from` + newest `to`;
  Contested/ContestCleared bounded by ruling 41's collapse) so the state is
  O(1) and needs no queue bound; "why this is not `core::ConnEvent`" (every
  other variant is already the wakeup behind a blocking verb — republishing
  invites the read-events-never-claim wedge §10.6/§16.8 forbid); "this is a
  shell change, the core is untouched" (the shell translates; §16.4's
  signals-not-payload-carriers framing holds at both layers).
- §16.3: paragraph on handle lifetimes — the awaitables are handle-borne,
  resolve `EndpointDropped` when the driver stops, live in shell-side
  bookkeeping released after §16.4's `Retired`, and a `closed()` future is
  not a handle (dropping every `Connection` still stops the driver).
- §16.4 enum: `Contested { under_probe: bool }` → `Contested` +
  `ContestCleared`.
- §16.4 bullet rewritten (marker now reads "ruling 45; emission moment and
  variant names fixed 2026/08/14 by ruling 46, per FAB-6"): **`Contested`
  emits at the probe's transmission**, the same instant the deadline arms;
  the mark-pending gap (reachable after a roam to an unvalidated address)
  emits nothing; `slither::policy` still traces all three moments.
  `ContestCleared` is a separate variant because `under_probe: false` read
  as the pending state.
- §7.5 "When the probe cannot be sent" and §15.4's contested row: made
  consistent with the transmission-moment rule (see judgment calls).
- §17.5 established-connection memory row: notification slots named as O(1).

## Ruling 47 — delivery confirmation

- §16.2 code block: `Connection::flush() -> Result<(), ConnectionLost>` and
  `SendStream::acked() -> Result<(), WriteError>`.
- §16.2 prose: `[RATIFIED 2026/08/14 — ruling 47]` block. `acked()` = the
  `StreamFinished` event awaited; legal after `finish()` and never returns
  `WriteError::Finished`; `Reset(code)` on a reset before acknowledgement.
  `flush()` = **snapshot** semantics (every byte handed to the connection at
  the instant of the call, across every stream including §9.8's handle-less
  message streams), resolving when each is acknowledged **or abandoned by a
  reset**; later bytes do not extend it, so it terminates during a bulk
  write. Explicit non-promise: transport receipt, not peer-application
  delivery/processing/storage, and not a guarantee against a peer that acks
  then dies. No wire change; §18.1 untouched.
- §16.2's `finish()` sentence now says "not when the peer has it".
- §9.8: `flush()` named as the way to await a message's acknowledgement,
  since no stream handle surfaces.
- §15.2 local-close bullet: clarifies that unacknowledged/queued data is
  never sent after `close()`, points at `flush()`, and states the clause is
  deliberately unchanged.

## Ruling 48 — the denylist is the application's

- §6.1, immediately after the "nothing durable may be keyed…" paragraph
  (which is unchanged): `[RATIFIED 2026/08/14 — ruling 48]` — that clause
  binds slither's own state; slither provides **no ban list, no deny list,
  no reputation store** and writes nothing durable about a peer at any
  stage, before or after 2 DH; the application may reject at any stage for
  any reason, including from a list it owns; a 3-row cost table
  (Intro 0 DH / Claimed 1 DH / Proven 2 DH) and the statement that there is
  **no 0-DH reject-by-identity** (msg1's static is encrypted; reading it
  *is* the `es`); the hazard — a list keyed on a *claimed* static bans on
  an assertion anyone can make, so an attacker can get a third party
  banned; deciding on a *proven* static is immune; the trade is the
  application's, and slither only prices it.
- §6.2: one sentence pointing at it. No mechanism added anywhere.

## Ruling 49 — the `Wire` trait; send-failure trace obligation

- §16.3: `[RATIFIED … — ruling 49]` + the fenced trait exactly as shipped
  (`send_to`, `recv_from`, `std::io::Result`), blanket impl for
  `tokio::net::UdpSocket`; four normative properties — application-supplied
  via `Endpoint::builder()`, **not required to be `Send`** (and no `Send`
  bound on its futures), `&self` on both methods because one driver owns
  the seam, and `testutil::FlakyWire` is a `Wire`.
- §16.3: `[RATIFIED … — ruling 49]` — a failing `send_to` **MUST** be traced
  against the connection and is **not acted on**, with the three reasons
  (not authoritative because liveness is receive-driven by ruling §7.4;
  `ENETUNREACH` precedes a successful roam §7.3, so acting on it would
  delete the migration guarantee; the application supplies the `Wire` and
  already sees every `io::Error` with its address — slither's job is to
  make the failure explicable). §18.1 stays closed.
- §18.2: new `slither::io` target row + an explanatory paragraph carrying
  the ruling marker.
- §16.10: `FlakyWire` is a `Wire` and nothing else; a send-failing `Wire` is
  the only fixture the trace obligation needs.

## Appendix B

- New **"The shell surface (rulings 46, 47, 49)"** block after CLOSE:
  `closed()` on every teardown row with no verb in flight + latching;
  notification retention/coalescing/cancel-safety; the message-then-close
  delivery obligation (send, `flush().await`, close, assert the peer
  received it — with the no-flush ordering failing at the injected loss
  rate) plus the `acked()` variants and `flush()` termination cases; the
  send-failure trace obligation (connection survives, traces emitted,
  a failure held past `DEAD_TIMEOUT` still dies receive-driven).
- Contested-probe block gains a sub-bullet: nothing while mark-pending,
  `Contested` at the probe's transmission (pinned with the deadline arming),
  `ContestCleared` on the covering ACK, at most one of each per mark.

## `[RATIFIED` accounting: 31 → 37

Six new markers, all for rulings 46–49: §6.1 (48), §16.2 (46), §16.2 (47),
§16.3 Wire trait (49), §16.3 send-failure obligation (49), §18.2 (49).
Ruling 49 carries three because its two normative parts live in different
sections and the trace target is a separate operator-contract addition.
§16.4's existing ruling-45 marker was **amended in place**, not duplicated.

## Judgment calls

1. **Verb names** `closed()` / `notified()`; enum `Notification` with
   `AddressMoved` / `Contested` / `ContestCleared`, `#[non_exhaustive]`.
   FAB-6 suggested `Contested`/`ContestCleared`; adopted at both layers.
2. **Retention = one slot per kind** with `AddressMoved` coalescing
   (oldest-unclaimed `from`, newest `to`). Chosen so the surface needs no
   new named constant and no queue bound — the constants table is untouched.
3. **`flush()` snapshot semantics**, and reset-abandoned bytes resolving
   `Ok(())` rather than hanging — needed to keep the verb terminating
   without a new error variant.
4. **A new trace target `slither::io`** rather than folding into
   `slither::roam`. §18.2 calls the target set operator-visible contract;
   adding one is additive, renaming/dropping is not.
5. **§7.5 and §15.4 touched** beyond the literal "ruling 45's text" — both
   carried the same mark-vs-transmission conflation FAB-6 flagged, and
   leaving them would have made the new §16.4 text inconsistent with them.
   Both edits are precision only, wire-free.
6. **§17.5** gains the notification slots so the memory accounting stays
   honest against the O(1) claim made in §16.2.
7. **`ConnectionLost: Clone`** stated as a derive requirement for a latched,
   multi-awaiter `closed()` — explicitly not a variant change.
8. **No preamble ledger entry** for rulings 46–49, following the precedent
   set by rulings 44 and 45 (both live only as in-section markers; the
   blockquoted ledger stops at ruling 43 / "with pass 4 the round closes").

## NEEDS A RULING

None. Nothing in rulings 46–49 required a decision the ruling text did not
already settle; the eight items above are shape choices inside the latitude
the rulings gave, recorded here so they can be overruled cheaply.

## Untouched, verified

Appendix A, §6.6, §6.7, §17.1, §18.1, the wire/constants tables — no diff
hunks in any of them. Wire constants unchanged: `IK_MSG1_LEN` 174,
`IK_MSG2_LEN` 81, `INIT_PACKET_LEN` 196, `RESP_PACKET_LEN` 107,
`MAX_DATAGRAM` 1200, `MAX_PLAINTEXT` 1170, `VERSION` 0x01,
`PROLOGUE b"slither\x01"`, `DEAD_TIMEOUT` 25 s, `KEEPALIVE_TIMEOUT` 10 s.
All 7 "15 s" occurrences intact, §6.9's `(1024 / 15 s)` included.
