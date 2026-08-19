# Round 44 — Agent A: Hurried Developer Walkthrough

**Role**: Simulated developer, solid tokio experience, zero slither knowledge.
**Goal**: two peers exchange an encrypted UDP message (initiator sends `b"hello"`,
responder prints it), then clean close.
**HEAD at start**: `721167a` — "Round 43: the SECV5 pins land — the one-shot keepalive, the spent-packet separator, the 272 pin" — matches expected 721167a. Good.

---

## Phase 1 — Docs-only walkthrough

### README.md

Solid pitch paragraph (WireGuard-shaped Noise-over-UDP, QUIC-shaped reliable
frames on top). Has a "Usage sketch" code block (`rust,ignore` — not
compile-tested by doctest, flagged as a risk immediately: sketches that
aren't compiled drift from reality).

Key things the sketch shows:
- `Endpoint::builder().identity(identity).wire(socket).config(Config::new()).build()`
- `SoftwareIdentity::from_scalar(my_static_scalar, my_rng)?` — but the sketch
  does not say where `my_static_scalar` or `my_rng` come from, or what types
  they are. No mention of a "generate me a fresh keypair" convenience.
- `endpoint.connect(peer_addr, peer_static)?.await?` — needs `peer_static`,
  the *peer's* public key, but nowhere does the sketch show how to obtain
  *your own* public key to hand to the other side out of band. This is a
  glaring gap for a two-peer demo: I need `my_pubkey` to pass to the other
  process/task, and the README's identity story stops at construction.
- Accept ladder: `endpoint.accept()` → `intro.read_identity()` (1 DH) →
  `claimed.authenticate()` (2 DH) → `proven.accept()`. Clear staged shape,
  reasonably self-explanatory once you buy into "staged accept."
- Runtime shape stated explicitly in a comment: "Inside a tokio
  current-thread runtime + LocalSet (the actor is `!Send`)" — good, this
  answers the "how do I run it" question up front, at least for the
  high-level shape. Doesn't show the actual `LocalSet`/`spawn_local`
  boilerplate though.
- No mention anywhere in README of which `tokio` features are required
  (rt, net, time, macros, rt-multi-thread vs current-thread?), nor of
  `hiss` as a dependency at all (is it needed directly, e.g. for
  `SoftwareIdentity` or `rand_core`, or is everything re-exported through
  `slither::identity`?).
- "Status" section confirms Leg 1 + Leg 2 both ratified/frozen — reassuring
  for a "is this crate finished" gut check.
- No `examples/` directory exists (README says so itself), so there is no
  runnable end-to-end demo to copy from. That's the first real friction
  point for a hurried dev: normally I'd `cargo run --example basic` and
  tweak it. Not available here.

STUMBLE CANDIDATE (not yet counted, will formalize in ledger once I hit it
in Phase 2): own-pubkey derivation is undocumented in README.

Moving to src/lib.rs next (docs.rs front page).

### src/lib.rs

Crate-level docs are dense but well-organized:
- "Shape" diagram: `core::Endpoint`/`core::Connection` (pure) → shell (one
  `!Send` actor via `tokio::task::spawn_local`) → compat. Confirms the
  runtime shape: current-thread runtime + `LocalSet`, spelled out clearly
  ("Nothing on that path requires `Send`, deliberately").
- "Six documentation obligations" section is genuinely useful — obligation
  #3 in particular ("A connection with nothing to say dies — in 25 s, in
  silence") is exactly the kind of gotcha that would blow up a first demo
  if I didn't send anything for a while. Good, it's flagged loudly.
- Obligation #1 shows a `no_run` code snippet for reconnecting — the first
  actual code in lib.rs, and it's a negative example (what NOT to do) more
  than a positive quickstart.
- Module list mentions `identity`, `packet`, `shell`, `config`, `compat`,
  `testutil` (behind `test-util` feature) — but does NOT mention the
  `channel!` macro anywhere in the module list, even though it's the
  first thing you need (see below). It's only cross-referenced deep in
  lib.rs's doc comment about `hiss` re-export ("`channel!` expands to
  absolute `::hiss::…` paths").
- "Features" table: `test-util`, `sink`, `codec`, `tower` — confirms
  nothing's on by default. **No tokio feature guidance anywhere** (rt,
  net, macros, time, rt-multi-thread?). I'll have to guess.
