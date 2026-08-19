# Round 44 — Agent B: the AI-ranker simulation

**Base commit:** `721167a` ("Round 43: the SECV5 pins land …") — verified as
the first command, matches the brief.
**Date:** 2026-08-19. **Role:** simulate a 2026 AI coding agent that ranks and
picks Rust dependencies for *"add encrypted peer-to-peer UDP transport with
reliable messages to a Rust app"*, plus the variants (WireGuard-like session
layer / Noise protocol transport / QUIC alternative without TLS).

Files read: `README.md` (all 120 lines), `src/lib.rs` `//!` block (lines
1–228), `Cargo.toml` `[package]`/`[features]`/`[dependencies]` (lines 1–140)
and its `[[test]]`/`[[bench]]` stanza headers, plus targeted greps. `SPEC.md`
and `rulings.md` were **not** read, per the brief.

## 0. Method and calibration

### 0.1 What the ranker actually sees

Two pages, roughly 2 000 tokens each:

| Page | Source | Raw size | Est. tokens | Fits budget? |
|---|---|---|---|---|
| crates.io | `[package]` metadata + rendered `README.md` | 5 903 chars README + ~600 chars metadata | **~1 630** | **yes, entirely** |
| docs.rs front page | `src/lib.rs` `//!` block | 12 903 chars raw, ~11 990 rendered | **~3 000** | **no — cut at ~66 %** |

Two calibration facts fall straight out of that table and they set up
everything below.

1. **The README is not truncated. Every word of it is inside the budget.**
   So nothing the README fails to answer is a *budget* problem — it is an
   **absence** problem. There is no "move it up" fix for the README; there is
   only "write it".
2. **The `lib.rs` doc block is truncated, at roughly raw line 158** — inside
   documentation obligation 3 (the persistent-keepalive knob). Everything from
   obligation 4 onward falls off the page for an extracting agent:
   obligation 4 (teardown on handle drop), **obligation 5 (messages xor
   streams)**, obligation 6 (`accept()` is a loop), the whole `# Modules`
   list, and — most expensively — the entire **`# Features` table**, which is
   the only feature documentation that exists anywhere in the ranked corpus.

### 0.2 A note on fidelity

I am simulating, not measuring: I have no instrumented ranker. Where I say
"the AI would", read "a competent extraction agent working to a budget, of
the kind I am, would". I have tried to keep every verdict traceable to a
quoted line or a verified absence rather than to taste. Every "ABSENT" below
was checked with `grep`, and the greps are named.

## 1. Part 1 — token-budget extraction

### 1.1 The corpus as the ranker sees it

**Metadata block (crates.io header, ~150 tokens):**

- `description`: *"A WireGuard-shaped Noise-over-UDP packet layer carrying a
  QUIC-shaped reliable frame layer: streams, messages and datagrams over an
  IK handshake (P-256 / ChaCha20-Poly1305 / BLAKE2b) with a staged accept."*
- `keywords`: `noise`, `udp`, `wireguard`, `quic`, `datagram`
- `categories`: `cryptography`, `network-programming`
- `version` `0.2.0` · `license` `MIT OR Apache-2.0` · `rust-version` `1.96`
- **no `[badges]` section** (verified: `grep -n 'badge' Cargo.toml` → empty)

The keyword set is genuinely good: all five are terms the task's search
queries actually contain. **slither will be *retrieved* for this task.** The
whole of the rest of this report is about what happens after retrieval.

### 1.2 Questions a–h against (metadata + README), first ~2 000 tokens

#### a. What is this, in one sentence a product engineer understands? — **PARTIAL**

> "A WireGuard-shaped Noise-over-UDP packet layer — an authenticated,
> encrypted, **unreliable** datagram session between two peers — with a
> QUIC-shaped **reliable frame layer** (Leg 2, ratified) riding inside the
> sealed packets." (README:3–5)

The raw keyword match is strong — *encrypted*, *UDP*, *between two peers*,
*reliable* are all present in the first sentence. But three things cost it:

1. **It names three other protocols before it names a capability.** The first
   six words are "WireGuard-shaped Noise-over-UDP packet layer". A ranker
   building a one-line summary has to carry all three proper nouns or drop
   information. The `description` field has the same shape, and it is the
   field that appears in *search results*, before any click.
2. **The first bolded word is `unreliable`.** For a task whose literal
   phrasing is "with reliable messages", the first emphasis on the page is the
   negation of the requirement. The reliable half arrives as a subordinate
   clause after an em-dash. Emphasis is a ranking signal, and this one points
   the wrong way.
3. **"(Leg 2, ratified)" is unglossed project vocabulary in sentence one.**
   Nothing on the page defines Leg 1 or Leg 2 before this use — the definition
   comes 20 lines later under `## Lineage`. An extractor either carries a token
   it cannot resolve or drops the parenthetical, and the parenthetical is
   carrying the word "ratified", which is the maturity claim.

A product engineer reading the sentence learns *what it resembles* before
*what it does for them*. The capability sentence — "two peers exchange
encrypted, reliable, unordered messages over UDP, authenticated by raw public
keys, with no certificates and no TLS" — is never written anywhere in the
corpus.

#### b. When to choose it over quinn / snow / libp2p / WireGuard? When NOT? — **ABSENT**

Verified: `grep -ic` over `README.md` for `quinn`, `snow`, `libp2p`,
`rustls`, `certificate` → **0 each**. `TLS` → 1 hit, and it is lineage prose
("the QUIC-style 'framing over a Noise channel instead of TLS' direction",
README:13), not a comparison. Same grep over the `lib.rs` doc block → **0 for
all six**, `TLS` included.

There is no "when to use", no "when not to use", no "alternatives", no
"non-goals" section in either document. This is the **single most expensive
absence in the corpus**, and it is worse than merely missing, because the
section that occupies its structural slot works *against* the crate:

> `## Lineage` — "**WireGuard-lite** — the handshake shape … are WireGuard's,
> adapted. … **QUIC-style frames** — … is **Leg 2**, riding ON TOP of this
> unreliable, authenticated packet layer." (README:16–27)

A ranking agent reads `## Lineage` as a **provenance disclosure**, and
provenance disclosures answer the question *"is this a reimplementation of
things I can already get from a maintained crate?"* with **yes, twice**. The
section is 170 tokens spent telling the ranker that quinn and boringtun exist
and that this crate re-did their work. It is honest, it is interesting to a
protocol reviewer, and in an extraction pass it is a net-negative block.

