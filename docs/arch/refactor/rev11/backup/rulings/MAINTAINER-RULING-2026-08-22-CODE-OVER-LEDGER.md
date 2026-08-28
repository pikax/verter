---
ruling_id: "CODE-OVER-LEDGER"
type: "maintainer-ruling"
date: "2026-08-22"
date_source: "stated"
binds: ["BV2", "B5", "CM1", "scripts/validate-program-state.mjs", "ledger bookkeeping protocol"]
summary: "Ends repeated ledger-correction churn over gaps a past orchestrator created and cannot honestly be reconstructed after the fact, while keeping bookkeeping a live, enforced requirement for work going forward. Grandfathers exactly three legacy rows (BV2, B5, CM1) for the context_packet_digest gap; fixes a structural bug in the fixed-landing-order rehearsal that produced false conflicts for already-landed blocks; does not touch any other gate."
supersedes: []
superseded_by: []
contradicts: []
---

# Maintainer ruling — code over ledger, bookkeeping stays a goal

**Status:** RATIFIED by the maintainer, 2026-08-22.

## Why this ruling exists

A prior orchestrator discovered that CM1's ledger row could not honestly
satisfy `context_packet_digest` (no context packet was ever produced for it at
dispatch time — the block was cut directly from the maintainer's beta.4
regression-intake directive), and, separately, that CM1's three review
mandates had been recorded `PASS` without a reviewer ever actually examining
the candidate. It reverted CM1's status from `ACCEPTED` back to `REVIEW` for
BOTH reasons together, then could not close the `context_packet_digest` gap,
because no amount of post-hoc editing produces a packet that was never
written. Chasing THAT gap — a legacy artifact that cannot honestly be
reconstructed after the fact — is the pure churn this ruling ends: no amount
of ledger editing makes it more correct, it just relitigates a fact about the
past that cannot change. The same shape of gap exists for BV2 and B5. **The
other reason for the reversion — the genuine `BLOCK` verdict — is not the
churn this ruling addresses**: a real review outcome is not a legacy
bookkeeping gap, and §4 below keeps review mandates fully live and untouched.

## The ruling, verbatim

> "if the code is correct and landed, the ledger should say accepted, the sha
> and other validation should be bypassed... code is more important ledger is
> only to make sure the code is going well"

and, clarifying it:

> "the book keeping should still be something we strive to do right, but we
> just encountered some issues with the previous orchestrator, I just want to
> avoid a lot of churn because of the ledger, I don't mind fixing actual
> issues with code"

## The operative rule

**This is not "disable the bookkeeping."** Bookkeeping remains a goal and
stays fully, mechanically enforced for new work. What ends is repeated
correction loops over gaps a past process failure created, which no amount of
editing can now honestly satisfy. Every decision under this ruling — and
every future one that cites it — turns on one distinction:

- **A legacy gap** — an artifact that was never produced when it should have
  been, and cannot be honestly reconstructed after the fact (fabricating one
  now would itself be a falsified record, worse than the gap). Grandfather
  it: record it once, explicitly, by name, with the reason, and stop. Do not
  loop on it, do not revert accepted status to chase it, do not invent an
  artifact to satisfy the check mechanically.
- **A live requirement** — bookkeeping for work dispatched and executed from
  this point forward. This stays a hard, failing gate. A block that reaches
  `REVIEW` without a real, digest-bound context packet fails the validator
  exactly as it did before this ruling. Nothing about "code over ledger"
  excuses skipping bookkeeping when it is actually possible to do it right.

A grandfather exemption must be **narrow and enumerated** — it names the
specific blocks it covers, so a gap discovered tomorrow does not silently
inherit yesterday's exemption. A structural fix to the validator (as opposed
to an exemption) is preferred wherever the underlying check is itself wrong,
not merely inconvenient — see §2 below.

## 1. `context_packet_digest` — three legacy rows grandfathered, by name

`scripts/validate-program-state.mjs` requires a well-formed
`context_packet_digest` for every block reaching `REVIEW`,
`ACCEPTANCE_RECOMMENDED`, `ACCEPTED`, or `PRIVATE_CHECKPOINT`. Exactly three
ledger rows violate this and cannot honestly be fixed by editing the ledger:
**BV2**, **B5**, and **CM1**. **BV2** and **CM1** were dispatched directly off
the maintainer's beta.4 regression-intake directive, bypassing the normal
context-packet-first dispatch flow. **B5** did not go through that flow
either, but for a distinct reason: its dispatch predated its predecessor
BV2's acceptance being recorded, and it was only retroactively authorised
under `MAINTAINER-RULING-2026-08-22-BV2-B5-J1.md` §2 once BV2's acceptance
came to satisfy B5's DAG predecessor set — see
`authority-registry.toml`'s `B5` authorization row. No `context-packet.md`
(or equivalent immutable dispatch packet) was ever produced for any of the
three, for either reason. Reconstructing
one now, after implementation, would not be a record of what was supplied at
dispatch — it would be a fabricated input artifact backdated to look like
one, which Stub Prevention and Verification Must Prove Execution both already
forbid in spirit.

