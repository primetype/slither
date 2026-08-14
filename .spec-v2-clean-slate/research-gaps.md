# research-gaps — verification of four load-bearing facts

Verified 2026-08-13 against local checkouts. No design proposals below; each
section states the fact and its consequence for the pending decision.

## hiss local-vs-released status (applies to Facts 1–3)

**The local hiss tree is functionally identical to released 0.3.1.**

- `/Users/nicolasdiprima/work/primetype/hiss/Cargo.toml:8` declares
  `version = "0.3.1"`; `hiss-macros/Cargo.toml:3` declares `version = "0.3.0"`.
- Working tree is **clean** (`git status --short` empty).
- HEAD is **2 commits ahead** of tag `v0.3.1` (`651ac29`, 2026-08-13):
  `50b86fc chore(meta): point both crates' homepage at the site` and
  `7107070 fix(site): canonicalize URLs to primetype.co.uk/hiss`.
- `git diff --stat v0.3.1..HEAD -- src/ hiss-macros/ tests/` reports exactly
  **one changed file, `hiss-macros/Cargo.toml`, +1/-1** (the `homepage` field).

So for every code question below, "in the local tree" and "released in 0.3.1"
are the same answer. Nothing in this report depends on unreleased code.

Note the workspace layout: the proc-macro subcrate is `hiss-macros/` inside the
same repo (`Cargo.toml:1-2` `[workspace] members = ["hiss-macros"]`), published
separately as `hiss-macros`, and pulled in by hiss as a required build-time
dependency (`Cargo.toml`, `hiss-macros = { version = "0.3.0", path = "hiss-macros" }`).

---

## Fact 1 — payload on the FINAL message of a pattern

### Verdict: **SUPPORTED-AS-IS** in released 0.3.1. All three sub-requirements (a), (b), (c) hold, and the exact shape is already exercised by hiss's own third-party-vector test suite.

### (a) The parser accepts `[N]` on the final message

`Payload` is parsed per-line, inside the generic message loop — there is no
special-casing of message index at all:

- `hiss-macros/src/parse.rs:262-282` — the `[N]` suffix is parsed after the
  token list of **any** handshake message line.
- The only three rejections are: `[0]`
  (`parse.rs:270-275`), a payload on a *pre*-message (`parse.rs:298-305`), and a
  payload in marker mode / no suite (`parse.rs:322-330`). **Final-message
  position is not among them.**
- The module doc states the rule positively —
  `parse.rs:26-28`: "the message carries a 12-byte application payload as its
  tail (Noise sanctions a payload on **every** handshake message)".

### (b) The generated write method takes the payload as a parameter

`gen_write_message` appends the payload argument uniformly, again with no
index special-case:

- `hiss-macros/src/codegen.rs:899-903` — `args.extend(quote!(, payload: &[u8; #n]));`
- The declared length feeds the size const identically for every message:
  `codegen.rs:376-393` and `codegen.rs:409-417` (`... #payload_term;`).

For the final message the write returns the message **and** the `Transport`,
because the "next state" is computed by one shared helper (see (c)).

### (c) The generated read method surfaces the payload alongside `Transport`

This is the crucial interplay, and it resolves cleanly. Two pieces:

`hiss-macros/src/codegen.rs:718-730` — `next_state()` returns
`Transport<Name>` / `into_transport(...)` when `msg + 1 == messages.len()`,
and a normal state struct otherwise:

```rust
fn next_state(ctx: &Ctx<'_>, role: Role, msg: usize) -> (TokenStream, TokenStream) {
    if msg + 1 == ctx.input.messages.len() {
        (quote!(::hiss::noise::Transport<#name>),
         quote!(::hiss::noise::support::into_transport::<#name, #role_ty, CP>(self.inner)))
```

`hiss-macros/src/codegen.rs:1166-1198` — the payload arm wraps whatever
`next_state` produced into a **tuple**:

```rust
        Some(payload) => (
            quote!(([u8; #n], #next_ty)),                       // return type
            quote! { let mut payload = [0u8; #n];
                     #support::recv_tail(&mut self.inner, &message[cursor..], &mut payload)?; },
            quote!((payload, #next_expr)),                      // Ok(...)
        ),
```

