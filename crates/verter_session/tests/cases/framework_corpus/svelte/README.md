# Svelte framework-surface corpus (hermetic)

Vendored, locally-authored `.svelte` fixtures exercising the Svelte typeinfo
surface adapter end-to-end through `resolve_framework_surface_with_audit`. Every
fixture is self-contained — these tests compile and run with NO third-party
repository checked out alongside this one (the Testing-Hermeticity rule).

Each fixture pairs a `.svelte` source with the surface expectations asserted by
`framework_corpus_svelte.rs` (the corpus test). The corpus covers:

- `runes_props.svelte` — runes `$props()` with a callback prop (PROPS, not EMITS).
- `legacy_export_let.svelte` — legacy `export let` props.
- `bindable_model.svelte` — `$bindable()` MODEL bindings.
- `instance_expose.svelte` — exported instance members (EXPOSE).
- `pure_markup.svelte` — a markup-only component (supported-empty everywhere).

Adding a fixture: drop a `.svelte` file here and add its expectations to the
`CORPUS` table in `framework_corpus_svelte.rs`.
