# @verter/framework-conformance-harness

Hermetic, **test-only** official-core compiler conformance harness for the
exact pinned Vue `3.6.0-rc.5` and Svelte `5.56.10` compatibility domains
(AMD-005 / BF1 / BF2). This package proves Verter's compiler output can be
falsified against the exact official compilers and runtimes. It never
implements, patches, or ships production compiler behavior.

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
  and the AUTHORED-SOURCE mapping oracle (see below). Independent oracles —
  none of which the normalizer can override. Every axis reports ran/skipped
  status, and the opt-in AUTHORITATIVE mode turns any skipped axis into a
  hard failure.
- **Authored-source mapping oracle** (`src/mapping-oracle.mjs`): validates a
  candidate's source map against the CANDIDATE's own generated code and the
  authored SFC fixture read from disk. No golden map, and no
  official-compiler map, is an input. A candidate-vs-official `mappings`
  comparison cannot be the oracle for this axis: a `mappings` field encodes
  (generated → original) correspondences over ONE specific generated
  document, and Verter's generated JS is legitimately not byte-identical to
  the official compiler's (cosmetic carrier differences are explicitly
  permitted), so the two maps describe different generated documents by
  construction — the comparison rejects correct maps whose layout differs
  and accepts wrong maps that happen to resemble official's. What the oracle
  checks instead: map presence matches the `sourceMap` request in both
  directions; `version` 3 and strictly-decodable VLQ with in-bounds source
  and name indices; generated and original positions in bounds as UTF-16
  code-unit offsets (CRLF and astral-plane characters included); every
  source spelling resolving to the exact authored fixture, and any
  `sourcesContent` equal to that file's real bytes; every source-bearing
  segment classified under a NAMED relation (verbatim carry, or an explicit
  rewrite relation such as a `$setup.<name>` context-binding prefix), with
  an unclassifiable segment a failure rather than a skip; every named
  segment's `names[nameIdx]` entry actually naming its authored or generated
  symbol, not merely being in bounds; per-fixture REQUIRED anchors resolving
  in both directions with exact authored text; and no authored provenance
  over generated-only ranges. `map.file` is deliberately NOT validated — no
  committed golden carries the field, so it has no materiality here; the
  omission is recorded rather than implied. The named relations divide into a
  POSITION-EXACT majority (the mapped original position must BE a specific
  authored lexeme, so re-pointing a segment elsewhere on the same authored
  line breaks it — with `verbatim-carry` position-exact only up to
  identical-lexeme interchangeability, since it is a text-equality relation)
  and three relations — `component-instance-surface`,
  `framework-emitted-token`, `delimiter-anchor` — that constrain only the
  GENERATED side and accept any in-bounds, non-word-interior original
  position; the module comment names which is which.
  The generated-only ranges are DERIVED from the candidate's own parsed
  module, for both Vue and Svelte, so that requirement runs on every
  candidate and cannot be switched off from a call site. **Exactly what they
  cover**, stated as the real boundary: a name is CLAIMABLE when it matches
  the framework profile's emitted-identifier shapes, is not a render-scope
  context root, and is not a word the author wrote in the fixture's own
  script blocks; a claimable name yields a range only in an enumerated
  BINDING position (declarator id, function/catch parameter, pattern target,
  object-pattern property VALUE, function/class declaration id, class member
  key, import-specifier local, object-LITERAL key — never an object-PATTERN
  key, which names a property of the source object and carries authored text)
  or one of the enumerated STATEMENT forms (an import whose specifier is an
  EXACT member of the profile's closed six-string runtime-module set — not a
  namespace prefix, so an authored `svelte/store` or `vue/reactivity` import
  stays mappable in both the named and the side-effect form — a
  member-assignment plumbing statement, a bare or wrapper-call default
  export, a helper call's callee, a claimable identifier passed directly to a
  helper call, a bare claimable return). A range that STARTS ITS OWN
  generated line additionally requires a boundary: nothing may map at the
  column before it, since a consumer resolving the range's start column
  would otherwise inherit that segment's authored provenance. A range
  beginning mid-line is exempt — it sits inside a larger, legitimately
  mapped expression. Everything else is UNCOVERED — most materially compiler
  scaffolding spelled with AUTHORED-shaped identifiers, and the
  non-identifier payload of a synthesized statement (a literal inside
  `$.delegate(['click'])` genuinely can carry authored provenance). Over that
  remainder the rail is `framework-emitted-token` / `delimiter-anchor`, which
  are not position-exact. Published Vue
  golden maps still describe the ASSEMBLED module — both official fragment
  maps (script half, and the render half chained through the descriptor
  block map to whole-file coordinates) re-anchored by the assembly's exact
  geometry (`src/sourcemap.mjs`) — and the assembly is required to be a pure
  coordinate translation of fragment maps that already validated against the
  authored fixture on their own.
- **Candidate acceptance check** (`src/check-candidate.mjs`,
  `bin/check-candidate.mjs`): compares one candidate artifact against one
  committed golden across every axis — parse, structural, diagnostics,
  mapping, link, and runtime execution (server-target goldens execute both
  arms through the pinned runtime and compare rendered HTML). Default
  behavior keeps skip-with-reason semantics for absent environment
  prerequisites; `--authoritative` / `BF2_AUTHORITATIVE=1` is the
  fail-closed contract for consumers that must prove every applicable axis
  genuinely ran (skipped axis ⇒ exit 2).
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
  `vue-official-cases.tsv` (2003 rows) and `svelte-official-cases.tsv` (3475
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
| diagnostic discrimination | `test/diagnostic-mapping-discrimination.spec.mjs` — every contract-observable field independently |
| mapping discrimination (authored-source oracle) | `test/mapping-oracle.spec.mjs` (contract, identity, whole-corpus classification, axis wiring, position binding, candidate-derived generated-only ranges, `names` content) + `test/mapping-oracle-mutations.spec.mjs` (shifted/dropped/mis-encoded positions, UTF-16 + CRLF) + `test/mapping-oracle-composition.spec.mjs` (fragment-first validation, pure-translation assembly, generated-only ranges) |

## What's deliberately NOT built here (honest scope)

This is a bounded, honestly-scoped slice, not a claim of full framework
conformance — see the charter's own "Scale and pacing guidance":

- **Fixture corpus is small (6 fixtures, 48 golden cells)**, not the full
  2003+3475-row official corpus. BF2 owns the harness MECHANISM; resolving
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
BF2_SVELTE_SOURCE=/path/to/svelte-at-56a036f4... \
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
