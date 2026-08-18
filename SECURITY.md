# Security Policy

`slither` is a WireGuard-shaped Noise-over-UDP packet layer: an
authenticated, encrypted datagram session (Noise **IK** over
**P-256 / ChaCha20-Poly1305 / BLAKE2b**, via
[`hiss`](https://github.com/primetype/hiss)) carrying a reliable frame layer
inside the sealed packets. This document covers what the *protocol layer*
does and does not defend against. For the cryptographic primitives, the
per-pattern Noise properties, side-channel posture and validated test
vectors, see **hiss's `SECURITY.md`** — everything Noise-level is inherited
from there.

## Reporting a vulnerability

**Please report security issues privately. Do not open a public issue or PR
for a suspected vulnerability.**

Use GitHub's private vulnerability reporting:

➡️ **<https://github.com/primetype/slither/security/advisories/new>**

(Repository → **Security** tab → **Report a vulnerability**.)

Please include enough detail to reproduce — the affected packet/frame path,
a proof of concept or failing test if you have one, and the version/commit.

### Supported versions

Only the latest published release receives security fixes.

## What a session guarantees

Within an established session, each accepted Data packet is confidential,
authenticated, and accepted **exactly once** (the RFC 6479-shaped replay
window suppresses duplicates and replays). The frame layer on top adds
reliable, unordered, exactly-once *message* delivery; its retransmissions
ride fresh packet counters, so reliability never weakens the replay rule.

## Threat model highlights

- **mac1 is a DoS gate, not an authenticator.** The 16-byte tag on both
  handshake packets is keyed by the *recipient's public static* — anyone who
  knows that public key can compute it. Its job is to make a garbage flood
  and mis-addressed packets droppable before any curve/DH work; the real
  authentication is the Noise IK handshake underneath.
- **The msg1 timestamp payload is 0-RTT.** Per Noise §7.7, an IK msg1
  payload is authenticated only by the static–static DH: it is replayable
  and KCI-forgeable in isolation. slither carries only the 12-byte
  initiation timestamp there and applies WireGuard's defence — a responder
  accepts an initiation only with a strictly greater timestamp per static
  — so a recorded msg1 cannot re-establish a session. Responder cost is
  staged (a ratified property, pinned by test): mac1 rejects before any DH,
  an unlisted static costs exactly one DH (`es`), and the timestamp is
  checked as soon as the msg1 payload opens, before the msg2 DHs.
- **Roaming trusts the seal, not the source address.** A session's endpoint
  moves only when a packet from the new address *authenticates and is
  replay-fresh*. An off-path attacker cannot redirect a session; an on-path
  attacker who can drop and re-inject traffic can, as in WireGuard.
- **Admission is application-driven, not a slither allow-list.** slither
  holds no list of permitted statics. The staged accept ladder
  (`Intro` → `Claimed` → `Proven` → `Connection`) hands the application
  the claimed static after 1 DH; the application decides whether to
  continue (`authenticate()`) or reject (drop the object — no bytes
  sent). Revoking an established peer is the application's own job:
  nothing here re-checks a list once a connection is up.
- **Randomness.** Handshake ephemerals are drawn from a caller-supplied
  CSPRNG via hiss; every handshake *retransmit* uses a fresh ephemeral (the
  WireGuard requirement). The endpoint's index/jitter CSPRNG is a
  `ChaCha20Rng` seeded from OS entropy (`getrandom`). **The seed is not
  currently zeroized after use** — it is a plain `[u8; 32]` on the stack,
  dropped without an explicit wipe.

## Known limitations and non-goals (v1, ratified)

- **No cookies / mac2**: under a spoofed-source flood, mac1 bounds the work
  per packet to one keyed hash, but there is no per-source cookie challenge;
  CPU-exhaustion resistance is weaker than WireGuard's full design.
- **Congestion control has no pacing** (ratified out of v1, §14.7): NewReno
  backs off on loss (RFC 9002 recovery + PTO), but sends are not paced to
  sub-RTT smoothness — a 12 KB initial window bounds bursts, but there is
  no ECN and no alternate controller (CUBIC/BBR). Evaluate burstiness
  before pointing it at the open internet at scale.
- **Traffic analysis is out of scope**: packet sizes, timing, and the
  cleartext header fields (type, version, indices, counter) are visible.
  There is no padding.
- **No persistence**: reliability lives within a connection; what a dead
  connection had not delivered is lost.
- **No PSK / first-contact secrecy gating**: slither v1 is plain IK. The
  initiator's static is hidden from a passive observer, with the standard
  IK caveats (see hiss's per-pattern table).
