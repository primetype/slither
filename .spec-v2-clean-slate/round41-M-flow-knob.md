# Round 41, report M — the Config flow knob (ruling 259(viii)) + the core comment sweep

*Agent ζ (Opus), worktree cut from `8c29808` (verified as its first act);
its commit `a8a8380` cherry-picked to main as `2c93102`; integration
follow-ups (`WindowError` → `ConfigError` fold, `mint_conn_seed` rename)
at `bb32603`. Report near-verbatim; the two "integrator must" items at
the end were both taken as recommended.*

## The knob

`Config::with_flow_windows(stream: u64, connection: u64) -> Result<Self, WindowError>`,
plus `stream_window()` / `connection_window()` accessors and
`DEFAULT_STREAM_WINDOW` / `DEFAULT_CONNECTION_WINDOW` associated consts
(ruling 82's shape). One atomic method rather than two builders: the
pair has an ordering invariant (`stream ≤ connection`), and two consumed
builders make its validation order-dependent. Rejections: `TooSmall`
(< the ratified default — refused, not clamped), `TooLarge`
(> `VarInt::MAX_VALUE`, the only ceiling an existing invariant demands),
`StreamAboveConnection`. Threading: `Config` → `Endpoint::draw_sub_seed`
→ a new `ConnSeed { sub_seed, windows }` →
`Connection::{connecting, established}` → `Flow::with_window` /
`Streams::with_window` → `RecvHalf::with_window`. **`SendHalf::max_data`
and `Flow::send_max_data` stay at the constants** — those are the
*peer's* un-negotiated limits and no local config speaks for them.

**The non-obvious half:** §10.2's initial windows are never sent, so a
peer assumes the constants, and `take_grant()` only fires on application
consumption. A raise stored but not *said* is invisible. So
`CreditWindow` gained a one-shot `pending_announce`: MAX_DATA is owed at
`connecting()`, and MAX_STREAM_DATA is owed **on the peer's first STREAM
frame, not at open** — `pack_control` runs before the STREAM fill, so a
grant owed at open leaves ahead of the frame that names the stream and
§8.4 drops it as inert.

## The `MESSAGE_RECV_MAX` corner — the knob does not raise it

`constants.rs:609`'s `MESSAGE_RECV_MAX == INITIAL_MAX_STREAM_DATA` is
**untouched and still true**, because the knob does not move
`INITIAL_MAX_STREAM_DATA`. §9.8's bound is checked on the **send** side
(`send_message`, and the shell handle), and a sender cannot know what
its receiver configured — a locally raised message bound emits a payload
a default peer resets with `MESSAGE_OVERFLOW`. §9.8's overflow
*predicate* is a loudness rule, and ruling 153's FIN-rides-the-last-frame
guarantee keeps a conforming message clear of it at any window. Pinned
by `the_message_bound_does_not_move_with_the_window`.

## Red-on-default evidence (rule 9), three mutants run

| Mutant | Result |
|---|---|
| `draw_sub_seed` uses `FlowWindows::default()` (knob ignored) | stream test **262144 vs 524288**, connection test **1048576 vs 2097152** — red |
| `pending_announce` forced `false` (widened ledger, never announced) | same two red, same numbers — the announcement is load-bearing |
| announce at open instead of on the peer's first frame | **only** the bidi test red (262144 vs 524288), uni tests green — isolates exactly the claim its rustdoc makes |

`tests/story_flow.rs` (new, 6 tests): defaults-on-the-wire pin, the
raise measured on **both** birth paths (b accepted, a dialled), the
connection level, the locally-opened bidi half, the message bound, and
the validation refusals from outside the crate.

**Wire pins:** the golden vectors are handshake-layer only (headers,
mac1, msg1 payload, prologue, sizes) — `grep MAX_DATA
src/packet/golden_vectors.rs` finds only the unrelated `MAX_DATAGRAM`.
Nothing embeds an advertisement; 12/12 golden tests and 112/112
`spec_constants` green. Nothing to flag.

## Two things ζ did NOT do, and why (working rule 5) — both taken at integration

1. **`WindowError` lived in `src/config.rs`, not `error::ConfigError`.**
   Its three variants belong beside the keepalive variants, and the crate
   root should re-export it beside `Config` — but `src/error.rs` and
   `src/lib.rs` were outside ζ's partition. *Integration (`bb32603`):
   folded into `ConfigError` as `WindowTooSmall` / `WindowTooLarge` /
   `StreamWindowAboveConnection`; `spec_errors.rs`'s exhaustiveness
   fence extended deliberately.*
2. **`Endpoint::draw_sub_seed` returned `ConnSeed` while keeping its
   name.** `Connection::established` is constructed at `staged.rs:797`,
   which ζ's partition forbade, and `sub_seed` was the only value flowing
   into that call whose *type* ζ controlled — so the windows ride it and
   staged.rs was textually unchanged. The design argument stands on its
   own (one mint point, so the two birth paths cannot disagree), but the
   method name became a half-truth. *Integration (`bb32603`): renamed
   `mint_conn_seed`.*

## Comment sweep (ruling 264(vi)), core half, all dated `[corrected 2026/08/18 — ruling 264]`

- `src/core/endpoint/mod.rs:26–41` — proven-LIVE. Re-verified stale at
  base: `staged.rs:666` `Some(t) if timestamp > t => replacing =
  Some(live)`, `:748` `retire_replaced`, `:753`
  `EndpointOutput::Replaced`, `:691` `EndpointOutput::Contested`. The
  section heading announced a slice boundary that no longer exists;
  rewritten.
- `src/core/endpoint/mod.rs:246–250` — **same root cause, not on the
  sweep's list.** `replacement_basis`'s doc said §6.4's admission "is
  still a later slice, so the field is written and never checked". It is
  checked. Corrected.
- `src/core/connection/mod.rs:3538–3543` (`ConnOutput`) — counted the
  enum: **14 variants**, matching ruling 259(i). The "only `Established`
  and `Closed`" claim replaced with 259(i)'s counting rule.
- `src/core/connection/testfix.rs:182–186` — the ACK-panic claim,
  refuted by the function's own `FRAME_ACK` arm 70 lines below.
- `Cargo.toml:172` — the `tower` comment described the shape ruling 225
  rejected; verified against `src/compat/tower.rs:1–19`.
- `recovery.rs` `path_gen` prose — re-verified **NOT stale** at base
  (`recovery.rs:219–224`, `324–332`, `645–653` all live). G-doc-sweep's
  negative finding is correct; nothing changed.
- Unverified candidates deliberately left alone (not in 14(d)'s list,
  same defect shape): `frame.rs:82` and `:814`, `session.rs:460–462`,
  `connection/mod.rs:3230`.

## Gates, all green on `a8a8380` (ζ's worktree; re-run on main at integration)

build / fmt / clippy `-D warnings` / docs both feature sets / `cargo
test` (754+112+11+4+11) / `--all-features` (1089) / `--release
--all-features` (1091) / wire pins byte-identical (112/112 + 12/12
golden) / `+1.96 check` / `cargo deny check` — all green. Test count
1083 → 1089 (`story_flow`'s 6); `src/config.rs` gained 5 unit tests
inside the lib count.
