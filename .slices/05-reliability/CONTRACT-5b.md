# CONTRACT-5b — the binding API contract for slice 5b (the reliability shell)

**Status: BINDING.** Both 5b agents compile against this and neither may
change it. If you believe something here is wrong, **say so in your report
and implement it as written anyway** (working rule 5).

Extracted from `.slices/05-reliability/PLAN-5.md` §2.5–§2.7. Slice 5a is
**landed and green** at `423c9f8` (784 tests, all nine gates).

---

## 0. The rulings that govern 5b

| # | Decision |
|---|---|
| **47 / 54** | `acked()` exists in two forms: `SendStream::acked()` (this stream's bytes **and its FIN**) and `Connection::acked()` (a **snapshot** across every stream at the instant of the call). |
| **124** | A handle's own terminal state outranks the connection's death latch. Order: terminal state → death latch → empty-buffer short-circuit → the core. **§2.6 below adds the one documented exception**, for `read` and `accept_*` only. |
| **128** | Received, unclaimed stream state **survives the connection's death**: `read` serves buffered bytes then `Ok(None)`; `accept_*` hands over streams opened before the death; when nothing is left, both report `ConnectionLost`. **Parking is never permitted on a dead connection.** |
| **132** | Ruling 128 is cheaper than 128 itself priced it. On the draining path `drop_state` runs at `CloseLinger` expiry, **not** at the death, so `streams`/`flow` are still there. **Ruling 81 is not touched.** The blockers are ruling 118's accept latch, ruling 124's read precedence, and `core::Connection::read`'s `self.lost` guard. |
| **133** | Local close frees `streams`/`flow` at once; the **draining** (peer-CLOSE) path retains them for `CLOSE_LINGER`; the no-linger deaths retain nothing. Document the last consequence rather than leaving it to be found. |
| **135** | **Both `acked()` verbs answer from their settled snapshot before the death latch** — ruling 128's defect on the sender's side, reachable by the identical mechanism. |
| **139(e)** | `SendStream::acked()` **before `finish()` parks and does not resolve.** Put the hazard in rustdoc: `write().await; acked().await;` hangs, and it is the one shape a caller writes by accident. |
| **143** | The handle's stream-id cache fills **eagerly at construction** — already applied at your base commit. Do not make it lazy again. |

---

### 2.5 Shell — `src/shell/stream.rs`

```rust
impl<S: Handshake> SendStream<S> {
    /// §16.2 / ruling 47. Resolves once every byte written to **this** stream
    /// and its FIN are acknowledged by the peer's transport.
    pub async fn acked(&mut self) -> Result<(), WriteError>;
    pub(crate) fn poll_acked(&mut self, cx: &mut Context<'_>)
        -> Poll<Result<(), WriteError>>;
}
```

**Every outcome, in ruling 124's precedence order:**

1. `local_end == LocalEnd::Reset`, or the stream was reset by the peer's §9.8
   overflow reset → `Err(WriteError::Reset(code))`. §16.2:4373.
2. This half already reached `DataRecvd` (all bytes **and** the FIN acked) →
   `Ok(())`. **This outranks the death latch** — ruling 124's principle: a
   stream that completed did not un-complete when the connection died, and
   reporting `ConnectionLost` over a fully-acknowledged transfer is ruling
   121's misreport with its sign flipped.
3. Connection death latch → `Err(WriteError::ConnectionLost(l))`.
4. Otherwise park in a **new** waker map, `blocked_ackers: BTreeMap<StreamRef,
   Wakers>`, woken by `ConnEvent::StreamFinished { r }` and by
   `ConnEvent::StreamReset { r, .. }`.

**Never `Err(WriteError::Finished)`** — §16.2:4372 in terms: *"It is legal and
expected after `finish()`, and never returns `WriteError::Finished` for that
reason."*

**Before `finish()` it parks and does not resolve on a live connection.** §16.2
requires "every byte … **and its FIN**", and a FIN that was never queued cannot
be acknowledged. This is stated because it is the one shape a caller will write
by accident (`write(..).await; acked().await;`) and it looks like a hang. It
goes in rustdoc at the call site, next to §16.2's own "legal and expected after
`finish()`".

```rust
impl<S: Handshake> Connection<S> {
    /// §16.2 / rulings 47, 54. Snapshot at the instant of the call.
    pub async fn acked(&self) -> Result<(), ConnectionLost>;
}
```

Because the snapshot is taken **at the call** and not at the first poll, this
is written as an `async fn` that takes the snapshot in its body and then
`poll_fn`s over `poll_acked(cx, &snapshot, key)`. Outcomes:

1. `snapshot_settled(&snap)` → `Ok(())`. **Outranks the death latch**, for
   ruling 124's reason and because the alternative reintroduces ruling 128's
   defect on the *sender's* side — see §10-Q1.
2. Connection death latch → `Err(ConnectionLost(l))`.
3. Otherwise park in `closed_wakers`' sibling: a new `settled_wakers: Wakers`
   on `ConnCell`, woken on **every** `StreamFinished` and `StreamReset`, and
   on the latch.

An **empty** snapshot (nothing ever written) resolves `Ok(())` on the first
poll, on a live *or* dead connection.

### 2.6 Shell — ruling 128's post-death drain

```rust
// src/core/connection/mod.rs — CHANGED
pub(crate) fn read(&mut self, now: Instant, r: StreamRef, buf: &mut [u8])
    -> Result<Option<usize>, ReadError>;
```
The unconditional `self.lost` check at `mod.rs:467–469` is **replaced** by:
serve the stream normally if the half is still in the table; return
`Err(ReadError::ConnectionLost(l))` only when `self.lost.is_some()` **and** the
half is gone or has nothing left (no buffered bytes and no pinned FIN reached).

`core::Connection::accept(dir)` needs **no change**: it already has no `lost`
guard (`mod.rs:405–408`) and already hands over unclaimed streams after death.

Shell-side, the ruling-124 precedence gains one step for `read` and `accept_*`
**only**:

| verb | new order |
|---|---|
| `RecvStream::poll_read` | (1) own terminal state → (2) empty-buffer short-circuit → (3) **the core call** → (4) death latch, **only if the core had nothing** → never `Pending` when the latch is set |
| `Connection::poll_accept_{bi,uni}` | (1) **the core call** (`accept(dir)`) → (2) death latch if `None` → never `Pending` when the latch is set |
| every other 4b verb | **unchanged** — ruling 124 stands as ratified |

**"Parking is never permitted on a dead connection"** (rulings.md:3221–3223) is
the load-bearing clause: when the latch is set, a core answer of `Ok(Some(0))`
("no data available") must be converted to `Err(ConnectionLost)`, not to
`Pending`. Getting this wrong hangs a reader forever, which is the same trap
class the `Ok(Some(0))`/`Ok(None)` convention closes.

### 2.7 What slice 5 must NOT build

- **No roaming.** §13.6 and §14.6 are implemented as the two `on_roam` /
  `reset` methods above and are **uncalled**; slice 7 calls them. See §10-Q9
  for the one schema decision that cannot be deferred with them.
- **No pacing, no ECN, no CUBIC/BBR** (§14.7).
- **No datagram tracking** (§11 is slice 6). §13.5's "DATAGRAM markers" get no
  `SentFrame` variant: a datagram's class is `never` (§8.7), so it needs no
  identity — only the packet's `size`, which it already contributes. Recorded
  as slice 6's, and the reason is stated so slice 6 does not re-open the map's
  schema by reflex.
- **No new error variants.** §18.1 is closed by process; `error.rs:3–41`. Both
  `acked()` verbs use existing variants, which §16.2:4412–4416 states as a
  requirement.

---


---

## Appendix — the integrator's files (working rule 15)

Not yours, either of you:

- **`Cargo.toml`'s `[[test]]` stanza** for `story_reliability`. Cargo
  *refuses to parse the manifest* when a `[[test]]` names a missing file,
  which reds every gate at once. 5b-impl may commit it **commented out**
  under an integration header; the integrator uncomments it.
- **`tests/spec_streams.rs`** — deleting the `#[ignore]` on
  `a_receiver_can_drain_a_stream_the_sender_closed_behind`, which ruling
  128 makes reachable. Integrator's, at the end.
- **`src/core/connection/tests_streams.rs`** — deleting the `#[ignore]` on
  `credit_frames_precede_the_stream_fill_in_a_packet`. Ruling 130 makes it
  slice 5's; ruling 134's "`write()` accepts what the window cannot send"
  is what makes it reachable.
- **`src/core/connection/testfix.rs`** — the shared in-crate fixture. If
  5b needs a capability it lacks, **report it, do not add it**.
