# Svelte reference-drift corpus

Vendored `.svelte` fixtures for the Svelte reference-drift gate: the committed
goldens are pinned against the official Svelte compiler's output, so a golden
that drifts from the pinned reference fails. (A Verter-output conformance use of
these same goldens is a follow-up for when the native Svelte codegen lands.)

## Provenance

These fixtures are **hand-authored** representative components — NOT copied
from a third-party repository — so the default `cargo nextest run --workspace`
run is fully hermetic (no external corpus checkout required, per the
`external_corpus_paths_not_present_outside_gated_tests` guard). Each fixture
exercises a feature family the native-Svelte runtime codegen owns.

The corpus is **self-contained**: a fixture that imports another component
(e.g. `components/child_and_snippet.svelte` → `./Child.svelte`) imports a
committed sibling fixture, never an uncommitted file. The generator sweeps the
whole tree, so an imported sub-component gets its own goldens too.

## Layout

One directory per feature family:

| Directory          | Family (helper-matrix rows)                                            |
| ------------------ | ---------------------------------------------------------------------- |
| `runes/`           | `$state` (primitive / object / reassign), `$derived`, `$effect`, `$props` |
| `reactive/`        | reactive text interpolation, dynamic attrs, `class:`/`style:`          |
| `bindings/`        | `bind:value`, `bind:this`, `bind:checked`, `bind:group`                |
| `events/`          | delegated (`onclick`) + native (`oninput`/`onmouseenter`) events       |
| `blocks/`          | `{#if}`/`{:else}`, keyed `{#each}`, `{#key}`, `{#await}`                |
| `components/`      | direct component call (`Child.svelte`), `{#snippet}` + `{@render}`     |
| `special/`         | `<svelte:head>`, `{@html}`                                             |
| `stores/`          | `$store` auto-subscription (`svelte/store`)                            |
| `css/`             | scoped `<style>` + `svelte-<hash>` scope class                        |
| `declaration_tags/`| `{@const}`                                                            |
| `options/`         | `<svelte:options namespace>`                                          |
| `regression/`      | extractor-robustness fixtures (e.g. a literal `$.<ident>` in MARKUP text that must NOT pollute `helperSequence`) |

## Goldens

The committed goldens under `../goldens/` are the NORMALIZED helper-topology
reference (structure + helper-call topology, NOT bytes), regenerated
mechanically from the pinned `svelte@5.56.3` compiler. They are NEVER
hand-edited.

Regenerate / verify:

```bash
node scripts/gen-svelte-goldens.mjs          # rewrite all goldens
node scripts/gen-svelte-goldens.mjs --check  # assert in sync (CI guard)
```

The corpus captures the official **client** + **server** passes now; the SSR
harness and the per-family runtime backends extend the corpus by adding
fixtures here (the generator sweeps the tree, so a new fixture only needs to be
dropped in and the goldens regenerated).

## Generated differential-parity corpus (`generated/`)

The `generated/` subtree (under both `fixtures/` and `../goldens/`) is a
SEPARATE, mechanically-generated differential-parity corpus owned by
`scripts/gen-svelte-diff-corpus.mjs`. The hand-vendored generator
(`gen-svelte-goldens.mjs`) and matrix (`runtime_tests.rs::topology_oracle`)
SKIP this subtree; the differential generator and the
`diff_oracle_tests::generated_differential_matrix_matches_oracle` matrix own it.

The differential generator emits a DETERMINISTIC pairwise/combinatorial set of
minimal `.svelte` fixtures across the topology axes (root kind, text, attributes,
directives, events, blocks, namespace/special contexts) — reactive values come
from `$props()` and components from a static import, so the official compiler
does NOT constant-fold a "dynamic" fixture into a static one. Each fixture is
compiled with the pinned compiler and normalized into an EXPANDED golden schema
that adds, on top of the hand-vendored helper/skeleton/import fields:

- `events` — per registered event: type + target-kind + delegation-kind
  (delegated / direct / forwarded_prop).
- `nonStaticProperties` — the `cannot_be_set_statically` set: name + kind.
- `attrParts` — per dynamic / mixed attribute the emitted value-part topology.
- `nodePaths` — per region, the multiset of node-path step sequences.
- `dynamicSlots` — per-slot-kind dynamic-surface counts.

The Rust matrix projects Verter's runtime IR into the SAME normalized schema and
diffs it; any divergence is an automatically-failing test UNLESS the
`(fixture, axis)` pair is on the honest `KNOWN_DIVERGENCES` allow-list
(`diff_oracle_divergences.rs`), which enumerates the real long tail grouped by
root cause. The `known_divergences_are_real` guard proves every allow-list row
still genuinely diverges (a stale row fails).

Regenerate / verify:

```bash
node scripts/gen-svelte-diff-corpus.mjs          # rewrite the generated corpus
node scripts/gen-svelte-diff-corpus.mjs --check  # assert in sync (CI guard)
```

To rebuild the allow-list after changing the IR projection or the corpus, run
the discovery harness and re-derive the rows:

```bash
cargo test -p verter_compiler --lib enumerate_divergences_discovery -- --ignored --nocapture
```
