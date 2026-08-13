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
  with the exact pinned official compilers. Package identity is verified at
  five independent layers — resolved direct versions, evidence-lock byte
  integrity, evidence-lock content cross-check, closure-evidence byte
  integrity, and full TRANSITIVE-closure derivation cross-check (every
  nested package path/name/version/integrity/edge re-derived from the
  committed lockfile and compared against the committed `closure.tsv`) —
  before a single line of the oracle compiler runs (`src/package-pin.mjs`,
  `src/closure-verify.mjs`, `src/domain-pin.mjs`). The REALIZED half — the
  committed lockfile actually installing to exactly the recorded closure —
  is proven by a disposable, scripts-disabled, network-denied install whose
  tree is independently enumerated (`test/closure-drift.spec.mjs`).
- **Fragment validation** (`src/fragments.mjs`): every declared fragment
  kind (Vue `script`, Vue `render`, Svelte `module`) has its OWN syntactic
  contract validated independently of assembled-module parsing — the two
  signals are proven independent in both directions
  (`test/fragment-validation.spec.mjs`), so assembled-parse success never
  stands in for fragment validity.
- **Immutable, atomically-published golden generation**
  (`bin/generate-goldens.mjs`, `src/golden-store.mjs`): produces 48 golden
  records with full provenance (source commit/tree, package-lock digest,
  generator digest, fixture digest, normalizer version + implementation
  digest, options, environment, raw/normalized digests) under `goldens/` as
  content-addressed record files named by ONE atomically-replaced
  `manifest.json` — a run that dies mid-generation leaves the entire
  previous set observable, never a partial one. This is the ONLY script
  that writes there; candidate output is never an input to it.
- **Parser-backed cosmetic normalizer** (`src/normalize.mjs`): re-parses
  generated code to an ESTree AST and produces a position-free canonical
  form. Whitespace, quote spelling, and redundant parentheses are free
  consequences of AST-level comparison. **Identifiers are structural — no
  alpha-renaming**: the pinned official compilers emit no explicit
  private-generated provenance marker, so no binding is ever renamed away
  (a candidate spelling any binding differently from the official output is
  a structural difference). Tool-consumed comments (`/*#__PURE__*/`-class
  annotations, license/preserve, sourceMappingURL/sourceURL, TS directives,
  triple-slash references, JSDoc) are classified, preserved, and attached
  to the node they precede — deleting, mutating, or relocating one is a
  structural difference; plain prose comments stay cosmetic.
- **Structural comparator** (`src/compare.mjs`): parse validity; FULL
  linking-surface validation against the real installed pinned packages
  (named/default/namespace/side-effect imports, module-load failure,
  `export … from` / `export * from` re-export sources, exact-package
  identity — the resolved module must be the exact pinned version, and
  imports outside the pinned closures fail); structural topology (via the
  normalizer); full-field diagnostic discrimination (kind, code, full
  message chain, source, start AND end spans, related information, order);
  and full-field source-map comparison over every contractual field
  (`version`, `mappings`, `sources`, `sourceRoot`, `sourcesContent`,
  `names`; `file` is the one explicitly-classified incidental field).
  Independent oracles — none of which the normalizer can override.
- **TypeScript-observation validator** (`src/typescript-observe.mjs`): the
  reusable mechanism for TypeScript-observable product validation — drives
  the real pinned `typescript` compiler over produced artifacts in memory
  and captures what TS observes (every export's checker-assigned type,
  structurally expanded so a named type cannot hide member drift, plus
  full-field diagnostics). A prop type that silently changes with zero
  diagnostics is caught (`test/typescript-observation.spec.mjs`). BF2
  supplies the MECHANISM only; the downstream candidate-producing blocks
  own actual product conformance results using it.
- **Deterministic runtime execution** (`src/execute-vue-runtime.mjs`,
  `src/execute-svelte-runtime.mjs`): SSR execution against the real official
  `@vue/server-renderer` / `svelte/server` runtimes. `test/failure-detection.spec.mjs`
  exercises both `executeVueSsr` and `executeSvelteSsr` with a real
  throws-on-error arm and a real succeeds-on-real-SSR arm each.
