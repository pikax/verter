# C1 phase 4 — `ResolverContext` call-site disposition table

**Status update**: `ResolverObservation` has 13 real methods now
(`env_hashes`, `project_identity`, `whole_hash`, `workspace_is_package_backed`,
`lookup_ambient_symbol`, `project_generation`, `type_decl`, `value_decl`,
`module_augmentation_index`, `function_body_skeleton`, `path_probe`,
`real_path`, `package_manifest` — see commits `e67b95c53`..`f8d0afe73` for
the first 8, round 6 for `module_augmentation_index`, round 7 for
`function_body_skeleton`, round 8 for `path_probe`/`real_path`/
`package_manifest`), backed by non-blocking peeks on THREE session-side
stores (`verter_session::DeclBodyMemo`'s `peek_type_decl`/`peek_value_decl`,
never calling `acquire_lease`'s worker-thread rendezvous; the
already-existing `FileArtifactStore::get_augmenter_set`, round 6, no new
session-side code needed; the NEW `FunctionFlowGraphStore::peek`/
`FlowSliceStores::peek_skeleton_for` pair, round 7, plain `DashMap::get`,
confirmed NEVER drives `RetainedSnapshotSkeletonSource`'s blocking cold
build — see F14) and FOUR type relocations (`EnvHashes`;
`LoweredTypeDecl`/`LoweredValueDecl`/`ValueBodyHashFact`; round 6:
`ProjectIdentity`/`AugmentationPopulation`/`AugmentationTargetKind`/
`AugmentationTargetKey`; round 7: the new dependency-neutral
`FlowFunctionObservationKey` — all re-exported from `verter_session` under
their historical names where applicable, zero call-site changes). **Round
8's three methods (F15) are DELIBERATELY INERT** — they reuse
`verter_workspace`'s existing `PathProbe`/`CanonicalId` types (same
`lookup_ambient_symbol` precedent) plus a new narrow
`ResolutionPackageManifest` DTO, and have zero session-side backing peek
and zero production `verter_workspace::resolver` call-site wiring: F15's
consult explicitly rejected relocating any live resolver type/helper in
this pass (a Cargo-cycle/duplicate-authority risk while `verter_semantic`
still depends on `verter_workspace`) — the actual `ProjectResolver` ->
`ModuleResolverCore` conversion these three methods are FOR is its own
separate, not-yet-started coordinated cutover (see F15's evidence file for
the full scope: the "staged priority-frontier batching" `NeedInputs`
shape, the semantic-owned `ResolverAttemptView` implementor design, and
the still-owed characterization/equivalence harness). `type_decl`/`value_decl` closes the "narrow per-declaration-body
demand" piece Part D's design left open — but does NOT fully close the
`shallow_file_state`/`ensure_indexed_ready_serve`/`observe_materialize_scope`
rows below: those return the WHOLE `ShallowFileState`/`IndexedReady`
(exports, imports, route_inventory, whole_hash, file_language,
parse_env_hash, raw_source, built_at_content_generation, ...), of which
decl-bodies is only one part. Those three rows still need their own
narrow-DTO design for the non-decl-body fields.

Working artifact per F8/F9's explicit instruction ("record a call-site
disposition table: required fact, semantic owner, DTO, missing `InputKey`,
side-effect/output disposition, and `cfg`. Only after every row closes
should the trait surface be written"). Authority order: this table is
subordinate to `docs/arch/refactor/rev11/evidence/C1/scoping-spec.md` and
the F7-F16 evidence files in `docs/arch/refactor/rev11/evidence/C1/` —
where this table and those disagree, re-verify against the tree and prefer
the evidence file, then fix this table.

Scope: the 23 `ResolverContext` methods `project_semantic_dispatch`
production code calls (per F8), plus the 28 `host_for_fact_tracer_install`
call sites (per F8, classified below), plus the transitively-required
return-type audit (per F9/F10), plus `semantic_query.rs`/`semantic_query/*`/
`semantic_query_memo/*`'s own per-site usage (round 5, see Part G — CLOSED).
**Not yet exhaustive** — the framework-surface split's exact file boundary
(F10) still needs its own pass before this table can be called complete.

