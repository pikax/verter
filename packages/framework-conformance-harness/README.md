# @verter/framework-conformance-harness

Hermetic, **test-only** official-core compiler conformance harness for the
exact pinned Vue `3.6.0-rc.3` and Svelte `5.56.8` compatibility domains
(AMD-005 / BF1 / BF2). This package proves Verter's compiler output can be
falsified against the exact official compilers and runtimes. It never
implements, patches, or ships production compiler behavior — see
`docs/arch/refactor/rev11/charters/BF2.md` and the contracts under
`docs/arch/refactor/rev11/contracts/` for the normative rules this package
follows.

## What this package does

- **Hermetic official invocation** (`src/invoke-vue-oracle.mjs`,
  `src/invoke-svelte-oracle.mjs`): compiles independently-authored fixtures
  with the exact pinned official compilers. Package identity is verified
  (three independent layers — resolved version, evidence-lock byte
  integrity, evidence-lock content cross-check) before a single line of the
  oracle compiler runs (`src/package-pin.mjs`, `src/domain-pin.mjs`).
- **Immutable golden generation** (`bin/generate-goldens.mjs`): produces 48
  golden JSON records with full provenance (source commit/tree, package-lock
  digest, generator digest, fixture digest, normalized options, environment,
  raw/normalized digests) under `goldens/`. This is the ONLY script that
  writes there; candidate output is never an input to it.
- **Parser-backed cosmetic normalizer** (`src/normalize.mjs`): re-parses
  generated code to an ESTree AST and produces a position-free,
  scope-aware-alpha-renamed canonical form. Whitespace, quote spelling, and
  redundant parentheses are free consequences of AST-level comparison;
  private local identifier renaming is the one explicit transform. Imports/
  exports, literal values, property keys, and statement order are never
  touched.
- **Structural comparator** (`src/compare.mjs`): parse validity, real-package
  import/link validity, structural topology (via the normalizer), diagnostic
  discrimination, and source-map presence/content comparison — independent
  oracles, none of which the normalizer can override.
- **Deterministic runtime execution** (`src/execute-vue-runtime.mjs`,
  `src/execute-svelte-runtime.mjs`): SSR execution against the real official
  `@vue/server-renderer` / `svelte/server` runtimes. The failure-detection
  self-test currently exercises the Vue path (`executeVueSsr`) only;
  `executeSvelteSsr` is implemented and real but not yet self-tested.
- **Hydration control pairing** (`src/hydration.mjs`): official server /
  official client hydration in a deterministic jsdom environment, for both
  frameworks — the harness-control pairing `ssr-hydration.md` names.
  Implemented, pluggable, and not yet exercised by this package's own tests
  (see the required-exits note below).
- **Official-case coverage accounting** (`src/coverage-report.mjs`,
  `bin/coverage-report.mjs`): re-enumerates every row of the BF1-ratified
  `vue-official-cases.tsv` (2003 rows) and `svelte-official-cases.tsv` (3457
  rows) against the pinned source checkouts, proving every declared case is
  reachable/runner-enumerable, not silently dropped.
- **Atomic result publication** (`src/result-writer.mjs`): every harness run
  that produces a result artifact publishes all-or-nothing.

## Required exits — status

| exit | status |
|---|---|
| `FC-HARNESS-001` (hermetic invocation/validation/execution/mutations) | satisfied for the bounded fixture corpus below; self-tests in `test/` |
| `FC-MANIFEST-001` (every official case has one disposition) | satisfied — every row already carries a BF1-ratified disposition; 100% of both manifests are additionally runner-re-enumerable against the pinned checkouts (`test/coverage.spec.mjs`) |
| `FC-NORMALIZER-001` (cosmetic-only, discriminating) | satisfied — `test/normalizer-mutations.spec.mjs` covers every forbidden category from `contracts/conformance-normalizer.md` plus positive cosmetic pairs, each mutation proven applied |
| source/package drift refusal | `test/drift-refusal.spec.mjs` — real mutated copies at all three pin layers |
| offline execution | `test/offline-execution.spec.mjs` — portable (poisoned globals) + macOS operational (`sandbox-exec`) proofs |
| non-vacuous official/candidate arms | `test/non-vacuous-arms.spec.mjs` |
| expected-golden immutability | `test/golden-immutability.spec.mjs` — structural (no write export) + operational (bytes unchanged after many divergent comparisons) |
| parse/link/runtime failure detection | `test/failure-detection.spec.mjs` |
| atomic result accounting | `test/atomic-result-accounting.spec.mjs` |
| diagnostic/mapping discrimination | `test/diagnostic-mapping-discrimination.spec.mjs` |

## What's deliberately NOT built here (honest scope)

This is a bounded, honestly-scoped slice, not a claim of full framework
conformance — see the charter's own "Scale and pacing guidance":

- **Fixture corpus is small (6 fixtures, 48 golden cells)**, not the full
  2003+3457-row official corpus. BF2 owns the harness MECHANISM; resolving
  every official case's actual disposition (imported/equivalent/etc.) is
  explicitly the later blocks' work (BV1/BS1/B2/B3 per the manifest's own
  `provisional_owner` column) — most manifest rows legitimately stay
  `blocked` pending that work. This harness proves every row is reachable
  and that the generate → normalize → compare → execute → hydrate pipeline
  works end to end on real official-core artifacts; it does not itself
  attempt to resolve the bulk corpus.
- **Hydration pairings #2 ("Verter server/Verter client") and #3 ("official
  server/Verter client")** are not exercised — they require real
  Verter-compiled candidate output in this exact assembled shape, which does
  not exist yet at this point in the program (BV1/BS1 are downstream of BF2
  in `program-dag.toml`: `B4 -> {BV1, BS1}`, while BF2's only predecessor is
  BF1). `hydrateVue`/`hydrateSvelteClient` are pluggable entry points BV1/BS1
  can drive through once that candidate output exists. Pairing #1
  (official/official, the harness control) is implemented but not yet
  exercised by any test or CLI path in this package — it will be driven for
  real once a BV1/BS1 consumer calls it, the same as pairings #2 and #3.
- **TypeScript-observable product conformance** (`FC-TS-001`,
  `typescript-product-conformance.md`) is not covered by this package — it
  names a distinct oracle (the TypeScript compiler/API) outside this
  package's Vue/Svelte-compiler scope.
- **Link validity** only resolves bare package specifiers (what generated
  fragments actually emit); relative-specifier resolution across a real
  fragment/assembly graph is B4-owned scope (`fragment-assembly.md`), not
  built here.

## Running

```sh
pnpm install --filter @verter/framework-conformance-harness...
pnpm --filter @verter/framework-conformance-harness test

# golden regeneration (only ever run by a human/CI reviewing a real change):
pnpm --filter @verter/framework-conformance-harness generate-goldens
pnpm --filter @verter/framework-conformance-harness check-goldens

# official-case coverage report (structural accounting always runs; runner
# re-enumeration against the pinned git checkouts requires local clones):
BF2_VUE_SOURCE=/path/to/vue-core-at-3adb2257... \
BF2_SVELTE_SOURCE=/path/to/svelte-at-44a78137... \
pnpm --filter @verter/framework-conformance-harness coverage-report
```

`BF2_VUE_SOURCE`/`BF2_SVELTE_SOURCE` must be local git checkouts pinned at
the exact commits in `src/domain-pin.mjs` (clean working tree). Without
them, the checkout-dependent self-tests SKIP with an explicit reason —
never a silent or fabricated pass.