So for a final message with `[N]` the generated signature is
`fn read_message_K(self, message: &[u8; SIZE]) -> Result<([u8; N], Transport<Name>), HandshakeError>`
— **payload and Transport together, from one call**. The ordering is safe in
the direction slither needs: `recv_tail` runs (and `?`-propagates a bad tag) at
`codegen.rs:1173`, *before* `into_transport` is evaluated in the `Ok` expression
at `codegen.rs:1175`. A tampered payload therefore yields **neither** payload
**nor** a `Transport`. The codegen doc text says the same in prose
(`codegen.rs:834-843`).

### Existing exercise of exactly slither's proposed shape

Not merely theoretical — hiss's Cacophony third-party vector suite declares
**IK with a payload on both messages, including the final one**:

`/Users/nicolasdiprima/work/primetype/hiss/tests/noise_cacophony.rs:343`:

```rust
hiss::noise! { pub IK<$curve, ChaChaPoly, $hash> { <- s ... -> e, es, s, ss [16] <- e, ee, se [15] } }
```

and the read side at `tests/noise_cacophony.rs:515-521` destructures precisely
the tuple described above:

```rust
// msg2: <- e, ee, se [15] — final, so the reader yields the
// `Transport` alongside the recovered payload.
let (got, mut transport) = hs.read_message_2(&msg2).unwrap();
```

with the write side at `tests/noise_cacophony.rs:1072-1076`
(`let (msg2, mut transport) = hs.write_message_2(&payload(...))`). This runs
against frozen third-party Cacophony ciphertexts across every curve/hash the
macro supports. **Reachable from the tag**: `git show v0.3.1:tests/noise_cacophony.rs`
contains the same IK line at :343.

Additional coverage: `tests/noise_macro_shapes.rs:82-83` declares
`IKPayload`/`NNPayload`, and the four compile-fail cases in `tests/ui/`
(`payload_zero`, `payload_on_premessage`, `payload_marker_mode`,
`payload_not_integer`) pin the parser's rejections — none of which is
"final message".

### Consequence for the architecture

**The decision stands unamended.** Declaring
`-> e, es, s, ss [13]` / `<- e, ee, se [1]` needs **no hiss change and no hiss
release**; it is expressible against crates.io `hiss = "0.3.1"` today. slither's
current invocation (`src/handshake.rs:75-81`) changes only in the two bracket
literals. The msg2 payload will be **encrypted and authenticated** (the cipher
is keyed after `e, ee, se`), which is what the architecture assumed.

The consequential change is arithmetic, not capability — the ratified wire
constants move, exactly as a redesign expects:
`src/wire.rs:81-83` `IK_MSG1_LEN` 174 → 175 (`65 + (65+16) + (13+16)`), and
`src/wire.rs:87` `IK_MSG2_LEN` 81 → 82 (`65 + (1+16)`; the const's current
doc-comment "the plaintext ephemeral and the **empty-payload** tag" must be
rewritten). Both are pinned by `src/handshake.rs:97` (`IK::MSG1_SIZE ==
IK_MSG1_LEN`), so the compile-time asserts will catch any mismatch. Also note
`IK_MSG2_LEN` gains a payload term that the current expression does not have.

---

## Fact 2 — the "split msg1 read" and `next_counter()` proposals

### Verdict (a) split read of message 1: **NOT PRESENT** — not in 0.3.1, not in the local tree.
### Verdict (b) `DatagramSend::next_counter()`: **NOT PRESENT** — not in 0.3.1, not in the local tree.

### Evidence

A repo-wide search over the whole hiss checkout (excluding `target/`) for
`MidRead`, `_intro`, `Mid::complete`, `read_message_1_intro` returns **zero
matches** in any `.rs` or `.md` file — including `CHANGELOG.md`, `README.md`,
`TODO.md`, and the `hiss-interop`/`hiss-aesgcm-lab` sibling crates.

Likewise `next_counter` and `fn counter` return **zero matches** anywhere,
including `src/noise/datagram.rs` (the file that defines `DatagramSend` /
`DatagramRecv`).

`TODO.md` — hiss's own short-term direction file — lists only the AES-GCM
cipher and additional hardware providers. **Neither proposal appears on it**,
so neither is scheduled work from hiss's side.

