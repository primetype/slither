# Round 44 — Agent D: the fix plan

**Base commit:** `721167a` — "Round 43: the SECV5 pins land — the one-shot
keepalive, the spent-packet separator, the 272 pin"
(verified as first command: `git log -1 --oneline`).

**Mandate:** one implementation-ready fix plan. Writers execute it without
re-deciding anything. Five decisions are reserved to the maintainer (§8).

---

## 0. Conflicts between input documents

Rule 3: reported, not silently resolved. Where a conflict is settled by a
first-hand check, the command is given so the maintainer can re-run it.

**1. Windows. B's status block vs. the CI matrix — B is wrong.**
`round44-B-ai-ranker.md` §3.1 P2 drafts *"Linux / macOS / Windows"*. The
orchestrator pass had already flagged this as a guess. Verified:
`.github/workflows/test.yml:33` reads `os: [ubuntu-latest, macos-latest]`,
and `check.yml`/`audit.yml` are `ubuntu-latest` only. **There is no Windows
job.** §2 says "Linux and macOS in CI". This is B's only factual error and it
would have shipped a false claim in the highest-visibility line on the page.

**2. The test count. B says 887 attributes / 25 targets; I count 1 147 / 27.**
```
$ grep -rho '#\[\(tokio::\)\?test\b' src tests --include='*.rs' | wc -l
1147          # 796 under src/, 351 under tests/
$ ls tests/*.rs | wc -l
27            # 24 explicit [[test]] stanzas; no `autotests = false`, so all 27 are targets
```
B's `tests/` figure (141) is under half of mine (351); the likeliest cause is
a pattern that missed `#[tokio::test]`, but I did not reconstruct B's grep and
**I am not asserting B ran the wrong command** — I am asserting the two counts
disagree and giving mine with its command. The project's own record puts the
executed total near 1 130. **Decision: the README states "1 100+ tests" and no
target count** — a floor supported by the lowest plausible reading of either
count, which is what B itself recommended ("a bound you will not have to
chase"). The writer re-runs the command above before committing.

**3. tokio features: A says "undocumented, I guessed `full`"; C says a
consumer "does not need to enable anything". Both true, and the resolution is
a mechanism neither states.** slither's own `[dependencies]` entry
(`Cargo.toml:129`) is `tokio = { version = "1", features = ["rt", "net",
"time", "sync", "macros"] }`. Cargo unifies features per crate across a build,
so a consumer writing a bare `tokio = "1"` *does* get those five enabled — C
is right that nothing must be enabled for **slither's** needs. But the
consumer's own code names `tokio::net::UdpSocket` and `tokio::task::LocalSet`,
and relying on a *dependency's* feature selection to make your own code compile
is a graph accident, not a contract: it breaks the moment slither drops a
feature. **Decision: state the five explicitly in the install block.** Neither
report is corrected; the third fact reconciles them.

**4. C's fix 2 vs. the brief's featureless-doctest constraint.** C proposes
lifting `src/shell/mod.rs`'s `#[cfg(test)]` send/recv test into a doc example
"behind `test-util`, `no_run` or `ignore`". Doctests run under **both**
featureless `cargo test` and `--all-features`, so a `testutil`-using doctest
fails the featureless gate whatever its fence. C's *goal* is adopted; its
*mechanism* is replaced (§5). Not a factual conflict — a constraint C was not
given.

**5. Minor, no decision rides on it.** The pass says lib.rs has "only 2
compilable (`no_run`) doctests in the crate root"; C's inventory shows the
crate-root file has **one** `no_run` fence (`lib.rs:61`), one `text`
(`lib.rs:24`) and one `toml` (`lib.rs:325`). C's count matches the file. The
pass was probably counting the `toml` fence.

**6. A's characterisation of the panic site is wrong, and it matters for
§8(ii).** A's fix 3b says the panic "comes from `spawn_local` deep in the
driver's own `tokio::task::spawn_local` call, not from a guard in `build()`
itself". Checked against the code (rule 11): `tokio::task::spawn_local(...)`
is called **directly in `EndpointBuilder::build()`**, at
`src/shell/endpoint.rs:524`, one line before the `Endpoint` is returned. The
site is already slither's own, in the function whose doc already documents the
panic (`src/shell/endpoint.rs:394`). This makes A's own fix *cheaper* than A
believed — see §8(ii).

## 1. Decision log

**Tally: 17 ADOPT · 6 ADAPT · 2 REJECT · 5 RESERVED.**

*Adopted:* A1, A2a, A3a, A's hiss-`default-features` correction; B's P3, P5,
P6.1, P6.2, P6.3, P7; B's §3.2 harmful list (in full, as a filter); C3, C4, C5;
and the three pass findings (the false `examples/` sentence, the stale
Leg-1/Leg-2 status dates, the never-compiled README sketch).
*Adapted:* A2b, P2 (two factual corrections), P4 (longer dependency list),
P8 (the fence half rests on a false premise), C2 (no `testutil`), C's §3
module-opening sweep (two modules, not eight).
*Rejected:* B's optional "trim obligation 1" (it is ruled content) and B's P9
comparison table (cost, staleness, and B ranks it ninth itself).
*Reserved to the maintainer:* §8(i)–(v).

### 1.1 Agent A's fixes

Source: `round44-A-hurried-dev.md`. Base verified `721167a`. Its Phase-2
demo **compiled and ran first try** — so the fault is discovery *order*,
not missing facts. 5 genuine stumbles: (1) `channel!` undiscoverable
[10–15 min], (2) consumer tokio features stated nowhere rendered [3–5],
(3) `#[tokio::main]` reflex → bare tokio `spawn_local` panic naming
nothing of slither [10–20, the give-up moment], (4) `public_static()`
absent from README [3–5], (5) `generate()` absent from README while
README shows the harder `from_scalar` [3–5].

| A's fix | Verdict | How |
|---|---|---|
| **A1** — one complete compile-tested end-to-end example: `channel!` → `generate` → `public_static` → builder + real `UdpSocket` → `block_on` → staged accept → `send_message`/`recv_message` | **ADOPT, twice** | It becomes (a) the lib.rs quickstart doctest, `no_run`, §4; and (b) `examples/echo.rs`, §6. Same shape, two homes. A's final `main.rs` is the proven template — writers derive from it, not from imagination. |
| **A2a** — state consumer tokio features in README | **ADOPT** | README install block, §2. Exact list `rt, net, time, sync, macros` (verified in slither's own `Cargo.toml`). Do **not** recommend `full`: name the five, mention `full` only as a prototyping aside. |
| **A2b** — name `channel!` up front in README | **ADAPT** | README shows the `channel!` line inside its short quickstart pointer, but the *worked* example stays in lib.rs/examples per the maintainer's why/how split. README names it, links it, does not teach it. |
| **A3a** — docs-only `#[tokio::main]` preemption | **ADOPT** | README "Requirements" block + lib.rs quickstart comment. Unconditional — it ships regardless of how the maintainer rules on A3b. |
| **A3b** — `EndpointBuilder::build()` panics with a slither-authored message | **RESERVED → maintainer (§8 ii)**, recommendation: ADOPT | Code change in a docs round; recommendation and alternative in §8. |

**A's hiss-default-features finding is upgraded, not merely adopted.** A
followed lib.rs's own `hiss = "0.3"` snippet and pulled the unneeded
`x25519-cryptoxide` default feature; slither's own manifest uses
`default-features = false`. The README install block and lib.rs's hiss
re-export snippet both get `default-features = false`. This is a
correctness fix to *existing* rendered guidance, not an addition.

### 1.2 Agent B's P1–P9

| # | Proposal | Verdict | How / why |
|---|---|---|---|
| **P1** | `Cargo.toml` `description` — capability first, lineage last | **RESERVED → maintainer (§8 v)**, recommendation ADOPT | Publishing metadata. Final text in §3, 197 chars vs today's 204 — under the current length as the brief requires. |
| **P2** | README status/facts block under the heading | **ADAPT — two corrections** | *(a)* B wrote **"Linux / macOS / Windows"**; the CI matrix is `os: [ubuntu-latest, macos-latest]` (`.github/workflows/test.yml:33`). **There is no Windows job.** The line reads "Linux and macOS in CI". *(b)* B wrote "800+ tests across 25 test targets"; my count is **1 147** `#[test]`/`#[tokio::test]` attributes (796 `src/` + 351 `tests/`) across **27** files in `tests/`, all auto-discovered (24 explicit `[[test]]` stanzas, no `autotests = false`). See §0 conflict 2. The block says **"1 100+ tests"** and drops the target count — a floor that will not drift, per B's own advice. Badges are drafted but **commented out** ("activates on publish", §2). |
| **P3** | Front-loaded what / why / when-not, replacing lines 1–27 | **ADOPT**; the competitor-naming half is **RESERVED → maintainer (§8 iii)**, recommendation ADOPT | `## Lineage` does not survive as a section — its two live facts fold into a four-line `## How it works` paragraph beside the architecture SVG, after the reader has a reason to care. Exactly B's prescription. |
| **P4** | `## Install` block with companion deps | **ADAPT — the dependency list is longer than B's** | B listed slither + tokio + hiss. A's *working* manifest also needed **`rand_chacha`** (for `ChaCha20Rng`, the default `R` of `SoftwareIdentity<S, R>`) and **`getrandom`** (to seed it). Both are normal dependencies of slither (`Cargo.toml` lines in `[dependencies]`), so a consumer copying a three-line block and calling `generate()` does not compile. Five entries, with the last two commented as conditional. `hiss` carries **`default-features = false`** (§1.1). The `rand_core` caveat ships as B specified. |
| **P5** | `## Requirements` — `!Send` in prose, above the example | **ADOPT verbatim in substance** | B's wording is the best in either report: constraint, consumer cost, and rationale in one block. §2 uses it, plus one added sentence naming the *runtime panic* A verified. |
| **P6.1** | Move `# Features` + install pointer above the obligations in `lib.rs` | **ADOPT** | §4, edit 3. The table keeps its ruling-225 `[RATIFIED]` block verbatim. |
| **P6.2** | Rename `# The six documentation obligations` | **ADOPT — as `# Before you integrate`** | B offered two candidates. `# Before you integrate` beats `# Limits, hazards and gotchas` because §2's README `## Limits` block links here, and two headings both called "Limits" on adjacent surfaces is the split ruling 225 warns about. The count, the framing and all six items are unchanged — heading text only. |
| **P6.3** | State `forbid(unsafe_code)` inside the `//!` block | **ADOPT** | Verified: the attribute is at `src/lib.rs:230`, below the doc block, so it never renders. §4, edit 1 puts it in the opening, together with the no-RustCrypto policy B correctly identifies as unstated. |
| **P6 optional** | Trim obligation 1's 20-line `no_run` block | **REJECT** | Ruled content (slice 9, §6.5 obligation 6). "Trim to a 6-line core" is *weakening* an obligation, which this round may not do. B's budget problem is solved instead by the quickstart landing **above** it (so the in-budget material is now a working example rather than a corner case) and by P6.1's relocation. |
| **P7** | README `## Limits` block | **ADOPT, with the numbers B could not source** | Reliable message **≤ 262 144 B** (`MESSAGE_RECV_MAX`, `src/constants.rs:312`); unreliable datagram payload **≤ 1 169 B** (`MAX_DATAGRAM_PAYLOAD`, `:364`); every wire datagram **≤ 1 200 B** (`MAX_DATAGRAM`, `:138`), never fragmented. B's §4.4 asked for exactly this and refused to guess — correctly. Summarise-and-link only; no obligation text is copied (§3.2). |
| **P8** | Free hygiene — delete the stale `examples/` sentence, link `SECURITY.md` and `CHANGELOG.md`, change the fence, add `examples/echo.rs` | **ADOPT 3 of 4; ADAPT the fence item** | Deletion, both links and `examples/echo.rs` all adopt. **The fence item rests on a false premise**: README fences are not compiled by anything — `grep -rn include_str src/ Cargo.toml` shows **no `#![doc = include_str!("../README.md")]`**, so `rust,ignore` → `no_run` changes zero mechanics and only the signal. §2 therefore drops `ignore` (plain ` ```rust `), keeps the snippet to ~12 lines, and earns the trust signal *honestly* by saying the full example is `examples/echo.rs`, which `cargo build --all-targets` compiles as a release gate. |
| **P9** | The full comparison table in README | **REJECT for this round** | Three reasons, in order. *(a)* The maintainer's binding directive is "not too many words — simplicity of reading is a goal in itself", and P9 is ~200 tokens of table. *(b)* B itself ranks it ninth and says P3's when-not bullets already capture most of its value at a fifth of the cost — §2 adopts those. *(c)* B names the maintenance hazard itself: a table making claims about *other people's crates* is the block most likely to go stale, and this project's rule 4 exists because a fact drifted in a file nobody swept. Revisit once P1–P8 have been measured. |

### 1.3 Agent B's §3.2 harmful list (applied as a filter over everything)

Adopted **in full**, as a filter, and it removes material from the other two
reports as well as from B's own:

1. **`llms.txt` / `docs/AI.md` — not planned.** No writer creates one.
2. **Duplicating the six obligations into README — forbidden.** §2's `## Limits`
   is one line per hazard plus a link. It is *also* the maintainer's ruled
   content constraint, so this is doubly binding.
3. **A second feature table — forbidden.** §2's install block links the
   docs.rs table; ruling 225 already adjudicated this split.
4. **A long FAQ — not planned**; B's technique is adopted instead, so §2's
   `## Limits` and `## When not to use it` bullets are phrased as the answers
   to the questions agents actually ask (NAT? max message size? `#[tokio::main]`?).
5. **Benchmark or performance claims — forbidden.** Nothing in §2 or §4 cites
   a latency or throughput figure, and `examples/bench_vs_tcp.rs` is not linked
   from README.
6. **Softening the MSRV or the `!Send` constraint — forbidden.** §2 states both
   plainly and gives the `!Send` reason in the same breath.

**Filter applied to C:** C's fix 2 proposes lifting the `#[cfg(test)]`
send/recv test into a doc example "behind `test-util`". That would break the
featureless `cargo test --doc` gate — see §0 conflict 4 and §5, where it is
re-specified over real UDP loopback with no `testutil`.

**Filter applied to A:** A's fix 2 suggests telling readers to use
`tokio = { features = ["full"] }` "while prototyping". Not planned — it is
the guess A made, not a documented fact, and naming the five real features
costs the same line.

### 1.4 Agent C's five fixes

| # | C's fix | Verdict | How |
|---|---|---|---|
| **C1** | Make `staged` / `endpoint` / `connection` / `stream` `pub mod`, **or** fold their opening prose into `shell/mod.rs`'s public `//!` | **RESERVED → maintainer (§8 i)**, recommendation: **the fold** | Verified first-hand: `src/shell/mod.rs:50-55` declares six private `mod`s; only `pub mod wire;` is public. C's finding is correct and is the round's best structural catch. |
| **C2** | Runnable two-endpoint example on `send_message` / `recv_message` | **ADAPT** | Adopted as the single biggest reference gap, but **not** by lifting the `#[cfg(test)]` test: that needs `testutil` and the paused clock, and a `test-util`-gated doctest fails featureless `cargo test --doc`. Re-specified in §5 as one shared `no_run` example over `tokio::net::UdpSocket` loopback, on `send_message`, with `recv_message` linking to it (one home, per §3.2's drift rule). |
| **C3** | Compiled examples on `SoftwareIdentity::generate` and `Identity::public_static` | **ADOPT** | §5. `generate` gets the RNG recipe finished — today `identity.rs:189` stops one line short, with the actual call as a `//` comment. `public_static` gets the out-of-band-exchange example that exists nowhere in the crate. |
| **C4** | A "Start here" quick-path at the top of `lib.rs`, before `# Shape` | **ADOPT, and upgraded** | The maintainer's directive is stronger than C's proposal: not a link list but the **full worked quickstart doctest**, first. §4, edit 2. C's insight that prose above the alphabetical Modules table is the *only* available lever is correct and is why §4 also reorders the hand-written `# Modules` list (C's §4 finding: two different orderings on one page, neither leading with `identity` or `shell`). |
| **C5** | Plain first-connect example on `Endpoint::connect` or `Connecting` | **ADOPT — on `connect`** | §5. `connect` is the item a reader reaches from `Endpoint`; `Connecting` gets a one-line link to it rather than a second copy. |

**C's §3 module-doc finding (8/8 open reviewer-first) — ADAPT, narrowly.**
The finding is true and well quantified, but "rewrite every module's opening
sentence" is a large surface with a real cost: the `(§X.Y)` anchors are what
make the module layout navigable against a ratified spec, which `lib.rs`
states as deliberate. **Decision: only the two modules on the golden path
change** — `identity` and `shell` get a user-facing first sentence with the
spec citation moved to the second. The other six keep their openings. This is
in §5's table with exact text.

### 1.3 Agent B's §3.2 harmful list (applied as a filter)
### 1.4 Agent C's five fixes
### 1.5 The orchestrator pass's hypotheses

| Hypothesis | Outcome | Consequence for this plan |
|---|---|---|
| (a) README sketch compiles once wrapped in a LocalSet main | **Confirmed** by A (with additions: it needs `channel!` and a concrete identity). | The README sketch is not *wrong*; it is *incomplete*. It is replaced by a link, not by a correction. |
| (b) The fastest stumble is `!Send`/LocalSet | **Confirmed**, and sharper than expected: it is a *runtime* panic naming no slither symbol. | Requirements block in README (§2), quickstart entry shape in lib.rs (§4), reserved decision §8(ii). |
| (c) An AI reading README's first 2000 tokens cannot answer "install + send one message" | **Confirmed**, refined by B: elimination happens ~40 tokens earlier, in the search-results pass, i.e. in `Cargo.toml`'s `description`. | §3 rewrites the description; README's first screen carries install + limits + requirements. |

Pass findings adopted without further argument (all first-hand verified by
the orchestrator): README's `There is no examples/ directory` sentence is
false and must die (§10 grep list); the Status section's Leg-1/Leg-2 dates
are stale against the 2026/08/14 full RATIFIED stamp (ruling 242) and
v0.2.0; the README code block is `rust,ignore` and compile-tested nowhere.

### 1.6 Merge table (B × C overlaps)

| Overlapping concern | B's form | C's form | Merged decision |
|---|---|---|---|
| No end-to-end compiled example anywhere | P8's "consider one real `examples/echo.rs`" | C2 + C4 ("zero doctests exercise a message between two endpoints") | **One example, three renderings, one source of truth**: `examples/echo.rs` is written first (§6); the `lib.rs` quickstart doctest (§4) is its compressed twin; README carries a ~12-line extract and a link. The writers' partition (§9) keeps these on disjoint paths and §10's grep list checks they agree. |
| `lib.rs` front page is badly ordered for the reader | P6.1/P6.2 (extraction budget) | C4 (click distance) | Same edit list, §4. Both diagnoses point at the same reorder; neither adds an item the other's does not. |
| Feature table unreachable | P6.1 (past the token cut) | C1(g) ("crate feature flags **are** well documented, 0 clicks") | **They disagree in emphasis, not in fact** — C measures clicks (0), B measures tokens (~2 900, past a 2 000 cut). Both are right about their own metric. The table moves up: it costs nothing for C's metric and fixes B's. |
| tokio features undocumented | P4 (consumer manifest) | C1(g) (rendered surface) | Merged, with C's mechanism correction folded in — see §0 conflict 3. |
| `SECURITY.md` / audit status invisible | P2 + §1.2g | not raised | B's; §2's status block. |
| The stale "no `examples/`" sentence | P8, called a defect | not raised | B's; §10's grep list makes its death checkable. |


## 2. The new README.md (full draft, ready to commit)

**133 lines, 865 words.** The brief targeted "≤ ~110 lines"; I missed it
deliberately and say so rather than trimming to the number (rule 5). Three
reasons. *(a)* The brief's own content list names ten blocks; ten headings plus
their blank lines are ~35 lines before a word is written. *(b)* B's central
thesis is that **a heading is a retrieval key** — I merged `Testability` /
`Status` / `License` into one heading to hit 129 and then reverted it, because
losing the `License` key to save four lines is a bad trade on the page
crates.io renders. *(c)* Against the maintainer's actual directive — *"not too
many words"* — this draft is **+8 % words** over today's README (865 vs 803)
while **adding** the status block, install, requirements, when-not-to, limits
and two diagrams, and deleting the 20-line one-paragraph protocol dump. If the
maintainer wants the line count regardless, the cheapest 13 lines are the
Quickstart code fence; I recommend against it, because that fence is the only
place a skimming reader sees `channel!`, `generate()` and `public_static()` —
A's stumbles 1, 5 and 4 — in one glance.

Every factual claim was checked at `721167a`; the two "activates on publish"
items carry HTML comments in the file itself.

````markdown
# slither

<!-- ACTIVATES ON PUBLISH: no git remote exists at 721167a, so these badges 404
     and crates.io cannot rewrite the docs/*.svg links until the repo is public.
[![crates.io](https://img.shields.io/crates/v/slither.svg)](https://crates.io/crates/slither) [![docs.rs](https://docs.rs/slither/badge.svg)](https://docs.rs/slither) [![CI](https://github.com/primetype/slither/actions/workflows/check.yml/badge.svg)](https://github.com/primetype/slither/actions) -->

**v0.2.0** · MSRV **1.96** (edition 2024) · `MIT OR Apache-2.0` ·
`#![forbid(unsafe_code)]` · wire **ratified and frozen** ([`SPEC.md`](SPEC.md))
· 1 100+ tests · Linux and macOS in CI · **not independently audited** —
see [`SECURITY.md`](SECURITY.md).

Two peers exchange encrypted, reliable, unordered messages over UDP — plus
streams and unreliable datagrams — authenticated by raw public keys. No
certificates, no TLS, no PKI.

## Why slither

- **A peer is its public key.** No CA, no trust store, no certificate plumbing.
- **A staged accept ladder** — your application inspects a *claimed* identity
  and authorises it **before** the second Diffie-Hellman is spent; dropping the
  handle is the silent reject.
- **Connections roam** — an authenticated packet from a new address moves the
  session there; nothing unauthenticated ever does.
- **Drivable without a kernel** — two pure state machines behind one `Wire`
  trait, so your tests run the real protocol in memory on a paused clock.

## When not to use it

- Need **NAT traversal or relay fallback**? Use [iroh](https://crates.io/crates/iroh).
- Have **certificates**, want mainstream QUIC? Use [quinn](https://crates.io/crates/quinn).
- Want the **Noise handshake alone**, no transport? Use [snow](https://crates.io/crates/snow).

## Install

```toml
[dependencies]
slither = "0.2"
# `slither::channel!` expands to `::hiss::…`, so your crate needs hiss too.
hiss = { version = "0.3", default-features = false }
# slither's driver runs on YOUR runtime; these are the features it uses.
tokio = { version = "1", features = ["rt", "net", "time", "sync", "macros"] }
rand_chacha = "0.10"   # only for `SoftwareIdentity` — it takes an RNG you own
getrandom = "0.4"      # …and something to seed it from
```

**Nothing is on by default**; `test-util`, `sink`, `codec` and `tower` are
opt-in, and the table is on [docs.rs](https://docs.rs/slither). **`rand_core`
must be the 0.10 line hiss names** (`hiss::rand_core` re-exports it): two
majors in one graph give an unsatisfiable `CryptoRng` bound, not a version error.

## Requirements

**slither's driver is `!Send`.** It runs with `tokio::task::spawn_local` on a
**current-thread** runtime inside a **`LocalSet`**, and no handle crosses a
thread. `slither::block_on` is the one line that pays that tax.

If your application uses `#[tokio::main]` — the multi-threaded runtime —
`Endpoint::builder()…build()` **panics at runtime**; run slither on its own
current-thread runtime and bridge with channels. This is deliberate: it lets a
hardware-backed static key — an iOS Secure Enclave `SecKey`, not `Send` —
drive the handshake.

## Quickstart

```rust
slither::channel! { pub MySuite<P256, ChaChaPoly, Blake2b>; }   // 1. one suite
slither::block_on(async {                       // 2. current-thread + LocalSet
    let me: SoftwareIdentity<MySuite> = SoftwareIdentity::generate(rng())?;
    let my_key = me.public_static().clone();    // 3. hand this to the peer
    let sock = tokio::net::UdpSocket::bind("0.0.0.0:0").await?; // it's a `Wire`
    let ep = Endpoint::builder().identity(me).wire(sock).build();
    let conn = ep.connect(peer_addr, peer_key)?.await?;          // 4. dial …
    conn.send_message(b"hello").await?;
    // … or answer: accept() -> read_identity() -> authenticate() -> accept()
});
```

![the staged accept ladder](docs/staged-accept.svg)

**The full worked example is [`examples/echo.rs`](examples/echo.rs)** — two
endpoints on UDP loopback, one message, clean close (`cargo run --example
echo`). It also opens [docs.rs](https://docs.rs/slither), compiled every run.

## Limits

- **No NAT traversal, no relays.** You supply reachable addresses.
  `set_persistent_keepalive` holds a NAT binding open; it does not punch one.
- **A reliable message is at most 262 144 B** (256 KiB); an unreliable datagram
  payload at most **1 169 B**. Every wire datagram is **≤ 1 200 B**, never
  fragmented.
- **A connection carrying no traffic dies in 25 s, in silence.** Connecting
  ahead of need does not keep a path warm.
- **One session per peer static** — reconnecting is `close()` then dial; and
  **messages and streams do not mix on one connection**.
- **Reliability lives inside a connection**; what a dead connection had not
  delivered is lost. No pacing, no ECN, no PMTUD; NewReno only.

Each is stated in full, at its call site, under **Before you integrate** on
[docs.rs](https://docs.rs/slither).

## How it works

![slither's architecture](docs/architecture.svg)

slither borrows WireGuard's homework — a keyed-BLAKE2b mac1 DoS gate,
fresh-ephemeral handshake retransmission, an RFC 6479 replay window, roaming
and the keepalive/liveness/rekey timers — over
[`hiss`](https://crates.io/crates/hiss)'s Noise **IK** (**P-256 /
ChaCha20-Poly1305 / BLAKE2b**; no RustCrypto crates). Inside the sealed packets
rides a QUIC-shaped frame layer: streams, messages, datagrams, RFC 9002 loss
recovery and NewReno.

## Testability

The socket sits behind a small `Wire` trait, so the suite runs two endpoints
over an in-memory `FlakyWire` (loss, reorder, duplication, delay, partition,
send failure) on tokio's **paused clock** — the 5 s / 10 s / 25 s / 90 s timers
resolve in virtual time. Enable `test-util` to do the same in your own tests.

## Status

The wire is **ratified and frozen** (2026/08/14): every constant, header
layout, frame type and timer lives in [`SPEC.md`](SPEC.md), and the code
follows the spec, never the reverse — see [`CHANGELOG.md`](CHANGELOG.md). An
**independent crate**: zero `bubble-*` deps, everything from crates.io.

## License

Licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at
your option. Unless you explicitly state otherwise, any contribution
intentionally submitted for inclusion in the work by you, as defined in the
Apache-2.0 license, shall be dual licensed as above, without any additional
terms or conditions.
````

### 2.1 Claim-by-claim provenance (for the writer and the reviewer)

| Claim in the draft | Verified how |
|---|---|
| `v0.2.0`, MSRV `1.96`, edition 2024, `MIT OR Apache-2.0` | `Cargo.toml` `[package]` |
| `#![forbid(unsafe_code)]` | `src/lib.rs:230` |
| "1 100+ tests" | `grep -rho '#\[\(tokio::\)\?test\b' src tests --include='*.rs' \| wc -l` → 1147 (§0 conflict 2) |
| "Linux and macOS in CI" | `.github/workflows/test.yml:33` — `os: [ubuntu-latest, macos-latest]`; **no Windows** |
| "not independently audited" | `SECURITY.md` states the delegation to hiss; no audit is claimed anywhere. §8(iv) |
| tokio features `rt, net, time, sync, macros` | `Cargo.toml:129` |
| `hiss = { version = "0.3", default-features = false }` | `Cargo.toml:101` uses `default-features = false`; A proved the naive copy pulls `x25519-cryptoxide` |
| `rand_chacha = "0.10"`, `getrandom = "0.4"` | `[dependencies]`; A's working manifest needed both |
| `rand_core` 0.10 caveat | `Cargo.toml:141-142` comment; `CLAUDE.md` crypto rules |
| 262 144 B | `MESSAGE_RECV_MAX`, `src/constants.rs:312` |
| 1 169 B | `MAX_DATAGRAM_PAYLOAD`, `src/constants.rs:364` |
| ≤ 1 200 B, never fragmented | `MAX_DATAGRAM`, `src/constants.rs:138` |
| 25 s death, `set_persistent_keepalive` | `DEAD_TIMEOUT`, `src/constants.rs:512`; lib.rs obligation 3 |
| 5 s / 10 s / 25 s / 90 s | `RETRANSMIT_BASE`, `KEEPALIVE_TIMEOUT`, `DEAD_TIMEOUT`, `HANDSHAKE_GIVEUP` |
| "one session per peer static", "messages xor streams" | lib.rs obligations 1 and 5 — **summarised and linked, never copied** (§3.2) |
| ratified 2026/08/14 | ruling 242, `cd12ed7`; replaces the stale 2026/07/16 and 07/17 dates |
| `cargo run --example echo` | auto-discovered; no `autoexamples = false` in `Cargo.toml` |

**Deletions the writer must make** (each is a live defect, not a preference):
the `## Lineage` section, the 20-line `## The protocol in one paragraph`, the
`rust,ignore` sketch with `my_static_scalar` / `my_rng` / `peer_static` /
`my_allow_list`, the sentence *"There is no `examples/` directory in the tree
today"*, and the Leg-1/Leg-2 (2026/07/16, 2026/07/17) status dates.

## 3. The new Cargo.toml `description`

Current: **204 chars**. Proposed: **187 chars** — shorter, as the brief
requires. B's P1 shape: capability, then differentiator, then lineage. (My
first draft was 205 and I only found that by counting it — worth doing again
before committing: `python3 -c "print(len(open('Cargo.toml').read()))"` is not
the check; count the string itself.)

```toml
description = "Encrypted peer-to-peer UDP transport: reliable messages, streams and datagrams, authenticated by raw public keys - no certificates, no TLS. WireGuard-shaped handshake, QUIC-shaped frames."
```

*"between two peers" was dropped as redundant with "peer-to-peer" — which is
also what brought it under length.*

Notes for the integrator, who owns this file (§9): keep it ASCII — a TOML
string is fine with an em dash, but crates.io search snippets and terminal
`cargo search` output are not uniformly, so the two dashes above are ASCII
hyphens. `keywords` and `categories` are **not** touched: B verified all five
keywords earn their place. This edit is **RESERVED → maintainer (§8 v)**.

## 4. The lib.rs restructure (surgical edit list)

`src/lib.rs`'s `//!` block is lines **1–228**. Nothing below line 229 changes
in this round except as noted in edit 8.

### 4.1 The new section order

| # | Section | Origin |
|---|---|---|
| 1 | opening paragraph (untitled) | lines 1–14, **kept**, one sentence appended |
| 2 | `# Quickstart` | **new** — the doctest in §4.3 |
| 3 | `# Install` | **new** — six lines, §4.4 |
| 4 | `# Features` | **moved verbatim** from 208–228, RATIFIED block included |
| 5 | `# Shape` | lines 22–43, **kept verbatim** (heading, ASCII block, both paragraphs) |
| 6 | `# Before you integrate` | lines 45–179, **kept verbatim; heading text only changes** |
| 7 | `# The spec is the authority` | lines 16–20, **moved down**, gains a heading |
| 8 | `# Modules` | lines 181–206, **reordered and two bullets de-staled** |

### 4.2 Edit 1 — the opening paragraph

**KEEP lines 1–14 exactly as they are.** Append one sentence to the end of that
paragraph (B's P6.3, and B's §1.2g finding that the crate's best security
signal never renders):

```text
//! slither is `#![forbid(unsafe_code)]`; every Noise and curve operation
//! goes through `hiss`, and no RustCrypto crate appears in the graph.
```

Nothing else in the opening changes. In particular the `(§6.5, documentation
obligation #6)` reference at line 14 **stays**: the rename in edit 6 changes a
heading, not the numbering, and eight call sites across `src/` cite
"documentation obligation #N" (`core/endpoint/staged.rs:83`,
`shell/staged.rs:85,296`, `shell/connection.rs:60`, `shell/endpoint.rs:36,44,68`).
Verified with `grep -rn "six documentation obligations" src/ tests/ README.md`
→ **one hit, `src/lib.rs:45`**, and `grep -rn "crate#" src/` → **no anchor
links** point at the old heading. The rename breaks nothing.

### 4.3 Edit 2 — `# Quickstart`, immediately after the opening

Fence kind: **`no_run`** — it binds real UDP sockets and would otherwise run a
handshake inside `cargo test --doc`. It contains its own `fn main`, so rustdoc
does not wrap it and `channel!` sits at true module scope. It uses `hiss`,
`rand_chacha` and `getrandom`, all three normal `[dependencies]` — no feature
gate, so it compiles under **featureless** `cargo test --doc`. It is A's proven
`main.rs`, compressed.

```text
//! # Quickstart
//!
//! Two peers on one machine: the dialler sends one message, the answerer
//! reads it, both close. The same program is [`examples/echo.rs`].
//!
//! ```no_run
//! use hiss::noise::{Blake2b, ChaChaPoly, P256};
//! use rand_chacha::ChaCha20Rng;
//! use rand_chacha::rand_core::SeedableRng;
//! use slither::identity::SoftwareIdentity;
//! use slither::{Config, Endpoint, Identity};
//!
//! // 1. Every consumer declares one crypto suite. IK is the only pattern,
//! //    and one invocation per module (the generated type is named `IK`).
//! slither::channel! {
//!     /// This application's suite.
//!     pub MySuite<P256, ChaChaPoly, Blake2b>;
//! }
//!
//! fn rng() -> ChaCha20Rng {
//!     let mut seed = [0u8; 32];
//!     getrandom::fill(&mut seed).expect("OS entropy");
//!     ChaCha20Rng::from_seed(seed)
//! }
//!
//! fn main() {
//!     // 2. `block_on` is the current-thread runtime + `LocalSet` the
//!     //    `!Send` driver needs. Do NOT use `#[tokio::main]`.
//!     slither::block_on(async {
//!         // 3. Two identities. `generate` makes a fresh static keypair.
//!         let dialler: SoftwareIdentity<MySuite> =
//!             SoftwareIdentity::generate(rng()).unwrap();
//!         let answerer: SoftwareIdentity<MySuite> =
//!             SoftwareIdentity::generate(rng()).unwrap();
//!         // 4. The key the dialler needs, handed over out of band.
//!         let answerer_key = answerer.public_static().clone();
//!
//!         // 5. A `tokio::net::UdpSocket` is a `Wire` out of the box.
//!         let a = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
//!         let b = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
//!         let answerer_addr = b.local_addr().unwrap();
//!
//!         let ep_a = Endpoint::builder()
//!             .identity(dialler).wire(a).config(Config::new()).build();
//!         let ep_b = Endpoint::builder()
//!             .identity(answerer).wire(b).config(Config::new()).build();
//!
//!         // 6. Answering is a ladder, so the application can authorise a
//!         //    *claimed* identity before the second DH is spent. Dropping
//!         //    a rung is the silent reject. `accept()` is a LOOP for the
//!         //    lifetime of the endpoint — see "Before you integrate" #6.
//!         let answering = tokio::task::spawn_local(async move {
//!             let intro = ep_b.accept().await.expect("endpoint alive");
//!             let claimed = intro.read_identity().await.unwrap();  // +1 DH
//!             let proven = claimed.authenticate().await.unwrap();  // +1 DH
//!             let conn = proven.accept().await.unwrap();           // +2 DH
//!             let msg = conn.recv_message().await.unwrap();
//!             assert_eq!(msg, b"hello");
//!             conn.close(slither::constants::NO_ERROR, b"done").await;
//!         });
//!
//!         // 7. `connect()` is synchronous and spends 0 DH; awaiting the
//!         //    `Connecting` future is what runs the handshake.
//!         let conn = ep_a
//!             .connect(answerer_addr, answerer_key)
//!             .unwrap()
//!             .await
//!             .unwrap();
//!         conn.send_message(b"hello").await.unwrap();
//!         conn.acked().await.unwrap();
//!         conn.close(slither::constants::NO_ERROR, b"done").await;
//!         answering.await.unwrap();
//!     });
//! }
//! ```
```

Two writer notes. *(a)* `[`examples/echo.rs`]` is **not** an intra-doc link —
write it as plain text or as a full `https://github.com/...` URL once the repo
is public; a bare bracket pair will fail `RUSTDOCFLAGS=-D warnings` as a broken
link. *(b)* `Identity` must be in scope for `public_static()`; it is
re-exported at the crate root, hence `use slither::{Config, Endpoint, Identity};`.

### 4.4 Edit 3 — `# Install`, then `# Features` moved above the obligations

New `# Install` section, placed after the quickstart:

```text
//! # Install
//!
//! ```toml
//! [dependencies]
//! slither = "0.2"
//! # `slither::channel!` expands to absolute `::hiss::…` paths, so your
//! # crate must depend on hiss directly, on the same minor line.
//! hiss = { version = "0.3", default-features = false }
//! # slither's driver runs on your runtime; these are the features it uses.
//! tokio = { version = "1", features = ["rt", "net", "time", "sync", "macros"] }
//! rand_chacha = "0.10"   # only for `SoftwareIdentity`: it takes your RNG
//! getrandom = "0.4"      # …and something to seed it from
//! ```
//!
//! `rand_core` must be the **0.10** line hiss names — [`hiss::rand_core`]
//! re-exports it. Two `rand_core` majors in one graph produce an
//! unsatisfiable `CryptoRng` bound, not a version error. MSRV **1.96**.
```

Then **move lines 208–228 (`# Features`, the table, and the ruling-225
`[RATIFIED 2026/08/16]` block) here, byte-for-byte.** The RATIFIED block is
preserved verbatim, per the brief. This is the whole of B's P6.1.

**Delete nothing from the `hiss` re-export doc at lines 317–335** — but the
writer must fix one line inside it: its `toml` fence at 325–329 says
`hiss = "0.3"` with no `default-features`, and A followed exactly that line
into an unwanted `x25519-cryptoxide`. Change that one line to
`hiss = { version = "0.3", default-features = false }`. The same fence is
duplicated at `src/packet/suite.rs:198-201` — W3 owns that copy (§9).

### 4.5 Edits 4–5 — `# Shape` kept, unchanged

**Lines 22–43 move as one block and are not edited.** The ASCII diagram stays;
the SVGs are README-facing only (§7). The `!Send` paragraph (32–38) and the
kernel-free paragraph (40–43) are the corpus's best prose per B and are kept
word for word.

### 4.6 Edit 6 — the obligations heading

**One line changes.** Line 45:

```text
- //! # The six documentation obligations
+ //! # Before you integrate
```

Everything from line 47 to line 179 — the preamble sentence and all six
obligations, including obligation 1's 20-line `no_run` example — is **kept
verbatim**. This is ruled content (slice 9, §6.5 obligation 6); it moves within
the file and its heading is renamed, and that is all this round may do to it.
B's optional "trim obligation 1" is rejected in §1.2.

### 4.7 Edit 7 — `# The spec is the authority`

Lines 16–20 move to just above `# Modules` and gain their own heading:

```text
//! # The spec is the authority
//!
//! **`SPEC.md` is the authority.** Every constant, header layout, frame
//! type and timer in this crate is ratified there, and where the code and
//! the spec disagree the spec is right. The module layout is deliberately
//! one-to-one with the spec's sections so a reviewer can find the code for
//! a section without searching.
```

Body text unchanged; only the leading `**`SPEC.md` is the authority.**` now
follows a heading that repeats it, which reads fine and keeps the sentence
greppable. It sits here because its second half is about the module layout the
next section lists — and because governance is no longer the second thing a
consumer meets.

### 4.8 Edit 8 — `# Modules`, reordered and de-staled

Newcomer order, per the brief, with `core` last because it is not public:

1. `identity` — kept.
2. `shell` — **REWRITE, it is stale (rule 4).** Today: *"Slice by slice it
   grows the driver and the handles; today it carries [`shell::wire::Wire`],
   the datagram seam an application supplies."* The driver and every handle
   exist at `721167a`. Replacement: *"the I/O shell: [`Endpoint`],
   [`Connection`], the staged accept ladder, the stream handles, and
   [`shell::wire::Wire`] — the datagram seam an application supplies."*
3. `config` — kept.
4. `error` — kept ("Its ten types" verified: the crate-root `pub use` list has
   exactly ten).
5. `packet` — kept.
6. `constants` — kept.
7. `compat` — kept.
8. `testutil` — kept, including the ruling-60 attestation sentence.
9. `core` — **REWRITE, it is stale.** Today: *"Crate-internal until the driver
   that can drive them exists."* That driver exists (`src/shell/driver.rs`).
   Replacement, stating the fact without inventing a rationale: *"§16.4's two
   sans-io state machines. Crate-internal: nothing outside the crate drives
   them directly."*
   **Flagged, not fixed:** the same stale clause appears in the non-rendered
   code comment at `src/lib.rs:242-247` ("publishing the surface now would
   freeze an unusable one"), whose rationale may or may not still be the
   maintainer's position. It is a `//` comment, invisible on docs.rs, and
   changing a recorded rationale is not a documentation-round decision. **This
   is a rule-4 finding for the maintainer, listed here so it is not lost.**

## 5. New doc examples across src/

Every one compiles **featureless** — all use only `slither`, `hiss`,
`rand_chacha`, `getrandom` and `tokio`, which are normal `[dependencies]`. **No
example may touch `testutil`** (it would fail featureless `cargo test --doc`);
this is the correction to C's fix 2 recorded in §0 conflict 4.

| File | Item | Fence | What it shows |
|---|---|---|---|
| `src/shell/connection.rs` | `Connection::send_message` | **`no_run`** | **C2, the round's biggest reference gap.** Two endpoints on `127.0.0.1:0`; the answerer climbs the ladder and calls `recv_message`; the dialler calls `send_message` → `acked` → `close`. Structurally the quickstart minus the identity setup, which is `#`-hidden. |
| `src/shell/connection.rs` | `Connection::recv_message` | **none** | One sentence + intra-doc link to `send_message`'s example. **One home for the example** — §3.2's drift rule; two copies of one idiom is how they diverge. |
| `src/identity.rs` | `SoftwareIdentity::generate` | **plain (runs)** | **C3.** Finishes the recipe that `identity.rs:189` leaves as a `//` comment: seed a `ChaCha20Rng` from OS entropy, then `SoftwareIdentity::<MySuite>::generate(rng)?`. Needs no socket, so it **runs** — a stronger signal than `no_run`, and it exercises real keygen. Requires a `channel!` invocation in the fence to supply `MySuite`. |
| `src/identity.rs` | `Identity::public_static` | **plain (runs)** | **C3.** The accessor that exists nowhere in a doc fence today: generate an identity, `let key = id.public_static().clone();`, and one comment saying this is the value the peer passes to `connect()` and that it travels out of band. Answers A's stumble 4. |
| `src/shell/endpoint.rs` | `Endpoint::connect` | **`no_run`** | **C5.** A plain **first** connect — bind, build, `connect(addr, peer_key)?.await?`, send, close. Today the only compiled `connect` example is the crate root's *reconnect* corner case. Keeps `connect`'s existing prose and its `text` cancel-then-redial diagram; the fence is added, nothing is removed. |
| `src/shell/endpoint.rs` | `Connecting` | **none** | One sentence + link to `connect`'s new example. Same one-home rule. |
| `src/identity.rs` | module `//!` | — | **C's §3, narrowed.** First sentence becomes user-facing; the `§16.4` citation moves to sentence two. Today: *"The identity seam — the `I` in §16.4's `core::Endpoint<I: Identity>`."* Proposed: *"Your static keypair, and the seam that lets it live in hardware. [`SoftwareIdentity`] is the in-memory default; implementing [`Identity`] yourself puts the key behind a Secure Enclave or an HSM. It is the `I` in §16.4's `core::Endpoint<I: Identity>`."* |
| `src/shell/mod.rs` | module `//!` | — | Same narrowing. Today: *"The I/O shell — `SPEC.md` §16."* Proposed: *"Everything you call: [`Endpoint`], [`Connection`], the staged accept ladder and the stream handles. One `!Send` driver task owns the cores and the [`wire::Wire`]. `SPEC.md` §16."* **Plus the §8(i) fold, if the maintainer takes it.** |
| `src/packet/suite.rs` | `channel!` | — | Its `toml` fence at `:198-201` says `hiss = "0.3"`; change to `hiss = { version = "0.3", default-features = false }`. A followed the identical line in `lib.rs` into an unwanted `x25519-cryptoxide`. Two copies of this fence exist; both are fixed (the other is `lib.rs:325`, W2's). |

**No other module's opening sentence changes.** C measured 8/8 opening
reviewer-first and that is true; rewriting all eight would strip the `(§X.Y)`
anchors that `lib.rs` states are deliberate, on six modules a newcomer does not
reach first. Two modules are on the golden path; two modules change.

## 6. examples/echo.rs

**It is A's `main.rs`, which compiled and ran first try — cleaned, commented,
and made self-contained.** It is written **first**, by W4, and the `lib.rs`
quickstart doctest (§4.3) is its compressed twin. Where the two differ, the
example is right.

**Comment voice — decided, and binding on W4:** second person, present tense,
one line per idea. Say what the line does and what goes wrong without it.
**No `§X.Y` citations, no ruling numbers, no "slice N" vocabulary anywhere in
this file** — it is the one artefact in the tree written purely for someone who
has never read `SPEC.md`. Numbered steps `1.`–`7.`, matching the quickstart's
numbering exactly, so a reader moving between them does not re-orient.

**Trim from A's version:** every meta-comment about where A found something
(*"Following packet::suite.rs's `channel!` doctest verbatim"*, *"per wire.rs's
promise"*) — they document A's search, not the API. **Keep:** the `expect()`
messages, which are load-bearing as documentation of what each step can fail at.

**Add**, beyond A's version:
- a `//!` header: what the program does, `cargo run --example echo`, and the
  one warning — *"this uses `slither::block_on`, not `#[tokio::main]`; the
  driver is `!Send`"*;
- a `println!` of the answerer's public key bytes before the dial, with a
  comment that in a real deployment this is what crosses the out-of-band
  channel — the gap A hit at stumble 4;
- the dialler calling `recv_message` for a reply, so **both** verbs appear on
  both sides — it is an *echo* example, and C's gap is the pair, not the send;
- one comment on the `accept()` call stating it is a loop for the endpoint's
  lifetime and that this example takes one and exits **because it is an
  example** (obligation 6 in one sentence, without restating the obligation).

**Constraints.** Featureless: only `slither`, `hiss`, `rand_chacha`,
`getrandom` and `tokio`, all normal `[dependencies]`. `cargo test` builds
examples, so a break here reddens the test gate, not just the build gate.
Target **≤ 90 lines including comments**. It must actually run:
`cargo run --example echo` prints the received message and exits 0.

README links it from the Quickstart section (§2); `lib.rs`'s quickstart names
it in prose (§4.3, note (a): plain text or an absolute URL, never a bare
bracket pair).

## 7. SVG plan

**Two SVGs, both README/GitHub-facing only.** rustdoc pages must not depend on
repo-relative images — they do not render on docs.rs — so `lib.rs` keeps its
ASCII `# Shape` block untouched (§4.5) and neither SVG is referenced from any
`//!` or `///`. Both live under `docs/`, which **does not exist at `721167a`**;
W1 creates it. They are hand-written SVG (no build step), with `viewBox` set and
no fixed pixel width, so GitHub scales them.

**Theme — a hard constraint, not a preference.** GitHub renders READMEs in
light *and* dark, and an SVG referenced through `![...](...)` is loaded as an
`<img>`, so it inherits nothing from the page: `currentColor` and
`prefers-color-scheme` are both unavailable. Use explicit mid-tone values that
read on either ground (strokes `#5b8def`, text `#7d8590`, no white or black
fills, no background rect). A black-on-transparent diagram is invisible in dark
mode, which is how most of these ship broken.

### 7.1 `docs/architecture.svg` — adopted

Referenced from README `## How it works`. Four stacked bands, top to bottom,
with the arrow direction and every label exact:

| Band | Box text | Side label |
|---|---|---|
| 1 | `core::Endpoint` · `core::Connection` | `pure state machines — no I/O, no clock` |
| 2 | `shell: one !Send driver task` | `owns the cores, owns the Wire` |
| 3 | `Endpoint · Connection · BiStream · …` (handles) | `your code holds these` |
| 4 | `compat: AsyncRead/Write · Stream/Sink · Codec · tower` | `optional, feature-gated` |

Two arrows on the left, both pointing **up**, labelled `poll_output() to
Timeout` (band 1←2) and `handles` (band 2←3). One box to the right of band 2
labelled `Wire` with a bidirectional arrow to a small `UDP socket` box, and a
caption under it: `or FlakyWire, in memory, on a paused clock`. No constants,
no timer values, no spec section numbers.

### 7.2 `docs/staged-accept.svg` — adopted

Referenced from README `## Quickstart`. It carries the one capability B
identified as slither's best unmade argument, and it is the thing A found
clearest once discovered. A left-to-right ladder:

```
Intro  ──read_identity()──▶  Claimed  ──authenticate()──▶  Proven  ──accept()──▶  Connection
0 DH        +1 DH             1 DH          +1 DH           2 DH       +2 DH
```

Under each of `Intro`, `Claimed`, `Proven`, one caption line, verbatim:

- `Intro` — *"a peer is offering. Nothing is proven."*
- `Claimed` — *"it says who it is. Still unproven — do not punish on this."*
- `Proven` — *"the DH proved possession. Now decide."*

Plus one downward arrow from any rung labelled **`drop = silent reject`**. The
DH counts are structural, not ratified wire constants, so this diagram creates
no constants-drift surface.

### 7.3 Rejected: a connection lifecycle / timers diagram

Two reasons. *(a)* Its content is the 5 s / 10 s / 25 s / 90 s timers, which are
**ratified constants** — and a value baked into an SVG path is the one place in
this repo that no test pins and no `grep` for the constant's name will find. The
brief's rule that every quoted constant must match `constants.rs` is
unenforceable inside a picture. *(b)* Obligation 3 already explains the death
clock better in prose than a diagram would, and README's `## Limits` now states
the 25 s rule in one line. Three diagrams also crosses from "aids
understanding" into decoration, which the maintainer's simplicity directive
weighs against.

## 8. The five reserved maintainer decisions

Each is stated as: **recommendation**, then the live alternative, then the fact
that decides it. Nothing in §§2–7 depends on these except where noted.

### (i) The shell's submodule docs — `pub mod` vs. folding the prose

**Recommendation: fold, do not publish.** Move `staged.rs`'s DH-annotated
ladder and the opening paragraphs of `endpoint.rs`, `connection.rs` and
`stream.rs` into `src/shell/mod.rs`'s already-public `//!`, and leave the six
`mod` declarations private. C's finding is right — that prose is invisible
today — but the fold is *strictly better navigationally*: it lands the ladder
at **0 clicks** from the crate root's `shell` link, where `pub mod` lands it at
**2**, and it costs no API surface. `pub mod staged;` would freeze
`slither::shell::staged` as a semver-visible path and give `Intro` a **third**
rendered address (`slither::Intro`, `slither::shell::Intro`, and now
`slither::shell::staged::Intro`), which rustdoc shows as duplicate pages and
which a future refactor could no longer move.

**The live alternative:** make `staged`, `endpoint`, `connection` and `stream`
`pub mod` (leaving `driver` and `shared` private, as C proposes). It preserves
the prose exactly where its author put it and needs no rewriting — cheaper for
the writer, more expensive forever after. If the maintainer wants module pages
for their own sake, this is the option; if the goal is that a reader *sees* the
ladder, the fold wins.

*Impact if the alternative is taken:* W3's §5 row for `shell/mod.rs` shrinks to
the first-sentence rewrite, and four files gain a `pub` keyword. No other
section of this plan changes.

### (ii) A slither-authored LocalSet panic guard in `EndpointBuilder::build()`

**Recommendation: adopt the code change, wrapping the existing call site.**
A verified the failure by experiment and it is the round's identified give-up
moment: `#[tokio::main]` plus `Endpoint::builder()…build()` panics with

```
`spawn_local` called from outside of a `task::LocalSet` or `runtime::LocalRuntime`
```

which names tokio, names no slither symbol, and gives no reader a reason to
open slither's docs. **The site is cheaper than A thought** (§0 conflict 6):
`tokio::task::spawn_local(...)` is called **directly in `build()`**, at
`src/shell/endpoint.rs:524`, in the function whose own doc at `:394` already
documents this panic. The change is to wrap that one line in
`std::panic::catch_unwind(AssertUnwindSafe(...))` and, on `Err`, re-panic with
slither's own message naming `slither::block_on` and `tokio::task::LocalSet`.
It is safe code, so `#![forbid(unsafe_code)]` is untouched.

**Two caveats I checked rather than assumed** (rule 11). *(a)* There is **no
public tokio predicate for "am I inside a `LocalSet`"**. I considered
`Handle::current().runtime_flavor()` and rejected it: a `LocalSet` runs
perfectly well on a multi-thread runtime via `run_until`, so a flavour check
would **falsely panic on a valid configuration**. `catch_unwind` is the only
mechanism that answers the actual question. *(b)* Under `panic = "abort"`,
`catch_unwind` does not catch, and the consumer sees today's message — no
worse than the status quo, and worth one sentence in the guard's doc.

**The live alternative:** docs-only (A's fix 3a). §2's `## Requirements` and
§4.3's quickstart comment already say "not `#[tokio::main]`" and already name
the panic, and they ship **regardless of this decision**. The argument for
docs-only is that a docs round should not change code, and that the panic is
documented on `build()` today. The argument against is A's evidence that the
developer who hits it has, by construction, not read those docs.

### (iii) Naming iroh / quinn / snow in README's when-not-to

**Recommendation: name them.** B's §2.3 shows the ranker eliminating slither on
*unknowns*, and an anonymous scope statement ("if you need NAT traversal, use a
relay-capable stack") leaves the reader with a category and no next step — it
converts an unknown into a smaller unknown. Naming three crates converts it
into a decision. B's stronger point is that a crate which correctly scopes
itself against named alternatives reads as *confident*, and this is the block
that has to beat iroh on a query slither cannot win on substance.

**The live alternative:** anonymous scoping. Its case is maintenance — three
crate names are three claims about other people's software, and B itself flags
that as the highest-drift content in the report (which is why §1.2 rejects the
full comparison table, P9). The mitigation is already in the draft: each bullet
names a *capability* slither does not have, not a competitor's quality, so
nothing goes stale unless slither itself changes.

### (iv) The "not independently audited" line

**Recommendation: keep it.** It is five words and B's evidence is direct: snow
clears the trust gate *by admitting* it is unaudited, and slither currently
scores below snow by declining to answer. A careful reader assumes unaudited
anyway — silence buys nothing and additionally records that the crate ducked
the question. Saying it also makes the rest of the status block believable: a
line that volunteers the worst fact is read as a line that is not selling.

**The live alternative:** silence, on the view that a `0.2.0` crypto crate
should not draw attention to the gap. I think this is the weaker read, and it
is also the only item in the status block that is a *judgement* rather than a
fact — which is exactly why it is reserved rather than decided.

### (v) The `Cargo.toml` description rewrite

**Recommendation: adopt.** Final text in §3, **187 chars** against today's 204.
B's §2.5 is the strongest single argument in any of the three reports: the
crates.io search-results pass sees ~40 tokens — name, description, version,
downloads — and eliminates before any page is fetched, so this string has more
leverage per character than anything in the README. Today's version names
WireGuard, Noise and QUIC before it names a capability.

**The live alternative:** leave it. It is published metadata, it is accurate,
and it is *lineage-precise* in a way the replacement is not — "carrying a
QUIC-shaped reliable frame layer" says something exact that "QUIC-shaped
frames" only gestures at. If the maintainer values the precision over the
retrieval, this is a defensible hold; the README's opening sentence then does
the whole job alone, one click later than it should.

## 9. Writer partition (rule 6 — disjoint paths)

Four writers plus an integrator. **No path appears twice.** Every writer's
first act is `git log -1 --oneline` and reporting its base (rule 14).

| Writer | Owns, exclusively | Depends on |
|---|---|---|
| **W1** | `README.md`, `docs/architecture.svg`, `docs/staged-accept.svg` (creates `docs/`) | §2 verbatim, §7. Independent of everyone. |
| **W2** | `src/lib.rs` | §4. Needs §8(ii)'s verdict **only** if the guard's doc lands here — it does not; the guard's doc is W3's (`endpoint.rs`). |
| **W3** | `src/identity.rs`, `src/shell/mod.rs`, `src/shell/connection.rs`, `src/shell/endpoint.rs`, `src/shell/staged.rs`, `src/shell/stream.rs`, `src/packet/suite.rs` | §5, §8(i), §8(ii). |
| **W4** | `examples/echo.rs` (new file) | §6. Runs **first**; W2's quickstart is derived from its result. |
| **Integrator** | `Cargo.toml` | §3, §8(v). Also owns the final commit and every gate in §10. |

Notes that make the partition hold:

- **W3 is one writer, not three**, because §8(i)'s fold moves text *between*
  `staged.rs`/`endpoint.rs`/`connection.rs`/`stream.rs` and `shell/mod.rs`.
  Splitting those files across agents puts one edit on two paths — the slice-2a
  failure exactly. If W3 is too large, split it as
  **W3a = `src/identity.rs` + `src/packet/suite.rs`** and
  **W3b = the five `src/shell/*.rs` files**; that cut is clean because no §5 row
  spans it.
- **`Cargo.toml` is the integrator's** (rule 15): it is the file whose contents
  are only valid once the decisions in §8 are known, and W1's README quotes the
  dependency block it contains. No writer touches it.
- **Ordering:** W4 → (W1 ∥ W2 ∥ W3) → integrator. W4 first because its file is
  the only artefact that is *executed*, and §4.3's doctest and §2's snippet are
  both derived from it; if W4 finds A's shape needs an adjustment, it is cheaper
  to learn before two writers have transcribed it.
- **W1 needs no source access** and should be given none: it writes the WHY.

## 10. Verification plan

### 10.1 Gates — the full nine-gate table on the final commit

Nothing here is new; the round adds doctests and an example, and every one of
them is already covered by an existing gate. Paste the command and its output
(rule 7):

| Gate | Command |
|---|---|
| Compiles | `cargo build --all-features --all-targets` |
| Format | `cargo fmt --all --check` |
| Lints | `cargo clippy --all-features --all-targets -- -D warnings` |
| Docs | `RUSTDOCFLAGS=-D warnings cargo doc --no-deps` **and** `RUSTDOCFLAGS=-D warnings cargo doc --no-deps --all-features` |
| Tests | `cargo test` **and** `cargo test --all-features` |
| Release tests | `cargo test --release --all-features` |
| Wire pins | golden-wire + size/constant tests (inside `cargo test`) |
| MSRV | `cargo +1.96 check --all-features --all-targets` |
| Supply chain | `cargo deny check` |

### 10.2 The two checks specific to this round

**(a) Doctests must compile featureless.** This is the constraint most likely to
be violated by a writer reaching for `testutil`:

```
cargo test --doc                 # featureless — MUST pass
cargo test --doc --all-features  # MUST pass
```

Baseline at `721167a`: **14 doctests, 0 failures** (C ran it). After this round
expect **19–20** — `+1` crate-root quickstart, `+1` `send_message`, `+2`
`identity.rs` (`generate`, `public_static`), `+1` `Endpoint::connect`, and the
crate-root `no_run` at `lib.rs:61` unchanged. A count that comes back at 14 or
15 means a fence was written as ` ```text ` by accident — check before
celebrating a green.

**(b) The example must run, not merely build.**

```
cargo build --all-targets        # examples compile featureless
cargo run --example echo         # prints the message, exits 0
```

### 10.3 Link check

`RUSTDOCFLAGS=-D warnings` in the Docs gate covers broken intra-doc links, and
the rename in §4.6 is the specific risk. Two additional greps, because rustdoc
cannot see markdown links in `README.md` at all:

```
grep -rn "crate#\|#the-six" src/                       # → must be empty
grep -o '](\([^)h][^)]*\))' README.md                  # every repo-relative target must exist
```
At `721167a` the first grep is already empty, so a hit after the round means
the writer added an anchor link to a heading that no longer exists.

### 10.4 Staleness greps — these strings must NOT survive

```
grep -rn "There is no \`examples/\` directory" README.md          # → empty
grep -rn "my_static_scalar\|my_rng\|my_allow_list" README.md      # → empty
grep -rn "2026/07/16\|2026/07/17" README.md                       # → empty (stale Leg-1/Leg-2 dates)
grep -rn "Leg 2, ratified" README.md                              # → empty (unglossed jargon, B §1.2a)
grep -rn "rust,ignore" README.md                                  # → empty
grep -rn "The six documentation obligations" src/                 # → empty (renamed)
grep -rn "Slice by slice it grows" src/lib.rs                     # → empty (stale shell bullet)
grep -rn "until the driver that can drive them exists" src/lib.rs #  → the //! bullet must be gone;
                                                                  #    the line-242 code comment is flagged, not fixed (§4.8)
grep -n 'hiss = "0.3"$' src/lib.rs src/packet/suite.rs            # → empty (default-features = false)
```

### 10.5 Constant conformance

Every number quoted in §2 and §4 must equal `src/constants.rs`. One command,
run by the integrator before committing:

```
grep -n "MESSAGE_RECV_MAX\|MAX_DATAGRAM_PAYLOAD\|pub const MAX_DATAGRAM\b" src/constants.rs
```
Expect `262_144`, `1169`, `1200`. The README writes them as `262 144 B`,
`1 169 B` and `1 200 B` — thin-space grouping only; **no writer may round, and
none of these appears inside an SVG** (§7.3).

### 10.6 The test-count claim

Re-run before committing, because it is the one README number that drifts:

```
grep -rho '#\[\(tokio::\)\?test\b' src tests --include='*.rs' | wc -l
```
It must remain **≥ 1 100** for the README's "1 100+ tests" to hold. If it ever
falls below, the claim changes; it does not get rounded up.

## 11. Objections to this brief (hard rule 4)

Three, none of which blocked the work.

1. **"Target ≤ ~110 lines" for the README fights the brief's own content
   list.** The brief names ten blocks the README must implement; ten headings
   and their blank lines are ~35 lines before a word is written. I landed at
   **133 lines / 865 words** and explain the deviation in §2 rather than
   trimming to the number, because the cheapest remaining cuts are either the
   Quickstart fence (which is where a skimmer meets `channel!`, `generate()`
   and `public_static()` — three of A's five stumbles at a glance) or the
   `## Status` / `## License` headings (which are retrieval keys, per B's
   central thesis). Against the maintainer's actual directive — *"not too many
   words"* — the draft is **+8 % words** over today's README while adding six
   blocks and deleting two.

2. **One "verified fact" in the brief is imprecise, and it would have shipped
   a wrong sentence had I quoted it.** The brief states: *"reliable message ≤
   262,144 B (MESSAGE_RECV_MAX, 256 KiB; **exceeding it is the MESSAGE_OVERFLOW
   error**)"*. Checked: `MESSAGE_OVERFLOW` is `0x06`, a **connection-close code**
   in §15.3's transport-owned registry (`src/constants.rs:591-592`), sent by a
   *receiver* about a *peer's* oversized message. What a caller gets back from
   `send_message` is **`MessageError::TooLarge`** (`src/error.rs:364-368`). Two
   different mechanisms on two different sides. The size — 262 144 — is
   correct, and neither §2 nor §4 names either error, so nothing wrong shipped;
   but a writer told "exceeding it is the MESSAGE_OVERFLOW error" would
   reasonably have written it that way. **Writers must not treat that clause as
   verified.**

3. **The brief's writer partition would have put one edit on two paths.** The
   proposed W3 is "`src/identity.rs` + `src/shell/*.rs` doc-comment edits", and
   §8(i)'s fold moves text *between* `shell/staged.rs` and `shell/mod.rs`. That
   is fine as long as W3 stays one agent — which §9 pins explicitly, along with
   the only clean way to split it if it is too large. The brief said "adjust as
   you see fit"; this is the adjustment, called out because "W3 owns
   `src/shell/*.rs`" reads as splittable and is not.

**One thing the brief got exactly right and is worth recording:** telling me A's
demo *compiled and ran* and to treat its `main.rs` as the proven template. Every
end-to-end artefact in this plan — §4.3's doctest, §6's example, §2's snippet —
is a projection of one program that has actually executed, rather than three
independently imagined ones. That is the fix for the failure mode all three
finders found: three documents each showing a different subset of the path,
none of them a whole path.
