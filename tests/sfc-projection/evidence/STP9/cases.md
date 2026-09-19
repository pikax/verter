# STP9 evidence index

Input snapshot (job baseline): `5506fca6316b3201dd45e1dd474b559f7ff450a7` on `tama_dag/0.1.0-tama/STP9`.

Engine pins: `tests/sfc-projection/STP1/products/engine-matrix.json` — ts-js `typescript@6.0.3`, ts-native `typescript@7.0.2`. Vue pin: `vue@3.6.0-rc.5`.

Selected case IDs: `STP9-ids`, `STP9-shadow`, `STP9-type-free`, `STP9-complete-cache`, `STP9-determinism`.

Commands (run on the candidate; do not treat this file as an execution transcript):

- `node scripts/sfc-projection/verify-node.mjs --node STP9 --engine all --require-all --json`
- `node --test tests/sfc-projection/STP9/plan.spec.ts`
- `cargo test -p verter_compiler --lib framework_common::projection_plan::`

Products: `ProjectionPlan`, `BindingOriginId`, `ComponentUseId`, `GenericBinderRef`, `OrderedAttributeOp`, `BranchEdge`.

Implementation home: `crates/verter_compiler/src/framework_common/projection_plan/mod.rs`. Dormant consumer: `VueProjectionBackend::projection_plan`. CodeTransform surface: `ProjectionPlan::syntax_obligations`. Vue IDE `project_ide` is unchanged; STP58 owns atomic activation. Svelte's current route is untouched.

Deletion population this node: empty.

This file does not claim the cases executed. Copying the charter here is not evidence.
