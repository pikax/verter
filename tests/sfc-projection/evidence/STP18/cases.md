# STP18 evidence index

Engine pins: `tests/sfc-projection/STP1/products/engine-matrix.json`. Vue pin: `vue@3.6.0-rc.5`.

Selected case IDs: `STP18-single-witness`, `STP18-uncoupled`, `STP18-contextual`, `STP18-literal`, `STP18-handler-check`, `STP18-fresh-id`, `STP18-script-template-parity`.

Commands (run on the candidate; this file is not an execution transcript):

- `node scripts/sfc-projection/verify-node.mjs --node STP18 --engine all --require-all --json`
- `cargo test -p verter_compiler --lib ide::vue_projection::component_use`
- `cargo test -p verter_compiler --test main component_uses_reads_admitted_carrier_blocks`
- `node --test --test-name-pattern STP18 scripts/sfc-projection/verify-node.test.mjs`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

Products: `ComponentUseWitness`, `SpecializedUseObservation`, `InferenceTransaction`.

Implementation home: `crates/verter_compiler/src/ide/vue_projection/component_use.rs`. Dormant consumer: `VueProjectionBackend::component_uses`, derived from the same `ProjectionPlan` and the attribute-operation products (`VueAttributeSequence`, `RuntimePropertyKeyPlan`) of the admitted parse. Vue IDE `project_ide`, the `tsc/script` carrier and the Svelte route are unchanged.

Changed symbols: `component_use::{project_component_uses, ComponentUseProjection, ComponentUseWitness, InferenceTransaction, TransactionMember, MemberValue, ValidationCheck, CheckContract, ExcludedOperation, ExclusionReason, SpecializedUseObservation, ObservationKind, SpecializationKey, USE_PRELUDE, USE_CONSTRUCTOR, USE_PROP, USE_LISTENER, USE_SLOT_PROPS, USE_MODEL, WITNESS_PREFIX}` (new); `VueProjectionBackend::component_uses` (new) plus the product re-exports; `projection_plan::{OrderedAttributeOp::handler, HandlerShape}` (new): the `v-on` value shape read from the plan's own template parse (`multi_statement`, arrow/function, identifier/member), so inline statements are recognized from the parse fact every backend wraps handlers from rather than from text. `packages/types/src/components/components.ts` needed no change: the rendered prelude declares the construction helper and observation types next to the witnesses.

Transaction shape: `const W = new (__VerterUseConstructor(Comp))({ ... })`. The helper hands back the component's own construct signature with an `unknown` attribute index on its props parameter, so TypeScript infers the binder once at the construction from every member, contextually types callbacks, keeps literal types and checks declared keys, while a fallthrough attribute is not an excess-key error (excess-key policy is STP20's obligation). Members follow the runtime property plan: certainly overwritten writes are excluded, `v-bind` object spreads keep their merge position, and the first handler of each listener key is placed after every spread; further listeners, repeated `class`/`style` writes and inline statements are validation-only checks against the specialized instance (`__VerterUseListener` / `__VerterUseProp`). Slot, event, model and instance observations read the witness binding. The specialization key digests the logical use and its checking inputs, never source offsets.

Deletion population this node: empty. Migrated uses (this dormant product) never call `instantiateComponent` or extract a channel from the uninstantiated component. The displaced live call sites stay on the legacy Vue projection until atomic activation and are recorded for STP58's retirement: `ide/script/comp_emit.rs` (`instantiateComponent` in the `v-slot`, `v-for` and plain component `Comp` functions), the `instantiateComponent` / `componentConstructor` declarations in `ide/script/type_constructs.rs`, and `instantiateComponent` in `packages/types/src/components/components.ts`.

Not claimed here (owned elsewhere): callable functional components and third-party adapters (STP19), Boolean casting, excess keys and spread obligations (STP20), dynamic event names and listener objects (STP21), model write/modifier contracts (STP23), slot-scope placement of nested uses (STP24), template ref names (STP35).

## Why the negative cases discriminate

Each `reject` row, and each accept row's forbidden twin, is exercised as an applied source mutation in `protocol.mjs` (`assertDirtyTwinsRejected` requires a unique anchor, writes the patch into `component_use.rs`, requires every named discriminator to report `FAILED` in the unit lane and, where named, in the production `--test main` carrier lane, then restores clean):

- model value split out of the construction — `component_use_single_witness_carries_every_channel_once`, `component_uses_reads_admitted_carrier_blocks`
- observations read from the uninstantiated component — `component_use_observations_read_the_specialized_witness`, carrier test
- bound callbacks erased to `any` (no contextual type) — `component_use_contextual_callbacks_stay_in_the_construction`, carrier test
- static discriminant widened to `string` — `component_use_static_discriminant_stays_a_literal`, carrier test
- collected listeners emitted as repeated construction members — `component_use_collected_listeners_validate_against_the_specialized_contract`, carrier test
- specialization key following source offsets — `component_use_specialization_is_per_use_and_offset_free`
- one witness shared by sibling uses — `component_use_sibling_uses_keep_independent_witnesses`, carrier test

The tsc probes carry the product's own rendering: `probe_fixtures_are_the_rendered_products` pins the rendered projections inside `positive.ts` / `negative.ts`, and `table_probe_fixture_is_the_rendered_declaration` pins the child component to the public-constructor product. Through both pinned engines the positive probe is diagnostic-free: slot props read `T = Row` (hover `number`), the sibling keeps `T = number, U = number`, the grid discriminant branch stays legal, and explicit (`new Table<Row, string>`) and inferred script constructions are mutually assignable with the template witness. The negative probe raises only the customer `2322` at the collected listener whose parameter contradicts the specialized payload.

Raw outcomes: see the CI artifact owner for the candidate run; local qualification is the charter §14 command above.