The differentiators that would answer (b) are all real and all unstated:
raw-key identity with **no PKI and no certificate plumbing**; a **staged
accept ladder** that lets the app authorise a peer *before* spending the
second DH (this is genuinely not available in quinn, snow, or libp2p, and it
is slither's best single argument); **endpoint roaming** built in; the
**sans-io core** being drivable with no kernel; and `forbid(unsafe_code)`.

#### c. How do I install it? — **ABSENT**

Verified: `grep -ic "cargo add"` → 0 in README and in the `lib.rs` doc block.
`grep -ic "\[dependencies\]"` → 0 in README. `grep -ic "feature"` → **0 in
README** — the word does not occur in the file.

An agent asked "add this to my project" cannot produce a working `Cargo.toml`
from the ranked corpus. Concretely, it cannot learn:

- that `default = []` and **nothing is on by default** (stated only in the
  `lib.rs` `# Features` section, ~3 000 tokens deep, **outside the budget**);
- that `tokio` is a *required companion* in the consumer's own manifest, with
  which features — the sketch calls `tokio::net::UdpSocket::bind` at
  README:62 and `Endpoint::builder().wire(socket)`, so the consumer needs
  `tokio` with at least `net`, `rt`, `macros`, and a `LocalSet`;
- that `SoftwareIdentity::from_scalar(my_static_scalar, my_rng)` (README:61)
  needs a scalar and an RNG **whose types come from `hiss`**, so `hiss` is a
  direct dependency of the *consumer*, not just of slither;
- **the `rand_core` hazard.** `grep -ic rand_core` over the `lib.rs` doc block
  → **0**; over README → 0. `CLAUDE.md` records that "two `rand_core` majors
  in one graph produce an unsatisfiable `CryptoRng` bound, not a version
  error." That is a real, confusing, first-five-minutes failure for anyone
  whose app already pulls a different `rand` line, and **the entire consumer
  corpus is silent on it**. An AI that writes the integration will hit it and
  will not know why.

This is the absence with the worst *consequence-per-token*: three lines of
copy-pasteable TOML would remove a whole class of failed integration.

#### d. Minimal working example — visible, and would the AI trust it? — **PARTIAL, and distrusted**

There is a sketch (README:56–85), and it is a *good* sketch — it shows the
builder, the sync-`connect()`/async-`Connecting` split, `send_message`, and
the full staged accept ladder with the drop-is-reject idiom. As
*explanation* it is above average.

As something an agent will lift into a file, it fails on four counts:

1. **It is fenced ` ```rust,ignore `** (verified: `grep -n '^```' README.md`
   → `56:```rust,ignore`). `ignore` is the fence that means *not compiled, not
   tested*. Rustdoc renders it with the "this example is not tested" marker.
   A ranker that parses info-strings — and the fence is four characters into
   the block, so a cheap one does — reads `ignore` as *the maintainer could
   not make this compile*. The trust signal available here, `no_run`, is
   already used correctly four times inside `lib.rs` (lines 61, 80). The
   README uses the weaker one.
2. **Four identifiers are undefined**: `my_static_scalar`, `my_rng`,
   `peer_static`, `my_allow_list`. Their *types* are the exact thing (c) also
   fails to answer, so the two absences compound — the reader cannot resolve
   them from anywhere on the page.
3. **The runtime setup the code requires is a comment, not code.** Line 60
   says "// Inside a tokio current-thread runtime + LocalSet (the actor is
   `!Send`):" — and then no `LocalSet` appears. The one structural constraint
   that will break the integration is the one line the example elides.
4. **README:87–88 is stale and says the wrong thing:**

   > "There is no `examples/` directory in the tree today; if one is added,
   > this section should link it rather than repeat the sketch inline."

   `examples/` **exists at `721167a`** and contains `audit_udp.rs` (20 KB) and
   `bench_vs_tcp.rs` (67 KB). There is no `autoexamples = false` in
   `Cargo.toml` (verified: `grep -n 'auto' Cargo.toml` finds only `autotests`
   discussion), and `/examples` is **not** in `exclude`, so both ship to
   crates.io and both are auto-discovered as `cargo run --example` targets.
   So the README tells the ranker "no examples" while the published package
   contains two — and the two it contains are a **latency-audit probe** and a
   **TCP benchmark harness** (both open with `//!` blocks about round-41 audit
   item 3.1 and measurement), not onboarding material. Both halves are worth
   fixing and they are different fixes: delete the stale sentence, *and*
   accept that `examples/` currently offers a new consumer nothing.

#### e. Async runtime constraints (`!Send` / current-thread / `LocalSet`) — **PARTIAL, and dangerously placed**

In the README this appears exactly once, at README:60, **as a comment inside
the fenced code block**. It is ~1 100 tokens into the page, it has no heading,
it appears in no prose sentence, and any chunker that strips or down-weights
code blocks (a common budget tactic) **loses it entirely**.

This is the constraint most likely to kill the integration. An app with
`#[tokio::main]` — i.e. the multi-threaded runtime, i.e. the default, i.e.
what every axum/tonic/tower app in the world is — **cannot use slither
without restructuring**, and no handle can cross a thread boundary. That is a
legitimate design choice with a stated rationale (Secure Enclave keys), and
it is a *first-paragraph* fact for a consumer, delivered here as a code
comment.

#### f. MSRV, license, maturity, maintenance — **MIXED: license yes, MSRV via metadata only, maturity actively misleading**

- **License — ANSWERABLE.** `MIT OR Apache-2.0` in metadata, `## License` in
  README:107–116. Clean.
- **MSRV — ANSWERABLE from metadata, ABSENT from README.**
  `rust-version = "1.96"` renders in the crates.io sidebar. `grep -ic MSRV`
  and `grep -c 1.96` over README → **0 each**. A ranker reading the sidebar
  gets it; one summarising the README text does not.