The codegen confirms the shape of what *does* exist: reads come in exactly
three styles (`hiss-macros/src/codegen.rs:930-943` `enum ReadStyle`
`{ Plain, Lookup, Verify }`), and a message gets at most **two** generated
methods — the plain `read_message_N` plus, when applicable, a single
`read_message_N_with` (`codegen.rs:958-984`). There is no third, two-phase
method and no intermediate `Mid`-style state type in the generated surface.

### Important nuance — the Claimed stage may not need the split read

The existing `_with` variant already delivers the property the staged accept
was reaching for. `verify_on_read` (`codegen.rs:944-956`) gates it, and its
doc-comment states the cost model explicitly:

> "on a non-final message the closure rejects the claimed identity before the
> message's remaining DH tokens are computed, so an unwanted peer costs no
> further provider work (**on IK's first message, rejection costs the responder
> exactly the one `es` DH**)."

`ReadStyle::Verify` is described at `codegen.rs:939-942` as "the message reveals
the peer's static: a verification closure sees the identity **as soon as it is
decrypted, before any of the message's remaining tokens are processed**" — i.e.
after `es`, before `ss`. That is exactly the "recover the claimed static without
yet doing `ss`" requirement as stated. slither already relies on this today
(`src/handshake.rs:26-38` documents `read_message_1_with` in precisely these
terms, and `src/handshake.rs:482` is the allow-list closure).

What the existing `_with` does **not** give is the ability to *return* from the
read at the Claimed point and resume later — the closure must decide
synchronously, inside the read. If the staged accept needs to suspend (await an
async policy lookup, park the claim across event-loop turns), the closure is
insufficient and a genuine two-phase API is required. If it only needs to
*decide* on the claim before `ss` is spent, the closure already suffices.

### Consequence for the architecture

**Both items need amending.**

(b) `next_counter()` is unambiguously blocking: constructing an AD header
*before* sealing requires knowing the counter `encrypt_next` will use, and
0.3.1 exposes no such accessor. The spec **must keep its "gated on a hiss 0.3.x
minor" framing** for this item. (Workaround space exists — slither could track
the counter itself in parallel with `DatagramSend` — but that duplicates state
hiss owns and is a design decision, not a fact, so it is out of scope here.)

(a) needs a **narrower** amendment than the spec currently assumes. The
architecture should first settle whether the Claimed stage must *suspend* or
merely *decide*. If merely decide, drop the hiss dependency for this item
entirely and build the Claimed stage on `read_message_1_with`, which ships in
0.3.1 and which slither already uses. Only if it must suspend does this stay
"gated on a hiss 0.3.x minor". Either way the current framing — that a split
read has landed — is false and must be corrected.

---

## Fact 3 — curve-generic canonical public-key encoding

### Verdict: **NEEDS A CAVEAT — two of them, and the first is blocking for the word "generically".**

### The trait surface

`/Users/nicolasdiprima/work/primetype/hiss/src/curve/mod.rs:52-72`:

```rust
pub trait Curve {
    const NAME: &'static str;
    const PUBLIC_KEY_SIZE: usize;
    const PRIVATE_KEY_SIZE: usize;
    type Error: std::error::Error + Send + Sync + 'static;
    type PublicKey: Clone;
    fn public_key_from_bytes(bytes: &[u8]) -> Result<Self::PublicKey, Self::Error>;
}
```

Two structural facts follow directly:

1. **`Curve` has a decoder but no encoder.** `public_key_from_bytes`
   (`src/curve/mod.rs:72`) is the only key-serialisation function in the trait.
   There is no `to_bytes` / `as_bytes` / `encode` counterpart.
2. **`type PublicKey: Clone`** (`src/curve/mod.rs:69`) — the associated type is
   bounded by `Clone` **and nothing else**. Not `AsRef<[u8]>`, not `Ord`, not
   `PartialOrd`. A grep for other bounds across `src/` finds only uses of
   `C::PublicKey` (in `provider/mod.rs`, `noise/transport.rs`), never an
   additional bound on it.

`PUBLIC_KEY_SIZE` is also a plain `usize` associated const, not a const
generic — so `[u8; C::PUBLIC_KEY_SIZE]` is not writable in generic code on
stable Rust.