`scripts/validate-program-state.mjs` now carries a `CONTEXT_PACKET_DIGEST_
LEGACY_GAP_GRANDFATHER` set containing exactly `{BV2, B5, CM1}`, cited to this
ruling inline. It exempts only the `context_packet_digest` field, only for
these three IDs. Every other required field on these rows — `base_sha`,
`candidate_sha`/`candidate_tree`, `charter_digest`, `evidence_digest`, the
three review mandates, `accepted_sha`/`accepted_tree` — remains fully
enforced, exactly as for any other block. A fourth block cannot join this
exemption by resembling BV2/B5/CM1; it can only join by an explicit amendment
to this ruling naming it and stating why its own gap is equally
unreconstructable. A block dispatched from today onward that reaches `REVIEW`
without a real context packet still fails this check — confirmed by
experiment (see the validator delta reported alongside this ruling).

## 2. The fixed-landing-order rehearsal — a model bug, fixed structurally

The rehearsal in `verifyConcurrentLandingSafety` replays each concurrently
active block's declared `base_sha..candidate_sha` delta onto a synthetic
cumulative tree, by design (AMD-013 v3, Finding A/D) — it exists to catch a
genuine landing conflict between blocks that have **not yet landed**. It has
no legitimate result to produce for a block whose candidate commits are
**already** part of trunk's real history: replaying a declared delta that
predates trunk's actual landing shape (which may be a squash, a rebase, or —
as observed for CM1's stale ledger state during the reversion above — a
declared `base_sha` that fuses in unrelated intervening commits) against a
cumulative tree that already contains that block's differently-shaped landed
change produces conflicts on files the block never touched. For CM1 this
manifested exactly that way: the rehearsal cited two governance files that
appear in neither of CM1's actual commit diffs.

This is a defect in the rehearsal's model, not a nuisance to route around
with a blanket disable. The fix is structural: before replaying a
concurrently-active block's delta, the rehearsal now checks whether that
block's own rehearsal candidate is already an ancestor of the pinned trunk.
If it is, the block has already landed for real — nothing is replayed for
it, a clear `NOTE:` line records why, and the rehearsal moves on. If it is
not, the full replay runs exactly as before, and a genuine conflict between
two blocks that have not yet landed still fails the check. The rehearsal is
not disabled; it is corrected to stop asking a question git's own history
has already answered, while staying live and failing for the case it exists
to catch.

**[CORRECTION, 2026-08-23, `block/ledger-subordinate-to-code`]:** §2 lands
the already-landed-against-pin shortcut only — a candidate that is an
ancestor of the pinned trunk skips replay. This block does **not** claim
the live rehearsal is globally accurate and does **not** make the live
validator exit 0. The remaining live-mode violation (CM1's rehearsal
replay against the pin, which predates CM1's landing so the shortcut does
not fire) is a real concurrent-landing conflict. J1 must rebase. Do not
silence that exit 1, do not advance the pin to the live tip, and do not
read §2's "fixed structurally" / "it is corrected" as a claim that this
block closed rehearsal accuracy.

## 3. Disposition of the three rows

