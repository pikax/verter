<!-- unified-charter-v2
id=CCA1GE
name=Eval-source semantic consumer cutover
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA1E,CCA1F
owner=compiler.compiler-bridge:eval-source authority route and combined-method deletion
conflict_domains=semantic_authority,compiler_execution
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
charter=charters/compiler-compiler-bridge/CCA1GE.md
max_production_loc=1200
max_production_files=20
max_related_packages=2
rescope_loc=1500
rescope_files=24
rescope_unrelated_packages=3
-->

# CCA1GE — Eval-source semantic consumer cutover

## Independently acceptable outcome, role, and owners

Make registered `FrameworkSemanticAuthority` the sole eval-source producer, publish those bytes once on `IndexedReady.eval_source` as the backend `Arc<str>`, and delete the displaced combined `CarrierCompiler::eval_source` declaration plus Vue/Svelte implementations. Current ownership is the combined compiler method plus host blanking / per-consumer recatalog; final ownership is catalog identity (adapter × artifact epoch × Semantic) with `IndexedReady` as the sole stored eval-source. This lands and rolls back independently of template facts.

## Exact production population and APIs

No dual-read may survive. Every file below is in-scope; a 21st production file is a rescope.

**Catalog lookup and backend bytes**

- `crates/verter_compiler/src/framework_common/registered_carrier_projection.rs` — type-erased semantic catalog lookup; `InstalledSemanticAuthority` is an eval-source fn payload keyed adapter × artifact epoch × Semantic, not a Vue/Svelte match dispatcher.
- `crates/verter_compiler/src/framework_common/catalog.rs` — catalog type-erasure helper (`map_semantic`) so the row can store that payload without naming Vue/Svelte in the generic selector.
- `crates/verter_compiler/src/framework_common/vue_semantic_authority.rs` — Vue backend eval bytes (position-preserving; length equals source).
- `crates/verter_compiler/src/svelte/semantic_authority.rs` — Svelte backend eval bytes.

**Combined-method deletion**

- `crates/verter_compiler/src/framework_common/carrier_compiler.rs` — delete the combined method and its trait-harness coverage only.
- `crates/verter_compiler/src/framework_common/vue_bridge.rs` — delete the Vue combined-method implementation after route equivalence.
- `crates/verter_compiler/src/svelte/carrier.rs` — delete the Svelte combined-method implementation after route equivalence.

**IndexedReady sole producer**

- `crates/verter_session/src/host_manage/eval_program.rs` — overlay and unscheduled catalog lookup; store the backend `Arc<str>` as-is; catalog miss / missing artifact is typed refusal before parse, lease, or publication.
- `crates/verter_session/src/host_executor.rs` — source-stage snapshot stores the catalog `Arc<str>` on `HostSourceData`. Source-stage snapshot and IndexedReady are one cold-load request; do not recatalog.
- `crates/verter_session/src/host_manage/prepared_decl.rs` — cold `IndexedReady` materialization clones the source-stage `Arc`; missing snapshot is typed refusal, never a second catalog call or byte copy.
- `crates/verter_session/src/host_manage/overlay_materialize.rs` — overlay `IndexedReady` materialization reads that producer only.

**Dispatch, miss refusal**

- `crates/verter_session/src/parse.rs` — replace the sole production `.eval_source(...)` combined-trait dispatch; preserve `carrier_eval_source_type`/snapshot behavior; catalog miss must refuse, never synthesize `ScriptAnalysisSnapshot::default()` as a successful empty analysis.
- `crates/verter_session/src/host_manage/analysis_io.rs` — analysis/source rebuild that calls `build_script_analysis_for_artifact` must refuse catalog miss the same way; it must not serve a default empty script snapshot as current analysis.

**Consumer Arc reuse (no second catalog call, no `String` copy)**