- **Maturity — PARTIAL, and the parts disagree.** `version = "0.2.0"` says
  *pre-release personal project*. `## Status` (README:98–105) says
  "every wire constant is **ratified** (2026/07/16) and frozen", "**Leg 2** …
  built and **ratified** (2026/07/17)". Those two signals point opposite ways
  and the ranker resolves the conflict the conservative way: **0.2.0 wins**,
  because a version number is a machine fact and "ratified" is a claim in
  project vocabulary whose authority ("`SPEC.md` §§1–8") the ranker cannot
  evaluate and will not fetch (`SPEC.md` is 502 KB).
- **Maintenance — ABSENT.** No badges of any kind (no CI, no crates.io, no
  docs.rs). No `CHANGELOG.md` link (`grep -ic CHANGELOG README.md` → 0,
  though the file exists). No release cadence, no maintainer statement, no
  "used in" line, no platform/OS support statement, **no test count**.

The gap between the artefact and its presentation is at its widest here. I
counted **887** `#[test]`/`#[tokio::test]` attributes in the tree (746 under
`src/`, 141 under `tests/`; the project's own record puts the executed total
near 1 130 with doctests), across **25 `[[test]]` targets**, against a
ratified 5 600-line specification, with a nine-gate release table. **None of
that is visible in the ranked corpus.** A ranking agent sees `0.2.0`, no
badges, no test count, no downloads history, and files it under *pre-alpha
hobby crate*. That single misclassification costs more positions than any
other finding in this report, and it is the cheapest one to fix.

#### g. Security posture — **ABSENT from the ranked corpus, despite existing on disk**

- `grep -ic audit README.md` → **0**. The word does not appear. Neither
  "audited" nor "unaudited" is stated. **Not saying "unaudited" is worse than
  saying it**: a careful ranker assumes unaudited *and* records that the crate
  declined to disclose. snow says so plainly; rustls/quinn have explicit
  posture statements. slither is silent, and silence scores below candour.
