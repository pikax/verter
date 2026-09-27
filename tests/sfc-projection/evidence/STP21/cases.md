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

Event identity is retained separately from `on*` runtime keys. `@save-item` and `@saveItem` share `onSaveItem`; a bound `:on-save` remains a prop spelling. Model updates publish `update:modelValue` behind `onUpdate:modelValue`. A dynamic `v-model:[arg]` keeps a dynamic `update:` identity and does not invent `update:modelValue`. Modifier order stays attached to the relation, including the listener key of each finite dynamic candidate. Finite dynamic names retain every literal key; a non-literal branch also leaves the open listener domain in the same consumer set. Every open listener on one use, including argument-less `v-on` objects, shares that set and records a collision when more than one consumer reaches it. Static `onSave="handler"` text is not a listener. Per-key consumer sets preserve every contributor and mark collisions instead of silently choosing one. Vue IDE `event_to_jsx_name` stays on the pre-activation spelling (hyphens preserved) until STP58.

The STP21 protocol executes the clean Rust discriminators and applies/restores mutations that conflate a bound prop with `v-on`, treat static `onSave` text as a listener, erase model aliases, discard event modifiers, erase finite dynamic-name candidates, or suppress collision state. The TypeScript probes verify aliases, model keys, modifier payloads, finite candidates against the declared listener contracts, and rejection of static listener text on each pinned engine.
