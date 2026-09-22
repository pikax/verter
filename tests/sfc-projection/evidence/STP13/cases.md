# STP13 evidence index

Candidate source: branch `tama_dag/0.1.0-tama/STP13` worktree (repair turn
over the captured candidate; baseline is the captured STP12 head).

Engine pins: `tests/sfc-projection/STP1/products/engine-matrix.json` (ts-js 6.0.3, ts-native 7.0.2).

Selected case IDs: `STP13-options-this`, `STP13-mixins`, `STP13-combined`, `STP13-shadowed-macro`, `STP13-options-instance`.

Commands and outcomes (run on the candidate in this turn):

- `node scripts/sfc-projection/verify-node.mjs --node STP13 --engine all --require-all --json` — pass; zero errors. Both engines check the positive/negative probes exactly once each, hover `stp13HoverTarget` is exactly `number`, `Instance` is exactly `Comp`, definition `stp13DefinitionTarget` resolves with references and edit participation, and the negative probe carries exactly one diagnostic (TS2551 misspelled-member suggestion for `instance.cout`).
- `cargo test -p verter_compiler --lib ide::vue_projection::options_api -- --test-threads=1` — pass; covers contextual `this` members, plain-object export, local wrapper shadowing, `defineComponent`/`defineOptions` alias-by-binding unwrapping, `ref`-as-`defineComponent` masquerade rejection, mixins/extends/components/directives, identifier-mixins opacity, combined coexistence without template leakage, shadowed-macro reporting, constructor shape, and refusal cases.
- `cargo test -p verter_compiler --test main options_projection_reads_admitted_carrier_blocks -- --test-threads=1` — pass; the production `VueProjectionBackend::options_projection` path runs over real admitted `.vue` carrier bytes (aliased `defineComponent` normal script plus setup script), asserting member extraction, constructor shape, named-export coexistence without template leakage, and foreign-source refusal.
- `cargo fmt --all -- --check` — pass.
- `cargo clippy --workspace --all-targets -- -D warnings` — pass.

Probe expectations: hover `stp13HoverTarget` is exactly `number`; `Instance` is exactly `Comp`; definition `stp13DefinitionTarget` with references and edit participation; negative probe carries exactly one diagnostic (TS2551 misspelled-member suggestion for `instance.cout`).

Probe provenance: `probes/components/*.vue.d.ts` are contract-shape pins for
the constructor-shaped public surface (header-commented as such), not
compiler output. The compiler is exercised physically by the
`options_projection` carrier test above; full carrier-to-declaration
generation stays STP58-owned (atomic Vue activation).

Products: `OptionsComponentProjection`, `CombinedScriptProjection`, `OptionsTemplateBindingView` (each `pub struct`; entries `pub fn project_options_pair` / `pub fn options_projection`).

Implementation home: `crates/verter_compiler/src/ide/vue_projection/options_api.rs`. Dormant consumer: `VueProjectionBackend::options_projection` (products re-exported there for the STP58 activation owner). Vue IDE `project_ide` and the Svelte route are unchanged.

Changed symbols: `vue_runtime_imports` (local-to-exported-symbol map), `is_vue_wrapper` (binding-resolved alias acceptance), `options_object` (map-typed parameter), mixins collection (non-array values mark `has_nonstatic_mixins`).

Deletion population this node: empty. Only duplicate projection-side Options API extraction converges here; runtime Options API lowering stays with the framework compiler.

Why the negative test discriminates: TS2551 fires only when the receiver type is fully known (contextual `defineComponent`-style instance); a broad-`any` constructor or vacuous-`never` success would produce no diagnostic or a different code, failing the anchored negative and the exact Instance/hover types.

Repair findings addressed: F1 binds product validation to exported items
(`pub struct` / `pub fn`) with crate compilation as the physical syntactic
proof; F2 runs the production `options_projection` path over admitted
carrier bytes inside verify-node and marks the `.d.ts` probes as
contract-shape pins; F3/F5 resolve imported `defineComponent` aliases by
binding; F4 rejects `ref`-as-`defineComponent` masquerades by checking the
imported symbol; F6 is this record; F7 marks non-array `mixins` opaque
instead of dropping it. Incremental/cancelled publication and hot-path
ownership are untouched by this projection-only boundary.
