# slither v0.2 — actor responsibility inventory (research for the SPEC v2 rewrite)

Read: `SPEC.md` (550 lines, in full), `src/endpoint.rs` (1093, in full), `src/session.rs`,
`src/recovery.rs`, `src/handshake.rs`, `src/frame.rs`, `src/wire.rs`, `src/mac.rs`,
`src/flow.rs`, `src/flow_frames.rs`, `src/testutil.rs`, `src/lib.rs`, `examples/udp_loopback.rs`,
`CLAUDE.md`, `Cargo.toml`, `TODO.md`. All paths below are relative to
`/Users/nicolasdiprima/work/primetype/slither`.

---

## 1. Actor responsibilities inventory

### 1.0 The task, the loop, and its shape

| # | Duty | Where |
|---|---|---|
| 1.0.1 | One `!Send` actor per `Endpoint`, spawned with `tokio::task::spawn_local`; consumer must supply a `LocalSet` | `src/endpoint.rs:248-279`, spawn at `:273` |
| 1.0.2 | `tokio::select!` over three arms: command channel, `Wire::recv_from`, 250 ms tick | `src/endpoint.rs:449-470` |
| 1.0.3 | Actor terminates when the command channel closes (every `Endpoint` **and** `SessionHandle` dropped) | `src/endpoint.rs:451-455` |
| 1.0.4 | Owns a single 2048-byte receive buffer; each datagram is copied to a `Vec` before dispatch | `src/endpoint.rs:448`, `:460` |
| 1.0.5 | Oversize receive (`> MAX_DATAGRAM` = 1200) → silent drop | `src/endpoint.rs:682-684` (SPEC §3 "Sizes and caps") |
| 1.0.6 | Packet classification (`type`,`version`) → Init / Resp / Data; everything else silent drop | `src/endpoint.rs:685-690` → `src/wire.rs:319-332` (SPEC §2) |
| 1.0.7 | Outbox flushing: sequential `await`ed `send_to`, errors traced and swallowed | `src/endpoint.rs:477-483` |
| 1.0.8 | Event emission into an **unbounded** mpsc; send failure ignored | `src/endpoint.rs:473-475`, channel at `:254` |
| 1.0.9 | Owns the `Wire` trait seam (real `UdpSocket` or `testutil::FlakyWire`) | `src/endpoint.rs:165-180` |
| 1.0.10 | Owns one endpoint-wide `ChaCha20Rng` used for **both** session-index minting and retransmit jitter | `src/endpoint.rs:420`; seeded `src/endpoint.rs:195-211` |

### 1.1 Timers (every one evaluated on the 250 ms scan)

| Timer | Value | Fires what | Code | SPEC |
|---|---|---|---|---|
| `TICK` | 250 ms, `MissedTickBehavior::Delay` | the whole timer scan; the granularity bound for every deadline below | `src/endpoint.rs:60`, `:446-447`, `:468` | §9.4 ("Both timers are evaluated on the actor's 250 ms TICK") — **not** in the §6 table |
| `RETRANSMIT_BASE` | 5 s | arms the next fresh initiation | `src/endpoint.rs:51`, armed `:561/:570`, `:1014/:1020`, fired `:931` | §5 Initiator 2, §6 table |
| `RETRANSMIT_JITTER_MAX` | 333 ms | uniform jitter added to the base, drawn from the actor RNG | `src/endpoint.rs:53`, `:485-489` | §5 Initiator 2, §6 table |
| `HANDSHAKE_GIVEUP` | 90 s | measured from `PendingConnect::started_at`; drops the pending, emits `Failed` (initial connect only) | `src/endpoint.rs:55`, `:929-930`, `:1025-1037` | §5 Initiator 4, §6 table, §7 (`Failed` is the added fifth event) |
| `KEEPALIVE_TIMEOUT` | 10 s | passive keepalive: `last_recv > last_send` and 10 s since `last_recv` ⇒ seal empty Data | `src/session.rs:28`, `:323-329`; driven `src/endpoint.rs:919-923` | §6 timers + "Keepalive is passive" |
| `DEAD_TIMEOUT` | 15 s | `last_send > last_recv` and 15 s **since the send** ⇒ session dead | `src/session.rs:30`, `:339-342`; driven `src/endpoint.rs:915-918` | §6 timers + "Liveness … measured from the last send" |
| `PERSISTENT_KEEPALIVE` | 25 s (constant), opt-in | endpoint-wide `Option<Duration>` from `Config`; seals a keepalive every interval since `last_send` | const `src/session.rs:32`; config `src/endpoint.rs:188`,`:220-223`; consulted `src/session.rs:326-327` via `src/endpoint.rs:919` | §6 timers |
| `REKEY_AGE` | 120 s | on a **payload-seal** path only: starts a fresh handshake under the same `ConnId` | `src/session.rs:34`, `:351-353`; `src/endpoint.rs:602-610` | §6 + 2026/07/17 amendment |
| `REJECT_AGE` | 180 s | payload-seal backstop: teardown + `Dead` | `src/session.rs:40`, `:363-365`; `src/endpoint.rs:596-599` | §6 amendment |
| RFC 9002 loss timer | `9/8 · max(smoothed_rtt, latest_rtt)`, floored at `K_GRANULARITY` 1 ms | `detect_lost` → re-queue frames | `src/recovery.rs:139-142`, `:462-500`; driven `src/endpoint.rs:966-968` | §9.4 |
| PTO | `smoothed_rtt + max(4·rttvar, 1 ms) + MAX_ACK_DELAY`, `×2^pto_count`, cap `2^6` | probe: oldest undelivered message, else PING | `src/recovery.rs:144-150`, `:505-529`; driven `src/endpoint.rs:969-971` | §9.4, §9.6 |
| (pre-sample) `K_INITIAL_RTT` | 333 ms ⇒ first PTO 1024 ms | | `src/recovery.rs:58`, `:124-135` | §9.6 |

**Ordering inside one tick** (`src/endpoint.rs:904-991`), which is itself behaviour the v2 core must reproduce:
1. iterate `sessions`: `is_dead` ⇒ collect for teardown and `continue`; else `should_keepalive` ⇒ seal empty Data into the outbox (`:909-924`);
2. iterate `pending`: give-up if `>= HANDSHAKE_GIVEUP`, **else if** past `next_retransmit` ⇒ retransmit (`:928-934`) — give-up wins over a same-tick retransmit;
3. flush keepalives (`:936`), then teardown dead (`:937-939`), then give-ups (`:940-942`), then retransmits (`:943-945`);
4. per connection with recovery state and a live session: fire **exactly one** of loss-timer / PTO (loss takes precedence, RFC 9002 §6.2) (`:950-975`); if that queued retransmittable DATA, run the payload age gate (`:984-986`); then one `pump` (`:987-989`).

### 1.2 State maps (all endpoint-global, all keyed differently)

