# A5 — Resolved current-owner rows

Resolves the sixteen `VERIFY` seed rows in
[`contracts/current-tree-reconciliation.md`](../../contracts/current-tree-reconciliation.md) §3
against the post-A4 tree. Every row is source-verified: each cites a real file and symbol read
from this checkout, not a restatement of `CLAUDE.md` or of the seed table.

Dispositions use the contract's §1 closed set: **Preserve**, **Converge**, **Replace**,
**Delete**, **Defer**.

**What "resolved" means here, and what it does not.** A5's exit criterion is that later blocks
cannot create a second owner or discover a hidden public/wire consumer by omission. So each row
below fixes the *surviving authority* and the *disposition*, and names the block that owns the
cutover. The contract's §4 completion rule — exact deletion set, characterization tests, proof
that adjacent rows are outside the causal closure — is a **per-block** gate ("before a block
enters `BLOCK_READY`"), not an A5 deliverable, and rows deferred to a later block deliberately
carry no deletion set here. Inventing one would be the hidden implementation decision this block
exists to remove.

Two seed rows were found to be **materially wrong about the current tree** (rows 9 and 13). Both
are called out in full; they are the concrete payoff of running this inventory against source
rather than against the plan's own prose.

---

## Row 1 — Open PRs / branches / queued changes touching an architecture owner

| field | value |
|---|---|
| Source | `git branch -a`, `git worktree list` (captured in [`open-changes.md`](open-changes.md)) |
| Current authority | none — these are unlanded candidates |
| Readers/writers | 10 live worktrees, 7 of them under `.claude/worktrees/` on a different repository root |
| Compatibility | none (nothing published, nothing pushed) |
| Disposition | **Delete** (abandon) for the eight stale `agent/rc-*`, `perf/*`, `feat/*` branches; **Preserve** for `program/architecture-lock` (the program's own integration lineage) |
| Final owner | orchestrator; requires a maintainer ruling in the shape of R-5 |
| Proof block | A5 (this row); ratification A6 |
| Status | **RESOLVED — pending maintainer ratification** |

Evidence: every candidate branch was cut from a merge-base at or before `2de3b2d07` — before the
squashes that superseded it — so none is a competing forward line. For 468 of the 469 candidates
the blunter reading agrees: the branch is *behind* `main` in content, not ahead of it, e.g.
`git diff main agent/rc-integration --shortstat` reports 43,506 insertions against 196,814
deletions, i.e. the branch is missing ~150k lines that `main` has. Their content landed as the
squashed `main` commits `a7f13496b` / `e6191e280` / `9af553dd2`. The one branch where the net
figure inverts (`port/rust`, whose +370,822 is a single 2,991,892-line generated artifact) is
dispositioned individually in [`open-changes.md`](open-changes.md) §2.1. None can be "included
before freeze"; none competes for an authority the program has not already frozen. Full table,
per-branch numbers, and both tests in [`open-changes.md`](open-changes.md).

## Row 2 — Registered source / VFS / `PublishedRoot` / workspace snapshot

| field | value |
|---|---|
| Source | `crates/verter_language/src/registered_source_authority.rs:219` (`RegisteredSourceAuthority`), `:139` (`RegisteredSourceSnapshot`), `:102` (`RegisteredSourceSnapshotId`); `crates/verter_workspace/src/published_state.rs:73` (`PublishedRoot`); `crates/verter_workspace/src/workspace_snapshot.rs:41` (`WorkspaceSnapshot`) |
| Current authority | `RegisteredSourceAuthority` owns registered carrier source + its snapshot identity; `PublishedRoot` owns publication state; `WorkspaceSnapshot` owns project/ownership resolution (`default_configured_owner_for_file`) |
| Readers/writers | `verter_session` (host upsert/load), `verter_lsp` (sync), `verter_workspace` internals |
| Lifetime | `RegisteredSourceAuthority` in `verter_language` (below the semantic kernel); `PublishedRoot`/`WorkspaceSnapshot` in `verter_workspace` |
| Compatibility | in-memory only; no wire form |
| Disposition | **Converge** into the single committed-input role |
| Final owner | `F1` (one committed input and coherent snapshot authority) |
| Proof block | `F1` |
| Status | **RESOLVED — Defer to `F1`** |

The seed row's warning ("do not create a second `InputStore` by name alone") is upheld: three
distinct owners exist today and each owns a *different* invariant (registered bytes, publication,
project ownership). `F1` converges them; A5 records that no fourth owner may be introduced and
that `WorkspaceSnapshot::default_configured_owner_for_file` is the single provider-neutral
ownership decision (`CLAUDE.md` → Project-Bound External-TS Contract), so it may not be forked
per provider.

## Row 3 — `verter_session::resolver_core` / `ProjectSemanticDispatch`

| field | value |
|---|---|
| Source | `crates/verter_session/src/project_semantic_dispatch/mod.rs:309` (`ProjectSemanticDispatch<'a>`); `crates/verter_session/src/resolver_core/` |
| Current authority | the SOLE query-time type resolver; five query modes; `SemanticQueryKey` → `execute` → `SemanticGraphStore` |
| Readers/writers | component-meta projection, typeinfo executor, framework-surface executor, compile fact emission, LSP/MCP/NAPI/WASM via the host |
| Lifetime | borrowed `'a` over host state; per-request |
| Compatibility | no wire form; `SemanticQueryKey` is content-free (R6) |
| Disposition | **Preserve** the single resolver semantics; **Converge** the crate placement |
| Final owner | `C1` (`ModuleResolverCore` + non-flow `TypeInfoCore`) |
| Proof block | `C1` |
| Status | **RESOLVED — Defer to `C1`** |

Confirmed single: `ProjectSemanticDispatch` is declared once in the workspace. `C1` may change
the *dependency direction* (extraction out of `verter_session`) but may not produce a second
resolver; the shared dispatch chokepoint is `project_semantic_dispatch/mod.rs:2214`, which is also
where A4's `session.semantic_dispatch` attribution scope sits — a second resolver would be
visible as a second chokepoint on that rail.

## Row 4 — `IndexedReady` and the shallow symbol inventory

| field | value |
|---|---|
| Source | `crates/verter_session/src/project_type_store.rs:114` (`pub struct IndexedReady`) |
| Current authority | the canonical post-parse artifact: shallow declaration index + body locators, not a body store |
| Readers/writers | `FileArtifactStore`, prepared-decl builder, component-meta, compile fact emission |
| Lifetime | host-owned, `Send + Sync`, content-addressed |
| Compatibility | cache-internal; keyed `(canonical, content_hash, parse_env_hash, parser_version, file_language_id)` |
| Disposition | **Preserve** |
| Final owner | `verter_session` today; `B2` (parse owner domains) reconciles the parse half, `G4` the store half |
| Proof block | `B2` / `G4` |
| Status | **RESOLVED — Preserve** |

The seed row's condition ("preserve if source proof matches") is met: publication lowers zero
declaration bodies (guard `indexed_ready_publish_lowers_zero_decl_bodies`), and the A4 baseline
measured 8,032 `session.indexed_ready_build` calls against 41 files — 196 per file. That number
is a *serve-path re-entry* count, not a rebuild count, and it is the strongest current argument
for `G4`; it is not evidence against preserving the artifact.

## Row 5 — `DeclBodyMemo` / retained parse workers / `DeclLoweringService`

| field | value |
|---|---|
| Source | `crates/verter_session/src/decl_body_memo.rs:487` (`DeclBodyMemo`); `crates/verter_session/src/decl_lowering.rs` (`DeclLoweringService`, referenced from `lib.rs:741`) |
| Current authority | lazy declaration-body lowering over a retained parse snapshot, keyed `(canonical, whole_hash, parse_env_hash)` |
| Readers/writers | `ProjectSemanticDispatch` (`lower_decl_body_to_node`), prepared-decl builder, locator/binder deref, `locator_span_recovery` |
| Lifetime | worker-shard-retained parse arena; jobs borrow on the worker and return owned typed IR |
| Compatibility | in-memory; no wire form |
| Disposition | **Converge** into managed parse owner domains; the direct compiler stays independent |
| Final owner | `B2` |
| Proof block | `B2` |
| Status | **RESOLVED — Defer to `B2`** |

## Row 6 — `ProjectTypeStore`, `RouteDb`, fact/read-set caches

| field | value |
|---|---|
| Source | `crates/verter_session/src/project_type_store.rs:870` (`ProjectTypeStore`); `crates/verter_session/src/resolver_core/route_db.rs:251` (`RouteDb`); `crates/verter_workspace/src/env_hash.rs` (the five env dimensions) |
| Current authority | one project-global cache graph; `ReadSetSignature.facts` is the sole cache-validity rail |
| Readers/writers | every host-backed query path |
| Lifetime | host-owned; `Arc` hand-out; validated per warm hit against the live `StoreView` |
| Compatibility | `CACHE_CLUSTER_SCHEMA_VERSION = 10` (`crates/verter_session/src/cache_schema.rs`) — a real compatibility domain, see [`compatibility-domains.md`](compatibility-domains.md) |
| Disposition | **Converge**, cache family by cache family |
| Final owner | `G4` (with `G1` owning query identity and `E4` owning reclaimable storage) |
| Proof block | `G4` |
| Status | **RESOLVED — Defer to `G4`; classify per family, never as one row** |

A5 fixes one constraint the later blocks may not relax: the five env-hash dimensions are
*orthogonal*, and a single bundled `project_config_hash` is forbidden (R21). Their producer is
`crates/verter_workspace/src/env_hash.rs` and it is the only place a dimension may be defined;
per-dimension input sets are tabulated in [`compatibility-domains.md`](compatibility-domains.md).

## Row 7 — `SemanticGraphStore` and component-meta materialization caches

| field | value |
|---|---|
| Source | `crates/verter_session/src/semantic_query_memo/mod.rs:234` (`SemanticGraphStore`); `crates/verter_session/src/component_meta_materialize.rs`; `crates/verter_session/src/component_meta_result_db.rs` |
| Current authority | family-memo storage for semantic query results + the component-meta result/structure caches |
| Readers/writers | `ProjectSemanticDispatch::execute_cooperative`, the projector pipeline, NAPI/WASM component-meta entry points |
| Lifetime | host-owned, multi-candidate per family slot, bounded per `FamilyKey::candidate_cap()` |
| Compatibility | `COMPONENT_META_SCHEMA_VERSION = 10` is the *published* payload domain; the store itself is internal |
| Disposition | **Converge** |
| Final owner | `G4`; the public payload half is `E1` → `E3` |
| Proof block | `G4`, `E1` |
| Status | **RESOLVED — Defer, split into internal-store and public-payload halves** |

This row also carries the live instrumentation collision: `SemanticGraphStore::execute_cooperative`
is instrumented **twice** today — by A4's attribution sites and by
`loop5_instrumentation`. Resolved in [`instrumentation-reconciliation.md`](instrumentation-reconciliation.md).

## Row 8 — `FunctionProgramIndex` / `FunctionFlowGraph`

| field | value |
|---|---|
| Source | `crates/verter_semantic/src/analysis/function_program.rs:554` (`FunctionProgramIndex`); `crates/verter_semantic/src/analysis/flow/flow_graph.rs:155` (`FunctionFlowGraph`) |
| Current authority | the canonical flow structure; per AMD-004 the sole demanded completion authority |
| Readers/writers | `verter_session` flow-slice substrate, `FlowSliceStores` |
| Lifetime | `verter_semantic`, content-free structural inventory |
| Compatibility | internal |
| Disposition | **Preserve**, extend the same graph only |
| Final owner | `D6` / `U6.LOOP_CLOSURE` |
| Proof block | `D6` |
| Status | **RESOLVED — Preserve** |

Source evidence does not disprove the seed hypothesis, so the seed's `PRESERVE unless disproven`
stands. AMD-004 / ruling R-11 additionally forbid a syntax-only fallback or a second classifier;
debt `FR-D8` is owned by `D6`.

## Row 9 — `flow_slice_content.rs` — SEED ROW IS WRONG ABOUT THE CURRENT TREE

| field | value |
|---|---|
| Seed claim | "second flow/control semantics path … REPLACE/DELETE through final flow blocks; do not port it as a new IR" |
| Source | `crates/verter_session/src/flow_slice_content.rs` (5,407 lines) |
| What it actually is | the **content half** of the one flow substrate: `FunctionProgramIndex` is the eager structural inventory, `verter_semantic::analysis::flow` plans the demanded slice as graph reachability into the content-free `FlowSliceIR`, and this module lowers *only* the slice-selected expression content into owned typed IR |
| Readers/writers | the flow evaluator; `FlowSliceStores` |
| Compatibility | internal; unsupported constructs are typed and fail closed (`SliceStatement::Unsupported` propagates to the root and degrades the whole result) |
| Disposition | **Defer** — and re-state the row before `D2` ratifies a charter against it |
| Final owner | `D2` (atomic public flow cutover), with `D6` owning completion |
| Proof block | `D2` |
| Status | **RESOLVED — seed premise NOT source-verified; row re-stated** |

This is a load-bearing correction. A `D2` charter written from the seed row would have been
ratified against "delete the second flow engine", and the implementor would then have found no
second engine to delete. The module's own header states the single-substrate relationship, and it
routes expression lowering through the **one** shared shallow-pass per-expression lowering
(`infer_declaration_expression_type`) rather than a private evaluator. A5 does **not** claim the
module is architecturally final — that is `D2`'s judgment — only that "second flow/control
semantics path" is not a source-verified description of it today.

## Row 10 — `CodeTransform`

| field | value |
|---|---|
| Source | `crates/verter_compiler/src/code_transform/code_transform.rs:48` (`CodeTransform<'a>`) |
| Current authority | atomic code-plus-mapping transformation; chunk list and source map stay consistent by construction |
| Readers/writers | `verter_compiler` (566 references), `verter_lsp` (31), `verter_session` (15), `verter_audit` (6) |
| Lifetime | borrowed `'a` over the source; per-compile |
| Compatibility | `CLAUDE.md` → CodeTransform Is the Single Source of Truth (CRITICAL); guard `compile_audit_sourcemap` |
| Disposition | **Preserve** semantics; **Converge** into the compact source-unit cutover |
| Final owner | `B4` |
| Proof block | `B4` |
| Status | **RESOLVED — Preserve semantics, Defer placement to `B4`** |

`B4`'s atomicity clause already binds: it does not land while any production compiler route still
requires full-carrier whitespace blanking for the migrated source-unit family.

## Row 11 — `StyleSyntaxIr` and the current fast CSS paths

| field | value |
|---|---|
| Source | `crates/verter_css_syntax/src/style_ir.rs:375` (`StyleSyntaxIr`), `:570` (`StyleSyntaxIrSink`) |
| Current authority | the CSS-family syntax/transform substrate, in its own leaf crate (`verter_css_syntax` depends only on `verter_span`) |
| Readers/writers | `verter_css_syntax` (11), `verter_compiler` (4) |
| Lifetime | per-compile |
| Compatibility | internal |
| Disposition | **Preserve** one syntax authority |
| Final owner | `J1` (reconcile CSS syntax/formatter/scanner/transform owners) |
| Proof block | `J1` |
| Status | **RESOLVED — Defer to `J1`** |

The seed row's guard stands and is now dated: `main` commit `e6191e280`
("refactor(css): replace lightning css with custom parser") already performed one CSS owner
replacement. `J1` may not delete a proven specialized fast path without equivalent-work evidence,
and A4 recorded that the CSS attribution sites (`compiler.css_parse`, `compiler.css_transform`,
`compiler.style_analysis`) record **zero** on the component-meta baseline corpus — so
equivalent-work evidence for `J1` requires a CSS-bearing workload, not the A4 baseline.

## Row 12 — component-meta native / compat boundary

| field | value |
|---|---|
| Source | `crates/verter_session/src/meta_resolve/` (native projection); `packages/component-meta/` (`@verter/component-meta`, PUBLISHED); `crates/verter_protocol/proto/verter/v1/component_meta.proto` |
| Current authority | the native payload is the semantic authority; `@verter/component-meta/compat` is a projection layer |
| Readers/writers | see [`consumer-protocol-map.md`](consumer-protocol-map.md) |
| Compatibility | published npm package at `0.0.1-beta.3` (pre-1.0); `COMPONENT_META_SCHEMA_VERSION = 10` |
| Disposition | **Preserve** the boundary rule; **Converge** the payload at `E1`/`E3` |
| Final owner | `E1` → `E3` |
| Proof block | `E1` |
| Status | **RESOLVED — Defer to `E1`; consumer inventory delivered by A5** |

Guards `no_napi_direct_verter_compiler_emitters` and `compat_one_napi_call_audit` hold the
"one async native request per query / JS is not a second resolver" half today.

## Row 13 — Provider lifecycle — SEED ROW NAMES A TYPE THAT DOES NOT EXIST

| field | value |
|---|---|
| Seed claim | "`ProviderHub`/`SyncCoordinator`/provider actors" |
| Finding | **`ProviderHub` does not exist in this tree.** `grep -rn "ProviderHub" crates/*/src` returns nothing. It is a Revision-11 target name (`H2 — Project-scoped ProviderHub bindings`), not a current owner. |
| Actual current owners | `crates/verter_lsp/src/sync_coordinator.rs:52` (`SyncCoordinatorHandle`), `:238` (`SyncCoordinatorDeps`); `crates/verter_type_runtime/src/traits.rs:130` (`pub trait TypeProvider`); `crates/verter_session/src/external_ts/engine.rs:410` (`trait EngineBackend`), `:300` (`BoundProject`), `:279` (`BoundProjectSeal`); `crates/verter_session/src/external_ts/resolver.rs:63` (`ProjectBinding`) |
| Lifetime | `SyncCoordinator` is a stateful actor in `verter_lsp`; `TypeProvider` implementations are `Send + Sync` in `verter_type_runtime`; `BoundProject` is a type-state witness in `verter_session` |
| Compatibility | `PROTOCOL_VERSION = 2` (`crates/verter_tsgo_api/src/control/messages.rs:21`) and `ADVERTISEMENT_VERSION = 1` (`…/control/advertisement.rs:25`); `provider_protocol_version = 12`, hand-pinned at `crates/verter_protocol/src/consumer_compatibility_manifest.rs:75` and consumed at `:109` — **three** provider-shaped version values with three separate producers, reconciled in [`compatibility-domains.md`](compatibility-domains.md) §4 (finding D-3: whether the third duplicates a domain the first two own is `H2`'s to decide) |
| Disposition | **Preserve** stateful actor ownership; **Converge** stamps/readiness |
| Final owner | `H2` (bindings), `H3` (readiness/publication) |
| Proof block | `H2` |
| Status | **RESOLVED — seed name has no current referent; row re-stated against the real owners** |

A `H2` charter that listed `ProviderHub` under "current authorities" would have been ratified
against a symbol nobody can migrate. The real migration surface is the three-layer
`ExternalTsProjectResolver` → `CarrierRegistry` → `EngineBackend` contract, whose `BoundProject`
witness is already the structural gate (`CLAUDE.md` → Project-Bound External-TS Contract).

## Row 14 — `VerterHost` / session facade

| field | value |
|---|---|
| Source | `crates/verter_session/src/lib.rs:456` (`pub struct VerterHost`) |
| Current authority | the catch-all public entry owner: workspace ops, resolution, compile, component-meta, typeinfo, framework surfaces, audit runtime |
| Readers/writers | `verter_napi`, `verter_wasm`, `verter_lsp`, `verter_mcp`, `verter_ffi`, `verter_tsc`, `verter_bench`, `verter_dx_baseline`, `verter_vue_conformance` |
| Lifetime | process-scoped; `Send + Sync`; owns the scheduler, the CPU pool, and `ProjectTypeStore` |
| Compatibility | reached through NAPI/WASM — see [`consumer-protocol-map.md`](consumer-protocol-map.md) |
| Disposition | **Converge** — reduce only after every extracted invariant has a complete owner |
| Final owner | `K3` |
| Proof block | `K3` |
| Status | **RESOLVED — Defer to `K3`; explicitly NOT reducible before its predecessors** |

## Row 15 — `TypeExpr` producers/consumers and the TypeInfo protobuf graph

| field | value |
|---|---|
| Source | `crates/verter_type_expr/` (the owning crate); `crates/verter_protocol/proto/verter/v1/typeinfo.proto`; `crates/verter_protocol/src/typeinfo/graph.rs` (`TYPEINFO_GRAPH_SCHEMA_VERSION = 7`) |
| Current authority | `TypeExpr` is the internal typed IR; `SemanticTypeGraph` is the wire projection |
| Readers/writers | 16 crates depend on `verter_type_expr`; full producer/consumer/wire table in [`consumer-protocol-map.md`](consumer-protocol-map.md) |
| Compatibility | closed-enum wire contract (`CLAUDE.md` → Typeinfo Wire Contract (CRITICAL)); four guards including the byte-pinned TS binding freshness test |
| Disposition | **Replace** (per `architecture.md` §8.2 and ruling R-3: the plan supersedes `CLAUDE.md`'s Typed-IR-Only rule for the *end state*) |
| Final owner | `E1` (consumer closure) → `E2` (transit elimination) → `E3` (operation DTOs) |
| Proof block | `E1` |
| Status | **RESOLVED — Replace; A5 delivers the seed consumer inventory that `E1` turns into the exact map** |

`E1`'s predecessor list is `A5, C1, D2`, so this row's completion is by construction A5's
responsibility only to the extent of *enumeration*. The enumeration is complete in the sense the
exit criterion demands: every crate and every published package that touches `TypeExpr` or the
graph wire is named, with counts, so `E1` cannot discover an unnamed one.

## Row 16 — Audit TLS / substrate / runtime

| field | value |
|---|---|
| Source | `crates/verter_audit/` (leaf substrate; depends only on `verter_span`); `crates/verter_audit/src/observer.rs` (`current_observer`); `crates/verter_session/src/host_audit_runtime.rs:66` (`pub struct HostAuditRuntime`, 22 references in its own module, 37 across `crates/` + `packages/`); `crates/verter_audit/src/attribution/` (A4) |
| Current authority | deterministic optional observability; the concrete host runtime lives in `verter_session`, the substrate stays a leaf |
| Readers/writers | `verter_session`, `verter_compiler`, `verter_scheduler`, `verter_workspace`, `verter_napi`, `verter_wasm` |
| Lifetime | TLS observer; per-request registration |
| Compatibility | `packages/types/audit.generated.ts` is the TS mirror; `RequestKind` parity guarded |
| Disposition | **Preserve** the leaf dependency direction; **Converge** the *second* instrumentation owner into it |
| Final owner | `verter_audit` (substrate) + `verter_session` (runtime); the convergence is `G4` / `K3` |
| Proof block | A5 decision; execution per [`instrumentation-reconciliation.md`](instrumentation-reconciliation.md) |
| Status | **RESOLVED — Preserve; second owner (`loop5_instrumentation`) dispositioned** |

Dependency direction is held structurally-enough today by `verter_audit_no_upward_deps` and
`audit_substrate_isolation`; both are grandfathered name-keyed scanners, and their durable
replacement is specified in [`dependency-direction.md`](dependency-direction.md).

---

## Rows added by A5 (not in the seed table)

The seed table is explicitly "hypotheses … must be source-verified **and expanded**". Two owners
that carry a real cutover obligation are absent from it:

### Row 17 — Framework adapter substrate

| field | value |
|---|---|
| Source | `crates/verter_session/src/framework/` (registry, descriptor, ctx), `crates/verter_session/src/typeinfo/framework_surface/` (executor, `graph_export`, `vue_exec`, `svelte_exec`) |
| Current authority | one shared adapter substrate; Vue is the reference adapter, not a privileged path |
| Compatibility | **PROVISIONAL wire**: `FrameworkSurfacePayload` rides the existing typeinfo graph envelope, and an outstanding obligation owes the retag to a `TypeInfoGraphPayload` carrier plus a `SemanticTypeGraph.schema_version` bump with the old field reserved (`CLAUDE.md` → Framework Adapter Substrate (CRITICAL), which books it against `U8` — a block of the **pre-existing non-Revision-11 series**, not a row in `program-dag.toml`) |
| Disposition | **Preserve**; the wire retag is an outstanding obligation |
| Final owner | Revision 11 owner is `E3` (public operation DTOs + bounded graph export), which is where the graph wire's compatibility domain is decided; `K1`/`K2` own the catalog/carriers |
| Status | **RESOLVED — recorded so the provisional wire is not mistaken for a frozen one, and so the obligation gets a Revision 11 owner rather than an orphaned out-of-program one** |

This matters for A5's exit criterion specifically: a later block reading only the seed table would
treat the framework-surface wire as settled. It is not.

### Row 18 — Off-store framework-surface caches

| field | value |
|---|---|
| Source | `FrameworkSurfaceStore` / `FrameworkScriptCaches`, held on the framework registry rows rather than on `ProjectTypeStore` |
| Current authority | fact-validated, but **off-store** — a declared exception to "no off-store host caches" |
| Disposition | **Converge** onto `ProjectTypeStore`, with true singleflight |
| Final owner | `G4` (cache/store convergence). `CLAUDE.md` books this against `U10`, a block of the pre-existing non-Revision-11 series with no row in `program-dag.toml`; A5 assigns the Revision 11 owner so the obligation cannot fall between the two programs |
| Status | **RESOLVED — recorded as an open second-cache-owner obligation** |
