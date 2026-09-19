# Architecture Contracts and Owner Boundaries

This page delivers the ratified architecture contracts as contributor
documentation: who owns each responsibility, which import directions are
legal, how long each piece of state lives, and which public surfaces are
retained. The contract rows are the source of truth — this page is their
delivery, and it deliberately re-curates nothing:

- Dependency, visibility and layer contracts:
  `tests/architecture-health/ARH1/products/dependency-contracts.json`
  (plus the cutover register in
  `tests/architecture-health/ARH1/products/cutover-register.json`)
- Responsibility and owner map:
  `tests/architecture-health/ARH0/products/responsibility-map.json`
- Capability evidence (engine and version pins per capability):
  `tests/architecture-health/ARH0/products/capability-matrix.json`

When this page and a contract row disagree, the contract wins; file an issue
against the owning module instead of editing around it. The docs build
validates that this page still cites those files and still covers every
contract hotspot (`pnpm --filter docs check`).

## Layer rules

Three ratified import-direction rules constrain the Rust tree. Each is
enforced evidence-first by the architecture-health lane
(`node tests/architecture-health/ARH1/verify.mjs` in CI, on `ubuntu-latest`
with the Node version pinned by `.nvmrc`):

| Crate                | Rule                                                            | May import                                                                                             | Must not import                                              |
| -------------------- | --------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------- |
| `crates/verter_scheduler` | scheduler-runtime stays below every orchestration crate     | `verter_audit`, `verter_debug_assert`, `verter_language`, `verter_span`                                  | `verter_session`, `verter_compiler`, `verter_semantic`, `verter_lsp`, `verter_workspace`, `verter_parser`, `verter_type_expr` |
| `crates/verter_semantic`  | the semantic core never depends upward on orchestration or transport | `verter_audit`, `verter_css_syntax`, `verter_debug_assert`, `verter_language`, `verter_macro_dto`, `verter_no_typeexpr`, `verter_parser`, `verter_span`, `verter_type_expr`, `verter_type_expr_oxc` | `verter_session`, `verter_scheduler`, `verter_compiler`, `verter_lsp`, `verter_workspace` |
| `crates/verter_session`   | editor/transport/FFI crates are leaves; the session core never imports them | the analysis, compiler, scheduler, semantic, workspace and utility crates                                                               | `verter_lsp`, `verter_napi`, `verter_ffi`, `verter_wasm`       |

The full crate-by-crate owner list (every `crates/` and `packages/` root plus
the docs, tests and tooling estates) lives in the ARH0 responsibility map
linked above; nothing here duplicates it.

## The five contract hotspots

ARH1 ratified five "hotspot" files — high-traffic modules whose surfaces,
state and import directions are under active narrowing. Each subsection
below names the surviving owners and the minimal public surface; the exact
rows (snapshots, per-hook consumer inventories, narrowing dispositions) are
in the dependency contracts file.

### Scheduler — `crates/verter_scheduler/src/scheduler.rs`

Responsibility split among cohesive modules:

- Pool submission and admission control: `crates/verter_scheduler/src/dag.rs`
  (`SchedulerDag` is the sole readiness authority) and
  `crates/verter_scheduler/src/pool.rs`.
- Batch coordination: `crates/verter_scheduler/src/driver.rs` (owns all
  admission and ordering policy, drains the `SubmissionInbox`).
- Cancellation and retry bookkeeping: `crates/verter_scheduler/src/cancellation.rs`
  (one-shot clonable latches; dropping a handle cancels its pending work).
- Snapshot-epoch source authority: `crates/verter_scheduler/src/source_root.rs`.

Constructor capabilities, two modes — do not mix them:

- Threaded native constructors (`new`, `with_executor`): host-injected
  `Arc<dyn SourceLoader>` plus CPU/I/O pool handles; spawn exactly one
  driver thread. Native-only (`cfg(not(target_arch = "wasm32"))`).
- Synchronous constructors (`new_sync`, `new_sync_with_executor`): no
  driver thread; the caller must `drive_one()` / `drive_all()` (or
  `wait_or_drive`). Waiting on a sync scheduler without driving it never
  completes. On native they still take host-injected pool handles; on
  `wasm32` the pool arguments are omitted and stages run inline on the
  calling thread.

The scheduler constructs no pool and performs no file I/O at construction;
no parse, analysis or compile work happens in a constructor.

