# Round 44 — documentation round: first-hand pass

Goal (maintainer, 2026/08/19): simple to use, simple to onboard. README =
the *why*; lib.rs = the *how*, fast. Short paragraphs, many step-by-step
examples. Two audiences: a developer in a hurry, and an AI ranking
dependencies. SVGs allowed for architecture.

Tree at `721167a`, clean. Pass done first-hand (Fable) before the gap agents.

## Inventory

| Surface | State |
|---|---|
| README.md | 120 lines, why-dense, spec-flavoured prose |
| src/lib.rs | 335 lines: 6 obligations, module map, feature table |
| Cargo.toml | metadata complete: keywords, categories, docs.rs all-features |
| examples/ | 2 files — both *measurement harnesses*, no onboarding example |
| CHANGELOG.md | current (0.2.0, 2026-08-18) |
| Diagrams | one ASCII "Shape" block in lib.rs; no SVG anywhere |
| SECURITY.md | present |

## README.md as it stands

- **No on-ramp blocks**: no badges, no `cargo add slither`, no MSRV
  statement, no feature table, no "when to use / when not to use", no
  status-at-a-glance. The install path is entirely implicit.
- **One code block, `rust,ignore`** — never compile-tested anywhere. I
  verified it against the real API by hand today (builder chain,
  `Wire for tokio::net::UdpSocket`, `accept() -> Option<Intro>`): it *is*
  currently accurate, but nothing keeps it so.
- **Stale self-reference (rule-4 shape)**: "There is no `examples/`
  directory in the tree today" — there is, since the round-41/42 harnesses
  landed. The sentence even instructs its own repair ("this section should
  link it").
- **Status section stale**: cites Leg-1 (2026/07/16) and Leg-2
  (2026/07/17) ratifications but not the full-spec RATIFIED stamp
  (2026/08/14, ruling 242) or v0.2.0.
- Prose is accurate and rich but *long-paragraph*: "The protocol in one
  paragraph" is ~20 lines. Against the maintainer's brief (short
  paragraphs, steps), it reads as a spec abstract, not a pitch.

## lib.rs docs as they stand

- Front-loads governance: "SPEC.md is the authority" is paragraph 2. The
  first *code* a reader meets is obligation 1's **reconnect edge case** —
  an excellent doc in the wrong slot; there is no quickstart at all.
- The six obligations (~150 lines) are the bulk. They are **ruled
  content** (slice 9, §6.5 obligation 6, rulings 225/89/259(iii) RATIFIED
  blocks) — restructuring must keep them present and unweakened; they may
  move below a quickstart but not shrink.
- Only 2 compilable (`no_run`) doctests in the crate root; the rest of the
  fences are `text`. The golden path (build endpoint → connect → send →
  accept ladder) exists nowhere as a compile-tested example.
- `block_on` — the first thing a consumer actually needs (the `!Send` /
  `LocalSet` tax) — is documented ~line 250, found only by reading down.

## Cargo.toml metadata

Good: keywords (`noise, udp, wireguard, quic, datagram`), both categories,
`documentation`/`repository`/`homepage`, docs.rs `all-features = true`,
SPEC.md deliberately shipped in the package. Weak: `description` is one
40-word sentence — the first clause an AI or crates.io search surfaces is
"A WireGuard-shaped Noise-over-UDP packet layer carrying a QUIC-shaped
reliable frame layer", which names two *other* protocols before what it
does for you.

## Examples

`audit_udp.rs` and `bench_vs_tcp.rs` are measurement harnesses with long
investigative preambles — valuable, but a newcomer opening `examples/`
meets a latency-anomaly triage, not hello-world. There is no runnable
"two endpoints exchange a message" example.

## First-hand verdict before the gap agents run

Reference documentation: excellent — `#![warn(missing_docs)]` + the docs
gate hold every item documented, and the obligations are genuinely good.
**Onboarding documentation: nearly absent.** The 10-minute path (two
machines, encrypted messages) requires mentally executing an untested
sketch and discovering the LocalSet requirement by reading 250 lines in.
For an AI ranking dependencies, the skimmable blocks it extracts first
(install, minimal example, MSRV, maturity, feature matrix, comparisons)
are missing from README entirely — the metadata sentence is the only
compact signal.

