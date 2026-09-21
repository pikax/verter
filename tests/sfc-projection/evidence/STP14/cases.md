# STP14 evidence index

Engine pins: `tests/sfc-projection/STP1/products/engine-matrix.json`. Vue pin: `vue@3.6.0-rc.5`.

Selected case IDs: `STP14-local-capture`, `STP14-dependent-default`, `STP14-typeof-capture`, `STP14-alias-cycle`, `STP14-duplicate-error`.

Commands (run on the candidate; this file is not an execution transcript):

- `node scripts/sfc-projection/verify-node.mjs --node STP14 --engine all --require-all --json`
- `node --test tests/sfc-projection/STP14/binder-capture.spec.ts`
- `node --test scripts/sfc-projection/verify-node.test.mjs`
- `cargo test -p verter_compiler --lib ide::vue_projection::`

Products: `PublicTypeDependencySlice`, `BinderCapturePlan`, `LiftedSourceDeclaration`.

Implementation home: `crates/verter_compiler/src/ide/vue_projection/binder_capture.rs`. Dormant consumer: `VueProjectionBackend::public_type_dependencies`. Vue IDE `project_ide` and the Svelte route are unchanged.

Changed symbols: new `binder_capture` module (capture products plus `capture_binder_plan`); `VueProjectionBackend::public_type_dependencies` and the shared private `script_blocks` accessor; `script_setup::MacroContext` extracted so macro recognition keeps one owner across both script-facing projections; `script_setup::binder_product_from` extracted so the binder product and the binder AST come from one `generic` parse.

Deletion population this node: empty. Retirement of native-reconstructed public type expressions for the TypeScript path happens at atomic activation.

## Why the negative cases discriminate

Each `reject` row is exercised twice: as a product dirty twin in `protocol.mjs`, and as an executable Rust case that fails when the rule is removed. Negative controls run on the candidate confirmed that removing any one of the following makes exactly its owning case fail and leaves the others green:

- binder scope restricted to `<script setup>` — `stp14_local_capture_keeps_the_binder_out_of_scope_in_the_normal_script`
- constraint/default closure over binder parameters — both `stp14_dependent_default_*` closure cases
- capture-avoiding alpha rename of a lifted binder parameter — `stp14_local_capture_renames_a_binder_parameter_that_module_scope_already_binds`
- duplicate-binding detection — both `stp14_duplicate_error_*` cases
- cycle reporting over the terminated closure graph — both `stp14_alias_cycle_*` cases

The probe pair additionally proves through both pinned engines that lifting keeps the authored member type: the negative probe still raises the customer diagnostic rather than collapsing into a permissive shape, and the positive probe's clean twin stays diagnostic-free.
