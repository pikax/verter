# synthetic-15k — the hermetic perf corpus

A committed, version-pinned synthetic corpus of TypeScript-typed Vue SFCs
for the external-TS-engine performance gate (design `docs/arch/external-ts-engine-architecture.md` §2.7).

## Shape

| Dimension            | Value                                                       |
| -------------------- | ----------------------------------------------------------- |
| SFCs                 | 15,000                                                      |
| Modules              | 600 (~25 SFCs/module)                                       |
| Imports / SFC        | ~8 cross-file type imports (props/emits type edges)         |
| Project references   | a `composite` `kernel` project + an `app` project that references it, under a root solution `tsconfig.json` |
| Alias layer          | `baseUrl` + `paths` (`@kernel/*`, `@app/*`)                 |
| lib floor            | `ES2022`, `DOM`, `DOM.Iterable`                             |
| @types floor         | explicit empty `types: []`                                  |
| Total files / bytes  | 15,603 / ~39 MB                                             |

Each SFC carries `<script setup lang="ts">` with `defineProps<T>()` /
`defineEmits<T>()` macros over imported, cross-file prop/emits types, plus a
template that references those props through `v-for` / `v-if` / `:class` /
`@click` — so a tsgo carrier typecheck actually checks the cross-file edges,
the project-reference boundary, and the alias resolution. App SFCs import
kernel types through the `@kernel/*` alias and sibling-module types through
relative specifiers, exercising both resolution modes.

## Generate-on-demand (the committed artifact is the manifest, not the tree)

The materialized corpus is **not committed** — at ~15,603 files it would
bloat the repository and stress the cross-platform path guard. Committed
instead are:

- `generator/generate.mjs` — the deterministic, seeded generator.
- `manifest.json` — the corpus identity: generator version, seed, config,
  tsconfig/project-reference topology, dependency floor, file counts, and a
  **content hash** (`sha256`) over the normalized relative paths + file
  bytes.

A benchmark run regenerates the corpus and verifies the produced bytes hash
to `manifest.json` **before any measurement**. A corpus change therefore can
never silently read as a "perf improvement" — the run refuses to proceed
until the manifest is refreshed in the same change.

```bash
# Materialize the full corpus into ./corpus (gitignored):
node generator/generate.mjs

# Print the content hash without touching disk:
node generator/generate.mjs --hash-only

# A smaller slice for smoke-testing the harness (not the gate corpus):
node generator/generate.mjs --out /tmp/slice --count 200 --modules 20 --composite 4
```

The content hash is deterministic across runs and operating systems: every
emitted relative path AND every path embedded in file content is normalized to
forward slashes, line endings are forced to LF, and the file list is sorted
before hashing — so neither the generating OS's path separator, a CRLF checkout
of the generator, nor directory-walk order affects it. A Windows dev refreshing
the manifest produces the exact hash Linux CI verifies. (`src/perf/corpus.spec.ts`
guards the no-backslash / LF / determinism invariants.)

## Diagnostic profile (a stability invariant, not a zero-error floor)

This is a **performance** corpus: it exercises the full carrier-codegen +
typecheck pipeline at scale. Run standalone (no real `node_modules`/`vue`
install), a carrier typecheck of the corpus emits a **stable, deterministic**
set of diagnostics (the `vue` runtime module, the JSX intrinsic floor, and the
SFC macro globals are unresolved without an install). That is by design and is
fine for the gate: the self-referential regression gate compares a candidate
build against a pinned baseline build on the **same** corpus, asserts the two
diagnostic **sets are equal** (an equivalence invariant), and gates **wall-time
ratios** — none of which requires a zero-error corpus, only a deterministic
one. App↔kernel edges use the `@kernel/*` alias (a real cross-project,
project-reference edge); same-project edges use relative specifiers, so both
resolution modes are exercised and resolve.

## Hermeticity

The corpus depends on **no third-party checkout** — it is generated entirely
from the committed generator. It is independent Verter-owned work,
deliberately comparable in scale to a generic SFC bench (so an offline
cross-tool compiler-throughput run is roughly apples-to-apples) but not a
copy of any third-party generator or corpus.

## Refreshing the corpus identity

A change to the emitted shape MUST bump `GENERATOR_VERSION` in
`generator/generate.mjs` and refresh `manifest.json`:

```bash
node generator/generate.mjs --hash-only   # new hash
# then update manifest.json's contentHash + counts and commit both together.
```

Treat a corpus-identity change like a baseline refresh (see
`packages/benchmark/baselines/README.md`): it is a deliberate, reviewed
change with a before/after note, never an incidental edit.
