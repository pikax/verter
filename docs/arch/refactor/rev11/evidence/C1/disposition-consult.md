Reading prompt from stdin...
OpenAI Codex v0.149.0-alpha.4
--------
workdir: <block worktree root>
model: gpt-5.6-sol
provider: openai
approval: never
sandbox: read-only
reasoning effort: xhigh
reasoning summaries: none
session id: 01a02bd4-a13c-7930-864c-ef38e673144a
--------
user
Read-only architecture consult. Do not write or edit any file. This is a disposition
request for a foundational, critical-path refactor charter ("C1") against fresh recon
findings that show the charter's own research is wrong or incomplete on several points.

Ground truth, in order:
1. Charter: docs/arch/refactor/rev11/charters/C1.md (binding ruling document:
   docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md — read both).
2. CLAUDE.md's global rules, especially "Exactly one type-resolution engine",
   "Planning" (no scope-shrinking for effort/breadth), and "Fix Quality" /
   "Explicit finding disposition" (ADOPT-NOW / DEFER / REJECT).
3. The recon findings below, produced by an independent read-only pass over the
   CURRENT tree (not the charter's tip).

# Recon findings requiring disposition

Each numbered item is a place where the charter's Legacy Deletions / convergence map
/ Context section makes a factual claim about the current tree that recon found to be
wrong, or a coupling the charter did not account for. For each, decide: **ADOPT-NOW**
(the correction is a same-scope clarification the ruling's already-decided boundary
already covers — implementer proceeds under the corrected understanding, no new
ruling needed) or **REOPEN** (this is a genuine new fork under the charter's own
Abort/rescope clause: "a discovered second query-time resolution path this research
did not find", "evidence that ProjectResolver is not... cleanly separable... in a way
full-coverage AttemptOutcome conversion cannot resolve", or equivalent — requiring a
second architecture challenge before implementation proceeds).

**F1 — Missing fourth carve-out file.** The charter names exactly three files that
hold `&VerterHost` and must stay in `verter_session` as adapter carve-outs
(`host_resolver_context.rs`, `session_resolver_context.rs`, the `impl ResolverContext
for VerterHost` block). Recon found a FOURTH: `resolver_core/request_store_view.rs`,
which the existing seal-bridge guard
(`crates/verter_session/tests/cases/architecture_guards.rs:~3652-3657`) already
exempts alongside the other three, and whose `CanonicalCompletionOverlay::
complete_canonical`/`complete_canonical_with_session_view`/`complete_canonical_inner`
write through host epoch / scheduler / `project_type_store` in production (not
`#[cfg(test)]`).

**F2 — `component_meta/` and `component_meta_query_engine/` are not dependency-neutral
in the sense the charter claims.** The charter's Context §1 verifies only that neither
subdirectory holds a `&VerterHost`/`Arc<VerterHost>` field or parameter in production
code, and concludes from that alone that both move wholesale with the rest of the
`resolver_core` wildcard. Recon found both subdirectories import heavily from OTHER
verter_session modules that are NOT in the charter's move set and have no stated
disposition: `component_meta/mod.rs` imports `crate::meta_resolve::
SurfaceNodeIdentities`, `crate::typeinfo::framework_surface::{vue_exec,svelte_exec}`,
`crate::host_executor::HostSourceData`, `crate::request_context`;
`component_meta/native_props.rs` imports `crate::semantic_query`, `crate::typeinfo::
surface`, `crate::meta_resolve`, `crate::typeinfo::raise`; `component_meta_query_
engine/` production code imports `crate::semantic_query`, `crate::meta_resolve`,
`crate::component_meta_materialize`, `crate::component_meta_caches`, `crate::
fact_signature_helpers`, `crate::cache_runtime`, `crate::request_context`, `crate::
structural_carrier_producer`, `crate::types` — none of which the charter lists for
relocation. Moving `component_meta/`+`component_meta_query_engine/` into
`verter_semantic` as-is, without also moving (or otherwise resolving) those session
modules, creates a fresh `verter_semantic -> verter_session` edge — which directly
violates C1-AC-2's exact requirement ("verter_semantic's production closure contains
neither verter_workspace, verter_session, verter_scheduler, nor verter_tsgo_api on any
target"). Leaving `component_meta_query_engine/` in `verter_session` instead
contradicts the charter's own stated target ("TypeInfoCore... lives in verter_semantic")
for whatever query-time resolution it performs, and — per recon's flagged risk —
is the single most likely way this block accidentally creates a SECOND query-time
resolution path (leaving `ProjectSemanticDispatch::execute` reachable from both a
session-resident query-engine facade and the relocated semantic-resident kernel).

**F3 — The `ResolverContext` trait as currently written cannot physically relocate
into `verter_semantic`.** The charter's convergence map says "Trait + `sealed` module
+ collapsed shared boilerplate relocate with the kernel — `resolver_context.rs` is
split, not moved whole" (implying the trait DEFINITION itself moves). Recon found the
production trait body directly names session/scheduler-only types in non-carve-out
regions: a `host_for_fact_tracer_install(&self) -> &crate::VerterHost` method: a
default `cancellation_token` returning/calling `verter_scheduler::cancellation::*`;
`ensure_indexed_ready_serve` returning `crate::host_manage::prepared_decl::
IndexedReadyServe`; `project_type_store` returning `&Arc<ProjectTypeStore>`; `config`
returning `&HostConfig`. None of these are inside the three-file carve-out. The
charter's own Fork-3 ruling text is explicit that the NEW observation interface "does
not extend ResolverContext" specifically BECAUSE a subtrait would inherit `ensure_
loaded`/the host escape hatch — but the charter's Legacy Deletions text describes
relocating the EXISTING `ResolverContext` trait wholesale (minus the VerterHost impl
block), which is a different trait shape than what Fork 3 ruled must not travel.

**F4 — `verter_workspace::resolver::ProjectResolver` is not a leaf; it is embedded in
`WorkspaceSnapshot`/`engine`/`project_graph`.** `WorkspaceSnapshot.resolver:
ProjectResolver` (`workspace_snapshot.rs:46`), constructed in `engine.rs:2277`,
`project_graph.rs:172`. `resolve_tracked` additionally takes
`&crate::engine::TrackedResolutionCapability` + `&crate::resolution_currency::
TransactionReader` (workspace-engine-coupled types). The charter's convergence map
and C1-AC-9 describe converting "any I/O [ProjectResolver] performs directly" to the
`AttemptOutcome`/`LoadSet` pattern, but does not address this struct-embedding: once
`ProjectResolver` relocates into `verter_semantic`, either `WorkspaceSnapshot` (which
stays in `verter_workspace`) gains a NEW downward `verter_workspace -> verter_semantic`
dependency to hold the field (an edge direction the charter's convergence map never
states as intended, though the ruling's crate-layering allows workspace depending on
semantic — verify), or the field type must become something else (a handle/id, not the
resolver value itself).

**F5 — `verter_lsp::project_resolver` re-export is unaccounted for.**
`crates/verter_lsp/src/project_resolver.rs:1` is `pub use verter_semantic::analysis::
project_resolver::*;`, which re-exports the WHOLE workspace resolver surface
(`NativeProjectResolver`, `IdeProjectConfig`, etc.) through the module the charter
says loses only its `:1-30` re-export half. If only that half is deleted and
`ModuleResolverCore`'s public names are not re-exported from the same path, this LSP
consumer breaks. Not named anywhere in the charter's Legacy Deletions or convergence
map.

**F6 (context, not requiring disposition — confirm only) — No fourth production
`ResolverContext` implementor exists** (recon confirmed exactly three `sealed::Sealed`
impls: `VerterHost`, `HostResolverContext`, `SessionResolverContext`), so that specific
named Abort/rescope trigger does not fire on its own. Confirm this reading is correct.

# What I need from you

For EACH of F1-F5: a clear **ADOPT-NOW** or **REOPEN** verdict, with reasoning tied to
the charter's own Abort/rescope clause and the four ruling forks (quote the relevant
clause). For every ADOPT-NOW, state the corrected scope/rule the implementer should
follow (e.g., "add request_store_view.rs as a fourth carve-out, apply the same
disposition logic as the other three"). For every REOPEN, state precisely what
question a second architecture challenge would need to resolve, and whether there is
a narrower ADOPT-NOW available that keeps the ruling's decided positions intact
without a full reopen (the charter itself notes two things are implementation
judgment, not forks — apply that same test here).

Be decisive. Default posture: prefer ADOPT-NOW wherever the correction is "the
charter's boundary rule already decided this, the file inventory was just
incomplete/wrong" — reserve REOPEN for genuine boundary-rule conflicts the four
rulings did not already resolve. State your confidence and flag anything you are
unsure about rather than asserting it as settled.

codex
I’ll trace each finding against the binding ruling, C1’s abort/rescope language, and the current dependency surfaces. I’ll keep this strictly read-only and treat incomplete file inventory as ADOPT-NOW unless it actually changes one of the four decided boundaries.
exec
/bin/zsh -lc 'wc -l docs/arch/refactor/rev11/charters/C1.md docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md CLAUDE.md && rg -n "Abort|rescope|fork|ResolverContext|ProjectResolver|component_meta|request_store_view|Legacy Deletions|convergence|Exactly one type-resolution engine|Planning|Fix Quality|Explicit finding disposition|ADOPT-NOW|DEFER|REJECT" docs/arch/refactor/rev11/charters/C1.md docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md CLAUDE.md' in <block worktree root>
 succeeded in 110ms:
CLAUDE.md:33:- Consumer packages (`@verter/component-meta`, LSP, MCP, unplugin, playground) consume the shared substrate, not their own semantic forks.
CLAUDE.md:40:**Exactly one type-resolution engine.** `SemanticQueryKey` → `ProjectSemanticDispatch::execute` → `SemanticGraphStore`, five query modes (`Identity`/`Navigate`/`Shallow`/`Expanded`/`Skeleton`) — the SOLE query-time type resolver. OXC is the syntax/lowering front-end ONLY (declaration bodies lower to `TypeExpr` lazily on first semantic demand through the scheduler-retained parse snapshot — the `DeclBodyMemo` owned by `IndexedReady`); OXC must never resolve types at query time. Any second query-time resolution path — a parallel `resolve_type` engine, a per-surface walker, a re-parse-and-resolve, an OXC element/frontier resolver — is a rule violation: delete it, route through the shared resolver. Two engines diverge; divergence is the bug/hang class.
CLAUDE.md:86:**Project-global cache (final state):** `VerterHost` owns a single `ProjectTypeStore` accessed via `.project_type_store()` — the sole shared cache graph: `FileArtifactStore`, `AnalysisReadyDb`, the rehomed `RouteDb`, `OwnerImportSurfaceDb`, `ComponentMetaResultDb<ComponentMetaAnalysis>`, `MaterializeStructureDb`, `SemanticGraphStore` (which also owns the Vue macro resolution artifacts — the former `ResolvedNamedTypesDb`), `ShapeCacheDb`, `FlowSliceStores` (the flow-return substrate's per-function graph/slice artifact stores), and the `IntrinsicRegistry`. `IndexedReady` is the single canonical post-parse artifact (the former `ModuleFactsDb` is retired). Validated cache writes record a `ReadSetSignature.facts` fact signature (the path-precise fact-tracer observation set) — the sole cache-validity rail, revalidated against the live `StoreView` on every warm hit. The `StoreViewValidationToken` is the complete reuse/validity oracle; the singleflight LANE identity is the narrower `external_supersession_fingerprint` (reuse-oracle = full token; lane-identity = external fingerprint). See `/host-session` (store-view token dimensions, token-advance rules, lane identity, singleflight, `RequestStoreView`/`CanonicalCompletionOverlay`, handle-backed dims), `/component-meta` (`get_component_meta` final-result flow, `resolve_owner_direct_import`, `materialize_component_meta_structure`, the `ShapeCacheDb` per-member route, `reduce_published_field_types` / sink-private `reduce_field_value_node`), `/type-cache-architecture` (admission, retired split stores), and `/type-resolution` (`execute_cooperative` dedup, `SemanticNodeData::VueMacroElements` hot path, `IntrinsicRegistry::lookup`).
CLAUDE.md:108:Guards: `cache_satisfaction_is_materialized_point_not_nominal_demand`, `cache_satisfaction_requires_path_exact_not_prefix`, `backfill_writes_only_recorded_materialized_points`, `no_off_store_host_caches`, the `r6_*` cluster, plus the four migrated-query-identity-key guards in `tests/cases/g_cache/r6_r21_query_identity_keys.rs` (`component_meta_result_key_*`, `route_name_key_*`/`barrel_surface_key_*`, `materialization_cycle_gate_key_*`, `materialization_cache_key_*`) — full list in `CRITICAL_RULE_GUARDS`.
CLAUDE.md:201:- **Open key domain ⇒ shallow carrier (L1) — route/mode-independent.** TWO families stay shallow carriers at EVERY entrance, in every mode, and open-OR-UNKNOWN (including traversal-budget exhaustion) preserves the carrier instead of falling through into Expanded materialisation: (1) an object-filter utility (`Pick`/`Omit`) whose enumeration domain is OPEN or undecidable (`Pick<PropsBase<T>, …>` over the SFC's open `generic="T"` stays `Pick<…>`); (2) a mapped type `{ [K in S]: V }` whose produced surface still depends on an unbound OUTER generic (a CLOSED-key/open-VALUE mapped enumerates its keys path-precisely with shallow values). Closed sources still materialise the requested keys path-precisely. A carrier-stopped `Pick` at a SURFACE-enumeration demand (heritage arm / macro props-slots surface) still publishes its CLOSED output-key selection from the source's enumerable arms via the shallow walker's `Pick`-carrier enumeration — the source is never whole-materialised and `Omit` (source-dependent-open output keys) stays a carrier; zero-member surface collapse was the nuxt-ui ContentSearch/DropdownMenuContent bug. Typed-IR only, no string matching. The carrier-stop is the PRIMARY defense for the open-generic class; the per-request projection budget (`request_budget.rs`) is an ARMED-by-default runaway fuse (`projection_op_budget == 0` ⇒ effective cap 2000) whose trip returns `BudgetExceeded` as a genuine partial — refused warm admission, the no-poison invariant. Publication demand is `Navigate`-only on the projector/registry macro surfaces: a full `get_component_meta` records ZERO `Published(Expanded)` projection contexts; `Table.vue` and `ChatMessages.vue` are COMPLETE corpus members with un-ignored green trackers (`table_resolves_complete_and_warm`, `chat_messages_resolves_complete_without_false_partial`, `chat_messages_resolves_without_timeout`). The FULL authoritative spec — entrances, owner predicates, the per-argument position-sensitive key-domain rule, the tri-state conditional oracle, per-utility output-key semantics, mapped family composition, OPEN/CLOSED definitions, memoization, invalidation, the `TypeOf` demand rails, and the four named current scoped exceptions — lives in `/type-resolution` → Open-Key-Domain Carrier-Stop (L1).
CLAUDE.md:230:- Substring path classification (`"/node_modules/"`, `"\\node_modules\\"`) — use `ResolverContext::workspace_is_package_backed(canonical_id)`. That predicate is the single structural authority for workspace-ownership classification, and it is what the live decision sites call directly (`component_meta_materialize.rs`, `framework/script_facts.rs`, `host_manage/jsdoc_resolve.rs`, `meta_resolve/graph_predicates.rs`, `meta_resolve/materialize/field_types.rs`, `meta_resolve/projectors/output_sink.rs`, `project_semantic_dispatch/raise.rs`/`walk.rs`, and others). Workspace-owned is its complement — there is no separate `workspace_is_workspace_owned` predicate.
CLAUDE.md:284:Multi-framework component support is ONE shared adapter substrate, not a per-framework semantic fork. `verter_session::framework` owns the `FrameworkAdapterRegistry` (built once at `VerterHost` construction), the per-adapter `FrameworkAdapterDescriptor` (identity, supported surface kinds, carrier language, the `VirtualFileNaming` column), the facts/carrier-only `FrameworkAdapterCtx`, the `ComponentDefaultSynth` seam, and the two-pass script-fact seam. Vue is the REFERENCE adapter — re-housed as a true plan/normalize adapter (`VueFrameworkAdapter` + the relocated `vue_exec` resolution delegates), NOT a privileged hardcoded path.
CLAUDE.md:307:Production external-TypeScript results for carrier sources are project-bound. The result-producing backend path is `ExternalTsProjectResolver` → `CarrierRegistry` → `EngineBackend`: `EngineBackend::ensure_project` is reached only from a resolved `ProjectBinding`, and `publish_snapshot`, `query`, and `diagnostics` require the resulting `BoundProject` witness. No production external-TS result path may infer a project from a bare path, open a carrier into a config-less/inferred project, or fall back to an inferred backend. Path-shaped transport notifications may exist below this contract, but they cannot construct external-TS results or bypass `BoundProject`.
CLAUDE.md:325:See the `/host-session` skill for the contract's three-layer structure (`ProjectResolver`/`CarrierRegistry`/`EngineBackend`), the `BoundProject` witness type-state, and the carrier-publish path.
CLAUDE.md:535:### Planning
CLAUDE.md:543:4. **Legacy Deletions** — explicit list of files, functions, code paths, feature flags to remove
CLAUDE.md:573:### Fix Quality
CLAUDE.md:578:- If the proper fix is outside approved scope, do not apply a workaround and do not use a `TODO` as its disposition. Route the finding through the applicable scope authority and record `ADOPT-NOW`, `DEFER`, or `REJECT` before related work continues. A `TODO` may reference an approved debt row but never replaces it.
CLAUDE.md:580:**Explicit finding disposition.** Every scope-deviating correctness finding is dispositioned before related work continues as `ADOPT-NOW`, `DEFER`, or `REJECT`. `ADOPT-NOW` records the scope and acceptance-contract change. `DEFER` requires a codex-DEFER ruling and a debt row naming the durable owner block, the resolution gate no later than plan close, the acceptance ID/test, and the ruling reference. `REJECT` records evidence and rationale. A TODO, a feedback entry, or an ephemeral agent identity is not a disposition; plan close requires zero open deferrals. Enforcement is judgment — codex at the scope consult, and the plan-close zero-open-deferral check.
docs/arch/refactor/rev11/charters/C1.md:8:architecture challenge against the prior draft's four open forks). All four of that draft's proposed
docs/arch/refactor/rev11/charters/C1.md:9:positions were **REJECTED**. See "Rulings applied" below for the four verdicts and their consequences; do
docs/arch/refactor/rev11/charters/C1.md:16:like an in-place refactor. **It is not.** The four rulings resolve every genuine fork in the direction
docs/arch/refactor/rev11/charters/C1.md:20:   including `component_meta/` (6 files) and `component_meta_query_engine/` (14 files): verified, neither
docs/arch/refactor/rev11/charters/C1.md:24:   wildcard, not a carve-out — see the convergence map below),
docs/arch/refactor/rev11/charters/C1.md:28:   `verter_workspace::resolver::ProjectResolver` (`crates/verter_workspace/src/resolver.rs`, 2122
docs/arch/refactor/rev11/charters/C1.md:40:Effort and breadth are not reasons to shrink this (`CLAUDE.md` → Planning: "Time constraints,
docs/arch/refactor/rev11/charters/C1.md:61:`AttemptOutcome`. It owns convergence of context/lifecycle plumbing, dependency direction, physical crate
docs/arch/refactor/rev11/charters/C1.md:79:`ResolverContext` trait with large amounts of copy-pasted delegation), by omission (no lifecycle exists
docs/arch/refactor/rev11/charters/C1.md:82:by a structural authority that forces convergence and permits a new I/O-free lifecycle to be added
docs/arch/refactor/rev11/charters/C1.md:86:- A module/name/type/relation query issued through `HostResolverContext`, `SessionResolverContext`, or
docs/arch/refactor/rev11/charters/C1.md:100:  `ModuleResolverCore` (the relocated `verter_workspace::resolver::ProjectResolver`) physically live in
docs/arch/refactor/rev11/charters/C1.md:105:  **not** extend today's `ResolverContext` and cannot name, hold, or return `VerterHost` or the
docs/arch/refactor/rev11/charters/C1.md:108:- The sealed `ResolverContext` trait (relocated to `verter_semantic`) has exactly as many
docs/arch/refactor/rev11/charters/C1.md:121:  a different completeness verdict depending on which `ResolverContext`/observation-interface implementor
docs/arch/refactor/rev11/charters/C1.md:127:  host/scheduler types at all. A subtrait of `ResolverContext` is never an acceptable substitute — it
docs/arch/refactor/rev11/charters/C1.md:135:point) — unchanged by the crate move. `ResolverContext`/observation-interface implementors are lifecycle
docs/arch/refactor/rev11/charters/C1.md:146:| C1-AC-1 | Same query, same content, different lifecycle ⇒ same resolved answer | New characterization suite driving one fixed corpus of queries through `HostResolverContext` and `SessionResolverContext` (and the new I/O-free observation interface once added), asserting structural equality of `SemanticNodeId` surfaces, sibling to `resolver_core/*_tests.rs` (relocated) |
docs/arch/refactor/rev11/charters/C1.md:149:| C1-AC-4 | A resolver-tier call site cannot reach resolution without a request-bound context | `RequestBoundResolverContext` sealed marker (relocated to `verter_semantic`) becomes the *only* production-constructible path once the bare `impl ResolverContext for VerterHost` production rail is deleted (see Legacy Deletions) — `VerterHost` stays in `verter_session` and implements the now-foreign trait for a local type, which the orphan rules permit |
docs/arch/refactor/rev11/charters/C1.md:150:| C1-AC-5 | `AttemptOutcome::{Complete, NeedInputs(LoadSet), Terminal}` covers **every** non-flow `ModuleResolverCore`/`TypeInfoCore` operation reachable from a C2 projection attempt — full coverage, not one load point | Discharged structurally, not by sampling. Per C1-AC-7 and the Authority/fallback order clause, the observation interface is the *only* surface an I/O-free (C2-shaped) caller can reach a non-flow `ModuleResolverCore`/`TypeInfoCore` operation through — `ResolverContext` itself can call `ensure_loaded`/`wait_or_drive`, so it is not usable I/O-free. "Every reachable operation" therefore reduces to "every method on this one finite, closed trait." The trait is defined so every method returns `AttemptOutcome<T>` at the type level (never a bare `T`, `Result<T, _>`, or a call that can block) — a non-conforming method is a compile error at authoring time, not a gap a test could miss. Gate: one exhaustive test double (`impl <ObservationInterface> for TestDouble`) that must implement every trait method to compile; a method added later that does not return `AttemptOutcome<T>` fails to compile at the double, not at a sampled runtime assertion. If a future operation cannot be expressed this way, that is itself a Fork-4-reopening discovery (see Abort/rescope), not a reason to fall back to sampling. |
docs/arch/refactor/rev11/charters/C1.md:151:| C1-AC-6 | Duplicated lifecycle-adapter boilerplate collapses | Diff proof: the ~10 near-identical delegation methods listed under Legacy Deletions shrink to one shared implementation; `HostResolverContext`/`SessionResolverContext` (staying in `verter_session`, implementing the relocated trait) retain only the ~9 genuinely session-specific overrides |
docs/arch/refactor/rev11/charters/C1.md:152:| C1-AC-7 | The observation interface does not extend `ResolverContext` and cannot be built holding a host/scheduler reference | Type-level: the interface is defined in `verter_semantic` with no `VerterHost`/scheduler type nameable in scope (proven by C1-AC-2's closure guard, not a separate scanner); a `trybuild`/compile-fail fixture asserting a `&VerterHost`-holding type does not satisfy the interface's bound |
docs/arch/refactor/rev11/charters/C1.md:154:| C1-AC-9 | `ModuleResolverCore` (the relocated `ProjectResolver`) has no direct scheduler/tsgo I/O left uncoverted | Any synchronous I/O call inside the relocated 2122-line resolver either does not exist in the moved code (pure computation) or is converted to the same `AttemptOutcome`/`LoadSet` pattern as `resolver_core`'s load points — audited as part of C1-AC-5's coverage sweep, not a separate carve-out |
docs/arch/refactor/rev11/charters/C1.md:159:(`performance-gates.toml:296`) — the two counters convergence work directly touches, since every
docs/arch/refactor/rev11/charters/C1.md:160:`ResolverContext` call funnels to `dispatch()` (`host_resolver_context.rs:494-500`,
docs/arch/refactor/rev11/charters/C1.md:167:*policy*, not owned by C1 — convergence must reuse them unchanged and must not introduce a second fuse
docs/arch/refactor/rev11/charters/C1.md:169:(`resolver_core/mod.rs:409-428`) with zero new heap allocation per fact — a converged `ResolverContext`
docs/arch/refactor/rev11/charters/C1.md:171:`HostResolverContext::new`/`SessionResolverContext::new` already do per request
docs/arch/refactor/rev11/charters/C1.md:174:path — a blocking `ResolverContext` call still resolves in one pass with no extra `LoadSet`
docs/arch/refactor/rev11/charters/C1.md:179:## Current-tree convergence map
docs/arch/refactor/rev11/charters/C1.md:183:| `resolver_core` / `ProjectSemanticDispatch` orchestration | `resolver_core/**` (59 files, incl. `component_meta/` (6) + `component_meta_query_engine/` (14), `verter_session`), `project_semantic_dispatch/mod.rs:309` (`verter_session`, `pub(crate)`) | Preserve resolver semantics; **physically relocate** into `verter_semantic` — **except** the three files named in the "Sealed lifecycle adapters" row below, which hold `&VerterHost` and cannot cross | `verter_semantic` (except the named adapter carve-outs) |
docs/arch/refactor/rev11/charters/C1.md:184:| TypeScript-style module/path/package resolution | `crates/verter_workspace/src/resolver.rs:122` (`ProjectResolver`, 2122 lines) | **Physically relocate** — this is the `ModuleResolverCore` target, wrongly homed in the scheduler/tsgo-dependent `verter_workspace` crate | `verter_semantic` |
docs/arch/refactor/rev11/charters/C1.md:186:| Sealed lifecycle adapters | `resolver_core/resolver_context.rs:161` (trait), `:817-1343` (`impl ResolverContext for VerterHost`, plus `VerterHost`-specific `sealed::Sealed`/`RequestBoundSealed` impls), `resolver_core/host_resolver_context.rs:189`, `resolver_core/session_resolver_context.rs:183` | Trait + `sealed` module + collapsed shared boilerplate relocate with the kernel — `resolver_context.rs` is **split**, not moved whole. The two concrete adapter structs (`HostResolverContext`, `SessionResolverContext`) and the bare-host production rail (the `:817-1343` impl block) stay/are-deleted in `verter_session` (they hold `&VerterHost`, which cannot cross into `verter_semantic`) | trait + `sealed` module: `verter_semantic`; adapters + `VerterHost` impl: `verter_session` |
docs/arch/refactor/rev11/charters/C1.md:198:different receiver (`self.inner` vs `ResolverContext::method(self.inner, ..)`) — a single shared default
docs/arch/refactor/rev11/charters/C1.md:204:712-715,171-180`). These structs stay in `verter_session` after the crate move (see convergence map
docs/arch/refactor/rev11/charters/C1.md:245:existing call site to stop blocking — `HostResolverContext`/`SessionResolverContext` keep blocking by
docs/arch/refactor/rev11/charters/C1.md:252:  `crates/verter_semantic/src/`. This includes `component_meta/` and `component_meta_query_engine/` in
docs/arch/refactor/rev11/charters/C1.md:254:  because they hold `&VerterHost` (per the "Sealed lifecycle adapters" convergence-map row): 
docs/arch/refactor/rev11/charters/C1.md:256:  ResolverContext for VerterHost` block plus its `VerterHost`-specific `sealed::Sealed`/`RequestBoundSealed`
docs/arch/refactor/rev11/charters/C1.md:265:- **`crates/verter_workspace/src/resolver.rs`** (`ProjectResolver`, 2122 lines) relocates into
docs/arch/refactor/rev11/charters/C1.md:269:- **The bare `impl ResolverContext for VerterHost`** (`resolver_context.rs:817`) production-reachable
docs/arch/refactor/rev11/charters/C1.md:273:  `verter_session`; `ResolverContext` becomes a foreign trait from `verter_session`'s perspective, which
docs/arch/refactor/rev11/charters/C1.md:275:  bare-host rail once convergence lands (verify at implementation time — every currently-known production
docs/arch/refactor/rev11/charters/C1.md:276:  entry already binds `HostResolverContext`/`SessionResolverContext`), delete the impl entirely and let
docs/arch/refactor/rev11/charters/C1.md:277:  `RequestBoundResolverContext` become the sole production-constructible rail. This turns "resolve
docs/arch/refactor/rev11/charters/C1.md:312:  (`HostResolverContext`/`SessionResolverContext`, which still legitimately block) or gains a non-blocking
docs/arch/refactor/rev11/charters/C1.md:329:- **Sealed-trait lifecycle closure.** `ResolverContext: sealed::Sealed` (relocated with the trait) already
docs/arch/refactor/rev11/charters/C1.md:331:  bare-host rail is deleted (see above), tightens `RequestBoundResolverContext` from "narrower than
docs/arch/refactor/rev11/charters/C1.md:332:  `ResolverContext`" to "identical to it" — a resolver-tier function taking `&dyn ResolverContext` becomes
docs/arch/refactor/rev11/charters/C1.md:334:  sealed trait, not a subtrait of `ResolverContext` — per the ruling, a subtrait inherits
docs/arch/refactor/rev11/charters/C1.md:383:resolved answers for identical inputs; the duplicated boilerplate between `HostResolverContext` and
docs/arch/refactor/rev11/charters/C1.md:384:`SessionResolverContext` is gone; `verter_semantic`'s production closure excludes `verter_workspace`,
docs/arch/refactor/rev11/charters/C1.md:399:| conformance | charter, diff, and the deletion set — including whether every listed relocation actually happened (not a re-export left behind), whether the bare-host `ResolverContext` rail was actually deleted (or, if retained, why a production call site still needs it), and whether A5-DD1 closed by exception-row deletion rather than a subset-checked allowance |
docs/arch/refactor/rev11/charters/C1.md:403:## Abort/rescope
docs/arch/refactor/rev11/charters/C1.md:406:`verter_workspace::resolver::ProjectResolver` is not, in fact, cleanly separable from `verter_workspace`'s
docs/arch/refactor/rev11/charters/C1.md:410:regression on `A6_META_COMPILE_40_COLD_RUST` that convergence cannot explain and correct within scope. A
docs/arch/refactor/rev11/charters/C1.md:429:3. **Fork 3 (non-blocking guarantee) — VIOLATES.** The proposed `IoFreeResolverContext` marker, as a
docs/arch/refactor/rev11/charters/C1.md:430:   subtrait of `ResolverContext`, inherits `ensure_loaded`/the host escape hatch — the draft's claim that
docs/arch/refactor/rev11/charters/C1.md:432:   decision**. Define a capability-limited observation interface that does not extend `ResolverContext`
docs/arch/refactor/rev11/charters/C1.md:442:**New open questions created by these rulings: none.** All four forks resolved to a specific, actionable
docs/arch/refactor/rev11/charters/C1.md:445:judgment calls *within* the ruling's already-decided boundary, not further forks: (a) the exact intra-crate
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:35:Read it first. Its author honestly flagged four unresolved design forks rather than guessing. Your job is
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:60:  PROPOSED POSITION: enforce structurally with a sealed `IoFreeResolverContext` marker (mirroring the
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:61:  existing `RequestBoundResolverContext`), so a resolver-tier function generic over it cannot reach
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:80:  "Planning"). If the correct answer is a large breaking change, say so.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:82:- Be concise. file:line throughout. End with four verdict lines, one per fork.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:120:convergence of context/lifecycle plumbing, dependency direction, and the addition of a batched,
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:130:`ResolverContext` trait with large amounts of copy-pasted delegation) and by omission (no lifecycle exists
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:131:yet that cannot block a thread on I/O), not by a structural authority that forces convergence and permits
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:135:- A module/name/type/relation query issued through `HostResolverContext`, `SessionResolverContext`, or
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:144:- The sealed `ResolverContext` trait (`resolver_core/resolver_context.rs:161`) has exactly as many
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:156:  a different completeness verdict depending on which `ResolverContext` implementor served it, for
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:163:point). `ResolverContext` implementors are lifecycle *adapters* over that one authority, never alternate
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:173:| C1-AC-1 | Same query, same content, different lifecycle ⇒ same resolved answer | New characterization suite driving one fixed corpus of queries through `HostResolverContext` and `SessionResolverContext` (and the new I/O-free context once added), asserting structural equality of `SemanticNodeId` surfaces, sibling to `resolver_core/*_tests.rs` |
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:175:| C1-AC-3 | Authority-uniqueness contract holds after convergence | Existing `project_semantic_dispatch_invariants_tests.rs` + the five-row Authority-uniqueness contract stay green, unmodified in substance |
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:176:| C1-AC-4 | A resolver-tier call site cannot reach resolution without a request-bound context | `RequestBoundResolverContext` sealed marker (`resolver_context.rs:780-783`) becomes the *only* production-constructible path if the bare `impl ResolverContext for VerterHost` production rail is deleted (see Legacy Deletions) |
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:178:| C1-AC-6 | Duplicated lifecycle-adapter boilerplate collapses | Diff proof: the ~10 near-identical delegation methods listed under Changes shrink to one shared implementation; `HostResolverContext`/`SessionResolverContext` retain only the ~9 genuinely session-specific overrides |
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:183:(`performance-gates.toml:296`) — the two counters convergence work directly touches, since every
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:184:`ResolverContext` call funnels to `dispatch()` (`host_resolver_context.rs:494-500`,
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:189:`union_member_explosion=100`) are budget *policy*, not owned by C1 — convergence must reuse them unchanged
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:192:converged `ResolverContext` construction path must not add a clone, extra `Arc` construction, or
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:193:normalization pass beyond what `HostResolverContext::new`/`SessionResolverContext::new` already do per
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:198:## Current-tree convergence map
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:203:| TypeScript-style module/path/package resolution | `crates/verter_workspace/src/resolver.rs:122` (`ProjectResolver`, 2122 lines) | Converge — physically the `ModuleResolverCore` target, wrongly homed in the scheduler/tsgo-dependent `verter_workspace` crate | C1 |
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:216:same delegation shape over a different receiver (`self.inner` vs `ResolverContext::method(self.inner,
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:251:- The bare `impl ResolverContext for VerterHost` (`resolver_context.rs:817`) production-reachable method
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:255:  bare-host rail once convergence lands (verify at implementation time — every currently-known production
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:256:  entry already binds `HostResolverContext`/`SessionResolverContext`), delete the impl entirely and let
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:257:  `RequestBoundResolverContext` (the sealed marker already excluding `VerterHost`,
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:289:- **Sealed-trait lifecycle closure.** `ResolverContext: sealed::Sealed` (`resolver_context.rs:82-98,161`)
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:291:  if the bare-host rail is deleted (see above), tightens `RequestBoundResolverContext`
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:293:  `ResolverContext`" to "identical to it" — a resolver-tier function taking `&dyn ResolverContext` becomes
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:308:  existing seal, a marker type for "I/O-free context" analogous to `RequestBoundResolverContext`, or a
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:313:  I/O-free context (e.g. an `IoFreeResolverContext` sealed subtrait, mirroring
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:314:  `RequestBoundResolverContext`'s shape) that is never implemented for any type holding a `&VerterHost`
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:329:authority may exist after `D2`"). If convergence work touches `project_semantic_dispatch/relation.rs`,
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:337:prepare/plan/project/emit state machine. C1's deliverable to C2 is: a `ResolverContext`-shaped kernel
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:346:three (or more, once the I/O-free context exists) `ResolverContext` implementors give bit-identical
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:347:resolved answers for identical inputs; the duplicated boilerplate between `HostResolverContext` and
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:348:`SessionResolverContext` is gone; `verter_semantic`'s production closure excludes `verter_scheduler` and
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:365:| conformance | charter, diff, and the deletion set — including whether the bare-host `ResolverContext` rail was actually deleted (or, if retained, why a production call site still needs it), and whether A5-DD1 closed without a subset-checked allowance |
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:369:## Abort/rescope
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:372:`verter_workspace::resolver::ProjectResolver` is not, in fact, cleanly separable from `verter_workspace`'s
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:375:research did not find; or a locked-cell regression on `A6_META_COMPILE_40_COLD_RUST` that convergence
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:381:implementation against them without a ruling — each is a real design fork, not a detail:
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:399:   not-yet-written committed-input-authority design. Flagged, not resolved, in Legacy Deletions above.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:401:   review-enforced one.** I recommend a sealed `IoFreeResolverContext`-shaped marker (mirroring
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:402:   `RequestBoundResolverContext`) so a resolver-tier function generic over it cannot reach
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:448:- Consumer packages (`@verter/component-meta`, LSP, MCP, unplugin, playground) consume the shared substrate, not their own semantic forks.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:455:**Exactly one type-resolution engine.** `SemanticQueryKey` → `ProjectSemanticDispatch::execute` → `SemanticGraphStore`, five query modes (`Identity`/`Navigate`/`Shallow`/`Expanded`/`Skeleton`) — the SOLE query-time type resolver. OXC is the syntax/lowering front-end ONLY (declaration bodies lower to `TypeExpr` lazily on first semantic demand through the scheduler-retained parse snapshot — the `DeclBodyMemo` owned by `IndexedReady`); OXC must never resolve types at query time. Any second query-time resolution path — a parallel `resolve_type` engine, a per-surface walker, a re-parse-and-resolve, an OXC element/frontier resolver — is a rule violation: delete it, route through the shared resolver. Two engines diverge; divergence is the bug/hang class.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:501:**Project-global cache (final state):** `VerterHost` owns a single `ProjectTypeStore` accessed via `.project_type_store()` — the sole shared cache graph: `FileArtifactStore`, `AnalysisReadyDb`, the rehomed `RouteDb`, `OwnerImportSurfaceDb`, `ComponentMetaResultDb<ComponentMetaAnalysis>`, `MaterializeStructureDb`, `SemanticGraphStore` (which also owns the Vue macro resolution artifacts — the former `ResolvedNamedTypesDb`), `ShapeCacheDb`, `FlowSliceStores` (the flow-return substrate's per-function graph/slice artifact stores), and the `IntrinsicRegistry`. `IndexedReady` is the single canonical post-parse artifact (the former `ModuleFactsDb` is retired). Validated cache writes record a `ReadSetSignature.facts` fact signature (the path-precise fact-tracer observation set) — the sole cache-validity rail, revalidated against the live `StoreView` on every warm hit. The `StoreViewValidationToken` is the complete reuse/validity oracle; the singleflight LANE identity is the narrower `external_supersession_fingerprint` (reuse-oracle = full token; lane-identity = external fingerprint). See `/host-session` (store-view token dimensions, token-advance rules, lane identity, singleflight, `RequestStoreView`/`CanonicalCompletionOverlay`, handle-backed dims), `/component-meta` (`get_component_meta` final-result flow, `resolve_owner_direct_import`, `materialize_component_meta_structure`, the `ShapeCacheDb` per-member route, `reduce_published_field_types` / sink-private `reduce_field_value_node`), `/type-cache-architecture` (admission, retired split stores), and `/type-resolution` (`execute_cooperative` dedup, `SemanticNodeData::VueMacroElements` hot path, `IntrinsicRegistry::lookup`).
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:523:Guards: `cache_satisfaction_is_materialized_point_not_nominal_demand`, `cache_satisfaction_requires_path_exact_not_prefix`, `backfill_writes_only_recorded_materialized_points`, `no_off_store_host_caches`, the `r6_*` cluster, plus the four migrated-query-identity-key guards in `tests/cases/g_cache/r6_r21_query_identity_keys.rs` (`component_meta_result_key_*`, `route_name_key_*`/`barrel_surface_key_*`, `materialization_cycle_gate_key_*`, `materialization_cache_key_*`) — full list in `CRITICAL_RULE_GUARDS`.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:616:- **Open key domain ⇒ shallow carrier (L1) — route/mode-independent.** TWO families stay shallow carriers at EVERY entrance, in every mode, and open-OR-UNKNOWN (including traversal-budget exhaustion) preserves the carrier instead of falling through into Expanded materialisation: (1) an object-filter utility (`Pick`/`Omit`) whose enumeration domain is OPEN or undecidable (`Pick<PropsBase<T>, …>` over the SFC's open `generic="T"` stays `Pick<…>`); (2) a mapped type `{ [K in S]: V }` whose produced surface still depends on an unbound OUTER generic (a CLOSED-key/open-VALUE mapped enumerates its keys path-precisely with shallow values). Closed sources still materialise the requested keys path-precisely. A carrier-stopped `Pick` at a SURFACE-enumeration demand (heritage arm / macro props-slots surface) still publishes its CLOSED output-key selection from the source's enumerable arms via the shallow walker's `Pick`-carrier enumeration — the source is never whole-materialised and `Omit` (source-dependent-open output keys) stays a carrier; zero-member surface collapse was the nuxt-ui ContentSearch/DropdownMenuContent bug. Typed-IR only, no string matching. The carrier-stop is the PRIMARY defense for the open-generic class; the per-request projection budget (`request_budget.rs`) is an ARMED-by-default runaway fuse (`projection_op_budget == 0` ⇒ effective cap 2000) whose trip returns `BudgetExceeded` as a genuine partial — refused warm admission, the no-poison invariant. Publication demand is `Navigate`-only on the projector/registry macro surfaces: a full `get_component_meta` records ZERO `Published(Expanded)` projection contexts; `Table.vue` and `ChatMessages.vue` are COMPLETE corpus members with un-ignored green trackers (`table_resolves_complete_and_warm`, `chat_messages_resolves_complete_without_false_partial`, `chat_messages_resolves_without_timeout`). The FULL authoritative spec — entrances, owner predicates, the per-argument position-sensitive key-domain rule, the tri-state conditional oracle, per-utility output-key semantics, mapped family composition, OPEN/CLOSED definitions, memoization, invalidation, the `TypeOf` demand rails, and the four named current scoped exceptions — lives in `/type-resolution` → Open-Key-Domain Carrier-Stop (L1).
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:645:- Substring path classification (`"/node_modules/"`, `"\\node_modules\\"`) — use `ResolverContext::workspace_is_package_backed(canonical_id)`. That predicate is the single structural authority for workspace-ownership classification, and it is what the live decision sites call directly (`component_meta_materialize.rs`, `framework/script_facts.rs`, `host_manage/jsdoc_resolve.rs`, `meta_resolve/graph_predicates.rs`, `meta_resolve/materialize/field_types.rs`, `meta_resolve/projectors/output_sink.rs`, `project_semantic_dispatch/raise.rs`/`walk.rs`, and others). Workspace-owned is its complement — there is no separate `workspace_is_workspace_owned` predicate.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:699:Multi-framework component support is ONE shared adapter substrate, not a per-framework semantic fork. `verter_session::framework` owns the `FrameworkAdapterRegistry` (built once at `VerterHost` construction), the per-adapter `FrameworkAdapterDescriptor` (identity, supported surface kinds, carrier language, the `VirtualFileNaming` column), the facts/carrier-only `FrameworkAdapterCtx`, the `ComponentDefaultSynth` seam, and the two-pass script-fact seam. Vue is the REFERENCE adapter — re-housed as a true plan/normalize adapter (`VueFrameworkAdapter` + the relocated `vue_exec` resolution delegates), NOT a privileged hardcoded path.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:722:Production external-TypeScript results for carrier sources are project-bound. The result-producing backend path is `ExternalTsProjectResolver` → `CarrierRegistry` → `EngineBackend`: `EngineBackend::ensure_project` is reached only from a resolved `ProjectBinding`, and `publish_snapshot`, `query`, and `diagnostics` require the resulting `BoundProject` witness. No production external-TS result path may infer a project from a bare path, open a carrier into a config-less/inferred project, or fall back to an inferred backend. Path-shaped transport notifications may exist below this contract, but they cannot construct external-TS results or bypass `BoundProject`.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:740:See the `/host-session` skill for the contract's three-layer structure (`ProjectResolver`/`CarrierRegistry`/`EngineBackend`), the `BoundProject` witness type-state, and the carrier-publish path.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:930:### Planning
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:938:4. **Legacy Deletions** — explicit list of files, functions, code paths, feature flags to remove
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:968:### Fix Quality
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:973:- If the proper fix is outside approved scope, do not apply a workaround and do not use a `TODO` as its disposition. Route the finding through the applicable scope authority and record `ADOPT-NOW`, `DEFER`, or `REJECT` before related work continues. A `TODO` may reference an approved debt row but never replaces it.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:975:**Explicit finding disposition.** Every scope-deviating correctness finding is dispositioned before related work continues as `ADOPT-NOW`, `DEFER`, or `REJECT`. `ADOPT-NOW` records the scope and acceptance-contract change. `DEFER` requires a codex-DEFER ruling and a debt row naming the durable owner block, the resolution gate no later than plan close, the acceptance ID/test, and the ruling reference. `REJECT` records evidence and rationale. A TODO, a feedback entry, or an ephemeral agent identity is not a disposition; plan close requires zero open deferrals. Enforcement is judgment — codex at the scope consult, and the plan-close zero-open-deferral check.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:1077:I’m using the project’s architecture, type-resolution, and host-session skills because the forks cross ownership, resolver authority, and I/O/session boundaries. Their constraints are part of the evidence: I’ll apply them only to this read-only ruling.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:1113:- Consumer packages (`@verter/component-meta`, LSP, MCP, unplugin, playground) consume the shared substrate, not their own semantic forks.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:1120:**Exactly one type-resolution engine.** `SemanticQueryKey` → `ProjectSemanticDispatch::execute` → `SemanticGraphStore`, five query modes (`Identity`/`Navigate`/`Shallow`/`Expanded`/`Skeleton`) — the SOLE query-time type resolver. OXC is the syntax/lowering front-end ONLY (declaration bodies lower to `TypeExpr` lazily on first semantic demand through the scheduler-retained parse snapshot — the `DeclBodyMemo` owned by `IndexedReady`); OXC must never resolve types at query time. Any second query-time resolution path — a parallel `resolve_type` engine, a per-surface walker, a re-parse-and-resolve, an OXC element/frontier resolver — is a rule violation: delete it, route through the shared resolver. Two engines diverge; divergence is the bug/hang class.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:1166:**Project-global cache (final state):** `VerterHost` owns a single `ProjectTypeStore` accessed via `.project_type_store()` — the sole shared cache graph: `FileArtifactStore`, `AnalysisReadyDb`, the rehomed `RouteDb`, `OwnerImportSurfaceDb`, `ComponentMetaResultDb<ComponentMetaAnalysis>`, `MaterializeStructureDb`, `SemanticGraphStore` (which also owns the Vue macro resolution artifacts — the former `ResolvedNamedTypesDb`), `ShapeCacheDb`, `FlowSliceStores` (the flow-return substrate's per-function graph/slice artifact stores), and the `IntrinsicRegistry`. `IndexedReady` is the single canonical post-parse artifact (the former `ModuleFactsDb` is retired). Validated cache writes record a `ReadSetSignature.facts` fact signature (the path-precise fact-tracer observation set) — the sole cache-validity rail, revalidated against the live `StoreView` on every warm hit. The `StoreViewValidationToken` is the complete reuse/validity oracle; the singleflight LANE identity is the narrower `external_supersession_fingerprint` (reuse-oracle = full token; lane-identity = external fingerprint). See `/host-session` (store-view token dimensions, token-advance rules, lane identity, singleflight, `RequestStoreView`/`CanonicalCompletionOverlay`, handle-backed dims), `/component-meta` (`get_component_meta` final-result flow, `resolve_owner_direct_import`, `materialize_component_meta_structure`, the `ShapeCacheDb` per-member route, `reduce_published_field_types` / sink-private `reduce_field_value_node`), `/type-cache-architecture` (admission, retired split stores), and `/type-resolution` (`execute_cooperative` dedup, `SemanticNodeData::VueMacroElements` hot path, `IntrinsicRegistry::lookup`).
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:1188:Guards: `cache_satisfaction_is_materialized_point_not_nominal_demand`, `cache_satisfaction_requires_path_exact_not_prefix`, `backfill_writes_only_recorded_materialized_points`, `no_off_store_host_caches`, the `r6_*` cluster, plus the four migrated-query-identity-key guards in `tests/cases/g_cache/r6_r21_query_identity_keys.rs` (`component_meta_result_key_*`, `route_name_key_*`/`barrel_surface_key_*`, `materialization_cycle_gate_key_*`, `materialization_cache_key_*`) — full list in `CRITICAL_RULE_GUARDS`.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:1281:- **Open key domain ⇒ shallow carrier (L1) — route/mode-independent.** TWO families stay shallow carriers at EVERY entrance, in every mode, and open-OR-UNKNOWN (including traversal-budget exhaustion) preserves the carrier instead of falling through into Expanded materialisation: (1) an object-filter utility (`Pick`/`Omit`) whose enumeration domain is OPEN or undecidable (`Pick<PropsBase<T>, …>` over the SFC's open `generic="T"` stays `Pick<…>`); (2) a mapped type `{ [K in S]: V }` whose produced surface still depends on an unbound OUTER generic (a CLOSED-key/open-VALUE mapped enumerates its keys path-precisely with shallow values). Closed sources still materialise the requested keys path-precisely. A carrier-stopped `Pick` at a SURFACE-enumeration demand (heritage arm / macro props-slots surface) still publishes its CLOSED output-key selection from the source's enumerable arms via the shallow walker's `Pick`-carrier enumeration — the source is never whole-materialised and `Omit` (source-dependent-open output keys) stays a carrier; zero-member surface collapse was the nuxt-ui ContentSearch/DropdownMenuContent bug. Typed-IR only, no string matching. The carrier-stop is the PRIMARY defense for the open-generic class; the per-request projection budget (`request_budget.rs`) is an ARMED-by-default runaway fuse (`projection_op_budget == 0` ⇒ effective cap 2000) whose trip returns `BudgetExceeded` as a genuine partial — refused warm admission, the no-poison invariant. Publication demand is `Navigate`-only on the projector/registry macro surfaces: a full `get_component_meta` records ZERO `Published(Expanded)` projection contexts; `Table.vue` and `ChatMessages.vue` are COMPLETE corpus members with un-ignored green trackers (`table_resolves_complete_and_warm`, `chat_messages_resolves_complete_without_false_partial`, `chat_messages_resolves_without_timeout`). The FULL authoritative spec — entrances, owner predicates, the per-argument position-sensitive key-domain rule, the tri-state conditional oracle, per-utility output-key semantics, mapped family composition, OPEN/CLOSED definitions, memoization, invalidation, the `TypeOf` demand rails, and the four named current scoped exceptions — lives in `/type-resolution` → Open-Key-Domain Carrier-Stop (L1).
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:1310:- Substring path classification (`"/node_modules/"`, `"\\node_modules\\"`) — use `ResolverContext::workspace_is_package_backed(canonical_id)`. That predicate is the single structural authority for workspace-ownership classification, and it is what the live decision sites call directly (`component_meta_materialize.rs`, `framework/script_facts.rs`, `host_manage/jsdoc_resolve.rs`, `meta_resolve/graph_predicates.rs`, `meta_resolve/materialize/field_types.rs`, `meta_resolve/projectors/output_sink.rs`, `project_semantic_dispatch/raise.rs`/`walk.rs`, and others). Workspace-owned is its complement — there is no separate `workspace_is_workspace_owned` predicate.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:1364:Multi-framework component support is ONE shared adapter substrate, not a per-framework semantic fork. `verter_session::framework` owns the `FrameworkAdapterRegistry` (built once at `VerterHost` construction), the per-adapter `FrameworkAdapterDescriptor` (identity, supported surface kinds, carrier language, the `VirtualFileNaming` column), the facts/carrier-only `FrameworkAdapterCtx`, the `ComponentDefaultSynth` seam, and the two-pass script-fact seam. Vue is the REFERENCE adapter — re-housed as a true plan/normalize adapter (`VueFrameworkAdapter` + the relocated `vue_exec` resolution delegates), NOT a privileged hardcoded path.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:1387:Production external-TypeScript results for carrier sources are project-bound. The result-producing backend path is `ExternalTsProjectResolver` → `CarrierRegistry` → `EngineBackend`: `EngineBackend::ensure_project` is reached only from a resolved `ProjectBinding`, and `publish_snapshot`, `query`, and `diagnostics` require the resulting `BoundProject` witness. No production external-TS result path may infer a project from a bare path, open a carrier into a config-less/inferred project, or fall back to an inferred backend. Path-shaped transport notifications may exist below this contract, but they cannot construct external-TS results or bypass `BoundProject`.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:1405:See the `/host-session` skill for the contract's three-layer structure (`ProjectResolver`/`CarrierRegistry`/`EngineBackend`), the `BoundProject` witness type-state, and the carrier-publish path.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:1595:### Planning
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:1603:4. **Legacy Deletions** — explicit list of files, functions, code paths, feature flags to remove
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:1633:### Fix Quality
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:1638:- If the proper fix is outside approved scope, do not apply a workaround and do not use a `TODO` as its disposition. Route the finding through the applicable scope authority and record `ADOPT-NOW`, `DEFER`, or `REJECT` before related work continues. A `TODO` may reference an approved debt row but never replaces it.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:1640:**Explicit finding disposition.** Every scope-deviating correctness finding is dispositioned before related work continues as `ADOPT-NOW`, `DEFER`, or `REJECT`. `ADOPT-NOW` records the scope and acceptance-contract change. `DEFER` requires a codex-DEFER ruling and a debt row naming the durable owner block, the resolution gate no later than plan close, the acceptance ID/test, and the ruling reference. `REJECT` records evidence and rationale. A TODO, a feedback entry, or an ephemeral agent identity is not a disposition; plan close requires zero open deferrals. Enforcement is judgment — codex at the scope consult, and the plan-close zero-open-deferral check.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:1776:- Consumer packages (`@verter/component-meta`, LSP, MCP, unplugin, playground) consume the shared substrate, not their own semantic forks.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:1783:**Exactly one type-resolution engine.** `SemanticQueryKey` → `ProjectSemanticDispatch::execute` → `SemanticGraphStore`, five query modes (`Identity`/`Navigate`/`Shallow`/`Expanded`/`Skeleton`) — the SOLE query-time type resolver. OXC is the syntax/lowering front-end ONLY (declaration bodies lower to `TypeExpr` lazily on first semantic demand through the scheduler-retained parse snapshot — the `DeclBodyMemo` owned by `IndexedReady`); OXC must never resolve types at query time. Any second query-time resolution path — a parallel `resolve_type` engine, a per-surface walker, a re-parse-and-resolve, an OXC element/frontier resolver — is a rule violation: delete it, route through the shared resolver. Two engines diverge; divergence is the bug/hang class.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:1829:**Project-global cache (final state):** `VerterHost` owns a single `ProjectTypeStore` accessed via `.project_type_store()` — the sole shared cache graph: `FileArtifactStore`, `AnalysisReadyDb`, the rehomed `RouteDb`, `OwnerImportSurfaceDb`, `ComponentMetaResultDb<ComponentMetaAnalysis>`, `MaterializeStructureDb`, `SemanticGraphStore` (which also owns the Vue macro resolution artifacts — the former `ResolvedNamedTypesDb`), `ShapeCacheDb`, `FlowSliceStores` (the flow-return substrate's per-function graph/slice artifact stores), and the `IntrinsicRegistry`. `IndexedReady` is the single canonical post-parse artifact (the former `ModuleFactsDb` is retired). Validated cache writes record a `ReadSetSignature.facts` fact signature (the path-precise fact-tracer observation set) — the sole cache-validity rail, revalidated against the live `StoreView` on every warm hit. The `StoreViewValidationToken` is the complete reuse/validity oracle; the singleflight LANE identity is the narrower `external_supersession_fingerprint` (reuse-oracle = full token; lane-identity = external fingerprint). See `/host-session` (store-view token dimensions, token-advance rules, lane identity, singleflight, `RequestStoreView`/`CanonicalCompletionOverlay`, handle-backed dims), `/component-meta` (`get_component_meta` final-result flow, `resolve_owner_direct_import`, `materialize_component_meta_structure`, the `ShapeCacheDb` per-member route, `reduce_published_field_types` / sink-private `reduce_field_value_node`), `/type-cache-architecture` (admission, retired split stores), and `/type-resolution` (`execute_cooperative` dedup, `SemanticNodeData::VueMacroElements` hot path, `IntrinsicRegistry::lookup`).
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:1851:Guards: `cache_satisfaction_is_materialized_point_not_nominal_demand`, `cache_satisfaction_requires_path_exact_not_prefix`, `backfill_writes_only_recorded_materialized_points`, `no_off_store_host_caches`, the `r6_*` cluster, plus the four migrated-query-identity-key guards in `tests/cases/g_cache/r6_r21_query_identity_keys.rs` (`component_meta_result_key_*`, `route_name_key_*`/`barrel_surface_key_*`, `materialization_cycle_gate_key_*`, `materialization_cache_key_*`) — full list in `CRITICAL_RULE_GUARDS`.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:1948:- **Open key domain ⇒ shallow carrier (L1) — route/mode-independent.** TWO families stay shallow carriers at EVERY entrance, in every mode, and open-OR-UNKNOWN (including traversal-budget exhaustion) preserves the carrier instead of falling through into Expanded materialisation: (1) an object-filter utility (`Pick`/`Omit`) whose enumeration domain is OPEN or undecidable (`Pick<PropsBase<T>, …>` over the SFC's open `generic="T"` stays `Pick<…>`); (2) a mapped type `{ [K in S]: V }` whose produced surface still depends on an unbound OUTER generic (a CLOSED-key/open-VALUE mapped enumerates its keys path-precisely with shallow values). Closed sources still materialise the requested keys path-precisely. A carrier-stopped `Pick` at a SURFACE-enumeration demand (heritage arm / macro props-slots surface) still publishes its CLOSED output-key selection from the source's enumerable arms via the shallow walker's `Pick`-carrier enumeration — the source is never whole-materialised and `Omit` (source-dependent-open output keys) stays a carrier; zero-member surface collapse was the nuxt-ui ContentSearch/DropdownMenuContent bug. Typed-IR only, no string matching. The carrier-stop is the PRIMARY defense for the open-generic class; the per-request projection budget (`request_budget.rs`) is an ARMED-by-default runaway fuse (`projection_op_budget == 0` ⇒ effective cap 2000) whose trip returns `BudgetExceeded` as a genuine partial — refused warm admission, the no-poison invariant. Publication demand is `Navigate`-only on the projector/registry macro surfaces: a full `get_component_meta` records ZERO `Published(Expanded)` projection contexts; `Table.vue` and `ChatMessages.vue` are COMPLETE corpus members with un-ignored green trackers (`table_resolves_complete_and_warm`, `chat_messages_resolves_complete_without_false_partial`, `chat_messages_resolves_without_timeout`). The FULL authoritative spec — entrances, owner predicates, the per-argument position-sensitive key-domain rule, the tri-state conditional oracle, per-utility output-key semantics, mapped family composition, OPEN/CLOSED definitions, memoization, invalidation, the `TypeOf` demand rails, and the four named current scoped exceptions — lives in `/type-resolution` → Open-Key-Domain Carrier-Stop (L1).
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:1977:- Substring path classification (`"/node_modules/"`, `"\\node_modules\\"`) — use `ResolverContext::workspace_is_package_backed(canonical_id)`. That predicate is the single structural authority for workspace-ownership classification, and it is what the live decision sites call directly (`component_meta_materialize.rs`, `framework/script_facts.rs`, `host_manage/jsdoc_resolve.rs`, `meta_resolve/graph_predicates.rs`, `meta_resolve/materialize/field_types.rs`, `meta_resolve/projectors/output_sink.rs`, `project_semantic_dispatch/raise.rs`/`walk.rs`, and others). Workspace-owned is its complement — there is no separate `workspace_is_workspace_owned` predicate.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:2031:Multi-framework component support is ONE shared adapter substrate, not a per-framework semantic fork. `verter_session::framework` owns the `FrameworkAdapterRegistry` (built once at `VerterHost` construction), the per-adapter `FrameworkAdapterDescriptor` (identity, supported surface kinds, carrier language, the `VirtualFileNaming` column), the facts/carrier-only `FrameworkAdapterCtx`, the `ComponentDefaultSynth` seam, and the two-pass script-fact seam. Vue is the REFERENCE adapter — re-housed as a true plan/normalize adapter (`VueFrameworkAdapter` + the relocated `vue_exec` resolution delegates), NOT a privileged hardcoded path.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:2054:Production external-TypeScript results for carrier sources are project-bound. The result-producing backend path is `ExternalTsProjectResolver` → `CarrierRegistry` → `EngineBackend`: `EngineBackend::ensure_project` is reached only from a resolved `ProjectBinding`, and `publish_snapshot`, `query`, and `diagnostics` require the resulting `BoundProject` witness. No production external-TS result path may infer a project from a bare path, open a carrier into a config-less/inferred project, or fall back to an inferred backend. Path-shaped transport notifications may exist below this contract, but they cannot construct external-TS results or bypass `BoundProject`.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:2072:See the `/host-session` skill for the contract's three-layer structure (`ProjectResolver`/`CarrierRegistry`/`EngineBackend`), the `BoundProject` witness type-state, and the carrier-publish path.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:2270:### Planning
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:2278:4. **Legacy Deletions** — explicit list of files, functions, code paths, feature flags to remove
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:2308:### Fix Quality
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:2313:- If the proper fix is outside approved scope, do not apply a workaround and do not use a `TODO` as its disposition. Route the finding through the applicable scope authority and record `ADOPT-NOW`, `DEFER`, or `REJECT` before related work continues. A `TODO` may reference an approved debt row but never replaces it.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:2315:**Explicit finding disposition.** Every scope-deviating correctness finding is dispositioned before related work continues as `ADOPT-NOW`, `DEFER`, or `REJECT`. `ADOPT-NOW` records the scope and acceptance-contract change. `DEFER` requires a codex-DEFER ruling and a debt row naming the durable owner block, the resolution gate no later than plan close, the acceptance ID/test, and the ruling reference. `REJECT` records evidence and rationale. A TODO, a feedback entry, or an ephemeral agent identity is not a disposition; plan close requires zero open deferrals. Enforcement is judgment — codex at the scope consult, and the plan-close zero-open-deferral check.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:2436:- `verter_session::resolver_core` owns the host-backed resolver stack and type-resolution orchestration. Resolver-path methods receive `ctx: &dyn ResolverContext` (sealed super-trait at `resolver_core/resolver_context.rs`) — only `VerterHost` implements it, enforced by the `no_concrete_verter_host_in_seal_scope` arch-guard.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:2670:- `ComponentMetaResultDb<ComponentMetaAnalysis>` — final payload cache for `get_component_meta`. Warm hits revalidate the recorded `ReadSetSignature.facts` fact signature against the live `StoreView` before returning.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:2705:- `ResolverContext::prepared_type_decl` preserves `Result<Option<Arc<PreparedTypeDecl>>, PreparationFailure>` end to end. `MissingExternalOwner` and `AuthoredOrdinalOverflow` are typed failures, never declaration absence: the prepared slot stays vacant, and an Option-shaped semantic boundary may serve them only through the single ReturnOnly adapter that marks the enclosing derivation non-cacheable. `LeaseMiss` remains the distinct recoverable `Ok(None)` + non-cacheable rail.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:2805:  (`SessionResolverContext::resolve_type_dependency_canonical` resolves through
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:3025:**Query-free structural carrier emission (LIVE macro-arg producer).** A separate, session-owned structural lowerer (`crates/verter_session/src/structural_carrier_producer/macro_arg_producer.rs`, entry `lower_type_expr_structural`, a BARE module-private fn — the producer is COLLAPSED into the ONE private module `crate::structural_carrier_producer::macro_arg_producer`, declared as a private `mod macro_arg_producer;` that re-exports only `macro_type_arg_hot_ref` + `MacroHotMirror` `pub(crate)`; the FOREIGN case is compiler-confined by module privacy and the SAME-MODULE residual is policed by the strengthened single-producer guards) emits the unresolved carriers directly from an owned `TypeExpr`, NodeScopeId-rooted, performing NO name / import / type resolution or reduction — it is a PRODUCER of carriers, never a second resolver, so it neither competes with nor duplicates the single demand-time resolution engine. A bare `Foo` becomes `BareRef`; `Foo<Arg>` a `BareRef` whose `type_args` are structurally lowered (never an `InstantiationRef`); `import("…")` an `ImportType`; unsupported raw syntax a `RawFallback` (display/compat only, never a control-flow miss); a construct-signature type a `Signature { kind: Construct }` node; tuple rest stays on `TupleElement.rest`; and `keyof` / indexed-access / conditional / mapped / `typeof` lower to their deferred shells carrying structurally-lowered operands — even where the eager path would reduce them. The only "resolution" it performs is the purely syntactic in-scope binder lookup that maps a `Ref` to a type-parameter / `infer` binder it already interned. It is intern-only (it never reaches `ProjectSemanticDispatch`, a `CarrierResolverContext`, a `SemanticQueryKey`, or any host / type-provider state) and stays demand-time — never pulled into publish or indexing (the `indexed_ready_publish_lowers_zero_decl_bodies` invariant). It is the LIVE macro-arg structural producer: macro type-argument carriers flow through production at the four macro sites, and its SOLE production caller is the session macro hot-mirror builder (Stage 5A, LANDED — see "Macro Hot Mirror" below), pinned by the module-privacy guard `structural_carrier_producer_lowerer_is_module_private` plus the ordering tripwire `no_production_macro_arg_eager_lowering_outside_mirror` and the purity guard `macro_hot_mirror_producer_is_pure_no_route_resolution`. The GLOBAL declaration-body structural flip (so a `type A = B` decl body lowers to `BareRef(B)` instead of the resolved body) is the **separate deferred query-free declaration-body structural-template producer** (NOT Stage 6, NOT landed) — the LANDED Stage 6 Option-B flip mints `HotTypeRef` handles in the `decl_body_hot_ref` accessor over the `Instantiate` result the graph-bearing dispatch producer drives via the RESOLVING lowerer, and does NOT route declaration bodies through this structural lowerer. Three further guards lock its query-freedom: `session_graph_lowerer_makes_no_query` (no query / resolver / host surface in the lowerer's production code), `unresolved_carriers_not_materialized_during_emission` (no `materialize_type_expr` / raise during emission, and the emitted root stays a carrier), and `oxc_worker_emits_no_session_graph_node` (the OXC worker / semantic-lowering surface produces owned `TypeExpr` IR only). Hermetic structural-equivalence fixtures prove its no-resolution shapes lower to the SAME interned graph as the eager `lower.rs` path.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:3029:**Carrier-arg descent + carrier-arg encapsulation.** The three arg-carrying carriers (`BareRef`/`TypeOf`/`ImportType`) are OPAQUE tuple payloads (`crates/verter_session/src/semantic_query/carrier.rs`, e.g. `TypeOf(carrier::TypeOfCarrier)`) whose fields are PRIVATE, so the anti-tail invariant is enforced BY CONSTRUCTION: the ONLY crate-wide channel to DESCEND a carrier's args is the shared exhaustive `SemanticNodeData::carrier_type_args` accessor (the carrier's own raw-args reader `arg_nodes` is PRIVATE to the carrier module), the ONLY reconstruction channel is `SemanticNodeData::map_carrier_type_args`, head fields read through `typeof_head`/`bare_ref_head`/`import_type_head` (which NEVER return `type_args`), and construction goes through `new_typeof`/`new_bare_ref`/`new_import_type`. Those eight sanctioned accessors LIVE in an `impl SemanticNodeData` block INSIDE the `semantic_query::carrier` module, alongside the PRIVATE carrier payload methods (`arg_nodes`/`with_type_args`/head getters/`new`) — so the raw-args surface is compiler-confined to that one file (a sibling `impl carrier::BareRefCarrier` in `semantic_query.rs` reading `self.type_args` fails `E0616`), NOT merely `pub(super)`-to-the-parent, which makes the file-scoped shape guard COMPLETE. Because the field is private, a hand-rolled `node.type_args` direct bind on one of these three sealed carriers is UNREPRESENTABLE outside the carrier module REGARDLESS of how the variant is named (qualified, bare/imported, or `use … as Alias` renamed) and regardless of `cfg` / `#[path]` / `include!` / macro expansion — the compiler resolves the exact compiled program, which the retired `CARRIER_TYPEARGS_*` source scanner never could. The earlier "STATICALLY-FORBIDDEN variant-literal binds" + "aliased-RENAME residual NOT statically caught" framing is therefore MOOT (a private field cannot be bound, aliased or not). The SCAN/CLASSIFY walkers — the absorb infer-scan, the open-node value-body / enumeration-domain walk, and the `subtree_references` reachability scan (the `build_mapped_type` key-independence hoist) — descend via `carrier_type_args`; the reconstruction / render / identity boundaries split by whether they REBUILD: raise/materialize and display read head fields via the head accessors and descend args via `carrier_type_args` to materialize / render (NO rebuild), while the substitute re-intern arm ALONE rebuilds the carrier node via `map_carrier_type_args` (after descending args via `carrier_type_args`); `eq`/`hash` compare/hash the opaque payload (derived on the carrier). The surviving compile-fences are the exhaustive, wildcard-free `carrier_type_args` (descent) and `map_carrier_type_args` (rebuild) matches: a new carrier variant FAILS TO COMPILE there until its author classifies it. The compiler-enforced BY-CONSTRUCTION proof covers the three sealed arg-carrying carriers (private payload fields); the fences force a future carrier to be CLASSIFIED but NOT ENCAPSULATED (classification ≠ encapsulation) — a future named-struct carrier with a public `type_args` field could compile and re-expose the bind, so the BY-CONSTRUCTION guarantee is per-carrier (the sealed payloads); that enum-wide gap is CLOSED by the PRE-EXISTING (pre-Stage-6) enum-wide structural guard `no_named_type_args_field_outside_opaque_carrier` (rejecting a named `type_args` field on any `SemanticNodeData` variant unless it is an opaque `Variant(carrier::VariantCarrier)`), introduced before the Stage-6 work and orthogonal to it (registered + implemented; `docs/arch/parselower-design.md` tracks it). Tripwires: `no_named_type_args_field_outside_opaque_carrier` (the enum-wide named-`type_args`-field rejection), `carrier_variants_are_opaque_tuple_payloads` (the payload's FULL path `carrier::{Name}Carrier`, not just its final segment — an unqualified / wrong-module / raw `Arc<[SemanticNodeId]>` payload is rejected), `carrier_module_has_no_public_type_args_surface` (a RECURSIVE scan of carrier.rs: fields private, NO manual trait impl on a carrier struct / nested module / free fn / item macro / non-allowlisted derive / `#[cfg_attr]`, EVERY carrier inherent method PRIVATE, and the only `pub(crate)`/`pub` methods are the sanctioned eight `SemanticNodeData` accessors), `carrier_type_args_accessor_is_exhaustive_and_wildcard_free`, `map_carrier_type_args_is_exhaustive_and_wildcard_free` (both inspect the `match self` specifically; `crates/verter_session/tests/cases/carrier_encapsulation_guards.rs`). `is_deferred` (the relation-pair deferred-root predicate) and `exactness.rs::object_is_closed_node` (which root-kind-matches a `BareRef`/`TypeOf`/`ImportType`-valued member to `false` / object-OPEN) are ROOT-KIND classifiers: their verdict is complete from the carrier's root kind alone, they make NO `carrier_type_args` call and descend no args — NOT accessor-descent sites. The `meta_resolve` ref/cycle/dep walkers (`graph_predicates.rs::{body_contains_recursive_ref_to_name, collect_ref_identities_node}`, `slot_binding_graph.rs::accumulate_lowered_node_carrier_deps`), the `build.rs` type-param collector (`collect_type_param_nodes_by_name`), and the free-type-param classifier `slot_binding_graph.rs::node_contains_free_type_param` now ALSO descend a carrier's args via `carrier_type_args` (args-only — they collect NO head identity, since a `BareRef`/`ImportType` head is unresolved and a `TypeOf` head is a value root). A NEW scanner that root-kind-matches a carrier and silently IGNORES its args is caught by neither encapsulation nor the accessor fence — the defense is the wildcard-free accessor (forces a NEW variant to be classified) plus review. A `BareRef`/`ImportType` HEAD resolves only through the ONE dispatch (the `lower.rs` bare-name / import-augmentation / enum / builtin-shadowing `Ref` path), never ad-hoc in a consumer walker; `CarrierResolverContext` is the value-side bundle (never a query key) that head-resolution helpers consume when those carriers are resolved as a query subject. The `lower_type_expr_structural` producer of `BareRef`/`ImportType` carriers is LIVE for the macro-arg path (its sole production caller is the macro hot-mirror builder; the raw lowerer is a BARE module-private fn in the single private producer module `crate::structural_carrier_producer::macro_arg_producer`, reachable from outside only through the `pub(crate)` `macro_type_arg_hot_ref` entry — see "Macro Hot Mirror" below), so those carrier heads resolve at the demand through the ONE dispatch; the GLOBAL declaration-body producer flip that lets `BareRef`/`ImportType` carriers flow from declaration bodies end-to-end is the separate deferred query-free declaration-body structural-template producer (NOT Stage 6, NOT landed) — the LANDED Stage 6 Option-B flip mints handles in `decl_body_hot_ref` (over the `Instantiate` result the producer drives via the resolving lowerer) rather than routing decl bodies through the structural lowerer (`docs/arch/parselower-design.md` tracks both).
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:3039:**Family-1 SURFACE-position demand point — `Pick`-carrier enumeration.** A `Pick` carrier's OUTPUT key set is exactly its CLOSED key-selection `K` even when the SOURCE's key domain is open, so at a SURFACE-enumeration demand (the shallow walker's carrier unwrap — a heritage arm, a macro props/slots surface arm) an L1 carrier-stopped `Pick` does NOT contribute zero members: `walk.rs::visit_shallow_node` detects the verbatim carrier round-trip (`object_filter_carrier_surface_filter`), walks the SOURCE through the ordinary carrier-preserving frames (open arms stay deferred and contribute nothing, exactly as the direct un-filtered route publishes them — never a whole-open-source materialisation), and filters the enumerable surface to `K` at `Frame::FlushObjectFilter` (public members only, signatures dropped — `build_builtin_utility` member parity). Dropping the picked members instead was the nuxt-ui ContentSearch (18 heritage props) / DropdownMenuContent (5 generic-value-dependent slot keys) zero-member collapse. `Omit` deliberately does NOT participate: its output key set (`keyof Source − K`) is source-dependent-open, so an `Omit` carrier stays a carrier at every position (`get_component_meta_table_shaped_open_omit_*` pins it). VALUE-position publication of an open `Pick` still stays a shallow carrier (`chatmessages_resolvable_barrel_publishes_open_pick_as_shallow_carrier`); the surface-position pin is `get_component_meta_chat_messages_shaped_open_pick_heritage_enumerates_picked_keys_only` plus the `pick_over_genuinely_open_source_*` / `pick_over_instantiated_generic_*` regressions in `component_meta_pick_omit_tests.rs`.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:3098:3. The intrinsic/fallthrough `ProjectPath:Published:Expanded` corpus exception — defined by `dispatch_helpers.rs`'s Class-A Expanded fall-through `project_expr_class_a_via_dispatch_threaded` (reached directly or via its non-threaded wrapper `project_expr_class_a_via_dispatch`) and its live callers: `intrinsic_members_for_tag`'s intrinsic/fallthrough composition in `host_manage/intrinsic_projection.rs` (records `ProjectPath:Published:Expanded` charges on the real corpus — measured: 214 on a ChatMessages resolve), the value-expression evaluator in `resolver_core/fallthrough.rs`, the imported-alias registry refinement in `host_manage/component_meta_methods.rs`, the registry materialiser's route-target and `KeyOf` publication sites in `meta_resolve/registry_materialize.rs`, and the JSDoc payload resolution in `host_manage/jsdoc_resolve.rs`. These sites legitimately demand Expanded today and are out of the Navigate-only-publication rule's scope — `publication_routes_never_demand_expanded` covers the projector/registry macro surfaces only; the correct end-state converts this consumer set to Navigate.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:3258:**Query-free structural carrier emission (LIVE macro-arg producer).** A separate, session-owned structural lowerer (`crates/verter_session/src/structural_carrier_producer/macro_arg_producer.rs`, entry `lower_type_expr_structural`, a BARE module-private fn — the producer is COLLAPSED into the ONE private module `crate::structural_carrier_producer::macro_arg_producer`, declared as a private `mod macro_arg_producer;` that re-exports only `macro_type_arg_hot_ref` + `MacroHotMirror` `pub(crate)`; the FOREIGN case is compiler-confined by module privacy and the SAME-MODULE residual is policed by the strengthened single-producer guards) emits the unresolved carriers directly from an owned `TypeExpr`, NodeScopeId-rooted, performing NO name / import / type resolution or reduction — it is a PRODUCER of carriers, never a second resolver, so it neither competes with nor duplicates the single demand-time resolution engine. A bare `Foo` becomes `BareRef`; `Foo<Arg>` a `BareRef` whose `type_args` are structurally lowered (never an `InstantiationRef`); `import("…")` an `ImportType`; unsupported raw syntax a `RawFallback` (display/compat only, never a control-flow miss); a construct-signature type a `Signature { kind: Construct }` node; tuple rest stays on `TupleElement.rest`; and `keyof` / indexed-access / conditional / mapped / `typeof` lower to their deferred shells carrying structurally-lowered operands — even where the eager path would reduce them. The only "resolution" it performs is the purely syntactic in-scope binder lookup that maps a `Ref` to a type-parameter / `infer` binder it already interned. It is intern-only (it never reaches `ProjectSemanticDispatch`, a `CarrierResolverContext`, a `SemanticQueryKey`, or any host / type-provider state) and stays demand-time — never pulled into publish or indexing (the `indexed_ready_publish_lowers_zero_decl_bodies` invariant). It is the LIVE macro-arg structural producer: macro type-argument carriers flow through production at the four macro sites, and its SOLE production caller is the session macro hot-mirror builder (Stage 5A, LANDED — see "Macro Hot Mirror" below), pinned by the module-privacy guard `structural_carrier_producer_lowerer_is_module_private` plus the ordering tripwire `no_production_macro_arg_eager_lowering_outside_mirror` and the purity guard `macro_hot_mirror_producer_is_pure_no_route_resolution`. The GLOBAL declaration-body structural flip (so a `type A = B` decl body lowers to `BareRef(B)` instead of the resolved body) is the **separate deferred query-free declaration-body structural-template producer** (NOT Stage 6, NOT landed) — the LANDED Stage 6 Option-B flip mints `HotTypeRef` handles in the `decl_body_hot_ref` accessor over the `Instantiate` result the graph-bearing dispatch producer drives via the RESOLVING lowerer, and does NOT route declaration bodies through this structural lowerer. Three further guards lock its query-freedom: `session_graph_lowerer_makes_no_query` (no query / resolver / host surface in the lowerer's production code), `unresolved_carriers_not_materialized_during_emission` (no `materialize_type_expr` / raise during emission, and the emitted root stays a carrier), and `oxc_worker_emits_no_session_graph_node` (the OXC worker / semantic-lowering surface produces owned `TypeExpr` IR only). Hermetic structural-equivalence fixtures prove its no-resolution shapes lower to the SAME interned graph as the eager `lower.rs` path.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:3262:**Carrier-arg descent + carrier-arg encapsulation.** The three arg-carrying carriers (`BareRef`/`TypeOf`/`ImportType`) are OPAQUE tuple payloads (`crates/verter_session/src/semantic_query/carrier.rs`, e.g. `TypeOf(carrier::TypeOfCarrier)`) whose fields are PRIVATE, so the anti-tail invariant is enforced BY CONSTRUCTION: the ONLY crate-wide channel to DESCEND a carrier's args is the shared exhaustive `SemanticNodeData::carrier_type_args` accessor (the carrier's own raw-args reader `arg_nodes` is PRIVATE to the carrier module), the ONLY reconstruction channel is `SemanticNodeData::map_carrier_type_args`, head fields read through `typeof_head`/`bare_ref_head`/`import_type_head` (which NEVER return `type_args`), and construction goes through `new_typeof`/`new_bare_ref`/`new_import_type`. Those eight sanctioned accessors LIVE in an `impl SemanticNodeData` block INSIDE the `semantic_query::carrier` module, alongside the PRIVATE carrier payload methods (`arg_nodes`/`with_type_args`/head getters/`new`) — so the raw-args surface is compiler-confined to that one file (a sibling `impl carrier::BareRefCarrier` in `semantic_query.rs` reading `self.type_args` fails `E0616`), NOT merely `pub(super)`-to-the-parent, which makes the file-scoped shape guard COMPLETE. Because the field is private, a hand-rolled `node.type_args` direct bind on one of these three sealed carriers is UNREPRESENTABLE outside the carrier module REGARDLESS of how the variant is named (qualified, bare/imported, or `use … as Alias` renamed) and regardless of `cfg` / `#[path]` / `include!` / macro expansion — the compiler resolves the exact compiled program, which the retired `CARRIER_TYPEARGS_*` source scanner never could. The earlier "STATICALLY-FORBIDDEN variant-literal binds" + "aliased-RENAME residual NOT statically caught" framing is therefore MOOT (a private field cannot be bound, aliased or not). The SCAN/CLASSIFY walkers — the absorb infer-scan, the open-node value-body / enumeration-domain walk, and the `subtree_references` reachability scan (the `build_mapped_type` key-independence hoist) — descend via `carrier_type_args`; the reconstruction / render / identity boundaries split by whether they REBUILD: raise/materialize and display read head fields via the head accessors and descend args via `carrier_type_args` to materialize / render (NO rebuild), while the substitute re-intern arm ALONE rebuilds the carrier node via `map_carrier_type_args` (after descending args via `carrier_type_args`); `eq`/`hash` compare/hash the opaque payload (derived on the carrier). The surviving compile-fences are the exhaustive, wildcard-free `carrier_type_args` (descent) and `map_carrier_type_args` (rebuild) matches: a new carrier variant FAILS TO COMPILE there until its author classifies it. The compiler-enforced BY-CONSTRUCTION proof covers the three sealed arg-carrying carriers (private payload fields); the fences force a future carrier to be CLASSIFIED but NOT ENCAPSULATED (classification ≠ encapsulation) — a future named-struct carrier with a public `type_args` field could compile and re-expose the bind, so the BY-CONSTRUCTION guarantee is per-carrier (the sealed payloads); that enum-wide gap is CLOSED by the PRE-EXISTING (pre-Stage-6) enum-wide structural guard `no_named_type_args_field_outside_opaque_carrier` (rejecting a named `type_args` field on any `SemanticNodeData` variant unless it is an opaque `Variant(carrier::VariantCarrier)`), introduced before the Stage-6 work and orthogonal to it (registered + implemented; `docs/arch/parselower-design.md` tracks it). Tripwires: `no_named_type_args_field_outside_opaque_carrier` (the enum-wide named-`type_args`-field rejection), `carrier_variants_are_opaque_tuple_payloads` (the payload's FULL path `carrier::{Name}Carrier`, not just its final segment — an unqualified / wrong-module / raw `Arc<[SemanticNodeId]>` payload is rejected), `carrier_module_has_no_public_type_args_surface` (a RECURSIVE scan of carrier.rs: fields private, NO manual trait impl on a carrier struct / nested module / free fn / item macro / non-allowlisted derive / `#[cfg_attr]`, EVERY carrier inherent method PRIVATE, and the only `pub(crate)`/`pub` methods are the sanctioned eight `SemanticNodeData` accessors), `carrier_type_args_accessor_is_exhaustive_and_wildcard_free`, `map_carrier_type_args_is_exhaustive_and_wildcard_free` (both inspect the `match self` specifically; `crates/verter_session/tests/cases/carrier_encapsulation_guards.rs`). `is_deferred` (the relation-pair deferred-root predicate) and `exactness.rs::object_is_closed_node` (which root-kind-matches a `BareRef`/`TypeOf`/`ImportType`-valued member to `false` / object-OPEN) are ROOT-KIND classifiers: their verdict is complete from the carrier's root kind alone, they make NO `carrier_type_args` call and descend no args — NOT accessor-descent sites. The `meta_resolve` ref/cycle/dep walkers (`graph_predicates.rs::{body_contains_recursive_ref_to_name, collect_ref_identities_node}`, `slot_binding_graph.rs::accumulate_lowered_node_carrier_deps`), the `build.rs` type-param collector (`collect_type_param_nodes_by_name`), and the free-type-param classifier `slot_binding_graph.rs::node_contains_free_type_param` now ALSO descend a carrier's args via `carrier_type_args` (args-only — they collect NO head identity, since a `BareRef`/`ImportType` head is unresolved and a `TypeOf` head is a value root). A NEW scanner that root-kind-matches a carrier and silently IGNORES its args is caught by neither encapsulation nor the accessor fence — the defense is the wildcard-free accessor (forces a NEW variant to be classified) plus review. A `BareRef`/`ImportType` HEAD resolves only through the ONE dispatch (the `lower.rs` bare-name / import-augmentation / enum / builtin-shadowing `Ref` path), never ad-hoc in a consumer walker; `CarrierResolverContext` is the value-side bundle (never a query key) that head-resolution helpers consume when those carriers are resolved as a query subject. The `lower_type_expr_structural` producer of `BareRef`/`ImportType` carriers is LIVE for the macro-arg path (its sole production caller is the macro hot-mirror builder; the raw lowerer is a BARE module-private fn in the single private producer module `crate::structural_carrier_producer::macro_arg_producer`, reachable from outside only through the `pub(crate)` `macro_type_arg_hot_ref` entry — see "Macro Hot Mirror" below), so those carrier heads resolve at the demand through the ONE dispatch; the GLOBAL declaration-body producer flip that lets `BareRef`/`ImportType` carriers flow from declaration bodies end-to-end is the separate deferred query-free declaration-body structural-template producer (NOT Stage 6, NOT landed) — the LANDED Stage 6 Option-B flip mints handles in `decl_body_hot_ref` (over the `Instantiate` result the producer drives via the resolving lowerer) rather than routing decl bodies through the structural lowerer (`docs/arch/parselower-design.md` tracks both).
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:3272:**Family-1 SURFACE-position demand point — `Pick`-carrier enumeration.** A `Pick` carrier's OUTPUT key set is exactly its CLOSED key-selection `K` even when the SOURCE's key domain is open, so at a SURFACE-enumeration demand (the shallow walker's carrier unwrap — a heritage arm, a macro props/slots surface arm) an L1 carrier-stopped `Pick` does NOT contribute zero members: `walk.rs::visit_shallow_node` detects the verbatim carrier round-trip (`object_filter_carrier_surface_filter`), walks the SOURCE through the ordinary carrier-preserving frames (open arms stay deferred and contribute nothing, exactly as the direct un-filtered route publishes them — never a whole-open-source materialisation), and filters the enumerable surface to `K` at `Frame::FlushObjectFilter` (public members only, signatures dropped — `build_builtin_utility` member parity). Dropping the picked members instead was the nuxt-ui ContentSearch (18 heritage props) / DropdownMenuContent (5 generic-value-dependent slot keys) zero-member collapse. `Omit` deliberately does NOT participate: its output key set (`keyof Source − K`) is source-dependent-open, so an `Omit` carrier stays a carrier at every position (`get_component_meta_table_shaped_open_omit_*` pins it). VALUE-position publication of an open `Pick` still stays a shallow carrier (`chatmessages_resolvable_barrel_publishes_open_pick_as_shallow_carrier`); the surface-position pin is `get_component_meta_chat_messages_shaped_open_pick_heritage_enumerates_picked_keys_only` plus the `pick_over_genuinely_open_source_*` / `pick_over_instantiated_generic_*` regressions in `component_meta_pick_omit_tests.rs`.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:3331:3. The intrinsic/fallthrough `ProjectPath:Published:Expanded` corpus exception — defined by `dispatch_helpers.rs`'s Class-A Expanded fall-through `project_expr_class_a_via_dispatch_threaded` (reached directly or via its non-threaded wrapper `project_expr_class_a_via_dispatch`) and its live callers: `intrinsic_members_for_tag`'s intrinsic/fallthrough composition in `host_manage/intrinsic_projection.rs` (records `ProjectPath:Published:Expanded` charges on the real corpus — measured: 214 on a ChatMessages resolve), the value-expression evaluator in `resolver_core/fallthrough.rs`, the imported-alias registry refinement in `host_manage/component_meta_methods.rs`, the registry materialiser's route-target and `KeyOf` publication sites in `meta_resolve/registry_materialize.rs`, and the JSDoc payload resolution in `host_manage/jsdoc_resolve.rs`. These sites legitimately demand Expanded today and are out of the Navigate-only-publication rule's scope — `publication_routes_never_demand_expanded` covers the projector/registry macro surfaces only; the correct end-state converts this consumer set to Navigate.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:3375:**Query-free structural carrier emission (LIVE macro-arg producer).** A separate, session-owned structural lowerer (`crates/verter_session/src/structural_carrier_producer/macro_arg_producer.rs`, entry `lower_type_expr_structural`, a BARE module-private fn — the producer is COLLAPSED into the ONE private module `crate::structural_carrier_producer::macro_arg_producer`, declared as a private `mod macro_arg_producer;` that re-exports only `macro_type_arg_hot_ref` + `MacroHotMirror` `pub(crate)`; the FOREIGN case is compiler-confined by module privacy and the SAME-MODULE residual is policed by the strengthened single-producer guards) emits the unresolved carriers directly from an owned `TypeExpr`, NodeScopeId-rooted, performing NO name / import / type resolution or reduction — it is a PRODUCER of carriers, never a second resolver, so it neither competes with nor duplicates the single demand-time resolution engine. A bare `Foo` becomes `BareRef`; `Foo<Arg>` a `BareRef` whose `type_args` are structurally lowered (never an `InstantiationRef`); `import("…")` an `ImportType`; unsupported raw syntax a `RawFallback` (display/compat only, never a control-flow miss); a construct-signature type a `Signature { kind: Construct }` node; tuple rest stays on `TupleElement.rest`; and `keyof` / indexed-access / conditional / mapped / `typeof` lower to their deferred shells carrying structurally-lowered operands — even where the eager path would reduce them. The only "resolution" it performs is the purely syntactic in-scope binder lookup that maps a `Ref` to a type-parameter / `infer` binder it already interned. It is intern-only (it never reaches `ProjectSemanticDispatch`, a `CarrierResolverContext`, a `SemanticQueryKey`, or any host / type-provider state) and stays demand-time — never pulled into publish or indexing (the `indexed_ready_publish_lowers_zero_decl_bodies` invariant). It is the LIVE macro-arg structural producer: macro type-argument carriers flow through production at the four macro sites, and its SOLE production caller is the session macro hot-mirror builder (Stage 5A, LANDED — see "Macro Hot Mirror" below), pinned by the module-privacy guard `structural_carrier_producer_lowerer_is_module_private` plus the ordering tripwire `no_production_macro_arg_eager_lowering_outside_mirror` and the purity guard `macro_hot_mirror_producer_is_pure_no_route_resolution`. The GLOBAL declaration-body structural flip (so a `type A = B` decl body lowers to `BareRef(B)` instead of the resolved body) is the **separate deferred query-free declaration-body structural-template producer** (NOT Stage 6, NOT landed) — the LANDED Stage 6 Option-B flip mints `HotTypeRef` handles in the `decl_body_hot_ref` accessor over the `Instantiate` result the graph-bearing dispatch producer drives via the RESOLVING lowerer, and does NOT route declaration bodies through this structural lowerer. Three further guards lock its query-freedom: `session_graph_lowerer_makes_no_query` (no query / resolver / host surface in the lowerer's production code), `unresolved_carriers_not_materialized_during_emission` (no `materialize_type_expr` / raise during emission, and the emitted root stays a carrier), and `oxc_worker_emits_no_session_graph_node` (the OXC worker / semantic-lowering surface produces owned `TypeExpr` IR only). Hermetic structural-equivalence fixtures prove its no-resolution shapes lower to the SAME interned graph as the eager `lower.rs` path.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:3379:**Carrier-arg descent + carrier-arg encapsulation.** The three arg-carrying carriers (`BareRef`/`TypeOf`/`ImportType`) are OPAQUE tuple payloads (`crates/verter_session/src/semantic_query/carrier.rs`, e.g. `TypeOf(carrier::TypeOfCarrier)`) whose fields are PRIVATE, so the anti-tail invariant is enforced BY CONSTRUCTION: the ONLY crate-wide channel to DESCEND a carrier's args is the shared exhaustive `SemanticNodeData::carrier_type_args` accessor (the carrier's own raw-args reader `arg_nodes` is PRIVATE to the carrier module), the ONLY reconstruction channel is `SemanticNodeData::map_carrier_type_args`, head fields read through `typeof_head`/`bare_ref_head`/`import_type_head` (which NEVER return `type_args`), and construction goes through `new_typeof`/`new_bare_ref`/`new_import_type`. Those eight sanctioned accessors LIVE in an `impl SemanticNodeData` block INSIDE the `semantic_query::carrier` module, alongside the PRIVATE carrier payload methods (`arg_nodes`/`with_type_args`/head getters/`new`) — so the raw-args surface is compiler-confined to that one file (a sibling `impl carrier::BareRefCarrier` in `semantic_query.rs` reading `self.type_args` fails `E0616`), NOT merely `pub(super)`-to-the-parent, which makes the file-scoped shape guard COMPLETE. Because the field is private, a hand-rolled `node.type_args` direct bind on one of these three sealed carriers is UNREPRESENTABLE outside the carrier module REGARDLESS of how the variant is named (qualified, bare/imported, or `use … as Alias` renamed) and regardless of `cfg` / `#[path]` / `include!` / macro expansion — the compiler resolves the exact compiled program, which the retired `CARRIER_TYPEARGS_*` source scanner never could. The earlier "STATICALLY-FORBIDDEN variant-literal binds" + "aliased-RENAME residual NOT statically caught" framing is therefore MOOT (a private field cannot be bound, aliased or not). The SCAN/CLASSIFY walkers — the absorb infer-scan, the open-node value-body / enumeration-domain walk, and the `subtree_references` reachability scan (the `build_mapped_type` key-independence hoist) — descend via `carrier_type_args`; the reconstruction / render / identity boundaries split by whether they REBUILD: raise/materialize and display read head fields via the head accessors and descend args via `carrier_type_args` to materialize / render (NO rebuild), while the substitute re-intern arm ALONE rebuilds the carrier node via `map_carrier_type_args` (after descending args via `carrier_type_args`); `eq`/`hash` compare/hash the opaque payload (derived on the carrier). The surviving compile-fences are the exhaustive, wildcard-free `carrier_type_args` (descent) and `map_carrier_type_args` (rebuild) matches: a new carrier variant FAILS TO COMPILE there until its author classifies it. The compiler-enforced BY-CONSTRUCTION proof covers the three sealed arg-carrying carriers (private payload fields); the fences force a future carrier to be CLASSIFIED but NOT ENCAPSULATED (classification ≠ encapsulation) — a future named-struct carrier with a public `type_args` field could compile and re-expose the bind, so the BY-CONSTRUCTION guarantee is per-carrier (the sealed payloads); that enum-wide gap is CLOSED by the PRE-EXISTING (pre-Stage-6) enum-wide structural guard `no_named_type_args_field_outside_opaque_carrier` (rejecting a named `type_args` field on any `SemanticNodeData` variant unless it is an opaque `Variant(carrier::VariantCarrier)`), introduced before the Stage-6 work and orthogonal to it (registered + implemented; `docs/arch/parselower-design.md` tracks it). Tripwires: `no_named_type_args_field_outside_opaque_carrier` (the enum-wide named-`type_args`-field rejection), `carrier_variants_are_opaque_tuple_payloads` (the payload's FULL path `carrier::{Name}Carrier`, not just its final segment — an unqualified / wrong-module / raw `Arc<[SemanticNodeId]>` payload is rejected), `carrier_module_has_no_public_type_args_surface` (a RECURSIVE scan of carrier.rs: fields private, NO manual trait impl on a carrier struct / nested module / free fn / item macro / non-allowlisted derive / `#[cfg_attr]`, EVERY carrier inherent method PRIVATE, and the only `pub(crate)`/`pub` methods are the sanctioned eight `SemanticNodeData` accessors), `carrier_type_args_accessor_is_exhaustive_and_wildcard_free`, `map_carrier_type_args_is_exhaustive_and_wildcard_free` (both inspect the `match self` specifically; `crates/verter_session/tests/cases/carrier_encapsulation_guards.rs`). `is_deferred` (the relation-pair deferred-root predicate) and `exactness.rs::object_is_closed_node` (which root-kind-matches a `BareRef`/`TypeOf`/`ImportType`-valued member to `false` / object-OPEN) are ROOT-KIND classifiers: their verdict is complete from the carrier's root kind alone, they make NO `carrier_type_args` call and descend no args — NOT accessor-descent sites. The `meta_resolve` ref/cycle/dep walkers (`graph_predicates.rs::{body_contains_recursive_ref_to_name, collect_ref_identities_node}`, `slot_binding_graph.rs::accumulate_lowered_node_carrier_deps`), the `build.rs` type-param collector (`collect_type_param_nodes_by_name`), and the free-type-param classifier `slot_binding_graph.rs::node_contains_free_type_param` now ALSO descend a carrier's args via `carrier_type_args` (args-only — they collect NO head identity, since a `BareRef`/`ImportType` head is unresolved and a `TypeOf` head is a value root). A NEW scanner that root-kind-matches a carrier and silently IGNORES its args is caught by neither encapsulation nor the accessor fence — the defense is the wildcard-free accessor (forces a NEW variant to be classified) plus review. A `BareRef`/`ImportType` HEAD resolves only through the ONE dispatch (the `lower.rs` bare-name / import-augmentation / enum / builtin-shadowing `Ref` path), never ad-hoc in a consumer walker; `CarrierResolverContext` is the value-side bundle (never a query key) that head-resolution helpers consume when those carriers are resolved as a query subject. The `lower_type_expr_structural` producer of `BareRef`/`ImportType` carriers is LIVE for the macro-arg path (its sole production caller is the macro hot-mirror builder; the raw lowerer is a BARE module-private fn in the single private producer module `crate::structural_carrier_producer::macro_arg_producer`, reachable from outside only through the `pub(crate)` `macro_type_arg_hot_ref` entry — see "Macro Hot Mirror" below), so those carrier heads resolve at the demand through the ONE dispatch; the GLOBAL declaration-body producer flip that lets `BareRef`/`ImportType` carriers flow from declaration bodies end-to-end is the separate deferred query-free declaration-body structural-template producer (NOT Stage 6, NOT landed) — the LANDED Stage 6 Option-B flip mints handles in `decl_body_hot_ref` (over the `Instantiate` result the producer drives via the resolving lowerer) rather than routing decl bodies through the structural lowerer (`docs/arch/parselower-design.md` tracks both).
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:3389:**Family-1 SURFACE-position demand point — `Pick`-carrier enumeration.** A `Pick` carrier's OUTPUT key set is exactly its CLOSED key-selection `K` even when the SOURCE's key domain is open, so at a SURFACE-enumeration demand (the shallow walker's carrier unwrap — a heritage arm, a macro props/slots surface arm) an L1 carrier-stopped `Pick` does NOT contribute zero members: `walk.rs::visit_shallow_node` detects the verbatim carrier round-trip (`object_filter_carrier_surface_filter`), walks the SOURCE through the ordinary carrier-preserving frames (open arms stay deferred and contribute nothing, exactly as the direct un-filtered route publishes them — never a whole-open-source materialisation), and filters the enumerable surface to `K` at `Frame::FlushObjectFilter` (public members only, signatures dropped — `build_builtin_utility` member parity). Dropping the picked members instead was the nuxt-ui ContentSearch (18 heritage props) / DropdownMenuContent (5 generic-value-dependent slot keys) zero-member collapse. `Omit` deliberately does NOT participate: its output key set (`keyof Source − K`) is source-dependent-open, so an `Omit` carrier stays a carrier at every position (`get_component_meta_table_shaped_open_omit_*` pins it). VALUE-position publication of an open `Pick` still stays a shallow carrier (`chatmessages_resolvable_barrel_publishes_open_pick_as_shallow_carrier`); the surface-position pin is `get_component_meta_chat_messages_shaped_open_pick_heritage_enumerates_picked_keys_only` plus the `pick_over_genuinely_open_source_*` / `pick_over_instantiated_generic_*` regressions in `component_meta_pick_omit_tests.rs`.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:3452:3. The intrinsic/fallthrough `ProjectPath:Published:Expanded` corpus exception — defined by `dispatch_helpers.rs`'s Class-A Expanded fall-through `project_expr_class_a_via_dispatch_threaded` (reached directly or via its non-threaded wrapper `project_expr_class_a_via_dispatch`) and its live callers: `intrinsic_members_for_tag`'s intrinsic/fallthrough composition in `host_manage/intrinsic_projection.rs` (records `ProjectPath:Published:Expanded` charges on the real corpus — measured: 214 on a ChatMessages resolve), the value-expression evaluator in `resolver_core/fallthrough.rs`, the imported-alias registry refinement in `host_manage/component_meta_methods.rs`, the registry materialiser's route-target and `KeyOf` publication sites in `meta_resolve/registry_materialize.rs`, and the JSDoc payload resolution in `host_manage/jsdoc_resolve.rs`. These sites legitimately demand Expanded today and are out of the Navigate-only-publication rule's scope — `publication_routes_never_demand_expanded` covers the projector/registry macro surfaces only; the correct end-state converts this consumer set to Navigate.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:3685:request-bound `ResolverContext`. `ImportedRootDb` is the sole routed-target
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:3749:Finite enumerable union operands fork whole alternatives at their exact spread position, and later effects apply per branch, so correlation survives overwrites. Selector liveness prunes only shadowed recursive effects before recursion (`Key(x)` on `{...Self, x}` never enters `Self`; `{x, ...Self}` must). An alternative's public surface is positive evidence only: omission never proves absence, emptiness, exact `keyof`, exhaustive domain, or closed materialization. Those operations exist only on the sealed witnesses `ClosedObjectProjectionAlternative` / `ClosedObjectProjectionFormula`; an open or mixed formula cannot mint one.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:3788:**Retained runaway-safety fuses (distinct from — not a contradiction of — the above).** Two hard fuses REMAIN and are correct precisely because visited-node cycle detection cannot bound FRESH-NODE regrowth (each operator re-dispatch / `Instantiate` step can mint a brand-new node id the `visited` set has never seen, so a cycle set alone does not terminate a runaway): the deferred evaluator's recursion ceiling `EVALUATE_DEFERRED_DEPTH_CEILING = 256` (stack safety) and the residual-carrier demand step fuse `STRUCTURAL_FACT_DEMAND_FUSE = 64` (backing `normalize_node_for_structural_fact_demand` / `peel_node_for_uninstantiated_carrier_fact_demand` and `resolve_signature_source_carrier`). Neither is a silent semantic cap: a trip returns a TYPED `ResultCompleteness::Partial` (`DEFERRED_EVALUATION_LIMIT` / `STRUCTURAL_FACT_DEMAND_LIMIT`) — node-HIDING at the demand surface (`StructuralFactDemandOutcome::Partial` carries no `SemanticNodeId`), never warm-admitted (`ComputeAdmission::ReturnOnly`), and folded into the enclosing build's taint frame (`into_active_query_build_node` → `fold_local_partial_completeness`) so no consumer can ever classify a truncated carrier as a settled result.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:3979:- Workspace classification uses `ResolverContext::workspace_is_package_backed(canonical_id)` — the single structural predicate (workspace-owned is its complement; there is no separate `workspace_is_workspace_owned` predicate). Substring checks on canonical paths (`"/node_modules/"`, `"\\node_modules\\"`) are banned. The classification API is path-agnostic and handles symlinked / pnpm-hoisted / Windows-backslash / workspace-linked-package cases.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4021:**The `TypeExpr`→handle migration TARGET** is to replace stored `TypeExpr` bodies on the hot parse/shallow/macro/lazy-body/prepared path with interned graph handles, materialising `TypeExpr` ONLY at compat/output boundaries. **Landed so far:** the macro hot mirror (Stage 5A) and the single bounded `decl_body_hot_ref` declaration-body hot-read anchor (Stage 6 Option B) are handle-native; the prepared declaration surface is fact+locator NoTypeExpr end-to-end (the lower-crate `Prepared*` DTOs carry classification facts + content-free body locators, the session builders copy memo-owned facts, and the superseded bundle-stored handle mirror `HotPrepared*` is DELETED — handles mint on demand at the dispatch boundary, never stored in the bundle); `DeclBodyMemo` records are facts+locators (`LoweredValueDecl` fully narrowed + NoTypeExpr-witnessed; authored bodies re-borrowed lease-only on demand), with the residual body readers at the terminal partition (1 `GraphBackedMigrated` + 6 `ProducerLowering` permanent transient ingress + 0 `AuthoredShape` + 5 `GraphFreeDto` + 0 `GraphBackedPending`; the reader-class debt row is CLOSED — the ledger is a permanent ratchet, not open debt). The two former stored type-parameter `TypeExpr` pockets are CLOSED (the type-parameter-bound confinement block): `LoweredTypeDecl` is wholly `NoTypeExpr` — the stored full `type_parameters: Vec<TypeParam>` is deleted; the `narrow_type_parameters` mirror is the sole stored authority (consumed by the prepared-decl builder and the external frontier, which content-free re-anchors the mirror's bound slots to the frontier symbol so `export default` behavior is preserved), and the locator/binder deref (`locator_deref.rs`) uses the mirror for ordinal/name/bound-presence authority while re-borrowing bound CONTENT + the full sibling frame lease-only via `transient_type_parts`. `TypeParamBinding` is shrunk to the content-free `(name, ordinal)` fact pair (`NoTypeExpr`); its `<script setup generic="…">` bounds are re-borrowed at query time through ONE artifact-local transient producer over the pinned `IndexedReady` and lowered by ONE dispatch helper shared by both content readers, a missing/stale re-borrow failing as a typed cache-suppressed miss, never a bound-free fabricated binder. Carrier production reach: the macro type-argument carriers (`TypeOf`/`BareRef`/`ImportType`) now flow through production via the **macro hot mirror** (Stage 5A, LANDED — see "Macro Hot Mirror" below). The eager `lower.rs` `TypeExpr::TypeOf` arm still RESOLVES typeof EAGERLY for the NON-macro declaration/body path (the LANDED Stage 6 Option-B flip mints handles in `decl_body_hot_ref` (over the `Instantiate` result the producer drives via the resolving lowerer) and does not route decl bodies through the structural lowerer; routing decl-body carriers structurally would be the separate deferred query-free declaration-body structural-template producer, NOT Stage 6, NOT landed); the macro-arg PRODUCER is the query-free structural lowerer (`structural_carrier_producer/macro_arg_producer.rs::lower_type_expr_structural`, a bare module-private fn in the single private producer module `crate::structural_carrier_producer::macro_arg_producer`), whose SOLE production caller is the session macro hot-mirror builder in the same module (every other `new_typeof` / `new_bare_ref` / `new_import_type` call site is test-only). The carrier reduction arms (`TypeOf`) and head-resolution helpers (`BareRef`/`ImportType`) resolve a carrier head through the shared `resolve_*_head` helpers consuming `CarrierResolverContext`, at the resolving DEMAND. The carrier types:
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4023:- **`HotTypeRef`** (`semantic_query.rs`) — the internal session hot handle wrapping a `SemanticNodeId` in the `semantic_query_memo` arena. `Send + Sync`; deliberately NOT `Hash`/`Ord`, so it can never be a cache key (R6). Distinct from the public content-hash DTO `component_meta_payload::TypeHandle`.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4028:- **`CarrierResolverContext`** (`project_semantic_dispatch/carrier.rs`) — the RUNTIME / value-side bundle a `BareRef`/`ImportType` carrier needs to resolve at demand time (env / scope / name_resolution / scope_payload / shadowing / reduction-demand axis). NEVER a query key — it borrows its inputs and derives no `Hash`/`Eq`. The mutable substitution accumulator and the dispatcher-local active-instantiate stack are threaded separately; the augmentation scope is derived from `scope` + the resolver.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4032:  - **Sealed carriers + payload vault + terminal-sink capability locality.** `OutputTypeExpr` / `MaterializedOutputTypeExpr` keep their inner `TypeExpr` in a deeply-private nested `carrier::payload` vault — NO readable `TypeExpr` field outside that vault, NO `pub` `Deref` / `AsRef<TypeExpr>` / `into_inner`; the only readers are the capability-gated `into_type_expr(self, &impl OutputProjector)` / `type_expr(&self, &impl OutputProjector)`. The capability is implemented (via EXPLICIT `impl OutputProjector` pairs in the private `projector` module) ONLY for the EIGHT true-output-SINK capabilities — one per exact output-sink module that projects, NOT one per subtree: `MetaResolveProjectorsOutputCap` (DEFINED + minted in the dedicated TERMINAL sink submodule `meta_resolve::projectors::output_sink`, re-exported at `meta_resolve::projectors::MetaResolveProjectorsOutputCap` for the owner impl to name — the submodule was extracted EXACTLY so the parent `projectors`' NON-sink helpers (`macro_payload_substrate`, `published_reducer`, `define_shapes`, the per-kind projector children) cannot mint; ALL boundary-consuming publication functions — `member_shape_peek_or_compute`, the sink-private `reduce_field_value_node` (successor of the deleted TypeExpr reducer `reduce_field_type_expr_with_mode`), and the publication APIs `surface_member_to_expanded_field` (which consumes a policy-admitted `&AdmittedPublishedMember` token) / `project_model` / `reduce_published_field_types` — live INSIDE `output_sink` so the raw `shell_raise_to_type_expr` / `unwrap_materialized` / `seal_type_expr` primitives can be MODULE-PRIVATE; the sink exposes ONLY published-DTO operations, never a bare `TypeExpr`. The COMPLEMENTARY INPUT-authority leak at the Kind-A PUBLICATION boundary — non-sink code forging a `SurfaceMember`/node and pairing it with a cursor to reverse-materialize a member `TypeExpr` — is closed by the sealed admitted-token chain in the sibling `meta_resolve::projectors::publication_authority` module (`ResolvedMacroPayload`/`ResolvedPayloadSurface`/`SurfaceMemberCandidate`/`AdmittedPublishedMember`, private fields + a private `Seal`, minted only by the admission fns; the framework-surface `ResolvedVueSurface` + `SvelteResolvedSurface` tokens drive the shared normalizers through the sealed `ResolvedSurfaceAccess` trait whose supertrait seal is a bare module-private `trait` in `resolved_surface_access.rs` where BOTH impls live — a `framework_surface` sibling `impl` is `E0603`, pinned defense-in-depth by `resolved_surface_access_impls_are_exactly_the_two_tokens`) as the COMPILER primary, pinned by the STRUCTURAL field-closure cross-sink guard `cross_sink_raw_authority_to_type_expr_boundary` (the Kind-A / PUBLICATION bar; the former Kind-B raise-then-decide sites are RETIRED — they decide on the node-domain `RaisedShapeFacts` / interned `RaisedShapeKey` and materialise once at a registered sink, the retired bridge symbol's absence tripwired by the lean tombstone `retired_kind_b_bridge_symbol_absent_from_production_source`)), `MetaResolveFieldTypesOutputCap` (`meta_resolve::materialize::field_types`), `HostManageComponentMetaOutputCap` (`host_manage::component_meta_methods`), `TypeinfoRaiseOutputCap` (`typeinfo::raise`), `TypeinfoSvelteSurfaceOutputCap` / `TypeinfoVueSurfaceOutputCap` (`typeinfo::framework_surface::{svelte_exec,vue_exec}` — `vue_exec`'s whole reachable scope, `vue_exec` + its `normalize` child, IS output-only, so the single cap is correct), `MetaQueryRegistryOutputCap` / `MetaQuerySurfaceOutputCap` (`component_meta_query_engine::{registry_decl,surface}`) — each a private-field type whose `new()` constructor is `pub(in <sink-module>)`. `pub(in P)` grants the mint to `P` AND every module at-or-under `P`, so the mint scope is scoped to a TERMINAL output sink whose entire reachable production module tree is itself output-only — that (not "per-leaf") is what makes the fence compiler-enforced for in-subtree code: a non-sink sibling (`meta_resolve::dispatch_helpers`, `host_manage::eval_env`) — or a non-sink helper sibling — is NOT reachable from any sink's mint scope, so a planted `*OutputCap::new` there is `E0624`. The honest guarantee: the PRIMARY barriers are COMPILER-ENFORCED — in safe production Rust OUTSIDE the payload vault a hot / session / Kind-B mint is `E0624`/`E0451`, a hot carrier-unwrap is `E0277` (the trait is sealed) AND the inner `TypeExpr` is not even a readable field, and a hot `.type_expr_for_test()` is `E0599` (the carrier `_for_test` accessors are `#[cfg(any(test, feature = "test-support"))]` — production-unreachable, COMPILE-ABSENT from every production build incl. debug; `debug_assertions` would have been present in debug builds). The residual TRUSTED surface — the inline payload vault + projector registration source (and the by-name identity of which owner types are sinks) — is the part the compiler cannot itself police; the claim does NOT cover guard deletion, deliberate edits inside that vault, or unsafe code unless the crate forbids unsafe globally. The raw `raise_node_to_type_expr` stays module-private; `raise_and_reduce_with_context` stays `pub(super)`; the shell output seam `output_shell_raise_sealed` returns a SEALED `Option<OutputTypeExpr>` (never a bare `TypeExpr`) so a `project_semantic_dispatch` sibling cannot launder via the `pub(super)` seam. The FFI boundary is the session-owned BYTES facade `VerterHost::project_node_to_type_expr_json_bytes` (the old `project_node_to_type_expr` + the all-`pub`-field `MaterializedTypeExpr` are DELETED). Over the BOUNDED trusted surface the `syn` guards are DEFENSE-IN-DEPTH (not the primary barrier), shaped as a CLOSED structural allowlist: the EXACT owner-file module topology (inline `projector` / `projector::sealed` / `carrier` / `carrier::payload` and nothing else, with item/impl/trait-position macro invocations, `include!`, unknown attributes, a `sealed::Sealed` alias `use`, and any owner-file `TypeExpr` alias BANNED) + the sanctioned sink set (the explicit `impl OutputProjector` / `impl sealed::Sealed` self-types, compared by FULL self-type path as a MULTISET — the dup-last-ident gap closed) by `output_projector_owner_registration_inventory`; a closed item/signature allowlist over the carrier/payload vault (every fn returning `TypeExpr` must be capability-gated or exactly test-gated) by `output_carriers_have_no_inherent_typeexpr_escape_method`; every carrier/payload struct field private regardless of spelled type by `output_carrier_payload_fields_are_private`; an accidental-regression CANARY (NOT proof-complete) for the common `Deref<Target = TypeExpr>` / `AsRef<TypeExpr>` / `Borrow<TypeExpr>` trait escapes in `src/project_semantic_dispatch/output_materialization_guards.rs` — completeness for the unbounded escape-trait surface comes from the payload vault, not the finite trait list; the out-of-crate visibility boundary by the trybuild `output_projector_non_owner_impl_is_compiler_sealed`; the terminal-sink mint scope by `output_cap_mint_scope_is_per_leaf_not_subtree` (a Rust-VISIBILITY reachable-module-tree model — for every `mint: pub(in P)` it builds the production module tree at P-and-below, excluding `#[cfg(test)]` modules, and default-DENIES any reachable module not on the cap's exact sink-module allowlist) + the walker self-test `mint_scope_module_tree_walker_self_test_discriminates`; the COMPLEMENTARY INPUT-authority boundary — a non-sink fn pairing a forgeable raw-authority subject (`SemanticNodeId` / `SurfaceMember` / `SurfaceView` / `VueMacroSurface` / `TypeInfoSurface*`) with a `TypeExpr`-bearing output — by the STRUCTURAL cross-sink transitive guard `cross_sink_raw_authority_to_type_expr_boundary` (a structurally-complete — vs the old name-based pin — residual SUPPLEMENT behind the sealed-token compiler primary, NOT a replacement: the production completeness guarantee is the sealed token, this scanner is the residual cross-module pairing supplement): it decides "TypeExpr-bearing" by FIELD-CLOSURE from `TypeExpr` over the type field graph — following struct fields / enum variants / `Vec`/`Option`/`Arc`/`Box`/tuple element types / `type` aliases across `verter_session` + the cross-crate seed homes — NOT a DTO-name list, and fails any reachable production fn across the registered sinks pairing a forgeable input with a `TypeExpr`-bearing output outside a closed sink-local allowlist of raisers + the token-minting projector callers; type identity is GENUINELY MODULE-QUALIFIED `(module, name)` — the closure graph is keyed by `TypeDefId { module, name }` carried through an 80/20, FAIL-CLOSED identity classifier (NOT a complete Rust name resolver) for the CURRENT production reference shapes (terminal architect ruling `8a3i2-consult-8020-terminal`; accepted EDGE-only final-state residual, recorded in the colocated section-header record in `crates/verter_session/tests/cases/output_projector_residual_guards.rs`). It covers the COMMON in-tree paths: own-module defs; rooted `crate`/`self`/`super` direct matches — the candidate's real module a SUFFIX of (or EQUAL to) the qualifier (relative `crate`/`self`/`super` rebased onto the referencing module; a `super` never escaping above the crate root; a too-short ANCESTOR prefix is NOT a direct match), where an UNROOTED first segment the file's `use`-index SHADOWS is re-resolved through the shadow binding (so `use crate::other as publication_authority` cannot bless `publication_authority::X`); EXACT-target `pub`/`pub(crate)` RE-EXPORTS whose TARGET module is the candidate's real home EXACTLY, never suffix slack (a cross-file `pub`/`pub(crate)`-ONLY re-export index — narrow `pub(self)`/`pub(in …)` re-exports are NOT recorded — keyed by the NORMALIZED absolute written path keeps genuine re-exports — `semantic_query::BudgetExceededFailure`, the two-hop `raise::MaterializedOutputTypeExpr`, the cross-crate `verter_semantic::analysis::*` — resolving); ordinary file imports (a `use … as Alias` import whose TARGET resolves by proof); and the audited intra-crate `use`-binding CHAIN at the normalized qualifier module (a module-scoped, intra-crate-only, non-glob, module/descendant-visibility, cycle-bounded use-binding graph — the genuine `registry_decl` `super::ResolvedTypeDeclaration` chain through the parent `component_meta_query_engine`'s private `use super::declaration_metadata::ResolvedTypeDeclaration`; an unsupported `use` form contributes no binding => Unresolved). A COLLIDING name (`crate::semantic_query::IndexSignature` — SemanticNodeId fields, the authority seed — vs `verter_type_expr::IndexSignature` — TypeExpr fields, an already-lowered-IR bearing leaf) is disambiguated into DISTINCT ids the same way, same-name re-export aliases collapse onto their target, and a reference the classifier cannot resolve to a single target stays `Unresolved` and is caught FAIL-CLOSED at the boundary completeness checks. The RESIDUAL forged shapes are OUTSIDE the proof claim — ACCEPTED EDGE-only final-state residual (the sealed-token compiler primary is the production guarantee; each sanctioned token is uniquely named so an over-resolution lands on the single genuine def; NOT a tightening backlog), disclosed by ROOT-CAUSE CLASS (complete-by-construction, NOT a per-instance list; the colocated section-header record in `crates/verter_session/tests/cases/output_projector_residual_guards.rs`): **Class A (syntactic `use` collection)** — all three `use`-collectors (`collect_use_index` / `collect_reexport_index` / `collect_use_binding_index`) are syntactic: none evaluates item-level `cfg`/`cfg_attr` and the file-import collector also ignores module nesting, so a cfg/cfg_attr-gated `use` (and, for file imports, a `use` inside an inline `#[cfg(test)] mod`) over-contributes across all three rails (the `mod_is_cfg_test` skip is the SINK-FN collector's, NOT these); **Class B (non-proof bare-name fallback)** — the unqualified arms resolve by uniqueness/first-match when a unique proven target isn't found: the `candidates.len()==1` fallback for a no-import name, an ambiguous multi-target `use` (`unique_path` None for >1), AND a unique single-segment self-import (`use Foo;`, recursion-guard skip); the use-binding chain returns the FIRST accessible resolving target (not single-proven); and an unrooted-unshadowed qualifier raw-suffix matches — all landing on a uniquely-named token's single genuine def — with a fail-closed anti-vacuity rail over the `(module, name)`-keyed safe-input / construction-chain tokens (a missing/moved token fires; a bare-name collision is no longer "accepted because one is bearing"); the dual-bearing defense is a DIRECT carve-out (the bearing-output-skip fence keeps a wrapper that DIRECTLY co-holds a resolution-authority seed in the forgeable set — the carve-out stays DIRECT, the 20-FP fence) plus a TRANSITIVE soundness tripwire (`forgeable_input_fence_has_no_dual_bearing_type` uses a transitive raw-authority reach on its seed side, since the tripwire needs soundness, not FP-freedom), and the tripwire's sanctioned-carrier exemption is keyed by QUALIFIED `(module, name)` (a wrong-module same-name token FIRES); BOTH sides are fail-closed on an unclassifiable PascalCase ident, and the non-authority exemptions are QUALIFIER-AWARE — a `(module, name)` Qualified entry (anti-vacuity-checked) or a non-field-bearing CATEGORY entry (trait bound / generic-or-assoc / non-collected external) carrying APPROVED qualified homes, matched against the `Unresolved` ref's PATH not its bare final segment (a forged `evil::Span` FIRES; a one-segment generic is benign; a one-segment trait-bound/external is exempt only with no same-name collected def); the safe-input set is SPLIT into policy-admitted publication tokens vs pre-admission construction-chain structs (a pre-admission chain struct taken directly fires); and the sink-fn collector is inline-mod-aware via a module-path stack); the admitted tokens' private fields + private `Seal` by `admitted_tokens_have_private_fields_and_seal`; the authority-callable scopes' no-`unsafe` ban (a transmute could fabricate a token) by `authority_scopes_contain_no_unsafe`; the carrier `_for_test` test-support gate by `carrier_for_test_accessors_are_test_support_gated_not_debug_assertions`; the mintable `TestOutputCap` staying `#[cfg(test)]`-gated by `test_output_cap_not_visible_or_mintable_in_non_test_builds`; the sealed raise seam by `raise_output_seam_returns_sealed_carrier_not_bare_type_expr` (its scan recurses non-test modules + transitive `TypeExpr` aliases, and now pins that NO public/restricted raise.rs fn returns a bare `TypeExpr` — the retired Kind-B bridge leaves no sanctioned exception); the retired Kind-B bridge symbol's absence by the lean tombstone `retired_kind_b_bridge_symbol_absent_from_production_source`. The `HotTypeRef`-shaped `materialize_type_expr` is a `#[cfg(test)]`-only harness (pinned by `materialize_type_expr_is_not_production_visible`).
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4137:- Workspace classification uses `ResolverContext::workspace_is_package_backed(canonical_id)` — the single structural predicate (workspace-owned is its complement; there is no separate `workspace_is_workspace_owned` predicate). Substring checks on canonical paths (`"/node_modules/"`, `"\\node_modules\\"`) are banned. The classification API is path-agnostic and handles symlinked / pnpm-hoisted / Windows-backslash / workspace-linked-package cases.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4179:**The `TypeExpr`→handle migration TARGET** is to replace stored `TypeExpr` bodies on the hot parse/shallow/macro/lazy-body/prepared path with interned graph handles, materialising `TypeExpr` ONLY at compat/output boundaries. **Landed so far:** the macro hot mirror (Stage 5A) and the single bounded `decl_body_hot_ref` declaration-body hot-read anchor (Stage 6 Option B) are handle-native; the prepared declaration surface is fact+locator NoTypeExpr end-to-end (the lower-crate `Prepared*` DTOs carry classification facts + content-free body locators, the session builders copy memo-owned facts, and the superseded bundle-stored handle mirror `HotPrepared*` is DELETED — handles mint on demand at the dispatch boundary, never stored in the bundle); `DeclBodyMemo` records are facts+locators (`LoweredValueDecl` fully narrowed + NoTypeExpr-witnessed; authored bodies re-borrowed lease-only on demand), with the residual body readers at the terminal partition (1 `GraphBackedMigrated` + 6 `ProducerLowering` permanent transient ingress + 0 `AuthoredShape` + 5 `GraphFreeDto` + 0 `GraphBackedPending`; the reader-class debt row is CLOSED — the ledger is a permanent ratchet, not open debt). The two former stored type-parameter `TypeExpr` pockets are CLOSED (the type-parameter-bound confinement block): `LoweredTypeDecl` is wholly `NoTypeExpr` — the stored full `type_parameters: Vec<TypeParam>` is deleted; the `narrow_type_parameters` mirror is the sole stored authority (consumed by the prepared-decl builder and the external frontier, which content-free re-anchors the mirror's bound slots to the frontier symbol so `export default` behavior is preserved), and the locator/binder deref (`locator_deref.rs`) uses the mirror for ordinal/name/bound-presence authority while re-borrowing bound CONTENT + the full sibling frame lease-only via `transient_type_parts`. `TypeParamBinding` is shrunk to the content-free `(name, ordinal)` fact pair (`NoTypeExpr`); its `<script setup generic="…">` bounds are re-borrowed at query time through ONE artifact-local transient producer over the pinned `IndexedReady` and lowered by ONE dispatch helper shared by both content readers, a missing/stale re-borrow failing as a typed cache-suppressed miss, never a bound-free fabricated binder. Carrier production reach: the macro type-argument carriers (`TypeOf`/`BareRef`/`ImportType`) now flow through production via the **macro hot mirror** (Stage 5A, LANDED — see "Macro Hot Mirror" below). The eager `lower.rs` `TypeExpr::TypeOf` arm still RESOLVES typeof EAGERLY for the NON-macro declaration/body path (the LANDED Stage 6 Option-B flip mints handles in `decl_body_hot_ref` (over the `Instantiate` result the producer drives via the resolving lowerer) and does not route decl bodies through the structural lowerer; routing decl-body carriers structurally would be the separate deferred query-free declaration-body structural-template producer, NOT Stage 6, NOT landed); the macro-arg PRODUCER is the query-free structural lowerer (`structural_carrier_producer/macro_arg_producer.rs::lower_type_expr_structural`, a bare module-private fn in the single private producer module `crate::structural_carrier_producer::macro_arg_producer`), whose SOLE production caller is the session macro hot-mirror builder in the same module (every other `new_typeof` / `new_bare_ref` / `new_import_type` call site is test-only). The carrier reduction arms (`TypeOf`) and head-resolution helpers (`BareRef`/`ImportType`) resolve a carrier head through the shared `resolve_*_head` helpers consuming `CarrierResolverContext`, at the resolving DEMAND. The carrier types:
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4181:- **`HotTypeRef`** (`semantic_query.rs`) — the internal session hot handle wrapping a `SemanticNodeId` in the `semantic_query_memo` arena. `Send + Sync`; deliberately NOT `Hash`/`Ord`, so it can never be a cache key (R6). Distinct from the public content-hash DTO `component_meta_payload::TypeHandle`.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4190:- **`CarrierResolverContext`** (`project_semantic_dispatch/carrier.rs`) — the RUNTIME / value-side bundle a `BareRef`/`ImportType` carrier needs to resolve at demand time (env / scope / name_resolution / scope_payload / shadowing / reduction-demand axis). NEVER a query key — it borrows its inputs and derives no `Hash`/`Eq`. The mutable substitution accumulator and the dispatcher-local active-instantiate stack are threaded separately; the augmentation scope is derived from `scope` + the resolver.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4194:  - **Sealed carriers + payload vault + terminal-sink capability locality.** `OutputTypeExpr` / `MaterializedOutputTypeExpr` keep their inner `TypeExpr` in a deeply-private nested `carrier::payload` vault — NO readable `TypeExpr` field outside that vault, NO `pub` `Deref` / `AsRef<TypeExpr>` / `into_inner`; the only readers are the capability-gated `into_type_expr(self, &impl OutputProjector)` / `type_expr(&self, &impl OutputProjector)`. The capability is implemented (via EXPLICIT `impl OutputProjector` pairs in the private `projector` module) ONLY for the EIGHT true-output-SINK capabilities — one per exact output-sink module that projects, NOT one per subtree: `MetaResolveProjectorsOutputCap` (DEFINED + minted in the dedicated TERMINAL sink submodule `meta_resolve::projectors::output_sink`, re-exported at `meta_resolve::projectors::MetaResolveProjectorsOutputCap` for the owner impl to name — the submodule was extracted EXACTLY so the parent `projectors`' NON-sink helpers (`macro_payload_substrate`, `published_reducer`, `define_shapes`, the per-kind projector children) cannot mint; ALL boundary-consuming publication functions — `member_shape_peek_or_compute`, the sink-private `reduce_field_value_node` (successor of the deleted TypeExpr reducer `reduce_field_type_expr_with_mode`), and the publication APIs `surface_member_to_expanded_field` (which consumes a policy-admitted `&AdmittedPublishedMember` token) / `project_model` / `reduce_published_field_types` — live INSIDE `output_sink` so the raw `shell_raise_to_type_expr` / `unwrap_materialized` / `seal_type_expr` primitives can be MODULE-PRIVATE; the sink exposes ONLY published-DTO operations, never a bare `TypeExpr`. The COMPLEMENTARY INPUT-authority leak at the Kind-A PUBLICATION boundary — non-sink code forging a `SurfaceMember`/node and pairing it with a cursor to reverse-materialize a member `TypeExpr` — is closed by the sealed admitted-token chain in the sibling `meta_resolve::projectors::publication_authority` module (`ResolvedMacroPayload`/`ResolvedPayloadSurface`/`SurfaceMemberCandidate`/`AdmittedPublishedMember`, private fields + a private `Seal`, minted only by the admission fns; the framework-surface `ResolvedVueSurface` + `SvelteResolvedSurface` tokens drive the shared normalizers through the sealed `ResolvedSurfaceAccess` trait whose supertrait seal is a bare module-private `trait` in `resolved_surface_access.rs` where BOTH impls live — a `framework_surface` sibling `impl` is `E0603`, pinned defense-in-depth by `resolved_surface_access_impls_are_exactly_the_two_tokens`) as the COMPILER primary, pinned by the STRUCTURAL field-closure cross-sink guard `cross_sink_raw_authority_to_type_expr_boundary` (the Kind-A / PUBLICATION bar; the former Kind-B raise-then-decide sites are RETIRED — they decide on the node-domain `RaisedShapeFacts` / interned `RaisedShapeKey` and materialise once at a registered sink, the retired bridge symbol's absence tripwired by the lean tombstone `retired_kind_b_bridge_symbol_absent_from_production_source`)), `MetaResolveFieldTypesOutputCap` (`meta_resolve::materialize::field_types`), `HostManageComponentMetaOutputCap` (`host_manage::component_meta_methods`), `TypeinfoRaiseOutputCap` (`typeinfo::raise`), `TypeinfoSvelteSurfaceOutputCap` / `TypeinfoVueSurfaceOutputCap` (`typeinfo::framework_surface::{svelte_exec,vue_exec}` — `vue_exec`'s whole reachable scope, `vue_exec` + its `normalize` child, IS output-only, so the single cap is correct), `MetaQueryRegistryOutputCap` / `MetaQuerySurfaceOutputCap` (`component_meta_query_engine::{registry_decl,surface}`) — each a private-field type whose `new()` constructor is `pub(in <sink-module>)`. `pub(in P)` grants the mint to `P` AND every module at-or-under `P`, so the mint scope is scoped to a TERMINAL output sink whose entire reachable production module tree is itself output-only — that (not "per-leaf") is what makes the fence compiler-enforced for in-subtree code: a non-sink sibling (`meta_resolve::dispatch_helpers`, `host_manage::eval_env`) — or a non-sink helper sibling — is NOT reachable from any sink's mint scope, so a planted `*OutputCap::new` there is `E0624`. The honest guarantee: the PRIMARY barriers are COMPILER-ENFORCED — in safe production Rust OUTSIDE the payload vault a hot / session / Kind-B mint is `E0624`/`E0451`, a hot carrier-unwrap is `E0277` (the trait is sealed) AND the inner `TypeExpr` is not even a readable field, and a hot `.type_expr_for_test()` is `E0599` (the carrier `_for_test` accessors are `#[cfg(any(test, feature = "test-support"))]` — production-unreachable, COMPILE-ABSENT from every production build incl. debug; `debug_assertions` would have been present in debug builds). The residual TRUSTED surface — the inline payload vault + projector registration source (and the by-name identity of which owner types are sinks) — is the part the compiler cannot itself police; the claim does NOT cover guard deletion, deliberate edits inside that vault, or unsafe code unless the crate forbids unsafe globally. The raw `raise_node_to_type_expr` stays module-private; `raise_and_reduce_with_context` stays `pub(super)`; the shell output seam `output_shell_raise_sealed` returns a SEALED `Option<OutputTypeExpr>` (never a bare `TypeExpr`) so a `project_semantic_dispatch` sibling cannot launder via the `pub(super)` seam. The FFI boundary is the session-owned BYTES facade `VerterHost::project_node_to_type_expr_json_bytes` (the old `project_node_to_type_expr` + the all-`pub`-field `MaterializedTypeExpr` are DELETED). Over the BOUNDED trusted surface the `syn` guards are DEFENSE-IN-DEPTH (not the primary barrier), shaped as a CLOSED structural allowlist: the EXACT owner-file module topology (inline `projector` / `projector::sealed` / `carrier` / `carrier::payload` and nothing else, with item/impl/trait-position macro invocations, `include!`, unknown attributes, a `sealed::Sealed` alias `use`, and any owner-file `TypeExpr` alias BANNED) + the sanctioned sink set (the explicit `impl OutputProjector` / `impl sealed::Sealed` self-types, compared by FULL self-type path as a MULTISET — the dup-last-ident gap closed) by `output_projector_owner_registration_inventory`; a closed item/signature allowlist over the carrier/payload vault (every fn returning `TypeExpr` must be capability-gated or exactly test-gated) by `output_carriers_have_no_inherent_typeexpr_escape_method`; every carrier/payload struct field private regardless of spelled type by `output_carrier_payload_fields_are_private`; an accidental-regression CANARY (NOT proof-complete) for the common `Deref<Target = TypeExpr>` / `AsRef<TypeExpr>` / `Borrow<TypeExpr>` trait escapes in `src/project_semantic_dispatch/output_materialization_guards.rs` — completeness for the unbounded escape-trait surface comes from the payload vault, not the finite trait list; the out-of-crate visibility boundary by the trybuild `output_projector_non_owner_impl_is_compiler_sealed`; the terminal-sink mint scope by `output_cap_mint_scope_is_per_leaf_not_subtree` (a Rust-VISIBILITY reachable-module-tree model — for every `mint: pub(in P)` it builds the production module tree at P-and-below, excluding `#[cfg(test)]` modules, and default-DENIES any reachable module not on the cap's exact sink-module allowlist) + the walker self-test `mint_scope_module_tree_walker_self_test_discriminates`; the COMPLEMENTARY INPUT-authority boundary — a non-sink fn pairing a forgeable raw-authority subject (`SemanticNodeId` / `SurfaceMember` / `SurfaceView` / `VueMacroSurface` / `TypeInfoSurface*`) with a `TypeExpr`-bearing output — by the STRUCTURAL cross-sink transitive guard `cross_sink_raw_authority_to_type_expr_boundary` (a structurally-complete — vs the old name-based pin — residual SUPPLEMENT behind the sealed-token compiler primary, NOT a replacement: the production completeness guarantee is the sealed token, this scanner is the residual cross-module pairing supplement): it decides "TypeExpr-bearing" by FIELD-CLOSURE from `TypeExpr` over the type field graph — following struct fields / enum variants / `Vec`/`Option`/`Arc`/`Box`/tuple element types / `type` aliases across `verter_session` + the cross-crate seed homes — NOT a DTO-name list, and fails any reachable production fn across the registered sinks pairing a forgeable input with a `TypeExpr`-bearing output outside a closed sink-local allowlist of raisers + the token-minting projector callers; type identity is GENUINELY MODULE-QUALIFIED `(module, name)` — the closure graph is keyed by `TypeDefId { module, name }` carried through an 80/20, FAIL-CLOSED identity classifier (NOT a complete Rust name resolver) for the CURRENT production reference shapes (terminal architect ruling `8a3i2-consult-8020-terminal`; accepted EDGE-only final-state residual, recorded in the colocated section-header record in `crates/verter_session/tests/cases/output_projector_residual_guards.rs`). It covers the COMMON in-tree paths: own-module defs; rooted `crate`/`self`/`super` direct matches — the candidate's real module a SUFFIX of (or EQUAL to) the qualifier (relative `crate`/`self`/`super` rebased onto the referencing module; a `super` never escaping above the crate root; a too-short ANCESTOR prefix is NOT a direct match), where an UNROOTED first segment the file's `use`-index SHADOWS is re-resolved through the shadow binding (so `use crate::other as publication_authority` cannot bless `publication_authority::X`); EXACT-target `pub`/`pub(crate)` RE-EXPORTS whose TARGET module is the candidate's real home EXACTLY, never suffix slack (a cross-file `pub`/`pub(crate)`-ONLY re-export index — narrow `pub(self)`/`pub(in …)` re-exports are NOT recorded — keyed by the NORMALIZED absolute written path keeps genuine re-exports — `semantic_query::BudgetExceededFailure`, the two-hop `raise::MaterializedOutputTypeExpr`, the cross-crate `verter_semantic::analysis::*` — resolving); ordinary file imports (a `use … as Alias` import whose TARGET resolves by proof); and the audited intra-crate `use`-binding CHAIN at the normalized qualifier module (a module-scoped, intra-crate-only, non-glob, module/descendant-visibility, cycle-bounded use-binding graph — the genuine `registry_decl` `super::ResolvedTypeDeclaration` chain through the parent `component_meta_query_engine`'s private `use super::declaration_metadata::ResolvedTypeDeclaration`; an unsupported `use` form contributes no binding => Unresolved). A COLLIDING name (`crate::semantic_query::IndexSignature` — SemanticNodeId fields, the authority seed — vs `verter_type_expr::IndexSignature` — TypeExpr fields, an already-lowered-IR bearing leaf) is disambiguated into DISTINCT ids the same way, same-name re-export aliases collapse onto their target, and a reference the classifier cannot resolve to a single target stays `Unresolved` and is caught FAIL-CLOSED at the boundary completeness checks. The RESIDUAL forged shapes are OUTSIDE the proof claim — ACCEPTED EDGE-only final-state residual (the sealed-token compiler primary is the production guarantee; each sanctioned token is uniquely named so an over-resolution lands on the single genuine def; NOT a tightening backlog), disclosed by ROOT-CAUSE CLASS (complete-by-construction, NOT a per-instance list; the colocated section-header record in `crates/verter_session/tests/cases/output_projector_residual_guards.rs`): **Class A (syntactic `use` collection)** — all three `use`-collectors (`collect_use_index` / `collect_reexport_index` / `collect_use_binding_index`) are syntactic: none evaluates item-level `cfg`/`cfg_attr` and the file-import collector also ignores module nesting, so a cfg/cfg_attr-gated `use` (and, for file imports, a `use` inside an inline `#[cfg(test)] mod`) over-contributes across all three rails (the `mod_is_cfg_test` skip is the SINK-FN collector's, NOT these); **Class B (non-proof bare-name fallback)** — the unqualified arms resolve by uniqueness/first-match when a unique proven target isn't found: the `candidates.len()==1` fallback for a no-import name, an ambiguous multi-target `use` (`unique_path` None for >1), AND a unique single-segment self-import (`use Foo;`, recursion-guard skip); the use-binding chain returns the FIRST accessible resolving target (not single-proven); and an unrooted-unshadowed qualifier raw-suffix matches — all landing on a uniquely-named token's single genuine def — with a fail-closed anti-vacuity rail over the `(module, name)`-keyed safe-input / construction-chain tokens (a missing/moved token fires; a bare-name collision is no longer "accepted because one is bearing"); the dual-bearing defense is a DIRECT carve-out (the bearing-output-skip fence keeps a wrapper that DIRECTLY co-holds a resolution-authority seed in the forgeable set — the carve-out stays DIRECT, the 20-FP fence) plus a TRANSITIVE soundness tripwire (`forgeable_input_fence_has_no_dual_bearing_type` uses a transitive raw-authority reach on its seed side, since the tripwire needs soundness, not FP-freedom), and the tripwire's sanctioned-carrier exemption is keyed by QUALIFIED `(module, name)` (a wrong-module same-name token FIRES); BOTH sides are fail-closed on an unclassifiable PascalCase ident, and the non-authority exemptions are QUALIFIER-AWARE — a `(module, name)` Qualified entry (anti-vacuity-checked) or a non-field-bearing CATEGORY entry (trait bound / generic-or-assoc / non-collected external) carrying APPROVED qualified homes, matched against the `Unresolved` ref's PATH not its bare final segment (a forged `evil::Span` FIRES; a one-segment generic is benign; a one-segment trait-bound/external is exempt only with no same-name collected def); the safe-input set is SPLIT into policy-admitted publication tokens vs pre-admission construction-chain structs (a pre-admission chain struct taken directly fires); and the sink-fn collector is inline-mod-aware via a module-path stack); the admitted tokens' private fields + private `Seal` by `admitted_tokens_have_private_fields_and_seal`; the authority-callable scopes' no-`unsafe` ban (a transmute could fabricate a token) by `authority_scopes_contain_no_unsafe`; the carrier `_for_test` test-support gate by `carrier_for_test_accessors_are_test_support_gated_not_debug_assertions`; the mintable `TestOutputCap` staying `#[cfg(test)]`-gated by `test_output_cap_not_visible_or_mintable_in_non_test_builds`; the sealed raise seam by `raise_output_seam_returns_sealed_carrier_not_bare_type_expr` (its scan recurses non-test modules + transitive `TypeExpr` aliases, and now pins that NO public/restricted raise.rs fn returns a bare `TypeExpr` — the retired Kind-B bridge leaves no sanctioned exception); the retired Kind-B bridge symbol's absence by the lean tombstone `retired_kind_b_bridge_symbol_absent_from_production_source`. The `HotTypeRef`-shaped `materialize_type_expr` is a `#[cfg(test)]`-only harness (pinned by `materialize_type_expr_is_not_production_visible`).
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4264:**Demand-site carrier resolution + dep recording (codex-ruled "A/C, not B").** The mirror is query-free, so it DEFERS the resolution eager lowering did eagerly (TypeOf value-root execution, Pick/utility source resolution, cross-file resolution-witness recording). Per the architecture ruling, resolution + dep recording happen at the resolving DEMAND, never as a side-band dep preflight on the mirror accessor (a second traversal policy is forbidden). The demand sites: `resolve_carrier_subject_node` (`carrier.rs`) resolves a `TypeOf` carrier subject (the empty-path `typeof config` macro payload — the walker's per-segment `TypeOf` arm never fires for an empty path) via `typeof_key_for` → `build_typeof` (resolve → project the carrier path in `Navigate` → apply `type_args`); a NON-empty `ProjectPath` path leaves the TypeOf base for the walker's mid-walk arm. The deferred-shell evaluator (`evaluate.rs`) resolves a `BareRef`/`ImportType` source carrier (Pick/Omit source, mapped source) one hop under `StructuralTransit(Navigate)`. `resolve_member_value_for_classification` (the exactness path) resolves a bare/import carrier so `type MyStr = string` classifies `ExactConcrete`. **Resolution-witness recording:** `build_typeof`'s import miss (a `typeof <unresolved import>`) observes the owner's path-precise resolution witness into the active tracer and marks `result_is_partial` + `cache_suppress` + the request materialization-suppress sticky. A genuine `MissingDependency` is `ReturnOnly`; the field materializer refuses `ShapeCacheDb` admission of a `TypeOf`-rooted semantic miss so the next request after the dependency appears recomputes cold and recovers.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4266:**Macro-surface partiality carries typed completeness; warm dispatch hits are NOT budget-debited (codex decider).** A macro surface that resolves through the shared dispatch carries a typed `ResultCompleteness` (`semantic_query.rs`). `vue_macro_dtos_with_ctx` (`vue_exec/mod.rs`) returns `MacroDtosRead { dtos, completeness }`, wraps its cold compute in a `ColdComputeCompletenessScope`, and admits the bundle into `vue_surface_store` ONLY when `completeness == Complete` AND `finalise == Ok` — a GENUINE partial DTO bundle is returned to the caller but never enters the surface store (no laundered warm replay). All DTO consumers (`define_shapes`, `slot_binding_graph::typeinfo_macro_dtos`, `resolver_core::component_meta`) fold the surface's partiality into the request-result completeness via `MacroDtosRead::observe_partial()`; the framework-surface executor surfaces `ResolvedOutcome::Partial`. `ResolvedComponentMetaState.completeness` is the authoritative per-result partial signal; `synthesis_should_suppress` is its compatibility projection (`= completeness.is_partial()`), preserved across the `ComponentMetaResultDb` `ResolutionTemplate` round-trip. The decider EXPLICITLY forbids charging warm cache hits to re-trip the projection budget (the budget is a runaway FUSE, not a semantic-complexity quota; charging warm hits risks false partials for complete-warmed work). **Per-result no-poison (LANDED):** the discriminating fixture `batch_partial_returned_never_admitted_while_complete_sibling_warms` (32 non-overlapping interfaces = 96 distinct props, `projection_op_budget = 6`, an unbounded oracle host) pins the invariant by typed completeness + admission-refusal, NOT a prop-count threshold: a result OBSERVED `Partial` (batch 1's budget-tripped cold compute) is REFUSED at the fixed-view cache boundary and never warmed (the primary discriminator); a genuine `Complete` recompute after warm resolver memos heal MAY admit (valid healing, not laundering — the partial result itself is never promoted). The discrimination is pinned by a `store_stable`-gate flip (`component_meta_request.rs:372`) that reddens the batch-1 `!result_admitted` witness. (The earlier 13-prop `<18` fixture rested on a non-deterministic budget re-trip the content-addressed mirror legitimately removed; it was corrected, not weakened — no rule exception, no carve-out. See `docs/arch/parselower-design.md` Stage 5A residual.)
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4305:`get_component_meta` PUBLISHES the inferred type, so it is genuinely incomplete under either and must not warm.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4329:The per-member half is enforced one layer down, in `broad_runtime.rs`: `ObservedRuntimePartial` splits an observed partial into `shape_is_trustworthy` (⊆ `FLOW_RETURN_DEGRADED` — the addressed member is still selectable out of the resolved structure) and `member_types_are_trustworthy` (⊆ `FLOW_RETURN_UNINFERRED` — a member's own node may be classified). Gating the member projection on the raw `result_is_partial` boolean instead handed the whole payload object to the classifier, which answered `Unknown` for EVERY member: `{ label: "x", made: f() }` emitted `label: { type: null }` while `get_component_meta` published `label` as `string` on the same tree. `QueryError::UnmodeledPosition` folds `FLOW_RETURN_UNINFERRED`, not `SEMANTIC_QUERY_FAULT`, so reading the marker node is the same observation as reading the flow result that produced it.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4331:**The class must survive the query/build boundary.** `CacheRead` carries `partial_reasons` alongside `result_is_partial` (read through `CacheRead::partial_reason_classes()`, which substitutes `PROPAGATED` only for a producer that named nothing), fed from `QueryBuildOutput::partial_reasons` ← `BuildLocalTaint::partial_reasons` ← `fold_into_top_build_local_taint_with`, and republished across the singleflight rendezvous through `InflightState::partial_reasons`. Without it the class was recorded in the innermost cold-compute scope AND an anonymous duplicate of the same cause was re-lifted at the read boundary (`observe_component_meta_read_suppress` → `mark_request_result_partial`), so the containment subtracted the name and the duplicate still faulted: `defineProps<ReturnType<typeof makeProps>>()` over `{ label: "x", made: f() }` observed `{FLOW_RETURN_UNINFERRED, PROPAGATED}`, residual `{PROPAGATED}`, and raised a hard `XUnavailableMacroSemanticResult` — no module bytes and no IDE TSX at all, for a component whose props `get_component_meta` publishes correctly on the same tree. The generic bridge (`mark_request_result_partial_from_read_with`) folds a NAMED set verbatim and records `PROPAGATED` only for an unnamed partial into a scope that carries no reason yet. Acceptance (all on RENDERED bytes, asserting the emitted `props` option object itself — a `code.contains("label")` assertion is satisfied by the authored script spliced verbatim into `setup(__props)` and passes against `props: {}`): `meta_tests::runtime_props_derive_each_member_from_that_members_own_evidence`, `meta_tests::a_root_position_flow_degradation_refuses_instead_of_publishing_empty_props`, `meta_tests::a_no_surface_flow_return_refuses_even_when_a_sibling_arm_contributes`, `meta_tests::an_unevaluable_emits_spread_source_refuses_rather_than_dropping_the_event`, `meta_tests::an_unverified_flow_return_publishes_its_member_set_with_validation_off`, `meta_tests::the_tsx_lane_emits_for_every_flow_return_degradation_class` (driven through `ensure_ide_compiled` + `get_ide` under the LSP's own `CompileTarget::IDE | TEMPLATE_DATA` profile — `get_virtual_file(Main, IDE)` returns the RUNTIME module under a names-only demand, and a default BUNDLER profile normalized with the TSX bit still demands runtime prop CONSTRUCTORS, so either spelling measures the runtime lane and reports the TSX lane healthy whatever it does) + `flow_return_tests::a_budget_truncated_flow_return_folds_a_faulting_class_not_a_contained_one`.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4349:- **Function-scoped (`var`) reaching definitions.** The evaluator carries TWO local layers: the LEXICAL layer (`const` / `let`) that block and `if`-arm evaluation saves and restores, and the FUNCTION-scoped `var` layer those restores never touch (`var` hoists, so `{ var y = 1 } return y` is `number`). Three rails ride that layer. (a) **Conditional definitions fail closed.** A conditional-arm nesting counter rises on `if` ARMS only — a plain block executes unconditionally and never increments it — and a `var` bound under non-zero nesting enters `var_conditional_locals`; OBSERVING such a binding records the `ConditionalVarDefinition` degradation (`ReturnOnly`, never warm), mirroring `var_degraded_locals` exactly, so an unobserved conditional `var` degrades nothing and an unconditional rebind clears the flag. The substrate has nothing to join on (`SliceStatement::If` carries no test content by design), and the join algebra is owned by `U6.NARROW_*` on the shared `FlowFrame` lattice over the ONE `FunctionFlowGraph` — a merge algebra over the evaluator's `locals` / `var_locals` maps would be exactly the forbidden second flow structure. Recorded as the `debt:CONDITIONAL-VAR-BRANCH-JOIN` ledger entry on `U6.FLOW_RETURN_SUBSTRATE` in `scripts/manifests/typeinfo-programme-reconciliation.json`. Accepted cost: `if (flag) { var w = 1 } return w` was a coincidentally-correct warm `number` and is now degraded — tsc REJECTS that program (TS2454). (b) **A `var` redeclaring a PARAMETER shares the parameter's slot.** The graph's lexical resolution unions the FUNCTION-scope frame's hits with every same-name hoisting-kind binding wherever written (inner block-scoped frames stay exact, so shadowing is preserved), the content lowering resolves a hoisted-`var` name as `SliceExpr::Local` / `CallOnBinding` AHEAD of the parameter list, and both carry the parameter ordinal so an unbound read (before the declarator, or on a path that never reached it) still resolves to the parameter — never a fabricated `any`. (c) **Loop transparency requires transfer-inert selected content.** A return-free loop stays `TransparentLoop` only when it declares no escaping `var` and its body has no live selected-slot guard, call/assertion, or write whose iteration flow can change a downstream selected read. Read/write comparison is path-aware: statically distinct member siblings do not alias, while whole-root and computed segments remain conservative. Literal-dead `if` branches and a `while (false)` body contribute neither loop transfer nor unapplied-write degradation. Nested closure effects are inspected only for a function/arrow in CALLEE position — callback arguments are values, not proven invocations — and a directly invoked closure inside a loop fails closed when its own skeleton writes a captured downstream slot or reads one in a control/call site. Such iteration-dependent flow requires a fixed point the substrate does not model, so it takes the typed `SliceUnsupported::Loop` no-value rail and cannot warm. A control read with no modelled guard (`while (x + 1)`), a callback argument, and a loop with no selected capture remain transparent; precise closure-guard transfer remains deferred.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4442:`ResolverContext::workspace_is_package_backed`), and the closed vocabulary
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4453:- **Function-scoped (`var`) reaching definitions.** The evaluator carries TWO local layers: the LEXICAL layer (`const` / `let`) that block and `if`-arm evaluation saves and restores, and the FUNCTION-scoped `var` layer those restores never touch (`var` hoists, so `{ var y = 1 } return y` is `number`). Three rails ride that layer. (a) **Conditional definitions fail closed.** A conditional-arm nesting counter rises on `if` ARMS only — a plain block executes unconditionally and never increments it — and a `var` bound under non-zero nesting enters `var_conditional_locals`; OBSERVING such a binding records the `ConditionalVarDefinition` degradation (`ReturnOnly`, never warm), mirroring `var_degraded_locals` exactly, so an unobserved conditional `var` degrades nothing and an unconditional rebind clears the flag. The substrate has nothing to join on (`SliceStatement::If` carries no test content by design), and the join algebra is owned by `U6.NARROW_*` on the shared `FlowFrame` lattice over the ONE `FunctionFlowGraph` — a merge algebra over the evaluator's `locals` / `var_locals` maps would be exactly the forbidden second flow structure. Recorded as the `debt:CONDITIONAL-VAR-BRANCH-JOIN` ledger entry on `U6.FLOW_RETURN_SUBSTRATE` in `scripts/manifests/typeinfo-programme-reconciliation.json`. Accepted cost: `if (flag) { var w = 1 } return w` was a coincidentally-correct warm `number` and is now degraded — tsc REJECTS that program (TS2454). (b) **A `var` redeclaring a PARAMETER shares the parameter's slot.** The graph's lexical resolution unions the FUNCTION-scope frame's hits with every same-name hoisting-kind binding wherever written (inner block-scoped frames stay exact, so shadowing is preserved), the content lowering resolves a hoisted-`var` name as `SliceExpr::Local` / `CallOnBinding` AHEAD of the parameter list, and both carry the parameter ordinal so an unbound read (before the declarator, or on a path that never reached it) still resolves to the parameter — never a fabricated `any`. (c) **Loop transparency requires transfer-inert selected content.** A return-free loop stays `TransparentLoop` only when it declares no escaping `var` and its body has no live selected-slot guard, call/assertion, or write whose iteration flow can change a downstream selected read. Read/write comparison is path-aware: statically distinct member siblings do not alias, while whole-root and computed segments remain conservative. Literal-dead `if` branches and a `while (false)` body contribute neither loop transfer nor unapplied-write degradation. Nested closure effects are inspected only for a function/arrow in CALLEE position — callback arguments are values, not proven invocations — and a directly invoked closure inside a loop fails closed when its own skeleton writes a captured downstream slot or reads one in a control/call site. Such iteration-dependent flow requires a fixed point the substrate does not model, so it takes the typed `SliceUnsupported::Loop` no-value rail and cannot warm. A control read with no modelled guard (`while (x + 1)`), a callback argument, and a loop with no selected capture remain transparent; precise closure-guard transfer remains deferred.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4507:- **Function-scoped (`var`) reaching definitions.** The evaluator carries TWO local layers: the LEXICAL layer (`const` / `let`) that block and `if`-arm evaluation saves and restores, and the FUNCTION-scoped `var` layer those restores never touch (`var` hoists, so `{ var y = 1 } return y` is `number`). Three rails ride that layer. (a) **Conditional definitions fail closed.** A conditional-arm nesting counter rises on `if` ARMS only — a plain block executes unconditionally and never increments it — and a `var` bound under non-zero nesting enters `var_conditional_locals`; OBSERVING such a binding records the `ConditionalVarDefinition` degradation (`ReturnOnly`, never warm), mirroring `var_degraded_locals` exactly, so an unobserved conditional `var` degrades nothing and an unconditional rebind clears the flag. The substrate has nothing to join on (`SliceStatement::If` carries no test content by design), and the join algebra is owned by `U6.NARROW_*` on the shared `FlowFrame` lattice over the ONE `FunctionFlowGraph` — a merge algebra over the evaluator's `locals` / `var_locals` maps would be exactly the forbidden second flow structure. Recorded as the `debt:CONDITIONAL-VAR-BRANCH-JOIN` ledger entry on `U6.FLOW_RETURN_SUBSTRATE` in `scripts/manifests/typeinfo-programme-reconciliation.json`. Accepted cost: `if (flag) { var w = 1 } return w` was a coincidentally-correct warm `number` and is now degraded — tsc REJECTS that program (TS2454). (b) **A `var` redeclaring a PARAMETER shares the parameter's slot.** The graph's lexical resolution unions the FUNCTION-scope frame's hits with every same-name hoisting-kind binding wherever written (inner block-scoped frames stay exact, so shadowing is preserved), the content lowering resolves a hoisted-`var` name as `SliceExpr::Local` / `CallOnBinding` AHEAD of the parameter list, and both carry the parameter ordinal so an unbound read (before the declarator, or on a path that never reached it) still resolves to the parameter — never a fabricated `any`. (c) **Loop transparency requires transfer-inert selected content.** A return-free loop stays `TransparentLoop` only when it declares no escaping `var` and its body has no live selected-slot guard, call/assertion, or write whose iteration flow can change a downstream selected read. Read/write comparison is path-aware: statically distinct member siblings do not alias, while whole-root and computed segments remain conservative. Literal-dead `if` branches and a `while (false)` body contribute neither loop transfer nor unapplied-write degradation. Nested closure effects are inspected only for a function/arrow in CALLEE position — callback arguments are values, not proven invocations — and a directly invoked closure inside a loop fails closed when its own skeleton writes a captured downstream slot or reads one in a control/call site. Such iteration-dependent flow requires a fixed point the substrate does not model, so it takes the typed `SliceUnsupported::Loop` no-value rail and cannot warm. A control read with no modelled guard (`while (x + 1)`), a callback argument, and a loop with no selected capture remain transparent; precise closure-guard transfer remains deferred.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4546:docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4792:docs/arch/architecture-lock/ledger/program-state.toml:192:notes = "Track A worker run: measurement-only work-attribution substrate (verter_audit attribution feature), 70 chokepoints instrumented, disabled-overhead proven structurally+behaviourally+by measurement. Reviews found 10 items (F1-F7 doc/number-accuracy fixes, F6 !Send hardening); F5 (gate coverage for attribution/compile-fail features) dispositioned DEFER to A5, recorded in evidence/A4-summary.md. Fix pass applied, reattested, squash-landed. Commit message history rewritten (tree-identical, message-only) to remove plan-vocabulary references discovered post-landing; context_packet_digest is over the verbatim dispatch-prompt record at evidence/A4/context-packet.md, re-digested after the A6-discovered machine-path portability fix (discovery D-1, ruled FIX-NOW). candidate_sha/accepted_sha (1ab403c01) remain a genuine ancestor of the live program/architecture-lock tip. EVIDENCE RE-BINDING (evidence/A4-summary.md): the original digest matched NO artifact ever committed under docs/arch, in any commit or file type - most likely a file digested from an uncommitted scratch path; unrecoverable, so re-bound to this block real landing summary."
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4793:docs/arch/architecture-lock/ledger/program-state.toml:234:notes = "Implementation Lock Record + locked performance-gates.toml (no placeholders) + bound B1 charter. Reviews initially BLOCKING (missing AMD-001 traceability, missing A5-L1 disposition, one gate-direction bug on session.cache_admit_cacheable); one bounded fix pass corrected all, reattested clean. Architecture ruling (Codex Sol xhigh, read-only): (1) AMD-001 §1's four-artifact stack-window-validator prerequisite is AMENDED (not delivered now, not deferred as open debt) so it binds to the first post-A6 opened stack window / unconditionally before D1 PRIVATE_CHECKPOINT, not to A6 itself — recorded as a governance §10 acceptance-contract rescope in evidence/A6/AMD-001-deviation-memo.md; (2) discovery D-1 (machine-path leak in A4/A5 context-packet.md) ruled FIX-NOW, applied, both blocks' context_packet_digest corrected. Maintainer ratified A5-L1/G1/DD1/R-12/S1 (loop5_instrumentation converge+delete owners G4/K3/L4; attribution/compile-fail locked as per-block commands, CI deferred post-program; verter_semantic->verter_workspace pinned exception, C1 removal gate; 469 unlanded local branches abandoned as a class; stack policy max_open_stack_layers=2/ATOMIC_REVIEW/LOCAL_BRANCH_CHAIN). LANDING EQUIVALENCE: accepted identity diverges from the reviewed candidate identity — a maintainer-directed cross-block housekeeping squash (collapsing A4-ledger/A5/A6 bookkeeping commits into one, excluding the real product commit 1ab403c01) replaced the branch tip after review; the delta is disclosed, evidenced, and tree-diffed in docs/arch/refactor/rev11/evidence/A6/landing-equivalence-A5-A6.md (landing_equivalence_digest binds that file). Reviewed candidate preserved from GC via tag program-history/A6-reviewed-candidate. B1 is BLOCK_READY (charter digest ac60d191221fc5e5938e0343091c6809648a482960ca7c1a49596e547d3e28e1); J1 stays not-unlocked, no CSS work selected. EVIDENCE RE-BINDING (evidence/A6-summary.md): original digest unrecoverable (matched no committed artifact); re-bound to this block real landing summary."
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4797:docs/arch/architecture-lock/ledger/program-state.toml:297:notes = "REOPEN #4 RESOLVED. Two triggers: (1) BV0-relanding scoping consult Q2/Q4 — named import-specifier order compared positionally (now cosmetic in the normalizer) and script-less SFC bindingMetadata passed truthy-empty instead of undefined (wrong render-arity golden, fixed). (2) Mapping axis was structurally unsound — compared candidate-vs-official source maps despite the project's own Compiled-Output Conformance rule allowing cosmetic generated-byte divergence, so two compilers' maps necessarily describe two different documents. Binding maintainer direction: source-map correctness is judged only against the exact authored SFC. Replaced end to end with a self-referential oracle (new src/mapping-oracle.mjs): map contract/bounds, source identity, per-segment truthfulness via a closed relation table, bidirectional round-trip completeness, fragment-first assembly composition, structural generated-only range detection against fabricated provenance. Four independent review rounds; rounds 1-3 each found and fixed real defects (false doc citation; synthetic-range enforcement that never ran on the real path; non-position-exact relations; an incomplete binding-pattern check; over/under-broad generated-range detection). Round 4 (review-round cap) found two final defects, fixed and independently re-verified by the program orchestrator against committed source rather than a fifth agent round. Disclosed bounded residuals (non-blocking, stated in-module): a reference-occurrence fallback relation not fully position-exact (267/812 corpus occurrences); the boundary check intentionally unenforced on ranges nested inside a larger legitimately-mapped expression; map.file unvalidated (no golden carries it). A related maintainer optimization concern was investigated in parallel (evidence/BF2/reopen4/optimization-vs-conformance-investigation.md) — partially confirmed (narrow waste, e.g. an unused-import case now flagged for BV0), no evidence of deeper architectural cost; disposed as narrow ADOPT-NOW items plus one CLAUDE.md-level DEFER (FC-VUE-002, owned by BV1/BS1). Full detail: evidence/BF2/reopen4/{context-packet,landing-record}.md. No production compiler code, no verter_session, no Svelte file, no crates/verter_vue_conformance/corpus touched — scope confirmed via diff by the program orchestrator. Independently re-verified: harness 411/411, verter_vue_conformance 8/8. BF1 not reopened (unaffected both times). BV0 and BF3 unblocked, next legal once this transition is validated; BV0's own restack still additionally waits on AMD-007/BV0A ratification (docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md, PENDING)."
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4798:docs/arch/architecture-lock/ledger/program-state.toml:402:notes = "Four findings corrected in their root-cause owners; all four named targets enabled and green. The first row understated the work: the each-block flag is coupled to the item read form, so clearing it while still emitting a signal deref produces a module that mounts and renders WRONG - non-reactive items are now demoted to a distinct kind. The fourth was corrected in the shared framework-surface leg, NOT the projector the charter names; a grok pass proved that leg has three consumers and a projector fix would have forked the shared surface - charter wording inaccurate, recorded not edited. While extending the gate outcome enum the block found the gate NEVER COMPARED the oracle verdict, so a state asserting failure could silently record a passing cell; the verdict is now compared and the empty-divergence state is structurally unrepresentable. Both seats found real defects, two of them regressions this block introduced. Seven out-of-scope defects deferred as ignored characterizations proven RED with named owners, including a pre-existing non-ASCII identifier panic. Gate PASS on the exact landing tree (24413/24413, 3 suites clean, 8634/8634); a first run FAILED on 4 tests proven to be concurrent-gate contention by isolated re-runs, not assumed. Detail: evidence/BS0/landing-record.md."
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4895:docs/arch/refactor/rev11/implementation-readiness-review-v9.md:34:Revision 9 described a DAG but did not provide a complete machine-readable predecessor relation. Important orderings—input authority before managed query convergence, final flow before TypeExpr cutover, and numeric gates before hot-path implementation—were not unambiguously enforced.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4898:docs/arch/refactor/rev11/implementation-readiness-review-v9.md:34:Revision 9 described a DAG but did not provide a complete machine-readable predecessor relation. Important orderings—input authority before managed query convergence, final flow before TypeExpr cutover, and numeric gates before hot-path implementation—were not unambiguously enforced.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4904:docs/arch/parselower-design.md:12:This v3 keeps the endorsed staged skeleton and bakes in the 7 review refinements (the v1→v2→v3 review chain endorsed the direction; v3 finalizes the stage-1/2/3 foundation). The breaking phase was re-sequenced per the architecture ruling at the orchestration ledger (the former monolithic "Stage 3C / Stage 5 atomic macro-producer" framing split into Stage 5A + Stage 6-prep + a reordered Stage 6), and then re-sequenced AGAIN per the architecture tie-breaker ruling at the orchestration ledger (Ruling 1 + Ruling 2 / defer-all-5A under (iii), adopted by the CTO as R1) — Stage 5A was DEFERRED behind a NEW non-breaking consumer-readiness prerequisite, **Stage 5A-prep**, so that the full macro-arg producer cutover (all four sites) could land atomically only once that prerequisite was satisfied. That prerequisite was SATISFIED and Stage 5A then LANDED: Stages 1-4 are NON-BREAKING (additive / dual-read; LANDED), the **Stage 5A-prep** prerequisite is LANDED (NON-BREAKING; refactor commit `8ff601882`), and **Stage 5A** — the atomic four-site macro hot-mirror structural producer cutover — is LANDED on the resume branch (`mom/stage5a-resume`): the session macro hot mirror is the SOLE production producer of the macro type-argument structural carriers and all four macro-arg sites read it (carriers flow through production). The inserted **Stage 6-prep** (NON-BREAKING — graph-native `whole_env()` consumer readiness; FOUR codex-confirmed-exhaustive consumers each gain a bounded graph-native reader beside the retained whole-env oracle) is LANDED on the resume branch (`mom/stage6prep`). **Stage 6 — the bounded Option-B declaration-body handle-payload / hot-read cutover (mint `HotTypeRef` in the `decl_body_hot_ref` accessor over the `Instantiate` result the graph-bearing dispatch producer drives via the RESOLVING lowerer; the migrated graph-backed semantic readers go handle-native and the superseded bundle-stored hot prepared layer (`HotPrepared*`) is DELETED — handles mint on demand at `decl_body_hot_ref`; `DeclBodyMemo` + the lower-crate `Prepared*` are fact+locator `NoTypeExpr` (the LIVE end-state — TWO memoized pockets excepted: `LoweredTypeDecl.type_parameters: Vec<TypeParam>` retains constraint/default `Arc<TypeExpr>` (`decl_body_memo.rs:104`), feeding locator/binder lowering (`locator_deref.rs:363`) and the external frontier (`external_type_frontier.rs:152`); and `TypeParamBinding.constraint/default` keeps `Arc<TypeExpr>` bounds in cached prepared decl bundles (`resolver_core/prepared_decl.rs:185` (fields `:193-194`)), read by query-time lowering (`project_semantic_dispatch/lower.rs:328`) — live violations of the terminal storage target owned by the type-parameter-bound confinement block)) — is LANDED**, with the eager-clone-path debt CLOSED — `prepare_type_decl_from_lowered` is the live wired prepare path (`resolver_core/prepared_decl.rs:364` / `:480`; its former body read moved to the extracted shared assembly tail `finish_prepared_type_decl`) — and the residual `TypeExpr`-body-reader surface reached its terminal partition (the 12-row curated inventory — 1 migrated anchor + 6 `ProducerLowering` permanent transient ingress + 5 `GraphFreeDto`; `AuthoredShape`/`GraphBackedPending` reached empty — a PERMANENT ratchet, not open debt; final state in `docs/arch/authored-shape-graph-native-migration-deferral.md`); the SEPARATE, DEFERRED future query-free global decl-body structural-template producer design is NOT landed. **Stages 7-9** (BREAKING — cache/artifact cutover, compat materialization fence, transitional-bridge deletion) are LANDED: the hot-materialize fence `hot_path_never_calls_materialize_type_expr` is enabled and green at ZERO offenders, and the transitional materialize allowlist reached 0. The ORIGINAL parselower sequence ended at Stage 9; the typed-degradation end-state has since LANDED (typed `QueryError` + per-leaf sidecar degradation replaced the sentinel-`Unknown` control flow; the global Unknown fence and the hot-materialize residual-deferral doc are deleted), and the named debt row is now the final-state note `docs/arch/authored-shape-graph-native-migration-deferral.md`.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4907:docs/arch/parselower-design.md:21:> **The "global structural producer flip / route decl bodies through `lower_type_expr_structural` → `BareRef(B)`" described throughout this document is therefore NOT the Stage-6 declaration-body flip and NOT the S4 session.** It is at most a **SEPARATE, DEFERRED, future query-free design** that, if ever pursued, must stay **query-free** and carry its **OWN** guarded surface — it never reuses or widens the macro-arg producer's guards. Wherever a downstream stanza below says "Stage 6 wires the decl-body path to reach the lowerer" / "the global structural producer flip" / "the decl body lowers to `BareRef(B)`", read it under THIS correction: that is the deferred future design, not the ratified Stage-6 mechanism.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4914:docs/arch/parselower-design.md:51:status: split into 3A (LANDED) + 3B (LANDED) + the producer flip (formerly "3C", now SUPERSEDED — split into Stage 5A + Stage 6 per the decider ruling; see the reconciliation stanza below). The 3A/3B split is by what reduction/resolution behavior the dispatch is ABLE to perform on demand, NOT by live carrier emission: AT THE 3A/3B LANDING WINDOW all three arg-carrying carriers (`TypeOf`/`BareRef`/`ImportType`) were dormant-producer-only at the node level (Stage 5A later flipped the macro-arg producer live — see the Stage 5A stanza). The eager `lower.rs` `TypeExpr::TypeOf` arm RESOLVES typeof EAGERLY and mints no `TypeOf` carrier node for the decl/body path; the production mint of the three arg-carrying carriers is the query-free structural lowerer (now the bare module-private `lower_type_expr_structural` in `structural_carrier_producer/macro_arg_producer.rs`), which at the 3A/3B window had ZERO production callers (held by `structural_lowerer_has_no_production_caller_until_carrier_resolution`), so no carrier flowed through production then (every `new_typeof`/`new_bare_ref`/`new_import_type` call site was test-only — a `#[cfg(test)]` source module or an integration test under `crates/verter_session/tests/`). 3A lands every demand-time behavior the `TypeOf` carrier reduction needs (exercised by directly-constructed carriers + the producer) plus the carrier-arg-safe consumer walkers; 3B makes the dispatch ABLE to resolve `BareRef`/`ImportType` carrier HEADS — wired and unit-proven for the single-hop, imported-builtin-name, and local-scope cases (proven by direct-constructed carriers), with carrier-head equivalence to the eager `name_resolution` fast-path for BARREL re-exports and NAMESPACE-SIBLING bare names DEFERRED to a **separate deferred query-free declaration-body structural-template producer (NOT Stage 6, NOT landed)** as LATENT (pinned by discriminating characterization tests; see the debt row below). The structural-lowerer PRODUCER activation is no longer a single "3C" flip: it is SPLIT (per the decider ruling) into **Stage 5A** (the narrow session macro hot-mirror producer — the only production `lower_type_expr_structural` caller; the lowerer is a BARE module-private `fn` (NO visibility modifier) in the single private module `structural_carrier_producer::macro_arg_producer`, reachable from outside only through the `pub(crate)` `macro_type_arg_hot_ref` accessor, so no other production module can name it — the FOREIGN case is compiler-confined and the SAME-MODULE residual is policed by the strengthened single-producer guards; it lowers NO declaration bodies; LANDED) and a **separate deferred query-free declaration-body structural-template producer** (the GLOBAL declaration-body producer flip; NOT Stage 6, NOT landed). See the superseded "Stage 3C" reconciliation stanza below.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4916:docs/arch/parselower-design.md:53:- Stage 3B (LANDED): `BareRef`/`ImportType` demand-time head resolution through the ONE dispatch. The live `TypeExpr::Ref` / `TypeExpr::ImportType` resolution (`lower.rs` bare-name / import-augmentation stitch / enum projection / builtin shadowing / the `is_instantiate_active` back-edge) is factored into the shared `resolve_bare_ref_head` / `resolve_import_type_head` helpers on `ProjectSemanticDispatch` (consuming the value-side `CarrierResolverContext`), called from BOTH the eager `Ref`/`ImportType` lowering arm AND the canonical query-entry carrier normalization (the head resolves BEFORE the type-args lower, so an unresolvable head never lowers dead args; the entry normalization runs inside a fact tracer so its serves taint the admission read-set; the normal + shallow-synth path walkers re-enter the same normalization for nested carriers — no walker-local resolver). `CarrierResolverContext` is CONSUMED. 3B does NOT wire `lower_type_expr_structural` into production and does NOT remove the dormant-wiring guard.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4919:docs/arch/parselower-design.md:58:guards: carrier resolution stays inside the ONE dispatch (`SemanticQueryKey → ProjectSemanticDispatch::execute`); no second carrier resolver / per-surface walker; no worker fallback resolution; the consumer walkers read carrier args ONLY via `SemanticNodeData::carrier_type_args`; resolution cache keys slot/fact-identity + env-split, anon subjects uncached. 3B adds: `CarrierResolverContext` is the explicit value-side bundle the shared head-resolution helpers consume (never a query key, never a second engine), and KEEPS the dormant-wiring guard. The dormant-wiring guard was REPLACED at Stage 5A (LANDED) by the collapsed private module — `lower_type_expr_structural` is a BARE module-private `fn` (NO visibility modifier) in the single private module `structural_carrier_producer::macro_arg_producer`, reachable from outside only through the `pub(crate)` `macro_type_arg_hot_ref` accessor, so the session macro hot-mirror builder is its sole production caller (the FOREIGN case is compiler-confined — no other production module can name it; the SAME-MODULE residual is policed by the strengthened single-producer guards), pinned by the privacy-shape guard `structural_carrier_producer_lowerer_is_module_private` + the ordering tripwire banning eager macro-arg lowering outside the mirror — and the GLOBAL decl-body producer flip is the **separate deferred query-free declaration-body structural-template producer** (NOT Stage 6, NOT landed), which would carry the production integration tests in that change.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4920:docs/arch/parselower-design.md:60:debt: ARCH-CONSULT-1 (apply `TypeOf` `type_args` SEMANTICALLY during reduction, via `apply_typeof_instantiation_args` in the evaluate / raise / walk `TypeOf` reducer arms) is CLOSED by 3A. ARCH-CONSULT-3 splits: the consumer-walker carrier-arg descent (the `meta_resolve` ref/cycle/dep walkers, the `build.rs` type-param collector, and the free-type-param classifier `slot_binding_graph.rs::node_contains_free_type_param`; `exactness.rs::object_is_closed_node` root-kind-classifies carrier-valued members and does not descend args) is CLOSED by 3A; the `BareRef`/`ImportType` demand-time head resolution is wired through the shared `resolve_*_head` helpers on the ONE dispatch (`CarrierResolverContext`-driven) in 3B and unit-proven for the single-hop, imported-builtin-name, and local-scope cases. Carrier-head equivalence with the eager `name_resolution` fast-path for **barrel re-exports** (the carrier path walks to the final defining file while the eager fast-path stores the intermediate barrel canonical) and **namespace-sibling bare names** (the eager path's per-decl `add_namespace_sibling_resolutions` injection has no scope-payload equivalent) is RE-VALIDATED if/when the **separate deferred query-free declaration-body structural-template producer** (NOT Stage 6, NOT landed) is ever pursued, when the structural lowerer's carrier-scope shape would first be exercised end-to-end on declaration bodies (Option B does NOT route decl bodies through the structural lowerer, so it does not re-validate this surface); both stay LATENT through Stage 5A and the LANDED Stage-6 Option-B flip (the macro hot-mirror producer lowers NO declaration bodies, and Option B mints handles in `decl_body_hot_ref` (over the `Instantiate` result the producer drives via the resolving lowerer) rather than routing decl bodies through the structural lowerer, so this global decl-body divergence surface is not exercised) and are pinned by characterization tests. The dormant-wiring guard `structural_lowerer_has_no_production_caller_until_carrier_resolution` was RETAINED through 3A/3B/Stage 4; it was REMOVED at **Stage 5A** (LANDED) and replaced by the collapsed private module — `lower_type_expr_structural` was re-homed into the single private module `structural_carrier_producer::macro_arg_producer` as a BARE module-private `fn` (NO visibility modifier), reachable from outside only through the `pub(crate)` `macro_type_arg_hot_ref` accessor, so the session macro hot-mirror builder is its sole production caller (the FOREIGN case is compiler-confined; the SAME-MODULE residual is policed by the strengthened single-producer guards, pinned by `structural_carrier_producer_lowerer_is_module_private`), plus the ordering tripwire `no_production_macro_arg_eager_lowering_outside_mirror` banning eager macro-arg lowering outside the mirror — the GLOBAL decl-body producer flip is the **separate deferred query-free declaration-body structural-template producer** (NOT Stage 6, NOT landed). (Authority: the decider ruling — the narrow macro structural producer is sound as a single-entry mirror, so the macro PRODUCER is LIVE after Stage 5A while the global decl-body producer stays a separate deferred future design.) The `type_args` carrier class is closed enum-wide by two PRE-EXISTING (pre-Stage-6) protections: the enum-wide anti-tail guard `no_named_type_args_field_outside_opaque_carrier` and the footprint carrier-arg structural-hashing fix — both introduced before the Stage-6 work and NOT Stage-6 deliverables; the earlier "remain open / move to the global flip" framing is corrected because the protections already existed. Precisely:
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4924:docs/arch/parselower-design.md:70:scope: make the ONE query-time dispatch ABLE to resolve a `BareRef`/`ImportType` carrier HEAD, recovering exactly the resolution the live `TypeExpr::Ref` / `TypeExpr::ImportType` path performs at `lower.rs` — bare-name resolution (`name_resolution` fast-path + `resolve_bare_name_in_scope`), external/relative module-augmentation stitch, enum-member projection, builtin-shadowing suppression, the `is_instantiate_active` recursion back-edge — then route the resolved head through `ResolveDecl` / `Instantiate` / `ProjectPath` exactly as the `Ref` path does. Factor that resolution into ONE shared helper pair on `ProjectSemanticDispatch` (in the `carrier` module): `resolve_bare_ref_head(&self, ctx: &CarrierResolverContext, name, arg_count: usize, lower_args: impl FnOnce() -> Arc<[SemanticNodeId]>) -> SemanticNodeId` and `resolve_import_type_head(...) -> SemanticNodeId`, each returning a `SemanticNodeId` with `Opaque(Miss)` / `Opaque(RecursiveRef)` exactly like the `Ref` arm (NOT a `QueryResult`). The helper takes the arg `arg_count` plus a LAZY `lower_args` closure and resolves/classifies the head BEFORE lowering the type-args, so an unresolvable head never lowers dead args — the closure fires (yielding already-lowered `SemanticNodeId` args; the eager `Ref` arm lowers structurally inside it, carrier resolution reads args via `carrier_type_args`) ONLY on the resolved/consuming branches, performing NO raw-`TypeExpr` arg lowering on the resolver's own path, preserving typed-IR-only + single-resolver. `CarrierResolverContext` is the value-side bundle (env / scope / `name_resolution` / `DeclarationScopePayload` / `ScopeShadowing` / reduction-demand) — `substitutions` and the active-instantiate stack stay OUTSIDE it (the stack is dispatcher state via `&self`). A carrier node alone does not carry `name_resolution`; 3B must PROVE `resolve_bare_name_in_scope + scope_payload` is semantically equivalent to the `name_resolution` fast-path, OR add a value-side rehydration — NEVER silently pass an empty `name_resolution`. Carrier-subject resolution enters at the canonical query entry (normalize a carrier subject after sugar-canonicalization, before cooperative memo admission — rewrite base-bearing keys `ProjectPath`/`KeyOf`/`MappedType` to the resolved subject, then memo the resolved key, preserving singleflight/dedup on the real semantic subject); the walker worklist re-enters the SAME dispatch normalization for nested carriers (inside `Intersection`/`Union`/heritage), NEVER resolving locally; the terminal pass-through/miss arms (`raise.rs`/`walk.rs`) stay as DEFENSIVE fallbacks. NO walker-local resolver, NO second engine. 3B does NOT wire the structural lowerer into production and does NOT remove the dormant-wiring guard — it makes the dispatch ABLE to resolve heads, proven by direct-constructed carriers driven through the dispatch.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4926:docs/arch/parselower-design.md:79:- The NARROW, single-entry, mode-neutral structural producer for the **session-owned macro hot mirror ONLY** is architecturally SOUND as an END-STATE and NOT a permanent dual path (every macro call site reads that ONE mirror; carrier resolution re-enters the ONE dispatch — it is not a second resolver). It does NOT require the global declaration-body flip and does NOT lower declaration bodies. This is now **Stage 5A** (below) — which the later architecture tie-breaker ruling (the orchestration ledger, R1) DEFERRED behind the non-breaking `Stage 5A-prep` mapped-key-domain consumer-side carrier resolution, so the atomic all-four-sites 5A could land only once that prerequisite existed (the partial interim is the forbidden dual path; see Stage 5A status). Both have now LANDED: the `Stage 5A-prep` prerequisite (refactor commit `8ff601882`) and then the atomic four-site Stage 5A producer cutover + the typeof negative-cache-recovery fix on the resume branch (`mom/stage5a-resume`). The macro hot mirror is the SOLE production producer and all four macro-arg sites read it.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4959:docs/arch/parselower-design.md:181:guards: cache-key lint forbids raw-text content + content/version hashes in shape/materialize keys; the regular member-shape subject `ShapeSubject::MemberValueNode` (component_meta_caches.rs:1187, the keyed `ShapeCacheKey` field used for REGULAR member dedup, not just synthetic) is DECIDED + LANDED as a CARVE-OUT (not a migration to a content-free R6 slot identity): it is reclassified as a generation/store-scoped graph-instance memo — a within-generation interning subject that is fact-rooted + generation-validated (single-entry, fact-validated, generation-gated), NOT a durable content-free R6 query-identity key, so R6's ban on content/version hashes + the versioned `DeclIdentity` does not apply to it. The carve-out is SEALED structurally: the subject keys on the module-private newtype `MemberShapeNodeSubject(SemanticNodeId)` (component_meta_caches.rs:1122) whose only sanctioned constructor reads a `&SurfaceMember` (`from_surface_member`), and the sole production key constructor is the narrow `ShapeCacheKey::surface_member_value_whole_with_context(scope, &SurfaceMember, ctx)` — a raw `SemanticNodeId` can no longer spread into the shape-key subject from an arbitrary node (the seal is the compiler/privacy mechanism, not a name scanner; guard `member_value_node_subject_is_sealed_newtype_and_member_constructed`). anon subjects bypass cache; producer+consumer schema versions advance atomically.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4960:docs/arch/parselower-design.md:190:guards: the reverse-materialization (`SemanticNodeId → TypeExpr`) fence landed as the sealed `OutputProjector` capability + sealed carriers (`OutputTypeExpr` / `MaterializedOutputTypeExpr`) at the true output sinks (block 8-A3, per-sink caps), plus the Kind-B raise-then-decide → node-domain conversion (block 8-A4, where the raise-then-decide callers C1-C4 each decide on node-domain facts/key and materialise once at a registered sink). C5 (the route fixpoint) is converted to the node-domain interned-shape cursor: convergence compares interned `RaisedShapeKey` identity via the sealed `AdmittedRouteProjectionNode` carrier across all its adapter legs, with ONE materialisation at the registered terminal surface sink (`materialize_route_projection_node`) — no per-iteration materialisation remains. The fence `hot_path_never_calls_materialize_type_expr` is enabled and green at zero offenders; the audited `HOT_TERMINAL_SINKS` allowlist names the pure one-shot publication sinks, self-policed by `hot_terminal_allowlist_entries_are_pure_one_shot_sinks`.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4963:docs/arch/parselower-design.md:215:FINAL STATE: the Stage-8 transitional materialize allowlist reached 0 (LANDED); residual Stage-4 dual-read is read-compat only (both arms route through the SAME dispatch — not a second resolution engine), faithful to CLAUDE.md's "one clean cutover / no double branches". The **Stage-5A** macro hot-mirror producer is likewise NOT a permanent dual path (authority: the decider ruling): it is a SINGLE-ENTRY producer — the macro hot mirror is the one production caller of `lower_type_expr_structural` (the eager `lower.rs` declaration-body path is unchanged — the LANDED Stage 6 Option-B flip mints `HotTypeRef` handles in the `decl_body_hot_ref` accessor over the `Instantiate` result the graph-bearing dispatch producer drives via the RESOLVING lowerer, and does NOT route declaration bodies through the structural lowerer), every macro call site reads that ONE mirror, and carrier resolution re-enters the ONE dispatch. It is not a callsite-scattered structural-vs-eager selection (which WOULD be a forbidden permanent dual lowering policy); the single entry is enforced by the COLLAPSED private module (`lower_type_expr_structural` is a bare module-private `fn` — NO visibility modifier — in `structural_carrier_producer::macro_arg_producer`, reachable from outside only through the `pub(crate)` `macro_type_arg_hot_ref` accessor: the FOREIGN case is compiler-confined and the SAME-MODULE residual is policed by the strengthened single-producer guards, pinned by the privacy-shape guard). Stage 6 Option B landed; the macro hot mirror remains the SOLE production caller of `lower_type_expr_structural`, and the single-entry guarantee + its privacy guard are unchanged by it (Option B mints handles in `decl_body_hot_ref` over the `Instantiate` result the graph-bearing dispatch producer drives via the RESOLVING lowerer, not through the structural lowerer). A decl-body structural-lowerer flip, if ever pursued, is separate future work with its own guarded surface. Per the architecture tie-breaker ruling at the orchestration ledger (R1), Stage 5A was DEFERRED behind the non-breaking `Stage 5A-prep` mapped-key-domain consumer-side carrier resolution precisely BECAUSE the partial interim — sites 1/2/4 read the mirror while `vue_exec` (site 3) stays eager — would have been a second base producer for the same macro type-argument syntax (the forbidden dual path), and a mapped-only deferral would silently lose slots (`slot_fields()` → `[]`). Now that `Stage 5A-prep` has LANDED (refactor commit `8ff601882`) AND Stage 5A itself has LANDED as the atomic all-four-sites flip (every macro call site reads the ONE mirror, single-entry, carrier resolution re-enters the ONE dispatch), the end-state is in force: NOT a dual path. The block 8-A4 Kind-B raise-then-decide conversion has LANDED (the interim bridge is gone — the raise-then-decide callers C1-C4 now decide on the node-domain facts/key and materialise once at a registered sink, so no Kind-B dual PATH remains; C5, the route fixpoint, is converted to the node-domain interned-shape cursor — the sealed `AdmittedRouteProjectionNode` compared by interned `RaisedShapeKey`, ONE resolver, NOT a second resolution path, so it is not a dual path either); the transitional materialize allowlist reached 0 and the hot-materialize fence `hot_path_never_calls_materialize_type_expr` is green at zero offenders (the global Unknown-as-control-flow fence it shipped alongside has since been deleted — the typed-degradation cutover removed the sentinel control-flow shape it scanned for), leaving ZERO permanent dual path. Stated explicitly so a reviewer does not read the transitional dual-read or the Stage-5A single-entry producer as a rule violation.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4965:docs/arch/parselower-design.md:224:- **LANDED in 8-A3 (the durable Kind-A fence).** Block 8-A3 landed the durable reverse-materialization (`SemanticNodeId → TypeExpr`) OUTPUT fence for the TRUE OUTPUT SINKS (Kind A) as a SEALED-TRAIT CAPABILITY — **`OutputProjector`** + the sealed carriers **`OutputTypeExpr`** (plain-raise) / **`MaterializedOutputTypeExpr`** (reduce-then-raise) — in `project_semantic_dispatch/output_materialization.rs`. The callable boundary MOVED OFF `ProjectSemanticDispatch` onto the `OutputProjector` capability; the carrier payload is a PRIVATE inner `TypeExpr` whose only reader is a capability-gated accessor (`into_type_expr` / `type_expr`, each taking `&impl OutputProjector`); each true output-**SINK** module (the exact module that projects — `meta_resolve::projectors::output_sink` (a DEDICATED terminal sink submodule, re-exported at `meta_resolve::projectors::MetaResolveProjectorsOutputCap` — extracted EXACTLY so the parent `projectors`' non-sink helpers (`macro_payload_substrate`, `published_reducer`, the per-kind projector children) cannot mint), `meta_resolve::materialize::field_types`, `host_manage::component_meta_methods`, `typeinfo::raise`, `typeinfo::framework_surface::{vue_exec,svelte_exec}` (`vue_exec`'s whole reachable scope — `vue_exec` + its `normalize` child — is output-only, so the single cap is correct), `component_meta_query_engine::{registry_decl,surface}` — **EIGHT per-sink caps**) owns a private-field capability whose constructor is `pub(in <sink-module>)` — scoped to the SINK, NOT the subtree. `pub(in P)` grants the mint to `P` AND every module at-or-under `P`, so the mint scope is scoped to a TERMINAL output sink whose entire reachable production module tree is itself output-only — that (not "per-leaf") is what makes the fence compiler-enforced for in-subtree code: a Kind-B bridge sibling (`meta_resolve::dispatch_helpers`, `host_manage::eval_env`) — or a non-sink helper sibling — is NOT reachable from any sink's mint scope and cannot name any cap's constructor (a planted `*OutputCap::new` there is `E0624`). In safe production Rust OUTSIDE the audited output-materialization payload vault, `OutputTypeExpr` and `MaterializedOutputTypeExpr` do not expose a readable `TypeExpr` field (the inner `TypeExpr` lives in the deeply-private `carrier::payload` vault): capability-free unwrap is unrepresentable by field access, auto-deref, arbitrary trait impls, or inherent methods, and the only production APIs returning `TypeExpr` / `&TypeExpr` are the capability-gated `into_type_expr` / `type_expr` accessors. The seal is COMPILER-enforced both OUT of crate and WITHIN it: `mod sealed` is PRIVATE (not `pub(super)`) inside `mod projector`, so `projector::sealed::Sealed` is nameable ONLY from within `projector` — a sibling `carrier` / `carrier::payload` (or any other crate module) that writes `impl projector::sealed::Sealed for HotCap` is `E0603` (module `sealed` is private), so the sealed trait cannot be implemented for a non-sanctioned type even from a sibling owner-descendant scope; the `output_projector_owner_registration_inventory` topology guard is the defense-in-depth backstop, not the primary. The mint half is the same shape (COMPILER-ENFORCED as the PRIMARY barrier): a hot / session / Kind-B mint is `E0624` / `E0451` (the terminal-sink capability constructor is unreachable from any non-sink module), a hot carrier-unwrap is `E0277` (unsatisfied `OutputProjector` bound — the trait is sealed) AND the inner `TypeExpr` is not even a readable field, and a hot `.type_expr_for_test()` is `E0599` (the carrier `_for_test` accessors are gated `#[cfg(any(test, feature = "test-support"))]` — the production-unreachable test-support feature activated only by `verter_session`'s own dev-dep self-edge — so they are COMPILE-ABSENT from every production build, unlike a `debug_assertions` gate which would be present in ordinary debug builds). The residual TRUSTED surface is the inline payload vault + the projector registration module (and the by-name identity of which owner types are sinks), whose exact item/module/signature shape is structurally guarded as DEFENSE-IN-DEPTH (a CLOSED structural allowlist, NOT the primary barrier); this claim does NOT cover guard deletion, deliberate edits inside that trusted vault, or unsafe code unless the crate forbids unsafe globally. The FFI boundary became the session-owned BYTES facade `VerterHost::project_node_to_type_expr_json_bytes` (mints the capability + materializes + unwraps + serializes to wire JSON internally; NAPI wraps the bytes in a `Buffer`, WASM does `String::from_utf8` — schema byte-identical, functionally identical); the old `pub fn project_node_to_type_expr` and the all-`pub`-field `MaterializedTypeExpr` struct are DELETED. The raw `raise_node_to_type_expr` primitive STAYS module-private; the reduce-then-raise orchestrator `raise_and_reduce_with_context` STAYS `pub(super)`; the shell output seam `output_shell_raise_sealed` returns a SEALED `Option<OutputTypeExpr>` (never a bare `TypeExpr`) so a `project_semantic_dispatch` sibling reaching the `pub(super)` seam cannot launder. The fence SHAPE is pinned by mechanism-matched guards over that trusted surface (a CLOSED structural allowlist, complete-by-construction): the EXACT module topology of the owner file — inline `projector` / `projector::sealed` / `carrier` / `carrier::payload` and nothing else, with item/impl/trait-position macro invocations, `include!`, unknown attributes, a `sealed::Sealed` alias `use`, and any owner-file `TypeExpr` alias BANNED — plus the sanctioned sink set (the explicit `impl OutputProjector` / `impl sealed::Sealed` self-types, compared by FULL self-type path as a MULTISET so a duplicate / last-ident-colliding registration is caught) by `output_projector_owner_registration_inventory`; a closed item/signature allowlist over the carrier/payload vault (every fn returning `TypeExpr` must be capability-gated or exactly test-gated) by `output_carriers_have_no_inherent_typeexpr_escape_method`; every carrier/payload struct field private regardless of spelled type by `output_carrier_payload_fields_are_private`; an accidental-regression CANARY for the common `Deref` / `AsRef` / `Borrow` trait escapes (NOT proof-complete — completeness for the unbounded escape-trait surface comes from the payload vault) in `project_semantic_dispatch/output_materialization_guards.rs`; the out-of-crate visibility boundary by the trybuild `output_projector_non_owner_impl_is_compiler_sealed`; the mintable `TestOutputCap` staying `#[cfg(test)]`-gated by `test_output_cap_not_visible_or_mintable_in_non_test_builds`; the terminal-sink mint scope by `output_cap_mint_scope_is_per_leaf_not_subtree` — a Rust-VISIBILITY reachable-module-tree model: for every `mint: pub(in P)` it builds the production module tree at P-and-below (excluding `#[cfg(test)]` modules) and default-DENIES any reachable module not on the cap's exact sink-module allowlist (a new non-sink descendant fails WITHOUT a denylist entry), paired with the walker self-test `mint_scope_module_tree_walker_self_test_discriminates`; the carrier `_for_test` gate by `carrier_for_test_accessors_are_test_support_gated_not_debug_assertions`; the sealed raise seam by `raise_output_seam_returns_sealed_carrier_not_bare_type_expr` (recursing non-test modules + transitive `TypeExpr` aliases) — STRENGTHENED at 8-A4 to assert ZERO public/restricted `raise.rs` bare-`TypeExpr` fns, no sanctioned bridge exception remaining.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:4967:docs/arch/parselower-design.md:227:- **The Kind-B raise-then-decide callers (C1-C4) decide node-domain and materialise once at a registered sink; C5 (the route fixpoint) is converted to the node-domain interned-shape cursor.** C1 (`execute_to_type_expr`, DELETED) → the demand-bound surface adapters in `resolver_core::component_meta_query_engine::surface`; C2 (`project_slot_binding_member_with_terminal_id`) is now node-only; C3 (`instantiate_local_generic_ref_via_dispatch`) → `instantiate_local_generic_ref_published`; C4 (the `eval_env` defineModel / ProjectPath / slot-binding raises) → the sink-owned demand methods `expand_define_model_output` / `expand_generic_project_path_output` / `expand_slot_binding_output` in `host_manage::component_meta_methods` (the eval_env branches pass only the closed demand — resolver ctx + owner canonical + macro index + the per-branch terminal demand — never a raw `SemanticNodeId`; the demand method resolves the carrier head + terminal INTERNALLY and materialises the produced node at the module-private sealed sink — the module-private `AdmittedExpansionNode` + `materialize_admitted_expansion_node`). C5 (the route fixpoint) stabilises on interned `shape_engine::RaisedShapeKey` identity via the sealed `AdmittedRouteProjectionNode` carrier across all its adapter legs and materialises EXACTLY ONCE at the registered terminal surface sink (`materialize_route_projection_node`) — no per-iteration materialisation remains, and the fence `hot_path_never_calls_materialize_type_expr` is enabled and green at zero offenders.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:5003:docs/arch/u2-query-value-domain-design.md:1012:  diffed structurally against `tsgo`'s resolved target. Owned at the U0 resolver rescope.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:5031:docs/arch/u6-flow-return-gaps-and-target.md:178:| `FR-D8` | `DEFER` under `AMD-004` | Exact structural completion and G10 discrimination; the current producer can still publish the G10 wrong-and-warm result. | D6 / `U6.LOOP_CLOSURE` | Must close before D6 enters `REVIEW`. Heavy implementation may begin only after the D6 lock contains a closed, code-first carrier inventory. The demanded `FunctionFlowGraph` must be the sole completion reducer; G10 must match the pinned checker, X05/X68/X80/X88 must remain checker-correct clean/warm, and no syntax-only classifier or second completion authority may exist. | `d6_structural_completion_closes_g10_without_false_refusals` |
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:5049:docs/arch/gate-integrity-ledger.md:56:| **GI-8** | banner validator | The canonical fence performs the check mechanically: it extracts the banner region structurally (between the CLI's two `----` rules), requires the region to be CLOSED, matches model and effort WHOLE-LINE, and fails the leg on mismatch — a substituted reviewer is refused, not merely noticed. **An earlier draft of this row claimed a reformatted banner "fails rather than passes"; that was FALSE and the review caught it.** With only the opening rule present, the awk scan ran to EOF, so the whole transcript — prompt echo included — became "the banner region", and the echoed policy lines satisfied both greps while the real banner read `cheap-substituted`. The fence now requires a CLOSED region, requires it to carry the banner's structural keys (`workdir:`/`provider:`/`sandbox:`/`session id:`), and requires exactly one `model:`/`reasoning effort:` line inside it. **It is still not spoof-proof, and the fence says so:** it scrapes a stream that also contains the echoed prompt, so a sufficiently banner-shaped block in the prompt plus a malformed real banner defeats it. Two further attempts to close it by text alone both failed under review — and one of them (anchoring on "the first line is a rule") was factually wrong about the CLI, which prints `Reading prompt from stdin...` and a version line first, so it REJECTED EVERY REAL LEG until it was actually run. That is the row's own justification, twice over: a check whose failure modes are understood only by its author is not a validator, and text-scraping a contaminated stream is the wrong substrate for a gate. What remains missing is CENTRALISED, TESTED enforcement — the check lives in a fence each caller copies rather than in one validated component, so a caller can still omit it, and the delimiter shape is an observed CLI behaviour, not a versioned contract. | One shared validator every leg routes through, with the banner contract pinned. Test SUITE: `banner_mismatch_blocks_the_leg`, `prefix_impostor_model_is_rejected` (a `-sol-mini` must not satisfy `-sol`), `unclosed_banner_region_fails_closed` (the regression above), `slug_in_prompt_echo_does_not_satisfy_the_banner`, `caller_that_skips_the_banner_check_is_rejected`. |
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:5088:docs/arch/native-typeinfo-parity-adapters-final-lift.md:168:- `component_meta_is_thin_framework_adapter_no_second_resolver` — asserts `@verter/component-meta` is a thin `FrameworkSurfacePayload` adapter with no second resolver / expander (cache-owned type recovery only); the framework surface is a structural projection of the published payload, not a re-resolution path.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:5149:docs/arch/ts-compat-two-mode-model.md:998:| `resolver_is_single_spec` | resolver / cache / session: (a) exact-token scan + (b) structural field-inventory over a closed target list, plus a registration `is_spec_selector` attestation (human-review) for novel-named selectors | (a) a re-introduced exact token — `SpecVariant` / `spec_variant` / `spec.diverge` / `TsCompat` / `ts_compat` / `bug_for_bug` / `compat_profile` reused ANYWHERE incl. as an enum VARIANT name (so `enum SemanticMode { Correct, TsCompat }` and a `compat_profile` axis are caught by **(a)**, not (b); NO `*_profile` glob, no "or equivalent" clause — `query_profile` / `compile_profile` / `tsx_profile` are legitimate non-spec profiles); (b) a deny-named OR forbidden-selector-TYPED field over the CLOSED list `SemanticQueryKey` / `FamilyKey` / `ComponentMetaResultKey` / `MaterializeRuntimeKey` / `MaterializationCacheKey` / `ShapeCacheKey` / named session-/per-key-context structs (`SessionResolverContext` / `InstantiateContext` / `MacroPayloadContext` / `ProjectionReductionContext`) / `SemanticQueryKeySpec` (TYPE in the CLOSED forbidden-selector-type set — NOT a `*Spec` glob — OR NAME in the (a) set; `query_profile` / `compile_profile` / `tsx_profile` allowlisted); rooted on the closed resolver-input surface + a registration meta-rule (new `derive(Hash)` `ProjectTypeStore` cache keys MUST register into (b) AND carry an `is_spec_selector` attestation). A FULLY NOVEL-NAMED selector is caught NOT by (a)/(b) mechanically but by that attestation — the SECOND acknowledged human-review link (peer to §6.4); oracle-harness / correction-metadata mentions are whitelisted (§4, §8) |
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:5159:docs/arch/architecture-lock/ledger/A2C/RESCOPE-PROPOSAL.md:328:| `FR-D8` | `DEFER` under `AMD-004` | Exact structural completion and G10 discrimination; the current producer can still publish the G10 wrong-and-warm result. | D6 / `U6.LOOP_CLOSURE` | Must close before D6 enters `REVIEW`. Heavy implementation may begin only after the D6 lock contains a closed, code-first carrier inventory. The demanded `FunctionFlowGraph` must be the sole completion reducer; G10 must match the pinned checker, X05/X68/X80/X88 must remain checker-correct clean/warm, and no syntax-only classifier or second completion authority may exist. | `d6_structural_completion_closes_g10_without_false_refusals` |
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:5217:docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:2460:| registered source/VFS/`PublishedRoot`/workspace snapshot | host-backed source, project, invalidation, and publication basis | preserve or converge into the single committed-input role before QueryRuntime convergence; do not create a second `InputStore` by name alone | VERIFY |
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:5524:   422	Before managed QueryRuntime/cache convergence, the current input/VFS/publication owner is classified and its snapshot/fact API is locked. Query infrastructure must not be built around a transitional or independently sampled source view.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:5604:   671	2. land the final relation/inference/effective-flow authorities and the exact identity/lifetime/admission contract for each consumer being migrated; global QueryRuntime/store convergence is not a prerequisite unless that consumer actually depends on it;
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:5660:   727	A `FlowDemandPlan` selects only nodes/edges/domains needed by the result. A fixed point runs only on selected obligations that require convergence. A missing registered prerequisite makes the plan invalid and cannot produce a complete result.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:5710:   876	Hits/tiny dependent work run inline. Many tiny independent items are chunked. Coarse independent parse/compile/projection work may fork through structured execution. Provider/I/O actors remain separate. Every fork family declares measured grain, fan-out, cancellation/budget inheritance, priority, queue bound, stack/recursion behavior, and structured lifetime. Mature process-local execution is used before custom runtime design.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:5841:   475	IDE, build, TypeInfo, lint, formatter, and framework consumers do not maintain independent parsers for equal bytes and syntax profile. If recovery or syntax capability is insufficient, Verter extends, forks, or replaces the shared frontend for both consumers rather than retaining a permanent fast-build/tolerant-IDE split.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:6552:crates/verter_session/src/project_semantic_dispatch/mod.rs:507:    pub(crate) fn new(ctx: &'a dyn ResolverContext) -> Self {
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:6580:crates/verter_session/src/project_semantic_dispatch/mod.rs:3803:    pub(crate) fn new(ctx: &'a dyn ResolverContext) -> Self {
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:6594:crates/verter_session/src/resolver_core/mod.rs:13:pub(crate) mod component_meta;
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:6595:crates/verter_session/src/resolver_core/mod.rs:14:pub mod component_meta_query_engine;
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:6596:crates/verter_session/src/resolver_core/mod.rs:15:pub mod component_meta_registry;
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:6597:crates/verter_session/src/resolver_core/mod.rs:16:mod component_meta_request;
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:6620:crates/verter_session/src/resolver_core/mod.rs:41:pub(crate) mod request_store_view;
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:6627:crates/verter_session/src/resolver_core/mod.rs:55:pub(crate) use host_resolver_context::HostResolverContext;
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:6628:crates/verter_session/src/resolver_core/mod.rs:57:pub(crate) use request_store_view::{CanonicalCompletionOverlay, RequestStoreView};
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:6630:crates/verter_session/src/resolver_core/mod.rs:61:pub(crate) use session_resolver_context::SessionResolverContext;
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:6631:crates/verter_session/src/resolver_core/mod.rs:72:pub(crate) use component_meta::component_meta_resolved_macros;
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:6632:crates/verter_session/src/resolver_core/mod.rs:80:pub(crate) use component_meta_request::{
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:6633:crates/verter_session/src/resolver_core/mod.rs:91:pub(crate) use component_meta_query_engine::{
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:6634:crates/verter_session/src/resolver_core/mod.rs:103:pub(crate) use component_meta_query_engine::{
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:6635:crates/verter_session/src/resolver_core/mod.rs:106:pub(crate) use component_meta_request::run_component_meta_request;
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:6636:crates/verter_session/src/resolver_core/mod.rs:107:pub(crate) use component_meta_request::ComponentMetaRequestHost;
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:7022:crates/verter_session/src/component_meta_indexed_access_early_out_tests.rs:38:/// `WorkspaceRead::is_workspace_owned`).
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:7040:crates/verter_session/tests/cases/component_meta_audit/harness.rs:231:        // `WorkspaceRead` trait surface).
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:7625:   612	/// trait hierarchy. There is no separate `ProjectResolverReader` or
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:8210:- `ComponentMetaResultDb<ComponentMetaAnalysis>` — final component-meta payload cache consulted by `get_component_meta` before any cold work.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:8222:**Resolver-context seal:** resolver-path code does NOT take `&VerterHost` directly. It takes `ctx: &'a dyn ResolverContext` — a `pub(crate)` sealed super-trait at `crates/verter_session/src/resolver_core/resolver_context.rs`. Only `VerterHost` implements `ResolverContext` (`sealed::Sealed` marker closed at trait definition). Guard `no_concrete_verter_host_in_seal_scope` mechanically forbids re-introducing `&VerterHost` parameters under the resolver_core/meta_resolve/host_manage/component_meta_query_engine seal scope. New trait-surface methods are an architectural decision; widen with care.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:8228:request-bound `ResolverContext` and fulfills `Runtime`, `Tsc`, or
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:8851:The `CURRENT_REQUEST_VIEW` thread-local, `EffectiveView`, and `*_in_view` helpers are retired; `RequestStoreView` (`crates/verter_session/src/resolver_core/request_store_view.rs`) itself remains a live `pub(crate)` overlay `StoreView`. Resolver-path helpers take `&HostStoreView` (or use the host's live probes directly). `HostStoreView::from_host(self)` snapshots a cheap immutable view of the host's current state; its cache-validation identity is the complete `StoreViewValidationToken`, while `StoreView::compat_token` returns the narrower `StoreViewCompatToken` lane identity (epoch + session + `validity_fingerprint` = the external-supersession fold) — see "Store-View Token, Lane Identity, and Singleflight" below.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:9079:The `CURRENT_REQUEST_VIEW` thread-local, `EffectiveView`, and `*_in_view` helpers are retired; `RequestStoreView` (`crates/verter_session/src/resolver_core/request_store_view.rs`) itself remains a live `pub(crate)` overlay `StoreView`. Resolver-path helpers take `&HostStoreView` (or use the host's live probes directly). `HostStoreView::from_host(self)` snapshots a cheap immutable view of the host's current state; its cache-validation identity is the complete `StoreViewValidationToken`, while `StoreView::compat_token` returns the narrower `StoreViewCompatToken` lane identity (epoch + session + `validity_fingerprint` = the external-supersession fold) — see "Store-View Token, Lane Identity, and Singleflight" below.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:9093:`HostStoreView` is Arc-backed by an immutable `StoreViewSnapshot`: `VerterHost::store_view_manager()` hands out ONE shared snapshot per `StoreViewValidationToken` generation by cheap `Arc` clone; `with_session_overlay` re-roots overlay/tombstone canonicals via copy-on-write so the shared base is never mutated in place. `RequestStoreView` (`resolver_core::request_store_view`) is the LIVE request-scoped read-through wrapper: it chains a `CanonicalCompletionOverlay` in front of the request-entry `HostStoreView` so mid-request additive loads (`ensure_loaded`/`ensure_indexed_ready_serve` successes the entry snapshot did not track) validate without a false miss. The `CanonicalCompletionOverlay` also carries the request-world prepared-decl bundle memo (`RequestBundleMemo`, reached through `CanonicalCompletionOverlay::bundle_memo()` and threaded explicitly into `prepared_decl_bundle_with_store_view` / consulted by `prepared_decl_bundle_with_context` via `ResolverContext::request_completion_overlay`). It is ONE memo covering the base, session-overlay and `RequestOnly` worlds, keyed `(canonical, BundleMemoWorld)` with the `StoreViewCompatToken` on the entry: `BundleMemoWorld::{Base, Overlay(content hash)}` keeps the base and session namespaces distinct (R17 keeps overlay-bearing bundles OUT of the shared `prepared_decl_bundles` cache, and the shared slot is keyed by canonical alone), while the token pins entries to ONE externally-coherent world so a stability-retry attempt under an externally-moved view misses and re-materialises. Admission is STRUCTURAL: `RequestBundleMemo::insert` itself refuses anything that is not `ReuseClass::is_request_reusable`, so a cancelled, partial, lease-missed, mutation-unstable or overflow-refused materialisation never enters it, and a `RequestOnly` entry replays its stored refusal on EVERY hit so reuse cannot launder the taint (provenance counter `bundle_request_memo_hits`; regressions in `crates/verter_session/src/request_bundle_memo_tests.rs`).
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:9100:**Singleflight LANE identity is NARROWER than the reuse oracle.** The coalescing-lane identity is `StoreViewCompatToken`, whose `validity_fingerprint` is the `lane_fingerprint` — delegating to `external_supersession_fingerprint`, the SAME oracle the promotion fence `is_stable` applies. The fingerprint folds ONLY the external-supersession dimensions: `store_view_epoch` + `project_generation` + workspace `content_generation` (file-set mutations — watcher recovery / dependency appearance — advance it without any host-side epoch; a snapshot's edge-currency gates evaluate against it at build time, so a cached snapshot MUST miss once it moves; a cold compute's own loads never advance it, so it cannot self-fence) + folded env hashes (`env_hash_fold`) + `project_identity` + frozen overlay identity. The additive generations (`artifact_generation` / `load_generation`) are DELIBERATELY EXCLUDED from the lane identity: a cold compute advances them through its OWN work (materialising content-addressed caches gated by `store_view_epoch`, loading its dependencies, admitting its own routes), so two identical concurrent requests snapshot at slightly different points in the load sweep — folding those generations would split them across separate lanes and spawn multiple cold winners instead of one leader + N-1 dedup-joining followers. Because the lane oracle IS the promotion oracle, a follower that joins a lane shares exactly the external dimensions the leader's promotion was gated on, so the leader's dedup-joined result is validation-equivalent for it; and a request whose snapshot externally-supersedes the leader's (an epoch / project / env / identity / overlay change, even at an equal `store_view_epoch`) gets a different lane key and forks its own lane — it never receives a result computed under a different external view. The complete `StoreViewValidationToken` (including the additive generations) REMAINS the store-view reuse/validity oracle — the `StoreViewManager` rebuilds its base snapshot on any additive-generation change; only the LANE identity was narrowed (reuse-oracle = full token; lane-identity = external fingerprint).
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:9119:- **Every branch of a cache-presence-selected lane binds a REQUEST-BOUND context.** `VerterHost::build_template_class_semantic_facts` (`host_manage/analysis_io.rs`) picks its resolver context from artifact presence: the base-publishable + `IndexedReady`-present branch builds a `HostResolverContext::from_cold_seed`, the other composes a `SessionResolverContext::from_cold_seed`. The resolver-tier builder takes `&dyn RequestBoundResolverContext` — the sealed marker implemented for those two contexts and NEVER for `VerterHost` — so a bare-host binding is a COMPILE error, not a cache-presence-dependent runtime abort. The rail matters because `classify_binding` demands `prepared_value_decl` for every template `:class` script binding, and only a request-bound context can serve a prepared declaration. Regression: `template_class_lane_context_tests.rs` (indexed-present assertion + cold-seed control).
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:9120:- **Cold contexts carry `Current` vs `ColdSeed`.** `HostResolverContext::from_current(&CurrentHostStoreView)` vs `HostResolverContext::from_cold_seed(&ColdSeedHostStoreView)` — and the session-bound counterpart `SessionResolverContext::from_cold_seed(&ColdSeedHostStoreView)`. The cold-seed constructor marks the request-bound `RequestStoreView` non-current iff the seed was `ReturnOnly`; its `validates*` family then fails CLOSED, so every nested warm-cache probe inside the dispatch MISSES rather than validating against the stale seed. This is the single-chokepoint enforcement — no individual nested validator knows about currentness.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:9122:  - **A helper that does its OWN fresh read** takes the cold-seed straight from that read: `self.resolver_store_view_read().into_cold_seed_view()` (then `.with_session_overlay(..)` for the session path). The view-bound component-meta cold compute (`VerterHost::view_bound_cold_seed` → `compute_component_meta_state_with_view` / `_from_captured_with_view`) and the bare-host overlay entries (`compute_component_meta_state_with_overlay` / `_from_captured_with_overlay`) use this — currentness and view come from one read, no flag to mismatch.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:9124:- **The cold-seed escape hatch (`ColdSeedHostStoreView::into_inner`) never feeds a validating context (INDIRECT-validation seam).** The raw unwrap `.into_cold_seed_view().into_inner()` DROPS the `is_current` flag. It is confined to NON-validating consumers: the request-driver `snapshot_store_view()` accessors, the overlay-priority `capture_component_meta_inputs_with_view` (builds `CapturedComponentMetaInputs` only), and `#[cfg(test)]` direct-`host` wrappers. The fallthrough resolver validates its per-element / per-child / per-root node-cache entries through the request-bound `ctx.store_view()` (currentness-gated), not a separately-rebuilt raw `HostStoreView`.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:9126:- **The raw-view escape hatch is allowlisted.** `StoreViewRead::into_owned_view()` yields a raw `HostStoreView` ONLY for the bare-host owned-view rail (`ResolverContext::resolver_store_view`, reachable when no request-bound context was installed), the request-driver owned-view snapshot accessors (currentness gated by `snapshot_view_is_current`), and `.into_cold_seed_view().into_inner()` fenced cold seeds.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:10044:   612	/// trait hierarchy. There is no separate `ProjectResolverReader` or
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:10529:   114	A6 accepts an architecture premise ledger. Falsified premises stop affected work, preserve independently valid work, and trigger refresh, rescope, or ADR amendment according to their class.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:10553:   422	Before managed QueryRuntime/cache convergence, the current input/VFS/publication owner is classified and its snapshot/fact API is locked. Query infrastructure must not be built around a transitional or independently sampled source view.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:10936:   114	map. An undiscovered public/wire consumer is an **abort/rescope** condition, not a widening.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:10960:   187	## Abort/rescope
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11074:    96	  `analysis::component_meta::OrderedSfcStructureAnalysis` plus `refs`), so it is not neutral. It
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11137:   159	DEBT A5-DD1  Disposition: DEFER
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11176:docs/arch/refactor/rev11/evidence/A5/dependency-direction.md:159:DEBT A5-DD1  Disposition: DEFER
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11192:docs/arch/refactor/rev11/evidence/A5/dependency-direction.md:159:DEBT A5-DD1  Disposition: DEFER
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11215:crates/verter_semantic/src/analysis/project_resolver.rs:3://! All resolver types and `ProjectResolver` live in `verter_workspace`.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11243:/bin/zsh -lc "rg -n \"trait (ResolverContext|RequestBoundResolverContext)|sealed|ensure_loaded|ensure_indexed_ready_serve|struct (HostResolverContext|SessionResolverContext)\" crates/verter_session/src/resolver_core/resolver_context.rs crates/verter_session/src -g '*.rs' | head -250" in <MACHINE_ROOT>/verter
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11245:crates/verter_session/src/resolver_core/resolver_context.rs:1://! sealed `ResolverContext` super-trait.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11249:crates/verter_session/src/resolver_core/resolver_context.rs:83:    /// Marker trait `ResolverContext` is sealed against. Only types
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11250:crates/verter_session/src/resolver_core/resolver_context.rs:161:pub(crate) trait ResolverContext: sealed::Sealed {
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11257:crates/verter_session/src/resolver_core/resolver_context.rs:746:impl<'a> sealed::Sealed for crate::resolver_core::host_resolver_context::HostResolverContext<'a> {}
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11260:crates/verter_session/src/resolver_core/resolver_context.rs:780:pub(crate) trait RequestBoundResolverContext:
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11261:crates/verter_session/src/resolver_core/resolver_context.rs:781:    ResolverContext + sealed::RequestBoundSealed
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11264:crates/verter_session/src/resolver_core/resolver_context.rs:838:                 production; construct HostResolverContext at the request entry"
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11265:crates/verter_session/src/resolver_core/resolver_context.rs:870:                 production; construct HostResolverContext at the request entry"
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11266:crates/verter_session/src/resolver_core/resolver_context.rs:899:                 production; construct HostResolverContext at the request entry"
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11271:crates/verter_session/src/resolver_core/resolver_context.rs:993:                 construct HostResolverContext::new(host, &view, overlay) at the request entry"
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11272:crates/verter_session/src/resolver_core/resolver_context.rs:1036:                 production; construct HostResolverContext::new(host, &view, overlay) at the \
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11273:crates/verter_session/src/resolver_core/resolver_context.rs:1069:                 from production; construct HostResolverContext::new(host, &view, overlay) at the \
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11274:crates/verter_session/src/resolver_core/resolver_context.rs:1096:                 called from production; construct HostResolverContext at the request entry"
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11275:crates/verter_session/src/resolver_core/resolver_context.rs:1122:                 production; construct HostResolverContext at the request entry"
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11276:crates/verter_session/src/resolver_core/resolver_context.rs:1186:                 from production; construct HostResolverContext at the request entry"
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11278:crates/verter_session/src/fact_signature_helpers.rs:891:///   `FileArtifactStore` artifact (base via [`ResolverContext::ensure_indexed_ready_serve`],
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11321:crates/verter_session/src/component_meta_host.rs:189:    pub fn ensure_loaded(&self, canonical_id: &str) -> Result<bool, ComponentMetaHostError> {
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11322:crates/verter_session/src/component_meta_host.rs:194:            .ensure_loaded(canonical_id)
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11337:crates/verter_session/src/cache_runtime/world_snapshot.rs:237:    //!    `&dyn ResolverContext`, a `pub(crate)` trait sealed to the
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11419:crates/verter_session/src/host_manage_tests.rs:17850:// `request_store_view_extends_across_mid_request_ensure_loaded` is
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11431:crates/verter_session/src/for_tests.rs:598:/// This shim is needed because `ResolverContext` is sealed — integration
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11494:crates/verter_session/src/component_meta_materialize.rs:1197:            // not the wrapping Pick. See R8-2. The sealed
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11497:/bin/zsh -lc "nl -ba crates/verter_session/src/resolver_core/resolver_context.rs | sed -n '1,110p'; nl -ba crates/verter_session/src/resolver_core/resolver_context.rs | sed -n '150,285p'; nl -ba crates/verter_session/src/resolver_core/resolver_context.rs | sed -n '735,810p'; nl -ba crates/verter_session/src/resolver_core/resolver_context.rs | sed -n '895,920p'; nl -ba crates/verter_session/src/resolver_core/host_resolver_context.rs | sed -n '1,130p'; rg -n \"struct SessionResolverContext|host:|VerterHost\" crates/verter_session/src/resolver_core/session_resolver_context.rs | head -80" in <MACHINE_ROOT>/verter
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11499:5://! The base `impl ResolverContext for VerterHost` returns
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11500:60:/// [`crate::VerterHost`] except [`ResolverContext::active_session_view`]
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11503:85:pub(crate) struct SessionResolverContext<'a> {
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11584:    85	pub(crate) struct SessionResolverContext<'a> {
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11590:    91	impl<'a> SessionResolverContext<'a> {
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11676:crates/verter_session/src/component_meta_host.rs:189:    pub fn ensure_loaded(&self, canonical_id: &str) -> Result<bool, ComponentMetaHostError> {
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11840:   111	    /// integration tests + the test-only arm on `impl ResolverContext
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11850:   121	        // Route through a cold-seed `HostResolverContext` so the warm-cache
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:11857:   128	        let ctx = crate::resolver_core::HostResolverContext::from_cold_seed(self, &view, overlay);
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12024:  1910	                    component_meta_trace_custom!(
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12036:   161	        ctx: &dyn crate::resolver_core::ResolverContext,
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12079:   204	        ctx: &dyn crate::resolver_core::ResolverContext,
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12121:   145	        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12148:   172	        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12180:   723	    ctx: &dyn crate::resolver_core::ResolverContext,
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12207:  1910	                    component_meta_trace_custom!(
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12242:  1945	            component_meta_trace_custom!(
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12371:    23	convergence of context/lifecycle plumbing, dependency direction, and the addition of a batched,
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12381:    33	`ResolverContext` trait with large amounts of copy-pasted delegation) and by omission (no lifecycle exists
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12382:    34	yet that cannot block a thread on I/O), not by a structural authority that forces convergence and permits
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12386:    38	- A module/name/type/relation query issued through `HostResolverContext`, `SessionResolverContext`, or
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12395:    47	- The sealed `ResolverContext` trait (`resolver_core/resolver_context.rs:161`) has exactly as many
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12407:    59	  a different completeness verdict depending on which `ResolverContext` implementor served it, for
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12414:    66	point). `ResolverContext` implementors are lifecycle *adapters* over that one authority, never alternate
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12424:    76	| C1-AC-1 | Same query, same content, different lifecycle ⇒ same resolved answer | New characterization suite driving one fixed corpus of queries through `HostResolverContext` and `SessionResolverContext` (and the new I/O-free context once added), asserting structural equality of `SemanticNodeId` surfaces, sibling to `resolver_core/*_tests.rs` |
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12426:    78	| C1-AC-3 | Authority-uniqueness contract holds after convergence | Existing `project_semantic_dispatch_invariants_tests.rs` + the five-row Authority-uniqueness contract stay green, unmodified in substance |
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12427:    79	| C1-AC-4 | A resolver-tier call site cannot reach resolution without a request-bound context | `RequestBoundResolverContext` sealed marker (`resolver_context.rs:780-783`) becomes the *only* production-constructible path if the bare `impl ResolverContext for VerterHost` production rail is deleted (see Legacy Deletions) |
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12429:    81	| C1-AC-6 | Duplicated lifecycle-adapter boilerplate collapses | Diff proof: the ~10 near-identical delegation methods listed under Changes shrink to one shared implementation; `HostResolverContext`/`SessionResolverContext` retain only the ~9 genuinely session-specific overrides |
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12434:    86	(`performance-gates.toml:296`) — the two counters convergence work directly touches, since every
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12435:    87	`ResolverContext` call funnels to `dispatch()` (`host_resolver_context.rs:494-500`,
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12440:    92	`union_member_explosion=100`) are budget *policy*, not owned by C1 — convergence must reuse them unchanged
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12443:    95	converged `ResolverContext` construction path must not add a clone, extra `Arc` construction, or
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12444:    96	normalization pass beyond what `HostResolverContext::new`/`SessionResolverContext::new` already do per
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12449:   101	## Current-tree convergence map
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12454:   106	| TypeScript-style module/path/package resolution | `crates/verter_workspace/src/resolver.rs:122` (`ProjectResolver`, 2122 lines) | Converge — physically the `ModuleResolverCore` target, wrongly homed in the scheduler/tsgo-dependent `verter_workspace` crate | C1 |
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12467:   119	same delegation shape over a different receiver (`self.inner` vs `ResolverContext::method(self.inner,
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12502:   154	- The bare `impl ResolverContext for VerterHost` (`resolver_context.rs:817`) production-reachable method
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12506:   158	  bare-host rail once convergence lands (verify at implementation time — every currently-known production
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12507:   159	  entry already binds `HostResolverContext`/`SessionResolverContext`), delete the impl entirely and let
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12508:   160	  `RequestBoundResolverContext` (the sealed marker already excluding `VerterHost`,
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12540:   192	- **Sealed-trait lifecycle closure.** `ResolverContext: sealed::Sealed` (`resolver_context.rs:82-98,161`)
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12542:   194	  if the bare-host rail is deleted (see above), tightens `RequestBoundResolverContext`
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12544:   196	  `ResolverContext`" to "identical to it" — a resolver-tier function taking `&dyn ResolverContext` becomes
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12559:   211	  existing seal, a marker type for "I/O-free context" analogous to `RequestBoundResolverContext`, or a
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12564:   216	  I/O-free context (e.g. an `IoFreeResolverContext` sealed subtrait, mirroring
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12565:   217	  `RequestBoundResolverContext`'s shape) that is never implemented for any type holding a `&VerterHost`
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12580:   232	authority may exist after `D2`"). If convergence work touches `project_semantic_dispatch/relation.rs`,
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12588:   240	prepare/plan/project/emit state machine. C1's deliverable to C2 is: a `ResolverContext`-shaped kernel
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12597:   249	three (or more, once the I/O-free context exists) `ResolverContext` implementors give bit-identical
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12598:   250	resolved answers for identical inputs; the duplicated boilerplate between `HostResolverContext` and
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12599:   251	`SessionResolverContext` is gone; `verter_semantic`'s production closure excludes `verter_scheduler` and
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12613:   264	| conformance | charter, diff, and the deletion set — including whether the bare-host `ResolverContext` rail was actually deleted (or, if retained, why a production call site still needs it), and whether A5-DD1 closed without a subset-checked allowance |
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12617:   268	## Abort/rescope
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12620:   271	`verter_workspace::resolver::ProjectResolver` is not, in fact, cleanly separable from `verter_workspace`'s
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12623:   274	research did not find; or a locked-cell regression on `A6_META_COMPILE_40_COLD_RUST` that convergence
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12629:   280	implementation against them without a ruling — each is a real design fork, not a detail:
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12647:   298	   not-yet-written committed-input-authority design. Flagged, not resolved, in Legacy Deletions above.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12649:   300	   review-enforced one.** I recommend a sealed `IoFreeResolverContext`-shaped marker (mirroring
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:12650:   301	   `RequestBoundResolverContext`) so a resolver-tier function generic over it cannot reach
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:13332:    18	Planning discovers the complete product prerequisite closure and closed semantic projection batch before projection. Emission does not discover new project-semantic demands. A new demand requires a new plan.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:13621:   691	    /// `SessionResolverContext`) resolve through the same host method.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:13644:   714	    /// [`crate::resolver_core::session_resolver_context::SessionResolverContext`])
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:13653:   723	    /// `impl ResolverContext for VerterHost` cannot satisfy — it owns no
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:13716:  1555	    /// trait method `ResolverContext::current_fact_tracer` and exists
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:13730:/bin/zsh -lc "rg -n \"Landed guards are structural|Planning|name-keyed|source-tree|scanner|breaking|migration breadth|a lot of work|best architecture\" CLAUDE.md | head -80; nl -ba CLAUDE.md | sed -n '445,495p'; rg -n \"Time constraints|breaking-change|migration|best architecture|approved plan\" CLAUDE.md" in <MACHINE_ROOT>/verter
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:13738:515:### Planning
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:14306:     3	//! All resolver types and `ProjectResolver` live in `verter_workspace`.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:14323:    20	    IdeProjectConfig, NativeProjectResolver, ProjectMembership, ProjectResolver, WorkspaceAlias,
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:14346:   117	// ── ProjectResolver ──
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:14351:   122	pub struct ProjectResolver {
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:14355:   126	/// Backward-compatible type alias for [`ProjectResolver`].
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:14359:   130	pub type NativeProjectResolver = ProjectResolver;
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:14361:   132	impl ProjectResolver {
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:14379:First, the draft is factually wrong that both modules are private: `resolver_core` is `pub mod`; only `resolver_store` is private (`crates/verter_session/src/lib.rs:341-342`). That does not rescue in-place convergence.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:14411:The marker is externally sealed, but it does not prove I/O freedom. `RequestBoundResolverContext` extends `ResolverContext` (`resolver_context.rs:780-782`), so an analogous subtrait inherits `ensure_loaded`, `ensure_indexed_ready_serve`, and the direct host escape hatch (`resolver_context.rs:261-266,700-717`). Both current request-bound implementors physically contain `&VerterHost` (`host_resolver_context.rs:81-84`; `session_resolver_context.rs:85-89`) and delegate those methods to it (`host_resolver_context.rs:264-287`). `ensure_loaded` submits to and waits on the scheduler (`host_lifecycle.rs:988-1013`).
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:14415:Ruling: extraction and I/O confinement are one decision. In the extracted kernel, use a separate capability-limited immutable observation interface that does **not** extend current `ResolverContext` and cannot return a host. The crate dependency firewall then makes naming `VerterHost` or the scheduler impossible. A marker may classify that interface, but the marker alone is not the proof.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:14429:FORK 3 — **VIOLATES — extraction plus a capability-limited observation interface; no `ResolverContext` marker.**  
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:14435:First, the draft is factually wrong that both modules are private: `resolver_core` is `pub mod`; only `resolver_store` is private (`crates/verter_session/src/lib.rs:341-342`). That does not rescue in-place convergence.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:14467:The marker is externally sealed, but it does not prove I/O freedom. `RequestBoundResolverContext` extends `ResolverContext` (`resolver_context.rs:780-782`), so an analogous subtrait inherits `ensure_loaded`, `ensure_indexed_ready_serve`, and the direct host escape hatch (`resolver_context.rs:261-266,700-717`). Both current request-bound implementors physically contain `&VerterHost` (`host_resolver_context.rs:81-84`; `session_resolver_context.rs:85-89`) and delegate those methods to it (`host_resolver_context.rs:264-287`). `ensure_loaded` submits to and waits on the scheduler (`host_lifecycle.rs:988-1013`).
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:14471:Ruling: extraction and I/O confinement are one decision. In the extracted kernel, use a separate capability-limited immutable observation interface that does **not** extend current `ResolverContext` and cannot return a host. The crate dependency firewall then makes naming `VerterHost` or the scheduler impossible. A marker may classify that interface, but the marker alone is not the proof.
docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:14485:FORK 3 — **VIOLATES — extraction plus a capability-limited observation interface; no `ResolverContext` marker.**  

exec
/bin/zsh -lc "sed -n '1,240p' docs/arch/refactor/rev11/charters/C1.md && sed -n '241,451p' docs/arch/refactor/rev11/charters/C1.md && sed -n '520,590p' CLAUDE.md && sed -n '1,55p' CLAUDE.md" in <block worktree root>
 succeeded in 0ms:
# C1 — Converge `ModuleResolverCore` and non-flow `TypeInfoCore`

**Status:** DRAFT — authored for maintainer review; no AMD ratifies it yet. **Class:** Foundational
(`program-dag.toml:162`). **Predecessors:** `A6`, `B1`, `B2` (`program-dag.toml:163`) — all three
`ACCEPTED` (`docs/arch/architecture-lock/ledger/program-state.toml:216,237,447`).

**Rulings applied.** This charter was rewritten to conform to `ARCH-RULING-C1-FOUR-FORKS.md` (the
architecture challenge against the prior draft's four open forks). All four of that draft's proposed
positions were **REJECTED**. See "Rulings applied" below for the four verdicts and their consequences; do
not relitigate them here.

## Context — why this is bigger than program.md's five lines

`program.md:191-197` gives C1 five lines: converge two logical owners, preserve resolution semantics,
use immutable observation views, return batched `NeedInputs`, exclude flow. Read literally that sounds
like an in-place refactor. **It is not.** The four rulings resolve every genuine fork in the direction
that costs the most, and the charter says so plainly rather than understating it:

1. **This is a real crate extraction, now, not a deferred one.** `resolver_core` (`pub mod`, 59 files —
   including `component_meta/` (6 files) and `component_meta_query_engine/` (14 files): verified, neither
   holds a `&VerterHost`/`Arc<VerterHost>` field or parameter in production code, only doc-comment mentions
   and `#[cfg(test)]` `VerterHost::new` fixture construction identical to the pattern already accepted for
   `resolver_core`'s own top-level `*_tests.rs` files, so both subdirectories move with the rest of the
   wildcard, not a carve-out — see the convergence map below),
   the private `resolver_store` module, and the `pub(crate)` `project_semantic_dispatch` module —
   all three currently living inside `verter_session` (`crates/verter_session/src/lib.rs:332,341-344`)
   — physically relocate into the existing `verter_semantic` crate (layer 3). So does
   `verter_workspace::resolver::ProjectResolver` (`crates/verter_workspace/src/resolver.rs`, 2122
   lines), the TypeScript-style module resolver wrongly homed in the scheduler/tsgo-dependent
   `verter_workspace` crate today. This is a multi-crate structural move touching `verter_session`'s
   module tree, `verter_workspace`'s module tree, `verter_semantic`'s Cargo.toml dependency edges, and
   the landed dependency-layers closure guard — not a `pub(crate)` facade behind which nothing moves.
2. **The `NeedInputs`/`AttemptOutcome` cutover is full-coverage, not a first cut.** Every non-flow
   `ModuleResolverCore`/`TypeInfoCore` operation reachable from a C2 projection attempt must support the
   batched outcome — not just the two `ensure_loaded`/`ensure_indexed_ready_serve` call sites the prior
   draft scoped it to. TypeInfo projection already reaches `ensure_indexed_ready_serve` today
   (`crates/verter_session/src/typeinfo/shallow_surface.rs:179-193`), so "module/import resolution only"
   was never even a clean existing boundary to cut at.

Effort and breadth are not reasons to shrink this (`CLAUDE.md` → Planning: "Time constraints,
implementation size, migration breadth... are not valid reasons to weaken the design"). The charter
states the true scope instead of preserving the five-line framing that would mislead whoever picks this
up. C1 converges the current host-backed module/type-resolution stack — `verter_session::resolver_core`,
`verter_session::resolver_store`, `ProjectSemanticDispatch`, and `verter_workspace::resolver` — into the
two target logical owners named in `architecture.md` §8.1: a `ModuleResolverCore` that owns
TypeScript-style module/path/package resolution, and a non-flow `TypeInfoCore` that owns authored-node
traversal, binder/name/reference semantics, substitution, relation/inference, effective typing,
recursion/work budgets, query-specific projection, and exactness/completeness propagation. Neither
target type exists in code today; both names describe roles the current tree already plays under
different names, in the wrong crate, with the wrong I/O coupling. C1 does not invent new resolution
semantics — every module/name/type/relation answer the converged kernel gives must be identical to
today's answer for every existing lifecycle. It changes who owns the code (crate, not just module), how
many duplicate implementations of the same lifecycle-adapter shape exist, whether the kernel can perform
I/O directly, and where the crate boundary sits.

C1 does not own flow semantics (`D1`–`D8`), Vue/Svelte macro projection (`C3`), the staged
prepare/plan/project/emit compile transaction or its anti-replay tokens (`C2`), or component-meta
publication policy (`/component-meta`). It extends nothing about `execute_relate`'s relation semantics or
`ProjectSemanticDispatch`'s five query modes — those are `Preserve`, not `Converge`, dispositions, even
though (per the full-coverage ruling) the *plumbing* around every call into them now must support
`AttemptOutcome`. It owns convergence of context/lifecycle plumbing, dependency direction, physical crate
placement, and full-coverage batched/resumable load outcomes so a future I/O-free lifecycle can consume
the same kernel a blocking lifecycle consumes today.

## Sequencing

C1's predecessors (`A6`, `B1`, `B2`) are all `ACCEPTED`; C1 is dependency-eligible now
(`docs/arch/architecture-lock/ledger/program-state.toml:594`, `status = "READY"`). The program executes
one block at a time, and the ledger's `current_block` is `BV1`, `status = "IN_PROGRESS"`
(`program-state.toml:22,510-513`). **C1 dispatch follows BV1's landing.** This charter does not propose
running C1 in parallel with BV1 or with any other in-flight block.

## Intent Contract

**Actor / problem.** Every current and future first-party consumer of Verter's type/module resolution —
the host-backed LSP session, the session-overlay (unsaved-edit) path, and the future project-aware direct
compiler (`C2`'s sealed `CompileTypeInfo`) — must resolve a symbol, import, or type to the *same* answer.
Today that guarantee is held by convention (three lifecycle adapters hand-implement the same sealed
`ResolverContext` trait with large amounts of copy-pasted delegation), by omission (no lifecycle exists
yet that cannot block a thread on I/O), and by the wrong crate holding the kernel at all (a session-tier
crate that already depends on the compiler, so nothing compiler-tier can embed it without a cycle) — not
by a structural authority that forces convergence and permits a new I/O-free lifecycle to be added
without duplicating resolution logic a fourth time.

**Required observable outcomes.**
- A module/name/type/relation query issued through `HostResolverContext`, `SessionResolverContext`, or
  (once added) an I/O-free project-capable context returns bit-identical `SemanticNodeId`/resolved-surface
  results for the same source content, project configuration, and query, modulo the outcome shape
  (blocking-complete vs. `NeedInputs`).
- A resolver-tier operation that needs a not-yet-loaded dependency and runs under an I/O-free environment
  returns `AttemptOutcome::NeedInputs(LoadSet)` (per `contracts/input-loading.md` §2, §4) instead of
  panicking, blocking, or fabricating a partial answer — for **every** non-flow `ModuleResolverCore`/
  `TypeInfoCore` operation reachable from a C2 projection attempt, not a subset.
- `verter_semantic`'s production dependency closure contains neither `verter_workspace`,
  `verter_session`, `verter_scheduler`, nor `verter_tsgo_api` on any target — this closes debt row A5-DD1
  by *deleting* the exception, not widening it (`crates/verter_identity/tests/cases/
  workspace_dependency_layers.rs:118-127`, the `ratified_upward_exceptions()` row keyed
  `"verter_semantic"`).
- `resolver_core`, `resolver_store`'s immutable observation types, `project_semantic_dispatch`, and
  `ModuleResolverCore` (the relocated `verter_workspace::resolver::ProjectResolver`) physically live in
  `verter_semantic`. `verter_compiler` can depend on the converged kernel directly — it already does
  (`crates/verter_compiler/Cargo.toml:59`, `verter_semantic = { path = "../verter_semantic" }`) — with no
  new edge and no cycle.
- A new capability-limited, immutable **observation interface**, defined inside `verter_semantic`, does
  **not** extend today's `ResolverContext` and cannot name, hold, or return `VerterHost` or the
  scheduler — not because a marker says so, but because `verter_semantic`'s crate dependency closure
  makes those types structurally unnameable from inside it.
- The sealed `ResolverContext` trait (relocated to `verter_semantic`) has exactly as many
  production-reachable implementors as there are genuinely distinct blocking lifecycles; a lifecycle that
  cannot resolve (e.g. a bare host with no request/session binding) is a compile-time impossibility for
  resolver-tier call sites, not a documented runtime `panic!`.

**Forbidden observable outcomes.**
- A second implementation of `SemanticQueryApi::execute`, `execute_relate`, `shallow_lower_type_expr`, a
  second struct owning a `RelationMemo` or the semantic node map (the existing Authority-uniqueness
  contract, `.claude/skills/type-resolution/SKILL.md:790-796` — C1 preserves this unchanged across the
  crate move).
- A converged kernel that reads live, un-snapshotted host state at validation time (defeats "immutable
  observation views").
- A lifecycle-specific answer: the same query returning a different resolved type, a different route, or
  a different completeness verdict depending on which `ResolverContext`/observation-interface implementor
  served it, for identical inputs.
- A blocking wait introduced on the path a future I/O-free lifecycle uses to reach `NeedInputs` — the
  point of the new outcome is that that path never calls `wait_or_drive`/`Condvar::wait`.
- A marker type presented as *the* proof of I/O freedom. Per the ruling on Fork 3, a marker may
  *classify* the observation interface; the proof is that the interface's home crate cannot name the
  host/scheduler types at all. A subtrait of `ResolverContext` is never an acceptable substitute — it
  inherits `ensure_loaded`/`ensure_indexed_ready_serve`/the host escape hatch
  (`resolver_context.rs:780-782,261-266,700-717`) exactly as the rejected draft position did.
- Any resolver-tier operation reachable from a C2 projection attempt that is left blocking-only, with no
  `NeedInputs`-capable path — that is exactly the coverage gap Fork 4's ruling forecloses.

**Authority/fallback order.** `SemanticQueryKey → ProjectSemanticDispatch::execute → SemanticGraphStore`
remains the sole query-time resolver (`project_semantic_dispatch/mod.rs:2206-2214`, the shared choke
point) — unchanged by the crate move. `ResolverContext`/observation-interface implementors are lifecycle
*adapters* over that one authority, never alternate authorities. A resolver-tier operation that cannot
prove a current, coherent view fails closed to a typed non-current miss (`StoreViewRead`'s
`CurrentHostStoreView`/`ColdSeedHostStoreView` split, `.claude/skills/host-session/SKILL.md:678-696`) —
C1 preserves this fail-closed order and must not weaken it while adding the `NeedInputs` outcome
alongside it.

**Acceptance IDs.**

| ID | Requirement | Planned test/gate |
|---|---|---|
| C1-AC-1 | Same query, same content, different lifecycle ⇒ same resolved answer | New characterization suite driving one fixed corpus of queries through `HostResolverContext` and `SessionResolverContext` (and the new I/O-free observation interface once added), asserting structural equality of `SemanticNodeId` surfaces, sibling to `resolver_core/*_tests.rs` (relocated) |
| C1-AC-2 | `verter_semantic`'s production closure excludes `verter_workspace`/`verter_session`/`verter_scheduler`/`verter_tsgo_api` on every target, with the A5-DD1 exception row **deleted**, not widened | `crates/verter_identity/tests/cases/workspace_dependency_layers.rs::workspace_production_closures_never_cross_upward_except_the_recorded_exception` — remove `"verter_semantic"` from `ratified_upward_exceptions()`; any surviving upward edge now hard-fails instead of matching an exception |
| C1-AC-3 | Authority-uniqueness contract holds after the crate move | Existing `project_semantic_dispatch_invariants_tests.rs` + the five-row Authority-uniqueness contract stay green, unmodified in substance, relocated alongside the module they test |
| C1-AC-4 | A resolver-tier call site cannot reach resolution without a request-bound context | `RequestBoundResolverContext` sealed marker (relocated to `verter_semantic`) becomes the *only* production-constructible path once the bare `impl ResolverContext for VerterHost` production rail is deleted (see Legacy Deletions) — `VerterHost` stays in `verter_session` and implements the now-foreign trait for a local type, which the orphan rules permit |
| C1-AC-5 | `AttemptOutcome::{Complete, NeedInputs(LoadSet), Terminal}` covers **every** non-flow `ModuleResolverCore`/`TypeInfoCore` operation reachable from a C2 projection attempt — full coverage, not one load point | Discharged structurally, not by sampling. Per C1-AC-7 and the Authority/fallback order clause, the observation interface is the *only* surface an I/O-free (C2-shaped) caller can reach a non-flow `ModuleResolverCore`/`TypeInfoCore` operation through — `ResolverContext` itself can call `ensure_loaded`/`wait_or_drive`, so it is not usable I/O-free. "Every reachable operation" therefore reduces to "every method on this one finite, closed trait." The trait is defined so every method returns `AttemptOutcome<T>` at the type level (never a bare `T`, `Result<T, _>`, or a call that can block) — a non-conforming method is a compile error at authoring time, not a gap a test could miss. Gate: one exhaustive test double (`impl <ObservationInterface> for TestDouble`) that must implement every trait method to compile; a method added later that does not return `AttemptOutcome<T>` fails to compile at the double, not at a sampled runtime assertion. If a future operation cannot be expressed this way, that is itself a Fork-4-reopening discovery (see Abort/rescope), not a reason to fall back to sampling. |
| C1-AC-6 | Duplicated lifecycle-adapter boilerplate collapses | Diff proof: the ~10 near-identical delegation methods listed under Legacy Deletions shrink to one shared implementation; `HostResolverContext`/`SessionResolverContext` (staying in `verter_session`, implementing the relocated trait) retain only the ~9 genuinely session-specific overrides |
| C1-AC-7 | The observation interface does not extend `ResolverContext` and cannot be built holding a host/scheduler reference | Type-level: the interface is defined in `verter_semantic` with no `VerterHost`/scheduler type nameable in scope (proven by C1-AC-2's closure guard, not a separate scanner); a `trybuild`/compile-fail fixture asserting a `&VerterHost`-holding type does not satisfy the interface's bound |
| C1-AC-8 | Route extraction takes an owned immutable snapshot, not a live `&dyn WorkspaceRead` | `analysis/routes.rs`'s extractors (`detect_routing_framework_from_json`, `extract_programmatic_routes`, and the four other call sites) take `&RouteAnalysisInputs`; zero `WorkspaceRead`-typed parameters remain anywhere in `verter_semantic`; existing route-extraction unit tests re-target the new snapshot type with unchanged assertions |
| C1-AC-9 | `ModuleResolverCore` (the relocated `ProjectResolver`) has no direct scheduler/tsgo I/O left uncoverted | Any synchronous I/O call inside the relocated 2122-line resolver either does not exist in the moved code (pure computation) or is converted to the same `AttemptOutcome`/`LoadSet` pattern as `resolver_core`'s load points — audited as part of C1-AC-5's coverage sweep, not a separate carve-out |

**Cold/warm/allocation/fan-out/latency bounds.** C1 is bound to the existing locked cell
`A6_META_COMPILE_40_COLD_RUST` (`performance-gates.toml:125`), not a new cell: that cell already measures
`session.semantic_dispatch.calls` (`performance-gates.toml:321`) and `session.semantic_cold_build.calls`
(`performance-gates.toml:296`) — the two counters convergence work directly touches, since every
`ResolverContext` call funnels to `dispatch()` (`host_resolver_context.rs:494-500`,
`session_resolver_context.rs:674-680`) and every cold miss reaches the chokepoint at
`project_semantic_dispatch/mod.rs:2214`. C1 must not move either counter adversely — the crate move must
not itself introduce cross-crate call overhead beyond ordinary function-call cost (no new serialization,
no new heap round-trip at the crate seam). Existing fan-out fuses (`resolver_core/fuses.rs:10-34`:
`wildcard_route_fanout=500`, `imported_root_fanout=200`, `registry_deepening_fanout=300`,
`member_surface_recursion_depth=10`, `projection_op_count=2000`, `union_member_explosion=100`) are budget
*policy*, not owned by C1 — convergence must reuse them unchanged and must not introduce a second fuse
table. Warm-hit validation stays the existing O(signature-length) `validate_fact_signature` loop
(`resolver_core/mod.rs:409-428`) with zero new heap allocation per fact — a converged `ResolverContext`
construction path must not add a clone, extra `Arc` construction, or normalization pass beyond what
`HostResolverContext::new`/`SessionResolverContext::new` already do per request
(`host_resolver_context.rs:96-149`, `session_resolver_context.rs:103-150`). The full-coverage
`AttemptOutcome` threading (C1-AC-5) must not add allocation to the existing blocking lifecycles' hot
path — a blocking `ResolverContext` call still resolves in one pass with no extra `LoadSet`
materialization when nothing is missing. C1 may add a dedicated micro-cell only via the ADR-016
new-lock-record path (mirroring `B1.md:169-171`); it may not reweight or reinterpret
`A6_META_COMPILE_40_COLD_RUST` after measurement.

## Current-tree convergence map

| current authority | source | disposition | final owner |
|---|---|---|---|
| `resolver_core` / `ProjectSemanticDispatch` orchestration | `resolver_core/**` (59 files, incl. `component_meta/` (6) + `component_meta_query_engine/` (14), `verter_session`), `project_semantic_dispatch/mod.rs:309` (`verter_session`, `pub(crate)`) | Preserve resolver semantics; **physically relocate** into `verter_semantic` — **except** the three files named in the "Sealed lifecycle adapters" row below, which hold `&VerterHost` and cannot cross | `verter_semantic` (except the named adapter carve-outs) |
| TypeScript-style module/path/package resolution | `crates/verter_workspace/src/resolver.rs:122` (`ProjectResolver`, 2122 lines) | **Physically relocate** — this is the `ModuleResolverCore` target, wrongly homed in the scheduler/tsgo-dependent `verter_workspace` crate | `verter_semantic` |
| Re-export shim + two real functions | `crates/verter_semantic/src/analysis/project_resolver.rs` (94 lines, not a pure shim: `:1-30` re-exports `verter_workspace::resolver::*`/`verter_workspace::types::*`; `:32-90` defines `collect_resolvable_module_reference_specifiers`/`resolve_known_module_reference_dependencies`, real analysis-dependent logic with production callers at `verter_napi/src/lib.rs:2095,2117` and `verter_wasm/src/lib.rs:640,667`) | Delete only the `:1-30` re-export half — its target no longer exists at that path once `ModuleResolverCore` relocates into `verter_semantic` alongside it. The `:32-90` functions stay at this same module path unchanged; their napi/wasm callers keep calling `verter_semantic::analysis::project_resolver::*` with no repointing | re-export half deleted; functions retained in place |
| Sealed lifecycle adapters | `resolver_core/resolver_context.rs:161` (trait), `:817-1343` (`impl ResolverContext for VerterHost`, plus `VerterHost`-specific `sealed::Sealed`/`RequestBoundSealed` impls), `resolver_core/host_resolver_context.rs:189`, `resolver_core/session_resolver_context.rs:183` | Trait + `sealed` module + collapsed shared boilerplate relocate with the kernel — `resolver_context.rs` is **split**, not moved whole. The two concrete adapter structs (`HostResolverContext`, `SessionResolverContext`) and the bare-host production rail (the `:817-1343` impl block) stay/are-deleted in `verter_session` (they hold `&VerterHost`, which cannot cross into `verter_semantic`) | trait + `sealed` module: `verter_semantic`; adapters + `VerterHost` impl: `verter_session` |
| Immutable observation view | `crates/verter_session/src/resolver_store.rs:1462-1525` (`HostStoreView`), `:427-558` (`StoreViewValidationToken`) | Relocate the immutable, `Arc`-backed value types; leave the host-lifecycle-bound `StoreViewManager`/cache-retention machinery in `verter_session` | value types: `verter_semantic`; manager: `verter_session` |
| Blocking cross-file load-on-demand | `host_lifecycle.rs:953` (`ensure_loaded`), `:1012` (`wait_or_drive`), `resolver_context.rs:913-914`, `host_resolver_context.rs:281-288`, `bare_name_resolve.rs:188-190` | Stays in `verter_session` (it needs `VerterHost`/the scheduler); the kernel-side call sites it's invoked from gain the full-coverage `AttemptOutcome` alternative instead | `verter_session` (blocking path) + `verter_semantic` (typed outcome) |
| `verter_semantic → verter_workspace` edge (A5-DD1) | `crates/verter_semantic/Cargo.toml:27`, consumers at `analysis/project_resolver.rs`, `analysis/routes.rs:196,251,661,672,869,1120` (`&dyn WorkspaceRead`), `facts/registry.rs:3` | **Delete the Cargo.toml edge outright** — the module resolver that motivated it relocates into `verter_semantic` itself; `WorkspaceRead` stays up (Fork 2), fact vocabulary moves down | edge deleted |

**Duplicated lifecycle-adapter boilerplate to collapse** (research-verified, not hypothesis, unaffected
by the crate move other than the trait's new home): `is_request_bound` (`host_resolver_context.rs:193-195`
vs `session_resolver_context.rs:187-189`), `request_completion_overlay` (`:221-224` vs `:234-237`),
`store_view` (`:321-324` vs `:508-511`), `aggregate_basis_seed` (`:326-329` vs `:513-516`), `dispatch`
(`:494-500` vs `:674-680`), `resolve_imported_type_root`/`_with_facts` (`:343-372` vs `:530-559`),
`resolve_type_declaration_for_dep` (`:431-450` vs `:616-634`), plus the constructor trio's near-verbatim
doc/rationale duplication (`:96-149` vs `:103-150`). Each pair is the same delegation shape over a
different receiver (`self.inner` vs `ResolverContext::method(self.inner, ..)`) — a single shared default
or a common inner-delegate helper removes the duplication without touching the genuinely session-specific
overrides (`authoritative_current_content_hash`, `observe_materialize_scope`, `indexed_for_current_content`,
`artifact_key_for_current_content`, `resolve_type_dependency_canonical`, `shallow_file_state`,
`active_session_view`, `complete_canonical`/`complete_canonical_with_session_view` — these have no
host-side analog and stay distinct; `session_resolver_context.rs:304-332,357-403,421-470,591-608,280-288,
712-715,171-180`). These structs stay in `verter_session` after the crate move (see convergence map
above); only the trait they implement relocates.

## Batched `NeedInputs` contract — full coverage (Fork 4)

C1 owns the **kernel-level** attempt outcome: a resolver-core operation run under an I/O-free environment
returns `AttemptOutcome::{Complete(T), NeedInputs(LoadSet), Terminal(AttemptFailure)}` per
`contracts/input-loading.md` §2, §4, instead of calling `ensure_loaded`/`ensure_indexed_ready_serve`
synchronously. `LoadSet` is normalized/sorted/deduplicated; `NeedInputs` on an empty delta with no basis
change is the typed `InputResolutionNoProgress` failure (§4.3-4.5 of that contract), never a silent retry
loop. This capability does not exist anywhere in the tree today (`grep -rn "NeedInputs\|LoadSet" crates/`
— zero hits) and is new work, not a refactor of an existing batching mechanism.

**Coverage is full, per the Fork 4 ruling — not "module/import resolution only."** Every non-flow
`ModuleResolverCore`/`TypeInfoCore` operation reachable from a C2 projection attempt must support the
outcome: module/import resolution, decl-body lowering, relation/inference queries driven through
`execute_relate`'s plumbing (the algorithm itself stays `Preserve`; only the calling convention around it
changes), and the member/JSDoc-hydration path that already reaches `ensure_indexed_ready_serve`
(`crates/verter_session/src/typeinfo/shallow_surface.rs:179-193`) today. `contracts/input-loading.md:5-19`
is unqualified across compiler/resolver/TypeInfo kernels, and ADR-011:19-28 requires each attempt to
report the maximal sound missing-observation set — a partial cut would leave C2 unable to plan its
complete semantic demand closure before projection (`contracts/compile-transaction.md:18,44-53`) for
whichever operations were left out. Internally, `ensure_loaded`/`ensure_indexed_ready_serve` may still
become the two central observation-or-demand choke points every covered operation funnels through — that
is an implementation strategy, not a scope carve-out. Acceptance is never "one real load point exercised
by a test harness"; it is every reachable operation covered.

**C1 does not own** the outer staged transaction. `contracts/input-loading.md` §5: "A direct/project
`CompileTypeInfo` over an immutable caller environment does not own commits or I/O. It returns
`NeedInputs`; the caller may rebuild/extend the environment and retry." That caller — the
prepare/plan/project/emit loop, `CompilePlanToken`/`ProjectionPlanToken` anti-replay, and the
load/commit/retry orchestration across *multiple* kernel attempts — is `C2` (`program.md:242-247`, ADR-011).
C1's obligation ends at: every resolver-tier operation reachable from a projection attempt, one immutable
snapshot, one typed outcome per attempt. C2's obligation starts at: drive repeated C1 attempts, own the
snapshot-extend/retry loop, own the anti-replay tokens.

The existing blocking behavior is **not deleted**: the host-backed LSP session and the session-overlay
lifecycle are permitted, documented lifecycles that legitimately block a cooperating thread
(`decl_body_memo.rs:21`, `store_view_manager_tests.rs:2399` — "in-flight work block cooperatively, never
busy-spin"). C1 adds the alternative outcome, across the full operation surface, so a *new* I/O-free
lifecycle can reuse the same resolution logic without being forced to block; it does not retrofit every
existing call site to stop blocking — `HostResolverContext`/`SessionResolverContext` keep blocking by
design.

## Legacy deletions

- **`resolver_core/**` minus three named carve-outs, `resolver_store`'s immutable value types, and
  `project_semantic_dispatch`** physically move out of `crates/verter_session/src/` into
  `crates/verter_semantic/src/`. This includes `component_meta/` and `component_meta_query_engine/` in
  full (verified dependency-neutral — see Context §1). The carve-outs, which stay in `verter_session`
  because they hold `&VerterHost` (per the "Sealed lifecycle adapters" convergence-map row): 
  `resolver_core/host_resolver_context.rs`, `resolver_core/session_resolver_context.rs`, and the `impl
  ResolverContext for VerterHost` block plus its `VerterHost`-specific `sealed::Sealed`/`RequestBoundSealed`
  impls inside `resolver_core/resolver_context.rs` (`:817-1343` — deleted rather than kept if the bare-host
  rail itself is deleted, see the bare-host bullet below). `resolver_context.rs`'s trait definition and
  `sealed` module (everything outside `:817-1343`) relocate with the kernel — the file is **split across
  two crates**, not moved whole. The `verter_session::lib.rs` module declarations for the relocating pieces
  (`mod project_semantic_dispatch` at line 332, `pub mod resolver_core`/`mod resolver_store` at lines
  341-344) are deleted and replaced with narrower declarations covering only the three staying carve-out
  files; `verter_semantic::lib.rs` gains the equivalent declarations for everything else. This is a
  relocation, not a rewrite, for the moved code — its behavior is unchanged; its crate is not.
- **`crates/verter_workspace/src/resolver.rs`** (`ProjectResolver`, 2122 lines) relocates into
  `verter_semantic` as the `ModuleResolverCore` target. `verter_workspace`'s module declaration for it is
  deleted; any I/O the resolver performs directly is converted to the `AttemptOutcome`/`LoadSet` pattern
  as part of C1-AC-9, not left as a silent exception to full coverage.
- **The bare `impl ResolverContext for VerterHost`** (`resolver_context.rs:817`) production-reachable
  method bodies — confirmed to be `panic!("Architectural violation...")` in production builds today
  (`resolver_context.rs:826-841,853-873,882-902,950-996,1021-1039,1054-1072,1081-1098,1107-1124,1171-1188`),
  live only under `#[cfg(any(test, feature = "test-support"))]`. `VerterHost` stays defined in
  `verter_session`; `ResolverContext` becomes a foreign trait from `verter_session`'s perspective, which
  Rust's orphan rules permit implementing for a local type. If no production call site needs the
  bare-host rail once convergence lands (verify at implementation time — every currently-known production
  entry already binds `HostResolverContext`/`SessionResolverContext`), delete the impl entirely and let
  `RequestBoundResolverContext` become the sole production-constructible rail. This turns "resolve
  without a request-bound context" from a runtime panic into a compile error.
- **`crates/verter_semantic/src/analysis/project_resolver.rs`** (94 lines, not a pure shim) — only the
  `:1-30` re-export half (`pub use verter_workspace::resolver::{...}` / `verter_workspace::types::{...}`)
  is deleted; its target no longer exists at that path once `ModuleResolverCore` relocates *into*
  `verter_semantic` alongside it. The `:32-90` functions (`collect_resolvable_module_reference_specifiers`,
  `resolve_known_module_reference_dependencies`) are real analysis-dependent logic, not shim, and stay at
  this module path unchanged — their production callers (`verter_napi/src/lib.rs:2095,2117`,
  `verter_wasm/src/lib.rs:640,667`) keep calling `verter_semantic::analysis::project_resolver::*` with no
  repointing needed, since the module is not deleted, only its now-redundant re-export half.
- **`crates/verter_semantic/src/facts/registry.rs:3`** (`pub use verter_workspace::fact_registry::*;`) —
  the fact-key vocabulary (`FactKey`, `FactDomain`, `Fact`, etc., currently
  `verter_workspace/src/fact_registry.rs`) is dependency-neutral value data, exactly the kind of type the
  ruling's boundary assigns to `verter_semantic` ("dependency-neutral semantic store/value types"). It
  moves into `verter_semantic` directly; the re-export is deleted, not left as a permanent alias.
- **`crates/verter_semantic/src/analysis/routes.rs:196,251,661,672,869,1120`** (`workspace: &dyn
  verter_workspace::WorkspaceRead`) — per Fork 2's ruling, `WorkspaceRead` does **not** relocate downward
  (it also exposes live authoritative import resolution and dependency-graph authority, `traits.rs:214-
  280,347-508`, which must stay a live capability, not a captured observation). Instead: the six call
  sites are rewritten to take an owned immutable `RouteAnalysisInputs` snapshot; the orchestration that
  currently calls `read_file`/`file_exists`/`is_dir`/`read_dir` to answer route-extraction questions moves
  *upward* into `verter_workspace`/`verter_session`, which builds the snapshot and passes it down. The
  existing pure extractors (`detect_routing_framework_from_json`, `extract_programmatic_routes`,
  `routes.rs:208-233,266-289`) are unchanged in substance — only their input type changes.
- **`crates/verter_identity/tests/cases/workspace_dependency_layers.rs:118-127`** —
  `ratified_upward_exceptions()`'s `"verter_semantic"` row is deleted (not widened, not target-scoped
  differently). The `"verter_diagnostics"` row is untouched — out of scope. This is the mechanical proof
  that A5-DD1 closes for real: after this row is gone, *any* remaining `verter_semantic → verter_workspace`
  edge fails the existing closure test, with no new guard authored.
- **Any singleflight/condvar/mutex blocking-wait code path** that a converged, full-coverage
  `AttemptOutcome::NeedInputs` caller would otherwise still be forced through — audit
  `SingleflightGroup::run`/`run_retaining` (`resolver_core/mod.rs:2116-2214,2595-2639`),
  `route_db_singleflight.rs:70-146`, and `prepared_decl.rs:35-38`'s `build_gate: parking_lot::Mutex<()>`.
  Because coverage is now full (Fork 4), this audit is broader than the prior draft's narrower scope: every
  blocking primitive reachable from a covered operation either stays confined to the blocking lifecycles
  (`HostResolverContext`/`SessionResolverContext`, which still legitimately block) or gains a non-blocking
  peek-and-decline path feeding `NeedInputs` instead of parking the calling thread.

## Structural confinement

Every invariant above is enforced structurally, matching the pattern the codebase already uses in this
exact area — C1 extends that pattern, it does not invent scanner-based enforcement:

- **The crate dependency firewall is the primary proof, not a marker.** Per the Fork 1/3 ruling, "prove
  extractability without extracting" is not mechanically checkable — the existing structural graph guard
  cannot see intra-crate direction (`docs/arch/refactor/rev11/evidence/A5/dependency-direction.md:
  182-189`). Once the kernel physically lives in `verter_semantic`, the **existing, landed**
  Cargo-metadata closure guard (`crates/verter_identity/tests/cases/workspace_dependency_layers.rs`) is
  sufficient and requires no new guard: `verter_semantic` simply cannot name `VerterHost`, the scheduler,
  or `verter_tsgo_api` types, because its production dependency closure does not reach them. That is what
  makes the new observation interface's non-blocking, host-free guarantee real — not a marker trait
  layered on top of a context that could still physically hold a host reference.
- **Sealed-trait lifecycle closure.** `ResolverContext: sealed::Sealed` (relocated with the trait) already
  makes "some fourth, unregistered lifecycle adapter" a compile error. C1 preserves the seal and, if the
  bare-host rail is deleted (see above), tightens `RequestBoundResolverContext` from "narrower than
  `ResolverContext`" to "identical to it" — a resolver-tier function taking `&dyn ResolverContext` becomes
  provably request-bound by construction, not by convention. The new observation interface is a *separate*
  sealed trait, not a subtrait of `ResolverContext` — per the ruling, a subtrait inherits
  `ensure_loaded`/the host escape hatch and proves nothing.
- **`StoreView`/`HostStoreView` immutability is already structural**, not a convention: `HostStoreView`
  has no `&VerterHost`/`Arc<Mutex<..>>` field, only `Arc`-shared by-value snapshot data; a future violation
  (a live host reference smuggled into the struct) is a visible field addition a reviewer catches at the
  type definition, not a runtime behavior to detect. This is one of the value types that relocates into
  `verter_semantic` unchanged.
- **The Authority-uniqueness contract** (`.claude/skills/type-resolution/SKILL.md:790-796`) is enforced
  today partly by a `syn`-AST content scanner (`crates/verter_session/tests/cases/architecture_guards.rs:
  3384` `mod resolver_context_seal`) and partly by grandfathered string-content assertions. Per `CLAUDE.md`'s
  forward-only landed-scanner rule, these are **grandfathered, not a template**. Because the scanned file
  physically moves to `verter_semantic`, this grandfathered scanner's target path must be updated to keep
  scanning the relocated file — that is "extending the existing grandfathered scanner's checked
  assertions" (permitted) tracking a genuine regression risk from the move, not authoring a new
  standalone scanner (forbidden). C1 must not add a new name-keyed source scanner for any new invariant it
  introduces (the deleted bare-host rail, the collapsed duplication, the new `AttemptOutcome`, the new
  observation interface) — new confinement here is type-level and crate-boundary-level only.

## Boundary against flow (D-track) and against C2

**Flow exclusion.** C1 converges the relation/inference authority (`execute_relate`,
`project_semantic_dispatch/relation.rs`) exactly as it stands today — `Preserve`, not `Converge`. It adds
no flow-specific relation classifier, no binding-slot integration, no nominal-identity/unique-symbol
comparability extension. Those are `D3`'s stated job: "Extend the already-shared relation authority with
nominal identities including unique symbols and tri-state comparability" (`program.md:249-253`). C1's only
obligation to the flow track is that the relation authority it converges (now physically in
`verter_semantic`) remains the single authority `D1` integrates against behind its private non-production
test boundary (`program.md:229`) and that `D3` later extends without needing a second relation/name
authority (`program.md:253`). The full-coverage `AttemptOutcome` threading touches the *calling
convention* around `execute_relate` (per Fork 4) but never the relation algorithm itself.

**C2 boundary.** C2 owns the staged compile transaction: `prepare → plan → project → emit`,
`CompilePlanToken`/`ProjectionPlanToken` anti-replay, `CompileFactsBatch`, the concrete sealed
`CompileTypeInfo`, and the outer load/commit/retry loop over C1's `NeedInputs` outcome (ADR-011,
`program.md:242-247`). C1 must not build any of that — no plan token, no fact batch, no staged
prepare/plan/project/emit state machine. C1's deliverable to C2 is now concretely: a converged kernel
physically resident in `verter_semantic` that (a) gives identical answers across lifecycles, (b) can be
driven from an I/O-free observation interface and returns `NeedInputs` instead of blocking across its
**full** operation surface, and (c) is constructible without pulling in `verter_scheduler`/
`verter_tsgo_api`/LSP-facing machinery *by construction of the crate graph*, so C2's compiler-facing
facade embeds `verter_semantic` the same way `verter_compiler` already does today
(`crates/verter_compiler/Cargo.toml:59`) with no cycle risk.

## Required exit

Per `program.md:196`: **lifecycle differs; module/name/type/relation meaning does not.** Concretely: the
converged kernel (`ModuleResolverCore`, non-flow `TypeInfoCore`, `ProjectSemanticDispatch`, relation/node
algorithms, dependency-neutral value types, `AttemptOutcome`/`LoadSet`) lives in `verter_semantic`; the
three (or more, once the I/O-free observation interface exists) lifecycle implementors give bit-identical
resolved answers for identical inputs; the duplicated boilerplate between `HostResolverContext` and
`SessionResolverContext` is gone; `verter_semantic`'s production closure excludes `verter_workspace`,
`verter_session`, `verter_scheduler`, and `verter_tsgo_api` on every target with the A5-DD1 exception row
deleted, not recorded as still-permitted; `AttemptOutcome::NeedInputs(LoadSet)` exists and covers every
non-flow `ModuleResolverCore`/`TypeInfoCore` operation reachable from a C2 projection attempt, exercised
by an I/O-free test harness that never touches the scheduler; the Authority-uniqueness contract's five
rows still hold; no `TypeExpr`-general transit, flow semantics, or C2-owned staged-transaction machinery
was introduced.

## Review

Foundational: three mandates, three independent contexts, one candidate SHA and tree
(`governance.md` §1, mirroring `B1.md:173-186`).

| mandate | scope |
|---|---|
| conformance | charter, diff, and the deletion set — including whether every listed relocation actually happened (not a re-export left behind), whether the bare-host `ResolverContext` rail was actually deleted (or, if retained, why a production call site still needs it), and whether A5-DD1 closed by exception-row deletion rather than a subset-checked allowance |
| architecture | diff plus the crate-boundary outcome — specifically whether `verter_semantic`'s new contents satisfy ADR-015's inward dependency direction with zero exception, whether the observation interface genuinely cannot reach `VerterHost`/the scheduler (not just "does not currently"), and whether any duplication-collapse accidentally created a new second authority instead of removing one |
| adversarial performance/memory | diff plus `A6_META_COMPILE_40_COLD_RUST` raw output — specifically whether `session.semantic_dispatch.calls`/`session.semantic_cold_build.calls` regressed, whether the crate-seam move added call/serialization overhead, and whether the full-coverage `AttemptOutcome` path adds allocation on the existing blocking lifecycles' hot path |

## Abort/rescope

Stop for: a discovered fourth production lifecycle this charter did not account for; evidence that
`verter_workspace::resolver::ProjectResolver` is not, in fact, cleanly separable from `verter_workspace`'s
scheduler-integrated file-loading responsibilities in a way full-coverage `AttemptOutcome` conversion
cannot resolve (i.e. A5-DD1 cannot close without also restructuring `F1`'s future committed-input-authority
scope); a discovered second query-time resolution path this research did not find; or a locked-cell
regression on `A6_META_COMPILE_40_COLD_RUST` that convergence cannot explain and correct within scope. A
discovery at this bar reopens the ruling itself (a second architecture challenge), not a quiet local
substitution for one of the four already-decided positions.

## Rulings applied

Binding ruling: `ARCH-RULING-C1-FOUR-FORKS.md` (Codex xhigh architecture challenge against the prior
charter draft, tip `339c06989`). All four proposed positions in that draft were rejected:

1. **Fork 1 (crate placement) — VIOLATES.** "Prove extractability without extracting" is not mechanically
   enforceable (the structural graph guard cannot see intra-crate direction); a `pub(crate)` facade is
   unusable by C2, and a public one would need `verter_compiler → verter_session` while session already
   depends on compiler — a cycle. Ruling: **EXTRACT-NOW into the existing `verter_semantic` crate**, not a
   new crate.
2. **Fork 2 (`WorkspaceRead`'s home) — VIOLATES.** Relocating the whole trait downward would drag live
   authoritative import resolution and dependency-graph authority into the kernel — resolution semantics,
   not a captured observation. Ruling: move the workspace-capture entry points **upward**; pass an owned
   immutable `RouteAnalysisInputs` snapshot **downward** into the existing pure extractors. This does not
   foreclose `F1`'s future committed-input-authority design; relocating the trait would have.
3. **Fork 3 (non-blocking guarantee) — VIOLATES.** The proposed `IoFreeResolverContext` marker, as a
   subtrait of `ResolverContext`, inherits `ensure_loaded`/the host escape hatch — the draft's claim that
   such a bound "does not expose" it was factually wrong. Ruling: extraction and I/O confinement are **one
   decision**. Define a capability-limited observation interface that does not extend `ResolverContext`
   and cannot return a host; the crate firewall (from ruling 1) is what makes the host unnameable, not the
   marker by itself.
4. **Fork 4 (coverage scope) — VIOLATES.** "Module/import resolution only" was never a clean cut — TypeInfo
   projection already reaches `ensure_indexed_ready_serve`. `contracts/input-loading.md` is unqualified,
   and ADR-011 requires the maximal sound missing-observation set per attempt. Ruling:
   **FULL-COVERAGE-REQUIRED** for every non-flow `ModuleResolverCore`/`TypeInfoCore` operation reachable
   from a C2 projection attempt; `ensure_loaded`/`ensure_indexed_ready_serve` may become internal choke
   points, but acceptance is never "one real load point."

**New open questions created by these rulings: none.** All four forks resolved to a specific, actionable
position with a named target (crate, boundary lines, interface shape, coverage scope); nothing here
requires a further architecture challenge before implementation. Two items are implementation-time
judgment calls *within* the ruling's already-decided boundary, not further forks: (a) the exact intra-crate
module layout for the relocated `resolver_core`/`resolver_store`/`project_semantic_dispatch` trees inside
`verter_semantic`, and (b) the precise split of `resolver_store` between the immutable value types that
move down and the `StoreViewManager`/cache-retention machinery that stays in `verter_session` — both are
already fully constrained by the ruling's boundary list (verter_semantic gets "immutable observation
contracts"; verter_session keeps "committed-store implementations... cache-retention policy") and do not
need a ruling to resolve.

See `/testing` skill for full TS/Rust test patterns, sourcemap testing, and server cleanup.

### VS Code Extension Testing (MANDATORY)

Changes to the VS Code extension or the LSP server MUST be verified with automated tests, NOT manual testing. Unit tests (Vitest) for pure logic, E2E tests (Mocha) for LSP integration features.

See `/testing` and `/e2e-vscode-testing` skills for commands, fixture design, and helpers API.

## Agent Implementation Rules

### Codebase Navigation

Use semantic code-navigation tools (Serena or equivalent MCP: symbol overviews, symbol/reference lookup, rename/refactor ops) before broad source reads. Read full source files only when symbolic context is insufficient or the file is small enough that a full read is clearly the most direct path.

### Planning

Prefer architecturally correct, long-term solutions; evaluate by correctness and durability, not implementation speed. Time constraints, implementation size, migration breadth, anticipated breaking changes, or "a lot of work" are not valid reasons to weaken the design, preserve a compromised path, or diverge from the approved plan — if the correct implementation is larger or breaking, plan for it explicitly or raise it before execution; never silently ship an architectural deviation. Do not provide time estimates unless explicitly asked, and never use estimated effort/duration/perceived time cost as a factor for doing, not doing, or partially doing planned work.

Plans must include these sections:
1. **Context** — why this change is being made
2. **Intent Contract** — the ratified statement of intent, before any mechanism design
3. **Changes** — specific files to modify with concrete modifications
4. **Legacy Deletions** — explicit list of files, functions, code paths, feature flags to remove
5. **Verification** — full workspace test commands and expected outcomes

Without explicit legacy deletion lists, agents skip deletions and leave dual paths alive.

**Intent before mechanism.** Before mechanism design for a block that changes observable behavior, authority, or fallback, record a ratified intent contract: the actor/problem and why the capability should exist; required and forbidden observable outcomes; authority/fallback order; a planned test or gate for each stable acceptance ID; and material cold, warm, allocation, fan-out, and latency bounds. An internal substrate block may reference its parent contract but must state the invariant and performance contribution it owns. Ratification comes from the approved plan or product authority; no implementation brief is dispatched without it. Enforcement is judgment — exercised at decomposition and again immediately before implementation dispatch.

### Execution

Execute approved plans fully in one pass, end-to-end, without intermediate checkpoints or mid-plan confirmation on already-approved steps. Do not pause, defer scope, leave planned work unfinished, or rewrite the plan into a smaller/safer variant because the correct path is breaking, broad, or labor-intensive. Approved plans land as written unless the user explicitly re-scopes them.

**One-pass execution applies only while the approved design remains valid.** The second-REOPEN circuit breaker lapses approval for the affected design: pause implementation, obtain and record the required architecture/product ruling, and resume only once the design is ratified again. This is not a checkpoint — one-pass governs *executing an approved design*, and the breaker fires when *approval itself has lapsed*, which is a different event and precisely why execution must stop rather than grind on. STOP, failed verification, rule conflict, and verified plan-invalidating discoveries pause at their prescribed evidence gate without creating a discretionary user checkpoint. Breadth, breakage, effort, or migration size never lapses approval; approved scope changes only through the recorded ruling or explicit user re-scope. See `/mom-cto-orchestration` → Decision Admission.

### Orchestrating Large Plans

For a large multi-block plan, refactor, migration, or staged cutover executed autonomously, drive it via the `/multi-agent-orchestration` skill rather than improvising: a pure orchestrator delegates blocks to implementer/reviewer/fix sub-agents, gates each on dual review (independent reviewer + `codex`), runs fix cycles until clean, and verifies sub-agent reports against git state (trust but verify).

When a block runs in a dedicated `git worktree`, run `pnpm install --frozen-lockfile` in the worktree root once at creation time, before any JS/TS test or workspace-importing Node script — fresh worktrees do not get the gitignored `node_modules/`, and a missing install makes JS/TS tests fail spuriously and read as a false regression. See the skill's "Worktree hygiene & environmental discipline" section.

### Self-Review

After completing a plan, review the full implementation before declaring done:
- Verify all plan steps were executed
- Check for missed edge cases or incomplete migrations
- Run the full workspace test suite (see End-of-change Checks above)

### Legacy Code Deletion

When replacing a feature or refactoring a system, delete the superseded code in the same change. Do not add shims, double branches, compatibility wrappers, or feature flags to preserve old behavior alongside new. If unsure whether specific files or code paths should be preserved, ask the user explicitly rather than silently keeping them.

### Fix Quality

When encountering issues during implementation:
- If the correct fix aligns with the architecture → implement it properly
- Never apply a dirty fix that contradicts architectural rules just to make tests pass
- If the proper fix is outside approved scope, do not apply a workaround and do not use a `TODO` as its disposition. Route the finding through the applicable scope authority and record `ADOPT-NOW`, `DEFER`, or `REJECT` before related work continues. A `TODO` may reference an approved debt row but never replaces it.

**Explicit finding disposition.** Every scope-deviating correctness finding is dispositioned before related work continues as `ADOPT-NOW`, `DEFER`, or `REJECT`. `ADOPT-NOW` records the scope and acceptance-contract change. `DEFER` requires a codex-DEFER ruling and a debt row naming the durable owner block, the resolution gate no later than plan close, the acceptance ID/test, and the ruling reference. `REJECT` records evidence and rationale. A TODO, a feedback entry, or an ephemeral agent identity is not a disposition; plan close requires zero open deferrals. Enforcement is judgment — codex at the scope consult, and the plan-close zero-open-deferral check.

### Stub Prevention (CRITICAL)

Do not use empty test bodies, trivially-passing stubs, or "deferred to follow-up commit" placeholders to satisfy a named contract — a gate check, a characterization test, a plan invariant, a review obligation, a declared completion criterion. A stub that happens to pass is a gate-bypass, not a pass.

Concrete anti-patterns, all forbidden on landed/mainline commits:

- **Empty `#[test]` bodies** — `#[test] fn verifies_cycle_guard_terminates_on_recursion() {}` passes trivially and falsely advertises coverage (worse than `#[ignore]`; keep `#[ignore]` until the body can be written).
- **Unconditional "unknown"/"default" returns as "scaffolding"** — `fn relate_nodes(...) -> RelationResult::Unknown` always-Unknown is a nop, not a scaffold; same for an always-`Opaque(Miss)` resolve. Write real logic, or use `todo!()` / `unimplemented!()` so the nop fails loudly.
- **"Real body deferred to follow-up commit"** — a stub satisfying a gate now with a later commit planned is a gate-bypass; the gate reflects the tree under review, not future intent.
# Verter

> **SUPERSEDED WHERE IT CONFLICTS — an architecture program is in flight.**
>
> The ratified Revision 11 architecture program lives at [`docs/arch/refactor/rev11/`](docs/arch/refactor/rev11/); its normative entry point is [`ORCHESTRATOR.md`](docs/arch/refactor/rev11/ORCHESTRATOR.md).
>
> **Precedence:** where this file and the Revision 11 plan disagree, **the plan wins**. The plan was designed to replace parts of this document, and the maintainer has ratified that precedence — see ruling R-3 in [`evidence/maintainer-rulings.md`](docs/arch/refactor/rev11/evidence/maintainer-rulings.md), which quotes both sides of the known conflicts.
>
> **How to read this file while the program runs:** treat it as an accurate description of how the code behaves **today**, and as authority for day-to-day operational matters — build and test commands, commit conventions, testing requirements, review discipline. Do **not** treat its architecture sections as authority over the program's target design; several describe structures the plan intends to change or remove. A rule here is not grounds to refuse a change the plan mandates.
>
> **If you are implementing a program block:** the plan's charters, contracts and ownership tables bind you. Follow them to the letter. Where implementing the plan appears impossible, record a deviation for maintainer ratification rather than substituting a local decision — an unrecorded deviation is far more expensive to unwind than a delay.
>
> This banner is removed when the program completes and the target architecture is described here directly.

Verter = a Vue compiler + Language Server Protocol (LSP) implementation. Converts Vue Single File Components (SFCs) to valid TSX (TypeScript type-checks them) and compiles templates to optimized render functions. Unlike Volar, Verter generates real valid TSX, not virtual files.

Hybrid Rust + TypeScript monorepo: Rust crates own carrier parsing, runtime and IDE code generation, the shared semantic session, and the LSP server (`verter_lsp` binary, stdio); TypeScript packages provide editor integration, TypeScript-provider adapters, protocol bindings, and bundler orchestration.

## Architecture

Detailed module reference, key files, and implementation specifics live in domain skills: `/type-resolution`, `/type-cache-architecture`, `/component-meta`, `/compiler-codegen`, `/host-session`, `/architecture`.

### Shared Optimized Codebase (CRITICAL)

Verter is one shared optimized codebase, not separate semantic implementations per consumer.

- Improvements land in the lowest reusable owner crate that can correctly serve all consumers.
- `verter_session` + shared workspace/VFS integration are the authority for host-backed loading, invalidation, dependency tracking, cache reuse.
- `verter_semantic` + `verter_compiler` own reusable semantics, lowering, codegen.
- `verter_session::resolver_core` owns the host-backed resolver stack + type-resolution orchestration.
- `verter_audit` is the leaf observability substrate (depends only on `verter_span`, no back-edge; lower crates emit through `current_observer()` (TLS) without knowing whether a `HostAuditRuntime` is installed); the concrete host runtime lives in `verter_session` — full ownership inventory in `/audit-infrastructure`.
- `verter_protocol` owns transport-facing schema DTOs; `verter_ffi` remains the thin native/WASM adapter layer.
- Consumer packages (`@verter/component-meta`, LSP, MCP, unplugin, playground) consume the shared substrate, not their own semantic forks.

Architectural consequence:

- A perf/correctness fix found in one surface is implemented in the shared owner layer whenever the behavior is reusable.
- Consumer-local wrappers stay thin and do not bypass shared parsing, analysis, resolution, or cache ownership.

**Exactly one type-resolution engine.** `SemanticQueryKey` → `ProjectSemanticDispatch::execute` → `SemanticGraphStore`, five query modes (`Identity`/`Navigate`/`Shallow`/`Expanded`/`Skeleton`) — the SOLE query-time type resolver. OXC is the syntax/lowering front-end ONLY (declaration bodies lower to `TypeExpr` lazily on first semantic demand through the scheduler-retained parse snapshot — the `DeclBodyMemo` owned by `IndexedReady`); OXC must never resolve types at query time. Any second query-time resolution path — a parallel `resolve_type` engine, a per-surface walker, a re-parse-and-resolve, an OXC element/frontier resolver — is a rule violation: delete it, route through the shared resolver. Two engines diverge; divergence is the bug/hang class.

**Audit infrastructure:** Rust-first deterministic per-request observability for every audited `VerterHost` entry-point (component-meta, type-resolution, compile, analyze, workspace ops, LSP handlers, MCP tools, bundler batches). TS bindings in `packages/types/audit.generated.ts`; opt-in via `HostConfig::audit_enabled + footprint_capture`. See `/audit-infrastructure` and [`docs/audit-footprint/`](docs/audit-footprint/).

Guards: `verter_audit_no_upward_deps`, `audit_substrate_isolation`, `audit_observer_single_accessor`. The former single-engine `no_new_*` shrinking-ledger scanners retired with the deleted parser-owned `type_surface` engine; the single-engine invariant is now held structurally (the compiler consumes only the dependency-neutral macro DTO) plus the Vue macro boundary suite registered under "Vue Macro Semantic Boundary" in `CRITICAL_RULE_GUARDS`.

### Build Philosophy (CRITICAL)

Same end-state philosophy as `binary-exploring-lamport.md`. Core rules:

1. Read, parse, shallow-process, cache each canonical file once per content hash through one shared host path.
2. Store the full shallow symbol inventory up front, then process only requested items on demand.
3. Same-file closure stays local to the owning file.
4. Cross-file deepening happens in one place only, one import level at a time.
5. The builder/solver reads only from cached lookup state; it does not reopen file loading or routing.
6. The design is demand-driven and query-scoped.

exec
/bin/zsh -lc "nl -ba docs/arch/refactor/rev11/charters/C1.md | sed -n '175,380p'" in <block worktree root>
 succeeded in 0ms:
   175	materialization when nothing is missing. C1 may add a dedicated micro-cell only via the ADR-016
   176	new-lock-record path (mirroring `B1.md:169-171`); it may not reweight or reinterpret
   177	`A6_META_COMPILE_40_COLD_RUST` after measurement.
   178	
   179	## Current-tree convergence map
   180	
   181	| current authority | source | disposition | final owner |
   182	|---|---|---|---|
   183	| `resolver_core` / `ProjectSemanticDispatch` orchestration | `resolver_core/**` (59 files, incl. `component_meta/` (6) + `component_meta_query_engine/` (14), `verter_session`), `project_semantic_dispatch/mod.rs:309` (`verter_session`, `pub(crate)`) | Preserve resolver semantics; **physically relocate** into `verter_semantic` — **except** the three files named in the "Sealed lifecycle adapters" row below, which hold `&VerterHost` and cannot cross | `verter_semantic` (except the named adapter carve-outs) |
   184	| TypeScript-style module/path/package resolution | `crates/verter_workspace/src/resolver.rs:122` (`ProjectResolver`, 2122 lines) | **Physically relocate** — this is the `ModuleResolverCore` target, wrongly homed in the scheduler/tsgo-dependent `verter_workspace` crate | `verter_semantic` |
   185	| Re-export shim + two real functions | `crates/verter_semantic/src/analysis/project_resolver.rs` (94 lines, not a pure shim: `:1-30` re-exports `verter_workspace::resolver::*`/`verter_workspace::types::*`; `:32-90` defines `collect_resolvable_module_reference_specifiers`/`resolve_known_module_reference_dependencies`, real analysis-dependent logic with production callers at `verter_napi/src/lib.rs:2095,2117` and `verter_wasm/src/lib.rs:640,667`) | Delete only the `:1-30` re-export half — its target no longer exists at that path once `ModuleResolverCore` relocates into `verter_semantic` alongside it. The `:32-90` functions stay at this same module path unchanged; their napi/wasm callers keep calling `verter_semantic::analysis::project_resolver::*` with no repointing | re-export half deleted; functions retained in place |
   186	| Sealed lifecycle adapters | `resolver_core/resolver_context.rs:161` (trait), `:817-1343` (`impl ResolverContext for VerterHost`, plus `VerterHost`-specific `sealed::Sealed`/`RequestBoundSealed` impls), `resolver_core/host_resolver_context.rs:189`, `resolver_core/session_resolver_context.rs:183` | Trait + `sealed` module + collapsed shared boilerplate relocate with the kernel — `resolver_context.rs` is **split**, not moved whole. The two concrete adapter structs (`HostResolverContext`, `SessionResolverContext`) and the bare-host production rail (the `:817-1343` impl block) stay/are-deleted in `verter_session` (they hold `&VerterHost`, which cannot cross into `verter_semantic`) | trait + `sealed` module: `verter_semantic`; adapters + `VerterHost` impl: `verter_session` |
   187	| Immutable observation view | `crates/verter_session/src/resolver_store.rs:1462-1525` (`HostStoreView`), `:427-558` (`StoreViewValidationToken`) | Relocate the immutable, `Arc`-backed value types; leave the host-lifecycle-bound `StoreViewManager`/cache-retention machinery in `verter_session` | value types: `verter_semantic`; manager: `verter_session` |
   188	| Blocking cross-file load-on-demand | `host_lifecycle.rs:953` (`ensure_loaded`), `:1012` (`wait_or_drive`), `resolver_context.rs:913-914`, `host_resolver_context.rs:281-288`, `bare_name_resolve.rs:188-190` | Stays in `verter_session` (it needs `VerterHost`/the scheduler); the kernel-side call sites it's invoked from gain the full-coverage `AttemptOutcome` alternative instead | `verter_session` (blocking path) + `verter_semantic` (typed outcome) |
   189	| `verter_semantic → verter_workspace` edge (A5-DD1) | `crates/verter_semantic/Cargo.toml:27`, consumers at `analysis/project_resolver.rs`, `analysis/routes.rs:196,251,661,672,869,1120` (`&dyn WorkspaceRead`), `facts/registry.rs:3` | **Delete the Cargo.toml edge outright** — the module resolver that motivated it relocates into `verter_semantic` itself; `WorkspaceRead` stays up (Fork 2), fact vocabulary moves down | edge deleted |
   190	
   191	**Duplicated lifecycle-adapter boilerplate to collapse** (research-verified, not hypothesis, unaffected
   192	by the crate move other than the trait's new home): `is_request_bound` (`host_resolver_context.rs:193-195`
   193	vs `session_resolver_context.rs:187-189`), `request_completion_overlay` (`:221-224` vs `:234-237`),
   194	`store_view` (`:321-324` vs `:508-511`), `aggregate_basis_seed` (`:326-329` vs `:513-516`), `dispatch`
   195	(`:494-500` vs `:674-680`), `resolve_imported_type_root`/`_with_facts` (`:343-372` vs `:530-559`),
   196	`resolve_type_declaration_for_dep` (`:431-450` vs `:616-634`), plus the constructor trio's near-verbatim
   197	doc/rationale duplication (`:96-149` vs `:103-150`). Each pair is the same delegation shape over a
   198	different receiver (`self.inner` vs `ResolverContext::method(self.inner, ..)`) — a single shared default
   199	or a common inner-delegate helper removes the duplication without touching the genuinely session-specific
   200	overrides (`authoritative_current_content_hash`, `observe_materialize_scope`, `indexed_for_current_content`,
   201	`artifact_key_for_current_content`, `resolve_type_dependency_canonical`, `shallow_file_state`,
   202	`active_session_view`, `complete_canonical`/`complete_canonical_with_session_view` — these have no
   203	host-side analog and stay distinct; `session_resolver_context.rs:304-332,357-403,421-470,591-608,280-288,
   204	712-715,171-180`). These structs stay in `verter_session` after the crate move (see convergence map
   205	above); only the trait they implement relocates.
   206	
   207	## Batched `NeedInputs` contract — full coverage (Fork 4)
   208	
   209	C1 owns the **kernel-level** attempt outcome: a resolver-core operation run under an I/O-free environment
   210	returns `AttemptOutcome::{Complete(T), NeedInputs(LoadSet), Terminal(AttemptFailure)}` per
   211	`contracts/input-loading.md` §2, §4, instead of calling `ensure_loaded`/`ensure_indexed_ready_serve`
   212	synchronously. `LoadSet` is normalized/sorted/deduplicated; `NeedInputs` on an empty delta with no basis
   213	change is the typed `InputResolutionNoProgress` failure (§4.3-4.5 of that contract), never a silent retry
   214	loop. This capability does not exist anywhere in the tree today (`grep -rn "NeedInputs\|LoadSet" crates/`
   215	— zero hits) and is new work, not a refactor of an existing batching mechanism.
   216	
   217	**Coverage is full, per the Fork 4 ruling — not "module/import resolution only."** Every non-flow
   218	`ModuleResolverCore`/`TypeInfoCore` operation reachable from a C2 projection attempt must support the
   219	outcome: module/import resolution, decl-body lowering, relation/inference queries driven through
   220	`execute_relate`'s plumbing (the algorithm itself stays `Preserve`; only the calling convention around it
   221	changes), and the member/JSDoc-hydration path that already reaches `ensure_indexed_ready_serve`
   222	(`crates/verter_session/src/typeinfo/shallow_surface.rs:179-193`) today. `contracts/input-loading.md:5-19`
   223	is unqualified across compiler/resolver/TypeInfo kernels, and ADR-011:19-28 requires each attempt to
   224	report the maximal sound missing-observation set — a partial cut would leave C2 unable to plan its
   225	complete semantic demand closure before projection (`contracts/compile-transaction.md:18,44-53`) for
   226	whichever operations were left out. Internally, `ensure_loaded`/`ensure_indexed_ready_serve` may still
   227	become the two central observation-or-demand choke points every covered operation funnels through — that
   228	is an implementation strategy, not a scope carve-out. Acceptance is never "one real load point exercised
   229	by a test harness"; it is every reachable operation covered.
   230	
   231	**C1 does not own** the outer staged transaction. `contracts/input-loading.md` §5: "A direct/project
   232	`CompileTypeInfo` over an immutable caller environment does not own commits or I/O. It returns
   233	`NeedInputs`; the caller may rebuild/extend the environment and retry." That caller — the
   234	prepare/plan/project/emit loop, `CompilePlanToken`/`ProjectionPlanToken` anti-replay, and the
   235	load/commit/retry orchestration across *multiple* kernel attempts — is `C2` (`program.md:242-247`, ADR-011).
   236	C1's obligation ends at: every resolver-tier operation reachable from a projection attempt, one immutable
   237	snapshot, one typed outcome per attempt. C2's obligation starts at: drive repeated C1 attempts, own the
   238	snapshot-extend/retry loop, own the anti-replay tokens.
   239	
   240	The existing blocking behavior is **not deleted**: the host-backed LSP session and the session-overlay
   241	lifecycle are permitted, documented lifecycles that legitimately block a cooperating thread
   242	(`decl_body_memo.rs:21`, `store_view_manager_tests.rs:2399` — "in-flight work block cooperatively, never
   243	busy-spin"). C1 adds the alternative outcome, across the full operation surface, so a *new* I/O-free
   244	lifecycle can reuse the same resolution logic without being forced to block; it does not retrofit every
   245	existing call site to stop blocking — `HostResolverContext`/`SessionResolverContext` keep blocking by
   246	design.
   247	
   248	## Legacy deletions
   249	
   250	- **`resolver_core/**` minus three named carve-outs, `resolver_store`'s immutable value types, and
   251	  `project_semantic_dispatch`** physically move out of `crates/verter_session/src/` into
   252	  `crates/verter_semantic/src/`. This includes `component_meta/` and `component_meta_query_engine/` in
   253	  full (verified dependency-neutral — see Context §1). The carve-outs, which stay in `verter_session`
   254	  because they hold `&VerterHost` (per the "Sealed lifecycle adapters" convergence-map row): 
   255	  `resolver_core/host_resolver_context.rs`, `resolver_core/session_resolver_context.rs`, and the `impl
   256	  ResolverContext for VerterHost` block plus its `VerterHost`-specific `sealed::Sealed`/`RequestBoundSealed`
   257	  impls inside `resolver_core/resolver_context.rs` (`:817-1343` — deleted rather than kept if the bare-host
   258	  rail itself is deleted, see the bare-host bullet below). `resolver_context.rs`'s trait definition and
   259	  `sealed` module (everything outside `:817-1343`) relocate with the kernel — the file is **split across
   260	  two crates**, not moved whole. The `verter_session::lib.rs` module declarations for the relocating pieces
   261	  (`mod project_semantic_dispatch` at line 332, `pub mod resolver_core`/`mod resolver_store` at lines
   262	  341-344) are deleted and replaced with narrower declarations covering only the three staying carve-out
   263	  files; `verter_semantic::lib.rs` gains the equivalent declarations for everything else. This is a
   264	  relocation, not a rewrite, for the moved code — its behavior is unchanged; its crate is not.
   265	- **`crates/verter_workspace/src/resolver.rs`** (`ProjectResolver`, 2122 lines) relocates into
   266	  `verter_semantic` as the `ModuleResolverCore` target. `verter_workspace`'s module declaration for it is
   267	  deleted; any I/O the resolver performs directly is converted to the `AttemptOutcome`/`LoadSet` pattern
   268	  as part of C1-AC-9, not left as a silent exception to full coverage.
   269	- **The bare `impl ResolverContext for VerterHost`** (`resolver_context.rs:817`) production-reachable
   270	  method bodies — confirmed to be `panic!("Architectural violation...")` in production builds today
   271	  (`resolver_context.rs:826-841,853-873,882-902,950-996,1021-1039,1054-1072,1081-1098,1107-1124,1171-1188`),
   272	  live only under `#[cfg(any(test, feature = "test-support"))]`. `VerterHost` stays defined in
   273	  `verter_session`; `ResolverContext` becomes a foreign trait from `verter_session`'s perspective, which
   274	  Rust's orphan rules permit implementing for a local type. If no production call site needs the
   275	  bare-host rail once convergence lands (verify at implementation time — every currently-known production
   276	  entry already binds `HostResolverContext`/`SessionResolverContext`), delete the impl entirely and let
   277	  `RequestBoundResolverContext` become the sole production-constructible rail. This turns "resolve
   278	  without a request-bound context" from a runtime panic into a compile error.
   279	- **`crates/verter_semantic/src/analysis/project_resolver.rs`** (94 lines, not a pure shim) — only the
   280	  `:1-30` re-export half (`pub use verter_workspace::resolver::{...}` / `verter_workspace::types::{...}`)
   281	  is deleted; its target no longer exists at that path once `ModuleResolverCore` relocates *into*
   282	  `verter_semantic` alongside it. The `:32-90` functions (`collect_resolvable_module_reference_specifiers`,
   283	  `resolve_known_module_reference_dependencies`) are real analysis-dependent logic, not shim, and stay at
   284	  this module path unchanged — their production callers (`verter_napi/src/lib.rs:2095,2117`,
   285	  `verter_wasm/src/lib.rs:640,667`) keep calling `verter_semantic::analysis::project_resolver::*` with no
   286	  repointing needed, since the module is not deleted, only its now-redundant re-export half.
   287	- **`crates/verter_semantic/src/facts/registry.rs:3`** (`pub use verter_workspace::fact_registry::*;`) —
   288	  the fact-key vocabulary (`FactKey`, `FactDomain`, `Fact`, etc., currently
   289	  `verter_workspace/src/fact_registry.rs`) is dependency-neutral value data, exactly the kind of type the
   290	  ruling's boundary assigns to `verter_semantic` ("dependency-neutral semantic store/value types"). It
   291	  moves into `verter_semantic` directly; the re-export is deleted, not left as a permanent alias.
   292	- **`crates/verter_semantic/src/analysis/routes.rs:196,251,661,672,869,1120`** (`workspace: &dyn
   293	  verter_workspace::WorkspaceRead`) — per Fork 2's ruling, `WorkspaceRead` does **not** relocate downward
   294	  (it also exposes live authoritative import resolution and dependency-graph authority, `traits.rs:214-
   295	  280,347-508`, which must stay a live capability, not a captured observation). Instead: the six call
   296	  sites are rewritten to take an owned immutable `RouteAnalysisInputs` snapshot; the orchestration that
   297	  currently calls `read_file`/`file_exists`/`is_dir`/`read_dir` to answer route-extraction questions moves
   298	  *upward* into `verter_workspace`/`verter_session`, which builds the snapshot and passes it down. The
   299	  existing pure extractors (`detect_routing_framework_from_json`, `extract_programmatic_routes`,
   300	  `routes.rs:208-233,266-289`) are unchanged in substance — only their input type changes.
   301	- **`crates/verter_identity/tests/cases/workspace_dependency_layers.rs:118-127`** —
   302	  `ratified_upward_exceptions()`'s `"verter_semantic"` row is deleted (not widened, not target-scoped
   303	  differently). The `"verter_diagnostics"` row is untouched — out of scope. This is the mechanical proof
   304	  that A5-DD1 closes for real: after this row is gone, *any* remaining `verter_semantic → verter_workspace`
   305	  edge fails the existing closure test, with no new guard authored.
   306	- **Any singleflight/condvar/mutex blocking-wait code path** that a converged, full-coverage
   307	  `AttemptOutcome::NeedInputs` caller would otherwise still be forced through — audit
   308	  `SingleflightGroup::run`/`run_retaining` (`resolver_core/mod.rs:2116-2214,2595-2639`),
   309	  `route_db_singleflight.rs:70-146`, and `prepared_decl.rs:35-38`'s `build_gate: parking_lot::Mutex<()>`.
   310	  Because coverage is now full (Fork 4), this audit is broader than the prior draft's narrower scope: every
   311	  blocking primitive reachable from a covered operation either stays confined to the blocking lifecycles
   312	  (`HostResolverContext`/`SessionResolverContext`, which still legitimately block) or gains a non-blocking
   313	  peek-and-decline path feeding `NeedInputs` instead of parking the calling thread.
   314	
   315	## Structural confinement
   316	
   317	Every invariant above is enforced structurally, matching the pattern the codebase already uses in this
   318	exact area — C1 extends that pattern, it does not invent scanner-based enforcement:
   319	
   320	- **The crate dependency firewall is the primary proof, not a marker.** Per the Fork 1/3 ruling, "prove
   321	  extractability without extracting" is not mechanically checkable — the existing structural graph guard
   322	  cannot see intra-crate direction (`docs/arch/refactor/rev11/evidence/A5/dependency-direction.md:
   323	  182-189`). Once the kernel physically lives in `verter_semantic`, the **existing, landed**
   324	  Cargo-metadata closure guard (`crates/verter_identity/tests/cases/workspace_dependency_layers.rs`) is
   325	  sufficient and requires no new guard: `verter_semantic` simply cannot name `VerterHost`, the scheduler,
   326	  or `verter_tsgo_api` types, because its production dependency closure does not reach them. That is what
   327	  makes the new observation interface's non-blocking, host-free guarantee real — not a marker trait
   328	  layered on top of a context that could still physically hold a host reference.
   329	- **Sealed-trait lifecycle closure.** `ResolverContext: sealed::Sealed` (relocated with the trait) already
   330	  makes "some fourth, unregistered lifecycle adapter" a compile error. C1 preserves the seal and, if the
   331	  bare-host rail is deleted (see above), tightens `RequestBoundResolverContext` from "narrower than
   332	  `ResolverContext`" to "identical to it" — a resolver-tier function taking `&dyn ResolverContext` becomes
   333	  provably request-bound by construction, not by convention. The new observation interface is a *separate*
   334	  sealed trait, not a subtrait of `ResolverContext` — per the ruling, a subtrait inherits
   335	  `ensure_loaded`/the host escape hatch and proves nothing.
   336	- **`StoreView`/`HostStoreView` immutability is already structural**, not a convention: `HostStoreView`
   337	  has no `&VerterHost`/`Arc<Mutex<..>>` field, only `Arc`-shared by-value snapshot data; a future violation
   338	  (a live host reference smuggled into the struct) is a visible field addition a reviewer catches at the
   339	  type definition, not a runtime behavior to detect. This is one of the value types that relocates into
   340	  `verter_semantic` unchanged.
   341	- **The Authority-uniqueness contract** (`.claude/skills/type-resolution/SKILL.md:790-796`) is enforced
   342	  today partly by a `syn`-AST content scanner (`crates/verter_session/tests/cases/architecture_guards.rs:
   343	  3384` `mod resolver_context_seal`) and partly by grandfathered string-content assertions. Per `CLAUDE.md`'s
   344	  forward-only landed-scanner rule, these are **grandfathered, not a template**. Because the scanned file
   345	  physically moves to `verter_semantic`, this grandfathered scanner's target path must be updated to keep
   346	  scanning the relocated file — that is "extending the existing grandfathered scanner's checked
   347	  assertions" (permitted) tracking a genuine regression risk from the move, not authoring a new
   348	  standalone scanner (forbidden). C1 must not add a new name-keyed source scanner for any new invariant it
   349	  introduces (the deleted bare-host rail, the collapsed duplication, the new `AttemptOutcome`, the new
   350	  observation interface) — new confinement here is type-level and crate-boundary-level only.
   351	
   352	## Boundary against flow (D-track) and against C2
   353	
   354	**Flow exclusion.** C1 converges the relation/inference authority (`execute_relate`,
   355	`project_semantic_dispatch/relation.rs`) exactly as it stands today — `Preserve`, not `Converge`. It adds
   356	no flow-specific relation classifier, no binding-slot integration, no nominal-identity/unique-symbol
   357	comparability extension. Those are `D3`'s stated job: "Extend the already-shared relation authority with
   358	nominal identities including unique symbols and tri-state comparability" (`program.md:249-253`). C1's only
   359	obligation to the flow track is that the relation authority it converges (now physically in
   360	`verter_semantic`) remains the single authority `D1` integrates against behind its private non-production
   361	test boundary (`program.md:229`) and that `D3` later extends without needing a second relation/name
   362	authority (`program.md:253`). The full-coverage `AttemptOutcome` threading touches the *calling
   363	convention* around `execute_relate` (per Fork 4) but never the relation algorithm itself.
   364	
   365	**C2 boundary.** C2 owns the staged compile transaction: `prepare → plan → project → emit`,
   366	`CompilePlanToken`/`ProjectionPlanToken` anti-replay, `CompileFactsBatch`, the concrete sealed
   367	`CompileTypeInfo`, and the outer load/commit/retry loop over C1's `NeedInputs` outcome (ADR-011,
   368	`program.md:242-247`). C1 must not build any of that — no plan token, no fact batch, no staged
   369	prepare/plan/project/emit state machine. C1's deliverable to C2 is now concretely: a converged kernel
   370	physically resident in `verter_semantic` that (a) gives identical answers across lifecycles, (b) can be
   371	driven from an I/O-free observation interface and returns `NeedInputs` instead of blocking across its
   372	**full** operation surface, and (c) is constructible without pulling in `verter_scheduler`/
   373	`verter_tsgo_api`/LSP-facing machinery *by construction of the crate graph*, so C2's compiler-facing
   374	facade embeds `verter_semantic` the same way `verter_compiler` already does today
   375	(`crates/verter_compiler/Cargo.toml:59`) with no cycle risk.
   376	
   377	## Required exit
   378	
   379	Per `program.md:196`: **lifecycle differs; module/name/type/relation meaning does not.** Concretely: the
   380	converged kernel (`ModuleResolverCore`, non-flow `TypeInfoCore`, `ProjectSemanticDispatch`, relation/node

exec
/bin/zsh -lc 'rg -n "''^#|''^##|''^###|''^FORK [1-4]|''^Ruling:|''^Verdict:|Abort/rescope|Implementation-time|New open questions" docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md | tail -160' in <block worktree root>
 succeeded in 552ms:
2308:### Fix Quality
2317:### Stub Prevention (CRITICAL)
2337:### Agent Feedback Capture
2345:## Dependencies Policy
2370:## Commit Convention
2387:## CI/CD
2391:## Skills Reference
2424:# Verter Architecture Reference
2428:## Shared Substrate Principle
2442:## TypeScript Packages
2452:## Unplugin Configuration (`packages/unplugin/`)
2468:## CSS Analysis & Selector Matching (`crates/verter_semantic/src/analysis/`)
2519:## Analysis MCP Server (`verter_mcp`)
2525:## verter_semantic::analysis — Static Analysis Types
2529:### AnalysisScope
2584:### ScriptAnalysisSnapshot
2599:### TemplateAnalysisSnapshot
2616:### ProjectIndex
2628:### Data Flow
2658:# Type Resolution
2660:## Project-Global Cache Authority (post-rewrite)
2680:## Canonical Dependency Cache Rule
2736:### Import-Route Admission Ownership
2779:### Parse/Resolve Ownership
2811:### Module-resolution keying (split env)
2825:## IndexedReady Target Contract
2841:### whole_env() consumer graph-native readers (Stage 6-prep readiness)
2852:## Semantic Query Identity Target Contract
2854:### Exact symbol-identity demand for typed feature roles
2905:### Vue Runtime Surface And Broad Runtime Classification
2958:## Semantic Heuristic Prevention (CRITICAL)
2979:## Typed Degradation And Completeness Contract (CRITICAL)
2988:## Cache Population Target Contract
3013:## Query Mode Contract
3031:### Open-Key-Domain Carrier-Stop (L1)
3055:### Reverse-homomorphic mapped recovery
3134:### Vue Runtime Surface And Broad Runtime Classification
3187:## Semantic Heuristic Prevention (CRITICAL)
3208:## Typed Degradation And Completeness Contract (CRITICAL)
3217:## Cache Population Target Contract
3246:## Query Mode Contract
3264:### Open-Key-Domain Carrier-Stop (L1)
3288:### Reverse-homomorphic mapped recovery
3363:## Query Mode Contract
3381:### Open-Key-Domain Carrier-Stop (L1)
3409:### Reverse-homomorphic mapped recovery
3469:## Path-Precise Navigation And Projection Contract
3491:## Navigator Boundary Contract
3515:## Generic Navigation And Expansion Contract
3536:## Derivation / Origin Layer Contract
3563:## Worked Examples
3567:### Example A — basic conditional
3577:### Example B — intersection with mixed static and generic members
3590:### Example C — `infer` inside a decidable conditional
3601:### Example D — path-precise projection
3627:### Example E — nested open conditionals
3642:### Example F — contributors-only union / intersection combining
3654:## Shallow File State and Frontier Engine
3702:## Semantic Dispatch (current authority)
3737:### Object-literal spreads: canonical construction programs
3820:## Retired solver surface and retained carriers
3844:## Declaration symbol inventories
3858:## Declaration Merging (CRITICAL)
3882:## Declaration Augmentation (CRITICAL)
3901:## Cross-File Type Resolution (Compiler Integration)
3940:## Macro Type Traversal Rule
3972:## Typed-IR-Only Resolver Rule (CRITICAL)
3987:### Typed Value Domain + Demand-Lattice Resolution (CRITICAL)
4019:## PARSELOWER Carrier Contracts (handle-migration foundation)
4039:### Macro Hot Mirror (Stage 5A, LANDED)
4094:## Macro Type Traversal Rule
4126:## Typed-IR-Only Resolver Rule (CRITICAL)
4145:### Typed Value Domain + Demand-Lattice Resolution (CRITICAL)
4177:## PARSELOWER Carrier Contracts (handle-migration foundation)
4201:### Macro Hot Mirror (Stage 5A, LANDED)
4272:## Frontier Engine Tests
4276:## Flow-Return Substrate (U6)
4339:### Flow-Return Substrate (U6) Debt Notes
4403:## Tagged-Component Publication: Root Admission Commits, Members Backfill
4419:## Template class fact demand
4436:## Reactive-wrapper demand (shared vocabulary)
8200:# Host & Session
8202:## Project-Global Cache on `VerterHost` (post-rewrite)
8224:## Vue Macro Codegen Producer
8266:## Language Server Architecture
8287:### Per-Project Configuration (`config.rs`)
8291:### TypeProvider Trait (`tsgo/traits.rs`)
8303:### TSGO Module (`tsgo/`)
8315:### tsserver Module (`tsserver/`)
8324:### Per-Project tsserver Routing (`tsserver/project_router.rs`)
8335:### Provider Selection (`main.rs`)
8352:### LSP Features (`features/`)
8378:### LSP Feature Flow
8384:### Macro Code-Action Authority: Membership, Placement, Revision
8454:## TypeProvider Architecture
8480:### Background File Sync
8484:### Public-API Entries: Response-Only vs Projection
8509:### Ordinary Carrier Import → Public-API Surface
8620:### Barrel-Import Eager Sync (TSGO)
8630:### Freeze Prevention (Fast Typing)
8652:### Heartbeat Watchdog
8656:### Async Workspace Scanning
8670:## Ownership Lifecycle & Bootstrap Sync
8696:### Carrier Owner Selection (tsgo-faithful, single-winner)
8712:### One Verter-Owned Diagnostic Set
8739:### One Rename-Plan Owner (prepare + rename)
8756:### Editor-Liveness Provider-Sync Invariant (CRITICAL)
8768:## Multi-Root Workspace & Per-Project Configuration
8795:## Async File Scheduler (`verter_scheduler`)
8811:### Snapshot Model
8815:### Host Integration
8819:### Batch Compile & Concurrency Model (current state)
8831:### LSP Integration
8835:### Authority Chain (Final State)
8849:### Host Store View (Post-Request-View Cutover)
8876:### Heartbeat Watchdog
8880:### Async Workspace Scanning
8894:## Ownership Lifecycle & Bootstrap Sync
8920:### Carrier Owner Selection (tsgo-faithful, single-winner)
8936:### One Verter-Owned Diagnostic Set
8963:### One Rename-Plan Owner (prepare + rename)
8984:### Editor-Liveness Provider-Sync Invariant (CRITICAL)
8996:## Multi-Root Workspace & Per-Project Configuration
9023:## Async File Scheduler (`verter_scheduler`)
9039:### Snapshot Model
9043:### Host Integration
9047:### Batch Compile & Concurrency Model (current state)
9059:### LSP Integration
9063:### Authority Chain (Final State)
9077:### Host Store View (Post-Request-View Cutover)
9091:### Store-View Token, Lane Identity, and Singleflight
9108:### Non-Current Store-View Contract — Capability Split (CRITICAL)
9130:### Key Files
9155:### Template class fact lanes
10960:   187	## Abort/rescope
12617:   268	## Abort/rescope
14381:### Fork 1 — VIOLATES
14389:Ruling: **EXTRACT-NOW into existing crate `verter_semantic`**, not a new `verter_typeinfo` crate. Exact boundary:
14397:### Fork 2 — VIOLATES
14403:Ruling: move the six workspace-taking capture/orchestration entry points upward, but keep semantic extraction downward. Pass an owned immutable `RouteAnalysisInputs` snapshot into the existing pure extractors—already exemplified by `detect_routing_framework_from_json` and `extract_programmatic_routes` (`routes.rs:208-233,266-289`). Moving all extraction into workspace would mix semantic interpretation into a lifecycle adapter.
14407:### Fork 3 — VIOLATES
14415:Ruling: extraction and I/O confinement are one decision. In the extracted kernel, use a separate capability-limited immutable observation interface that does **not** extend current `ResolverContext` and cannot return a host. The crate dependency firewall then makes naming `VerterHost` or the scheduler impossible. A marker may classify that interface, but the marker alone is not the proof.
14417:### Fork 4 — VIOLATES
14425:Ruling: **FULL-COVERAGE-REQUIRED** for every non-flow `ModuleResolverCore`/`TypeInfoCore` operation reachable from a C2 projection attempt. Internally, the two ensure functions may become central observation-or-demand choke points, but acceptance cannot be “one real load point” as the draft proposes (`C1-CHARTER-DRAFT.md:80,252-255`).
14427:FORK 1 — **VIOLATES — EXTRACT-NOW into `verter_semantic`.**  
14428:FORK 2 — **VIOLATES — move workspace capture upward; pass immutable route inputs downward.**  
14429:FORK 3 — **VIOLATES — extraction plus a capability-limited observation interface; no `ResolverContext` marker.**  
14430:FORK 4 — **VIOLATES — FULL-COVERAGE-REQUIRED for all non-flow projection-reachable operations.**
14437:### Fork 1 — VIOLATES
14445:Ruling: **EXTRACT-NOW into existing crate `verter_semantic`**, not a new `verter_typeinfo` crate. Exact boundary:
14453:### Fork 2 — VIOLATES
14459:Ruling: move the six workspace-taking capture/orchestration entry points upward, but keep semantic extraction downward. Pass an owned immutable `RouteAnalysisInputs` snapshot into the existing pure extractors—already exemplified by `detect_routing_framework_from_json` and `extract_programmatic_routes` (`routes.rs:208-233,266-289`). Moving all extraction into workspace would mix semantic interpretation into a lifecycle adapter.
14463:### Fork 3 — VIOLATES
14471:Ruling: extraction and I/O confinement are one decision. In the extracted kernel, use a separate capability-limited immutable observation interface that does **not** extend current `ResolverContext` and cannot return a host. The crate dependency firewall then makes naming `VerterHost` or the scheduler impossible. A marker may classify that interface, but the marker alone is not the proof.
14473:### Fork 4 — VIOLATES
14481:Ruling: **FULL-COVERAGE-REQUIRED** for every non-flow `ModuleResolverCore`/`TypeInfoCore` operation reachable from a C2 projection attempt. Internally, the two ensure functions may become central observation-or-demand choke points, but acceptance cannot be “one real load point” as the draft proposes (`C1-CHARTER-DRAFT.md:80,252-255`).
14483:FORK 1 — **VIOLATES — EXTRACT-NOW into `verter_semantic`.**  
14484:FORK 2 — **VIOLATES — move workspace capture upward; pass immutable route inputs downward.**  
14485:FORK 3 — **VIOLATES — extraction plus a capability-limited observation interface; no `ResolverContext` marker.**  
14486:FORK 4 — **VIOLATES — FULL-COVERAGE-REQUIRED for all non-flow projection-reachable operations.**

exec
/bin/zsh -lc "nl -ba docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md | sed -n '14355,14490p'" in <block worktree root>
 succeeded in 0ms:
 14355	   126	/// Backward-compatible type alias for [`ProjectResolver`].
 14356	   127	///
 14357	   128	/// Kept for downstream crates that reference the original name from
 14358	   129	/// the verter_semantic::analysis era.
 14359	   130	pub type NativeProjectResolver = ProjectResolver;
 14360	   131	
 14361	   132	impl ProjectResolver {
 14362	   133	    pub fn new(projects: Vec<IdeProjectConfig>) -> Self {
 14363	   134	        let mut projects = projects;
 14364	   135	        projects.sort_by(compare_projects);
 14365	   136	        Self { projects }
 14366	   137	    }
 14367	   138	
 14368	   139	    /// Every project config that EFFECTIVELY claims `file_id`, most-specific
 14369	   140	    /// (nearest-root) first, with genuine overlap PRESERVED (non-collapsing).
 14370	   141	    ///
 14371	   142	    /// Configured owners take precedence: when any configured project claims the
 14372	   143	    /// file, only the configured candidates survive (after nearest-root pruning — a
 14373	   144	    /// strict-ancestor root loses to a deeper co-claiming root, so `extends`/breadth
 14374	   145	    /// at an ancestor root does not make a descendant package file ambiguous when a
 14375	
 14376	codex
 14377	All four proposed positions fail. Forks 1 and 3 are one coupled decision.
 14378	
 14379	First, the draft is factually wrong that both modules are private: `resolver_core` is `pub mod`; only `resolver_store` is private (`crates/verter_session/src/lib.rs:341-342`). That does not rescue in-place convergence.
 14380	
 14381	### Fork 1 — VIOLATES
 14382	
 14383	Invariant: inward, cycle-free dependency direction; compiler may depend on the semantic kernel, never on session (`docs/arch/refactor/rev11/architecture.md:1110-1130`, `decisions/ADR-015-binding-dependency-direction.md:24-34`).
 14384	
 14385	A `pub(crate)` session facade is unusable by C2. Making it public would require `verter_compiler -> verter_session`, while session already depends on compiler (`crates/verter_session/Cargo.toml:117`), creating a cycle. Compiler already depends on the ratified layer-3 kernel crate, `verter_semantic` (`crates/verter_compiler/Cargo.toml:59`; `crates/verter_identity/tests/cases/workspace_dependency_layers.rs:44-47`).
 14386	
 14387	“Prove extractability without extracting” is not mechanically enforceable inside this crate. The existing structural graph guard explicitly cannot see intra-crate direction (`docs/arch/refactor/rev11/evidence/A5/dependency-direction.md:182-189`). A facade controls exposure, not what its implementation imports. The current context directly names scheduler state and `VerterHost` (`resolver_context.rs:183-196,700-717`).
 14388	
 14389	Ruling: **EXTRACT-NOW into existing crate `verter_semantic`**, not a new `verter_typeinfo` crate. Exact boundary:
 14390	
 14391	- `verter_semantic`: `ModuleResolverCore`, non-flow `TypeInfoCore`, `ProjectSemanticDispatch`, relation/node algorithms, dependency-neutral semantic store/value types, immutable observation contracts, and `AttemptOutcome`/`LoadSet`.
 14392	- `verter_workspace`/`verter_session`: observation capture, committed-store implementations, `VerterHost`, scheduler, I/O flights, cache-retention policy, and outer retry/commit orchestration.
 14393	- `verter_compiler`: C2’s concrete `CompileTypeInfo`, plans, facts, and anti-replay tokens.
 14394	
 14395	The kernel closure must contain only layers 1–3 and exclude compiler/workspace/session/scheduler/provider. The existing Cargo-metadata closure guard is the landed structural enforcement (`workspace_dependency_layers.rs:1-18,316-350`).
 14396	
 14397	### Fork 2 — VIOLATES
 14398	
 14399	Invariant: environments provide immutable data observations, not resolution semantics or live I/O (`architecture.md:461,631,660-662`).
 14400	
 14401	Relocating the whole `WorkspaceRead` trait is the wrong boundary. Route analysis uses only `read_file`, `file_exists`, `is_dir`, and `read_dir` (`routes.rs:195-205,250-259,660-678,868-879`). `WorkspaceRead`, however, also exposes authoritative import resolution (`traits.rs:214-280`), generations and dependency graphs (`traits.rs:347-508`), and directory/filesystem operations (`traits.rs:510-532`). Moving that interface downward would preserve live workspace authority inside the kernel and let environments continue implementing resolution semantics.
 14402	
 14403	Ruling: move the six workspace-taking capture/orchestration entry points upward, but keep semantic extraction downward. Pass an owned immutable `RouteAnalysisInputs` snapshot into the existing pure extractors—already exemplified by `detect_routing_framework_from_json` and `extract_programmatic_routes` (`routes.rs:208-233,266-289`). Moving all extraction into workspace would mix semantic interpretation into a lifecycle adapter.
 14404	
 14405	This does not foreclose F1: F1 can later replace the capture producer with its coherent committed snapshot without changing semantic computation. Moving `WorkspaceRead` downward would foreclose that separation; its current methods are live authority calls, not captured observations. F1 requires one coherent captured state (`architecture.md:410-422`).
 14406	
 14407	### Fork 3 — VIOLATES
 14408	
 14409	Invariant: compiler and semantic compute are structurally I/O-free (`contracts/input-loading.md:5-19`; `architecture.md:461`).
 14410	
 14411	The marker is externally sealed, but it does not prove I/O freedom. `RequestBoundResolverContext` extends `ResolverContext` (`resolver_context.rs:780-782`), so an analogous subtrait inherits `ensure_loaded`, `ensure_indexed_ready_serve`, and the direct host escape hatch (`resolver_context.rs:261-266,700-717`). Both current request-bound implementors physically contain `&VerterHost` (`host_resolver_context.rs:81-84`; `session_resolver_context.rs:85-89`) and delegate those methods to it (`host_resolver_context.rs:264-287`). `ensure_loaded` submits to and waits on the scheduler (`host_lifecycle.rs:988-1013`).
 14412	
 14413	The draft’s assertion that such a bound “does not expose” `ensure_loaded` is therefore plainly wrong (`C1-CHARTER-DRAFT.md:215-219`).
 14414	
 14415	Ruling: extraction and I/O confinement are one decision. In the extracted kernel, use a separate capability-limited immutable observation interface that does **not** extend current `ResolverContext` and cannot return a host. The crate dependency firewall then makes naming `VerterHost` or the scheduler impossible. A marker may classify that interface, but the marker alone is not the proof.
 14416	
 14417	### Fork 4 — VIOLATES
 14418	
 14419	Invariant: whole-attempt, maximal-sound batched demand discovery and complete compile facts.
 14420	
 14421	The normative contract covers compiler, resolver, and TypeInfo kernels without qualification (`contracts/input-loading.md:5-19`) and reruns the whole operation after each load wave (`contracts/input-loading.md:34-45`). ADR-011 additionally requires each attempt to report the maximal sound missing-observation set (`decisions/ADR-011-staged-compile-attempt-and-input-loading.md:19-28`).
 14422	
 14423	C2 plans the complete semantic demand closure before projection (`contracts/compile-transaction.md:18`) and requires one complete result per demand (`contracts/compile-transaction.md:44-53`). A remaining type-query path must either block—violating direct project mode—or return incomplete facts, which emission must reject. Current TypeInfo projection already reaches `ensure_indexed_ready_serve` for member/JSDoc hydration (`crates/verter_session/src/typeinfo/shallow_surface.rs:179-193`), so “module/import only” is not even a clean existing boundary.
 14424	
 14425	Ruling: **FULL-COVERAGE-REQUIRED** for every non-flow `ModuleResolverCore`/`TypeInfoCore` operation reachable from a C2 projection attempt. Internally, the two ensure functions may become central observation-or-demand choke points, but acceptance cannot be “one real load point” as the draft proposes (`C1-CHARTER-DRAFT.md:80,252-255`).
 14426	
 14427	FORK 1 — **VIOLATES — EXTRACT-NOW into `verter_semantic`.**  
 14428	FORK 2 — **VIOLATES — move workspace capture upward; pass immutable route inputs downward.**  
 14429	FORK 3 — **VIOLATES — extraction plus a capability-limited observation interface; no `ResolverContext` marker.**  
 14430	FORK 4 — **VIOLATES — FULL-COVERAGE-REQUIRED for all non-flow projection-reachable operations.**
 14431	tokens used
 14432	445 824
 14433	All four proposed positions fail. Forks 1 and 3 are one coupled decision.
 14434	
 14435	First, the draft is factually wrong that both modules are private: `resolver_core` is `pub mod`; only `resolver_store` is private (`crates/verter_session/src/lib.rs:341-342`). That does not rescue in-place convergence.
 14436	
 14437	### Fork 1 — VIOLATES
 14438	
 14439	Invariant: inward, cycle-free dependency direction; compiler may depend on the semantic kernel, never on session (`docs/arch/refactor/rev11/architecture.md:1110-1130`, `decisions/ADR-015-binding-dependency-direction.md:24-34`).
 14440	
 14441	A `pub(crate)` session facade is unusable by C2. Making it public would require `verter_compiler -> verter_session`, while session already depends on compiler (`crates/verter_session/Cargo.toml:117`), creating a cycle. Compiler already depends on the ratified layer-3 kernel crate, `verter_semantic` (`crates/verter_compiler/Cargo.toml:59`; `crates/verter_identity/tests/cases/workspace_dependency_layers.rs:44-47`).
 14442	
 14443	“Prove extractability without extracting” is not mechanically enforceable inside this crate. The existing structural graph guard explicitly cannot see intra-crate direction (`docs/arch/refactor/rev11/evidence/A5/dependency-direction.md:182-189`). A facade controls exposure, not what its implementation imports. The current context directly names scheduler state and `VerterHost` (`resolver_context.rs:183-196,700-717`).
 14444	
 14445	Ruling: **EXTRACT-NOW into existing crate `verter_semantic`**, not a new `verter_typeinfo` crate. Exact boundary:
 14446	
 14447	- `verter_semantic`: `ModuleResolverCore`, non-flow `TypeInfoCore`, `ProjectSemanticDispatch`, relation/node algorithms, dependency-neutral semantic store/value types, immutable observation contracts, and `AttemptOutcome`/`LoadSet`.
 14448	- `verter_workspace`/`verter_session`: observation capture, committed-store implementations, `VerterHost`, scheduler, I/O flights, cache-retention policy, and outer retry/commit orchestration.
 14449	- `verter_compiler`: C2’s concrete `CompileTypeInfo`, plans, facts, and anti-replay tokens.
 14450	
 14451	The kernel closure must contain only layers 1–3 and exclude compiler/workspace/session/scheduler/provider. The existing Cargo-metadata closure guard is the landed structural enforcement (`workspace_dependency_layers.rs:1-18,316-350`).
 14452	
 14453	### Fork 2 — VIOLATES
 14454	
 14455	Invariant: environments provide immutable data observations, not resolution semantics or live I/O (`architecture.md:461,631,660-662`).
 14456	
 14457	Relocating the whole `WorkspaceRead` trait is the wrong boundary. Route analysis uses only `read_file`, `file_exists`, `is_dir`, and `read_dir` (`routes.rs:195-205,250-259,660-678,868-879`). `WorkspaceRead`, however, also exposes authoritative import resolution (`traits.rs:214-280`), generations and dependency graphs (`traits.rs:347-508`), and directory/filesystem operations (`traits.rs:510-532`). Moving that interface downward would preserve live workspace authority inside the kernel and let environments continue implementing resolution semantics.
 14458	
 14459	Ruling: move the six workspace-taking capture/orchestration entry points upward, but keep semantic extraction downward. Pass an owned immutable `RouteAnalysisInputs` snapshot into the existing pure extractors—already exemplified by `detect_routing_framework_from_json` and `extract_programmatic_routes` (`routes.rs:208-233,266-289`). Moving all extraction into workspace would mix semantic interpretation into a lifecycle adapter.
 14460	
 14461	This does not foreclose F1: F1 can later replace the capture producer with its coherent committed snapshot without changing semantic computation. Moving `WorkspaceRead` downward would foreclose that separation; its current methods are live authority calls, not captured observations. F1 requires one coherent captured state (`architecture.md:410-422`).
 14462	
 14463	### Fork 3 — VIOLATES
 14464	
 14465	Invariant: compiler and semantic compute are structurally I/O-free (`contracts/input-loading.md:5-19`; `architecture.md:461`).
 14466	
 14467	The marker is externally sealed, but it does not prove I/O freedom. `RequestBoundResolverContext` extends `ResolverContext` (`resolver_context.rs:780-782`), so an analogous subtrait inherits `ensure_loaded`, `ensure_indexed_ready_serve`, and the direct host escape hatch (`resolver_context.rs:261-266,700-717`). Both current request-bound implementors physically contain `&VerterHost` (`host_resolver_context.rs:81-84`; `session_resolver_context.rs:85-89`) and delegate those methods to it (`host_resolver_context.rs:264-287`). `ensure_loaded` submits to and waits on the scheduler (`host_lifecycle.rs:988-1013`).
 14468	
 14469	The draft’s assertion that such a bound “does not expose” `ensure_loaded` is therefore plainly wrong (`C1-CHARTER-DRAFT.md:215-219`).
 14470	
 14471	Ruling: extraction and I/O confinement are one decision. In the extracted kernel, use a separate capability-limited immutable observation interface that does **not** extend current `ResolverContext` and cannot return a host. The crate dependency firewall then makes naming `VerterHost` or the scheduler impossible. A marker may classify that interface, but the marker alone is not the proof.
 14472	
 14473	### Fork 4 — VIOLATES
 14474	
 14475	Invariant: whole-attempt, maximal-sound batched demand discovery and complete compile facts.
 14476	
 14477	The normative contract covers compiler, resolver, and TypeInfo kernels without qualification (`contracts/input-loading.md:5-19`) and reruns the whole operation after each load wave (`contracts/input-loading.md:34-45`). ADR-011 additionally requires each attempt to report the maximal sound missing-observation set (`decisions/ADR-011-staged-compile-attempt-and-input-loading.md:19-28`).
 14478	
 14479	C2 plans the complete semantic demand closure before projection (`contracts/compile-transaction.md:18`) and requires one complete result per demand (`contracts/compile-transaction.md:44-53`). A remaining type-query path must either block—violating direct project mode—or return incomplete facts, which emission must reject. Current TypeInfo projection already reaches `ensure_indexed_ready_serve` for member/JSDoc hydration (`crates/verter_session/src/typeinfo/shallow_surface.rs:179-193`), so “module/import only” is not even a clean existing boundary.
 14480	
 14481	Ruling: **FULL-COVERAGE-REQUIRED** for every non-flow `ModuleResolverCore`/`TypeInfoCore` operation reachable from a C2 projection attempt. Internally, the two ensure functions may become central observation-or-demand choke points, but acceptance cannot be “one real load point” as the draft proposes (`C1-CHARTER-DRAFT.md:80,252-255`).
 14482	
 14483	FORK 1 — **VIOLATES — EXTRACT-NOW into `verter_semantic`.**  
 14484	FORK 2 — **VIOLATES — move workspace capture upward; pass immutable route inputs downward.**  
 14485	FORK 3 — **VIOLATES — extraction plus a capability-limited observation interface; no `ResolverContext` marker.**  
 14486	FORK 4 — **VIOLATES — FULL-COVERAGE-REQUIRED for all non-flow projection-reachable operations.**

codex
The binding ruling is clearer than the charter’s inventory: the semantic crate gets only kernel algorithms, dependency-neutral values, immutable observation contracts, and typed outcomes; host capture, committed-store implementations, scheduler/I/O flights, and retention policy stay above. I’m now testing each recon item against that boundary, rather than treating the charter’s wildcard move literally.
exec
/bin/zsh -lc "rg -n \"VerterHost|CanonicalCompletionOverlay|complete_canonical|project_type_store|scheduler|epoch|request_store_view\" crates/verter_session/src/resolver_core/request_store_view.rs crates/verter_session/tests/cases/architecture_guards.rs && nl -ba crates/verter_session/src/resolver_core/request_store_view.rs | sed -n '1,280p' && nl -ba crates/verter_session/tests/cases/architecture_guards.rs | sed -n '3625,3675p'" in <block worktree root>
 succeeded in 802ms:
crates/verter_session/src/resolver_core/request_store_view.rs:14://! do not bump `store_view_epoch` on first-time additive loads — so a
crates/verter_session/src/resolver_core/request_store_view.rs:23://! - [`CanonicalCompletionOverlay`]: request-scoped shadowing side maps
crates/verter_session/src/resolver_core/request_store_view.rs:38://! ## Identity / epoch contract
crates/verter_session/src/resolver_core/request_store_view.rs:43://! with the same base epoch must still coalesce on singleflight lanes,
crates/verter_session/src/resolver_core/request_store_view.rs:49://! [`CanonicalCompletionOverlay::complete_canonical`] is **epoch-
crates/verter_session/src/resolver_core/request_store_view.rs:51://! `current_store_view_epoch()` no longer matches the base view's
crates/verter_session/src/resolver_core/request_store_view.rs:52://! `mutation_epoch()` at the time of the completion call, the call
crates/verter_session/src/resolver_core/request_store_view.rs:113:/// - `route_surface_index_fingerprints` — `complete_canonical` never
crates/verter_session/src/resolver_core/request_store_view.rs:130:/// [`CanonicalCompletionOverlay::complete_canonical`] (one short
crates/verter_session/src/resolver_core/request_store_view.rs:132:pub(crate) struct CanonicalCompletionOverlay {
crates/verter_session/src/resolver_core/request_store_view.rs:150:    /// `complete_canonical` has run for the request).
crates/verter_session/src/resolver_core/request_store_view.rs:226:/// it lives and dies with one `CanonicalCompletionOverlay` (one top-level
crates/verter_session/src/resolver_core/request_store_view.rs:328:/// `CanonicalCompletionOverlay::complete_canonical`.
crates/verter_session/src/resolver_core/request_store_view.rs:345:impl Default for CanonicalCompletionOverlay {
crates/verter_session/src/resolver_core/request_store_view.rs:351:impl CanonicalCompletionOverlay {
crates/verter_session/src/resolver_core/request_store_view.rs:481:    /// cheap (one `FileArtifactStore` lookup + a few scheduler reads);
crates/verter_session/src/resolver_core/request_store_view.rs:486:    /// `store_view_epoch` no longer matches `base.mutation_epoch()`, the
crates/verter_session/src/resolver_core/request_store_view.rs:494:    /// call [`Self::complete_canonical_with_session_view`] instead so
crates/verter_session/src/resolver_core/request_store_view.rs:496:    /// base scheduler hash) for canonicals the session has masked. See
crates/verter_session/src/resolver_core/request_store_view.rs:498:    pub(crate) fn complete_canonical(
crates/verter_session/src/resolver_core/request_store_view.rs:500:        host: &crate::VerterHost,
crates/verter_session/src/resolver_core/request_store_view.rs:504:        self.complete_canonical_inner(host, base, canonical, None);
crates/verter_session/src/resolver_core/request_store_view.rs:507:    /// Session-overlay-aware variant of [`Self::complete_canonical`].
crates/verter_session/src/resolver_core/request_store_view.rs:512:    /// the base host's scheduler-rooted state. Without this routing the
crates/verter_session/src/resolver_core/request_store_view.rs:528:    /// Epoch-guarded identically to [`Self::complete_canonical`].
crates/verter_session/src/resolver_core/request_store_view.rs:529:    pub(crate) fn complete_canonical_with_session_view(
crates/verter_session/src/resolver_core/request_store_view.rs:531:        host: &crate::VerterHost,
crates/verter_session/src/resolver_core/request_store_view.rs:536:        self.complete_canonical_inner(host, base, canonical, Some(view));
crates/verter_session/src/resolver_core/request_store_view.rs:540:    /// without consulting `host.scheduler` /
crates/verter_session/src/resolver_core/request_store_view.rs:541:    /// `host.effective_file_state` / `host.project_type_store().indexed()`.
crates/verter_session/src/resolver_core/request_store_view.rs:543:    /// The base [`Self::complete_canonical`] / `_inner` path resolves
crates/verter_session/src/resolver_core/request_store_view.rs:544:    /// the canonical's `whole_hash` from the scheduler then loads
crates/verter_session/src/resolver_core/request_store_view.rs:546:    /// `file_facts` + the authored route-surface hash. A canonical the scheduler does
crates/verter_session/src/resolver_core/request_store_view.rs:561:    /// The epoch guard against `host.current_store_view_epoch !=
crates/verter_session/src/resolver_core/request_store_view.rs:562:    /// base.mutation_epoch()` lives at the producer-side call site
crates/verter_session/src/resolver_core/request_store_view.rs:564:    /// concrete `&VerterHost` and the base view, and can short-circuit
crates/verter_session/src/resolver_core/request_store_view.rs:583:    fn complete_canonical_inner(
crates/verter_session/src/resolver_core/request_store_view.rs:585:        host: &crate::VerterHost,
crates/verter_session/src/resolver_core/request_store_view.rs:592:        // external-supersession dimensions (epoch / project-generation /
crates/verter_session/src/resolver_core/request_store_view.rs:593:        // env / identity), NOT `store_view_epoch` alone: an env-hash shift
crates/verter_session/src/resolver_core/request_store_view.rs:594:        // that moves no epoch still supersedes the base snapshot, so the
crates/verter_session/src/resolver_core/request_store_view.rs:595:        // epoch-only check would let a stale overlay write through
crates/verter_session/src/resolver_core/request_store_view.rs:617:        // authority — NOT the base scheduler. Without this branch the
crates/verter_session/src/resolver_core/request_store_view.rs:619:        // view's overlay hash with the scheduler's base hash, breaking
crates/verter_session/src/resolver_core/request_store_view.rs:638:        // scheduler first, then the host's `effective_file_state`
crates/verter_session/src/resolver_core/request_store_view.rs:642:            .scheduler()
crates/verter_session/src/resolver_core/request_store_view.rs:669:        host: &crate::VerterHost,
crates/verter_session/src/resolver_core/request_store_view.rs:685:        host: &crate::VerterHost,
crates/verter_session/src/resolver_core/request_store_view.rs:700:            host.project_type_store()
crates/verter_session/src/resolver_core/request_store_view.rs:774:    /// canonical id. The discriminating tests for the epoch guard
crates/verter_session/src/resolver_core/request_store_view.rs:775:    /// inspect this to assert that `complete_canonical` is a no-op
crates/verter_session/src/resolver_core/request_store_view.rs:776:    /// when `host.current_store_view_epoch() != base.mutation_epoch()`.
crates/verter_session/src/resolver_core/request_store_view.rs:784:    /// flag. Bypasses [`Self::complete_canonical`]'s host-state
crates/verter_session/src/resolver_core/request_store_view.rs:785:    /// lookups + the epoch guard so a test can stage the exact
crates/verter_session/src/resolver_core/request_store_view.rs:800:    /// without driving `complete_canonical`. Used by the discriminating
crates/verter_session/src/resolver_core/request_store_view.rs:856:/// shadowing [`CanonicalCompletionOverlay`].
crates/verter_session/src/resolver_core/request_store_view.rs:879:    overlay: Arc<CanonicalCompletionOverlay>,
crates/verter_session/src/resolver_core/request_store_view.rs:908:    pub(crate) fn new(base: &'a HostStoreView, overlay: Arc<CanonicalCompletionOverlay>) -> Self {
crates/verter_session/src/resolver_core/request_store_view.rs:931:        overlay: Arc<CanonicalCompletionOverlay>,
crates/verter_session/src/resolver_core/request_store_view.rs:946:    pub(crate) fn overlay(&self) -> &Arc<CanonicalCompletionOverlay> {
crates/verter_session/src/resolver_core/request_store_view.rs:956:    /// canonical id. The discriminating tests for the epoch guard
crates/verter_session/src/resolver_core/request_store_view.rs:957:    /// inspect this to assert that `complete_canonical` is a no-op
crates/verter_session/src/resolver_core/request_store_view.rs:958:    /// when `host.current_store_view_epoch() != base.mutation_epoch()`.
crates/verter_session/src/resolver_core/request_store_view.rs:1240:        // by mid-request `complete_canonical`) is authoritative; if the
crates/verter_session/src/resolver_core/request_store_view.rs:1308:    /// carries no source-env identities (`complete_canonical` promotes
crates/verter_session/src/resolver_core/request_store_view.rs:1408:        // NOT a per-canonical fact, so `complete_canonical` cannot
crates/verter_session/src/resolver_core/request_store_view.rs:1440:        // path. The epoch guard lives at
crates/verter_session/src/resolver_core/request_store_view.rs:1443:        // `VerterHost` type and the resolver-context seal
crates/verter_session/tests/cases/architecture_guards.rs:2706:    // `host: &'a VerterHost` to `ctx: &'a dyn ResolverContext` (the
crates/verter_session/tests/cases/architecture_guards.rs:2726:fn no_scheduler_backed_workspace_shim_in_session_src() {
crates/verter_session/tests/cases/architecture_guards.rs:2730:    // After Phase 6c, the deleted scheduler-backed shim file under
crates/verter_session/tests/cases/architecture_guards.rs:2760:    const FORBIDDEN_MODULE_FILE: &str = concat!("scheduler", "_shim.rs");
crates/verter_session/tests/cases/architecture_guards.rs:2778:         after Phase 6c removal — re-introducing the scheduler-backed \
crates/verter_session/tests/cases/architecture_guards.rs:2829:                 the scheduler-backed shim; re-introduction under any \
crates/verter_session/tests/cases/architecture_guards.rs:2853:        "no_scheduler_backed_workspace_shim_in_session_src violations:\n{}",
crates/verter_session/tests/cases/architecture_guards.rs:2867:// Phase 6b's classification of every cache-shaped `VerterHost` field is
crates/verter_session/tests/cases/architecture_guards.rs:2876://   2. Locate `pub struct VerterHost`.
crates/verter_session/tests/cases/architecture_guards.rs:2886://        - the `project_type_store` field itself (the destination).
crates/verter_session/tests/cases/architecture_guards.rs:2891:// Future commits that add a new cache-shaped field on `VerterHost` MUST
crates/verter_session/tests/cases/architecture_guards.rs:2908:/// `VerterHost` cache-shape field that Phase 6b classified as
crates/verter_session/tests/cases/architecture_guards.rs:2927:        // longer live on `VerterHost`; the syn-walk that drives this
crates/verter_session/tests/cases/architecture_guards.rs:2929:        // `VerterHost` would fail this guard until a fresh
crates/verter_session/tests/cases/architecture_guards.rs:2942:            "phase-06b-report.md §6b.2.F6.bypass: single-cell workspace handle (Arc<RwLock<Arc<dyn WorkspaceAccess>>>) shared with the scheduler's SourceLoader so the lock always reads through the latest workspace after set_workspace(). NOT a cache; a re-pointable handle.",
crates/verter_session/tests/cases/architecture_guards.rs:2947:        //     `compile_many_propagates_*_priority` tests on `VerterHost::compile_many`.
crates/verter_session/tests/cases/architecture_guards.rs:2955:            "phase-09b-report.md §0 row \"Test-only observables on VerterHost\": Mutex<Option<Priority>> test mailbox written by upsert_with_priority and read by compile_many_propagates_*_priority. Compiled out in production builds. NOT a cache.",
crates/verter_session/tests/cases/architecture_guards.rs:3017:        //     raw-analysis-snapshot scheduler lane (after the lane's
crates/verter_session/tests/cases/architecture_guards.rs:3078:        //     cache lives on `VerterHost` (not ProjectTypeStore)
crates/verter_session/tests/cases/architecture_guards.rs:3158:            // Skip the project_type_store destination field itself.
crates/verter_session/tests/cases/architecture_guards.rs:3162:            if field_name == "project_type_store" {
crates/verter_session/tests/cases/architecture_guards.rs:3219:            "Phase 8 regression: VerterHost field `{forbidden}` was \
crates/verter_session/tests/cases/architecture_guards.rs:3225:    // Parse lib.rs via syn and walk VerterHost fields.
crates/verter_session/tests/cases/architecture_guards.rs:3228:        no_off_store_host_caches_inner(&parsed, "VerterHost", &allow_list);
crates/verter_session/tests/cases/architecture_guards.rs:3237:         fields on VerterHost, which means either the cache-shape \
crates/verter_session/tests/cases/architecture_guards.rs:3341:            pub(crate) future_db: Arc<crate::project_type_store::ProjectTypeStore>,
crates/verter_session/tests/cases/architecture_guards.rs:3342:            pub(crate) future_cache: parking_lot::Mutex<crate::project_type_store::ProjectTypeStore>,
crates/verter_session/tests/cases/architecture_guards.rs:3364:// asserts no production reference to `crate::VerterHost`. Three classes
crates/verter_session/tests/cases/architecture_guards.rs:3367://   1. Use items: `use crate::VerterHost;`,
crates/verter_session/tests/cases/architecture_guards.rs:3368://      `use crate::VerterHost as Host;` (pulls the type into scope).
crates/verter_session/tests/cases/architecture_guards.rs:3369://   2. Type-position paths: `&VerterHost`, `Arc<VerterHost>`,
crates/verter_session/tests/cases/architecture_guards.rs:3370://      `host: &VerterHost`, generic bounds — anything where the type
crates/verter_session/tests/cases/architecture_guards.rs:3372://   3. Expression-position paths: `VerterHost::method`,
crates/verter_session/tests/cases/architecture_guards.rs:3373://      `<VerterHost as Trait>::method`, `VerterHost::new` — the type
crates/verter_session/tests/cases/architecture_guards.rs:3378:// resolver-tier modules legitimately construct `VerterHost`).
crates/verter_session/tests/cases/architecture_guards.rs:3415:                Self::UsePath => write!(f, "use VerterHost"),
crates/verter_session/tests/cases/architecture_guards.rs:3416:                Self::TypePath => write!(f, "type-position VerterHost"),
crates/verter_session/tests/cases/architecture_guards.rs:3417:                Self::ExprPath => write!(f, "expr-position VerterHost"),
crates/verter_session/tests/cases/architecture_guards.rs:3465:    /// Final segment ident equals "VerterHost".
crates/verter_session/tests/cases/architecture_guards.rs:3469:            .map(|s| s.ident == "VerterHost")
crates/verter_session/tests/cases/architecture_guards.rs:3473:    /// Any segment ident equals "VerterHost". Used for use-paths so
crates/verter_session/tests/cases/architecture_guards.rs:3474:    /// `use crate::VerterHost::field` (which would not parse) and
crates/verter_session/tests/cases/architecture_guards.rs:3475:    /// `use crate::{VerterHost, X}` (where the use group does not include
crates/verter_session/tests/cases/architecture_guards.rs:3476:    /// the trailing `VerterHost` directly) both still register.
crates/verter_session/tests/cases/architecture_guards.rs:3478:        path.segments.iter().any(|s| s.ident == "VerterHost")
crates/verter_session/tests/cases/architecture_guards.rs:3530:            if p.ident == "VerterHost" {
crates/verter_session/tests/cases/architecture_guards.rs:3618:            "found {} concrete VerterHost reference(s) in {} file(s):\n{}",
crates/verter_session/tests/cases/architecture_guards.rs:3626:    /// reference `VerterHost` to register the trait impl
crates/verter_session/tests/cases/architecture_guards.rs:3627:    /// (`impl ResolverContext for crate::VerterHost`). Whitelisting it
crates/verter_session/tests/cases/architecture_guards.rs:3635:                // for VerterHost` bridge — the trait surface itself.
crates/verter_session/tests/cases/architecture_guards.rs:3637:                // wrapper that owns the `&VerterHost` borrow needed to
crates/verter_session/tests/cases/architecture_guards.rs:3642:                // wrapper that owns the `&VerterHost` borrow needed to
crates/verter_session/tests/cases/architecture_guards.rs:3645:                // `request_store_view.rs` owns the
crates/verter_session/tests/cases/architecture_guards.rs:3646:                // `CanonicalCompletionOverlay::complete_canonical`
crates/verter_session/tests/cases/architecture_guards.rs:3648:                // `current_store_view_epoch` / `scheduler` /
crates/verter_session/tests/cases/architecture_guards.rs:3649:                // `derived_raw_cache` / `project_type_store` to
crates/verter_session/tests/cases/architecture_guards.rs:3657:                    || n == "request_store_view.rs"
crates/verter_session/tests/cases/architecture_guards.rs:3694:                 the concrete `VerterHost` type. See sub-plan §10a for \
crates/verter_session/tests/cases/architecture_guards.rs:3709:    // through the concrete `VerterHost` type. Re-introduction of a
crates/verter_session/tests/cases/architecture_guards.rs:3710:    // `VerterHost` reference in a seal-scope file fails this test.
crates/verter_session/tests/cases/architecture_guards.rs:3723:// `VerterHost::compile_many` / `get_virtual_file` substrate.
crates/verter_session/tests/cases/architecture_guards.rs:4042:                 `VerterHost::compile_many` / `VerterHost::get_virtual_file`. \
crates/verter_session/tests/cases/architecture_guards.rs:5189:            "crates/verter_scheduler/src/source_loader.rs",
crates/verter_session/tests/cases/architecture_guards.rs:5246:            // call, no VerterHost/WorkspaceAccess context to route
crates/verter_session/tests/cases/architecture_guards.rs:5703:        // entry-point that wires `VerterHost::resolve_type_with_audit`
crates/verter_session/tests/cases/architecture_guards.rs:5712:        // `VerterHost::get_flow_return_type_with_audit` (the single
crates/verter_session/tests/cases/architecture_guards.rs:5732:        // Consumed by host construction, the scheduler SourceLoader
crates/verter_session/tests/cases/architecture_guards.rs:5790:        // `VerterHost::analyze_with_audit` for semantic-analysis
crates/verter_session/tests/cases/architecture_guards.rs:5809:        // handlers through `VerterHost::lsp_audit_begin` exposed by
crates/verter_session/tests/cases/architecture_guards.rs:5845:        // tests/cases/g_misc0/host_tests.rs (project_type_store::*)
crates/verter_session/tests/cases/architecture_guards.rs:5846:        "pub mod project_type_store",
crates/verter_session/tests/cases/architecture_guards.rs:5968:        // via `VerterHost::batch_coordinator()`.
crates/verter_session/tests/cases/architecture_guards.rs:5973:        // the root `VerterHost` so the struct stays thin. Both the module's contents
crates/verter_session/tests/cases/architecture_guards.rs:5974:        // and the `VerterHost` field are `#[cfg(test)]`, so a release build carries
crates/verter_session/tests/cases/architecture_guards.rs:6047:        // re-exports the canonical data types (HostConfig, VerterHost,
crates/verter_session/tests/cases/architecture_guards.rs:6118:        // `VerterHost::provenance().session_overlay_cows`
crates/verter_session/tests/cases/architecture_guards.rs:9265:            "crates/verter_scheduler/src/source_loader.rs",
crates/verter_session/tests/cases/architecture_guards.rs:9266:            "scheduler source-loader fallback — reads disk only when the workspace overlay/snapshot is absent for a host-loaded path; transitional pending the full WorkspaceAccess integration.",
crates/verter_session/tests/cases/architecture_guards.rs:9335:            "Svelte conformance-oracle comparison engine, gated behind the `svelte-oracle` feature (excluded from the default gate). `load_golden` / `load_all_goldens` read the committed golden JSON TEST FIXTURES off disk for the conformance consumers to diff a normalized candidate against — in-repo test corpus, never workspace/semantic state, with no `VerterHost` / `WorkspaceAccess` context. Not a NativeFs/VFS disk-boundary bypass.",
crates/verter_session/tests/cases/architecture_guards.rs:9367:            "dev-dependency-only shared test-harness crate (`unique_temp_dir` etc.), never depended on by production code. The only `std::fs::` calls are inside `#[cfg(test)] mod tests` — a self-test that the minted path is actually a writable scratch dir. No production-path call, and the crate has no `VerterHost`/`WorkspaceAccess` context to route through — sibling of the `verter_lsp/src/config.rs` and `verter_lsp/src/test_utils.rs` test-fixture entries above.",
crates/verter_session/tests/cases/architecture_guards.rs:9752:// `every_db_in_project_type_store_participates_in_invalidation` walks
crates/verter_session/tests/cases/architecture_guards.rs:9760:/// [`crate::project_type_store::ProjectTypeStore`]?
crates/verter_session/tests/cases/architecture_guards.rs:9837:fn every_db_field_in_project_type_store_appears_in_inventory() {
crates/verter_session/tests/cases/architecture_guards.rs:9838:    let src = read_workspace_file("crates/verter_session/src/project_type_store.rs");
crates/verter_session/tests/cases/architecture_guards.rs:9839:    let inventory = verter_session::project_type_store::PROJECT_TYPE_STORE_DB_INVENTORY;
crates/verter_session/tests/cases/architecture_guards.rs:9849:         crates/verter_session/src/project_type_store.rs."
crates/verter_session/tests/cases/architecture_guards.rs:10018:    let src = read_workspace_file("crates/verter_session/src/project_type_store.rs");
crates/verter_session/tests/cases/architecture_guards.rs:10387:// 2. `no_direct_oxc_parser_calls_outside_scheduler_path` — only the
crates/verter_session/tests/cases/architecture_guards.rs:10473:/// Other production callers must go through the scheduler-routed parse
crates/verter_session/tests/cases/architecture_guards.rs:10505:/// IS the scheduler-bound entry point. Test sources are exempt.
crates/verter_session/tests/cases/architecture_guards.rs:10507:fn no_direct_oxc_parser_calls_outside_scheduler_path() {
crates/verter_session/tests/cases/architecture_guards.rs:10512:    // scheduler-bound parse path or a documented TODO to migrate.
crates/verter_session/tests/cases/architecture_guards.rs:10518:        // scheduler-bound parse entry — the single eval-program parse
crates/verter_session/tests/cases/architecture_guards.rs:10528:        // out), not a per-file materialise lane — the scheduler is not
crates/verter_session/tests/cases/architecture_guards.rs:10537:        // materialise flight, so the scheduler is not its authority — the parse
crates/verter_session/tests/cases/architecture_guards.rs:10549:        // The scheduler-path parse module itself, four counted parse
crates/verter_session/tests/cases/architecture_guards.rs:10550:        // funnels: `parse_non_sfc_snapshot` is the scheduler snapshot
crates/verter_session/tests/cases/architecture_guards.rs:10561:        // scheduler-bound snapshot builders.
crates/verter_session/tests/cases/architecture_guards.rs:10563:        // Typeinfo oracle-core sites — tracked debt, not scheduler
crates/verter_session/tests/cases/architecture_guards.rs:10598:        // typed-IR rules forbid). Not a file-processing path; the scheduler is
crates/verter_session/tests/cases/architecture_guards.rs:10804:                         sites; route the new site through the scheduler \
crates/verter_session/tests/cases/architecture_guards.rs:10815:        "Tier 1A guard `no_direct_oxc_parser_calls_outside_scheduler_path`: \
crates/verter_session/tests/cases/architecture_guards.rs:10818:         outside the scheduler-bound parse path: {violators:#?}\n\n\
crates/verter_session/tests/cases/architecture_guards.rs:10819:         Either route through the scheduler's `execute_source` (preferred) \
crates/verter_session/tests/cases/architecture_guards.rs:12238:    // scheduler-side request-context handle) are NOT producer audit
crates/verter_session/tests/cases/architecture_guards.rs:12542:/// `verter_scheduler/` is also out of scope: it documents
crates/verter_session/tests/cases/architecture_guards.rs:12544:/// scheduler does not call it (the scheduler crate's own TLS
crates/verter_session/tests/cases/architecture_guards.rs:12545:/// accessor is `verter_scheduler::request_context::current_request_id`).
crates/verter_session/tests/cases/architecture_guards.rs:13648:///   `project_type_store.evict_canonical(&canonical_id)`. A warm
crates/verter_session/tests/cases/architecture_guards.rs:13751:                self.project_type_store.evict_canonical(&canonical_id);
crates/verter_session/tests/cases/architecture_guards.rs:14871:/// - [`crate::VerterHost::current_content_pinned_indexed`] — the
crates/verter_session/tests/cases/architecture_guards.rs:14872:///   scheduler-pinned `IndexedReady` read.
crates/verter_session/tests/cases/architecture_guards.rs:14873:/// - [`crate::VerterHost::artifact_current_indexed`] — the artifact-only
crates/verter_session/tests/cases/architecture_guards.rs:14874:///   `IndexedReady` authority for a canonical the scheduler does not
crates/verter_session/tests/cases/architecture_guards.rs:14876:/// - [`crate::VerterHost::current_content_pinned_artifacts`] — the
crates/verter_session/tests/cases/architecture_guards.rs:14877:///   `FileArtifacts` analogue (scheduler-pinned, artifact-only
crates/verter_session/tests/cases/architecture_guards.rs:15140:            "fn read(&self) { let _ = self.project_type_store.indexed().get_any(c); }".to_string(),
crates/verter_session/tests/cases/architecture_guards.rs:15165:            "fn read(&self) {\n    let _ = self\n        .project_type_store\n        \
crates/verter_session/tests/cases/architecture_guards.rs:15194:            "fn read(&self) { let _ = self.project_type_store.member_display_facts().get_any(c); }"
crates/verter_session/tests/cases/architecture_guards.rs:15266:    /// uses the scheduler authority, a caller needing artifacts uses an
crates/verter_session/tests/cases/architecture_guards.rs:15289:             artifact. Resolve current identity through the scheduler \
crates/verter_session/tests/cases/architecture_guards.rs:15636:        //     detected; a clean scheduler-authority call MUST NOT.
crates/verter_session/tests/cases/architecture_guards.rs:15649:            "self-test: a scheduler-authority call MUST NOT be flagged",
crates/verter_session/tests/cases/architecture_guards.rs:16255:/// fresh membership epoch, so no `FileArtifactRoot` loses reachability
crates/verter_session/tests/cases/architecture_guards.rs:16258:/// that SAME epoch. Both are therefore impossible to bypass by
crates/verter_session/tests/cases/architecture_guards.rs:16312:        choke_body.contains("invalidate_augmentation_index_at_epoch"),
crates/verter_session/tests/cases/architecture_guards.rs:16314:         `invalidate_augmentation_index_at_epoch` — the chokepoint exists \
crates/verter_session/tests/cases/architecture_guards.rs:16316:         ONE membership epoch."
crates/verter_session/tests/cases/architecture_guards.rs:16341:        choke_body.contains("reserve_membership_epoch"),
crates/verter_session/tests/cases/architecture_guards.rs:16342:        "`retire_artifact_keys` MUST reserve a retirement epoch, else the \
crates/verter_session/tests/cases/architecture_guards.rs:16347:    // epoch.
crates/verter_session/tests/cases/architecture_guards.rs:16349:        .find("reserve_membership_epoch")
crates/verter_session/tests/cases/architecture_guards.rs:16353:        .expect("`retire_artifact_keys` MUST release its epoch reservation");
crates/verter_session/tests/cases/architecture_guards.rs:16356:        "the epoch reservation MUST span the whole application — a capture \
crates/verter_session/tests/cases/architecture_guards.rs:16357:         may never name an epoch whose mutation is still in flight."
crates/verter_session/tests/cases/architecture_guards.rs:16470:    // `VerterHost` (the leak this contract closes) fails here.
crates/verter_session/tests/cases/architecture_guards.rs:16474:        "VerterHost::resolver_store_view must return the capability-split \
crates/verter_session/tests/cases/architecture_guards.rs:16480:        "VerterHost::resolver_store_view must NOT hand back a raw `HostStoreView` \
crates/verter_session/tests/cases/architecture_guards.rs:17688:    // brace (the lib.rs `VerterHost` host-state shape: the principal
crates/verter_session/tests/cases/architecture_guards.rs:17770:    // carries `#[cfg(test)]`-gated `VerterHost` fields followed by
crates/verter_session/tests/cases/architecture_guards.rs:17776:    // `VerterHost`, declared below every gated field.
crates/verter_session/tests/cases/architecture_guards.rs:17787:        "the scanner must see the production `VerterHost` fields below the \
crates/verter_session/tests/cases/architecture_guards.rs:17881:        // UFCS / fully-qualified calls (`VerterHost::set_exact_resolutions(
crates/verter_session/tests/cases/architecture_guards.rs:18029:    // The scheduler-snapshot integrate re-syncs bundler routes through
crates/verter_session/tests/cases/architecture_guards.rs:18041:        "integrate_scheduler_snapshot",
crates/verter_session/tests/cases/architecture_guards.rs:18049:    // and bumps `store_view_epoch` before returning. A byte-identical
crates/verter_session/tests/cases/architecture_guards.rs:18251:        "impl H { fn set(&self) { VerterHost::set_exact_resolutions(self, c, r); } }";
crates/verter_session/tests/cases/architecture_guards.rs:18257:    let ufcs_clean = "impl H { fn set(&self) { VerterHost::set_exact_resolutions(self, c, r); ProjectTypeStore::bump_project_generation(&self.store); } }";
crates/verter_session/tests/cases/architecture_guards.rs:18351:/// (`project_type_store.rs` — the definition and its docs). Test files
crates/verter_session/tests/cases/architecture_guards.rs:18358:        .filter(|(loc, _)| !loc.contains("src/project_type_store.rs"))
crates/verter_session/tests/cases/architecture_guards.rs:18371:            .any(|(loc, _)| loc.contains("src/project_type_store.rs")),
crates/verter_session/tests/cases/architecture_guards.rs:18382:// `DeclBodyMemo` (demand-materialised through the scheduler-retained parse
crates/verter_session/tests/cases/architecture_guards.rs:18455:        let store_src = strip_comments(&read_production_source("src/project_type_store.rs"));
     1	//! Per-request shadowing wrapper around the immutable
     2	//! [`HostStoreView`].
     3	//!
     4	//! ## Why
     5	//!
     6	//! [`crate::resolver_core::ReadSetSignature`].`facts` is the sole
     7	//! cache-validity rail. Fact validation requires a live
     8	//! [`HostStoreView`]; that view is an immutable snapshot of the
     9	//! workspace's per-canonical facts at request-entry time.
    10	//!
    11	//! `HostStoreView::from_host` captures immutable store roots in O(1). The
    12	//! per-request wrapper keeps one fixed view and threads a borrow through the
    13	//! pipeline. But `ensure_loaded` and `ensure_indexed_ready_serve` deliberately
    14	//! do not bump `store_view_epoch` on first-time additive loads — so a
    15	//! request-entry view built BEFORE dependency discovery does not track
    16	//! later-loaded self-root canonicals. Without the overlay below the
    17	//! self-root validator (`validates_self_root_whole_hash`) rejects every
    18	//! such freshly-loaded canonical forever inside the request, creating a
    19	//! new first-cold regression.
    20	//!
    21	//! ## What this file owns
    22	//!
    23	//! - [`CanonicalCompletionOverlay`]: request-scoped shadowing side maps
    24	//!   that record additive loads observed mid-request. Their key sets only
    25	//!   grow, but an effective value may be replaced; one bracketed revision
    26	//!   identifies each stable shadowing state.
    27	//!   Constructed once, shared across cooperative-admission lanes via
    28	//!   `Arc`, dropped at request end.
    29	//!
    30	//! - [`RequestStoreView`]: a wrapper that owns the overlay and borrows
    31	//!   the request-entry [`HostStoreView`]. Implements
    32	//!   [`crate::resolver_core::StoreView`] with **shadowing-first**
    33	//!   semantics — if the overlay has a canonical/fact key, the overlay
    34	//!   value is authoritative and a mismatch is REJECTED (not retried
    35	//!   against the base view). If the overlay is absent for a key, reads
    36	//!   fall through to the base view.
    37	//!
    38	//! ## Identity / epoch contract
    39	//!
    40	//! The completion overlay does NOT participate in
    41	//! [`crate::resolver_core::StoreView::compat_token`]: the wrapper
    42	//! reports the base's compat token unchanged. Two concurrent requests
    43	//! with the same base epoch must still coalesce on singleflight lanes,
    44	//! while fact signatures distinguish completion states through their
    45	//! [`verter_workspace::ViewPopulation`]. This separation is deliberate:
    46	//! the compat token is the frozen base/session lane identity, whereas the
    47	//! completion population may advance inside one request.
    48	//!
    49	//! [`CanonicalCompletionOverlay::complete_canonical`] is **epoch-
    50	//! guarded**: if the host's
    51	//! `current_store_view_epoch()` no longer matches the base view's
    52	//! `mutation_epoch()` at the time of the completion call, the call
    53	//! returns without writing to the overlay. The outer stable executor
    54	//! then retries with a fresh base view, and the old overlay is dropped
    55	//! along with the retried context.
    56	//!
    57	//! ## 6.B preservation
    58	//!
    59	//! - **Session-overlay validation**: the wrapper chains in front of an
    60	//!   already session-rooted [`HostStoreView`] (constructed via
    61	//!   `with_session_overlay` once at request entry). Completion runs
    62	//!   ATOP that view; the overlay does not try to model session
    63	//!   overlay/tombstone state.
    64	//! - **`validated_at_generation`**: unaffected. The
    65	//!   `ProjectGeneration` fact validator routes through the base
    66	//!   view's project-generation snapshot; completion never alters it.
    67	//! - **Family memo gating + FIFO prune**: the overlay
    68	//!   changes validation visibility for facts already observed; it
    69	//!   does not change what `traced_facts`, `dispatch_dep_signature`,
    70	//!   `canonical_ids()`, or FIFO prune register. Consequently the generic
    71	//!   FIFO still cannot prefer durable Base/Session candidates over a
    72	//!   request-completion candidate; the population is visible only inside
    73	//!   `fact_dep_signature`.
    74	
    75	use std::sync::atomic::{AtomicBool, Ordering};
    76	use std::sync::Arc;
    77	
    78	#[cfg(test)]
    79	use std::sync::atomic::AtomicUsize;
    80	
    81	use parking_lot::RwLock;
    82	use rustc_hash::FxHashMap;
    83	use verter_workspace::{CompletionOverlayState, OverlayId};
    84	
    85	use crate::file_artifact_store::FileFacts;
    86	use crate::resolver_core::bracketed_generation::BracketedGeneration;
    87	use crate::resolver_core::prepared_decl::PreparedDeclBundle;
    88	use crate::resolver_core::reuse::ReuseClass;
    89	use crate::resolver_core::{
    90	    DerivedFactKind, FactVersionRef, ParseFactRef, ResolveImportsFactRef, ResolverHash16,
    91	    RouteSurfaceFactRef, StoreView, StoreViewCompatToken,
    92	};
    93	use crate::resolver_store::HostStoreView;
    94	use crate::types::Hash16;
    95	
    96	/// Per-request shadowing side maps recording additive loads that the
    97	/// request-entry [`HostStoreView`] does not track. Keys are retained for
    98	/// the request lifetime, while equal replacement is a no-op and changed
    99	/// replacement advances [`Self::revision`].
   100	///
   101	/// Overlay shape (post-iter3 bug audit):
   102	/// - `whole_hashes`
   103	/// - `derived_hashes` (per-canonical bundled `RouteDerivedHashes`)
   104	/// - `file_facts`
   105	///
   106	/// `import_routes`, `resolved_import_facts` handle, and `route_db`
   107	/// handle stay OUT — they are `Arc` clones of project-wide `DashMap`s
   108	/// and are already up-to-date through host-side concurrent writers.
   109	///
   110	/// The `route_surface_index_fingerprints` and
   111	/// `resolved_import_facts_known_miss_tags` fields that previously lived
   112	/// here were dead:
   113	/// - `route_surface_index_fingerprints` — `complete_canonical` never
   114	///   populated it (the augmentation index is a project-wide
   115	///   structural index, not a per-canonical fact), so the read site at
   116	///   `validates_route_surface_domain` always fell through to the base.
   117	/// - `resolved_import_facts_known_miss_tags` — `validates_resolve_imports_domain`
   118	///   probed `whole_hashes` + `known_miss_tags` and then
   119	///   unconditionally delegated to `self.base` regardless of the result.
   120	///   The shared `ResolvedImportFactsDb` is concurrently updated by
   121	///   writers on both base and overlay paths, so the base validator
   122	///   already sees mid-request additive entries.
   123	///
   124	/// Removing both eliminates hot-path probes + lock acquires per
   125	/// validation.
   126	///
   127	/// Reads are wait-free against concurrent writers within the request
   128	/// because [`RwLock`] readers do not block each other, and the
   129	/// overlay's writers are scoped to
   130	/// [`CanonicalCompletionOverlay::complete_canonical`] (one short
   131	/// critical section per first-cold canonical).
   132	pub(crate) struct CanonicalCompletionOverlay {
   133	    /// Process-unique identity plus a bracketed revision of the exact
   134	    /// shadowing state. The memo map below is deliberately excluded.
   135	    overlay_id: OverlayId,
   136	    revision: BracketedGeneration,
   137	    whole_hashes: RwLock<FxHashMap<String, Hash16>>,
   138	    /// Per-canonical derived hashes bundled by `DerivedFactKind` so a
   139	    /// read can locate the entry with a `&str` lookup (no per-read
   140	    /// owned tuple allocation).
   141	    derived_hashes: RwLock<FxHashMap<String, RouteDerivedHashes>>,
   142	    file_facts: RwLock<FxHashMap<String, Arc<FileFacts>>>,
   143	    /// Per-map monotonic "non-empty" flags (read-path
   144	    /// hygiene). Set to `true` (Release) when the corresponding map
   145	    /// receives its first insert; never flip back to `false` within a
   146	    /// request (the maps never become empty again). Readers `load(Acquire)`
   147	    /// and skip the `RwLock::read` + map lookup when the flag is
   148	    /// `false` — a hot-path optimisation for the very common case of an
   149	    /// empty overlay (validations that fire before any
   150	    /// `complete_canonical` has run for the request).
   151	    ///
   152	    /// **Strict ordering:** after acquiring the map's write lock, the
   153	    /// writer sets the flag BEFORE inserting (see the `write_*` helpers).
   154	    /// A reader that still observes `false` therefore precedes the insert;
   155	    /// a reader that observes `true` takes the read lock and cannot inspect
   156	    /// the map until the insertion completes. Setting the flag after the
   157	    /// insert would leave a false-negative window even if both operations
   158	    /// occurred under the same lock, because the false fast path skips that
   159	    /// lock entirely. A reader can therefore safely return `None` without
   160	    /// falling into a stale base-view validation; reordering the
   161	    /// store before the lock release would have left a window in
   162	    /// which the map was populated but the flag still read `false`,
   163	    /// causing concurrent readers to skip a real overlay entry and
   164	    /// optimistically accept a stale cached dependency via the base
   165	    /// view's untracked-canonical accept rule. Strict shadowing
   166	    /// correctness is preserved.
   167	    whole_hashes_nonempty: AtomicBool,
   168	    derived_hashes_nonempty: AtomicBool,
   169	    file_facts_nonempty: AtomicBool,
   170	    /// Request-scoped memo of prepared-decl bundles — the ONE
   171	    /// request-world memo covering the base, session-overlay and
   172	    /// `RequestOnly` worlds. See [`RequestBundleMemo`].
   173	    bundle_memo: RequestBundleMemo,
   174	    #[cfg(test)]
   175	    verify_write_protocol: AtomicBool,
   176	}
   177	
   178	/// Which world a memoised prepared-decl bundle was materialised for.
   179	///
   180	/// Base and session-overlay bundles for the SAME canonical are different
   181	/// values — the overlay one is built from the session's frozen bytes, the
   182	/// base one from the store-current artifact — so they occupy DISTINCT
   183	/// namespaces. Collapsing them would serve a base consumer the session's
   184	/// edit (and vice versa) inside the same request.
   185	#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
   186	pub(crate) enum BundleMemoWorld {
   187	    /// The base (non-overlaid) bundle.
   188	    Base,
   189	    /// The bundle built from the session view's overlay content, keyed by
   190	    /// that overlay's content hash so a request that re-snapshots under
   191	    /// different overlay bytes cannot reuse the previous one.
   192	    Overlay(Hash16),
   193	}
   194	
   195	/// Per-`(canonical, world)` memo slot.
   196	struct BundleMemoEntry {
   197	    /// The store-view compat token the bundle was materialised under —
   198	    /// the SAME complete external-coherence oracle singleflight lanes
   199	    /// coalesce on. A stability-retry attempt re-snapshots the base view,
   200	    /// so without the token a bundle whose import canonicalization walked
   201	    /// the superseded world could serve the fresh attempt.
   202	    token: StoreViewCompatToken,
   203	    /// How far the memoised value may travel. `RequestOnly` entries
   204	    /// replay their refusal on EVERY hit.
   205	    reuse: ReuseClass,
   206	    bundle: Arc<PreparedDeclBundle>,
   207	}
   208	
   209	/// The ONE request-world prepared-decl bundle memo.
   210	///
   211	/// ## What it is for
   212	///
   213	/// A prepared-decl bundle that cannot be SHARED still costs a full cold
   214	/// materialisation — including the per-import re-export-chain walk — on
   215	/// every touch. Two classes hit this:
   216	///
   217	/// * an overlay-bearing bundle, which R17 keeps out of the shared
   218	///   `prepared_decl_bundles` cache because that slot is keyed by
   219	///   canonical alone and would alias the base bundle;
   220	/// * a `RequestOnly` bundle, whose materialisation consumed a
   221	///   deterministic non-cacheable read (a FENCED serve, an unrootable
   222	///   import-route witness), so the shared admission gate declines it.
   223	///
   224	/// Both are COMPLETE and deterministic under the request's immutable
   225	/// view. This memo is the request-scoped home for exactly those values:
   226	/// it lives and dies with one `CanonicalCompletionOverlay` (one top-level
   227	/// request), never writes to any host / shared / persistent cache, and is
   228	/// NOT a request-local mirror of host state — the values it holds are
   229	/// precisely the ones host state must not hold.
   230	///
   231	/// ## Identity
   232	///
   233	/// The key is `(canonical, world)` and the entry carries the
   234	/// [`StoreViewCompatToken`]; a token mismatch is a MISS that replaces the
   235	/// superseded entry. The token folds the external-supersession
   236	/// dimensions AND the session-overlay identity, so it supplies the
   237	/// store-view validation token, the resolution-world identity, the
   238	/// population and the session/overlay identity in one comparison, while
   239	/// [`BundleMemoWorld`] supplies the base/overlay namespace split.
   240	///
   241	/// ## Admission is structural, not by convention
   242	///
   243	/// [`Self::insert`] itself refuses anything that is not
   244	/// [`ReuseClass::is_request_reusable`] — a cancelled, partial,
   245	/// lease-missed, mutation-unstable or overflow-refused materialisation
   246	/// cannot be memoised even by a caller that asks. That keeps the rule at
   247	/// ONE place instead of at every producer.
   248	#[derive(Default)]
   249	pub(crate) struct RequestBundleMemo {
   250	    entries: RwLock<FxHashMap<(String, BundleMemoWorld), BundleMemoEntry>>,
   251	    /// Monotonic "non-empty" flag with the same flag-after-insert
   252	    /// ordering discipline as `write_completion_entry`: readers skip the
   253	    /// lock entirely while the request has memoised nothing.
   254	    nonempty: AtomicBool,
   255	}
   256	
   257	impl RequestBundleMemo {
   258	    /// Read the memoised bundle for `(canonical, world)` if this request
   259	    /// already materialised it under exactly this view identity.
   260	    ///
   261	    /// Returns the bundle together with its [`ReuseClass`]; the caller
   262	    /// must [`ReuseClass::replay_refusal`] before returning the value, or
   263	    /// the reuse launders the taint the cold return carried.
   264	    pub(crate) fn get(
   265	        &self,
   266	        canonical: &str,
   267	        world: BundleMemoWorld,
   268	        token: StoreViewCompatToken,
   269	    ) -> Option<(Arc<PreparedDeclBundle>, ReuseClass)> {
   270	        if !self.nonempty.load(Ordering::Acquire) {
   271	            return None;
   272	        }
   273	        let entries = self.entries.read();
   274	        // One owned key per miss is the price of a tuple key; the
   275	        // `nonempty` fast path keeps it off the empty-memo hot path.
   276	        let entry = entries.get(&(canonical.to_owned(), world))?;
   277	        (entry.token == token).then(|| (Arc::clone(&entry.bundle), entry.reuse))
   278	    }
   279	
   280	    /// Memoise a request-reusable bundle for the rest of this request.
  3625	    /// `resolver_context.rs` is the bridging trait file: it must
  3626	    /// reference `VerterHost` to register the trait impl
  3627	    /// (`impl ResolverContext for crate::VerterHost`). Whitelisting it
  3628	    /// is structural — without it the seal scope would be
  3629	    /// self-violating.
  3630	    fn is_seal_bridge_file(path: &Path) -> bool {
  3631	        path.file_name()
  3632	            .and_then(|n| n.to_str())
  3633	            .map(|n| {
  3634	                // `resolver_context.rs` carries the `impl ResolverContext
  3635	                // for VerterHost` bridge — the trait surface itself.
  3636	                // `session_resolver_context.rs` is the session-bound
  3637	                // wrapper that owns the `&VerterHost` borrow needed to
  3638	                // reach view-aware host internals
  3639	                // (`prepared_decl_bundle_with_context` etc.) without
  3640	                // widening the trait surface.
  3641	                // `host_resolver_context.rs` is the request-bound
  3642	                // wrapper that owns the `&VerterHost` borrow needed to
  3643	                // reach the view-taking `*_with_store_view` helpers
  3644	                // and the canonical-completion hook.
  3645	                // `request_store_view.rs` owns the
  3646	                // `CanonicalCompletionOverlay::complete_canonical`
  3647	                // helper, which threads through the host's
  3648	                // `current_store_view_epoch` / `scheduler` /
  3649	                // `derived_raw_cache` / `project_type_store` to
  3650	                // promote freshly-loaded canonicals into the request
  3651	                // overlay.
  3652	                // All four are seal-bridge exemptions per sub-plan
  3653	                // §10a.0.A.
  3654	                n == "resolver_context.rs"
  3655	                    || n == "session_resolver_context.rs"
  3656	                    || n == "host_resolver_context.rs"
  3657	                    || n == "request_store_view.rs"
  3658	            })
  3659	            .unwrap_or(false)
  3660	    }
  3661	
  3662	    pub(super) fn run() {
  3663	        // Seal scope per sub-plan §10a.0.A. Three resolver-tier
  3664	        // directories (recursive) plus two top-level files.
  3665	        let crate_root = workspace_root().join("crates/verter_session/src");
  3666	        let scope_roots = [
  3667	            crate_root.join("resolver_core"),
  3668	            crate_root.join("meta_resolve"),
  3669	            crate_root.join("project_semantic_dispatch"),
  3670	        ];
  3671	        let scope_files = [
  3672	            crate_root.join("component_meta_caches.rs"),
  3673	            crate_root.join("component_meta_materialize.rs"),
  3674	        ];
  3675	

exec
/bin/zsh -lc "nl -ba crates/verter_session/src/resolver_core/request_store_view.rs | sed -n '460,735p'" in <block worktree root>
 succeeded in 84ms:
   460	                "the presence flag must be published while the map write lock is held"
   461	            );
   462	        }
   463	        facts
   464	            .insert(canonical.to_owned(), Arc::clone(&file_facts))
   465	            .is_none_or(|previous| previous.as_ref() != file_facts.as_ref())
   466	    }
   467	
   468	    /// The request's prepared-decl bundle memo — the request-world reuse
   469	    /// tier every bundle producer consults. See [`RequestBundleMemo`].
   470	    #[inline]
   471	    pub(crate) fn bundle_memo(&self) -> &RequestBundleMemo {
   472	        &self.bundle_memo
   473	    }
   474	
   475	    /// Idempotently promote a freshly-loaded canonical's facts into the
   476	    /// overlay.
   477	    ///
   478	    /// Called after a successful `ensure_loaded` / `ensure_indexed_ready_serve`
   479	    /// on a canonical the request-entry base view does not track.
   480	    /// Walking the host's currently-published per-canonical state is
   481	    /// cheap (one `FileArtifactStore` lookup + a few scheduler reads);
   482	    /// re-running the call after a prior completion is a no-op when the
   483	    /// state has not changed.
   484	    ///
   485	    /// **Epoch guard:** if the host's current
   486	    /// `store_view_epoch` no longer matches `base.mutation_epoch()`, the
   487	    /// call returns without writing to the overlay. The outer stable
   488	    /// executor will retry the request with a fresh view; mutating an
   489	    /// already-superseded overlay would risk steering validation toward
   490	    /// stale data.
   491	    ///
   492	    /// This is the BASE-only variant — used by [`HostResolverContext`]
   493	    /// where no session view is present. Session-bearing contexts must
   494	    /// call [`Self::complete_canonical_with_session_view`] instead so
   495	    /// the completion overlay records the session-overlay hash (not the
   496	    /// base scheduler hash) for canonicals the session has masked. See
   497	    /// [`Self::write_completion_entry`] for the strict-ordering contract.
   498	    pub(crate) fn complete_canonical(
   499	        &self,
   500	        host: &crate::VerterHost,
   501	        base: &HostStoreView,
   502	        canonical: &str,
   503	    ) {
   504	        self.complete_canonical_inner(host, base, canonical, None);
   505	    }
   506	
   507	    /// Session-overlay-aware variant of [`Self::complete_canonical`].
   508	    ///
   509	    /// When `view` carries an explicit overlay-Upsert for `canonical`
   510	    /// the overlay hash (and overlay-published artifacts) is the
   511	    /// authoritative source for the completion overlay's entries, NOT
   512	    /// the base host's scheduler-rooted state. Without this routing the
   513	    /// completion overlay's `whole_hashes` would shadow the session-
   514	    /// overlay's hash with the base hash, breaking the 6.B session-
   515	    /// overlay validation contract (`096e124a2`): a session-overlaid
   516	    /// canonical's facts would mis-validate against the base hash on
   517	    /// subsequent reads inside the same request.
   518	    ///
   519	    /// Resolution order (mirrors [`SessionResolverContext::authoritative_current_content_hash`]):
   520	    /// 1. `view.overlay_content_hash_for(canonical)` returns `Some` →
   521	    ///    write the overlay hash; load the overlay's
   522	    ///    [`FileArtifacts`] (not the base) for `file_facts` / derived
   523	    ///    hashes.
   524	    /// 2. `view.is_tombstoned(canonical)` → skip entirely (the session
   525	    ///    deleted the file; there is no current content to promote).
   526	    /// 3. Otherwise → fall through to the base-only logic.
   527	    ///
   528	    /// Epoch-guarded identically to [`Self::complete_canonical`].
   529	    pub(crate) fn complete_canonical_with_session_view(
   530	        &self,
   531	        host: &crate::VerterHost,
   532	        base: &HostStoreView,
   533	        view: &dyn crate::session_view::SessionView,
   534	        canonical: &str,
   535	    ) {
   536	        self.complete_canonical_inner(host, base, canonical, Some(view));
   537	    }
   538	
   539	    /// Promote a producer-known canonical's facts into the overlay
   540	    /// without consulting `host.scheduler` /
   541	    /// `host.effective_file_state` / `host.project_type_store().indexed()`.
   542	    ///
   543	    /// The base [`Self::complete_canonical`] / `_inner` path resolves
   544	    /// the canonical's `whole_hash` from the scheduler then loads
   545	    /// `FileArtifacts` from the indexed authority to populate
   546	    /// `file_facts` + the authored route-surface hash. A canonical the scheduler does
   547	    /// not track and the indexed authority cannot answer for at
   548	    /// completion time has NO artifact to read, so the
   549	    /// indexed-authority fallback inside
   550	    /// [`Self::write_completion_entry`] returns `None` and the route
   551	    /// derived-hash entry never enters the overlay. The next warm-read
   552	    /// validation therefore falls through to the immutable base roots,
   553	    /// which cannot see an artifact published after their capture, and
   554	    /// rejects the bundle as untracked, causing a fresh cold rebuild.
   555	    ///
   556	    /// This method writes the producer-known `(whole_hash, route_hash)`
   557	    /// pair directly into the overlay. Each
   558	    /// presence-before-insert ordering matches the `write_*` helpers
   559	    /// (lock held across flag publication and map insertion).
   560	    ///
   561	    /// The epoch guard against `host.current_store_view_epoch !=
   562	    /// base.mutation_epoch()` lives at the producer-side call site
   563	    /// (the host-tier prepared-decl-bundle materialiser holds the
   564	    /// concrete `&VerterHost` and the base view, and can short-circuit
   565	    /// before invoking this overlay write). Keeping `host` out of the
   566	    /// resolver-tier API surface preserves the resolver-context seal
   567	    /// (`no_concrete_verter_host_in_seal_scope` architecture guard).
   568	    pub(crate) fn complete_route_canonical(
   569	        &self,
   570	        canonical: &str,
   571	        whole_hash: Hash16,
   572	        route_hash: Option<Hash16>,
   573	    ) {
   574	        self.revision.mutate(|| {
   575	            let mut changed = self.write_whole_hash(canonical, whole_hash);
   576	            if let Some(route_hash) = route_hash {
   577	                changed |= self.write_route_hash(canonical, route_hash);
   578	            }
   579	            ((), changed)
   580	        });
   581	    }
   582	
   583	    fn complete_canonical_inner(
   584	        &self,
   585	        host: &crate::VerterHost,
   586	        base: &HostStoreView,
   587	        canonical: &str,
   588	        view: Option<&dyn crate::session_view::SessionView>,
   589	    ) {
   590	        // The base view is superseded — skip the overlay write; the outer
   591	        // executor retries against a fresh view. Gated on the COMPLETE
   592	        // external-supersession dimensions (epoch / project-generation /
   593	        // env / identity), NOT `store_view_epoch` alone: an env-hash shift
   594	        // that moves no epoch still supersedes the base snapshot, so the
   595	        // epoch-only check would let a stale overlay write through
   596	        // (view-liveness). The compute's own artifact / route /
   597	        // load-generation advances are deliberately EXCLUDED (the base
   598	        // view stays live across its own dependency loads).
   599	        //
   600	        // The `overlay_identity` dimension is normalised OUT of the
   601	        // comparison: `base` may be a session-overlaid view (its overlay
   602	        // identity is `Some(_)`) while the host's live base token carries
   603	        // `None`, and the request's frozen overlay is not an external
   604	        // mutation — so an overlay-identity difference must NOT read as a
   605	        // supersession here.
   606	        let base_external = crate::resolver_store::StoreViewValidationToken {
   607	            overlay_identity: None,
   608	            ..base.validation_token()
   609	        };
   610	        if base_external.externally_superseded_by(&host.current_validation_token()) {
   611	            return;
   612	        }
   613	
   614	        // Session-overlay precedence:
   615	        // if a session view is present and carries explicit overlay
   616	        // state for the canonical, that state is the request-scoped
   617	        // authority — NOT the base scheduler. Without this branch the
   618	        // completion overlay would shadow the session-rooted base
   619	        // view's overlay hash with the scheduler's base hash, breaking
   620	        // the 6.B session-overlay validation contract (096e124a2).
   621	        if let Some(view) = view {
   622	            if view.is_tombstoned(canonical) {
   623	                // The session deleted the file; there is no current
   624	                // content. The `with_session_overlay` re-rooting on
   625	                // `base` has already dropped any base per-canonical
   626	                // snapshot, so the completion overlay must not promote
   627	                // a stale state on top.
   628	                return;
   629	            }
   630	            if let Some(overlay_hash) = view.overlay_content_hash_for(canonical) {
   631	                self.write_completion_entry_from_overlay(host, view, canonical, overlay_hash);
   632	                return;
   633	            }
   634	        }
   635	
   636	        // Resolve the canonical's currently-tracked whole hash via the
   637	        // same authority chain `HostStoreView::build` consults: the
   638	        // scheduler first, then the host's `effective_file_state`
   639	        // fallback for canonicals that exist only in the artifact
   640	        // store.
   641	        let whole_hash = host
   642	            .scheduler()
   643	            .try_get_source(canonical)
   644	            .map(|source| source.whole_hash)
   645	            .or_else(|| {
   646	                host.effective_file_state(canonical, None)
   647	                    .map(|state| state.whole_hash)
   648	            });
   649	
   650	        let Some(whole_hash) = whole_hash else {
   651	            // No tracked content for this canonical — nothing to
   652	            // promote. A consumer that observed a fact against an
   653	            // unloaded canonical will fail the base view's untracked
   654	            // path (or accept the optimistic-accept rule), exactly as
   655	            // it did before the overlay was introduced.
   656	            return;
   657	        };
   658	
   659	        self.write_completion_entry(host, canonical, whole_hash, None);
   660	    }
   661	
   662	    /// Write the completion-overlay entries for a session-overlaid
   663	    /// canonical. Reads the overlay [`FileArtifacts`] (not the base)
   664	    /// so `file_facts` and the derived hashes match the OVERLAY
   665	    /// content version — the same authority `HostStoreView::with_session_overlay`
   666	    /// re-rooted the base view's per-canonical snapshots from.
   667	    fn write_completion_entry_from_overlay(
   668	        &self,
   669	        host: &crate::VerterHost,
   670	        view: &dyn crate::session_view::SessionView,
   671	        canonical: &str,
   672	        overlay_hash: crate::types::Hash16,
   673	    ) {
   674	        let overlay_identity = host.overlay_artifact_identity(canonical);
   675	        let file_artifacts = overlay_identity.lookup_overlay_artifacts(host, view);
   676	        self.write_completion_entry(host, canonical, overlay_hash, file_artifacts);
   677	    }
   678	
   679	    /// Write the completion-overlay entries for `(canonical, whole_hash)`.
   680	    /// When `file_artifacts` is `None`, falls back to a content-hash-
   681	    /// keyed `get_artifacts_for_content` read on the project store (the
   682	    /// base-only path used when no session view is in play).
   683	    fn write_completion_entry(
   684	        &self,
   685	        host: &crate::VerterHost,
   686	        canonical: &str,
   687	        whole_hash: crate::types::Hash16,
   688	        file_artifacts: Option<Arc<crate::file_artifact_store::FileArtifacts>>,
   689	    ) {
   690	        // Per-canonical `IndexedReady` projection — populates `file_facts`
   691	        // and the authored `Route` derived-hash entry. For a
   692	        // session-overlaid canonical the caller passes the overlay
   693	        // artifacts directly; for the base-only path we look them up by
   694	        // content hash here.
   695	        let file_artifacts = file_artifacts.or_else(|| {
   696	            let key = host.authoritative_current_artifact_key(canonical)?;
   697	            if key.content_hash != whole_hash {
   698	                return None;
   699	            }
   700	            host.project_type_store()
   701	                .indexed()
   702	                .get_artifacts_for_content(
   703	                    canonical,
   704	                    whole_hash,
   705	                    &key.parse_key,
   706	                    &key.file_language_id,
   707	                )
   708	        });
   709	        let route_hash = file_artifacts.as_ref().and_then(|file_artifacts| {
   710	            let indexed = &file_artifacts.indexed;
   711	            // Parse-environment reuse gate. The route surface is authored
   712	            // parse state, so an artifact built under a different parse
   713	            // environment must not publish its hash into this overlay.
   714	            let edge_current = host.indexed_surface_is_current(canonical, indexed);
   715	            if indexed.shallow_state.has_resolvable_surface() && edge_current {
   716	                Some(crate::resolver_store::hash_route_surface(
   717	                    &indexed.shallow_state,
   718	                ))
   719	            } else {
   720	                None
   721	            }
   722	        });
   723	
   724	        // One revision brackets the whole logical promotion even though
   725	        // its effective shadowing spans three maps. A reader can therefore
   726	        // never name a population for a partially-published completion.
   727	        self.revision.mutate(|| {
   728	            let mut changed = self.write_whole_hash(canonical, whole_hash);
   729	            if let Some(file_artifacts) = file_artifacts {
   730	                changed |= self.write_file_facts(canonical, Arc::clone(&file_artifacts.facts));
   731	            }
   732	            // The owner's import-route dependency is not a completion-
   733	            // overlay fact. Only the authored route surface is shadowed.
   734	            if let Some(route_hash) = route_hash {
   735	                changed |= self.write_route_hash(canonical, route_hash);

exec
/bin/zsh -lc "rg -n \"ProjectResolver|resolve_tracked|TrackedResolutionCapability|TransactionReader|resolver:\" crates/verter_workspace/src/workspace_snapshot.rs crates/verter_workspace/src/engine.rs crates/verter_workspace/src/project_graph.rs crates/verter_workspace/src/resolver.rs crates/verter_workspace/Cargo.toml crates/verter_semantic/Cargo.toml crates/verter_identity/tests/cases/workspace_dependency_layers.rs docs/arch/refactor/rev11/architecture.md docs/arch/refactor/rev11/decisions/ADR-015-binding-dependency-direction.md && nl -ba crates/verter_workspace/src/workspace_snapshot.rs | sed -n '1,95p' && nl -ba crates/verter_workspace/src/resolver.rs | sed -n '1,175p' && rg -n \"fn resolve_tracked|pub fn resolve_tracked\" crates/verter_workspace/src/resolver.rs && nl -ba crates/verter_workspace/src/resolver.rs | sed -n '1970,2055p'" in <block worktree root>
 succeeded in 125ms:
     1	//! Project-aware import resolver.
     2	//!
     3	//! Resolves import specifiers against tsconfig paths, project references,
     4	//! workspace aliases, node_modules (package.json exports/imports), and
     5	//! relative/absolute paths. Produces [`ResolveResult`] containing both the
     6	//! source path and the provider-graph path used by the type provider.
     7	
     8	use std::collections::{HashMap, HashSet};
     9	use std::path::Path;
    10	
    11	use crate::canonical_path::CanonicalPath;
    12	use crate::membership::ConfiguredMembership;
    13	use crate::types::PackageManifest;
    14	use crate::types::{
    15	    ProviderTarget, ResolutionContext, ResolutionKind, ResolvePhase, ResolveRequest,
    16	    ResolveRequestKind, ResolveResult,
    17	};
    18	
    19	// ── Types ──
    20	
    21	/// A workspace alias maps a prefix (e.g. `@/`) to a filesystem replacement.
    22	#[derive(Debug, Clone, PartialEq, Eq)]
    23	pub struct WorkspaceAlias {
    24	    pub find: String,
    25	    pub replacement: String,
    26	}
    27	
    28	/// Compiler options extracted from a tsconfig for resolution.
    29	#[derive(Debug, Clone, PartialEq, Eq, Default)]
    30	pub struct IdeProjectCompilerOptions {
    31	    pub base_url: Option<String>,
    32	    pub paths: Vec<(String, Vec<String>)>,
    33	    /// `compilerOptions.allowJs` — when set (or `checkJs`), `.js`/`.jsx`/
    34	    /// `.cjs`/`.mjs` join the project's supported-extension set.
    35	    pub allow_js: bool,
    36	    /// `compilerOptions.checkJs` — implies `allowJs` for membership purposes
    37	    /// (TypeScript treats `checkJs` as turning on JS type-checking, which
    38	    /// requires the JS files to be project members).
    39	    pub check_js: bool,
    40	    /// `compilerOptions.allowImportingTsExtensions` — when explicitly true,
    41	    /// tsserver barrel publication preserves authored `.vue`/`.svelte`
    42	    /// specifiers. Missing/false projects receive the `.verter.ts`
    43	    /// compatibility rewrite.
    44	    pub allow_importing_ts_extensions: bool,
    45	    /// `compilerOptions.disableSolutionSearching` — when a solution config sets
    46	    /// it, default-project selection does NOT climb from that solution to its
    47	    /// ancestor solution (mirrors tsgo `DisableSolutionSearching`). Default
    48	    /// `false`. Consumed by
    49	    /// [`WorkspaceSnapshot::default_configured_owner_for_file`](crate::workspace_snapshot::WorkspaceSnapshot::default_configured_owner_for_file).
    50	    pub disable_solution_searching: bool,
    51	}
    52	
    53	impl IdeProjectCompilerOptions {
    54	    /// Whether JavaScript files are project members (either `allowJs` or
    55	    /// `checkJs` is set).
    56	    #[must_use]
    57	    pub fn js_is_member(&self) -> bool {
    58	        self.allow_js || self.check_js
    59	    }
    60	}
    61	
    62	/// Membership filter for a tsconfig project.
    63	#[derive(Debug, Clone, PartialEq, Eq, Default)]
    64	pub enum ProjectMembership {
    65	    #[default]
    66	    MatchAll,
    67	    IncludeExclude {
    68	        files: Vec<String>,
    69	        include: Vec<String>,
    70	        exclude: Vec<String>,
    71	    },
    72	}
    73	
    74	/// Configuration for a single IDE project (tsconfig-backed).
    75	#[derive(Debug, Clone, PartialEq, Eq)]
    76	pub struct IdeProjectConfig {
    77	    pub root: String,
    78	    pub workspace_root: String,
    79	    pub tsconfig_path: Option<String>,
    80	    pub provider_root: String,
    81	    pub workspace_aliases: Vec<WorkspaceAlias>,
    82	    pub compiler_options: IdeProjectCompilerOptions,
    83	    pub references: Vec<String>,
    84	    /// Exact configured membership — the SAME [`ConfiguredMembership`] the
    85	    /// snapshot's `configured_owner_resolution_for_file` consults, so the
    86	    /// resolver and the ownership authority never diverge on a glob-vs-exact
    87	    /// membership answer. A fallback (tsconfig-less) config carries a
    88	    /// [`ConfiguredMembership::match_all_under_root`] membership.
    89	    pub membership: ConfiguredMembership,
    90	}
    91	
    92	impl IdeProjectConfig {
    93	    pub fn new(root: String, workspace_root: String, tsconfig_path: Option<String>) -> Self {
    94	        let provider_root = root.clone();
    95	        let membership = ConfiguredMembership::match_all_under_root(&CanonicalPath::new(&root));
    96	        Self {
    97	            root,
    98	            workspace_root,
    99	            tsconfig_path,
   100	            provider_root,
   101	            workspace_aliases: Vec::new(),
   102	            compiler_options: IdeProjectCompilerOptions::default(),
   103	            references: Vec::new(),
   104	            membership,
   105	        }
   106	    }
   107	
   108	    /// Whether `file_id` is a member of this project, per the exact
   109	    /// [`ConfiguredMembership`] (its materialized file set, or the compiled
   110	    /// spec globs for a match-all / filesystem-less membership). One
   111	    /// membership engine — no second glob evaluator.
   112	    pub fn matches_file(&self, file_id: &str) -> bool {
   113	        self.membership.contains(&CanonicalPath::new(file_id))
   114	    }
   115	}
   116	
   117	// ── ProjectResolver ──
   118	
   119	/// The main project resolver. Holds a sorted list of IDE project configs
   120	/// and resolves import specifiers against them.
   121	#[derive(Debug, Clone, PartialEq, Eq, Default)]
   122	pub struct ProjectResolver {
   123	    projects: Vec<IdeProjectConfig>,
   124	}
   125	
   126	/// Backward-compatible type alias for [`ProjectResolver`].
   127	///
   128	/// Kept for downstream crates that reference the original name from
   129	/// the verter_semantic::analysis era.
   130	pub type NativeProjectResolver = ProjectResolver;
   131	
   132	impl ProjectResolver {
   133	    pub fn new(projects: Vec<IdeProjectConfig>) -> Self {
   134	        let mut projects = projects;
   135	        projects.sort_by(compare_projects);
   136	        Self { projects }
   137	    }
   138	
   139	    /// Every project config that EFFECTIVELY claims `file_id`, most-specific
   140	    /// (nearest-root) first, with genuine overlap PRESERVED (non-collapsing).
   141	    ///
   142	    /// Configured owners take precedence: when any configured project claims the
   143	    /// file, only the configured candidates survive (after nearest-root pruning — a
   144	    /// strict-ancestor root loses to a deeper co-claiming root, so `extends`/breadth
   145	    /// at an ancestor root does not make a descendant package file ambiguous when a
   146	    /// descendant configured project also claims it) and fallbacks are suppressed;
   147	    /// otherwise the matching fallback configs are returned.
   148	    ///
   149	    /// This is the resolver's path→config lookup for import resolution and
   150	    /// provider-path derivation — NOT the carrier-ownership authority (that is
   151	    /// `verter_session`'s `CarrierOwnershipResolution`, which wraps the snapshot's
   152	    /// exact `configured_owner_resolution_for_file` and fails closed on a genuine
   153	    /// overlap). This lookup therefore never collapses an overlap into a no-owner
   154	    /// answer; callers choose nearest / any via [`Self::nearest_config_for_path`].
   155	    /// Candidates come out in the resolver's pre-sorted project precedence order
   156	    /// (deepest root first), so the first element is the nearest.
   157	    pub fn effective_configs_for_path(&self, file_id: &str) -> Vec<&IdeProjectConfig> {
   158	        // Collect every configured project whose membership claims the file.
   159	        let configured: Vec<&IdeProjectConfig> = self
   160	            .projects
   161	            .iter()
   162	            .filter(|project| project.tsconfig_path.is_some() && project.matches_file(file_id))
   163	            .collect();
   164	
   165	        if !configured.is_empty() {
   166	            // Nearest-root pruning: drop a configured candidate whose root is a
   167	            // STRICT ANCESTOR of another matching candidate's root. The length
   168	            // check makes containment STRICT (equal roots are not ancestors).
   169	            return configured
   170	                .iter()
   171	                .copied()
   172	                .filter(|candidate| {
   173	                    let candidate_root = normalize_canonical_id(&candidate.root);
   174	                    !configured.iter().any(|other| {
   175	                        if std::ptr::eq(*other, *candidate) {
354:    pub(crate) fn resolve_tracked(
  1970	            part => parts.push(part),
  1971	        }
  1972	    }
  1973	
  1974	    let mut result = String::new();
  1975	    if !prefix.is_empty() {
  1976	        result.push_str(&prefix);
  1977	    }
  1978	    if absolute {
  1979	        result.push('/');
  1980	    }
  1981	    result.push_str(&parts.join("/"));
  1982	
  1983	    if result.is_empty() {
  1984	        if absolute {
  1985	            "/".to_string()
  1986	        } else {
  1987	            ".".to_string()
  1988	        }
  1989	    } else if result.len() == 2 && result.as_bytes()[1] == b':' {
  1990	        format!("{result}/")
  1991	    } else {
  1992	        result
  1993	    }
  1994	}
  1995	
  1996	/// Join two path segments, collapsing `.`/`..`.
  1997	pub fn join_paths(base: &str, path: &str) -> String {
  1998	    if path.is_empty() {
  1999	        return normalize_canonical_id(base);
  2000	    }
  2001	    if is_absolute_specifier(path) {
  2002	        return collapse_path(path);
  2003	    }
  2004	
  2005	    let normalized_base = normalize_canonical_id(base)
  2006	        .trim_end_matches('/')
  2007	        .to_string();
  2008	    let normalized_path = normalize_canonical_id(path);
  2009	    collapse_path(&format!(
  2010	        "{}/{}",
  2011	        normalized_base,
  2012	        normalized_path
  2013	            .trim_start_matches("./")
  2014	            .trim_start_matches('/')
  2015	    ))
  2016	}
  2017	
  2018	/// Return the parent directory of a path.
  2019	pub fn parent_dir(path: &str) -> String {
  2020	    let normalized = normalize_canonical_id(path);
  2021	    normalized
  2022	        .rsplit_once('/')
  2023	        .map(|(dir, _)| dir.to_string())
  2024	        .unwrap_or_default()
  2025	}
  2026	
  2027	/// Check if a specifier is relative. Matches TypeScript's `pathIsRelative`
  2028	/// (`/^\.\.?($|[\\/])/`) exactly: the bare `.` / `..` directory specifiers
  2029	/// plus the `./`, `../`, `.\`, `..\` prefixes (the regex's `[\\/]` class
  2030	/// covers both separators) — `import ... from '..'` resolves to the parent
  2031	/// directory's index module, never as a bare package name.
  2032	///
  2033	/// Classification runs on the raw specifier text (like TS), and this
  2034	/// predicate does NOT normalize anything: separator normalization for a
  2035	/// specifier classified relative happens in the relative resolution
  2036	/// branches' [`join_paths`] call, whose `normalize_canonical_id` pass
  2037	/// rewrites `\` → `/` (TS `combinePaths`/`normalizeSlashes` semantics), so
  2038	/// `'..\index'` resolves byte-identically to `'../index'`. Non-relative
  2039	/// specifiers (package names, `#imports`) keep their bytes — `pkg\sub`
  2040	/// stays a package name.
  2041	pub fn is_relative_specifier(specifier: &str) -> bool {
  2042	    matches!(specifier, "." | "..")
  2043	        || specifier.starts_with("./")
  2044	        || specifier.starts_with("../")
  2045	        || specifier.starts_with(".\\")
  2046	        || specifier.starts_with("..\\")
  2047	}
  2048	
  2049	/// Check if a specifier is an absolute path.
  2050	pub fn is_absolute_specifier(specifier: &str) -> bool {
  2051	    specifier.starts_with('/')
  2052	        || Path::new(specifier).is_absolute()
  2053	        || specifier.as_bytes().get(1) == Some(&b':')
  2054	}
  2055	

exec
/bin/zsh -lc "nl -ba crates/verter_workspace/src/resolver.rs | sed -n '320,410p' && nl -ba crates/verter_workspace/src/engine.rs | sed -n '155,190p;2240,2295p;3305,3340p' && nl -ba crates/verter_workspace/src/project_graph.rs | sed -n '150,190p' && sed -n '1,90p' crates/verter_workspace/Cargo.toml && sed -n '1,80p' crates/verter_semantic/Cargo.toml && nl -ba crates/verter_identity/tests/cases/workspace_dependency_layers.rs | sed -n '20,145p' && nl -ba docs/arch/refactor/rev11/architecture.md | sed -n '1100,1140p' && nl -ba docs/arch/refactor/rev11/decisions/ADR-015-binding-dependency-direction.md | sed -n '20,42p'" in <block worktree root>
 succeeded in 1595ms:
   320	    /// project references) requires an owning project.
   321	    fn resolve_with_reader(
   322	        &self,
   323	        reader: &dyn crate::traits::WorkspaceRead,
   324	        request: &ResolveRequest,
   325	    ) -> Option<ResolveResult> {
   326	        let importer_owner = self.nearest_config_for_path(&request.importer_id);
   327	        let ctx = ResolutionContext {
   328	            phase: request.phase,
   329	            kind: request.kind,
   330	        };
   331	
   332	        let (source_id, resolution_kind) = match importer_owner {
   333	            Some(owner) => self.resolve_source_id(
   334	                reader,
   335	                owner,
   336	                &request.importer_id,
   337	                &request.specifier,
   338	                ctx,
   339	            )?,
   340	            None => {
   341	                // No owning project — try owner-independent branches only
   342	                self.resolve_source_id_unowned(
   343	                    reader,
   344	                    &request.importer_id,
   345	                    &request.specifier,
   346	                    ctx,
   347	                )?
   348	            }
   349	        };
   350	
   351	        Some(self.build_resolve_result(request, source_id, resolution_kind))
   352	    }
   353	
   354	    pub(crate) fn resolve_tracked(
   355	        &self,
   356	        _capability: &crate::engine::TrackedResolutionCapability,
   357	        reader: &crate::resolution_currency::TransactionReader<'_>,
   358	        request: &ResolveRequest,
   359	    ) -> Option<ResolveResult> {
   360	        self.resolve_with_reader(reader, request)
   361	    }
   362	
   363	    fn resolve_for_project_with_reader(
   364	        &self,
   365	        reader: &dyn crate::traits::WorkspaceRead,
   366	        owner: &crate::types::ProjectOwnership,
   367	        specifier: &str,
   368	        ctx: ResolutionContext,
   369	    ) -> Option<ResolveResult> {
   370	        let project = self.project_for_ownership(owner)?;
   371	        let (source_id, resolution_kind) =
   372	            self.resolve_source_id_for_project(reader, project, specifier, ctx)?;
   373	        Some(self.build_project_resolve_result(specifier, source_id, resolution_kind))
   374	    }
   375	
   376	    pub(crate) fn resolve_for_project_tracked(
   377	        &self,
   378	        _capability: &crate::engine::TrackedResolutionCapability,
   379	        reader: &crate::resolution_currency::TransactionReader<'_>,
   380	        owner: &crate::types::ProjectOwnership,
   381	        specifier: &str,
   382	        ctx: ResolutionContext,
   383	    ) -> Option<ResolveResult> {
   384	        self.resolve_for_project_with_reader(reader, owner, specifier, ctx)
   385	    }
   386	
   387	    pub(crate) fn project_exact_result(
   388	        &self,
   389	        importer_id: &str,
   390	        specifier: &str,
   391	        source_id: String,
   392	        context: ResolutionContext,
   393	    ) -> ResolveResult {
   394	        self.build_resolve_result(
   395	            &ResolveRequest {
   396	                importer_id: importer_id.to_owned(),
   397	                specifier: specifier.to_owned(),
   398	                kind: context.kind,
   399	                phase: context.phase,
   400	            },
   401	            source_id,
   402	            ResolutionKind::Bundler,
   403	        )
   404	    }
   405	
   406	    /// Build a [`ResolveResult`] from a resolved source path.
   407	    ///
   408	    /// Looks up `nearest_config_for_path()` on the **target** (not importer) for correct
   409	    /// `provider_id`/`provider_specifier`/`provider_target`/`owner_tsconfig_path`.
   410	    fn build_resolve_result(
   155	    base_epoch: ResolutionEpoch,
   156	    session_epoch: Option<ResolutionEpoch>,
   157	    session_domain: Option<Arc<SessionResolutionDomain>>,
   158	    world: Arc<CapturedResolutionWorld>,
   159	}
   160	
   161	struct ParsedEdgeInputs {
   162	    parsed_resolved: BTreeSet<String>,
   163	    unresolved_pairs: Vec<((String, ResolveRequestKind), String)>,
   164	    bare_specifiers: Vec<(String, ResolveRequestKind)>,
   165	}
   166	
   167	/// Unforgeable crate-internal proof that a resolver call is owned by Engine's
   168	/// sealed resolution transaction.
   169	///
   170	/// `ProjectResolver` accepts this only alongside a `TransactionReader`.
   171	/// Keeping construction private to this module prevents sibling production
   172	/// code from bypassing fact capture while preserving direct resolver unit
   173	/// tests inside the resolver module.
   174	pub(crate) struct TrackedResolutionCapability {
   175	    _private: (),
   176	}
   177	
   178	impl TrackedResolutionCapability {
   179	    fn new() -> Self {
   180	        Self { _private: () }
   181	    }
   182	}
   183	
   184	/// One canonical's re-observed resolution-visible values, read live before
   185	/// the resolution-world write gate is entered.
   186	struct ReobservedEvidence {
   187	    canonical: String,
   188	    live: crate::resolution_currency::LiveResolutionObservation,
   189	}
   190	
  2240	            snapshot_bytes: snapshot.approx_bytes(),
  2241	            edge_file_count: edges.file_count(),
  2242	            reverse_dep_bucket_count: edges.reverse_dep_bucket_count(),
  2243	            package_manifest_count: package_index.found_count(),
  2244	            published_project_count: published
  2245	                .as_ref()
  2246	                .map(|root| root.snapshot.projects.len())
  2247	                .unwrap_or(0),
  2248	        }
  2249	    }
  2250	
  2251	    /// Build and publish a snapshot from the current project graph.
  2252	    ///
  2253	    /// Derives a `WorkspaceSnapshot` + `ProjectResolver` from the current
  2254	    /// `project_graph` and atomically publishes them to `published_state`.
  2255	    /// Called by `set_project_graph()` and `configure_resolver()`.
  2256	    ///
  2257	    /// **Env-hash composition (project-scoped env-hash API).** Computes per-project
  2258	    /// `[parse, resolve, type_, lib]` env-hash arrays and project-identity
  2259	    /// hashes ONCE here, before publication, so the published snapshot
  2260	    /// carries its env-hash tables atomically. Producer reads from the
  2261	    /// project graph's `compiler_options` and the engine-level resolve
  2262	    /// extensions; consumers look up tables on the published snapshot.
  2263	    pub(crate) fn rebuild_and_publish(&self) {
  2264	        let _strict_transition = self.strict_self_root_transition();
  2265	        // The second env-table republication path (the first is
  2266	        // `publish_snapshot`): this recomposes `env_hashes_by_project` /
  2267	        // `project_identity_hashes` from the rebuilt project set, with no
  2268	        // content bump. Over-bumping a monotonic counter is conservative;
  2269	        // MISSING a bump here would leave a source-env-compacted signature
  2270	        // valid across a project reconfiguration.
  2271	        self.bump_source_env_generation();
  2272	        self.mutate_resolution_world(|world| {
  2273	            let configured_projects = self.configured_resolver_projects.read().clone();
  2274	            let graph = self.project_graph.read();
  2275	            let resolver = configured_projects
  2276	                .clone()
  2277	                .map(crate::resolver::ProjectResolver::new)
  2278	                .unwrap_or_else(|| graph.to_project_resolver());
  2279	
  2280	            // Build a WorkspaceSnapshot from the graph's projects
  2281	            let projects: Vec<_> = graph
  2282	                .iter()
  2283	                .enumerate()
  2284	                .map(|(i, config)| {
  2285	                    crate::snapshot_builder::ownership_project_from_vfs_config(
  2286	                        config,
  2287	                        crate::workspace_snapshot::ProjectId(i as u32),
  2288	                    )
  2289	                })
  2290	                .collect();
  2291	
  2292	            let generation = SnapshotGeneration(graph.generation());
  2293	
  2294	            drop(graph);
  2295	
  3305	                            .unwrap_or_else(|| {
  3306	                                transaction.lock().mark_incomplete_provenance();
  3307	                                ResolveResult {
  3308	                                    source_id: id.clone(),
  3309	                                    provider_id: id.clone(),
  3310	                                    provider_specifier: specifier.to_string(),
  3311	                                    provider_target: crate::types::ProviderTarget::SourceFile,
  3312	                                    resolution_kind: crate::types::ResolutionKind::Bundler,
  3313	                                    owner_tsconfig_path: None,
  3314	                                }
  3315	                            })
  3316	                    })
  3317	                } else {
  3318	                    crate::probe_scope!(RESOLVE_TRACKED);
  3319	                    let tracked = TransactionReader::new(reader, &transaction);
  3320	                    let capability = TrackedResolutionCapability::new();
  3321	                    captured.world.base.published.as_ref().and_then(|root| {
  3322	                        let request = crate::types::ResolveRequest {
  3323	                            importer_id: importer_id.to_string(),
  3324	                            specifier: specifier.to_string(),
  3325	                            kind: ctx.kind,
  3326	                            phase: ctx.phase,
  3327	                        };
  3328	                        root.snapshot
  3329	                            .resolver
  3330	                            .resolve_tracked(&capability, &tracked, &request)
  3331	                    })
  3332	                }
  3333	            };
  3334	            if !reused {
  3335	                let complete_context = Self::complete_provider_context(
  3336	                    captured.world.base.as_ref(),
  3337	                    selected_context,
  3338	                    result.as_ref(),
  3339	                    population,
  3340	                    &transaction,
   150	
   151	    /// Number of projects.
   152	    pub fn len(&self) -> usize {
   153	        self.projects.len()
   154	    }
   155	
   156	    /// Whether there are no projects.
   157	    pub fn is_empty(&self) -> bool {
   158	        self.projects.is_empty()
   159	    }
   160	
   161	    /// Get the project config at a given index (in precedence order).
   162	    pub fn get(&self, index: usize) -> Option<&VfsProjectConfig> {
   163	        self.projects.get(index)
   164	    }
   165	
   166	    /// Iterate over all projects in precedence order.
   167	    pub fn iter(&self) -> impl Iterator<Item = &VfsProjectConfig> {
   168	        self.projects.iter()
   169	    }
   170	
   171	    /// Convert the project graph to a `ProjectResolver` for import resolution.
   172	    pub fn to_project_resolver(&self) -> crate::resolver::ProjectResolver {
   173	        crate::resolver::ProjectResolver::new(
   174	            self.projects
   175	                .iter()
   176	                .map(VfsProjectConfig::to_ide_project_config)
   177	                .collect(),
   178	        )
   179	    }
   180	}
   181	
   182	/// Result of building a project graph from workspace roots.
   183	#[cfg(not(target_arch = "wasm32"))]
   184	pub struct ProjectGraphBuildResult {
   185	    pub graph: ProjectGraph,
   186	    /// Configs that need user trust before their aliases can be used.
   187	    pub trust_required: Vec<crate::vite_config::ViteConfigTrustInfo>,
   188	}
   189	
   190	#[cfg(not(target_arch = "wasm32"))]
[package]
name = "verter_workspace"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
homepage.workspace = true
authors.workspace = true
description = "Workspace layer for Verter — file access, resolution, project ownership"
publish = false

[features]
hotpath = ["dep:hotpath", "hotpath/hotpath"]
# Measurement-only: enables the `currency_probe` wall-clock/call-count rail
# (`src/currency_probe.rs`). Absent from `default`; requested ONLY by the
# `verter_bench` measurement harness, never by a production edge.
currency_probe = []
# Tests that require third-party repository checkouts beside this repo
# (Testing-Hermeticity): excluded from the default canonical run; enable
# explicitly with `--features external-corpus`.
external-corpus = []

[dependencies]
verter_debug_assert = { workspace = true }
arc-swap = "1.9"
dashmap = "6.2"
rustc-hash = { workspace = true }
hotpath = { version = "0.16", optional = true }
serde = { workspace = true }
serde_json = { workspace = true }
glob = "0.3.3"
im = "15.1"
parking_lot = "0.12.5"
smallvec = "1.15"
thiserror = "2.0"
verter_audit = { path = "../verter_audit" }
verter_language = { path = "../verter_language" }
verter_scheduler = { path = "../verter_scheduler" }
verter_span = { path = "../verter_span" }
verter_type_expr = { workspace = true }
xxhash-rust = { version = "0.8.15", features = ["xxh3"] }

[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
oxc_allocator = { workspace = true }
oxc_parser = { workspace = true }
oxc_ast = { workspace = true }
oxc_span = { workspace = true }
tracing = "0.1.44"
walkdir = "2.5"
verter_tsgo_api = { path = "../verter_tsgo_api" }

[dev-dependencies]
tempfile = "3.27"
[package]
name = "verter_semantic"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
homepage.workspace = true
authors.workspace = true
description = "Semantic authority for Verter — revision-tracked query engine, component surface, binding analysis, reactivity provenance"
publish = false

[features]
hotpath = ["dep:hotpath", "hotpath/hotpath"]
# Test-only observability accessors: the flow-slice hash invocation
# counter behind the hash-then-lower behavioral guard
# (`compute_flow_slice_hash_thread_invocations`). Enabled by consumers'
# DEV-dependency edges only (the session crate binds the guard from its
# test targets) — never on the default/production closure, so release
# builds compile `compute_flow_slice_hash` without the counter TLS or
# its increment. Plain `#[cfg(test)]` cannot gate this: the consumer is
# a different crate, where this crate's `test` cfg is false.
test-support = []

[dependencies]
verter_debug_assert = { workspace = true }
verter_span = { path = "../verter_span" }
verter_parser = { path = "../verter_parser" }
verter_workspace = { path = "../verter_workspace" }
# The stage-1 framework script-fact trait names the framework adapter / language
# ids and the carrier-language syntax gate. `verter_language` is a
# `verter_span`-only leaf, so this edge introduces no cycle (verter_semantic
# already depends on parser/workspace/span; verter_language depends on none of
# them).
verter_language = { path = "../verter_language" }
verter_css_syntax = { path = "../verter_css_syntax" }
verter_no_typeexpr = { workspace = true }
verter_type_expr = { workspace = true }
verter_type_expr_oxc = { workspace = true }
rustc-hash = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
smallvec = "1.15"
hotpath = { version = "0.16", optional = true }
oxc_allocator = { workspace = true }
oxc_parser = { workspace = true }
oxc_ast = { workspace = true }
oxc_ast_visit = { workspace = true }
oxc_semantic = { workspace = true }
oxc_span = { workspace = true }
oxc_str = { workspace = true }
oxc_syntax = { workspace = true }
sha2 = { workspace = true }
bitflags = "2.11"

[target.'cfg(target_arch = "wasm32")'.dependencies]
web-time = "1.1"

[dev-dependencies]
criterion = { version = "0.8.2", features = ["html_reports"] }
static_assertions = "1.1.0"
verter_test_support = { path = "../verter_test_support" }
# Activates the `test-support` feature on `verter_css_syntax` for
# `cargo test --package verter_semantic` (and example/bench) builds only —
# the `parse_inline_style_declarations` invocation counter the A20 "shared
# parser invoked exactly once" routing proof binds to
# (`extract_static_style_vars_shared_parser_invoked_exactly_once`). The
# regular dependency in `[dependencies]` does NOT enable the feature, so
# production builds compile `parse_inline_style_declarations` without the
# counter TLS or its increment.
verter_css_syntax = { path = "../verter_css_syntax", features = ["test-support"] }
    20	const LAYER_1_IDENTITY_SPAN_LANGUAGE_CONTRACTS: &[&str] = &[
    21	    "verter_span",
    22	    "verter_language",
    23	    "verter_ecma",
    24	    "verter_analysis_inputs",
    25	    "verter_audit",
    26	    "verter_no_typeexpr",
    27	    "verter_no_typeexpr_derive",
    28	    "verter_no_storedspan",
    29	    "verter_no_storedspan_derive",
    30	    // The dependency-neutral typed-identity/profile/mapping/result-contract
    31	    // vocabulary crate this test file itself lives in.
    32	    "verter_identity",
    33	    // Zero-dependency debug_assert!/debug_assert_eq!/debug_assert_ne! entry
    34	    // point; sits below every crate that uses it.
    35	    "verter_debug_assert",
    36	];
    37	
    38	const LAYER_2_SYNTAX_FRONTENDS_AND_NEUTRAL_DTOS: &[&str] = &[
    39	    "verter_type_expr",
    40	    "verter_type_expr_oxc",
    41	    "verter_parser",
    42	    "verter_css_syntax",
    43	    "verter_macro_dto",
    44	    "verter_session_query",
    45	];
    46	
    47	const LAYER_3_SEMANTIC_KERNEL: &[&str] =
    48	    &["verter_semantic", "verter_diagnostics", "verter_actions"];
    49	
    50	const LAYER_4_COMPILER: &[&str] = &["verter_compiler"];
    51	
    52	const LAYER_5_MANAGED_ENGINE_SESSION: &[&str] = &[
    53	    "verter_session",
    54	    "verter_workspace",
    55	    "verter_scheduler",
    56	    "verter_tsgo_api",
    57	    "verter_type_runtime",
    58	    "verter_protocol",
    59	];
    60	
    61	const LAYER_6_ADAPTERS: &[&str] = &[
    62	    "verter_lsp",
    63	    "verter_napi",
    64	    "verter_wasm",
    65	    "verter_ffi",
    66	    "verter_mcp",
    67	    "verter_mcp_server",
    68	    "verter_tsc",
    69	    "verter_relay_shim",
    70	    "verter-editor-client",
    71	];
    72	
    73	const LAYER_7_HARNESSES: &[&str] = &[
    74	    "verter_bench",
    75	    "verter_dx_baseline",
    76	    "verter_vue_conformance",
    77	    "verter_svelte_conformance",
    78	    "verter_session_oracle_macro",
    79	    // Test-only shared primitives (unique scratch paths, ephemeral ports,
    80	    // deterministic counters). Consumed exclusively via `[dev-dependencies]`,
    81	    // which is outside this test's tracked production closure, so its wide
    82	    // dev-dependent fan-out never trips the "nothing may depend on a
    83	    // harness" firewall.
    84	    "verter_test_support",
    85	    // Architecture/policy/portability guards relocated out of verter_session's
    86	    // consolidated test binary (gate-performance step 2). A pure test-only
    87	    // crate: its production [dependencies] is empty, and verter_session /
    88	    // verter_span / verter_workspace are consumed exclusively via
    89	    // [dev-dependencies] to check generated output against verter_session's
    90	    // public API — never to be depended ON by anything.
    91	    "verter_source_policy_gate",
    92	    // The gate's shipped-cfg guard target (gate-performance step 3,
    93	    // SINGLE-TEST-UNIVERSE directive): a pure test-only crate whose
    94	    // production [dependencies] is empty and whose `verter_session` edge is
    95	    // [dev-dependencies]-only — never depended ON by anything.
    96	    "verter_shipped_cfg_contract",
    97	];
    98	
    99	/// Build tooling, not a layer. Checked by
   100	/// `xtask_is_never_a_production_dependency_of_a_layered_crate`.
   101	const REPOSITORY_TOOLING_NOT_IN_THE_LAYER_MATRIX: &[&str] = &["xtask"];
   102	
   103	fn layer_map() -> HashMap<&'static str, u8> {
   104	    let mut m = HashMap::new();
   105	    for &name in LAYER_1_IDENTITY_SPAN_LANGUAGE_CONTRACTS {
   106	        m.insert(name, 1);
   107	    }
   108	    for &name in LAYER_2_SYNTAX_FRONTENDS_AND_NEUTRAL_DTOS {
   109	        m.insert(name, 2);
   110	    }
   111	    for &name in LAYER_3_SEMANTIC_KERNEL {
   112	        m.insert(name, 3);
   113	    }
   114	    for &name in LAYER_4_COMPILER {
   115	        m.insert(name, 4);
   116	    }
   117	    for &name in LAYER_5_MANAGED_ENGINE_SESSION {
   118	        m.insert(name, 5);
   119	    }
   120	    for &name in LAYER_6_ADAPTERS {
   121	        m.insert(name, 6);
   122	    }
   123	    for &name in LAYER_7_HARNESSES {
   124	        m.insert(name, 7);
   125	    }
   126	    m
   127	}
   128	
   129	/// Recorded upward exception: layer-3 `verter_semantic` /
   130	/// `verter_diagnostics` → `verter_workspace` (layer 5) →
   131	/// `verter_scheduler` (unconditional) and `verter_tsgo_api` (native-only).
   132	/// Equality-pinned, never subset-checked — shrinking or growing the set
   133	/// fails until this map is deliberately updated.
   134	///
   135	/// `cargo metadata` without `--filter-platform` unions every target-gated
   136	/// edge, so both scheduler and tsgo_api appear. Target conditions are
   137	/// pinned by
   138	/// `the_ratified_exception_records_its_target_condition_precisely`.
   139	fn ratified_upward_exceptions() -> HashMap<&'static str, BTreeSet<&'static str>> {
   140	    let allowed: BTreeSet<&'static str> =
   141	        ["verter_workspace", "verter_scheduler", "verter_tsgo_api"]
   142	            .into_iter()
   143	            .collect();
   144	    let mut m = HashMap::new();
   145	    m.insert("verter_semantic", allowed.clone());
  1100	- host heap delta;
  1101	- conversion CPU/wall time;
  1102	- cancelled/superseded work after conversion starts.
  1103	
  1104	Avoid mandatory JSON on hot compiler paths when a typed native or compact binary transfer is appropriate. One explicit final copy may be safer and faster than unsafe borrowed host-memory lifetimes. FFI output does not retain internal arenas or unvalidated semantic handles.
  1105	
  1106	# 18. Dependency direction and owner model
  1107	
  1108	## 18.1 Binding dependency rules
  1109	
  1110	Logical dependency direction is inward:
  1111	
  1112	```text
  1113	identity/span/language/contracts
  1114	-> shared syntax frontends and dependency-neutral DTOs
  1115	-> semantic kernel / module resolver / relation / flow
  1116	-> compiler
  1117	-> managed engine/session
  1118	-> LSP/MCP/provider and NAPI/WASM/CLI adapters
  1119	```
  1120	
  1121	Rules:
  1122	
  1123	- syntax/contracts do not depend on compiler, session, provider, or LSP;
  1124	- semantic kernel does not depend on compiler, session, provider, or LSP;
  1125	- compiler may depend on syntax, the sealed semantic facade, and dependency-neutral closed DTOs;
  1126	- semantic projection DTOs live in a neutral owner and do not require compiler↔semantic cycles;
  1127	- managed engine depends on compiler/semantic, never the reverse;
  1128	- provider lifecycle never enters direct compiler or semantic kernel;
  1129	- adapters depend inward and do not expose internals back into core;
  1130	- durable build tests reject crate cycles and forbidden edges.
  1131	
  1132	## 18.2 Logical owners
  1133	
  1134	```text
  1135	Managed Engine
  1136	├── CommittedInputAuthority
  1137	├── QueryRuntime
  1138	├── CpuExecutor
  1139	├── FrameworkCatalog
  1140	├── CompilerService
    20	-> managed engine/session
    21	-> LSP/provider/MCP/NAPI/WASM/CLI adapters
    22	```
    23	
    24	Rules:
    25	
    26	- syntax/contracts do not depend on compiler, session, provider, or LSP;
    27	- semantic kernel does not depend on compiler, session, provider, or LSP;
    28	- compiler may depend on syntax, sealed semantic facade, and neutral closed DTOs;
    29	- managed engine depends on compiler/semantic, never the reverse;
    30	- provider lifecycle never enters direct compiler or semantic kernel;
    31	- adapters depend inward only;
    32	- durable build tests reject crate dependency cycles and forbidden edges.
    33	
    34	Logical owners do not automatically require crates; use modules/functions until a real dependency firewall or multi-consumer stable contract exists.
    35	
    36	## Consequences
    37	
    38	- direct compiler cannot become a session mode;
    39	- semantic kernel remains reusable across lifecycles;
    40	- fewer traits, erased bags, and `Arc` workarounds are required.
    41	
    42	## Rejected alternatives

exec
/bin/zsh -lc "nl -ba crates/verter_semantic/src/analysis/project_resolver.rs | sed -n '1,120p' && nl -ba crates/verter_lsp/src/project_resolver.rs | sed -n '1,120p' && rg -n \"crate::project_resolver|project_resolver::|NativeProjectResolver|IdeProjectConfig|ProjectResolver|WorkspaceAlias|IdeProjectCompilerOptions|ProjectMembership\" crates/verter_lsp --glob '*.rs' && rg -n \"verter_lsp::project_resolver|verter_lsp.*NativeProjectResolver|project_resolver::\\*\" crates packages --glob '*.rs' --glob '*.ts' --glob '*.tsx' --glob '*.js'" in <block worktree root>
 succeeded in 1009ms:
     1	//! Project resolver — re-exported from `verter_workspace::resolver`.
     2	//!
     3	//! All resolver types and `ProjectResolver` live in `verter_workspace`.
     4	//! This module re-exports them and adds analysis-dependent utility
     5	//! functions that reference `AnalyzedModuleReference`.
     6	
     7	use std::collections::HashSet;
     8	
     9	// ── Re-exports from verter_workspace ──
    10	//
    11	// Everything that was originally defined here is now in verter_workspace::resolver
    12	// and verter_workspace::types.  Downstream code (`verter_session`, `verter_lsp`,
    13	// `verter_napi`, `verter_wasm`) can keep importing via
    14	// `verter_semantic::analysis::project_resolver::*` unchanged.
    15	
    16	pub use verter_workspace::resolver::{
    17	    build_known_file_index, collapse_path, is_absolute_specifier, is_relative_specifier,
    18	    join_paths, normalize_canonical_id, normalize_known_file_id, parent_dir,
    19	    resolve_known_dependency_base, resolve_known_dependency_id, IdeProjectCompilerOptions,
    20	    IdeProjectConfig, NativeProjectResolver, ProjectMembership, ProjectResolver, WorkspaceAlias,
    21	};
    22	pub use verter_workspace::types::{
    23	    ProviderTarget, ResolutionContext, ResolutionKind, ResolvePhase, ResolveRequest,
    24	    ResolveRequestKind, ResolveResult,
    25	};
    26	
    27	// ── Analysis-dependent functions ──
    28	//
    29	// These functions reference `crate::analysis::AnalyzedModuleReference` and so stay
    30	// in `verter_semantic::analysis`. They delegate to the verter_workspace path helpers.
    31	
    32	pub fn collect_resolvable_module_reference_specifiers(
    33	    module_references: &[crate::analysis::AnalyzedModuleReference],
    34	) -> Vec<String> {
    35	    let mut seen = HashSet::new();
    36	    let mut specifiers = Vec::new();
    37	
    38	    for reference in module_references {
    39	        let candidates: &[String] = match reference.analyzability {
    40	            crate::analysis::ModuleReferenceAnalyzability::Exact => {
    41	                if let Some(specifier) = reference.literal_specifier.as_ref() {
    42	                    if !specifier.is_empty() && seen.insert(specifier.clone()) {
    43	                        specifiers.push(specifier.clone());
    44	                    }
    45	                }
    46	                continue;
    47	            }
    48	            crate::analysis::ModuleReferenceAnalyzability::FiniteSet => {
    49	                &reference.finite_specifiers
    50	            }
    51	            crate::analysis::ModuleReferenceAnalyzability::UnknownDynamic => continue,
    52	        };
    53	
    54	        for specifier in candidates {
    55	            if specifier.is_empty() || !seen.insert(specifier.clone()) {
    56	                continue;
    57	            }
    58	            specifiers.push(specifier.clone());
    59	        }
    60	    }
    61	
    62	    specifiers
    63	}
    64	
    65	pub fn resolve_known_module_reference_dependencies(
    66	    owner_id: &str,
    67	    module_references: &[crate::analysis::AnalyzedModuleReference],
    68	    known_ids: &[String],
    69	    extensions: &[String],
    70	) -> Vec<String> {
    71	    let known_index = build_known_file_index(known_ids);
    72	    let owner_id = normalize_known_file_id(owner_id);
    73	    let mut resolved = Vec::new();
    74	    let mut seen = HashSet::new();
    75	
    76	    for specifier in collect_resolvable_module_reference_specifiers(module_references) {
    77	        let Some(match_id) =
    78	            resolve_known_dependency_id(owner_id.as_str(), &specifier, &known_index, extensions)
    79	        else {
    80	            continue;
    81	        };
    82	        let match_id_normalized = normalize_known_file_id(&match_id);
    83	        if match_id_normalized == owner_id || !seen.insert(match_id_normalized) {
    84	            continue;
    85	        }
    86	        resolved.push(match_id);
    87	    }
    88	
    89	    resolved
    90	}
    91	
    92	#[cfg(test)]
    93	#[path = "project_resolver_tests.rs"]
    94	mod project_resolver_tests;
     1	pub use verter_semantic::analysis::project_resolver::*;
crates/verter_lsp/src/project_resolver.rs:1:pub use verter_semantic::analysis::project_resolver::*;
crates/verter_lsp/src/carrier_provider_projection.rs:19:use crate::project_resolver::{ResolvePhase, ResolveRequestKind};
crates/verter_lsp/src/carrier_provider_projection.rs:506:            vec![verter_workspace::IdeProjectConfig::new(
crates/verter_lsp/src/server_utils.rs:276:    resolver: &crate::project_resolver::NativeProjectResolver,
crates/verter_lsp/src/server_utils.rs:285:    resolver: &crate::project_resolver::NativeProjectResolver,
crates/verter_lsp/src/server_utils.rs:292:    resolver: &crate::project_resolver::NativeProjectResolver,
crates/verter_lsp/src/server_utils.rs:363:pub(super) struct LspProjectResolverReader<'a> {
crates/verter_lsp/src/server_utils.rs:367:impl<'a> LspProjectResolverReader<'a> {
crates/verter_lsp/src/server_utils.rs:373:impl verter_workspace::WorkspaceRead for LspProjectResolverReader<'_> {
crates/verter_lsp/src/server_utils.rs:456:impl verter_workspace::WorkspaceAccess for LspProjectResolverReader<'_> {
crates/verter_lsp/src/server_utils.rs:459:    // Rationale (§2.16b): `LspProjectResolverReader` is a thin file-read
crates/verter_lsp/src/server_utils.rs:521:    _resolver: &crate::project_resolver::NativeProjectResolver,
crates/verter_lsp/src/server_utils.rs:545:                phase: crate::project_resolver::ResolvePhase::ProviderGraph,
crates/verter_lsp/src/server_utils.rs:859:    resolver: &crate::project_resolver::NativeProjectResolver,
crates/verter_lsp/src/server_utils.rs:924:    _resolver: &crate::project_resolver::NativeProjectResolver,
crates/verter_lsp/src/server_utils.rs:929:) -> Option<Vec<crate::project_resolver::ResolveResult>> {
crates/verter_lsp/src/server_utils.rs:945:                            phase: crate::project_resolver::ResolvePhase::ProviderGraph,
crates/verter_lsp/src/server_utils.rs:970:                            phase: crate::project_resolver::ResolvePhase::ProviderGraph,
crates/verter_lsp/src/server_utils.rs:994:    _resolver: &crate::project_resolver::NativeProjectResolver,
crates/verter_lsp/src/server_utils.rs:999:) -> Option<Vec<crate::project_resolver::ResolveResult>> {
crates/verter_lsp/src/server_utils.rs:1023:                    phase: crate::project_resolver::ResolvePhase::ProviderGraph,
crates/verter_lsp/src/server_utils.rs:1045:) -> crate::project_resolver::ResolveRequestKind {
crates/verter_lsp/src/server_utils.rs:1047:        crate::project_resolver::ResolveRequestKind::TypeImport
crates/verter_lsp/src/server_utils.rs:1049:        crate::project_resolver::ResolveRequestKind::RequireCall
crates/verter_lsp/src/server_utils.rs:1051:        crate::project_resolver::ResolveRequestKind::EsmImport
crates/verter_lsp/src/server_utils.rs:1057:) -> crate::project_resolver::ResolveRequestKind {
crates/verter_lsp/src/server_utils.rs:1059:        crate::project_resolver::ResolveRequestKind::TypeImport
crates/verter_lsp/src/server_utils.rs:1061:        crate::project_resolver::ResolveRequestKind::RequireCall
crates/verter_lsp/src/server_utils.rs:1063:        crate::project_resolver::ResolveRequestKind::EsmImport
crates/verter_lsp/src/server_utils.rs:1643:                    phase: crate::project_resolver::ResolvePhase::ProviderGraph,
crates/verter_lsp/src/server_utils.rs:1654:            if resolved.provider_target == crate::project_resolver::ProviderTarget::CarrierPublicApi
crates/verter_lsp/src/workspace_state.rs:255:        NormalizedGlob, ProjectResolver,
crates/verter_lsp/src/workspace_state.rs:285:            resolver: ProjectResolver::default(),
crates/verter_lsp/src/extension_provider_tests.rs:777:    let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
crates/verter_lsp/src/extension_provider_tests.rs:778:        crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/extension_provider_tests.rs:783:        crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/extension_provider_tests.rs:1137:    let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
crates/verter_lsp/src/extension_provider_tests.rs:1138:        crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/extension_provider_tests.rs:1143:        crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/server_tests.rs:1208:        verter_workspace::IdeProjectCompilerOptions::default(),
crates/verter_lsp/src/server_tests.rs:1216:    compiler_options: verter_workspace::IdeProjectCompilerOptions,
crates/verter_lsp/src/server_tests.rs:1227:    // (`publish_carrier` → `WorkspaceProjectResolver`), and that resolver only
crates/verter_lsp/src/server_tests.rs:1289:    let mut resolver_project = crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/server_tests.rs:1295:    let resolver = verter_workspace::ProjectResolver::new(vec![resolver_project]);
crates/verter_lsp/src/server_tests.rs:1319:/// `WorkspaceProjectResolver` — the ownership-resolution source the carrier-sync
crates/verter_lsp/src/server_tests.rs:1347:            compiler_options: verter_workspace::IdeProjectCompilerOptions::default(),
crates/verter_lsp/src/server_tests.rs:1352:    let resolver = verter_workspace::ProjectResolver::new(vec![
crates/verter_lsp/src/server_tests.rs:1353:        crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/server_tests.rs:1800:            vec![crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/server_tests.rs:2130:    let ide_project = crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/server_tests.rs:2293:        crate::project_resolver::ResolveRequestKind::RequireCall
crates/verter_lsp/src/server_tests.rs:2308:        crate::project_resolver::ResolveRequestKind::TypeImport
crates/verter_lsp/src/server_tests.rs:2368:    let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
crates/verter_lsp/src/server_tests.rs:2369:        crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/server_tests.rs:2410:    let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
crates/verter_lsp/src/server_tests.rs:2411:        crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/server_tests.rs:2506:    let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
crates/verter_lsp/src/server_tests.rs:2507:        crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/server_tests.rs:2563:    let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
crates/verter_lsp/src/server_tests.rs:2564:        crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/server_tests.rs:2587:    let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
crates/verter_lsp/src/server_tests.rs:2588:        crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/server_tests.rs:2622:    let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
crates/verter_lsp/src/server_tests.rs:2623:        crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/server_tests.rs:2643:    let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
crates/verter_lsp/src/server_tests.rs:2644:        crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/server_tests.rs:2674:    let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
crates/verter_lsp/src/server_tests.rs:2675:        crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/server_tests.rs:2698:    let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
crates/verter_lsp/src/server_tests.rs:2699:        crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/server_tests.rs:2742:    let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
crates/verter_lsp/src/server_tests.rs:2743:        crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/server_tests.rs:3585:                compiler_options: verter_workspace::IdeProjectCompilerOptions::default(),
crates/verter_lsp/src/server_tests.rs:4886:    let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
crates/verter_lsp/src/server_tests.rs:4887:        crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/server_tests.rs:4938:    let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
crates/verter_lsp/src/server_tests.rs:4939:        crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/server_tests.rs:5854:    host.configure_projects(vec![crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/server_tests.rs:16232:    host.configure_projects(vec![crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/server_tests.rs:16689:    // the shared `WorkspaceProjectResolver` over this published vfs, so the owner key is
crates/verter_lsp/src/server_tests.rs:16694:        resolver: crate::project_resolver::NativeProjectResolver::new(vec![
crates/verter_lsp/src/server_tests.rs:16695:            crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/server_tests.rs:16836:        resolver: crate::project_resolver::NativeProjectResolver::new(vec![
crates/verter_lsp/src/server_tests.rs:16837:            crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/server_tests.rs:16952:    // vfs through the shared `WorkspaceProjectResolver`.
crates/verter_lsp/src/server_tests.rs:16956:        resolver: crate::project_resolver::NativeProjectResolver::new(vec![
crates/verter_lsp/src/server_tests.rs:16957:            crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/server_tests.rs:17237:        verter_workspace::IdeProjectCompilerOptions {
crates/verter_lsp/src/server_tests.rs:17343:                compiler_options: verter_workspace::IdeProjectCompilerOptions {
crates/verter_lsp/src/server_tests.rs:17359:        resolver: verter_workspace::ProjectResolver::new(Vec::new()),
crates/verter_lsp/src/server_tests.rs:17383:        resolver: verter_workspace::ProjectResolver::new(Vec::new()),
crates/verter_lsp/src/server_tests.rs:20185:    let mut project = crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/server_tests.rs:20190:    project.compiler_options = crate::project_resolver::IdeProjectCompilerOptions {
crates/verter_lsp/src/server_tests.rs:20222:        resolver: crate::project_resolver::NativeProjectResolver::new(vec![project]),
crates/verter_lsp/src/server_tests.rs:20240:        crate::project_resolver::ResolutionContext {
crates/verter_lsp/src/server_tests.rs:20241:            kind: crate::project_resolver::ResolveRequestKind::SfcSrcAttr,
crates/verter_lsp/src/server_tests.rs:20242:            phase: crate::project_resolver::ResolvePhase::CodegenBlocker,
crates/verter_lsp/src/server_tests.rs:20343:        resolver: crate::project_resolver::NativeProjectResolver::new(vec![
crates/verter_lsp/src/server_tests.rs:20344:            crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/server_tests.rs:25021:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/server_tests.rs:31143:            compiler_options: verter_workspace::IdeProjectCompilerOptions::default(),
crates/verter_lsp/src/server_tests.rs:31148:    let resolver = verter_workspace::ProjectResolver::new(vec![
crates/verter_lsp/src/server_tests.rs:31149:        crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/server_tests.rs:32901:        resolver: crate::project_resolver::NativeProjectResolver::new(vec![
crates/verter_lsp/src/server_tests.rs:32902:            crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/background_drain.rs:287:        let reader = LspProjectResolverReader::new(documents);
crates/verter_lsp/src/background_drain.rs:1451:    let reader = LspProjectResolverReader::new(documents);
crates/verter_lsp/src/server/workspace_symbol_frontier_tests.rs:139:    host.configure_projects(vec![crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/server/workspace_symbol_frontier_tests.rs:219:                compiler_options: verter_workspace::IdeProjectCompilerOptions::default(),
crates/verter_lsp/src/server/workspace_symbol_frontier_tests.rs:239:    let mut project_config = crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/server/workspace_symbol_frontier_tests.rs:245:        project_config.compiler_options = crate::project_resolver::IdeProjectCompilerOptions {
crates/verter_lsp/src/server/workspace_symbol_frontier_tests.rs:254:    let resolver = verter_workspace::ProjectResolver::new(vec![project_config]);
crates/verter_lsp/src/provider_sync_tests.rs:12:    let resolver = NativeProjectResolver::new(vec![
crates/verter_lsp/src/provider_sync_tests.rs:13:        crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/provider_sync_tests.rs:18:        crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/provider_sync_tests.rs:58:        NativeProjectResolver::new(vec![crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/config.rs:400:    pub membership: crate::project_resolver::ProjectMembership,
crates/verter_lsp/src/config.rs:402:    pub workspace_aliases: Vec<crate::project_resolver::WorkspaceAlias>,
crates/verter_lsp/src/config.rs:404:    pub compiler_options: crate::project_resolver::IdeProjectCompilerOptions,
crates/verter_lsp/src/config.rs:476:    pub fn to_ide_project_config(&self) -> crate::project_resolver::IdeProjectConfig {
crates/verter_lsp/src/config.rs:477:        let mut project = crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/config.rs:596:                                    crate::project_resolver::WorkspaceAlias {
crates/verter_lsp/src/config.rs:634:                                                crate::project_resolver::WorkspaceAlias {
crates/verter_lsp/src/config.rs:649:                                                crate::project_resolver::WorkspaceAlias {
crates/verter_lsp/src/config.rs:678:                membership: crate::project_resolver::ProjectMembership::MatchAll,
crates/verter_lsp/src/config.rs:680:                compiler_options: crate::project_resolver::IdeProjectCompilerOptions::default(),
crates/verter_lsp/src/config.rs:751:                membership: crate::project_resolver::ProjectMembership::MatchAll,
crates/verter_lsp/src/config.rs:753:                compiler_options: crate::project_resolver::IdeProjectCompilerOptions::default(),
crates/verter_lsp/src/config.rs:832:    pub fn to_native_project_resolver(&self) -> crate::project_resolver::NativeProjectResolver {
crates/verter_lsp/src/config.rs:833:        crate::project_resolver::NativeProjectResolver::new(
crates/verter_lsp/src/config.rs:885:        crate::project_resolver::ProjectMembership::MatchAll => true,
crates/verter_lsp/src/config.rs:886:        crate::project_resolver::ProjectMembership::IncludeExclude {
crates/verter_lsp/src/config.rs:1528:                membership: crate::project_resolver::ProjectMembership::MatchAll,
crates/verter_lsp/src/config.rs:1530:                compiler_options: crate::project_resolver::IdeProjectCompilerOptions::default(),
crates/verter_lsp/src/config.rs:1562:                    membership: crate::project_resolver::ProjectMembership::MatchAll,
crates/verter_lsp/src/config.rs:1564:                    compiler_options: crate::project_resolver::IdeProjectCompilerOptions::default(),
crates/verter_lsp/src/config.rs:1584:                    membership: crate::project_resolver::ProjectMembership::MatchAll,
crates/verter_lsp/src/config.rs:1586:                    compiler_options: crate::project_resolver::IdeProjectCompilerOptions::default(),
crates/verter_lsp/src/config.rs:1893:                membership: crate::project_resolver::ProjectMembership::MatchAll,
crates/verter_lsp/src/config.rs:1895:                compiler_options: crate::project_resolver::IdeProjectCompilerOptions::default(),
crates/verter_lsp/src/server/nav_features_rename_completeness_tests.rs:151:    host.configure_projects(vec![crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/provider_sync.rs:1:use crate::project_resolver::NativeProjectResolver;
crates/verter_lsp/src/provider_sync.rs:482:    resolver: &NativeProjectResolver,
crates/verter_lsp/src/provider_sync.rs:534:    resolver: &NativeProjectResolver,
crates/verter_lsp/src/server/provider_state.rs:542:        let reader = super::server_utils::LspProjectResolverReader::new(&self.documents);
crates/verter_lsp/src/workspace_scanner.rs:700:) -> Vec<crate::project_resolver::ResolveResult> {
crates/verter_lsp/src/workspace_scanner.rs:843:    initial_deps: Vec<crate::project_resolver::ResolveResult>,
crates/verter_lsp/src/workspace_scanner.rs:851:    let mut pending: Vec<crate::project_resolver::ResolveResult> = initial_deps;
crates/verter_lsp/src/workspace_scanner.rs:855:        if dep.provider_target == crate::project_resolver::ProviderTarget::CarrierPublicApi {
crates/verter_lsp/src/workspace_scanner.rs:861:        if dep.provider_target == crate::project_resolver::ProviderTarget::ShadowSourceFile {
crates/verter_lsp/src/workspace_scanner.rs:1658:        let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
crates/verter_lsp/src/workspace_scanner.rs:1659:            crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/workspace_scanner.rs:1664:            crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/workspace_scanner.rs:1744:        let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
crates/verter_lsp/src/workspace_scanner.rs:1745:            crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/workspace_scanner.rs:1845:        let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
crates/verter_lsp/src/workspace_scanner.rs:1846:            crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/workspace_scanner.rs:1851:            crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/workspace_scanner.rs:1944:        let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
crates/verter_lsp/src/workspace_scanner.rs:1945:            crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/workspace_scanner.rs:2179:            ProjectResolver, StaticMembershipSpec,
crates/verter_lsp/src/workspace_scanner.rs:2218:            resolver: ProjectResolver::default(),
crates/verter_lsp/src/workspace_scanner.rs:2238:        use verter_workspace::ProjectResolver;
crates/verter_lsp/src/workspace_scanner.rs:2243:            resolver: ProjectResolver::default(),
crates/verter_lsp/src/workspace_scanner.rs:2256:            CanonicalPath, ConfiguredMembership, ProjectResolver, StaticMembershipSpec,
crates/verter_lsp/src/workspace_scanner.rs:2278:            resolver: ProjectResolver::default(),
crates/verter_lsp/src/workspace_scanner.rs:2319:        let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
crates/verter_lsp/src/workspace_scanner.rs:2320:            crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/workspace_scanner.rs:2477:                compiler_options: verter_workspace::IdeProjectCompilerOptions::default(),
crates/verter_lsp/src/workspace_scanner.rs:2482:        let resolver = verter_workspace::ProjectResolver::new(vec![
crates/verter_lsp/src/workspace_scanner.rs:2483:            crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/server/import_publication.rs:324:        let reader = LspProjectResolverReader::new(&self.documents);
crates/verter_lsp/src/server/import_publication.rs:597:            let reader = LspProjectResolverReader::new(&self.documents);
crates/verter_lsp/src/server/mod.rs:199:    pub(crate) resolver: crate::project_resolver::NativeProjectResolver,
crates/verter_lsp/src/server/mod.rs:250:    pub(crate) resolved_dependencies: Vec<crate::project_resolver::ResolveResult>,
crates/verter_lsp/src/server/sync_orchestration.rs:635:        let reader = LspProjectResolverReader::new(&self.documents);
crates/verter_lsp/src/server/sync_orchestration.rs:666:        let reader = LspProjectResolverReader::new(&self.documents);
crates/verter_lsp/src/server/sync_orchestration.rs:738:                    == crate::project_resolver::ProviderTarget::CarrierPublicApi
crates/verter_lsp/src/server/sync_orchestration.rs:752:                    == crate::project_resolver::ProviderTarget::ShadowSourceFile
crates/verter_lsp/src/server/sync_orchestration.rs:754:                        == crate::project_resolver::ProviderTarget::SourceFile
crates/verter_lsp/src/server/sync_orchestration.rs:772:        let reader = LspProjectResolverReader::new(&self.documents);
crates/verter_lsp/src/server/sync_orchestration.rs:859:                    == crate::project_resolver::ProviderTarget::CarrierPublicApi
crates/verter_lsp/src/server/sync_orchestration.rs:864:                    == crate::project_resolver::ProviderTarget::ShadowSourceFile
crates/verter_lsp/src/server/sync_orchestration.rs:868:                    == crate::project_resolver::ProviderTarget::SourceFile
crates/verter_lsp/src/configured_owner_tests.rs:15:use crate::project_resolver::{IdeProjectConfig, NativeProjectResolver};
crates/verter_lsp/src/configured_owner_tests.rs:36:    let resolver = NativeProjectResolver::new(vec![
crates/verter_lsp/src/configured_owner_tests.rs:37:        IdeProjectConfig::new(
crates/verter_lsp/src/configured_owner_tests.rs:42:        IdeProjectConfig::new(
crates/verter_lsp/src/configured_owner_tests.rs:117:    let resolver = NativeProjectResolver::new(vec![IdeProjectConfig::new(
crates/verter_lsp/src/configured_owner_tests.rs:157:    let resolver = NativeProjectResolver::new(vec![IdeProjectConfig::new(
crates/verter_lsp/src/configured_owner_tests.rs:179:    let resolver = NativeProjectResolver::new(vec![IdeProjectConfig::new(
crates/verter_lsp/src/real_provider_tests/mod.rs:28://!   single `IdeProjectConfig`. The actual VS Code workspace would have multiple roots
crates/verter_lsp/src/test_utils.rs:31:    // through the shared `WorkspaceProjectResolver`; spec-bridge `include: {root}/**/*`
crates/verter_lsp/src/test_utils.rs:55:                compiler_options: verter_workspace::IdeProjectCompilerOptions::default(),
crates/verter_lsp/src/test_utils.rs:77:    let resolver = verter_workspace::ProjectResolver::new(vec![
crates/verter_lsp/src/test_utils.rs:78:        crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/test_utils.rs:144:                        compiler_options: verter_workspace::IdeProjectCompilerOptions::default(),
crates/verter_lsp/src/test_utils.rs:186:/// Create a test VFS workspace from a pre-built `NativeProjectResolver`.
crates/verter_lsp/src/test_utils.rs:190:    resolver: verter_workspace::ProjectResolver,
crates/verter_lsp/src/test_utils.rs:211:    resolver: verter_workspace::ProjectResolver,
crates/verter_lsp/src/test_utils.rs:220:            // carrier through the shared `WorkspaceProjectResolver`; the spec-bridge
crates/verter_lsp/src/test_utils.rs:246:                        compiler_options: verter_workspace::IdeProjectCompilerOptions::default(),
crates/verter_lsp/tests/cases/shared_provider_live.rs:47:    CarrierOwnershipResolution, EnvDims, ExternalTsProjectResolver, WorkspaceProjectResolver,
crates/verter_lsp/tests/cases/shared_provider_live.rs:473:/// `WorkspaceProjectResolver` over a real published fixture snapshot (the same
crates/verter_lsp/tests/cases/shared_provider_live.rs:489:    let resolver = WorkspaceProjectResolver::new(
crates/verter_lsp/tests/cases/shared_provider_live.rs:499:/// Resolve the carrier SOURCE through the PRODUCTION `WorkspaceProjectResolver` over a
crates/verter_lsp/tests/cases/shared_provider_live.rs:516:    let resolver = WorkspaceProjectResolver::new(
crates/verter_lsp/tests/cases/shared_provider_live.rs:1043:// `WorkspaceProjectResolver` over the host's LIVE published snapshot (never the
crates/verter_lsp/tests/cases/shared_provider_live.rs:1197:/// uses, so the SHARED route's ownership authority (`WorkspaceProjectResolver`) is
crates/verter_lsp/tests/cases/shared_provider_live.rs:1263:/// provider decides serving from the SAME `WorkspaceProjectResolver::resolve()`
crates/verter_lsp/tests/cases/shared_provider_live.rs:1283:    let resolver = WorkspaceProjectResolver::new(
crates/verter_lsp/tests/cases/shared_provider_live.rs:1364:/// REFERENCE-CLOSURE — the production `WorkspaceProjectResolver` resolves a
crates/verter_lsp/tests/cases/shared_provider_live.rs:1386:    let resolver = WorkspaceProjectResolver::new(
crates/verter_lsp/tests/cases/shared_provider_live.rs:1512:/// type, PROVING SHARED bound per query through the real `WorkspaceProjectResolver`
crates/verter_lsp/src/tsgo/composite.rs:10://! [`WorkspaceProjectResolver`](verter_session::external_ts::WorkspaceProjectResolver)
crates/verter_lsp/src/sync_coordinator_tests.rs:431:    let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
crates/verter_lsp/src/sync_coordinator_tests.rs:432:        crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/sync_coordinator_tests.rs:437:        crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/sync_coordinator_tests.rs:4734:    host.configure_projects(vec![crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/external_ts/publish_coordinator.rs:36:    CarrierOwnershipResolution, EngineBackend, EnvDims, ExternalTsProjectResolver, OpenState,
crates/verter_lsp/src/external_ts/publish_coordinator.rs:37:    ProjectBinding, ScriptKind, SnapshotFile, SnapshotRole, WorkspaceProjectResolver,
crates/verter_lsp/src/external_ts/publish_coordinator.rs:399:    /// negotiated `ts_version`: it builds the shared [`WorkspaceProjectResolver`] over
crates/verter_lsp/src/external_ts/publish_coordinator.rs:567:/// direct-open). Builds the shared [`WorkspaceProjectResolver`] over
crates/verter_lsp/src/external_ts/publish_coordinator.rs:587:    let resolver = WorkspaceProjectResolver::new(
crates/verter_lsp/src/external_ts/carrier_sync_tests.rs:16:    AmbiguityCause, CarrierOwnershipResolution, EnvDims, ExternalTsProjectResolver, ProjectBinding,
crates/verter_lsp/src/external_ts/carrier_sync_tests.rs:17:    WorkspaceProjectResolver,
crates/verter_lsp/src/external_ts/carrier_sync_tests.rs:40:use crate::project_resolver::{IdeProjectConfig, NativeProjectResolver};
crates/verter_lsp/src/external_ts/carrier_sync_tests.rs:1062:    let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
crates/verter_lsp/src/external_ts/carrier_sync_tests.rs:1063:        crate::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/external_ts/carrier_sync_tests.rs:1420:    let resolver = NativeProjectResolver::new(vec![IdeProjectConfig::new(
crates/verter_lsp/src/external_ts/carrier_sync_tests.rs:1549:    let resolver = NativeProjectResolver::new(vec![IdeProjectConfig::new(
crates/verter_lsp/src/external_ts/carrier_sync_tests.rs:1667:    let resolver = NativeProjectResolver::new(vec![IdeProjectConfig::new(
crates/verter_lsp/src/external_ts/carrier_sync_tests.rs:1777:    let resolver = NativeProjectResolver::new(vec![IdeProjectConfig::new(
crates/verter_lsp/src/external_ts/carrier_sync_tests.rs:1854:    let resolver = NativeProjectResolver::new(vec![IdeProjectConfig::new(
crates/verter_lsp/src/external_ts/carrier_sync_tests.rs:1984:    // carrier resolver (`WorkspaceProjectResolver::resolve`) BOTH derive from the
crates/verter_lsp/src/external_ts/carrier_sync_tests.rs:2008:    let resolver = WorkspaceProjectResolver::new(
crates/verter_lsp/src/external_ts/carrier_sync_tests.rs:2195:    let resolver = NativeProjectResolver::new(vec![
crates/verter_lsp/src/external_ts/carrier_sync_tests.rs:2196:        IdeProjectConfig::new(ws_root.clone(), ws_root.clone(), Some(tsconfig_a.clone())),
crates/verter_lsp/src/external_ts/carrier_sync_tests.rs:2197:        IdeProjectConfig::new(ws_root.clone(), ws_root.clone(), Some(tsconfig_b.clone())),
crates/verter_lsp/src/external_ts/carrier_sync_tests.rs:2315:    let resolver = NativeProjectResolver::new(vec![IdeProjectConfig::new(
crates/verter_lsp/src/external_ts/carrier_sync.rs:48:use crate::project_resolver::NativeProjectResolver;
crates/verter_lsp/src/external_ts/carrier_sync.rs:99:    pub resolver: &'a NativeProjectResolver,
crates/verter_lsp/src/external_ts/carrier_sync.rs:1458:    resolver: &NativeProjectResolver,
crates/verter_lsp/src/external_ts/carrier_sync.rs:1496:    resolver: &NativeProjectResolver,
crates/verter_lsp/src/external_ts/membership_reconciler.rs:8://!    once by the caller through the shared `WorkspaceProjectResolver` — and routes
crates/verter_lsp/src/external_ts/membership_reconciler.rs:128:/// [`WorkspaceProjectResolver`](verter_session::external_ts::WorkspaceProjectResolver)
crates/verter_lsp/src/external_ts/membership_reconciler.rs:416:/// `WorkspaceProjectResolver`).
crates/verter_lsp/src/external_ts/membership_reconciler.rs:790:    /// shared `WorkspaceProjectResolver`) and hands it here — the reconciler never
crates/verter_lsp/src/tsgo/project_binding.rs:9://! shared [`WorkspaceProjectResolver`], minting the witness from the resolved
crates/verter_lsp/src/tsgo/project_binding.rs:23:    ExternalTsProjectResolver, ProjectBinding, WorkspaceProjectResolver,
crates/verter_lsp/src/tsgo/project_binding.rs:122:/// snapshot through the shared [`WorkspaceProjectResolver`], returning the FULL
crates/verter_lsp/src/tsgo/project_binding.rs:173:    let resolver = WorkspaceProjectResolver::new(
crates/verter_lsp/src/tsgo/project_binding.rs:207:/// Published-snapshot → [`WorkspaceProjectResolver`] → `resolve(source)` → on
crates/verter_session/src/cross_file.rs:634:            use verter_semantic::analysis::project_resolver::*;
crates/verter_semantic/src/analysis/project_resolver.rs:14:// `verter_semantic::analysis::project_resolver::*` unchanged.
crates/verter_lsp/src/project_resolver.rs:1:pub use verter_semantic::analysis::project_resolver::*;

exec
/bin/zsh -lc "rg -n \"mod project_resolver|pub mod project_resolver\" crates/verter_lsp/src/lib.rs crates/verter_lsp/src/main.rs crates/verter_lsp/src/*.rs && rg -n \"collect_resolvable_module_reference_specifiers|resolve_known_module_reference_dependencies\" crates/verter_lsp crates/verter_napi crates/verter_wasm crates/verter_session --glob '*.rs' && rg -n \"verter_semantic::analysis::project_resolver::\" crates --glob '*.rs'" in <block worktree root>
 succeeded in 364ms:
crates/verter_lsp/src/lib.rs:92:pub mod project_resolver;
crates/verter_lsp/src/lib.rs:92:pub mod project_resolver;
crates/verter_wasm/src/lib.rs:631:    pub fn collect_resolvable_module_reference_specifiers(
crates/verter_wasm/src/lib.rs:640:            verter_semantic::analysis::project_resolver::collect_resolvable_module_reference_specifiers(
crates/verter_wasm/src/lib.rs:649:    pub fn resolve_known_module_reference_dependencies(
crates/verter_wasm/src/lib.rs:667:            verter_semantic::analysis::project_resolver::resolve_known_module_reference_dependencies(
crates/verter_napi/src/lib.rs:2093:    pub fn collect_resolvable_module_reference_specifiers(
crates/verter_napi/src/lib.rs:2102:            verter_semantic::analysis::project_resolver::collect_resolvable_module_reference_specifiers(
crates/verter_napi/src/lib.rs:2111:    pub fn resolve_known_module_reference_dependencies(
crates/verter_napi/src/lib.rs:2124:            verter_semantic::analysis::project_resolver::resolve_known_module_reference_dependencies(
crates/verter_napi/src/meta.rs:165:            let configs: Vec<verter_semantic::analysis::project_resolver::IdeProjectConfig> =
crates/verter_napi/src/lib.rs:1332:) -> verter_semantic::analysis::project_resolver::IdeProjectConfig {
crates/verter_napi/src/lib.rs:1333:    let mut ide = verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_napi/src/lib.rs:1345:                |a| verter_semantic::analysis::project_resolver::WorkspaceAlias {
crates/verter_napi/src/lib.rs:1596:            let configs: Vec<verter_semantic::analysis::project_resolver::IdeProjectConfig> =
crates/verter_napi/src/lib.rs:2102:            verter_semantic::analysis::project_resolver::collect_resolvable_module_reference_specifiers(
crates/verter_napi/src/lib.rs:2124:            verter_semantic::analysis::project_resolver::resolve_known_module_reference_dependencies(
crates/verter_napi/src/lib.rs:2171:            let configs: Vec<verter_semantic::analysis::project_resolver::IdeProjectConfig> =
crates/verter_bench/benches/repo_warm_second_pass.rs:32:use verter_semantic::analysis::project_resolver::IdeProjectConfig;
crates/verter_bench/benches/cache_baseline.rs:120:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_bench/benches/repo_first_pass_loop.rs:42:use verter_semantic::analysis::project_resolver::IdeProjectConfig;
crates/verter_bench/examples/profile_component_meta.rs:432:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_semantic/src/analysis/project_resolver.rs:14:// `verter_semantic::analysis::project_resolver::*` unchanged.
crates/verter_wasm/src/lib.rs:640:            verter_semantic::analysis::project_resolver::collect_resolvable_module_reference_specifiers(
crates/verter_wasm/src/lib.rs:667:            verter_semantic::analysis::project_resolver::resolve_known_module_reference_dependencies(
crates/verter_session/src/typeinfo/typeinfo_tests/vue_import_recursion.rs:61:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/project_resolver.rs:1:pub use verter_semantic::analysis::project_resolver::*;
crates/verter_session/src/typeinfo/typeinfo_tests/cache_invalidation.rs:363:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/typeinfo/framework_surface/svelte_exec_tests.rs:481:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_lsp/src/server_tests.rs:25021:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/meta_resolve/projectors_silent_miss_tests.rs:60:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/meta_resolve/callable_view_tests.rs:1216:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/resolver_core/component_meta_query_engine/tests.rs:1934:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/tests/cases/g_misc2/per_macro_projector_decomposition_tests.rs:78:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/tests/cases/g_misc2/per_macro_projector_decomposition_tests.rs:106:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/component_meta_canonical_reuse_tests.rs:55:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/component_meta_canonical_reuse_tests.rs:547:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/component_meta_host.rs:244:        configs: Vec<verter_semantic::analysis::project_resolver::IdeProjectConfig>,
crates/verter_session/src/component_meta_component_config_fast_path_tests.rs:37:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/tests/cases/g_misc2/projector_decomposition_warm_cold_benchmark.rs:60:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/frontier_tests.rs:244:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/meta_tests.rs:8483:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/meta_tests.rs:8893:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/meta_tests.rs:10531:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/meta_tests.rs:10948:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/meta_tests.rs:11096:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/meta_tests.rs:11201:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/meta_tests.rs:11308:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/meta_tests.rs:21977:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/meta_tests.rs:22081:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/meta_tests.rs:22690:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/svelte_vertical_tests.rs:74:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/meta_resolve_tests.rs:477:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/meta_resolve_tests.rs:572:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/meta_resolve_tests.rs:4535:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/meta_resolve_tests.rs:4597:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/component_meta_owner_local_registry_route_tests.rs:46:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/tests/cases/g_misc2/per_macro_projector_native_payload_parity.rs:44:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/host_manage_tests.rs:3794:            verter_semantic::analysis::project_resolver::IdeProjectConfig {
crates/verter_session/src/host_manage_tests.rs:3804:                    verter_semantic::analysis::project_resolver::IdeProjectCompilerOptions::default(
crates/verter_session/src/host_manage_tests.rs:5380:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/host_manage_tests.rs:10606:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/host_manage_tests.rs:10670:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/host_manage_tests.rs:10732:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/host_manage_tests.rs:10805:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/host_manage_tests.rs:10889:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/host_manage_tests.rs:10979:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/host_manage_tests.rs:11031:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/host_manage_tests.rs:11106:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/host_manage_tests.rs:11184:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/host_manage_tests.rs:11260:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/host_manage_tests.rs:11343:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/cross_file.rs:634:            use verter_semantic::analysis::project_resolver::*;
crates/verter_session/src/host_lifecycle.rs:654:        projects: Vec<verter_semantic::analysis::project_resolver::IdeProjectConfig>,
crates/verter_session/src/parse_env_asymmetry_tests.rs:73:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/parse_env_asymmetry_tests.rs:78:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/overlay_artifact_key_isolation_tests.rs:67:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/component_meta_indexed_access_early_out_tests.rs:51:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/component_meta_indexed_access_early_out_tests.rs:449:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/component_meta_concurrency_tests.rs:53:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/component_meta_repo_first_pass_diagnosis_tests.rs:61:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/meta.rs:304:        projects: Vec<verter_semantic::analysis::project_resolver::IdeProjectConfig>,
crates/verter_session/src/host_resolve_tests.rs:1391:        let mut cfg = verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/host_resolve_tests.rs:2669:        verter_semantic::analysis::project_resolver::IdeProjectConfig {
crates/verter_session/src/host_resolve_tests.rs:2679:                verter_semantic::analysis::project_resolver::IdeProjectCompilerOptions::default(),
crates/verter_session/src/host_resolve_tests.rs:4700:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/host_resolve_tests.rs:4796:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/host_resolve_tests.rs:4863:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/host_resolve_tests.rs:4961:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/lib_tests.rs:2170:) -> verter_semantic::analysis::project_resolver::IdeProjectConfig {
crates/verter_session/src/lib_tests.rs:2171:    let mut config = verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/lib_tests.rs:2706:    use verter_semantic::analysis::project_resolver::IdeProjectConfig;
crates/verter_session/src/component_meta_invalidation_tests.rs:95:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/tests/cases/g_misc3/repo_first_pass_diagnosis_corpus.rs:196:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/cold_artifact_dedup_tests.rs:620:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/cold_artifact_dedup_tests.rs:1907:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/src/external_ts/resolver.rs:5://! `ProjectResolver` — `verter_semantic::analysis::project_resolver::ProjectResolver`
crates/verter_session/src/external_ts/mod.rs:53:// `verter_semantic::analysis::project_resolver::ProjectResolver`. The resolver
crates/verter_session/tests/cases/g_block/block_6i_round7_slots_unresolved_diagnostic.rs:53:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/tests/cases/g_cache/cache_invariant_migration.rs:83:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/tests/cases/g_cache/cache_invariant_migration.rs:236:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/tests/cases/g_cache/cache_invariant_migration.rs:336:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/tests/cases/g_block/block_6i_round7_emits_unresolved_diagnostic.rs:70:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
crates/verter_session/tests/cases/defect_b_corpus_prevention_gate.rs:289:        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(

exec
/bin/zsh -lc "rg -n \"impl(?:<'["'^>]*>)?'"\\s+(?:resolver_context::)?sealed::Sealed\\s+for|impl(?:<'["'^>]*>)?'"\\s+sealed::Sealed\\s+for|impl(?:<'["'^>]*>)?'"\\s+ResolverContext\\s+for|RequestBoundSealed\\s+for\" crates/verter_session/src --glob '*.rs' && nl -ba crates/verter_session/src/resolver_core/resolver_context.rs | sed -n '65,115p;150,290p;675,810p;805,850p' && nl -ba crates/verter_session/src/resolver_core/host_resolver_context.rs | sed -n '170,205p' && nl -ba crates/verter_session/src/resolver_core/session_resolver_context.rs | sed -n '165,200p'" in <block worktree root>
 succeeded in 1389ms:
crates/verter_session/src/fact_signature_helpers.rs:760:    // overlay+base). Bare-host contexts (`impl ResolverContext for
crates/verter_session/src/request_context.rs:579:/// to the bare `impl ResolverContext for VerterHost` rail).
crates/verter_session/src/cache_runtime/world_snapshot.rs:239:    //!    bare-host `impl ResolverContext for VerterHost` rail.
crates/verter_session/src/cache_runtime/world_snapshot.rs:516:        // `impl ResolverContext for VerterHost` rail directly.
crates/verter_session/src/host_resolve/frontier_engine.rs:735:    /// rebuild on `impl ResolverContext for VerterHost`.
crates/verter_session/src/host_resolve/frontier_engine.rs:795:    /// arm on `impl ResolverContext for VerterHost` reaches this
crates/verter_session/src/host_manage/prepared_decl.rs:3274:    /// `impl ResolverContext for VerterHost` reaches this wrapper on
crates/verter_session/src/host_manage/imported_type_root.rs:23:    /// `impl ResolverContext for VerterHost` reaches this wrapper on
crates/verter_session/src/project_semantic_dispatch/output_materialization.rs:270:    impl sealed::Sealed for crate::meta_resolve::projectors::MetaResolveProjectorsOutputCap<'_, '_> {}
crates/verter_session/src/project_semantic_dispatch/output_materialization.rs:277:    impl sealed::Sealed for crate::meta_resolve::materialize::MetaResolveFieldTypesOutputCap<'_, '_> {}
crates/verter_session/src/project_semantic_dispatch/output_materialization.rs:284:    impl sealed::Sealed for crate::typeinfo::raise::TypeinfoRaiseOutputCap<'_, '_> {}
crates/verter_session/src/project_semantic_dispatch/output_materialization.rs:368:    impl sealed::Sealed for TestOutputCap<'_, '_> {}
crates/verter_session/src/project_semantic_dispatch/mod.rs:504:    /// type-erasure at the call site because `impl ResolverContext for
crates/verter_session/src/project_semantic_dispatch/mod.rs:3801:    /// upcast implicitly because `impl ResolverContext for VerterHost`.
crates/verter_session/src/resolver_core/resolver_context.rs:169:    /// `impl ResolverContext for VerterHost` rail, which rebuilds an
crates/verter_session/src/resolver_core/resolver_context.rs:439:    /// The bare `impl ResolverContext for VerterHost::store_view` panics —
crates/verter_session/src/resolver_core/resolver_context.rs:592:    // `impl ResolverContext for VerterHost` definitions do not need
crates/verter_session/src/resolver_core/resolver_context.rs:756:    /// `impl ResolverContext for VerterHost` cannot satisfy — it owns no
crates/verter_session/src/resolver_core/resolver_context.rs:778:impl sealed::Sealed for crate::VerterHost {}
crates/verter_session/src/resolver_core/resolver_context.rs:779:impl<'a> sealed::Sealed for crate::resolver_core::host_resolver_context::HostResolverContext<'a> {}
crates/verter_session/src/resolver_core/resolver_context.rs:794:/// (`impl ResolverContext for VerterHost`) is UNCONSTRUCTIBLE at the type
crates/verter_session/src/resolver_core/resolver_context.rs:850:impl ResolverContext for crate::VerterHost {
crates/verter_session/src/resolver_core/resolver_context.rs:986:        // The bare `impl ResolverContext for VerterHost` cannot satisfy
crates/verter_session/src/semantic_query/deferred_callable.rs:51:impl sealed::Sealed for ResolveOverloadSetConsumer {}
crates/verter_session/src/semantic_query/deferred_callable.rs:52:impl sealed::Sealed for ResolveCallConsumer {}
crates/verter_session/src/resolver_core/session_resolver_context.rs:5://! The base `impl ResolverContext for VerterHost` returns
crates/verter_session/src/resolver_core/session_resolver_context.rs:183:impl<'a> ResolverContext for SessionResolverContext<'a> {
crates/verter_session/src/resolver_core/host_resolver_context.rs:4://! ## Why a separate wrapper (rather than `impl ResolverContext for VerterHost`)
crates/verter_session/src/resolver_core/host_resolver_context.rs:6://! The bare `impl ResolverContext for VerterHost` cannot satisfy the
crates/verter_session/src/resolver_core/host_resolver_context.rs:189:impl<'a> ResolverContext for HostResolverContext<'a> {
crates/verter_session/src/locator_identity.rs:101:impl sealed::Sealed for ParseEnvHash {}
crates/verter_session/src/locator_identity.rs:114:impl sealed::Sealed for ResolveEnvHash {}
crates/verter_session/src/locator_identity.rs:127:impl sealed::Sealed for TypeEnvHash {}
crates/verter_session/src/locator_identity.rs:140:impl sealed::Sealed for LibEnvHash {}
crates/verter_session/src/locator_identity.rs:163:impl sealed::Sealed for ProjectIdentityDim {}
    65	use verter_semantic::analysis::type_solver::{PreparedTypeDecl, PreparedValueDecl};
    66	use verter_workspace::{AmbientSymbolHit, ProjectStableKey};
    67	
    68	use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    69	use crate::project_type_store::{IndexedReady, ProjectTypeStore};
    70	use crate::resolver_core::fact_tracer_tls;
    71	use crate::resolver_core::prepared_decl::PreparedDeclBundle;
    72	use crate::resolver_core::ValueDeclIdentity;
    73	use crate::resolver_core::{FactVersionRef, ShallowFileState};
    74	use crate::resolver_store::HostStoreView;
    75	use crate::semantic_query::{SemanticNodeData, SemanticNodeId};
    76	use crate::types::Hash16;
    77	use crate::FileAnalysisSnapshot;
    78	use crate::HostConfig;
    79	
    80	/// Private markers used to seal `ResolverContext` (and its request-bound
    81	/// refinement) against external implementations.
    82	mod sealed {
    83	    /// Marker trait `ResolverContext` is sealed against. Only types
    84	    /// inside `verter_session` that implement this marker can implement
    85	    /// `ResolverContext`. Today the implementers are `VerterHost` and the
    86	    /// two request-bound contexts.
    87	    pub trait Sealed {}
    88	
    89	    /// Narrower marker sealing [`super::RequestBoundResolverContext`].
    90	    ///
    91	    /// Implemented ONLY for the two genuinely request-bound contexts
    92	    /// (`HostResolverContext`, `SessionResolverContext`) — NEVER for the
    93	    /// bare-host `VerterHost`. Because the marker trait carries this as a
    94	    /// supertrait bound, a non-request-bound context cannot be laundered
    95	    /// into `RequestBoundResolverContext` even from inside this crate
    96	    /// without adding a visible, reviewable `impl RequestBoundSealed`
    97	    /// here, and no external crate can add one at all.
    98	    pub trait RequestBoundSealed {}
    99	}
   100	
   101	/// A single, tear-free observation of a materialize-memo scope's
   102	/// content identity.
   103	///
   104	/// The materialize-memo publish site
   105	/// (`meta_resolve/materialize/field_types.rs`) needs the scope's
   106	/// content version for two distinct consumers that MUST agree:
   107	///
   108	/// 1. the `NodeScopeId::File { whole_hash }` the materialiser lowers
   109	///    the `TypeExpr` against — the lowered value's semantic identity;
   110	/// 2. the `MaterializeMemoDb` entry's fact-signature self-root — the
   111	///    view-correct shared-cache admission gate.
   112	///
   113	/// Sourcing those from two separate oracles (`shallow_file_state` for
   114	/// the scope id, `authoritative_current_content_hash` for the
   115	/// signature) can tear: an edit landing between the two reads roots a
   150	/// Restricted host facade for resolver-tier code (`resolver_core/*`,
   151	/// `meta_resolve/*` post-moves, `component_meta_caches.rs`,
   152	/// `component_meta_materialize.rs`, `project_semantic_dispatch/*`).
   153	///
   154	/// `ResolverContext` is the only way for seal-scope code to reach host
   155	/// state at runtime. It is a **flat trait** with no super-traits — see
   156	/// the module-level rationale.
   157	///
   158	/// Visibility is `pub(crate)` because this is purely an internal seal — no
   159	/// external integrators construct
   160	/// `&dyn ResolverContext`.
   161	pub(crate) trait ResolverContext: sealed::Sealed {
   162	    // -------- Identity --------------------------------------------
   163	
   164	    /// `true` when this context is request-bound — i.e. a
   165	    /// [`crate::resolver_core::HostResolverContext`] or
   166	    /// [`crate::resolver_core::SessionResolverContext`] backed by a
   167	    /// per-request [`HostStoreView`] (and overlay) constructed at the
   168	    /// request entry boundary. `false` for the bare-host
   169	    /// `impl ResolverContext for VerterHost` rail, which rebuilds an
   170	    /// owned view on every `resolver_store_view()` / `store_view()`
   171	    /// call (retired from production resolver-tier code — only the
   172	    /// request-bound rails remain as live entry points).
   173	    ///
   174	    /// Used by `ComponentMetaQueryEngine::new` to bump the
   175	    /// `bare_engine_constructions` diagnostic counter whenever the
   176	    /// engine is bound to a non-request-bound ctx — the empirical
   177	    /// signal the 3-way consult identified as the residual perf-gap
   178	    /// suspect.
   179	    fn is_request_bound(&self) -> bool {
   180	        false
   181	    }
   182	
   183	    /// Cancellation authority for the current semantic execution. A
   184	    /// scheduler-owned aggregate token with registered owners takes precedence
   185	    /// over the request token so cancelling one singleflight waiter never
   186	    /// poisons a live sibling's shared compute. Ordinary ownerless DAG stages
   187	    /// fall back to their installed request token.
   188	    fn cancellation_token(&self) -> Option<verter_scheduler::cancellation::CancellationToken> {
   189	        let job = verter_scheduler::cancellation::current_job_cancellation_token();
   190	        if job
   191	            .as_ref()
   192	            .is_some_and(|token| token.has_registered_owners())
   193	        {
   194	            return job;
   195	        }
   196	        crate::request_context::current_request_cancellation_token().or(job)
   197	    }
   198	
   199	    /// Cheap cancellation checkpoint used at semantic dispatch/work charges.
   200	    fn is_cancelled(&self) -> bool {
   201	        self.cancellation_token()
   202	            .is_some_and(|token| token.is_cancelled())
   203	    }
   204	
   205	    // -------- Cache accessors --------------------------------------
   206	
   207	    fn prepared_decl_bundle(&self, canonical_id: &str) -> Option<Arc<PreparedDeclBundle>>;
   208	
   209	    fn prepared_type_decl(
   210	        &self,
   211	        canonical_id: &str,
   212	        owner: verter_type_expr::TopLevelOwnerId,
   213	        symbol_name: &str,
   214	    ) -> Result<
   215	        Option<Arc<PreparedTypeDecl>>,
   216	        crate::resolver_core::prepared_decl::PreparationFailure,
   217	    >;
   218	
   219	    /// Consume a typed preparation failure as a ReturnOnly absence at an
   220	    /// Option-shaped semantic boundary. The failure stays explicit through
   221	    /// [`Self::prepared_type_decl`]; this adapter is the sole lossy boundary
   222	    /// and taints every enclosing cacheability scope before returning `None`.
   223	    fn prepared_type_decl_return_only(
   224	        &self,
   225	        canonical_id: &str,
   226	        owner: verter_type_expr::TopLevelOwnerId,
   227	        symbol_name: &str,
   228	    ) -> Option<Arc<PreparedTypeDecl>> {
   229	        match self.prepared_type_decl(canonical_id, owner, symbol_name) {
   230	            Ok(decl) => decl,
   231	            Err(failure) => {
   232	                note_non_cacheable_read_fan_out(NonCacheableReadReason::PreparationFailure);
   233	                tracing::error!(
   234	                    canonical_id,
   235	                    ?owner,
   236	                    symbol_name,
   237	                    ?failure,
   238	                    "prepared type declaration failed; serving ReturnOnly absence"
   239	                );
   240	                None
   241	            }
   242	        }
   243	    }
   244	
   245	    fn prepared_value_decl(
   246	        &self,
   247	        canonical_id: &str,
   248	        owner: verter_type_expr::TopLevelOwnerId,
   249	        symbol_name: &str,
   250	    ) -> Result<
   251	        Option<Arc<PreparedValueDecl>>,
   252	        crate::resolver_core::prepared_decl::PreparationFailure,
   253	    >;
   254	
   255	    /// Consume a typed preparation failure as a ReturnOnly absence at an
   256	    /// Option-shaped semantic boundary — the value-space mirror of
   257	    /// [`Self::prepared_type_decl_return_only`]. The failure stays explicit
   258	    /// through [`Self::prepared_value_decl`]; this adapter is the sole lossy
   259	    /// boundary and taints every enclosing cacheability scope before
   260	    /// returning `None`. Callers that must preserve the `Failed` distinction
   261	    /// (the `defineExpose` admission gate) call [`Self::prepared_value_decl`]
   262	    /// directly instead.
   263	    fn prepared_value_decl_return_only(
   264	        &self,
   265	        canonical_id: &str,
   266	        owner: verter_type_expr::TopLevelOwnerId,
   267	        symbol_name: &str,
   268	    ) -> Option<Arc<PreparedValueDecl>> {
   269	        match self.prepared_value_decl(canonical_id, owner, symbol_name) {
   270	            Ok(decl) => decl,
   271	            Err(failure) => {
   272	                note_non_cacheable_read_fan_out(NonCacheableReadReason::PreparationFailure);
   273	                tracing::error!(
   274	                    canonical_id,
   275	                    ?owner,
   276	                    symbol_name,
   277	                    ?failure,
   278	                    "prepared value declaration failed; serving ReturnOnly absence"
   279	                );
   280	                None
   281	            }
   282	        }
   283	    }
   284	
   285	    /// Materialise (or warm-read) the canonical post-parse artifact,
   286	    /// with the publication status flowed BY VALUE — see
   287	    /// [`crate::host_manage::prepared_decl::IndexedReadyServe`]. This is
   288	    /// the ONLY resolver-tier accessor for a cold/warm `IndexedReady`:
   289	    /// a consumer that derives shared-cache entries from the artifact
   290	    /// gates admission on `serve.store_published`; structurally
   675	    /// The default impl returns `None`; overlay-bearing session contexts
   676	    /// override this to return their `SessionView` so resolver-tier
   677	    /// helpers can read overlay content without carrying an explicit
   678	    /// view parameter. Default is reached today through the
   679	    /// `for_tests::active_session_view_is_none_for_tests` shim in
   680	    /// `lib.rs` (see `tests/cases/g_misc0/resolver_context_active_session_view.rs`).
   681	    fn active_session_view(&self) -> Option<&dyn crate::session_view::SessionView> {
   682	        None
   683	    }
   684	
   685	    /// Return the request-scoped
   686	    /// [`CanonicalCompletionOverlay`](crate::resolver_core::CanonicalCompletionOverlay)
   687	    /// this context threads through the request, if any.
   688	    ///
   689	    /// The overlay is the per-request carrier (constructed once at the
   690	    /// request boundary, shared across every context the request builds,
   691	    /// dropped with the request — R18-compliant: passed by explicit
   692	    /// argument, never via a thread-local). The prepared-decl producers
   693	    /// consult it for the request-world bundle memo
   694	    /// (`CanonicalCompletionOverlay::bundle_memo`) — the R17-compliant
   695	    /// home for values that must never enter host/shared caches.
   696	    ///
   697	    /// The default impl returns `None`: a context with no request scope
   698	    /// (the bare host) owns no request world, so it can reuse nothing
   699	    /// beyond what the shared cache already holds. Both request-bound
   700	    /// contexts override it to expose their request overlay.
   701	    fn request_completion_overlay(
   702	        &self,
   703	    ) -> Option<&crate::resolver_core::CanonicalCompletionOverlay> {
   704	        None
   705	    }
   706	
   707	    /// Rewrite a raw canonical to its analysis canonical — the identity
   708	    /// every `FileArtifactStore` artifact (base and overlay) is keyed by.
   709	    ///
   710	    /// A raw canonical has two forms: the form the session edited /
   711	    /// requested, and the `normalized_analysis_canonical` rewrite (a
   712	    /// runtime `.js` whose `.d.ts` companion is the analysis target). The
   713	    /// two coincide for an ordinary `.ts` / `.tsx` / `.d.ts` file. The
   714	    /// overlay materialiser publishes under the normalised id, and the
   715	    /// base [`Self::ensure_indexed_ready_serve`] normalises before publishing,
   716	    /// so `FileArtifactKey::canonical` is always the normalised id.
   717	    ///
   718	    /// Content-addressed `FileArtifactStore` lookups (parse-fact
   719	    /// recovery in particular) MUST normalise the canonical before
   720	    /// keying the store — a raw-keyed lookup misses the artifact
   721	    /// whenever `normalize(raw) != raw`. The default impl delegates to
   722	    /// [`crate::VerterHost::normalized_analysis_canonical`]; both
   723	    /// implementers ([`crate::VerterHost`] and the overlay-aware
   724	    /// `SessionResolverContext`) resolve through the same host method.
   725	    fn normalized_analysis_canonical<'a>(
   726	        &self,
   727	        raw_canonical: &'a str,
   728	    ) -> std::borrow::Cow<'a, str> {
   729	        self.host_for_fact_tracer_install()
   730	            .normalized_analysis_canonical(raw_canonical)
   731	    }
   732	
   733	    /// Reach the concrete `VerterHost` underneath this context.
   734	    ///
   735	    /// Used by Family B/C/D producers (`MaterializeStructureDb`,
   736	    /// `AppConfigNoOverrideProofDb`,
   737	    /// `OwnerImportSurfaceDb`) to call
   738	    /// [`crate::VerterHost::with_fact_tracer`] from inside their
   739	    /// cooperative-admission cold-compute closures. The seal trait
   740	    /// itself cannot expose `with_fact_tracer` directly because
   741	    /// `FnOnce<R>` is non-dyn-compatible; this accessor lets
   742	    /// cold-compute closures install the tracer through the existing
   743	    /// `fact_signature_helpers::install_fact_tracer(host, ...)`
   744	    /// surface without bypassing the seal.
   745	    ///
   746	    /// Both production implementers ([`crate::VerterHost`] and
   747	    /// [`crate::resolver_core::session_resolver_context::SessionResolverContext`])
   748	    /// return their inner `&crate::VerterHost`. There is no other
   749	    /// implementer; the seal guarantees the trait contract.
   750	    fn host_for_fact_tracer_install(&self) -> &crate::VerterHost;
   751	
   752	    /// This context's contribution to a fact tracer's compaction basis.
   753	    ///
   754	    /// Deliberately NOT `self.store_view().aggregate_basis_seed()` at the
   755	    /// call site. [`Self::store_view`] is a BORROW contract that the bare
   756	    /// `impl ResolverContext for VerterHost` cannot satisfy — it owns no
   757	    /// view, so its implementation is an architectural panic in
   758	    /// production. The tracer chokepoint runs on EVERY cold compute,
   759	    /// including the ones still reached through a bare host, so reaching
   760	    /// `store_view()` from there would turn that guard into a live crash.
   761	    ///
   762	    /// So the projection is a context-level question with a fail-safe
   763	    /// default: a context that is not request-bound vouches for nothing,
   764	    /// its scopes compact nothing and detect no movement. The two
   765	    /// request-bound implementers override it by forwarding the view they
   766	    /// already hold — a borrow, never a `StoreViewManager` read.
   767	    #[inline]
   768	    fn aggregate_basis_seed(&self) -> verter_workspace::AggregateBasisSeed {
   769	        verter_workspace::AggregateBasisSeed::Unvouched
   770	    }
   771	}
   772	
   773	// Sealed marker — `VerterHost` is the base implementer,
   774	// `HostResolverContext` is the request-bound wrapper that carries a
   775	// borrowed `HostStoreView`, and `SessionResolverContext` is the
   776	// overlay-aware wrapper that delegates every method to a borrowed host
   777	// alongside an overlay-rooted view.
   778	impl sealed::Sealed for crate::VerterHost {}
   779	impl<'a> sealed::Sealed for crate::resolver_core::host_resolver_context::HostResolverContext<'a> {}
   780	impl<'a> sealed::Sealed
   781	    for crate::resolver_core::session_resolver_context::SessionResolverContext<'a>
   782	{
   783	}
   784	
   785	/// Sealed marker subtrait: a [`ResolverContext`] that is genuinely
   786	/// REQUEST-BOUND — it carries a per-request [`HostStoreView`] (and, for a
   787	/// session query, an overlay) constructed at the request entry boundary,
   788	/// so [`ResolverContext::is_request_bound`] is `true` and every artifact
   789	/// serve is view-correct for the requesting caller.
   790	///
   791	/// This is the STRUCTURAL rail behind
   792	/// [`crate::query_host_port::SessionQueryHostPort::new`]: the port binds a
   793	/// `&dyn RequestBoundResolverContext`, so the retired bare-host rail
   794	/// (`impl ResolverContext for VerterHost`) is UNCONSTRUCTIBLE at the type
   795	/// level — `VerterHost` implements [`ResolverContext`] but NOT this
   796	/// marker, and therefore cannot coerce to `&dyn RequestBoundResolverContext`.
   797	/// The runtime `is_request_bound` check the port formerly asserted is now
   798	/// redundant defense-in-depth.
   799	///
   800	/// Sealed via [`sealed::RequestBoundSealed`], implemented ONLY for
   801	/// [`crate::resolver_core::HostResolverContext`] and
   802	/// [`crate::resolver_core::SessionResolverContext`]. It is NEVER
   803	/// implemented for [`crate::VerterHost`]; the seal makes an external or
   804	/// in-crate-laundered non-request-bound implementer impossible without a
   805	/// visible `impl RequestBoundSealed`.
   805	/// visible `impl RequestBoundSealed`.
   806	///
   806	///
   807	/// The marker deliberately does NOT distinguish a base
   807	/// The marker deliberately does NOT distinguish a base
   808	/// [`crate::resolver_core::HostResolverContext`] from an overlay
   808	/// [`crate::resolver_core::HostResolverContext`] from an overlay
   809	/// [`crate::resolver_core::SessionResolverContext`] — both are
   809	/// [`crate::resolver_core::SessionResolverContext`] — both are
   810	/// request-bound. Overlay-vs-base correctness stays the caller's
   810	/// request-bound. Overlay-vs-base correctness stays the caller's
   811	/// obligation (chosen at the request entry) and its regression coverage is
   812	/// tracked separately.
   813	pub(crate) trait RequestBoundResolverContext:
   814	    ResolverContext + sealed::RequestBoundSealed
   815	{
   816	}
   817	
   818	// The request-bound seal: `RequestBoundSealed` (and hence
   819	// `RequestBoundResolverContext`) is implemented for the two genuinely
   820	// request-bound contexts ONLY, and NEVER for the bare-host `VerterHost`.
   821	impl<'a> sealed::RequestBoundSealed
   822	    for crate::resolver_core::host_resolver_context::HostResolverContext<'a>
   823	{
   824	}
   825	impl<'a> sealed::RequestBoundSealed
   826	    for crate::resolver_core::session_resolver_context::SessionResolverContext<'a>
   827	{
   828	}
   829	impl<'a> RequestBoundResolverContext
   830	    for crate::resolver_core::host_resolver_context::HostResolverContext<'a>
   831	{
   832	}
   833	impl<'a> RequestBoundResolverContext
   834	    for crate::resolver_core::session_resolver_context::SessionResolverContext<'a>
   835	{
   836	}
   837	
   838	// Compile-time dyn-compatibility check. If a future trait edit
   839	// accidentally introduces an associated type, generic method, or
   840	// `where Self: Sized` bound that breaks dyn-compatibility, this assertion
   841	// fires inside this file at compile time long before a callsite-cascade
   842	// error.
   843	static_assertions::assert_obj_safe!(ResolverContext);
   844	// The request-bound refinement is used as `&dyn RequestBoundResolverContext`
   845	// by the query host port, so it must stay dyn-compatible too. A marker
   846	// subtrait of a dyn-compatible trait adding no new methods is dyn-safe;
   847	// this pins it against a future edit.
   848	static_assertions::assert_obj_safe!(RequestBoundResolverContext);
   849	
   850	impl ResolverContext for crate::VerterHost {
   170	    pub(crate) fn overlay(&self) -> &Arc<CanonicalCompletionOverlay> {
   171	        self.view.overlay()
   172	    }
   173	
   174	    /// Idempotently promote a newly-loaded canonical into the overlay
   175	    /// (epoch-guarded).
   176	    ///
   177	    /// Called from `ensure_loaded` / `ensure_indexed_ready_serve` success
   178	    /// paths so subsequent self-root fact validation observes the
   179	    /// freshly-loaded canonical's current content rather than
   180	    /// false-missing because the request-entry base view did not track
   181	    /// it.
   182	    pub(crate) fn complete_canonical(&self, canonical: &str) {
   183	        self.view
   184	            .overlay()
   185	            .complete_canonical(self.inner, self.view.base(), canonical);
   186	    }
   187	}
   188	
   189	impl<'a> ResolverContext for HostResolverContext<'a> {
   190	    // -------- Identity --------------------------------------------
   191	
   192	    #[inline]
   193	    fn is_request_bound(&self) -> bool {
   194	        true
   195	    }
   196	
   197	    // -------- Cache accessors --------------------------------------
   198	
   199	    #[inline]
   200	    fn prepared_decl_bundle(&self, canonical_id: &str) -> Option<Arc<PreparedDeclBundle>> {
   201	        // Pass the request-bound `RequestStoreView` (which chains the
   202	        // `CanonicalCompletionOverlay` in front of the base) so cache
   203	        // validation consults the overlay-shadowed view rather than
   204	        // bypassing it via `self.view.base()`. The overlay-bypass
   205	        // bug fix: the previous
   165	    /// Called from `ensure_loaded` / `ensure_indexed_ready_serve` success
   166	    /// paths so subsequent self-root fact validation observes the
   167	    /// freshly-loaded canonical's current content rather than
   168	    /// false-missing because the request-entry base view did not track
   169	    /// it.
   170	    #[allow(dead_code)]
   171	    pub(crate) fn complete_canonical(&self, canonical: &str) {
   172	        self.request_view
   173	            .overlay()
   174	            .complete_canonical_with_session_view(
   175	                self.inner,
   176	                self.request_view.base(),
   177	                self.view,
   178	                canonical,
   179	            );
   180	    }
   181	}
   182	
   183	impl<'a> ResolverContext for SessionResolverContext<'a> {
   184	    // -------- Identity --------------------------------------------
   185	
   186	    #[inline]
   187	    fn is_request_bound(&self) -> bool {
   188	        true
   189	    }
   190	
   191	    // -------- Cache accessors --------------------------------------
   192	
   193	    #[inline]
   194	    fn prepared_decl_bundle(&self, canonical_id: &str) -> Option<Arc<PreparedDeclBundle>> {
   195	        // Route through the host's view-aware variant so an
   196	        // overlay-bearing view observes overlay-content-rooted
   197	        // prepared declarations rather than the host's base bundle
   198	        // cache. Non-overlay canonicals fall through to the warm
   199	        // bundle cache transparently.
   200	        self.inner

exec
/bin/zsh -lc 'rg -n "''^(use|pub use) crate::|crate::(semantic_query|meta_resolve|typeinfo|host_executor|request_context|component_meta_materialize|component_meta_caches|fact_signature_helpers|cache_runtime|structural_carrier_producer|types)" crates/verter_session/src/resolver_core/component_meta crates/verter_session/src/resolver_core/component_meta_query_engine --glob '"'*.rs' | sed -n '1,320p' && find crates/verter_session/src/resolver_core/component_meta crates/verter_session/src/resolver_core/component_meta_query_engine -type f -name '*.rs' | sort" in <block worktree root>
 succeeded in 1385ms:
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:29://! ([`crate::fact_signature_helpers::with_cacheability_scope`]) as the
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:31://! [`CacheabilityProbe`](crate::fact_signature_helpers::CacheabilityProbe) to
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:80:use crate::component_meta_caches::ComputedEntry;
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:188:                    if let crate::cache_runtime::SignatureAdmission::Cacheable(sig) =
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:201:                                crate::component_meta_caches::ImportedRegistryEntry {
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:262:        observed_keyed_hash: Option<crate::types::Hash16>,
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:263:    ) -> crate::cache_runtime::singleflight::ComputeAdmission<
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:265:        crate::component_meta_caches::ImportedRegistryEntry,
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:315:                    crate::request_context::mark_request_result_partial();
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:316:                    return crate::cache_runtime::singleflight::ComputeAdmission::ReturnOnly {
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:318:                        reason: crate::cache_runtime::NonAdmissionReason::PartialResult,
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:330:            return crate::cache_runtime::singleflight::ComputeAdmission::ReturnOnly {
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:332:                reason: crate::cache_runtime::NonAdmissionReason::ForcedTestRefusal,
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:341:            return crate::cache_runtime::singleflight::ComputeAdmission::ReturnOnly {
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:343:                reason: crate::cache_runtime::NonAdmissionReason::UnresolvedProvenance,
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:347:            crate::cache_runtime::SignatureAdmission::Cacheable(sig) => {
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:348:                crate::cache_runtime::singleflight::ComputeAdmission::Cacheable(
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:349:                    crate::component_meta_caches::ImportedRegistryEntry {
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:356:            crate::cache_runtime::SignatureAdmission::NonCacheable(reason) => {
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:357:                crate::cache_runtime::singleflight::ComputeAdmission::ReturnOnly {
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:459:                            crate::cache_runtime::NonAdmissionReason::EmptySignature,
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:468:                        crate::cache_runtime::SignatureAdmission::Cacheable(sig) => {
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:471:                        crate::cache_runtime::SignatureAdmission::NonCacheable(reason) => {
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:568:                    if crate::cache_runtime::refuse_result_cache_admission_if_partial(
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:569:                        crate::request_context::current_request_result_is_partial(),
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:573:                            crate::cache_runtime::NonAdmissionReason::PartialResult,
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:579:                            crate::cache_runtime::NonAdmissionReason::EmptySignature,
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:588:                        crate::cache_runtime::SignatureAdmission::Cacheable(sig) => {
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:591:                        crate::cache_runtime::SignatureAdmission::NonCacheable(reason) => {
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:681:                    crate::cache_runtime::SignatureAdmission::Cacheable(sig) => {
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:684:                    crate::cache_runtime::SignatureAdmission::NonCacheable(reason) => {
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:761:        let (decl_result, non_cacheable) = crate::fact_signature_helpers::with_cacheability_scope(
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:762:            &crate::fact_signature_helpers::FactTracerBasisSource::from_ctx(self.ctx),
crates/verter_session/src/resolver_core/component_meta/direct_macro.rs:7:use crate::resolver_core::ResolvedTypeDeclaration;
crates/verter_session/src/resolver_core/component_meta_query_engine/engine_accessors.rs:8:use crate::resolver_core::FuseTrip;
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_route_scoped.rs:13:use crate::semantic_query::{ProjectionMode, SemanticNodeId};
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_route_scoped.rs:149:                    crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_route_scoped.rs:154:                .map(|hot: crate::semantic_query::HotTypeRef| hot.node())?
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_route_scoped.rs:236:        use crate::semantic_query::SemanticNodeData;
crates/verter_session/src/resolver_core/component_meta/native_props.rs:6:use crate::semantic_query::{
crates/verter_session/src/resolver_core/component_meta/native_props.rs:10:use crate::typeinfo::surface::TypeInfoSurfaceMember;
crates/verter_session/src/resolver_core/component_meta/native_props.rs:102:            binder_scope_id: crate::semantic_query::BinderScopeId::file_scope(root_owner),
crates/verter_session/src/resolver_core/component_meta/native_props.rs:106:    crate::meta_resolve::emit_dispatch_dep_signature_facts(dispatch.ctx, &read.dep_signature);
crates/verter_session/src/resolver_core/component_meta/native_props.rs:138:                crate::typeinfo::raise::render_node_display_with_ctx(ctx, member.value)
crates/verter_session/src/resolver_core/component_meta_query_engine/tests.rs:11:use crate::types::{AnalysisLevel, HostConfig};
crates/verter_session/src/resolver_core/component_meta_query_engine/tests.rs:12:use crate::VerterHost;
crates/verter_session/src/resolver_core/component_meta_query_engine/tests.rs:175:        vec![crate::types::DependencyResolution {
crates/verter_session/src/resolver_core/component_meta_query_engine/tests.rs:183:        vec![crate::types::DependencyResolution {
crates/verter_session/src/resolver_core/component_meta_query_engine/tests.rs:277:        vec![crate::types::DependencyResolution {
crates/verter_session/src/resolver_core/component_meta_query_engine/tests.rs:364:        vec![crate::types::DependencyResolution {
crates/verter_session/src/resolver_core/component_meta_query_engine/tests.rs:1465:            crate::semantic_query::ProjectionMode::Expanded,
crates/verter_session/src/resolver_core/component_meta_query_engine/tests.rs:1738:            crate::semantic_query::ProjectionMode::Expanded,
crates/verter_session/src/resolver_core/component_meta_query_engine/tests.rs:1807:            crate::semantic_query::ProjectionMode::Expanded,
crates/verter_session/src/resolver_core/component_meta_query_engine/tests.rs:2040:        vec![crate::types::DependencyResolution {
crates/verter_session/src/resolver_core/component_meta_query_engine/tests.rs:2346:        vec![crate::types::DependencyResolution {
crates/verter_session/src/resolver_core/component_meta_query_engine/tests.rs:2379:    let inflight_key: crate::component_meta_caches::ImportedRegistryKey = (
crates/verter_session/src/resolver_core/component_meta_query_engine/tests.rs:2511:        vec![crate::types::DependencyResolution {
crates/verter_session/src/resolver_core/component_meta_query_engine/tests.rs:2613:        use crate::request_context::{RequestContext, RequestContextGuard};
crates/verter_session/src/resolver_core/component_meta_query_engine/tests.rs:2629:            crate::request_context::current_request_result_is_partial(),
crates/verter_session/src/resolver_core/component_meta_query_engine/tests.rs:2677:    use crate::request_context::{RequestContext, RequestContextGuard};
crates/verter_session/src/resolver_core/component_meta_query_engine/tests.rs:2731:            !crate::request_context::current_request_result_is_partial(),
crates/verter_session/src/resolver_core/component_meta_query_engine/tests.rs:2762:    use crate::request_context::{RequestContext, RequestContextGuard};
crates/verter_session/src/resolver_core/component_meta_query_engine/tests.rs:2823:            !crate::request_context::current_request_result_is_partial(),
crates/verter_session/src/resolver_core/component_meta/tests.rs:2:use crate::resolver_core::declaration_metadata::ResolvedExportTarget;
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval_recursion_tests.rs:14:use crate::meta::MetaProject;
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval_recursion_tests.rs:15:use crate::request_context::{
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval_recursion_tests.rs:19:use crate::resolver_core::{
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval_recursion_tests.rs:23:use crate::semantic_query::{SemanticNodeData, SemanticNodeId, SurfaceView};
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval_recursion_tests.rs:24:use crate::types::{AnalysisLevel, HostConfig};
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval_recursion_tests.rs:25:use crate::VerterHost;
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval_recursion_tests.rs:37:    crate::semantic_query::surface_view! {
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval_recursion_tests.rs:69:    graph: &crate::semantic_query_memo::SemanticGraphStore,
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval_recursion_tests.rs:271:        vec![crate::types::DependencyResolution {
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval_recursion_tests.rs:281:        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval.rs:20:use crate::project_semantic_dispatch::{node_data_for, ProjectSemanticDispatch};
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval.rs:21:use crate::resolver_core::fallthrough::{
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval.rs:26:use crate::resolver_core::ResolverContext;
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval.rs:27:use crate::semantic_query::{ProjectionMode, SemanticNodeData, SemanticNodeId};
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval.rs:73:    if crate::request_context::current_request_budget()
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval.rs:79:        crate::request_context::mark_request_result_partial();
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval.rs:157:        if let Some(admitted) = crate::meta_resolve::project_expr_class_a_node_via_dispatch_threaded(
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval.rs:322:            crate::request_context::mark_request_result_partial();
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval.rs:350:            args.push(crate::semantic_query::CallArgKey::Eager {
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval.rs:356:                        crate::semantic_query::ArgumentLiteralMode::Widened
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval.rs:359:                        crate::semantic_query::ArgumentLiteralMode::Literal
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval.rs:374:        let result = dispatch.execute_indexed_resolve_call(crate::semantic_query::ResolveCallKey {
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval.rs:375:            point: crate::semantic_query::ProgramPointId {
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval.rs:381:                IndexedValueCallKind::Call => crate::semantic_query::CallKind::Call,
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval.rs:382:                IndexedValueCallKind::Construct => crate::semantic_query::CallKind::Construct,
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval.rs:387:            flow: crate::semantic_query::FlowNarrowingKey::empty(),
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval.rs:391:            crate::request_context::mark_request_result_partial();
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval.rs:496:fn known_spread_keys_from_surface(surface: &crate::semantic_query::SurfaceView) -> KnownSpreadKeys {
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval.rs:552:    if crate::request_context::current_request_budget()
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval.rs:555:        crate::request_context::mark_request_result_partial();
crates/verter_session/src/resolver_core/component_meta_query_engine/node_materialize.rs:5:use crate::semantic_query::SemanticNodeId;
crates/verter_session/src/resolver_core/component_meta_query_engine/node_materialize.rs:18:    use crate::semantic_query::SemanticNodeData;
crates/verter_session/src/resolver_core/component_meta/cold_resolver.rs:7:use crate::resolver_core::resolve_local_type_declaration;
crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs:20://! - [`MaterializeMemoDb`](crate::component_meta_caches::MaterializeMemoDb)
crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs:23://! - [`ComponentMetaResultDb`](crate::component_meta_caches::ComponentMetaResultDb)
crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs:26://! - [`SemanticGraphStore`](crate::semantic_query_memo::SemanticGraphStore)
crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs:35://! - [`MaterializeStructureDb`](crate::component_meta_caches::MaterializeStructureDb)
crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs:90:use crate::resolver_core::bare_name_resolve::DeclarationScopePayload;
crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs:91:use crate::resolver_core::scope_shadowing::ScopeShadowing;
crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs:92:use crate::resolver_core::ResolverContext;
crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs:93:use crate::resolver_core::{FuseBudgets, FuseState};
crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs:94:use crate::semantic_query::SemanticNodeId;
crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs:181:pub(crate) use crate::semantic_query::compat_spelling::{
crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs:198:/// Returns [`crate::cache_runtime::SignatureAdmission::NonCacheable`]
crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs:206:) -> crate::cache_runtime::SignatureAdmission {
crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs:207:    crate::fact_signature_helpers::fact_signature_for_exported_type(
crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs:309:///   [`crate::semantic_query::DepVersion`] the materialiser recorded.
crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs:360:    materialized_dep_signature: &crate::semantic_query::DepSignature,
crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs:361:) -> crate::cache_runtime::SignatureAdmission {
crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs:362:    use crate::cache_runtime::{NonAdmissionReason, SignatureAdmission};
crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs:364:    use crate::semantic_query::DepVersion;
crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs:433:    SignatureAdmission::Cacheable(crate::fact_signature_helpers::ReadSetSignature::new(
crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs:445:) -> crate::semantic_query::DepSignature {
crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs:446:    let mut entries: Vec<(std::sync::Arc<str>, crate::semantic_query::DepVersion)> = Vec::new();
crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs:454:            crate::semantic_query::DepVersion::WholeHash(whole_hash),
crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs:497:    pub hot: crate::semantic_query::HotTypeRef,
crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs:542:    // typed DBs on `ProjectTypeStore` (see `crate::component_meta_caches`).
crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs:926:            crate::request_context::bump_bare_engine_construction();
crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs:981:    /// ([`crate::meta_resolve::materialize`]'s shape-subject lowering)
crates/verter_session/src/resolver_core/component_meta_query_engine/helpers.rs:17:use crate::resolver_core::ResolverContext;
crates/verter_session/src/resolver_core/component_meta/mod.rs:9:use crate::resolver_core::{
crates/verter_session/src/resolver_core/component_meta/mod.rs:236:    pub surface_identities: Option<crate::meta_resolve::SurfaceNodeIdentities>,
crates/verter_session/src/resolver_core/component_meta/mod.rs:314:    pub surface_identities: Option<crate::meta_resolve::SurfaceNodeIdentities>,
crates/verter_session/src/resolver_core/component_meta/mod.rs:361:        let dtos_read = crate::typeinfo::framework_surface::vue_exec::vue_macro_dtos_with_ctx(
crates/verter_session/src/resolver_core/component_meta/mod.rs:363:            &crate::typeinfo::types::VueMacroSurfaceRequest {
crates/verter_session/src/resolver_core/component_meta/mod.rs:368:                level: crate::typeinfo::types::TypeInfoQueryLevel::FullMetadata,
crates/verter_session/src/resolver_core/component_meta/mod.rs:438:                .downcast_data::<crate::host_executor::HostSourceData>()
crates/verter_session/src/resolver_core/component_meta/mod.rs:444:        use crate::typeinfo::framework_surface::SvelteSurfaceSource;
crates/verter_session/src/resolver_core/component_meta/mod.rs:464:            let outcome = crate::typeinfo::framework_surface::svelte_exec::resolve_svelte_surface(
crates/verter_session/src/resolver_core/component_meta/mod.rs:472:                crate::typeinfo::framework_surface::ResolvedOutcome::Partial { .. }
crates/verter_session/src/resolver_core/component_meta/mod.rs:474:                crate::request_context::mark_request_result_partial();
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:40:use crate::project_semantic_dispatch::output_materialization::OutputProjector;
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:41:use crate::project_semantic_dispatch::raise::node_raised_shape_facts_with_dispatch;
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:42:use crate::project_semantic_dispatch::{resolve_decl_key, ProjectSemanticDispatch};
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:43:use crate::resolver_core::RouteDemand;
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:44:use crate::semantic_query::{
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:129:    /// [`HotTypeRef`]: crate::semantic_query::HotTypeRef
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:163:        use crate::semantic_query::{ProjectionMode, QueryResult};
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:210:        base: crate::semantic_query::SemanticNodeId,
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:233:        base: crate::semantic_query::SemanticNodeId,
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:235:    ) -> Option<crate::semantic_query::SemanticNodeId> {
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:236:        use crate::component_meta_materialize::{
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:272:        crate::meta_resolve::emit_dispatch_dep_signature_facts(self.ctx, &read.dep_signature);
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:373:        let (resolved, non_cacheable) = crate::fact_signature_helpers::with_cacheability_scope(
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:374:            &crate::fact_signature_helpers::FactTracerBasisSource::from_ctx(ctx),
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:517:            crate::semantic_query::ProjectionReductionContext::published(
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:518:                crate::semantic_query::ProjectionMode::Shallow,
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:522:            crate::semantic_query::InstantiateKey::new(
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:556:    ) -> Option<(crate::semantic_query::SurfaceView, SemanticNodeId)> {
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:612:        use crate::semantic_query::SemanticNodeData;
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:649:                    crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:787:                    context: crate::semantic_query::ProjectionReductionContext::published(
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:854:        let keys_node = crate::meta_resolve::build_keys_union_node(dispatch.graph(), keys)?;
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:860:            crate::semantic_query::InstantiateKey::new(
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:865:                    crate::semantic_query::ProjectionReductionContext::published(
crates/verter_session/src/resolver_core/component_meta_query_engine/route_admission.rs:47:use crate::project_semantic_dispatch::raise::{NodeShapeEq, RaisedNodeShapeFacts};
crates/verter_session/src/resolver_core/component_meta_query_engine/route_admission.rs:48:use crate::semantic_query::SemanticNodeId;
crates/verter_session/src/resolver_core/component_meta_query_engine/intrinsic_surface.rs:35:use crate::project_semantic_dispatch::semantic_source::SourceRaiseContext;
crates/verter_session/src/resolver_core/component_meta_query_engine/intrinsic_surface.rs:36:use crate::project_semantic_dispatch::{node_data_for, ProjectSemanticDispatch};
crates/verter_session/src/resolver_core/component_meta_query_engine/intrinsic_surface.rs:37:use crate::resolver_core::ResolverContext;
crates/verter_session/src/resolver_core/component_meta_query_engine/intrinsic_surface.rs:38:use crate::semantic_query::{
crates/verter_session/src/resolver_core/component_meta_query_engine/intrinsic_surface.rs:81:        let node = crate::meta_resolve::project_expr_class_a_node_via_dispatch_threaded(
crates/verter_session/src/resolver_core/component_meta_query_engine/surface.rs:20:use crate::project_semantic_dispatch::output_materialization::OutputProjector;
crates/verter_session/src/resolver_core/component_meta_query_engine/surface.rs:21:use crate::resolver_core::ResolverContext;
crates/verter_session/src/resolver_core/component_meta_query_engine/surface.rs:22:use crate::semantic_query::{QueryError, SemanticNodeData, SemanticNodeId, SurfaceView};
crates/verter_session/src/resolver_core/component_meta_query_engine/surface.rs:99:    use crate::semantic_query::{PathSegment, ProjectionMode, QueryResult, SemanticQueryKey};
crates/verter_session/src/resolver_core/component_meta_query_engine/surface.rs:111:        context: crate::semantic_query::ProjectionReductionContext::published(
crates/verter_session/src/resolver_core/component_meta_query_engine/surface.rs:144:    use crate::semantic_query::{PathSegment, ProjectionMode, QueryResult, SemanticQueryKey};
crates/verter_session/src/resolver_core/component_meta_query_engine/surface.rs:147:        crate::meta_resolve::dispatch_helpers::decompose_indexed_access_chain(expr);
crates/verter_session/src/resolver_core/component_meta_query_engine/surface.rs:172:        context: crate::semantic_query::ProjectionReductionContext::published(
crates/verter_session/src/resolver_core/component_meta_query_engine/surface.rs:198:/// ([`crate::meta_resolve::project_expr_class_a_node_via_dispatch_threaded`]):
crates/verter_session/src/resolver_core/component_meta_query_engine/surface.rs:214:    let node = crate::meta_resolve::project_expr_class_a_node_via_dispatch_threaded(
crates/verter_session/src/resolver_core/component_meta_query_engine/surface.rs:287:    use crate::semantic_query::{
crates/verter_session/src/resolver_core/component_meta_query_engine/surface.rs:513:/// [`crate::semantic_query::compat_spelling`] (the single family home), so
crates/verter_session/src/resolver_core/component_meta_query_engine/surface.rs:516:    use crate::semantic_query::compat_spelling as spell;
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:52:        let Some(product) = crate::structural_carrier_producer::macro_type_arg_hot_ref(
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:118:                crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:119:                    crate::semantic_query::ProjectionMode::Navigate,
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:138:    /// [`crate::semantic_query::SemanticNodeData`] carriers:
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:152:    ///    shared [`crate::meta_resolve::exactness::classify_node`];
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:159:        field_value: crate::semantic_query::HotTypeRef,
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:169:        let symbolic_preserve = |hot: crate::semantic_query::HotTypeRef| {
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:202:        if let crate::semantic_query::SemanticNodeData::InstantiationRef { base, .. } =
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:218:        if let crate::semantic_query::SemanticNodeData::IndexedAccess {
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:220:            index: crate::semantic_query::IndexKey::String(member_name),
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:267:                            crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:268:                                crate::semantic_query::ProjectionMode::Navigate,
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:280:                    let exactness = match crate::meta_resolve::exactness::classify_node(
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:330:                                    crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:331:                                        crate::semantic_query::ProjectionMode::Navigate,
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:335:                                        match crate::meta_resolve::exactness::classify_node(
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:395:    ) -> Option<crate::semantic_query::HotTypeRef> {
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:401:        let product = crate::structural_carrier_producer::macro_type_arg_hot_ref(
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:413:        if let crate::semantic_query::SemanticNodeData::Object(surface) = data.as_ref() {
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:415:                crate::semantic_query::SurfaceKeyProjection::Exact(member) => {
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:416:                    Some(crate::semantic_query::HotTypeRef::new(member.value))
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:418:                crate::semantic_query::SurfaceKeyProjection::AbsentProven => None,
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:442:                crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:443:                    crate::semantic_query::ProjectionMode::Navigate,
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:456:        node: crate::semantic_query::SemanticNodeId,
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:459:        use crate::semantic_query::SemanticNodeData;
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:468:            |engine: &mut Self, args: &[crate::semantic_query::SemanticNodeId]| {
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:616:        node: crate::semantic_query::SemanticNodeId,
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:619:        use crate::semantic_query::SemanticNodeData;
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:680:        node: crate::semantic_query::SemanticNodeId,
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:684:        use crate::semantic_query::SemanticNodeData;
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:731:                                crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:732:                                    crate::semantic_query::ProjectionMode::Navigate,
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:922:    node: crate::semantic_query::SemanticNodeId,
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:926:    use crate::semantic_query::SemanticNodeData;
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:934:    let recur = |n: crate::semantic_query::SemanticNodeId| {
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:953:                || matches!(index, crate::semantic_query::IndexKey::Computed(inner) if recur(*inner))
crates/verter_session/src/resolver_core/component_meta/cold_resolver.rs
crates/verter_session/src/resolver_core/component_meta/direct_macro.rs
crates/verter_session/src/resolver_core/component_meta/mod.rs
crates/verter_session/src/resolver_core/component_meta/native_props.rs
crates/verter_session/src/resolver_core/component_meta/native_props_rehome_contract_tests.rs
crates/verter_session/src/resolver_core/component_meta/tests.rs
crates/verter_session/src/resolver_core/component_meta_query_engine/engine_accessors.rs
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval.rs
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval_recursion_tests.rs
crates/verter_session/src/resolver_core/component_meta_query_engine/helpers.rs
crates/verter_session/src/resolver_core/component_meta_query_engine/intrinsic_surface.rs
crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs
crates/verter_session/src/resolver_core/component_meta_query_engine/node_materialize.rs
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_route_scoped.rs
crates/verter_session/src/resolver_core/component_meta_query_engine/route_admission.rs
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs
crates/verter_session/src/resolver_core/component_meta_query_engine/surface.rs
crates/verter_session/src/resolver_core/component_meta_query_engine/tests.rs

exec
/bin/zsh -lc 'wc -l .claude/skills/component-meta/SKILL.md .claude/skills/type-resolution/SKILL.md .claude/skills/host-session/SKILL.md .claude/skills/architecture/SKILL.md && rg -n "ComponentMetaQueryEngine|component_meta_query_engine|ProjectSemanticDispatch|exactly one|sole|query-time|ProjectResolver|ModuleResolverCore|ResolverContext|observation|lifecycle|publication|cache|materializ" .claude/skills/component-meta/SKILL.md .claude/skills/type-resolution/SKILL.md .claude/skills/host-session/SKILL.md .claude/skills/architecture/SKILL.md | head -260' in <block worktree root>
 succeeded in 162ms:
.claude/skills/architecture/SKILL.md:8:For domain-specific detail, see: `/type-resolution`, `/type-cache-architecture`, `/component-meta`, `/compiler-codegen`, `/host-session`.
.claude/skills/architecture/SKILL.md:16:- `verter_session` is the shared host/session/cache boundary for host-backed consumers.
.claude/skills/architecture/SKILL.md:18:- `verter_session::resolver_core` owns the host-backed resolver stack and type-resolution orchestration. Resolver-path methods receive `ctx: &dyn ResolverContext` (sealed super-trait at `resolver_core/resolver_context.rs`) — only `VerterHost` implements it, enforced by the `no_concrete_verter_host_in_seal_scope` arch-guard.
.claude/skills/architecture/SKILL.md:32:| **`@verter/unplugin`** | Universal bundler plugin (Vite, Rollup, webpack, esbuild, rspack, Rolldown, Farm). Compiles `.vue` files via `@verter/native`. Supports `preCompile` for build-start cache warming | `src/index.ts` |
.claude/skills/architecture/SKILL.md:42:| `preCompile` | `boolean` | `false` | Pre-compile all `.vue` files during `buildStart`. Scans project root, upserts files into host cache (including type dependencies for macros), and compiles them. When `transform()` later receives same content, host returns cached result instantly. `node_modules` excluded from scanning. |
.claude/skills/architecture/SKILL.md:124:| `VUE_API_USAGE` | 5 | Track provide/inject/lifecycle/watcher calls |
.claude/skills/host-session/SKILL.md:3:description: "LSP host integration: TypeProvider (TSGO/tsserver), workspace management, async scheduler, SyncCoordinator, ownership lifecycle"
.claude/skills/host-session/SKILL.md:10:`VerterHost` owns one `Arc<ProjectTypeStore>` per loaded project, exposed via `.project_type_store()` — the single shared cache graph for component-meta and cross-file type resolution:
.claude/skills/host-session/SKILL.md:14:- `RouteDb` (rehomed) — barrel / route surface cache.
.claude/skills/host-session/SKILL.md:15:- `OwnerImportSurfaceDb` — direct-owner-imports cache. Reached via `VerterHost::owner_import_surface` / `resolve_owner_direct_import`.
.claude/skills/host-session/SKILL.md:16:- `ComponentMetaResultDb<ComponentMetaAnalysis>` — final component-meta payload cache consulted by `get_component_meta` before any cold work.
.claude/skills/host-session/SKILL.md:17:- `SemanticGraphStore` — host-owned semantic-query memo, dispatched through `ProjectSemanticDispatch::execute`. The canonical lazy semantic layer and sole authority for reusable type-resolution work. Two parallel memos:
.claude/skills/host-session/SKILL.md:19:  - **Relation memo** (keyed by full-identity `RelateMemoKey` — source / target / relation kind / policy / source freshness / inference context / env+substitution+projection-reduction context) for `Relate` judgements. Admission is decided-only: only the binary `Assignable { bindings }` / `NotAssignable` payloads publish (cache-with-fence); `Unknown`, `BudgetExceeded` (public payload, `cache_suppress`), session-local inference deltas, and abandoned sessions publish NOTHING — no memo entry, no fact signature, no reverse index.
.claude/skills/host-session/SKILL.md:24:**Own-canonical drain** runs on every `upsert` for the upserted canonical itself: `resolver.runtime.evict_canonical(&canonical_id)` + `project_type_store.evict_canonical(&canonical_id)` + `resolved_type_cache().clear()` — drained together so a file-content change cannot leave one cache authority stale for that file. NO reverse-dependent cascade: an `upsert` never iterates `reverse_deps_for` to drain dependents. Cross-file consumers revalidate lazily on read via their own `fact_dep_signature` checks. (Retained until query-identity caches self-version-root a same-canonical content edit.) Workspace-shape changes (tsconfig / SDK / project-graph) call `bump_project_generation_and_evict`, clearing route-sensitive layers (`OwnerImportSurfaceDb`, `ComponentMetaResultDb`, `SemanticGraphStore`) atomically.
.claude/skills/host-session/SKILL.md:26:Host view: resolver-path helpers receive `&HostStoreView` directly as result-DB fence authority; `IndexedReady` is the single canonical post-parse artifact (former `ModuleFactsDb` deleted). Validated-cache writes record a `ReadSetSignature.facts` fact signature; warm hits revalidate it against the live `StoreView` before returning. Full store-view contract: "Host Store View" + "Store-View Token, Lane Identity, and Singleflight" below.
.claude/skills/host-session/SKILL.md:28:**Resolver-context seal:** resolver-path code does NOT take `&VerterHost` directly. It takes `ctx: &'a dyn ResolverContext` — a `pub(crate)` sealed super-trait at `crates/verter_session/src/resolver_core/resolver_context.rs`. Only `VerterHost` implements `ResolverContext` (`sealed::Sealed` marker closed at trait definition). Guard `no_concrete_verter_host_in_seal_scope` mechanically forbids re-introducing `&VerterHost` parameters under the resolver_core/meta_resolve/host_manage/component_meta_query_engine seal scope. New trait-surface methods are an architectural decision; widen with care.
.claude/skills/host-session/SKILL.md:32:`typeinfo/vue_macro_codegen.rs` is the sole semantic producer for compiler-facing
.claude/skills/host-session/SKILL.md:34:request-bound `ResolverContext` and fulfills `Runtime`, `Tsc`, or
.claude/skills/host-session/SKILL.md:37:`ProjectSemanticDispatch` and TypeInfo projection, never from parser type
.claude/skills/host-session/SKILL.md:40:The output is request-local and is not retained as an aggregate graph-id cache.
.claude/skills/host-session/SKILL.md:42:The producer submits exactly one interactive scoped cache-node job per
.claude/skills/host-session/SKILL.md:46:member, owner, runtime classifier, and TSC materialization stays inside that one
.claude/skills/host-session/SKILL.md:48:not a second cache authority.
.claude/skills/host-session/SKILL.md:51:completeness/cacheability, and reports the sorted transitive canonicals observed
.claude/skills/host-session/SKILL.md:53:marks the result non-cacheable, and never publishes the cancelled aggregate; a
.claude/skills/host-session/SKILL.md:79:server/ (LSP message loop, request dispatch — split post-Phase-11e into mod.rs + 8 siblings: handler_guard, provider_state, component_resolve, sync_orchestration, custom_methods, lifecycle, aux_features, nav_features)
.claude/skills/host-session/SKILL.md:105:**The `DisplaySignature` brand is sealed**: private inner `String`; construction only via `DisplaySignature::from_provider_wire` with a `DisplaySignatureWireWitness` (obtainable only through a `TypeProvider` impl — `provider_wire_witness()`); the sole reader is the labelled `as_display_str()`; display rewrites derive through `with_display_rewrite`; no `Deserialize`, no `Deref`/`AsRef<str>`/`Into<String>`/`Display`. Primary rail = the ordinary compile; trybuild witness = `verter_type_runtime/tests/cases/compile_fail.rs` (`--features compile-fail`, out of the default gate — recorded as GI-20 in `docs/arch/gate-integrity-ledger.md`). `$/verter/getBindingTypes` projects the signature onto the client wire as `Record<string, { displaySignature: string } | null>`; TS renders it verbatim (`packages/vue-vscode/src/bindingTypeDisplay.ts`) — any TS-side re-split of the display value is a Native-vs-Compat violation.
.claude/skills/host-session/SKILL.md:116:| `resilient.rs`    | `ResilientTypeProvider`: crash/silence detection via `Notify`, auto-restart with attempt-bounded respawn, file cache replay |
.claude/skills/host-session/SKILL.md:138:- **Resolution caching.** Per-project engine resolution is cached under a published-snapshot generation fence (success AND refusal), so hover/completion never repeats the ancestor walk, the `read_dir` of the install's `lib/`, or discovery's `npm root -g` fallback. A project-graph republish re-resolves; a bare `node_modules` mutation that publishes no snapshot still needs a reload.
.claude/skills/host-session/SKILL.md:139:- **`child_pid()`** returns the first started engine's PID. `$/verter/typeProviderStarted` carries exactly one PID (a single-engine wire affordance); orphan containment does not depend on it — every spawned tsserver arms its own process-group `TreeKill` and registers in the process-wide engine-tree table the client-death monitor terminates in full.
.claude/skills/host-session/SKILL.md:187:Request (stdio) -> server/mod.rs -> Find document in host cache -> Feature handler -> Response (stdio)
.claude/skills/host-session/SKILL.md:267:**Store-backed carrier refresh**: the plugin/store owns generated carrier membership and snapshots; protocol opens never carry generated bytes. `TsserverTypeProvider::register_carrier_member` hydrates local routing/position state and tracks the authored source in the explicit active working set. The first activation in a cold configured project uses one transient contentless companion open to instantiate the project/plugin, then immediately closes it; each editor-active authored source remains contentlessly open as the durable configured-project owner, matching the lifecycle official Volar/Svelte plugins receive from the editor. Workspace-discovered sources stay closed and enter project-scoped `getExternalFiles` lazily. Repeated activation is a no-op. Publishing advances the plugin's monotonic `carrierStoreRefreshToken`; a detached coalescing actor submits one background batch only after interactive traffic has been quiet for the grace window. That batch performs the constant-size `configurePlugin` refresh and one response-bearing no-op `configure` host-turn fence under a single background admission, so the project/plugin mutation queued for the next Node event-loop turn is visible before later interactive frames without paying the idle grace twice. An interactive carrier activation supersedes a running background refresh and republishes the newest working-set generation immediately. Any background refresh preempted by interactive traffic is requeued after the interactive lane becomes idle; retry is not conditional on the urgent-generation counter, because ordinary hover/completion may preempt without advancing it. The plugin compares manifest identities, reloads only changed source ScriptInfos, clears only the owning project's resolution cache, and reconciles ready authored-source roots through TypeScript's public `Project.addRoot` / `Project.removeFile` APIs. No generated file list, `updateOpen` payload, private whole-project filename reload, or per-file `projectInfo` probe is sent. Interactive requests preempt queued/running background diagnostics and refresh work through tsserver's cancellation pipe; creation of that out-of-band channel is a startup invariant, never a best-effort degradation to an unpreemptible session. Cancellation files are retained by exact request sequence until the matching response/`requestCompleted` acknowledgement (never reaped by age/count while an unbounded request may still be queued). Project-loading events suspend false hang strikes while retaining the absolute-silence backstop. The silence window begins at the later of the last engine message and the start of the current non-empty pending interval, so a request after a long idle receives the full health allowance.
.claude/skills/host-session/SKILL.md:269:**`@verter/types` resolution**: generated carriers retain the public `@verter/types` import. A project-installed or project-host-resolved package is authoritative. When resolution misses, the tsserver plugin serves its bundled declaration at a virtual package path; managed tsgo rewrites only the provider buffer to an adjacent virtual declaration overlay. Neither fallback writes into the user's `node_modules`. The tsgo carrier and overlay are serialized as one lifecycle unit: load/open/update semantics match, the dependency is published before a rewritten carrier, a newly created dependency is rolled back if carrier publication fails, transition to an installed package closes the old overlay only after the unrewritten carrier succeeds, and close cleans both.
.claude/skills/host-session/SKILL.md:271:**No-silent-empty provider recovery (hover / definition / typeDefinition)**: a FAILED provider query (`Err` from router bring-up, transport, engine restart, torn publish) never degrades to a vanishing tooltip or a silently dead CTRL+CLICK. All three carrier paths share ONE bounded-recovery owner (`server/provider_recovery.rs`, provider-neutral, above the per-route trait): resync the current file, recapture the request surface, and retry exactly once against it, validating and merging against the retry surface. The retry is IDENTITY-FENCED — it runs only when the recaptured surface's carrier `source_hash` equals the initial capture's, because a concurrent edit can put a different token at the same coordinates and a cross-revision retry would return a coherent-but-wrong result for the original request; on drift the recovery fails closed to the native result. Recovery is attempt-bounded, not wall-clock-bounded; a persistent failure fails closed after the retry, a legitimate empty (`Ok(None)` / `Ok(vec![])`) is not retried, and the resync is a current-file repair on the error path — never a dependency-publication join.
.claude/skills/host-session/SKILL.md:284:During `initialized()`, the LSP spawns a priority-aware `WorkspaceScanner`. Filesystem-backed tsserver continues to resolve real `.ts`/`.tsx`/`.js`/`.jsx` and `node_modules` from disk. Framework carriers are compiled on the background lane and published by authored source identity into the durable plugin store; after the carrier pass, one coalesced refresh advances metadata without making every workspace carrier a Program root. No generated file is opened over the tsserver protocol. Before every carrier unit the scanner yields to active LSP handlers, with a bounded background-deferral interval so continuous editor traffic cannot starve project-wide warmup. TSGO retains its explicit eager project-input path for carrier and plain-source materialization. Verter semantic/type-info caches remain a separate host concern; they are not serialized into either TypeScript engine or used as a substitute for its project graph.
.claude/skills/host-session/SKILL.md:369:would produce a sticky `TS2307` no publication could clear; abstaining lets
.claude/skills/host-session/SKILL.md:370:TypeScript's own answer stand and the next publication heal it. The carrier source
.claude/skills/host-session/SKILL.md:372:attempt, so a publication that makes it ready counts as a relevant ready-version
.claude/skills/host-session/SKILL.md:373:change and triggers that configured project's resolution-cache clear
.claude/skills/host-session/SKILL.md:393:listing, no filesystem walk, exactly one manifest stat on the keystroke path
.claude/skills/host-session/SKILL.md:400:owned carrier still in its publication warm-up window (no ready entry, no
.claude/skills/host-session/SKILL.md:424:When a carrier script imports through a non-carrier barrel (for example `components/index.ts`), publication follows exact configured ownership and each effective owner's `allowImportingTsExtensions` option. For tsserver, every effective owner must explicitly opt in before authored `.vue`/`.svelte` specifiers stay under the plugin + TypeScript resolver with no rewritten barrel buffer; one missing/`false` co-owner keeps the single shared buffer on the `.verter.ts` compatibility projection. Deterministic nearest-config convenience lookup is never ownership authority. TSGO retains explicit publication because it has no equivalent host plugin. Both explicit-publication paths seed from shallow import facts, walk only `ExportFrom` references, sync terminal carrier dependencies first, and publish only barrel projections whose bytes differ from disk; unchanged closure files are never pushed. The walk is cycle-terminated and complete (no depth/node cap may mint a false `DependencyReady` receipt) and yields periodically instead of truncating. Ordinary imports, dynamic imports, `require` edges, and unchanged compiled output remain provider-resolved from disk.
.claude/skills/host-session/SKILL.md:426:**Nonblocking dependency readiness**: definition and type-definition use capture-only readiness. A missing receipt enqueues/coalesces background publication but never joins it; the request queries the provider immediately against whatever project state is already valid. Rename classifies the cursor first and refuses a public component prop before provider synchronization or rename query; ordinary rename then performs the latest-current-file interactive repair and captures readiness without joining. Production navigation never waits for the barrel/publication walk. Settled publication joins exist only as explicit test setup.
.claude/skills/host-session/SKILL.md:428:**Demand-driven workspace-symbol completeness**: ordinary hover/completion/definition keep only editor-active/import-observed carriers in the provider. References and rename are different: neither tsserver nor tsgo can prove a workspace-wide result unless every framework source in the initiating carrier's configured project is represented. On the first such request, Verter reads immutable configured-project membership. Tsserver proves the frontier by promoting every current store advertisement through one `activate_carrier_members` transaction plus one interactive plugin refresh. TSGO never treats that editor-tsserver store as its witness, and proves TWO independent things about its own provider graph. (1) **Roots**: every expected carrier has receipt-gated, owner-matching IDE direct-open state. The API companion is deliberately not demanded here — the interactive IDE path never opens it for the file under the cursor, so demanding it would gate the frontier on a companion neither arm activates. (2) **Import closure**: for every carrier-import edge reachable from those carriers — direct imports, dynamic module references, and the `export … from` graph through rewritten barrels — the target's API companion (or the barrel's rewritten shadow buffer) is both LIVE and CURRENT. Roots alone prove each carrier's own symbols are in the Program, not that a cross-carrier reference RESOLVES: an importer's buffer imports the rewritten `{carrier}.verter.ts` specifier, so an unopened — or stale — API companion silently drops that importer from a project-wide answer. Liveness is the committed loaded flag; currency is a ROLE-SPECIFIC witness (the identity of the API declarations actually delivered to this provider, which only an API delivery may write — the state-wide commit stamp advances with IDE-only receipts and cannot stand in for it). An interactive IDE sync carries the liveness flag forward (that buffer really is still open) and re-observes the current projection, so an API-NEUTRAL edit stays ready with no reopen while a public-surface edit fails closed until publication delivers the new declarations. **Every writer of a shadow buffer must record the delivery it performed** — the source identity it built from AND the owner the live resolver reports — because the barrel re-publication leg skips an already-live shadow and is therefore not a recovery path: a state that sets only the liveness flag makes a delivered barrel read as undelivered-and-unowned, and the closure then refuses every project-wide references/rename answer for the rest of the session. This includes the OPEN-document own-path sync (`sync_self_file_shadow_state`), which serves the same rewritten projection when a user simply opens the barrel `.ts` in the editor; it recomputes the binding from the live resolver rather than clearing it, so `Unresolved` means ownership is genuinely unpublished. Carriers that are not import targets — a standalone initiating carrier, the file under the cursor — are never gated, and neither are targets outside the configured project. If the frontier is incomplete, the request signals scanner priority and fails closed instead of joining compilation or returning partial references/edits. The activated frontier stays warm for later workspace-symbol requests.
.claude/skills/host-session/SKILL.md:437:2. **Version-fenced staged push diagnostics**: LSP uses push diagnostics exclusively (no pull/`diagnostic_provider`). After the quiet window, the coordinator computes the provider-free Verter/ownership batch and publishes it immediately if URI, source identity, and exact document version still match. Provider diagnostics run afterward on a cancellable detached task and replace that batch with the merged result under the same exact-version/source fence. A slow or hung provider therefore cannot starve Verter-owned errors/hints, while a newer edit cannot publish an older batch. Completion of optional native semantic enrichment broadcasts a versioned `SemanticReady` event that invalidates only the Verter diagnostic cache and schedules a diagnostics-only pass: it never repeats provider file sync or graph refresh. Broadcast lag recovers by scheduling every open document once; channel closure terminates the coordinator instead of spinning.
.claude/skills/host-session/SKILL.md:438:   **Cross-file importer re-arm** (`arm_open_importer_republish`): after a tick processes a settled REAL edit (`requires_sync`), it arms a debounced diagnostics-only republish for every OPEN importer in the edited file's transitive reverse closure (`WorkspaceRead::affected_canonicals` — the R22 read-only consumer; never cache invalidation). A child edit moves neither the parent's document version nor the parent's diagnostics epoch, so the arm ADVANCES that epoch (`VerterHost::bump_diagnostics_generation`) and drops the importer's cached document-half entry. The advance is the fence, not the drop: the drop cannot stop a parent computation already in flight from landing its pre-arm write after it, and with version and epoch both unmoved every later read warm-hits that write. Every computation stamps the epoch it sampled at entry into its write and every read re-validates against the live value, so a computation that began before the arm can never satisfy a read after it. That requires the advance to be OBSERVABLE for every open importer — including one with no `ProfileState` row (never compiled, or the row dropped by the `set_workspace` / `close` authority-reset cascade) and one flagged evicted (a close/reopen pair leaves the flag set, because a byte-identical reopen takes the `upsert` quintuple-unchanged fast path): the epoch is reported independently of that flag, `evict` advances it, `bump_diagnostics_generation` materialises the row for any canonical the host knows, and the LSP stores the epoch as `Option<u64>` so "none recorded" can never collide with a real stamp. This is what makes a parent re-report/clear `verter/unknown-prop` and provider type errors when a child's prop surface changes. Fan-out bounds: once per settled quiet window (rides the tick, never `did_change`), open-editor count only, pending-map coalescing, `requires_sync: false` on the armed signal (no provider sync, warm IDE refresh; the tick appends to `settled_edits` only for a signal carrying `requires_sync`, so armed publishes never re-arm further — no cascade).
.claude/skills/host-session/SKILL.md:439:   tsserver diagnostics come from the three synchronous pull commands, issued as one ordered background transaction under one editor-idle admission; category-local failures degrade independently while interactive preemption restarts the idempotent transaction. Its pushed `semanticDiag` / `syntaxDiag` / `suggestionDiag` event bodies carry a file but no ScriptInfo version in supported TypeScript protocol versions, so those events are progress/health signals only and never enter the result cache. A last-good synchronous pull is reusable after a transient transport failure only while the provider's globally unique local content generation still matches; edits and close/reopen cycles invalidate it. The LSP's exact authored document-version plus immutable source-identity fence remains the final publication authority, including same-version edits and close/reopen ABA.
.claude/skills/host-session/SKILL.md:440:3. **Lifecycle health, not feature deadlines** (`verter_type_runtime/src/tsgo/ipc.rs` AND `verter_type_runtime/src/tsserver/ipc.rs`): production feature requests have no wall-clock latency timeout. Dropping a client-cancelled future removes its pending registration and sends engine cancellation. A separate watchdog observes pending work plus complete engine-output silence; sustained silence signals `crash_notify` so the resilient wrapper restarts the provider without returning a fabricated empty result. Engine EOF atomically closes the pending registry, drains in-flight requests, and rejects requests racing after death. Explicit timed helpers remain test/diagnostic-only; initialization, shutdown, writer-stall, and lifecycle bounds are not feature latency budgets.
.claude/skills/host-session/SKILL.md:446:**Completion D1 snapshot invariant** (`lifecycle.rs`, `nav_features.rs`, `features/cursor_context.rs`): `did_change` has two ordered lanes: a short completion-visible commit fence covering only registry/host upsert, plus provider-publication turns enqueued in commit order and awaited only AFTER the commit fence is released. `did_open` / `did_close` registry membership mutations (including virtual open and early close) take that same short fence, but no provider or lifecycle await may occur while it is held. Thus slow provider I/O preserves publication order without blocking later same-document or unrelated registry commits, and membership ABA cannot race a final native calculation. Completion holds the commit fence only long enough to capture one immutable parent `(source, line index, analysis, blocks, canonical ID)` snapshot, then releases it before provider awaits. After any provider await, completion validates both LSP version and immutable source identity (close/reopen may reuse a version); identity advance triggers bounded recomputation. The final native attempt keeps the commit fence through its synchronous cache-only native calculation, so sustained churn returns one coherent current result without panic or stale items. Cold imported carriers belong to the background scanner and TypeScript provider; interactive completion never `ensure_loaded`s them. Root-template ownership is framework-explicit: Vue markup is owned only by an explicit `<template>` block (script-only Vue never borrows Svelte root rules), while Svelte owns carrier-root markup, treats every paired root element/component (including `<template>`) as ordinary markup, and recognizes only `<script>` / `<style>` as SFC blocks. Vue root scaffold snippets never leak at Svelte root whitespace; fail closed until a Svelte-native root producer exists. Optional Svelte public-prop enrichment is built asynchronously in the isolated semantic host and published as an immutable cache snapshot; completion only reads it, preserving authored public keys (aliases, string keys, rest-covered members, named interfaces, and whole-object `$props()`) without parsing source on the request path. The source-authoritative Svelte cursor lexer searches bounded structural tag candidates and tracks nested braces, strings, comments, and regex literals, so `<` inside JavaScript never replaces the owning tag anchor. Component prop label/snippet syntax is owned by the parent carrier language, never inferred from the raw import suffix: extensionless and barrel-resolved Svelte children still use authored keys and `prop={$1}`. Vue attribute syntax is never a fallback for unresolved or zero-public-prop Svelte components; fail closed until a Svelte-native producer owns that surface. Virtual editor projections do not participate in carrier repair generations or lanes, preventing encoded/raw virtual URI identities from splitting lifecycle cleanup.
.claude/skills/host-session/SKILL.md:498:1. **Claimants** = configured projects that directly include the file (ordered `projects` Vec, never a set). Zero ⇒ `None` (⇒ `NoProject`); exactly one ⇒ that owner; ≥2 ⇒ the walk.
.claude/skills/host-session/SKILL.md:512:`server_utils::verter_owned_diagnostics` is the ONE producer of the Verter half of a document's diagnostics, and every publisher calls it: the debounced coordinator (`sync_coordinator::compute_verter_diagnostics` — the `did_open`/`did_change` path), BOTH background-initialization sweeps (`background_init.rs` — post-scan and step 7a), and the pull `textDocument/diagnostic` path (`server::compute_full_diagnostics`). Each publisher REPLACES the client's whole list for the document, so a publisher assembling a narrower set silently erases whatever the others surfaced — last writer wins, with no error anywhere. The version-cached document half (`compute_verter_diagnostics_for_with_views`) is PRIVATE to `server_utils` so a future publisher cannot reach it without the state-derived categories; tests go through the `#[cfg(test)] document_diagnostics_for_test` wrapper.
.claude/skills/host-session/SKILL.md:514:The document half is cached per `(version, diagnostics generation)`; the categories below derive from workspace/ownership/provider state that moves independently of the document version, so they are recomputed on every publish and never enter that cache.
.claude/skills/host-session/SKILL.md:541:- **Admission** (`rename_request_admission` ⇒ `Serve` / `Decline` / `Refuse`): an editor-owned carrier rename and a GENERATED virtual buffer answer nothing (the editor keeps its own behaviour); every carrier under a missing/non-authoritative root or a root/authority-assignment-marker generation mismatch refuses with the authority-unavailable error, while an authoritative multi-claimant carrier refuses with the terminal user-visible error above. Identical for both handlers. A carrier passes this gate only when a ready root says it is not multi-claimant and the assignment marker names the same generation; both handlers revalidate that witness after provider work before returning an offer or edit. This is an assignment/publication witness, not proof that `resync_open_files` completed or that the provider rebound every open file.
.claude/skills/host-session/SKILL.md:543:- **Classified public component props refuse before provider rename** (`RenameTargetClass::PublicComponentProp`): a prop cursor is positional, never inferred from provider non-emptiness. Vue macro/runtime prop fields, Options-API prop keys, and component-usage prop-name spans come from the existing shallow analysis; the usage check intentionally does NOT require child resolution, so an unresolved prop-shaped cursor cannot fall through `NotChildProp`. Svelte typed `$props()` members and legacy `export let` names use the existing public-API projector source map, whose mapped runs are byte-verified prop-name anchors only. Prepare projects this class to `Decline`; direct rename returns the clear incomplete-cross-file-proof `RequestFailed` error immediately after the shared resolution, before current-file provider synchronization and before `get_rename_locations`. No `WorkspaceEdit` is constructed. The reason is workspace completeness, not a discovered-parent count: every current reverse edge, open-document set, configured project, and provider graph is a positive cache and cannot prove that no unseen parent exists. The late `gate_cross_file_child_prop_rename` also unconditionally drops any `Confirmed` result as defense in depth.
.claude/skills/host-session/SKILL.md:549:- **Per-request, never transferable**: a resolution is never cached across requests. `handle_rename` re-resolves, re-queries, and re-validates its own captured surface; a yes-prepare licenses none of its gates.
.claude/skills/host-session/SKILL.md:615:Feature-gated (`scheduler`): `VerterHost` holds an `Arc<Scheduler>`. During `upsert()`, the host submits to the scheduler, awaits the `CompletionHandle`, reads back the result, populates the compile cache. The `HostStageExecutor` calls real `parse_vue_snapshot`/`parse_non_sfc_snapshot` for the Source stage. Host-specific data is stored in snapshots via the `SnapshotData` trait (opaque `Arc<dyn Any>`), avoiding circular dependencies between scheduler and host.
.claude/skills/host-session/SKILL.md:623:- **Per-input requested mode, classifier-owned actual mode.** Each input carries a `requested_mode` (`CompileBatchInput.requested_mode`, defaulting to `CompileBatchOptions.default_mode` → `Session`). The `compile_cache_mode` classifier is SOLE authority for `actual_mode`: `Session` stays `Session` under every eligibility reason (its fact rail handles them); `Content` downgrades to `Stateless` on any reason (its pure key cannot represent cross-file / session-scoped input); `Stateless` is the floor. Compile dedup keyed by `(canonical, effective requested_mode)`.
.claude/skills/host-session/SKILL.md:624:- **Svelte `cssHash` override — cache identity + fail-closed content admission.** A resolved Svelte `cssHash` override (the callback is resolved OUTSIDE the compiler; only the resolved `Option<String>` threads in) is COMPILE-OUTPUT PROFILE identity, carried on `CompileProfile.svelte_css_hash_override`. It participates automatically in BOTH cache keys — `compile_profile_hash` (the session slot u64) and `content_mode_profile_hash` (the Content pure key). Because the session slot addresses on the u64 alone, `CompileOutputNodeFactValidatedSession::lookup` ALSO re-checks the exact `Option<Arc<str>>` override on the slot against the live value (`slot.css_hash_override != live` misses), so a u64 collision can never serve a result with a different scope hash ("never wrong"). A user-supplied override is not provably content-deterministic, so `classify_compile_mode` pushes `DowngradeReason::CssHashOverridePresent` when one is present ⇒ a requested `Content` compile fail-closes to `Stateless`; `Session` caching stays safe via the profile identity + the exact slot check. The override never overloads Vue's `component_id`; a static guard bans `component_id` reads from Svelte CSS hashing.
.claude/skills/host-session/SKILL.md:625:- **Session-only compile-tier prefetch.** The cold compute installs `prefetch_compile_tier_observation_targets` (cross-file import-route cache + dependency `IndexedReady` pre-population) ONLY for `actual_mode == Session`, because the compile-tier fact tracer it feeds is installed only for `Session`. `Content` / `Stateless` compile correctness (external `src=` resolution, macro-type collection, dep sync) is produced independently by `compile_entry`.
.claude/skills/host-session/SKILL.md:631:LSP file ingestion goes through the one shared upsert engine: `did_open`/`did_change` call `VerterHost::upsert` (→ `upsert_many_with_priority` → one `Scheduler::submit_batch_atomic`), which owns generation tracking, request-context propagation, post-commit cache invalidation, and the canonical-uniqueness contract. No separate LSP-side `submit_request` shim — a file is never source-updated outside the engine (the sole direct `submit_request` is `host_lifecycle.rs` disk-reload with `source: None`, a read). `compile_blockers.rs` is deprecated -- the scheduler's blocker model replaces imperative hydration.
.claude/skills/host-session/SKILL.md:635:1. **Scheduler** = sole parser, raw source + analysis authority (`HostSourceData`, `HostAnalysisData`). `HostSourceData::source_type` is the authoritative `oxc_span::SourceType` for downstream cache-key sites -- computed once at `execute_source` time from the framework-neutral parse artifact (`HostSourceData::framework_parse: Option<Arc<FrameworkParseArtifact>>`, the carrier payload every host parse slot stores; Vue's `ParsedSfc` sits behind it, reachable only via the blessed `vue_parse()` accessor). Cache-key callers read via `VerterHost::authoritative_source_type_for(canonical)` or the higher-level `imported_eval_source_type_for(...)` helper.
.claude/skills/host-session/SKILL.md:636:2. **compile_cache** (`DashMap`) = profile state authority (compile_slots, overrides, diagnostics, deps, resolved_type_hashes). `CompileCacheEntry.evicted_whole_hash: Option<Hash16>` carries the pre-evict hash; `ensure_loaded` compares it to the post-reload hash and skips `bump_store_view_epoch` on no-op reloads so thread-local caches stay warm.
.claude/skills/host-session/SKILL.md:639:Architectural target for the project-global cache overhaul:
.claude/skills/host-session/SKILL.md:641:- scheduler remains the sole source and parse authority
.claude/skills/host-session/SKILL.md:645:- component-meta and analysis-triggered symbol expansion should populate the same host-owned resolver caches
.claude/skills/host-session/SKILL.md:649:The `CURRENT_REQUEST_VIEW` thread-local, `EffectiveView`, and `*_in_view` helpers are retired; `RequestStoreView` (`crates/verter_session/src/resolver_core/request_store_view.rs`) itself remains a live `pub(crate)` overlay `StoreView`. Resolver-path helpers take `&HostStoreView` (or use the host's live probes directly). `HostStoreView::from_host(self)` snapshots a cheap immutable view of the host's current state; its cache-validation identity is the complete `StoreViewValidationToken`, while `StoreView::compat_token` returns the narrower `StoreViewCompatToken` lane identity (epoch + session + `validity_fingerprint` = the external-supersession fold) — see "Store-View Token, Lane Identity, and Singleflight" below.
.claude/skills/host-session/SKILL.md:656:- Cache-validation staleness is enforced by the `ReadSetSignature.facts` path-precise fact signature on cache entries. Warm hits revalidate every recorded fact against the live `StoreView` before returning; stale entries miss and force a cold rebuild.
.claude/skills/host-session/SKILL.md:657:- Host-scoped caches (final `ComponentMetaResultDb`, `OwnerImportSurfaceDb`, `SemanticGraphStore`) validate through dep-signatures; transient `TypeSurfaceDb` writes only happen through `publish_with_facts` which attaches dep-signatures.
.claude/skills/host-session/SKILL.md:659:Native canonical loading goes through `ensure_loaded` — the scheduler is the sole source authority. Disk-read fall-throughs were deleted on native. WASM keeps the `files` map + workspace fallback.
.claude/skills/host-session/SKILL.md:663:`HostStoreView` is Arc-backed by an immutable `StoreViewSnapshot`: `VerterHost::store_view_manager()` hands out ONE shared snapshot per `StoreViewValidationToken` generation by cheap `Arc` clone; `with_session_overlay` re-roots overlay/tombstone canonicals via copy-on-write so the shared base is never mutated in place. `RequestStoreView` (`resolver_core::request_store_view`) is the LIVE request-scoped read-through wrapper: it chains a `CanonicalCompletionOverlay` in front of the request-entry `HostStoreView` so mid-request additive loads (`ensure_loaded`/`ensure_indexed_ready_serve` successes the entry snapshot did not track) validate without a false miss. The `CanonicalCompletionOverlay` also carries the request-world prepared-decl bundle memo (`RequestBundleMemo`, reached through `CanonicalCompletionOverlay::bundle_memo()` and threaded explicitly into `prepared_decl_bundle_with_store_view` / consulted by `prepared_decl_bundle_with_context` via `ResolverContext::request_completion_overlay`). It is ONE memo covering the base, session-overlay and `RequestOnly` worlds, keyed `(canonical, BundleMemoWorld)` with the `StoreViewCompatToken` on the entry: `BundleMemoWorld::{Base, Overlay(content hash)}` keeps the base and session namespaces distinct (R17 keeps overlay-bearing bundles OUT of the shared `prepared_decl_bundles` cache, and the shared slot is keyed by canonical alone), while the token pins entries to ONE externally-coherent world so a stability-retry attempt under an externally-moved view misses and re-materialises. Admission is STRUCTURAL: `RequestBundleMemo::insert` itself refuses anything that is not `ReuseClass::is_request_reusable`, so a cancelled, partial, lease-missed, mutation-unstable or overflow-refused materialisation never enters it, and a `RequestOnly` entry replays its stored refusal on EVERY hit so reuse cannot launder the taint (provenance counter `bundle_request_memo_hits`; regressions in `crates/verter_session/src/request_bundle_memo_tests.rs`).
.claude/skills/host-session/SKILL.md:668:- A successful FIRST-TIME additive `ensure_loaded` bumps the dedicated `load_generation` (as does a positive import-route admission via `cache_positive_import_route_result`): it adds a scheduler node + `derived_raw_cache` state the build folds into `whole_hashes`/`derived_hashes` but does NOT publish into `FileArtifactStore`, so `artifact_generation` alone would not cover it. It deliberately does NOT bump `store_view_epoch` — a cold compute's own dependency loads stay EXCLUDED from the publish fence's `externally_superseded_by` check and never self-fence promotion.
.claude/skills/host-session/SKILL.md:670:**Singleflight LANE identity is NARROWER than the reuse oracle.** The coalescing-lane identity is `StoreViewCompatToken`, whose `validity_fingerprint` is the `lane_fingerprint` — delegating to `external_supersession_fingerprint`, the SAME oracle the promotion fence `is_stable` applies. The fingerprint folds ONLY the external-supersession dimensions: `store_view_epoch` + `project_generation` + workspace `content_generation` (file-set mutations — watcher recovery / dependency appearance — advance it without any host-side epoch; a snapshot's edge-currency gates evaluate against it at build time, so a cached snapshot MUST miss once it moves; a cold compute's own loads never advance it, so it cannot self-fence) + folded env hashes (`env_hash_fold`) + `project_identity` + frozen overlay identity. The additive generations (`artifact_generation` / `load_generation`) are DELIBERATELY EXCLUDED from the lane identity: a cold compute advances them through its OWN work (materialising content-addressed caches gated by `store_view_epoch`, loading its dependencies, admitting its own routes), so two identical concurrent requests snapshot at slightly different points in the load sweep — folding those generations would split them across separate lanes and spawn multiple cold winners instead of one leader + N-1 dedup-joining followers. Because the lane oracle IS the promotion oracle, a follower that joins a lane shares exactly the external dimensions the leader's promotion was gated on, so the leader's dedup-joined result is validation-equivalent for it; and a request whose snapshot externally-supersedes the leader's (an epoch / project / env / identity / overlay change, even at an equal `store_view_epoch`) gets a different lane key and forks its own lane — it never receives a result computed under a different external view. The complete `StoreViewValidationToken` (including the additive generations) REMAINS the store-view reuse/validity oracle — the `StoreViewManager` rebuilds its base snapshot on any additive-generation change; only the LANE identity was narrowed (reuse-oracle = full token; lane-identity = external fingerprint).
.claude/skills/host-session/SKILL.md:672:**Base-view build:** no-torn-return (a coherent capture or `Superseded`, never a torn publishable view) and SINGLEFLIGHTED — on a token miss exactly one caller sweeps while concurrent token-miss callers wait on a condvar and clone the winner's `Arc<StoreViewSnapshot>` (no N-way parallel sweeps). The component-meta cold publish fence rechecks the computed-under token against the live host before promotion (mismatch → return-only, no shared-cache warm), keying off `externally_superseded_by` so the compute's OWN artifact publications do not self-fence.
.claude/skills/host-session/SKILL.md:674:**Captured resolution root (C5a).** The snapshot also stamps the workspace's immutable published resolution world (`Option<Arc<verter_workspace::CapturedResolutionWorld>>`, captured O(1) in the same `PreBuildTokenInputs` read window via `WorkspaceRead::capture_resolution_world`: the current base root plus, for a session population, that session's overlay root). It is the SOLE authority for the `ResolveImportsFactRef::Resolution` arm — `HostStoreView::validates_resolution_fact` (shared by `validates_resolve_imports_domain` and its overlay-aware `_for_content_hash` sibling; `RequestStoreView` routes resolution facts straight to the base view because the completion overlay carries no resolver observations). A view with NO captured world validates no resolution fact. It stays OUT of the token because validity is fact-precise (world IDENTITY is barred from being a cross-root warm-validity oracle) and because a cold compute publishes replacement worlds through its OWN work (a first observation of any path records an evidence baseline), which would self-fence exactly like the additive generations. Residual, owned by C4/C5b: a view captured between a content mutation and the reader-driven evidence refresh holds a root that has not yet observed the change, so it keeps validating the pre-mutation witness until the refresh advances the fact.
.claude/skills/host-session/SKILL.md:676:**Handle-backed dimensions stay out of the token for DIFFERENT reasons:** `ResolvedImportFactsDb` is content-addressed — its key carries `content_hash`, so a new version is a new key and a fixed handle reads correctly (immutable-by-key). `RouteDb` is NOT content-addressed (its keys carry no content hash, and evict/clear/replace reuse the same key) — it stays out because every value it hands out is validated per candidate against the reading view through the candidate's own recorded `fact_dep_signature`, so an evict/replace yields a conservative fail-closed MISS, never a stale positive. The per-candidate signature comparison IS the validity rail, so the token needs no `RouteDb` generation. The route-surface FACT domain does not read `RouteDb` at all: its sole arm (`StoreView::validates_route_surface_domain`) compares the recorded `expected_hash` against the augmentation-index fingerprint snapshot captured on the view's artifact root.
.claude/skills/host-session/SKILL.md:682:- `StoreViewRead::current()` → `CurrentHostStoreView` — the `StoreViewManager` proved this view current at handoff. Allowed for warm-cache fact validation AND for returning a normal query result.
.claude/skills/host-session/SKILL.md:687:- **Warm validators require `&CurrentHostStoreView`.** The top-level warm-validation entry points with no outer publish fence (`try_get_cached_meta_payload`, `ComponentMetaResultDb::get_with_view`, the imported-root / owner-import-surface warm reads) accept ONLY a proven-current view. A `ReturnOnly` read is a cache MISS, not a validation — the caller falls to the cold path whose own `is_stable` / publish fence gates promotion.
.claude/skills/host-session/SKILL.md:688:- **typeinfo query-returners do bounded-retry-then-supersede.** `resolve_named_symbol`, `evaluate_type_expression`, `project_node_to_type_expr_json_bytes` (the session-owned bytes FFI facade that mints the `OutputProjector` capability + materializes + serializes internally), `resolve_shallow_surface`, `resolve_vue_macro_surface`, `resolve_vue_public_type`, and `resolve_type_with_audit` build a request-bound dispatch context and RETURN the resolved node with NO outer fence. They acquire a `CurrentHostStoreView` via `typeinfo::current_store_view_for_query` (bounded retry, default 3); on sustained churn they surface a typed non-current MISS (`None`, or `QueryError::UnstableState`) rather than resolving against a superseded snapshot and returning a stale node. `None` is the established FFI miss signal (`typeExpr: null`). The retry is bounded — it terminates, never spins. A non-current evaluation must NOT warm the `scratch_cache`.
.claude/skills/host-session/SKILL.md:689:- **Every branch of a cache-presence-selected lane binds a REQUEST-BOUND context.** `VerterHost::build_template_class_semantic_facts` (`host_manage/analysis_io.rs`) picks its resolver context from artifact presence: the base-publishable + `IndexedReady`-present branch builds a `HostResolverContext::from_cold_seed`, the other composes a `SessionResolverContext::from_cold_seed`. The resolver-tier builder takes `&dyn RequestBoundResolverContext` — the sealed marker implemented for those two contexts and NEVER for `VerterHost` — so a bare-host binding is a COMPILE error, not a cache-presence-dependent runtime abort. The rail matters because `classify_binding` demands `prepared_value_decl` for every template `:class` script binding, and only a request-bound context can serve a prepared declaration. Regression: `template_class_lane_context_tests.rs` (indexed-present assertion + cold-seed control).
.claude/skills/host-session/SKILL.md:690:- **Cold contexts carry `Current` vs `ColdSeed`.** `HostResolverContext::from_current(&CurrentHostStoreView)` vs `HostResolverContext::from_cold_seed(&ColdSeedHostStoreView)` — and the session-bound counterpart `SessionResolverContext::from_cold_seed(&ColdSeedHostStoreView)`. The cold-seed constructor marks the request-bound `RequestStoreView` non-current iff the seed was `ReturnOnly`; its `validates*` family then fails CLOSED, so every nested warm-cache probe inside the dispatch MISSES rather than validating against the stale seed. This is the single-chokepoint enforcement — no individual nested validator knows about currentness.
.claude/skills/host-session/SKILL.md:693:  - **A helper that holds the EXECUTOR's single-read `(view, is_current)` pair** re-binds it via the SOLE pairing constructor `StoreViewRead::from_executor_snapshot(view, is_current)` (returns the intrinsic-currentness enum, consumed via `.into_cold_seed_view()`), confined to the executor boundary where the pair provably came from one read. The fallthrough cold compute (`compute_fallthrough_surface_uncached`) and the component-meta `*_with_view_arg` entries use this — the executor's `snapshot_view` destructured one `StoreViewRead` and threaded both into `compute`. The executors (`ComponentMetaRequestExecutor`, `FallthroughRequestExecutor`) track `snapshot_view_current` and thread it into `compute(.., base_is_current)`.
.claude/skills/host-session/SKILL.md:694:- **The cold-seed escape hatch (`ColdSeedHostStoreView::into_inner`) never feeds a validating context (INDIRECT-validation seam).** The raw unwrap `.into_cold_seed_view().into_inner()` DROPS the `is_current` flag. It is confined to NON-validating consumers: the request-driver `snapshot_store_view()` accessors, the overlay-priority `capture_component_meta_inputs_with_view` (builds `CapturedComponentMetaInputs` only), and `#[cfg(test)]` direct-`host` wrappers. The fallthrough resolver validates its per-element / per-child / per-root node-cache entries through the request-bound `ctx.store_view()` (currentness-gated), not a separately-rebuilt raw `HostStoreView`.
.claude/skills/host-session/SKILL.md:695:- **Fenced cold seeds remain correct.** A real fenced cold builder (`cold_seed_view_and_fence` + `ColdSeedFence`, the request-driver `compute` path) may compute from a `ReturnOnly` coherent seed to avoid blocking, but re-checks currentness before publishing: a non-current seed's result is non-cacheable and surfaces as superseded/degraded, never warmed into the shared cache. Cold builders are NOT forced to `Current` (that would needlessly block cold progress under churn).
.claude/skills/host-session/SKILL.md:696:- **The raw-view escape hatch is allowlisted.** `StoreViewRead::into_owned_view()` yields a raw `HostStoreView` ONLY for the bare-host owned-view rail (`ResolverContext::resolver_store_view`, reachable when no request-bound context was installed), the request-driver owned-view snapshot accessors (currentness gated by `snapshot_view_is_current`), and `.into_cold_seed_view().into_inner()` fenced cold seeds.
.claude/skills/host-session/SKILL.md:698:Pinned by the static guards in `crates/verter_session/tests/cases/architecture_guards.rs`: `resolver_store_view_returns_store_view_read` (return type is `StoreViewRead`), `cold_seed_store_view_exposes_no_validation_surface` (no `validates*` on `ColdSeedHostStoreView`), `warm_validation_entry_points_require_current_store_view` (warm validators keep `&CurrentHostStoreView`), `resolver_store_view_into_owned_view_is_allowlisted` (raw-view escape hatch confined to the allowlist), `cold_seed_into_inner_confined_to_non_validating_allowlist` (the `into_cold_seed_view().into_inner()` raw-unwrap that drops currentness is confined to non-validating consumers — the INDIRECT-validation seam), `cold_compute_context_constructors_carry_currentness` (cold-compute context constructors are the currentness-carrying `from_cold_seed` form rooted on `RequestStoreView::new_cold_seed`; the footgun `from_raw_for_compute` stays retired; `from_executor_snapshot` is the sole executor-boundary re-bind), and `cold_seed_currentness_is_intrinsic_to_the_read` (the `(view, flag)` re-bind `from_executor_snapshot` is confined to the executor-boundary allowlist, and a fresh `resolver_store_view_read()` may never feed it — closing the view+flag divergence the constructor-shape guards missed), plus the discriminating regressions in `crates/verter_session/src/store_view_non_current_contract_tests.rs` (`session_cold_seed_context_fails_warm_probes_closed`, `view_bound_cold_seed_currentness_comes_from_its_own_read`, `fallthrough_cold_compute_node_cache_validation_fails_closed_under_churn`).
.claude/skills/host-session/SKILL.md:710:| `crates/verter_session/src/lib.rs` | `VerterHost` -- holds `Arc<Scheduler>`, `compile_cache` |
.claude/skills/host-session/SKILL.md:733:base-only caches. Content publication is owner-only, so dependency-derived
.claude/skills/type-resolution/SKILL.md:3:description: "Cross-file type resolution: type solver, ShallowFileState, ExternalTypeFrontier, canonical cache rules, macro traversal, prepared declarations"
.claude/skills/type-resolution/SKILL.md:14:- `RouteDb` — rehomed barrel/route surface cache, validated against live host facts.
.claude/skills/type-resolution/SKILL.md:15:- `OwnerImportSurfaceDb` — direct-owner-imports cache keyed by `(owner_canonical, owner_whole_hash)`. `VerterHost::owner_import_surface(...)` builds-or-fetches the surface; `resolve_owner_direct_import(owner, local_name)` is the single-call lookup every direct-owner-import caller uses.
.claude/skills/type-resolution/SKILL.md:16:- `FlowSliceStores` (`.flow_slice()`) — the flow-return substrate's per-function artifact stores: the shared `FunctionFlowGraphStore` graph memo, the retained-snapshot skeleton source, and the two content-addressed cache nodes over them (`FlowSliceHashNode` → `FlowSliceLoweredBodyNode`).
.claude/skills/type-resolution/SKILL.md:17:- `SemanticGraphStore` — host-owned memo table + node arena for the `SemanticQueryKey` / `ProjectSemanticDispatch` layer. Every `SemanticQueryKey` variant dispatches through `ProjectSemanticDispatch::execute`; semantic subqueries dedup through `SemanticGraphStore::execute_cooperative` (the one cooperative memo). Same-path recursion returns a sentinel instead of self-awaiting; cross-thread joiners block cooperatively on a per-entry `Condvar`.
.claude/skills/type-resolution/SKILL.md:18:- `ComponentMetaResultDb<ComponentMetaAnalysis>` — final payload cache for `get_component_meta`. Warm hits revalidate the recorded `ReadSetSignature.facts` fact signature against the live `StoreView` before returning.
.claude/skills/type-resolution/SKILL.md:23:  `ProjectSemanticDispatch` consults it before the `BuiltinUtility`
.claude/skills/type-resolution/SKILL.md:26:Dep-signature semantics: every reusable cache read returns a `CacheRead<T>` carrying the touched fact fragment. Callers merge those into the active `CompletionFence`, which bounds retries at 3 and publishes `UnstableState` when mid-flight invalidation persists.
.claude/skills/type-resolution/SKILL.md:30:Host-backed type/import resolution must treat the canonical file ID as cache identity. Contract:
.claude/skills/type-resolution/SKILL.md:32:- Load a dependency source at most once per canonical ID per workspace content generation. Parse immediately and cache raw source, parsed/OXC snapshot, and reusable eval/build state right away.
.claude/skills/type-resolution/SKILL.md:33:- On a cold miss materializing an imported dependency, derive the AST-backed bundle from that single parse and cache together: file snapshot, semantic `ScriptShallowIndex`, lazy declaration-body memo, and any other reusable per-file analysis. Do not let later resolver stages trigger a second parse of the same canonical file just to build another artifact.
.claude/skills/type-resolution/SKILL.md:34:- Host-owned imported-file caches are long-lived for the `VerterHost` lifetime. Distinct queries on the same host reuse the same cached canonical file state until that file's content hash or workspace generation changes.
.claude/skills/type-resolution/SKILL.md:36:- Treat named-node discovery as local symbol lookup. Once a file is parsed for a canonical ID/version, future lookups hit cached symbol/export maps instead of rewalking the full AST to rediscover names.
.claude/skills/type-resolution/SKILL.md:37:- Treat AST ownership as single-pass work. For a canonical ID/version, do at most one full top-level AST walk to discover named symbols/exports, then cache lookup entries and leave deeper expansion lazy per symbol. Do not rewalk the full file to rediscover the same symbol later.
.claude/skills/type-resolution/SKILL.md:39:- Resolve the requested import from the cached parsed file first. If the name is absent, only then BFS through explicit barrel/re-export hops. Do not rescan the same file graph on the second request.
.claude/skills/type-resolution/SKILL.md:44:- Keep expansion lazy. Do not eagerly resolve every transitive type in a file up front. Preserve named references so later requests expand from cache when needed.
.claude/skills/type-resolution/SKILL.md:45:- Collected imported aliases stay shallow. Root normalization is demand-driven: once a demand resolves the defining root through the shared route authority, reuse that memoized root (do not re-walk the barrel chain per touch), and do not eagerly materialize a prepared declaration during collection.
.claude/skills/type-resolution/SKILL.md:46:- Builder-owned shallow imported aliases treat their stored canonical ID as the defining-file root. They consult cached barrel/export state only when a canonical root is still unknown. Cache the prepared alias on the defining canonical file and hydrate from that file's host cache or its lazy declaration-body memo. Do not synthesize barrel-local prepared aliases for symbols that resolve to another file.
.claude/skills/type-resolution/SKILL.md:47:- Whole-file hashes are for long-lived update handling and cache validation, not repeated warm reads. Compute/store the hash once for the current source version, reuse until VFS reports a newer content generation / file version.
.claude/skills/type-resolution/SKILL.md:48:- VFS is the authority for file-change invalidation. When a canonical file's version/hash changes, host caches derived from that canonical ID must be discarded together across source snapshots, parsed state, declaration-body memos, and resolved-type/import caches.
.claude/skills/type-resolution/SKILL.md:49:- Invalidation stays selective. If `/src/type.ts` changes, invalidate caches owned by `/src/type.ts` and downstream final expansion/query results depending on it, but do not reparse or reshallow unchanged owner files that merely import it. Those owners stay warm on their own-file caches and only re-resolve against the refreshed imported dependency state.
.claude/skills/type-resolution/SKILL.md:51:- Concurrent cold requests reaching the same canonical imported file must collapse onto one host-owned materialization path. `Promise.all([MetaA, MetaB, MetaC])` must not produce three separate read/parse/shallow passes for the same `type.ts`.
.claude/skills/type-resolution/SKILL.md:53:- `ResolverContext::prepared_type_decl` preserves `Result<Option<Arc<PreparedTypeDecl>>, PreparationFailure>` end to end. `MissingExternalOwner` and `AuthoredOrdinalOverflow` are typed failures, never declaration absence: the prepared slot stays vacant, and an Option-shaped semantic boundary may serve them only through the single ReturnOnly adapter that marks the enclosing derivation non-cacheable. `LeaseMiss` remains the distinct recoverable `Ok(None)` + non-cacheable rail.
.claude/skills/type-resolution/SKILL.md:54:- Prepared import canonicalization is DEMAND-DRIVEN. Bundle build (`build_prepared_import_canonicalization`) walks NO import chain: it records each resolvable binding's DIRECT hop as `(local owner, local name) → (direct target canonical, ordinary-file owner, imported name)` — the `ordinary_file()` owner is the provisional final-resolution-owed marker. The FINAL `(canonical, owner, symbol)` resolves at the first decl-prepare / ref-head demand through the shared route authority (the type-export rail `resolve_imported_type_root_with_facts*`, memoized in `ImportedRootDb` under an R6 content-free key; the graph-native value-export rail with the terminal alias peel for value demands), and every demand site observes the chain hops' `FileWholeHash` + `Route` facts into the ACTIVE fact tracer AT DEMAND TIME — so the CONSUMING query's read-set (a `LowerLocator` shape memo, an `Instantiate` memo, a component-meta proof) invalidates on a barrel retarget or leaf edit anywhere on the chain. Chain facts are never pinned on the bundle's fact rail. Never default the target owner, substitute the source owner, or recover by name/span; an UNRESOLVABLE specifier records no entry and remains `MissingExternalOwner` and non-cacheable at prepare.
.claude/skills/type-resolution/SKILL.md:55:- A member-value-position reference to an unresolved AUTHORED IMPORT stays an honest `BareRef` carrier — it never poisons the root object's completeness (authored-partial preparation is declaration-wide, Instantiate completeness is demand-local; the member consumer that actually demands the value degrades it member-locally, e.g. the Vue runtime constructor's per-member `null` degradation). The unresolved-head site observes the owner's request-bound path-precise resolution witness into the active tracer (the demand-time recovery rail, same as `build_typeof`'s import-miss arm), so carrier-bearing surfaces stay COMPLETE + cacheable and every consuming warm entry invalidates the moment the missing dependency appears. Root-alias / heritage / authored intersection-union-arm reaches remain authoritative missing-dependency debt (typed partial + ReturnOnly).
.claude/skills/type-resolution/SKILL.md:57:- Reuse the current host-owned route/barrel cache path: `RouteDb` for barrel/export route facts and `ImportedRootDb` for imported-root proofs. Do not add a second route-cache subsystem for the same work without explicit proof it is needed.
.claude/skills/type-resolution/SKILL.md:58:- Route discovery stays lazy and demand-driven. First-hit discovery may follow barrel/reexport hops only until the symbol is found (or proven absent under the current negative-cache policy). Do not require a full scan of all barrel exports on every first hit.
.claude/skills/type-resolution/SKILL.md:63:- Do not use `Arc` next-hop chains as the primary barrel cache shape if a future route-cache redesign is introduced.
.claude/skills/type-resolution/SKILL.md:64:- Route caches and prepared-declaration caches invalidate independently. If a leaf file body changes but its export surface stays the same, the route fact may remain valid while prepared declarations and downstream final results refresh.
.claude/skills/type-resolution/SKILL.md:69:- Negative route/cache misses may be cached only against a concrete snapshot (hash/generation/store-view context). Cancelled or interrupted results must never be promoted to warm reusable cache entries.
.claude/skills/type-resolution/SKILL.md:71:- Legacy fallback paths that reparse or rewalk imported dependency files on warm requests should be removed, not preserved behind alternative code paths. Default behavior must go through the cache-aware host/VFS path.
.claude/skills/type-resolution/SKILL.md:72:- Architectural cache/resolver changes land as one clean cutover. No temporary shims, compatibility wrappers, feature flags, or duplicated old/new paths. Delete the superseded path in the same change, or upgrade the surviving path to first-class shared ownership with the same invariants and tests.
.claude/skills/type-resolution/SKILL.md:73:- Imported dependency loading, type-resolution source materialization, and dependency canonical resolution should be host-owned single entry points. Do not add request-local cache layers or alternative parser/import paths on top of the host cache for the same work.
.claude/skills/type-resolution/SKILL.md:74:- Imported type root/declaration resolution and prepared imported-type alias caching should also be host-owned single entry points keyed by canonical ID plus current file version/hash. Do not rebuild the same imported symbol route or prepared alias body per request when the host cache already has it.
.claude/skills/type-resolution/SKILL.md:75:- Do not add new request-scoped lookup memos over host-owned resolver work in the final architecture. Existing request-view-era memos are legacy and must be removed as part of the project-global cache cutover.
.claude/skills/type-resolution/SKILL.md:76:- `source_type` for downstream cache keys is authoritative from the scheduler: `HostSourceData::source_type` is computed once at `execute_source` time with full access to the parsed SFC; readers consume via `VerterHost::authoritative_source_type_for(canonical)`. Recomputing from `(canonical_id, framework_parse)` is unstable when the `framework_parse` artifact is dropped mid-resolution. Carrier files read the neutral `FrameworkParseCommon.script_regions[].source_type` (populated by the owning adapter's producer — Vue: `verter_compiler::framework_common::vue_bridge::build_vue_parse_artifact`); plain scripts derive from the classified `FileLanguage` row (`verter_language` registry — the SOLE plain-script dialect authority, `.d.ts`-family included via the `Dts` rows; `ScriptSourceType` carries `JsModuleKind` fidelity for JS: `.js` unambiguous, `.mjs` module, `.cjs` commonjs, `.jsx` JSX — session parse code never re-sniffs path extensions; guard: `plain_script_dialect_from_file_language`).
.claude/skills/type-resolution/SKILL.md:81:- If a later batch requests `MetaB` and `MetaC` again with no file changes, it must reuse the warm cached state for both the owner files and `type.ts`.
.claude/skills/type-resolution/SKILL.md:82:- If `type.ts` changes between batches, `MetaB` and `MetaC` may keep their own-file caches, while `type.ts` is processed exactly once for the new hash and then shared by both later requests.
.claude/skills/type-resolution/SKILL.md:102:  (`cache_positive_import_route_result`), its `PositiveRouteStamp` /
.claude/skills/type-resolution/SKILL.md:121:- A direct `derived_raw_cache().entry(...).import_routes.insert(...)` outside
.claude/skills/type-resolution/SKILL.md:122:  `set_import_dependencies` and the lifecycle reset methods is rejected — that
.claude/skills/type-resolution/SKILL.md:153:  (`SessionResolverContext::resolve_type_dependency_canonical` resolves through
.claude/skills/type-resolution/SKILL.md:162:`### Module-Resolution Keying (CRITICAL)` in the `/type-cache-architecture`
.claude/skills/type-resolution/SKILL.md:175:Architectural target for the project-global cache cutover:
.claude/skills/type-resolution/SKILL.md:178:- Scheduler remains the sole source and parse authority. `IndexedReady` is built from scheduler-owned parsed snapshots.
.claude/skills/type-resolution/SKILL.md:180:- `IndexedReady` eagerly owns the declaration inventory: top-level symbol names, kinds, declaration/name spans, source-order contributor grouping (statement locators), type-parameter names, syntactic member headers, enum member headers, and the augmentation-scope inventory — all safe for host-owned `Send + Sync` caches. It must NOT store per-symbol lowered `TypeExpr` bodies, per-symbol body dependency vectors, member deps, `typeof` roots, a whole-file `EvalEnv`, or body semantic hashes.
.claude/skills/type-resolution/SKILL.md:182:- Parse once per content generation through the lowering service's retained snapshot. Retention is LEASE-PINNED, not LRU/budget-evicted: the cold-index parse acquires ONE `SnapshotLease` for the artifact's `(canonical, whole_hash, parse_env_hash)` key and hands it to the artifact's `DeclBodyMemo`, so the header-index parse and every later body / whole-env / raw-surface demand reuse that ONE parse for the artifact's whole life — a live artifact never silently re-parses. The lease drops with the memo (hence the artifact), releasing the retained `Rc<ParsedEvalProgram>`. The temporary OXC parse arena stays on the worker (native) / the wasm thread-local shard, is per-file and per-version only, never crosses a thread boundary, and must not leak into long-lived `Send + Sync` shared caches; jobs return owned typed IR. A content edit produces a new key (fresh memo, fresh lease), so a superseded snapshot can never answer a new-content demand.
.claude/skills/type-resolution/SKILL.md:183:- `IndexedReady` is authoritative for declaration STRUCTURE and locators (import edges, export edges, headers); lazily lowered bodies are authoritative only after materialization and are keyed by the observed file content/version.
.claude/skills/type-resolution/SKILL.md:185:- If analysis or component-meta expands a shallow symbol, both paths must populate and reuse the same host-owned route, prepared-declaration, owner-import, and projection caches.
.claude/skills/type-resolution/SKILL.md:191:The `DeclBodyMemo::whole_env()` whole-file env product has exactly four consumers, all reaching it through `VerterHost::base_eval_env_arc`. Each now has a NON-BREAKING, bounded, graph-native per-symbol reader sitting BESIDE the legacy whole-env path; the legacy `whole_env()` path is retained in production as the equivalence ORACLE. The LANDED Stage 6 Option-B flip mints `HotTypeRef` handles at the dispatch boundary (the `decl_body_hot_ref` accessor) over the `Instantiate` query result — the consumer-visible `SemanticNodeId` the graph-bearing producer drives via the RESOLVING lowerer — and does NOT remove `EvalEnv` / `whole_env()` (the oracle is retained as the parity rail); `EvalEnv` / `whole_env()` removal remains a LATER stage (the oracle-deletion + Stage 7+ work), not landed. The readers route through `ShallowFileState::{type_decl, value_decl, header_index}` and never materialise `whole_env()` — including any DEPENDENCY whole env (C3's export-target + alias peel routes through `resolve_value_export_target_graph_native` → `peel_value_decl_alias_graph_native`, never the legacy `resolve_value_export_target` whose peel materialises the dependency's `base_eval_env_arc`). A non-test debug cross-check on the C1/C2/C4 consumers exercises each graph-native reader against the oracle (release builds skip it): C1 and C4 run on every real host call; C2 runs on every non-rune-module call (the Svelte rune-ambient-env modules are gated out because their per-symbol reader does not replay the rune ambient overlay). C1/C2/C4 assert presence/terminal/field equivalence against the oracle. C3 carries NO in-production cross-check: its equivalence is proved OFFLINE on full `(source_canonical, source_name)` pairs by `c3_fallthrough_runtime_value_deps_graph_native_equals_materializer_touched_full_pairs` (subset/equality on the touched-pair SET, never a name-count proxy — legal double-alias-onto-one-source hydrates two bindings from a single dep pair, so any `deps >= added` count bound is unsound). The only faithful in-production touched-pair recompute would route through the legacy `resolve_value_export_target` whole-env peel — the exact dependency-whole-env cost the readiness work removes — so the offline pair-equality test is the authoritative C3 equivalence rail. Inventory guard `whole_env_consumer_graph_native_inventory.rs` asserts, for each ENUMERATED consumer, that it keeps its retained parity-rail oracle AND has its graph-native reader beside it, AND that NO graph-native reader BODY routes through the whole-env path (`base_eval_env_arc`/`base_eval_env`/`whole_env`/the legacy `resolve_value_export_target`) — comment/string-stripped, whole-identifier, across every same-named definition. Its reach scan is a DIRECT-reach tripwire (`no_unanchored_direct_whole_env_reach_in_production`): a NEW production fn that DIRECTLY names a materialization root (`base_eval_env_arc`/`base_eval_env`/`whole_env`) outside the allowlisted anchors reddens, but it does NOT catch a TRANSITIVE reach through the retained oracle (a syn scanner cannot soundly emulate transitive call-graph resolution, and several legitimate callers already reach a whole env transitively through the oracle). That the consumer SET is exactly these four (no fifth) is established by the codex-confirmed exhaustive `whole_env()` consumer enumeration + the per-consumer oracle-equivalence tests + review — NOT by the guard's token-scan.
.claude/skills/type-resolution/SKILL.md:193:- **C1 `local_type_declaration_id`** → `local_type_declaration_id_graph_native`: both paths select the sole authored owner from the cached declaration-header inventory, then perform the exact-owner type-header lookup. Import bindings are not declaration headers and cannot mask a same-named local declaration in another SFC owner; an import-only name has no candidate, while same-name declarations in multiple lexical owners are genuinely ambiguous at this owner-agnostic API and fail closed. The oracle's `DeclarationId` is the 1-based ordinal in the INTERLEAVED type+value `add_type`/`add_value` registration order of `build_eval_env` (single shared `next_declaration_id` counter), NOT recoverable from the unordered, kind-split `DeclHeaderIndex` without replaying the registration walk. The id is an OPAQUE in-process token — it never crosses the FFI/wire surface (`FfiResolvedTypeDeclaration` carries no `declaration_id`), is never compared cross-file, and no production reader branches on its value. C1's contract is therefore STABLE-AND-UNIQUE, NOT EQUAL-TO-ORACLE; the reader returns a stable per-owner header-name ordinal id and the oracle stays authoritative for the value. The equivalence test pins presence (`Some`/`None`); the value-id derivation stays oracle-owned.
.claude/skills/type-resolution/SKILL.md:195:- **C3 `build_fallthrough_eval_env_lightweight`** → `fallthrough_runtime_value_deps_graph_native`: the whole-env CLONE this consumer takes of the OWNER env as its mutable base is NOT eliminated here (the LANDED Stage 6 Option-B flip mints handles in `decl_body_hot_ref` (over the `Instantiate` result the producer drives via the resolving lowerer) and retains `EvalEnv`/`whole_env()` as the parity-rail oracle, so that collapse is a LATER stage — the oracle-deletion + Stage 7+ work, not landed). The readiness deliverable is the graph-native runtime-value DEP SET — the `(source_canonical, source_name)` pairs the materializer touches, enumerated via the per-import route + export resolution (through `resolve_value_export_target_graph_native`, so NO DEPENDENCY whole env is materialised) WITHOUT a whole-env clone, proven equal on FULL pairs to the materializer-touched set (a re-export/aliased fixture where `source_canonical != dep_canonical` and `source_name != binding.name` pins source identity, not a name collapse).
.claude/skills/type-resolution/SKILL.md:196:- **C4 `dependency_eval_env`** → `dependency_value_symbol_graph_native`: the consumer's sole whole-env use is `source_env.value_symbols.get(name).primary().clone()` after a `prepared_value_decl` miss. The per-name reader reproduces that read via `value_decl(name)` (declaration_id 0, matching the prepared/alias hydration path) without the dependency's whole env.
.claude/skills/type-resolution/SKILL.md:204:`ProjectSemanticDispatch::demand_symbol_identity` is the shared adapter for
.claude/skills/type-resolution/SKILL.md:210:cycle/budget/work limits, and cache-suppression behavior.
.claude/skills/type-resolution/SKILL.md:219:Architectural target for the project-global cache cutover:
.claude/skills/type-resolution/SKILL.md:226:- Qualified namespace lookup is a two-stage exact-owner route: prove `(owner, namespace_alias)` from cached shallow import facts as a MODULE handle, then resolve the qualified MEMBER through the shared type/value export resolver. Never probe the dependency for an export named after the local namespace alias, and never stop a re-export at its barrel identity; ambiguous namespace bindings fail closed.
.claude/skills/type-resolution/SKILL.md:227:- Semantic query-identity keys are content-free (R6): a resolved declaration or route identity carries semantic identity plus the split env dimensions only, never a content/version hash, whole-hash, or `fact_dep_signature`. Version-rooting lives EXCLUSIVELY on the cached value (`ReadSetSignature.facts` + `self_root_canonicals`, revalidated on every warm read); the live content version (whole-hash) is re-sourced at value-compute time (`ensure_indexed_ready_serve`), never carried in the key.
.claude/skills/type-resolution/SKILL.md:262:request-local aggregate cache. Its internal
.claude/skills/type-resolution/SKILL.md:275:`ProjectionReductionContext::with_orthogonal_axes_from`; this is the sole
.claude/skills/type-resolution/SKILL.md:283:the two publication demands additionally occupy independent non-backfilling
.claude/skills/type-resolution/SKILL.md:287:`ClassifyBroadRuntime` is the sole broad constructor classifier. It traverses
.claude/skills/type-resolution/SKILL.md:304:Semantic behavior must be driven by typed facts, explicit projection policy, and complete semantic query identity. Do not encode type meaning, resolver routing, cache validity, or published component-meta shape in local heuristics.
.claude/skills/type-resolution/SKILL.md:310:- Letting projection mode, substitution environment, conditional context, scope version, workspace/source generation, package-boundary policy, or solver options affect a result without being represented in semantic identity, cached-value validation/read-set metadata, or the projection plan.
.claude/skills/type-resolution/SKILL.md:311:- Treating numeric caps, recursion limits, or fanout fuses as normal semantic answers. A fuse may stop work, but must return a structured degraded result such as `BudgetExceeded`, `Unsupported`, or an explicit recursion sentinel, and that result must not be promoted into a warm shared cache entry as if complete.
.claude/skills/type-resolution/SKILL.md:314:Allowed optimization heuristics are limited to performance triage that cannot change observable meaning: skipping a cache lookup, choosing an equivalent fast path after tests prove equivalence, or deciding a result is not safe to cache. If an optimization can change which type is returned, which members are visible, how aliases are preserved, or whether a result is considered complete, it is not an optimization heuristic; it is semantic policy and must be modeled explicitly.
.claude/skills/type-resolution/SKILL.md:318:- Every semantic query has typed identity plus validation metadata covering all meaning-affecting dimensions: declaration identity, scope/version root, projection mode, type arguments/substitution environment, conditional context, package/workspace policy, solver options. The cache may split these between slot key, per-mode entry, and cached-value fact signature, but no dimension may be implicit.
.claude/skills/type-resolution/SKILL.md:329:- Public query envelopes must preserve completeness. `Complete` means the required inputs were available, current, and no budget/unsupported/unstable branch affected the answer. A query may return `Complete(None)` only when absence itself was proven under the current facts; missing analysis, stale cache data, unavailable providers, unsupported operators, and budget exits must surface as `Unavailable`, `Partial`, or a typed degraded result.
.claude/skills/type-resolution/SKILL.md:330:- Degraded results may be displayed and returned to callers, but must not be promoted into warm shared caches as complete answers.
.claude/skills/type-resolution/SKILL.md:334:Architectural target for the project-global cache cutover:
.claude/skills/type-resolution/SKILL.md:337:  - file artifact caches (`IndexedReady`, prepared declarations, route surfaces, owner import surfaces, optional analysis)
.claude/skills/type-resolution/SKILL.md:338:  - semantic query caches (resolved declaration identity, instantiated meaning, indexed access, projected members, mapped or conditional results, normalized reusable intermediates)
.claude/skills/type-resolution/SKILL.md:339:  - final result caches (e.g. final component-meta payloads)
.claude/skills/type-resolution/SKILL.md:340:- Final payload caches should hand out immutable `Arc` values. Cache backend choice is an implementation detail as long as concurrency, size bounds, and validation rules are preserved.
.claude/skills/type-resolution/SKILL.md:341:- Reusable semantic cache population must be path-independent. If the same semantic result is reached through different entry paths, a successful computation must populate the same shared cache entry.
.claude/skills/type-resolution/SKILL.md:343:- Narrower successful results must not claim broader work is cached.
.claude/skills/type-resolution/SKILL.md:346:- A whole-surface projection may backfill per-member or per-indexed-access caches for the members or accesses it actually materialized.
.claude/skills/type-resolution/SKILL.md:347:- A narrow member or indexed-access result must not pretend sibling members or whole-surface projection are cached.
.claude/skills/type-resolution/SKILL.md:348:- Cancelled, superseded, interrupted, budget-exceeded, or partial results must not be promoted as warm shared cache entries.
.claude/skills/type-resolution/SKILL.md:350:- Top-level live-host results must publish through a completion fence: record the touched dependency signature, revalidate before publish, retry at most 3 times on mid-flight changes, never warm shared caches with torn provisional or unstable results.
.claude/skills/type-resolution/SKILL.md:354:- If `ProjectSurface(C, Expanded)` materializes member `"foo"`, a later `ProjectMember(C, "foo", Expanded)` should reuse that work.
.claude/skills/type-resolution/SKILL.md:355:- If `ProjectMember(C, "foo", Expanded)` ran first, that must not imply `ProjectSurface(C, Expanded)` is cached.
.claude/skills/type-resolution/SKILL.md:359:The shared semantic query system has exactly five modes. Every caller picks one; there is no implicit mode. Every `SemanticQueryKey` carries its mode as part of the cache key.
.claude/skills/type-resolution/SKILL.md:362:- **`Navigate`** — do the minimum semantic work to continue a requested path. Unwrap aliases transparently (aliases have no independent structural shell; `Navigate(X)` where `type X = Y` returns the same node as `Navigate(Y)`), follow member / index hops already materialized in the graph, reduce closed conditionals, stop at undecidability barriers. Does NOT recursively materialize subtrees; does NOT expand sibling members. Used by path projection to step one hop at a time. `Navigate` is one of the entrances of the open-key-domain carrier-stop — see **Open-Key-Domain Carrier-Stop (L1)** below.
.claude/skills/type-resolution/SKILL.md:363:- **`Shallow`** — return one shell / one surface level of the requested node without recursive expansion. Object: produce member names + per-member reference nodes; do NOT recurse into member bodies. Conditional: if the check is open / undecidable, expose both branches as references; if closed / decidable, reduce immediately and return only the selected branch shell. Union / intersection: expose contributor references. Alias: unwrap transparently to the target's `Shallow` form (aliases have no structural shell to materialize).
.claude/skills/type-resolution/SKILL.md:364:- **`Expanded`** — recursively materialize the requested result. Walk into member bodies, resolve nested references, evaluate every decidable conditional, distribute open conditionals into their remaining path projections, normalize unions / intersections. Aliases: unwrap transparently. Most expensive mode.
.claude/skills/type-resolution/SKILL.md:369:**Query-free structural carrier emission (LIVE macro-arg producer).** A separate, session-owned structural lowerer (`crates/verter_session/src/structural_carrier_producer/macro_arg_producer.rs`, entry `lower_type_expr_structural`, a BARE module-private fn — the producer is COLLAPSED into the ONE private module `crate::structural_carrier_producer::macro_arg_producer`, declared as a private `mod macro_arg_producer;` that re-exports only `macro_type_arg_hot_ref` + `MacroHotMirror` `pub(crate)`; the FOREIGN case is compiler-confined by module privacy and the SAME-MODULE residual is policed by the strengthened single-producer guards) emits the unresolved carriers directly from an owned `TypeExpr`, NodeScopeId-rooted, performing NO name / import / type resolution or reduction — it is a PRODUCER of carriers, never a second resolver, so it neither competes with nor duplicates the single demand-time resolution engine. A bare `Foo` becomes `BareRef`; `Foo<Arg>` a `BareRef` whose `type_args` are structurally lowered (never an `InstantiationRef`); `import("…")` an `ImportType`; unsupported raw syntax a `RawFallback` (display/compat only, never a control-flow miss); a construct-signature type a `Signature { kind: Construct }` node; tuple rest stays on `TupleElement.rest`; and `keyof` / indexed-access / conditional / mapped / `typeof` lower to their deferred shells carrying structurally-lowered operands — even where the eager path would reduce them. The only "resolution" it performs is the purely syntactic in-scope binder lookup that maps a `Ref` to a type-parameter / `infer` binder it already interned. It is intern-only (it never reaches `ProjectSemanticDispatch`, a `CarrierResolverContext`, a `SemanticQueryKey`, or any host / type-provider state) and stays demand-time — never pulled into publish or indexing (the `indexed_ready_publish_lowers_zero_decl_bodies` invariant). It is the LIVE macro-arg structural producer: macro type-argument carriers flow through production at the four macro sites, and its SOLE production caller is the session macro hot-mirror builder (Stage 5A, LANDED — see "Macro Hot Mirror" below), pinned by the module-privacy guard `structural_carrier_producer_lowerer_is_module_private` plus the ordering tripwire `no_production_macro_arg_eager_lowering_outside_mirror` and the purity guard `macro_hot_mirror_producer_is_pure_no_route_resolution`. The GLOBAL declaration-body structural flip (so a `type A = B` decl body lowers to `BareRef(B)` instead of the resolved body) is the **separate deferred query-free declaration-body structural-template producer** (NOT Stage 6, NOT landed) — the LANDED Stage 6 Option-B flip mints `HotTypeRef` handles in the `decl_body_hot_ref` accessor over the `Instantiate` result the graph-bearing dispatch producer drives via the RESOLVING lowerer, and does NOT route declaration bodies through this structural lowerer. Three further guards lock its query-freedom: `session_graph_lowerer_makes_no_query` (no query / resolver / host surface in the lowerer's production code), `unresolved_carriers_not_materialized_during_emission` (no `materialize_type_expr` / raise during emission, and the emitted root stays a carrier), and `oxc_worker_emits_no_session_graph_node` (the OXC worker / semantic-lowering surface produces owned `TypeExpr` IR only). Hermetic structural-equivalence fixtures prove its no-resolution shapes lower to the SAME interned graph as the eager `lower.rs` path.
.claude/skills/type-resolution/SKILL.md:371:**Demand-time `TypeOf` carrier reduction.** The `TypeOf` carrier reduction arms reduce through the ONE query-time dispatch in the evaluate / raise-semantic-reduce / PathWalker reducer arms: resolve the value root via `typeof_key_for`, project the carrier's dotted `path` via `ProjectPath` (intermediate hops in `Navigate`), THEN apply the carrier's instantiation `type_args` via `apply_typeof_instantiation_args` (resolve → project → apply, mirroring the eager order; an arity/shape mismatch composes an honest `Opaque(Miss)` AFTER projection). These arms are wired and exercised by directly-constructed `TypeOf` carriers and by the LIVE macro hot-mirror structural producer (Stage 5A, LANDED — a macro-arg `typeof` lowers to a `TypeOf` carrier that resolves at the demand) — the eager `lower.rs` `TypeExpr::TypeOf` arm still RESOLVES the typeof EAGERLY (via `execute_type_node(typeof_key_for(…))`) and mints NO `TypeOf` carrier node for the NON-macro declaration/body path — the LANDED Stage 6 Option-B flip mints `HotTypeRef` handles in the `decl_body_hot_ref` accessor (over the `Instantiate` result the producer drives via the resolving lowerer) and does NOT change this; routing decl-body `typeof` through a `TypeOf` carrier would be the separate deferred query-free declaration-body structural-template producer (NOT Stage 6, NOT landed) (see "Carrier production reach" below). This is the SEMANTIC instantiation application — it is NOT the structural raise/round-trip, which preserves `type_args` separately in the shared `shape_engine` fold the `raise_node_to_type_expr` shell primitive delegates to. The `instantiate_active` recursion guard sits at demand-time inside the `build_instantiate` push→pop windows, so a `typeof`-bearing instantiate / Vue-default body resolves WHILE the active identity is pushed — resolution firing after `pop_instantiate_active` would lose the back-edge sentinel (pinned by a leak regression test).
.claude/skills/type-resolution/SKILL.md:373:**Carrier-arg descent + carrier-arg encapsulation.** The three arg-carrying carriers (`BareRef`/`TypeOf`/`ImportType`) are OPAQUE tuple payloads (`crates/verter_session/src/semantic_query/carrier.rs`, e.g. `TypeOf(carrier::TypeOfCarrier)`) whose fields are PRIVATE, so the anti-tail invariant is enforced BY CONSTRUCTION: the ONLY crate-wide channel to DESCEND a carrier's args is the shared exhaustive `SemanticNodeData::carrier_type_args` accessor (the carrier's own raw-args reader `arg_nodes` is PRIVATE to the carrier module), the ONLY reconstruction channel is `SemanticNodeData::map_carrier_type_args`, head fields read through `typeof_head`/`bare_ref_head`/`import_type_head` (which NEVER return `type_args`), and construction goes through `new_typeof`/`new_bare_ref`/`new_import_type`. Those eight sanctioned accessors LIVE in an `impl SemanticNodeData` block INSIDE the `semantic_query::carrier` module, alongside the PRIVATE carrier payload methods (`arg_nodes`/`with_type_args`/head getters/`new`) — so the raw-args surface is compiler-confined to that one file (a sibling `impl carrier::BareRefCarrier` in `semantic_query.rs` reading `self.type_args` fails `E0616`), NOT merely `pub(super)`-to-the-parent, which makes the file-scoped shape guard COMPLETE. Because the field is private, a hand-rolled `node.type_args` direct bind on one of these three sealed carriers is UNREPRESENTABLE outside the carrier module REGARDLESS of how the variant is named (qualified, bare/imported, or `use … as Alias` renamed) and regardless of `cfg` / `#[path]` / `include!` / macro expansion — the compiler resolves the exact compiled program, which the retired `CARRIER_TYPEARGS_*` source scanner never could. The earlier "STATICALLY-FORBIDDEN variant-literal binds" + "aliased-RENAME residual NOT statically caught" framing is therefore MOOT (a private field cannot be bound, aliased or not). The SCAN/CLASSIFY walkers — the absorb infer-scan, the open-node value-body / enumeration-domain walk, and the `subtree_references` reachability scan (the `build_mapped_type` key-independence hoist) — descend via `carrier_type_args`; the reconstruction / render / identity boundaries split by whether they REBUILD: raise/materialize and display read head fields via the head accessors and descend args via `carrier_type_args` to materialize / render (NO rebuild), while the substitute re-intern arm ALONE rebuilds the carrier node via `map_carrier_type_args` (after descending args via `carrier_type_args`); `eq`/`hash` compare/hash the opaque payload (derived on the carrier). The surviving compile-fences are the exhaustive, wildcard-free `carrier_type_args` (descent) and `map_carrier_type_args` (rebuild) matches: a new carrier variant FAILS TO COMPILE there until its author classifies it. The compiler-enforced BY-CONSTRUCTION proof covers the three sealed arg-carrying carriers (private payload fields); the fences force a future carrier to be CLASSIFIED but NOT ENCAPSULATED (classification ≠ encapsulation) — a future named-struct carrier with a public `type_args` field could compile and re-expose the bind, so the BY-CONSTRUCTION guarantee is per-carrier (the sealed payloads); that enum-wide gap is CLOSED by the PRE-EXISTING (pre-Stage-6) enum-wide structural guard `no_named_type_args_field_outside_opaque_carrier` (rejecting a named `type_args` field on any `SemanticNodeData` variant unless it is an opaque `Variant(carrier::VariantCarrier)`), introduced before the Stage-6 work and orthogonal to it (registered + implemented; `docs/arch/parselower-design.md` tracks it). Tripwires: `no_named_type_args_field_outside_opaque_carrier` (the enum-wide named-`type_args`-field rejection), `carrier_variants_are_opaque_tuple_payloads` (the payload's FULL path `carrier::{Name}Carrier`, not just its final segment — an unqualified / wrong-module / raw `Arc<[SemanticNodeId]>` payload is rejected), `carrier_module_has_no_public_type_args_surface` (a RECURSIVE scan of carrier.rs: fields private, NO manual trait impl on a carrier struct / nested module / free fn / item macro / non-allowlisted derive / `#[cfg_attr]`, EVERY carrier inherent method PRIVATE, and the only `pub(crate)`/`pub` methods are the sanctioned eight `SemanticNodeData` accessors), `carrier_type_args_accessor_is_exhaustive_and_wildcard_free`, `map_carrier_type_args_is_exhaustive_and_wildcard_free` (both inspect the `match self` specifically; `crates/verter_session/tests/cases/carrier_encapsulation_guards.rs`). `is_deferred` (the relation-pair deferred-root predicate) and `exactness.rs::object_is_closed_node` (which root-kind-matches a `BareRef`/`TypeOf`/`ImportType`-valued member to `false` / object-OPEN) are ROOT-KIND classifiers: their verdict is complete from the carrier's root kind alone, they make NO `carrier_type_args` call and descend no args — NOT accessor-descent sites. The `meta_resolve` ref/cycle/dep walkers (`graph_predicates.rs::{body_contains_recursive_ref_to_name, collect_ref_identities_node}`, `slot_binding_graph.rs::accumulate_lowered_node_carrier_deps`), the `build.rs` type-param collector (`collect_type_param_nodes_by_name`), and the free-type-param classifier `slot_binding_graph.rs::node_contains_free_type_param` now ALSO descend a carrier's args via `carrier_type_args` (args-only — they collect NO head identity, since a `BareRef`/`ImportType` head is unresolved and a `TypeOf` head is a value root). A NEW scanner that root-kind-matches a carrier and silently IGNORES its args is caught by neither encapsulation nor the accessor fence — the defense is the wildcard-free accessor (forces a NEW variant to be classified) plus review. A `BareRef`/`ImportType` HEAD resolves only through the ONE dispatch (the `lower.rs` bare-name / import-augmentation / enum / builtin-shadowing `Ref` path), never ad-hoc in a consumer walker; `CarrierResolverContext` is the value-side bundle (never a query key) that head-resolution helpers consume when those carriers are resolved as a query subject. The `lower_type_expr_structural` producer of `BareRef`/`ImportType` carriers is LIVE for the macro-arg path (its sole production caller is the macro hot-mirror builder; the raw lowerer is a BARE module-private fn in the single private producer module `crate::structural_carrier_producer::macro_arg_producer`, reachable from outside only through the `pub(crate)` `macro_type_arg_hot_ref` entry — see "Macro Hot Mirror" below), so those carrier heads resolve at the demand through the ONE dispatch; the GLOBAL declaration-body producer flip that lets `BareRef`/`ImportType` carriers flow from declaration bodies end-to-end is the separate deferred query-free declaration-body structural-template producer (NOT Stage 6, NOT landed) — the LANDED Stage 6 Option-B flip mints handles in `decl_body_hot_ref` (over the `Instantiate` result the producer drives via the resolving lowerer) rather than routing decl bodies through the structural lowerer (`docs/arch/parselower-design.md` tracks both).
.claude/skills/type-resolution/SKILL.md:379:**Scope & entrances.** TWO families, judged through shared predicates consulted at EVERY entrance: the `Navigate` projector reduce route, the dispatch lowering entrances (`lower.rs` Pick/Omit + mapped), the `Instantiate`/`MappedType` build entrances (`build.rs`), the empty-path Shallow surface synthesiser (`walk.rs::synthesise_mapped_surface`), and the component-meta registry materialiser (`registry_decl.rs`, whose top-level alias/union/intersection composition walk fails OPEN-OR-UNKNOWN — traversal-budget exhaustion preserves the carrier instead of falling through into Expanded materialisation; guard: `materialize_member_surface_expr_preserves_open_mapped_carrier_on_walk_budget_exhaustion`).
.claude/skills/type-resolution/SKILL.md:383:**Family-1 SURFACE-position demand point — `Pick`-carrier enumeration.** A `Pick` carrier's OUTPUT key set is exactly its CLOSED key-selection `K` even when the SOURCE's key domain is open, so at a SURFACE-enumeration demand (the shallow walker's carrier unwrap — a heritage arm, a macro props/slots surface arm) an L1 carrier-stopped `Pick` does NOT contribute zero members: `walk.rs::visit_shallow_node` detects the verbatim carrier round-trip (`object_filter_carrier_surface_filter`), walks the SOURCE through the ordinary carrier-preserving frames (open arms stay deferred and contribute nothing, exactly as the direct un-filtered route publishes them — never a whole-open-source materialisation), and filters the enumerable surface to `K` at `Frame::FlushObjectFilter` (public members only, signatures dropped — `build_builtin_utility` member parity). Dropping the picked members instead was the nuxt-ui ContentSearch (18 heritage props) / DropdownMenuContent (5 generic-value-dependent slot keys) zero-member collapse. `Omit` deliberately does NOT participate: its output key set (`keyof Source − K`) is source-dependent-open, so an `Omit` carrier stays a carrier at every position (`get_component_meta_table_shaped_open_omit_*` pins it). VALUE-position publication of an open `Pick` still stays a shallow carrier (`chatmessages_resolvable_barrel_publishes_open_pick_as_shallow_carrier`); the surface-position pin is `get_component_meta_chat_messages_shaped_open_pick_heritage_enumerates_picked_keys_only` plus the `pick_over_genuinely_open_source_*` / `pick_over_instantiated_generic_*` regressions in `component_meta_pick_omit_tests.rs`.
.claude/skills/type-resolution/SKILL.md:397:**Tri-state conditionals via the shared oracle.** Conditional closedness routes through `ProjectSemanticDispatch::conditional_branch_selection` — the ONE branch-selection oracle that owns the FULL selection path `build_conditional` reduces with, so build-time reduction and predicate-time classification cannot diverge. Selection order: an `error` check dominates (⇒ Deferred); then the INFER ROUTING (`conditional_infer_route`) — a bare-`infer` extends binds `X := check` THROUGH the sole relation authority for ANY check (pre-`any`-guard placement is load-bearing), an in-scope direct pattern (object property / tuple head-tail / array element / function positions) binds through the relation's inference session, and an exact homomorphic mapped target `{ [P in keyof infer T]: X }` enables the frozen `ReverseHomomorphicMapped` session pass. That pass substitutes the mapper parameter by node identity for each closed source member, routes every property relation back through `execute(Relate)`, reduces substituted conditionals through the canonical conditional query whenever ordinary relation descent reaches them (including nested object, tuple, array, and function positions), records only registered canonical `T[key]` projection deposits, and submits one lower-priority aggregate candidate for `T`. Before any projection deposit, the full bound subtree is scanned through the exhaustive `SemanticNodeData::means_type_is_not_yet_known` classification: unresolved bare/import/`typeof` carriers, `RawFallback`, and failure/degradation `Opaque(QueryError)` variants refuse the deposit, while resolvable `DeclRef`/`InstantiationRef` carriers and the publishable `Opaque(RecursiveRef | DeclPlaceholder)` carriers do not. The reverse-source preflight applies those checks only to positions `assemble_reverse_candidate` can carry into the result: object member values and index-signature keys/values, array elements, and tuple element values; root object call/construct signatures, keyspace, and open-spread operands are not scanned because assembly drops them (open spreads are independently rejected by the root-open guard). The exhaustive subtree edge walk includes free-standing `TypeParam.constraint` / `default` in addition to signature-declared type-parameter bounds. Reverse construction preserves an array or tuple source's readonly container bit; mapped member modifier inversion remains an object-member/index-signature concern. A decided-positive property with no projection candidate recovers `unknown` and marks the aggregate partial, while a failed or `Unknown` property aborts and rolls back the pass. Name remaps, non-`keyof T` key spaces, inactive infer declarations, and other deep infer patterns ⇒ Deferred (never an unbound substitution). Then an `any` check (uses both branches) ⇒ Deferred; then the full relation authority `execute(SemanticQueryKey::Relate)` via `execute_relate_pair` (`Unknown` ⇒ Deferred). The O(tag) fast-reject prefilter lives INSIDE the authority — never consulted as a parallel truth source. A binding-producing selection is always TRUE: `build_conditional` substitutes the relation payload's fixed bindings into the true branch; the closedness classifiers consume a bare-infer binding the same way and widen non-bare shapes to the Deferred treatment (a conservative superset). True-selected classifies ONLY the true branch — an open LOSING branch is dead (guard: `bare_infer_extends_selects_true_through_the_shared_oracle`); False-selected classifies only the false branch; Deferred classifies check/extends value-sensitively plus BOTH branches. The classifier only INVOKES the oracle: operands resolve to nodes solely via environment-free interning (literals/primitives/`infer` placeholders), identity bindings (including default-bound params — guard: `defaulted_type_parameters_bind_their_default_identity` — and wrapper-forwarded bindings — guard: `binding_identity_selects_conditionals_through_concrete_arguments`), and own-scope named-ref resolution (`prepared.name_resolution` → interned `DeclRef`; guard: `closed_named_ref_operands_select_through_the_shared_oracle`); unresolvable operands ⇒ Deferred; the classifier never reimplements assignability and never materialises branches.
.claude/skills/type-resolution/SKILL.md:403:This full-subtree boundary is sound but conservative, and refusal is **aggregate-wide**. One semantically unresolved node in any scanned nested non-slice position refuses the whole reverse recovery; no unaffected property is recovered separately. For `S = { a: { value: string; (x: unknown): x is string } }` against `{ [P in keyof infer T]: { value: T[P] } }`, the nested predicate's `RawFallback` cannot affect `T[P]`, but the preflight refuses the entire aggregate and the conditional stays deferred. This is a false refusal (`Unknown`), never publication of a wrong recovered type.
.claude/skills/type-resolution/SKILL.md:424:**Conditional-infer identity and cyclic fixation.** Every authored `infer` declaration receives a canonical `InferBinderId` derived from the stable lexical `NodeScopeId`, the declaration or macro body's content-free `AuthoredBodyLocator`, and the declaration's exact typed child path. The identity is neither graph-allocation order, traversal order, nor a lossy digest, so eager, locator-shape, and structural lowering, spread-fragment lowering, re-lowering, unrelated demand order, and concurrent demand all converge on the same token. Anonymous/transient roots use a separate exact typed-root identity and remain ReturnOnly unless an authored locator is available. The wrapper is non-forgeable in production; only a test-gated synthetic constructor exists. Display names are not binder identity: one exhaustive typed syntactic traversal predeclares a conditional's `extends` binders for declaration sites and its true branch only, while ordinary sibling refs keep using the ordinary lexical environment; carrier arguments, template interpolations, signature constraints/defaults, and every other `TypeExpr` child position participate. Every `InferRef`, `InferBinding.param`, substitution, open-walk association, and reverse projection carries or compares the exact token/node identity. Nested same-name declarations therefore cannot intern together or deposit into one another's session. Inference sessions use the one-way `Collecting → StagedDeterministic → CommittedDeterministic` lifecycle, with `Collecting | StagedDeterministic → Abandoned`; only the innermost `Collecting` session is deposit-active. Fixation stores an immutable staged binding snapshot. Relation roots stage and immediately commit at their existing safe pop, while call candidates always push their own collector even when an outer collector exists and delay commit until mixed-SCC stability. A cyclic binding judgement stages and commits its session before SCC re-discharge, then re-enters the sole `execute(SemanticQueryKey::Relate)` authority for every SCC member, including negative non-binding consumers; publication requires the re-discharged polarity and exact `(param, bound)` binding snapshot to equal the provisional snapshot, otherwise the whole deferred batch is `ReturnOnly`. The transient redischarge frame is ReturnOnly and cannot recursively initiate another SCC discharge unless it is itself cyclic. A cold top-level binding relation uses a store-owned independent flight: it never joins another transaction's transient inference session, but retains the ordinary cooperative cancellation, invalidation, admission, retention, and reverse-index fences. Nested non-binding relations register ordinary family flights so concurrent top-level consumers may join their decided publication.
.claude/skills/type-resolution/SKILL.md:436:**Corpus forensics.** The two heaviest real-corpus components terminate fast off the backstop. `Table.vue`'s storm was eager Shallow decl-body lowering recursively executing member-value `Instantiate(StructuralTransit:Shallow)` across the transitive TanStack decl graph, entered once from the macro-payload eval — 94.3% of all budget charges compounding distinct instantiation keys — removed by the carrier-preserving decl-body lowering rule (see Query Mode Contract). `ChatMessages.vue`'s storm was the registry/publication Expanded demand pipeline materialising the open-conditional mapped slots surface — removed by the Navigate-only publication demand (component-meta publication routes record zero `Published(Expanded)` contexts; guard `publication_routes_never_demand_expanded`; ChatMessages resolves in seconds with 0 trips — the no-timeout tracker `chat_messages_resolves_without_timeout` in `defect_b_corpus_prevention_gate.rs` pins it). Both are COMPLETE corpus-set members with un-ignored green trackers (`table_resolves_complete_and_warm`, `chat_messages_resolves_complete_without_false_partial`). Supporting rails: the `TypeOf` query mirrors `Instantiate`'s key convention — an env-bearing content-free `ValueRootSlotIdentity` (T/L/J) plus a dedicated `TypeOfContext { projection_reduction, resolve_env_hash }` (R), derived solely through `ProjectSemanticDispatch::typeof_key_for` (memo slot via `context_to_slot`; key-shape parity with `KeyOf`/`MappedType`); `build_typeof` lowers the value's annotation / object shape / signature surface / enum surface AT the requested demand — a Skeleton/Navigate transit crossing `typeof` of a value typed against a large decl graph gets carriers, never an Expanded build-time materialisation of that graph (the `lower.rs` decl-body sites pass the ambient lowering context, the PathWalker typeof hop passes its own mode — a demand point, operator recursion passes the enclosing context, and the class-surface Static side stays a genuine-Expanded consumer; the overload-visibility projection rule in `build_typeof` is mode-independent; pinned by `typeof_value_graph_lowers_at_requested_demand` + `typeof_macro_payload_publication_stays_bounded`); `TypeOf` counts toward the projection fuse (a demand-bearing projection reducer is fuse-backed); and an admission-REFUSED complete materialise returns the COMPUTED value non-cacheably (`cache_suppress=true`, `result_is_partial` CLEAR — post-compute revalidation refusing a COMPLETE entry is benign non-cacheability, never a partial and never a fabricated `Tainted` substitute; pinned by `admission_revalidation_refusal_is_not_a_partial_result`).
.claude/skills/type-resolution/SKILL.md:442:3. The intrinsic/fallthrough `ProjectPath:Published:Expanded` corpus exception — defined by `dispatch_helpers.rs`'s Class-A Expanded fall-through `project_expr_class_a_via_dispatch_threaded` (reached directly or via its non-threaded wrapper `project_expr_class_a_via_dispatch`) and its live callers: `intrinsic_members_for_tag`'s intrinsic/fallthrough composition in `host_manage/intrinsic_projection.rs` (records `ProjectPath:Published:Expanded` charges on the real corpus — measured: 214 on a ChatMessages resolve), the value-expression evaluator in `resolver_core/fallthrough.rs`, the imported-alias registry refinement in `host_manage/component_meta_methods.rs`, the registry materialiser's route-target and `KeyOf` publication sites in `meta_resolve/registry_materialize.rs`, and the JSDoc payload resolution in `host_manage/jsdoc_resolve.rs`. These sites legitimately demand Expanded today and are out of the Navigate-only-publication rule's scope — `publication_routes_never_demand_expanded` covers the projector/registry macro surfaces only; the correct end-state converts this consumer set to Navigate.
.claude/skills/type-resolution/SKILL.md:443:4. The view-snapshot false-stale admission refusal — a live-session contributor first parsed mid-request (the `Icon.vue` slot-binding class) fails the materialise admission revalidation deterministically on the first cold request; the unadmitted-value protocol keeps the published result correct (the computed value flows back, non-cacheable, never partial) but warm caching is delayed by one request. The correct end-state snapshots the lazily-parsed contributor into the request view at `ensure_indexed_ready_serve` time (hermetic plain-view pin: `first_cold_request_admits_materialise_whose_contributor_parsed_mid_request`).
.claude/skills/type-resolution/SKILL.md:445:R6-registry guards for the publication surface: `chatmessages_resolvable_barrel_publishes_open_pick_as_shallow_carrier`, `closed_pick_sources_still_materialize_path_precisely`, `projection_budget_counts_instantiate_and_conditional`, `cycle_guard_roots_at_utility_source_type_argument`.
.claude/skills/type-resolution/SKILL.md:449:**Backfill rule.** Broader successful results may backfill narrower modes for the same key, but ONLY when a recorded materialised point dominates the narrower target (path-exact `cached_satisfies`, never enum rank): an `Expanded` result may satisfy `Shallow`, `Navigate`, and `Identity`; a `Shallow` result may satisfy `Identity` only — NOT `Navigate` (`Shallow ⊅ Navigate`: a one-shell Shallow surface does not materialise a `Navigate` next-hop); a `Navigate` result may satisfy `Identity`. Narrower successful results MUST NOT claim broader modes are cached.
.claude/skills/type-resolution/SKILL.md:451:**Cache topology.** The five modes do NOT imply five separate cache subsystems. `SemanticGraphStore` owns one semantic memo layer. At the semantic-contract level, mode is part of request identity; at the storage level, implementations should group same-base different-mode requests into one memo-entry family (or equivalent single-authority structure) keyed by the mode-erased semantic shape — operation, base identity, path / projection, substitutions, scope, version root — with per-mode slots or equivalent one-way upgrade/backfill semantics. Required behaviour: backfill is directional (broader-projection → narrower-projection) and gated by recorded-point dominance — `Expanded` may satisfy `Shallow` / `Navigate` / `Identity`, `Shallow` may satisfy `Identity` (NOT `Navigate`: `Shallow ⊅ Navigate`), `Navigate` may satisfy `Identity`, and no lower mode may claim a broader mode is cached. `Skeleton` is a separate policy slot; it does not alias `Navigate` or any other mode unless a typed equivalence proof and regression tests justify a backfill edge. Distinct mode requests must not duplicate in-flight authority or split into independent wait graphs.
.claude/skills/type-resolution/SKILL.md:453:**Cache-key distinctness.** `ProjectMember(C, "foo", Shallow)` and `ProjectMember(C, "foo", Expanded)` are distinct cache entries. Generic substitutions are part of the key: `MyType<string>` and `MyType<number>` never alias; two callers reaching `MyType<string>` through different entry paths dedup to one entry. Do not compress mode identity into booleans or partial encodings such as "is navigate"; store the exact `ProjectionMode` or a typed projection-policy key.
.claude/skills/type-resolution/SKILL.md:473:- Navigating `A['c']['full']['bar']` through an object surface touches at most three projection nodes plus the terminal; no unrelated siblings materialize.
.claude/skills/type-resolution/SKILL.md:479:Architectural target for the project-global cache cutover:
.claude/skills/type-resolution/SKILL.md:485:  - inspect already-materialized member or keyspace shape
.claude/skills/type-resolution/SKILL.md:492:  - populating shared caches from outside the shared semantic query API
.claude/skills/type-resolution/SKILL.md:494:- Any operation that can recurse, cross files, instantiate meaning, or produce a reusable cached result must be represented as a semantic subquery.
.claude/skills/type-resolution/SKILL.md:507:- If two callers reach the same instantiated semantic node through different entry paths, they converge onto the same cache entry.
.claude/skills/type-resolution/SKILL.md:508:- If two callers reach the same declaration name with different type arguments, they do not alias to the same cache entry.
.claude/skills/type-resolution/SKILL.md:511:- **Navigation-once invariant.** Navigating `type MyType<T> = …` performs at most one full body lowering per `(decl_identity, whole_hash)`. Later instantiations (`MyType<string>`, `MyType<number>`, …) reuse the parameterized lowering and run only substitution + terminal leaf materialization + branch selection. Distinct concrete instantiations never re-lower the body.
.claude/skills/type-resolution/SKILL.md:514:- **Utility fast-path rule.** Utility-specific fast paths are permissible only when all three clauses hold: (1) measurably faster than the generic dispatch path on a benchmark fixture, (2) observationally equivalent — same `SemanticNodeId` for the same inputs, same mode / cache behaviour, same `SemanticGraphStats` attribution — to the generic path, (3) emit the **same origin edges** the generic path would have emitted. A fast path that skips edge emission is not allowed; revert to the generic dispatch instead. Optimisation happens only after equivalence is test-enforced.
.claude/skills/type-resolution/SKILL.md:519:- If `ProjectMember(ResolveDecl(Box), "full", [C])` is already cached, a later path reaching the same instantiated query reuses it.
.claude/skills/type-resolution/SKILL.md:547:**First-class telemetry.** `SemanticGraphStore` exposes `SemanticGraphStats` as a public API. Per-`SemanticQueryKey`-variant counters (cache hits, misses, same-path-sentinel returns, in-flight peak, cross-thread-join wait time) and per-dispatch-builder counters (instantiations, conditional branch selections, budget/fallback invocations, path length p50/p95, projection depth p50/p95, origin edges emitted, origin edges per node p50/p95) are mandatory — not an optional observability pass. The trace-check harness, benchmark pipeline, and feedback-file report at track exit consume `SemanticGraphStats::snapshot()` directly.
.claude/skills/type-resolution/SKILL.md:560:- **Expand(`MyType`)** (T unbound): return a `Conditional` graph with both branches materialized. Shape retains `T`, `string`, `StringType`, `NotStringType`. Origin layer carries `Instantiate(MyType, [T])` and structural links into each branch.
.claude/skills/type-resolution/SKILL.md:573:- **Expand(`OtherType`)** (T unbound): object arm expands — `a: Foo` fully resolved, `b: T` preserves `T` as a parameter reference (origin links to the `[T]` parameter list). Conditional arm stays `Conditional` with both branches materialized. Intersection is NOT collapsed because the conditional remains open.
.claude/skills/type-resolution/SKILL.md:607:This is path-precise projection, not whole-branch expansion. Sibling members and unrelated branches are not materialized.
.claude/skills/type-resolution/SKILL.md:618:- **Expand(`Deep`)** (T unbound): outer check is open. Keep outer `Conditional` shell. In the True branch, inner check is also open; keep inner `Conditional` shell. In the False branch, materialize `{ kind: "other"; value: T }` with `T` preserved.
.claude/skills/type-resolution/SKILL.md:646:Populated once through the shared host ensure-path and cached in `FileArtifactStore`. Invalidated when the file's whole-hash changes.
.claude/skills/type-resolution/SKILL.md:667:request-bound `ResolverContext`. `ImportedRootDb` is the sole routed-target
.claude/skills/type-resolution/SKILL.md:670:from that routed declaration and executes through `ProjectSemanticDispatch`.
.claude/skills/type-resolution/SKILL.md:686:**Architectural decision:** `ProjectSemanticDispatch` +
.claude/skills/type-resolution/SKILL.md:687:`SemanticGraphStore` are the canonical lazy semantic layer and the sole
.claude/skills/type-resolution/SKILL.md:697:- `mod.rs` — `ProjectSemanticDispatch` struct + `SemanticQueryApi::execute` impl
.claude/skills/type-resolution/SKILL.md:717:- **Relation memo** — keyed by the full-identity `RelateMemoKey` (source / target / relation kind / policy / source freshness / inference context / env+substitution+projection-reduction context) for `Relate` judgements. Admission is decided-only: only the binary `Assignable { bindings }` / `NotAssignable` payloads publish (cache-with-fence); `Unknown`, `BudgetExceeded` (public payload, `cache_suppress`), session-local inference deltas, and abandoned sessions publish NOTHING — no memo entry, no fact signature, no reverse index.
.claude/skills/type-resolution/SKILL.md:721:Every spread-bearing object is a `SemanticNodeData::ObjectSpreadProgram(ObjectSpreadProgram)` — one immutable, source-ordered effect list (`DirectProperty` / `DirectMethod` / `DirectGet` / `DirectSet` / `DirectIndex` / `DirectCall` / `DirectConstruct` / `Spread`). It is the sole stored description of the object: no folded member surface, completeness mirror, replay cursor, or side log coexists. A non-spread object stays a closed `SemanticNodeData::Object` surface (every `SurfaceView` is closed by construction; the open-operand representation is gone). Direct effects carry lossless typed facts: typed key or computed-key child, value/signature children, optional/readonly, method/get/set kind, implementation-body fact, visibility, spans/declaration origin, and excess origin.
.claude/skills/type-resolution/SKILL.md:731:Finite enumerable union operands fork whole alternatives at their exact spread position, and later effects apply per branch, so correlation survives overwrites. Selector liveness prunes only shadowed recursive effects before recursion (`Key(x)` on `{...Self, x}` never enters `Self`; `{x, ...Self}` must). An alternative's public surface is positive evidence only: omission never proves absence, emptiness, exact `keyof`, exhaustive domain, or closed materialization. Those operations exist only on the sealed witnesses `ClosedObjectProjectionAlternative` / `ClosedObjectProjectionFormula`; an open or mixed formula cannot mint one.
.claude/skills/type-resolution/SKILL.md:745:**Canonical deferred forms** (plan §2 — only these variants cross any cache boundary):
.claude/skills/type-resolution/SKILL.md:770:**Recursion guard contract.** Per-call `in_flight: FxHashSet<SemanticNodeId>` + RAII pop + completion memo. Stack-local; dies with the call; NOT a host-owned cache:
.claude/skills/type-resolution/SKILL.md:787:`ProjectSemanticDispatch::execute`. No parser cache adapter stores or returns
.claude/skills/type-resolution/SKILL.md:792:- A second `impl SemanticQueryApi for ...` — FORBIDDEN (besides `ProjectSemanticDispatch`).
.claude/skills/type-resolution/SKILL.md:793:- A second relation entry point — FORBIDDEN (`execute_relate` in `project_semantic_dispatch::relation` is the sole authority; a bare-pair `relate_nodes` may not return).
.claude/skills/type-resolution/SKILL.md:812:| `arena.rs` | query-local node and primitive/literal carrier types; no solver caches |
.claude/skills/type-resolution/SKILL.md:819:resolve through `ProjectSemanticDispatch::execute(SemanticQueryKey::...)` and
.claude/skills/type-resolution/SKILL.md:843:- `ShallowTypeSymbol` / `ShallowValueSymbol` are SLIM HEADER views — they OWN no body product and hold NO `Arc<Lowered*Decl>` handle. `ShallowTypeSymbol` carries header facts only (`kind`, `type_param_names`, `member_names`, `contributor_count`); `ShallowValueSymbol` carries `kind`, `object_member_headers`, and the `.vue`-default provenance flag `is_synthesised_vue_default`. They are built directly from the `DeclHeaderIndex` (no body lowering), so `ShallowFileState::symbol(name)` / `value_symbol(name)` are header-only probes. The THREE demand surfaces are separate accessors on `ShallowFileState`: **body** — `type_decl(name) -> Arc<LoweredTypeDecl>` / `value_decl(name) -> Arc<LoweredValueDecl>` (the single `DeclBodyMemo`-owned lowered result, lowered on first touch; `value_decl` routes synthesised `.vue`-default bodies first); **dependency edges** — `type_deps(name) -> Arc<ClassifiedTypeDeps>` (per-state `local_deps` / `external_deps`, cached, edges ONLY — never a body); **augmentation bodies** — `augmentation_type_decl(scope, name)` / `augmentation_value_decl(scope, name)`. The synthesised `.vue`-default is an eager macro-producer `LoweredValueDecl` stored in a dedicated body map (read through `value_decl`), with its slim header in `synthesised_value_symbols`. Shallow same-file member readers consult the content-free body surface — `type_decl(name)` yields `LoweredTypeDecl` whose `body` is `TypeDeclBody` (`Single`/`Merged` contributor SLOTS only; no `lookup_object` / embedded `TypeExpr` on the prepare surface). Member-name inventory comes from the slim header (`member_names` / `DeclHeaderIndex`); typed body demand re-borrows authored bounds lease-only through the shared lazy lowering path and reduces on the graph, never via a prepare-surface object projection. `PreparedTypeDecl` records merged declarations as `body_facts.merged_contributor_slots: Arc<[TypeBodySlot]>` (empty = single) — ordered content-free body-slot LOCATORS, never embedded `TypeExpr` bodies (the whole `PreparedTypeDecl` is fact+locator `NoTypeExpr`); the slim shallow symbols are thinner still (header facts only).
.claude/skills/type-resolution/SKILL.md:873:**Overlay-aware index.** `AugmentationTargetKey.population: AugmentationPopulation {Base, Session(u64)}`. A `Base` scan reads `is_base()` artifacts only (base parse-env hash + current parser version); a `Session` scan reads the session's overlay artifacts (matched by the session overlay discriminator) UNIONED with base, keyed by the overlay-set fingerprint. **`Session(u64)` is the overlay-set fingerprint, NOT a raw session id** — the producer (the body stitch) and the route-surface validator (`HostStoreView::validates_route_surface_domain`) derive `(population, overlay_discriminator)` through the SINGLE shared `session_view::augmentation_population_for_view`, so a session view can never be cached as, or validated against, a base-only set. Overlay augmenters NEVER poison the base index and NEVER cross sessions. There is NO base-only `assert!(view.compat_token().session.is_none(), …)` on the augmentation-index surface — a session view is accepted under `Session` population.
.claude/skills/type-resolution/SKILL.md:885:2. It resolves each typed macro root through `ProjectSemanticDispatch` and
.claude/skills/type-resolution/SKILL.md:893:cache; underlying semantic query nodes retain their normal memo and
.claude/skills/type-resolution/SKILL.md:908:All query-time expansion for macro types, component-meta, and imported aliases
.claude/skills/type-resolution/SKILL.md:909:enters through `ProjectSemanticDispatch::execute`. Consumer-specific framework
.claude/skills/type-resolution/SKILL.md:927:  - `Identity`: declaration identity and canonical source location only. No body read, no shape materialization.
.claude/skills/type-resolution/SKILL.md:930:  - `Expanded`: recursive materialization of the requested result.
.claude/skills/type-resolution/SKILL.md:932:- `Skeleton` is a distinct mode, not a synonym for `Navigate`. It is currently scoped to cycle/generic-helper traversal — the materialization cycle gate's per-hop `Instantiate` (`ClassifyMaterializationCycleGate`, see `project_semantic_dispatch::cycle_gate`). New call sites must justify why they need Skeleton semantics instead of `Navigate` / `Shallow`.
.claude/skills/type-resolution/SKILL.md:937:- Parsing a `.ts`/`.js`/declaration file for type resolution must cache discovered symbol name -> canonical location mappings.
.claude/skills/type-resolution/SKILL.md:938:- Re-exported names and barrel hops must also be cached once discovered. If traversal follows `export * from './foo'`, cache that result so later lookups do not rescan the same barrel chain.
.claude/skills/type-resolution/SKILL.md:950:- OXC AST lowers to `TypeExpr` via `lower_ts_type(ts_type, source)` (in `verter_semantic::analysis::type_expr_lower`) at exactly two boundary classes: macro / JSDoc PRODUCER fields lower eagerly at their producer boundary (stored on `Analyzed*Field`, `ResolvedLocalType.type_expr`, surviving every cache); top-level DECLARATION BODIES lower LAZILY on first semantic demand through the scheduler-retained parse snapshot (`DeclBodyMemo` → `DeclLoweringService`), NOT eagerly during shallow analysis — an `IndexedReady` publish lowers ZERO declaration bodies. Either way the analyzer/lowering arm takes an OXC node it already holds; downstream stages walk the resulting `TypeExpr`, never re-parse or re-lower at query time.
.claude/skills/type-resolution/SKILL.md:953:- Workspace classification uses `ResolverContext::workspace_is_package_backed(canonical_id)` — the single structural predicate (workspace-owned is its complement; there is no separate `workspace_is_workspace_owned` predicate). Substring checks on canonical paths (`"/node_modules/"`, `"\\node_modules\\"`) are banned. The classification API is path-agnostic and handles symlinked / pnpm-hoisted / Windows-backslash / workspace-linked-package cases.
.claude/skills/type-resolution/SKILL.md:964:domain and the demand-lattice that decides cache satisfaction/backfill. Resolution is typed end to
.claude/skills/type-resolution/SKILL.md:967:- **One key → one value arm.** Every `SemanticQueryKey` maps to exactly one `SemanticQueryValue` arm.
.claude/skills/type-resolution/SKILL.md:974:- **Display is a projection.** Canonical display is computed at publish from the cached typed value,
.claude/skills/type-resolution/SKILL.md:979:  never warm-admitted. A fact-rooted error (a recorded missing-dep fact) IS cacheable. `admit_decision`
.claude/skills/type-resolution/SKILL.md:985:  behavioural guards land with STAGE-B: `cache_satisfaction_is_demand_lattice_not_enum_order` (U10),
.claude/skills/type-resolution/SKILL.md:986:  `cache_satisfaction_is_materialized_point_not_nominal_demand`,
.claude/skills/type-resolution/SKILL.md:988:  `error_tolerance_broken_input_is_returnonly_fact_rooted_error_is_cacheable`, and
.claude/skills/type-resolution/SKILL.md:995:**The `TypeExpr`→handle migration TARGET** is to replace stored `TypeExpr` bodies on the hot parse/shallow/macro/lazy-body/prepared path with interned graph handles, materialising `TypeExpr` ONLY at compat/output boundaries. **Landed so far:** the macro hot mirror (Stage 5A) and the single bounded `decl_body_hot_ref` declaration-body hot-read anchor (Stage 6 Option B) are handle-native; the prepared declaration surface is fact+locator NoTypeExpr end-to-end (the lower-crate `Prepared*` DTOs carry classification facts + content-free body locators, the session builders copy memo-owned facts, and the superseded bundle-stored handle mirror `HotPrepared*` is DELETED — handles mint on demand at the dispatch boundary, never stored in the bundle); `DeclBodyMemo` records are facts+locators (`LoweredValueDecl` fully narrowed + NoTypeExpr-witnessed; authored bodies re-borrowed lease-only on demand), with the residual body readers at the terminal partition (1 `GraphBackedMigrated` + 6 `ProducerLowering` permanent transient ingress + 0 `AuthoredShape` + 5 `GraphFreeDto` + 0 `GraphBackedPending`; the reader-class debt row is CLOSED — the ledger is a permanent ratchet, not open debt). The two former stored type-parameter `TypeExpr` pockets are CLOSED (the type-parameter-bound confinement block): `LoweredTypeDecl` is wholly `NoTypeExpr` — the stored full `type_parameters: Vec<TypeParam>` is deleted; the `narrow_type_parameters` mirror is the sole stored authority (consumed by the prepared-decl builder and the external frontier, which content-free re-anchors the mirror's bound slots to the frontier symbol so `export default` behavior is preserved), and the locator/binder deref (`locator_deref.rs`) uses the mirror for ordinal/name/bound-presence authority while re-borrowing bound CONTENT + the full sibling frame lease-only via `transient_type_parts`. `TypeParamBinding` is shrunk to the content-free `(name, ordinal)` fact pair (`NoTypeExpr`); its `<script setup generic="…">` bounds are re-borrowed at query time through ONE artifact-local transient producer over the pinned `IndexedReady` and lowered by ONE dispatch helper shared by both content readers, a missing/stale re-borrow failing as a typed cache-suppressed miss, never a bound-free fabricated binder. Carrier production reach: the macro type-argument carriers (`TypeOf`/`BareRef`/`ImportType`) now flow through production via the **macro hot mirror** (Stage 5A, LANDED — see "Macro Hot Mirror" below). The eager `lower.rs` `TypeExpr::TypeOf` arm still RESOLVES typeof EAGERLY for the NON-macro declaration/body path (the LANDED Stage 6 Option-B flip mints handles in `decl_body_hot_ref` (over the `Instantiate` result the producer drives via the resolving lowerer) and does not route decl bodies through the structural lowerer; routing decl-body carriers structurally would be the separate deferred query-free declaration-body structural-template producer, NOT Stage 6, NOT landed); the macro-arg PRODUCER is the query-free structural lowerer (`structural_carrier_producer/macro_arg_producer.rs::lower_type_expr_structural`, a bare module-private fn in the single private producer module `crate::structural_carrier_producer::macro_arg_producer`), whose SOLE production caller is the session macro hot-mirror builder in the same module (every other `new_typeof` / `new_bare_ref` / `new_import_type` call site is test-only). The carrier reduction arms (`TypeOf`) and head-resolution helpers (`BareRef`/`ImportType`) resolve a carrier head through the shared `resolve_*_head` helpers consuming `CarrierResolverContext`, at the resolving DEMAND. The carrier types:
.claude/skills/type-resolution/SKILL.md:997:- **`HotTypeRef`** (`semantic_query.rs`) — the internal session hot handle wrapping a `SemanticNodeId` in the `semantic_query_memo` arena. `Send + Sync`; deliberately NOT `Hash`/`Ord`, so it can never be a cache key (R6). Distinct from the public content-hash DTO `component_meta_payload::TypeHandle`.
.claude/skills/type-resolution/SKILL.md:998:- **Declaration-body hot read path (Stage 6 Option B, LANDED)** — the LANDED hot-read surface is the thin shared accessor `decl_body_hot_ref` (`project_semantic_dispatch/mod.rs:1455`), which wraps the `SemanticGraphStore` `Instantiate` memo (handle minted at the dispatch boundary over the `Instantiate` query result — `build_instantiate`'s post-processed `SemanticNodeId`: the graph-bearing helper `lower_decl_body_with_provenance` produces the resolving-lowered body-SHAPE via the RESOLVING lowerer, interning a producer-owned `MergedDecl` / `Intersection` wrapper in its branches where applicable, and `build_instantiate` post-processes that body-shape — member-index backfill + cross-file augmentation stitch — into the final `Instantiate` node; not a re-lowering) and is read by the ONE migrated graph-backed reader anchor `lower_decl_body_to_node` (`meta_resolve/projectors/macro_payload_substrate.rs`, the SOLE `GraphBackedMigrated` reader — the 1 migrated row of the 12-row residual `TypeExpr`-body-reader inventory (1 `GraphBackedMigrated` / 6 `ProducerLowering` / 0 `AuthoredShape` / 5 `GraphFreeDto` / 0 `GraphBackedPending`), beside a separate 5-row output/compat table). Handles mint ON DEMAND at that dispatch boundary and are NEVER stored in the prepared-decl bundle — the superseded bundle-stored handle mirror (`HotPrepared*`) is DELETED. The prepared surface is fact+locator NoTypeExpr end-to-end: `PreparedTypeDecl` carries `body_facts` (classification + body slot + merged-contributor slots), `member_index: PreparedMemberFact` (content-free member-value locators + span origins), `wrapper_shape`, `projection_class`, and NARROW type-parameter facts; `PreparedValueDecl` carries `ValueTypeAnnotationFact` + signature/shape/enum facts. The session builders COPY the memo-owned classification facts (produced once at lazy lowering from the transient contributor bodies via the shared `verter_semantic` classifiers). `DeclBodyMemo` records are facts+locators end to end (the two former memoized type-parameter pockets are CLOSED: `LoweredTypeDecl` is wholly `NoTypeExpr`-witnessed — the stored full `type_parameters` is deleted, the `narrow_type_parameters` mirror is the sole stored authority, and bound content re-borrows lease-only; `TypeParamBinding` is the content-free `(name, ordinal)` fact pair, likewise `NoTypeExpr`) — `LoweredTypeDecl.body` is `TypeDeclBody` (`Single(TypeBodySlot)`/`Merged` contributor SLOTS) plus memo-owned classification facts, and `LoweredValueDecl` is fully narrowed (annotation/signature/shape/enum facts + the memo-owned value `body_hash`), compile-witnessed by `#[derive(verter_no_typeexpr::NoTypeExpr)]`; the locator-deref worker re-borrows authored bodies lease-only from the retained snapshot (transient type-body / value-part / JSDoc-typedef services) and navigates value-space paths (`ValueSignature`/`FunctionParam`/`FunctionReturn`/shape-member steps). Residual SEMANTIC reader classes are recorded in `docs/arch/authored-shape-graph-native-migration-deferral.md`; a hot consumer must never go `HotTypeRef → TypeExpr → semantic decision`. (Decl bodies still LOWER to typed IR transiently; only facts/locators are stored, and only the hot READ surface is handle-native.)
.claude/skills/type-resolution/SKILL.md:1002:- **`CarrierResolverContext`** (`project_semantic_dispatch/carrier.rs`) — the RUNTIME / value-side bundle a `BareRef`/`ImportType` carrier needs to resolve at demand time (env / scope / name_resolution / scope_payload / shadowing / reduction-demand axis). NEVER a query key — it borrows its inputs and derives no `Hash`/`Eq`. The mutable substitution accumulator and the dispatcher-local active-instantiate stack are threaded separately; the augmentation scope is derived from `scope` + the resolver.
.claude/skills/type-resolution/SKILL.md:1003:- **Output-materialization capability fence** (`project_semantic_dispatch/output_materialization.rs`, block 8-A3) — graph-node → `TypeExpr` reverse materialization for the TRUE OUTPUT SINKS (Kind A) is a SEALED-TRAIT CAPABILITY. The callable boundary is the sealed `OutputProjector` trait (NOT inherent methods on `ProjectSemanticDispatch`); its two methods take a `SemanticNodeId` and hand back SEALED CARRIERS, never a bare `TypeExpr`:
.claude/skills/type-resolution/SKILL.md:1004:  - **`OutputProjector::materialize_output_type_expr(SemanticNodeId) -> Option<OutputTypeExpr>`** — the PLAIN, shell-only boundary. Wraps the module-private `raise_node_to_type_expr` (via the `pub(super)` raise-side seam `output_shell_raise_sealed`, which returns the SEALED `Option<OutputTypeExpr>`) into a sealed `OutputTypeExpr` carrier: every carrier round-trips through it (raw-fallback text → `Unknown`, synthetic binding, constructor carrier, `RecursiveRef` back-edge; tuple-rest rides `TupleElement.rest`). It does NOT reduce operator shapes — an operator node (`IndexedAccess` / `Conditional` / `Mapped` / `KeyOf` / `TypeOf`) raises un-reduced. `None` is the un-folded miss. Tests: `carrier_materialize_tests.rs`.
.claude/skills/type-resolution/SKILL.md:1005:  - **`OutputProjector::materialize_reduced_output_type_expr(SemanticNodeId, ProjectionReductionContext) -> MaterializedOutputTypeExpr`** — the PROJECTION-output boundary: reduce-under-context FIRST, then raise (via the `pub(super)` `raise_and_reduce_with_context`, called directly). An operator node collapses before being raised, distinct from the plain boundary. Returns the sealed `MaterializedOutputTypeExpr`: a PRIVATE inner `OutputTypeExpr` `type_expr` payload (capability-gated unwrap) PLUS the readable facts-rail metadata (`node_id()` / `dep_signature()` / `result_is_partial()`). The reduce-vs-no-reduce discrimination over a `typeof f<string>` carrier is asserted in `carrier_reduction_tests.rs`.
.claude/skills/type-resolution/SKILL.md:1006:  - **Sealed carriers + payload vault + terminal-sink capability locality.** `OutputTypeExpr` / `MaterializedOutputTypeExpr` keep their inner `TypeExpr` in a deeply-private nested `carrier::payload` vault — NO readable `TypeExpr` field outside that vault, NO `pub` `Deref` / `AsRef<TypeExpr>` / `into_inner`; the only readers are the capability-gated `into_type_expr(self, &impl OutputProjector)` / `type_expr(&self, &impl OutputProjector)`. The capability is implemented (via EXPLICIT `impl OutputProjector` pairs in the private `projector` module) ONLY for the EIGHT true-output-SINK capabilities — one per exact output-sink module that projects, NOT one per subtree: `MetaResolveProjectorsOutputCap` (DEFINED + minted in the dedicated TERMINAL sink submodule `meta_resolve::projectors::output_sink`, re-exported at `meta_resolve::projectors::MetaResolveProjectorsOutputCap` for the owner impl to name — the submodule was extracted EXACTLY so the parent `projectors`' NON-sink helpers (`macro_payload_substrate`, `published_reducer`, `define_shapes`, the per-kind projector children) cannot mint; ALL boundary-consuming publication functions — `member_shape_peek_or_compute`, the sink-private `reduce_field_value_node` (successor of the deleted TypeExpr reducer `reduce_field_type_expr_with_mode`), and the publication APIs `surface_member_to_expanded_field` (which consumes a policy-admitted `&AdmittedPublishedMember` token) / `project_model` / `reduce_published_field_types` — live INSIDE `output_sink` so the raw `shell_raise_to_type_expr` / `unwrap_materialized` / `seal_type_expr` primitives can be MODULE-PRIVATE; the sink exposes ONLY published-DTO operations, never a bare `TypeExpr`. The COMPLEMENTARY INPUT-authority leak at the Kind-A PUBLICATION boundary — non-sink code forging a `SurfaceMember`/node and pairing it with a cursor to reverse-materialize a member `TypeExpr` — is closed by the sealed admitted-token chain in the sibling `meta_resolve::projectors::publication_authority` module (`ResolvedMacroPayload`/`ResolvedPayloadSurface`/`SurfaceMemberCandidate`/`AdmittedPublishedMember`, private fields + a private `Seal`, minted only by the admission fns; the framework-surface `ResolvedVueSurface` + `SvelteResolvedSurface` tokens drive the shared normalizers through the sealed `ResolvedSurfaceAccess` trait whose supertrait seal is a bare module-private `trait` in `resolved_surface_access.rs` where BOTH impls live — a `framework_surface` sibling `impl` is `E0603`, pinned defense-in-depth by `resolved_surface_access_impls_are_exactly_the_two_tokens`) as the COMPILER primary, pinned by the STRUCTURAL field-closure cross-sink guard `cross_sink_raw_authority_to_type_expr_boundary` (the Kind-A / PUBLICATION bar; the former Kind-B raise-then-decide sites are RETIRED — they decide on the node-domain `RaisedShapeFacts` / interned `RaisedShapeKey` and materialise once at a registered sink, the retired bridge symbol's absence tripwired by the lean tombstone `retired_kind_b_bridge_symbol_absent_from_production_source`)), `MetaResolveFieldTypesOutputCap` (`meta_resolve::materialize::field_types`), `HostManageComponentMetaOutputCap` (`host_manage::component_meta_methods`), `TypeinfoRaiseOutputCap` (`typeinfo::raise`), `TypeinfoSvelteSurfaceOutputCap` / `TypeinfoVueSurfaceOutputCap` (`typeinfo::framework_surface::{svelte_exec,vue_exec}` — `vue_exec`'s whole reachable scope, `vue_exec` + its `normalize` child, IS output-only, so the single cap is correct), `MetaQueryRegistryOutputCap` / `MetaQuerySurfaceOutputCap` (`component_meta_query_engine::{registry_decl,surface}`) — each a private-field type whose `new()` constructor is `pub(in <sink-module>)`. `pub(in P)` grants the mint to `P` AND every module at-or-under `P`, so the mint scope is scoped to a TERMINAL output sink whose entire reachable production module tree is itself output-only — that (not "per-leaf") is what makes the fence compiler-enforced for in-subtree code: a non-sink sibling (`meta_resolve::dispatch_helpers`, `host_manage::eval_env`) — or a non-sink helper sibling — is NOT reachable from any sink's mint scope, so a planted `*OutputCap::new` there is `E0624`. The honest guarantee: the PRIMARY barriers are COMPILER-ENFORCED — in safe production Rust OUTSIDE the payload vault a hot / session / Kind-B mint is `E0624`/`E0451`, a hot carrier-unwrap is `E0277` (the trait is sealed) AND the inner `TypeExpr` is not even a readable field, and a hot `.type_expr_for_test()` is `E0599` (the carrier `_for_test` accessors are `#[cfg(any(test, feature = "test-support"))]` — production-unreachable, COMPILE-ABSENT from every production build incl. debug; `debug_assertions` would have been present in debug builds). The residual TRUSTED surface — the inline payload vault + projector registration source (and the by-name identity of which owner types are sinks) — is the part the compiler cannot itself police; the claim does NOT cover guard deletion, deliberate edits inside that vault, or unsafe code unless the crate forbids unsafe globally. The raw `raise_node_to_type_expr` stays module-private; `raise_and_reduce_with_context` stays `pub(super)`; the shell output seam `output_shell_raise_sealed` returns a SEALED `Option<OutputTypeExpr>` (never a bare `TypeExpr`) so a `project_semantic_dispatch` sibling cannot launder via the `pub(super)` seam. The FFI boundary is the session-owned BYTES facade `VerterHost::project_node_to_type_expr_json_bytes` (the old `project_node_to_type_expr` + the all-`pub`-field `MaterializedTypeExpr` are DELETED). Over the BOUNDED trusted surface the `syn` guards are DEFENSE-IN-DEPTH (not the primary barrier), shaped as a CLOSED structural allowlist: the EXACT owner-file module topology (inline `projector` / `projector::sealed` / `carrier` / `carrier::payload` and nothing else, with item/impl/trait-position macro invocations, `include!`, unknown attributes, a `sealed::Sealed` alias `use`, and any owner-file `TypeExpr` alias BANNED) + the sanctioned sink set (the explicit `impl OutputProjector` / `impl sealed::Sealed` self-types, compared by FULL self-type path as a MULTISET — the dup-last-ident gap closed) by `output_projector_owner_registration_inventory`; a closed item/signature allowlist over the carrier/payload vault (every fn returning `TypeExpr` must be capability-gated or exactly test-gated) by `output_carriers_have_no_inherent_typeexpr_escape_method`; every carrier/payload struct field private regardless of spelled type by `output_carrier_payload_fields_are_private`; an accidental-regression CANARY (NOT proof-complete) for the common `Deref<Target = TypeExpr>` / `AsRef<TypeExpr>` / `Borrow<TypeExpr>` trait escapes in `src/project_semantic_dispatch/output_materialization_guards.rs` — completeness for the unbounded escape-trait surface comes from the payload vault, not the finite trait list; the out-of-crate visibility boundary by the trybuild `output_projector_non_owner_impl_is_compiler_sealed`; the terminal-sink mint scope by `output_cap_mint_scope_is_per_leaf_not_subtree` (a Rust-VISIBILITY reachable-module-tree model — for every `mint: pub(in P)` it builds the production module tree at P-and-below, excluding `#[cfg(test)]` modules, and default-DENIES any reachable module not on the cap's exact sink-module allowlist) + the walker self-test `mint_scope_module_tree_walker_self_test_discriminates`; the COMPLEMENTARY INPUT-authority boundary — a non-sink fn pairing a forgeable raw-authority subject (`SemanticNodeId` / `SurfaceMember` / `SurfaceView` / `VueMacroSurface` / `TypeInfoSurface*`) with a `TypeExpr`-bearing output — by the STRUCTURAL cross-sink transitive guard `cross_sink_raw_authority_to_type_expr_boundary` (a structurally-complete — vs the old name-based pin — residual SUPPLEMENT behind the sealed-token compiler primary, NOT a replacement: the production completeness guarantee is the sealed token, this scanner is the residual cross-module pairing supplement): it decides "TypeExpr-bearing" by FIELD-CLOSURE from `TypeExpr` over the type field graph — following struct fields / enum variants / `Vec`/`Option`/`Arc`/`Box`/tuple element types / `type` aliases across `verter_session` + the cross-crate seed homes — NOT a DTO-name list, and fails any reachable production fn across the registered sinks pairing a forgeable input with a `TypeExpr`-bearing output outside a closed sink-local allowlist of raisers + the token-minting projector callers; type identity is GENUINELY MODULE-QUALIFIED `(module, name)` — the closure graph is keyed by `TypeDefId { module, name }` carried through an 80/20, FAIL-CLOSED identity classifier (NOT a complete Rust name resolver) for the CURRENT production reference shapes (terminal architect ruling `8a3i2-consult-8020-terminal`; accepted EDGE-only final-state residual, recorded in the colocated section-header record in `crates/verter_source_policy_gate/tests/cases/output_projector_residual_guards.rs`). It covers the COMMON in-tree paths: own-module defs; rooted `crate`/`self`/`super` direct matches — the candidate's real module a SUFFIX of (or EQUAL to) the qualifier (relative `crate`/`self`/`super` rebased onto the referencing module; a `super` never escaping above the crate root; a too-short ANCESTOR prefix is NOT a direct match), where an UNROOTED first segment the file's `use`-index SHADOWS is re-resolved through the shadow binding (so `use crate::other as publication_authority` cannot bless `publication_authority::X`); EXACT-target `pub`/`pub(crate)` RE-EXPORTS whose TARGET module is the candidate's real home EXACTLY, never suffix slack (a cross-file `pub`/`pub(crate)`-ONLY re-export index — narrow `pub(self)`/`pub(in …)` re-exports are NOT recorded — keyed by the NORMALIZED absolute written path keeps genuine re-exports — `semantic_query::BudgetExceededFailure`, the two-hop `raise::MaterializedOutputTypeExpr`, the cross-crate `verter_semantic::analysis::*` — resolving); ordinary file imports (a `use … as Alias` import whose TARGET resolves by proof); and the audited intra-crate `use`-binding CHAIN at the normalized qualifier module (a module-scoped, intra-crate-only, non-glob, module/descendant-visibility, cycle-bounded use-binding graph — the genuine `registry_decl` `super::ResolvedTypeDeclaration` chain through the parent `component_meta_query_engine`'s private `use super::declaration_metadata::ResolvedTypeDeclaration`; an unsupported `use` form contributes no binding => Unresolved). A COLLIDING name (`crate::semantic_query::IndexSignature` — SemanticNodeId fields, the authority seed — vs `verter_type_expr::IndexSignature` — TypeExpr fields, an already-lowered-IR bearing leaf) is disambiguated into DISTINCT ids the same way, same-name re-export aliases collapse onto their target, and a reference the classifier cannot resolve to a single target stays `Unresolved` and is caught FAIL-CLOSED at the boundary completeness checks. The RESIDUAL forged shapes are OUTSIDE the proof claim — ACCEPTED EDGE-only final-state residual (the sealed-token compiler primary is the production guarantee; each sanctioned token is uniquely named so an over-resolution lands on the single genuine def; NOT a tightening backlog), disclosed by ROOT-CAUSE CLASS (complete-by-construction, NOT a per-instance list; the colocated section-header record in `crates/verter_source_policy_gate/tests/cases/output_projector_residual_guards.rs`): **Class A (syntactic `use` collection)** — all three `use`-collectors (`collect_use_index` / `collect_reexport_index` / `collect_use_binding_index`) are syntactic: none evaluates item-level `cfg`/`cfg_attr` and the file-import collector also ignores module nesting, so a cfg/cfg_attr-gated `use` (and, for file imports, a `use` inside an inline `#[cfg(test)] mod`) over-contributes across all three rails (the `mod_is_cfg_test` skip is the SINK-FN collector's, NOT these); **Class B (non-proof bare-name fallback)** — the unqualified arms resolve by uniqueness/first-match when a unique proven target isn't found: the `candidates.len()==1` fallback for a no-import name, an ambiguous multi-target `use` (`unique_path` None for >1), AND a unique single-segment self-import (`use Foo;`, recursion-guard skip); the use-binding chain returns the FIRST accessible resolving target (not single-proven); and an unrooted-unshadowed qualifier raw-suffix matches — all landing on a uniquely-named token's single genuine def — with a fail-closed anti-vacuity rail over the `(module, name)`-keyed safe-input / construction-chain tokens (a missing/moved token fires; a bare-name collision is no longer "accepted because one is bearing"); the dual-bearing defense is a DIRECT carve-out (the bearing-output-skip fence keeps a wrapper that DIRECTLY co-holds a resolution-authority seed in the forgeable set — the carve-out stays DIRECT, the 20-FP fence) plus a TRANSITIVE soundness tripwire (`forgeable_input_fence_has_no_dual_bearing_type` uses a transitive raw-authority reach on its seed side, since the tripwire needs soundness, not FP-freedom), and the tripwire's sanctioned-carrier exemption is keyed by QUALIFIED `(module, name)` (a wrong-module same-name token FIRES); BOTH sides are fail-closed on an unclassifiable PascalCase ident, and the non-authority exemptions are QUALIFIER-AWARE — a `(module, name)` Qualified entry (anti-vacuity-checked) or a non-field-bearing CATEGORY entry (trait bound / generic-or-assoc / non-collected external) carrying APPROVED qualified homes, matched against the `Unresolved` ref's PATH not its bare final segment (a forged `evil::Span` FIRES; a one-segment generic is benign; a one-segment trait-bound/external is exempt only with no same-name collected def); the safe-input set is SPLIT into policy-admitted publication tokens vs pre-admission construction-chain structs (a pre-admission chain struct taken directly fires); and the sink-fn collector is inline-mod-aware via a module-path stack); the admitted tokens' private fields + private `Seal` by `admitted_tokens_have_private_fields_and_seal`; the authority-callable scopes' no-`unsafe` ban (a transmute could fabricate a token) by `authority_scopes_contain_no_unsafe`; the carrier `_for_test` test-support gate by `carrier_for_test_accessors_are_test_support_gated_not_debug_assertions`; the mintable `TestOutputCap` staying `#[cfg(test)]`-gated by `test_output_cap_not_visible_or_mintable_in_non_test_builds`; the sealed raise seam by `raise_output_seam_returns_sealed_carrier_not_bare_type_expr` (its scan recurses non-test modules + transitive `TypeExpr` aliases, and now pins that NO public/restricted raise.rs fn returns a bare `TypeExpr` — the retired Kind-B bridge leaves no sanctioned exception); the retired Kind-B bridge symbol's absence by the lean tombstone `retired_kind_b_bridge_symbol_absent_from_production_source`. The `HotTypeRef`-shaped `materialize_type_expr` is a `#[cfg(test)]`-only harness (pinned by `materialize_type_expr_is_not_production_visible`).
.claude/skills/type-resolution/SKILL.md:1008:- **The single `QueryError` disposition authority** — `project_semantic_dispatch::query_error_disposition::classify_query_error` is the ONE exhaustive match over every `QueryError` variant. It yields the coarse `QueryErrorDisposition` (`OptionalAbsence` = `Miss`/`RaiseMiss`; `RecursionCarrier` = `RecursiveRef`; `ExpandableDecl` = `DeclPlaceholder`; `ControlCarrier` = `AliasCycle`/`RaiseAliasCycle`/`TypeParamCycle`; `UnsupportedSurface` = the two `Unrepresentable*`; `Partial` = `BudgetExceeded`/`Cancelled`/`UnstableState`; `Failure` = `Other`/`UnsupportedIntrinsic`/`ValueDomainMismatch`) PLUS the precise published `ClosedLiteralDomainUnresolvedReason` — one match, so the class and the reason cannot drift. `QueryError::is_error_type` (the §22 error type) and `node_is_unknown_materializing_failure` (which `Opaque` carriers are publishable) are DERIVED from it; no site re-lists the arms. `DeclPlaceholder` never publishes `MissingDependency` (a found-but-unmaterialized declaration is not "not found") and `UnstableState` publishes `RevisionMismatch`, not `Fault`.
.claude/skills/type-resolution/SKILL.md:1011:Guards: `no_verter_semantic_to_verter_session_dep`, `synthetic_binding_identity_is_content_free`, `carrier_constructors_do_not_use_unknown_as_control_flow`, `hot_type_ref_is_distinct_handle_and_not_hash_or_ord_derived`. Round-trip + discrimination tests: `crates/verter_session/src/project_semantic_dispatch/carrier_materialize_tests.rs`.
.claude/skills/type-resolution/SKILL.md:1021:or query identity. `macro_type_arg_hot_ref` remains the sole producer entry.
.claude/skills/type-resolution/SKILL.md:1068:The **macro hot mirror** (`crate::structural_carrier_producer::macro_arg_producer`, the single private producer module) is the SOLE production producer of a macro type-argument's structural carrier graph. `MacroHotMirror` lives on `IndexedReady`, keyed `(owner, whole_hash, macro_index)` → `MacroHotProduct` (lazy / singleflight / content-addressed `OnceLock<Option<Arc<MacroHotProduct>>>` per macro index); its `hot` field is the historical graph handle. `macro_type_arg_hot_ref(ctx, file, macro_index)` is the sole production entry. The mirror is PURE: it performs NO host route lookup and emits NO dependency facts — it only produces the UNRESOLVED structural carrier graph (the `BareRef` / `ImportType` / operator-shell carriers, resolved on demand at the consuming dispatch) via the mode-neutral `lower_type_expr_structural`, plus the graph-free authored-head sidecar described above. Script-setup `generic="T"` binders are SEEDED at build (lower to the `SemanticNodeData::TypeParam` binder, not `BareRef(T)`); macro-own-body provenance (`declared_in_macro_type_arg`) is baked at production time. Direct dispatch lowering of an authored SFC macro shell must likewise use `lower_type_expr_in_owner_scope_with_mode` with the macro's recorded owner. The owner-agnostic convenience entry denotes the ordinary/module owner; it cannot infer that a synthetic test expression came from `<script setup>`, and callers must not compensate with file-wide or get-any owner matching.
.claude/skills/type-resolution/SKILL.md:1070:**Single-producer guarantee — TWO confinement regimes + bounded same-module policing.** The producer-capable code is COLLAPSED into ONE private module, `crate::structural_carrier_producer::macro_arg_producer`, declared as a private `mod macro_arg_producer;` that re-exports EXACTLY `pub(crate) use macro_arg_producer::{macro_type_arg_hot_ref, MacroHotMirror};`. The THREE producer-capable builders — `lower_type_expr_structural`, the macro hot-mirror builder `build_macro_hot_ref`, and the `<script setup generic="…">` binder-seed builder `build_script_setup_seed_frames` — are EACH a BARE module-private fn (no visibility modifier) inside that one file. (1) The FOREIGN case is COMPILER-CONFINED: no module outside `macro_arg_producer` can NAME any of the three builders — a foreign reference is a compile error (E0603 / E0433), so a second producer in a foreign file is unrepresentable by construction. (2) The SAME-MODULE case is NOT compiler-confined — Rust privacy is module-scoped, so a SECOND producer written INSIDE `macro_arg_producer.rs` CAN name the module-private builders, and the collapse to one file does not make that a compile error. That same-module residual is POLICED by the strengthened single-producer architecture guards, which together cover the 8-category same-module exposure surface: `structural_carrier_producer_lowerer_is_module_private` pins ALL THREE builders bare-private + single-defined + not re-exported; `macro_hot_mirror_exposes_single_crate_visible_producer_entry` is the producer-EXPOSURE collector — `macro_type_arg_hot_ref` is the ONLY crate-visible producer entry, covering module-level FREE fns AND INHERENT-impl associated fns under a cfg-SATISFIABILITY test-gate classifier (an item is test-only ONLY when its `#[cfg]` ENTAILS test — `#[cfg(any(test, debug_assertions))]` / `#[cfg(any(test, feature = "x"))]` are PRODUCTION-satisfiable and COUNTED, closing the prior over-exclusion), PLUS crate-visible VALUE exposure (`const`/`static`/associated-const fn-pointers of a builder), PLUS TRAIT exposure (only the hand-written `Debug`/`Clone` for `MacroHotMirror`; any other trait impl / trait def/alias reds), PLUS the EXACT `mod.rs` re-export-shape pin (private module decl + exactly the two-leaf `pub(crate) use`, no alias/glob/extra leaf/widened decl); `macro_arg_producer_has_no_production_expansion_surface` bans ALL production bang-macro invocations (a denylist is incomplete because a macro defined elsewhere and invoked here is invisible to `syn`; the one `matches!` and the one `vec!` are de-sugared to keep the file bang-macro-free) plus `macro_rules!` / proc-macro attributes / `#[macro_use]` / out-of-line-or-`#[path]` child mods (only `#[cfg(test)] #[path] mod *_tests;` wiring is allowed) plus the SCOPED derive rules — derive paths must be single-segment built-ins (a qualified `evil::Debug` reds) and NO production import / glob / `#[macro_use]` may shadow a built-in-derive name into scope (the module KEEPS its `#[derive(Debug, Clone, …)]`); `session_graph_lowerer_makes_no_query` + `macro_hot_mirror_producer_is_pure_no_route_resolution` ban the QUERY/DISPATCH route (`.dispatch(`, `lower_type_expr_in_scope_with_`, `ProjectSemanticDispatch`, `SemanticQueryKey`, the route-resolving `ensure_indexed_ready(` — DISTINCT from the allowed route-free `ensure_indexed_ready_serve`, prepared-decl/route helpers) — a producer that route-resolves would be a SECOND query-time resolution engine; and `no_production_macro_arg_eager_lowering_outside_mirror` is the file-scope ordering tripwire. The IRREDUCIBLE residual NOT covered by either regime is trust in the one sanctioned producer implementation plus compiler bugs / build-time substitution / out-of-tree proc-macros — by design. The FOUR production graph-lowering uses of a macro `parsed_type_argument` all read the ONE mirror handle and re-enter the shared dispatch from it for their TERMINAL demand (no site lowers the macro arg a second time): the `resolve_macro_payload` projector + silent-miss probe (`meta_resolve/projectors/mod.rs`), the slot-binding extractor (`meta_resolve/slot_binding_graph.rs`), the `vue_exec` `defineSlots<mapped>` / graph-native indexed-access decompose (`typeinfo/framework_surface/vue_exec/mod.rs`), and `eval_env` `expand_macro_types` + `defineModel` (`host_manage/eval_env.rs`).
.claude/skills/type-resolution/SKILL.md:1072:**Demand-site carrier resolution + dep recording (codex-ruled "A/C, not B").** The mirror is query-free, so it DEFERS the resolution eager lowering did eagerly (TypeOf value-root execution, Pick/utility source resolution, cross-file resolution-witness recording). Per the architecture ruling, resolution + dep recording happen at the resolving DEMAND, never as a side-band dep preflight on the mirror accessor (a second traversal policy is forbidden). The demand sites: `resolve_carrier_subject_node` (`carrier.rs`) resolves a `TypeOf` carrier subject (the empty-path `typeof config` macro payload — the walker's per-segment `TypeOf` arm never fires for an empty path) via `typeof_key_for` → `build_typeof` (resolve → project the carrier path in `Navigate` → apply `type_args`); a NON-empty `ProjectPath` path leaves the TypeOf base for the walker's mid-walk arm. The deferred-shell evaluator (`evaluate.rs`) resolves a `BareRef`/`ImportType` source carrier (Pick/Omit source, mapped source) one hop under `StructuralTransit(Navigate)`. `resolve_member_value_for_classification` (the exactness path) resolves a bare/import carrier so `type MyStr = string` classifies `ExactConcrete`. **Resolution-witness recording:** `build_typeof`'s import miss (a `typeof <unresolved import>`) observes the owner's path-precise resolution witness into the active tracer and marks `result_is_partial` + `cache_suppress` + the request materialization-suppress sticky. A genuine `MissingDependency` is `ReturnOnly`; the field materializer refuses `ShapeCacheDb` admission of a `TypeOf`-rooted semantic miss so the next request after the dependency appears recomputes cold and recovers.
.claude/skills/type-resolution/SKILL.md:1074:**Macro-surface partiality carries typed completeness; warm dispatch hits are NOT budget-debited (codex decider).** A macro surface that resolves through the shared dispatch carries a typed `ResultCompleteness` (`semantic_query.rs`). `vue_macro_dtos_with_ctx` (`vue_exec/mod.rs`) returns `MacroDtosRead { dtos, completeness }`, wraps its cold compute in a `ColdComputeCompletenessScope`, and admits the bundle into `vue_surface_store` ONLY when `completeness == Complete` AND `finalise == Ok` — a GENUINE partial DTO bundle is returned to the caller but never enters the surface store (no laundered warm replay). All DTO consumers (`define_shapes`, `slot_binding_graph::typeinfo_macro_dtos`, `resolver_core::component_meta`) fold the surface's partiality into the request-result completeness via `MacroDtosRead::observe_partial()`; the framework-surface executor surfaces `ResolvedOutcome::Partial`. `ResolvedComponentMetaState.completeness` is the authoritative per-result partial signal; `synthesis_should_suppress` is its compatibility projection (`= completeness.is_partial()`), preserved across the `ComponentMetaResultDb` `ResolutionTemplate` round-trip. The decider EXPLICITLY forbids charging warm cache hits to re-trip the projection budget (the budget is a runaway FUSE, not a semantic-complexity quota; charging warm hits risks false partials for complete-warmed work). **Per-result no-poison (LANDED):** the discriminating fixture `batch_partial_returned_never_admitted_while_complete_sibling_warms` (32 non-overlapping interfaces = 96 distinct props, `projection_op_budget = 6`, an unbounded oracle host) pins the invariant by typed completeness + admission-refusal, NOT a prop-count threshold: a result OBSERVED `Partial` (batch 1's budget-tripped cold compute) is REFUSED at the fixed-view cache boundary and never warmed (the primary discriminator); a genuine `Complete` recompute after warm resolver memos heal MAY admit (valid healing, not laundering — the partial result itself is never promoted). The discrimination is pinned by a `store_stable`-gate flip (`component_meta_request.rs:372`) that reddens the batch-1 `!result_admitted` witness. (The earlier 13-prop `<18` fixture rested on a non-deterministic budget re-trip the content-addressed mirror legitimately removed; it was corrected, not weakened — no rule exception, no carve-out. See `docs/arch/parselower-design.md` Stage 5A residual.)
.claude/skills/type-resolution/SKILL.md:1076:**Prepared-wrapper payloads — NARROWED.** The `verter_semantic` prepared-wrapper payloads were narrowed to locator-based `*Fact` forms (`verter_type_expr/src/facts.rs:2106-2281`) — `Opaque(TypeExpr)` became a `TypeBodySlot` locator; nothing type-tree-carrying remains open or deferred. The Stage-4 absence tripwire (`stage4_deferred_carriers_have_no_session_resolution_consumer`) stays green as a regression rail: no direct `verter_session` reference to the legacy payload API names / `.target_args`. Crate boundary holds: `verter_semantic::AnalyzedMacro.parsed_type_argument` is `Option<MacroPayloadLocator>` (a content-free locator); public + `verter_semantic` DTOs are compat-materialized at OUTPUT.
.claude/skills/type-resolution/SKILL.md:1078:**Handle-capable consumers (additive dual-read).** Several session-side component-meta consumers accept BOTH a parser-produced `TypeExpr` and an already-lowered handle. A handle-native consumer treats a `HotTypeRef` as an ALREADY-LOWERED `SemanticNodeId`: whole-node reduction enters `raise_and_reduce_with_context(node, context)`; a carrier-headed or base-bearing (`ProjectPath` / `KeyOf` / `MappedType`) query enters through `execute_type_node(SemanticQueryKey::…)` / the carrier-subject normalization (`normalize_carrier_subject_key` / `resolve_carrier_subject_node`) so the carrier head normalizes before memo admission. Both arms route through the SAME dispatch — read-compat, ONE resolver — and a handle arm NEVER calls `materialize_type_expr(handle)` to re-lower (the `materialize_type_expr(HotTypeRef)` harness is `#[cfg(test)]`-only, structurally gated by `materialize_type_expr_is_not_production_visible` — the successor to the deleted `no_hot_path_materialize_type_expr_bridge` line scanner). The shape: the `TypeExpr` arm lowers ONCE to a `SemanticNodeId` and delegates to a shared private `*_node(…)` core; the handle arm unwraps `HotTypeRef::node()` and calls that core. Consumer dual-read remains; prepared-wrapper / analyzer payload storage is already narrowed to locators and `*Fact` forms (not a pending TypeExpr-carrying deferral). The handle arms are LIVE for the macro-arg path (the macro hot mirror is their production producer — see "Macro Hot Mirror" above) and additionally proven by direct-constructed-handle equivalence fixtures (`crates/verter_session/src/meta_resolve/handle_capable_equivalence_tests.rs`); the structural-lowerer producer is the macro hot-mirror builder (single-entry, guarded by `structural_carrier_producer_lowerer_is_module_private`). **Prepared-DTO caveat:** the `verter_semantic` prepared-wrapper payloads are NARROWED locator-based `*Fact` forms (`verter_type_expr/src/facts.rs:2106-2281`) — `Opaque(TypeExpr)` became a `TypeBodySlot` locator; nothing type-tree-carrying remains open or deferred. The per-inventory guard (`stage4_carrier_inventory_handle_native_consumers_present` + `stage4_deferred_carriers_have_no_session_resolution_consumer`) keeps the narrowing regression-free via a short-lived absence-of-direct-reference tripwire — non-test production `verter_session` source must not directly name the four legacy payload type names / `.target_args`; this is an ordering tripwire, NOT a semantic dataflow proof (it does not prove no possible consumer exists). Guards (this surface): `materialize_type_expr_is_not_production_visible` (replaces the deleted `no_hot_path_materialize_type_expr_bridge` line scanner; the durable output boundary is the sealed `OutputProjector` capability + the private `carrier::payload` vault, with the retired Kind-B bridge symbol's absence tripwired by the lean tombstone `retired_kind_b_bridge_symbol_absent_from_production_source` and the fence shape by the mechanism-matched guards `output_projector_owner_registration_inventory` (sanctioned sink set + EXACT owner module topology) / `output_carriers_have_no_inherent_typeexpr_escape_method` (closed item/signature accessor allowlist) / `output_carrier_payload_fields_are_private` / `output_projector_non_owner_impl_is_compiler_sealed` / `test_output_cap_not_visible_or_mintable_in_non_test_builds` plus the accidental-regression `assert_not_impl_any!` carrier-escape CANARY (completeness is the payload vault, not the finite trait list) in `crates/verter_session/src/project_semantic_dispatch/output_materialization_guards.rs`, the `syn` guards in `crates/verter_source_policy_gate/tests/cases/output_projector_residual_guards.rs`), `stage4_carrier_inventory_handle_native_consumers_present`, `stage4_deferred_carriers_have_no_session_resolution_consumer` (`crates/verter_source_policy_gate/tests/cases/handle_capable_consumer_guards.rs`).
.claude/skills/type-resolution/SKILL.md:1100:- **Hash-then-lower slice nodes** (`crates/verter_session/src/cache_runtime/flow_slice_node.rs`) — `FlowSliceHashNode` plans + folds exactly the selected subgraph into the opaque `FlowSliceHash` (no byte export, `Debug` redacted; sole constructor `compute_flow_slice_hash`), then `FlowSliceLoweredBodyNode` lowers ONLY the plan into the arena-free `FlowSliceIR` (each `FlowSlot` carries its binding-identifier span — the declaration-precise slot identity the content lowering gates on; the slice hash stays span-free). The hash-then-lower split is TWO rails: the type-state pins hash-before-lowered-KEY (the lowered store is unaddressable without a minted hash), and the per-thread `compute_flow_slice_hash_thread_invocations` counter behaviorally pins that the lowered COMPUTE performs zero hash computations (the type system cannot — the hasher is a public producer); the counter, its accessor, and its increment are gated behind `verter_semantic`'s `test-support` feature (enabled by `verter_session`'s dev-dependency edge — `#[cfg(test)]` cannot serve a cross-crate consumer), so production builds compile the hasher without the TLS or the increment. Keys carry the per-function body-sensitive, cosmetic-insensitive `flow_body_stable_hash` (from the per-file `FunctionProgramIndex`), NOT `parse_stable_hash` — PLUS the EXACT per-function byte hash `flow_body_exact_hash`. Both nodes publish content-addressed artifacts with EMPTY fact rails — slice identity is never a warm-validity oracle — and "the key IS the validity oracle" holds only because TWO things are true together: the key carries that exact byte axis, and the artifacts carry NO absolute source position. That second half is a TYPE, not a convention: every span in `FunctionBodySkeleton` — and therefore in `FlowSliceIR` — is a `FrameSpan` (`verter_semantic::analysis::flow::frame_span`), a private-field newtype whose only constructor from a source position is `FrameSpan::rebase(anchor, span)` and whose only way back is `FrameSpan::to_absolute(anchor)`. `SkeletonBuilder::frame_span` is the ingress crossing (so `finish` is a plain MOVE with no per-family rebase pass to apply to five families and forget on two — which is exactly what happened: the read and call footprints kept ABSOLUTE offsets, and `lower_slice_plan`'s source-order effect sort mixed the two coordinate systems, ordering every call after every write at any non-zero anchor); `Lowerer::rebase` is the one crossing on the consumer side, and `FrameSpan::to_absolute` has exactly one caller (the parameter inventory comparing stored binding positions against live default-initializer offsets). A mixed comparison — `frame.start < absolute.start`, or a sort keyed off one of each — does not COMPILE. That claim was one accessor away from false: `FrameSpan::start()` / `::end()` were `pub fn -> u32` with ZERO production callers (one test), so the comparison the module exists to forbid was writable in the module that forbids it. They are deleted; `FrameSpan` now exposes no offset at all, and every consumer comparison is `FrameSpan`-to-`FrameSpan` (`contains`, the derived `Ord`). Two things stay CONVENTIONS and the module now says so instead of implying otherwise: the anchor is a bare `u32` on both crossings, so `rebase(0, absolute)` still mints a `FrameSpan` holding an absolute offset and `to_absolute(wrong_anchor)` still lands somewhere (nothing pairs a `FrameSpan` with its anchor — it outlives the file version it was recorded on, which is exactly why the anchor is re-supplied at egress); and the derived `Ord` / `Hash` let two frames' spans compare without either frame being named (harmless because every artifact here is per-function). `SkeletonRead` carries no span at all: the reference position had no consumer, and dead state is where a stale coordinate hides. Acceptance: `skeleton_is_invariant_under_the_function_position`, `lowered_ir_is_invariant_under_the_function_position`, `lower_orders_effects_by_authored_position_at_any_anchor` (the predecessor of the last read `span.start` back off the result and asserted it ascended — the same key the lowering had just sorted on, over a fixture at offset 0 where both systems coincide, so it held by construction and passed while the invariant was violated). Each half covers a blind spot the other cannot: `flow_body_stable_hash` is an AST fold, so it is blind to a leading blank line (which moves every absolute position and is fixed by the relative anchoring) AND, because it alpha-normalizes binding/reference identifiers, blind to a local rename that shifts every position INSIDE the body (which relative anchoring does NOT survive and only the exact byte axis catches). The exact axis is deliberately per-FUNCTION, not the file's content hash, so an edit to a sibling function leaves an untouched function's artifacts warm. Acceptance: `flow_bundle_reuse_is_sound_for_every_content_its_key_admits`, `the_two_shift_edits_are_invisible_to_flow_body_stable_hash`.
.claude/skills/type-resolution/SKILL.md:1101:- **`SemanticQueryKey::FlowReturn`** — the ONE dispatch arm (`project_semantic_dispatch/flow_return.rs`): key = content-free function slot + normalized type args + `FlowReturnContext` (all five env dims) + `ReturnProjectionDemand` + `FlowInputContext`. The published memo entry records `satisfied_projection` — the materialized point set the compute ACTUALLY produced (§3.4 lattice-relation satisfaction; `semantic_query_memo/flow_return_memo.rs` `cached_satisfies` decides warm hits against the REQUESTED point, never nominal demand). Warm validity roots on the sole `ProgramAnalysisFactRef::FlowBody` rail (function identity + `flow_body_stable_hash`, validated fail-closed by `StoreView::validates_program_analysis_domain`); the subplan's separate `FlowSlice` DISCRIMINANT fact does not exist on this tree (ratified C7 divergence — `FlowSlice` never enters `ReadSetSignature.facts`). Success carries `FlowReturnResult { return_type, can_fall_through, degradation }`: a degraded success (modeled-`any` substitution, typed `FlowReturnDegradation` — `NonCallableBinding`, `UnrepresentableCallee`, `FailedBindingInitializer`, the fail-closed `UnappliedWriteEffect` for a slice carrying a whole-slot write effect into a parameter or value-selected slot the evaluator does not apply, the fail-closed `ConditionalVarDefinition` for an observed function-scoped `var` whose surviving reaching definition was recorded inside a conditional arm, the fail-closed `UnreducedDeclaredUnion` for an annotated declarator whose DECLARED union could not be reduced to the constituents its initializer selects, the POSITIONAL `UnmodeledPosition` for a sub-expression whose resolver is a named downstream block, or `UnresolvedValue`) RETURNS through the success carrier but is `ReturnOnly` (never warmed). **The EVALUATION is the authority on its own degradation; the construction-time fold is a BACKSTOP, and the two deciding fields (`return_type`, `degradation`) are PRIVATE so neither can be bypassed** (`can_fall_through` is `pub` — a plain reachability bit no admission channel reads). `FlowReturnResult::new` is the sole construction point: it takes the evaluation's own observed reason and additionally consults `SemanticGraphStore::node_reaches_unresolved` (`semantic_query_memo/unresolved_reach.rs`), folding `UnresolvedValue` in when the value reaches a semantic-miss carrier (`QueryError::means_type_is_not_yet_known`). That verdict is a MEMOIZED INDUCTIVE BIT over the hash-consed immutable graph — `self_bit(n) || any(bit(child))`, decided once per node id, O(1) amortized at every later read. It carries NO budget, because a bound whose trip reports "unresolved" computes a DIFFERENT function: a 4,100-arm literal union with zero misses is fully known, and stamping it `UnresolvedValue` propagates a factually false permanent warm refusal into every enclosing result. The walk descends only structure the evaluation composes or lowers inline (`Union`/`Intersection`, `Object` members + signatures + index signatures + keyspace, `ObjectSpreadProgram`, `Array`/`Tuple`, `TemplateLiteral`, `KeyOf`/`IndexedAccess`/`Mapped`, `Conditional`, `Signature`, `InstantiationRef` args, `Alias`, and the three carriers' type ARGUMENTS through the sanctioned `carrier_type_args` accessor) and STOPS at every shallow carrier (`DeclRef`, `MergedDecl`, `BareRef`, `ImportType`, `TypeOf`) — descending one would be materialisation, and a miss inside a referenced declaration is that declaration's own admission problem. Its match over `SemanticNodeData` is wildcard-free (and covers struct FIELDS a variant match does not — `MapperKey::parameter_node` is a descent), an unresolvable node id answers "unresolved" (fail closed), `RawFallback` is not-known (agreeing with `SemanticNodeData::means_type_is_not_yet_known`, which is the reconciliation of the two classifications), and a deferred `BareRef` is explicitly NOT a miss. Without it, `function f(x: HasQ) { return x.q }` published `Opaque(Miss)` with `degradation: None` and `slot_candidate_count == 1` — a complete, warm answer with an opaque interior, so `execute_function_return_source` never folded the cache-read rails and every enclosing composition warmed around it; the same shape reached admission from FOUR independent ingress paths (the `FrameShadowed` arm, the free-leaf arm, a composed object member, and a leaf lowering's own `Array`/`Signature` interior), which is why the fold exists at all. The AUTHORITY, however, is the compute: a construction-time walk cannot see a fabricated `any` (a value cannot distinguish a fabricated `any` from an authored one), so a position whose resolver is a named downstream block RECORDS `UnmodeledPosition` where it stands and contributes the typed unresolved marker, and the fold catches only the residue no position observed. Acceptance: `a_value_reaching_a_miss_carrier_is_never_admitted_warm` + the over-degradation discriminator `a_deferred_carrier_and_a_resolved_composition_still_admit_warm`; no-value outcomes are typed `FlowReturnFailure`s through `ReturnOnly`; `FlowSliceBudget` refusals are non-admitted at every layer (planner refusal → no hash entry → unaddressable lowered store → no memo candidate). At the ONE sealed consumer entry (`execute_function_return_source`), the outcome routes through ONE exhaustive `FunctionReturnNode::consumer_fold` classification — no `degradation.is_some()` condition AT THAT ENTRY, and no second call site. (The bit is still read elsewhere, for questions that are not the consumer's fold: `flow_return.rs`'s own admission gate, `scc_publish.rs`'s component-wide publication gate, and `tsc_projection.rs`'s inferred-class-member row.) `consumer_fold()` is called from inside the `Flow` source arm ALONE (the `Declared` / `Absent` source arms return before reaching it), so only `Flow(_)` and `NoValue(_)` are LIVE there; `Declared` / `Absent` / `DeclaredMiss` are classified for the TYPE, not for a live call. A degraded success KEEPS its value and folds BOTH rails (`ConsumerFold::Partial(reasons)`), and so does a NO-VALUE outcome (`NoValue` / `DeclaredMiss`): the request-partial sticky gating component-meta/shape/materialize warm, plus the build-local `result_is_partial`/`cache_suppress` taint. `Declared` / `Absent` / a clean `Flow` are `ConsumerFold::Clean`. The carried `reasons` are derived per-OUTCOME, not per-arm — `degradation_reason_class` for a degraded success and `failure_reason_class` for a no-value failure — so a budget edge or a torn view is never folded under an inference-gap class.
.claude/skills/type-resolution/SKILL.md:1105:Both shapes fold ALIKE, and that is the decision. Suppressing only the build-local taint for the NO-VALUE shape (the former `ConsumerFold::NonCacheable`, now deleted) left the REQUEST unmarked, and `mark_request_result_partial` is the sole gate on `ComponentMetaResultDb`: six measured programs published `props: []`, `synthesis_should_suppress: false`, and a WARM cache hit on replay, for shapes the checker types without difficulty (a `for` / `switch` / `try` in `makeProps`; a nested arrow with a `while`; a local helper called for a member value). "The consumer decides what a contained failure means for its own surface" is true of the VALUE — which is why the value still returns — but not of the ADMISSION rail. Localisation is the POSITIONAL rule's job, one level down inside the evaluator. Pinned by `an_uninferred_body_return_never_publishes_a_complete_warm_meta_surface` (six rows + a clean warm control).
.claude/skills/type-resolution/SKILL.md:1122:| `RuntimeMemberValue` | `FLOW_RETURN_DEGRADED ∪ FLOW_RETURN_NO_SURFACE` | the SAME demand asked while classifying ONE member: here the class says that member's TYPE is unknown, which the lane encodes as `type: null` on that member. The observation is STICKY for the scope, so faulting here collapses every LATER member's constructor — `defineProps<{ a: ReturnType<typeof unmodelled>; b: string }>()` publishes `b: { type: null }` for a `b` the same tree types exactly (`meta_tests::a_no_surface_producer_at_a_member_value_degrades_only_that_member`) |

exec
/bin/zsh -lc "rg -n \"struct ComponentMetaQueryEngine|impl<'|impl ComponentMetaQueryEngine|ProjectSemanticDispatch::|dispatch\\(|execute\\(|SemanticQueryApi\" crates/verter_session/src/resolver_core/component_meta_query_engine crates/verter_session/src/resolver_core/component_meta --glob '*.rs' | sed -n '1,300p' && nl -ba crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs | sed -n '1,125p;860,1015p' && rg -n \"ProjectSemanticDispatch::new|fn dispatch|pub\\(crate\\) fn dispatch\" crates/verter_session/src/resolver_core/resolver_context.rs crates/verter_session/src/resolver_core/host_resolver_context.rs crates/verter_session/src/resolver_core/session_resolver_context.rs" in <block worktree root>
 succeeded in 156ms:
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:5://! `impl<'a>` block (the engine's read-through scratch mirrors and the
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_cache_producers.rs:82:impl ComponentMetaQueryEngine<'_> {
crates/verter_session/src/resolver_core/component_meta_query_engine/engine_accessors.rs:10:impl<'a> ComponentMetaQueryEngine<'a> {
crates/verter_session/src/resolver_core/component_meta_query_engine/tests.rs:9://! `impl<'a> ComponentMetaQueryEngine<'a>` block defined the method.
crates/verter_session/src/resolver_core/component_meta_query_engine/tests.rs:1091:fn instantiate_generic_function_alias_preserves_shadowing_inner_type_param_via_dispatch() {
crates/verter_session/src/resolver_core/component_meta_query_engine/intrinsic_surface.rs:43:impl ComponentMetaQueryEngine<'_> {
crates/verter_session/src/resolver_core/component_meta_query_engine/intrinsic_surface.rs:88:        let view = ctx.dispatch().resolve_typeinfo_surface_view(
crates/verter_session/src/resolver_core/component_meta_query_engine/intrinsic_surface.rs:122:        let dispatch = ProjectSemanticDispatch::new(ctx);
crates/verter_session/src/resolver_core/component_meta_query_engine/intrinsic_surface.rs:169:        let dispatch = ProjectSemanticDispatch::new(ctx);
crates/verter_session/src/resolver_core/component_meta_query_engine/intrinsic_surface.rs:246:    let dispatch = ProjectSemanticDispatch::new(ctx);
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_route_scoped.rs:15:impl ComponentMetaQueryEngine<'_> {
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_route_scoped.rs:145:            let dispatch = self.semantic_dispatch();
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_route_scoped.rs:158:        let (view, _surface_node) = compound_root_surface_view_via_dispatch(self.ctx, body_root)?;
crates/verter_session/src/resolver_core/component_meta/native_props.rs:96:    let dispatch = ctx.dispatch();
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:23:impl<'a> ComponentMetaQueryEngine<'a> {
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:114:        let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(self.ctx);
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:261:                        crate::project_semantic_dispatch::ProjectSemanticDispatch::new(self.ctx);
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:323:                                    crate::project_semantic_dispatch::ProjectSemanticDispatch::new(
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:438:        let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(self.ctx);
crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs:724:                                crate::project_semantic_dispatch::ProjectSemanticDispatch::new(
crates/verter_session/src/resolver_core/component_meta_query_engine/surface.rs:80:    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);
crates/verter_session/src/resolver_core/component_meta_query_engine/surface.rs:101:    let dispatch = ProjectSemanticDispatch::new(ctx);
crates/verter_session/src/resolver_core/component_meta_query_engine/surface.rs:123:    let shape = node_raised_shape_for_eq_with_dispatch(&dispatch, result_node, expr)?;
crates/verter_session/src/resolver_core/component_meta_query_engine/surface.rs:148:    let dispatch = ProjectSemanticDispatch::new(ctx);
crates/verter_session/src/resolver_core/component_meta_query_engine/surface.rs:193:    let witness = node_raised_shape_facts_with_dispatch(&dispatch, result_node)?;
crates/verter_session/src/resolver_core/component_meta_query_engine/surface.rs:283:pub(super) fn compound_root_surface_view_via_dispatch(
crates/verter_session/src/resolver_core/component_meta_query_engine/surface.rs:291:    let (surface, surface_node) = ctx.dispatch().resolve_typeinfo_surface_view_with_node(
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:5://! Inherent methods defined in a sibling `impl<'a>` block; they read
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:45:    PathSegment, ProjectionMode, QueryResult, SemanticNodeId, SemanticQueryApi, SemanticQueryKey,
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:62:impl<'a> ComponentMetaQueryEngine<'a> {
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:166:            let dispatch = ProjectSemanticDispatch::new(self.ctx);
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:217:        let dispatch = ProjectSemanticDispatch::new(self.ctx);
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:420:    pub(super) fn semantic_dispatch(&self) -> ProjectSemanticDispatch<'_> {
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:421:        ProjectSemanticDispatch::new(self.ctx)
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:457:        let dispatch = self.semantic_dispatch();
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:494:        let dispatch = self.semantic_dispatch();
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:580:            compound_root_surface_view_via_dispatch(self.ctx, anchor)?;
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:645:            let dispatch = self.semantic_dispatch();
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:692:        let (view, _surface_node) = compound_root_surface_view_via_dispatch(self.ctx, body_root)?;
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:781:                let dispatch = self.semantic_dispatch();
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:797:                        node_raised_shape_facts_with_dispatch(&dispatch, node)
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:853:        let dispatch = self.semantic_dispatch();
crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs:877:                node_raised_shape_facts_with_dispatch(&dispatch, node)
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval.rs:98:impl ComponentMetaQueryEngine<'_> {
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval.rs:140:        let dispatch = ProjectSemanticDispatch::new(self.ctx);
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval.rs:305:        ProjectSemanticDispatch::new(self.ctx).lower_type_expr_in_owner_scope_with_mode(
crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval.rs:364:        let dispatch = ProjectSemanticDispatch::new(self.ctx);
crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs:539:pub struct ComponentMetaQueryEngine<'a> {
crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs:919:impl<'a> ComponentMetaQueryEngine<'a> {
     1	//! Declaration-aware component-meta query engine.
     2	//!
     3	//! `ComponentMetaQueryEngine` is the per-request execution surface for
     4	//! one `get_component_meta()` call. It resolves type declarations
     5	//! lazily from the ctx's prepared-decl bundles. All solve-like
     6	//! operations dispatch through [`ProjectSemanticDispatch`].
     7	//!
     8	//! ## Authority model — what is durable vs. scratch
     9	//!
    10	//! The engine sits **above** the host's authoritative caches and
    11	//! **below** the public component-meta API. It does NOT own any
    12	//! durable cache state. The authoritative caches that survive
    13	//! beyond a single request are listed below; the engine reads from
    14	//! them via [`ResolverContext`] and (where applicable) writes to
    15	//! them through cooperative-admission `post_publish` so concurrent
    16	//! requests collapse onto one cold build.
    17	//!
    18	//! ### Authoritative host-owned caches (durable, dep-validated, reused across queries)
    19	//!
    20	//! - [`MaterializeMemoDb`](crate::component_meta_caches::MaterializeMemoDb)
    21	//!   — interned semantic instantiations keyed by
    22	//!   `(target_decl, mode, args)`. Final-result reuse across requests.
    23	//! - [`ComponentMetaResultDb`](crate::component_meta_caches::ComponentMetaResultDb)
    24	//!   — final `ComponentMetaAnalysis` payloads keyed by `(canonical, profile)`.
    25	//!   `get_component_meta` consults this first; the engine only runs on cold misses.
    26	//! - [`SemanticGraphStore`](crate::semantic_query_memo::SemanticGraphStore)
    27	//!   — interned semantic-node arena.
    28	//!   Engine subqueries dispatch via
    29	//!   [`ProjectSemanticDispatch`](crate::project_semantic_dispatch::ProjectSemanticDispatch)
    30	//!   which deduplicates against this store.
    31	//! - `ClassifyMaterializationCycleGate`
    32	//!   (`project_semantic_dispatch::cycle_gate`) — the sealed
    33	//!   materialization cycle gate, a semantic-query family over the
    34	//!   `SemanticGraphStore` memo (singleflight cold build).
    35	//! - [`MaterializeStructureDb`](crate::component_meta_caches::MaterializeStructureDb)
    36	//!   — interned structural projections produced by the canonical
    37	//!   materialiser; sole authoritative materialiser cache.
    38	//!
    39	//! All five participate in `ProjectTypeStore`'s invalidation cascade
    40	//! and are fact-validated on warm hit by re-walking each candidate's
    41	//! `read_set_signature.facts` against the live `StoreView`.
    42	//!
    43	//! ### Per-request scratch (NOT promoted, dies with the engine)
    44	//!
    45	//! The engine retains a small set of `RefCell`-wrapped maps used to
    46	//! avoid recomputing the same projection within one request. These
    47	//! are scratch only:
    48	//!
    49	//! - Type-param substitution maps and projection-chain scopes —
    50	//!   per-frame state for the current dispatch path.
    51	//!
    52	//! **None of these are written back to the host store directly.**
    53	//! When durable population is required, the engine uses the ctx's
    54	//! cooperative-admission `post_publish` path so the host store sees
    55	//! exactly one canonical write per cache key.
    56	//!
    57	//! ### Never-promoted results
    58	//!
    59	//! The following partial outcomes MUST NOT be admitted to the
    60	//! authoritative caches above. They produce request-local outputs only
    61	//! and are discarded when the engine drops:
    62	//!
    63	//! - cancelled requests (cooperative cancellation),
    64	//! - superseded results (a later request's input changed before this
    65	//!   one published),
    66	//! - interrupted results (panic / stack overflow / OS error caught by
    67	//!   the cooperative-admission guard),
    68	//! - budget-exceeded results (FuseBudgets tripped before the projection
    69	//!   converged), and
    70	//! - partial results (any path that returned `Opaque(Miss)` or an
    71	//!   intentionally incomplete shape because a strict-legality precondition
    72	//!   was not met).
    73	//!
    74	//! The engine's `post_publish` discipline enforces this: it commits
    75	//! only when the cooperative-admission guard records a complete success.
    76	//!
    77	//! See the `/component-meta` skill for the public API surface and the
    78	//! `/type-resolution` skill for the cross-file resolver query modes.
    79	
    80	use std::cell::RefCell;
    81	use std::collections::BTreeSet;
    82	
    83	use rustc_hash::FxHashMap;
    84	use verter_semantic::analysis::type_eval::DeclarationId;
    85	
    86	use super::declaration_metadata::{
    87	    DeclarationMetadataResolver, ResolvedDeclarationKind, ResolvedLocalTypeSymbolMetadata,
    88	    ResolvedTypeDeclaration,
    89	};
    90	use crate::resolver_core::bare_name_resolve::DeclarationScopePayload;
    91	use crate::resolver_core::scope_shadowing::ScopeShadowing;
    92	use crate::resolver_core::ResolverContext;
    93	use crate::resolver_core::{FuseBudgets, FuseState};
    94	use crate::semantic_query::SemanticNodeId;
    95	
    96	// The output-sink capabilities for this subtree are defined PER-SINK in the
    97	// exact output-SINK modules that project — NOT subtree-wide:
    98	// `MetaQuerySurfaceOutputCap` in `surface.rs` and `MetaQueryRegistryOutputCap`
    99	// in `registry_decl.rs` (each a single-file sink with no production
   100	// submodule). A subtree-wide cap
   101	// (`pub(in crate::resolver_core::component_meta_query_engine)`) would let any
   102	// sibling in this subtree mint it; terminal-sink minting (each mint scope's
   103	// whole reachable production module tree is output-only) makes the
   104	// output-materialization fence compiler-enforced.
   105	
   106	// Surface-projection helpers, prepared-substitution
   107	// machinery, and arc cache-key constructors live in the private
   108	// `surface` child module. The `pub(crate) use` block re-exports the
   109	// existing public-API symbols so external `crate::resolver_core::component_meta_query_engine::<name>`
   110	// paths remain stable.
   111	mod engine_accessors;
   112	mod fallthrough_value_eval;
   113	mod helpers;
   114	mod intrinsic_surface;
   115	mod node_materialize;
   116	// The node-domain object-surface classifier — re-exported for the
   117	// meta_resolve consumers that lower a decl-body locator through the shared
   118	// dispatch and read the object-surface verdict off the lowered node.
   119	pub(crate) use node_materialize::component_meta_registry_node_has_explicit_object_surface;
   120	mod registry_cache_producers;
   121	mod registry_decl;
   122	mod registry_route_scoped;
   123	mod route_admission;
   124	mod shallow_preserve;
   125	mod surface;
   860	/// closure or leave the gate armed for the next test.
   861	#[cfg(test)]
   862	pub(crate) struct ImportedRegistryWinnerParkGuard(std::sync::Arc<ImportedRegistryWinnerPark>);
   863	
   864	#[cfg(test)]
   865	impl Drop for ImportedRegistryWinnerParkGuard {
   866	    fn drop(&mut self) {
   867	        // Disarm first so no new winner can park, then release any winner
   868	        // already blocked on the gate.
   869	        *IMPORTED_REGISTRY_WINNER_PARK.lock().unwrap() = None;
   870	        self.0.release();
   871	    }
   872	}
   873	
   874	/// Arm the imported-registry winner-park gate for `marker_canonical`.
   875	/// The cold winner of a `resolve_imported_registry_symbol` whose keyed
   876	/// canonical equals `marker_canonical` blocks inside its cooperative
   877	/// admission compute closure until the returned handle's `release()` is
   878	/// called. The returned guard disarms the gate (and releases any parked
   879	/// winner) on drop.
   880	#[cfg(test)]
   881	pub(crate) fn arm_imported_registry_winner_park_for_tests(
   882	    marker_canonical: &str,
   883	) -> (
   884	    ImportedRegistryWinnerParkHandle,
   885	    ImportedRegistryWinnerParkGuard,
   886	) {
   887	    let park = std::sync::Arc::new(ImportedRegistryWinnerPark {
   888	        released: std::sync::Mutex::new(false),
   889	        ready: std::sync::Condvar::new(),
   890	    });
   891	    *IMPORTED_REGISTRY_WINNER_PARK.lock().unwrap() =
   892	        Some((marker_canonical.to_string(), std::sync::Arc::clone(&park)));
   893	    (
   894	        ImportedRegistryWinnerParkHandle(std::sync::Arc::clone(&park)),
   895	        ImportedRegistryWinnerParkGuard(park),
   896	    )
   897	}
   898	
   899	/// Block the cold winner on the imported-registry winner-park gate when
   900	/// it is armed for `canonical_id`. Invoked by the
   901	/// `resolve_imported_registry_symbol` cooperative-admission compute
   902	/// closure exactly once per cold compute, AFTER the in-flight slot is
   903	/// claimed and BEFORE the resolution runs. A no-op when the gate is
   904	/// unarmed or armed for a different canonical.
   905	#[cfg(test)]
   906	pub(crate) fn await_imported_registry_winner_park_for_tests(canonical_id: &str) {
   907	    let park = {
   908	        let slot = IMPORTED_REGISTRY_WINNER_PARK.lock().unwrap();
   909	        match slot.as_ref() {
   910	            Some((marker, park)) if marker == canonical_id => Some(std::sync::Arc::clone(park)),
   911	            _ => None,
   912	        }
   913	    };
   914	    if let Some(park) = park {
   915	        park.wait();
   916	    }
   917	}
   918	
   919	impl<'a> ComponentMetaQueryEngine<'a> {
   920	    pub(crate) fn new(ctx: &'a dyn ResolverContext) -> Self {
   921	        // Bump `bare_engine_constructions` whenever the engine is
   922	        // bound to a non-request-bound ctx. Final-state invariant:
   923	        // `0` — every production engine binds to a request-bound
   924	        // ctx (`HostResolverContext` / `SessionResolverContext`).
   925	        if !ctx.is_request_bound() {
   926	            crate::request_context::bump_bare_engine_construction();
   927	        }
   928	        Self {
   929	            ctx,
   930	            imported_registry_symbols: RefCell::new(FxHashMap::default()),
   931	            declarations: RefCell::new(FxHashMap::default()),
   932	            resolvable: RefCell::new(FxHashMap::default()),
   933	            owner_collection_exprs: RefCell::new(FxHashMap::default()),
   934	            scope_payloads: FxHashMap::default(),
   935	            scope_shadowings: FxHashMap::default(),
   936	            prepared_type_decls: FxHashMap::default(),
   937	            #[cfg(test)]
   938	            prepared_type_decl_query_count: 0,
   939	            fuse_budgets: FuseBudgets::default(),
   940	            fuse_state: FuseState::default(),
   941	        }
   942	    }
   943	
   944	    /// Returns the cached [`DeclarationScopePayload`] for
   945	    /// `scope_canonical_id`, lazily loading the underlying
   946	    /// `prepared_decl_bundle` on first access ( D35:
   947	    /// promoted to `pub(crate)` so the session-layer materialize wrapper
   948	    /// in `meta_resolve.rs` can reuse the cache without re-walking the
   949	    /// bundle).
   950	    pub(crate) fn scope_payload_for_scope(
   951	        &mut self,
   952	        scope_canonical_id: &str,
   953	        owner: verter_type_expr::TopLevelOwnerId,
   954	    ) -> Option<std::sync::Arc<DeclarationScopePayload>> {
   955	        let ctx = self.ctx;
   956	        self.scope_payloads
   957	            .entry((scope_canonical_id.to_string(), owner))
   958	            .or_insert_with(|| {
   959	                ctx.prepared_decl_bundle(scope_canonical_id)
   960	                    .or_else(|| {
   961	                        // Lazy first-time loading for dependency files discovered
   962	                        // during resolution. This is NOT re-walking cached state —
   963	                        // it triggers the normal load/parse/cache pipeline for files
   964	                        // not yet in the ctx's cache.
   965	                        ctx.ensure_loaded(scope_canonical_id)
   966	                            .then(|| ctx.prepared_decl_bundle(scope_canonical_id))
   967	                            .flatten()
   968	                    })
   969	                    .map(|bundle| {
   970	                        std::sync::Arc::new(DeclarationScopePayload::from_bundle(&bundle, owner))
   971	                    })
   972	            })
   973	            .clone()
   974	    }
   975	
   976	    /// Returns the per-scope [`ScopeShadowing`] for `scope_canonical_id`,
   977	    /// built ONCE from the cached [`scope_payload_for_scope`] entry and
   978	    /// memoized for the engine's request lifetime.
   979	    ///
   980	    /// The materialize → lower pipeline
   981	    /// ([`crate::meta_resolve::materialize`]'s shape-subject lowering)
   982	    /// probes this per published field; folding the scope's local type
   983	    /// names, script-setup type bindings, and resolved import bindings into a
   984	    /// fresh shadow set on every probe is O(fields × scope names/imports).
   985	    /// Building it once here and reusing the cached [`ScopeShadowing`] across
   986	    /// every probe makes the gate O(1) per field (the
   987	    /// [`ScopeShadowing::is_shadowing_lib`] lookup is a hash-set membership
   988	    /// check). The shadow set is byte-equivalent to the
   989	    /// [`ScopeShadowing::from_host_scope`] bundle-derived one dispatch
   990	    /// consumes — both fold the same three sets — so the two lowering fronts
   991	    /// still observe identical shadow decisions.
   992	    ///
   993	    /// [`scope_payload_for_scope`]: Self::scope_payload_for_scope
   994	    pub(crate) fn scope_shadowing_for_scope(
   995	        &mut self,
   996	        scope_canonical_id: &str,
   997	        owner: verter_type_expr::TopLevelOwnerId,
   998	    ) -> std::sync::Arc<ScopeShadowing> {
   999	        let key = (scope_canonical_id.to_string(), owner);
  1000	        if let Some(shadowing) = self.scope_shadowings.get(&key) {
  1001	            return std::sync::Arc::clone(shadowing);
  1002	        }
  1003	        let payload = self.scope_payload_for_scope(scope_canonical_id, owner);
  1004	        let shadowing = std::sync::Arc::new(ScopeShadowing::from_scope_payload(payload.as_deref()));
  1005	        self.scope_shadowings
  1006	            .insert(key, std::sync::Arc::clone(&shadowing));
  1007	        shadowing
  1008	    }
  1009	}
  1010	
  1011	fn local_type_symbol_metadata_for_known_source(
  1012	    ctx: &dyn ResolverContext,
  1013	    canonical_source: &str,
  1014	    owner: verter_type_expr::TopLevelOwnerId,
  1015	    resolved_name: &str,
crates/verter_session/src/resolver_core/session_resolver_context.rs:68:/// site signature in [`ProjectSemanticDispatch::new`].
crates/verter_session/src/resolver_core/session_resolver_context.rs:99:    /// [`ProjectSemanticDispatch::new`]: callers create the wrapper on
crates/verter_session/src/resolver_core/session_resolver_context.rs:678:    fn dispatch(&self) -> ProjectSemanticDispatch<'_> {
crates/verter_session/src/resolver_core/session_resolver_context.rs:682:        ProjectSemanticDispatch::new(self)
crates/verter_session/src/resolver_core/session_resolver_context.rs:686:    fn dispatch_node_data(&self, node: SemanticNodeId) -> Option<Arc<SemanticNodeData>> {
crates/verter_session/src/resolver_core/host_resolver_context.rs:24://! transient `ProjectSemanticDispatch::new(...)`) build the view via
crates/verter_session/src/resolver_core/host_resolver_context.rs:498:    fn dispatch(&self) -> ProjectSemanticDispatch<'_> {
crates/verter_session/src/resolver_core/host_resolver_context.rs:502:        ProjectSemanticDispatch::new(self)
crates/verter_session/src/resolver_core/host_resolver_context.rs:506:    fn dispatch_node_data(&self, node: SemanticNodeId) -> Option<Arc<SemanticNodeData>> {
crates/verter_session/src/resolver_core/resolver_context.rs:21://! `ProjectSemanticDispatch::new`,
crates/verter_session/src/resolver_core/resolver_context.rs:579:    fn dispatch(&self) -> ProjectSemanticDispatch<'_>;
crates/verter_session/src/resolver_core/resolver_context.rs:581:    fn dispatch_node_data(&self, node: SemanticNodeId) -> Option<Arc<SemanticNodeData>>;
crates/verter_session/src/resolver_core/resolver_context.rs:1268:    fn dispatch(&self) -> ProjectSemanticDispatch<'_> {
crates/verter_session/src/resolver_core/resolver_context.rs:1269:        ProjectSemanticDispatch::new(self)
crates/verter_session/src/resolver_core/resolver_context.rs:1273:    fn dispatch_node_data(&self, node: SemanticNodeId) -> Option<Arc<SemanticNodeData>> {

exec
/bin/zsh -lc "rg -n \"fn (cancellation_token|ensure_indexed_ready_serve|project_type_store|config|host_for_fact_tracer_install)\" crates/verter_session/src/resolver_core/resolver_context.rs && nl -ba crates/verter_session/src/resolver_core/resolver_context.rs | sed -n '280,345p;445,485p;560,615p;730,755p'" in <block worktree root>
 succeeded in 0ms:
188:    fn cancellation_token(&self) -> Option<verter_scheduler::cancellation::CancellationToken> {
294:    fn ensure_indexed_ready_serve(
457:    fn project_type_store(&self) -> &Arc<ProjectTypeStore>;
459:    fn config(&self) -> &HostConfig;
750:    fn host_for_fact_tracer_install(&self) -> &crate::VerterHost;
941:    fn ensure_indexed_ready_serve(
1035:    fn project_type_store(&self) -> &Arc<ProjectTypeStore> {
1040:    fn config(&self) -> &HostConfig {
1301:    fn host_for_fact_tracer_install(&self) -> &crate::VerterHost {
   280	                None
   281	            }
   282	        }
   283	    }
   284	
   285	    /// Materialise (or warm-read) the canonical post-parse artifact,
   286	    /// with the publication status flowed BY VALUE — see
   287	    /// [`crate::host_manage::prepared_decl::IndexedReadyServe`]. This is
   288	    /// the ONLY resolver-tier accessor for a cold/warm `IndexedReady`:
   289	    /// a consumer that derives shared-cache entries from the artifact
   290	    /// gates admission on `serve.store_published`; structurally
   291	    /// read-only consumers take `serve.indexed` (the fenced consumption
   292	    /// still reaches every enclosing traced admission point through the
   293	    /// `note_non_cacheable_read_fan_out` chokepoint flag).
   294	    fn ensure_indexed_ready_serve(
   295	        &self,
   296	        canonical_id: &str,
   297	    ) -> Option<crate::host_manage::prepared_decl::IndexedReadyServe>;
   298	
   299	    fn ensure_loaded(&self, canonical_id: &str) -> bool;
   300	
   301	    fn shallow_file_state(&self, canonical_id: &str) -> Option<Arc<ShallowFileState>>;
   302	
   303	    fn local_type_declaration_id(
   304	        &self,
   305	        canonical_source: &str,
   306	        resolved_name: &str,
   307	    ) -> Option<DeclarationId>;
   308	
   309	    fn get_whole_hash(&self, canonical: &str) -> Option<Hash16>;
   310	
   311	    /// Authoritative current content hash for `canonical` — the hash
   312	    /// source [`Self::indexed_for_current_content`] pins against.
   313	    ///
   314	    /// Unlike [`Self::get_whole_hash`] this accessor has **no
   315	    /// permissive fallback**: it never derives a hash from a
   316	    /// content-agnostic `FileArtifactStore` scan
   317	    /// (`FileArtifactStore::get_any`).
   318	    /// When only a stale artifact could answer (the canonical was
   319	    /// evicted/deleted while its `IndexedReady` lingers) it returns
   320	    /// `None` so the pinned read becomes a miss rather than resolving
   321	    /// the stale artifact via its own hash.
   322	    ///
   323	    /// The default impl delegates to
   324	    /// [`crate::VerterHost::authoritative_current_content_hash`] on the
   325	    /// concrete host — the scheduler `parse.whole_hash` gated on the
   326	    /// `DerivedRawState` entry being non-evicted. The overlay-aware
   327	    /// [`crate::resolver_core::session_resolver_context::SessionResolverContext`]
   328	    /// overrides it to consult the active [`SessionView`](crate::session_view::SessionView):
   329	    /// an overlay-covered canonical resolves to the overlay's content
   330	    /// hash (the hash the overlay `IndexedReady` was prewarmed under),
   331	    /// not the base host's hash.
   332	    fn authoritative_current_content_hash(&self, canonical: &str) -> Option<Hash16> {
   333	        self.host_for_fact_tracer_install()
   334	            .authoritative_current_content_hash(canonical)
   335	    }
   336	
   337	    /// Content-pinned [`IndexedReady`] lookup.
   338	    ///
   339	    /// Resolves the canonical's authoritative current content hash via
   340	    /// [`Self::authoritative_current_content_hash`] (no `get_any`
   341	    /// fallback; overlay-aware under `SessionResolverContext`) and
   342	    /// reads the artifact store pinned to that hash via
   343	    /// [`crate::file_artifact_store::FileArtifactStore::get_for_current_content`].
   344	    /// Returns `None` when the canonical has no authoritative current
   345	    /// content hash OR when the only cached artifact is a stale
   445	    /// Returns `&dyn StoreView` (not the concrete [`HostStoreView`]) so
   446	    /// the trait stays dyn-compatible AND so a request-bound implementer
   447	    /// can hand back a [`crate::resolver_core::RequestStoreView`]
   448	    /// wrapper that chains a
   449	    /// [`crate::resolver_core::CanonicalCompletionOverlay`] in front of
   450	    /// the request-entry base view. The overlay records additive loads
   451	    /// observed mid-request (`ensure_loaded` / `ensure_indexed_ready_serve`
   452	    /// successes) so the self-root validator does not false-miss on
   453	    /// canonicals loaded after the request-entry snapshot.
   454	    #[allow(dead_code)]
   455	    fn store_view(&self) -> &dyn crate::resolver_core::StoreView;
   456	
   457	    fn project_type_store(&self) -> &Arc<ProjectTypeStore>;
   458	
   459	    fn config(&self) -> &HostConfig;
   460	
   461	    // -------- Symbol / route resolution ----------------------------
   462	
   463	    /// Fact-DISCARDING import-root resolution (final `(canonical, symbol)`
   464	    /// tuple only).
   465	    ///
   466	    /// MUST NOT be used on a memoized-build path (a `LowerLocator` /
   467	    /// read-set-validated cold build): the discarded route-chain facts are
   468	    /// the only proof a barrel/re-export retarget invalidates the enclosing
   469	    /// cache entry — dropping them false-warms the entry when an
   470	    /// intermediate barrel changes while the owner file does not. Memoized
   471	    /// builds call [`Self::resolve_imported_type_root_with_facts`] and
   472	    /// record the returned facts onto the active tracer.
   473	    fn resolve_imported_type_root(
   474	        &self,
   475	        dep_canonical: &str,
   476	        imported_name: &str,
   477	    ) -> Option<verter_semantic::analysis::type_solver::ResolvedRootIdentity>;
   478	
   479	    /// Like [`Self::resolve_imported_type_root`] but ALSO returns the full
   480	    /// route-chain fact list the resolution observed (every barrel /
   481	    /// re-export participant's version).
   482	    ///
   483	    /// REQUIRED on any memoized-build path: the caller records the returned
   484	    /// facts onto the active fact tracer
   485	    /// ([`Self::observe_borrowed_signature`]) so the enclosing cache
   560	    /// callers MUST NOT substitute `path.contains("/node_modules/")`
   561	    /// for this method.
   562	    #[cfg(test)]
   563	    fn workspace_is_workspace_owned(&self, canonical_id: &str) -> bool;
   564	
   565	    /// Whether `canonical_id` is package-backed per the workspace's
   566	    /// resolver-classification (NOT a path-substring check on
   567	    /// `node_modules`). True only when the realpath sits under
   568	    /// `node_modules/` AND no registered project root claims the file.
   569	    ///
   570	    /// Used by Issue #11 (workspace-local canonical cache reuse) and
   571	    /// the shared symbolic-preservation helper to decide when an
   572	    /// imported ref must materialize canonically vs. stay symbolic.
   573	    /// Callers MUST NOT substitute `path.contains("/node_modules/")`
   574	    /// for this method.
   575	    fn workspace_is_package_backed(&self, canonical_id: &str) -> bool;
   576	
   577	    // -------- Dispatch facade --------------------------------------
   578	
   579	    fn dispatch(&self) -> ProjectSemanticDispatch<'_>;
   580	
   581	    fn dispatch_node_data(&self, node: SemanticNodeId) -> Option<Arc<SemanticNodeData>>;
   582	
   583	    // -------- Component-meta-tier bridges --------------------------
   584	    //
   585	    // clippy cleanup — these two trait methods are part of
   586	    // the resolver-context surface contract for component-meta-tier
   587	    // adapters but have no caller in the landed tree. The trait is
   588	    // sealed (only `VerterHost` implements it) and the methods are
   589	    // retained for symmetry with the dependency-fact and analysis-snap
   590	    // bridges defined in the impl block below. `#[allow(dead_code)]` is
   591	    // applied at the trait definition so the corresponding
   592	    // `impl ResolverContext for VerterHost` definitions do not need
   593	    // their own `#[allow]` annotations.
   594	
   595	    #[allow(dead_code)]
   596	    fn current_dependency_fact_versions(
   597	        &self,
   598	        canonical: &str,
   599	        tracked_deps: &BTreeSet<String>,
   600	    ) -> Vec<FactVersionRef>;
   601	
   602	    #[allow(dead_code)]
   603	    fn get_raw_analysis_snapshot(&self, canonical: &str) -> Option<FileAnalysisSnapshot>;
   604	
   605	    // -------- Push-style fact-read tracer (cold-path only) ---------
   606	    //
   607	    // Cold-compute callers record each fact they read from a
   608	    // content-addressed source through [`observe`] /
   609	    // [`observe_borrowed_signature`]. On warm-hit paths no tracer is
   610	    // installed; both convenience methods become observable no-ops.
   611	    //
   612	    // The tracer is owned by an installer that brackets one cold
   613	    // compute on one thread; see
   614	    // [`crate::VerterHost::with_fact_tracer`] for the RAII entry
   615	    // point. The trait method [`current_fact_tracer`] returns the
   730	            .normalized_analysis_canonical(raw_canonical)
   731	    }
   732	
   733	    /// Reach the concrete `VerterHost` underneath this context.
   734	    ///
   735	    /// Used by Family B/C/D producers (`MaterializeStructureDb`,
   736	    /// `AppConfigNoOverrideProofDb`,
   737	    /// `OwnerImportSurfaceDb`) to call
   738	    /// [`crate::VerterHost::with_fact_tracer`] from inside their
   739	    /// cooperative-admission cold-compute closures. The seal trait
   740	    /// itself cannot expose `with_fact_tracer` directly because
   741	    /// `FnOnce<R>` is non-dyn-compatible; this accessor lets
   742	    /// cold-compute closures install the tracer through the existing
   743	    /// `fact_signature_helpers::install_fact_tracer(host, ...)`
   744	    /// surface without bypassing the seal.
   745	    ///
   746	    /// Both production implementers ([`crate::VerterHost`] and
   747	    /// [`crate::resolver_core::session_resolver_context::SessionResolverContext`])
   748	    /// return their inner `&crate::VerterHost`. There is no other
   749	    /// implementer; the seal guarantees the trait contract.
   750	    fn host_for_fact_tracer_install(&self) -> &crate::VerterHost;
   751	
   752	    /// This context's contribution to a fact tracer's compaction basis.
   753	    ///
   754	    /// Deliberately NOT `self.store_view().aggregate_basis_seed()` at the
   755	    /// call site. [`Self::store_view`] is a BORROW contract that the bare

exec
/bin/zsh -lc "nl -ba docs/arch/refactor/rev11/charters/C1.md | sed -n '377,451p' && nl -ba CLAUDE.md | sed -n '535,585p'" in <block worktree root>
 succeeded in 78ms:
   535	### Planning
   536	
   537	Prefer architecturally correct, long-term solutions; evaluate by correctness and durability, not implementation speed. Time constraints, implementation size, migration breadth, anticipated breaking changes, or "a lot of work" are not valid reasons to weaken the design, preserve a compromised path, or diverge from the approved plan — if the correct implementation is larger or breaking, plan for it explicitly or raise it before execution; never silently ship an architectural deviation. Do not provide time estimates unless explicitly asked, and never use estimated effort/duration/perceived time cost as a factor for doing, not doing, or partially doing planned work.
   538	
   539	Plans must include these sections:
   540	1. **Context** — why this change is being made
   541	2. **Intent Contract** — the ratified statement of intent, before any mechanism design
   542	3. **Changes** — specific files to modify with concrete modifications
   543	4. **Legacy Deletions** — explicit list of files, functions, code paths, feature flags to remove
   544	5. **Verification** — full workspace test commands and expected outcomes
   545	
   546	Without explicit legacy deletion lists, agents skip deletions and leave dual paths alive.
   547	
   548	**Intent before mechanism.** Before mechanism design for a block that changes observable behavior, authority, or fallback, record a ratified intent contract: the actor/problem and why the capability should exist; required and forbidden observable outcomes; authority/fallback order; a planned test or gate for each stable acceptance ID; and material cold, warm, allocation, fan-out, and latency bounds. An internal substrate block may reference its parent contract but must state the invariant and performance contribution it owns. Ratification comes from the approved plan or product authority; no implementation brief is dispatched without it. Enforcement is judgment — exercised at decomposition and again immediately before implementation dispatch.
   549	
   550	### Execution
   551	
   552	Execute approved plans fully in one pass, end-to-end, without intermediate checkpoints or mid-plan confirmation on already-approved steps. Do not pause, defer scope, leave planned work unfinished, or rewrite the plan into a smaller/safer variant because the correct path is breaking, broad, or labor-intensive. Approved plans land as written unless the user explicitly re-scopes them.
   553	
   554	**One-pass execution applies only while the approved design remains valid.** The second-REOPEN circuit breaker lapses approval for the affected design: pause implementation, obtain and record the required architecture/product ruling, and resume only once the design is ratified again. This is not a checkpoint — one-pass governs *executing an approved design*, and the breaker fires when *approval itself has lapsed*, which is a different event and precisely why execution must stop rather than grind on. STOP, failed verification, rule conflict, and verified plan-invalidating discoveries pause at their prescribed evidence gate without creating a discretionary user checkpoint. Breadth, breakage, effort, or migration size never lapses approval; approved scope changes only through the recorded ruling or explicit user re-scope. See `/mom-cto-orchestration` → Decision Admission.
   555	
   556	### Orchestrating Large Plans
   557	
   558	For a large multi-block plan, refactor, migration, or staged cutover executed autonomously, drive it via the `/multi-agent-orchestration` skill rather than improvising: a pure orchestrator delegates blocks to implementer/reviewer/fix sub-agents, gates each on dual review (independent reviewer + `codex`), runs fix cycles until clean, and verifies sub-agent reports against git state (trust but verify).
   559	
   560	When a block runs in a dedicated `git worktree`, run `pnpm install --frozen-lockfile` in the worktree root once at creation time, before any JS/TS test or workspace-importing Node script — fresh worktrees do not get the gitignored `node_modules/`, and a missing install makes JS/TS tests fail spuriously and read as a false regression. See the skill's "Worktree hygiene & environmental discipline" section.
   561	
   562	### Self-Review
   563	
   564	After completing a plan, review the full implementation before declaring done:
   565	- Verify all plan steps were executed
   566	- Check for missed edge cases or incomplete migrations
   567	- Run the full workspace test suite (see End-of-change Checks above)
   568	
   569	### Legacy Code Deletion
   570	
   571	When replacing a feature or refactoring a system, delete the superseded code in the same change. Do not add shims, double branches, compatibility wrappers, or feature flags to preserve old behavior alongside new. If unsure whether specific files or code paths should be preserved, ask the user explicitly rather than silently keeping them.
   572	
   573	### Fix Quality
   574	
   575	When encountering issues during implementation:
   576	- If the correct fix aligns with the architecture → implement it properly
   577	- Never apply a dirty fix that contradicts architectural rules just to make tests pass
   578	- If the proper fix is outside approved scope, do not apply a workaround and do not use a `TODO` as its disposition. Route the finding through the applicable scope authority and record `ADOPT-NOW`, `DEFER`, or `REJECT` before related work continues. A `TODO` may reference an approved debt row but never replaces it.
   579	
   580	**Explicit finding disposition.** Every scope-deviating correctness finding is dispositioned before related work continues as `ADOPT-NOW`, `DEFER`, or `REJECT`. `ADOPT-NOW` records the scope and acceptance-contract change. `DEFER` requires a codex-DEFER ruling and a debt row naming the durable owner block, the resolution gate no later than plan close, the acceptance ID/test, and the ruling reference. `REJECT` records evidence and rationale. A TODO, a feedback entry, or an ephemeral agent identity is not a disposition; plan close requires zero open deferrals. Enforcement is judgment — codex at the scope consult, and the plan-close zero-open-deferral check.
   581	
   582	### Stub Prevention (CRITICAL)
   583	
   584	Do not use empty test bodies, trivially-passing stubs, or "deferred to follow-up commit" placeholders to satisfy a named contract — a gate check, a characterization test, a plan invariant, a review obligation, a declared completion criterion. A stub that happens to pass is a gate-bypass, not a pass.
   585	

exec
/bin/zsh -lc "rg -n \"CanonicalCompletionOverlay|RequestStoreView|RequestBundleMemo|BundleMemoWorld\" crates/verter_session/src --glob '*.rs' | sed -n '1,280p'" in <block worktree root>
 succeeded in 253ms:
crates/verter_session/src/meta/output_api.rs:232:        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/typeinfo/shallow_surface.rs:93:        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/semantic_graph_self_root_tests.rs:1973:            std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
crates/verter_session/src/semantic_graph_self_root_tests.rs:2007:            std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
crates/verter_session/src/semantic_graph_self_root_tests.rs:2039:            std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
crates/verter_session/src/semantic_graph_self_root_tests.rs:2181:        std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
crates/verter_session/src/semantic_graph_self_root_tests.rs:2559:            std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
crates/verter_session/src/semantic_graph_self_root_tests.rs:2608:            std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
crates/verter_session/src/semantic_graph_self_root_tests.rs:2652:            std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
crates/verter_session/src/semantic_graph_self_root_tests.rs:2828:            std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
crates/verter_session/src/semantic_graph_self_root_tests.rs:2875:            std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
crates/verter_session/src/u6_flow_expect_tests.rs:568:        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/u6_flow_expect_tests.rs:713:    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/request_store_view_derived_hash_tests.rs:1://! Discriminating tests for `RequestStoreView::derived_hash_for`.
crates/verter_session/src/request_store_view_derived_hash_tests.rs:5://! Production request paths build a `RequestStoreView` (overlay + base)
crates/verter_session/src/request_store_view_derived_hash_tests.rs:17://! Before this fix, the `StoreView` impl on `RequestStoreView` did not
crates/verter_session/src/request_store_view_derived_hash_tests.rs:19://! that returns `None`. Real mismatches on a `RequestStoreView` were
crates/verter_session/src/request_store_view_derived_hash_tests.rs:25://!   `RequestStoreView::derived_hash_for` returns `None` for every
crates/verter_session/src/request_store_view_derived_hash_tests.rs:35:    CanonicalCompletionOverlay, DerivedFactKind, RequestStoreView, StoreView,
crates/verter_session/src/request_store_view_derived_hash_tests.rs:67:/// `RequestStoreView` wrapper MUST return the same `Some(hash)`.
crates/verter_session/src/request_store_view_derived_hash_tests.rs:69:/// Discriminating: pre-fix, `RequestStoreView::derived_hash_for`
crates/verter_session/src/request_store_view_derived_hash_tests.rs:93:    let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/request_store_view_derived_hash_tests.rs:94:    let req = RequestStoreView::new(&base, overlay);
crates/verter_session/src/request_store_view_derived_hash_tests.rs:100:        "RequestStoreView::derived_hash_for MUST return the same answer as \
crates/verter_session/src/request_store_view_derived_hash_tests.rs:122:    let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/request_store_view_derived_hash_tests.rs:139:    let req = RequestStoreView::new(&base, Arc::clone(&overlay));
crates/verter_session/src/request_store_view_derived_hash_tests.rs:145:        "RequestStoreView::derived_hash_for MUST consult the overlay \
crates/verter_session/src/non_cacheable_bundle_reuse_tests.rs:43://! [`RequestBundleMemo`](crate::resolver_core::request_store_view::RequestBundleMemo),
crates/verter_session/src/non_cacheable_bundle_reuse_tests.rs:130:    memo: &crate::resolver_core::CanonicalCompletionOverlay,
crates/verter_session/src/non_cacheable_bundle_reuse_tests.rs:168:    let request = crate::resolver_core::CanonicalCompletionOverlay::new();
crates/verter_session/src/non_cacheable_bundle_reuse_tests.rs:218:    let request = crate::resolver_core::CanonicalCompletionOverlay::new();
crates/verter_session/src/resolver_core/resolver_context.rs:447:    /// can hand back a [`crate::resolver_core::RequestStoreView`]
crates/verter_session/src/resolver_core/resolver_context.rs:449:    /// [`crate::resolver_core::CanonicalCompletionOverlay`] in front of
crates/verter_session/src/resolver_core/resolver_context.rs:686:    /// [`CanonicalCompletionOverlay`](crate::resolver_core::CanonicalCompletionOverlay)
crates/verter_session/src/resolver_core/resolver_context.rs:694:    /// (`CanonicalCompletionOverlay::bundle_memo`) — the R17-compliant
crates/verter_session/src/resolver_core/resolver_context.rs:703:    ) -> Option<&crate::resolver_core::CanonicalCompletionOverlay> {
crates/verter_session/src/tests/overlay_pollution_probe.rs:81:        std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
crates/verter_session/src/typeinfo/oracle_core/source_walk/tests.rs:14:use crate::resolver_core::{CanonicalCompletionOverlay, HostResolverContext};
crates/verter_session/src/typeinfo/oracle_core/source_walk/tests.rs:45:    let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/resolver_core/session_resolver_context.rs:45:use crate::resolver_core::request_store_view::{CanonicalCompletionOverlay, RequestStoreView};
crates/verter_session/src/resolver_core/session_resolver_context.rs:75:/// The owned [`RequestStoreView`] field chains a
crates/verter_session/src/resolver_core/session_resolver_context.rs:76:/// [`CanonicalCompletionOverlay`] in front of an overlay-rooted base
crates/verter_session/src/resolver_core/session_resolver_context.rs:88:    request_view: RequestStoreView<'a>,
crates/verter_session/src/resolver_core/session_resolver_context.rs:109:        overlay: Arc<CanonicalCompletionOverlay>,
crates/verter_session/src/resolver_core/session_resolver_context.rs:114:            request_view: RequestStoreView::new(base, overlay),
crates/verter_session/src/resolver_core/session_resolver_context.rs:125:    /// context MISSES (the `RequestStoreView` fails its `validates*` family
crates/verter_session/src/resolver_core/session_resolver_context.rs:143:        overlay: Arc<CanonicalCompletionOverlay>,
crates/verter_session/src/resolver_core/session_resolver_context.rs:148:            request_view: RequestStoreView::new_cold_seed(base.view(), overlay, base.is_current()),
crates/verter_session/src/resolver_core/session_resolver_context.rs:158:    pub(crate) fn overlay(&self) -> &Arc<CanonicalCompletionOverlay> {
crates/verter_session/src/resolver_core/session_resolver_context.rs:234:    /// `CanonicalCompletionOverlay::bundle_memo`). Every context this
crates/verter_session/src/resolver_core/session_resolver_context.rs:238:    fn request_completion_overlay(&self) -> Option<&CanonicalCompletionOverlay> {
crates/verter_session/src/resolver_core/session_resolver_context.rs:490:    /// [`CanonicalCompletionOverlay`].
crates/verter_session/src/resolver_core/session_resolver_context.rs:506:    /// The chained [`CanonicalCompletionOverlay`] shadows the base view
crates/verter_session/src/meta_resolve_tests.rs:9094:        std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
crates/verter_session/src/meta_resolve_tests.rs:9222:        std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
crates/verter_session/src/meta_resolve_tests.rs:9363:        std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
crates/verter_session/src/framework/api_projectors/svelte.rs:200:                Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
crates/verter_session/src/resolver_core/host_resolver_context.rs:54:use crate::resolver_core::request_store_view::{CanonicalCompletionOverlay, RequestStoreView};
crates/verter_session/src/resolver_core/host_resolver_context.rs:66:/// Holds `(&'a VerterHost, RequestStoreView<'a>)`. Every
crates/verter_session/src/resolver_core/host_resolver_context.rs:69:/// [`RequestStoreView`] field. The base [`HostStoreView`] is built
crates/verter_session/src/resolver_core/host_resolver_context.rs:74:/// The owned [`RequestStoreView`] chains a
crates/verter_session/src/resolver_core/host_resolver_context.rs:75:/// [`CanonicalCompletionOverlay`] in front of the borrowed base view
crates/verter_session/src/resolver_core/host_resolver_context.rs:83:    view: RequestStoreView<'a>,
crates/verter_session/src/resolver_core/host_resolver_context.rs:92:    /// owns its own [`RequestStoreView`]; the base view is borrowed for
crates/verter_session/src/resolver_core/host_resolver_context.rs:93:    /// the duration of the wrapper. Pass an `Arc<CanonicalCompletionOverlay>`
crates/verter_session/src/resolver_core/host_resolver_context.rs:101:        overlay: Arc<CanonicalCompletionOverlay>,
crates/verter_session/src/resolver_core/host_resolver_context.rs:105:            view: RequestStoreView::new(base, overlay),
crates/verter_session/src/resolver_core/host_resolver_context.rs:121:        overlay: Arc<CanonicalCompletionOverlay>,
crates/verter_session/src/resolver_core/host_resolver_context.rs:125:            view: RequestStoreView::new(base.view(), overlay),
crates/verter_session/src/resolver_core/host_resolver_context.rs:135:    /// warm-cache probe through this context MISSES (the `RequestStoreView`
crates/verter_session/src/resolver_core/host_resolver_context.rs:143:        overlay: Arc<CanonicalCompletionOverlay>,
crates/verter_session/src/resolver_core/host_resolver_context.rs:147:            view: RequestStoreView::new_cold_seed(base.view(), overlay, base.is_current()),
crates/verter_session/src/resolver_core/host_resolver_context.rs:170:    pub(crate) fn overlay(&self) -> &Arc<CanonicalCompletionOverlay> {
crates/verter_session/src/resolver_core/host_resolver_context.rs:201:        // Pass the request-bound `RequestStoreView` (which chains the
crates/verter_session/src/resolver_core/host_resolver_context.rs:202:        // `CanonicalCompletionOverlay` in front of the base) so cache
crates/verter_session/src/resolver_core/host_resolver_context.rs:222:    fn request_completion_overlay(&self) -> Option<&CanonicalCompletionOverlay> {
crates/verter_session/src/resolver_core/host_resolver_context.rs:560:    let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/types.rs:3875:    /// bundle read is served from the request's `RequestBundleMemo`
crates/verter_session/src/meta_resolve/projectors/define_shapes_tests.rs:9:use crate::resolver_core::{CanonicalCompletionOverlay, HostResolverContext};
crates/verter_session/src/meta_resolve/projectors/define_shapes_tests.rs:47:    let overlay = std::sync::Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/meta_resolve/projectors/define_shapes_tests.rs:65:    let overlay = std::sync::Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/meta_resolve/projectors/define_shapes_tests.rs:430:    let overlay = std::sync::Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/meta_resolve/projectors/define_shapes_tests.rs:483:    let overlay = std::sync::Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/typeinfo/oracle_core/source_digest.rs:29:use crate::resolver_core::{CanonicalCompletionOverlay, HostResolverContext};
crates/verter_session/src/typeinfo/oracle_core/source_digest.rs:66:    let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/cache_runtime/flow_slice_node_tests.rs:912:    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/framework/script_facts.rs:1117:            let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/host_flow_return_audit.rs:131:                    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/artifact_reads_pinned_tests.rs:391:        std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
crates/verter_session/src/host_resolve_type_audit.rs:271:                let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/resolver_core/mod.rs:57:pub(crate) use request_store_view::{CanonicalCompletionOverlay, RequestStoreView};
crates/verter_session/src/resolver_core/mod.rs:516:/// rides the request-bound, currentness-gated `RequestStoreView` rather
crates/verter_session/src/typeinfo/oracle_core/relation_driver.rs:148:    let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/host_manage_tests.rs:7697:    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/host_manage_tests.rs:12231:    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/host_manage_tests.rs:17859:// intentionally not part of this suite: the `RequestStoreView` type
crates/verter_session/src/flow_gap_retraction_tests.rs:50:    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/component_meta_caches_tests.rs:40:    run_component_meta_request, CanonicalCompletionOverlay, ComponentMetaCacheLookup,
crates/verter_session/src/component_meta_caches_tests.rs:156:        overlay: Arc::new(CanonicalCompletionOverlay::new()),
crates/verter_session/src/component_meta_caches_tests.rs:500:            overlay: Arc::new(CanonicalCompletionOverlay::new()),
crates/verter_session/src/component_meta_caches_tests.rs:581:        overlay: Arc::new(CanonicalCompletionOverlay::new()),
crates/verter_session/src/component_meta_caches_tests.rs:1512:            overlay: Arc::new(CanonicalCompletionOverlay::new()),
crates/verter_session/src/host_resolve/virtual_file_pipeline.rs:2536:    /// fresh [`crate::resolver_core::CanonicalCompletionOverlay`] (it does NOT
crates/verter_session/src/host_resolve/virtual_file_pipeline.rs:2870:                std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/resolver_core/request_store_view.rs:23://! - [`CanonicalCompletionOverlay`]: request-scoped shadowing side maps
crates/verter_session/src/resolver_core/request_store_view.rs:30://! - [`RequestStoreView`]: a wrapper that owns the overlay and borrows
crates/verter_session/src/resolver_core/request_store_view.rs:49://! [`CanonicalCompletionOverlay::complete_canonical`] is **epoch-
crates/verter_session/src/resolver_core/request_store_view.rs:130:/// [`CanonicalCompletionOverlay::complete_canonical`] (one short
crates/verter_session/src/resolver_core/request_store_view.rs:132:pub(crate) struct CanonicalCompletionOverlay {
crates/verter_session/src/resolver_core/request_store_view.rs:172:    /// `RequestOnly` worlds. See [`RequestBundleMemo`].
crates/verter_session/src/resolver_core/request_store_view.rs:173:    bundle_memo: RequestBundleMemo,
crates/verter_session/src/resolver_core/request_store_view.rs:186:pub(crate) enum BundleMemoWorld {
crates/verter_session/src/resolver_core/request_store_view.rs:226:/// it lives and dies with one `CanonicalCompletionOverlay` (one top-level
crates/verter_session/src/resolver_core/request_store_view.rs:239:/// [`BundleMemoWorld`] supplies the base/overlay namespace split.
crates/verter_session/src/resolver_core/request_store_view.rs:249:pub(crate) struct RequestBundleMemo {
crates/verter_session/src/resolver_core/request_store_view.rs:250:    entries: RwLock<FxHashMap<(String, BundleMemoWorld), BundleMemoEntry>>,
crates/verter_session/src/resolver_core/request_store_view.rs:257:impl RequestBundleMemo {
crates/verter_session/src/resolver_core/request_store_view.rs:267:        world: BundleMemoWorld,
crates/verter_session/src/resolver_core/request_store_view.rs:287:        world: BundleMemoWorld,
crates/verter_session/src/resolver_core/request_store_view.rs:317:    pub(crate) fn len_in_world_for_tests(&self, world: BundleMemoWorld) -> usize {
crates/verter_session/src/resolver_core/request_store_view.rs:328:/// `CanonicalCompletionOverlay::complete_canonical`.
crates/verter_session/src/resolver_core/request_store_view.rs:345:impl Default for CanonicalCompletionOverlay {
crates/verter_session/src/resolver_core/request_store_view.rs:351:impl CanonicalCompletionOverlay {
crates/verter_session/src/resolver_core/request_store_view.rs:364:            bundle_memo: RequestBundleMemo::default(),
crates/verter_session/src/resolver_core/request_store_view.rs:469:    /// tier every bundle producer consults. See [`RequestBundleMemo`].
crates/verter_session/src/resolver_core/request_store_view.rs:471:    pub(crate) fn bundle_memo(&self) -> &RequestBundleMemo {
crates/verter_session/src/resolver_core/request_store_view.rs:801:    /// test for `RequestStoreView::derived_hash_for` overlay coverage.
crates/verter_session/src/resolver_core/request_store_view.rs:856:/// shadowing [`CanonicalCompletionOverlay`].
crates/verter_session/src/resolver_core/request_store_view.rs:877:pub(crate) struct RequestStoreView<'a> {
crates/verter_session/src/resolver_core/request_store_view.rs:879:    overlay: Arc<CanonicalCompletionOverlay>,
crates/verter_session/src/resolver_core/request_store_view.rs:903:impl<'a> RequestStoreView<'a> {
crates/verter_session/src/resolver_core/request_store_view.rs:908:    pub(crate) fn new(base: &'a HostStoreView, overlay: Arc<CanonicalCompletionOverlay>) -> Self {
crates/verter_session/src/resolver_core/request_store_view.rs:931:        overlay: Arc<CanonicalCompletionOverlay>,
crates/verter_session/src/resolver_core/request_store_view.rs:946:    pub(crate) fn overlay(&self) -> &Arc<CanonicalCompletionOverlay> {
crates/verter_session/src/resolver_core/request_store_view.rs:1105:impl<'a> StoreView for RequestStoreView<'a> {
crates/verter_session/src/resolver_core/request_store_view.rs:1246:        // `RequestStoreView` and hit the default `None` arm — every real
crates/verter_session/src/host_manage/component_meta_request_impl.rs:140:/// [`CanonicalCompletionOverlay`](crate::resolver_core::CanonicalCompletionOverlay)
crates/verter_session/src/host_manage/component_meta_request_impl.rs:145:/// promotions through the `RequestStoreView` shadowing rail.
crates/verter_session/src/host_manage/component_meta_request_impl.rs:158:    pub(crate) overlay: std::sync::Arc<crate::resolver_core::CanonicalCompletionOverlay>,
crates/verter_session/src/host_manage/component_meta_request_impl.rs:314:                    std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/host_manage/component_meta_methods.rs:220:        // `CanonicalCompletionOverlay` is built ONCE here at the request
crates/verter_session/src/host_manage/component_meta_methods.rs:228:            overlay: std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
crates/verter_session/src/host_manage/component_meta_methods.rs:533:        overlay: &std::sync::Arc<crate::resolver_core::CanonicalCompletionOverlay>,
crates/verter_session/src/host_manage/component_meta_methods.rs:567:        overlay: &std::sync::Arc<crate::resolver_core::CanonicalCompletionOverlay>,
crates/verter_session/src/host_manage/component_meta_methods.rs:602:        overlay: &std::sync::Arc<crate::resolver_core::CanonicalCompletionOverlay>,
crates/verter_session/src/host_manage/component_meta_methods.rs:626:        overlay: &std::sync::Arc<crate::resolver_core::CanonicalCompletionOverlay>,
crates/verter_session/src/host_manage/component_meta_methods.rs:658:        overlay: &std::sync::Arc<crate::resolver_core::CanonicalCompletionOverlay>,
crates/verter_session/src/host_manage/component_meta_methods.rs:680:        overlay: &std::sync::Arc<crate::resolver_core::CanonicalCompletionOverlay>,
crates/verter_session/src/host_manage/fallthrough.rs:577:            // currentness-gated `RequestStoreView`, built once, so a
crates/verter_session/src/host_manage/prepared_decl.rs:122:        // probe reads through the cold-seed-aware `RequestStoreView`: a
crates/verter_session/src/host_manage/prepared_decl.rs:127:        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/host_manage/prepared_decl.rs:208:    /// `memo` is the request's [`RequestBundleMemo`](crate::resolver_core::request_store_view::RequestBundleMemo)
crates/verter_session/src/host_manage/prepared_decl.rs:215:        memo: Option<&crate::resolver_core::request_store_view::RequestBundleMemo>,
crates/verter_session/src/host_manage/prepared_decl.rs:234:        memo: Option<&crate::resolver_core::request_store_view::RequestBundleMemo>,
crates/verter_session/src/host_manage/prepared_decl.rs:255:        memo: Option<&crate::resolver_core::request_store_view::RequestBundleMemo>,
crates/verter_session/src/host_manage/prepared_decl.rs:261:        use crate::resolver_core::request_store_view::BundleMemoWorld;
crates/verter_session/src/host_manage/prepared_decl.rs:275:            if let Some((bundle, reuse)) = memo.get(canonical_id, BundleMemoWorld::Base, token) {
crates/verter_session/src/host_manage/prepared_decl.rs:312:                        BundleMemoWorld::Base,
crates/verter_session/src/host_manage/prepared_decl.rs:501:                        BundleMemoWorld::Base,
crates/verter_session/src/host_manage/prepared_decl.rs:523:                        BundleMemoWorld::Base,
crates/verter_session/src/host_manage/prepared_decl.rs:547:                        BundleMemoWorld::Base,
crates/verter_session/src/host_manage/prepared_decl.rs:579:        use crate::resolver_core::request_store_view::BundleMemoWorld;
crates/verter_session/src/host_manage/prepared_decl.rs:638:                // `RequestBundleMemo` for the identity contract.
crates/verter_session/src/host_manage/prepared_decl.rs:642:                let world = BundleMemoWorld::Overlay(overlay_hash);
crates/verter_session/src/host_manage/prepared_decl.rs:1339:        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/host_manage/prepared_decl.rs:1353:        memo: Option<&crate::resolver_core::request_store_view::RequestBundleMemo>,
crates/verter_session/src/host_manage/prepared_decl.rs:1409:        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/host_manage/prepared_decl.rs:1419:        memo: Option<&crate::resolver_core::request_store_view::RequestBundleMemo>,
crates/verter_session/src/host_manage/prepared_decl.rs:3000:        // cold-seed-aware `RequestStoreView`, so a known-stale
crates/verter_session/src/host_manage/prepared_decl.rs:3005:        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/host_manage/prepared_decl.rs:3285:        // `RequestStoreView`, so a stale read fails the warm validation
crates/verter_session/src/host_manage/prepared_decl.rs:3288:        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/host_manage/component_meta_entry_resolution.rs:177:        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/host_manage/component_meta_entry_resolution.rs:367:                    std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/host_manage/component_meta_entry_resolution.rs:407:        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/host_manage/component_meta_entry_resolution.rs:512:        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/host_manage/component_meta_entry.rs:247:        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/host_manage/component_meta_entry.rs:371:                std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/host_manage/component_meta_entry.rs:418:        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/host_manage/component_meta_entry.rs:617:        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/host_manage/overlay_priority.rs:144:    let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/meta_resolve/callable_view_tests.rs:18:use crate::resolver_core::{CanonicalCompletionOverlay, HostResolverContext, ResolverContext};
crates/verter_session/src/meta_resolve/callable_view_tests.rs:1247:    let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/meta_resolve/callable_view_tests.rs:1417:            Arc::new(CanonicalCompletionOverlay::new()),
crates/verter_session/src/meta_resolve/callable_view_tests.rs:1471:        Arc::new(CanonicalCompletionOverlay::new()),
crates/verter_session/src/meta_resolve/callable_view_tests.rs:1523:        Arc::new(CanonicalCompletionOverlay::new()),
crates/verter_session/src/meta_resolve/callable_view_tests.rs:1558:        Arc::new(CanonicalCompletionOverlay::new()),
crates/verter_session/src/meta_resolve/callable_view_tests.rs:1608:    let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/meta_resolve/callable_view_tests.rs:1733:    let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/meta_resolve/callable_view_tests.rs:1825:    let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/meta_resolve/callable_view_tests.rs:1929:    let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/meta_resolve/callable_view_tests.rs:2030:    let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/meta_resolve/callable_view_tests.rs:2143:    let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/meta_resolve/callable_view_tests.rs:2262:    let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/meta_resolve/callable_view_tests.rs:2321:    let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/meta_resolve/callable_view_tests.rs:2473:    let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/meta_resolve/callable_view_tests.rs:2530:    let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/meta_resolve/callable_view_tests.rs:2582:    let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/meta_resolve/callable_view_tests.rs:2641:    let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/meta_resolve/callable_view_tests.rs:2709:    let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/meta_resolve/callable_view_tests.rs:3049:    let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/meta_resolve/callable_view_tests.rs:3154:    let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/meta_resolve/callable_view_tests.rs:3321:    let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/meta_resolve/callable_view_tests.rs:3489:    let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/host_manage/analysis_io.rs:157:            let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/host_manage/analysis_io.rs:186:        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/session_view.rs:37://!   globals — `_in_view` / `RequestStoreView` / `CURRENT_REQUEST_VIEW`
crates/verter_session/src/project_global_cache_tests.rs:939:/// Source-audit invariant: the `RequestStoreView` type and the
crates/verter_session/src/project_global_cache_tests.rs:945:    // references to `RequestStoreView` / `CURRENT_REQUEST_VIEW` /
crates/verter_session/src/project_global_cache_tests.rs:986:        // module set — its `RequestStoreView` / `_in_view` audit entry
crates/verter_session/src/project_global_cache_tests.rs:997:        "RequestStoreView",
crates/verter_session/src/u6_flow_shape_corpus_tests.rs:785:    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/u6_flow_shape_corpus_tests.rs:1779:            let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/u6_flow_shape_corpus_tests.rs:1910:                let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/project_semantic_dispatch/flow_return_positional_tests.rs:181:    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/resolver_store.rs:421:/// [`crate::resolver_core::CanonicalCompletionOverlay`] shadows it; for a
crates/verter_session/src/resolver_store.rs:555:    /// signatures. `RequestStoreView::compat_token` intentionally remains
crates/verter_session/src/resolver_store.rs:2927:    /// by [`crate::resolver_core::RequestStoreView`] when a canonical
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:303:    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/project_semantic_dispatch/flow_return_frame_seal_tests.rs:102:    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/session_view_current_content_tests.rs:600:        std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
crates/verter_session/src/session_view_current_content_tests.rs:772:        std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
crates/verter_session/src/session_view_current_content_tests.rs:962:        std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
crates/verter_session/src/store_view_non_current_contract_tests.rs:326:    use crate::resolver_core::{CanonicalCompletionOverlay, HostResolverContext};
crates/verter_session/src/store_view_non_current_contract_tests.rs:349:        let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/store_view_non_current_contract_tests.rs:375:        let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/store_view_non_current_contract_tests.rs:484:    use crate::resolver_core::{CanonicalCompletionOverlay, SessionResolverContext};
crates/verter_session/src/store_view_non_current_contract_tests.rs:513:        let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/store_view_non_current_contract_tests.rs:544:        let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/store_view_non_current_contract_tests.rs:583:        resolver_context::ResolverContext, CanonicalCompletionOverlay, SessionResolverContext,
crates/verter_session/src/store_view_non_current_contract_tests.rs:630:        let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/store_view_non_current_contract_tests.rs:670:        let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/store_view_non_current_contract_tests.rs:705:    use crate::resolver_core::{CanonicalCompletionOverlay, HostResolverContext};
crates/verter_session/src/store_view_non_current_contract_tests.rs:731:        let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/typeinfo/typeinfo_tests/relation_verdict_oracle.rs:179:    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/typeinfo/typeinfo_tests/relation_verdict_oracle.rs:237:    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/cross_file_augmentation_merge_equivalence_tests.rs:683:        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/cross_file_augmentation_merge_equivalence_tests.rs:906:        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/cross_file_augmentation_merge_equivalence_tests.rs:923:        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/cross_file_augmentation_merge_equivalence_tests.rs:1031:        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/cross_file_augmentation_merge_equivalence_tests.rs:1160:        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/cross_file_augmentation_merge_equivalence_tests.rs:1177:        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/cross_file_augmentation_merge_equivalence_tests.rs:1318:        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/cross_file_augmentation_merge_equivalence_tests.rs:1429:                let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/cross_file_augmentation_merge_equivalence_tests.rs:1572:        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/host_manage.rs:708:        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
crates/verter_session/src/host_manage.rs:755:    /// `RequestStoreView` built once at the cold-compute boundary — a
crates/verter_session/src/host_manage.rs:787:        // into the cold compute's currentness-gated `RequestStoreView`,
crates/verter_session/src/typeinfo/typeinfo_tests/vue_adapter_cache.rs:705:    use crate::resolver_core::{CanonicalCompletionOverlay, HostResolverContext};
crates/verter_session/src/typeinfo/typeinfo_tests/vue_adapter_cache.rs:726:    let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/request_bundle_memo_tests.rs:16://! view. [`RequestBundleMemo`](crate::resolver_core::request_store_view::RequestBundleMemo)
crates/verter_session/src/request_bundle_memo_tests.rs:19://! [`CanonicalCompletionOverlay`](crate::resolver_core::CanonicalCompletionOverlay)
crates/verter_session/src/request_bundle_memo_tests.rs:34://! Admission is STRUCTURAL: `RequestBundleMemo::insert` itself refuses
crates/verter_session/src/request_bundle_memo_tests.rs:45:use crate::resolver_core::request_store_view::BundleMemoWorld;
crates/verter_session/src/request_bundle_memo_tests.rs:48:    CanonicalCompletionOverlay, ResolverContext, SessionResolverContext, StoreView,
crates/verter_session/src/request_bundle_memo_tests.rs:104:    Arc<CanonicalCompletionOverlay>,
crates/verter_session/src/request_bundle_memo_tests.rs:110:    (store_view, Arc::new(CanonicalCompletionOverlay::new()))
crates/verter_session/src/request_bundle_memo_tests.rs:166:    let memo_only = CanonicalCompletionOverlay::new();
crates/verter_session/src/request_bundle_memo_tests.rs:169:        BundleMemoWorld::Overlay(overlay_hash),
crates/verter_session/src/request_bundle_memo_tests.rs:304:        memo.len_in_world_for_tests(BundleMemoWorld::Base),
crates/verter_session/src/request_bundle_memo_tests.rs:309:        memo.len_in_world_for_tests(BundleMemoWorld::Overlay(overlay_hash)),
crates/verter_session/src/request_bundle_memo_tests.rs:347:    let overlay = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/request_bundle_memo_tests.rs:531:            .len_in_world_for_tests(BundleMemoWorld::Overlay(overlay_hash)),
crates/verter_session/src/request_bundle_memo_tests.rs:657:    memo: &CanonicalCompletionOverlay,
crates/verter_session/src/request_bundle_memo_tests.rs:719:    let request = CanonicalCompletionOverlay::new();
crates/verter_session/src/request_bundle_memo_tests.rs:753:            .len_in_world_for_tests(BundleMemoWorld::Base),
crates/verter_session/src/request_bundle_memo_tests.rs:777:    let request = CanonicalCompletionOverlay::new();
crates/verter_session/src/request_bundle_memo_tests.rs:815:    let request = CanonicalCompletionOverlay::new();
crates/verter_session/src/request_bundle_memo_tests.rs:891:    let request = CanonicalCompletionOverlay::new();
crates/verter_session/src/request_bundle_memo_tests.rs:916:    let control_request = CanonicalCompletionOverlay::new();
crates/verter_session/src/request_bundle_memo_tests.rs:944:    let request = CanonicalCompletionOverlay::new();
crates/verter_session/src/request_bundle_memo_tests.rs:982:    let miss_request = CanonicalCompletionOverlay::new();
crates/verter_session/src/request_bundle_memo_tests.rs:1025:    let request = CanonicalCompletionOverlay::new();
crates/verter_session/src/request_bundle_memo_tests.rs:1063:    let request = Arc::new(CanonicalCompletionOverlay::new());
crates/verter_session/src/request_bundle_memo_tests.rs:1104:            .len_in_world_for_tests(BundleMemoWorld::Base),
crates/verter_session/src/query_db_self_root_tests.rs:2668:        std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
crates/verter_session/src/query_db_self_root_tests.rs:2737:        std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
crates/verter_session/src/query_db_self_root_tests.rs:2829:        std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
crates/verter_session/src/query_db_self_root_tests.rs:3235:            std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
crates/verter_session/src/query_db_self_root_tests.rs:3369:            std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),

codex
## Disposition

All five findings are **ADOPT-NOW**. None reaches C1’s REOPEN threshold on the evidence presented.

| Finding | Verdict | Confidence |
|---|---|---:|
| F1 | ADOPT-NOW | High |
| F2 | ADOPT-NOW | High on verdict; medium on exact file split |
| F3 | ADOPT-NOW | High |
| F4 | ADOPT-NOW | High |
| F5 | ADOPT-NOW | High |
| F6 | Confirmed | High |

The controlling distinction is C1’s Abort/rescope clause: reopening requires either a fourth lifecycle, inseparability that `AttemptOutcome` **cannot** resolve, or an actually discovered second query-time resolver—not merely an incomplete file/dependency inventory. See [C1.md:403](docs/arch/refactor/rev11/charters/C1.md:403).

### F1 — ADOPT-NOW

This is an inventory error, not a fourth lifecycle.

The ruling assigns `VerterHost`, scheduler access, observation capture, committed-store implementations, and I/O flights to workspace/session. [request_store_view.rs:498](crates/verter_session/src/resolver_core/request_store_view.rs:498) takes `&VerterHost`; its inner completion path reads the live validation token, scheduler, artifact state, and `ProjectTypeStore`. The existing guard explicitly calls it the fourth seal-bridge exemption. [architecture_guards.rs:3645](crates/verter_session/tests/cases/architecture_guards.rs:3645)

Relevant ruling:

> “`verter_workspace`/`verter_session`: observation capture, committed-store implementations, `VerterHost`, scheduler, I/O flights, cache-retention policy...”  
> — [ruling:14389](docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:14389)

Corrected scope:

- Add `resolver_core/request_store_view.rs` as the fourth session-side adapter carve-out.
- Keep `CanonicalCompletionOverlay`, `RequestStoreView`, and their host/scheduler completion machinery in `verter_session`.
- Relocate only the dependency-neutral `StoreView`/immutable observation contracts they implement or wrap, as required by Forks 1 and 3.
- Apply the same module-declaration and guard-retargeting treatment as the other adapter carve-outs.

This does not satisfy “a discovered fourth production lifecycle”; it is another file serving the already-known host/session lifecycles.

### F2 — ADOPT-NOW

The charter’s “dependency-neutral” and “move wholesale” claims are false, but the ruling already decides the ownership boundary.

Fork 1 says:

> “`verter_semantic`: `ModuleResolverCore`, non-flow `TypeInfoCore`, `ProjectSemanticDispatch`, relation/node algorithms, dependency-neutral semantic store/value types, immutable observation contracts...”  
> “The kernel closure must ... exclude compiler/workspace/session/scheduler/provider.”  
> — [ruling:14389](docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:14389)

Therefore C1 cannot move these directories wholesale as currently coupled, nor leave their query algorithms in session. It must split by authority:

- Move query-time type/name/projection algorithms into `verter_semantic`, including the necessary semantic portions of `semantic_query`, `meta_resolve`, `typeinfo`, and `structural_carrier_producer`.
- Move or redefine dependency-neutral DTOs and contracts below the boundary.
- Keep request TLS, host executor data, cache admission/singleflight, `ProjectTypeStore` publication, final component-meta caching, and other lifecycle/publication machinery in `verter_session`.
- Have the session-side publication facade call the relocated semantic kernel through the new immutable observation interface.
- A `ComponentMetaQueryEngine`-named session facade may remain only if it becomes pure lifecycle/publication glue and owns no independent query semantics.

This finding does **not yet establish** the Abort trigger:

> “a discovered second query-time resolution path this research did not find”

The current module declares that “all solve-like operations dispatch through `ProjectSemanticDispatch`,” and the inspected call sites do so. [component_meta_query_engine/mod.rs:1](crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs:1) The risk is real, but it is a migration constraint, not evidence of an existing second authority.

Narrower rule preserving the ruling: split these directories and their dependencies by the Fork-1 ownership list; do not move session cache/publication machinery downward, and do not leave semantic algorithms upward. If the implementation audit finds an actual independent query resolver rather than a facade/projector over `ProjectSemanticDispatch`, that specific discovery would REOPEN C1.

### F3 — ADOPT-NOW

This is the clearest charter correction. The binding ruling already knew that current `ResolverContext` exposes scheduler/I/O and a host escape hatch. It expressly rejected using that trait as the semantic I/O-free interface:

> “use a separate capability-limited immutable observation interface that does **not** extend current `ResolverContext` and cannot return a host.”  
> — [ruling:14407](docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:14407)

The current trait names scheduler cancellation, `IndexedReadyServe`, `ProjectTypeStore`, `HostConfig`, and `VerterHost` directly. [resolver_context.rs:188](crates/verter_session/src/resolver_core/resolver_context.rs:188), [resolver_context.rs:294](crates/verter_session/src/resolver_core/resolver_context.rs:294), [resolver_context.rs:457](crates/verter_session/src/resolver_core/resolver_context.rs:457), [resolver_context.rs:750](crates/verter_session/src/resolver_core/resolver_context.rs:750)

Corrected scope:

- Do **not** relocate the current `ResolverContext` trait body or its existing `sealed` module as a unit.
- Define the separate, capability-limited observation interface in `verter_semantic`; all its projection-reachable methods return `AttemptOutcome<T>`.
- Migrate kernel call sites to that interface.
- Keep or decompose the blocking/session-specific `ResolverContext` surface in `verter_session`; it may adapt to the semantic interface but must not become the semantic interface.
- Keep `RequestBoundResolverContext` and host/scheduler lifecycle concerns on the session side unless they disappear after migration.
- Delete the bare-host `ResolverContext` rail if the required implementation-time call-site audit confirms it is unused.
- Correct C1-AC-1/4/6 and Required Exit language that currently treats the new I/O-free observation implementation as another `ResolverContext` implementor.

The exact Rust mechanism—split traits, semantic-owned adapter, or equivalent sealed construction—is implementation judgment. The non-negotiable boundary is already settled by Fork 3.

### F4 — ADOPT-NOW

The embedding and tracked wrapper are additional migration work, but they do not prove inseparability.

The Abort clause is deliberately narrower:

> evidence that `ProjectResolver` is not cleanly separable ... **“in a way full-coverage `AttemptOutcome` conversion cannot resolve”**  
> — [C1.md:405](docs/arch/refactor/rev11/charters/C1.md:405)

That qualifier is not met here. `ProjectResolver` itself stores project configuration data, while `resolve_tracked` is a thin workspace-controlled wrapper accepting `TrackedResolutionCapability` and `TransactionReader`. [resolver.rs:354](crates/verter_workspace/src/resolver.rs:354)

The new `verter_workspace → verter_semantic` edge is permitted and intended. The architecture says:

> “managed engine depends on compiler/semantic, never the reverse”  
> — [architecture.md:1121](docs/arch/refactor/rev11/architecture.md:1121)

Corrected scope:

- Reverse the current dependency: remove `verter_semantic → verter_workspace`, then allow `verter_workspace → verter_semantic`.
- Let `WorkspaceSnapshot` hold the relocated semantic `ModuleResolverCore` value or an `Arc` to it. [workspace_snapshot.rs:46](crates/verter_workspace/src/workspace_snapshot.rs:46)
- Move/split the resolver’s required dependency-neutral configuration, request, result, membership-observation, and path-resolution vocabulary downward.
- Keep `TrackedResolutionCapability`, `TransactionReader`, transaction capture, publication, and currency enforcement in `verter_workspace`.
- Replace the current inherent `resolve_tracked` method with a workspace-owned adapter/free function/extension that invokes `ModuleResolverCore` through the immutable observation and `AttemptOutcome` boundary.
- Do not leave the resolver algorithm in workspace merely to preserve the existing field or method shape.

No handle/id substitution is architecturally required by the evidence. It remains an implementation-layout option. REOPEN would become necessary only if a concrete resolver operation requires an in-attempt scheduler mutation or commit that cannot be represented as immutable observation plus `NeedInputs` without pulling F1’s committed-input-authority redesign into C1.

### F5 — ADOPT-NOW

This is a missed consumer migration, not a new architecture fork.

The current LSP module publicly wildcard-re-exports the semantic path. [verter_lsp/project_resolver.rs:1](crates/verter_lsp/src/project_resolver.rs:1) The semantic path currently obtains all resolver names through the upward workspace re-export that C1 deletes. [verter_semantic/project_resolver.rs:16](crates/verter_semantic/src/analysis/project_resolver.rs:16)

Corrected rule:

- Delete the `verter_semantic → verter_workspace` re-export source, not the resolver API names without disposition.
- Prefer making the relocated definitions themselves live at the canonical semantic module path, so the existing LSP re-export points at the actual owner rather than a workspace shim.
- If C1 chooses a different canonical semantic path or renames `ProjectResolver` to `ModuleResolverCore`, update `crates/verter_lsp/src/project_resolver.rs` and every LSP consumer in the same change.
- Audit the other current consumers of `verter_semantic::analysis::project_resolver::*` and direct `verter_workspace::ProjectResolver` imports as part of the relocation.
- No compatibility re-export back to `verter_workspace` may survive; that would preserve the forbidden upward edge or create a dual home.

The exact semantic module path is the charter’s acknowledged intra-crate layout judgment. A broken or silently missing public name is not.

### F6 — Confirmed

The current resolver-context seal has exactly three implementors:

- `VerterHost`
- `HostResolverContext`
- `SessionResolverContext`

The three seal implementations are together at [resolver_context.rs:773](crates/verter_session/src/resolver_core/resolver_context.rs:773); the corresponding trait implementations are the bare-host block plus the host- and session-bound adapter blocks.

Therefore no fourth production `ResolverContext` implementor was found, and that named Abort/rescope trigger does not fire. The fourth file in F1 is not a fourth lifecycle.

No files were modified.
tokens used
164 251
## Disposition

All five findings are **ADOPT-NOW**. None reaches C1’s REOPEN threshold on the evidence presented.

| Finding | Verdict | Confidence |
|---|---|---:|
| F1 | ADOPT-NOW | High |
| F2 | ADOPT-NOW | High on verdict; medium on exact file split |
| F3 | ADOPT-NOW | High |
| F4 | ADOPT-NOW | High |
| F5 | ADOPT-NOW | High |
| F6 | Confirmed | High |

The controlling distinction is C1’s Abort/rescope clause: reopening requires either a fourth lifecycle, inseparability that `AttemptOutcome` **cannot** resolve, or an actually discovered second query-time resolver—not merely an incomplete file/dependency inventory. See [C1.md:403](docs/arch/refactor/rev11/charters/C1.md:403).

### F1 — ADOPT-NOW

This is an inventory error, not a fourth lifecycle.

The ruling assigns `VerterHost`, scheduler access, observation capture, committed-store implementations, and I/O flights to workspace/session. [request_store_view.rs:498](crates/verter_session/src/resolver_core/request_store_view.rs:498) takes `&VerterHost`; its inner completion path reads the live validation token, scheduler, artifact state, and `ProjectTypeStore`. The existing guard explicitly calls it the fourth seal-bridge exemption. [architecture_guards.rs:3645](crates/verter_session/tests/cases/architecture_guards.rs:3645)

Relevant ruling:

> “`verter_workspace`/`verter_session`: observation capture, committed-store implementations, `VerterHost`, scheduler, I/O flights, cache-retention policy...”  
> — [ruling:14389](docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:14389)

Corrected scope:

- Add `resolver_core/request_store_view.rs` as the fourth session-side adapter carve-out.
- Keep `CanonicalCompletionOverlay`, `RequestStoreView`, and their host/scheduler completion machinery in `verter_session`.
- Relocate only the dependency-neutral `StoreView`/immutable observation contracts they implement or wrap, as required by Forks 1 and 3.
- Apply the same module-declaration and guard-retargeting treatment as the other adapter carve-outs.

This does not satisfy “a discovered fourth production lifecycle”; it is another file serving the already-known host/session lifecycles.

### F2 — ADOPT-NOW

The charter’s “dependency-neutral” and “move wholesale” claims are false, but the ruling already decides the ownership boundary.

Fork 1 says:

> “`verter_semantic`: `ModuleResolverCore`, non-flow `TypeInfoCore`, `ProjectSemanticDispatch`, relation/node algorithms, dependency-neutral semantic store/value types, immutable observation contracts...”  
> “The kernel closure must ... exclude compiler/workspace/session/scheduler/provider.”  
> — [ruling:14389](docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:14389)

Therefore C1 cannot move these directories wholesale as currently coupled, nor leave their query algorithms in session. It must split by authority:

- Move query-time type/name/projection algorithms into `verter_semantic`, including the necessary semantic portions of `semantic_query`, `meta_resolve`, `typeinfo`, and `structural_carrier_producer`.
- Move or redefine dependency-neutral DTOs and contracts below the boundary.
- Keep request TLS, host executor data, cache admission/singleflight, `ProjectTypeStore` publication, final component-meta caching, and other lifecycle/publication machinery in `verter_session`.
- Have the session-side publication facade call the relocated semantic kernel through the new immutable observation interface.
- A `ComponentMetaQueryEngine`-named session facade may remain only if it becomes pure lifecycle/publication glue and owns no independent query semantics.

This finding does **not yet establish** the Abort trigger:

> “a discovered second query-time resolution path this research did not find”

The current module declares that “all solve-like operations dispatch through `ProjectSemanticDispatch`,” and the inspected call sites do so. [component_meta_query_engine/mod.rs:1](crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs:1) The risk is real, but it is a migration constraint, not evidence of an existing second authority.

Narrower rule preserving the ruling: split these directories and their dependencies by the Fork-1 ownership list; do not move session cache/publication machinery downward, and do not leave semantic algorithms upward. If the implementation audit finds an actual independent query resolver rather than a facade/projector over `ProjectSemanticDispatch`, that specific discovery would REOPEN C1.

### F3 — ADOPT-NOW

This is the clearest charter correction. The binding ruling already knew that current `ResolverContext` exposes scheduler/I/O and a host escape hatch. It expressly rejected using that trait as the semantic I/O-free interface:

> “use a separate capability-limited immutable observation interface that does **not** extend current `ResolverContext` and cannot return a host.”  
> — [ruling:14407](docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md:14407)

The current trait names scheduler cancellation, `IndexedReadyServe`, `ProjectTypeStore`, `HostConfig`, and `VerterHost` directly. [resolver_context.rs:188](crates/verter_session/src/resolver_core/resolver_context.rs:188), [resolver_context.rs:294](crates/verter_session/src/resolver_core/resolver_context.rs:294), [resolver_context.rs:457](crates/verter_session/src/resolver_core/resolver_context.rs:457), [resolver_context.rs:750](crates/verter_session/src/resolver_core/resolver_context.rs:750)

Corrected scope:

- Do **not** relocate the current `ResolverContext` trait body or its existing `sealed` module as a unit.
- Define the separate, capability-limited observation interface in `verter_semantic`; all its projection-reachable methods return `AttemptOutcome<T>`.
- Migrate kernel call sites to that interface.
- Keep or decompose the blocking/session-specific `ResolverContext` surface in `verter_session`; it may adapt to the semantic interface but must not become the semantic interface.
- Keep `RequestBoundResolverContext` and host/scheduler lifecycle concerns on the session side unless they disappear after migration.
- Delete the bare-host `ResolverContext` rail if the required implementation-time call-site audit confirms it is unused.
- Correct C1-AC-1/4/6 and Required Exit language that currently treats the new I/O-free observation implementation as another `ResolverContext` implementor.

The exact Rust mechanism—split traits, semantic-owned adapter, or equivalent sealed construction—is implementation judgment. The non-negotiable boundary is already settled by Fork 3.

### F4 — ADOPT-NOW

The embedding and tracked wrapper are additional migration work, but they do not prove inseparability.

The Abort clause is deliberately narrower:

> evidence that `ProjectResolver` is not cleanly separable ... **“in a way full-coverage `AttemptOutcome` conversion cannot resolve”**  
> — [C1.md:405](docs/arch/refactor/rev11/charters/C1.md:405)

That qualifier is not met here. `ProjectResolver` itself stores project configuration data, while `resolve_tracked` is a thin workspace-controlled wrapper accepting `TrackedResolutionCapability` and `TransactionReader`. [resolver.rs:354](crates/verter_workspace/src/resolver.rs:354)

The new `verter_workspace → verter_semantic` edge is permitted and intended. The architecture says:

> “managed engine depends on compiler/semantic, never the reverse”  
> — [architecture.md:1121](docs/arch/refactor/rev11/architecture.md:1121)

Corrected scope:

- Reverse the current dependency: remove `verter_semantic → verter_workspace`, then allow `verter_workspace → verter_semantic`.
- Let `WorkspaceSnapshot` hold the relocated semantic `ModuleResolverCore` value or an `Arc` to it. [workspace_snapshot.rs:46](crates/verter_workspace/src/workspace_snapshot.rs:46)
- Move/split the resolver’s required dependency-neutral configuration, request, result, membership-observation, and path-resolution vocabulary downward.
- Keep `TrackedResolutionCapability`, `TransactionReader`, transaction capture, publication, and currency enforcement in `verter_workspace`.
- Replace the current inherent `resolve_tracked` method with a workspace-owned adapter/free function/extension that invokes `ModuleResolverCore` through the immutable observation and `AttemptOutcome` boundary.
- Do not leave the resolver algorithm in workspace merely to preserve the existing field or method shape.

No handle/id substitution is architecturally required by the evidence. It remains an implementation-layout option. REOPEN would become necessary only if a concrete resolver operation requires an in-attempt scheduler mutation or commit that cannot be represented as immutable observation plus `NeedInputs` without pulling F1’s committed-input-authority redesign into C1.

### F5 — ADOPT-NOW

This is a missed consumer migration, not a new architecture fork.

The current LSP module publicly wildcard-re-exports the semantic path. [verter_lsp/project_resolver.rs:1](crates/verter_lsp/src/project_resolver.rs:1) The semantic path currently obtains all resolver names through the upward workspace re-export that C1 deletes. [verter_semantic/project_resolver.rs:16](crates/verter_semantic/src/analysis/project_resolver.rs:16)

Corrected rule:

- Delete the `verter_semantic → verter_workspace` re-export source, not the resolver API names without disposition.
- Prefer making the relocated definitions themselves live at the canonical semantic module path, so the existing LSP re-export points at the actual owner rather than a workspace shim.
- If C1 chooses a different canonical semantic path or renames `ProjectResolver` to `ModuleResolverCore`, update `crates/verter_lsp/src/project_resolver.rs` and every LSP consumer in the same change.
- Audit the other current consumers of `verter_semantic::analysis::project_resolver::*` and direct `verter_workspace::ProjectResolver` imports as part of the relocation.
- No compatibility re-export back to `verter_workspace` may survive; that would preserve the forbidden upward edge or create a dual home.

The exact semantic module path is the charter’s acknowledged intra-crate layout judgment. A broken or silently missing public name is not.

### F6 — Confirmed

The current resolver-context seal has exactly three implementors:

- `VerterHost`
- `HostResolverContext`
- `SessionResolverContext`

The three seal implementations are together at [resolver_context.rs:773](crates/verter_session/src/resolver_core/resolver_context.rs:773); the corresponding trait implementations are the bare-host block plus the host- and session-bound adapter blocks.

Therefore no fourth production `ResolverContext` implementor was found, and that named Abort/rescope trigger does not fire. The fourth file in F1 is not a fourth lifecycle.

No files were modified.
