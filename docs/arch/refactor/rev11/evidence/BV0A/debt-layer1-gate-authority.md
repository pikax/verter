# Tracked debt — AMD-008 layer-1 freeze lacks independent gate authority and non-retroactive proof

Disposition: **DEFER** (per CLAUDE.md "Explicit finding disposition").

## What happened

AMD-008 redefines BV0A's acceptance criterion as exact ordered map-artifact
equality against an independent, input-only reference, and splits the
composition algebra's specification into two layers: layer 1 (the frozen
semantic specification — DTO schema, validation order, chaining/collision
policy, assembler write/boundary manifest), independently reviewed and frozen
BEFORE either the reference or the production implementation is written
against it; and layer 2 (the literal vector coverage set), an ongoing BV0A
implementation deliverable frozen at acceptance.

Round 10 review (three independent blind mandates — architecture findings 2
and 3, governance finding 1, all converging on the same root) found this split
real and meaningful, but still incomplete in two ways:

1. Layer 1's enumerated payload (DTO, validation order, chaining/collision
   policy, write/boundary *manifest*) is narrower than the umbrella sentence
   describing it (canonical output *schema*, sourceless-boundary *rules*), so
   some semantic decisions — output-field presence, table merge/dedup/remap
   policy, boundary placement behavior — could still fall to layer 2, where a
   candidate settles them for itself.
2. The freeze process requires only three independent reviews plus a recorded
   digest — no maintainer adoption as a lock record, and no requirement to
   prove chronology: an implementer could prototype a reference/production
   implementation first, author layer 1 to match what was already built,
   obtain independent reviews, freeze it, and then "write against" the frozen
   text using the same prototypes — satisfying the letter of "before either
   implementation is written against it" without the substance.

This is the same class of problem this program already ruled on for BF2's
performance-gate: gate authority must be separated from implementation, and
review approvals alone cannot create that authority — only the maintainer can
(governance §1.1). See `evidence/BF2/debt-BF2-perf-gate-deferred.md` for the
precedent's full reasoning and the elaborate bootstrap-authority protocol a
prior consult proposed and the maintainer declined to invoke.

## Ruling reference

- Round 10 review, three independent blind Codex xhigh dispatches
  (`--sandbox read-only`), 2026-08-14. Full verdicts:
  `docs/arch/refactor/rev11/evidence/BV0A/amd-008-round10-reviews.md`.
- Maintainer decision (via the program orchestrator, conversational ratification
  exchange): accept this finding as recorded residual risk rather than pursue
  a further fix-and-review cycle. The core acceptance criterion (ordered
  map-artifact equality, `CodeTransform` semantics, deleted violation
  attribution) has been stable and independently confirmed since round 3; this
  finding is about proving PROCESS chronology for a not-yet-built artifact,
  which prose in an amendment cannot fully close regardless of how it is
  worded — the actual proof is in how BV0A's implementation is later conducted
  and reviewed, not in this text.

## Durable owner

**BV0A's own acceptance review** — the same review that must independently
confirm the completed layer-1 specification, the completed layer-2 vector
suite, the independent JavaScript reference, and the production Rust
implementation before BV0A can be accepted at all (AMD-008 §5, "a candidate
implementing this amended charter ... must separately receive fresh
conformance, architecture, and adversarial review"). That review is the
natural, and only practical, point to close this gap, because only at that
point do real commit/tree identities for layer 1, the reference, and
production all exist to be checked against each other for chronology and
scope-completeness.

## Resolution gate

Before BV0A's acceptance review closes, its reviewers (particularly the
adversarial/governance mandate) must additionally verify, and record verbatim:

1. **Layer-1 completeness.** That the frozen layer-1 specification actually
   contains every semantic decision the umbrella description promises — output
   field presence/policy, table merge/deduplication/remapping rules, and
   boundary placement behavior — not merely the manifest/DTO/policy categories
   AMD-008 names as illustrative. Any semantic gap found must be closed by a
   further amendment to layer 1 (not silently absorbed into layer 2 coverage)
   before BV0A can rely on it.
