# STP21 evidence index

Engine pins: `tests/sfc-projection/STP1/products/engine-matrix.json`. Vue pin: `vue@3.6.0-rc.5`.

Selected case IDs: `STP21-event-alias`, `STP21-model-key`, `STP21-static-handler`, `STP21-modifier`, `STP21-dynamic-name`, `STP21-collision`.

Commands (run on the candidate; this file is not an execution transcript):

- `node scripts/sfc-projection/verify-node.mjs --node STP21 --engine all --require-all --json`
- `cargo test -p verter_compiler --lib ide::vue_projection::event_keys`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

Products: `EventTransportPlan`, `EventAliasRelation`, `ListenerConsumerSet`.

Implementation home: `crates/verter_compiler/src/ide/vue_projection/event_keys.rs`. Dormant consumer: `VueProjectionBackend::event_transport`, joined to the admitted `ProjectionPlan` and STP17's ordered attribute products. Vue IDE routing remains unchanged until STP58 activation.

Event identity is retained separately from `on*` runtime keys. `@save-item` and `@saveItem` share `onSaveItem`; a bound `:on-save` remains a prop spelling. Model updates publish `update:modelValue` behind `onUpdate:modelValue`; modifier order stays attached to the relation; finite dynamic names retain every literal key and argument-less `v-on` remains an open object-listener domain. Per-key consumer sets preserve every contributor and mark collisions instead of silently choosing one.

The STP21 protocol executes the clean Rust discriminators and applies/restores mutations that conflate a bound prop with `v-on`, erase model aliases, discard event modifiers, erase finite dynamic-name candidates, or suppress collision state. The TypeScript probes verify aliases, model keys, modifier payloads, finite candidates and rejection of static listener text on each pinned engine.
