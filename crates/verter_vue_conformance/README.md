# verter_vue_conformance

Hermetic official **Vue 3.6 RC** compiler conformance goldens for Verter's VDOM
and Vapor backends, and the structural comparator that checks Verter's shipped
output against them. This crate houses the seed corpus, the VENDORED oracle
artifacts produced by the pinned official Vue RC toolchain, and the OXC
structural comparator + seed conformance suite.

## Oracle topology (non-inline)

The vendored goldens use the official **non-inline** emission — the same
`_sfc_main`-shaped module with a separate `function render` that Verter ships
at runtime (`verter_session::assemble_vue_main_module`). Script-setup cells:
`compileScript({ inlineTemplate: false })` + `compileTemplate({ compilerOptions:
{ bindingMetadata } })` (identical invocation shape for VDOM and Vapor),
assembled host-style; template-only cells get the bundler-equivalent
`const _sfc_main = {}` + attach wrapper. The official `inlineTemplate: true`
production topology is a different, behaviorally equivalent shape Verter does
not emit — comparing against it makes every cell fail on assembly topology,
not real divergence (tracked in
[`docs/arch/next/vue-inline-template-runtime.md`](../../../docs/arch/next/vue-inline-template-runtime.md)).
Vendored `.map.json` files are the compileTemplate maps re-anchored
SFC-absolute (the same line offset the bundler applies).

## Layout

```text
corpus/
├── manifest.json                      # generated: case-id → SFC → per-backend artifacts + disposition
├── cases/<area>/<case>.vue            # seed SFC corpus (32 cases, one focused feature each)
├── support/types.ts                   # locally vendored shared types (hermetic compile reads stay in corpus/)
└── goldens/3.6.0-rc.1/                # vendored official output, version-scoped
    ├── vdom/<area>/<case>.js          # official emitted render/component module (VDOM backend)
    ├── vdom/<area>/<case>.map.json    # its source map
    ├── vdom/<area>/<case>.meta.json   # per-cell metadata (schema, versions, hashes, disposition, diagnostics, helpers)
    └── vapor/…                        # same for the Vapor backend
```

The generator/oracle package lives at `packages/vue-conformance-oracle`
(private, never published). The single pin authority for the toolchain version
is `VUE_ORACLE_VERSION` in `packages/vue-conformance-oracle/vue-golden-lib.mjs`
(currently `3.6.0-rc.1` for `vue`, `@vue/compiler-dom`, `@vue/compiler-sfc`,
and `@vue/compiler-vapor`; `esbuild` is pinned separately for the TS type-strip
post-process).

## Regenerating

```bash
pnpm install                # install the pinned RC toolchain (exact versions, no ranges)
pnpm gen:vue-goldens        # clean regenerate goldens + manifest (the ONLY writer of goldens/)
pnpm gen:vue-goldens:check  # regenerate in-memory; fail on missing / drifted / stale artifacts
```

The generator is **deterministic and idempotent**: sorted case order, fixed
recorded options, no timestamps, no absolute paths. Re-running with the same
pins reproduces byte-identical artifacts (that is exactly what `--check`
verifies).

It is also **hermetic**: the `compileScript` imported-type-resolution `fs` is
replaced with a guard that resolves real paths and denies any read outside
`corpus/` (outside files are invisible; reads throw). Shared support files are
vendored under `corpus/support/`.

## Structural comparator (`src/canon.rs`, `src/compare.rs`, `src/sourcemap.rs`)

The comparator parses BOTH a Verter-emitted module and a vendored official
golden with OXC (pinned `0.126.0`), canonicalizes, and compares the
in-contract dimensions. Waived (cosmetic): whitespace/formatting, redundant
parens, ordinary comments, quote delimiters, empty statements, and the
spelling of private/compiler-generated local bindings — via scope-aware
alpha equivalence (`oxc_semantic` is fail-closed; `BindingKey = (structural
scope identity, declaration ordinal, binding pattern slot, binding kind)`,
references carry the same key, preserving shadowing/closure topology).
Contract (compared exactly): statement/expression/property order, operators,
arguments, string/template/numeric/bigint/regex payloads, static HTML, patch
flags, helper family identity (import aliases keyed by `(source, imported)`,
never declaration order), import sources + side-effect sequence, export
names, source-authored/public/member/label identifiers, semantic comments
(PURE/license/JSDoc/bundler — ordinary comments like `/* TEXT */` dropped)
anchored to their AST occurrence node, source-map original-anchor rows
(generated positions waived), and the ordered diagnostic sequence. Every JS
AST variant is represented; TS/JSX/intrinsic variants are explicitly refused
(no catch-all).

`compare_modules(verter, golden, authored, max_reasons)` returns a bounded
list of structured `DiffReason { dim, path, detail }` (empty ⇒ PASS).
`authored` is the SFC identifier set — source-authored provenance; the
official RC maps ship empty `names`, so the SFC token set is the documented
substitute (conservative-exact).

