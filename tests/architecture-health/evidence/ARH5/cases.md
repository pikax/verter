# ARH5 evidence cases

The sole owning interface is `node tests/architecture-health/ARH5/verify.mjs` (verify) plus `node --test tests/architecture-health/ARH5/arh5.test.mjs` (dirty twins), wired into `test:scripts` and the CI `architecture-health` lane. The verifier joins the shipped ARH2 predecessor (imported `validate()`, never re-implemented) and the live compiler facade. No DAG copy is read from this tree.

Production `VueCarrierCompiler` is parse plus identity/downcast only, matching `SvelteCarrierCompiler`. `compile_ide` and `compile_bundle` remain as test-support shims; CMP1 still owns deleting the `IdeCompileOptions`/`RuntimeCompileOptions` conversion. Production owners are `VueProjectionBackend::project_ide` and `VueHostIntegrationBackend::compile_host_products` (`vue_carrier_bundle` is the shared orchestration). Qualified maps stay on `CompileArtifactSet`.

## ARH5-ratification (accept)

Clean products validate. Manifest cases, products and verify/test commands match the verifier. The candidate is a 40-hex commit; `--provenance` (full-history architecture-health lane) proves it is an ancestor of HEAD. Shallow `test:scripts` checkouts skip `--provenance` when the pin object is absent.

## ARH5-cutover (reject) — AC1

- `displaced-population-drift` / `retained-population-drift`: displaced methods are exactly `compile_bundle` and `compile_ide`; retained production methods are `adapter_id`, `carrier_language_id`, `parse`.
- `displaced-method-still-production`: each displaced method must carry `#[cfg(any(test, feature = "test-support"))]` on the production `impl VueCarrierCompiler`.
- `retained-method-gated`: parse/identity must not take that cfg.
- `owner-type-missing` / `owner-method-missing`: CUT-1 survives on `VueProjectionBackend::project_ide`; CUT-2 on `VueHostIntegrationBackend::compile_host_products`.
- `retained-adapter-stolen`: `IdeCompileOptions`, `RuntimeCompileOptions`, `derive_legacy_vue_options` remain for CMP1.
- `session-guard-token-missing`: host compile-entry still forbids `compile_bundle`.

## ARH5-authority (reject) — AC2

A shorter facade that reparses output or merges semantic and runtime authority is rejected by existing compile-fail rows (`frontend_only_has_no_runtime_accessor`, `projection_only_has_no_runtime_accessor`) plus the new production-absent `compile_bundle`/`compile_ide` fixtures. Dirty twin drops the frontend-only row.

## ARH5-work (reject) — AC3

Fresh/incremental, edit/revert and cancellation are not applicable (no state/query/map mutation). Stale/partial and deterministic map qualification bind existing `artifact_schema_tests` witnesses. Dirty twins empty stale/partial evidence or invent a concern.

## ARH5-delivery (reject) — AC4

`examples/reference` must not name the displaced methods. AC4 rationale records the compile-time migration and the DOC1 non-duplication.

## ARH5-cost (reject) — AC5

No committed wall-clock/RSS/speedup. Required work is not removed; CMP1 adapters stay. Dirty twin plants `wallNs`.
