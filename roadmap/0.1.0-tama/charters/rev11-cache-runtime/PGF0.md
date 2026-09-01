<!-- unified-charter-v2
id=PGF0
name=Project-shape and route-publication fence separation
phase=rev11
train=rev11.cache-runtime
product=rev11
kind=implementation
semantic_role=delivery
class=foundational
predecessors=A6
owner=rev11.cache-runtime:ProjectTypeStore project-shape generation domain, parse-owned syntactic route-interface fact, and exact route-publication fencing
conflict_domains=semantic_cache_store,ratified_rev11_contract
resource_class=rust-mixed
review_profile=architecture-3
gate_profile=targeted-domain
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
size=M
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/rev11-cache-runtime/PGF0.md
max_production_loc=1000
max_production_files=13
max_related_packages=3
rescope_loc=1500
rescope_files=13
rescope_unrelated_packages=3
-->

# PGF0 — Project-shape and route-publication fence separation

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

`ProjectGeneration` identifies project configuration, environment, project identity, and workspace-authority resets. Exact route publication and restoration keep that identity stable and instead advance their existing resolution facts, resolution-fact generation, and store-view epoch. The current owner is **one generation used for both project shape and route publication**. The final and sole owner is **ProjectTypeStore project-shape generation plus the existing exact route-publication fences**.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_semantic/src/facts/registry.rs`, `crates/verter_audit/src/payloads/tags.rs`, `crates/verter_session/src/resolver_core/shallow_file_state.rs`, `crates/verter_session/src/resolver_store.rs`, `crates/verter_session/src/fact_emission.rs`, `crates/verter_session/src/file_artifact_store.rs`, `crates/verter_session/src/host_manage/prepared_decl.rs`, `crates/verter_session/src/host_manage/overlay_materialize.rs`, `crates/verter_session/src/host_manage/component_meta_methods.rs`, `crates/verter_session/src/project_type_store.rs`, `crates/verter_session/src/host_manage/analysis_io.rs`, `crates/verter_session/src/host_lifecycle.rs`, and the evidence-bearing post-commit call site in `crates/verter_session/src/host_upsert.rs`.
- Named API/data boundaries: `FactKey::SyntacticRouteInterface`, the parse-owned authored route-interface hash and fact producer, `ProjectTypeStore::current_project_generation`, `ProjectTypeStore::bump_project_generation`, `VerterHost::set_import_dependencies`, `VerterHost::set_exact_resolutions`, workspace exact-resolution facts, `WorkspaceRead::resolution_fact_generation`, and the host store-view epoch.
- Mutation boundary: separate the content-independent parse-owned authored route-interface fact from the legacy whole-content route digest, classify the existing generation/fence domains, and remove route-publication project-generation bumps; no cache, resolver, scheduler, compiler, or public API authority moves.

## Exact predecessor contracts

- **A6:** implemented ledger row for “Implementation Lock Record”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- Keep `ProjectGeneration` exact validation for every consumer that already records it.
- Advance it for configuration, project/environment identity, and workspace-authority resets.
- Do not advance it when a caller publishes, restores, or retargets exact routes. Preserve exact-resolution fact publication, `resolution_fact_generation`, owner-scoped repair, and `store_view_epoch` fencing.

## Acceptance IDs and discriminating proof

- **PGF0-AC1 — sole-owner outcome:** behavioral domain tests prove project-shape resets advance project generation while route publication/restoration does not; the evidence-bearing `HeldIndexedSource` remains crate-private and confined to the prepared-declaration materializer module, with no candidate-source scanner as authority.
- **PGF0-AC2 — positive contract:** an unchanged route restored after a source edit keeps project generation stable and is immediately usable; a real retarget invalidates the old route through resolution facts without changing project generation.
- **PGF0-AC3 — incremental equivalence:** an incrementally retargeted resolved-import surface equals a fresh host at the same target, and a route mutation in the publication window cannot admit stale state.
- **PGF0-AC4 — bounded work:** direct and barrel child-contract completion after an unrelated parent edit remains cache-only with zero request-time compile, projection, provider write, or live resolution work.
- Test homes: `crates/verter_session/src/*_tests.rs`, `crates/verter_session/tests/cases`, and the existing LSP child-contract matrix.

## Deletions and forbidden designs

- Delete or structurally reject project-generation bumps from exact route publication/restoration.
- Never weaken exact `ProjectGeneration` validation, remove route facts or store-view fencing, add a parallel generation/counter, introduce request-time projection/resolution work, or move compiler/scheduler ownership.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 1,000 production LOC, 13 production files, 3 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 13 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, wrong-complete result, route-map loss, or project-identity aliasing.
- Performance budget: equivalent request work may increase by 0; the cache-only child-contract matrix must remain zero-work.

## Abort conditions

- Stop before mutation if route currency no longer has exact resolution facts plus a request-view fence, or if the correction requires a new resolver/cache/scheduler/compiler authority.
- Abort on incremental/fresh divergence, stale mid-window admission, or any request-time work added to restore immediate completion.

## Targeted verification

1. Run the focused project-generation route restore/retarget, mid-window, configuration/root-reset, structural-guard, and incremental/fresh tests in `verter_session`.
2. Run the immediate direct/barrel TS/JS/provider child-contract matrix and assert zero request-time work.
3. Run `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs` and the owning targeted Rust checks.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. Any post-review semantic change invalidates affected verdicts.

## Trusted implementation ledger

Before squashing or review, the implementation patch adds one `[[implemented]]` row to `authority/state/implemented.toml` with the node ID, planned squash commit message, approximate timezone-bearing date, and optional pull-request number. Row presence is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub.