## Discriminator guard

`vue_structural_conformance_discriminates_cosmetic_from_behavioral_diffs`
(`tests/cases/conformance_discriminator.rs`) proves the comparator on real
goldens with reversible in-memory mutation recipes (plant → require verdict →
restore; each plant proven to have applied): cosmetic mutations (reformat,
parens, ordinary comment, alpha-renames incl. helper aliases) PASS;
behavioral mutations FAIL on their own axis — VDOM (`createElementBlock`→
`createElementVNode`, patch flag `128`→`127`, drop `openBlock`, rename the
exported `_sfc_main` binding, rename member property), Vapor (`setText`→
`setHtml`, setter moved out of `_renderEffect`, retargeted setter binding,
reordered effect setters, `_template` payload/flag, dropped `_delegateEvents`,
`$evtclick` ABI rename), and common (source-authored rename, diagnostics
reorder, PURE-comment drop/move, import source/imported-helper change,
missing source-map row + round-trip control).

## Seed conformance run + tracked dispositions

`tests/cases/seed_conformance.rs` compiles all 32 seed SFCs with Verter for
BOTH backends (`verter_compiler::compile`, `force_vapor` for vapor) and
assembles the runtime Main through the GENUINE shipped pipeline — no hand
copy: `verter_compiler::framework_common::vue_bridge::vue_result_to_runtime_bundle`
(the carrier's real `VerterCompileResult` → `RuntimeCompileOutput`
conversion) → `verter_session::assemble_vue_main_module` (the host's real
assembly). The harness `CompileProfile` sets `is_production: true` so the
assembly omits the bundler-only `__file`/HMR suffixes the compiler-level
oracle lacks (block codegen itself stays dev, matching the official
`compileTemplate` defaults). It compares each cell against the vendored
golden (code + diagnostics + template source-map original anchors, both
sides SFC-absolute). Every cell Verter currently fails is tracked in
`corpus/known-divergences.json` with its exact comparator signature + a
curated note — **the parity backlog**. The suite is green-with-known-gaps: a
new/changed signature fails the suite, and so does a stale entry for a cell
that starts passing (parity improved — remove the entry). Regenerate
signatures after intentional changes:

```bash
VERTER_CONFORMANCE_UPDATE=1 cargo test -p verter_vue_conformance --test main seed_conformance
# review the known-divergences.json diff before committing
VERTER_CONFORMANCE_DEBUG=<case-id> cargo test -p verter_vue_conformance --test main seed_conformance -- --nocapture
```

## Freshness contract (byte-pin pattern)

- Goldens are **committed, never hand-edited**. `gen-vue-goldens.mjs` is the
  only writer.
- Every `.meta.json` records: schema version, ALL package versions, source
  SHA-256, options SHA-256 (canonical summary of the exact compile options),
  code/map SHA-256 + byte length, backend, disposition
  (`compiled` | `rejected`), the ordered diagnostic sequence, helper inventory,
  and generator version. A toolchain or options bump is therefore visible in
  committed metadata.
- The default Rust guards (`cargo test -p verter_vue_conformance --tests`) are
  hermetic: they assert the manifest ↔ corpus bijection, recorded hashes vs
  committed bytes, metadata pin versions vs `vue-golden-lib.mjs`, golden JS
  validity (parsed with OXC), and lockfile pin resolution. They need no Node
  and no Vue runtime.
- The live regenerate-and-compare check is opt-in, mirroring the Svelte
  committed-vs-live split: `pnpm gen:vue-goldens:check` (JS) and
  `cargo test -p verter_vue_conformance --features vue-oracle-live` (Rust
  wrapper, fails loudly when Node/the pinned toolchain is unavailable).
- Bumping the RC: update `VUE_ORACLE_VERSION` (+ the four exact pins) and
  `ESBUILD_VERSION` as needed, `pnpm install`, `pnpm gen:vue-goldens`, and
  commit everything together. The version-scoped `goldens/<version>/` tree
  makes the bump explicit.

## Corpus conventions

- One focused feature per SFC; ids are `path/safe-hierarchical` (directory =
  feature area).
- Each case is compiled for BOTH backends (`vdom` via `@vue/compiler-dom`,
  `vapor` via `@vue/compiler-vapor` with `vapor: true`); per-cell dispositions
  are recorded independently, including official rejections with diagnostics.
- `lang="ts"` script-setup cells are type-stripped with the pinned esbuild
  (`{ loader: "ts" }` only — PURE annotations and the official export shape
  survive), mirroring the official SFC-loader pipeline; the compiler source
  map is chained through the strip. The post-process is recorded in metadata.
  All other cells vendor the raw compiler bytes untouched.
