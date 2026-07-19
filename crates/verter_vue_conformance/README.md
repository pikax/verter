# verter_vue_conformance

Hermetic official **Vue 3.6 RC** compiler conformance goldens for Verter's VDOM
and Vapor backends. This crate houses the seed corpus and the VENDORED oracle
artifacts produced by the pinned official Vue RC toolchain; the structural
comparator that checks Verter output against these goldens is a later slice.

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