### The concrete impls (encoding is stable and canonical *per type*)

| Curve | `PUBLIC_KEY_SIZE` | stored form | `AsRef<[u8]>` yields | `Packed::SIZE` |
|---|---|---|---|---|
| P256 (`p256/mod.rs:59-70`) | **65** | 65-B uncompressed SEC1 `0x04‖X‖Y` | 65 B (`p256/mod.rs:351-355`) | **33** (`p256/mod.rs:326-328`) |
| X25519 (`x25519.rs:76-87`) | 32 | raw u-coordinate | 32 B (`x25519.rs:121-125`) | 32 (derived `Packed`, `x25519.rs:99-100`) |
| X448 (`x448.rs:72-83`) | 56 | raw u-coordinate | 56 B (`x448.rs:117`) | 56 (derived, `x448.rs:95-96`) |
| Ed25519 (`ed25519.rs:93-104`) | 32 | raw | 32 B (`ed25519.rs:136`) | 32 (derived, `ed25519.rs:113-114`) |

Canonicality **is** solid at the concrete-type level for P-256, which matters
most: `src/curve/p256/mod.rs:145-150` documents and enforces normalisation —

> "Stored internally as the 65-byte uncompressed SEC1 encoding (`0x04 ‖ X ‖ Y`),
> **regardless of the encoding it was parsed from**."

`from_bytes` accepts `0x04`/`0x02`/`0x03` and decompresses to the 65-byte form
(`p256/mod.rs:216-236`), so two `P256r1PublicKey` values denoting the same point
are byte-identical however they arrived. `to_compressed()`
(`p256/mod.rs:205-214`) derives the 33-byte form deterministically from that
storage (prefix from Y-parity, X copied). Both encodings are therefore
canonical *functions of the point*; there is no ambiguity **within** an
encoding.

Ordering: every public-key type derives `PartialOrd, Ord` over its stored array
(`p256/mod.rs:149-150`, `x25519.rs:99-100`, `x448.rs:95-96`), and because P-256
storage is normalised, that derived `Ord` is a stable lexicographic order over
the **65-byte uncompressed** octets.

### Caveat 1 (blocking): "generically per curve" is not expressible against `Curve` in 0.3.1

Because `Curve::PublicKey: Clone` is the whole bound, generic code holding a
`C::PublicKey` **cannot** obtain its octets or compare two of them. Every
concrete type happens to impl `AsRef<[u8]>` and `Ord`, but a generic function
cannot use impls the trait does not require. The method names are also
inconsistent — P-256 spells it `to_bytes(&self) -> &[u8]`
(`p256/mod.rs:201-203`) while the others spell it `as_bytes(&self) -> &[u8; N]`
(`x25519.rs:116-118`, `x448.rs:112`, `ed25519.rs:126`) — so even a macro over a
fixed curve list would have to special-case P-256.

**Smallest hiss change that would provide it:** add `AsRef<[u8]> + Ord` to the
`Curve::PublicKey` bound at `src/curve/mod.rs:69`. Every shipped curve already
satisfies both, so no impl in hiss changes. It is technically breaking for any
*downstream* implementor of `Curve`, so it wants a ruling, not a patch release
assumption. (An `encode`/`to_canonical_bytes` method on `Curve` would be the
alternative, and would additionally let hiss *name* which encoding is canonical
— see Caveat 2.)

### Caveat 2: "the canonical encoding" is ambiguous for P-256 — the one curve slither uses

P-256 has **two** live encodings inside hiss, and they disagree on length:

- 65-byte uncompressed — `PUBLIC_KEY_SIZE` (`p256/mod.rs:61`), `to_bytes()`
  (`p256/mod.rs:201`), `AsRef<[u8]>` (`p256/mod.rs:351-355`), the derived `Ord`.
- 33-byte compressed — `to_compressed()` (`p256/mod.rs:209`) and
  `impl Packed for P256r1PublicKey { const SIZE: usize = 33; }`
  (`p256/mod.rs:326-328`).

X25519/X448/Ed25519 have exactly one encoding each, so this is a P-256-specific
split. And slither **already depends on the distinction in both directions**:

