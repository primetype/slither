# Round 44 — writer W1 report

**Writer:** W1 (README rewrite + two SVG diagrams)
**Base commit (first command, verified):** `721167a` — "Round 43: the SECV5 pins land — the one-shot keepalive, the spent-packet separator, the 272 pin"
**Date:** 2026/08/19
**Exclusive paths:** `README.md`, `docs/architecture.svg`, `docs/staged-accept.svg`, this report.

## 1. Sources read

_(appended live, in order)_

- `.spec-v2-clean-slate/round44-D-fix-plan.md` §2 (lines 213–369, the full draft),
  §2.1 (370–397, provenance), §7 + §7.1 + §7.2 + §7.3 (723–790), §9 (903–933,
  the writer partition), §10.3–§10.4 (978–1005, the checks I owe).
- `src/constants.rs` — greps only, for §5 below. No other source file was read
  except one grep of `src/lib.rs`/`Cargo.toml` for `include_str!` (see D2).
- Current `README.md` before the rewrite: 120 lines, 803 words.

## 2. Deviations from §2's draft

The draft was transcribed **verbatim** except for the following. Nothing else
in §2's text was altered — the badge block with its `ACTIVATES ON PUBLISH`
comment, the `not independently audited` line and the named competitors
(iroh / quinn / snow) are all present as ratified today.

**D1 — ordered correction 1 (Quickstart, `examples/echo.rs` sentence).**
The draft's *"It also opens [docs.rs](https://docs.rs/slither), compiled every
run."* is garbled (it reads as though the example opens a web page). Replaced
with the fact it was reaching for: **"It is compiled by every `cargo test`
run."** — `cargo test` builds `examples/` under the test profile, so the
snippet cannot rot silently. One word longer than the draft (866 vs 865 words).

**D2 — ordered correction 2 (fence tag).** The Quickstart fence is kept as a
display sketch, tagged plain ```` ```rust ```` — no `,ignore`, so §10.4's
`rust,ignore` grep finds nothing. **Checked that this is safe before writing
it:** a plain `rust` fence is only compiled if the README is pulled into
rustdoc, and it is not — `grep -n 'include_str!' src/lib.rs Cargo.toml`
returns nothing (`Cargo.toml:12` sets `readme = "README.md"`, which is
crates.io metadata only). So the fence is display-only and no doctest gate
can see it.

**D3 — one element added to `docs/architecture.svg`, not in §7.1.** §7.1 places
a `Wire` box "to the right of band 2" but names no connector between them, so
as specified the box floats unattached. I drew a **plain hairline from band 2's
right edge to the `Wire` box — no arrowhead, no label, no new text** — rather
than ship a disconnected box. Declared here because §7.1's element list is read
as exhaustive (rule 8); it is trivially removable (one `<line>`).

**No other deviation.** In particular I did **not** "fix" §7.2's `+2 DH` under
`accept()`, which looks like an arithmetic slip against `0 → +1 → 1 → +1 → 2`
but is correct: Noise IK spends four DHs — `es` to decrypt the static
(`read_identity()`), `ss` to open msg1's payload (`authenticate()`), then `ee`
and `se` to write msg2 (`accept()`). That also matches §2's own prose, "before
the second Diffie-Hellman is spent" = before `ss`. Rendered exactly as §7.2
writes it.

## 3. Verification — link targets

`grep -o '](\([^)h][^)]*\))' README.md` → every repo-relative target, `ls`-checked:

```
OK      CHANGELOG.md
OK      docs/architecture.svg
OK      docs/staged-accept.svg
OK      examples/echo.rs
OK      LICENSE-APACHE
OK      LICENSE-MIT
OK      SECURITY.md
OK      SPEC.md
```

**All eight exist — no "expected from W4" row is needed.** Worth recording
precisely: `examples/echo.rs` did **not** exist when I began (an early `ls
examples/` showed only `audit_udp.rs` and `bench_vs_tcp.rs`); W4 landed it
while I was writing. The link is live as of this check, not merely promised.

The absolute (`https://…`) targets — crates.io iroh/quinn/snow/hiss/slither,
docs.rs, the two badge images and the Actions URL — are not `ls`-checkable and
are unchanged from §2's draft; the GitHub ones sit inside the
`ACTIVATES ON PUBLISH` comment (README.md:3) and are therefore inert.

No anchor links of the `crate#…` / `#the-six…` shape were introduced (§10.3's
first grep is about `src/`, which is not mine, but the README has no `#`
fragment link at all).

## 4. Verification — staleness greps

Each run as `grep -n -- "<pattern>" README.md`; **all eight empty**:

```
There is no `examples/` directory        -> 0 hits
my_static_scalar                         -> 0 hits
my_rng                                   -> 0 hits
my_allow_list                            -> 0 hits
2026/07/16                               -> 0 hits
2026/07/17                               -> 0 hits
Leg 2, ratified                          -> 0 hits
rust,ignore                              -> 0 hits
```

The §10.4 combined forms exit 1 (empty) as well:
`grep -n "my_static_scalar\|my_rng\|my_allow_list" README.md` → exit 1;
`grep -n "2026/07/16\|2026/07/17" README.md` → exit 1.

The four §2.1 deletions are therefore all discharged: `## Lineage`,
`## The protocol in one paragraph`, the `rust,ignore` sketch, the
"no `examples/` directory" sentence and the Leg-1/Leg-2 dates are gone; the
only date the file now carries is the ratification date **2026/08/14**
(README.md:126), which is ruling 242's.

Fence tags after the rewrite — exactly two fences, neither `ignore`:

