# STP11 evidence index

Engine pins: `tests/sfc-projection/STP1/products/engine-matrix.json`. Vue pin: `vue@3.6.0-rc.5`.

Selected case IDs: `STP11-universal`, `STP11-scope`, `STP11-await`, `STP11-assertion`, `STP11-one-body`.

Commands (run on the candidate; this file is not an execution transcript):

- `node scripts/sfc-projection/verify-node.mjs --node STP11 --engine all --require-all --json`
- `node --test tests/sfc-projection/STP11/script-setup.spec.ts`
- `cargo test -p verter_compiler --lib ide::vue_projection::`

Products: `TsSetupProjection`, `ModuleScopeProjection`, `UniversalSetupBinder`.

Implementation home: `crates/verter_compiler/src/ide/vue_projection/script_setup.rs`. Dormant consumer: `VueProjectionBackend::script_projection`. Vue IDE `project_ide` and the Svelte route are unchanged.

Deletion population this node: empty. Retirement of the angle-assertion repair and duplicate setup-string wrappers happens at atomic activation.