Minimal public surface: the submission API (`submit_request`,
`submit_batch_atomic`, `wait_batch`, `account_batch_submission`,
`execute_scoped_cache_node`), constructors, snapshot readers
(`try_get_source`, `try_get_analysis`, `try_get_artifact`,
`try_get_last_known_good`, `capture_source_root`), and lifecycle
(`reset`, `restart_driver`, `quiesce`, `invalidate`, `remove`). The full
retained list is the `minimalPublicSurface` row of the contract. The three
bookkeeping fields (`tombstones`, `generation_floors`,
`deferred_blocker_ids`) and the fifteen `test_`-prefixed hooks are ratified
for narrowing to `pub(crate)` / test configuration — do not add new
consumers of them; they are not a supported API for contributions.

### Flow-return dispatch — `crates/verter_session/src/project_semantic_dispatch/flow_return.rs`

Sole owner of flow-return semantic dispatch: the demand-sliced
`FlowReturn` authority, the one `SemanticQueryKey::FlowReturn` producer
through `ProjectSemanticDispatch`. Flow product assembly (all mutable
product state) lives in
`crates/verter_session/src/project_semantic_dispatch/flow_products.rs`.
The module is already `pub(crate)` with zero cross-crate surface; the
contract is that boundary itself — no widening to `pub` without a charter
amendment.

### Dispatch build/admission — `crates/verter_session/src/project_semantic_dispatch/build.rs`

Every semantic query variant that produces a new `SemanticNodeId` goes
through one of the `build_*` methods collected here, with helpers kept
`pub(super)`. The project-global dispatcher assembly is
`crates/verter_session/src/project_semantic_dispatch/mod.rs` — the single
dispatch site every reusable type-resolution operation flows through.

### Semantic query envelope — `crates/verter_session/src/semantic_query.rs`

The host-owned memo table keyed by `SemanticQueryKey`. The cross-crate
surface is cut to exactly ten retained envelope types (`HashValue`,
`SemanticNodeId`, `ScopeId`, `ResolveDeclKey`, `SemanticQueryKey`,
`ResultCompleteness`, `PartialReason`, `PartialReasonSet`,
`ProjectionMode`, `OriginEdgeKind`) plus seven retained associated items —
everything else is ratified `pub(crate)` (see the bulk narrowing row and
its 68-file consumer migration in the contract). The per-variant key/env
binding table lives in
`crates/verter_session/src/semantic_query/query_key_spec.rs` and is
written only by its generator.

### Flow slice content — `crates/verter_session/src/flow_slice_content.rs`

The owned, arena-free content lowering of exactly one planned flow slice.
A `pub(crate)` module with zero cross-crate consumers; slice publication
admission stays with `flow_return.rs`, where the budget outcome gates
admission.

## Capability mapping and evidence basis

These substrate surfaces are not user-facing capabilities; they serve the
capability catalog (sole truth: the product surface catalog cited by
`tests/documentation/DOC0/products/generated-reference-plan.v1.json` —
surface ids such as `vue.language_service.typing`, `vue.tsc.project_check`,
`vue.component_meta.scalar`). Per-capability engine and version evidence,
with its exact sources, is the ARH0 capability matrix linked above; this
page adds no second matrix.

Population and snapshot counts in the contract (importers, retained
surfaces, test-hook consumers) were measured on the candidate commit
recorded in the contract JSON, and the CI architecture-health lane
re-derives them from the live tree on every run — a drifted count fails
`node tests/architecture-health/ARH1/verify.mjs`. That command is the
canonical ARH1 ratification; it does not require `--provenance`.

`--provenance` is an extra ancestor check on the product-pinned candidate.
After ARH1's squash onto main that pin is not an ancestor of this tree, so
`--provenance` currently fails on main-side checkouts while canonical
`verify.mjs` still passes. Re-pinning those JSON files is ARH1's job, not
this page's. Any count claim that contradicts a live `verify.mjs` result
is a bug in the doc.

## Related pages

- [Source and map identity](./source-identity.md) — span types, position
  encodings, stable hashes and source maps.
- [Query lifetimes and determinism](./query-lifetimes.md) — lifetime
  classes, memo identity, scheduler fencing, deterministic ordering.
- [First contribution](./first-contribution.md) — an end-to-end walkthrough
  that uses the owning interfaces above.
