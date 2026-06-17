# Svelte conformance-oracle corpus

Vendored `.svelte` fixtures for the native-Svelte compiler conformance oracle.

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
oracle (structure + helper-call topology, NOT bytes), regenerated mechanically
from the pinned `svelte@5.56.3` compiler. They are NEVER hand-edited.

Regenerate / verify:

```bash
node scripts/gen-svelte-goldens.mjs          # rewrite all goldens
node scripts/gen-svelte-goldens.mjs --check  # assert in sync (CI guard)
```

The corpus captures the official **client** + **server** passes now; the SSR
harness and the per-family runtime backends extend the corpus by adding
fixtures here (the generator sweeps the tree, so a new fixture only needs to be
dropped in and the goldens regenerated).