- **Hydration control pairing** (`src/hydration.mjs`): official server /
  official client hydration in a deterministic jsdom environment, for both
  frameworks — the harness-control pairing `ssr-hydration.md` names.
  `test/hydration.spec.mjs` drives both `hydrateVue` and `hydrateSvelteClient`
  against real official-compiled server + client artifacts (pairing #1), plus
  a negative-control arm each.
- **Official-case coverage accounting** (`src/coverage-report.mjs`,
  `bin/coverage-report.mjs`): verifies every row of the BF1-ratified
  `vue-official-cases.tsv` (2003 rows) and `svelte-official-cases.tsv` (3457
  rows) against the pinned source checkouts. Exact mechanism: per-row
  path presence PLUS git object-hash content identity (the file/directory
  bytes the row was generated from are byte-identical to the pinned tree)
  plus closed-set/format field validation. It does NOT re-extract each
  declaration or recompute title hashes — the whole-file content pin is
  what keeps the recorded locator/title-hash valid; their initial recording
  is BF1's generation run.
- **Atomic result publication** (`src/result-writer.mjs`): every harness run
  that produces a result artifact publishes all-or-nothing; the golden SET
  additionally publishes through the single manifest commit point above.

## Required exits — status

| exit | status |
|---|---|
| `FC-HARNESS-001` (hermetic invocation/validation/execution/mutations) | satisfied for the bounded fixture corpus below; self-tests in `test/` incl. transitive-closure drift (`closure-drift.spec.mjs`) and the TypeScript-observation validator (`typescript-observation.spec.mjs`) |
| `FC-MANIFEST-001` (every official case has one disposition) | satisfied — every row carries a BF1-ratified disposition; 100% of both manifests are content-verified against the pinned checkouts (`test/coverage.spec.mjs`; see the exact-mechanism wording above) |
| `FC-NORMALIZER-001` (cosmetic-only, discriminating) | satisfied — `test/normalizer-mutations.spec.mjs` covers every forbidden category re-derived from `contracts/conformance-normalizer.md`, plus semantic-comment preservation and positive cosmetic pairs, each mutation proven applied |
| source/package drift refusal | `test/drift-refusal.spec.mjs` + `test/closure-drift.spec.mjs` — real mutated copies at all five pin layers plus realized-install enumeration |
| offline execution | `test/offline-execution.spec.mjs` — portable (poisoned globals) + macOS operational (`sandbox-exec`) proofs |
| non-vacuous official/candidate arms | `test/non-vacuous-arms.spec.mjs` |
| expected-golden immutability | `test/golden-immutability.spec.mjs` — structural (no write export) + operational (manifest and record bytes unchanged after many divergent comparisons) |
| parse/link/runtime failure detection | `test/failure-detection.spec.mjs` + `test/link-surface.spec.mjs` + `test/fragment-validation.spec.mjs` |
| atomic result accounting | `test/atomic-result-accounting.spec.mjs` + `test/atomic-golden-set.spec.mjs` |
| diagnostic/mapping discrimination | `test/diagnostic-mapping-discrimination.spec.mjs` — every contract-observable field independently |

## What's deliberately NOT built here (honest scope)

This is a bounded, honestly-scoped slice, not a claim of full framework
conformance — see the charter's own "Scale and pacing guidance":

- **Fixture corpus is small (6 fixtures, 48 golden cells)**, not the full
  2003+3457-row official corpus. BF2 owns the harness MECHANISM; resolving
  every official case's actual disposition (imported/equivalent/etc.) is
  explicitly the later blocks' work (per the manifest's own
  `provisional_owner` column) — most manifest rows legitimately stay
  `blocked` pending that work. This harness proves every row is reachable
  and content-pinned and that the generate → normalize → compare → execute
  → hydrate pipeline works end to end on real official-core artifacts; it
  does not itself attempt to resolve the bulk corpus.
- **Hydration pairings #2 ("Verter server/Verter client") and #3 ("official
  server/Verter client")** are not exercised — they require real
  Verter-compiled candidate output in this exact assembled shape, which does
  not exist yet at this point in the program. `hydrateVue`/`hydrateSvelteClient`
  are pluggable entry points the downstream backend blocks can drive
  through once that candidate output exists. Pairing #1 (official/official,
  the harness control) IS exercised — `test/hydration.spec.mjs` drives both
  entry points against real official-compiled server + client artifacts.
- **TypeScript-observable product CONFORMANCE RESULTS** are not produced
  here. The validator MECHANISM (`src/typescript-observe.mjs`) is built and
  self-tested with planted observation drifts; running it against real
  Verter candidate products and judging conformance belongs to the
  downstream candidate-producing blocks.
- **Link validity** resolves bare package specifiers (what generated
  fragments actually emit); relative-specifier resolution across a real
  fragment/assembly graph is production fragment-placement scope
  (`fragment-assembly.md`), not built here.

## Running

```sh
pnpm install --filter @verter/framework-conformance-harness...
pnpm --filter @verter/framework-conformance-harness test

# golden regeneration (only ever run by a human/CI reviewing a real change):
pnpm --filter @verter/framework-conformance-harness generate-goldens
pnpm --filter @verter/framework-conformance-harness check-goldens

# one-time provisioning (the ONLY network-touching steps; never run by tests):
pnpm --filter @verter/framework-conformance-harness provision-oracles      # pinned git checkouts
node scripts/provision-oracle-npm-cache.mjs                                 # offline npm cache for the realized-closure proof

# official-case coverage report (structural accounting always runs; content
# verification against the pinned git checkouts requires the local clones):
BF2_VUE_SOURCE=/path/to/vue-core-at-3adb2257... \
BF2_SVELTE_SOURCE=/path/to/svelte-at-44a78137... \
pnpm --filter @verter/framework-conformance-harness coverage-report
```

`BF2_VUE_SOURCE`/`BF2_SVELTE_SOURCE` must be local git checkouts pinned at
the exact commits in `src/domain-pin.mjs` (clean working tree); without
them (or the default provisioned `.oracle-checkouts/`), the
checkout-dependent self-tests SKIP with an explicit reason. Without the
provisioned `.oracle-npm-cache/`, the realized-closure install self-tests
SKIP with an explicit reason. Never a silent or fabricated pass.

Oracle realization itself (`ensureOracleDomain`) is offline-only and
fail-closed: with the cache provisioned it installs via
`npm ci --offline` from that cache exclusively; without it, realization
REFUSES with an actionable `OracleCacheUnprovisionedError` naming the
provisioning command — it never silently falls back to a networked
`npm ci`.