| Field | Key → value | Written | Read | Notes |
|---|---|---|---|---|
| `sessions` | `ConnId` → `Session` | `:1051`, removed `:1040`,`:1074` | everywhere | one live session per logical connection |
| `recovery` | `ConnId` → `Recovery` | `:1055-1060`, removed `:1073` | `:628`,`:783`,`:859`,`:894`,`:950-973` | sibling of `Session`; **outlives the session swap** |
| `index_to_conn` | our session index `u32` → `ConnId` | `:1048`, removed `:1041-1044`,`:1075-1077` | `:798` (Data demux) | random 32-bit key, **no collision check on insert** |
| `static_to_conn` | remote static compressed `[u8;33]` → `ConnId` | `:1049-1050`, removed `:1078-1081` | `:534` (revoke), `:732` (responder replace-vs-new) | **one connection per peer static, endpoint-wide** |
| `pending` | `ConnId` → `PendingConnect<I>` | `:562-574`, removed `:774`,`:1026`,`:1089` | `:603`,`:634`,`:928`,`:994`,`:1017` | initial connect **and** rekey |
| `pending_by_index` | our msg1 `sender_index` → `ConnId` | `:575`,`:1016`, removed `:775`,`:1015`,`:1090` | `:758` (Resp demux) | separate namespace from `index_to_conn` |
| `allow` | set of `[u8;33]` | `:259`, `:529`, `:533` | `:707` (into the Noise read closure) | policy |
| `ts_guard` | `TimestampGuard`: static `[u8;33]` → greatest `Timestamp` | `:720` | `:720` | endpoint-global, per-peer-identity; never pruned (`src/handshake.rs:511-534`) |
| `last_init_timestamp` | `Option<Timestamp>` | `:512` | `:496` | **endpoint-global monotonic**, not per peer |
| `next_inbound` | `u64` counter from `INBOUND_CONN_BASE` = 2^63 | `:736` | `:735` | inbound ConnId minting |
| `Endpoint::next_connect` | `u64` from 1 | `src/endpoint.rs:277`,`:291` | `:290` | **minted on the handle, not in the actor** |
| `persistent_keepalive` | `Option<Duration>` | config only | `:919` | endpoint-wide, not per connection |
| `rng` | `ChaCha20Rng` | `:549`,`:724`,`:1002` (indices), `:486-488` (jitter) | — | single shared stream |

### 1.3 Initiator flow

1. **`connect`** (`src/endpoint.rs:285-301`): mints `ConnId` on the handle (`next_connect`), sends `Command::Connect`, returns a `SessionHandle` immediately (fire-and-forget; establishment arrives as an `Event`). Not gated by the allow-list (SPEC §5 "Allow-list": "`connect` … is not gated").
2. **`begin_connect`** (`:541-577`): random nonzero index (`random_index`, `src/handshake.rs:538-545`), `next_timestamp()`, fresh provider + fresh static copy from `Identity` (`src/handshake.rs:158-172`), `handshake::build_init` (`src/handshake.rs:272-304`) → 196-byte packet + `InitiatorPending`; installs `PendingConnect { remote_addr, remote_static, attempt: Some(..), current_index, started_at, next_retransmit = now + 5 s + jitter, is_rekey, queued: vec![] }`; registers `pending_by_index`; flushes msg1.
3. **`next_timestamp`** (`:494-514`): wall clock, forced strictly greater than `last_init_timestamp` by +1 ns (with s/ns carry) — SPEC §5 "forced strictly greater than the previous one this endpoint emitted".
4. **Retransmit** (`:993-1023`): a *completely fresh initiation* — new provider/ephemeral, new random index, new strictly-greater timestamp, new jitter; swaps `pending_by_index` (`:1015-1016`) and replaces `attempt`, `current_index`, `next_retransmit`. SPEC §5 Initiator 2.
5. **Completion** (`on_resp`, `:748-790`): length gate (`RESP_PACKET_LEN`), parse `RespHeader`, demux by `receiver_index` through `pending_by_index`, **take** `attempt` (`:762-769` — one completion attempt per interval; a second Resp in the same interval is dropped), `handshake::complete_init` (mac1 keyed on our own static + `receiver_index` echo check + Noise `<- e, ee, se`, `src/handshake.rs:313-357`). On success: remove pending, `emit = !sessions.contains_key(conn)`, `Session::new(established, pending.remote_addr, now)`, `install_session`, re-queue `pending.queued` into recovery, `pump`. On failure the attempt is simply spent — the next retransmit refreshes.
   - **`src` of the Resp is deliberately ignored** (`:749 let _ = src;`): the session anchors to the dialled `remote_addr`.
6. **Give-up** (`:1025-1037`): removes pending + index; emits `Failed { conn, TimedOut(90 s) }` **only when `!is_rekey`**.

### 1.4 Responder flow

