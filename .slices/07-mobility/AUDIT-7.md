# AUDIT-7 — adversarial spec audit for slice 7 (mobility)

Base commit `b649575`. Auditor: adversarial spec auditor, blind to `PLAN-7.md`.

Target defect classes:
- **Class A** — a stated construction with an unstated or contradicted scope (working rule 8).
- **Class B** — prose vs. formal rule in conflict (working rule 3); report, do not resolve.
- **Class C** — a rationale that names a mechanism that does not exist in the code (working rule 11).
- **Class D** — a ruling's blast radius includes the ruling.

Status: COMPLETE. Twelve findings (F-A … F-L), ranked by consequence; a
"checked and clean" section and an explicit coverage/not-covered note at
the end.

**Nothing here is a resolution.** Every finding names the reading I
believe is the intent and marks it as mine. Per working rule 3, the
conflicts are reported, not picked.

---

## Findings (ranked by consequence)

### F-A. §13.6 "What resets when" is not what resets when — the section title promises an exhaustive seam list and omits every reset outside the congestion subsystem

**Lines: SPEC.md 3799–3820 (§13.6), against 3918–3920 (§7.3, `both counters resetting at each such address change`).**

§13.6 is titled **"What resets when — the roam seam"** and its body reads (3801–3815):

> Roaming is the **only** recovery seam: there is no rekey, and a connection's one session lives as long as the connection (§7.8).
> **Roaming** (§7.3): the sent map is **kept** … The congestion controller resets with the **pre-roam flight fenced off** (§14.6) … The RTT estimator is treated as suspect-but-kept, with `min_rtt` re-seeded from the first post-roam sample (§13.1).

Everything it names is recovery/congestion state. But §7.3 (1917–1920) mandates a *different* reset on the *same* seam:

> the address is **unvalidated** and a send-side budget arms — total bytes sent to the address MUST NOT exceed `AMPLIFICATION_FACTOR` (= 3) × total bytes **received from it and authenticated** on this session, **both counters resetting at each such address change**.

So the amplification counters reset on a roam and §13.6 does not list them. Working rule 8: a list is read as exhaustive whether or not it says so, and this list carries a *title* that says so. An implementer building slice 7's roam handler from §13.6 writes a `Connection::on_roam()` that touches the controller and the sent map and nothing else, and the amplification budget silently carries the old address's credit onto the new one — which is precisely the reflector §7.3 exists to prevent (old-address credit funds sends to a fresh attacker-supplied address).

The same omission covers, at minimum: the anti-replay window (§7.2 — see F-E), the contested-probe state (§7.5 — see F-C), the keepalive/dead timers, and `pto_count`. §13.6 says "loss detection and PTO continue undisturbed", which resolves PTO explicitly; it says nothing about the rest.

**My reading (not a resolution):** the intent is that §13.6 is the *congestion/recovery* reset seam and was written inside chapter 13's scope, but its title claims global scope. Either the title is narrowed ("what resets in loss recovery"), or §13.6 grows a cross-reference list of every per-connection reset on the roam seam. The second is safer: rule 8's whole point is that the omission is invisible until someone builds against it, and slice 7 is the someone.

**Consequence: high.** A missing amplification-counter reset is a security regression, not a behaviour difference.

### F-B. Ruling 137's amendment resolves three of the four fences it names, and its own last clause contradicts its own first clause

**Lines: SPEC.md 3949–3967 (§14.6), against 3807–3811 (§13.6).**

§13.6 and §14.6 both state the same four fences for pre-roam packets:

> they resolve for loss and retransmission but feed **no congestion event**, **no persistent-congestion walk**, **no RTT sample**, and **no `app_limited` growth** (§14.6, 3953–3955)

The ruling-137 amendment (3957–3967) then says:

> *One marker cannot serve those four fences and the text must not be read as saying it does.* The recovery-period marker serves the congestion event and `app_limited` growth, because §14.3 already gates both on it. It **cannot** serve the RTT fence: `recovery_start` is also set by every ordinary congestion event … The mechanism that works is the one this bullet already names: **path-generation stamping**. `SentPacket` therefore carries a `u32` path generation from slice 5 onward, held at 0 until roaming exists, and **§13.6's fences read that stamp rather than the recovery marker.**

Two defects, both of the rule-8 shape:

