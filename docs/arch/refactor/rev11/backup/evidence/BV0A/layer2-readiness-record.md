# Layer-2 vector suite — readiness record

**Artifact:** `packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json`
**Content revision:** 3 (75 total entries: the `vectors` array holds 23, `failClosedVectors` holds
52, of which F7 is itself a composed/non-rejection case placed there because it is an
authored-fragment-inventory boundary case, not a rejection; `knownGaps` records the residual
write-manifest sites W-05/W-13/W-16′ that remain unreached by any vector)

This is **not** the layer-2 freeze itself. Per this file's own `status` block, "freezing is a
maintainer action taken after this file's own independent review" and layer 2 "is FROZEN at BV0A
acceptance" — a distinct act from layer 1's own dedicated pre-implementation freeze, and one this
record does not have the authority to perform on its own. This record establishes that layer 2 is
CONTENT-COMPLETE, independently reviewed to the depth recorded below, and REPRODUCED END TO END BY
BOTH IMPLEMENTATIONS — the state BV0A's own acceptance review needs in hand before it can freeze
layer 2 as part of accepting the whole package.

## Content completeness

Revision 2 closed every item in revision 1's `knownGaps` (recorded in-file under `closedGaps`, six
items, each naming the vectors that closed it) — `knownGaps` was `[]` at revision 2. A dedicated
layer-2-only review round then found revision 2's `knownGaps: []` claim itself overstated (real
layer-1 branches — template-rewrite immunity, §6.4 case 3, a table-less contributing map, `sourceRoot`
absent-vs-null agreement, and several write-manifest sites — had no discriminating vector despite the
empty-gaps claim). Revision 3 closes five of those with new vectors V19–V23 and HONESTLY records the
three still-unreached write-manifest sites (W-05/W-13/W-16′) in a non-empty `knownGaps` rather than
re-claiming completeness — the dedicated review round judged that disclosed, narrow residual
acceptable and materially different from a false empty-gaps claim.

Every `input` is a complete layer-1 §3.3 `AssembleInput` DTO (all fifteen fields present). Every
successful vector's `expected` carries either a complete §7.1 `MapArtifact` plus the ordered
`segments` sequence, or — for the one vector where `sourceMapRequested` is `false` (V17) — the
positively-asserted absence of a map (`map: null`, §7.7). Every fail-closed vector whose rejection is
a validated-map defect uses §4.2's `UncomposableInputMap` shape with an exact §4.4 sub-code; the two
vectors rejecting for a missing REQUIRED map (F8, F9) correctly use §4.2's separate
`MissingRequiredInputMap` shape instead, which carries no §4.4 sub-code — a different taxonomy branch,
not an omission.
`coverage.uncomposableSubCodes` is exhaustive over all 31 sub-codes by construction.

## Both implementations reproduce every entry, count asserted against inventory

This is the specific gap the final conformance review found and blocked on: "Rust does not run all
70 JSON entries ... the exact requirement that both implementations reproduce every vector, with
count asserted against inventory, is therefore not met." Closed:

- **JavaScript reference:** `packages/framework-conformance-harness/test/assembled-map-composition-vectors.spec.mjs`
  reads the vectors file directly (`SUITE.vectors` / `SUITE.failClosedVectors`) and drives every
  entry through `composeAssembledVueMainModule`, per-vector `it()` blocks plus a family-coverage
  sanity test. The driver records every id it actually exercises, and a trailing coverage test
  asserts that recorded set exactly equals the suite's own id inventory (none missing, none extra,
  duplicate ids rejected) — so a vector added to the JSON without an `it()` block that drives it is
  a structural failure naming the missing id, not a silent skip. 77/77 pass (75 vectors + the
  family-coverage test + the coverage-parity test).