`on_init` (`src/endpoint.rs:693-746`):
1. mac1 gate before any DH — inside `handshake::accept_init` (`src/handshake.rs:447-467`), length gate first (SPEC §4, §5 Responder 1).
2. Verification read `read_message_1_with` drives `e, es, s` and hands the **claimed** static to the closure; the closure **is the allow-list** (`src/endpoint.rs:702-708` → `src/handshake.rs:480-489`). `HsError::Unlisted` ⇒ traced on `slither::policy`, return: no `ss`, no timestamp decryption, no msg2 (SPEC §5 Responder 2–3).
3. Admitted read continues through `ss`, authenticating the claim and decrypting the 12-byte timestamp payload; pauses on `RespAccept` (`src/handshake.rs:367-427`, SPEC §5 Responder 4).
4. **Greatest-timestamp guard** per initiator static (`:720`, `src/handshake.rs:515-533`): non-greater ⇒ trace on `slither::replay`, drop, **no msg2** (SPEC §5 Responder 5).
5. Random nonzero responder index (`:724`), `accept.accept(index)` writes msg2 (`ee`, `se`) + mac1 keyed on the initiator, and yields `Established` (SPEC §5 Responder 6).
6. **Replace-vs-new** (`:732-739`): if `static_to_conn` already knows this static ⇒ reuse that `ConnId` and **do not** emit `Established` (silent in-place replacement, SPEC §5 Responder 7); else mint `ConnId(next_inbound)` and emit.
7. `Session::new(established, src, now)` — anchored at the msg1 source address; `install_session`; flush msg2; `pump` (so a replacement's re-queued messages land at once, `:742-745`).

### 1.5 Rekey (both directions)

- **Trigger** (`gate_payload`, `:593-612`): only on payload-seal paths. `session.needs_rekey(now)` (age ≥ 120 s) **and** no pending for that conn ⇒ `begin_connect(conn, session.endpoint(), *session.remote_static(), is_rekey = true)`. Either side may rekey (SPEC §7).
- **Call sites of `gate_payload`** — exactly three: fresh send (`:621`), post-ACK pump when loss detection re-queued DATA (`:894-900`), tick retransmit path (`:984-986`). Control traffic (keepalive, pure ACK, bare PING probe) never calls it (SPEC §6 amendment).
- **Old session keeps sealing** on its own counters until the swap or the backstop.
- **Swap** (`install_session`, `:1039-1068`): removes the old session, removes its `index_to_conn` entry **if it still points at this conn**, inserts the new index, re-inserts `static_to_conn`, inserts the session, then `recovery.epoch_reset()` if recovery exists else `Recovery::new()`, then `Established` only if `emit_established`.
- **What survives the swap**: `ConnId`; `Recovery`'s per-connection half — `next_seq`, `outstanding`, `to_send`, `RttEstimator`, `delivered_floor`/`delivered_sparse` (`src/recovery.rs:261-273` + module doc `:12-17`); `static_to_conn`; the allow-list; `ts_guard`; `last_init_timestamp`; the peer address (carried through `pending.remote_addr`, captured at `:607` from the possibly-roamed `session.endpoint()`).
- **What resets**: the whole `Session` (keys, counter space, replay window, `established_at`, `last_recv`/`last_send` = now, `our_index`, `peer_index`); `Recovery`'s per-epoch half — `sent`, `largest_acked`, `loss_time`, `time_last_ack_eliciting`, `pto_count`, `ping_pending`, `ack_pending`, `largest_recv_at`; every undelivered message is re-queued to `to_send` (`src/recovery.rs:270-272`). SPEC §9.5.
- **No `Established` re-emission**, no `Dead` for the old session (SPEC §6 amendment "the silent swap").
- **Rekey give-up is silent** (`:1030`) — the old session stays usable.
- **Responder-side equivalent** is the in-place replacement of §1.4.6, which takes the same `epoch_reset` path.

### 1.6 Roaming

- Entirely inside `Session::open` (`src/session.rs:277-318`): decrypt first, **then** replay-admit, and only a fresh authenticated packet from a different `src` moves `self.endpoint` and reports `moved_from` (`:300-306`). Nothing unauthenticated and no replay ever moves it (SPEC §6 Roaming).
- The actor's only duty is to surface it: `Event::EndpointMoved { conn, remote_static, from, to: src }` (`src/endpoint.rs:816-823`).
- Outbound packets always go to `session.endpoint()` (`:669`, `:921`), so roaming redirects sends implicitly.
- Handshake packets do **not** roam a session: `on_resp` ignores `src` (`:749`); `on_init` anchors a new/replacement session at `src` (`:740`).

### 1.7 Keepalive

- **Passive** (SPEC §6): `last_recv > last_send && now - last_recv >= 10 s` (`src/session.rs:324-325`).
- **Persistent** (opt-in, endpoint-wide): `now - last_send >= interval` (`src/session.rs:326-327`), interval from `Config::persistent_keepalive` (`src/endpoint.rs:220-223`).
- Emission: `session.seal(&[], now)` — the *liveness-marking* seal, empty plaintext ⇒ 30-byte datagram; bypasses the frame layer entirely (`src/endpoint.rs:919-923`).
- A received keepalive is admitted by the replay window (so it *does* appear in later ACK ranges) but never reaches `Recovery::note_received`, because `on_data` only calls `on_frames` for a non-empty payload (`src/endpoint.rs:827-830`) — hence SPEC §9.2's "ack_delay is zero when that arrival was not seen at the frame layer".

### 1.8 Death conditions (all of them)

| Cause | Path | `Dead` emitted? | Notes |
|---|---|---|---|
| Liveness `DEAD_TIMEOUT` | tick `:915-918` → `teardown(conn, true)` `:937-939` | yes | the **only** idle killer (SPEC §6 amendment) |
| `REJECT_AGE` payload backstop | `gate_payload` `:596-599` → `teardown(conn, true)` | yes | reachable from all three payload paths (fresh send, post-ACK retransmit pump, tick retransmit) |
| `revoke(static)` | `:531-537` → `teardown(conn, true)` | yes | removes from allow-list, then tears down the single conn found via `static_to_conn` |
| `SessionHandle::close()` | `:372-374` → `Command::Close` → `teardown(conn, false)` `:527` | **no** | nothing is transmitted (0x04 close is reserved/never emitted, SPEC §2) |
| `connect` give-up | `:1025-1037` | no — `Failed` instead | only for `!is_rekey` |
| actor stop (all handles dropped) | `:454` | no | every session dies silently with the task |
| implicit: session swap | `install_session` `:1040` | no | old session simply dropped |

`teardown` (`:1070-1092`) always: drops `Recovery` (undelivered reliable messages are lost — SPEC §9.3), drops the session, conditionally removes `index_to_conn`/`static_to_conn` entries **only if they still point at this conn**, drops any pending + its index.

### 1.9 Leg 2 integration points

| Integration | Code | SPEC |
|---|---|---|
| `Recovery` is a sibling of `Session`, keyed by `ConnId` | `src/endpoint.rs:429-433`, `src/recovery.rs:1-17` | §9.5 |
| Handle-side size gate before the channel | `src/endpoint.rs:355-362` (`MAX_MESSAGE` 1159) | §9.3, §9.7 |
| Fresh send: age gate → `queue_message` → `pump` | `:614-640` | §6 amendment, §9.3 |
| Early send (pre-establish): `pending.queued.push` | `:634-636`, flushed `:783-787` | not spec'd explicitly |
| Packet planning: ACK first, then queued/retransmitted DATA oldest-first while it fits, then a PING probe if nothing ack-eliciting got aboard | `src/recovery.rs:297-362` | §9.2 (coalescing), §9.4 |
| **ACK source is the Leg 1 replay window** | `src/session.rs:258-260` (`ack_window`) → `src/endpoint.rs:656` → `src/frame.rs:298-343` (`AckFrame::from_window`) | §9.2 "The ACK source is the Leg 1 replay window" |
| Fresh vs quiet seal | `src/endpoint.rs:657-661`; `Session::seal` `:207` vs `seal_quiet` `:224` | §9.5 liveness-neutral control |
| Counter recovered by re-parsing the sealed packet header, then `on_packet_sent` | `src/endpoint.rs:665-668` | — (implementation seam; v2 should return the counter directly) |
| Inbound: decode frames, `note_received`, dispatch Padding/Ping/Ack/Data, dedup, `mark_ack_pending` | `src/endpoint.rs:839-887` | §9.1, §9.2, §9.3 |
| Malformed frame stream ⇒ whole packet dropped with a trace | `:846-856` | §9.1 |
| ACK validation bound: `highest_sent = Session::last_counter()`; an ACK above it is ignored whole | `:858`, `src/session.rs:264-266`, `src/recovery.rs:398-411` | §9.4 |
| ACK processing intersects with the in-flight set (`contains`, no materialisation) | `src/recovery.rs:416-423`, `src/frame.rs:387-416` | §9.2 "Bounded processing" |
| RTT sample only when the frame's `largest` is newly ACKed; ack_delay capped at `MAX_ACK_DELAY` | `src/recovery.rs:429-440`, `:100-121` | §9.4 |
| Loss detection then re-queue of frames (never packets) | `src/recovery.rs:462-495` | §9.4 |
| PTO: probe = oldest undelivered message, else PING; backoff doubles, reset on any new ACK | `src/recovery.rs:451-453`, `:505-529` | §9.4 |
| Payload-vs-control discriminator for the age gate | `Recovery::has_retransmittable` `src/recovery.rs:370-374`; used `:894-897`, `:973` | §6 amendment |
| Exactly-once delivery dedup by message `seq` | `src/recovery.rs:562-575` | §9.3 |
| `epoch_reset` on any session swap | `src/recovery.rs:261-273`, called `src/endpoint.rs:1056` | §9.5 |

### 1.10 The command-channel surface (what the object model must relocate)

`enum Command` (`src/endpoint.rs:379-398`) — five variants:

| Command | Public entry | Actor handler | v2 destination |
|---|---|---|---|
| `Connect { conn, remote_addr, remote_static }` | `Endpoint::connect` `:285` | `:518-525` | `Endpoint::connect() -> Connection` |
| `Send { conn, payload }` | `SessionHandle::send` `:355` | `:526` → `:614` | `Reliable<Connection>::send` |
| `Close { conn }` | `SessionHandle::close` `:372` | `:527` | `Connection::close` |
| `Allow { remote_static }` | `Endpoint::allow` `:313` | `:528-530` | dissolved — staged accept |
| `Revoke { remote_static }` | `Endpoint::revoke` `:321` | `:531-537` | dissolved — user drops the `Connection` |

Other public surface: `Endpoint::start` (`:248`), `Endpoint::session(conn)` (`:305`, fabricates a handle for *any* `ConnId`), `Endpoint::next_event` (`:328`), `SessionHandle::id` (`:342`), `Config::{new, with_rng_seed, allow, persistent_keepalive}` (`:186-224`), `SlitherError::{PayloadTooLarge, EndpointClosed}` (`:76-92`), `ConnectError::TimedOut` (`:96-101`).

### 1.11 Event emissions (the whole set)

| Event | Emitted at | Trigger |
|---|---|---|
| `Established { conn, remote_static, endpoint }` | `:1061-1067` | new inbound connection (`:737`) or first completion of an outbound connect (`:776`). **Never** on a rekey swap or a responder-side replacement |
| `Incoming { conn, remote_static, payload }` | `:881-887` | each first-arrival DATA frame, in arrival order, before the owed ACK is pumped |
| `Dead { conn, remote_static }` | `:1082-1087` | `teardown(_, true)`: liveness, `REJECT_AGE` backstop, revoke |
| `EndpointMoved { conn, remote_static, from, to }` | `:816-823` | authenticated fresh Data from a new source |
| `Failed { conn, error }` | `:1031-1035` | initial-connect give-up at 90 s |

### 1.12 Queued sends before establishment

- `handle_send` with no session but a live pending pushes onto `PendingConnect::queued` (`:634-636`); unbounded, no size accounting beyond the `MAX_MESSAGE` handle check.
- Flushed only on **initiator** completion (`:783-787`), into `Recovery::queue_message`, then `pump`.
- A give-up (`:1026`) or a `teardown` (`:1089`) **silently discards** the queue — no `Failed`-carried payloads, no error to the caller.
- A rekey's `PendingConnect` always starts with an empty `queued` (`:572`) because sends during a rekey ride the still-live old session.
- `handle_send` for an unknown conn is a silent trace-only drop (`:637-639`).

### 1.13 Close paths, summarised

1. `SessionHandle::close()` → `teardown(conn, false)`: local-only, no wire signal, no event, undelivered messages lost.
2. `Endpoint::revoke(static)` → allow-list removal + `teardown(conn, true)`.
3. Dropping all handles → actor exit, no teardown at all.
4. There is **no** close frame and no close packet type: `0x04` is reserved at both the packet level (SPEC §2) and the frame level (SPEC §9.1 `FRAME_RESERVED_CLOSE = 0x06`, `src/frame.rs:56`) and is never emitted.

---

## 2. Protocol vs policy vs I/O

**P** = pure protocol (sans-io core) · **Y** = policy (the user's, in the new model) · **G** = I/O / runtime glue (thin shell) · **?** = ambiguous, needs a ruling.

| Responsibility | Class | Note for the v2 spec |
|---|---|---|
| `classify` + version/type drop (`wire.rs:319`) | **P** | pure byte inspection |
| Oversize-datagram drop (`endpoint.rs:682`) | **P** | the bound is protocol (`MAX_DATAGRAM`), the buffer size is glue |
| mac1 verify/compute (`mac.rs`, `handshake.rs:461`) | **P** | |
| Noise reads/writes, `Established` production | **P** | already pure |
| Index minting `random_index` | **P** (needs an RNG service) | the *source* of randomness is glue; the nonzero rule and the uniqueness requirement are protocol |
| `next_timestamp` monotonic forcing | **P** (needs a clock service) | endpoint-scoped state, see §5 hazards |
| `TimestampGuard::admit` | **P** | SPEC §5 Responder 5 explicitly: "the guard is not policy" (TODO stage table) |
| Allow-list membership check | **Y** | v2: the `Claimed` stage; today it is wired *inside* the Noise read closure, which is why it costs exactly 1 DH |
| Who may `connect` | **Y** | already ungated |
| `Config::persistent_keepalive` | **Y** (per-connection knob in v2) | today endpoint-wide |
| Replay window admit + snapshot | **P** | |
| Roaming decision | **P** | |
| Keepalive/liveness/rekey/reject decisions | **P** | pure functions of `(last_send, last_recv, established_at, now)` |
| The 250 ms scan itself | **G** | v2 core should expose `next_timeout()`; the tick is a shell artefact that the current tests' tolerances bake in |
| Retransmit jitter draw | **P** (RNG service) | value is protocol (§6), source is glue |
| Frame encode/decode, ACK from window, coalescing/packetisation | **P** | already pure |
| RFC 9002 loss/PTO/RTT | **P** | already pure |
| Payload-vs-control age gating | **P** | |
| `Session::seal` vs `seal_quiet` choice | **P** | driven by `PacketPlan::fresh` |
| Counter recovery by re-parsing the sealed header (`:665-668`) | **G**/**?** | an artefact; v2 `seal` should return `(counter, bytes)` |
| Socket read/write, buffer ownership, `flush` | **G** | |
| `spawn_local`, `LocalSet`, `!Send` | **G** | |
| Command channel + `SessionHandle` cloning | **G** | dissolves into the object model |
| Event stream + unbounded mpsc | **G** | dissolves; becomes per-object futures/streams |
| `ConnId` minting (both spaces) | **G**/**?** | dissolves in v2, but "what is the identity of a logical connection" is a **protocol** question (see hazard 5.1) |
| `static_to_conn` replace-vs-new decision on inbound msg1 | **?** | classified as protocol today (SPEC §5 Responder 7 "replaces the peer's session in place"), but it is *also* the policy "one connection per peer static". v2 must rule explicitly |
| Which side surfaces `Established` (emit flags at `:737`,`:776`,`:1061`) | **?** | the "silent swap" rule is protocol; the event is API |
| Queued-sends-before-establish | **?** | today an actor convenience; in a quinn-shaped API it becomes either "connect returns a usable `Connection` immediately" (keep it) or "await the handshake" (drop it). `flow_frames::queued_sends_before_establish_flow_reliably` pins the current answer |
| Discarding the queue on give-up without notifying | **?** | arguably a bug; v2 should rule |
| Tracing targets `slither::policy` / `slither::replay` / `slither::frames` | **G** | observable contract for operators; worth keeping |

---

## 3. SPEC v1 section map

Legend: **WIRE** = byte-visible/interop-critical, untouchable in v0.2 phases 1–3 · **BEHAV** = behavioural, amendable in v2 · **MIX** = both.

| § | Title | Ratifies | Class |
|---|---|---|---|
| **§1** | Crypto suite | `Noise_IK_P256_ChaChaPoly_BLAKE2b`; P-256; ChaCha20-Poly1305 (16-byte tag); BLAKE2b; 65-byte uncompressed SEC1 on the wire, 33-byte compressed for addressing/mac1; msg1 carries the 12-byte timestamp, msg2 empty payload; hiss `DatagramSend`/`DatagramRecv` | **WIRE** (phase 2 generalises the *suite selection*, not the default channel's bytes) |
| **§2** | Packet types and version | `VERSION 0x01`; types `0x01/0x02/0x03`; reserved `0x04` close, `0x05` cookie/mac2, `0x06/0x07` probes, `0x08..`; unknown ⇒ silent drop; all multi-byte integers big-endian | **WIRE** |
| **§3** | Wire layouts + sizes | InitHeader 6 B / RespHeader 10 B / DataHeader 14 B (the 14 header bytes are the AEAD AD); `IK_MSG1_LEN 174`, `IK_MSG2_LEN 81`, `INIT_PACKET_LEN 196`, `RESP_PACKET_LEN 107`, `TIMESTAMP_LEN 12`, `AEAD_TAG_LEN 16`, `MAC1_LEN 16`, `MAX_DATAGRAM 1200`, `MAX_PLAINTEXT 1170`, nonzero-`u32` session index; empty plaintext = keepalive; oversize send = typed error, oversize receive = silent drop | **WIRE** (the "typed error" half is API) |
| **§4** | mac1 — the DoS gate | `MAC1_LABEL = b"slither mac1"`; key = BLAKE2b-256(label ‖ recipient compressed static); tag = keyed-BLAKE2b-128 over all preceding bytes; verified before any curve work; explicitly not a secret authenticator | **WIRE** |
| **§5** | Handshake behaviour | `PROLOGUE = b"slither\x01"`; msg1 payload = `ts_secs(8 BE) ‖ ts_nanos(4 BE)` encrypted in the tail; confidentiality-level-2 analysis; **Initiator** 1–4 (fresh index+timestamp, retransmit with jitter, *each retransmit a wholly fresh initiation*, complete on index-matching mac1-valid msg2, give up at 90 s); **Responder** 1–7 (mac1 → verification read → allow-list closure before `ss` → admitted tail (`ss` + timestamp) → greatest-timestamp guard → msg2 + immediate session → newer msg1 replaces in place); **Allow-list** paragraph | prologue/payload/layout = **WIRE**; steps and staging = **BEHAV** (this is exactly what the staged-accept typestate rewrites); the *DH-cost* property is BEHAV but test-pinned |
| **§6** | Session behaviour | Replay window (RFC 6479 shape, `REPLAY_WINDOW = 128`, checked only after AEAD success, drop-without-delivery); Roaming; the timer table (5 s / 333 ms / 90 s / 10 s / 15 s / 25 s / 120 s / 180 s); the **2026/07/17 amendment** (idle age-death removed, age gates payload only, control is age-exempt, inbound opening age-exempt); **Key ratchet** (`REKEY_EPOCH_MSGS = 65 536`, per-direction, counter never reset, `2^64−1` reserved, previous-epoch straggler tolerance, `MAX_EPOCH_JUMP = 2`, no post-compromise healing) | replay-window *width* and the key ratchet = **WIRE**-equivalent (interop-critical key schedule); timers, roaming, the amendment = **BEHAV** |
| **§7** | Deviations from the brief | timestamp moved into the encrypted payload (resolved); **either side may rekey**; the fifth event `Failed` | **BEHAV** (the `Failed` bullet is pure API and dies with the event stream) |
| **§8** | Out of scope (Leg 1) | frames/ACK/reliability (superseded by §9), cookies/mac2, probes/hole punching, reflector, mDNS, PSK, CC, bubble, persistence | **BEHAV** (scope statement) |
| **§9** | Leg 2 preamble | frames within sealed packets; the datagram `counter` **is** the packet number; frames retransmitted, never packets; no TLS/0-RTT/flow control/CC | **WIRE** framing model |
| **§9.1** | Frame types | `0x00` PADDING, `0x01` PING, `0x02` ACK, `0x03` DATA; `0x04..=0x0F` reserved ⇒ stop parsing, keep what was parsed; `0x10..` ⇒ drop the whole packet; malformed stream fails the whole packet; empty plaintext bypasses the layer | **WIRE** |
| **§9.2** | ACK | 16-byte `AckHeader` layout, 4-byte range pairs, RFC 9000 §19.3.1 descending semantics, underflow = violation; ack_delay µs on tokio's clock; **ACK built from the Leg 1 replay window**; immediate ACK policy; ACKs not ack-eliciting; pure-ACK packets untracked; bounded (intersecting) ACK processing | layout = **WIRE**; policy/bounding = **BEHAV** |
| **§9.3** | DATA | 11-byte `DataFrameHeader`; `seq` = retransmittable identity + dedup key; unordered reliable, exactly-once surfacing; `MAX_MESSAGE` cap, no fragmentation; zero-length valid; reliability lives within the connection; a rekey does not lose messages | layout + `MAX_MESSAGE` = **WIRE**; delivery semantics = **BEHAV** (phase 3's `Ordered` wrapper explicitly rides on top with zero wire change) |
| **§9.4** | Loss detection and PTO | tracked ack-eliciting packets; RTT per RFC 9002 §5 with §5.3 ack_delay subtraction; ACK above the highest sealed = violation; `K_PACKET_THRESHOLD` + 9/8 time threshold; PTO formula with doubling; probe = oldest undelivered or PING; **"both timers are evaluated on the actor's 250 ms TICK"** | **BEHAV** — and the TICK sentence is the one place SPEC v1 hard-codes the actor into the protocol; v2 must restate it as a granularity bound |
| **§9.5** | Interplay with Leg 1 timers | control packets are liveness-neutral (`seal_quiet`); only fresh application sends and the Leg 1 keepalive mark the clock; on a rekey/replacement, per-epoch recovery resets and undelivered messages re-queue while message seqs, receiver dedup and the RTT estimate survive | **BEHAV** (but it is the single most load-bearing behavioural rule for the object split) |
| **§9.6** | Constants | frame type bytes; `AckHeader::SIZE 16`; `AckRangePair::SIZE 4`; `DATA_OVERHEAD 11`; `MAX_MESSAGE 1159`; `MAX_ACK_RANGES 63` (max ACK 268 B); `MAX_ACK_DELAY 25 ms`; `K_PACKET_THRESHOLD 3`; 9/8; `K_GRANULARITY 1 ms`; `K_INITIAL_RTT 333 ms`; `PTO_BACKOFF_CAP 2^6` | sizes/type bytes/`MAX_MESSAGE`/`MAX_ACK_RANGES` = **WIRE** (the last is a peer-visible validation cap); the RFC 9002 tunables = **BEHAV**, local-only |
| **§9.7** | Behavioural deltas | `send` is reliable; the cap moved 1170 → 1159; an empty send is a real empty message | **BEHAV**/API |
| **§10** | Out of scope (Leg 2) | ordered streams + fragmentation, CC/pacing, cookies/mac2, probes, reflector, mDNS, PSK, bubble, persistence | **BEHAV** (phase 3 moves "ordered" out of this list at the *receiver*, with no wire change) |

**Sections the v2 spec must rewrite rather than amend**: §5 Responder 2–3 (allow-list closure → staged typestate), §5 Allow-list paragraph, §7's `Failed` bullet, §9.4's TICK sentence, and every "event" mention. **Sections that must survive verbatim**: §1–§4, §5's prologue/payload/layout, §6's ratchet, §9.1–§9.3 layouts, §9.6 sizes.

---

## 4. What the tests pin

### 4.1 Golden / constant tests — must keep passing byte-for-byte

| Test | File:line | Pins |
|---|---|---|
| `handshake::tests::golden_wire_is_byte_identical_to_the_pre_migration_driver` | `src/handshake.rs:929-1007` | the **whole 196-byte Init hex**, the **107-byte Resp hex**, and the **64-byte session-id hex**, for scalar fills `0x11`/`0x22`, master seeds `0xA1`/`0xB2`, indices `0x12345678`/`0x9abcdef0`, ts `secs 0x0102030405060708 / nanos 0x090A0B0C`. **The** verdict test for phases 1–3 |
| `handshake::tests::rekey_transform_kat` | `:734-747` | `REKEY(0^32) = 25ce5d37df19f3783185f2ffd5ab17fa3397c212f02d62fb1733e0b875b74c58`, `n == 48` |
| `handshake::tests::protocol_name_is_pinned` | `:603-619` | the string `Noise_IK_P256_ChaChaPoly_BLAKE2b` (phase 2's genericity must keep the *default* channel producing exactly this) |
| `handshake::tests::msg_sizes_match_the_wire_pins` | `:749-778` | 196 / 107 |
| `handshake::tests::timestamp_is_not_on_the_wire` | `:901-926` | the 12 timestamp bytes appear nowhere in the Init datagram |
| `handshake::tests::tampered_payload_fails_then_clean_msg1_succeeds` | `:780-830` | a flipped payload-ciphertext byte (with mac1 recomputed) dies at the tail AEAD **inside** the read; a subsequent clean msg1 still succeeds |
| `handshake::tests::responder_dh_cost_is_staged` | `:832-899` | **1** DH for a rejected read, **+2** for an accepted read, **+2** for the msg2 write, **+0** on the initiator's completion (total 5). See hazard 5.11 |
| `handshake::tests::handshake_agrees_on_session_and_indices` / `established_datagrams_round_trip` / `epoch_boundary_delivers_and_reordered_straggler_opens` | `:621-726` | index mirroring, AD-bound datagram round-trip, and a full 65 536-message epoch crossing with a previous-epoch straggler still opening |
| `handshake::tests::timestamp_guard_rejects_non_greater` | `:1009-1035` | strict-greater admission per static |
| `wire::tests::{header_sizes_are_pinned, derived_sizes_are_pinned, constants_are_frozen, data_header_round_trips_big_endian, timestamp_round_trips_and_orders, classify_drops_unknown_version_type_and_short}` | `src/wire.rs:338-436` | 6/10/14 header sizes; 16/12/174/81/196/107/1170/1200/16; `VERSION`, `PROLOGUE`, every type byte incl. reserved; the explicit 14-byte DataHeader byte array; Timestamp ordering; drop rules |
| `mac::tests::{label_and_lengths_are_frozen, tag_verifies_and_is_deterministic, wrong_recipient_key_fails, tampered_message_or_tag_fails}` | `src/mac.rs:108-148` | `b"slither mac1"`, 16-byte tag, 32-byte key, determinism, constant-time compare behaviour |
| `frame::tests::{header_sizes_are_pinned, constants_are_proposed_values}` | `src/frame.rs:561-582` | 16/4/11, `DATA_OVERHEAD 11`, `MAX_MESSAGE 1159`, `MAX_ACK_WIRE 268`, `MAX_ACK_RANGES 63`, frame-type bytes `0x00-0x03` + reserved `0x04/0x05/0x06/0x0F` |
| `frame::tests::{padding_and_ping_round_trip, data_round_trips_big_endian, ack_round_trips_big_endian, coalesced_sequence_round_trips}` | `:584-659` | exact frame byte layouts (e.g. DATA = `03 ‖ seq BE(8) ‖ len BE(2) ‖ bytes`) |
| `frame::tests::{malformed_streams_fail_the_packet, reserved_type_skips_the_remainder, max_ack_fits_max_plaintext, over_cap_ack_is_rejected}` | `:661-775` | truncation errors with exact needed/remaining, `UnknownType(0x10)`, `AckUnderflow`, reserved-type stop-parsing, 268-byte maximal ACK, 64-range rejection |
| `frame::tests::{window_to_ack_and_back, window_alternating_worst_case_is_complete, window_near_zero_stays_in_bounds}` | `:777-820` | replay-window → ACK-range construction, including the 63-range alternating worst case (nothing truncated) and near-zero bounds |
| `session::tests::window_*` (4) | `src/session.rs:388-433` | RFC 6479 admit/duplicate/edge (`diff == 128` is out)/large-jump behaviour |
| compile-time `const _: () = assert!(…)` | `src/wire.rs:290-299`, `src/frame.rs:151-156`, `src/handshake.rs:97-98` | the sizes cannot drift without failing to compile |
| `recovery::tests` (9) | `src/recovery.rs:578-849` | `has_retransmittable` payload-vs-control discrimination, RTT smoothing + ack_delay cap, `K_PACKET_THRESHOLD == 3` exactly, time-threshold `112.5 ms` from a 100 ms sample, PTO interval **1024 ms** and doubling, PING-when-nothing-outstanding, dedup floor compaction, `epoch_reset` re-queue semantics, plan coalescing/splitting at `MAX_PLAINTEXT`, over-claiming ACK ignored |

These are all independent of the actor and survive the refactor unchanged (except `recovery`'s visibility if the module moves).

### 4.2 Flow tests — behaviour to preserve vs API shape to rewrite

All of `src/flow.rs` and `src/flow_frames.rs` drive `Endpoint`/`Event`/`ConnId`/`SessionHandle` over `testutil::FlakyWire` on a paused clock inside a `LocalSet`, so **every one is API-shaped in its harness**. The column below is about the *behaviour* each encodes.

| Test | File:line | Behaviour that must survive | Harness rewrite |
|---|---|---|---|
| `happy_path` | `flow.rs:141` | bidirectional establish + message each way | full |
| `msg1_lost_twice_retransmits_fresh` | `:159` | establish in ~10–11.5 s after 2 losses; the first three Init packets are **pairwise distinct** | partial (uses `net.sends()`, survives) |
| `msg2_lost_reaccepted` | `:198` | a fresh msg1 with a strictly-greater timestamp is re-accepted after a lost msg2 | full |
| `handshake_gives_up` | `:212` | give-up at 90–90.5 s with a typed error | full — `Failed` event becomes a `connect` future error |
| `mac1_flood_never_reaches_dh` | `:240` | 40 garbage + 10 wrong-key Inits ⇒ **zero** additional DH; session unharmed | partial |
| `initiation_replay` | `:308` | a replayed Init produces **no** second HandshakeResp | partial |
| `data_through_reorder_and_dup_exactly_once` | `:348` | 50 messages under 50 % duplication + jitter, each surfaced exactly once | full |
| `roaming_follows_authenticated` | `:384` (+ helper `:433`) | roam on authenticated-fresh only; forgery and replay never move the endpoint | **none** — drives `Session` directly; survives as-is |
| `keepalive_after_idle` | `:468` | an empty-plaintext (tag-only) Data leaves ~10–10.6 s after the received-not-sent flip | partial (tap-based) |
| `dead_after_silence` | `:521` | `Dead` at 15–15.6 s after a send into a partition | full |
| `rekey_keeps_flow` | `:554` | a send past 125 s triggers a silent swap and payload keeps flowing | full |
| `idle_survives_past_reject_age_then_a_payload_send_backstops` | `:595` | zero-traffic idle survives 300 s with **no** re-handshake and no death; a fresh payload send then hits the backstop | full |
| `retransmit_into_partition_dies_by_liveness` | `:666` | symmetric loss ⇒ liveness (~15 s) preempts the age gate; no re-establish | full |
| `size_caps` | `:705` | oversize send is a typed error, never enqueued; a `MAX_MESSAGE` message fills exactly 1200 bytes on the wire | partial |
| `unknown_packets_dropped` | `:769` | bad version/unknown type/reserved/short/garbage never disturb a live session | full |
| `allow_list_rejects_unlisted` | `:796` | an unlisted but valid initiator gets no msg2 and no session; the dialler gives up | **rewrite as policy** — v2 has no allow-list; becomes "drop the `Claimed`/`Intro` object ⇒ nothing transmitted" |
| `unlisted_initiator_costs_one_dh_and_no_resp` | `:827` | **exactly 1 ECDH** for an unlisted mac1-valid Init, no Resp; a subsequent admitted handshake adds exactly `es+ss+ee+se` (total 5) | rewrite as the stage-cost table; see hazard 5.11 |
| `real_udp_loopback` | `:894` | end-to-end over a real socket | full |
| `asymmetric_loss_retransmit_gate_rekeys_then_backstops` | `:948` | with the peer's control still arriving, the retransmit-path gate fires the rekey in **[120 s, 180 s)** and the backstop kills at **[180 s, 205 s]** | partial (session-layer puppet survives) |
| `flow_frames::lost_message_redelivered_once_on_a_fresh_counter` | `flow_frames.rs:134` | a retransmitted frame **never reuses a counter**; delivered exactly once | full |
| `flow_frames::empty_send_surfaces_as_empty_incoming` | `:172` | an empty send is a real message (SPEC §9.7) | full |
| `flow_frames::duplicate_retransmit_is_acked_but_surfaces_once` | `:195` | every duplicate is ACKed again but surfaced once | full |
| `flow_frames::acks_keep_flow_over_a_lossy_wire` | `:239` | 30 messages through 20 % two-way loss + jitter, exactly once each | full |
| `flow_frames::pto_fires_and_backoff_doubles` | `:276` | probe train at ~1.0–1.3 s, ~3.1–3.6 s, ~7.2–8.0 s (the 1024 ms base doubling on the 250 ms tick) | full — **and tick-granularity-sensitive** |
| `flow_frames::rtt_tracking_prevents_spurious_probes` | `:307` | 100 ms symmetric delay ⇒ exactly N sends, no spurious probe | full |
| `flow_frames::coalesced_retransmits_ride_one_packet` | `:347` | two re-queued frames coalesce into one packet (exact ciphertext-length signature 42 B) | full |
| `flow_frames::queued_sends_before_establish_flow_reliably` | `:384` | sends issued **before** the handshake completes are delivered, exactly once | full — **and it pins an API decision** (see §1.12) |
| `flow_frames::paused_clock_is_virtual` | `:409` | harness sanity | keep |

---

## 5. Hazards for the object-model refactor

**5.1 One connection per peer static, endpoint-wide (`static_to_conn`).**
`src/endpoint.rs:435`, written `:1049-1050`, read `:534` and `:732`. The responder's replace-vs-new decision, and therefore SPEC §5 Responder 7, is keyed on the peer's long-term static — the logical connection's identity *is* the peer identity. Consequences: two `connect`s to the same peer produce two `ConnId`s but collapse to one `static_to_conn` entry, so a later inbound msg1 from that peer replaces whichever conn won the map, and `revoke` tears down only one of them. quinn-shaped `Endpoint::connect` returning independent `Connection`s makes multi-connection-per-peer natural — v2 must rule explicitly whether that is allowed, and if it is, the responder's "replace in place" rule needs a new discriminator.

**5.2 The replace-vs-new decision cannot be made before paying 2 DH.**
`on_init` learns the peer static only *after* the accepted read (`:718`). So an inbound msg1 that is really a **rekey from an established peer** is indistinguishable from a brand-new connection until `Proven`. In the staged-accept design (TODO §3), a rekey msg1 would otherwise be parked in the accept queue and surfaced to the application as a new `Intro`. The v2 spec needs an explicit rule: at `Proven` (or `Claimed`), the endpoint re-routes to an existing `Connection` for that static instead of surfacing an `Intro`. This also interacts with the accept-queue dedup key `(source addr, sender_index)` — a rekey has a *different* sender_index and possibly a different address, so the dedup key does not catch it.

**5.3 Session-index minting has no collision check.**
`random_index` (`src/handshake.rs:538-545`) guarantees only non-zero. `install_session` inserts into `index_to_conn` unconditionally (`:1048`), so a 32-bit birthday collision silently steals another connection's inbound Data routing, and the victim's later `teardown` (guarded by `get(..) == Some(&conn)` at `:1075`) leaves the thief's mapping intact. Same for `pending_by_index` (`:575`, `:1016`). With an `Endpoint`-owned demux table in v2 this becomes the endpoint's job — the spec should require rejecting/redrawing a colliding index.

**5.4 `INBOUND_CONN_BASE` and two independent id counters.**
`const INBOUND_CONN_BASE: u64 = 1 << 63` (`:65`); inbound ids counted up in the actor (`:735-737`), outbound ids counted up **on the handle** (`Endpoint::next_connect`, `:240`, `:290-291`) — i.e. in a different task from the one that uses them. `ConnId` dissolves in v2, but note (a) nothing recycles ids after teardown, (b) `Endpoint::session(conn)` (`:305`) will fabricate a handle for any id, live or not, and commands for unknown conns are silently dropped (`:637`).

**5.5 The timestamp guard is endpoint-global and must stay that way.**
`TimestampGuard` (`src/handshake.rs:511-534`, held at `src/endpoint.rs:425`) is keyed by initiator static, not by connection. It is correct *protocol* scope (SPEC §5 Responder 5) and must **not** be pushed into a per-`Connection` object, or a peer could replay a msg1 into a second connection. Two further notes: the map is never pruned, and today its growth is bounded by the allow-list because the policy closure fires first (`src/endpoint.rs:699-701` comment). Under the staged accept, the guard fires at `.authenticate()`, *after* the user's `Claimed` decision — so the bound now depends on user policy; if the user accepts anything, the map becomes an unbounded attacker-controlled allocation. Flag for the v2 threat model.

**5.6 `last_init_timestamp` is endpoint-global monotonic.**
`src/endpoint.rs:427`, `:494-514`. It is the *only* thing that makes two rapid retransmits strictly ordered under a coarse clock. If v2 moves it into `Connection`, two connections to the same peer (see 5.1) can emit equal or out-of-order timestamps and the peer's per-static guard will silently drop one side's handshakes. Keep it endpoint-scoped (or explicitly per-peer-static).

**5.7 One RNG stream shared by index minting and jitter.**
`src/endpoint.rs:420`; consumed at `:486-488`, `:549`, `:724`, `:1002`. `Config::with_rng_seed` (`:208`) is used by every flow test for determinism. Splitting it per-connection changes every draw ordering; no golden test depends on it, but test expectations that assume specific jitter timing (the ±500/600 ms slacks) do depend on the *distribution*, not the values.

**5.8 `pending` and `sessions` coexist for one connection, and the paths cross-talk.**
During a rekey both maps hold the same `ConnId`; `on_resp`'s `emit = !sessions.contains_key(conn)` (`:776`) and `give_up`'s `is_rekey` (`:1030`) are the only disambiguators. Two concrete entanglements:
- If a peer's msg1 arrives while our *initial* connect to that peer is still pending, `on_init` finds the conn via `static_to_conn`, installs a session under it (`:732-741`) but **leaves the pending alive**; that pending keeps retransmitting and, at 90 s, `give_up` emits `Failed` for a connection that is actually established (`:1031`), while `teardown` is never called. Conversely if the old session had already died, a *rekey* completion re-emits `Established` (`:776`).
- `gate_payload` reaches across `sessions`, `pending`, the RNG, the timestamp state and the socket in one call (`:593-612`) — in the object model it needs endpoint-level services injected into the per-connection core.

**5.9 The session swap cuts inbound reception on the old index immediately.**
`install_session` removes the old `index_to_conn` entry (`:1040-1044`) with no grace window, and `Session::open` is only reachable through that map (`:798-808`). In-flight packets sealed under the old session are dropped the instant the new one installs — unlike WireGuard, which keeps the previous session for a while. This is not stated in SPEC v1; v2 should either ratify it or introduce a previous-session grace. (Note `Recovery::epoch_reset` already assumes it: `ack_pending`/`largest_recv_at` are dropped, "the old counter space owes no ACK".)

**5.10 Seal failure silently strands frames — and can stall a connection.**
`pump` (`:670-674`) breaks out on a seal error after `next_packet` has already removed the seq from `to_send`, marked `transmitted`, and (for an ACK) cleared `ack_pending` (`src/recovery.rs:315`, `:328-329`). Nothing re-queues it: `on_packet_sent` was never called, so `detect_lost` cannot see it, and `pto_deadline` returns `None` while `self.sent` is empty (`src/recovery.rs:506-508`). If the *first* packet of a connection fails to seal, no timer ever fires again for it. Latent today (seal only fails on nonce exhaustion or oversize plaintext), but the sans-io core makes this an explicit "transmit returns bytes" contract — worth fixing while relocating.

**5.11 The DH-cost pins are entangled with the hiss split-read decision.**
`handshake::tests::responder_dh_cost_is_staged` (`src/handshake.rs:832-899`) and `flow::unlisted_initiator_costs_one_dh_and_no_resp` (`src/flow.rs:827-890`) pin **1** DH on reject and **+2** on accept. TODO §4's fallback (stage 1 reads with a recording closure that returns `false`, stage 2 re-reads with an accepting closure) re-does `es`, so the accepted path costs **3** DHs on msg1, not 2 — both pins go red, and SPEC §5 Responder 4's "costs the responder both DHs" becomes "three". The v2 spec must either (a) wait for hiss's native split read, or (b) ratify the fallback's cost table explicitly and update the pins. Note the *reject* cost (1 DH) is unchanged either way, which is the DoS-relevant half.

**5.12 The 250 ms TICK is written into SPEC §9.4 and into test tolerances.**
`TICK` (`:60`) is the sole granularity for retransmit, give-up, keepalive, liveness, loss and PTO. Tests assert bounds like `[10 s, 11.5 s]`, `[15 s, 15.6 s]`, `[90 s, 90.5 s]`, `[180 s, 205 s]` and the PTO staircase, all of which assume a coarse scan rather than exact deadlines. A sans-io `next_timeout()` that fires precisely will *tighten* these — mostly harmless, but `asymmetric_loss_retransmit_gate_rekeys_then_backstops`'s upper bound (205 s) exists because "the first PTO consult past 180 s lands at ~196 s on the doubling schedule": with exact deadlines the value moves. Restate §9.4 as "no timer fires earlier than its deadline; the shell bounds lateness" and re-derive the test windows.

**5.13 The event stream is unbounded and global.**
`mpsc::unbounded_channel` (`:254`), `emit` swallows send failure (`:473-475`). `Incoming` is emitted *before* the owed ACK is pumped (`:881-901`), so a slow consumer cannot exert backpressure on ACK generation today. Per-connection receive queues in v2 need a bound and a documented overflow rule (the accept queue already gets one in TODO §3; the data path needs the same treatment).

**5.14 Endpoint-wide knobs that are really per-connection.**
`persistent_keepalive` (`:188`, applied to every session at `:919`) and the allow-list (`:435`) are `Config`-level. In the object model both become per-`Connection` (or per-accept) decisions; the spec should say which defaults apply to an accepted vs a dialled connection.

**5.15 `on_resp` ignores the responder's source address.**
`:749 let _ = src;` — the session anchors at the dialled `remote_addr` (`:778`) even if msg2 arrived from a different port (common behind NAT/multi-homing). The peer only "roams" into place on the first authenticated Data packet (`src/session.rs:300-306`). Preserve deliberately or rule otherwise; either way it belongs in the v2 §5/§6 text.

**5.16 Actor lifetime vs handle lifetime.**
The actor lives while *any* `SessionHandle` clone lives (`:454`, handles clone `cmd_tx` at `:299`/`:308`); dropping the `Endpoint` alone does not stop it, and drops the event receiver so every subsequent `emit` is a no-op. In a quinn shape, `Endpoint` drop semantics (does the socket close? do live `Connection`s survive?) must be ruled.

**5.17 Two connections genuinely touching the same state — the full list.**
`allow` (all inbound), `ts_guard` (all inbound, per static), `last_init_timestamp` (all outbound), `rng` (all index/jitter draws), `index_to_conn` (all inbound Data demux), `pending_by_index` (all inbound Resp demux), `static_to_conn` (all inbound Init routing + revoke), `persistent_keepalive`, the receive buffer, the outbox flush order, and the single event channel. Everything else (`sessions`, `recovery`, `pending`) is already per-connection and moves into `Connection` cleanly.