1. **The fourth fence is never adjudicated.** Four fences are named; the amendment assigns the congestion event and `app_limited` to the recovery marker and the RTT sample to the path stamp. **The persistent-congestion walk is not mentioned again.** An implementer must guess whether it is recovery-marker-gated (it is not — §14.4's walk ranges over the lost set, not over a start instant) or stamp-gated.
2. **The closing clause reverses the opening one.** "The recovery-period marker serves the congestion event and `app_limited` growth" vs. "**§13.6's fences read that stamp** rather than the recovery marker" — plural, unqualified, covering all four. Reading A builds a hybrid: two fences on `recovery_start`, one on the stamp, one undefined. Reading B builds all four on the stamp and leaves `recovery_start` doing only its ordinary §14.3 job.

**My reading (not a resolution):** Reading B is the intent and the last clause is the operative sentence. The middle sentences are *diagnosis* (why the naive single-marker reading fails), not *prescription*; the prescription is the last clause. Reading B is also the only one that is uniformly correct: a stamp answers all four fences by construction, whereas `recovery_start` answers the congestion-event fence only *by accident of the roam instant coinciding with a recovery start*, and the amendment itself has just finished explaining why overloading `recovery_start` is hazardous. Building the hybrid means two fences that behave differently after the first post-roam ordinary loss episode — exactly the silent, permanent failure the amendment warns about, reintroduced one clause later.

**Consequence: high.** This is the central mechanism of slice 7's congestion seam and the two readings differ in observable behaviour.

**Ruling 137 has the same gap, independently** (`.spec-v2-clean-slate/rulings.md:3470–3480`): *"§13.6 lists **four** things pre-roam packets must not feed: a congestion event, the persistent-congestion walk, an RTT sample, and `app_limited` growth. The marker serves **the first and fourth** … **It cannot serve the RTT fence** …"* — first, fourth, third. The **second** is named in the enumeration and never assigned. The ruling then closes with *"it pre-empts a defect that would otherwise be written into **four** fences"*, which reads as all four on the stamp. This is the "a ruling's blast radius includes the ruling" pattern the rulings file itself tracks at line 1997.

**And the code has already picked a third answer, which neither text states.** `src/core/connection/congestion.rs:70–80`:

> *"the recovery-period marker is set to the roam instant, not cleared, which fences the pre-roam flight out of both §14.3's congestion event and §14.5's `app_limited` growth. It does **not** fence the RTT sample **or the persistent-congestion walk**: ruling 137 puts those on `SentPacket::path_gen` …"*

Slice 5's implementer resolved the unassigned second fence to the stamp and split the four 2/2. That is very likely the right engineering answer — but it is **written only in a doc comment on an uncalled function**, and it directly contradicts §14.6's closing clause ("§13.6's fences read that stamp rather than the recovery marker", i.e. 0/4 on the marker). Slice 7 is the slice that calls `reset()` for the first time, so this divergence goes live now. A blind slice-7 test author reading §14.6 will assert Reading B; the implementer reading `congestion.rs` will build the 2/2 hybrid; both will be working from something authoritative-looking.

### F-C. The amplification budget has no stated disarm condition, and three other sections say it has one ("until the address validates by traffic")

**Lines: SPEC.md 1908–1945 (§7.3) against 920–925 (§5.6), 1815–1816 and 1833–1834 (§6.9), 3923–3924 (§14.5).**

§7.3 states the budget as a standing inequality with an explicit non-lifting clause (1914–1936):

> whenever a session's endpoint address changes (a roam) or is first anchored from a msg1 source, the address is **unvalidated** and a send-side budget arms — total bytes sent to the address MUST NOT exceed `AMPLIFICATION_FACTOR` (= 3) × total bytes **received from it and authenticated** on this session, both counters resetting at each such address change. … **The 3× ratio is never lifted** … a genuine peer clears it within about one round trip, because its own authenticated traffic funds the budget continuously

There is **no predicate anywhere in §7.3 that ends the unvalidated state.** The word "unvalidated" is introduced, the budget "arms", and nothing disarms it. Yet three other sections speak of validation as an event that occurs:

- §5.6, 922–925: "That anchor arms §7.3's anti-amplification budget: **until the address validates by traffic**, output to it is capped at `AMPLIFICATION_FACTOR` × the authenticated bytes received from it (§7.3)."
- §6.9, 1815–1816: "a peer-supplied anchor or roam target is send-capped by the anti-amplification budget **until it validates by traffic** (§7.3)."
- §6.9, 1820–1822: "It is not, by itself, the bound on everything an accepted session may emit toward a msg1 source that **has not yet validated**".

Two implementations follow:

- **Reading A (§7.3 literal, no disarm).** Every responder-side connection, and every connection after a roam, permanently enforces `sent ≤ 3 × authenticated_received` on the current address. The two counters live for the connection's lifetime. **This caps every asymmetric transfer at 3×.** A responder streaming a file to a peer that replies only with ACKs is throttled to three times the ACK volume — for the whole connection, not for one RTT. §7.3's own reassurance ("a genuine peer clears it within about one round trip") is true only for a *symmetric* exchange; it is false exactly in the download case, which is the common case. If Reading A is the intent, this is a throughput cliff that no test in the repo can currently see, because nothing roams and the counters do not exist yet.
- **Reading B (the prose in §5.6/§6.9).** The address transitions to *validated* on some traffic condition, after which the budget is dropped and the counters are freed. But **no section states the condition** — not a byte threshold, not a packet count, not "the first authenticated packet received *from the new address*". §7.3 explicitly declines the "N-authenticated-packets-over-1-RTT validation unlock" as one of "the judgment calls needing the ruling", which reads as *declining Reading B* — but then §5.6 and §6.9 were left describing Reading B anyway.

Working rule 3 applies: do not default to the code-like rule. §7.3 is the formal rule; §5.6 and §6.9 are the prose; they disagree about whether validation is a state transition or a permanent inequality.

**My reading (not a resolution):** §7.3 literal (Reading A) is the ratified mechanism — the declined-alternatives paragraph shows the maintainer considered and rejected an unlock, and "never lifted" is emphatic. If so, the three "until it validates by traffic" phrasings are stale prose from before that decision and should be reworded ("while the budget binds", not "until it validates"), **and** §7.3 needs an explicit sentence about the asymmetric-transfer consequence, because an implementer who ships Reading A and an implementer who ships Reading B produce protocols with different achievable throughput. I flag with low confidence: the phrase "clears it within about one round trip" is language for a transition, not for a standing cap, and appears inside §7.3 itself.

**Consequence: highest.** Either a permanent 3× throughput cap nobody has costed, or a security control with no defined end. Both readings are shipped somewhere in the document.

### F-D. `AMPLIFICATION_FACTOR`'s scope: the constant table says "per unvalidated address", the rule says "on this session" — and nothing says what happens when two connections share an address

**Lines: SPEC.md 1917–1920 and 1943–1945 (§7.3), 6375 (§ constants appendix).**

The rule text scopes the received-byte counter to the session:

> total bytes sent to the address MUST NOT exceed `AMPLIFICATION_FACTOR` (= 3) × total bytes **received from it and authenticated** **on this session**

The constant table (1945) and the appendix (6375) both scope it to the address:

> | `AMPLIFICATION_FACTOR` | 3 (× authenticated bytes received, **per unvalidated address**) |

These differ whenever two connections on one endpoint share a peer address — which §6.9's own threat model makes routine, since an attacker spoofs a hint-set source and a msg1 source freely, and NAT collapses many peers onto one address. Per-session gives an attacker N connections × the budget aimed at one victim address; per-address requires a shared per-address accounting table on the endpoint, which is state the endpoint does not have and which §17.4/§17.5 do not budget for.

**My reading (not a resolution):** per-session is the intent — it is the normative sentence, it is the only one implementable inside `core::Connection` (which is where §13.6/§14.6 put the roam seam), and §17.5's per-connection state census (5577) lists the connection's state without any endpoint-side address table. The table's "per unvalidated address" is then shorthand meaning "per unvalidated address, per session", and reads as a stronger claim than the rule delivers. The residual — N sessions to one address multiplying the reflector by N — deserves a sentence either way. Note this is exactly the §2.3 `TAG`-beside-`PK` shape from rule 8: a parenthetical in a constant table asserting a scope the normative text does not.

**Consequence: medium-high.** Wrong choice is either a security hole or unbuildable state.

### F-E. Nothing states the priority order of output competing for a scarce amplification budget — and the pending contested probe is the output that can be starved by it

**Lines: SPEC.md 1925–1927 (§7.3), 2425–2436 and 2438–2442 (§7.5), 4967–4979 (§16.4), 3923–3940 (§14.5).**

§7.3, 1925–1927:

> The budget binds **all** output to the address, **explicitly including the §14.5 and §13.4 congestion-window exemptions** — those exemptions are scoped to cwnd, never to this budget.

§7.5, 2438–2442:

> The deadline is armed at the probe's **transmission**, not at the mark, so a probe that §7.3's budget will not yet admit leaves the mark **pending** rather than failed — the endpoint sends it, and arms, at the first instant the budget allows.

The construction is complete; **its scope is not.** When the budget admits fewer bytes than the pending output demands, the spec never says which output goes first. Immediately after a roam the competing claimants are, at minimum: the ACK owed for the roam-triggering packet (§12), any keepalive due (§7.5), the PTO probe (§13.4), retransmissions, new Data, and the pending contested PING. The budget after a roam is exactly `3 ×` the roam-triggering packet's size, since §7.3 resets both counters at the address change — so a ~32-byte trigger yields a ~96-byte budget, and one ACK plus one keepalive exhausts it.

Two implementations:

- **Ordering A (naive, output-queue order).** The probe is whatever position the output queue puts it in — so an endpoint that emits its ACK first, then a keepalive, then discovers the budget exhausted, leaves the mark pending. Repeat on the next roam and the mark is pending for the connection's life.
- **Ordering B (probe-first, or liveness-first).** The pending probe pre-empts other output because it is the only output with a *verdict* attached.

Under Ordering A, an attacker with a supply of withheld peer→us Data — the exact attacker §7.5's "Why an ACK rather than a receive" paragraph (2357–2367) is written against — injects one small withheld packet just under `DEAD_TIMEOUT` **from a fresh source address each time**. Each injection is authenticated and window-fresh, so it (a) refreshes liveness, so `DEAD_TIMEOUT` never fires, (b) roams the session, so §7.3 **resets the budget counters to that one packet's bytes**, and (c) keeps the budget too small for the probe to win against the ACK it also owes. §16.4 (4977–4979) handles the pending case only by saying "**if** the connection dies before the budget ever admits the probe, the death arrives as `Closed` like any other" — but in this scenario the connection does not die, because the same injections that starve the probe also refresh the death clock. The zombie §7.5 exists to reap becomes immortal, and `Contested` never fires, so the application sees nothing.

I want to be precise about the strength of this: with typical packet sizes a `3 ×` budget usually *does* admit a ~40-byte PING, so this is not an unconditional break. It is a break **whose existence is decided entirely by an ordering the spec does not state**, which is why it belongs here rather than in a threat annex.

**My reading (not a resolution):** the intent is Ordering B — §7.5 says the probe is exempt from the congestion gate precisely because "a probe the gate could delay past its own deadline would silently convert congestion into a liveness verdict, so that exemption is correct and must not be removed" (2426–2429), and the identical argument applies verbatim to the budget delaying it past its own reap. The budget cannot be waived (§7.3 is emphatic), so the only lever left is **priority within the budget**. §7.3 or §7.5 should state one. Note also that §7.5's answer "a connection may not be killed by a question that was never asked" is *only half a rule*: it says what must not happen to a pending mark, and nothing about what bounds how long it may stay pending.

**Consequence: high.** It is the one path that defeats the contested probe, and it is reachable by the attacker the probe was designed against.

**§6.8 states the unqualified guarantee, in the same paragraph that establishes the attacker roams the session.** SPEC.md 1694–1711:

> §7.4's clock is driven by mere authenticated receipt, so an adversary who harvested genuine peer→us Data … can inject one harvested packet every less than `DEAD_TIMEOUT` from anywhere off-path and keep the zombie's liveness clock reset forever, **roaming the session to itself in the process**. … The **contested-connection probe** (ruling 36, §7.5) is the answer … **so the zombie dies within `KEEPALIVE_TIMEOUT` of the probe** and the restart resolves after all.

The paragraph names the roam and then asserts the reap, and never joins them. §7.3 resets both amplification counters at every address change, so each injection re-arms an unvalidated budget of exactly `3 ×` one small harvested packet — and §7.5 2440–2442 says a probe the budget will not admit "leaves the mark **pending**". "Dies within `KEEPALIVE_TIMEOUT` **of the probe**" is trivially true and says nothing about a probe that never leaves. §6.8 is the one section in this range that explicitly does the premise-naming exercise ("that bound rests on a premise, and the premise must be named", 1684–1685) — and it does it for the `DEAD_TIMEOUT` bound and not for the probe bound that replaces it.

### F-F. §7.3's budget is funded by *authenticated* bytes; §7.2 restricts every other consumer to *authenticated and window-marked*. The divergence is deliberate in one place and unremarked everywhere else

**Lines: SPEC.md 1872–1874 (§7.2), 1918–1924 (§7.3), 1895–1897 (§7.3).**

§7.2, 1872–1874:

> **Liveness and roaming are driven only by packets that are both authenticated and window-marked** (fresh). No replayed packet ever moves the endpoint or refreshes liveness.

§7.3, 1918–1924, funding the budget:

> total bytes **received from it and authenticated** … Authenticated means the packet's AEAD tag verified (**§7.2's authenticated class**; the anchoring initiation qualifies, its handshake tail tags having verified at admission) — merely-received bytes, unauthenticated or undecryptable datagrams claiming the address, MUST NOT replenish the budget.

