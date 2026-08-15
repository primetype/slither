# FIXES-3b round 2 — post-slice-3b seam review, second pass

Baseline: HEAD `9a26c15` on `main`, tree clean, 454 tests green
(`--all-features`), 420 bare.

Scope: the four findings confirmed real and deliberately left at
`9a26c15`. `src/core/**` is frozen; `SPEC.md`, `PLAN.md`, `STORIES.md`,
`rulings.md` are not to be touched.

---

## F5 — the linger test pins nothing

### Status

_pending_

### The test as found

_pending_

### The mutation (executed, before)

_pending_

### What observably differs

_pending_

### The rewrite

_pending_

### The mutation (executed, after)

_pending_

---

## F8 — `command_cancel` releases bookkeeping before delivering `Retired`

### Status

_pending_

### Verification

_pending_

### The fix

_pending_

---

## F9 — `serve()`'s outer drain loop exhausts silently

### Status

_pending_

### The fix

_pending_

---

## F10 — wakers invoked under a live `RefCell` borrow

### Status

_pending_

### Reachability analysis

_pending_

### The fix

_pending_

---

## Gates

_pending_

---

## Notes / conflicts found

_pending_
