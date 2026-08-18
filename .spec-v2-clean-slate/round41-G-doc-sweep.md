# G — Doc Sweep (round 41 material, items 10, 12, 14)

Base commit: `94dab20` (Ruling 257: the bare FIN defers when no frame fits)
Status: COMPLETE

READ-ONLY inventory agent. All text below is DRAFT ONLY — nothing applied.

---

## Item 14 — doc-staleness sweep

### 14(a) README.md

Read fully: `README.md:1-109`. Verified against `src/lib.rs`, `src/shell/endpoint.rs`,
`src/shell/connection.rs`, `src/shell/staged.rs`, `src/config.rs`,
`src/identity.rs`, `src/constants.rs`, `SPEC.md` §14.7, and the repo root
(`examples/` does not exist).

**Finding 1 — the "reserved" claim is false on all three counts.**
`README.md:24-25`:
> `9002 loss detection + PTO — retransmit frames, never packets) is **Leg 2**,`
> `riding ON TOP of this unreliable, authenticated packet layer. Built and`
> `**ratified** in `SPEC.md` §9; congestion control, streams, and fragmentation`
> `stay reserved.`

All three are implemented: congestion control is NewReno, ratified and
built (`src/core/connection/congestion.rs`; `SPEC.md` §14, "## 14.
Congestion control" at `SPEC.md:4751`); streams are §9.8's message/stream
surface (`Connection::open_bi`/`open_uni`/`accept_bi`/`accept_uni`,
`src/shell/connection.rs:548,575,618,639`); fragmentation is the STREAM
frame's offset field, not a separate reserved feature (`SPEC.md:470`,
`"fragmentation and ordering are the STREAM frame's offset field (§9)"`).
`lib.rs`'s own crate doc already says the opposite of the README
(`src/lib.rs:8-9`, `"...streams, messages and unreliable datagrams inside
the sealed plaintext with flow control, RFC 9002 loss recovery and
congestion control"`). What v1 actually leaves out is pacing, ECN, and
alternate controllers (CUBIC/BBR) — `SPEC.md:4958-4961`, §14.7 "Explicitly
out".

**Proposed replacement** (`README.md:21-25`, the "QUIC-style frames" bullet):
> - **QUIC-style frames** — the reliable frame layer (frames, ACK ranges, RFC
>   9002 loss detection + PTO — retransmit frames, never packets — and NewReno
>   congestion control) is **Leg 2**, riding ON TOP of this unreliable,
>   authenticated packet layer. Built and **ratified** in `SPEC.md` §§9, 14:
>   streams, reliable messages, unreliable datagrams and congestion control
>   all ship in v1. Pacing, ECN and alternate controllers (CUBIC/BBR) stay
>   reserved (§14.7).

**Finding 2 — the "Usage sketch" (`README.md:51-69`) does not match any API
in the crate**, and its referenced example does not exist.

| README claim | Reality | Evidence |
|---|---|---|
| `use slither::endpoint::{Config, Endpoint};` | No `endpoint` module. `Config` lives in `config`, `Endpoint` in `shell`; both are re-exported at the crate root. | `src/lib.rs:234-239` (`pub mod` list has no `endpoint`), `src/lib.rs:270,277-279` (root re-exports) |
| `use slither::handshake::SoftwareIdentity;` | No `handshake` module. It is `identity::SoftwareIdentity`, re-exported at the crate root. | `src/lib.rs:237,275` |
| `Endpoint::start(identity, socket, config)` | No `start` associated fn. Construction is `Endpoint::builder::<W>().identity(id).wire(socket).config(cfg).build()`, inside a `tokio::task::LocalSet` (`build()` panics outside one). | `src/shell/endpoint.rs:55,420-503` |
| `Config::new().allow(&family_device_static)` | `Config` has no `allow`/allow-list method at all — its methods are `with_intro_queue_cap`, `with_intro_max_per_source`, `with_epoch_size`, `with_clock` (plus accessors). "family-devices set" appears nowhere outside this README. | `src/config.rs:97-211` (grep confirms `family-devices`/`family_device` occur only in `README.md:34,60`) |
| `let session = endpoint.connect(peer_addr, peer_static);` (used directly) | `connect()` is sync and returns `Result<Connecting<I>, ConnectError>`; the `Connecting` must itself be awaited to reach a `Connection`. | `src/shell/endpoint.rs:218-222` (signature), `src/lib.rs:66-68` (doc obligation #1's own snippet: `endpoint.connect(addr, peer)?.await`) |
| Peer allow-listing via `Config` | There is no allow-list config. The app inspects the claimed identity itself, mid-ladder: `Endpoint::accept() -> Option<Intro<I>>`, then `Intro::read_identity() -> Claimed<I>`, whose `claimed_static()` the app checks before calling `authenticate()` — dropping the staged object is the silent reject. | `src/shell/staged.rs:107,130,179,243,249,262,318,330,347` |
| `session.send(b"hello".to_vec())?;` | No `send` method. The reliable-message API is `async fn send_message(&self, msg: &[u8]) -> Result<(), MessageError>` — async, takes a slice, must be awaited. | `src/shell/connection.rs:723-725` |
| `while let Some(event) = endpoint.next_event().await { Established, Incoming { payload, .. }, Dead, EndpointMoved, Failed }` | `Endpoint` has no `next_event` and no such enum exists anywhere in the crate. The real per-connection event surface is `Connection::notified() -> Result<Notification, ConnectionLost>` where `Notification` is `AddressMoved { from, to } \| Contested \| ContestCleared`; death is observed via `Connection::closed() -> ConnectionLost`; new inbound connections arrive via `Endpoint::accept()`'s staged ladder, not a flat event enum. | `src/shell/connection.rs:269`, `src/shell/shared.rs:611-641`, `src/shell/endpoint.rs:103` |
| `[`examples/udp_loopback.rs`](examples/udp_loopback.rs)` / `cargo run --example udp_loopback` (`README.md:71-77`) | The `examples/` directory does not exist in the repository at all. | `ls examples/` → "no examples dir" at `94dab20` |

**Proposed replacement** for the whole "Usage sketch" section
(`README.md:51-77`), rewritten against the verified API (illustrative —
maintainer should confirm exact wording before landing, but every symbol
below exists at the cited line):

> ## Usage sketch
>
> ```rust,ignore
> use slither::{Config, Endpoint};
> use slither::identity::SoftwareIdentity;
>
> // Inside a tokio current-thread runtime + LocalSet (the actor is `!Send`):
> let identity = SoftwareIdentity::from_scalar(my_static_scalar, my_rng)?;
> let socket = tokio::net::UdpSocket::bind("0.0.0.0:51820").await?;
> let endpoint: Endpoint<_> = Endpoint::builder()
>     .identity(identity)
>     .wire(socket)
>     .config(Config::new())
>     .build();
>
> // Dial: connect() is sync (0 DH so far); the returned `Connecting` future
> // is what spends the 2 initiator DH and resolves once the handshake lands.
> let connection = endpoint.connect(peer_addr, peer_static)?.await?;
> connection.send_message(b"hello").await?;   // reliable, unordered, exactly-once
>
> // Accept: a staged ladder, so the app can inspect a claimed identity
> // before spending a DH on it. `accept()` is driven in a loop.
> while let Some(intro) = endpoint.accept().await {
>     let claimed = intro.read_identity().await?;      // 1 DH
>     if !my_allow_list.contains(claimed.claimed_static()) {
>         continue; // dropping `claimed` is the silent reject
>     }
>     let proven = claimed.authenticate().await?;       // 2 DH
>     let connection = proven.accept().await?;
>     // connection.recv_message().await, connection.notified().await, ...
> }
> ```
>
> There is no `examples/` directory in the tree today; if one is added, this
> section should link it rather than repeat the sketch inline.

(The maintainer may prefer to restore a real `examples/udp_loopback.rs`
instead of inlining the sketch — either fix removes the dangling link.)

**Finding 3 — timer values are stale, including a mechanism that no
longer exists.**

`README.md:40-42`:
> `...Idle sessions exchange 10 s keepalives; a session that`
> `sends into 15 s of silence is declared dead; a session past 120 s rekeys on its`
> `next send and is refused outright at 180 s.`

and `README.md:83-84` (Testability section):
> `...so the 5 s / 15 s / 90 s / 120 s / 180 s timers resolve in virtual time;`

Checked against `src/constants.rs`:
- Keepalive default: `PERSISTENT_KEEPALIVE_DEFAULT_MS = 10_000` (10 s) — README's "10 s keepalives" is correct.
- Dead timeout: `DEAD_TIMEOUT_MS = 25_000` (25 s), not 15 s. `src/constants.rs:553`, doc at `:446-447` ("Silence after which the connection is declared dead").
- Handshake give-up: `HANDSHAKE_GIVEUP_MS = 90_000` (90 s) — README's "90 s" (retransmission section, unaffected) is correct.
- Retransmit base: `RETRANSMIT_BASE_MS = 5_000` (5 s) — README's "~5 s" is correct.
- **There is no 120 s or 180 s constant anywhere in `src/constants.rs`.** Rekeying is not time-triggered at all: it is message-count triggered, `REKEY_EPOCH_MSGS: u64 = 65_536` (2¹⁶ messages/epoch — `src/constants.rs:150`, pinned by `src/constants.rs:645` `assert!(REKEY_EPOCH_MSGS == 1 << 16)`, consumed at `src/core/connection/mod.rs:3611`). The 120 s/180 s figures are WireGuard's `REKEY_AFTER_TIME`/`REJECT_AFTER_TIME`, which slither's ratified spec does not use.

**Proposed replacement** (`README.md:40-42`):
> Idle sessions exchange 10 s keepalives; a session that sends into 25 s of
> silence is declared dead; a session rekeys after 65 536 (2¹⁶) messages in
> the current epoch (§7.7), not on a wall-clock timer.

**Proposed replacement** (`README.md:83-84`):
> without a kernel. The tests run two endpoints over an in-memory
> `FlakyWire` (loss, reorder, duplication, delay, partitioning) on tokio's
> **paused clock**, so the 5 s / 10 s / 25 s / 90 s timers resolve in virtual
> time; one test uses a real UDP loopback socket (`tests/spec_shell.rs`).

(The UDP-loopback claim itself checks out — `tests/spec_shell.rs` and
`src/testutil/mod.rs` both reference `UdpSocket` — so only the timer list
needs correcting there.)

**Finding 4 — hiss version.** README does not name a hiss version anywhere
(only a generic `crates.io/crates/hiss` link at `README.md:10`), so there is
no README hiss-version staleness to fix. The stale `0.3.1` reference lives
in `CHANGELOG.md:18` instead — see §14(b) below.

### 14(b) CHANGELOG.md

Read fully: `CHANGELOG.md:1-37`. The entire file is one `## [Unreleased]`
section (`CHANGELOG.md:8`) describing the **pre-rewrite v0.1 design** —
it predates the SPEC v1 rewrite and every claim in it is now either wrong
or superseded, matching README's staleness for the same reasons (§14(a)
above): "no congestion control" (`CHANGELOG.md:26`, contradicted by
`SPEC.md` §14 / `src/core/connection/congestion.rs`), "dead 15 s" / "rekey
120 s" (`CHANGELOG.md:22`, contradicted by `DEAD_TIMEOUT_MS = 25_000` and
message-count rekeying — see §14(a) Finding 3), "an `Event` stream" and
"allow-list of remote statics" (`CHANGELOG.md:29-30`, contradicted by
`Notification`/staged-ladder API — see §14(a) Finding 2), and
`examples/udp_loopback.rs` (`CHANGELOG.md:31-33`, file does not exist).
Also stale: `hiss 0.3.1` (`CHANGELOG.md:18`) — `Cargo.toml:101,199` both
pin `hiss = "0.3.2"` today.