```
35:```toml
44:```
65:```rust
76:```
```

## 5. Verification — Limits numbers vs `src/constants.rs`

| `src/constants.rs` | README |
|---|---|
| `312:pub const MESSAGE_RECV_MAX: u64 = 262_144;` | `88:- **A reliable message is at most 262 144 B** (256 KiB); an unreliable datagram` |
| `364:pub const MAX_DATAGRAM_PAYLOAD: usize = 1169;` | `89:  payload at most **1 169 B**. Every wire datagram is **≤ 1 200 B**, never` |
| `138:pub const MAX_DATAGRAM: usize = 1200;` | (same line 89) |

262 144 = 256 KiB ✓. The three README figures are the three constants,
digit-for-digit, with thin-space grouping only.

Two further numeric claims, checked while I was there:

- "**1 100+ tests**" (README.md:9) — §2.1's own command,
  `grep -rho '#\[\(tokio::\)\?test\b' src tests --include='*.rs' | wc -l`
  → **1147** at `721167a`. The claim understates, which is the safe direction.
- "25 s" (README.md:91) and "5 s / 10 s / 25 s / 90 s" (README.md:117) are
  §2.1's `DEAD_TIMEOUT` / `RETRANSMIT_BASE` / `KEEPALIVE_TIMEOUT` /
  `HANDSHAKE_GIVEUP` row; `src/constants.rs:618` reads
  `const DEAD_TIMEOUT_MS: u64 = 25_000;` ✓.

## 6. Verification — SVG well-formedness

```
$ xmllint --noout docs/*.svg
xmllint: OK, both
```

(exit 0, no output; the "OK, both" line is my `&& echo`.)

Content guard, dumping every text node in both files:

- `grep -c '§' docs/*.svg` → `0` and `0` — no spec section numbers.
- The 34 text nodes are exactly §7.1's and §7.2's labels; the only numerals
  anywhere in either file's *text* are `0 DH`, `+1 DH`, `1 DH`, `+1 DH`,
  `2 DH`, `+2 DH` — §7.2's structural DH counts, which it explicitly rules
  create no constants-drift surface. **No ratified constant and no timer value
  appears in either SVG.**

Theme and portability constraints, each satisfied by construction:

- no `<rect>` covering the canvas — no background rect; the ground is
  transparent.
- every stroke and every marker fill is `#5b8def`; every text fill is
  `#7d8590`; box fills are `#5b8def` at `fill-opacity="0.08"`. **No `#fff`, no
  `#000`, no `white`, no `black`, no `currentColor`, no
  `prefers-color-scheme`** — nothing that depends on the host page.
- `viewBox` set on both, **no `width`/`height` attribute** on either root.
- `font-family="system-ui, -apple-system, Segoe UI, Roboto, sans-serif"` — a
  stack only; no `@font-face`, no external font, no `<script>`, no `<style>`,
  no CSS class.
- 2 851 B and 2 596 B.

**One caveat for the integrator, not a deviation.** The no-fixed-pixel-width
rule is binding and I followed it, but a `viewBox`-only SVG loaded through
markdown `![…](…)` has an intrinsic *ratio* and no intrinsic *size*, so
browsers fall back to the 300 px default object width — both diagrams will
render ~300 px wide on GitHub rather than filling the column. The fix, if the
maintainer wants one, is a `width`/`height` pair on the SVG root or an
`<img width="…">` in the README; both contradict a constraint I was given, so
I did neither and flag it here instead.

## 7. Render sanity (one line per SVG)

- **`docs/architecture.svg`** — four stacked bands, top to bottom in §7.1's
  table order (1 `core::Endpoint · core::Connection`, 2 `shell: one !Send
  driver task`, 3 `Endpoint · Connection · BiStream · …`, 4 `compat:
  AsyncRead/Write · Stream/Sink · Codec · tower`), each with its side label in
  the right margin; two arrows in the left margin both pointing **up**,
  `poll_output() to Timeout` from band 2 to band 1 and `handles` from band 3 to
  band 2; right of band 2 a `Wire` box (joined to the band by the plain hairline
  of D3) with a **bidirectional** vertical arrow down to a smaller `UDP socket`
  box, captioned `or FlakyWire, in memory, / on a paused clock` on two lines.
- **`docs/staged-accept.svg`** — one left-to-right ladder in §7.2's order,
  `Intro` →`read_identity()`→ `Claimed` →`authenticate()`→ `Proven`
  →`accept()`→ `Connection`, verb labels above each arrow and the DH row
  beneath it (`0 DH`, `+1 DH`, `1 DH`, `+1 DH`, `2 DH`, `+2 DH`, aligned rung /
  arrow / rung / arrow / rung / arrow as in §7.2's ASCII); the three captions
  sit under `Intro`, `Claimed` and `Proven` verbatim (wrapped to 2 / 3 / 2
  lines to stay inside their columns — line breaks only, no word changed); one
  elbowed arrow leaves the `Claimed` rung downward into the empty channel
  between the first two caption blocks and is labelled `drop = silent reject`.

## 8. Final counts

| File | Lines | Words | Bytes |
|---|---|---|---|
| `README.md` | **133** | **866** | 6 414 |
| `docs/architecture.svg` | 53 | 279 | 2 851 |
| `docs/staged-accept.svg` | 54 | 247 | 2 596 |

`README.md` before this round: 120 lines, 803 words. §2 predicted 133 lines /
865 words; the one-word difference is D1. `git status` over my paths only:
`M README.md`, `?? docs/`. **No commit, no `git add`, no cargo command, and no
file outside my four paths was written.**
