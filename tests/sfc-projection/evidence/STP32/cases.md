# STP32 evidence index

Engine pins: `tests/sfc-projection/STP1/products/engine-matrix.json` (`ts-js` typescript 6.0.3, `ts-native` typescript 7.0.2). Vue pin: `vue@3.6.0-rc.5`.

Selected case IDs: `STP32-dynamic-correlated`, `STP32-async`, `STP32-recursive`, `STP32-global`, `STP32-namespace`, `STP32-missing`.

Commands (run on the candidate; this file is not an execution transcript):

- `node scripts/sfc-projection/verify-node.mjs --node STP32 --engine all --require-all --json`
- `cargo test -p verter_compiler --lib ide::vue_projection::component_resolution_tests`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

Products: `ComponentResolutionObservation`, `DynamicComponentUseContract`.

Implementation home: `crates/verter_compiler/src/ide/vue_projection/component_resolution.rs`. Consumer: `VueProjectionBackend::component_resolution`, joined to the admitted `ProjectionPlan`, component-use witnesses and script binding inventory. Configured custom elements are omitted before resolution. Vue IDE routing stays unchanged until STP58 activation.

Changed symbols: `ComponentResolutionObservation` (one revision-qualified route per use: local, namespace, recursive self via `__VerterPublicComponent`, global `GlobalComponents` fallback, dynamic `:is`, or unresolved member); `DynamicComponentUseContract` (`correlated_source`, `component_key`, `props_key`); `DYNAMIC_USE_PRELUDE` (`__VerterDynamicCorrelated`). `ResolutionSpecializationKey` mixes the plan snapshot, use id, route, expression and the component-use witness digest, so an `:is` or `v-bind` spelling change cannot reuse a stale check.

A dynamic finite union is correlated only when `:is` and a single pure `v-bind` are simple member paths of the same object (`choice.component` / `choice.props`, `choice.view` / `choice.data`, `item.comp` / `item.props`, `row.choice.view` / `row.choice.data`). The emitted check passes those authored property names into `__VerterDynamicCorrelated`. The helper distributes over each arm and refuses the value when any arm's props property is not assignable to that arm's constructor. It does not union every component with every props bag. A different object, a bare `:is` identifier, a non-member path, or a spread of a nested property that is not the sibling (`choice.props.extra`) stays on the ordinary construction and does not claim correlation. Direct attributes or validations beside the spread also stay uncorrelated.

`defineAsyncComponent` stays a local binding wrapped by `asyncComponent`; the wrapper is not erased to `any`, and generic public instance props remain. A namespace or barrel member (`Icons.Button`) keeps the member path when the root binding is in script scope. The carrier's own sanitized name resolves to `__VerterPublicComponent` instead of expanding the file again. An absent PascalCase name is a `GlobalComponents` lookup whose missing member is `unknown`, so construction is an error rather than an `any` constructor. A configured custom element never enters this product.

Deletion population this node: none.

The STP32 probes discriminate the negative side. `WrongChoice` pairs `Icons.Button` with card props, so the correlated helper reports TS2345 naming `WrongChoice`. `HiddenUnion` stores `typeof Local | typeof Icons.Button` beside one props object; that independent bag is TS2345 naming `HiddenUnion`, not a successful check. A `component: never` arm is TS2345 naming `component: never`, not a vacuous `never` success. An async card given `label: true` is TS2322 (`boolean` not assignable to `string`). An async generic constructed with `value: 42` and read as `string` is TS2322 (`number` not assignable to `string`). `MissingThing` is TS18046 on construction and TS2339 on `globalComponentsNav().MissingThing`. Author-written `any` on both properties stays open: `__VerterDynamicBad` of that pair is `never`.

Not claimed here (owned elsewhere): live IDE routing (STP58), built-in component closure (STP33), root and fallthrough contracts (STP34).