- **`SECURITY.md` is invisible.** It exists, it is 4.8 KB, and it is good —
  private reporting via GitHub advisories, an explicit threat model, and the
  correct delegation ("for the cryptographic primitives … see **hiss's
  `SECURITY.md`**"). But **crates.io does not render or link `SECURITY.md`**,
  and `grep -ic SECURITY README.md` → **0**. It is reachable only by someone
  who navigates to the GitHub repo, which a budgeted ranker does not do.
- **The trust story reads as risk, not assurance.** README:9 says "the
  cryptography is Bubble's". *Bubble* is an unexplained proper noun. To a
  ranker, an unknown organisation name attached to the phrase "the
  cryptography is X's" is a **negative** signal — it means the crypto comes
  from somewhere the ranker cannot evaluate. The intended reading (a
  deliberate, principled, RustCrypto-free lineage with `hiss` as the single
  audited-by-reference seam) requires context the page never supplies. "No
  RustCrypto" is a real, defensible policy and it is nowhere in the corpus.
- **`#![forbid(unsafe_code)]` gets zero credit.** It is at `src/lib.rs:230` —
  *after* the `//!` block, so it does **not** render on the docs.rs front
  page, and the only occurrence of "unsafe" inside the doc block is
  obligation 5's "the safe and unsafe shapes look alike" (line 146), which is
  about API misuse and will be *mis-extracted* as an unsafety admission. For
  a crypto transport this is the highest-value-per-word security signal
  available and slither currently scores worse than zero on it.

#### h. Limits — **PARTIAL; the honest ones are stated, the integration-blocking ones are not**

Present and in budget, and genuinely creditable:

> "Pacing, ECN and alternate controllers (CUBIC/BBR) stay reserved (§14.7)."
> (README:26–27)

> "Reliability lives within the connection — what a dead connection had not
> delivered is lost." (README:51–52)

> "a session that sends into 25 s of silence is declared dead" (README:44)

Absent from the README, and each one is a question the stated task forces:

| Limit | In README? | In `lib.rs` budget? |
|---|---|---|
| One session per peer static; reconnect is `close()`-then-dial | no | yes (obligation 1) |
| A connection with no traffic dies in 25 s, silently | no (the 25 s is stated as protocol, not as a *hazard*) | yes (obligation 3) |
| **Messages xor streams on one connection** | no | **no — obligation 5 is past the cut** |
| Teardown on last *handle* drop, not endpoint drop | no | **no — obligation 4 is past the cut** |
| **MTU / PMTUD / max datagram size** | **no** (`grep -ic MTU\|PMTU` → 0) | **no** (0 hits) |
| **NAT traversal: does slither hole-punch?** | **no** (`grep -n '\bNAT\b'` → 0 hits in README) | one mention, "through a NAT binding", in obligation 3's keepalive knob, **at/past the cut** |
| Certificates / PKI: what identity model? | implied by "peer static", never stated as *no PKI* | implied, never stated |

Two of those matter enough to call out individually.

**NAT traversal is *the* question for the word "peer-to-peer", and neither
document answers it.** `\bNAT\b` does not appear in the README at all. A
ranker scoring a P2P task assumes libp2p does traversal, assumes a
WireGuard-shaped thing does not, and marks slither **unknown** — which ranks
below a clear *no*, because unknown means "I would have to read the source to
find out". One sentence ("slither does not do NAT traversal or hole punching;
supply reachable addresses, and use `set_persistent_keepalive` to hold a NAT
binding open") converts a downgrade into a scoped, honest boundary — and
slither *does* have the keepalive knob that makes the answer a good one.

**Max message size is a top-three integration question and is unanswerable.**
"Can I send a 1 MB message" has no answer anywhere in the ranked corpus.

### 1.3 Questions e–h against the docs.rs front page (`lib.rs`), ~2 000-token budget

The `lib.rs` block is much better written for a *reader* and structurally
worse for an *extractor*, because its best material is stacked behind the cut.

#### e. Runtime constraints — **ANSWERABLE, in budget, well argued**

> "the shell is a **single `!Send` actor** run with `tokio::task::spawn_local`
> on a current-thread runtime inside a `LocalSet`. Nothing on that path
> requires `Send`, deliberately: a hardware-backed static key (an iOS Secure
> Enclave `SecKey`) is not `Send`…" (`lib.rs`:33–39)

This is the corpus's best paragraph: constraint, mechanism, and *rationale*
in three sentences, roughly 350 tokens in. A ranker extracts it cleanly.

The residual gap is that it states the constraint and never states **the
cost to the consumer**: no sentence says "if your application uses
`#[tokio::main]`, you must run slither on a separate current-thread runtime /
inside a `LocalSet`", and no sentence says "connection handles cannot be sent
between threads". The ranker has to *infer* the integration cost from the
word `!Send`, and inference under budget is exactly what does not happen. A
single "what this means for your app" clause here is worth more than the
whole rationale it would sit beside.

#### f. MSRV / license / maturity — **ABSENT from `lib.rs` entirely**

`grep -ic MSRV` → 0, `grep -c 1.96` → 0 over the doc block. No license
statement, no version, no status, no test count, no stability policy. docs.rs
renders the version and license in its own chrome, so the ranker recovers
those two; everything else is unavailable on this page.

`## Status`'s ratification claim, the only maturity content that exists,
lives **only in the README** — so a ranker that fetches docs.rs and skips
crates.io (a real pattern, since docs.rs is the richer page) sees **no
maturity signal at all**.

#### g. Security posture — **ABSENT**

`grep -ic audit` → 0; `SECURITY` → 0; `cryptoxide` → 0; `rand_core` → 0. The
opening does say the sealing is "driven entirely through
[`hiss`](https://docs.rs/hiss)" (`lib.rs`:6), which is the right pointer, but
nothing states the trust boundary, the no-RustCrypto policy, the audit status,
or `forbid(unsafe_code)`. As noted in (g) above, the only in-budget
"unsafe" token is misleading.

#### h. Limits — **the strongest block in the corpus, and half of it is past the cut**

`# The six documentation obligations` is, in substance, the best "gotchas"
section I have read in a Rust crate. It is specific, it is honest, it names
the wrong guess before the right one ("Reconnecting is `close()` then dial,
not `connect()` again … 'Call connect again' is the natural guess and it is
wrong"), and obligation 3 opens with the sentence a consumer most needs:

> "**A connection with nothing to say dies — in 25 s, in silence.** …
> **Connecting ahead of need does not keep a path warm**, and this is the
> single most surprising behaviour for a new consumer." (`lib.rs`:110–116)

Three defects, in ascending cost:

1. **The heading is unretrievable.** "The six documentation obligations" is
   project vocabulary. An agent searching the page for *limitations*,
   *caveats*, *gotchas*, *pitfalls*, *before you integrate* matches none of
   those words. A heading is a retrieval key and this one indexes on a phrase
   only the maintainer uses.
2. **Obligations 1–3 consume ~1 400 tokens of the ~1 650 available after the
   preamble** — obligation 1 alone carries a 20-line `no_run` example.
   Obligations 4, 5 and 6 are shorter and at least as consequential
   (messages-xor-streams is a *panic-class* API constraint; `accept()`-in-a-
   loop is a correctness requirement with a two-case proof) and all three are
   **past the cut**.
3. **The `# Features` table is the last block on the page.** It is the *only*
   feature documentation in the whole consumer corpus — the README never says
   the word "feature" — and it sits at roughly token 2 900 of a 2 000-token
   budget. For question (c), the answer exists and is unreachable.

### 1.4 Part 1 scorecard

| # | Question | crates.io (metadata + README) | docs.rs (`lib.rs`) |
|---|---|---|---|
| a | What is it, in one sentence | **PARTIAL** — lineage-first, `unreliable` bolded first, unglossed "Leg 2" | PARTIAL (same opening, no "Leg" jargon — slightly better) |
| b | When to choose / not choose | **ABSENT** — and `## Lineage` occupies the slot with a net-negative message | **ABSENT** |
| c | Install, features, companion deps | **ABSENT** — "feature" is not in the file | ABSENT (feature table exists, past the cut) |
| d | Trusted minimal example | **PARTIAL** — `rust,ignore`, 4 undefined idents, no `LocalSet`, stale "no examples" line | PARTIAL — obligation 1's `no_run` is the only runnable-looking code, and it demonstrates *reconnection*, not first use |
| e | `!Send` / current-thread / `LocalSet` | **PARTIAL** — one code comment at README:60 | **ANSWERABLE** — in budget, with rationale; missing the "what it costs you" clause |
| f | MSRV / license / maturity | MIXED — license yes, MSRV metadata-only, maturity contradicted by `0.2.0`, **no badges, no test count** | **ABSENT** |
| g | Security posture / audit / `SECURITY.md` | **ABSENT** — and "the cryptography is Bubble's" reads as risk | **ABSENT** — plus `forbid(unsafe_code)` sits below the doc block |
| h | Limits | **PARTIAL** — pacing/ECN and in-connection reliability stated; **no MTU, no NAT, no xor-rule** | **PARTIAL→good** — the best block in the corpus, half of it past the cut, under an unsearchable heading |

## 2. Part 2 — ranking simulation

### 2.1 Sourcing

Competitor positioning was fetched live on 2026-08-19 (crates.io itself is a
JS app and returns an empty shell to a fetcher — a small finding in its own
right, since it means *README-on-GitHub* is what many extractors actually
read). Sources used:

- `raw.githubusercontent.com/quinn-rs/quinn/main/README.md`
- `raw.githubusercontent.com/mcginty/snow/main/README.md`
- `raw.githubusercontent.com/cloudflare/boringtun/master/README.md`
- `raw.githubusercontent.com/n0-computer/iroh/main/README.md`

`libp2p-noise` is characterised from knowledge (it is a transport *upgrader*
inside the `libp2p` ecosystem, not a standalone transport), and flagged as
such.

**I added `iroh` to the comparison set, which the brief did not name.** I
think leaving it out would have made the simulation dishonest: for the exact
query *"encrypted peer-to-peer UDP transport with reliable messages in Rust"*,
iroh is the crate a 2026 search surfaces first, and it is slither's nearest
positional neighbour — an `Endpoint`, dialled **by public key**, no
certificates, QUIC streams and datagrams. Ranking slither without it would
have flattered the result.

### 2.2 The comparison table the AI builds

| | **slither 0.2.0** | **iroh** | **quinn** | **snow** | **boringtun** | *libp2p (+ -noise)* |
|---|---|---|---|---|---|---|
| **Out of the box** | Sealed UDP session + reliable unordered messages, streams, datagrams, roaming, staged accept | Endpoint, **NAT hole-punching**, **relay fallback**, QUIC streams + datagrams, pre-built protocol layers (blobs, gossip, docs) | Full IETF QUIC: streams, datagrams, congestion control, client+server | **Noise handshake state machine only** — no I/O, no transport, no framing | WireGuard protocol logic for building a tunnel; **no network/tunnel stack** | Whole p2p stack: transports, muxing, discovery, DHT, NAT |
| **Reliability story** | Reliable, **unordered**, exactly-once messages; RFC 9002 recovery + NewReno; *"what a dead connection had not delivered is lost"* | QUIC — ordered streams, unordered datagrams | QUIC — ordered *and* unordered stream reads, plus datagrams | **none** — caller frames and retransmits | **none** — WireGuard is unreliable by design | QUIC/TCP underneath |
| **Identity / PKI** | **Raw static public keys, no certificates** — but the README never says the words "no certificates" | **Public-key addressing, explicitly stated as the headline** | **TLS certificates via rustls** (self-signed for local dev) | Raw keys, caller-supplied | Raw WireGuard keys | Raw peer IDs |
| **Runtime constraints** | **`!Send`, current-thread runtime, inside a `LocalSet`** — stated once, as a code comment | ordinary `tokio`, `Send` | ordinary async, `Send`, cross-platform on stable | sans-io, runtime-agnostic | sans-io, runtime-agnostic | ordinary `tokio` |
| **Maturity signal** | `0.2.0`; **no badges**; no test count; no downloads history; "ratified" in project vocabulary the ranker can't evaluate | Funded org (n0), relay infrastructure operated in production, monitored | **Since 2018, 30+ releases, CI + coverage + Matrix/Discord badges, commercial sponsors, MSRV 1.80** | Long-lived, widely embedded; **candid: "has not received any formal audit"** | **"powers millions of mobile devices … and Cloudflare Linux servers"** | Foundation-backed, enormous |
| **Onboarding as presented** | **Unknown.** No `cargo add`, no `[dependencies]`, no companion deps, no feature list in the README, example fenced `ignore` with 4 undefined idents; **MSRV 1.96 + edition 2024** (the most aggressive floor in the set) | **`cargo add iroh`** + a 3-step pattern | `cargo run --example server` / `--example client`, working out of the box | 12-line copy-pasteable snippet in the README | `cargo install boringtun-cli` | Heavy, but exhaustively documented |
| **Task-fit headline** | Very close on substance | **Near-exact on substance *and* on wording** | Close, but drags TLS/certs in | A component, not a solution | A component, not a solution | Solves it plus 20 things you didn't ask for |

### 2.3 Where slither ranks today, and why

**On the stated primary task — "add encrypted peer-to-peer UDP transport with
reliable messages to a Rust app" — slither ranks 5th of 6.** It is retrieved,
shortlisted, and then eliminated in the second pass. The order I would produce
today:

1. **iroh** — matches the request almost word-for-word, and every column is
   answered on its front page.
2. **quinn** — unbeatable maturity signal; loses only on "certificates" if the
   requester cares.
3. **libp2p** — over-delivers, but nothing is unknown about it.
4. **snow** — obviously only a component, but it says so, and its candour
   about the audit *raises* its trust score.
5. **slither** — substantively a strong fit, eliminated on unknowns.
6. **boringtun** — a tunnel-building component with no reliability story.

That is the honest number, and the reason it stings is that **slither is
probably the second-best technical fit in the table.** It is the only entry
besides iroh that gives you reliable messages over raw-key-authenticated UDP
with no PKI, and it has one capability *nobody else in the table has* — the
staged accept ladder, where the application authorises a claimed identity
before the second DH is spent. That is a real, defensible, differentiating
feature and **it is not mentioned in the `description`, not named as a
benefit anywhere, and appears only as unexplained mechanics inside a
`rust,ignore` code block.**

Three mechanisms do the eliminating, in the order a ranker applies them:

**(i) The trust filter, applied first and hardest.** For anything in the
`cryptography` category, a ranking agent applies a near-binary gate: *is there
evidence this is maintained and safe to depend on?* slither presents `0.2.0`,
zero badges, no test count, no audit statement, no CI signal, and a crypto
lineage attributed to an unexplained proper noun ("the cryptography is
Bubble's"). Every competitor clears this gate with a single visible artefact —
quinn with badges and a seven-year history, boringtun with "millions of mobile
devices", snow by *admitting* it is unaudited. **Candour clears the gate;
silence does not.** slither's actual evidence — 887 test attributes across 25
test targets, a ratified 5 600-line spec, a nine-gate release table,
`forbid(unsafe_code)` — is entirely absent from both ranked pages. This is
where most of the lost positions are, and it is a *presentation* loss, not a
substance loss.

**(ii) The onboarding-cost column, scored as "unknown" = "high".** A ranker
cannot write the `Cargo.toml`. It cannot list the features. It cannot resolve
the four undefined identifiers in the example. It sees `ignore` on the fence.
And it sees `rust-version = 1.96` with `edition 2024` — the most aggressive
MSRV in the comparison set, against quinn's 1.80 — which is a genuine,
correctly-scored negative that no amount of writing removes, but which is
currently *unaccompanied* by any of the compensating detail that would make
the cost look bounded rather than unbounded.

**(iii) The head-to-head against iroh, which slither has no argument to
win.** Both are "dial a peer by its public key". iroh additionally answers the
question the word *peer-to-peer* puts on the table — **NAT traversal** — and
slither's README does not contain the string `NAT` at all. A ranker resolves
that silence as *unknown*, and unknown loses to a clear yes and also to a
clear no. slither's honest answer ("we don't traverse; supply reachable
addresses; here is the keepalive knob that holds a NAT binding open") is a
perfectly respectable scoping statement that would move it above libp2p for
requesters who already have addressability — and it is unwritten.

**The variants rank differently, and better:**

- **"WireGuard-like session layer"** → slither ranks **2nd**, behind
  boringtun. `keywords = [... "wireguard" ...]` and the README's first line
  both hit, and boringtun's "you supply the network and tunnel stack" makes
  slither's batteries-included session look attractive. This is slither's
  strongest query today.
- **"Noise protocol transport"** → **2nd or 3rd**, behind snow (and level with
  the `noise-protocol` family). The word "transport" is where slither beats
  snow, and the README does convey it.
- **"QUIC alternative without TLS/certificates"** → **3rd**, behind iroh and
  ahead of quinn (which is disqualified by the query). This *should* be
  slither's best query and it is held back by the same thing: the README never
  writes the phrase "no certificates" or "no TLS/PKI" as a *property*. `TLS`
  occurs once, in a sentence about lineage.

### 2.4 The absences that cost the most positions

Ranked by positions lost:

| Rank | Absent block | Positions | Note |
|---|---|---|---|
| 1 | **Trust / maturity block** (badges, test count, audit status, `forbid(unsafe_code)`, platforms, spec-ratified stated in ranker-legible terms) | **~2** | This is the *eliminating* filter, applied before any technical comparison. Also the cheapest to fix. |
| 2 | **Install + quickstart** (`cargo add`, `[dependencies]` with tokio features, hiss, the `rand_core` caveat, the feature list) | **~1–2** | Turns "unknown onboarding cost" into "bounded". |
| 3 | **When-to-choose / when-not-to, incl. a comparison table** | **~1** — and *all* of the upside | Absence (1) and (2) are why slither loses; absence (3) is why it can never win. Without it there is no argument against iroh. |
| 4 | **The NAT-traversal and scope statement** | ~1 on the p2p query specifically | A stated *no* beats an unstated anything. |
| 5 | **The staged accept ladder as a named benefit** | 0 lost, ~1 unearned | The only capability in the table nobody else has, currently invisible. |

**One absence that does not cost positions and is worse for it.** Burying the
`!Send` / current-thread / `LocalSet` requirement in a code comment does not
lower the rank — it *inflates* it, and then the integration fails after the
choice is made. That is a strictly worse outcome than being ranked below
iroh: the agent commits, writes code against a multi-threaded runtime,
discovers handles won't cross threads, and abandons the crate with a bad
impression. A constraint that changes the answer belongs where the answer is
formed.

### 2.5 A refinement to the brief's model

The brief models the ranker as fetching two pages per candidate and reading
~2 000 tokens of each. That is right for the *shortlist* pass, but it
under-models the pass before it: **crates.io search results show only name,
`description`, version, and downloads.** A large share of elimination happens
there, on ~40 tokens, before any page is fetched.

Two consequences the brief's ordering would miss:

1. The **`description` field is the single highest-leverage string in the
   crate** — higher than any README block — because it is the only prose that
   participates in the pre-fetch cull. Fixing it costs zero added tokens.
2. **Downloads history is a first-order ranking input that no amount of
   writing can change.** It is worth naming so the round doesn't over-attribute
   the ranking to prose. Everything in Part 3 is about the share that *is*
   addressable.

## 3. Part 3 — the machine-extraction fixes

Ranked by **extraction value per added token**. "Cost" is added tokens on the
ranked page; several of the best are *negative or zero* cost because they
replace or relocate text that is already there.

### 3.1 Ranked proposals

---

#### P1 — `Cargo.toml` `description`: lead with capability, not lineage · cost **0 tokens** · value **highest**

The only string in the crate that participates in the pre-fetch cull (§2.5).
Today it names three other protocols before saying what the crate does:

> "A WireGuard-shaped Noise-over-UDP packet layer carrying a QUIC-shaped
> reliable frame layer: streams, messages and datagrams over an IK handshake
> (P-256 / ChaCha20-Poly1305 / BLAKE2b) with a staged accept."

Proposed shape — capability, then differentiator, then lineage, within the
same length:

> "Encrypted peer-to-peer UDP transport: reliable messages, streams and
> datagrams between two peers, authenticated by raw public keys — no
> certificates, no TLS. WireGuard-shaped handshake, QUIC-shaped frames."

This keeps every fact and reorders them so the first clause answers the query.
It also gets the phrase "no certificates, no TLS" — the thing that wins the
"QUIC alternative without TLS" variant — into the pre-fetch pass.

I would **not** touch `keywords`; all five already earn their place.

---

#### P2 — `README.md`, immediately under the `# slither` heading: a status/facts block · cost **~80 tokens** · value **very high**

This is the eliminating filter from §2.3(i), and it is the cheapest fix in the
report. Badges first (they are machine-read *and* human-read), then a
four-line fact list. Everything in it is already true:

```
[crates.io badge] [docs.rs badge] [CI badge] [license badge]

**Status:** v0.2.0 · MSRV 1.96 (edition 2024) · `MIT OR Apache-2.0` ·
`#![forbid(unsafe_code)]` · wire protocol **ratified and frozen**
([`SPEC.md`](SPEC.md)) · 800+ tests across 25 test targets, all timers
driven in virtual time · Linux / macOS / Windows · **not independently
audited** — see [`SECURITY.md`](SECURITY.md).
```

Four things this single block fixes at once: the `0.2.0` misclassification
(f), the invisible `forbid(unsafe_code)` (g), the undiscoverable `SECURITY.md`
(g), and the missing MSRV-in-prose (f). The **"not independently audited"**
clause is the highest-value five words on the page — snow's candour is
precisely why snow clears the trust gate, and slither currently scores below
snow by declining to answer.

*(Only publish the CI badge if the workflow is public; a broken badge is worse
than none. And state the test count as a bound you will not have to chase —
"800+", not a number that drifts every slice.)*

---

#### P3 — `README.md`, replacing lines 1–27: a front-loaded what / why / when-not triplet · cost **~0 net** (it replaces `## Lineage`, which is 170 tokens of net-negative) · value **very high**

Today's opening is lineage-first and its second section tells the ranker that
quinn and boringtun already did this work (§1.2b). Proposed structure:

- **What** (one sentence, no protocol names): two peers exchange encrypted,
  reliable, unordered messages — plus streams and unreliable datagrams — over
  UDP, authenticated by raw public keys.
- **Why slither** (three bullets, the actual differentiators): no
  certificates and no PKI; a **staged accept ladder** that lets the
  application authorise a claimed peer *before* spending the second DH;
  connections that **roam** across address changes; a sans-io core drivable
  with no kernel.
- **When not to** (three bullets, blunt): if you want NAT traversal or relay
  fallback, use **iroh**; if you have certificates and want mainstream QUIC,
  use **quinn**; if you want the Noise handshake alone with no transport, use
  **snow**.

Naming the competitors is not a weakness — it is what makes the block
extractable, and a ranker that sees a crate correctly scope itself against
three named alternatives upgrades the crate's credibility, not just its
clarity. `## Lineage` can survive as two sentences much lower on the page,
after the reader has been given a reason to care.

---

#### P4 — `README.md`, a `## Install` block right after the pitch · cost **~140 tokens** · value **very high**

Fixes (c) entirely, which is currently a total absence — the word "feature"
does not occur in `README.md`. It must be copy-pasteable and must name the
companion deps:

```toml
[dependencies]
slither = "0.2"
# slither's actor runs on YOUR runtime: current-thread, inside a LocalSet.
tokio = { version = "1", features = ["rt", "net", "time", "sync", "macros"] }
# You construct the identity, so hiss's key types appear in your own code.
hiss = { version = "0.3", default-features = false }
```

Plus, in prose, the three things that cannot be inferred:

- **Nothing is on by default.** `test-util`, `sink`, `codec` (implies `sink`),
  `tower` — one line each, or one link to the docs.rs feature table (see P8:
  do **not** duplicate the table).
- **`rand_core` must be the line `hiss` names (0.10);** `hiss::rand_core`
  re-exports it. Two `rand_core` majors in one graph produce an unsatisfiable
  `CryptoRng` bound, **not** a version error. This is currently documented
  *nowhere* a consumer can see (`grep -ic rand_core` → 0 in both README and
  the `lib.rs` doc block) and it is a genuine first-five-minutes failure.
- MSRV 1.96, edition 2024.

---

#### P5 — `README.md`, a `## Requirements` line *above* the example · cost **~40 tokens** · value **high**

Lift README:60's code comment into prose with a heading, and add the sentence
that is missing everywhere:

> **slither's driver is `!Send`.** It runs with `tokio::task::spawn_local` on
> a **current-thread** runtime inside a **`LocalSet`**, and connection handles
> do not cross threads. If your application uses `#[tokio::main]` (the
> multi-threaded runtime), run slither on its own current-thread runtime and
> bridge with channels. This is deliberate: it is what lets a hardware-backed
> static key — an iOS Secure Enclave `SecKey`, which is not `Send` — drive the
> handshake.

This is the fix that prevents the §2.4 *failed-integration* mode rather than a
ranking loss, which makes it structurally different from P1–P4: it does not
raise the rank, it makes the rank honest.

---

#### P6 — `src/lib.rs`: reorder the doc block, and rename one heading · cost **~30 tokens net** (mostly relocation) · value **high**

Three edits, all cheap:

1. **Move `# Features` and a two-line install pointer above `# The six
   documentation obligations`.** The feature table is the only feature
   documentation in the corpus and it currently sits ~900 tokens past the
   extraction cut. Moving it up is free and it is the single largest
   in-budget/out-of-budget swing available.
2. **Rename the heading.** "The six documentation obligations" indexes on
   vocabulary only the maintainer uses. `# Limits, hazards and gotchas` (or
   `# Before you integrate`) matches what an extractor greps for. Keep the
   count and the internal framing; change the retrieval key. *The content
   under it is the best writing in the crate — it deserves a heading that gets
   it found.*
3. **State `forbid(unsafe_code)` inside the `//!` block.** It is at
   `src/lib.rs:230`, *below* the doc block, so it does not render on the
   docs.rs front page; meanwhile the only in-budget "unsafe" token is
   obligation 5's "the safe and unsafe shapes look alike", which extracts as
   the opposite of the truth. Six words in the preamble fix this.

Optionally: trim obligation 1's 20-line `no_run` block, which alone consumes
roughly a quarter of the in-budget space, to a 6-line core plus a link. That
buys obligations 4–6 their way back inside the cut.

---

#### P7 — `README.md`: a `## Limits` block · cost **~120 tokens** · value **high**

Five bullets, each a question the stated task forces and none currently
answerable (§1.2h):

- **No NAT traversal and no relays.** You supply reachable addresses.
  `set_persistent_keepalive` holds a NAT binding open; it does not punch one.
- **One session per peer static.** Reconnecting is `close()` then dial, not
  `connect()` again.
- **A connection that carries no traffic dies in 25 s, silently.** Connecting
  ahead of need does not keep a path warm.
- **Messages and streams do not mix on one connection.**
- **No pacing, no ECN, no PMTUD; NewReno only** (CUBIC/BBR reserved). Max
  datagram/message size: *state the number.*
- Reliability is **within a connection** — what a dead connection had not
  delivered is lost.

Two notes. First, the MTU/max-message figure is the one item on this list I
could not source from the ranked corpus at all and could not verify without
reading `SPEC.md`, which the brief forbids; **somebody has to look it up.** It
is a top-three consumer question. Second, this block must **summarise and
link** the `lib.rs` obligations, never restate them — see §3.2.

---

#### P8 — free hygiene · cost **~15 tokens** · value **moderate, but zero risk**

- **Delete README:87–88.** "There is no `examples/` directory in the tree
  today" is **false at `721167a`** — `examples/` holds `audit_udp.rs` and
  `bench_vs_tcp.rs`, neither excluded from the package, both auto-discovered
  by cargo. This is a factual defect, not a wording preference.
- **Link `SECURITY.md` and `CHANGELOG.md`** from the README. Both exist,
  neither is referenced (`grep -ic` → 0 for each). crates.io renders neither
  on its own.
- **Change the example fence from `rust,ignore` to `no_run`** if it can be
  made to compile — `lib.rs` already uses `no_run` correctly four times, so
  the idiom is established in-tree. `ignore` reads as *the maintainer could not
  make this compile*. If it genuinely cannot compile, that is worth knowing
  and is a different finding.
- Consider one real `examples/echo.rs` — the two files in `examples/` today
  are a latency-audit probe and a TCP benchmark harness, so a consumer running
  `cargo run --example` finds measurement tooling, not onboarding.

---

#### P9 — `README.md`: the comparison table itself · cost **~200 tokens** · value **high, but lower per token than P1–P7**

The §2.2 table, trimmed to four competitors and five rows. This is the block
that converts *shortlisted* into *chosen*, so its value is real — it is ranked
ninth only because P3's "when not to" bullets already capture most of its
extraction value at a fifth of the cost. **Add it after P1–P7 land**, and
accept the maintenance burden knowingly: a comparison table is the block most
likely to go stale, because it makes claims about *other people's crates*.
Keep every cell to a property that changes slowly (identity model, runtime
model, scope) and put no version numbers or benchmark figures in it.

---

### 3.2 Proposals I consider actively harmful

**`llms.txt` or `docs/AI.md` — recommend against, at least for this round.**
Two reasons, and the first is decisive. *(a)* The brief's own model of the
ranker says it fetches the crates.io page and the docs.rs front page. Nothing
in that pipeline fetches `llms.txt`. Building a channel the model says is not
read is pure bloat with a maintenance cost. *(b)* It would be a **fourth**
copy of facts already living in `Cargo.toml` comments, `README.md` and
`src/lib.rs`, and this project has a documented history of exactly that
failure mode — `CLAUDE.md` rule 4 exists because prose drifted away from a
value that had been changed elsewhere, and README:87–88 (P8) is a live example
of a duplicated fact going stale in-tree *right now*. Adding a fourth home for
the same facts, in a file no human ever opens and therefore no human ever
notices is wrong, is the highest-drift-risk proposal on the table. If the
concern is machine consumption, the fix is to make the README extractable —
which is P1–P8 — not to add a machine-only mirror of it.

**Duplicating the six obligations into the README — harmful.** Same drift
argument, and here the duplicated content is *safety-relevant* (the 25 s death
rule, the messages-xor-streams rule). Two copies of a hazard that disagree is
worse than one copy that is hard to find. P7 deliberately specifies
*summarise-and-link*, one line each, pointing at docs.rs.

**A second feature table in the README — harmful.** The `lib.rs` table is
correct and carries a ratification note (ruling 225). Cloning it into the
README creates precisely the manifest-comment-vs-reasoned-statement split that
ruling 225 had to adjudicate. P4 deliberately says *link the table*, and P6
moves it into the docs.rs budget so the link lands somewhere useful.

**A long FAQ — mostly harmful, folded instead.** A 12-question FAQ would
duplicate P3 and P7 under different phrasings and drift independently. The
extraction value of an FAQ is entirely in its *question wording* as retrieval
keys — so capture that by phrasing P7's bullets as the questions agents
actually ask ("Does slither do NAT traversal?", "What's the maximum message
size?", "Can I use it with `#[tokio::main]`?") rather than by adding a
section. Same keys, no second home.

**Benchmark or performance claims in the README — actively harmful.** The
tree has real measurement work (`examples/bench_vs_tcp.rs`,
`.spec-v2-clean-slate/bench-vs-tcp-2026-08.md`), and a "faster than X" line
from a `0.2.0` crypto crate with no independent audit *lowers* the trust score
it is meant to raise. If numbers ship at all, they ship as a linked, dated,
reproducible harness — not as a README claim.

**Softening the MSRV or the `!Send` constraint — harmful.** Both are genuine
costs, both are correctly scored as costs by a ranker, and both have real
rationales. P5 states the `!Send` cost *and* its reason in the same breath;
that is the honest version and it is also the one that scores best, because a
constraint with a stated reason reads as a design decision and an unexplained
one reads as an unfinished implementation.

### 3.3 Suggested order of work

P1, P2, P8 first — together roughly 95 added tokens and one deletion, and they
address the *eliminating* filter (§2.3(i)) plus the one factual defect. Then
P3+P4+P5 as a single README rewrite of lines 1–88. Then P6 (a `lib.rs`
reorder, no new prose). P7 and P9 last, as the blocks that convert rank into
selection.

Expected movement, stated so it can be checked rather than believed: P1+P2+P8
should clear the trust gate and move slither from **5th to 3rd** on the
primary task. P3+P4+P5+P7 should take it to **2nd**, behind iroh, on
substance-plus-clarity. It does not overtake iroh on a task whose first word
is "peer-to-peer" **unless slither traverses NAT**, which it does not and
should not claim to — which is exactly why P3's "when not to" bullet naming
iroh is the honest ceiling, and a better outcome than an unearned first place.

## 4. Brief-level objections and notes

Per hard rule 3, the places I think the brief was off, and one where I
departed from it.

1. **The brief's ranker model omits the pre-fetch cull.** It describes two
   ~2 000-token page fetches, which is right for the shortlist pass but skips
   the search-results pass on ~40 tokens (name + `description` + version +
   downloads). This is not academic: it *reorders the fixes*, promoting the
   zero-cost `Cargo.toml` `description` edit (P1) above every README block.
   Recorded as §2.5.

2. **I added `iroh` to the comparison set, which the brief did not name.** The
   brief offered "boringtun-style WireGuard or libp2p-noise" as the fourth
   slot. For the stated task, iroh — `Endpoint`, dial by public key, no
   certificates, NAT traversal, QUIC streams — is slither's nearest neighbour
   and the crate that actually beats it. Ranking against boringtun and snow
   alone would have produced a flattering and useless answer. I kept boringtun
   and snow as well; the table has six columns rather than four.

3. **Two findings are defects, not wording.** *(a)* README:87–88 asserts
   "There is no `examples/` directory in the tree today" and `examples/`
   exists at `721167a` with two shipped, auto-discovered files. *(b)*
   `#![forbid(unsafe_code)]` is at `src/lib.rs:230`, below the `//!` block, so
   the crate's best security signal never renders on docs.rs while the only
   in-budget "unsafe" token (`lib.rs:146`) extracts as its opposite. Both are
   in scope for a documentation round, but neither is a style question, and
   (a) is a stale-fact failure of exactly the class `CLAUDE.md` rule 4
   describes.

4. **One question I could not answer without violating hard rule 2.** The
   maximum message / datagram size — the MTU and fragmentation story — is a
   top-three consumer question, appears nowhere in the ranked corpus
   (`grep -ic 'MTU\|PMTU'` → 0 in both files), and I could not source it
   without reading `SPEC.md`. P7 flags it; **somebody with spec access has to
   supply the number.** I have not guessed it.

5. **What I did not do.** I did not read `SPEC.md` or `rulings.md` (rule 2),
   made no commits, and wrote to no file in the repo other than this report
   (rule 1). Competitor positioning was fetched live on 2026-08-19 from the
   four GitHub raw READMEs named in §2.1; `libp2p-noise` is characterised from
   knowledge and marked as such. crates.io returns a JS shell to a fetcher, so
   the "crates.io page" half of the simulation was reconstructed from
   `Cargo.toml` metadata plus the rendered README, which is what crates.io
   assembles anyway.