Hypotheses for the gap agents to test, not conclusions: (a) the README
sketch compiles as-is once wrapped in a LocalSet main; (b) the fastest
stumble is the `!Send`/LocalSet requirement; (c) an AI reading the first
2000 tokens of README cannot answer "how do I install and send one
message".

## Gap-agent reports

- A (hurried dev, hands-on, Sonnet): `round44-A-hurried-dev.md`
- B (AI dependency-ranker, Opus): `round44-B-ai-ranker.md`
- C (docs.rs reference experience, Sonnet): `round44-C-docsrs.md`
- Fix decision (Opus): `round44-D-fix-plan.md`

## Finder outcomes (post-run, orchestrator's verification notes)

All three verified their base at `721167a` as their first act.

- **A (hurried dev, Sonnet)**: the demo COMPILED AND RAN FIRST TRY —
  every needed fact is documented *somewhere*; the failure is discovery
  order. 5 stumbles, ~30–50 min: `channel!` on no entry page (worst),
  `#[tokio::main]` reflex → bare tokio panic never naming slither
  (verified by experiment; the give-up moment), tokio features stated
  nowhere rendered, `public_static()` and `generate()` absent from README.
  Hypotheses (a) and (b) from this pass: confirmed.
- **B (AI ranker, Opus)**: slither ranks 5th of 6 on its own primary task
  (behind iroh, quinn, libp2p, snow) while being arguably the 2nd-best
  technical fit. Eliminated by the trust filter (0.2.0, no badges, no
  audit statement — silence loses to snow's candour) and unknown
  onboarding cost. The pre-fetch cull makes Cargo.toml `description` the
  highest-leverage string in the crate (0 tokens). Ranked fixes P1–P9;
  harmful list: llms.txt, obligation duplication, second feature table,
  benchmark claims. Hypothesis (c): confirmed, with the refinement that
  the ~40-token search-results pass eliminates before any page is read.
- **C (docs.rs audit, Sonnet)**: structural finding — six shell submodules
  are private `mod`, so their `//!` docs (incl. staged.rs's DH-annotated
  ladder) NEVER render on docs.rs (verified first-hand: shell/mod.rs:52-57).
  Zero examples on send_message/recv_message; zero end-to-end doctests in
  the rendered surface; 8/8 public module docs open reviewer-first; the
  auto-generated Modules table is alphabetical, so `compat` leads and
  `shell` is 7th of 8.

Orchestrator-supplied facts for the decider (verified against the tree):
message ≤ 262,144 B (`MESSAGE_RECV_MAX`); datagram payload ≤ 1,169 B
(`MAX_DATAGRAM_PAYLOAD`); wire datagrams ≤ 1,200 B, never fragmented;
tokio features `rt, net, time, sync, macros`; internal hiss dep is
`default-features = false`; CI = Linux + macOS only (B's draft status
block guessed Windows — a false claim caught before it shipped); no git
remote configured, so badges/raw-image links are "activates on publish".

## Integration close (2026/08/19)

Ruling 276 ratified all five reserved decisions as recommended (fold /
guard / named rivals / candour line / description rewrite). Four writers
on disjoint paths (W1 README+SVGs Opus, W2 lib.rs Opus, W3 source docs +
fold + guard Opus, W4 echo.rs Sonnet), W4 first so every example derives
from an executed artefact. Integrator items: `compat/rt.rs`'s panic
sentence (W3's D2, a no-owner file), the SVG intrinsic sizes (W1's
flagged contradiction — explicit width/height + viewBox is the mechanism
that actually scales on GitHub), the Cargo.toml description, the
CHANGELOG rows.

Content landed at `d039a28`; all nine gates green on it (923/0
featureless, 1136/0 all-features, 1138/0 release, 12+112 wire pins,
MSRV, deny). Doctests 11→16 featureless / 14→19 all-features.
`cargo run --example echo` round-trips and prints the key handover.

Corrections banked in the ruling: the false "no examples/" README line;
the ruling-264 sweep's two missed neighbours (AsyncRead/Write "absent")
— caught by W3, confirmed against compat/io.rs; B's Windows-CI guess;
the answerer-side acked()/PeerClosed race echo.rs surfaced; the
viewBox-only 300×150 default.

Held for later (recorded, not scheduled): P9's comparison table
(post-publish); D3's two-home overlap on identity.rs's RNG recipe;
lib.rs:342's code comment "until the driver lands" (flagged by W2,
internal-facing, harmless); badges + crates.io image rewriting activate
when the maintainer publishes the repo.
