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

## Current state (as of this record)

- AMD-008's layer-1/layer-2 split is ratified as a mechanism (once the
  maintainer records ratification), not yet exercised by any real
  implementation — no layer-1 artifact, JavaScript reference, or corrected
  production implementation exists yet.
- The seed vector artifact
  (`packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json`)
  is explicitly marked SEED/incomplete and non-normative pending layer-1
  freeze; one vector (V4) that decided a layer-1-scoped question ahead of that
  freeze is explicitly flagged provisional in-place, not treated as settled.
- This debt row must be cited in Track B's (or whichever track builds BV0A)
  dispatch brief as a required reading before implementation begins.
