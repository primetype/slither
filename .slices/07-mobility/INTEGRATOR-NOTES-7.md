# Slice 7 — integrator notes

Owned by the integrator. Not the planner's, auditor's, implementer's or
test authors' path. Base commit `b649575`.

---

## N1 — the fixture cannot move an address (working rule 13)

**Verified directly at `b649575`, not inferred.**

`src/testutil/mod.rs:577` —

```rust
pub struct FlakyWire {
    addr: SocketAddr,      // plain field, set once at construction
    net: Rc<RefCell<Inner>>,
    ...
    notify: Rc<Notify>,
}
```

`addr` is immutable after `Network::endpoint(addr)` builds it, and
`Inner.endpoints: BTreeMap<SocketAddr, EndpointState>` (`:372`) is keyed
by that same address. `FlakyWire::local_addr()` returns the field.
`send_to` stamps `src: self.addr` (`:685`); `recv_from` looks the inbox up
by `self.addr` (`:737`, `:755`). `FlakyWire` is **not** `Clone` — the
`#[derive(Clone)]` at `:390` is `Network`.

**Consequence: no wire can change its address.** Slice 7's stories S18
("the peer changes network") and S19 ("*our* address changes, and a NAT
rebind") are therefore **unreachable from the fixture by construction**,
in precisely the way working rule 13 describes: *"when a whole class of
fault is absent from the results, suspect the harness before the
authors."*

The one existing escape is `Network::inject(from, to, bytes)` (`:537`),
which delivers with a **spoofed source** and no policy. It is the forgery
fixture — mac1 garbage and off-path spoofing. It can *simulate* a
datagram arriving from a new address, but it cannot make a **real
endpoint originate from one**, so it cannot produce an authenticated,
window-fresh packet from a moved peer except by replaying bytes the tap
already saw — which is a replay, and §7.2's window is exactly what
rejects it. `inject` cannot close S18.

### The gap is one-sided in the same way slice 9's tracing gap is

Roaming is driven by **authenticated receipt** (S18, RESOLVED at
approval). The *receiving* side of a roam is testable today — `inject`
plus a hand-sealed packet can exercise the re-home decision. The
*originating* side is not testable at all: nothing can make an endpoint
send from a new address. So the fixture can pin "we re-home when a valid
packet arrives from elsewhere" and cannot pin "a peer that moves actually
gets its traffic through", which is the half the story is about.

### What the extension has to be

`FlakyWire::rebind(new_addr)`, plus `Network` support:

1. `addr` becomes `Cell<SocketAddr>` (the wire is `!Send` and every other
   mutable field is already a `Cell`/`RefCell` — `:572` documents the
   convention, so this is in keeping).
2. `Network` moves the `EndpointState` from the old key to the new one,
   **carrying the existing `notify: Rc<Notify>`** — the driver's recv loop
   is parked on that exact `Rc`, and a fresh `Notify` would hang it.
3. Rebinding onto an already-registered address must panic, matching
   `Network::endpoint`'s existing assertion (`:435`).

### The one real design question — in-flight datagrams (needs a ruling)

On rebind, datagrams already queued in the old address's inbox with a
future `deliver_at` are datagrams **in flight toward the mover**.

- **(a) carry the inbox across.** Models a soft handover; nothing is lost.
- **(b) abandon it — fresh empty inbox at the new address.** Models what
  an interface change and a NAT rebind actually do: the old mapping stops
  delivering, and anything already sent to it is gone.

**(b) is the honest model and I lean to it**, because it is what makes
S18's "positive obligation on the mover" bite: under (a) a peer could
move and stay silent and still receive, which is exactly the case S18
says must be indistinguishable from a peer that vanished. Under (a) the
story's central claim would pass for the wrong reason — a bound the
degenerate implementation satisfies for free (working rule 9).

Note (b) needs no change to `deliver`: an unregistered destination
already drops (`:553`), so vacating the old key gives the right behaviour
for anything sent *after* the rebind for free. Only the already-queued
heap needs an explicit decision.

### Ownership

`src/testutil/mod.rs` is a **single path** and rule 6 forbids handing it
to two concurrent agents. It is shared infrastructure that both the
implementer and the test authors need before either can start, so it
cannot belong to either of them mid-slice. **It lands before dispatch,
committed, as part of the contract's inputs** — working rule 14: an
isolated agent sees a commit, not a working tree, and a fixture that
arrives after the cut is a fixture no worktree agent has.

---

## N2 — what slice 7 does NOT have to build (verified at `b649575`)

Checked directly so no one re-plans work that is already done, and so no
contract claims a seam that is not there (working rule 11).

- **Ruling 50 — dropping a `Connecting` cancels — is DONE.**
  `impl Drop for Connecting` at `src/shell/endpoint.rs:233`, cited from
  `src/shell/driver.rs:55,67`. Round 8 also verified its PENDING
  interaction clean: all three readers of "is this static PENDING?"
  (§6.4's ruling-35 branch, §6.5's hint set, §5.4's state row) read the
  same pending tables that cancellation empties. S3a's evil twin is
  closed; slice 7 inherits it rather than building it.
- **`Connection::closed()` — half of ruling 46 — is DONE.**
  `src/shell/connection.rs:167` (`pub async fn closed(&self) ->
  ConnectionLost`) over `poll_closed` at `:182`. So **S27 splits**: its
  `closed()` half already stands, and only the **notification stream**
  half is slice 7 work. A plan that treats S27 as wholly new will
  duplicate a working future.
- **The notification seam is pre-staged, deliberately empty.**
  `src/shell/shared.rs:589`:
  ```rust
  /// §16.2's per-kind notification retention. Slice 7 fills it.
  ///
  /// A unit struct rather than an empty enum: it is a **place**, not a
  /// claim about which kinds exist.
  #[derive(Debug, Default)]
  pub(crate) struct NotificationSlots;
  ```
  That comment is a deliberate defence against defect class 1 — it
  refuses to state a list it cannot yet bound. Slice 7 fills it, and
  **the moment it does, the list becomes exhaustive-by-reading** (working
  rule 8). Whatever slice 7 puts here must say what bounds it.
- **`ConnEvent` (`src/core/connection/mod.rs:1768`) has neither
  `Contested` nor `AddressMoved`.** Twelve variants today, ending at
  `Closed(ConnectionLost)`. Both are slice 7 additions.

## N3 — the ruling set slice 7 must satisfy, read from the record

From rounds 3–8, so the contract quotes the *current* rule and not the
superseded one. Several of these supersede each other and the
superseded form is the intuitive one, which is exactly how a contract
goes wrong.

| # | rule as it now stands | note |
|---|---|---|
| 35 | §6.7's comparison runs **inside** the PENDING branch; ours smaller ⇒ `AcceptError::Stale`, pending left in place, candidate timestamp recorded as the winner-side record does | amends ruling 32, whose "the tie-break does not run here" was wrong |
| 36 | a basis-`None` refusal marks the connection **contested**: ack-eliciting PING, ACK required within `KEEPALIVE_TIMEOUT`, else `ConnectionLost::TimedOut` and the parked `Intro` becomes acceptable | option (c), a 12-byte timestamp in msg2, **declined as wire-affecting** — do not re-propose |
| 39 | idle-from-install dies at `DEAD_TIMEOUT`; the dance stays **automatic** for any connection that has carried one exchange | making all keepalive opt-in was declined |
| 40+42 | `set_persistent_keepalive` admits **[1 s, `DEAD_TIMEOUT`)** — floor *and* ceiling; default **10 s**; the beacon fires unconditionally, **not** gated on `R > S`, and stays in the marking set | 40 alone removed the floor; 42 put it back. Quoting 40 without 42 admits 1 ms |
| 41 | `probe_floor` = the counter the **next seal will use at mark time**; clear on any ACK covering any counter **≥** `probe_floor`; concurrent marks collapse to **one** state, one floor, one deadline | "an ACK covering the packet that carried the PING" is the **superseded** form and is unsatisfiable — §8.7 files PING as never-retransmitted, so a lost probe packet can never be ACKed |
| 43 | the probe **counts in the sent map** so §17.5's cwnd bound stays true; ≤ one probe per `KEEPALIVE_TIMEOUT` per connection | |
| 44 | `set_persistent_keepalive -> Result<(), ConfigError>` with `ConfigError::{KeepaliveTooShort, KeepaliveTooLong}`; **never** panic, debug-assert or clamp; a rejected call leaves the interval **unchanged** | panicking is UB across bubble-ffi to iOS. `ConfigError` sits **outside** §18.1's taxonomy |
| 45→46 | the mark is application-visible via §16.2's **notification stream**, not `ConnEvent`; emitted at **probe transmission**, not at marking; **variants renamed** — `under_probe: bool` was rejected because `false` reads as the mark-pending state | 45's `ConnEvent::Contested { under_probe: bool }` "achieved nothing" and is superseded. A contract that quotes 45 builds the wrong type |

**The 41 and 45→46 rows are the trap.** In both, the superseded text is
the one a reasonable implementer would invent unaided, and in both the
superseded form is *unbuildable* rather than merely different. These
belong in `CONTRACT-7.md`'s §0 override table.

---

## N4 — ruling 137's four fences: checked, and the design is already right

I have been describing this as carried debt with an open question. It is
neither. **Slice 5 landed the full mechanism and stated the split**; the
question I posed to the planner ("can one field serve four fences?") is
already answered in the code, and the answer is *no — and it does not
have to, because there are two mechanisms.*

§13.6:3804–3815 fences the pre-roam flight out of four things:
**(1)** a congestion event, **(2)** the persistent-congestion walk,
**(3)** an RTT sample, **(4)** `app_limited` window growth. (A fifth
behaviour sits beside them and is *not* a fence: `min_rtt` is **re-seeded**
from the first post-roam sample.)

The split, from `src/core/connection/congestion.rs:71–81`:

> §14.6's roam reset. **Uncalled until slice 7.** … the recovery-period
> marker is **set to the roam instant, not cleared**, which fences the
> pre-roam flight out of both §14.3's congestion event and §14.5's
> `app_limited` growth. It does **not** fence the RTT sample or the
> persistent-congestion walk: ruling 137 puts those on
> `SentPacket::path_gen`, because `recovery_start` is also set by every
> ordinary congestion event and reusing it would suppress RTT sampling
> after every normal loss episode.

So: `recovery_start` ⇒ fences (1) and (4); `path_gen` ⇒ fences (2) and
(3). Two mechanisms, four fences, and the reason they cannot be one is
recorded — reusing `recovery_start` for the RTT fence would suppress
sampling after **every ordinary loss episode**, silently and permanently
on a lossy path.

### What slice 7 actually owes here — it is small and mechanical

1. `Congestion::reset(now)` exists and is **uncalled**. Call it on roam.
2. `SentPacket::path_gen: u32` exists and is **pinned at 0** by a live
   assertion, `src/core/connection/recovery.rs:198`:
   ```rust
   debug_assert_eq!(packet.path_gen, 0,
       "ruling 137: the path generation is held at 0 until slice 7");
   ```
   and a literal `path_gen: 0` at `src/core/connection/mod.rs:1615`.
   Slice 7 increments a per-connection generation on roam, stamps it at
   send, and **removes that assertion**. The assertion is the tripwire
   that makes this impossible to forget — it will fire the moment
   roaming works, which is the intended design.
3. Gate the RTT sample and the persistent-congestion walk on
   `path_gen == current_gen`.

**Correction to my own framing.** I have twice described this as "four
fences that one recovery marker cannot serve", as though the gap were
open. The gap was *identified* by ruling 137 and *closed* by slice 5 in
the same stroke; what remains is wiring, not design. Ruling 137's
rationale names a mechanism that exists — checked, per working rule 11,
by opening the file rather than re-reading the ruling.
