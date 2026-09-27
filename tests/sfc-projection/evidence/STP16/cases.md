# STP16 evidence index

Engine pins: `tests/sfc-projection/STP1/products/engine-matrix.json`. Vue pin: `vue@3.6.0-rc.5`.

Selected case IDs: `STP16-required-api`, `STP16-public-members`, `STP16-private-leak`, `STP16-generic-constraint`, `STP16-public-callable`, `STP16-type-precision`, `STP16-public-specialization`.

Commands (run on the candidate; this file is not an execution transcript):

- `node scripts/sfc-projection/verify-node.mjs --node STP16 --engine all --require-all --json`
- `cargo test -p verter_compiler --lib ide::vue_projection::public_constructor`
- `cargo test -p verter_compiler --test main public_constructor_reads_admitted_carrier_blocks`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

Products: `VuePublicConstructorContract`, `PublicInstanceProjection`, `ConstructorCompatibilityReceipt`.

Implementation home: `crates/verter_compiler/src/ide/vue_projection/public_constructor.rs`. Dormant consumer: `VueProjectionBackend::public_constructor`, which is also the backend's `ProjectionBackend::PublicApi`. Vue IDE `project_ide`, the `tsc/script` public carrier and the Svelte route are unchanged.

Changed symbols: `project_public_constructor` (new) deriving the contract from the admitted script pair with the shared setup macro resolution; `VuePublicConstructorContract::{declaration, receipt}` (new) rendering one generic construct signature over the authored binder (`const` kept on the signature, constraints and defaults on the signature and on the specialized `__VerterPublicProps`/`__VerterPublicInstance` aliases), Vue's `ComponentPublicInstance` base with typed `$props`/`$emit`/`$slots`, exposed members through the expose provider rendered in the same declaration (one function over the binder whose body is the setup statements and which returns the `defineExpose` argument, `async` when setup awaits at top level) instantiated with the binder, runtime values Vue hoists out of setup (runtime props options `satisfies` Vue's props options type so a literal `required: true` stays literal, runtime emits, open `withDefaults` defaults, a non-literal model `required`) rendered as module constants or, when they name a binder parameter, as functions over the binder read through an instantiation expression, and literal `name`/`inheritAttrs` statics; `PropsRequirement` deciding the props parameter syntactically (a required prop or model ⇒ required, where a `withDefaults` default makes its prop omissible, every same-name interface declaration across both blocks contributes and `required` is read through parentheses / `as const` / `<const>` / `satisfies`; all optional ⇒ optional; otherwise the conditional rest tuple so TypeScript decides); `PropsDefaults` and `__VerterPropsWithDefaults` making defaulted keys optional on the props type; `VueProjectionBackend::public_constructor` (new) and `ProjectionBackend::PublicApi = VuePublicConstructorContract`; `packages/types/src/components/components.ts` needed no change (the dormant product names Vue's own exported types).

Deletion population this node: empty. The `tsc/script` public carrier and the IDE-derived public type authority converge onto this product and are deleted at Vue atomic activation (STP58), not while two public authorities can answer.

## Why the negative cases discriminate

The probe fixture `probes/components/Picker.vue.ts` ends with the product's rendered declaration byte for byte, expose provider included (`picker_probe_fixture_is_the_rendered_declaration` over setup bytes and `public_constructor_reads_admitted_carrier_blocks` over real carrier bytes), so the tsc probes check the product's output through both pinned engines and the exposed `current` is typed from the authored `ref<T>()`. `Defaulted.vue.ts`, `Merged.vue.ts` and `Strict.vue.ts` are pinned the same way (`requirement_probe_fixtures_are_the_rendered_declarations`): the positive probe accepts `new Defaulted()`, rejects `new Merged()` and `new Merged({ id: 1 })` (a later merged interface's `id`, an `as const` required model) and `new Strict()`, and specializes `Strict<"a">` runtime props and events. Every rejected design is an `@ts-expect-error` in the clean positive probe — a permissive constructor turns it into an unused-directive diagnostic — and the negative probe keeps the private-binding access a customer TS2339.

`protocol.mjs` applies each rejected design as a source patch to the owned product (anchor proven unique and the mutation proven on disk), requires each named unit discriminator to report `FAILED`, then restores clean. The production carrier test runs in the clean lane only, since it renders through the same product:

- blanket optional props argument — `required_props_keep_one_constructor_with_a_required_argument`, fixture
- dropped exposed members — `instance_publishes_typed_framework_and_exposed_members`
- private setup bindings published — `private_setup_bindings_stay_off_the_instance`
- permissive overload fallback — `generic_constraints_stay_on_the_single_construct_signature`, fixture
- constraints dropped from the construct signature — `generic_constraints_stay_on_the_single_construct_signature`, fixture
- callable default export — `default_export_is_never_callable`, fixture
- authored field widened to `Record<string, any>` — `rendered_fields_keep_authored_types`
- aliases rendered without binder arguments — `binder_arguments_reach_every_public_surface`, fixture

Clean/control twins: all-optional props keep `props?`, an imported props type yields the conditional rest, a local interface and a required model decide `Required`, `withDefaults` keys make their props omissible while an undefaulted required prop stays required and open defaults defer to TypeScript, merged interfaces join across both blocks, `true as const` / `<const>true` / `true satisfies boolean` stay required while a possibly-widened value defers to TypeScript, runtime options naming no binder parameter stay plain constants, runtime prop names and an untyped `defineModel()` keep Vue's open domains and are reported in the receipt, a carrier without `<script setup>` keeps its authored `defineComponent` constructor, and a local function named like a macro is an ordinary call.

## Acceptance axes

- AC1 completeness: seven mandatory cases selected, each mapped to executed Rust cases plus the two-engine probe pair.
- AC2 observability: two-engine diagnostics, instance type (`__VerterPublicInstance<string | number>`), hover (`number`), definition and references on the probe; `ConstructorParameters`, `InstanceType`, `Comp<number>` and `new Comp({ test: 0 })` asserted independently.
- AC3 state safety: the product is a pure function of the admitted parse (exact-source check shared with the other script products); it owns no cache or published state.
- AC4 bounded work: one parse per authored block plus one `generic` parse; no checker run, no retention. The setup statements are copied into the declaration only when setup exposes members, as the provider body, and appear there once. Dormant, so no hot path changes.

Raw outcomes: see the CI artifact owner for the candidate run; local qualification is the charter §14 command above.