- `pub use compat::block_on` — "the one line that pays that tax" for
  LocalSet — promising, will check compat module next.
- `pub use hiss::noise::SessionId` and `pub use hiss;` — confirms `hiss`
  is a re-export, but the doc comment on the `hiss` re-export (bottom of
  lib.rs) is EXPLICIT: "Your crate must depend on `hiss` itself, on the
  same minor line" — shows exact `Cargo.toml` snippet:
  ```toml
  [dependencies]
  slither = "0.2"
  hiss = "0.3"
  ```
  This directly answers one of my brief's open questions (do I need hiss
  as a direct dep — YES) but I only found it because I read lib.rs in
  full, including the trailing re-export docs. A skimming dev reading
  just the top module docs and the README's usage sketch (which imports
  only `slither::{Config, Endpoint}` and `slither::identity::SoftwareIdentity`,
  no `hiss`) would NOT learn this and would hit a confusing compile error
  later (from `channel!` macro's `::hiss::…` paths not resolving).

### src/identity.rs — SoftwareIdentity

- Explains the `Identity` trait as a *factory* pattern — reasonable, if
  verbose (mostly "why", not "how").
- `SoftwareIdentity<S, R = ChaCha20Rng>` — needs a suite type `S` and an
  RNG type `R`. `S: Handshake<Curve = P256>` bound on the `Identity` impl —
  first sighting of a `Handshake`/`Suite` requirement, but no forward
  pointer here to how you *get* an `S`. I now know I need a "suite" type
  but identity.rs alone doesn't tell me how to declare one.
- Good: a compile-tested (`no_run`) example showing the production RNG
  recipe:
  ```rust
  use rand_chacha::ChaCha20Rng;
  use rand_chacha::rand_core::SeedableRng;
  let mut seed = [0u8; 32];
  getrandom::fill(&mut seed)?;
  let rng = ChaCha20Rng::from_seed(seed);
  // …then `SoftwareIdentity::<MySuite>::generate(rng)`.
  ```
  This tells me I need `rand_chacha` and `getrandom` as direct deps too
  (not mentioned in README or lib.rs). `MySuite` is a placeholder — still
  undefined at this point in my reading.
- `SoftwareIdentity::generate(rng)` is the "give me a fresh keypair"
  constructor (better than `from_scalar` which the README sketch used —
  README's sketch is actually the *worse* / more advanced entry point for
  a newcomer who has no scalar yet).
- `public_static(&self) -> &P256r1PublicKey` is how you read your own
  public key back out — this answers the "how do I learn my own pubkey to
  hand to the peer" question, but it's only discoverable by reading the
  `Identity` trait definition (not surfaced in README or lib.rs at all).

STUMBLE (found via source-adjacent doc reading, not yet hands-on):
**Nothing in README or lib.rs shows declaring a `Suite`/`Channel` type via
`slither::channel!`, yet every constructor (`SoftwareIdentity`,
`Endpoint<I>`, `Connection<S>`) is generic over one.** I had to grep
`packet::suite` to find `channel!`'s doctest, which is a fully compiling
example:
```rust
use hiss::noise::{Blake2b, ChaChaPoly, P256};
slither::channel! {
    pub MySuite<P256, ChaChaPoly, Blake2b>;
}
```
This is buried in `packet/suite.rs` module docs on the `channel!` macro
itself — reachable from docs.rs's crate-root "Macros" sidebar section
(since it's `#[macro_export]`), but NOT linked from README's usage sketch,
NOT linked from lib.rs's "Modules" list prose, and NOT mentioned by name
in the "six documentation obligations" section. A hurried dev copy-pasting
the README sketch would get `Endpoint::<SoftwareIdentity<???>>` and be
stuck on what `???` is with no in-context pointer to `channel!`.

### src/shell/endpoint.rs, src/shell/wire.rs, src/shell/staged.rs, src/compat/rt.rs

- `EndpointBuilder::build()` docs are excellent and precise: panics if
  `identity`/`wire` weren't supplied, **panics outside a `LocalSet`**, and
  the doc explicitly says "Named `build` for the builder convention, but
  note what it does: it spawns a task." Good, no surprises there.
- `shell::wire::Wire` trait: **`impl Wire for tokio::net::UdpSocket` ships
  out of the box** (confirmed in `EndpointBuilder::wire`'s own doc: "A
  `tokio::net::UdpSocket` is a `Wire` out of the box"). Good — I do not
  need to write my own `Wire` impl for the real-UDP case. This is stated
  in-line where I need it (on `.wire()`), which is exactly right.
- `compat::rt::block_on` (re-exported as `slither::block_on`) has a full
  **compiling (`no_run`) example**: builds an endpoint, dials, opens a bi
  stream, writes, finishes, waits for ack — the best piece of copy-paste
  code found in Phase 1, and it is what I will crib from. But it still
  does not show suite/identity construction concretely (`I: slither::Identity
  + 'static` stays abstract) and uses `open_bi()` + raw stream write, not
  `send_message()`. lib.rs's own doc for `pub use compat::block_on` DOES
  point here ("see compat::block_on for the copy-pasteable example") — that
  cross-link works, credit where due.
- `shell::staged::Intro`/`Claimed`/`Proven` docs match the README's ladder
  exactly (`read_identity` 1 DH, `authenticate` 2 DH, `accept` +2 DH) and
  add the crucial "dropping IS the rejection" rule with a `#[must_use]`
  lint backing it up. Solid.

### End of Phase 1 assessment

The docs are unusually thorough and well cross-referenced **once you are
inside the right module**, but the entry path (README to a copy-pasteable
example) is broken in one specific way: **the crate is generic over a
`Suite` type that only `channel!` can produce, and no top-level doc page
shows declaring one.** README's sketch and `compat::block_on`'s sketch
both dodge this by staying generic (`I: Identity`) or by leaving `MySuite`
unexplained. I now believe I have everything I need EXCEPT a worked
suite declaration + a worked pairing of `channel!` output with
`SoftwareIdentity`. Moving to hands-on to see how costly that gap
actually is.

---

## Phase 2 — Hands-on

Scratch project: `cargo init --name hurried_demo` at the scratchpad path.

### Building the manifest, following docs only

- `cargo add slither --path <repo>` — trivial.
- `cargo add hiss@0.3` — per lib.rs's `hiss` re-export doc, which shows the
  exact snippet `hiss = "0.3"` (no `default-features = false`, no feature
  list — that's how I added it). Result: `hiss` pulled in its
  `x25519-cryptoxide` default feature, which I don't need (I need P-256).
  Harmless for compiling, but it means an unmodified copy-paste of the
  README/lib.rs guidance pulls extra dependency surface (`x25519-cryptoxide`
  → more of `cryptoxide`) that a P-256-only consumer doesn't want. Compare:
  slither's OWN `Cargo.toml` uses `hiss = { version = "0.3.2",
  default-features = false }` internally — that detail is in
  `Cargo.toml`'s comments, which are not "docs" a docs.rs reader sees, and
  neither README nor lib.rs nor the `channel!` macro doc mentions it.
- `cargo add rand_chacha` and `cargo add getrandom` — needed per
  identity.rs's RNG-recipe doctest, no version pinned by the docs, `cargo
  add` resolved current versions automatically (0.10.0 / 0.4.3). Not
  really a stumble — this is normal Rust workflow.
- `cargo add tokio -F full` — **STUMBLE**. No doc anywhere (README, lib.rs,
  identity.rs, endpoint.rs, wire.rs) states which tokio features the
  *consumer's own* `Cargo.toml` needs. I know from tokio experience that
  I'll need at least `rt`, `net`, `macros`/`rt` for `LocalSet`, so I
  reached for the common hurried-dev shortcut: `features = ["full"]`.
  This worked, but it is a guess, not something the docs told me — and
  slither's own internal `Cargo.toml` comment (not visible to a docs.rs
  reader) is precise about needing only `rt, net, time, sync, macros` —
  info a consumer never sees rendered anywhere.

### main.rs, following docs only

Wrote the suite declaration via `channel!` (found in Phase 1 via a source
grep for `channel!`/`noise!` — see the Phase 1 stumble above; that
discovery cost is being counted once, here in the ledger, even though the
grep itself happened during the docs-reading phase), the two
`SoftwareIdentity::generate(fresh_rng())` identities (following
identity.rs's RNG-recipe doctest, generalized from `getrandom`+
`ChaCha20Rng` to a small helper function), `Endpoint::builder()...build()`
per README, real `tokio::net::UdpSocket::bind(...)` per wire.rs's promise
that it's a `Wire` "out of the box", the staged accept ladder
(`accept()` → `read_identity()` → `authenticate()` → `accept()`) verbatim
from the README sketch, `send_message`/`recv_message`/`acked`/`close` per
their own doc comments (the `send_message` doc literally states the idiom
`send_message(m).await; acked().await; close().await`, which I copied).

For the runtime shape I used `fn main() { slither::block_on(async move {
... }) }`, copying the shape of `compat::rt::block_on`'s own doctest
(`send_hello`) almost exactly, rather than `#[tokio::main] async fn
main()`. This was a deliberate choice **because I had read block_on's doc
warning** ("If called from inside an existing tokio runtime ... panics").

**Verification experiment — the single most important finding of this
review.** A "solid tokio experience" developer's actual first reflex is
`#[tokio::main] async fn main() { ... Endpoint::builder()...build() ... }`
— that is the idiomatic tokio entry point for virtually every crate. I
tested this directly: wrote a `#[tokio::main]` version calling
`Endpoint::builder()....build()` (which calls `spawn_local` internally)
with no `LocalSet`. Result — a hard runtime panic, **not a compile
error**, with a message that names only tokio internals and never
mentions slither, `LocalSet`, or `slither::block_on`:

```
thread 'main' (...) panicked at .../tokio-1.53.1/src/task/local.rs:446:29:
`spawn_local` called from outside of a `task::LocalSet` or `runtime::LocalRuntime`
```

Nothing about this message tells a developer to go read `slither`'s docs
at all — it reads exactly like a tokio-usage bug on their own part
(perhaps: "did I need `LocalSet` for some tokio feature I forgot?"). A
dev who hadn't happened to read `compat::rt::block_on`'s specific
module-level doc (which is NOT the first thing linked from README, though
it IS linked once from lib.rs's `pub use compat::block_on` line) would
burn real time here — this is the closest thing in this whole exercise to
"give up and pick another crate" territory, because the panic message
gives zero forward pointer back into slither's own solution.

### Compile and run — first full attempt

The full `main.rs` (below) compiled clean on the **first** `cargo build`,
and ran correctly on the **first** `cargo run`, printing:

```
responder received: "hello"
round trip complete
```

This is a genuinely good result for the docs — every piece needed
(suite declaration, identity generation, public-key extraction, real UDP
`Wire`, staged accept, message send/recv, close) was in fact documented
*somewhere* reachable without reading `SPEC.md`, `rulings.md`, or test
files. The cost was concentrated entirely in **discovery order**: the
README's usage sketch and lib.rs's front page do not, by themselves, form
a working path — you have to already know to go looking in
`packet::suite` for `channel!` and in `compat::rt` for `block_on`'s
warning, neither of which is linked from the one place (README) a hurried
dev reads first.

---

## (i) The working code

Ran successfully. Output:

```
   Compiling hurried_demo v0.1.0 (.../scratchpad/hurried)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.56s
     Running `target/debug/hurried_demo`
responder received: "hello"
round trip complete
```

**Cargo.toml:**

```toml
[package]
name = "hurried_demo"
version = "0.1.0"
edition = "2024"

[dependencies]
getrandom = "0.4.3"
hiss = "0.3"
rand_chacha = "0.10.0"
slither = { version = "0.2.0", path = "<path to the slither repo>" }
tokio = { version = "1.53.1", features = ["full"] }
```

**src/main.rs:**

```rust
// Hurried-developer attempt: two slither peers over real UDP loopback,
// initiator sends b"hello", responder receives and prints it, clean close.
//
// Following the README usage sketch + lib.rs docs + identity.rs docs +
// packet::suite's `channel!` doctest + compat::rt::block_on's doctest.

use hiss::noise::{Blake2b, ChaChaPoly, P256};
use rand_chacha::ChaCha20Rng;
use rand_chacha::rand_core::SeedableRng;

// Following packet::suite.rs's `channel!` doctest verbatim.
slither::channel! {
    pub MySuite<P256, ChaChaPoly, Blake2b>;
}

use slither::identity::SoftwareIdentity;
use slither::{Config, Endpoint};

fn fresh_rng() -> ChaCha20Rng {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).expect("OS entropy");
    ChaCha20Rng::from_seed(seed)
}

fn main() {
    // Runtime shape per lib.rs: current-thread runtime + LocalSet, the
    // actor is !Send. slither::block_on pays that tax.
    slither::block_on(async move {
        // Two identities -- following identity.rs's `generate` constructor.
        let initiator_identity: SoftwareIdentity<MySuite> =
            SoftwareIdentity::generate(fresh_rng()).expect("generate initiator identity");
        let responder_identity: SoftwareIdentity<MySuite> =
            SoftwareIdentity::generate(fresh_rng()).expect("generate responder identity");

        // How do I learn my own public key to hand to the peer? identity.rs
        // showed `Identity::public_static()`.
        use slither::identity::Identity as _;
        let responder_static = responder_identity.public_static().clone();

        // Real UDP sockets -- wire.rs says `tokio::net::UdpSocket` is a
        // `Wire` out of the box.
        let initiator_socket = tokio::net::UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind initiator socket");
        let responder_socket = tokio::net::UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind responder socket");
        let responder_addr = responder_socket.local_addr().expect("responder addr");

        let initiator_endpoint: Endpoint<SoftwareIdentity<MySuite>> = Endpoint::builder()
            .identity(initiator_identity)
            .wire(initiator_socket)
            .config(Config::new())
            .build();

        let responder_endpoint: Endpoint<SoftwareIdentity<MySuite>> = Endpoint::builder()
            .identity(responder_identity)
            .wire(responder_socket)
            .config(Config::new())
            .build();

        // Responder task: staged accept ladder from the README sketch.
        let responder_task = tokio::task::spawn_local(async move {
            let intro = responder_endpoint
                .accept()
                .await
                .expect("responder: got an Intro");
            let claimed = intro.read_identity().await.expect("read_identity");
            // Trust anyone for this demo.
            let proven = claimed.authenticate().await.expect("authenticate");
            let connection = proven.accept().await.expect("accept");

            let msg = connection.recv_message().await.expect("recv_message");
            println!(
                "responder received: {:?}",
                String::from_utf8_lossy(&msg)
            );

            connection
                .close(slither::constants::NO_ERROR, b"done")
                .await;
        });

        // Initiator: dial and send.
        let connection = initiator_endpoint
            .connect(responder_addr, responder_static)
            .expect("connect (sync, 0 DH)")
            .await
            .expect("handshake completed");

        connection
            .send_message(b"hello")
            .await
            .expect("send_message");
        connection.acked().await.expect("acked");
        connection
            .close(slither::constants::NO_ERROR, b"done")
            .await;

        responder_task.await.expect("responder task join");

        println!("round trip complete");
    });
}
```

## (ii) Stumble ledger

| # | What I needed | What the docs said | Where the truth actually was | Est. minutes lost |
|---|---|---|---|---|
| 1 | How to declare the `Suite`/`Channel` type every generic (`SoftwareIdentity<S>`, `Endpoint<I>`, `Connection<S>`) requires | Nothing in README's usage sketch or lib.rs's front page. identity.rs's own doctest *uses* a placeholder `MySuite` without defining it. | `src/packet/suite.rs`'s doc comment on the `channel!` macro (found via `grep -rn "channel!" src/`) — a full, correct, compiling example | 10–15 |
| 2 | Which tokio features my own `Cargo.toml` needs | Nothing, anywhere reachable from docs.rs. slither's own internal `Cargo.toml` comment lists exactly `rt, net, time, sync, macros`, but that comment is not part of the published docs a consumer reads. | Guessed `features = ["full"]` (worked, but is over-broad and unverified against docs) | 3–5 |
| 3 | The runtime entry-point shape (plain `fn main` + `slither::block_on`, *not* `#[tokio::main]`) | `compat::rt::block_on`'s module doc states this explicitly and warns of the panic — but it is one hop away from lib.rs (`pub use compat::block_on` links to it) and zero hops from README (README's own sketch shows neither `#[tokio::main]` nor `block_on`, just a bare `.await` inside an implied async context). Verified by experiment: the naive `#[tokio::main]` + `Endpoint::builder()....build()` reflex panics at runtime with a bare tokio-internal message (`` `spawn_local` called from outside of a `task::LocalSet` ``) that never mentions slither or points back to any fix. | `src/compat/rt.rs` module doc, one click from lib.rs but zero from README | 10–20 (if the dev doesn't happen to click through to `compat::block_on` before writing their first `main`) |
| 4 | Own-public-key accessor to hand to the dialling peer | README's sketch never shows this — it only shows the *dialler's* side needing `peer_static` and never demonstrates deriving it | `Identity::public_static()`, found by reading `identity.rs`'s trait definition | 3–5 |
| 5 | A "give me a fresh keypair" constructor (README's sketch uses `from_scalar`, which needs a scalar you don't have yet) | README shows `SoftwareIdentity::from_scalar(my_static_scalar, my_rng)` as *the* example; `generate()` exists but isn't in the README at all | `identity.rs`'s doc comment / struct impl block | 3–5 |
| 6 | Confirmation that a real `tokio::net::UdpSocket` can be used directly as a `Wire` (no adapter to write) | `EndpointBuilder::wire()`'s doc says so explicitly ("A `tokio::net::UdpSocket` is a `Wire` out of the box") | Found directly in docs — **not a stumble**, listed here as a documentation success | 0 |
| 7 | Read several function bodies (`SoftwareIdentity::generate`/`open`, `Endpoint::connect`, `EndpointBuilder::build`, `Wire for UdpSocket`) while verifying doc claims | N/A — this is auditor-style over-verification, not something the working code needed | Doc comments alone were sufficient for every one of these; the body reads were confirmatory, not load-bearing | ~2 (not counted as a genuine blocking stumble) |

Total genuine stumbles: **5** (rows 1–5). Row 6 is a documentation win,
row 7 is a methodology note, not a stumble. Aggregate time a hurried
human would plausibly lose: **roughly 30–50 minutes**, concentrated
almost entirely in row 1 (finding `channel!`) and row 3 (finding the
correct runtime entry point before writing code that panics at runtime).
Neither is a "give up" wall on its own — both are findable by
browsing docs.rs's sidebar (Macros: `channel!`; the `compat::block_on`
re-export at the crate root) if the dev thinks to look there — but a
truly rushed developer skimming only the README, hitting the
`#[tokio::main]` runtime panic, and not immediately recognizing
"LocalSet" as a slither-specific concept (its own error message doesn't
say so) is a plausible **give-up moment**: the error looks like generic
tokio friction, not something slither's docs would fix, so there's no
obvious next doc to go read.

## (iii) The three fixes that would have saved the most time

1. **Put a complete, compile-tested end-to-end example at the top of
   `lib.rs` (or in a real `examples/` directory the README links to),
   covering the full path: `channel!` declaration → `SoftwareIdentity::generate`
   → `public_static()` → `Endpoint::builder()` with a real
   `tokio::net::UdpSocket` → `slither::block_on` (not `#[tokio::main]`) →
   staged accept → `send_message`/`recv_message`.** Every individual piece
   of this already has good documentation *somewhere* in the crate — the
   problem is purely that the README's sketch and lib.rs's front page each
   show a different subset (README shows the staged accept and identity
   *construction from a scalar you don't have*; `compat::rt::block_on`
   shows the runtime shape and a `send`/`finish`/`acked` sequence over a
   raw stream; `packet::suite`'s `channel!` doctest shows the suite
   declaration) and none of the three cross-links to the other two from a
   place a newcomer reads first. A single worked example stitching all
   three together — ideally exactly the "two peers, one message,
   clean close" shape this exercise asked for — would have cut the whole
   Phase 2 exercise from ~40 minutes of guided reading down to
   copy-paste-and-run. `README.md` even says "There is no `examples/`
   directory in the tree today; if one is added, this section should link
   it rather than repeat the sketch inline" — that's the maintainer's own
   acknowledgment of the gap.

2. **State the tokio features a consumer's `Cargo.toml` needs, and state
   `channel!` up front, both in README.md.** Two one-line additions
   with an outsized return: (a) a line like "Your `Cargo.toml` needs
   `tokio` with at least `rt`, `net`, `time`, `sync`, `macros` (or just
   `full` while prototyping)" — this is exactly the comment already
   sitting in slither's own `Cargo.toml` next to its `tokio = {...}` line,
   it just never made it into a rendered doc; and (b) a line in the usage
   sketch itself: "Every consumer must first declare a crypto suite with
   `slither::channel!` — see [`channel!`](crate::channel!)" placed
   *before* the `SoftwareIdentity::from_scalar` line, since that line's
   own generic parameter (`SoftwareIdentity<MySuite>` in the real API,
   elided in the README's untyped sketch) is exactly what `channel!`
   produces.

3. **Make the `#[tokio::main]` mistake fail loudly and specifically, or
   preempt it in the docs a newcomer reads first.** The current failure
   mode — a runtime panic from deep inside tokio's `LocalSet`
   implementation, naming no slither type or slither doc — is the single
   worst moment in this whole walkthrough, because tokio-experienced
   developers will reach for `#[tokio::main]` by reflex and the resulting
   panic looks like their own tokio mistake, not a slither usage question,
   so there is no natural "go read the docs again" trigger. Two ways to
   fix it, either is enough: (a) put the `#[tokio::main]` warning and the
   `slither::block_on` pointer directly in the README's usage sketch
   (right now it's one hop from lib.rs and zero hops from README), or (b)
   have `EndpointBuilder::build()` detect it is not inside a `LocalSet`
   and panic with a slither-authored message pointing at
   `slither::block_on` / `LocalSet` — `build()`'s own doc already says it
   panics "if this is called outside a `tokio::task::LocalSet`", so the
   panic site is already known and owned by slither's code; the message
   text is the only missing piece (today the panic comes from
   `spawn_local` deep in the driver's own `tokio::task::spawn_local` call,
   not from a guard in `build()` itself, which is why the message is
   generic tokio text rather than slither's own).
