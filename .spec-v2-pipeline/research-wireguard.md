# WireGuard reference practices — research for slither's staged-accept redesign

Sources used (primary, with permalinks/commit context where possible):

- **[Paper]** Donenfeld, *WireGuard: Next Generation Kernel Network Tunnel*, NDSS 2017 / draft revision `e2da747`, https://www.wireguard.com/papers/wireguard.pdf (fetched and read directly, pages 1–16: §§1–7).
- **[Kernel]** Linux kernel, `drivers/net/wireguard/` (mainline mirror `torvalds/linux`, `master` at fetch time 2026-08-13): `noise.c`, `noise.h`, `messages.h`, `cookie.c`, `cookie.h`, `receive.c`, `timers.c`, `timers.h`, `device.c`. Fetched raw via `raw.githubusercontent.com/torvalds/linux/master/drivers/net/wireguard/…` and read verbatim (this is the same code published at `git.zx2c4.com/wireguard-linux-compat`, upstreamed into mainline).
- **[boringtun]** Cloudflare, `cloudflare/boringtun`, `master`: `boringtun/src/noise/handshake.rs`, `boringtun/src/noise/session.rs`, `boringtun/src/device/mod.rs`. Fetched raw and grepped/read verbatim.
- **[wireguard-go]** `WireGuard/wireguard-go`, `master`: `device/constants.go`, `device/receive.go`, `device/queueconstants_default.go`. Fetched raw and read verbatim.

All C/Rust/Go excerpts below are quoted from those live fetches, not from memory.

---

## 1. Initiation processing cost & staging — is every mac1-valid initiation processed fully before a decision?

**No — all three implementations stage the first message so that the peer lookup happens *between* the two DH operations, not after both.** This exactly matches what slither's SPEC.md already asserts ("WireGuard's exact shape") — now with primary-source line citations.

### Kernel (`noise.c`, `wg_noise_handshake_consume_initiation`)

```c
handshake_init(chaining_key, hash, wg->static_identity.static_public);

/* e */
message_ephemeral(e, src->unencrypted_ephemeral, chaining_key, hash);

/* es */
if (!mix_dh(chaining_key, key, wg->static_identity.static_private, e))
        goto out;

/* s */
if (!message_decrypt(s, src->encrypted_static, sizeof(src->encrypted_static), key, hash))
        goto out;

/* Lookup which peer we're actually talking to */
peer = wg_pubkey_hashtable_lookup(wg->peer_hashtable, s);
if (!peer)
        goto out;
handshake = &peer->handshake;

/* ss */
if (!mix_precomputed_dh(chaining_key, key, handshake->precomputed_static_static))
        goto out;

/* {t} */
if (!message_decrypt(t, src->encrypted_timestamp, sizeof(src->encrypted_timestamp), key, hash))
        goto out;
```
(`drivers/net/wireguard/noise.c`, function body starting ~line 592; peer lookup at line 617.)

The function's **first identifying argument is `struct wg_device *wg`, not a peer** — it is inherently "anonymous" until the static key is recovered:
```c
struct wg_peer *
wg_noise_handshake_consume_initiation(struct message_handshake_initiation *src,
                                      struct wg_device *wg);
```
(`noise.h`)

So the cost of an initiation from an **unknown static** is exactly **one live DH (`es`)** plus one AEAD decrypt of the static field, then a hashtable miss → `goto out` → `NULL` returned, packet dropped silently (`net_dbg_skb_ratelimited`, `receive.c` line ~146). No `ss`, no timestamp decrypt, no msg2.

**Important correction to the "2 live DHs" mental model:** for a *known* peer, the `ss` step is **not a live scalar multiplication at all**. `handshake->precomputed_static_static` is computed once, at peer-configuration time:
```c
void wg_noise_precompute_static_static(struct wg_peer *peer)
{
        ...
        curve25519(peer->handshake.precomputed_static_static, ...)
}
```
(`noise.c` line 47, called from peer creation, line 75) and consumed via `mix_precomputed_dh()` (a KDF mix over the cached scalar, not a new EC operation) both when creating msg1 (as initiator) and consuming msg1 (as responder, line 623-625) and when creating msg2. **So the live per-handshake DH cost for a known peer is: 1 live DH (`es`) on message-1 consumption, plus 1 live DH (`ee`) and 1 live DH (`se`) on message-2 creation/consumption — `ss` is amortized to configuration time.** This is a materially different cost shape than "2 DH to reject, 4 DH total to complete" — it's "1 DH to reject (unknown peer), 1 DH + 1 cached mix to authenticate a claim, +2 live DH to finish."

boringtun confirms the same *shape* at the device level even though it does *not* cache `ss` (boringtun recomputes the x25519 DH per handshake — `self.params.static_shared` is precomputed once analogous to the kernel, so it likely follows the same amortization; not fully confirmed from the fetched excerpt, but the field name `static_shared` stored on `handshake.params` strongly suggests the same peer-time precomputation pattern as the kernel).

