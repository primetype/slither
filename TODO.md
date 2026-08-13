# slither v0.2 plan — generalize and simplify

Recorded 2026-08-13, after the 0.1.0 preparation. The spec is **unfrozen** for
this round. One invariant survives everything below: the default IK channel
stays **byte-compatible** with 0.1.0 — the golden-wire test must keep passing
throughout. Wire bytes do not move in phases 1–3.

## Decisions taken (discussion of 2026-08-13)

1. **Pattern stays IK, suite goes generic.** No arbitrary patterns, no NK
   (anonymous initiators out). Curve/Cipher/Hash become generic via a
   `Channel` trait; each concrete suite is stamped by an exported
   `macro_rules!` (`slither::channel! { pub MyChannel<X25519, ChaChaPoly, Sha256> }`)
   that expands to the `hiss::noise!` invocation (IK token block + `[12]`
   timestamp payload hardcoded) plus the `Channel` impl. No proc-macro crate.
   Rationale: `noise!` generates concrete fixed-size types; a type-generic
   `IK<C, Ci, H>` would need `generic_const_exprs` (nightly). The trait
   boundary uses slices/`Vec`, hiding the const arrays.
   mac1 stays pinned keyed-BLAKE2b regardless of suite (WireGuard-style),
   keyed over the curve's compressed encoding generically. `Identity` gains
   the curve parameter; the provider seam (hardware statics) survives.

2. **Object model replaces the actor API** (quinn-shaped). The global
   `Event` stream, `ConnId`, allow-list `Config` all dissolve:

   ```text
   Endpoint<C: Channel>                    // socket + demux (receiver index)
   ├── connect(addr, static) → Connection
   ├── accept().await → Intro              // staged Incoming, see (3)
   └── Connection = Leg 1 as an object     // seal/open, unreliable datagrams,
         │                                 // roaming + keepalive + rekey inside
         ├── Reliable<Connection>          // Leg 2: exactly-once, unordered
         ├── Ordered<Reliable<…>>          // resequencing (no wire change)
         ├── (later) Streams + CC          // 0x04 reserved space
         └── split() → halves implementing futures Stream / Sink
   ```

   Base `Connection` exposes Leg 1's honest unreliable-datagram semantics;
   `Reliable` is the documented default wrapper.

3. **Staged accept (typestate, each stage consumes into the next).** This
   generalizes the already-ratified staged-responder-cost rule — the
   `read_message_1_with` closure turned inside-out:

   | Stage | Cost so far | Visible | Reject on |
   |---|---|---|---|
   | `Intro` (parsed + mac1) | 1 keyed hash, **0 DH** | addr, sender_index | banned addr, bad mac1 (automatic) |
   | `.read_identity()` → `Claimed` | **1 DH** (`es`) | *claimed* static | unwanted identity (sound: rejecting a claim) |
   | `.authenticate()` → `Proven` | **+1 DH** (`ss`) | possession proven, timestamp | replay = automatic fail (guard is not policy) |
   | `.accept()` → `Connection` | **+2 DH** (`ee`,`se`), msg2 sent | established | — |

   Dropping the object at any stage = silent reject (nothing transmitted).
   Accept queue parks **stage-0 objects only** (raw ~196 B + addr), bounded,
   overflow drops silently; dedup by `(source addr, sender_index)`
   replace-with-newest (retransmits keep the index, refresh ephemeral +
   timestamp); each `Intro` carries a deadline (initiator gives up at 90 s).

4. **hiss addition (proposed): split msg1 read.** `noise!` additionally
   emits `read_message_1_intro(&msg1) → (ClaimedStatic, MidRead)` and
   `MidRead::complete() → RespAccept`, so the stage boundaries are native.
   **Fallback needing no hiss change**: stage 1 runs `read_message_1_with`
   with a recording closure returning `false` (1 DH, state dropped); stage 2
   re-reads with an accepting closure (re-does `es`). Rejects cost 1 DH
   either way; only the accept path pays +1 DH. Usable to start phase 1
   without blocking on a hiss release.

5. **Sans-io core + thin tokio shell** (recommended, to confirm): the
   protocol logic moves out of the `endpoint.rs` actor into pure state
   machines ("bytes/deadline in → transmits + next deadline out"), the async
   objects are a thin shell. `session.rs`/`recovery.rs`/`frame.rs`/
   `handshake.rs` are already pure — the actor is the only entanglement.
   Alternative (a): keep an internal driver task behind the objects.

6. **Ordering**: receiver-side `Ordered` wrapper, **zero wire change** — the
   per-connection DATA sequence already exists inside the seal, and ordering
   is a post-decrypt concern (a cleartext order field would only leak
   metadata; the cleartext packet counter already exists as the nonce, same
   as WireGuard). WireGuard itself does not reorder — window + deliver
   as-arrived. Head-of-line blocking + bounded resequencing buffer accepted.

7. **Streams + congestion control**: later milestone (0x04 frame space).
   Fragmentation (today's ~1.1 KB message cap) falls out of streams.
   mac2/cookies also deferred to that round.

## Phases

| Phase | Content | Wire | Estimate |
|---|---|---|---|
| 1 | Sans-io core split + object API: `Endpoint`, staged `Intro`→`Connection`, `Reliable` wrapper; actor/Event/ConnId retired | unchanged | ~2–3 weeks |
| 2 | Suite genericity: `Channel` trait, `channel!` macro, `Identity<C>`, generic mac1 | unchanged (golden test) | ~1 week |
| 3 | `Ordered` wrapper, `split()`, futures `Stream`/`Sink` impls | unchanged | days |
| 4 (later) | Streams + CC, cookies/mac2 | yes — reserved space | separate round |

**Process, per phase: spec first, code second.** Draft the SPEC.md v2
section, agree, then implement with tests pinning the spec (same discipline
as v1, new ratification dates). All of phases 1–3 = release 0.2.0;
default-channel wire compatibility with 0.1.0 preserved.

## Open items

- Confirm sans-io (5b) over internal-driver (5a). Recommendation: sans-io.
- Decide hiss split-read timing: land in hiss first, or start phase 1 on the
  fallback and swap when hiss ships it.
- Naming pass on the staged types (`Intro`/`Claimed`/`Proven` are working
  names).

## Immediate next steps

1. Draft the SPEC.md v2 section for phase 1 (object model, staged accept
   semantics + costs, queue bound/dedup/deadline, Leg 1 as public surface).
2. Spike the sans-io extraction of the actor to validate the shape.
3. Write up the `noise!` split-read proposal for hiss.

## Standing context

- v0.1.0 is release-ready on `main` (gates green, `cargo package` verified);
  awaiting: GitHub remote + push, CHANGELOG `[Unreleased]` → `[0.1.0]`, tag,
  `cargo publish`. Deliberately deferred by the maintainer.
- bubble integration deferred; slither is independent.