- The Noise wire carries the **65-byte uncompressed** form —
  `src/wire.rs:81-83` and `:87` build `IK_MSG1_LEN`/`IK_MSG2_LEN` from
  `SlitherChannel::PUBLIC_KEY_SIZE` (65).
- mac1 keys off the **33-byte compressed** form —
  `src/mac.rs:37` (`COMPRESSED_KEY_LEN: usize = 33`) and `src/mac.rs:46`
  (`let compressed: [u8; 33] = recipient.to_compressed();`), documented at
  `src/mac.rs:9-10` as
  `key = BLAKE2b-256(b"slither mac1" ‖ recipient_static_pub_compressed[33])`.
- Identity indexing likewise uses the 33-byte form: `src/handshake.rs:482`,
  `src/handshake.rs:525`, `src/handshake.rs:561`, `src/endpoint.rs:215`,
  `src/endpoint.rs:529-532`, and the `HashSet<[u8; 33]>` allow-lists at
  `src/flow.rs:446` and `src/flow.rs:973`.

### Consequence for the architecture

**Both halves of the decision need amending, and one of them has a wire-byte trap.**

*Generic mac1 keying:* as written ("generically per curve") it does not
compile against hiss 0.3.1. Either (i) drop the genericity and keep mac1
concrete over `P256r1PublicKey` as slither does today (no hiss change, no
release gate), or (ii) define a slither-local helper trait with a per-curve impl
naming the encoding explicitly, or (iii) take the hiss change above — which is
a ratification question, not a patch.

*The wire-byte trap:* if genericity is obtained via `AsRef<[u8]>` (the obvious
route), P-256 silently switches mac1 keying from the **33-byte compressed** form
to the **65-byte uncompressed** form, changing every mac1 tag on the wire. The
frozen `SPEC.md` §mac1 names the compressed encoding, and
`handshake::tests::golden_wire_is_byte_identical_to_the_pre_migration_driver`
would go red. Any generic formulation **must pin the encoding explicitly**
(compressed for P-256) rather than inheriting whatever `AsRef` returns.

*The simultaneous-open tie-break:* "compare statics as octet strings" is
well-defined only once the encoding is named — comparing 33-byte compressed
octets and comparing 65-byte uncompressed octets produce **different orders**
in general (the compressed prefix encodes Y-parity, and the uncompressed form
sorts on a full Y that the compressed form omits). The architecture must state
which encoding the comparison is over; recommend the 33-byte compressed form
for consistency with mac1 and the existing allow-list keys. With the encoding
named, the comparison is stable and canonical (P-256 storage normalisation
guarantees a single octet string per point), and can be implemented today via
the concrete type's derived `Ord` *only if* the chosen encoding is the
uncompressed one — otherwise it must compare `to_compressed()` arrays
explicitly, since the derived `Ord` sorts on the 65-byte storage.

---

## Fact 4 — DATAGRAM receive-queue policy precedent

### Verdict: **drop-OLDEST has clear precedent; bounding by COUNT (64) does not.** quinn-proto bounds both queues by **bytes**, with defaults three to four orders of magnitude larger than 64 datagrams, and has **no count cap at all**.

Source: quinn-proto 0.11.16 at
`/Users/nicolasdiprima/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/quinn-proto-0.11.16/src/`.

### Receive side — drop-oldest, byte-bounded

State (`connection/datagrams.rs:102-111`) is `recv_buffered: usize` — a **byte**
total — plus `incoming: VecDeque<Datagram>`. Default limit
`connection/../config/transport.rs:384`:

```rust
datagram_receive_buffer_size: Some(STREAM_RWND as usize),   // 1_250_000 bytes
```

(`STREAM_RWND` derived at `config/transport.rs:355-359`.) `None` disables
incoming datagrams entirely.

The eviction is unambiguously **head-drop / tail-insert**
(`connection/datagrams.rs:132-139`):

```rust
let was_empty = self.recv_buffered == 0;
while datagram.data.len() + self.recv_buffered > window {
    debug!("dropping stale datagram");
    self.recv();                       // pop_front, datagrams.rs:186-190
}
self.recv_buffered += datagram.data.len();
self.incoming.push_back(datagram);
```

