# STP19 evidence index

Engine pins: `tests/sfc-projection/STP1/products/engine-matrix.json`. Vue pin: `vue@3.6.0-rc.5`.

Selected case IDs: `STP19-explicit`, `STP19-higher-rank`, `STP19-forward`, `STP19-overloads`, `STP19-foreign`, `STP19-erasure`, `STP19-instantiation-alias`.

Commands (run on the candidate; this file is not an execution transcript):

- `node scripts/sfc-projection/verify-node.mjs --node STP19 --engine all --require-all --json`
- `cargo test -p verter_compiler --lib ide::vue_projection::generic_interop`
- `cargo test -p verter_compiler --test main advanced_generic_uses_reads_admitted_carrier_blocks`
- `node --test --test-name-pattern "STP18|STP19" scripts/sfc-projection/verify-node.test.mjs`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

Products: `AdvancedGenericUseProjection`, `ForeignComponentContractAdapter`.

Implementation home: `crates/verter_compiler/src/ide/vue_projection/generic_interop.rs`. Dormant consumer: `VueProjectionBackend::advanced_generic_uses`, derived from the same `ProjectionPlan`, attribute-operation and component-use products of the admitted parse plus the carrier's authored `generic` binder. Vue IDE `project_ide`, the `tsc/script` carrier and the Svelte route are unchanged.

Changed symbols: `generic_interop::{ForeignComponentContractAdapter, AdvancedGenericUseProjection, AdvancedGenericUse, UseContractAvailability, project_advanced_generic_uses, USE_COMPONENT, USE_CONTRACT, USE_TOLERANT, USE_FUNCTIONAL, USE_OPEN_ARGS, USE_SCOPE}` (new); `VueProjectionBackend::advanced_generic_uses` (new) plus the product re-exports; `component_use::{USE_PRELUDE, ComponentUseWitness::render, ComponentUseProjection::render_witnesses}`: the construction callee is the adapter and the prelude embeds the adapter declarations; `public_constructor::{render_binder_list, public_binder}`: the binder list renderer and binder parse shared with the use scope (no second binder renderer). `packages/types/src/components/components.ts` needed no change: the rendered prelude declares the adapter next to the witnesses.

Adapter shape: `new (__VerterUseComponent(Comp, __VerterUseConstructor(Comp)))({ ... })` is the intersection of the component's exact contract and an attribute-tolerant signature, so TypeScript's own overload resolution picks the first signature that accepts the use:

- a typed constructor is passed through whole (every construct overload, every binder: constraints, defaults, `const`, dependent and variadic parameters), with no fixed overload count;
- an Options API `defineComponent` constructor, which Vue publishes as `new (...args: any[])`, is rebuilt from the `$props` its instance publishes instead of accepting anything;
- a functional component is consumed through its own call signature (props parameter, context `slots` / `emit`) by higher-order inference, so a generic functional component keeps its binder; generated SFC defaults are never made callable;
- the tolerant fallback (the last signature's props plus an `unknown` attribute index) accepts undeclared fallthrough attributes; an exact overload always wins over it.

Every use of a parent is placed in one generic function over the parent's authored binder, so a forwarded parameter stays the parent's parameter. A use whose component cannot be named is recorded `Unavailable` with no witness; a component TypeScript types as `any` keeps `any`; an erased generic stays `unknown`.

Deletion population this node: the fixed single-signature construction adapter of the migrated component-use path (the former `__VerterUseConstructor<P, I>(component: abstract new (props: P) => I)` as the sole callee, which kept only the last construct overload and widened Options API components to `any`). The live Vue route keeps its own helpers until atomic activation and is recorded for STP58's retirement: `instantiateComponent` / `componentConstructor` in `crates/verter_compiler/src/ide/script/type_constructs.rs` and `ide/script/comp_emit.rs`, and `instantiateComponent` in `packages/types/src/components/components.ts`.

Not claimed here (owned elsewhere): an overloaded component's undeclared fallthrough attributes are tolerated only by its last signature (excess-key and attribute obligations are STP20's); listener props of a plain generic functional component whose declaration publishes only a context `emit` are not reconstructed from that `emit` (the dependency's declared props are what the adapter reads); event payload overloads (STP22); slot-scope placement of nested uses (STP24); global/async/recursive component resolution (STP32); composition of setup statements into the checking module (STP58).

## Why the negative cases discriminate

Each `reject` row, and each accept row's forbidden twin, is exercised in `protocol.mjs`:

- Rust source patches against `generic_interop.rs` (`assertDirtyTwinsRejected` requires a unique anchor, proves the plant landed, writes it, requires every named discriminator to report `FAILED` in the unit lane and, where named, in the production `--test main` carrier lane, then restores clean):
  - callee reduced to the tolerant single signature — `generic_use_construction_applies_the_exact_contract_first`, `probe_fixtures_are_the_rendered_products`, carrier test
  - open-argument constructors passed through (widened) — `foreign_contract_declarations_keep_published_shapes`, `probe_fixtures_are_the_rendered_products`
  - use scope without the parent binder — `generic_use_scope_carries_the_authored_binder`, `probe_fixtures_are_the_rendered_products`, carrier test
  - a fabricated witness for an unavailable use — `generic_use_availability_keeps_unavailable_uses_unwitnessed`, carrier test
  - the contract's non-constructor result fabricated as `any` — `foreign_contract_declarations_keep_published_shapes`
- TypeScript rewrites of a copy of a pinned probe (`assertTsTwinsRejected`, on every resolved engine, the clean probe first holding the opposite verdict):
  - callee reduced to the tolerant single signature — the first of six overloads (`kind: "circle"`) stops resolving in the positive probe
  - open-argument constructors widened — the Options API `count: '1'` construction error disappears
  - the contract's non-constructor result as `any` — the functional component's `level: 4` error disappears
  - the explicit alias replaced by its base component — the alias's fixed `K = "id"` no longer rejects `field: "label"`
- Anchored negatives (`assertAnchoredNegatives`): every construction violation is exactly one TS2769 at an authored member (for the six-overload use, TypeScript reports its last candidate's failure exactly as for `new Shape(...)` written directly); every violated explicit argument is exactly one TS2344 at the authored argument token.

The tsc probes carry the product's own rendering: `probe_fixtures_are_the_rendered_products` pins the rendered projections inside `positive.ts`, `negative.ts` and `negative-construction.ts`, and `picker_probe_fixture_is_the_rendered_declaration` pins the child component to the public-constructor product. Through both pinned engines the positive probe is diagnostic-free: the forwarded `Picker` slot item is the parent's `T` and its value `T["label"]`; the slot's `map` and the `format` prop keep their own binders per call (hover `number[]`); the explicit alias `Picker<Row, "id">` reached through a renamed re-export and a namespace fixes every channel; Options API, generic setup-function, functional and generic functional dependencies keep their published props, payloads and slots; both the first and fifth of six construct overloads resolve; the erased item stays exactly `unknown` and the untyped component stays `any`. The negative probe raises only customer TS2322 diagnostics.

Raw outcomes: see the CI artifact owner for the candidate run; local qualification is the charter §14 command above.
