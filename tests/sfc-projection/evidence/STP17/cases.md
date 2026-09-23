# STP17 evidence index

Engine pins: `tests/sfc-projection/STP1/products/engine-matrix.json`. Vue pin: `vue@3.6.0-rc.5` (runtime-key semantics read from the pinned `@vue/compiler-core` `buildProps`/`dedupeProperties`/`transformBind`/`transformModel`, `@vue/compiler-dom` `transformOn`, and `@vue/runtime-core` `mergeProps`/`setFullProps`/`isEmitListener`/`emit` handler lookup).

Selected case IDs: `STP17-spellings`, `STP17-merge`, `STP17-overwrite`, `STP17-optional-spread`, `STP17-collision`.

Commands (run on the candidate; this file is not an execution transcript):

- `node scripts/sfc-projection/verify-node.mjs --node STP17 --engine all --require-all --json`
- `cargo test -p verter_compiler --lib ide::vue_projection::attribute_operations`
- `cargo test -p verter_compiler --test main attribute_operations_reads_admitted_carrier_blocks`
- `node --test --test-name-pattern STP17 scripts/sfc-projection/verify-node.test.mjs`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

Products: `VueAttributeSequence`, `RuntimePropertyKeyPlan`, `AttributeConsumerRelation`.

Implementation home: `crates/verter_compiler/src/ide/vue_projection/attribute_operations.rs`. Dormant consumer: `VueProjectionBackend::attribute_operations`, derived from the same `ProjectionPlan` operations and admitted expressions (`ComponentUse::operations` joined to the admitted parse's element props through the use's component-expression occurrence). Vue IDE `project_ide` and the Svelte route are unchanged.

Changed symbols: `attribute_operations::{project_attribute_operations, VueAttributeSequence, VueAttributeOp, AttributeSyntax, RuntimePropertyKeyPlan, RuntimePropertyWrite, RuntimeKey, WriteValue, MergeRule, OpaqueSpread, SpreadKind, EffectiveProperty, Contribution, Certainty, AttributeConsumerRelation, ConsumerChannel, ValidationObligation, AttributeOperationsProjection}` (new); `VueProjectionBackend::attribute_operations` (new) plus the product re-exports; `projection_plan::classify_op` widened to `pub(crate)` so the sequence joins exactly the operations the plan emitted. Runtime-key spelling reuses the runtime compiler's `camelize` and handler-key assembly.

Deletion population this node: empty. The dormant product publishes ordered operations and never flattens attrs into an object; the live companion's JSX attribute flattening (`ide/template/props.rs`) is part of the legacy Vue projection population whose deletion STP57/STP58 own at atomic activation.

## Why the negative cases discriminate

Each `reject` row, and each accept row's forbidden twin, is exercised as an applied source mutation in `protocol.mjs` (`assertDirtyTwinsRejected` requires a unique anchor, writes the patch into `attribute_operations.rs`, requires every named discriminator to report `FAILED` in both the unit lane and the production `--test main` carrier lane, then restores clean):

- `:on-save` normalized like `@save` (bound keys always camelized) — `attribute_ops_spellings_keep_raw_runtime_prop_and_event_lookup_distinct`, `attribute_operations_reads_admitted_carrier_blocks`
- last-wins listeners (listener keys overwrite) — `attribute_ops_merge_listeners_accumulate_across_interleaved_v_bind`, carrier test
- earlier write survives a later definite overwrite — `attribute_ops_overwrite_later_definite_prop_wins`, carrier test
- optional/opaque spread treated as an unconditional overwrite — `attribute_ops_optional_spread_keeps_earlier_key_possible`, carrier test
- collision validates only the first reachable channel — `attribute_ops_collision_validates_every_reachable_channel`, carrier test

Control twins: literal-group first-wins dedupe (`attribute_ops_overwrite_later_definite_prop_wins` second use), models/directives/modifiers ordering and `.once`/`.camel`/reserved keys (`attribute_ops_sequence_keeps_models_directives_and_modifiers_ordered`), `<component :is>` selection (`attribute_ops_dynamic_component_is_selects_instead_of_writing`), and deterministic/incomplete products (`attribute_ops_products_are_deterministic_and_incomplete_plans_stay_incomplete`). The probe pair proves through both pinned engines that the shared `onSave` key keeps its declared callback contract: the negative probe raises the customer `2322` diagnostic and the positive probe's clean twin stays diagnostic-free.

Raw outcomes: see the CI artifact owner for the candidate run; local qualification is the charter §14 command above.