The just-arrived datagram is **always** accepted; already-queued ones are
evicted from the front. This is drop-oldest, matching the architecture's choice
of direction.

Two behaviours that differ from a naive queue and are worth carrying across:

- A **single datagram larger than the whole buffer is not a drop — it is fatal
  to the connection** (`connection/datagrams.rs:128-130`,
  `TransportError::PROTOCOL_VIOLATION("oversized datagram")`). This also
  guarantees the `while` loop terminates (it cannot spin on an empty deque).
- Receiving a DATAGRAM when locally disabled is likewise connection-fatal
  (`connection/datagrams.rs:119-124`).

### Send side — caller chooses drop-oldest *or* backpressure

`Datagrams::send(&mut self, data: Bytes, drop: bool)`
(`connection/datagrams.rs:28`), byte-bounded at `1024 * 1024` by default
(`config/transport.rs:385`). `connection/datagrams.rs:38-54`:

- `drop == true` → drop-oldest via `pop_front`.
- `drop == false` → `Err(SendDatagramError::Blocked(data))`, handing the bytes
  back and setting `send_blocked`, later cleared by `Event::DatagramsUnblocked`
  (`connection/mod.rs:3342-3345`).

`SendDatagramError::TooLarge` (`connection/datagrams.rs:35-37`) is a separate
size check against path MTU / peer `max_datagram_frame_size`, not a queue
condition. Note the drop loop tests `outgoing_total` *without* adding
`data.len()` and uses strict `>`, so the send queue can overshoot its limit by
up to one datagram.

### Drop observability — quinn has none worth copying

The only receive-side signal is `debug!("dropping stale datagram")`
(`connection/datagrams.rs:134`). There is **no counter** in
`connection/stats.rs` and **no event** variant. Worse, `was_empty` is sampled
at `connection/datagrams.rs:132` *before* the eviction loop, so a datagram that
causes an eviction emits no `Event::DatagramReceived` either — the application
learns nothing about the drop from the event stream.

### RFC 9221

The RFC neither prescribes nor forbids any policy. §5.3: "DATAGRAM frames do
not provide any explicit flow control signaling and do not contribute to any
per-flow or connection-wide data limit." And on the receiver: "However, since
DATAGRAM frames are inherently unreliable, they **MAY** be dropped by the
receiver if the receiver cannot process them." The `max_datagram_frame_size`
transport parameter (§3) bounds **the size of one frame**, not queue depth. The
RFC says nothing about oldest-vs-newest or about queueing discipline. So the
drop policy is entirely an implementation choice, and quinn is precedent rather
than a standard.

### Consequence for the architecture

**Drop-oldest stands; "64 each" needs amending or explicit justification.**

The *direction* is well-precedented — quinn drops oldest on receive
unconditionally, and offers drop-oldest as the default-ish send mode. Keep it.

The *bound* is materially different in both dimension and magnitude. quinn
bounds by bytes (1.25 MB recv / 1 MiB send) and never by count; a 64-datagram
count cap is a much tighter and qualitatively different limit. With slither's
`MAX_PLAINTEXT` (`src/wire.rs:75`), 64 datagrams is on the order of tens of KB
— roughly two orders of magnitude below quinn's receive window. That may well
be right for slither (a `!Send` single-actor endpoint has different memory
posture than a general QUIC stack), but the architecture should say so
deliberately rather than implying quinn precedent for the number. Two concrete
amendments:

1. State whether 64 is a **count** cap by intent, and if so justify it against
   quinn's byte bound — or restate the bound in bytes. A count cap admits an
   adversarial small-datagram flood that a byte cap does not, and conversely
   bounds worst-case memory much more crisply.
2. Decide the **oversize** case explicitly. quinn treats a single datagram
   larger than the whole buffer as a protocol violation that kills the
   connection, not as a drop. slither's fixed `MAX_PLAINTEXT` may make this
   unrepresentable — if so, say that; if not, the case needs a ruling.

One improvement over the precedent worth taking: quinn's silent drop (log only,
no counter, no event) is a known operability weakness. slither already emits
DoS-gate drop counters via `tracing` (per `CLAUDE.md` and `src/mac.rs`
conventions), so surfacing a queue-overflow counter costs nothing and is
strictly better than what quinn does.