- `crates/verter_session/src/host_manage/component_meta_methods.rs` — overlay capture reuses `IndexedReady.eval_source` `Arc`; no recatalog, no `to_string`.
- `crates/verter_session/src/host_manage/component_meta_request_impl.rs` — base capture and `CapturedComponentMetaInputs.owner_eval_source` reuse that `Arc`.
- `crates/verter_session/src/framework/script_facts.rs` — reuse `IndexedReady.eval_source` when present; catalog miss stays unavailable, never a second producer.
- `crates/verter_session/src/host_manage/eval_env.rs` — reuse the stored `Arc`; do not recatalog then `to_string`.
- `crates/verter_session/src/host_resolve/route_surface.rs` — dependency eval-source reads refuse catalog miss; no `Arc::<str>::from` recopy of catalog bytes.

**Displaced host-blanking producer**

- `crates/verter_session/src/host_resolve/vue_script_extract.rs` — `extract_vue_script_content` / host blanking are not production eval-source producers (test-only if retained).
- `crates/verter_session/src/host_resolve/mod.rs` — re-export of that extract helper follows the same production-vs-test visibility.

Focused existing eval-source tests, including the Svelte conformance matrix, are evidence surfaces and do not enlarge the production-file budget.

The named boundary is source text plus immutable framework/parse identity to the backend-owned eval source and source kind, stored once on `IndexedReady`. Parse publication, template facts, projection, runtime, assembly, and unrelated host routing are excluded.

## Exact predecessor contracts and binding laws

- **CCA1E:** the Vue `FrameworkSemanticAuthority` produces eval source from a registered parse artifact.
- **CCA1F:** the Svelte `FrameworkSemanticAuthority` produces eval source from a registered parse artifact.
- Lookup is once per request by framework/catalog epoch; no generic framework branch, second resolver, source reparse, host blanking fallback, or combined-trait fallback is allowed.
- Eval source and its `FileLanguage`/source-kind classification remain bound to the same source revision and parse artifact. Cancelled, stale, or partial work publishes and warms nothing.
- Catalog miss is typed refusal. An empty `ScriptAnalysisSnapshot::default()` is not a miss success.

## Internal subblocks, migration, and deletions

1. Characterize Vue/Svelte fresh, preloaded, incremental, and refusal outcomes through existing tests.
2. Switch production dispatch atomically to the registered semantic backend; publish once onto `IndexedReady.eval_source`; point analysis miss paths and component-meta/script-fact/eval-env/route readers at that `Arc`.
3. Delete the combined trait method, both framework implementations, production host-blanking extract, and method-only tests in the same candidate.

No shadow/dual read may survive the candidate. Delete no template-fact, IDE, runtime, assembly, projection, or unrelated host-routing authority. Catalog type-erasure, IndexedReady producer wiring, miss refusal, and consumer Arc reuse are this node's eval-source route, not excluded host/registry work.

## Acceptance, performance, aborts, and verification

- **CCA1GE-AC1:** structural evidence finds no production `.eval_source(...)` combined-trait dispatch, declaration, or framework implementation, and no production host-blanking eval-source producer.
- **CCA1GE-AC2:** Vue/Svelte eval bytes and source kind remain equivalent for fresh, preloaded, and incremental inputs; a planted hardcoded-framework selection fails.
- **CCA1GE-AC3:** stale/cancelled outputs cannot publish or warm, and edit-revert equals fresh; catalog miss refuses and does not publish or warm `ScriptAnalysisSnapshot::default()` as successful analysis.
- **CCA1GE-AC4:** one request performs one semantic-backend call; `IndexedReady` stores that `Arc<str>` as-is; component-meta and other consumers reuse it with no second catalog call, `String` copy, duplicate parse, semantic pass, or retained candidate; absent/inapplicable work stays zero.

Ceiling: 1200 production LOC, 20 production files, 2 crates (`verter_compiler`, `verter_session`). Abort on an unlisted production file, semantic divergence, a second resolver, or any template-fact mutation. Run focused compiler/session eval-source and Svelte conformance evidence plus `targeted-domain`; CCA1G joins this result with CCA1GT.