**Sequencing discovery**: at least three of the 23 methods
(`resolve_type_dependency_canonical`, and by strong suspicion —
not yet independently confirmed —
`resolve_imported_type_root_with_facts`/`resolve_value_export_target`/
`normalized_analysis_canonical`'s companion-resolution tail) are built
directly on `verter_workspace`'s resolution-currency/publication machinery
(`ResolutionPublication::{Admitted,Refused}`, `TrackedResolutionCapability`)
— the SAME system F4/phase 7 already targets for the `ProjectResolver` ->
`ModuleResolverCore` conversion. Do not design these methods'
`AttemptOutcome`/`LoadSet` shapes independently of that phase-7 work, or
risk two divergent designs for one resolution system. This is a real
argument for doing phase 7 (or at least its `AttemptOutcome`/`LoadSet`
vocabulary design) BEFORE finishing this group of methods, even though the
scoping-spec's phase order lists phase 7 after phases 4-6 — flag for a
sixth-deviation-style consult if this becomes blocking rather than
guessing at a shape now.

## Part A — the 23 `ResolverContext` methods

| Method | Return type (current) | Return type entangled? | Disposition |
|---|---|---|---|
| `active_session_view` | `Option<&dyn SessionView>` | Yes — live trait object, request-scoped | **TRACED, round 5**: exactly ONE production call site (`project_semantic_dispatch/build.rs:3393`), and it needs ONLY `sv.fingerprint(): u64` — fed straight into `crate::session_view::augmentation_population_for_view(...)` to build `AugmentationTargetKey.population` + an overlay discriminator, which then feed DIRECTLY into `artifact_store.ensure_augmentation_index_populated(...)` (`build.rs:3403` — the SAME `.indexed()`/`FileArtifactStore` call F11 already dispositioned as staying in `verter_session`). `session_overlay_discriminator`'s own body confirms the discriminator is a PURE function of the fingerprint alone (`overlay_artifact_discriminator_from_fingerprint(fingerprint)`, `session_view.rs:1004-1010`) — no further `SessionView` access needed once you have the `u64`. **Does NOT become its own `ResolverObservation` method — folds into `module_augmentation_index()`'s query design.** **CONFIRMED CLOSED, round 7**: `module_augmentation_index` landed (round 6) taking `&AugmentationTargetKey` directly, whose `population: AugmentationPopulation` field (`Base | Session(u64)`) already IS the anticipated `active_session_fingerprint: Option<u64>` dimension — no separate trait method or additional query field was needed; the session-side caller builds the key's `population` from `sv.fingerprint()` exactly as this row predicted, at the eventual wiring step. |
| `artifact_key_for_current_content` | `Option<FileArtifactKey>` | Yes — `FileArtifactKey.build_toolchain_fingerprint` is session-private (confirmed, `file_artifact_store.rs:106`) | **TRACED, round 6**: both production call sites (`project_semantic_dispatch/enumerate.rs:1167`, `fact_signature_helpers.rs:943`) read only `.content_hash`/`.parse_key`/`.file_language_id` from the returned key — narrower than Part A's original "canonical/content_hash/parse_env_hash/parse_key/file_language_id" guess; `.canonical`/`.parse_env_hash` are NOT read at either site (a separately-computed `analysis_canonical` covers the canonical dimension). **BUT confirmed BLOCKED on F12**: the backing session function `authoritative_current_artifact_key` (`host_manage/analysis_io.rs:994`) calls `self.normalized_analysis_canonical(canonical)` at line 998 — the EXACT SAME `host_manage/eval_env.rs:749` function F12 already found blocks `shallow_file_state`/`prepared_decl_bundle`. This is a MECHANICAL application of F12's already-ratified finding (a fourth blocked method group, not a new architecture question) — no fresh consult needed, just recorded here per F12's own "continue every genuinely independent row" instruction (this row is NOT independent). |
| `config` | `&HostConfig` | Yes — `HostConfig` is the whole host config struct (dev_mode, compile_error_policy, lsp_scheme, max_profiles_per_file, resolve_extensions, analysis_level, analysis_scope, generic_root_propagation, ...) | **CONFIRMED narrow**: `project_semantic_dispatch` reads exactly ONE field, `.depth_budget` (`walk.rs:1505`, the only `.config().` call site in the whole directory). Becomes a plain attempt-input `usize` (immediate/no I/O — likely a constructor parameter on `ResolverAttemptView`, not an `AttemptOutcome`-returning method at all). |
| `ensure_indexed_ready_serve` | `Option<IndexedReadyServe>` | Yes — `IndexedReadyServe` wraps `Arc<IndexedReady>` as a session publication/fencing carrier (F9); `IndexedReady` itself embeds `shallow_state: Arc<ShallowFileState>` which embeds `decl_bodies: Arc<DeclBodyMemo>` (session-owned lazy body service, confirmed this round: `project_type_store.rs:123`, `shallow_file_state.rs:78`) | Per F8: nonblocking peek, miss -> `NeedInputs`. The publication-status/fencing half stays session-side. Needs a dependency-neutral `IndexedReady`-shaped DTO split into (a) plain shallow-index fields (whole_hash, file_language, exports, imports, wildcard_reexports, import_targets, route_inventory, parse_env_hash, raw_source — all confirmed plain data) that CAN cross, and (b) a narrow per-declaration-body demand (`AttemptOutcome<LoweredDecl>` or similar) backed by the session-owned `DeclBodyMemo`/`DeclLoweringService` that stays behind. Do NOT hand back `Arc<ShallowFileState>` or `Arc<IndexedReady>` wholesale. |
| `get_whole_hash` | `Option<Hash16>` | No — plain `[u8;16]` | **DONE** (this row's text was stale). Landed as `fn whole_hash(&self, canonical: &str) -> AttemptOutcome<Option<Hash16>>` — `Complete(None)` IS the "genuinely untracked" stable-missing fact (contract §7); `NeedInputs` covers "trackedness itself not yet known," resolving this row's own open question the opposite way its draft text guessed. Corrected round 7. |
| `host_for_fact_tracer_install` | `&crate::VerterHost` | Yes — raw host escape | **DISAPPEARS ENTIRELY** per F8. See Part B for its 28 call sites' individual disposition. |
| `is_cancelled` | `bool` | Indirectly — default body reads `verter_scheduler::cancellation::current_job_cancellation_token()` + `crate::request_context::current_request_cancellation_token()`, both TLS/scheduler state `ResolverObservation` cannot name (F3: "cannot name any scheduler type") | **TRACED, round 5, hypothesis CONFIRMED**: 3 call sites, all in `mod.rs` (577/627/659), all inside query-depth/work-limit tracking (`self.connected_work_limit()`/`self.connected_query_depth_limit()`) — the check sets a `PartialReasonSet::CANCELLED` bit on already-bounded, already-limited walks, an OPTIMIZATION (bail out even faster than the work-limit would) not a correctness requirement. Structurally CANNOT be exposed to `ResolverObservation` regardless (F3's scheduler-type ban is absolute) — the kernel's own work/depth limits already bound one attempt's cost; the session driver checking cancellation BETWEEN attempts (never promoting a mid-flight-cancelled result warm, per CLAUDE.md's Shallow File Processing invariants) fully subsumes this. Does NOT become a `ResolverObservation` method — drop the mid-attempt check, rely on the bounding limits + driver-side between-attempt cancellation. |
| `is_request_bound` | `bool` | No — but it's a LIFECYCLE-IDENTITY question about the CALLER (bare host vs request-bound wrapper), not a fact about resolution state | **TRACED, round 5, hypothesis CONFIRMED**: exactly ONE call site (`mod.rs:516`, `ProjectSemanticDispatch::new`), used ONLY to bump a diagnostic counter (`bump_bare_engine_construction`) — pure session-side construction-time telemetry, zero resolution-affecting use. Does NOT become a `ResolverObservation` method; this counter either moves to the session-side driver's construction path or is dropped once the bare-host rail itself is gone (C1-AC-4). |
| `lookup_ambient_symbol` | `Option<AmbientSymbolHit>` | No — confirmed plain data this round (`ambient_lib.rs:59-64`: `ProjectStableKey`, `Arc<str>`, `Arc<str>`, `u32`) | Clean-ish, BUT `AmbientSymbolHit`/`ProjectStableKey` are `verter_workspace`-owned. `verter_semantic` depends on `verter_workspace` TODAY (fine for now) but F4 reverses that edge in phase 7-8 — this return type needs revisiting once that edge deletes (either `AmbientSymbolHit` gets its own `verter_semantic`-owned mirror, or the ambient-lookup capability moves to the new `verter_workspace -> verter_semantic` shape). Flag, don't block phase 4 on it. |
| `normalized_analysis_canonical` | `Cow<'a, str>` | **CORRECTED — my "unlikely" carry-over assumption was WRONG.** The real body (`host_manage/eval_env.rs:749`, NOT a trivial delegate) is NOT pure string manipulation: for a runtime JS canonical preferring a `.d.ts` companion, it calls `self.resolve_for_persistent_state(canonical_id, candidate, ResolutionContext { phase: CodegenBlocker, kind: TypeImport })` — a genuine WORKSPACE RESOLUTION operation with an `Admitted`/`Refused` outcome — to decide whether the companion resolves. | Same "resolve_* operations -> move the algorithm into the kernel, expose the facts it needs" bucket as `resolve_imported_type_root_with_facts`/`resolve_type_dependency_canonical`/`resolve_value_export_target`, NOT a quick narrow-fact add. The early-out fast paths (empty id, raw import specifier, non-runtime-JS with an explicit extension) ARE pure and could stay a cheap synchronous prefix; only the companion-resolution tail needs porting. Do not add this method to `ResolverObservation` until that resolution algorithm is traced (not done this round). |
| `observe_borrowed_signature` | `()` (side-effecting: writes to the active fact tracer) | Type-wise no (takes `&[FactVersionRef]`), but `FactVersionRef` is `verter_workspace::fact_read_set::FactVersionRef` (confirmed this round — NOT in `verter_session` as I originally assumed) | **CONFIRMED, round 5**: 7 production call sites (`carrier.rs`, `locator_shape.rs`, `raise.rs` ×3, `symbol_identity.rs` ×2), every one `self.ctx.observe_borrowed_signature(&route_facts)`. Default body: `fact_tracer_tls::observe_fan_out_borrowed(sig)` — writes a `verter_session`-owned THREAD-LOCAL, definitively cannot cross the crate boundary as a read. Side-effect/output disposition per F8: becomes part of the `AttemptOutcome`'s OUTPUT — the kernel accumulates every observed `FactVersionRef` slice into its own return value (an "attempt output" bundle, same shape as F11's `ShapeCacheAdmissionCandidate`), and the session driver writes them into the TLS tracer AFTER a `Complete` attempt. `FactVersionRef` CONFIRMED CLEAN (round 7, Part C) and `AttemptOutput::record_fact` LANDED (round 8, F16) as the accumulator slot this method's output feeds — the method itself is still not wired (needs the relocated kernel call sites this observation is threaded through, which don't exist yet). |
| `observe_materialize_scope` | `Option<MaterializeScopeObservation>` | Yes — `MaterializeScopeObservation.indexed: Arc<IndexedReady>` (confirmed this round, `resolver_context.rs:127`) | Same `IndexedReady`-embedding problem as `ensure_indexed_ready_serve`. F9: "needs a narrower no-tear DTO." Depends on the same `IndexedReady` split above. |
| `prepared_decl_bundle` | `Option<Arc<PreparedDeclBundle>>` | **CORRECTED, round 4 — my round-1/round-3 "clean" claim was WRONG.** `PreparedDeclBundle.prepared_type_decls: PreparedTypeDeclCache` / `.prepared_value_decls: PreparedValueDeclCache` (`prepared_decl.rs:851-863`, `:1099-1110`) themselves embed `state: Arc<ShallowFileState>` (entangled — carries `decl_bodies: Arc<DeclBodyMemo>`), `interner: Arc<IdentityInterner>` (session-owned — its own top-level file `identity_interner.rs`, doc'd as "store-owned identity intern pool, per-store lifetime"), and `import_canonicalization: Arc<ImportCanonicalization>` (**CONFIRMED CLEAN, round 5**: `ImportCanonicalization.final_resolution: FxHashMap<DeclBindingKey, ResolvedRootIdentity>` — `ResolvedRootIdentity` already `verter_semantic`-owned per round 1; genuinely plain data, `prepared_decl.rs:157-163`). Round-1's audit only read `PreparedDeclBundle`'s OWN field TYPES without checking those types' transitive contents — an incomplete audit, now corrected. | NOT clean, AND the per-symbol `PreparedTypeDeclCache::get_in`/`PreparedValueDeclCache::get_in` slot design (`OnceLock<Option<Arc<T>>> + parking_lot::Mutex` build gate, `prepared_decl.rs:912-1019`) IS peek-safe by itself, mirroring `DeclBodyMemo`'s `DemandCell` EXACTLY: `slot.value.get()` non-blocking; a `LeaseMiss` OR a genuine `PreparationFailure` (`MissingExternalOwner`/`AuthoredOrdinalOverflow`, confirmed this round — NEITHER commits to `slot.value`, both leave it vacant per `prepared_decl.rs:1004-1018`) both collapse to "not yet resolved" -> `NeedInputs`, uniformly, same as the lease-miss case elsewhere — no separate `Terminal`/`AttemptFailure` arm needed for a peek, since a peek never triggers the cold path that could hit `PreparationFailure` at all. **BUT** obtaining the `PreparedTypeDeclCache`/`PreparedDeclBundle` instance for a `canonical_id` in the first place (`prepared_decl_bundle_with_context`, `prepared_decl.rs:574`) drives `ensure_indexed_ready_serve` internally (confirmed this round, `prepared_decl.rs:~589`) — the SAME dependency Part E already deferred (transitively reaches `normalized_analysis_canonical`'s companion-resolution tail, tied to phase 7). **This whole row (and `prepared_type_decl`/`prepared_type_decl_return_only`/`prepared_value_decl_return_only` below) is blocked on the SAME Part E dependency, not a separate concern** — the per-symbol slot-peek design is ready to implement the MOMENT a `peek_shallow_file_state`/`peek_indexed_ready` exists to obtain the bundle handle without blocking. `PreparedOwnerScope`'s own fields (**CONFIRMED CLEAN, round 5**: `ImportBinding{canonical_id: String, exported_name: String}` and `TypeParamBinding{name: Arc<str>, ordinal: u16}`, `prepared_decl.rs:140-192` — both plain data). |
| `prepared_type_decl` | `Result<Option<Arc<PreparedTypeDecl>>, PreparationFailure>` | **`PreparedTypeDecl` itself CONFIRMED clean this round**: `verter_semantic::analysis::type_solver::prepared::PreparedTypeDecl` (`prepared.rs:121`) — already `verter_semantic`-owned, not a session type at all. But the ACCESSOR (this `ResolverContext` method) is backed by `PreparedTypeDeclCache` (session-owned, `prepared_decl.rs:851`), which embeds `state: Arc<ShallowFileState>`/`interner: Arc<IdentityInterner>` — see the corrected `prepared_decl_bundle` row above; the entanglement is in the LAZY-BUILD CACHE, not the produced value. | Same peek-over-already-built-slots shape as `prepared_decl_bundle` above, not yet designed. The `Result<_, PreparationFailure>` shape still needs mapping onto `AttemptOutcome` (`PreparationFailure` probably becomes an `AttemptFailure` variant or folds into `Terminal`) once the peek exists. |
| `prepared_type_decl_return_only` | `Option<Arc<PreparedTypeDecl>>` | Same as above | This is a LOSSY adapter over `prepared_type_decl` (drops `PreparationFailure` to `None` with a `tracing::error!` + non-cacheable-fan-out taint). Per the `AttemptOutcome` contract, lossy-to-`None` is probably wrong — prefer keeping `prepared_type_decl`'s full `AttemptOutcome<Arc<PreparedTypeDecl>>` (mapping `PreparationFailure` to `Terminal`) and deleting the `_return_only` adapter's kernel-side use, OR keep an equivalent adapter that maps `Terminal` to `Complete(None)` explicitly at the ONE call site that needs it. Design not started. |
| `prepared_value_decl_return_only` | `Option<Arc<PreparedValueDecl>>` | Mirror of `prepared_type_decl_return_only` for values | Same treatment. |
| `project_type_store` | `&Arc<ProjectTypeStore>` | Yes — the WHOLE session cache graph (F7's finding). `project_semantic_dispatch` only calls 5 sub-accessors on it: `.semantic_graph()`, `.indexed()`, `.shape_cache_db()`, `.flow_slice()`, `.project_generation()` (confirmed this round via grep) | **Do NOT expose `Arc<ProjectTypeStore>` through `ResolverObservation`** (F9, explicit). `.semantic_graph()` -> `SemanticGraphStore` relocates per F9, becomes a DIRECT FIELD/HANDLE, not an observation method. `.project_generation()` -> **DONE** (`ResolverObservation::project_generation`, round 4). The other three are **F11-dispositioned** (round 4, per-store, NOT one ruling): `.indexed()` (`FileArtifactStore`) STAYS — **DONE, round 6**: `ResolverObservation::module_augmentation_index(&AugmentationTargetKey) -> AttemptOutcome<ModuleAugmentationIndexObservation>` landed, backed by the session's already-existing non-blocking `FileArtifactStore::get_augmenter_set` peek (population/write stays session-only, unchanged); `.shape_cache_db()` (`ShapeCacheDb`) STAYS — only its cold-compute ALGORITHM relocates with the dispatcher, kernel gets a narrow future `cached_synthetic_binding_shape()` peek + emits an admission candidate as attempt output. **TRACED further, round 8**: confirmed NOT a quick win even now that `AttemptOutput` exists (F16) — `ShapeCacheDb::peek(key, ctx)` itself takes `ctx: &dyn ResolverContext` for LIVE fact-signature validation (`single_entry_peek`, not a bare `DashMap::get`) and touches `request_context::current_request_context()` TLS counters, so it cannot be exposed directly. Worse, `ShapeCacheKey`'s value type `MaterializedOutputTypeExpr::from_parts(node, sealed, dep_signature, ..)` takes a live `SemanticNodeId`-shaped graph-instance handle plus a SEALED capability token — this is part of the session-owned "hot materialize"/`OutputProjector` sealed system CLAUDE.md's Component-Meta-Shallow-By-Default section guards (`a hot consumer must never take HotTypeRef -> TypeExpr -> semantic decision`), not a portable value type at all. `ShapeCacheKey`'s own transitive closure (`ShapeSubject`/`MemberShapeNodeSubject`/`SyntheticBindingId`/`ShapeDemand`/`PathSegment`/`ProjectionReductionContext`/`KeyFilter`/`PublishedSurfaceKind`) is NOT yet individually audited either. This needs its own dedicated investigation pass (F14-scale, not a quick DTO narrowing) — correctly stays deferred, F11's original disposition stands unchanged; `.flow_slice()` (`FlowSliceStores`) RELOCATES, scope CORRECTED by F14 (round 6): F11's "F9-shaped" language was wrong — `FlowSliceStores`'s query nodes (`FunctionFlowGraphStore`, `FlowSliceHashNode`) are built on the SHARED `cache_runtime::{ArtifactNode, CacheEntry, InflightTable}` framework (unlike `SemanticGraphStore`, which has its own bespoke primitives and touches `cache_runtime` only via a bare re-export) — that framework does NOT relocate (confirmed durable session-side: `ShapeCacheDb` is also a direct client; `RouteDb` isn't and stays session-side too, proving implementation substrate isn't the ownership criterion). Relocates: the plain value types, `build_function_flow_graph`, `FunctionFlowGraphStore`, the hash/lowered-body memo state, `FlowSliceBudget`, canonical eviction, plus a NEW bespoke dependency-neutral memo/dedup mechanism replacing `ArtifactNode`/`CacheEntry`/`InflightTable` (preserving cold-build-once, cooperative wait, panic safety, hash-before-lower ordering, content-key validity, non-admission of `BudgetExceeded`/`ReturnOnly`). Stays: `RetainedSnapshotSkeletonSource`, `DeclBodyMemo`, `ensure_indexed_ready_serve`, the generic `cache_runtime` framework itself. Full corrected scope: `docs/arch/refactor/rev11/evidence/C1/f14-deviation-consult.md`. **The bespoke replacement for `FunctionFlowGraphStore`'s underlying compute (the `ArtifactNode`/`CacheEntry`/`InflightTable` replacement for `FlowSliceHashNode`/the lowered-body memo) is NOT YET implemented** — genuinely new-implementation work, deferred to its own pass. **BUT the `function_body_skeleton()` OBSERVATION METHOD ITSELF IS DONE, round 7**: `FunctionFlowGraphStore` (the per-function GRAPH store, `FunctionFlowGraphStore::get_or_build`) turned out to be ALREADY framework-free — a plain `DashMap<FlowSliceFunctionKey, Arc<FlowGraphBundle>>`, distinct from `FlowSliceHashNode` (the slice-IDENTITY node, which DOES use the framework). Added a non-blocking `FunctionFlowGraphStore::peek`/`FlowSliceStores::peek_skeleton_for` pair (plain `DashMap::get`, NEVER drives `RetainedSnapshotSkeletonSource`'s blocking cold build — confirmed `DeclBodyMemo::run_leased`/`ensure_lease` ARE blocking via `mpsc::sync_channel` worker-thread rendezvous, so a genuinely non-blocking peek could NOT be built directly off `DeclBodyMemo` as F14's consult text loosely suggested; it had to be built off `FunctionFlowGraphStore`'s own memo instead). `ResolverObservation::function_body_skeleton(&FlowFunctionObservationKey) -> AttemptOutcome<Option<Arc<FunctionBodySkeleton>>>` landed, backed by that peek pair. New dependency-neutral `FlowFunctionObservationKey` (narrows `FlowSliceFunctionKey`, omitting the session-private `build_toolchain_fingerprint` — same treatment as `FileArtifactKey`'s eventual narrow mirror). `FunctionDeclarationRef`/`FunctionProgramKey` gained `PartialOrd`/`Ord` (additive, for `InputKey`'s `LoadSet` sort/dedup contract). |
| `record_ambient_dependency` | `()` (side-effecting) | No (takes two `&str`) | **CONFIRMED, round 5**: exactly ONE production call site (`apparent_type.rs:116`). Default body: `self.workspace().record_ambient_dependency(consumer_canonical, virtual_id)` — a genuine WORKSPACE MUTATION (not just a TLS write like `observe_borrowed_signature` above), recording a dependency edge for invalidation tracking. Even more clearly attempt-OUTPUT-shaped than `observe_borrowed_signature`: the kernel returns the `(consumer_canonical, virtual_id)` pair(s) it discovered, and the session driver applies them to the workspace after `Complete` — the kernel must never call `workspace()` directly under any circumstance. **`AttemptOutput::record_ambient_dependency`/`AmbientDependency` LANDED (round 8, F16)** as this method's eventual output slot — the method itself is still not wired (needs the relocated kernel call sites). |
| `resolve_imported_type_root_with_facts` | `(Option<ResolvedRootIdentity>, Arc<[FactVersionRef]>)` | `ResolvedRootIdentity` already `verter_semantic`-owned (confirmed round 1: `use verter_semantic::analysis::type_solver::host::ResolvedRootIdentity`); `FactVersionRef` is `verter_workspace`-owned (confirmed this round, likely clean) | Per F8: "resolve_* operations -> move the resolution ALGORITHM into the kernel; expose the route/probe FACTS it needs, not a semantic operation implemented by the environment." This is NOT a 1:1 observation method — it represents session-side resolution LOGIC (barrel/re-export chain walking) that needs to be PORTED into the relocated kernel, backed by narrower `ResolverObservation` fact-lookups (e.g. "what does this barrel file re-export"). Design not started — likely the single most involved item in this table besides the framework-surface split (F10) and `SemanticGraphStore` dependency-neutralization (F9). |
| `resolve_type_dependency_canonical` | `Option<String>` | No (plain string) | **TRACED this round**: the default body calls `crate::VerterHost::resolve_type_dependency_canonical(self, owner_canonical, import_source)`, returning `verter_workspace::ResolutionPublication::{Admitted, Refused}` — this is `verter_workspace`'s RESOLUTION-CURRENCY/PUBLICATION machinery (the same `TrackedResolutionCapability`/publication system F4 already targets for the `ProjectResolver` -> `ModuleResolverCore` conversion, phase 7). **Do not design this method's `AttemptOutcome` shape in isolation** — it should reuse whatever `LoadSet`/`AttemptOutcome` vocabulary phase 7's `ModuleResolverCore` conversion produces for the SAME underlying resolution system, or genuinely duplicate effort. Sequence this method (and likely `resolve_imported_type_root_with_facts`/`resolve_value_export_target`, both suspected to share the same publication machinery — not yet independently confirmed) alongside or after phase 7, not ahead of it. |
| `resolve_value_export_target` | `Option<ValueDeclIdentity>` | `ValueDeclIdentity` confirmed clean this round (`runtime_values.rs:7-15`: `String`, `TopLevelOwnerId`, `String` — no host handles) | Same "port the algorithm in" bucket. `ValueDeclIdentity` itself is a safe return type; the RESOLUTION LOGIC behind it needs porting, not just re-typing the accessor. |
| `shallow_file_state` | `Option<Arc<ShallowFileState>>` | Yes — `ShallowFileState.decl_bodies: Arc<DeclBodyMemo>` (confirmed this round, `shallow_file_state.rs:78`) | Same split as `IndexedReady`/`ensure_indexed_ready_serve`: the non-`decl_bodies` fields (exports, imports, wildcard_reexports, import_targets, route_inventory, whole_hash) are plain and can cross; `decl_bodies` needs a narrow per-declaration demand instead of a whole-memo handoff. |
| `workspace_is_package_backed` | `bool` | No | **DONE** (this row's text was stale — the method landed in the original 8, `fn workspace_is_package_backed(&self, canonical: &str) -> AttemptOutcome<bool>`, unconditionally `Complete`-shaped per canonical; see `observation.rs`). Corrected round 7. |

## Part B — `host_for_fact_tracer_install`'s 28 call sites, classified by pattern

Read all 28 sites this round (10 files: `mod.rs` ×9, `carrier.rs` ×2,
`apparent_type.rs` ×2, `call_resolve.rs` ×1, `build.rs` ×5, `cycle_gate.rs`
×1, `flow_return.rs` ×2, `locator_shape.rs` ×1, `relation.rs` ×4,
`semantic_source.rs` ×1). They reduce to a small number of DISTINCT
downstream calls:

| Downstream call (what `host.<call>` actually does) | Site count (approx) | Files | Disposition |
|---|---|---|---|
| `host_view_env_hashes_for(canonical)` / `host_view_env_hashes()` (no canonical — project-default) | ~13 | `mod.rs`, `apparent_type.rs`, `flow_return.rs`, `cycle_gate.rs`, `locator_shape.rs`, `build.rs`, `relation.rs` | The single most common pattern. **`EnvHashes` CONFIRMED CLEAN this round** (`session_view.rs:62-68`: `#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)] pub struct EnvHashes { parse_env_hash, resolve_env_hash, type_env_hash, lib_env_hash: Hash16 }` — 4 plain `Hash16` fields, no host/session handles). **DONE**: landed as ONE narrow observation, `fn env_hashes(&self, canonical: Option<&str>) -> AttemptOutcome<EnvHashes>` — the per-canonical/project-default split didn't diverge enough to warrant two methods. |
| `host_view_project_identity_for(canonical)` (often `.fold_u32()`) / `host_view_project_identity()` | ~3-4 | `mod.rs`, `apparent_type.rs` (indirectly) | **DONE**: landed as `fn project_identity(&self, canonical: Option<&str>) -> AttemptOutcome<StoreViewProjectIdentity>` — the full identity, not just a `.fold_u32()`-style fold. **Self-correction, round 7**: I initially wrote (wrongly, without checking) that callers could get a `.fold_u32()` fold off `StoreViewProjectIdentity` itself — verified this round that type has NO `fold_u32` method; `fold_u32` belongs to the UNRELATED `ProjectIdentity` newtype (round 6's augmentation-key relocation, `augmentation_key.rs`) — do not conflate the two `*ProjectIdentity` types. Whoever wires this method against a `.fold_u32()`-consuming call site needs to check that call site's actual need, not assume the fold travels with `project_identity()`. |
| `.test_force.load(Ordering::Relaxed)` | 2 | `mod.rs`, `carrier.rs` | Test-only knob. Becomes an explicit test-only attempt input (a field on a test-only attempt-view constructor, `#[cfg(test)]`-gated), never a production observation method. |
| `.observe_owner_import_route_witness(canonical_id)` | 2 | `carrier.rs`, `build.rs` | Fact-tracer observation. Per F8: "semantic-owned tracing or a returned sidecar; no host accessor." Same OUTPUT-disposition bucket as `observe_borrowed_signature`/`record_ambient_dependency` above — becomes part of the attempt's recorded output, not an inbound call. |
| `host.provenance` (direct field read, `Arc::clone`) | 1 | `mod.rs` | **CONFIRMED**: `host.provenance: Arc<MetaProvenance>` (`lib.rs:516`) — the SAME `MetaProvenance` type F9's sweep flagged in `semantic_query_memo`. `MetaProvenance` (`types.rs:3743`) is a large bag of plain `AtomicU64` PROCESS-WIDE DIAGNOSTIC COUNTERS (`get_component_meta_calls`, `host_upsert_calls`, `resolver_node_cache_hits/misses`, etc.) — pure observability/telemetry, not semantic resolution data. The mod.rs:2717 site clones it into a closure purely to bump a cold/warm counter after a build runs (confirmed by reading the surrounding code — a `cold_build_ran` `AtomicBool` tracked for the "per-kind cold/warm counters" comment right above it). **Disposition: extract entirely from the kernel.** The session driver observes whether an attempt was a cold build (from the `AttemptOutcome`/attempt-scope bookkeeping it already owns) and bumps `MetaProvenance` counters itself, outside the relocated kernel. Zero `ResolverObservation` method needed for this. Same disposition likely applies to `semantic_query_memo`'s `MetaProvenance` touch points — worth confirming when that subsystem's own audit happens, but not blocking here. |
| `host.workspace().reverse_deps_for(canonical)` | 1 | `build.rs` | **TRACED, round 6**: the ONE call site is `stitch_module_augmentations`'s (`build.rs:3276`) "program completeness for relative augmentation" candidate-discovery step (`build.rs:3286-3297`) — a relative `declare module "./base"` augmenter lives in a file that DEPENDS ON `decl_canonical`, so the base's reverse-dependency set IS the candidate-augmenter set; every candidate must be indexed BEFORE the augmentation-index scan or a sibling augmenter pulled in only via a side-effect import is silently dropped. This is genuine QUERY-TIME CORRECTNESS (the augmentation-index scan's completeness depends on it), not an optional warm-up the kernel could skip — settles the "not yet decided" question. Bounded (one canonical's reverse-dep set, not workspace-wide). Eventual shape: a bounded `workspace_reverse_deps(canonical) -> AttemptOutcome<Arc<[CanonicalId]>>` observation; the KERNEL only DEMANDS the set, the SESSION DRIVER performs the actual `ensure_indexed_ready_serve` warm-load loop on a miss (same kernel-demands/driver-loads split as everywhere else). **NOT implementable yet**: this call lives inside `stitch_module_augmentations`/`collect_augmentation_contributions`, which are themselves not-yet-relocated `project_semantic_dispatch` functions (per F8's phase order, trait methods land ahead of the functions that would consume them only when the DEMAND is freestanding — this one is embedded inside a function that hasn't moved). Record disposition now, implement when `stitch_module_augmentations` itself relocates. |
| `host.workspace().known_canonicals()` | 1 | `build.rs` | **TRACED, round 6**: the ONE call site is the SIBLING "program completeness for EXTERNAL augmentation" discovery step (`build.rs:3824-3839`, inside the same augmentation-stitch family, the `ExternalSpecifier` branch) — an ambient `declare module "<bare>"` declarer may be a program-root `.d.ts` NOTHING imports (reachable only via program membership: tsconfig `types`/`include`, not the import graph), so unlike the relative case there is no base-file anchor to take a bounded reverse-dep set from; the ENTIRE known-canonicals set must be indexed first. Confirmed genuinely workspace-wide and confirmed genuine CORRECTNESS (not an optimization) for the SAME reason as `reverse_deps_for` — settles the "not yet decided" question the same way, but the SCOPE stays the "biggest scope concern" it already was. Large-but-finite (a workspace's `known_canonicals()` is bounded at any point in time, just potentially large) — the contract's existing `InputResolutionByteLimit`/`InputResolutionUniqueKeyLimit`/`InputResolutionAttemptLimit` `AttemptFailure` variants (already defined, `attempt_outcome.rs`) are the EXISTING vocabulary for "bounded but large," so this does not need new contract vocabulary in principle: a `InputKey::WorkspaceMembership`-shaped single key returning the current known-canonicals snapshot, consumed as its own `NeedInputs` round (the contract's iterative delta-retry loop, §4, already supports a multi-round demand sequence natively) is a plausible shape — NOT a proposal to implement, this paragraph is scoping only. Same "not implementable yet, lives inside a not-yet-relocated function" blocker as `reverse_deps_for`. |
| `host.resolve_project_for_canonical(canonical)` + `host.workspace().project_stable_key(project)` | 2 | `apparent_type.rs`, `call_resolve.rs` | "Workspace enumeration/reverse dependencies -> Captured finite snapshot or `NeedInputs`" bucket. Becomes `fn project_stable_key(&self, canonical: &str) -> AttemptOutcome<ProjectStableKey>` (or similar) — a narrow per-canonical project-ownership lookup, not the two-step host+workspace call chain. |
| `host.relation_knobs.{strict_family_relax_bits, force_overflow_observations, force_budget_exhaustion}` | 3 | `relation.rs` | **CONFIRMED test-only**: `host_construction::RelationHostKnobs` (`host_construction.rs:47-51`) is a tiny 3-field struct of `AtomicUsize`/`AtomicBool`/`AtomicU8` fault-injection knobs (`force_overflow_observations`, `force_budget_exhaustion` are unambiguous fault-injection test names; `strict_family_relax_bits` grouped with them, treat as the same bucket absent contrary evidence). Same disposition as `.test_force` above: explicit `#[cfg(test)]`-gated attempt-view test inputs, never production observation methods. |
| `resolve_vue_macro_surface_with_ctx(self.ctx, ...)` | 1 | `semantic_source.rs` | **Disposition: F10.** Extract the raw macro-surface query algorithm into `verter_semantic`; this call site's function (`replay_vue_macro_type_argument_surface`) calls the relocated operation directly instead of escaping through the host. |

Row counts above are approximate reconstructions from this round's greps,
not a rigorously double-counted total — re-verify the exact 28-site
breakdown against `grep -c` before treating any count as final.

## Part C — confirmed-clean vs confirmed-entangled types, running list

**Confirmed clean (plain data, no host/session/scheduler handles) —
verified by direct read, not assumption:**
- `StoreViewProjectIdentity`, `StoreViewOverlayIdentity`,
  `StoreViewValidationToken` (relocated in phase 3, already in
  `verter_semantic`).
- `AmbientSymbolHit` (`verter_workspace`, but see the edge-reversal flag in
  Part A).
- `ValueDeclIdentity`.
- `PreparedTypeDecl`/`PreparedValueDecl` themselves (already
  `verter_semantic::analysis::type_solver::prepared`-owned — confirmed
  round 4; NOT the same as `PreparedDeclBundle`, see entangled list below).
- `Hash16` (identical alias already exists in both crates).
- `EnvHashes` (`session_view.rs:62-68` — 4 plain `Hash16` fields).
- `FactVersionRef` (actual definition: `verter_workspace::fact_cache.rs:905`,
  NOT `fact_read_set.rs` which only re-imports it — corrects my own round-1
  assumption it was session-owned). **CONFIRMED CLEAN, round 7**: closed
  enum, `#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]`,
  every variant plain data (`String`/`FactHash16`/nested fact-ref structs/
  `ParseEnvHash`/`verter_language::{ParseKey,FileLanguage}`/`u64`/
  `StrictSelfRootWorld { authority_id, authority_generation, source_epoch,
  artifact_epoch, population: ViewPopulation }`) — no host/session handle
  anywhere. Ready to cross as-is once Part F's attempt-output bundle
  design unblocks (itself blocked on the not-yet-relocated top-level
  kernel entry point, see Part F).

**Confirmed entangled (need a narrower DTO or an on-demand peek, not a
wholesale handoff) — verified by direct read:**
- `HostStoreView` (F7 — stays behind entirely).
- `HostConfig` (only `.depth_budget` needed by `project_semantic_dispatch`
  — **CONFIRMED, round 5**: zero `.config()` call sites anywhere in
  `semantic_query.rs`/`semantic_query/*.rs`/`semantic_query_memo/*.rs`
  (grep across all three, test files included in the sweep, found
  nothing) — `depth_budget` is confirmed the only field the G-scope SCC
  needs; `HostConfig` itself stays entangled as a type, but the field
  surface doesn't widen).
- `FileArtifactKey` (`build_toolchain_fingerprint` is session-private).
- `ShallowFileState` (`decl_bodies: Arc<DeclBodyMemo>`).
- `IndexedReady` (embeds `ShallowFileState` transitively, same problem).
- `MaterializeScopeObservation` (embeds `Arc<IndexedReady>`).
- `IndexedReadyServe` (wraps `Arc<IndexedReady>` as a session
  publication/fencing carrier — per the consult, not independently
  re-verified by me this round).
- `PreparedTypeDeclCache`/`PreparedValueDeclCache` (`prepared_decl.rs:851`,
  `:1099` — embed `Arc<ShallowFileState>` + `Arc<IdentityInterner>`;
  confirmed round 4, corrects a round-1 false-clean claim), and by
  extension `PreparedDeclBundle` (which embeds both caches).
- `FileArtifactStore`/`ShapeCacheDb` (F11 — stay in `verter_session`
  entirely; only narrow future observation methods over them, not the
  stores themselves).
- `ProjectTypeStore` (F7 — the whole session cache graph; only narrow
  sub-accessors are usable).
- `SemanticGraphStore` (F9 — relocates, but not as an observation method;
  becomes engine state; itself needs dependency-neutralizing first).
- The framework-surface macro-query family (F10) —
  `resolve_vue_macro_surface_with_ctx`/`vue_macro_dtos_with_ctx` and
  friends.

**Resolved since round 3** (moved out of this list): `active_session_view`
(traced, folds into F11, round 5); `PreparedTypeDecl`/`PreparedValueDecl`
(confirmed clean, round 5) / `PreparedTypeDeclCache`/`PreparedValueDeclCache`
(confirmed entangled, round 5) / `PreparedOwnerScope`/`ImportBinding`/
`TypeParamBinding`/`ImportCanonicalization` (confirmed clean, round 5, see
Part A); `resolve_type_declaration_for_dep`
(confirmed FALSE POSITIVE — zero production call sites in
`project_semantic_dispatch`, round 5, safe to drop entirely, not part of
the 23-method surface); `is_cancelled`/`is_request_bound` (confirmed,
round 5, neither becomes a trait method).

**Resolved this round (round 5, second pass)**: `semantic_query.rs`/
`semantic_query/*`/`semantic_query_memo/*`'s per-site entanglement — see
Part G, CLOSED with a concrete blocker list (not open-ended anymore).
`typeinfo/framework_surface`'s exact split boundary beyond the two
originally-named entry points — see Part H (F13), CLOSED with a corrected/
expanded operation-family scope (ownership ruling only, not implemented).

**Still not independently audited (genuinely open)**:
- The exact resolution ALGORITHMS behind `resolve_imported_type_root_with_facts`/
  `resolve_type_dependency_canonical`/`resolve_value_export_target` (I
  have their SIGNATURES, not their bodies — the actual barrel/re-export
  walking logic needs reading before it can be ported into the kernel).
  Per F12, these are now confirmed BLOCKED on the phase-4/7 cutover
  anyway (same `verter_workspace` resolution engine) — no longer urgent
  to read the bodies until that cutover starts.
**Resolved this round (round 7)**: both of F13's residual gaps.
`normalize.rs:96`/`normalize_slots.rs:175` (`ctx.ensure_indexed_ready_
serve(macro_surface.owner_canonical...)`, reading `indexed.snapshot.
macros` — analyzer macro facts, not JSDoc text) are the SAME
`ensure_indexed_ready_serve` call shape already known to be gated on
F12's `normalized_analysis_canonical` fast-path pre-check — mechanical
application of F12, not a new finding. `structural_carrier_producer`'s
actual caller count: confirmed 13+ distinct production call sites of
`macro_type_arg_hot_ref` across ~10 files (`template_class_facts.rs`,
`component_meta_registry.rs`, `component_meta_query_engine/
shallow_preserve.rs` ×2, `meta_resolve/slot_binding_graph.rs`,
`meta_resolve/macro_member_walk.rs`, `meta_resolve/projectors/mod.rs`
×2, `host_manage/eval_env.rs`, `host_manage/component_meta_methods/
macro_output_expansion.rs`, `project_semantic_dispatch/semantic_
source.rs` ×3, `project_semantic_dispatch/broad_runtime.rs`,
`typeinfo/framework_surface/vue_exec/mod.rs`) — far more than the stale
"four" doc comment, confirming F13's own flag. Does NOT change F13's
disposition (only the inner `lower_type_expr_structural` lowerer
relocates; the outer `macro_type_arg_hot_ref` stays session-side, `ctx.
ensure_indexed_ready_serve`-gated) — just means more call sites convert
to thin-caller shape at implementation time, already anticipated
generically.

## Part D — the `ShallowFileState`/`DeclBodyMemo` split, traced

Round-3 continuation: this is the design work "next step #1" of the
round-3 report named. Traced in round 4; **the decl-body-demand half
(`type_decl`/`value_decl`) IMPLEMENTED that same round** (see the
"`AttemptOutcome` mapping for decl-body demand — RESOLVED this round"
section below) — the non-decl-body half is what Part E picks up.

### `LoweredTypeDecl`/`LoweredValueDecl` are ALREADY dependency-neutral in content

`ShallowFileState.decl_bodies: Arc<DeclBodyMemo>` was flagged entangled
because `DeclBodyMemo` is session-owned lazy-lowering machinery. But its
PRODUCTS — `LoweredTypeDecl`/`LoweredValueDecl` (`decl_body_memo.rs:89`,
`:235`) — are a DIFFERENT question, and this round traced their field
types individually:

- `LoweredTypeDecl` derives `verter_no_typeexpr::NoTypeExpr` (compile-witnessed
  no-stored-`TypeExpr`) and every field is a content-free FACT type:
  `TypeDeclKind`/`TypeDeclBody` (`verter_semantic::analysis::type_eval`,
  ALREADY verter_semantic-owned), `NarrowTypeParam`/`VueIgnoredHeritageFact`/
  `PreparedMemberFact`/`PreparedWrapperShapeFact`/`PreparedProjectionClassFact`/
  `HeritageBaseFact`/`ShallowRouteFacts` (ALL `verter_type_expr::facts`,
  which `verter_semantic` already depends on), `HashOutcome`,
  `FxHashSet<TypeDependencyPathFact>`, `Vec<String>`, `Arc<[...]>`,
  `FxHashMap<FactPropertyKey, PreparedMemberFact>` — no host/session
  handle anywhere.
- `LoweredValueDecl`'s fields (`ValueTypeAnnotationFact`,
  `Vec<FunctionSignature>`, `Option<ObjectShapeFact>`,
  `Option<EnumMemberFact>`, `Option<EnumMemberNamesFact>`) are the same
  shape — content-free facts, not independently re-verified field-by-field
  this round but consistent with `LoweredTypeDecl`'s pattern and CLAUDE.md's
  own description ("`LoweredValueDecl` are fact+locator `NoTypeExpr`").

**This means `LoweredTypeDecl`/`LoweredValueDecl` themselves can relocate
to `verter_semantic` largely as-is** (same "physically misplaced, content
already clean" shape as `EnvHashes`/`StoreViewValidationToken` before their
relocations) — NOT a redesign-a-new-DTO task as I'd assumed in round 2's
write-up. The entangled part is narrower than I'd thought: only the
`DeclBodyMemo`/`DeclLoweringService` MACHINERY that produces them stays
behind.

### The actual blocking mechanism, located

`ShallowFileState::type_decl(name)` -> `DeclBodyMemo::type_decl_outcome_in`
(`decl_body_memo.rs:826`) calls `demand_and_commit`, whose lease step
(`SnapshotLease` via `DeclLoweringService::acquire_lease`,
`decl_lowering.rs:576`) does a GENUINE cross-thread blocking rendezvous:
`workers[shard_index].send(job)` then `result_rx.recv()` on a
`std::sync::mpsc::sync_channel(1)` — the calling thread blocks until a
worker-shard thread answers. This is exactly the class of operation
C1-AC-5 forbids inside the pure kernel, now traced to its exact call site
rather than assumed.

### A lock-free peek is already structurally available, just not exposed

`type_entries: DashMap<DeclBindingKey, TypeCell>` where `TypeCell =
Arc<OnceLock<DemandCell<LoweredTypeDecl>>>` (`decl_body_memo.rs:309`,
`:521`). `OnceLock::get()` is non-blocking by construction — it returns
`None` immediately if the cell hasn't been populated yet, `Some(&T)` if it
has, with NO thread-parking either way. This means a genuine
`ResolverObservation`-shaped peek is possible WITHOUT new concurrency
machinery:

```text
fn peek_type_decl(owner, name) -> Option<Arc<LoweredTypeDecl>> {
    type_entries.get(&DeclBindingKey::new(owner, name))?  // DashMap read
        .get()?                                            // OnceLock::get(), non-blocking
        .ready_value()                                     // DemandCell's own ready-state accessor — NOT YET READ
}
```

`DemandCell<D>`'s own shape (does it expose a ready-state read distinct
from the lease/poison machinery `demand_and_commit` also touches?) is the
ONE remaining piece not yet read this round — needed before writing the
real peek method, since `DemandCell` might itself gate on more than a
plain `OnceLock` would suggest (poison/lease-miss states per
`DemandOutcome`'s doc comment above).

### `AttemptOutcome` mapping for decl-body demand — RESOLVED this round

Read `DemandCell<D>`'s definition (`decl_body_memo.rs:304`) to close the
ambiguity the first pass of this section flagged:

```rust
enum DemandCell<D> {
    Ready(Option<Arc<D>>),
    LeaseMiss,
}
```

Its own doc comment settles the question directly: **"A `LeaseMiss` cell
is EVICTED from its owning map... a `Ready(None)` is a genuine, cacheable
absence retained warm."** A `LeaseMiss` never persists in `type_entries` —
the committing thread removes it immediately after every waiter observes
it. So the externally-observable states a peek can find collapse to
exactly three, not four:

| Peek observes | Meaning | `AttemptOutcome` mapping |
|---|---|---|
| Key absent from `type_entries` (never entered, OR entered and evicted after a `LeaseMiss`) | No committed answer available from already-materialized state | `NeedInputs(...)` — the driver triggers the blocking `acquire_lease` path and retries. "Never demanded" and "was demanded, raced, evicted" are — BY THE CELL'S OWN EVICTION INVARIANT — indistinguishable from outside, and that's correct: both cases mean "go trigger it (again)," never a stable fact. |
| Key present, `OnceLock` unpopulated (a demand is in-flight on another thread) | Same as above — the calling thread must not wait for it | `NeedInputs(...)` — same disposition; a peek never blocks on someone else's in-flight `OnceLock::get_or_init`. |
| Key present, `OnceLock` populated with `DemandCell::Ready(Some(decl))` | Genuine hit | `Complete(Some(decl))` |
| Key present, `OnceLock` populated with `DemandCell::Ready(None)` | Genuine, CACHEABLE absence (not inventoried, or fatal-parse-empty) — per the cell's own doc comment, retained warm, never evicted | `Complete(None)` — a stable fact, same pattern as the already-implemented `whole_hash` method |
| Key present, `OnceLock` populated with `DemandCell::LeaseMiss` (a narrow race window before the evicting thread removes the entry) | Transient — the SAME "go trigger/retry" case as absent | `NeedInputs(...)` — never surfaced as a distinct case; folds into the same arm as "absent." |

**Design is now complete enough to implement** (still not implemented this
round — `type_entries`/`aug_type_entries`/the value-side mirror are all
`pub(crate)`-private fields on `DeclBodyMemo`, so the peek method itself
must be added inside `verter_session::decl_body_memo` before anything in
`verter_semantic` can call it; `ResolverObservation`'s method would then
be `fn type_decl(&self, canonical, owner, name) -> AttemptOutcome<Option<Arc<LoweredTypeDecl>>>`,
mirroring `whole_hash`'s already-implemented `Complete(None)`-is-a-fact
shape exactly). Next concrete step: relocate `LoweredTypeDecl`/
`LoweredValueDecl` to `verter_semantic` (per the "already dependency-neutral"
finding above), THEN add the peek method to `DeclBodyMemo` in
`verter_session`, THEN wire the `ResolverObservation` method.

**Update, round 4**: done — see `LoweredTypeDecl`/`LoweredValueDecl`
relocation (`559368207`), `DeclBodyMemo::peek_type_decl`/`peek_value_decl`
(`f49d364aa`), and `ResolverObservation::type_decl`/`value_decl`
(`f8d0afe73`).

## Part E — `shallow_file_state`'s non-decl-body accessors, traced (round 4)

Round 5's next-step #1 continuation: traced what `project_semantic_dispatch`'s
13 non-test `.shallow_file_state(canonical)` call sites actually do with
the result, rather than assuming the whole struct needs a DTO.

### Finding: `.whole_hash` dominates, and is ALREADY covered

Grepped every call site's immediate usage. The overwhelming majority (8+
of 13) do exactly `.shallow_file_state(canonical).map(|s| s.whole_hash)` —
identical to what the ALREADY-IMPLEMENTED `ResolverObservation::whole_hash`
method (round 3) returns. These sites don't need a new method at all once
the kernel is retyped onto `ResolverObservation` — they already have their
answer.

The remaining sites call five `ShallowFileState` methods:
`export_target(name) -> Option<&ExportTarget>`,
`export_assignment_target() -> Option<&str>`,
`import_target_in(owner, name) -> Option<&ImportTarget>` (`pub(crate)`),
`has_type_symbol_in(owner, name) -> bool` (`pub(crate)`),
`visible_value_binding(owner, name) -> Option<LexicalValueBinding<'_>>`
(`pub(crate)`, a BORROWED enum — needs its own owned-projection design,
not yet done). `ExportTarget`/`ImportTarget` are confirmed plain data
(defined in `shallow_file_state.rs` itself: `ExportTarget` —
`Local{owner,symbol_name}`/`Reexport{source_specifier,original_name,is_type}`;
`ImportTarget` — `{source_specifier,imported_name,is_namespace}` — no
host/session handles).

### Finding: the fast path IS peek-safe, EXCEPT for one entangled dependency

Traced `ShallowFileState`'s ONLY route into existence —
`ensure_indexed_ready_serve_uninstrumented`'s FAST PATH (before its
`materialize` closure, which does the genuinely blocking work via
`effective_file_state`'s scheduler-miss + `ensure_loaded` fallback,
confirmed by the "On scheduler miss, call ensure_loaded once" comment,
`prepared_decl.rs:~2003`):

```text
authoritative_current_artifact_key(canonical)   -- non-blocking (traced: calls
                                                    effective_file_state, which
                                                    is `scheduler.try_get_source(..)?`
                                                    -- a peek, `None` on miss,
                                                    NEVER calls ensure_loaded itself)
  -> project_type_store.indexed().get(...)      -- a cache read
  -> indexed_surface_is_current(canonical, indexed) -- pure comparison
                                                       (host_view_env_hashes_for
                                                       == indexed.parse_env_hash) --
                                                       host_view_env_hashes_for is
                                                       the SAME accessor `env_hashes`
                                                       already wraps
```

Every step here is non-blocking — UNTIL `normalized_analysis_canonical`,
which BOTH `authoritative_current_artifact_key` and the `get_any` fallback
path (`artifact_current_indexed_raw`) call first. Round 3 already found
`normalized_analysis_canonical`'s companion-resolution TAIL (for a
runtime-JS canonical preferring a `.d.ts` companion) calls
`resolve_for_persistent_state` — a method in `host_lifecycle.rs`, the SAME
file CLAUDE.md's spec names as home to the blocking cross-file
load-on-demand machinery (`ensure_loaded`/`wait_or_drive`). Its early-out
fast paths (empty id, raw import specifier, non-runtime-JS with an
explicit extension — i.e. `.ts`/`.d.ts`/framework carriers) ARE pure; only
a bare runtime `.js`/`.jsx` canonical hits the slow tail.

### Disposition: DEFER, not blocked, not a new deviation

This is the SAME phase-7 sequencing risk round 3 already flagged and
correctly declined to design around — `normalized_analysis_canonical`
shares `verter_workspace`'s resolution-currency/publication machinery with
`ProjectResolver` (F4/phase 7's target). Building `peek_shallow_file_state`
now would mean either (a) silently accepting it can spuriously fall
through to a blocking resolve for `.js`/`.jsx` canonicals — a genuine
C1-AC-5 violation for exactly the file class this whole exercise exists to
cover — or (b) inventing a second, throwaway "is normalization safe here"
classifier ahead of phase 7's real design. Neither is acceptable; this is
NOT a new deviation requiring a fresh consult (round 3 already
dispositioned the underlying cause), just a concrete confirmation that the
DEFER applies here too.

**Not blocking further phase-4 progress** — `whole_hash` already covers
the dominant use, and the other `ResolverObservation` methods (Part A/B)
don't transitively depend on `normalized_analysis_canonical`'s slow tail.
Revisit `export_target`/`import_target_in`/`has_type_symbol_in`/
`export_assignment_target`/`visible_value_binding` once phase 7 (or at
least its `normalized_analysis_canonical` piece) has a real
`AttemptOutcome` design to build on.

**Update, same round**: `prepared_decl_bundle`/`prepared_type_decl`/
`prepared_value_decl` (Part A) turn out to share this EXACT dependency —
`prepared_decl_bundle_with_context` drives `ensure_indexed_ready_serve`
internally to obtain the bundle in the first place. So this one deferral
now blocks TWO method groups, not one. The per-symbol
`PreparedTypeDeclCache`/`PreparedValueDeclCache` slot-peek design itself
is fully ready (see Part A's `prepared_decl_bundle` row) — only the
"get the bundle/shallow-state handle for a canonical without blocking"
piece is the shared blocker. Building `peek_shallow_file_state`/
`peek_indexed_ready` unblocks BOTH groups at once — worth prioritizing
once phase 7's `normalized_analysis_canonical` design exists.

## Part F — the "attempt output" bucket, consolidated (round 5), DESIGNED round 8 (F16)

Not a `ResolverObservation` design question at all — these are OUTPUTS the
relocated kernel produces alongside a `Complete` `AttemptOutcome`, applied
by the session driver AFTER the attempt succeeds, never inbound calls the
kernel makes. FOUR confirmed members (round 8 added the fourth, from F15):

1. **`observe_borrowed_signature`** (Part A) — every `&[FactVersionRef]`
   slice the kernel observed while answering, for the driver to write into
   `verter_session`'s TLS fact tracer.
2. **`record_ambient_dependency`** (Part A) — every `(consumer_canonical,
   virtual_id)` ambient-dependency edge discovered, for the driver to
   apply to the workspace's dependency graph.
3. **`cached_synthetic_binding_shape`'s admission candidate** (F11,
   `ShapeCacheDb`) — a `ShapeCacheAdmissionCandidate` the driver
   validates/adopts into `ShapeCacheDb` after a `Complete` attempt.
4. **F15's consumed-vs-prefetched observation tracking** — `path_probe`/
   `real_path`/`package_manifest` (round 8) support "staged
   priority-frontier batching" `NeedInputs` rounds that may prefetch
   several sibling observations speculatively; the eventual `Complete`
   attempt's recorded witness must report ONLY what it actually consumed,
   never every prefetch — this is now a genuine phase-7 CORRECTNESS
   prerequisite, not just a nice-to-have (F15's own explicit finding).

**DESIGNED and LANDED (inert), round 8, per F16's consult**
(`docs/arch/refactor/rev11/evidence/C1/f16-deviation-consult.md`):
`AttemptOutcome::Complete(T)` stays UNCHANGED (attaching outbound kernel
effects to every inbound `ResolverObservation` response would be wrong —
all 13 landed methods are inbound queries). The eventual top-level shape
reuses `AttemptOutcome`'s existing generic payload without touching the
enum: `CompletedAttempt<T> { value: T, output: AttemptOutput }`,
`type KernelAttempt<T> = AttemptOutcome<CompletedAttempt<T>>` — NOT built
yet, needs the not-yet-relocated top-level kernel entry point to decide
the final completion envelope. What IS landed: the bare `AttemptOutput`
accumulator itself (`crates/verter_semantic/src/resolver_core/
attempt_output.rs`) — private fields (`observed_facts: Vec<FactVersionRef>`,
`ambient_dependencies: Vec<AmbientDependency>`,
`consumed_resolution_observations: Vec<ConsumedResolutionObservationKey>`
— a NEW dedicated key (4 variants — `PathProbe`/`RealPath`/
`PackageManifest`/`RecoveryScope`, the last added round 9 per Part I's
finding — see below), deliberately NOT a bare `Vec<InputKey>`, whose
variants mix unrelated semantics), `Default`/`new()`/per-category
`record_*` methods/read accessors/`merge()`/`is_empty()`, no public struct
literal (every future field, e.g. `ShapeCacheAdmissionCandidate` once F11's
own DTO design lands, is additive). `is_cancelled`/`is_request_bound`
(Part A) are a DIFFERENT question (session-driver-only lifecycle concerns,
not kernel output) and are NOT folded into this bucket.

**LANDED, F19's implementation round** (`docs/arch/refactor/rev11/
evidence/C1/f19-deviation-consult.md`): `CompletedAttempt<T>`/
`KernelAttempt<T>` now built exactly as designed above
(`attempt_outcome.rs`), plus the `ResolverAttemptView` seam
(`resolver_attempt_view.rs`) and the `priority_frontier` combinator
(`priority_frontier.rs`) that consumes `KernelAttempt<Option<T>>` per
candidate. Still inert — no production driver threads real closures or
`&mut AttemptOutput` through a live kernel call chain yet.

**Still open**: `ShapeCacheAdmissionCandidate`'s own concrete DTO (F11,
separate ruling); actually threading a live driver's real observations
through `ResolverAttemptView`'s closures and wiring `priority_frontier`
into a ported resolve algorithm (needs the algorithm conversion itself,
F15's deferred scope, and item 6's dual-runner harness as the first
consumer).

## Part G — `semantic_query.rs`/`semantic_query/*`/`semantic_query_memo/*` per-site audit, CLOSED (round 5)

The item F9 left open ("`semantic_query_memo`'s own per-site
`ResolverContext`/TLS/host_manage/capture_token/MetaProvenance/cache_runtime
usage, 11+ files"). Split into two sub-surveys, both grep-plus-targeted-read
based (not a full line-by-line read of ~16k+12k lines) with file:line
citations for every finding below. Does NOT change F9's ownership verdict
(`SemanticGraphStore` still relocates as engine state, not as a
`ResolverObservation` method) — it closes out the "needs
dependency-neutralizing first" open item into a concrete, bounded list.

### G1 — `semantic_query.rs` (9628 lines) + `semantic_query/*.rs` (10 files,
6668 lines, `object_spread_projection_tests.rs` excluded as test-only)

Near-fully clean — this module is domain-vocabulary (query key tags, node
data enum, display projection, demand bitset, DTOs), not host-coupled
machinery. Exhaustive grep for `&dyn ResolverContext`/similarly-named
context trait, `request_context`/`current_request_cancellation_token`/
`verter_scheduler::cancellation`/`thread_local!`, `host_manage::*`/
`&crate::VerterHost`, `capture_token`, `cache_runtime::*` found **zero live
call sites** for any of those in these 12 files — every one of those names
that appears at all is a DOC COMMENT pointing at a downstream consumer
OUTSIDE these files (e.g. `semantic_query.rs:4475` documents
`result_is_partial` as read by `crate::cache_runtime::refuse_result_cache_admission_if_partial`
elsewhere; `semantic_query.rs:7544` documents `bit_index()` as consumed by
`crate::request_context::RequestContext::record_dispatched_query_tag`
elsewhere; `semantic_query.rs:8386` documents `discriminant_index()` as
consumed by `semantic_query_memo/arena.rs:227`'s counter, itself already
covered under G2). Two genuine (small) findings:

- **`&SemanticGraphStore` in `display.rs`** (~20 signatures, touched via
  exactly one accessor `.node_data(id)`, e.g. `display.rs:92-96,167,1104-1109`)
  — read-only walk of an already-computed typed node to render a
  `DisplayString`; never writes, never triggers a query. **Disposition:
  clean/narrow observation candidate** — if exposed at all, a single
  `node_data(SemanticNodeId) -> Option<Arc<SemanticNodeData>>` accessor is
  the whole surface, nothing else on `SemanticGraphStore` leaks in here.
- **`verter_audit::attribute!()` counters in `admit.rs`** (2 sites,
  `admit.rs:94-95`, inside `admit_decision`'s `Warm`/`ReturnOnly` arms) —
  pure work-site attribution counters (`attribution::record_call`,
  no-op outside the `attribution` feature), same telemetry bucket as
  `MetaProvenance`. **Disposition: session-driver-only bookkeeping —
  extract from the kernel entirely.**

### G2 — `semantic_query_memo/*.rs` (20 production files, ~12k lines;
`test_support.rs`/`wait_cycle_tests.rs`/`test_gates.rs`/
`store_test_support.rs`/`object_spread_projection_tests.rs`/
`cancellation_tests.rs`/`scc_publish_tests.rs`/`tests.rs` excluded as
test-only) — the actual `SemanticGraphStore` engine implementation.

**One real blocker, four telemetry-strip buckets, one false alarm, rest
portable as-is:**

1. **`&dyn ResolverContext`/`&dyn StoreView`** (real blocker) — threaded
   through ~15+ signatures across `mod.rs` (11 sites),
   `family.rs:114,798,840`, `scc_publish.rs:181,456,548`,
   `relation_memo.rs:197`, `flow_return_memo.rs:116`,
   `resolve_call_memo.rs:82`, plus `mod.rs:3575`'s narrower `&dyn StoreView`.
   The ACTUAL surface consumed from inside these files is narrow — only
   `ctx.is_cancelled()` (`mod.rs:2072,2113,2473,2495,2762,2944,3210,3440`,
   `scc_publish.rs:342`), `ctx.project_type_store().current_project_generation()`
   (`mod.rs:1848,2184,3179,3403`, `relation_memo.rs:212`,
   `flow_return_memo.rs:132`, `resolve_call_memo.rs:94`), and
   `family.rs:114`'s forward of `ctx` into
   `ReadSetSignatureExt::validate_with_self_roots`/`.bubble(ctx)`
   (`fact_signature_helpers.rs:1356,1364,1390`, outside the 20 files) for
   fact-signature validation/bubbling. **Needs a narrow observation surface
   exposing exactly these three facts (cancellation check, project
   generation, fact-signature validation), not a `ResolverContext`
   relocation** — `ResolverContext`'s own default methods name
   `verter_scheduler` and its cache accessors reach
   `ProjectTypeStore`/`PreparedDeclBundle` (session-owned), so it cannot
   cross as-is.
2. **`request_context`/`verter_scheduler::request_context` telemetry**
   (8+ sites: `mod.rs:1767,1894,2254,2940,3245,3462,3678,3692` (accumulator
   mirror), `mod.rs:195` (`mark_request_result_cancelled`),
   `origin_edges.rs:121` (accumulator), `mod.rs:2263-2265,2360-2392,2490-2492`
   (`CacheEventKind` ledger), `mod.rs:1122`/`arena.rs:234,349`/
   `reverse_index.rs:62` (`current_timing_enabled`)) — pure read-only
   counter/ledger attribution, none feeds cache-identity or admission
   decisions. **Disposition: session-driver-only bookkeeping — strip from
   the relocated core.**
3. **`host_manage::*` lock-contention telemetry** (`mod.rs:1140,1149`,
   `arena.rs:246,263,360`, `reverse_index.rs:178`, `interner.rs:95` — all
   `record_*_lock_acquisition`, pure `AtomicU64`/histogram bumps; the two
   `&crate::VerterHost` params found, `mod.rs:981`/`relation_memo.rs:380`,
   are already `#[cfg(test)]`-gated helper constructors, not production
   surface) — **strip.**
4. **`capture_token`** (`mod.rs:2277,2337`, `derivation.rs:182,189`,
   `stats.rs:291`, `origin_edges.rs:139,142,143`) — every site already
   `#[cfg(any(test, feature = "test-support"))]`-gated; `with_active_capture`
   is a no-op TLS lookup in production. **Non-issue, strip or keep gated at
   the new location.**
5. **`MetaProvenance`** (`mod.rs:336,846,854,862,2685-2688,2743-2746`,
   `arena.rs:174` — `Option<Arc<MetaProvenance>>` field + bump sites) —
   confirmed pure `AtomicU64` counters (per Part B's prior finding), held
   only so the arena/cooperative path can bump named counters without
   threading `&VerterHost` through every helper. **Strip; if still wanted,
   the caller bumps at the `verter_session` boundary after the call
   returns.**
6. **`cache_runtime::NonAdmissionReason`** (`mod.rs:3580,3591,3621`) —
   **FALSE ALARM**: `crate::cache_runtime::admission.rs:28` is
   `pub(crate) use verter_audit::NonAdmissionReason;`, a re-export. No
   lookup/publish/admission call is made from these 20 files — type-only
   usage. `verter_semantic` is permitted to depend on `verter_audit`
   directly. **Not a blocker, fix the import path only.**
7. **`inflight.rs`/`wait_cycle.rs`** (the `Condvar`/`Mutex` cooperative wait
   + same-thread cycle-detection graph, consumed at
   `mod.rs:2467-2477`'s `inflight.ready.wait_for(...)` loop) — pure
   `parking_lot::{Condvar, Mutex}` + std collections, no session coupling.
   The only coupling in the wait LOOP is the embedded `ctx.is_cancelled()`
   check (already counted under finding 1) and the telemetry call right
   after it (finding 2). **Portable as-is, ready to relocate wholesale.**
8. **Everything else** — `arena.rs` (minus finding 3), `family.rs` (minus
   `MemoEntry::validate`'s `ctx` param, finding 1), `derivation.rs` (minus
   finding 4), `stats.rs`, `member_index.rs`, `unresolved_reach.rs`,
   `hash_cons_memos.rs`, `origin_edges.rs` (minus findings 2/4),
   `reverse_index.rs` (minus findings 2/3), `family_retention.rs`,
   `prepared.rs`, `interner.rs` (minus finding 3), `trait_impls.rs`
   (implements `invalidation_domain`'s two traits, itself pure),
   `resolve_call_memo.rs`/`flow_return_memo.rs` (minus their one
   `ctx.project_type_store()` call each, finding 1) — **portable as-is,
   pure data/algorithm/arena/hash-consing/SCC substrate**, ready to
   relocate once findings 1-6 above are excised or replaced.

### Net effect on F9

F9's "`SemanticGraphStore`... needs dependency-neutralizing first" open
item is now a closed, bounded list rather than an 11+-file unknown: build a
narrow observation surface for (cancellation, project generation,
fact-signature validation) — the single real blocker — strip four
telemetry buckets, fix one import path, and the rest of the ~12k-line
engine substrate (`semantic_query_memo/*`) plus the ~16k-line domain
vocabulary (`semantic_query.rs`/`semantic_query/*`, plus `display.rs`'s one
narrow `node_data` accessor) is confirmed portable as-is. No new `Terminal`/
`AttemptFailure` design surprises found. Not implemented this round — per
F9/F12, trait growth for `SemanticGraphStore`-adjacent methods stays
deferred until the relocation itself is scheduled.

### New Part C additions (round 5, from G1/G2)

**Confirmed entangled** (add to Part C's list): `ResolverContext`'s
`ReadSetSignatureExt::validate_with_self_roots`/`.bubble(ctx)` extension
(session-owned, takes `&dyn ResolverContext`, `fact_signature_helpers.rs`).

**Confirmed clean** (add to Part C's list): `SemanticNodeData` (rendered
read-only by `display.rs`'s `node_data` accessor, no host/session/scheduler
handle observed in the slice read); `invalidation_domain`'s
`ParticipatesInInvalidation`/`InvalidationByCanonical` traits and
`InvalidationDomain` enum (`crates/verter_session/src/invalidation_domain.rs`
— pure, no `VerterHost`/scheduler naming); `bounded_query_retention`'s
`next_retention_seq`/`GlobalRetentionBudget` (pure generic FIFO-budget
substrate, no session coupling observed).

## Part H — F10's audited-boundary addendum (F13), CLOSED (round 5)

Full record: `docs/arch/refactor/rev11/evidence/C1/f13-deviation-consult.md`.
Closes F10's own "genuinely open" follow-up ("audit the exact file/function
boundary beyond the two named entry points"). Consulted and ADOPTED — a
correction/expansion of F10, not a reopen. Condensed disposition (see the
evidence file for full per-question reasoning and citations):

- **The relocating operation family is wider than F10's two named
  functions**: adds `resolve_vue_public_type` (own kernel entry point,
  same F10 ownership rule — synthesized-default gate + `Instantiate` +
  shallow projection split into a semantic-owned `attempt_*` plus a
  session-side current-view/load-retry wrapper) and
  `svelte_callable_role.rs`'s `classify_svelte_callable_role` (relocates —
  genuine Svelte identity-classification query policy, not DTO
  formatting).
- **`shallow_surface.rs`'s three functions do NOT relocate as one unit**:
  only `project_shallow_surface_graph_only` relocates (as
  `attempt_project_shallow_surface_graph`, pending its own
  `read_positive_surface_members` split); `resolve_shallow_surface_for`
  (current-view acquisition) and `project_shallow_surface_from_base`
  (JSDoc source-hydration wrapper over the graph projector) STAY as session
  wrappers. The wide caller fan-out (component-meta, meta_resolve,
  executor, codegen, Vue, Svelte) is FINE — they call the same relocated
  narrow projector without becoming "thin callers" of anything beyond that
  one operation.
- **`scope.rs` STAYS** (output-normalization glue, narrow the `&VerterHost`
  param later, don't relocate the file); **`resolved_surface_access.rs`
  STAYS with the session normalizers** (its sealed trait protects
  session-minted `ResolvedVueSurface`/`SvelteResolvedSurface`
  normalization-authority tokens, not raw semantic result types — no
  atomic-rehome requirement established, corrects my prior over-read of
  F10's `output_materialization.rs` atomic-rehome language onto this file).
- **`define_shapes::macro_surface_resolves`** is a confirmed additional
  direct raw-kernel caller (corrects F10's addendum, which described all
  three swept files as cache-wrapper-only callers) — its `AttemptOutcome`
  conversion must keep `Complete(Some)`/`Complete(None)`/`NeedInputs`
  distinct, never collapse through the current `.is_some()` boolean.
- **`structural_carrier_producer` does NOT relocate wholesale** — my
  investigation's "portable, query-free" premise for the crate-visible
  `macro_type_arg_hot_ref` entry was WRONG (self-caught during this same
  consult round, corrected before landing in the table): it reaches
  `ctx.ensure_indexed_ready_serve`, and its `MacroHotMirror` is a
  session-owned `IndexedReady` child with singleflight/lease machinery.
  Only the INNER `lower_type_expr_structural` graph lowerer is genuinely
  query-free and relocates; artifact lookup/mirror/singleflight/lease/
  admission stay session-side; `host_manage::eval_env` is NOT reclassified.
- **The JSDoc/raw-source-slicing chain
  (`member_jsdoc_from_spans`/`signature_jsdoc_from_spans` ->
  `slice_canonical_span` -> `ensure_indexed_ready_serve`) is a CONFIRMED
  shared F12 dependency** — full removal of this source-supply seam is
  gated on F12's phase-4/7 cutover — but this does NOT block the raw
  Vue/Svelte query relocation itself; a narrower `RawSource(canonical)`-
  shaped finite observation could unblock pure normalization NOW, short of
  eliminating the seam entirely.

**Not implemented this round** — ownership ruling only, per the same
pattern as F9/F10/F11. `normalize.rs`/`normalize_slots.rs`'s remaining
(non-JSDoc) `ensure_indexed_ready_serve` call sites and
`structural_carrier_producer`'s actual current caller count are flagged
genuinely open (moved into the "still not independently audited" list
above) rather than assumed closed by this consult.

## Part I — F15's characterization item, re-assessed; a precision gap found in `ConsumedResolutionObservationKey` (round 9)

F15's safe-first-slice item 1 ("a characterization/equivalence harness...
reuses existing coverage where possible") was left as still-owed work at
the end of round 8. Investigated: `verter_workspace::resolver_tests.rs`
(3929 lines, 137 test functions) already covers essentially the ENTIRE
named matrix by direct inspection of test names — runtime JS w/wo `.d.ts`
companion (`resolve_js_specifier_prefers_source_ts_over_colocated_dts`,
`type_import_relative_js_specifier_prefers_declaration_companion`,
`sfc_src_attr_js_does_not_substitute_to_ts_sibling`), candidate priority
(`resolve_tsconfig_paths_before_base_url`, `context_aware_exports_*`,
`resolve_package_exports_prefers_types_for_root_imports`), package-follow
evidence (`type_import_relative_package_follow_requires_package_manifest_confirmation`,
`package_imports_reread_per_importer`, `node_modules_missing_ancestor_manifests_do_not_trigger_reads`),
observation ORDER and call-counting (`counting_reader_tracks_calls`,
`bare_package_json_reread_per_importer`), project-reference cycles/
diamonds/depth budgets (`cyclic_project_references_terminate_without_overflow`,
`diamond_project_references_resolve_through_both_arms`,
`project_reference_depth_budget_bounds_deep_chain`). **Conclusion: the
"characterization" half of F15's item 1 is substantially ALREADY
SATISFIED by existing coverage** — the still-genuinely-missing half is a
DUAL-RUNNER comparison (old blocking lifecycle vs. the new I/O-free
kernel), which cannot be built until the second runner exists (unchanged
conclusion from round 8, now with concrete evidence backing it rather
than an inference from `wc -l` alone).

### A more valuable find: `resolution_witness_contract_tests.rs` already specifies the EXACT witness-retention contract `AttemptOutput`/`ConsumedResolutionObservationKey` must satisfy

`verter_workspace::resolution_witness_contract_tests.rs` (319 lines, a
private `#[cfg(test)]`-only module inside `resolver.rs`) is NOT part of
the coverage-matrix sweep above — it's a NARROWER, more load-bearing
fixture: a `TraceReader` that records every `probe_path`/`realpath` call
the resolver makes during ONE resolution, plus two tests
(`resolution_witness_positive_retains_every_precedence_guard_and_both_recovery_chains`,
`resolution_witness_miss_retains_the_complete_exhausted_probe_set`) that
assert exactly which of those observations must survive into the
resolution's cache-invalidation "witness."

**The load-bearing finding**: a POSITIVE resolution's witness must retain
NOT ONLY the winning candidate's probe, but every HIGHER-PRIORITY
candidate checked and rejected (`Absent`) along the SAME winning
fallthrough chain before the actual winner was reached — e.g. resolving
`./mod.js` to `/store/pkg/mod.tsx` (a lower-priority `.tsx` sibling) still
retains the `Absent` probe of `/p/mod.ts` (the higher-priority `.ts`
sibling that was checked first and rejected), because "recording only the
selected `.tsx` target would serve a stale positive after `/p/mod.ts`
appears." A MISS witness retains the COMPLETE exhausted candidate set (all
24 extension/index candidates for a bare specifier miss), in precedence
order, none dropped. Both tests also retain `RecoveryScope` facts for
every ANCESTOR DIRECTORY of every requested AND resolved path (directory
recovery — detects a new file appearing in a previously-empty/absent
directory chain).

**This sharpens F15's "consumed vs. prefetched" distinction precisely**:
"consumed" means every observation ALONG THE ACTUAL WINNING FALLTHROUGH
CHAIN (including rejected higher-priority candidates WITHIN that chain,
and the complete exhausted set on a miss) — NOT just the single final
winning probe. "Prefetched-but-not-consumed" (the case the witness must
NOT include) is scoped to a DIFFERENT, unreached branch — e.g. batched
node_modules ancestor-directory manifest probes fetched speculatively in
one `NeedInputs` round but never actually examined because an EARLIER,
unrelated branch (a workspace alias, say) already resolved before the
node_modules branch was ever reached.

**Gap CLOSED, round 9**: confirmed `RecoveryScope` is a genuinely SEPARATE,
already-PRODUCTION fact kind in `verter_workspace` — `resolution_currency.rs`'s
`ResolutionFactKey` enum carries BOTH `DirectoryMembers { canonical,
population }` AND a DISTINCT `RecoveryScope { canonical_prefix, population }`
variant side by side (`resolution_currency.rs:350,354`), so recovery-scope
tracking is NOT subsumed by `DirectoryMembers` — it needed its own key.
Added `ConsumedResolutionObservationKey::RecoveryScope { canonical_prefix:
CanonicalId }` as a 4th variant (`attempt_output.rs`), mirroring
`ResolutionFactKey::RecoveryScope` by name. Purely additive per
`AttemptOutput`'s private-fields-no-public-literal design (F16) — zero
breaking change to anything already landed. `AttemptOutput` now has 4
consumed-observation variants, not 3.

**Scope boundary confirmed, round 9**: `ResolutionFactKey`'s remaining two
primitive variants, `ExactResolution { entry, specifier, phase, kind,
population }` and `ContextSelection { entry, population }`, are NOT
missing `ConsumedResolutionObservationKey` variants — their own doc
comment (`resolution_currency.rs:568`) states "`ExactResolution` and
`ContextSelection` are TABLE LOOKUPS, not disk [I/O]": they're
`verter_workspace`-side published-decision/currency bookkeeping (the
"exact resolution" table + context-selection cache backing the retained
witness of PUBLISHED decisions), not raw disk-I/O-shaped primitives a
`path_probe`/`real_path`/`package_manifest`-style kernel method would
ever consume. Confirms `ConsumedResolutionObservationKey`'s 4-variant
shape (mirroring exactly `PathProbe`/`Realpath`/`Manifest`/`RecoveryScope`
— the disk-I/O-shaped primitive quartet) is the correct, now-complete
scope for the kernel side; `ExactResolution`/`ContextSelection` correctly
stay `verter_workspace`-owned transaction/currency machinery per F4's/
F15's scope ("`TrackedResolutionCapability`, `TransactionReader`,
transaction capture, publication, and resolution-currency enforcement all
STAY in `verter_workspace`").