- **CM1 → stays `REVIEW`, review mandates `BLOCKING` (not `PASS`), NOT
  `ACCEPTED`.** `0e5177931` and `13eafb2ab` are both on trunk. The single
  genuine review dispatched against `13eafb2ab` (per the BV2/B5/J1/CM1
  single-lane waiver) returned `BLOCK`, not `PASS` — see
  `evidence/CM1/2026-08-22-acceptance-repair.md`.
  `block/cm1-acceptance-repair` closes the disclosed D3 gap and adopts D1 as
  Vue-faithful against vendored `runtime-core`; its first attempt at the
  `eval`-classification fix (skip poisoning whenever the callee resolved to
  ANY bound symbol) was itself unsound — a bound alias such as `const eval =
  globalThis.eval` is still direct eval — and was corrected on re-review to
  skip poisoning only for a provably fresh, unmutated, unredeclared local
  function binding. That corrected code lands under this ruling.
  **[CORRECTION, 2026-08-23, `block/ledger-subordinate-to-code`]:** the
  preceding sentence is factually superseded — the described `crates/`
  changes (`450abbc4c`) are not on any branch this ruling's own ledger
  correction lands on; they were dropped from `block/ledger-subordinate-to-
  code` (which carries `docs/`/`scripts/` changes only) and ownership of the
  D3/D1/`eval`-classification repair moved to `block/binding-index-owner-
  and-eval`, an independent implementation (not a descendant of `450abbc4c`)
  not yet landed or reviewed. This correction is factual only — it does not
  alter this ruling's disposition of CM1 (`REVIEW`, review mandates
  `BLOCKING`, not `ACCEPTED`, unchanged below) or any other decision here;
  see `program-state.toml`'s CM1 row FOLLOW-ON note for the current
  ownership record. **This
  ruling's operative scope is identity bookkeeping only** (§1's
  `context_packet_digest` grandfather, which does apply to CM1 same as
  BV2/B5) **and does not extend to review outcomes** — see §4: review
  mandates are explicitly untouched. The maintainer's verbatim words ("if
  the code is correct and landed, the ledger should say accepted") are
  conditional; CM1's code is not yet unqualifiedly correct — four real gaps
  remain open and disclosed (below) — so that premise is not met, and no
  override of the genuine `BLOCK` verdict is recorded. The two
  ledger-reversion commits that followed the original repair on that branch
  (`364db87de`, `4f4ab6d39`) correctly reverted `ACCEPTED` → `REVIEW`; that
  reversion is **not** superseded by this ruling and stands. CM1's
  `conformance_review`/`architecture_review`/`adversarial_review` fields
  record the true verdict, `BLOCKING` — never a fabricated `PASS` — with
  `*_reviewed_sha` cleared to `""` per the validator's universal
  review-verdict-identity-binding check (the fact that a real review examined
  `13eafb2ab` is recorded in the row's `notes`, in prose, not in the
  reviewed-SHA fields). There is no review-mandate override grandfather in
  `scripts/validate-program-state.mjs` for CM1 or any other block — the
  ordinary ACCEPTED-gate PASS requirement applies to CM1 exactly as it does
  to every other block, and CM1 cannot reach `ACCEPTED` until it does. Real
  gaps the repair does NOT close remain open, disclosed debt, tracked with an
  owner (a dedicated CM1 corrective follow-up track) and a gate (the normal
  three-mandate review protocol — the single-lane waiver was specific to the
  original CM1/BV2/B5/J1 batch and is not re-invoked for the follow-up): the
  compat surface (`packages/component-meta/test/compat-gaps.test.ts`) still
  asserts the pre-repair `unknown | undefined` regression value; no
  producer-owned typed constructor-identity enum exists (constructor
  identity is still a raw `Arc<str>` spelling folded by string match);
  Finding C's acceptance-matrix cells are proven mostly at
  binding-classification level, not end-to-end through public
  `get_component_meta`/compat output; the `Present -> UnraisableSource`
  fixture is discriminating but does not pin the exact error variant.
- **BV2, B5 → stay `ACCEPTED`**, under the `context_packet_digest`
  grandfather in §1, with genuine `PASS` review mandates unchanged — nothing
  about their acceptance changes.
- All three rows continue to rest on the single independent review lane
  MAINTAINER-RULING-2026-08-22-BV2-B5-J1.md §4 ratified for BV2/B5/J1/CM1 —
  not on three separately-named review mandates each. This ruling does not
  license recording a review that did not happen: BV2's and B5's `PASS`
  values are genuine dispatched-and-passed verdicts, and CM1's row does not
  claim one — it honestly records the `BLOCK` that was actually issued, with
  no override of it.

## 4. What this ruling does not touch

Review mandates and tiering, the literal exit-criteria instruction,
discriminating tests and plant-red-green mutation checks, the required
post-rebase gate, the green-trunk requirement, and Stub Prevention are all
untouched. Every defect actually found and fixed on the way to this ruling —
a false "fails closed" claim, an unimplemented required charter cell, the
`eval` classification bug, an allocator-canary suite that asserted
`count > 0` while hiding a real regression — was found by reviewing **code**.
None was found by validating a digest. This ruling changes what the ledger
demands of the past; it does not lower what review demands of the present.

## 5. Going forward

Bookkeeping produced by future dispatches is a live requirement, not a
grandfather candidate. See `docs/arch/refactor/rev11/rulings/context-packet-
dispatch-procedure.md` (filed alongside this ruling) for the short procedure
that keeps a context packet produced at dispatch time the normal case, so the
next gap is prevented rather than discovered at `REVIEW`.