§7.3 defines its funding predicate as *authenticated*, cites §7.2's **authenticated** class by name, and then rules out only the *unauthenticated* case. §7.2's own sentence establishes a strictly narrower class — authenticated **and** window-marked — for the two consumers it names. So on the literal text, **a replayed (authenticated, non-fresh) packet replenishes the amplification budget** even though it is dropped without delivery and moves nothing else. The exclusion list in §7.3 ("unauthenticated or undecryptable datagrams") does not mention replays; rule 8 reads that list as exhaustive.

- **Reading A (literal):** the budget counts authenticated bytes including duplicates. An on-path attacker can inflate our send budget toward an address by replaying the peer's own packets at us. The reflection target is the genuine peer, so the damage is bounded — but it means the one security counter in §7.3 is advanceable by an attacker who holds no key.
- **Reading B (§7.2's class):** fund only on authenticated-and-window-marked bytes, matching liveness and roaming, so no replay ever advances anything.

**My reading (not a resolution):** Reading B is the intent. §7.2's sentence reads as a global invariant about what replayed packets may drive, §7.3's own summary line 1897 restates it for roaming ("Nothing unauthenticated, and no replayed packet, ever moves it"), and there is no argument anywhere for letting duplicates fund a security budget. The literal §7.3 text is a scope slip: it needed to exclude *unauthenticated* bytes (that is what the sentence is doing) and did not notice it was also, by citing the broader class, admitting *replayed* ones. Working rule 3 is squarely in point — the prose invariant in §7.2 is right and the formal predicate in §7.3 is the one with the bug.

**Consequence: medium-high.** One word decides whether a keyless on-path attacker can move a security counter, and the choice is invisible in any test that does not replay.

### F-G. §16.5 declares its equal-deadline list "exhaustive" and it is not a total order — the two unordered pair-groups are exactly the ones slice 7 makes reachable

**Lines: SPEC.md 5086–5106 (§16.5), against `src/core/connection/timers.rs:33–65`.**

§16.5, 5086–5090:

> **Equal-deadline priorities** (normative). … This list is **exhaustive**: every pair of deadlines that can fall on one instant is ordered here, because §16.4 makes generation order normative and an unordered pair would make that claim hollow exactly where two timers collide.

The per-connection orderings the text actually states (5096–5106) are:

> Per connection, loss detection beats PTO and exactly one of the two fires per evaluation; **teardown collection (liveness, `CloseLinger` expiry, then `Contested`) precedes keepalive evaluation** …; `AckDelay` fires after the loss/PTO evaluation at the same instant …; and `PersistentKeepalive` is evaluated last.

Extracting the relation over the eight timers: `Liveness < CloseLinger < Contested`; `{Liveness, CloseLinger, Contested} < Keepalive`; `Loss < Pto`; `{Loss, Pto} < AckDelay`; `PersistentKeepalive` last. **Two pair-groups are left unordered:**

1. `{Liveness, CloseLinger, Contested}` vs. `{Loss, Pto, AckDelay}` — nothing relates the teardown group to the recovery group.
2. `{Loss, Pto, AckDelay}` vs. `Keepalive` — the text orders teardown-before-keepalive and loss-before-AckDelay, and never joins the two chains.

The claim of exhaustiveness is therefore false as written. `timers.rs` already resolved it — the declaration order is `Liveness, CloseLinger, Contested, Loss, Pto, AckDelay, Keepalive, PersistentKeepalive`, with `Ord` derived from the discriminant, and the module doc quotes §16.5 as its authority. That resolution is a *reasonable* extension of the stated governing principle ("a terminal outcome precedes a routine one, and state removal precedes emission"), but it is the code's extension, not the spec's.

**Why this bites in slice 7 specifically:** before slice 7 only five of the eight timers are ever armed (`Liveness`, `CloseLinger` from slice 3; `Loss`, `Pto`, `AckDelay` from slice 5). Slice 7 arms `Contested`, `Keepalive` and `PersistentKeepalive` for the first time — so *every* collision in the two unordered groups becomes reachable for the first time in this slice, and `timers.rs`'s own module doc says it was written to stop slices 5 and 7 "re-deriving it from prose". A slice-7 test author working from §16.5 (as working rule 6 requires) can derive `Loss` before `Contested` from the prose and write a passing-looking assertion that contradicts the frozen enum.

**My reading (not a resolution):** `timers.rs`'s order is the intent and §16.5's prose needs the two missing relations spelled out — "teardown collection precedes loss/PTO evaluation" and "loss/PTO/`AckDelay` precede keepalive evaluation" — because the governing principle yields the first but not obviously the second (`AckDelay` before `Keepalive` is emission-before-emission, which the principle does not reach). Flagging under rule 8: the word "exhaustive" is the same self-certifying scope claim as §13.6's title.

**Consequence: medium.** No wire impact; a real risk of a blind test author asserting the opposite of the frozen order.

### F-H. §6.4's `Stale` enumeration omits re-home-walk exhaustion, and §6.9 and §6.4 disagree about whether an exhausted walk marks the live connection contested

**Lines: SPEC.md 1323–1332 and 1449–1456 (§6.4), against 1763–1766 and 1791–1797 (§6.9).**

§6.4's re-home walk (1330–1332):

> A failing candidate is discarded and the next-newest tried, **until admission or exhaustion**. The per-source cap bounds the walk at four `es` + `ss` pairs.

§6.4's `Stale` bullet (1449–1452) then enumerates the refusal cases:

> **Stale.** If no initiation for that static is currently parked, if the admitted candidate fails the basis rule above, or if the static is PENDING and we are the tie-break winner, `accept()` returns `AcceptError::Stale`

**Exhaustion is not in that list** — and it is a distinct case from "no initiation currently parked", because the walk exhausts precisely when initiations *are* parked and all of them fail (wrong static, bad tail tag, or guard-rejected timestamp). §6.9 supplies the missing answer in passing (1763–1766): "mac1-valid rubbish parked for a chain's source can cost a legitimate `accept()` up to four wasted `es` + `ss` pairs **before it returns `Stale`**". Rule 8: an implementer reading §6.4's list as exhaustive has no return value for the exhaustion path.

**The consequential half — does exhaustion mark the connection contested?** §6.4 scopes the mark to admission (1333–1348):

> **At admission**, fast path or re-home: if a LIVE connection exists for the proven static … Otherwise — basis `None` … `accept()` returns `AcceptError::Stale` … **One exception to "untouched"**: a refusal against a **`None`** basis marks that connection **contested**

§6.9 scopes it to the refusal, with no admission qualifier (1791–1794):

> **A refused `accept()` against a live connection whose `replacement_basis` is `None`** marks that connection contested and sends one ack-eliciting PING on it

An exhausted walk *is* a refused `accept()` against a live `None`-basis connection, and it reached no admission. The two texts therefore differ on a security-relevant question: **can an attacker who can only park mac1-valid rubbish provoke a contested mark?** Under §6.4 no — the mark requires a candidate that yielded the same proven static with a verifying tail tag, i.e. a genuine (possibly replayed) initiation. Under §6.9 yes — rubbish that exhausts the walk refuses the accept, and the refusal marks.

**My reading (not a resolution):** §6.4's narrower rule is the intent. §7.5's security argument depends on it — "an attacker's **replay supply** buys refusals, and refusals now buy at most one collapsed mark" (2393–2394) prices the primitive in *captured genuine initiations*, not in rubbish, and §6.9's own DoS table prices the rubbish rows at "0 DH … one bounded queue slot", never at a probe. §6.9's sentence is a cost summary written loosely. But the difference is exactly the kind of scope slip rule 8 names, and slice 7 is the slice that has to pick one.

**Consequence: medium-high.** Decides whether the `Contested` notification can be triggered by an attacker holding no key material at all.

> **F-C reinforcement.** §7.4 is a **fourth** site using validation-as-an-event language. SPEC.md 2001–2005: *"the beacon's output to an unvalidated address stays capped by §7.3's anti-amplification budget at 3× the authenticated bytes received, so the session emits at most 588 B for the replayed 196 B **and then goes quiet until the address validates**."* Four sections (§5.6 923, §6.9 1816, §6.9 1821, §7.4 2004) describe a transition that §7.3 never defines and arguably declines. Note also that §7.4's "goes quiet **until the address validates**" is doing load-bearing work in a *security* claim — it is the sentence that bounds a replayed-initiation beacon. If Reading A (no disarm) is correct the sentence is still true but for a different reason (the budget never grows because nothing genuine arrives); if Reading B is correct it names a mechanism that does not exist, which is working rule 11's defect class in the spec rather than in a ruling.

### F-I. The probe-rate bound ("one packet per `KEEPALIVE_TIMEOUT` per connection") holds only against a *dead* peer; against a live one the mark clears in ~1 RTT and the next refusal re-marks

**Lines: SPEC.md 2411–2423 (§7.5, ruling 43), 1798–1803 (§6.9), 2315–2317 and 2326–2330 (§7.5); `.spec-v2-clean-slate/rulings.md:546–553` (ruling 43).**

§7.5, 2415–2421:

> What actually bounds the probe is the collapse rule above. A connection that is already contested absorbs every further refusal without a mark, a PING, or a re-armed deadline, so the probe rate is **at most one packet per `KEEPALIVE_TIMEOUT` per connection** — about one packet per 10 s — **no matter how many Intros arrive**. … That is a bound the attacker cannot move

§6.9 states the same number (1800–1803). Ruling 43 states it as the correction to an earlier false bound.

**The collapse rule only suppresses refusals that land *while the mark is outstanding*.** §7.5, 2326–2330: "A connection is contested or it is not: the state is a single `Option<(probe_floor, deadline)>` … A refusal that lands while the connection is **already** contested is **not** a second mark." And 2315–2317: "The mark clears on **any ACK covering any counter at or above the probe floor**." On a **live** connection the peer ACKs within one RTT, so the mark's lifetime is ~1 RTT, not `KEEPALIVE_TIMEOUT`. The next refusal then lands on an *uncontested* connection and is a full second mark: new floor, new PING, new deadline.

So the true probe rate is `min(refusal rate, 1/RTT)`, and `KEEPALIVE_TIMEOUT` bounds it **only in the one case where the peer never answers** — precisely the case in which the connection is about to die anyway. On a 10 ms-RTT path that is up to 100 probes/s per connection rather than 0.1/s: a factor of 1000. Each is cwnd-exempt (§14.5), enters the sent map, and arms the death clock.

Working rule 3 does not apply here — both texts say the same thing and both are wrong in the same way, which is why review did not catch it. Ruling 43 replaced one dishonest bound ("the application's own accept rate and nothing an attacker controls") with another. It is bounded by the application's accept rate — the very thing ruling 43 ruled insufficient — with a `1/RTT` ceiling on top.

**My reading (not a resolution):** the honest statement is "at most one probe per mark, one mark per uncontested refusal, and marks cannot overlap" — plus, if a real rate bound is wanted, an explicit re-mark cooldown, which §7.5 does not currently have and which would need a ruling because it interacts with the security argument (a cooldown means a genuine second doubt goes unprobed for its duration). Note the security half is untouched either way: every re-mark records a *fresh* floor, so each probe still demands post-doubt acknowledged progress. This is a **cost** defect, not a security one, but it is the exact sentence ruling 43 exists to make true.

**Consequence: high.** A ratified, twice-stated quantitative bound that is off by ~1000× in the common case, and slice 7 will be tested against it.

### F-J. The pending-mark state has one stated entry and one stated exit; the other two exits are unstated, and one of them emits `ContestCleared` with no preceding `Contested`

**Lines: SPEC.md 2438–2444 (§7.5), 4967–4990 (§16.4), 5066–5069 (§16.5).**

§7.5, 2438–2442:

> The deadline is armed at the probe's **transmission**, not at the mark, so a probe that §7.3's budget will not yet admit leaves the mark **pending** rather than failed — **the endpoint sends it, and arms, at the first instant the budget allows.**

§16.4, 4982–4983: "**`ContestCleared` is emitted when the mark clears** because an ACK covering the probe floor arrived." — unconditional, with no reference to whether the probe was ever sent.

The pending state's transitions, as the text leaves them:

| From pending | Stated? |
|---|---|
| budget admits → send probe, arm `Contested`, emit `Contested` | **yes** (2440–2444) |
| connection dies of something else → `Closed` | **yes** (4977–4979) |
| **an ACK covering the floor arrives while still pending** | **no** |
| **the connection roams again while still pending** | **no** |

Row 3 is reachable and ordinary: the mark's floor is "the counter the next seal will use", and *any* post-mark seal — a keepalive, a retransmission, a pure ACK, an application Data packet — lands at or above it, so an ACK can cover the floor before the probe itself is ever emitted. Two literal consequences follow from the text as written:

1. **`ContestCleared` fires with no preceding `Contested`.** §16.4 justifies the two-variant design by saying "a reconnect scheduler needs set, cleared, and `Closed`, no more" (4988–4990), which presupposes pairing. An unmatched `ContestCleared` is exactly the mis-read that ruling 46 deleted `under_probe: bool` to avoid.
2. **The probe is still sent when the budget later admits it**, because 2441 says "the endpoint sends it, and arms, at the first instant the budget allows" with no condition — arming a `KEEPALIVE_TIMEOUT` verdict deadline for a mark that no longer exists. `timers.rs` has one `Contested` slot and §16.5 says it "is disarmed by any ACK covering the mark's probe floor", which cannot disarm a deadline that was not yet armed.

Row 4 matters because a roam resets the amplification counters (§7.3) — a pending probe's budget prospects change discontinuously at every roam, and §13.6 does not list the pending mark among what a roam touches (see F-A).

**My reading (not a resolution):** clearing a pending mark should cancel the pending probe and emit **nothing** — the same "the mark-pending gap emits nothing" principle §16.4 states for the gap should cover its exit. §7.5's rule needs "unless the mark has already cleared" on the send, and §16.4 needs "`ContestCleared` is emitted only where `Contested` was".

**Consequence: medium-high.** An unpaired notification and a stray probe/deadline, both in the state slice 7 introduces and neither reachable by any pre-slice-7 test.

### F-K. Ruling 91's amendment records an *unresolved* ambiguity in the exact predicate §6.4's slice-7 work reads, and nothing since closes it

**Lines: `.spec-v2-clean-slate/rulings.md:1987–1994`; SPEC.md 1407 (§6.4), 1465 (§6.5), 5513–5514 (§17.4).**

The rulings file records, explicitly as "recorded rather than resolved":

> **A gap ruling 90 created that §6.5 predates** … `mint_pending` without `start_attempt` is a pending with **no initiation in flight**. §6.5 speaks of "in-flight outbound initiations" and §17.4 of "the pending tables' dialled addresses" — those named the same set before ruling 90 and no longer do, and **the same ambiguity reaches §6.4's PENDING branch ("if an in-flight outbound initiation exists")**.

I grepped `rulings.md` for every later mention of `mint_pending` / `start_attempt` / "in-flight outbound": the last is this passage. **The gap is still open**, and §6.4's PENDING branch text at 1407 still reads "If an in-flight outbound initiation exists for the proven static". Slice 7 owns the rest of §6.4 (the re-home walk and the proven-LIVE replacement admission), so it will be reading the same static-map row through the same ambiguous predicate; whether a `mint_pending`-only pending counts as PENDING decides whether a re-home walk's admission takes the replacement path or the PENDING path.

This is not a new finding — it is a flag that a **known-open** item sits directly in slice 7's path and must not be assumed closed by the fact that slice 4 shipped. Note also that §17.4's hint-set sentence (5513–5514) is one of the two texts the gap is about, and §17.4 is in slice 7's stated spec surface.

**Consequence: medium.** Known-open, cheap to resolve, expensive if two blind agents resolve it differently.

### F-L. §7.5 makes a contested mark on a closing/draining connection a no-op; §6.4, which is where the mark is taken, never mentions it

**Lines: SPEC.md 2446–2448 (§7.5) against 1333–1348 (§6.4).**

§7.5, 2446–2448:

> A contested mark taken on a connection **already closing or draining (§15.2)** is a **no-op**: that connection is already leaving, and the parked `Intro` will meet no live static.

§6.4's admission rule (1333–1348) speaks only of "a **LIVE** connection … for the proven static" and its `replacement_basis`. It never mentions closing/draining, and §17.4's map is a `static → connection` map whose entry §6.4 reads. Whether a closing connection is still LIVE in that map — and therefore whether §6.4's admission even reaches the mark — is decided elsewhere (§5.4's three-valued rule, §15.2's linger). If a closing connection has already left the map, §7.5's no-op clause is unreachable and describes a case that cannot arise; if it has not, then §6.4's rule as written takes the mark and §7.5 has to undo it.

**My reading (not a resolution):** the closing connection is still in the map for its linger (that is what makes the `Retired` event and the guard-entry pin necessary, §16.4 5040–5050), so §7.5's clause is live and §6.4's admission needs the same carve-out stated where the mark is taken. This is minor as a behaviour question and worth stating only because §6.4 is where an implementer writes the code.

**Consequence: low-medium.**

---

## Checked and clean

Covered and found nothing worth a ruling:

- **S11's three collapse claims are all stated.** §7.5 2326–2337 states, in terms, that a refusal landing on an already-contested connection is "**not** a second mark", "sends no second PING", and "does **not** re-arm the deadline" — with the security rationale (an attacker postponing the verdict by dripping Intros). §6.4 1358–1360 and §16.5 5069–5072 restate it consistently. No divergence among the three.
- **"Notification at transmission, not at marking" is stated four times and agrees every time**: §7.5 2443–2444, §16.4 4967–4976, §16.5 5066–5068, §15.4 4093. All four also agree that the deadline arms at the same instant, and all four give the same reason (the budget can separate the two moments).
- **The probe floor's scope across a ratchet, a roam, and nonce exhaustion is safe — by construction, though §7.5 never says so.** §7.7 2476 is explicit: "the counter is **never reset** by the ratchet; `2⁶⁴ − 1` is reserved for the `Rekey()` transform", and §1.x 465 confirms one counter space `0 ..= 2⁶⁴ − 2` with exhaustion terminal (`ConnectionLost::NonceExhausted`, §7.9). §7.8/§5.4 give one session per connection, so there is no second counter space a roam or a rekey could introduce. A floor recorded as a `u64` counter therefore stays meaningful for the connection's life under all three events. I checked this specifically because the brief asked; the scope is *unstated in §7.5* but *unambiguous from §7.7*, so I am not raising it as a defect. It is worth one cross-reference if §7.5 is edited anyway.
- **The anti-replay window across a roam.** §7.2 never mentions roaming and §7.3 never mentions the window, which initially looks like the F-A omission — but with one session, one counter space and a never-reset counter (§7.7 2476), keeping the window is the only coherent reading, and resetting it would re-admit every previously-seen counter. §7.2's "greatest authenticated counter plus the bitmap" is a property of the session, not of the path. Clean, though it is another item §13.6's title implicitly claims to cover.
- **Every slice-7 death path exists in `src/error.rs`.** `ConnectionLost` carries `TimedOut`, `NonceExhausted`, `LocallyClosed`, `PeerClosed`, `ProtocolViolation`, `Replaced`, `EndpointDropped` (`src/error.rs:196–225`). §15.4's contested row (SPEC.md 4093) maps the contested death to `ConnectionLost::TimedOut` — "the same variant, no new one" — and S3's replacement death to `Replaced`, which exists. No slice-7 death needs a new variant, and none is missing.
- **§16.5's `Contested` timer exists and is in the frozen order.** `src/core/connection/timers.rs:33–65` declares all eight `TimerKind`s with `Contested` third, derives `Ord` from the declaration order, and documents the order as ruling 76's. Slice 7 needs no new timer kind. (The order's *spec* incompleteness is F-G; the code side is clean.)
- **The tie-break's replacement basis is consistent across §6.7, §17.4 and the slice-4 code.** §6.7 1592–1594: the tie-break **winner**'s connection "replacement basis stays `None`: we are its initiator". §17.4 5524–5529 gives `Some(t)` for "a staged `accept()`, a re-homed `accept()`'s admitted candidate, or the tie-break loser's admit step" and `None` for "a `connect()` completed by msg2, **or a tie-break we won**". `src/core/endpoint/staged.rs:611–680` sets `lost_tiebreak = true` on the loser branch and installs with `replacement_basis: Some(timestamp)`; the winner branch returns `Stale` with the pending left in place (`staged.rs:623–626`) and keeps its guard record (`staged.rs:784–790`), matching §6.4 1401–1406's single carve-out. No divergence.
- **S4's "holds under both orderings" is what slice 4 built.** `src/core/endpoint/routing.rs:23–45`'s module doc addresses exactly the staged-path-vs-internal-tie-break asymmetry, states that both branches read the same `StaticState::Pending` row ("One map, one answer"), and `wins_tiebreak` is shared by both routes (`routing.rs:455`, `staged.rs:623`). §6.7 1594–1598 says the only visible difference is `AcceptError::Stale` vs. a silent drop. Consistent. The residual risk here is F-K's predicate ambiguity, not the tie-break itself.
- **Ruling 137's named mechanism does exist in the code** (working rule 11 check): `SentPacket::path_gen: u32` is present at `src/core/connection/recovery.rs:66`, documented as ruling 137's, and asserted 0 at `recovery.rs:198`. The rationale names a real field. Its *scope* is the problem (F-B), not its existence.
- **Ruling 139(a) is coherent with the budget.** `rulings.md:3497–3499`: "`pto_count` increments **at the `Pto` timer's firing**, before the probe is built — … the one that stays right in slice 7 where §7.3's budget can prevent a probe leaving." That is a deliberate, stated decision that a budget-blocked PTO probe still backs off. It reinforces F-E (several output classes contend for the budget) but is itself clean.
- **§7.4's liveness model needs no slice-7 change.** The marking/arming split (SPEC.md 1953–1979), the install pin (1981–2007), and the terminology note (2037–2043) are self-consistent, and §7.5's passive rule (`R > S`, 2056–2057, 2117–2119) uses the same two variables. Rulings 40 and 42's bounds — `[1 s, DEAD_TIMEOUT)` — are stated identically in §5.7 935, §7.5 2051 and §7.5 2170, with the same default (10 s) in all three. No drift.
- **`AMPLIFICATION_FACTOR = 3` is stated identically in all three of its sites** (SPEC.md 1918, 1945, 6375). The value is clean; only its scope (F-C, F-D) and its funding predicate (F-F) are not.

---

## Coverage note

Read in full: §5.6 (partial, 886–925), §5.7's timer table (927–935), §6.4 (1310–1457), §6.7 (1564–1662), §6.9 (1713–1841), §7.2 (1859–1891), §7.3 (1893–1945), §7.4 (1947–2044), §7.5 (2045–2179, 2296–2455), §13.6 (3799–3820), §14.6 (3942–3977), §16.4's `Contested` bullet (4955–4996), §16.5 (5040–5117), §17.4 (5510–5566). Grepped but not read whole: §6.3, §6.5, §6.6, §6.8, §7.7, §7.9, §12, §14.3–§14.5, §15.4. `SPEC.md` was never read whole (working rule 1).

Code read: `src/core/connection/timers.rs` (1–70), `src/core/connection/congestion.rs` (70–100), `src/core/connection/recovery.rs` (55–70, 536–556), and greps over `src/core/endpoint/routing.rs`, `src/core/endpoint/staged.rs`, `src/error.rs`. Per working rule 16 the tree was clean at `b649575` and no other agent had written to these paths, so the working copy and the commit agree; nothing was staged or committed by me.

Rulings read: 137 (3470–3485), 139 (3494–3510), 41–43 (505–556), the ruling-91 amendment and the "blast radius" pattern note (1962–2010), and the ruling-31/32 summaries (270–300). `rulings.md` was never read whole.

§6.8 (1663–1712) was read after the first pass and is folded into F-E.

**Not covered** (out of time/context, flagged so the maintainer knows): §6.3's stage-0 queue (1161–1309) was only grepped — the evict-oldest/park-time question of ruling 69 is *specifically* called out in `CLAUDE.md` as a place where prose and rule diverged, and I did not re-audit it; §6.8's restart summary (1663–1712) was not read; §12's ACK derivation was not read, and F-I/F-J both depend on how promptly an ACK can cover the probe floor, which §12.3's delayed-ACK rules govern.
