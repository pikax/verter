# BF2 reopen #1 — manifest classification accounting (honest, explicit)

Written as part of item 2 of the reopen fix pass. Records exactly what this
pass did and did not classify in `vue-official-cases.tsv` (2003 rows) and
`svelte-official-cases.tsv` (3457 rows = 3313 `blocked` + 144
`not_applicable`), and why.

## What this pass DID complete for real

Both manifests are 100% **content-verified against the pinned trees** by the
check in `src/coverage-report.mjs` (`reEnumerateVueRows`/`reEnumerateSvelteRows`),
whose exact mechanism is — stated precisely, no more and no less:

- every row's `source_locator` FILE (Vue) or DIRECTORY (Svelte) must be
  tracked/present in the exact pinned Vue 3.6.0-rc.3 / Svelte 5.56.8
  checkout, and
- the row's recorded `source_object` (git blob hash for Vue files, git tree
  object for Svelte sample directories) must equal the LIVE object hash at
  that path in the pinned tree — i.e. the bytes the row was generated from
  are byte-identical to the pinned tree today, and
- `declaration_kind`/`title_kind` must be members of the closed sets the
  manifest generator ever emits, and `title_sha256` must be a well-formed
  64-hex digest.

What this mechanism does **NOT** do: it does not re-parse the file at the
locator's line/column, re-extract the declaration title, or independently
recompute `title_sha256` from source. Because the whole file's bytes are
pinned by the object-hash check, the locator and title-hash remain exactly
as valid as when the generator recorded them — but their initial recording
is trusted from the BF1 generation run, not independently re-derived here.
`test/coverage.spec.mjs` proves the mechanism above for all 2003 + 3457
rows — zero unresolvable — plus corrupted-locator and corrupted-content-hash
negative controls. This is the "every row is reachable and content-pinned,
not silently dropped" half of `FC-MANIFEST-001`.

## What this pass did NOT classify, and why

**Zero rows were reclassified out of `blocked`/`not_applicable` in this
pass.** Every one of the 2003 Vue rows and 3313 Svelte `blocked` rows still
carries `disposition = blocked` and `evidence_id = -`.

This is not an oversight — it reflects a real scope boundary this pass
cannot cross without violating BF2's charter:

- The five closed dispositions are `imported`, `equivalent`, `not_applicable`,
  `unsupported_fail_closed`, `blocked` (`src/coverage-report.mjs`'s
  `VALID_DISPOSITIONS`). Assigning any of the first four to a currently
  `blocked` row requires comparing the official case's behavior/output
  against Verter's OWN candidate compiler output for that exact case —
  that is what distinguishes `imported`/`equivalent` (candidate output
  matches) from `unsupported_fail_closed` (candidate correctly refuses) from
  a row that should stay `blocked` (genuinely unresolved).
- Verter's Vue/Svelte production backends (BV1, BS1) and the case-disposition
  resolution blocks (B2, B3) are explicitly DOWNSTREAM of BF2 in the
  program DAG: `docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md`
  records `B1 -> BF1 -> BF2 -> BF3 -> {B2, B3}` and `{B2, B3} -> B4`, and
  states BF2's charter is "test-only official-core harness, generated-code
  validation/execution, goldens, manifests, coverage, and mutation tests;
  **no production compiler behavior or runtime**." The BF2 charter
  (`docs/arch/refactor/rev11/charters/BF2.md`) states outright: "BF2 cannot
  change production compiler behavior, implement a runtime, patch generated
  output... or let candidate output update expectations."
- Concretely: there is no Verter-compiled candidate artifact for any of
  these 5316 `blocked` rows to compare against. Fabricating a disposition
  without that comparison — e.g. marking a row `equivalent` because its
  locator resolves, or `unsupported_fail_closed` by guessing — would be
  exactly the "classified without real backing evidence" outcome this pass's
  own dispatch prompt and CLAUDE.md's Stub Prevention rule forbid.

## What WAS already correctly classified (pre-existing, not this pass's work)

The 144 Svelte `not_applicable` rows (the `migrate`/`preprocess`/`print`
official-product-boundary suites and non-sample suites like `manual`/
`motion`/`signals`/`store`/`types`) were already classified by BF1's
manifest generator (`generate-official-case-manifests.mjs`'s
`SVELTE_NOT_APPLICABLE`/`SVELTE_NON_SAMPLE_SUITES` maps) on structural
grounds that do NOT require a Verter candidate — they are outside Verter's
compiler product boundary by definition, independent of what any compiler
produces. This pass's re-enumeration re-verified all 144 rows resolve and
carry the correct `not_applicable` disposition; it did not newly classify
them.

## Exact residue

| manifest | disposition | count | evidence_id | resolvable this pass? |
|---|---|---:|---|---|
| Vue | `blocked` | 2003 | `-` | No — needs a Verter Vue candidate; per the manifest's `provisional_owner` column these rows are owned by BV1 (1,494) and B2/BV1 (509), downstream of BF2 |
| Svelte | `blocked` | 3313 | `-` | No — needs a Verter Svelte candidate; per `provisional_owner` these rows are owned by BS1 (2,695), B2/BS1 (589), and BS1/B4 (29), downstream of BF2 |
| Svelte | `not_applicable` | 144 | `-` | Already correctly classified (product-boundary, no candidate needed); `evidence_id` staying `-` is consistent with BF1's original manifest (disposition, not resolution, is the completeness signal for this class) |

**Total unresolved-by-this-pass: 5316 of 5460 rows (2003 + 3313).** This
residue is not silently dropped: it is the exact set the manifests'
`provisional_owner` column assigns to BV1, B2/BV1, BS1, B2/BS1, and BS1/B4
(B3 owns no manifest rows) once those blocks exist, and BF3's charter explicitly covers
retracting any reachable-success claim these rows might otherwise imply. No
row in either manifest was marked with a fabricated `evidence_id` or moved
off `blocked` without a real comparison behind it.

If a maintainer or architecture review determines BF2 SHOULD have narrower
authority to resolve some subset of these rows (e.g. purely structural
dispositions that need no candidate comparison, analogous to the Svelte
`not_applicable` class), that is a scope decision for the charter/AMD-005
owners, not one this implementer pass makes unilaterally.