**No `[0.2.0]` (or any dated) entry exists for the rewrite at all.** The
rewrite runs from `73757c3` ("Slice 0 (Ground): delete the v0.1 wire, lay
the v0.2 foundation") to `94dab20` (current HEAD) — 160 commits
(`git log --oneline 73757c3^..94dab20 | wc -l`) — ratifying `SPEC.md` as
"RATIFIED" (`cd12ed7`, "Ruling 242: SPEC.md is stamped RATIFIED, with its
amendment table") after 80 rulings across ten rounds through slices 0–9,
plus five round-40 sub-slices (R40-A..E) and rulings up to 257. Gates were
last measured green with 880 tests (`cargo test`) / 1068 (`cargo test
--all-features`) at `1395ea6` ("Round 40 closes"); two rulings (256, 257)
landed after that with one new regression test, so the exact count at
`94dab20` should be re-measured by the maintainer before this entry is
finalized, not copied verbatim from this draft.

Headline capabilities are STORIES.md's ten section groups (`STORIES.md`
headers, grep only — not read in full): A. Connection lifecycle, B.
Inbound admission — the staged accept, C. Data transfer, D. Mobility, E.
Identity and crypto, F. Operational, G. Death/contest/ack await (rulings
46–47), H. Redial and mixing-error detection (rulings 50–52), I.
Composability — `AsyncRead`/`AsyncWrite`, codecs, `tower::Service`
(ruling 209), J. Liveness of the accept loop (ruling 252).

**Proposed new entry** (insert above the existing `## [Unreleased]`
section, i.e. before `CHANGELOG.md:8`):

> ## [0.2.0] - 2026-08-18
>
> A clean rewrite against `SPEC.md`, now **ratified** as slither's v1
> wire (80 rulings across ten rounds, `cd12ed7`). Replaces the
> pre-rewrite design described below in its entirety — different object
> model, different handle API, different timers. See `SPEC.md` and
> `.spec-v2-clean-slate/rulings.md` for the specification and the record
> of why.
>
> ### Added
>
> - **Two sans-io cores plus one shell.** `core::Endpoint<I: Identity>`
>   and `core::Connection` are pure state machines — `now: Instant` is an
>   argument on every mutating call, neither reads a clock — driven by a
>   single `!Send` shell actor (`tokio::task::spawn_local`, one `Wire`
>   trait for the socket seam), so the whole protocol is drivable without
>   a kernel on tokio's paused clock.
> - **A. Connection lifecycle** — dial and close (`Endpoint::connect`,
>   `Connection::close`/`closed`), a dial that never answers times out and
>   reports why, a second `connect()` to a live peer is refused rather
>   than silently superseding it, simultaneous dial-dial resolves to one
>   connection, and an idle connection with no traffic is reaped rather
>   than held open for free.
> - **B. Inbound admission — the staged accept.** A four-rung ladder
>   (`Intro` → `Claimed` → `Proven` → `Connection`, 0/1/2 DH) so an
>   application can reject an inbound identity, or park the decision
>   across event-loop turns, before spending a DH on it; a flood of
>   inbound initiations does not disturb established connections.
> - **C. Data transfer** — reliable unordered exactly-once messages
>   (`send_message`/`recv_message`), multiple concurrent streams with no
>   head-of-line blocking (`open_bi`/`open_uni`/`accept_bi`/`accept_uni`),
>   stream abandonment without killing the connection, unreliable
>   datagrams (`send_datagram`/`recv_datagram`), and backpressure via
>   flow-control credit rather than unbounded buffering.
> - **D. Mobility** — a connection survives the peer changing network or
>   our own address changing (NAT rebind), and a peer that restarts gets
>   a working connection back, all via authenticated-only roaming.
> - **E. Identity and crypto** — the `Identity`/`DhProvider` seam admits a
>   hardware-backed static key (no `Send` bound anywhere on the driver
>   path), a pluggable crypto suite with fail-closed mismatches, and
>   silent long-lived rekeying (message-count epochs, §7.7).
> - **F. Operational** — the whole protocol drivable without a kernel
>   (`testutil::FlakyWire` on tokio's paused clock), a caller-supplied
>   `Wire` with explicable send failures, and clean teardown on dropping
>   every handle.
> - **G/H — death, contest and redial.** `Connection::closed()`/
>   `notified()` for death/roam/contest events, `acked()` to wait for
>   send-and-close, immediate redial after giving up on a dial, and loud
>   (not silent) failure when an application mixes `send_message` with
>   uni streams.
> - **I. Composability** (`compat`, ratified ruling 209) —
>   `AsyncRead`/`AsyncWrite` over a stream, a `Sink`/`Stream`-backed codec
>   surface, and a `tower::Service` adapter.
> - **J. Liveness of the accept loop** (ruling 252) — a responder that
>   keeps calling `accept()` survives a lost msg2.
> - **RFC 9002 loss recovery and NewReno congestion control** (`SPEC.md`
>   §§13–14), fully implemented — not deferred, contrary to the previous
>   `[Unreleased]` entry below.
>
> ### Changed
>
> - Endpoint construction moved from a one-shot `start()` (below) to
>   `Endpoint::builder().identity(..).wire(..).config(..).build()`.
> - Session events moved from a single `Event` stream to per-connection
>   `notified()`/`closed()` plus the staged-accept ladder.
> - Peer admission moved from a `Config`-level allow-list to an
>   application-driven decision mid-ladder (`Claimed::claimed_static()`).
> - `hiss` pinned to `0.3.2` (was `0.3.1`).
>
> ### Removed
>
> - `examples/udp_loopback.rs` — not present in the current tree; either
>   restore it against the new API or drop the README's reference to it.

**Disposition of the existing `[Unreleased]` section (`CHANGELOG.md:8-37`):**
it describes a design that no longer exists in any form. Recommend either
(a) deleting it outright now that `[0.2.0]` supersedes it, or (b)
relabeling its heading to something like `## [0.1.0] - 2026-08-13
(superseded)` so the historical record survives under an honest label —
consistent with how CLAUDE.md already treats the v0.1 wire as
"historical, not authoritative" (its spec archived, its implementation
named by commit). Marked **DRAFT-FOR-RULING is not required here**
(CHANGELOG.md is not a governance document CLAUDE.md protects), but the
choice between (a)/(b) is the maintainer's call, not mine to make
unilaterally.

### 14(c) SECURITY.md and TODO.md

**SECURITY.md** — read fully: `SECURITY.md:1-84`.

**Finding 1 — the congestion-control claim is false**, same defect as
README/CHANGELOG (§14(a)/(b)). `SECURITY.md:73-76`:
> `- **No congestion control or pacing** (ratified out of Leg 2): the frame`
> `  layer retransmits on RFC 9002 loss detection/PTO but will not yield`
> `  fairly under sustained congestion. Do not point it at the open internet`
> `  at scale.`

NewReno congestion control is ratified and implemented (`SPEC.md` §14,
`src/core/connection/congestion.rs`) — it does back off under loss. Only
pacing (plus ECN and alternate controllers) stays out (`SPEC.md:4958-4961`,
§14.7). Security-relevant because the current text overstates the risk in
one place (implies no fairness mechanism exists at all) while the
"open internet at scale" caution may still be warranted for the parts
that *are* still missing (pacing means bursty sends, not sub-RTT paced).

**Proposed replacement** (`SECURITY.md:73-76`):
> - **Congestion control has no pacing** (ratified out of v1, §14.7): NewReno
>   backs off on loss (RFC 9002 recovery + PTO), but sends are not paced to
>   sub-RTT smoothness — a 12 KB initial window bounds bursts, but there is
>   no ECN and no alternate controller (CUBIC/BBR). Evaluate burstiness
>   before pointing it at the open internet at scale.

**Finding 2 — "allow-list gating" names a mechanism that does not exist.**
`SECURITY.md:59-61`:
> `- **Allow-list gating.** Inbound handshakes are accepted only from statics`
> `  on the caller-supplied allow-list; the caller owns that policy, including`
> `  revocation (a revoked static kills the session at the next timer scan).`

Same defect as README `Config::allow()` (§14(a) Finding 2 table): there is
no allow-list construct anywhere in `Config` (`src/config.rs:97-211`) or on
`Identity`. The actual mechanism is the staged-accept ladder: the
application inspects `Claimed::claimed_static()` and decides per-connection
whether to call `authenticate()` or drop the object (silent reject) —
`src/shell/staged.rs:243-284`. The revocation half of the claim ("kills the
session at the next timer scan") is not something a dropped `Claimed`
object does — revocation of an *established* connection is the
application's own responsibility (there is no timer that consults a
list slither doesn't hold).

**Proposed replacement** (`SECURITY.md:59-61`):
> - **Admission is application-driven, not a slither allow-list.** slither
>   holds no list of permitted statics. The staged accept ladder
>   (`Intro` → `Claimed` → `Proven` → `Connection`) hands the application
>   the claimed static after 1 DH; the application decides whether to
>   continue (`authenticate()`) or reject (drop the object — no bytes
>   sent). Revoking an established peer is the application's own job:
>   nothing here re-checks a list once a connection is up.

**Finding 3 — the zeroization claim is unverified and appears false.**
`SECURITY.md:62-66`:
> `- **Randomness.** Handshake ephemerals are drawn from a caller-supplied`
> `  CSPRNG via hiss; every handshake *retransmit* uses a fresh ephemeral (the`
> `  WireGuard requirement). The endpoint's index/jitter CSPRNG is a`
> `  `ChaCha20Rng` seeded from OS entropy (`getrandom`); the seed is zeroized`
> `  after use.`

The `ChaCha20Rng` claim checks out (`src/core/endpoint/mod.rs:68,153,188`
— `use rand_chacha::ChaCha20Rng`, seeded via `ChaCha20Rng::from_seed`) and
the OS-entropy claim checks out (`src/shell/endpoint.rs:511-513`,
`getrandom::fill(&mut seed)`). **"the seed is zeroized after use" does
not.** There is no `zeroize` dependency in `Cargo.toml` and no `Zeroize`/
`Zeroizing` usage anywhere in `src/` except one unrelated comment
explicitly stating the *opposite* for a different key
(`src/packet/mac.rs:44`, `"There is deliberately no `Zeroize` and no
`Drop`"` — about the mac1 key, not this seed). The `seed: [u8; 32]` at
`src/shell/endpoint.rs:510-513` is a plain stack array passed by value into
`CoreEndpoint::new(..., rng_seed)`, consumed by `ChaCha20Rng::from_seed`,
and then simply dropped — Rust does not zero stack memory on drop.

**Proposed replacement** (`SECURITY.md:62-66`):
> - **Randomness.** Handshake ephemerals are drawn from a caller-supplied
>   CSPRNG via hiss; every handshake *retransmit* uses a fresh ephemeral (the
>   WireGuard requirement). The endpoint's index/jitter CSPRNG is a
>   `ChaCha20Rng` seeded from OS entropy (`getrandom`). **The seed is not
>   currently zeroized after use** — it is a plain `[u8; 32]` on the stack,
>   dropped without an explicit wipe.

(This finding may itself prompt a small code fix — adding `zeroize` for
the seed — rather than only a doc correction; flagging it as a security
document telling readers something the code does not do is the sweep's
job, the fix decision is the maintainer's.)

Everything else in `SECURITY.md:1-58,68-72,77-84` (the reporting process,
"what a session guarantees," mac1-as-DoS-gate, 0-RTT timestamp payload,
roaming-trusts-the-seal, no-cookies/mac2, traffic analysis, no
persistence, no PSK) was checked against the same crypto/threat-model
ground already covered by CLAUDE.md's crypto rules and SPEC.md §§7/15/17
and found consistent — no further staleness found there.

---

**TODO.md** — read fully: `TODO.md:1-122`.

The whole file is the pre-rewrite planning brainstorm, dated 2026-08-13,
titled *"slither v0.2 plan — generalize and simplify."* Checked against
what actually shipped:

- §"Decisions taken" item 1 (IK stays, suite goes generic via a
  `channel!` macro) — **done**, matches shipped code
  (`macro_rules! channel` at `src/packet/suite.rs:219`, invoked at
  `src/packet/suite.rs:472`).
- Item 2's object-model diagram (`TODO.md:26-36`) — **superseded in its
  specifics**: it names `Reliable<Connection>` and `Ordered<Reliable<…>>`
  as wrapper types; neither exists in the shipped crate (`grep -rn
  "struct Reliable\|struct Ordered" src/` — no matches). The shipped
  design folds reliable messaging and stream ordering directly into
  `Connection`'s own methods (`send_message`, `open_bi`/`open_uni`, etc.
  — §14(a) above) rather than composable wrapper types.
- Item 3's staged-accept cost table (`TODO.md:45-50`) — **done, and
  accurately**: `Intro`/0 DH, `Claimed`/+1 DH, `Proven`/+1 DH,
  `Connection`/+2 DH matches `src/shell/staged.rs` exactly (§14(a)
  Finding 2 table).
- Item 6 ("Ordering: receiver-side `Ordered` wrapper") — **not built as
  described**; see the `Reliable`/`Ordered` non-existence above.
- Item 7 / the phase table (`TODO.md:84,90-95`, "Streams + congestion
  control: later milestone... separate round") — **overtaken**: streams
  and congestion control shipped as part of this same rewrite (Leg 2,
  `SPEC.md` §§9,14), not deferred to a later phase-4 round.
- "Standing context" (`TODO.md:117-122`) — **entirely obsolete**: it
  describes v0.1.0 sitting unpublished on `main` awaiting a GitHub push
  and `cargo publish`, a state from before this repository's current
  history even starts (`0e913d8`, "Initial commit: slither extracted from
  the bubble-reboot workspace" — already 172 commits and a full ratified
  rewrite ago).

**Every substantive line of TODO.md is superseded** by documents that now
exist and outrank it: `PLAN.md` ("Status: APPROVED 2026/08/14," ten
decisions ruled at rulings 53–59) is the realized version of TODO.md's
"Phases" table; `SPEC.md` (ratified) is the realized version of its
"Decisions taken"; `STORIES.md` is the realized version of its
capability goals; `.spec-v2-clean-slate/rulings.md` is the record of *why*
each decision landed where it did, including the several places the
shipped design diverges from TODO.md's sketch (no `Reliable`/`Ordered`
wrapper types). Nothing in `round41-material.md` or `rulings.md`
contradicts this — TODO.md is simply the document those two superseded.

**Recommendation: delete TODO.md outright.** It is not "stale in parts"
the way README/CHANGELOG/SECURITY.md are — it is a complete planning
artifact for work now finished, ratified, and implemented differently in
several details, and `PLAN.md`/`SPEC.md`/`STORIES.md`/`rulings.md`
already carry its content forward accurately. Keeping it invites a reader
to treat its `Reliable`/`Ordered`/phase-4-deferred-CC sketch as current,
which it is not.

### 14(d) In-code stale comments

**Group 1 — the proven-LIVE "still returns Stale" claim, confirmed stale
in all 4 named sites** (the brief's "+3 files" — verified as exactly
`staged.rs`, `tables.rs`, `guard.rs`). Checked against
`src/core/endpoint/staged.rs:606-724` (`accept()`'s LIVE match arm),
which fully implements the basis-timestamp comparison
(`Some(t) if timestamp > t => replacing = Some(live)` at line 637),
completes the replacement (`retire_replaced` + `EndpointOutput::Replaced`
at lines 718-724), and is exercised in production — the "later slice"
these four comments describe **has landed**.

1. `src/core/endpoint/mod.rs:34-41`:
   > `//! **LIVE is what remains.** §6.4's re-home walk and its proven-LIVE`
   > `//! replacement admission — the basis check, `ConnectionLost::Replaced`,`
   > `//! and §7.5's contested-connection probe — still return`
   > `//! [`AcceptError::Stale`] here. That is not correct in general: a`
   > `//! replacement whose timestamp passes both the guard and the basis must`
   > `//! succeed. It is a documented, deliberate boundary, and what it preserves`
   > `//! meanwhile is §16.1's **one session per peer static**, which returning`
   > `//! `Stale` cannot violate.`

   **Proposed replacement:**
   > `//! **LIVE is implemented.** §6.4's re-home walk and its proven-LIVE`
   > `//! replacement admission — the basis check, `ConnectionLost::Replaced`,`
   > `//! and §7.5's contested-connection probe — are all live`
   > `//! (`src/core/endpoint/staged.rs`'s `accept()`). A replacement whose`
   > `//! timestamp passes both the guard and the basis succeeds and installs;`
   > `//! §16.1's **one session per peer static** is preserved by the basis`
   > `//! check, not by a blanket refusal.`

2. `src/core/endpoint/staged.rs:568-571`:
   > `/// * **LIVE** — still [`AcceptError::Stale`], and that one is a knowing`
   > `///   boundary: §6.4's re-home walk and its proven-LIVE replacement`
   > `///   admission are a later slice. What `Stale` preserves meanwhile is`
   > `///   §16.1's one-session-per-peer invariant, which it cannot violate.`

   **Proposed replacement:**
   > `/// * **LIVE** — admitted when the candidate's timestamp exceeds the`
   > `///   row's `replacement_basis` (see `accept()` below); refused as`
   > `///   [`AcceptError::Stale`] otherwise. §16.1's one-session-per-peer`
   > `///   invariant holds either way — the basis check is what makes the`
   > `///   admission sound, not the refusal.`

3. `src/core/endpoint/tables.rs:16-19` (module doc):
   > `//! §6.4's admission is its **only** reader. Its proven-LIVE replacement`
   > `//! admission is still a later slice; §6.4's **PENDING** branch and §6.6's`
   > `//! internal tie-break landed with ruling 91, and the latter is why`
   > `//! [`StaticMap::promote`] takes the basis as an argument rather than`
   > `//! leaving whatever the dialled row was born with.`

   **Proposed replacement:**
   > `//! §6.4's admission is its **only** reader. Its proven-LIVE replacement`
   > `//! admission (the basis-timestamp comparison in `staged.rs`'s`
   > `//! `accept()`) reads it directly; §6.4's **PENDING** branch and §6.6's`
   > `//! internal tie-break landed with ruling 91, and the latter is why`
   > `//! [`StaticMap::promote`] takes the basis as an argument rather than`
   > `//! leaving whatever the dialled row was born with.`

4. `src/core/endpoint/guard.rs:10-15` (module doc):
   > `//! # Every write site is post-`ss``
   > `//!`
   > `//! §17.1 has four write sites and all four sit **after** the proving `ss`,`
   > `//! so **only a key-holder can write a guard entry**. Three are implemented:`
   > `//! `authenticate()` on the staged path, §6.6 step 4's tie-break admit, and`
   > `//! §6.7's winner-side record (by either of §6.6's two routes). §6.4's`
   > `//! **re-homed** candidate admission is the one still outstanding.`

   **Proposed replacement** (drop "still outstanding," mark it implemented):
   > `//! §17.1 has four write sites and all four sit **after** the proving `ss`,`
   > `//! so **only a key-holder can write a guard entry**: `authenticate()` on`
   > `//! the staged path, §6.6 step 4's tie-break admit, §6.7's winner-side`
   > `//! record (by either of §6.6's two routes), and §6.4's re-homed`
   > `//! candidate admission (`staged.rs`'s `accept()` LIVE arm).`

**Group 2 — `Cargo.toml:172`.** Current text:
> `# `Service` over the message verb (feature `tower`).`

Confirmed stale against `src/compat/tower.rs:1-14` and ruling 225,
recorded there in full: *"slither has **no request/response correlation
on the wire**, so a `Service` over §9.8's message verb would need a
request id the transport does not carry... There is **no `Rpc`**
(ruling 230)."* The shipped shape is exactly the opposite of what the
comment says — `call()` opens **one bi stream per call**, not a message.
Same line number as it held at the 2026-08-17 survey's base commit
(`6448e6f`); the file has grown below it (ruling 251's dev-dependency
block) but this specific line is unchanged and was already wrong then.

**Proposed replacement** (`Cargo.toml:172`):
> `# `tower_service::Service` over one bi stream per call — not the message`
> `# verb (ruling 225 rejected that shape; no request id on the wire).`

**Group 3 — three more stale absence comments found by sweeping `grep -rn
"slice " src --include="*.rs"` (166 non-test hits) and verifying each
candidate against current code**, beyond the four the brief named:

5. `src/shell/connection.rs:531-538` (doc comment on the **public**
   `open_bi()`):
   > `/// # In slice 4 an exhausted bidi space parks for ever`
   > `///`
   > `/// A bidi index is returned to the peer's allowance only when **both**`
   > `/// halves are freed, and the send half is freed on acknowledgement —`
   > `/// which needs §12's ACK processing, which this slice does not have. So`
   > `/// once the bidi limit is reached, `open_bi` never resumes. `open_uni``
   > `/// has no such gap: a peer-opened uni stream read to end-of-stream is`
   > `/// fully closed at once and grants its MAX_STREAMS_UNI.`

   Verified stale: §12 ACK processing shipped in slice 5a (`59ea084`,
   "Slice 5a: §12 ACK, §13 loss recovery, §14 NewReno (core)"). The send
   half **is** freed on acknowledgement today —
   `src/core/connection/streams.rs:608-625`'s `on_ack_range`: when
   `send.is_terminal()` it sets `stream.send = None` and calls
   `after_half_freed`; it is wired from the connection core at
   `src/core/connection/mod.rs:1167-1175,1466`. This is a **public API
   doc** describing a bug that appears fixed — highest-priority item in
   this whole sub-item, since a consumer reading rustdoc for `open_bi`
   today is told it can wedge forever when it (apparently) no longer can.

   **Proposed replacement** (`src/shell/connection.rs:531-538`):
   > `/// # A freed bidi index returns the peer's allowance`
   > `///`
   > `/// A bidi index is returned to the peer's allowance only when **both**`
   > `/// halves are freed, and the send half is freed on acknowledgement`
   > `/// (§12's ACK processing, `Streams::on_ack_range`). `open_uni` has no`
   > `/// such dependency: a peer-opened uni stream read to end-of-stream is`
   > `/// fully closed at once and grants its MAX_STREAMS_UNI.`

   **This is a doc-only fix candidate, not a verified behavior fix** — the
   sweep confirms the freeing *mechanism* exists and is wired, but did not
   run a paused-clock test exhausting the bidi limit and confirming
   `open_bi` resumes; the maintainer should either point at an existing
   test that covers this or add one before treating the behavior itself as
   confirmed (this agent is read-only and did not run `cargo test`).

6. `src/core/connection/mod.rs:3402-3405` (doc comment on `ConnOutput`):
   > `/// The variants §16.4 lists that this slice cannot yet construct — every`
   > `/// `ConnEvent` but `Established` and `Closed` — are absent rather than`
   > `/// stubbed: an uninhabited variant is a claim about the protocol, and these`
   > `/// will each arrive with the section that defines them.`

   Verified stale: the `ConnEvent` enum immediately below (`mod.rs:3418
   onward`) already defines and constructs `StreamOpened`,
   `StreamsAvailable`, `StreamReadable`, `StreamWritable`, `StreamFinished`
   and more — nowhere near "only `Established` and `Closed`."

   **Proposed replacement** (`src/core/connection/mod.rs:3402-3405`):
   > `/// One item of the connection core's drain. §16.4.`
   (drop the "this slice cannot yet construct" paragraph entirely — every
   `ConnEvent` variant §16.4 lists is now constructed somewhere in this
   file; if a residual gap exists the maintainer should name it, not this
   agent, since confirming a *negative* — that nothing is still missing —
   needs a full enum-vs-spec cross-check this sweep did not do.)

7. `src/core/connection/testfix.rs:182-186` (doc comment on the test
   fixture `parse_frames`):
   > `/// Decode a whole plaintext frame stream (§8.3).`
   > `///`
   > `/// Panics, with the offending type byte named, on a frame type slice 4`
   > `/// cannot legitimately emit. That panic is itself an assertion: a core`
   > `/// emitting an ACK in slice 4 has crossed the slice boundary.`

   Verified stale, and self-refuted by the function's own body: ACK is
   explicitly parsed today, not panicked on —
   `src/core/connection/testfix.rs:260-275`, `t if t ==
   crate::constants::FRAME_ACK => { ... out.push(Wire::Ack { .. }); }`.
   The comment immediately above that arm (`testfix.rs:276-278`) even
   names the pattern: *"the third instance of this file's own aged-out-
   decoder problem, after `Ack` (slice 5) and `Datagram` (slice 6)"* — the
   function's maintainers already know this doc comment is the fourth
   instance of the same problem; it was simply never revisited.

   **Proposed replacement** (`src/core/connection/testfix.rs:182-186`):
   > `/// Decode a whole plaintext frame stream (§8.3).`
   > `///`
   > `/// Panics, with the offending type byte named, on a frame type this`
   > `/// fixture does not yet decode — a signal to extend this function, not`
   > `/// a claim about what the core may emit (every ratified frame type is`
   > `/// legitimate here; see the match arms below for what is currently`
   > `/// wired up).`

**Group 4 — one more, module-doc-level (`src/shell/connection.rs:1-8`):**
> `//! §16.2's `Connection` handle — the subset slice 3 builds.`
> `//!`
> `//! `close()`, ruling 46's `closed()`, the four accessors, slice 4's stream`
> `//! verbs, slice 5's `acked()`, slice 6's four sugar verbs, and slice 7's`
> `//! `notified()`, `set_persistent_keepalive()` and `persistent_keepalive()`.`
> `//! §16.2's surface is now complete; what remains for slice 8 is the`
> `//! composability layer (`AsyncRead`/`AsyncWrite`, the `Sink`/`Stream``
> `//! adapters), which wraps these verbs rather than adding to them.`

The title line ("the subset slice 3 builds") contradicts the body's own
"§16.2's surface is now complete," and "what remains for slice 8" is
stale since slice 8 shipped (`e2c339c`, "Slice 8: compat/ integrated —
1041 tests, all nine gates green"; `src/compat/` exists and is populated).

**Proposed replacement** (`src/shell/connection.rs:1-8`):
> `//! §16.2's `Connection` handle.`
> `//!`
> `//! `close()`, `closed()`, the four accessors, the stream verbs,`
> `//! `acked()`, the four sugar verbs (§9.8, §11), `notified()`,`
> `//! `set_persistent_keepalive()` and `persistent_keepalive()`. §16.2's`
> `//! surface is complete; the composability layer`
> `//! (`AsyncRead`/`AsyncWrite`, `Sink`/`Stream`, `tower::Service`) wraps`
> `//! these verbs from `src/compat/` rather than adding to them.`

**Negative findings — checked and NOT stale**, kept here so the next
sweep does not re-open them: `src/core/connection/recovery.rs`'s
`path_gen` prose (ruling 137's "discharged by slice 7" comment at
`recovery.rs:219-224`, and ruling 172's two fence comments at
`recovery.rs:324-332,645-653`, all read as accurate, live descriptions
of an implemented mechanism — **round41-material.md item 14's "`recovery.rs`
path_gen prose" claim did not reproduce under inspection at `94dab20`**;
either it was already fixed by ruling 172's work or the surveyed location
differs from what this agent checked); `src/core/connection/recv.rs:93`'s
`earns_stream_credit` claim (verified: the field is set to `true` at
construction with no other write site, `recv.rs:113,347` — still
accurate); `src/core/endpoint/routing.rs:1355-1362` (already
self-corrected in place — *"It is not a later slice any more"* — a good
model for how the other stale comments above should read once fixed);
`src/core/connection/mod.rs:3459-3463`, `frame.rs:18-28,150-162,1469-1471`,
`src/shell/connection.rs:1219`, `src/shell/shared.rs:428`,
`src/shell/wire.rs:50-59`, `src/packet/golden_vectors.rs:76-80` — all
correctly scoped historical narration ("slice N built X," "slice N-M add
parse arms") or forward-looking design rationale that remains true
regardless of elapsed time, not live-absence claims.

**Total for 14(d): 8 confirmed-stale comment sites** (4 in Group 1 sharing
one root cause, 1 in Group 2, 2 in Group 3, 1 in Group 4) **against the
brief's "~20" estimate.** This sweep covered every "slice "-mentioning
line outside test files (166 hits) with a progressively broadened filter
and individually verified each surviving candidate against the current
code rather than counting greps; it did not open every one of the 106
`tests_*.rs`/`tests.rs` hits (test-file comments are lower-stakes and the
brief's examples are all non-test), so a second pass limited to test
files could still find more, but is unlikely to find the same
consumer-facing severity as Group 3, item 5 (`open_bi`'s public doc).

### 14(e) CLAUDE.md rule 13's fixture claim — DRAFT-FOR-RULING

**Verified: `send_failure` exists in `testutil` and is exactly the fault
class rule 13 says cannot be expressed.**

- `src/testutil/mod.rs:203`: `FlakyPolicy.send_failure: Option<SendFailure>`.
- `src/testutil/mod.rs:160`: `pub struct SendFailure` — "Fail `send_to`
  while `Instant::now()` is inside the window."
- `src/testutil/mod.rs:1059`: a comment ties it directly to the ruling —
  *"[`FlakyPolicy`] mid-run — which is what ruling 49's send-failure..."*
  — confirming the brief's "since ruling 49" claim.
- `src/testutil/mod.rs:1755`: `async fn
  send_failure_is_an_err_and_then_heals()` — a live test exercising it.
- `FlakyPolicy::fail_sends()` (`src/testutil/mod.rs:226` area) — "Turn
  `ENETUNREACH` on every `send_to` on or off, **now**" — a mid-run toggle,
  shared via `Rc<Cell<bool>>` across clones.

**Checked the other half of the claim too (not just the changed token —
working rule 4):** `"this driver panics"` still has no fixture mechanism.
`grep -n "panic" src/testutil/mod.rs` finds only unrelated panics (a
`LocalSet` requirement panic, a rebind-onto-a-registered-address panic —
both pre-existing invariant checks, not fault-injection for "the driver
under test panics"). So the claim is **half stale, half still true** —
the amendment must narrow the sentence, not delete it.

**Current text (CLAUDE.md, working rule 13, the load-bearing clause):**
> `because `FlakyWire` models everything a *network* does and nothing a`
> `*socket* does: a fabric that loses, delays, duplicates and reorders`
> `cannot express "this send fails" or "this driver panics". No amount of`
> `test-writing against it would have found them.`

**Draft amended sentence** (DRAFT-FOR-RULING — CLAUDE.md amendments need a
ruling per the project's own working rules; this agent is not making the
change, only drafting it for maintainer review):
> `because `FlakyWire` models everything a *network* does and nothing a`
> `*socket* does: a fabric that loses, delays, duplicates and reorders`
> `could not, at review time, express "this send fails" or "this driver`
> `panics". Ruling 49 later added `FlakyPolicy::send_failure` (`ENETUNREACH``
> `injection, toggleable mid-run), closing the first gap; "this driver`
> `panics" remains unexpressed today. No amount of test-writing against the`
> `fixture as it stood would have found either at the time.`

The rest of rule 13 (the "two of the four seam-review findings were
unreachable... suspect the harness before the authors" framing) does not
need to change — it is a historical claim about what happened during that
review, which remains true; only the present-tense "cannot express"
clause needs the past/present split above.

### 14(f) `Identity` trait absent from SPEC.md — DRAFT-FOR-RULING

**Verified: `grep -n Identity SPEC.md` returns 6 lines, none of them a
trait definition** — `SPEC.md:564,5821,5828,5832,6796,6912`. All six use
`Identity` only as a generic bound (`I: Identity`, `core::Endpoint<I:
Identity>`) or refer informally to `Identity::open()` failing
(`SPEC.md:6796,6912`, in the error-mapping table). None states what the
trait requires.

The trait itself, read at `src/identity.rs:89-118`:
```rust
pub trait Identity {
    type Suite: Handshake;
    type Provider: DhProvider<CurveOf<Self>>;
    type Error: ::core::error::Error + 'static;
    fn public_static(&self) -> &PublicKeyOf<Self>;
    fn open(&self) -> Result<(Self::Provider, PrivateKeyOf<Self>), Self::Error>;
}
```
Its own doc comments cite §2.4 (`public_static`, "the canonical static
public key") and §17.5 (`open`'s laziness — "never at park... an enclave
static would otherwise hold up to 1024 concurrent provider handles for
packets nobody has looked at yet").

**Where it does *not* belong:** §2.4 (`SPEC.md:601-615`) only specifies
the wire-level canonical **encoding** of a static public key
(`AsRef<[u8]>` octets) — it has no Rust API content and is the wrong
place for a trait signature.

**Where a mention belongs:** `### 16.4 The two cores and the poll
contract` (`SPEC.md:5817`, headers listed via `grep -n "^### 16\." SPEC.md`
— §§16.1 object model, 16.2 shell surface, 16.3 driver/handle lifetimes,
16.4 the two cores, 16.5 time/timers, 16.6 RNG, 16.7
plan-seal-commit, 16.8 no-blocking, 16.9 early sends, 16.10 kernel-free
drivability, 16.11 composability). §16.4 already introduces `I: Identity`
as `core::Endpoint`'s generic parameter and gives its method surface in a
```rust ... ``` block (`SPEC.md:5832` onward) — it is the section that
should also state what `Identity` requires, since it is the one section
that currently *uses* the bound without ever defining it. This is rule
8's shape from CLAUDE.md: a symbol is introduced and the text never says
what bounds it.

**Draft addition** (DRAFT-FOR-RULING — a SPEC.md change needs a
ratification decision recorded in `rulings.md`, not a doc-sweep edit; this
agent proposes text only):

Insert after the `impl<I: Identity> core::Endpoint<I> { ... }` code block
in §16.4, as a new paragraph:

> **The `Identity` seam.** `I: Identity` is the static-key and DH-provider
> abstraction (§2.4's canonical encoding is what `public_static()`
> returns the octets of; §17.5 is why `open()` is lazy):
>
> ```rust
> pub trait Identity {
>     type Suite: Handshake;
>     type Provider: DhProvider<CurveOf<Self>>;
>     type Error: core::error::Error + 'static;
>     fn public_static(&self) -> &PublicKeyOf<Self>;
>     fn open(&self) -> Result<(Self::Provider, PrivateKeyOf<Self>), Self::Error>;
> }
> ```
>
> `open()` mints the provider and static-key handle for **one** handshake,
> called lazily — at `read_identity()` on the responder path, and once per
> attempt on the initiator path — never at park, so an enclave-backed
> static does not hold up to `INTRO_QUEUE_CAP` concurrent provider handles
> for introductions nobody has inspected yet. No `Send` bound appears
> anywhere on `Provider`, deliberately (§16.3, S21).

Alternative placement the maintainer may prefer instead: a new §16.1
subsection (the object model already names the actors) rather than
folding it into §16.4's core-API block — this agent has a mild preference
for §16.4 because that is where the bound is first *used* in the spec
text, but the choice is the maintainer's.

### 14(g) .claude/worktrees leftovers

Per the task's hard rule, this agent did not enter or list
`.claude/worktrees/`. `round41-material.md:79-80` records that this item
"grown this round" (round 40) — i.e. it is known to the maintainer already
and is not a fresh discovery. Noting only, as instructed: the directory
exists (visible in this session's own working-directory listing as
`/Users/nicolasdiprima/work/primetype/slither/.claude/worktrees/agent-ac999434a8102c68a`,
an *additional working directory* supplied by the harness for this very
session — i.e. at least one worktree is live right now, for this task).
Cleanup is explicitly the orchestrator's responsibility, not this
inventory agent's, and no action was taken here.

---

## Item 10 — ruling 253 addendum

**Ruling 253's relevant paragraph**, read via `grep -n "^### 253"
.spec-v2-clean-slate/rulings.md` (`:7523`) and reading only that ruling's
text (`:7523-7583`). The load-bearing sentence:

> `The overlap policy — `recv.rs:566` claims *"stored bytes win"* — is`
> `relaxed to §9.5's "either" (small-to-large can invert which copy`
> `survives; `tests_reassembly.rs:191–196` already uses equal bytes so as`
> `not to lean on it) **[S-71]**.`

**The landed code doc**, `grep -n "survives\|non-guarantee" src/core/connection/recv.rs`,
read at `src/core/connection/recv.rs:39-50`:

> `//! **[ruling 253]** Which copy survives is **not a promise this module`
> `//! makes**. §9.5's "either" is the whole rule, and a merge that is free to`
> `//! keep whichever side is cheaper to keep is exactly what the small-to-large`
> `//! discipline needs. As it happens this build keeps the **stored** copy —`
> `//! the merge writes the arriving frame only where no stored chunk already`
> `//! holds the byte, which is both the first-copy-wins answer and the cheap`
> `//! one — but nothing may depend on that, and a merge that reversed it would`
> `//! still be conformant.`

**The gap:** ruling 253 states "small-to-large can invert which copy
survives" as if that is a description of the shipped merge's *behavior*.
It is not — the code doc is explicit that **this build always keeps the
stored copy** (deterministic, first-copy-wins), and that the relaxation
to §9.5's "either" is a *specification permission* (nothing may rely on
today's determinism continuing), not an observed or intended inversion.
"Can invert" describes a hypothetical alternative small-to-large
implementation the ruling was reasoning about in the abstract, not the one
that landed. The ruling's **conclusion** — the overlap policy is relaxed
from "stored bytes always win" to §9.5's "either," so nothing may depend
on which copy wins — is correct and unaffected; only the mechanism
description is wrong, matching round41-material.md item 10's own framing
("Cosmetic unless someone re-reads 253 as describing the code").

**House style for the addendum**, per the existing precedent found via
`grep -n "Addendum" .spec-v2-clean-slate/rulings.md` (one freestanding
`**Addendum to ruling 123 — ...**` section at `:3307`, placed in a
different round's narrative rather than inline) **and** the inline
bracketed dated-annotation convention used throughout the document for
corrections made in place — e.g. ruling 253's own `**[Discharged at slice
R40-E, 2026/08/17, with a stronger answer than the obligation asked
for: ...]**` a few lines below the target sentence, and
round41-material.md's `**[RESOLVED 2026/08/18 — ruling 256: ...]**`. The
inline bracketed form is the better fit here: it is a small mechanism
correction to one clause of one ruling, not a new argument deserving its
own narrative section.

**Draft addendum** (to append immediately after the target sentence,
inside ruling 253's "What the rewrite must preserve" paragraph,
`.spec-v2-clean-slate/rulings.md:7566-7567`):

> `The overlap policy — `recv.rs:566` claims *"stored bytes win"* — is`
> `relaxed to §9.5's "either" (small-to-large can invert which copy`
> `survives; `tests_reassembly.rs:191–196` already uses equal bytes so as`
> `not to lean on it) **[S-71]**. **[Addendum 2026/08/18 — the landed merge`
> `does not invert.** The code doc at `recv.rs:44-50` states the landed`
> `merge is deterministic and always keeps the **stored** copy — the`
> `arriving frame is written only where no stored chunk already holds the`
> `byte, which is both first-copy-wins and the cheap side to keep under`
> `small-to-large. "Can invert" described a hypothetical alternative`
> `small-to-large merge under consideration at ruling time, not the one`
> `that shipped. The **conclusion stands**: the relaxation to §9.5's`
> `"either" is a specification permission, so nothing may depend on`
> `today's determinism — a future merge that did invert would still be`
> `conformant — but as shipped, it does not.]**`

This is the addendum only; landing it is the maintainer's call (rulings.md
edits need the same ratification discipline as any other spec-record
change, and this agent is read-only).

---

## Item 12 — testing conventions (R40-D author)

Source wording, `round41-material.md:65-68`: *"Testing conventions worth
writing down where authors look: session ids are wall-clock-fed — never
golden-pin one; the `Tap` records **before** the loss draw — a tapped
datagram is not a delivered one (both from R40-D's author)."*

### Convention 1 — session ids are wall-clock-fed

**Verified, and not documented anywhere today.**

- `src/core/endpoint/mod.rs:348-361`, `draw_timestamp()`: *"§5.3's
  initiation timestamp: the wall clock, **forced strictly greater** than
  the last this endpoint emitted."* It reads `self.config.clock().now()`
  (`:356`) — `Config`'s `WallClock` (`src/config.rs:31-40`), which
  defaults to `SystemClock` — real `SystemTime` — unless a test supplies
  its own via `Config::with_clock` (`src/config.rs:190`). This is a
  **separate clock** from the paused **virtual** `Instant` every timer in
  the crate runs on (`SPEC.md`/CLAUDE.md's "tokio's paused clock" — the
  timestamp is §5.3's wire value, not a timer deadline).
- `src/shell/connection.rs:1185-1189`, `Connection::session_id()`:
  *"hiss derives it from the handshake hash"* — the handshake hash covers
  msg1's payload, and msg1's payload **is** the wall-clock-sourced
  initiation timestamp (§5.1/§5.3, README's own "a strictly-greater
  timestamp, carried encrypted as msg1's Noise payload").
- Net effect: `SessionId` depends on real wall-clock time by default, even
  under `Network::seeded` + tokio's paused **virtual** clock + a fixed RNG
  seed — none of which touch `Config`'s `WallClock`. A test that captures
  a `SessionId`'s bytes today and asserts them as a golden constant will
  be flaky from the next run onward, unless it explicitly injects a fixed
  `Config::with_clock`.
- Cross-checked against `src/testutil/mod.rs:32`'s determinism claim —
  *"No wall clock, no `SystemTime`... anywhere in a decision path"* — this
  is scoped to the **fabric's** loss/delay/duplicate decisions, not to
  every byte the protocol under test produces. It does not cover
  `SessionId`, and a reader could easily believe it does since it is the
  crate's one blanket "no wall clock" promise.

**Draft convention text:**
> **Never golden-pin a `SessionId`.** It derives from the handshake hash,
> which covers §5.3's initiation timestamp — real wall-clock time by
> default (`Config`'s `WallClock`, `SystemClock` unless overridden via
> `Config::with_clock`). `Network::seeded`, the paused virtual clock and a
> fixed RNG seed do not touch it. A test may assert that two peers' session
> ids are **equal to each other**, or that a rekey changes it — never that
> it equals a fixed byte sequence.

### Convention 2 — the `Tap` records before the loss draw

**Verified, and already documented at exactly the right place** —
`src/testutil/mod.rs:105-111`, on the `Tap` struct itself:
> `/// "Accepted" means the send was not refused by an injected send failure`
> `/// and was not blackholed by a partition — it is what left the wire, which`
> `/// is a different question from what arrived. Loss and duplication are`
> `/// applied *after* the tap.`

This already states the convention precisely (loss/duplication applied
*after* the tap is recorded, i.e. a tapped datagram may still be lost and
never delivered), confirmed against the tests that exercise it —
`src/testutil/mod.rs:1725` (`"a blackholed send is not tapped"`, a
*different* mechanism from probabilistic loss — a partition refuses the
send outright, before any tap) and `:1787` (`"only the healed send was
tapped"`). No gap found here: **this convention does not need writing
down, it needs nothing** — round41-material.md's author likely flagged it
because it is easy to miss on a first read (or because, at the time it
was written down as an open item, this doc comment did not yet exist and
has since been added — this agent did not check `git blame` on the exact
lines to confirm which). Recommend closing this half of item 12 as
already satisfied rather than drafting new text for it.

### Placement recommendation

`src/testutil/mod.rs`'s module doc — the `# Determinism is a MUST`
section (`:17-38`) — is where existing conventions of this shape already
live (the `Network::seeded`/no-OS-entropy rule, the one-RNG-per-wire rule,
the "prefer `drop_at`/`drop_first` over probabilistic loss" rule are all
there), and it is the file `testutil`'s own doc calls "normative surface,
not a test convention" (`:3`) — i.e. authors are told to read it as
binding. But convention 2 shows the project's actual practice is
**dual placement**: the general determinism section states the blanket
rule, and the specific struct (`Tap`) carries its own precise caveat at
the point of use — the same pattern CLAUDE.md's "six documentation
obligations" already uses ("each hazard...stated here as well as at its
call site").

**Recommend the same for convention 1:**
1. A short caveat added to `src/testutil/mod.rs`'s `# Determinism is a
   MUST` section, immediately after the existing "No wall clock..." bullet
   (`:32`), narrowing its scope: *"...anywhere in a decision path — this
   does not cover `Config`'s `WallClock` (default `SystemClock`), which
   feeds §5.3's initiation timestamp and therefore `SessionId`; see
   `Connection::session_id`'s doc."*
2. The full convention text (drafted above) on `Connection::session_id`'s
   own doc comment (`src/shell/connection.rs:1185-1197`), which is where
   a test author reaching for a session id to assert against would
   actually be looking — mirroring how `Tap`'s doc comment carries its own
   convention rather than relying solely on the module header.

**Not CLAUDE.md** — this is test-fixture documentation, not a
project-wide working rule; it belongs in the code the tests already read,
not in a document CLAUDE.md's own rule 4(b) says "no slice ever puts on
an agent's path." No ruling is needed for either half.

---

## SUMMARY

**Ready-to-apply** (verified against current code, no ratification
needed — ordinary doc/comment corrections):

| # | Location | What |
|---|---|---|
| 1 | `README.md` — Lineage bullet | congestion control / streams / fragmentation are shipped, not reserved |
| 2 | `README.md` — Usage sketch | whole section rewritten against the real API; dangling `examples/udp_loopback.rs` link |
| 3 | `README.md` — timer sentence (protocol paragraph) | dead timeout 25s not 15s; rekey is message-count (2¹⁶), not 120s/180s |
| 4 | `README.md` — timer sentence (Testability) | same timer fix |
| 5 | `CHANGELOG.md` | new `[0.2.0]` entry drafted; disposition choice offered for the stale `[Unreleased]` block |
| 6 | `SECURITY.md` — congestion control bullet | NewReno is shipped; only pacing/ECN/alt-controllers are out |
| 7 | `SECURITY.md` — allow-list bullet | no `Config` allow-list exists; corrected to the staged-ladder mechanism |
| 8 | `SECURITY.md` — randomness bullet | "seed is zeroized" is false; no `zeroize` dependency exists |
| 9 | `TODO.md` | recommend deletion outright — fully superseded by `PLAN.md`/`SPEC.md`/`STORIES.md`/`rulings.md` |
| 10–13 | `src/core/endpoint/{mod,staged,tables,guard}.rs` | the proven-LIVE "still returns Stale" claim, stale in all 4 sites (§6.4 LIVE is implemented) |
| 14 | `Cargo.toml:172` | tower comment contradicts ruling 225 (stream-based, not message-verb) |
| 15 | `src/shell/connection.rs:531-538` | `open_bi` public doc describes a slice-4 bug fixed since slice 5a |
| 16 | `src/core/connection/mod.rs:3402-3405` | `ConnOutput` doc understates `ConnEvent`'s variant count |
| 17 | `src/core/connection/testfix.rs:182-186` | fixture doc says ACK panics; code parses it |
| 18 | `src/shell/connection.rs:1-8` | module doc title/"remains for slice 8" stale since compat/ shipped |
| 19 | `.spec-v2-clean-slate/rulings.md` ruling 253 | addendum correcting "can invert" to "always keeps stored" |
| 20 | `src/testutil/mod.rs` + `src/shell/connection.rs` | new convention: never golden-pin a `SessionId` (wall-clock-fed) |

**DRAFT-FOR-RULING** (changes to CLAUDE.md or SPEC.md — both require a
ratification decision per the project's own rules, not a doc sweep):

| # | Location | What |
|---|---|---|
| 21 | `CLAUDE.md` working rule 13 | fixture claim half-stale: `send_failure` (ruling 49) now expresses "this send fails"; "this driver panics" remains unexpressed |
| 22 | `SPEC.md` §16.4 | `Identity` trait used throughout as a bound, never defined; draft text proposed for insertion after the `core::Endpoint<I>` code block |

**No action needed** (checked, found accurate or already fixed): the
`Tap`-records-before-loss-draw convention (already documented at
`src/testutil/mod.rs:105-111`); `recovery.rs`'s `path_gen` prose (ruling
137/172 comments read as accurate); `recv.rs:93`'s `earns_stream_credit`
claim; `routing.rs:1355-1362` (already self-corrected — a good model);
several other "slice N" comments that are correctly-scoped historical
narration rather than live-absence claims.

**Total: 22 distinct proposed edits across 11 files** (20 ready-to-apply,
2 draft-for-ruling), plus one confirmed-adequate item and roughly a dozen
explicitly-checked-and-rejected candidates recorded so the next sweep does
not re-open them.
