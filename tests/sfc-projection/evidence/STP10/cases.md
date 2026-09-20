# STP10 evidence index

Input snapshot (job baseline): docs(doc): deliver architecture and contributor documentation (DOC2) (#639) 2026-09-19 on `tama_dag/0.1.0-tama/STP10`.

Engine pins: `tests/sfc-projection/STP1/products/engine-matrix.json` — ts-js `typescript@6.0.3`, ts-native `typescript@7.0.2`. Vue pin: `vue@3.6.0-rc.5`.

Selected case IDs: `STP10-roundtrip`, `STP10-role`, `STP10-overlap`, `STP10-stale-map`, `STP10-synthetic`.

Commands (run on the candidate; do not treat this file as an execution transcript):

- `node scripts/sfc-projection/verify-node.mjs --node STP10 --engine all --require-all --json`
- `node --test tests/sfc-projection/STP10/emission.spec.ts`
- `cargo test -p verter_compiler --lib framework_common::projection_plan::origin::`

Products: `ProjectionEmission`, `ProjectionOrigin`, `ObservationRole`, `EditOrigin`, `MappingProduct`.

Implementation home: `crates/verter_compiler/src/framework_common/projection_plan/origin.rs`. Dormant consumer: `VueProjectionBackend::projection_emission`. Mapping geometry: TCM1 `MappingProduct::of` from the same `CodeTransform` record. Vue IDE `project_ide` is unchanged; STP58 owns atomic activation. Svelte's current route is untouched. Canonical CodeTransform map machinery is retained.

Deletion population this node: empty.

This file does not claim the cases executed. Copying the charter here is not evidence.
