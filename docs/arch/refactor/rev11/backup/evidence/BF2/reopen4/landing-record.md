# BF2 reopen #4 evidence record

Base: `cf131b4cc` (post reopen #3, accepted). Candidate lands as one squashed commit.

## Reopen triggers

1. BV0's round-1 review found two option-propagation defects in the harness's `compileScript`/
   `compileTemplate` invocation: named ESM import-specifier order was compared positionally (should
   be cosmetic — see the Codex scoping consult at
   `evidence/vue-known-defect-correction/reviews/round2-scope-consult-ruling.md`, Q2), and a
   script-less SFC's `bindingMetadata` was passed as a truthy empty object instead of `undefined`
   (Q4 of the same consult), producing a wrong render-arity golden.
2. Maintainer direction (relayed mid-reopen) established that the existing mapping axis
   (candidate-vs-official-golden source-map field/segment comparison) is structurally unsound: once
   generated JS is allowed to differ cosmetically between compilers (the project's own
   Compiled-Output Conformance rule), a source map's `mappings` field necessarily describes two
   different generated documents, so byte/segment comparison can reject a correct candidate map and
   accept a wrong one. Full investigation:
   `evidence/BF2/reopen4/mapping-oracle-scoping-consult.md`.

## Owned-scope verification (charter `docs/arch/refactor/rev11/charters/BF2.md`)

- Named import-specifier order: `packages/framework-conformance-harness/src/normalize.mjs` now
  canonicalizes only the named-specifier run of an `ImportDeclaration`, content-sorted; membership,
  aliasing, source, default/namespace form and position, import attributes, declaration order and
  grouping, and side-effect sequence all stay structural.
- Script-less `bindingMetadata`: `src/invoke-vue-oracle.mjs` now leaves it `undefined` (not `{}`)
  for a script-less SFC, matching `@vitejs/plugin-vue@6.0.7`.
- `transformAssetUrls` is now passed explicitly to `compileTemplate`, matching the pinned plugin's
  build-mode default (`{ includeAbsolute: true }`), recorded in golden provenance.
- The mapping axis is replaced end to end: `compareMappings`/`mappingsFieldEqual`/
  `CONTRACTUAL_MAP_FIELDS`/`INCIDENTAL_MAP_FIELDS` are deleted (no telemetry survivor — no remaining
  consumer). The new `src/mapping-oracle.mjs` validates a candidate's own map against its own
  generated code and the authored SFC fixture read from disk: map contract/bounds, source identity,
  per-segment truthfulness through a closed, named relation table, completeness/round-trip anchors
  in both directions, fragment-first assembly composition, and structural (parse-based, closed
  runtime-module-set) generated-only range detection so fabricated authored provenance over
  synthesized scaffolding is rejected at the real candidate-acceptance boundary, not only in a unit
  test.
- Full golden corpus regenerated where the fixes changed compiled output or the generation
  implementation's provenance digest; every changed record traced to a named defect (no
  candidate-output-driven expectation update).
- No production compiler code, no `verter_session`, no Svelte file, no
  `packages/vue-conformance-oracle/`, nothing under `crates/verter_vue_conformance/corpus/` touched.

## Review history

Four full review rounds (conformance + architecture + adversarial, mixed CLI models) ran against
this reopen as the mapping-axis replacement grew in scope; each of the first three found real,
reproduced defects that were fixed before the next round:

1. Round 1 found: a false citation in the normalizer contract (claimed parity with the Rust
   comparator it contradicts), an undeclared `transformAssetUrls` default, and a stale comment.
2. Round 2 (first full pass on the mapping-oracle replacement) found: synthetic-range enforcement
   never executed on the real candidate-acceptance path (`syntheticRanges: []` hardcoded), and two
   relations (`verbatim-carry`, `synthesized-local-for-authored-name`) accepted a shifted/wrong
   position as long as the text matched.
3. Round 3 found: the new `destructured-binding-pattern` relation was not actually position-exact
   (brace-span text scan, not a real pattern check), and the generated-only range derivation was
   both over-narrow (missed real compiler scaffolding) and over-broad (swept in authored code with
   coincidentally emitted-shaped spelling).
4. Round 4 (the review-round cap for this reopen) found two narrower, final defects: the
   no-inherited-provenance boundary check was silently disabled on exactly the three range classes
   this round added (reopening the prior round's exploit one column to the left of a covered range);
   the "closed" runtime-module set was implemented as an open namespace-prefix regex; and an
   ObjectPattern property KEY was wrongly classified as a binding position. All three fixed and
   independently re-verified by the orchestrator directly (source-level spot checks, not a fifth
   agent review round) rather than a further dispatched round, per this program's review-round cap.

Final round-4 verdicts: conformance and adversarial both found real defects (fixed in the closing
commit); architecture PASS. No further agent-driven review round was dispatched after round 4; the
orchestrator independently re-verified every round-4 finding's specific fix against the committed
source before landing.

## Disclosed, bounded residuals (not blocking)

- `generatedOnlyRanges`' structural coverage extends to enumerated binding positions and statement
  forms; a compiler-emitted reference occurrence (e.g. a helper call's own identifier used as an
  argument elsewhere) falls back to the non-position-exact `framework-emitted-token`/
  `delimiter-anchor` relations. Measured at 267 of 812 emitted-shaped corpus occurrences. Stated
  plainly in the module header and README, not claimed as covered.
- The boundary (no-inherited-provenance) check is intentionally NOT enforced for ranges that do not
  start their own generated line (helper calls genuinely nested inside a larger legitimately-mapped
  expression) — enforcing it there would reject real official segments. Measured: 193 whitespace-
  prefixed ranges keep the boundary check at zero real-segment cost; the remaining nested-expression
  ranges keep the exemption. Stated in the module doc.
- `map.file` is not validated (no committed golden carries it); recorded in the README rather than
  implemented, since materiality is nil today.

## Required exits (`FC-HARNESS-001`, `FC-MANIFEST-001`, `FC-NORMALIZER-001`, locked performance cells)

- Harness self-tests: 411/411 (`pnpm test` in `packages/framework-conformance-harness`),
  independently re-run by the orchestrator on the final rebased tree.
- `verter_vue_conformance` corpus/generator/normalizer suite: 8/8, independently re-run.
- `node bin/generate-goldens.mjs --check`: OK, 48/48, independently re-run.
- Locked performance cells: unaffected (no change to the measured generation workload's
  input identity).
