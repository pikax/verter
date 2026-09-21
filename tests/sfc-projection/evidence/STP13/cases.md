# STP13 evidence index

Engine pins: `tests/sfc-projection/STP1/products/engine-matrix.json` (ts-js 6.0.3, ts-native 7.0.2).

Selected case IDs: `STP13-options-this`, `STP13-mixins`, `STP13-combined`, `STP13-shadowed-macro`, `STP13-options-instance`.

Commands (run on the candidate; this file is not an execution transcript):

- `node scripts/sfc-projection/verify-node.mjs --node STP13 --engine all --require-all --json`
- `cargo test -p verter_compiler --lib ide::vue_projection::options_api`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

Probe expectations: hover `stp13HoverTarget` is exactly `number`; `Instance` is exactly `Comp`; definition `stp13DefinitionTarget` with references and edit participation; negative probe carries exactly one diagnostic (TS2551 misspelled-member suggestion for `instance.cout`).

Products: `OptionsComponentProjection`, `CombinedScriptProjection`, `OptionsTemplateBindingView`.

Implementation home: `crates/verter_compiler/src/ide/vue_projection/options_api.rs`. Dormant consumer: `VueProjectionBackend::options_projection` (products re-exported there for the STP58 activation owner). Vue IDE `project_ide` and the Svelte route are unchanged.

Deletion population this node: empty. Only duplicate projection-side Options API extraction converges here; runtime Options API lowering stays with the framework compiler.

Why the negative test discriminates: TS2551 fires only when the receiver type is fully known (contextual `defineComponent`-style instance); a broad-`any` constructor or vacuous-`never` success would produce no diagnostic or a different code, failing the anchored negative and the exact Instance/hover types.