- **Production Rust:** `crates/verter_session/src/compile/map_equality_tests/vector_inventory.rs`
  (new) reads the SAME JSON file directly, builds each vector's `AssembleInput` via
  `AssembleInput::from_dto_json` (the exact reverse of the existing bridge's
  `AssembleInput::to_dto_json`), and asserts exact agreement with `production_outcome` — the same
  bridge and comparator the cross-implementation equality suite already uses, not a second DTO
  projection. Three tests: `every_positive_vector_reproduces_its_frozen_expected`,
  `every_fail_closed_vector_reproduces_its_frozen_expected`, and
  `every_vector_in_the_suite_was_exercised`. All three run one shared driver that returns the ids
  it ACTUALLY drove through production and assert that executed-id sequence exactly equals the id
  inventory derived from the loaded arrays themselves — no count is hardcoded anywhere in the
  module, so a driver change that silently skips an entry (not just a suite that grew) fails the
  parity assertion naming the skipped id. All 3 pass. Discrimination independently proven twice:
  (1) a single-character mutation to V1's frozen `expected.code` in the vectors file was planted,
  confirmed present/unique/new, produced the named RED (`production: ... frozen expected: ...` diff
  on V1), then reverted and re-confirmed green with an empty `git diff`; (2) a driver mutation
  silently skipping V15 was planted, confirmed present/unique/new, produced the named RED in both
  the positive-arm parity assertion and `every_vector_in_the_suite_was_exercised`, then reverted
  and re-confirmed green with an empty `git diff`. The JS coverage test received the mirror proof:
  a synthetic 24th positive vector planted into the vectors file (confirmed present/unique/new)
  produced the named RED (`suite vectors never driven through the entry point: V24`), then was
  reverted and re-confirmed green.

Both implementations now reproduce the complete 75-entry inventory, independently, with the
EXECUTED ids (not merely array lengths) asserted against the suite's own inventory rather than a
number either test hardcodes.

## Independent review applied to layer-2 content

**Layer 2 has now received its own dedicated, standalone three-mandate review pass** (conformance,
architecture, adversarial — the same standard layer 1 revision 7 and D-8 each received), distinct from
the whole-package rounds below. Two rounds:

- **Round 1 (all three mandates FAIL, tightly converged).** Each mandate independently built its own
  clean-room implementation from layer 1 alone and confirmed all 70 then-current vectors' `expected`
  values correct — zero incorrect vectors found. The blocking findings were artifact-completeness, not
  correctness: 50 fail-closed vectors lacked the `derivation` AMD-008 §2 item 1 requires, and
  `knownGaps: []` overstated positive coverage (template-rewrite immunity, §6.4 case 3, a table-less
  contributing map, `sourceRoot` absent-vs-null agreement, and several write-manifest sites had no
  discriminating vector). Both architecture and adversarial independently constructed new candidate
  vectors with fully worked, cross-implementation-verified expected values.
- **Fix pass.** All 50 missing derivations added; five new vectors (V19–V23) added closing five of the
  named gaps, reusing the reviewers' own verified candidate content where available; three residual
  write-manifest sites (W-05/W-13/W-16′) honestly recorded in a non-empty `knownGaps` rather than forced.
- **Round 2 (conformance-only recheck, FAIL narrowed to one finding, then closed).** Independently
  re-derived V19–V23 from layer 1 alone before reading their committed derivations; all five `expected`
  values confirmed correct. All 50 new derivations spot-checked as genuine hand-working. The one
  remaining finding was a prose defect in V23's derivation (a claim about `resolveAt`'s cross-line
  behavior that layer 1 §2.3's strict line-scoping does not support) — the `expected` value itself was
  never in question. Fixed directly; the residual `knownGaps` disclosure was explicitly judged
  acceptable (a disclosed, narrow gap is materially different from the original FALSE empty-gaps claim).

Earlier whole-package rounds (architecture, adversarial) had already spot-checked layer 2 as a side
effect of reviewing the whole package — those findings (V8/V14/V18/F8/F24/F35/F41/F44 re-derived and
confirmed; ADV-1/ADV-2/ADV-3 constructed and confirmed; F45/D-8 scope confirmed) are superseded in depth,
not contradicted, by the dedicated rounds above.

## Disposition against the FC-VUE-003 resolution gate

Layer 2 is not itself a check FC-VUE-003 names (that debt row is about layer 1's gate authority
specifically), but AMD-008 §2 item 1's exit condition — "a completed, independently reviewed and
frozen ... literal vector coverage set ... reproduced by both implementations" — is now: completed
(content, with an honestly-disclosed narrow residual), independently reviewed (its own dedicated
standalone rounds, not merely whole-package spot-checks), reproduced by both implementations exactly
with count asserted against inventory, and NOT YET frozen — per this file's own `status` block, freezing
is a maintainer action taken at BV0A acceptance, which this record does not have the authority to
perform on its own.