2. **Non-retroactive chronology.** That the layer-1 freeze commit/digest
   predates, by recorded commit ancestry, the independent JavaScript
   reference's and the production implementation's own commits. Commit
   ancestry proves commit ORDER, not when code was actually authored or
   whether layer 1 was itself derived from an existing prototype — so ancestry
   alone does not close this check. DISQUALIFYING, not merely
   assessed: any pre-freeze prototype, draft, or exploratory implementation of
   the reference or production composer — including any code path traceable
   to the superseded `work/bv0a-implementation` candidate — may NOT be reused,
   adapted, rebased, or referenced as evidence for the post-freeze reference or
   production implementation, matching the BF2 precedent's explicit
   prohibition on reusing invalid candidate-derived evidence
   (`evidence/BF2/debt-BF2-perf-gate-deferred.md`, "It may NOT reuse BF2's
   invalidated ... session or its derived numbers as inputs"). The acceptance
   reviewer must obtain and record an explicit developer attestation (or,
   preferably, verify directly via full commit history) that no such reuse
   occurred, not merely note that ancestry looks correct. A reviewer unable to
   rule out prototype reuse must treat this check as UNMET, not as
   inconclusive-but-acceptable.
3. **Maintainer adoption.** That the exact layer-1 commit/blob identity is
   itself recorded in the ledger/evidence trail as adopted, not merely
   "independently reviewed" — closing the specific gap governance finding 1
   named (no maintainer lock record for layer 1 itself, distinct from BV0A's
   eventual acceptance).

If any of these three checks fails at BV0A's acceptance review, that review
must treat it as a genuine BV0A-blocking defect (not a debt to defer further)
— this deferral covers ONE round of not-yet-provable-in-prose risk between
AMD-008's ratification and BV0A's first real candidate; it does not license
deferring the same question indefinitely.

## Acceptance ID

`FC-VUE-003` — "BV0A's composition-equality gate has independently
maintainer-adopted authority over its semantic specification (layer 1),
provably prior to and independent of both the reference and production
implementations checked against it." Not satisfied by AMD-008's ratification
alone. Owned by BV0A's own acceptance review, per the resolution gate above.

## Resolution gate — verbatim disposition (supersedes "Current state" below)

This section is the definitive, dated closure record the resolution gate above requires —
"record verbatim," not merely assert — covering layer 1 in FULL (revisions 1–7's original
freeze AND revision 8/D-8's later amendment together, since both are now live authority a
BV0A candidate is checked against).

**Check 1 — Layer-1 completeness.** MET. `layer1-freeze-adoption.md` maps every requirement
of AMD-008's umbrella description and this debt record's own text to the layer-1 section that
answers it (its own §10 cross-reference), independently confirmed by the program orchestrator
before that record was written. Revision 8/`DECISION` D-8 is a narrow, scoped, independently
reviewed addendum (two full review rounds — see check 3) that does not reopen this check for
the surrounding revision 1–7 text.

**Check 2 — Non-retroactive chronology.** MET, independently re-verified on the actual
candidate tree by THREE separate review rounds, not asserted once and carried forward:

- Architecture (final package review): confirmed via direct `git merge-base --is-ancestor`
  that all eight superseded-attempt commits (`26f1dae9d`, `db26cde00`, `ddcc255ba`,
  `e7ca0e68b`, `1493b158e`, `64cfe9777`, `2b08cddd7`, `32efc149b`) are non-ancestors of the
  candidate tip; confirmed the layer-1 freeze commit is the direct git parent of both
  implementations' root commits, with wall-clock timestamps corroborating (freeze at
  20:42:59, both implementations starting ~90 minutes later).
- Adversarial (final package review): independently re-derived the same non-ancestor result
  for all eight commits plus both superseded branch tips (`work/bv0a-implementation`,
  `work/bv0-relanding`); confirmed zero occurrences of identifiers unique to the superseded
  attribution-matching design (`violation_key`, `probe_fragments`, `violation_multiset`) in
  the candidate diff.
- Conformance (fresh review round, commit `115f176dc`): independently re-confirmed at the
  D-8 layer specifically — both implementations were already built against revision 7 and had
  independently exposed the `U8.1` gap BEFORE D-8 was drafted, so D-8 could not have shaped
  either implementation's initial construction; separately confirmed (per the mandated
  program-wide exclusion review) that BV0A's own 19 candidate commits touch none of the files
  carrying pre-existing machine-path markers, closing a parallel chronology question raised in
  that same round.

No round found reuse of the superseded candidate's code paths, design, or derived evidence.
This satisfies the resolution gate's explicit requirement to verify DIRECTLY via commit
history (achieved, repeatedly, across independent reviewers) rather than merely noting
ancestry looks correct.

**Check 3 — Maintainer adoption.** MET. `layer1-freeze-adoption.md` (revisions 1–7, blob
`0ea47424acfbd4913e11f16156baa597216c84fb`) and `layer1-d8-adoption.md` (revision 8/D-8, blob
`085139c5267136ed0c2fa39d78ad48168c6e0e76`) are both present on the accepted candidate tree,
each recording the exact adopted commit/blob identity distinct from ordinary review approval —
closing the specific gap the original governance finding named (no maintainer lock record for
layer 1 itself). Both records are independently re-verified as present, correctly cited, and
digest-matched by the architecture, adversarial, and conformance rounds above.

**Disposition: FC-VUE-003 is satisfied for layer 1 as a whole (revisions 1–8).** This debt row
is CLOSED as of this record. It does not extend to layer 2, which is a separate artifact under
its own acceptance track (`layer2-readiness-record.md` and, once complete, its own dedicated
independent review — the same standard layer 1 itself received, not the incidental spot-checks
layer 2 has absorbed as a side effect of whole-package reviews).

## Current state (historical — superseded by the resolution gate disposition above)

- AMD-008's layer-1/layer-2 split has been exercised by a real candidate.
  Layer 1 is adopted through revision 8 (`layer1-freeze-adoption.md` for
  revisions 1–7, `layer1-d8-adoption.md` for D-8). The independent JavaScript
  reference and the corrected production Rust implementation both exist and
  both reproduce the complete layer-2 vector suite (70/70 entries each),
  cross-implementation equality holds with zero divergence, and the 36-cell
  BF2 matrix passes against a genuinely historical code-byte baseline (see
  `historical-baseline-provenance.md`).
- The seed vector artifact is content-complete (`knownGaps: []`) and reproduced
  end to end by both implementations; see `layer2-readiness-record.md` for its
  own readiness status (freezing itself remains a maintainer action taken at
  BV0A acceptance, not self-declared by that record).
