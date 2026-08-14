# Ruling 34 sweep — every "15 s" / `DEAD_TIMEOUT` occurrence, dispositioned

Prepared 2026/08/14 against draft v4 (3259 lines) **before** the routing
pass edits it, so **line numbers will have shifted** — match on the quoted
context, not the line number. The hazard this list exists to prevent:
`INTRO_TTL` is *also* 15 s, and one flood-arithmetic figure is derived
from it, so a blind find-and-replace corrupts the intro-queue bounds.

## CHANGE — the death timeout and prose derived from it (15 s → 25 s)

| Where | Quoted context | Note |
|---|---|---|
| §5.7 timers table | `` `DEAD_TIMEOUT` \| 15 s \| keepalive + one retransmit interval of grace `` | value **and** derivation note → "2 × `KEEPALIVE_TIMEOUT` + 5 s grace" |
| §6.7 tie-break | "would install different key sets on the two sides and go mutually dark for 15 s" | liveness-derived |
| §6.x | "receives nothing it can open, dies at liveness within 15 s (§7.5)" | liveness-derived |
| §7.3 amplification | "dies by liveness inside 15 s (no deadlock)" | liveness-derived |
| §7.4 | "A sender writing into a black hole therefore dies 15 s after its last authenticated receive" | liveness-derived; ruling 33 also rewrites this clause's arming condition |
| §7.5 constants table | `` `DEAD_TIMEOUT` \| 15 s `` | value |
| §7.5 liveness bullet | "receives nothing authenticated for 15 s is dead" | liveness-derived |
| §15.1 CLOSE | "a clean disconnect costs the peer 15 s of liveness wait" | liveness-derived |
| §15.4 table | "liveness — 15 s without an authenticated fresh receive (§7.5)" | liveness-derived |
| §15.4 table | "liveness, ≤ 15 s" (endpoint-dropped row) | liveness-derived |
| §17.1 honesty clause | "a half-open session reaped by liveness in 15 s (WireGuard accepts the same…)" | liveness-derived |
| §17.1 honesty clause | "the resulting half-open session dies at 15 s liveness and leaks nothing" | liveness-derived |
| final constants table | `` `KEEPALIVE_TIMEOUT` / `DEAD_TIMEOUT` \| 10 s / 15 s `` | → 10 s / 25 s |

## DO NOT CHANGE — `INTRO_TTL` and its arithmetic (stays 15 s)

| Where | Quoted context | Why |
|---|---|---|
| §6.3 constants table | `` `INTRO_TTL` \| **15 s** after the entry's last refresh `` | intro-queue TTL, unrelated timer |
| §6.3 flood posture | "the per-source cap, and the 15 s TTL — is a ruled posture" | `INTRO_TTL` |
| §6.3 expiry | "A consumed chain's mid-state expires 15 s after the initiation that fed it" | `INTRO_TTL` |
| §6.9 attacker cost | "≈ 68 packets/second (1024 / 15 s)" | **arithmetic derived from `INTRO_TTL`** — 1024/15 = 68.3. Changing the divisor would silently falsify the flood bound |
| final constants table | `` `INTRO_QUEUE_CAP` / `INTRO_MAX_PER_SOURCE` / `INTRO_TTL` \| 1024 / 4 / 15 s `` | `INTRO_TTL` |

## NAME-ONLY references — no value in the text, nothing to edit

§7.4 death condition (`now − last_authenticated_recv > DEAD_TIMEOUT`);
§7.5 liveness bullet's opening; §7.7 epoch-death clause (SECV4-10 rewrites
it anyway); §13.3 probe-train sentence (ruling 33 touches it); §16.x
persistent-keepalive floor ("intervals below `DEAD_TIMEOUT`"); Appendix B's
two obligations naming it.

## JUDGMENT CALLS for the second pass

1. **§16.5's paused-clock timer family** — "the 5 s/10 s/15 s/25 ms/90 s
   family included". With `DEAD_TIMEOUT` at 25 s the list needs both
   values, since 15 s survives as `INTRO_TTL`: suggest
   "5 s/10 s/15 s/25 s/25 ms/90 s". Do not simply swap 15 → 25.
2. **`PERSISTENT_KEEPALIVE` now sits exactly at its floor.** The default
   is 25 s and ruling 14's floor rejects intervals below `DEAD_TIMEOUT`,
   which becomes 25 s — so 25 ≥ 25 holds, but with zero margin. This is
   not a new bug (the 10 s passive dance, not the persistent beacon, is
   what sustains liveness — §7.5), but the coincidence is worth a
   sentence, and whether to raise the default is a **maintainer call**:
   flag it, do not change it.
3. **`DEAD_TIMEOUT`'s derivation note** was "keepalive + one retransmit
   interval of grace" (10 + 5). At 25 s it becomes
   2 × `KEEPALIVE_TIMEOUT` + 5 s grace — i.e. explicitly one-lost-keepalive
   tolerance, which is exactly what ruling 34 bought. Say that.
4. **§7.5's "lives indefinitely"** and the matching Appendix B obligation
   must gain the residual SECV4-6 names: two *consecutive* lost keepalives
   still end an idle connection, and reconnection is the application's.