### boringtun — anonymous decrypt, then hashtable lookup, then peer-bound continuation

Device level (`device/mod.rs`), inside the UDP receive path:
```rust
let peer = match &parsed_packet {
    Packet::HandshakeInit(p) => {
        parse_handshake_anon(private_key, public_key, p)
            .ok()
            .and_then(|hh| {
                d.peers.get(&x25519::PublicKey::from(hh.peer_static_public))
            })
    }
    ...
```
`parse_handshake_anon` is the **device-scoped, peer-less** function that performs `es` and decrypts the claimed static (mirroring the kernel's split). Only its *result* (the recovered static key) is used to do a hashtable lookup (`d.peers.get(...)`) — if that misses, `peer` is `None` and the packet is dropped before any peer-bound `Handshake` object (and hence before `ss`/timestamp) is touched.

Once a peer *is* found, `Handshake::receive_handshake_initialization` (`noise/handshake.rs`) does the rest — `ss` (`self.params.static_shared`) and the timestamp AEAD open/replay check — bound to that specific peer's persistent `Handshake` struct.

### wireguard-go

Not independently re-derived line-by-line in this pass, but `device/receive.go`'s handshake worker (`RoutineHandshake`, shown in §2 below) shows the same mac1 → (mac2 if under load) → **then** hand off to `device.ConsumeMessageInitiation` structure, which internally does the identical anon-decrypt/lookup/authenticate staging (same author, same design — the Go and C implementations are deliberately kept in lockstep).

### Implication for slither

slither's SPEC.md §5 responder algorithm ("the policy gate sits between `s` and `ss`") is *exactly* WireGuard's shape, and the TODO.md's staged-accept table (`Intro`=0 DH, `Claimed`=+1 DH `es`, `Proven`=+1 DH `ss`, `Connection`=+2 DH `ee`,`se`) is a faithful generalization of it — **except** that WireGuard doesn't pay a live DH for `ss` on the accept path at all (it's cached at peer-add time). If hiss/slither's `Identity`/allow-list could similarly precompute a per-peer `ss`-equivalent at `allow()` time, the `Proven` stage would cost **zero additional live DH**, only a KDF mix + AEAD open — worth flagging as a possible future optimization distinct from the staging discipline itself.

---

## 2. Half-open state bounds, and the mac1/mac2 "under load" cookie mechanism

### Bounded global queue for *unprocessed* handshake packets — directly analogous to slither's proposed stage-0 queue

The kernel keeps **one device-global bounded ring buffer** of raw (not-yet-DH-processed) handshake skbs, sized exactly `MAX_QUEUED_INCOMING_HANDSHAKES = 4096` (`messages.h`):

```c
ret = wg_packet_queue_init(&wg->handshake_queue, wg_packet_handshake_receive_worker,
                           MAX_QUEUED_INCOMING_HANDSHAKES);
```
(`device.c`, interface bring-up)

Enqueue path (`receive.c`, `wg_packet_receive`):
```c
if (atomic_read(&wg->handshake_queue_len) > MAX_QUEUED_INCOMING_HANDSHAKES / 2) {
        if (spin_trylock_bh(&wg->handshake_queue.ring.producer_lock)) {
                ret = __ptr_ring_produce(&wg->handshake_queue.ring, skb);
                spin_unlock_bh(&wg->handshake_queue.ring.producer_lock);
        }
} else
        ret = ptr_ring_produce_bh(&wg->handshake_queue.ring, skb);
if (ret) {
drop:
        net_dbg_skb_ratelimited("%s: Dropping handshake packet from %pISpfsc\n", ...);
        goto err;
}
atomic_inc(&wg->handshake_queue_len);
```

This is a **fixed-capacity ring, overflow = silent drop** (no error signalled to the network) — the same shape slither's TODO.md proposes for the stage-0 `Intro` queue ("bounded, overflow drops silently"). Note this queue holds **raw undifferentiated bytes across all peers and all unknown senders** — it is populated *before* mac1 is even checked (mac1 verification happens in the worker, `wg_receive_handshake_packet`, pulled off this same queue), so the bound here is really "how much undifferentiated garbage may be parked awaiting a worker thread," a slightly different (lower) bar than slither's stage-0 (which has already paid mac1). wireguard-go uses an analogous but smaller bound: `QueueHandshakeSize = 1024` (`device/queueconstants_default.go`), a fixed-size Go channel.

### "Under load" trigger — global queue-depth heuristic with 1-second hysteresis, *not* per-packet

```c
static u64 last_under_load;   // "This is global, so that our load calculation applies to the whole system."
...
under_load = atomic_read(&wg->handshake_queue_len) >= MAX_QUEUED_INCOMING_HANDSHAKES / 8;  // >= 512
if (under_load) {
        last_under_load = ktime_get_coarse_boottime_ns();
} else if (last_under_load) {
        under_load = !wg_birthdate_has_expired(last_under_load, 1);   // sticky for 1s
        if (!under_load) last_under_load = 0;
}
```
(`receive.c`, `wg_receive_handshake_packet`, lines ~97–120)

So: "under load" flips on when the **global** parked-handshake queue depth reaches 1/8 of its cap (512 of 4096), and once triggered it **stays true for at least 1 more second** even if the queue drains, to avoid flapping. wireguard-go mirrors this exactly with `UnderLoadAfterTime = time.Second` (`device/constants.go`) and a `device.IsUnderLoad()` global flag.

### What "under load" changes: mac2/cookie becomes mandatory, gated by `wg_cookie_validate_packet`

```c
enum cookie_mac_state wg_cookie_validate_packet(...)
{
        ret = INVALID_MAC;
        compute_mac1(...);
        if (crypto_memneq(computed_mac, macs->mac1, COOKIE_LEN)) goto out;   // mac1 fails -> straight drop, no DH, no cookie reply

        ret = VALID_MAC_BUT_NO_COOKIE;
        if (!check_cookie) goto out;          // not under load: mac1 alone is enough

        make_cookie(cookie, skb, checker);
        compute_mac2(computed_mac, skb->data, skb->len, cookie);
        if (crypto_memneq(computed_mac, macs->mac2, COOKIE_LEN)) goto out;   // under load, mac2 missing/wrong

        ret = VALID_MAC_WITH_COOKIE_BUT_RATELIMITED;
        if (!wg_ratelimiter_allow(skb, dev_net(checker->device->dev))) goto out;  // per-source-IP token bucket, see below

        ret = VALID_MAC_WITH_COOKIE;
out:
        return ret;
}
```
(`cookie.c`)

Caller logic (`receive.c`):
```c
mac_state = wg_cookie_validate_packet(&wg->cookie_checker, skb, under_load);
if ((under_load && mac_state == VALID_MAC_WITH_COOKIE) ||
    (!under_load && mac_state == VALID_MAC_BUT_NO_COOKIE)) {
        packet_needs_cookie = false;
} else if (under_load && mac_state == VALID_MAC_BUT_NO_COOKIE) {
        packet_needs_cookie = true;           // valid mac1, no/bad mac2, under load -> send cookie reply, stop
} else {
        /* drop: either mac1 invalid, or mac2 present+valid but the per-IP ratelimiter said no */
        return;
}
...
if (packet_needs_cookie) {
        wg_packet_send_handshake_cookie(wg, skb, message->sender_index);
        return;                                // no wg_noise_handshake_consume_initiation call — zero DH spent
}
```

**Cost of a cookie reply**: mac1 verify (keyed BLAKE2s, already paid), `make_cookie` (one more keyed BLAKE2s over the *device's* 2-minute secret + source IP/port — **no DH**), and `xchacha20poly1305_encrypt` of the 16-byte cookie (symmetric AEAD, **no DH**). The paper explicitly frames this as the whole point: *"Computing Curve25519 point multiplication is CPU intensive... In order to fend off a CPU-exhaustion attack, if the responder is under load, it may choose not to process a handshake message... but instead to respond with a cookie reply message"* (paper §5.3, p.8) — the cookie reply is deliberately **all symmetric crypto**, asymmetric work is never spent responding to a load-triggered probe.

**Second layer**: even a mac2-valid (cookie-holding) packet is still subject to a **per-source-IP token bucket** (`wg_ratelimiter_allow`, `ratelimiter.c`):
```c
PACKETS_PER_SECOND = 20
PACKETS_BURSTABLE  = 5
PACKET_COST        = NSEC_PER_SEC / PACKETS_PER_SECOND
TOKEN_MAX          = PACKET_COST * PACKETS_BURSTABLE
```
i.e. 20 pps sustained / burst of 5 per source IP, backed by an adaptively-sized hash table (`table_size` scales with `totalram_pages()`, capped, `max_entries = table_size * 8`) garbage-collected every second (`queue_delayed_work(..., HZ)`). This is the **final admission gate before any DH is attempted at all**, independent of whether mac1/mac2 passed.

### Cookie format and how the initiator responds

Cookie reply (`MESSAGE_HANDSHAKE_COOKIE`, type 3): `receiver_index ‖ nonce[24] ‖ encrypted_cookie` (XChaCha20-Poly1305, keyed on `HASH("cookie--" ‖ responder_static)`, with the initiating message's `mac1` as AEAD associated data — binds the reply to the specific initiation that provoked it, preventing a passive-MITM cookie-reply forgery from applying to a different message (paper §5.3, p.8: *"we use the AD field to bind cookie replies to initiation messages"*).

The initiator's reaction is explicitly **not** an immediate retransmission:
> *"Upon receiving this message, if it is valid, the only thing the recipient of this message should do is store the cookie along with the time at which it was received... this cookie reply message should not, by itself, cause a retransmission."* (paper §5.4.7, p.13)
> *"On receipt of the cookie reply message, which will enable the peer to send a new initiation or response message with a valid msg.mac2 that will not be discarded. The peer is not supposed to immediately resend the now valid message. Instead, it should simply store the decrypted cookie value... and wait for the expiration of the Rekey-Timeout timer for retrying a handshake initiation message."* (paper §6.6, p.15)

Kernel/boringtun/wireguard-go all implement this as "just cache it": `wg_cookie_message_consume` (`cookie.c`) decrypts and stores `peer->latest_cookie.{cookie, birthdate, is_valid}`, no send is triggered from that path. The stored cookie is then folded into `mac2` on the *next* retry the existing timer machinery would have sent anyway (`wg_cookie_add_mac_to_packet`, using the cookie only if `< COOKIE_SECRET_MAX_AGE − COOKIE_SECRET_LATENCY` = `120 − 5` seconds old).

### Implication for slither

This is strong external validation for slither's design direction:
- WireGuard's own "under-load" gate is **global and queue-depth-triggered with hysteresis**, not per-connection — a good model for a future global `Endpoint`-level "under load" flag driving mac2, distinct from the per-`Intro` stage-0 queue bound itself.
- WireGuard's stage-0 equivalent (the ring buffer of raw skbs) is **smaller and cheaper than slither's `Intro`** — it's pre-mac1, so it must be tiny/cheap-to-drop (just bytes), matching slither's own "raw ~196 B + addr" design for `Intro` (post-mac1, but still pre-DH).
- The per-source-IP token bucket (20 pps/burst 5) is a second, independent axis of defense orthogonal to the global queue-depth trigger — worth keeping distinct in a deferred-cookies appendix: *global load* decides whether cookies are required at all; *per-IP rate* decides whether a specific source gets to skip the queue even with a valid cookie.

---

## 3. Duplicate/retransmitted initiations — dedup, replace, and the flood-attack timestamp rule

### Per-peer handshake state is a *singleton*, not a queue — a fresh valid initiation unconditionally replaces the peer's in-flight one

The kernel's `struct noise_handshake` is embedded **once** per `wg_peer` (`peer->handshake`), not allocated per-inbound-attempt. `wg_noise_handshake_consume_initiation`, on success, does:
```c
down_write(&handshake->lock);
memcpy(handshake->remote_ephemeral, e, NOISE_PUBLIC_KEY_LEN);
if (memcmp(t, handshake->latest_timestamp, NOISE_TIMESTAMP_LEN) > 0)
        memcpy(handshake->latest_timestamp, t, NOISE_TIMESTAMP_LEN);
memcpy(handshake->hash, hash, NOISE_HASH_LEN);
memcpy(handshake->chaining_key, chaining_key, NOISE_HASH_LEN);
handshake->remote_index = src->sender_index;
...
handshake->state = HANDSHAKE_CONSUMED_INITIATION;
up_write(&handshake->lock);
```
There is no branch checking "is a handshake already pending for this peer" — the newer, timestamp-valid message **overwrites** `remote_ephemeral`, `hash`, `chaining_key`, and `remote_index` in place, discarding whatever partial state the previous in-flight attempt had. This is precisely slither's "replace-with-newest" dedup rule (TODO.md §3), confirmed against real code, not just the paper's prose.

### The two admission gates that decide whether a new initiation is even allowed to replace the old one

```c
down_read(&handshake->lock);
replay_attack = memcmp(t, handshake->latest_timestamp, NOISE_TIMESTAMP_LEN) <= 0;
flood_attack = (s64)handshake->last_initiation_consumption + NSEC_PER_SEC / INITIATIONS_PER_SECOND
               > (s64)ktime_get_coarse_boottime_ns();
up_read(&handshake->lock);
if (replay_attack || flood_attack)
        goto out;
```
(`noise.c`, lines ~632–640; `INITIATIONS_PER_SECOND = 50` from `messages.h`)

Two **independent per-peer** rules, both checked only *after* both DHs and the timestamp AEAD decrypt have already succeeded (they gate *acceptance*, not *cost*):
1. **`replay_attack`** — strict-greater-than rule on the 12-byte TAI64N timestamp versus `handshake->latest_timestamp`, the "greatest timestamp received" state described in the paper.
2. **`flood_attack`** — a **separate, per-peer pacing limiter**: even a perfectly legitimate, correctly-signed, monotonically-increasing-timestamp initiation is rejected if the *previous accepted* consumption for this peer was less than `1/INITIATIONS_PER_SECOND` (20 ms) ago. This caps how fast a single already-known peer can force full-cost handshake-1 processing, independent of the global under-load/cookie mechanism in §2. It is keyed on `last_initiation_consumption` (peer-local, updated monotonically via a "never move backwards" guard at line 651: `if ((s64)(handshake->last_initiation_consumption - initiation_consumption) < 0) handshake->last_initiation_consumption = initiation_consumption;`).

### Greatest-timestamp rule — exact semantics and retransmit interaction

Paper §5.1 (p.7), quoted verbatim:
> *"The responder keeps track of the greatest timestamp received per peer and discards packets containing timestamps less than or equal to it. (In fact, it does not even have to be an accurate timestamp; it simply must be a per-peer monotonically increasing 96-bit number.) If the responder restarts and loses this state, that is not a problem: an initial packet from earlier can be replayed, but it could not possibly disrupt any ongoing secure sessions, since the responder has just restarted."*

**Retransmissions do carry a fresh timestamp** — the paper is explicit that message construction (§5.4.2) calls `TIMESTAMP()` fresh at send time, and §6.4 (Handshake Initiation Retransmission) confirms each retry is *"a new handshake initiation message... constructed (with new random ephemeral keys) and sent"* — new `Ii` (sender index, generated randomly "when this message is sent," §5.4.2), new ephemeral, and (since `TIMESTAMP()` is evaluated at construction) a new, later TAI64N value. This is exactly slither's rule ("a retransmit is always admitted even when the coarse clock has not advanced" via forced strictly-greater local sequencing) and confirms retransmissions are never rejected by the greatest-timestamp guard on the sending side — the wall clock (or slither's forced-increment) guarantees strictly-increasing values across retries.

### Scope of the timestamp-guard state: per-peer-static, not global

`latest_timestamp` is a field of `struct noise_handshake`, which is embedded in `struct wg_peer` — **one instance per configured peer, keyed by that peer's static public key**, never shared across peers or globally on the device. boringtun mirrors this exactly: `last_handshake_timestamp: Tai64N` is a field on the per-peer `Handshake` struct (confirmed by direct fetch of `boringtun/src/noise/handshake.rs`), checked via `if !timestamp.after(&self.last_handshake_timestamp) { return Err(WireGuardError::WrongTai64nTimestamp); }`.

### Implication for slither

- The kernel's real code validates slither's TODO.md dedup rule word-for-word: single in-flight handshake slot per peer, newest-valid-timestamp wins, replaces in place.
- Worth adopting explicitly: WireGuard has **two** independent post-DH admission gates (replay-by-timestamp, and a **separate per-peer pacing/flood limiter** at a fixed rate, 50 Hz in WireGuard's case) — slither's spec currently only has the timestamp guard; a per-peer pacing cap (distinct from the global stage-0 queue bound and distinct from any future per-IP mac2 rate limiter) is a second, orthogonal defense worth at least naming in the deferred-cookies appendix, since it protects a peer's `ss`+timestamp-decrypt cost from being re-spent by a *legitimate* but overly chatty holder of a valid claim.
- The timestamp-guard state must be scoped **per remote static identity**, matching slither's own §5 step 5 ("per initiator static, admit only a strictly greater timestamp") — this is now cross-checked against three independent real implementations (kernel struct layout, boringtun struct layout, and the paper's prose), not just the paper.

---

## 4. Timers & give-up

### Ratified constants (cross-checked across paper §6.1, kernel `messages.h`, and wireguard-go `device/constants.go` — all three agree exactly)

| Constant | Value | Source |
|---|---|---|
| `Rekey-After-Messages` | 2⁶⁰ messages | paper Table 6.1; kernel `REKEY_AFTER_MESSAGES = 1ULL << 60`; go `RekeyAfterMessages = 1<<60` |
| `Reject-After-Messages` | 2⁶⁴ − 2¹³ − 1 messages | paper; kernel `REJECT_AFTER_MESSAGES = U64_MAX - COUNTER_WINDOW_SIZE - 1` (`COUNTER_WINDOW_SIZE = 8192-64 = 8128`, i.e. 2¹³ = 8192, matches); go `RejectAfterMessages = (1<<64) - (1<<13) - 1` |
| `Rekey-After-Time` | 120 s | paper; kernel `REKEY_AFTER_TIME = 120`; go `RekeyAfterTime = 120s` |
| `Reject-After-Time` | 180 s | paper; kernel `REJECT_AFTER_TIME = 180`; go `RejectAfterTime = 180s` |
| `Rekey-Attempt-Time` | 90 s | paper; go `RekeyAttemptTime = 90s`; kernel expresses it only implicitly via `MAX_TIMER_HANDSHAKES = 90 / REKEY_TIMEOUT` |
| `Rekey-Timeout` | 5 s | paper; kernel `REKEY_TIMEOUT = 5`; go `RekeyTimeout = 5s` |
| `Rekey-Timeout-Jitter` | 0–333 ms | paper (§6.1, "additional amount of jitter"); kernel `REKEY_TIMEOUT_JITTER_MAX_JIFFIES = HZ/3`; go `RekeyTimeoutJitterMaxMs = 334` |
| `Keepalive-Timeout` | 10 s | paper; kernel `KEEPALIVE_TIMEOUT = 10`; go `KeepaliveTimeout = 10s` |
| Handshake-initiation rate cap (flood_attack, §3 above) | 50/s (1 per 20 ms) | kernel `INITIATIONS_PER_SECOND = 50`; go `HandshakeInitationRate = time.Second/50` |
| Cookie secret max age | 120 s (refresh every 2 min) | paper §5.3 ("changing every two minutes"); kernel `COOKIE_SECRET_MAX_AGE = 2*60`; go `CookieRefreshTime = 120s` |
| Under-load hysteresis | 1 s | kernel (`wg_birthdate_has_expired(last_under_load, 1)`); go `UnderLoadAfterTime = time.Second` |
| Max queued handshake attempts before giving up | `MAX_TIMER_HANDSHAKES = 90/5 = 18` | kernel `messages.h`; go `MaxTimerHandshakes = 90/5` |
| Zero-key-material delay after give-up | `Reject-After-Time × 3` = 540 s (9 min) | kernel `timers.c`: `jiffies + REJECT_AFTER_TIME * 3 * HZ` |

Note the (documented) design gap the paper itself flags: *"Critically important future work includes adjusting the Rekey-Timeout value to use exponential backoff, instead of the current fixed value."* (paper §6.4, p.15) — **WireGuard's retransmit is fixed-interval + jitter, not exponential backoff**, unlike slither's own current `RETRANSMIT_BASE = 5s` + jitter (which matches WireGuard's *current*, admittedly-suboptimal-by-its-own-admission, fixed shape) — this is directly relevant if slither ever revisits backoff shape.

### Retransmit / give-up mechanics (paper §6.4, cross-checked against kernel `timers.c`)

> *"After sending a handshake initiation message... if a handshake response message is not subsequently received after Rekey-Timeout seconds, a new handshake initiation message is constructed (with new random ephemeral keys) and sent. This reinitiation is attempted for Rekey-Attempt-Time seconds before giving up, though this counter is reset when a peer explicitly attempts to send a new transport data message."* (paper §6.4)

Kernel's actual give-up code (`timers.c`, `wg_expired_retransmit_handshake`):
```c
if (peer->timer_handshake_attempts > MAX_TIMER_HANDSHAKES) {
        pr_debug("... did not complete after %d attempts, giving up\n", ...);
        timer_delete(&peer->timer_send_keepalive);
        /* We drop all packets without a keypair and don't try again,
         * if we try unsuccessfully for too long to make a handshake. */
        wg_packet_purge_staged_packets(peer);
        /* We set a timer for destroying any residue that might be left
         * of a partial exchange. */
        if (!timer_pending(&peer->timer_zero_key_material))
                mod_peer_timer(peer, &peer->timer_zero_key_material,
                               jiffies + REJECT_AFTER_TIME * 3 * HZ);
} else {
        ++peer->timer_handshake_attempts;
        wg_socket_clear_peer_endpoint_src(peer);
        wg_packet_send_queued_handshake_initiation(peer, true);
}
```

**What is dropped at give-up**: (1) all locally-queued outgoing plaintext packets waiting on this handshake (`wg_packet_purge_staged_packets`) — the paper's *"clear all existing packets queued up to be sent"* (§6.1) — immediately; (2) the handshake/ephemeral/key residue itself is *not* wiped immediately, but only after a further, much longer `Reject-After-Time × 3` = 540 s grace timer (`wg_queued_expired_zero_key_material` → `wg_noise_handshake_clear` + `wg_noise_keypairs_clear`), in case a very late response still arrives. So give-up is two-phase: fast queue-purge, slow key-zeroing.

### Implication for slither

slither's `HANDSHAKE_GIVEUP = 90s` matches `Rekey-Attempt-Time` exactly (same value, same role). slither's give-up currently emits a typed `Failed` and (per SPEC.md) presumably drops session/queued-send state at that point; WireGuard's two-phase give-up (fast packet-purge now, slow key-zero much later) is a refinement slither may want to note for its own initiator give-up and, more relevantly, for what happens to `Claimed`/`Proven` intermediate objects that are dropped without ever reaching `accept()` — WireGuard's closest analogue (a responder-side in-flight handshake with no follow-up) has no such grace period at all: it simply gets overwritten by the next initiation or ages out with the peer's normal timers, there's no dedicated "half-open responder give-up timer" distinct from the initiator-side one, because half-open responder state (per §1–§2) is minimal and free to discard.

---

## 5. Replay window — RFC 6479 shape, exact sizes, edge rules

Both real implementations cite RFC 6479 explicitly but use **different window sizes**; neither is 128 bits.

### Kernel: 8192-bit total, 8128-bit usable window, per-keypair

```c
enum counter_values {
        COUNTER_BITS_TOTAL = 8192,
        COUNTER_REDUNDANT_BITS = BITS_PER_LONG,     // 64 on 64-bit
        COUNTER_WINDOW_SIZE = COUNTER_BITS_TOTAL - COUNTER_REDUNDANT_BITS   // 8128
};
```
(`messages.h`)
```c
struct noise_replay_counter {
        u64 counter;
        spinlock_t lock;
        unsigned long backtrack[COUNTER_BITS_TOTAL / BITS_PER_LONG];
};
```
(`noise.h`) — embedded as `receiving_counter` inside `struct noise_keypair` (i.e. **per session/keypair**, reset on every rekey, not peer-lifetime cumulative).

Check function (`receive.c`, explicitly commented as the RFC 6479 algorithm):
```c
/* This is RFC6479, a replay detection bitmap algorithm that avoids bitshifts */
static bool counter_validate(struct noise_replay_counter *counter, u64 their_counter)
{
        ...
        if (unlikely(counter->counter >= REJECT_AFTER_MESSAGES + 1 ||
                     their_counter >= REJECT_AFTER_MESSAGES))
                goto out;                                  // hard nonce-space ceiling, unconditional reject
        ++their_counter;
        if (unlikely((COUNTER_WINDOW_SIZE + their_counter) < counter->counter))
                goto out;                                  // too far behind the window -> reject
        index = their_counter >> ilog2(BITS_PER_LONG);
        if (likely(their_counter > counter->counter)) {
                // counter advances: zero out the newly-uncovered words, then advance
                ...
                WRITE_ONCE(counter->counter, their_counter);
        }
        index &= (COUNTER_BITS_TOTAL / BITS_PER_LONG) - 1;
        ret = !test_and_set_bit(their_counter & (BITS_PER_LONG - 1), &counter->backtrack[index]);
out:
        return ret;
}
```
Edge rules: counter **ahead of** the current greatest → accept, slide window forward, zero the newly-exposed bits, mark bit. Counter **within** the window but **already marked** (duplicate) → `test_and_set_bit` returns true → function returns `false` (reject). Counter **more than `COUNTER_WINDOW_SIZE` behind** the greatest → reject outright, no bit test. Checked **only from the data-receive path, after AEAD decryption/authentication succeeds** (`receive.c` line ~461, inside the transport-data consume path, downstream of the decrypt call) — matching slither's own "checked-and-marked only after `decrypt_at` authenticates" rule.

### boringtun: 1024-bit window, per-session — smaller than the kernel by 8×

```rust
const WORD_SIZE: u64 = 64;
const N_WORDS: u64 = 16;   // "Suffice to reorder 64*16 = 1024 packets; can be increased at will"
const N_BITS: u64 = WORD_SIZE * N_WORDS;   // 1024
```
```rust
struct ReceivingKeyCounterValidator {
    ...
    bitmap: [u64; N_WORDS as usize],
}
```
(`boringtun/src/noise/session.rs`, confirmed by direct raw fetch)

Edge rules, quoted from the file:
```rust
if counter >= self.next { return Ok(()); }                        // ahead of window: accept (will be marked on receive)
if counter + N_BITS < self.next { return Err(WireGuardError::InvalidCounter); }  // too far behind: reject
if !self.check_bit(counter) { Ok(()) } else { Err(WireGuardError::DuplicateCounter) }  // duplicate: reject
```
`receiving_key_counter: Mutex<ReceivingKeyCounterValidator>` is a field of `Session` — **per session/keypair**, same scoping discipline as the kernel, just an 8× smaller window (1024 vs 8128 usable bits).

### Implication for slither

slither's `REPLAY_WINDOW = 128` bits is **far smaller than either reference implementation** (kernel: 8128 usable; boringtun: 1024). Both real implementations pick their size purely as an engineering tradeoff (memory + reordering tolerance) with RFC 6479's bitshift-avoiding algorithm making large windows cheap; slither's tighter 128-bit window is a legitimate but much more conservative choice worth flagging explicitly if UDP path reordering beyond ~128 packets is plausible on the target network (WireGuard picked 1024–8192 specifically to tolerate heavier reordering on real internet paths, per boringtun's own comment). Not a bug, but a deliberate parameter to reconsider or explicitly justify in the v2 spec. Scoping (per-session/per-keypair, checked after AEAD auth) is already correct in slither and matches both references exactly.

---

## 6. Object-model-relevant: per-device vs per-peer state split, and where the timestamp guard lives

### The cryptokey-routing split (paper §2, "Cryptokey Routing")

> *"The interface itself has a private key and a UDP port on which it listens... Each [peer] then has a list of allowed source IPs."* (paper §2, p.4)

This is the origin of the two-level object model WireGuard uses everywhere downstream, and it maps cleanly onto slither's proposed `Endpoint<C>` / `Connection` split in TODO.md:

| WireGuard concept | Scope | slither analogue |
|---|---|---|
| `wg_device` / interface static identity (`noise_static_identity`), `cookie_checker` (2-min secret), `peer_hashtable` (pubkey→peer), `index_hashtable` (session-index→peer/handshake/keypair), the global handshake-queue ring + `handshake_queue_len` + `last_under_load` | **device-global**, one per interface | `Endpoint<C>`: socket + demux by receiver index, global stage-0 queue, (future) global under-load state |
| `wg_peer.handshake` (`noise_handshake`: ephemeral/static/hash/chaining-key/**`latest_timestamp`**/remote_index), `wg_peer.keypairs` (current/previous/next `noise_keypair`, each with its own replay window), `wg_peer.latest_cookie` (cookie *this side* holds for sending mac2 to that peer), all the per-peer timers (`timer_retransmit_handshake`, `timer_send_keepalive`, `timer_new_handshake`, `timer_zero_key_material`, `timer_persistent_keepalive`) | **per configured peer**, keyed by static public key | `Connection` (per remote static) |

Both `peer_hashtable` (lookup by static pubkey, used for the msg1 claimed-identity lookup in §1) and `index_hashtable` (lookup by the random session index, used to route msg2/cookie replies and — critically — **every transport data packet**, since `receiver_index` is the only routing key on the wire) are **device-global**, even though what they resolve *to* is peer-scoped state. This is the same shape slither's TODO.md already assumes ("`Endpoint`... receiver index" for demux, `Connection` objects hanging off it).

### Timestamp-guard scoping — confirmed per-remote-static, both implementations, not global

Already detailed in §3: `latest_timestamp` lives on `noise_handshake` (kernel) / `Handshake.last_handshake_timestamp` (boringtun), both embedded exactly once per configured peer, addressed by that peer's static public key. There is **no device-global "greatest timestamp seen from anyone"** — each peer's clock-monotonicity guard is independent, which is required by the threat model the paper describes (§5.1): a global guard would let one peer's replay attempt block a legitimate initiation from a different peer, and would break the "responder restart is harmless" property (since after restart, per-peer state resets independently and prior sessions with *other* peers are unaffected by one peer's stale replay).

This directly confirms slither's SPEC.md §5 step 5 ("per initiator static, admit only a strictly greater timestamp") is scoped correctly, and gives a concrete rule for the v2 spec: **the timestamp guard belongs on whatever object represents "this remote static, across all its handshake attempts" — i.e., logically pinned to the identity, not to any single in-flight `Intro`/`Claimed`/`Proven` object, and not to the `Endpoint`.** Practically, in slither's staged-typestate model, this likely means the guard state must be attached to a longer-lived per-peer record the `Endpoint` maintains (keyed by static, surviving across individual staged-accept attempts, including ones that were dropped/rejected), not to the ephemeral `Intro`/`Claimed`/`Proven` chain itself, and not global — mirroring exactly how WireGuard keeps it on the persistent `wg_peer`/`Handshake`, separate from the transient per-attempt DH state that gets thrown away on rejection.

### Implication for slither

- The `Endpoint` (global) vs `Connection` (per-peer) split slither is already moving to is a direct structural match for WireGuard's device/peer split — good validation of the TODO.md direction.
- slither needs a **third scoping tier** beyond "global `Endpoint`" and "live `Connection`": a lightweight **per-known-static record** that outlives any single staged-accept attempt and survives rejects, to hold the timestamp guard (and, if adopted, the WireGuard-style per-peer flood-attack pacing counter from §3). This is exactly what `wg_peer` already *is* in WireGuard — worth naming explicitly in the v2 SPEC.md rather than leaving it implicit in "the allow-list," since the allow-list today is described as just a set of permitted keys (SPEC.md §5, "Allow-list"), not a place to hang per-peer mutable state.

---

## Summary of primary-source citations

- Paper (fetched PDF, read directly): pages 3–16, §§1, 2, 5.1, 5.3, 5.4.1–5.4.7, 6.1–6.6.
  https://www.wireguard.com/papers/wireguard.pdf
- Kernel `drivers/net/wireguard/{noise.c,noise.h,messages.h,cookie.c,cookie.h,receive.c,timers.c,timers.h,device.c}`, mainline `torvalds/linux`, fetched via `raw.githubusercontent.com/torvalds/linux/master/drivers/net/wireguard/...` (same code as `git.zx2c4.com/wireguard-linux-compat`).
- boringtun `boringtun/src/noise/{handshake.rs,session.rs}`, `boringtun/src/device/mod.rs`, `github.com/cloudflare/boringtun`, `master`.
- wireguard-go `device/{constants.go,receive.go,queueconstants_default.go}`, `github.com/WireGuard/wireguard-go`, `master`.
