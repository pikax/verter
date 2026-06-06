---
name: type-resolution
description: "Cross-file type resolution: type solver, ShallowFileState, ExternalTypeFrontier, canonical cache rules, macro traversal, prepared declarations"
---

# Type Resolution

## Project-Global Cache Authority (post-rewrite)

`VerterHost` owns one `ProjectTypeStore` accessed via `.project_type_store()`. The store holds:

- `FileArtifactStore` — the single canonical post-parse artifact per `(canonical_id, whole_hash)` (the former `ModuleFactsDb` has been retired).
- `AnalysisReadyDb` — scope-parameterised analysis augmentation with bitflag-based satisfaction (`find_satisfying`).
- `RouteDb` — rehomed barrel/route surface cache, validated against live host facts.
- `OwnerImportSurfaceDb` — direct-owner-imports cache keyed by `(owner_canonical, owner_whole_hash)`. `VerterHost::owner_import_surface(...)` builds-or-fetches the surface; `resolve_owner_direct_import(owner, local_name)` is the single-call lookup that every direct-owner-import caller now uses.
- `SemanticGraphStore` — host-owned memo table + node arena for the `SemanticQueryKey` / `ProjectSemanticDispatch` layer. Every `SemanticQueryKey` variant dispatches through `ProjectSemanticDispatch::execute`. Same-path recursion returns a sentinel instead of self-awaiting; cross-thread joiners block cooperatively on a per-entry `Condvar`. Also owns Vue macro resolution artifacts (`SemanticNodeData::VueMacroElements`, keyed by `HostResolvedNamedTypeKey` through an internal identity map) — the former `ResolvedNamedTypesDb` has been folded in; the parser's `NamedTypeCache` adapter hits the graph directly on the refcount-only hot path via `get_resolved_named_type` / `insert_resolved_named_type`.
- `ComponentMetaResultDb<ComponentMetaAnalysis>` — final payload cache for `get_component_meta`. Warm hits revalidate the recorded `ReadSetSignature.facts` fact signature against the live `StoreView` before returning.
- `IntrinsicRegistry` — authoritative table for `= intrinsic` declarations. `SessionSolverHost::utility_source` consults it before the `BuiltinUtility` fallback.

Dep-signature semantics: every reusable cache read returns a `CacheRead<T>` carrying the touched fact fragment. Callers merge those fragments into the active `CompletionFence`, which bounds retries at 3 and publishes `UnstableState` when mid-flight invalidation persists.

## Canonical Dependency Cache Rule

Host-backed type/import resolution must treat the canonical file ID as the cache identity. The cache contract is:

- Load a dependency source at most once per canonical ID per workspace content generation. Parse it immediately and cache the raw source, parsed/OXC snapshot, and any reusable eval/build state right away.
- When the host materializes an imported dependency on a cold miss, derive the AST-backed bundle from that single parse and cache it together: file snapshot, eval env, external-type analysis, symbol/export lookup tables, and any other reusable per-file analysis. Do not let later resolver stages trigger a second parse of the same canonical file just to build another artifact.
- Host-owned imported-file caches are long-lived for the lifetime of the `VerterHost`. Distinct queries on the same host must keep reusing the same cached canonical file state until that file's content hash or workspace generation changes.
- Cache named declarations from that parsed file by name, not just exported entrypoints. Internal named types/interfaces/aliases still matter because exported declarations in the same file may depend on them later.
- Treat named-node discovery as local symbol lookup. Once a file is parsed for a given canonical ID/version, future lookups should hit cached symbol/export maps instead of walking the full AST again to rediscover names.
- Treat AST ownership as single-pass work. For a given canonical ID/version, the resolver should do at most one full top-level AST walk to discover named symbols/exports, then cache those lookup entries and leave deeper expansion lazy per symbol. Do not rewalk the full file to rediscover the same symbol on later requests.
- Imported-file analysis should expose one shallow symbol graph keyed by `(canonical_id, symbol_name)`. That graph is the authoritative source for local symbol kind/span, local import targets, direct reexports, and local export aliases. Resolver stages must consume that graph instead of maintaining parallel rediscovery paths.
- Resolve the requested import from the cached parsed file first. If the requested name is not present, only then BFS through explicit barrel/re-export hops. Do not rescan the same file graph on the second request.
- Imported-file traversal stays shallow-first. After a canonical file is read and processed once for the current version, inspect only that file's shallow/export surface first. Do not navigate into an imported target just because the file imports it.
- Direct imported-file navigation is allowed only when the requested symbol is present in the current shallow file's direct export/import route information, or when the current file is a barrel and the requested symbol was not found locally.
- For barrel files, if the requested symbol is absent from the current shallow/export surface and the file has wildcard barrel reexports, enqueue all of those barrel targets as one BFS layer, shallow each target once, then check each shallow surface for the requested symbol before descending deeper. Do not deepen one barrel branch ahead of its same-layer siblings.
- Barrel traversal must remain symbol-directed. If the requested symbol is not present in a shallow barrel child, only then continue from that child to its own barrel reexports under the same BFS rule. Do not eagerly open unrelated imported files or non-matching sibling branches.
- Keep expansion lazy. We do not need to eagerly resolve every transitive type in a file up front. Preserve named references so later requests can expand them from cache when needed.
- Collected imported aliases stay shallow but must already be root-normalized. Store the defining file's canonical ID plus the final exported symbol name; do not keep unresolved barrel routes once the root is known, and do not eagerly materialize a prepared declaration during collection.
- Builder-owned shallow imported aliases should treat their stored canonical ID as the defining-file root. They may consult cached barrel/export state only when a canonical root is still unknown. Cache the prepared alias on the defining canonical file and hydrate from that file's host cache or base eval env. Do not synthesize barrel-local prepared aliases for symbols that resolve to another file.
- Whole-file hashes are for long-lived update handling and cache validation, not for repeated warm reads. Compute/store the hash once for the current source version, then reuse it until the VFS reports a newer content generation / file version.
- VFS is the authority for file-change invalidation. When a canonical file's version/hash changes, host caches derived from that canonical ID must be discarded together across source snapshots, parsed state, eval envs, and resolved-type/import caches.
- Invalidation must stay selective. If `/src/type.ts` changes, invalidate caches owned by `/src/type.ts` and downstream final expansion/query results that depend on it, but do not force reparsing or reshallowing unchanged owner files that merely import it. Those owner files should stay warm on their own-file caches and only re-resolve against the refreshed imported dependency state.
- A changed imported dependency may be reparsed once for its new hash, even if several owners or several later queries need it. That single refreshed canonical file state must then be shared across all of those requests.
- Concurrent cold requests that reach the same canonical imported file must collapse onto one host-owned materialization path. `Promise.all([MetaA, MetaB, MetaC])` is not allowed to produce three separate read/parse/shallow passes for the same `type.ts`.
- Prepared declarations are also host-owned warm artifacts. Once `(canonical_id, symbol_name, whole_hash)` has been prepared, later lookups from other owners and from later distinct queries on the same host must reuse that prepared declaration until invalidation.
- Reuse the current host-owned route/barrel cache path. Today that means `RouteDb` for barrel/export route facts and `ImportedRootDb` for imported-root proofs; do not add a second route-cache subsystem for the same work without explicit proof it is needed.
- Route discovery must stay lazy and demand-driven. First-hit discovery may follow barrel/reexport hops only until the requested symbol is found (or proven absent under the current negative-cache policy). Do not require a full scan of all barrel exports on every first hit.
- Warm same-owner lookups should reuse the existing valid importer-local route entry rather than replaying the full barrel chain.
- Cross-owner reuse in the current architecture should come primarily from shared imported-file state, shared barrel/export surfaces, and prepared declarations. Do not assume canonical cross-owner route-fact backfill exists unless a later change explicitly adds it.
- Stable negative route answers in the current architecture are gated by `BarrelResolutionState.fully_resolved` plus tracked dependency/store-view freshness. If richer persisted completeness states are ever needed, add them as an explicit follow-up rather than treating them as an existing invariant.
- If in-flight dedup is needed for concurrent cold route work, model that separately from the persisted barrel state. Do not overload `fully_resolved` to mean "currently being built".
- Do not use `Arc` next-hop chains as the primary barrel cache shape if a future route-cache redesign is introduced.
- Route caches and prepared-declaration caches must invalidate independently. If a leaf file body changes but its export surface stays the same, the route fact may remain valid while prepared declarations and downstream final results refresh.
- On file update, eagerly recompute the changed file's own parse/shallow/export surface once. That write-path cost is acceptable and keeps later reads fast.
- Do not eagerly rewrite every upstream barrel/route fact on every changed-file update. After the changed file's fresh shallow/export snapshot is available, let upstream route facts validate lazily against tracked route participants/generations on demand.
- Prefer comparing old vs new shallow/export surface for the changed file. If the export surface is unchanged, keep route generations stable and refresh only body/prepared-declaration/final-result layers. If the export surface changed, bump the route/export-surface generation so affected warm route facts become stale and lazily rebuild on next access.
- Route invalidation is not file-hash-only. tsconfig path changes, vite alias changes, workspace graph changes, package target changes, and barrel export-surface changes must invalidate affected route facts even if the owner file text did not change.
- Negative route/cache misses may be cached only against a concrete snapshot (hash/generation/store-view context). Cancelled or interrupted results must never be promoted to warm reusable cache entries.
- One query must resolve against one coherent host/store snapshot. Resolver stages must not mix captured stale owner routes with newer live dependency routes within a single query flow.
- Legacy fallback paths that reparse or rewalk imported dependency files on warm requests should be removed, not preserved behind alternative code paths. Default behavior must go through the cache-aware host/VFS path.
- Architectural cache/resolver changes must land as one clean cutover. Do not leave temporary shims, compatibility wrappers, feature flags, or duplicated old/new paths behind. Delete the superseded path in the same change, or upgrade the surviving path to first-class shared ownership with the same invariants and tests.
- Imported dependency loading, type-resolution source materialization, and dependency canonical resolution should be host-owned single entry points. Do not add request-local cache layers or alternative parser/import paths on top of the host cache for the same work.
- Imported type root/declaration resolution and prepared imported-type alias caching should also be host-owned single entry points keyed by canonical ID plus current file version/hash. Do not rebuild the same imported symbol route or prepared alias body per request when the host cache already has it.
- Do not add new request-scoped lookup memos over host-owned resolver work in the final architecture. Existing request-view-era memos are legacy and must be removed as part of the project-global cache cutover.
- `source_type` for downstream cache keys is authoritative from the scheduler: `HostSourceData::source_type` is computed once at `execute_source` time with full access to the parsed SFC, and readers consume via `VerterHost::authoritative_source_type_for(canonical)`. Recomputing from `(canonical_id, raw_source, cached_parse)` is unstable when `cached_parse` is dropped mid-resolution (pre-Phase-1 hazard).

**Concrete performance contract:**

- If `MetaA`, `MetaB`, and `MetaC` all depend on `type.ts`, the first query batch may process each owner file once and `type.ts` once.
- If a later batch requests `MetaB` and `MetaC` again with no file changes, that later batch must reuse the warm cached state for both the owner files and `type.ts`.
- If `type.ts` changes between batches, `MetaB` and `MetaC` may keep their own-file caches, while `type.ts` is processed exactly once for the new hash and then shared by both later requests.

### Import-Route Admission Ownership

`DerivedRawState.import_routes` and the `DerivedRawState.import_routes_known_miss_recorded_at_generation` sidecar split into two admission modes with distinct validity models:

- **Complete caller-supplied snapshot + known-miss sidecar admission** — `VerterHost::set_import_dependencies` is the **single producer** that admits both the full route snapshot AND the per-specifier `content_generation` stamp for known-miss specifiers. A known miss is a specifier with no resolved canonical, no candidates, and no effective target (`import_route_is_known_miss`). The sidecar tag lets the reader detect when a new canonical (which advances workspace `content_generation`) may now satisfy a previously unresolvable specifier.
- **Positive route point admission** — `VerterHost::cache_positive_import_route_result` is the **single positive-only point producer**. It constructs `DependencyResolution { resolved_canonical_id: Some(...), possible_canonical_ids: vec![...] }` for the supplied `(owner, specifier, resolved)` tuple and must NOT touch the known-miss sidecar. Positive resolutions stay valid until the owner's source content changes; they do not need a generation tag.
- **Lifecycle reset** — `VerterHost::configure_projects` (project-graph reconfiguration) and `VerterHost::upsert_via_scheduler_with_priority` (owner source update) may `.clear()` both `import_routes` and the known-miss sidecar in lockstep. Resetting only the route map would leave a stale `content_generation` stamp in the sidecar that would suppress re-resolution after the next admission.

Architectural rules carried by the writer guard at `crates/verter_session/tests/import_route_writer_guard.rs`:

- Any new positive-route discovery must call `cache_positive_import_route_result`. A direct `derived_raw_cache().entry(...).import_routes.insert(...)` outside that helper, the snapshot writer, and the lifecycle reset methods is rejected.
- The known-miss generation sidecar is admission-only inside `set_import_dependencies`. Any sidecar `insert`, `extend`, `retain`, or `remove` outside that writer is rejected.
- Routing positive-only point inserts through `set_import_dependencies` is wrong: it would synthesize a full snapshot, re-stamp previously admitted known misses at the current `content_generation`, and extend stale negative answers that should have re-resolved.

### Module-resolution keying (split env)

Import/module resolution is keyed on the **split** env dimensions — see
`### Module-Resolution Keying (CRITICAL)` in the `/type-cache-architecture`
skill (the owner). In short: `resolve_env_hash` carries the resolve-domain
inputs (the `moduleResolution` mode, the `exports`/`imports` `ConditionSet`,
`base_url`/`paths`, aliases, references, extension order); the lib corpus
(`lib_names`/`typeRoots`/ambient fingerprint) is NEVER folded into
`resolve_env_hash` — it keys `lib_env_hash` only (R21: resolve and lib are
orthogonal dimensions). The module-resolution SHAPE vocabulary
(`ModuleResolutionMode`, `SpecifierKind`, `ConditionSet`) lives in
`verter_workspace::module_resolution`; the FORK-C resolution matrix walker
that consumes it is U0 `verter_session::resolver_core`.

## IndexedReady Target Contract

Architectural target for the project-global cache cutover:

- `IndexedReady` is the canonical post-parse per-file artifact.
- Scheduler remains the sole source and parse authority. `IndexedReady` is built from scheduler-owned parsed snapshots.
- `IndexedReady` stores canonical imports and exports for the file.
- `IndexedReady` owns all top-level symbols through compact owned symbol indexes, spans, operator tags, interned names, and shallow bodies that are safe for host-owned `Send + Sync` caches.
- Parse once through OXC, then lower only the shallow syntax needed by later passes into the long-lived owned `IndexedReady` representation.
- The temporary OXC parse arena is per-file and per-version only. It may be dropped after lowering completes and must not leak into long-lived shared caches.
- `IndexedReady` is the authoritative source of type-facing symbol bodies, shallow declaration structure, import edges, and export edges for later type walking.
- `AnalysisReady` is an additive layer built from `IndexedReady`; it must not rediscover the same file structure through a second path.
- If analysis or component-meta expands a shallow symbol, both paths must populate and reuse the same host-owned route, prepared-declaration, owner-import, and projection caches.
- Shared symbol expansion helpers should be the default. Do not add consumer-specific shallow expansion paths when the existing shared resolver can serve the work.
- New architecture work in this area should move the codebase toward `SourceReady -> IndexedReady -> optional higher layers`, not further toward request-local or duplicate parser/resolver paths.

## Semantic Query Identity Target Contract

Architectural target for the project-global cache cutover:

- Type expansion should have one authoritative semantic query path.
- Query memoization keys must be semantic and scope-aware, not request-local and not raw-text-based.
- Request ids are not query ids. They must not be the primary dedup key for reusable type work.
- Reusable semantic operations such as resolved declaration lookup, indexed access, member projection, instantiation, mapped-type application, and conditional-type branches should enter through shared query-key types.
- Bare-name lookups must include the declaration scope or resolved root identity needed to avoid cross-scope poisoning.
- Semantic query-identity keys are content-free (R6): a resolved declaration or route identity carries semantic identity plus the split env dimensions only, never a content/version hash, whole-hash, or `fact_dep_signature`. Version-rooting lives EXCLUSIVELY on the cached value (`ReadSetSignature.facts` + `self_root_canonicals`, revalidated on every warm read); the live content version (whole-hash) is re-sourced at value-compute time (`ensure_indexed_ready`), never carried in the key.
- Semantic nodes are immutable. File changes create new identities rather than mutating old ones in place.
- The shared semantic layer should be a host-owned memo table keyed by semantic query identity. Any ID-backed semantic graph behind it is secondary and must store immutable AST-free semantic data rather than borrowed OXC pointers.
- Same-file shallow closure may run inside the winning query, but reusable cross-file and projection work should be represented as dedupable semantic subqueries.
- Recursive and mutually-recursive expansion must use an explicit in-flight table so one winning query computes each cold semantic node and recursive re-entry dedups cleanly.
- Same-path recursion must never self-await. If the same execution path sees a `Running` semantic node again, it should return the solver's normal recursion sentinel / unresolved recursive form instead of blocking on itself.
- Distinct top-level callers encountering a `Running` semantic node should wait cooperatively on a completion primitive rather than spin-retrying.
- Nested semantic builders may also wait cooperatively after releasing short-lived memo guards or locks. Do not busy-spin, and do not make whole-stack unwind a required waiting strategy for ordinary DAG dependencies.

Concrete expectation:

- If one larger expression references `C`, `C['foo']`, `C['bar']`, and `B`, and `B` itself references `C` again, the resolver should converge those onto one shared semantic query graph rather than recomputing each path ad hoc.

## Semantic Heuristic Prevention (CRITICAL)

Semantic behavior must be driven by typed facts, explicit projection policy, and complete semantic query identity. Do not encode type meaning, resolver routing, cache validity, or published component-meta shape in local heuristics.

Forbidden patterns:

- Inferring semantic meaning from rendered type strings, raw source slices, identifier suffixes/prefixes, path substrings, display text, or compatibility formatting.
- Choosing between candidate type results with subjective "better shape" scoring when the candidates do not carry exactness/provenance that proves which one is authoritative.
- Letting projection mode, substitution environment, conditional context, scope version, workspace/source generation, package-boundary policy, or solver options affect a result without being represented in semantic identity, cached-value validation/read-set metadata, or the projection plan.
- Treating numeric caps, recursion limits, or fanout fuses as normal semantic answers. A fuse may stop work, but it must return a structured degraded result such as `BudgetExceeded`, `Unsupported`, or an explicit recursion sentinel, and that degraded result must not be promoted into a warm shared cache entry as if it were complete.
- Adding compatibility or transition paths that preserve old heuristic behavior. Replace the old path with a typed producer fact, a richer IR variant, an explicit policy enum, or a structured unsupported result.

Allowed optimization heuristics are limited to performance triage that cannot change observable meaning: skipping a cache lookup, choosing an equivalent fast path after tests prove equivalence, or deciding that a result is not safe to cache. If an optimization can change which type is returned, which members are visible, how aliases are preserved, or whether a result is considered complete, it is not an optimization heuristic; it is semantic policy and must be modeled explicitly.

Best-architecture target:

- Every semantic query has typed identity plus validation metadata that covers all meaning-affecting dimensions: declaration identity, scope/version root, projection mode, type arguments/substitution environment, conditional context, package/workspace policy, and solver options. The cache architecture may split these between slot key, per-mode entry, and cached-value fact signature, but no dimension may be implicit.
- Every semantic result carries enough exactness/provenance to distinguish `Exact`, `SurfaceOnly`, `Unsupported`, `BudgetExceeded`, and recursion-sentinel outcomes without re-inspecting text or shape.
- Projection planning is explicit. Callers request identity/navigation/shallow/expanded behavior and package-boundary/depth policy up front; resolvers do not infer these policies from ad hoc shape checks.
- If the current IR cannot represent a TypeScript construct, extend the IR/schema or return a structured unsupported result with diagnostics. Do not recover meaning by reparsing display text.

## Typed Degradation And Completeness Contract (CRITICAL)

Semantic degraded states are part of the type system contract. They must be typed, propagated, and observable.

- `TypeExpr::Unknown { raw }` is not a carrier for semantic control flow. Misses, unsupported intrinsics/operators, alias cycles, recursion sentinels, budget exits, unstable-state exits, and bridge-depth exits must use typed variants or typed sidecar state. Do not encode them as strings such as `"budgetExceeded(...)"` / `"aliasCycle(...)"` and do not recover them with `starts_with` / regex checks.
- `Unknown` is allowed only for a genuine unknown type value with provenance explaining why the producer could not represent it. If the producer knows this is `Unsupported`, `BudgetExceeded`, `Recursive`, `Miss`, or `Unstable`, use that state instead.
- Public query envelopes must preserve completeness. `Complete` means the required inputs were available, current, and no budget/unsupported/unstable branch affected the answer. A query may return `Complete(None)` only when absence itself was proven under the current facts; missing analysis, stale cache data, unavailable providers, unsupported operators, and budget exits must surface as `Unavailable`, `Partial`, or a typed degraded result.
- Degraded results may be displayed and may be returned to callers, but they must not be promoted into warm shared caches as complete answers.

## Cache Population Target Contract

Architectural target for the project-global cache cutover:

- Cache ownership is split into three reusable layers:
  - file artifact caches (`IndexedReady`, prepared declarations, route surfaces, owner import surfaces, optional analysis)
  - semantic query caches (resolved declaration identity, instantiated meaning, indexed access, projected members, mapped or conditional results, normalized reusable intermediates)
  - final result caches (for example final component-meta payloads)
- Final payload caches should hand out immutable `Arc` values. Cache backend choice is an implementation detail as long as concurrency, size bounds, and validation rules are preserved.
- Reusable semantic cache population must be path-independent. If the same semantic result is reached through different entry paths, a successful computation must populate the same shared cache entry.
- Broader successful results may backfill narrower reusable entries they actually satisfied.
- Narrower successful results must not claim that broader work is cached.
- An `Expanded` result may satisfy and backfill `Shallow` or `Identity` for the same semantic key.
- A `Shallow` result may satisfy and backfill `Identity` for the same semantic key.
- A whole-surface projection may backfill per-member or per-indexed-access caches for the members or accesses it actually materialized.
- A narrow member or indexed-access result must not pretend sibling members or whole-surface projection are cached.
- Cancelled, superseded, interrupted, budget-exceeded, or partial results must not be promoted as warm shared cache entries.
- Versioned semantic nodes and final-result entries must also be sweepable. Project-global caching may be aggressive, but old identities must not accumulate forever across long editing sessions.
- Top-level live-host results must publish through a completion fence: record the touched dependency signature, revalidate it before publish, retry at most 3 times on mid-flight changes, and never warm shared caches with torn provisional or unstable results.

Concrete expectation:

- If `ProjectSurface(C, Expanded)` materializes member `"foo"`, a later `ProjectMember(C, "foo", Expanded)` should reuse that work.
- If `ProjectMember(C, "foo", Expanded)` ran first, that must not imply `ProjectSurface(C, Expanded)` is cached.

## Query Mode Contract

The shared semantic query system has exactly five modes. Every caller picks one; there is no implicit mode. Every `SemanticQueryKey` carries its mode as part of the cache key.

- **`Identity`** — return the declaration identity only (canonical file + symbol name + optional substitution environment). Do not read the body; do not walk the decl graph; do not produce a result shape. Cheapest operation. For an alias `type X = Y`, `Identity(X)` returns **`X`'s** declaration identity — not `Y`'s. Used for "does this name resolve" checks and for wiring dependency edges.
- **`Navigate`** — do the minimum semantic work required to continue a requested path. Unwrap aliases transparently (aliases have no independent structural shell; `Navigate(X)` where `type X = Y` returns the same node as `Navigate(Y)`), follow member / index hops that are already materialized in the graph, reduce closed conditionals, stop at undecidability barriers. Does NOT recursively materialize subtrees; does NOT expand sibling members. Used by path projection to step one hop at a time.
- **`Shallow`** — return one shell / one surface level of the requested node without recursive expansion. For an object: produce member names + per-member reference nodes; do NOT recurse into member bodies. For a conditional: if the check is open / undecidable, expose both branches as references; if the check is closed / decidable, reduce immediately and return only the selected branch shell. For a union / intersection: expose contributor references. For an alias: unwrap transparently to the target's `Shallow` form (aliases have no structural shell to materialize).
- **`Expanded`** — recursively materialize the requested result. Walk into member bodies, resolve nested references, evaluate every decidable conditional, distribute open conditionals into their remaining path projections, normalize unions / intersections. For aliases: unwrap transparently. Most expensive mode.
- **`Skeleton`** — specialised traversal mode for BFS/generic-helper surfaces where unbound type parameters must remain `TypeParam` shells and conditional branches must not collapse to `never` just because arguments are intentionally absent. It is not a display mode and does not alias `Navigate`; it is a distinct semantic policy.

**Alias boundary preservation.** Although `Navigate` / `Shallow` / `Expanded` unwrap aliases structurally, the alias boundary is preserved on the origin layer via the `AliasResolve` edge kind (see "Derivation / Origin Layer Contract"). Clients that need to render alias provenance (LSP hover, error messages, compat display) walk that edge; clients that need pure structural meaning ignore it. Each alias hop (direct alias, re-export alias, barrel alias) emits one `AliasResolve` edge; chains are walkable end-to-end.

**Backfill rule.** Broader successful results may backfill narrower modes for the same key, but ONLY when a recorded materialised point dominates the narrower target (path-exact `cached_satisfies`, never enum rank): an `Expanded` result may satisfy `Shallow`, `Navigate`, and `Identity`; a `Shallow` result may satisfy `Identity` only — NOT `Navigate` (`Shallow ⊅ Navigate`: a one-shell Shallow surface does not materialise a `Navigate` next-hop); a `Navigate` result may satisfy `Identity`. Narrower successful results MUST NOT claim broader modes are cached.

**Cache topology.** The five modes do NOT imply five separate cache subsystems. `SemanticGraphStore` owns one semantic memo layer. At the semantic-contract level, mode is part of request identity; at the storage level, implementations should group same-base different-mode requests into one memo-entry family (or an equivalent single-authority structure) keyed by the mode-erased semantic shape — operation, base identity, path / projection, substitutions, scope, and version root — with per-mode slots or equivalent one-way upgrade/backfill semantics. The required behaviour is: backfill is directional (broader-projection → narrower-projection) and gated by recorded-point dominance — `Expanded` may satisfy `Shallow` / `Navigate` / `Identity`, `Shallow` may satisfy `Identity` (NOT `Navigate`: `Shallow ⊅ Navigate`), `Navigate` may satisfy `Identity`, and no lower mode may claim a broader mode is cached. `Skeleton` is a separate policy slot; it does not alias `Navigate` or any other mode unless a typed equivalence proof and regression tests justify a backfill edge. Distinct mode requests must not duplicate in-flight authority or split into independent wait graphs.

**Cache-key distinctness.** `ProjectMember(C, "foo", Shallow)` and `ProjectMember(C, "foo", Expanded)` are distinct cache entries. Generic substitutions are part of the key: `MyType<string>` and `MyType<number>` never alias; two callers reaching `MyType<string>` through different entry paths dedup to one entry. Do not compress mode identity into booleans or partial encodings such as "is navigate"; store the exact `ProjectionMode` or a typed projection-policy key.

## Path-Precise Navigation And Projection Contract

Path projection is the default shape of every semantic query. Whole-surface expansion is a degenerate case of projecting the empty path; single-hop `ProjectMember` / `IndexedAccess` are sugar for `ProjectPath` with length 1.

- **Path queries are first-class semantic queries.** `SemanticQueryKey::ProjectPath { base, path, context }` is the canonical form (`context` is a `ProjectionReductionContext { mode, demand, provenance, merge_role }` — `provenance` is a `SurfaceProvenanceContext` and `merge_role` is a `MemberMergeRole`, both folded into `FamilyKey` for every context-bearing projection-reduction family; the terminal projection mode rides on `context.mode`).
- **Materialize only subpaths needed for the requested path.** Sibling members and unrelated branches are not touched.
- **Mode cascades along the path.** Intermediate hops run in `Navigate`; only the terminal hop runs in the caller's requested mode (`Shallow`, `Expanded`, or `Identity`).
- **Intersection contribution rule.** When projecting a path through `A & B`, only arms that contribute to the next path segment are projected; non-contributing arms are ignored for that path (not rewritten to `never`). If multiple arms contribute, the projected results of the contributing arms are intersected. Zero contributors is a projection miss.
- **Union contribution rule.** Every union member must contribute to the path; if any member fails the path, the union projection is a union-wide miss.
- **Conditional path rule.**
  - **Closed/decidable conditional** (check resolves deterministically under the current substitution environment): reduce immediately, select the winning branch, continue projecting the remaining path through the selected branch only. Emit `ConditionalSelect` origin.
  - **Open/undecidable conditional** (check depends on free type parameters or unresolved inference): keep the conditional shell and distribute the remaining path **into both branches**, producing a conditional whose branches are per-branch projections. Do NOT short-circuit to an opaque symbolic node as the default behaviour.
- **Barrier rule.** Undecidability is the barrier, not the existence of a conditional. An `infer` binding is not itself a barrier; if the enclosing conditional's check is decidable with the current substitution environment, the navigator reduces the conditional and binds `infer` via the relation engine. Only when the check stays open does the conditional form a barrier.
- **Coarse symbolic stop is retired as the default.** The old "open-generic symbolic stop" (`Applied { identity, args }` returned when args are open at depth > 0) is retired for path projection and expansion. Symbolic-stop-style fallback survives only as a **budget-exceeded failure**, signalled explicitly via `BudgetExceededFailure`, never as the main semantic path for open generics.

Concrete expectations:

- Navigating `A['c']['full']['bar']` through an object surface touches at most three projection nodes plus the terminal; no unrelated siblings materialize.
- Projecting `OtherType<string>['a']['foo']` through `type OtherType<T> = { a: Foo, b: T } & (T extends string ? S : N)` touches `Foo['foo']` and `S['a']['foo']`; `N` is never walked. `b` is not touched because `'a'` is the requested segment.
- Navigating `OtherType['a']['foo']` with `T` open distributes `['a']['foo']` into both conditional branches, intersects their contributions, and leaves `b` untouched.

## Navigator Boundary Contract

Architectural target for the project-global cache cutover:

- Navigators are not a second resolver. They are thin path-walkers over the shared semantic query system.
- Navigators may perform only non-owning normalization:
  - unwrap already-resolved aliases
  - apply already-known substitutions
  - inspect already-materialized member or keyspace shape
  - choose the next hop in the requested path
- Navigators must not privately perform reusable semantic work such as:
  - recursively resolving a new declaration identity
  - crossing imports or barrel routes
  - instantiating a new generic body outside the query system
  - expanding a mapped, conditional, or indexed-access type through an ad hoc path
  - populating shared caches from outside the shared semantic query API
- Boundary rule: same semantic node may continue inline; a new semantic node must enter through the shared query API.
- Any operation that can recurse, cross files, instantiate meaning, or produce a reusable cached result must be represented as a semantic subquery.
- Prefer enforcing this boundary as a Rust API/trait split, not only as prose. Navigators should not receive owning semantic query operations directly.

Concrete expectation:

- While navigating `A['c']['full']['bar']`, the navigator may determine that the next hop after `"full"` points at instantiated `C`, but resolving or expanding that instantiated `C` must occur through the shared semantic query layer rather than by private navigator recursion.

## Generic Navigation And Expansion Contract

Generic substitutions are part of semantic meaning and therefore part of semantic query identity. Navigation and expansion operate on instantiated meaning, not on the raw generic declaration body alone.

- Query keys for reusable semantic work must include the relevant type arguments or substitution environment.
- Member projection and indexed access against generic aliases, instantiated types, or mapped helpers must apply substitutions before deciding what member or keyspace is visible.
- If two callers reach the same instantiated semantic node through different entry paths, they converge onto the same cache entry.
- If two callers reach the same declaration name with different type arguments, they do not alias to the same cache entry.
- When a declaration body is navigated in `Navigate` or `Shallow` mode with some type parameters unbound, the resulting graph preserves those parameters as first-class type-parameter references. Clients may later instantiate the preserved graph by applying a substitution environment.
- When a declaration is instantiated with a concrete substitution, every substituted occurrence of a type parameter in the projected result has a `SubstituteTypeParam` edge on the origin layer linking the substituted concrete type back to the parameter's declaration site.
- **Navigation-once invariant.** Navigating `type MyType<T> = …` performs at most one full body lowering per `(decl_identity, whole_hash)`. Later instantiations (`MyType<string>`, `MyType<number>`, …) reuse the parameterized lowering and run only substitution + terminal leaf materialization + branch selection. Distinct concrete instantiations never re-lower the body.
- **Relation-check-once invariant.** For a conditional `T extends S ? A : B`, the relation check runs at most once per distinct check-relevant substitution class. Two instantiations whose substituted check types are identical (e.g., `MyType<string>` called 50 times) run the relation check once.
- **Built-in utilities follow the same semantic model.** `Partial<T>`, `Pick<T, K>`, `Omit<T, K>`, `Record<K, V>`, `ReturnType<F>`, `Parameters<F>`, `Awaited<P>`, `Uppercase`/`Lowercase`/`Capitalize`/`Uncapitalize`, `Extract`/`Exclude`/`NonNullable`, `NoInfer`, and every other built-in utility participate in the same `Instantiate` / `SubstituteTypeParam` / `ProjectMember` / `Normalize` / `ConditionalSelect` / `InferBind` / `AliasResolve` origin model as user-defined aliases and helpers. A utility is not a second semantic class.
- **Utility fast-path rule.** Utility-specific fast paths are permissible only when all three clauses hold: (1) measurably faster than the generic dispatch path on a benchmark fixture, (2) observationally equivalent — same `SemanticNodeId` for the same inputs, same mode / cache behaviour, same `SemanticGraphStats` attribution — to the generic path, (3) emit the **same origin edges** the generic path would have emitted. A fast path that skips edge emission is not allowed; revert to the generic dispatch instead. Optimisation happens only after equivalence is test-enforced.

Concrete expectations:

- Navigating `Box<C>['full']['bar']` resolves `"full"` against the instantiated `Box<C>` meaning, not the unsubstituted generic body.
- If `ProjectMember(ResolveDecl(Box), "full", [C])` is already cached, a later path reaching the same instantiated query reuses it.
- Navigating `MyType<T>` with `T` free returns a shape whose body preserves `T` as a parameter reference. Navigating `MyType<string>` returns a shape with `T` substituted, every substituted position carrying a `SubstituteTypeParam` origin edge to the decl's parameter list.

## Derivation / Origin Layer Contract

The type graph structurally interns nodes — two distinct derivations that produce the same structural result share one arena entry. Origin therefore cannot live inline on each node: the same node may be the result of many derivations. Origin is modelled as a **separate graph layer** of edges from result nodes to their source nodes, co-owned by the `SemanticGraphStore`.

- Origin edges are stored outside the interned node table, in a sibling edge set keyed by `(result_node, edge_kind, source_node)` with optional per-edge metadata.
- A single result node may have multiple origin edges of the same kind from different derivations; the layer MUST support this. Walking from a result returns the full edge set, not one canonical chain.
- Walking is a first-class API on `SemanticGraphStore`, not a private solver internal. External consumers (component-meta compat, LSP hover, error-message rendering) walk origin to present provenance.
- Origin edges are immutable once published; they participate in the same `ReadSetSignature.facts` fact-signature validation as the interned nodes they point at. Cancelled / budget-exceeded / superseded derivations do not publish origin edges.

**Required edge kinds** (names are normative; semantics must not drift):

- **`Instantiate`** — `result = decl<args>`. From the instantiated result back to the declaration identity and concrete argument nodes.
- **`SubstituteTypeParam`** — `result_position = T -> V`. From a concrete type in a substituted position back to the declaration's type parameter and the binding that produced the substitution.
- **`ConditionalSelect`** — `result = select(conditional, True | False | Deferred)`. From the selected-branch result back to the conditional's check, extends, branches, and the deciding relation judgement. Records the branch taken (or `Deferred` if the check stayed open).
- **`InferBind`** — `result = T bound via infer`. From the inferred type back to the `infer` binding site and the concrete type captured by the relation check.
- **`ProjectMember` / `ProjectIndex` / `ProjectPath`** — `result = base.segment(s)`. From the projected result back to the base node and the path segment(s) requested.
- **`Normalize`** — `result = normalize(source_members)`. From a normalized union / intersection / simplified result back to each contributing member node.
- **`AliasResolve`** — `result = unwrap(alias)`. From the unwrapped-target result node back to the alias declaration identity. Emitted once per alias hop (direct alias, re-export alias, barrel alias). Chains are walkable end-to-end so clients can render the full alias provenance.

Edges compose. `ProjectPath(OtherType<string>, ['a','foo'])` produces a node whose origin traces `ProjectMember → ProjectMember → Instantiate → SubstituteTypeParam → ConditionalSelect`. Clients that need derivation-aware display walk the edges and present whichever chain is relevant.

**Three distinct `OriginEdgeKind` taxonomies (do not conflate or reconcile).** The nine derivation kinds above live as `verter_session::semantic_query::OriginEdgeKind`. The audit substrate mirrors them and adds one audit-only kind: `verter_audit::OriginEdgeKind` = the same nine + `SharedLoadReuse` (emitted when a request joins a winner's in-flight artifact via scheduler dedup). The typeinfo **wire** graph uses a SEPARATE 10-arm graph-relationship taxonomy (proto `GraphOriginEdgeKind`: `DECLARES`/`INSTANTIATES`/`REFERENCES`/`MEMBER_OF`/`RESOLVES_TO`/`SHARED_LOAD_REUSE`/`FALLTHROUGH`/`RELATION_PROOF_STEP`/`BACK_EDGE_CYCLE`/`AUGMENTATION_STITCH`) — it is NOT the derivation taxonomy renamed, and only session↔audit is name-isomorphic (modulo `SharedLoadReuse`). The three lists are pinned by `origin_edge_taxonomy_locked` (`crates/verter_session/tests/typeinfo_graph_contract_guards.rs`).

**Typeinfo wire-contract guard surface.** The closed typeinfo wire surface (proto node/symbol/origin/request/error taxonomies, the split env-hash query identity, the closure-bound and schema-version request contracts, and the `AuditedResult` audit carrier) is pinned by a family of static guards under `crates/verter_session/tests/typeinfo_{wire_surface,graph_contract,request_contract,audit_contract}_guards.rs`, all registered under the `Typeinfo Wire Contract` rule in `CRITICAL_RULE_GUARDS`.

**First-class telemetry.** `SemanticGraphStore` exposes `SemanticGraphStats` as a public API. Per-`SemanticQueryKey`-variant counters (cache hits, misses, same-path-sentinel returns, in-flight peak, cross-thread-join wait time) and per-dispatch-builder counters (instantiations, conditional branch selections, budget/fallback invocations, path length p50/p95, projection depth p50/p95, origin edges emitted, origin edges per node p50/p95) are mandatory — not an optional observability pass. The trace-check harness, benchmark pipeline, and feedback-file report at track exit consume `SemanticGraphStats::snapshot()` directly.

## Worked Examples

Normative fixtures. Any solver / dispatch change that breaks an expected behaviour below breaks the contract.

### Example A — basic conditional

```ts
type MyType<T> = T extends string ? StringType : NotStringType;
```

- **Navigate(`MyType`)**: decl identity, parameters `[T]`, body shape `Conditional(check=T, extends=string)`. Do NOT descend into `StringType` / `NotStringType`. Stop at the open conditional shell.
- **Expand(`MyType`)** (T unbound): return a `Conditional` graph with both branches materialized. Shape retains `T`, `string`, `StringType`, `NotStringType`. Origin layer carries `Instantiate(MyType, [T])` and structural links into each branch.
- **Expand(`MyType<string>`)**: conditional is closed. Return only `StringType`. Origin layer carries `Instantiate(MyType, [string])`, `SubstituteTypeParam(T -> string)`, `ConditionalSelect(check=string extends string, branch=True)`. `NotStringType` is NOT walked and does NOT appear in the result.

### Example B — intersection with mixed static and generic members

```ts
type Foo = { foo: number };
type StringType = { a: Foo };
type NotStringType = { a: Foo[] };
type OtherType<T> = { a: Foo, b: T } & (T extends string ? StringType : NotStringType);
```

- **Navigate(`OtherType`)**: decl identity, parameters `[T]`, intersection shell with two arms: object `{ a, b }` and conditional `(T extends string ? … : …)`. Members `a` and `b` visible; `b` marked as a generic-parameter reference. Conditional arm stays a symbolic shell.
- **Expand(`OtherType`)** (T unbound): object arm expands — `a: Foo` fully resolved, `b: T` preserves `T` as a parameter reference (origin links to the `[T]` parameter list). Conditional arm stays `Conditional` with both branches materialized. Intersection is NOT collapsed because the conditional remains open.
- **Expand(`OtherType<string>`)**: object arm expands — `a: Foo`, `b: string` with `SubstituteTypeParam(T -> string)` origin. Conditional arm closes to `StringType`, which expands to `{ a: Foo }`. Intersection collapses to `{ a: Foo, b: string } & { a: Foo }` and normalizes; origin records `Normalize` over both contributing members.

### Example C — `infer` inside a decidable conditional

```ts
type Foo = { a: number };
type OtherFoo = Foo extends { a: infer T } ? T : never;
```

- The conditional is closed: `Foo` is concrete, `{ a: infer T }` is the extends pattern, no free parameters surround the check.
- **Expand(`OtherFoo`)**: the relation engine matches `Foo` against `{ a: infer T }`, binds `T = number` via `InferBind`, selects the True branch, returns `number`. Origin: `ConditionalSelect(True)` + `InferBind(T = number)`.
- **Navigate(`OtherFoo`)** follows the same reduction because `Navigate` reduces closed conditionals. The navigate result is the same `number` node. `infer` does NOT force a stop.

### Example D — path-precise projection

```ts
type Foo = { foo: number };
type StringType = { a: Foo };
type NotStringType = { a: Foo };
type OtherType<T> = { a: Foo, b: T } & (T extends string ? StringType : NotStringType);
```

- **`OtherType['a']['foo']`** (T unbound): project path `['a', 'foo']` through the intersection.
  - Object arm contributes `a: Foo`; project `Foo['foo']` → `number`.
  - Conditional arm is open; distribute `['a', 'foo']` into both branches.
    - True: `StringType['a']['foo']` → `Foo['foo']` → `number`.
    - False: `NotStringType['a']['foo']` → `Foo['foo']` → `number`.
  - Intersect contributing arms. Final: `number`, with origin edges for each contributing path. The `b` field is NEVER touched. `NotStringType`'s non-`a` members are NEVER walked.
- **`OtherType<string>['a']['foo']`**: conditional is closed.
  - Object arm: `Foo['foo']` → `number`.
  - Conditional arm: `StringType['a']['foo']` → `number`. `NotStringType` is NEVER touched.
  - Intersection collapses to `number`.

This is path-precise projection, not whole-branch expansion. Sibling members and unrelated branches are not materialized.

### Example E — nested open conditionals

```ts
type Deep<T> =
  T extends string
    ? (T extends "ab" | "cd" ? { kind: "pair"; value: T } : { kind: "str"; value: T })
    : { kind: "other"; value: T };
```

- **Expand(`Deep`)** (T unbound): outer check is open. Keep outer `Conditional` shell. In the True branch, inner check is also open; keep inner `Conditional` shell. In the False branch, materialize `{ kind: "other"; value: T }` with `T` preserved.
- **Expand(`Deep<"ab">`)**: outer check `"ab" extends string` closed → True. Inner check `"ab" extends "ab" | "cd"` closed → True. Return `{ kind: "pair"; value: "ab" }`. Origin chain: `Instantiate(Deep, ["ab"])` → `SubstituteTypeParam(T -> "ab")` → `ConditionalSelect(outer=True)` → `ConditionalSelect(inner=True)`.
- **Expand(`Deep<"xx">`)**: outer closed → True. Inner `"xx" extends "ab" | "cd"` closed → False. Return `{ kind: "str"; value: "xx" }`. Origin: same outer chain, `ConditionalSelect(inner=False)`.

> Template-literal pattern matching (e.g. `T extends \`${infer _}${infer _}\``) is a future relation-engine extension and is **not** part of this contract's normative fixtures. The nested-conditional semantics above apply uniformly once template-literal infer support lands — adding it does not require a contract revision.

### Example F — contributors-only union / intersection combining

```ts
type A = { a: number; x: string };
type B = { a: string; y: boolean };
type C = { z: number };
type AB = A | B | C;
```

- **`AB['a']`** projection: `A` contributes `number`, `B` contributes `string`, `C` does NOT contribute (no `a`). For a union, every member must contribute to the path; `C`'s miss makes the union projection a miss as a whole. (Contrast: if `AB` were `A | B` only, the result would be `number | string`.)
- For intersection analogs (`A & B & C`): `A` contributes `number`, `B` contributes `string`, `C` does not contribute. Contributing arms' projection: `number & string` → `never` via intersection normalization. `C` is ignored for the path; the `never` arises from the contributors' intersection semantics, not from `C` being rewritten.

## Shallow File State and Frontier Engine

Cross-file type resolution for macros (`defineProps<T>()`, component-meta, etc.) is built on two shared primitives in `verter_session::resolver_core`:

**ShallowFileState** (`shallow_file_state.rs`) is the authoritative shallow symbol/export surface for one imported type file. Keyed by `(canonical_id, whole_hash)`. Contains:
- `exports` map (exported name -> `ExportTarget`: Local or Reexport)
- `wildcard_reexports` (`export * from` sources, in declaration order)
- `symbols` (all locally-declared type symbols with raw body, type params, local deps, external deps)
- `import_locals` / `import_targets` (import classification for closure)

Populated once through the shared host ensure-path and cached in `FileArtifactStore`. Invalidated when the file's whole-hash changes.

**ExternalTypeFrontier** (`external_type_frontier.rs`) is the single BFS engine for all cross-file type deepening. Level-by-level traversal:
1. Seed with initial `(canonical_id, exported_name)` pairs
2. For each pending symbol: load `ShallowFileState` via `FrontierHost` trait, route the export (direct > alias > wildcard in declared order), run local closure
3. Collect `ExternalSymbolRef` entries from unresolved external deps into the next level
4. Dedup on `(canonical_id, exported_name)` across the entire request via `seen` set
5. Repeat until frontier is empty or budget is exceeded

**Barrel BFS contract:** when a pending file is a barrel and the requested symbol is not found in that file's shallow surface, process that file's wildcard barrel children as one BFS layer. Shallow every child in the layer before choosing any deeper barrel grandchildren. A barrel child that does not expose the requested symbol at its own shallow surface may contribute its wildcard children to the next layer, but must not trigger immediate depth-first descent.

**Local closure** (`ShallowFileState::local_closure()`) resolves same-file transitive deps iteratively. Uses a visited set for cycle handling (revisited nodes are silently skipped). Never crosses import boundaries -- external deps become `ExternalSymbolRef` for the frontier.

**Budget contract** -- three domains with high ceilings (safety rails, not normal control flow):
- `local_closure_steps`: 500 (same-file symbols per closure)
- `frontier_symbol_visits`: 2000 (cross-file `(canonical_id, exported_name)` pairs)
- `builder_expansion_steps`: 5000 (symbolic expansion steps)

When a budget trips, the system returns a structured `BudgetExceededFailure` with domain, limit, actual count, and context -- never silently normalizes.

**Host integration**: `HostFrontierAdapter` (`host_resolve.rs`) bridges the frontier to the real `VerterHost`, resolving through `FileArtifactStore` for per-file facts, `RouteDb`/`ImportedRootDb` for cross-file routing, and workspace fallback for cold misses. Route discovery runs exclusively through the frontier/final-target path; once the defining symbol is selected, the shared source-body evaluator materializes the final `ResolvedElements`.

**Key files:**

| File | Purpose |
| --- | --- |
| `crates/verter_session/src/resolver_core/shallow_file_state.rs` | ShallowFileState, ExportTarget, ShallowTypeSymbol, ExternalSymbolRef, ResolutionBudgets, local_closure() |
| `crates/verter_session/src/resolver_core/external_type_frontier.rs` | ExternalTypeFrontier, FrontierHost trait, PendingExternalSymbol, ResolvedSymbol, RouteKind |
| `crates/verter_session/src/host_resolve.rs` | HostFrontierAdapter, resolve_external_type_from_loaded_files() |
| `crates/verter_session/src/frontier_tests.rs` | Behavioral invariant tests (diamond dedup, barrel ordering, cycle termination, budget enforcement, etc.) |

## Semantic Dispatch (Post Phase-D authority)

**Plan §2 architectural decision:** `ProjectSemanticDispatch` + `SemanticGraphStore` are the canonical lazy semantic layer and the sole authority for every reusable type-resolution operation. The former `verter_semantic::analysis::type_solver` walker (`resolve_node`, `resolve_indexed_access`, `resolve_conditional`, `collect_structural_property_descriptors_inner`, etc.) demotes to **per-request scratch for TypeExpr lowering and Vue macro parsing only** — it no longer serves as a reusable semantic authority.

The reusable semantic layer lives in `crates/verter_session/src/project_semantic_dispatch/` (module tree split in Phase D §5.2). Each sub-module owns a distinct responsibility:

- `mod.rs` — `ProjectSemanticDispatch` struct + `SemanticQueryApi::execute` impl
- `build.rs` — `build_instantiate`, `build_mapped_type`, `build_conditional`, `build_key_of`, `build_project_path` (Phase 1B path-prefix peek + linear-step backfill, plan §1.B), `build_typeof`, `build_builtin_utility`
- `walk.rs` — `PathWalker` + `walk_path` (iterative worklist per plan §2)
- `guards.rs` — `SubstitutionGuard`, `EvaluationGuard`, `WalkGuard`, `KeyEnumerationGuard`, `RelationGuard` (per-call RAII cycle detection)
- `enumerate.rs` — `KeyEnumeration`, `EnumeratedKey`, `key_names_*` helpers
- `relation.rs` — `relate_nodes` + full relation engine (plan §2 Relation engine)
- `lower.rs` — `shallow_lower_type_expr` (TypeExpr → `SemanticNodeId`)
- `substitute.rs` — `substitute_semantic_type_param` (guarded)
- `evaluate.rs` — `evaluate_deferred_semantic_node` (guarded)
- `origin.rs` — origin-edge emitters

`SemanticGraphStore` (crate `verter_session::semantic_query_memo`) owns all reusable semantic identity via two parallel memos:

- **Node memo** — mode-erased `FamilyKey` → `FamilySlots` map for single-node queries (`ResolveDecl`, `Instantiate`, `KeyOf`, `MappedType`, `Conditional`, `ProjectPath`, `TypeOf`, `NormalizeUnion`, `NormalizeIntersection`, `ResolvedNamedType`).
- **Relation memo** — keyed by the full-identity `RelateMemoKey` (source / target / relation kind / policy / source freshness / inference context / env+substitution+projection-reduction context) for `Relate` judgements (plan §2). `RelationResult` is `{ Assignable { bindings }, NotAssignable, Unknown }`; all three cache-with-fence.

**Canonical deferred forms** (plan §2 — only these variants cross any cache boundary):

- `SemanticNodeData::Mapped { source, mapper }` — deferred mapped type
- `SemanticNodeData::Conditional { check, extends, true_branch_ref, false_branch_ref, distributive }` — deferred conditional
- `SemanticNodeData::IndexedAccess { object, index }` — deferred indexed access
- `SemanticNodeData::KeyOf { base }` — deferred keyof
- `SemanticNodeData::TypeOf { value_root, path }` — deferred typeof
- `SemanticNodeData::TypeParam { name }` — open type parameter reference
- `SemanticNodeData::Alias(target)` — alias identity preservation (target must be `DeclAnchor`-identifiable — plan Change B)
- `SemanticNodeData::Opaque(err)` — genuine projection miss or structured budget failure
- `SemanticNodeData::Function { params, return_type, type_parameters }` — function shape (plan §2 "the only new variant" — added in §5.6 WIP-L; class/interface lower to `Object` with heritage merged)

All surrogate encodings are retired: `Alias(KeyOf(source))` (replaced by canonical `Mapped` shell with `KeyEnumeration::Unresolvable` branch), arena `Node::Error { description }` used as a materialisation sentinel, `Primitive(Undefined)` returned from failed solver projections.

**No hard caps in the semantic layer.** Three legacy caps deleted or reclassified:

- `evaluate_deferred_semantic_node`'s `for _ in 0..32` — DELETED (§5.3 WIP-R); replaced by stack-local `EvaluationGuard` cycle detection.
- `PathWalker::max_depth = 64` — DELETED (§5.3 WIP-R); replaced by `WalkGuard` cycle detection.
- Parser-level `MAX_TYPE_RESOLUTION_DEPTH = 64` — RENAMED (§5.10 WIP-P) to `PARSER_SYNTACTIC_DEPTH_LIMIT = 256` and documented as syntactic stack-safety, not a semantic budget.

**Bounded-loop annotation convention.** Any bounded loop in `crates/verter_session/src/project_semantic_dispatch/` MUST be annotated `// bounded-loop: <reason>` on the preceding line. The only currently-approved reason is `fence-retry` (CLAUDE.md completion-fence 3-retry rule).

**Recursion guard contract.** Per-call `in_flight: FxHashSet<SemanticNodeId>` + RAII pop + completion memo. Stack-local; dies with the call; NOT a host-owned cache:

| Function | Cycle sentinel | Publishable | Publication surface |
| --- | --- | --- | --- |
| `substitute_semantic_type_param` | input node unchanged | yes | Caller's `SemanticQueryKey` memo |
| `evaluate_deferred_semantic_node` | current node (fix-point) | yes | Caller's `SemanticQueryKey` memo |
| `walk_path` | `Opaque(QueryError::AliasCycle { chain })` | yes | Originating `ProjectPath` memo |
| `key_names_from_base_node` / `_keyspace_node` | `KeyEnumeration::Unresolvable` | no (Rust-local) | Caller publishes canonical `Mapped` shell |
| `relate_nodes` | `RelationResult::Unknown` | yes (cache-with-fence) | `RelationMemo` entry with dep-fence |

**Parser → semantic graph integration.** No new adapter struct. The existing `HostNamedTypeCacheAdapter` in `crates/verter_session/src/host_manage.rs` (implements `verter_parser`'s `NamedTypeCache` trait) reads/writes `SemanticGraphStore` directly via `get_resolved_named_type` / `insert_resolved_named_type` and drives deep type reduction through `ProjectSemanticDispatch::execute`.

**Authority-uniqueness contract** (normative, mechanically enforced by §6.5 gate tests):

- A second `impl SemanticQueryApi for ...` — FORBIDDEN (besides `ProjectSemanticDispatch`).
- A second `fn relate_nodes` — FORBIDDEN (besides `project_semantic_dispatch::relation`).
- A second `fn shallow_lower_type_expr` — FORBIDDEN (besides `project_semantic_dispatch::lower`).
- A second struct owning a `RelationMemo` field — FORBIDDEN (besides `SemanticGraphStore`).
- A second struct owning the semantic node map — FORBIDDEN (besides `SemanticGraphStore`).

## Legacy Native Type Solver (demoted to per-request scratch)

The arena-based `verter_semantic::analysis::type_solver` that previously owned reusable type expansion is demoted to per-request TypeExpr lowering scratch + Vue macro parsing only. Its `TypeSolverHost` trait, `EvalEnvSolverHost` struct, `SessionSolverHost` struct, and `TypeQueryEngine` struct are scheduled for deletion in Phase D §5.8 WIP-W. Call-sites that previously used those APIs migrate to `ProjectSemanticDispatch::execute(SemanticQueryKey::...)` per plan §9 appendix.

Historical pipeline (retained for TypeExpr → arena lowering only): `TypeExpr -> lower -> QueryArena -> project_to_type_expr -> TypeExpr`.

**Request-scoped engine ownership:** `TypeQueryEngine` is the single request-scoped mutable solver owner for component-meta queries. One engine is created per `get_component_meta()` request and shared across all solves in that request. Declaration-scoped solves reuse the shared engine via `TypeQueryEngine::solve_scoped()` -- they share the arena, instantiation cache, and solver caches while using a different `TypeSolverHost` (scoped to the declaration file). The `solve_scoped` method partitions the op-cache key and bare-name root_identity cache by `scope_canonical_id` so results from one declaration scope do not alias with results from another scope. Do not construct fresh `TypeQueryEngine` instances for declaration-scoped solves.

**Scope-aware bare-name caching:** The `SolverCaches.root_identity` cache uses `(canonical_id, symbol_name)` as its key. For bare-name lookups (empty `canonical_id`), `resolve_root_identity_cached()` in `solve.rs` substitutes the `SolveState.scope_canonical_id` as the cache key. This prevents cross-scope poisoning: a bare-name miss in scope A does not prevent scope B from resolving the same name.

**Ambient/global environment (deferred):** Names like `Function`, `Promise`, `ThisType`, and DOM globals currently fall through as unresolved bare-name misses. The long-term target is explicit ambient/global declaration support as a first-class input to the host/engine boundary, modeled from the project's TypeScript configuration (`compilerOptions.lib`). This is deferred correctness work, not a speculative enhancement -- it remains a required follow-up in the same architectural track as the shared-engine cutover.

**Declaration context propagation:** `PreparedTypeDecl` and `PreparedValueDecl` carry a `name_resolution: FxHashMap<String, ResolvedRootIdentity>` field that maps bare names appearing in their bodies to resolved root identities. Built at preparation time from the defining file's local and import scope (local deps -> same-file identity, external deps -> resolved canonical_id via dep_edges). The solver's `SolveState` maintains `type_decl_context_stack` and `value_decl_context_stack`. When `resolve_prepared_ref` enters a declaration body, it pushes the prepared decl onto the stack. The `resolve_name_in_context` helper checks only the INNERMOST context (topmost stack entry) -- bare names in an imported type body resolve in that declaration's defining file scope, not in parent scopes.

**Barrel re-export following:** `prepared_type_decl` and `prepared_value_decl` follow barrel re-exports when a symbol is not found in the target file's local prepared decl cache. For named re-exports (`export { Foo } from './bar'`), the source specifier is resolved and the lookup continues in the target file. For wildcard re-exports (`export * from './bar'`), all wildcard sources are tried in declaration order with depth-limited recursion (max 20 hops).

**Namespace import resolution:** `SessionSolverHost::root_identity` handles dotted names (`Ns.Member`) by splitting on the first dot, resolving the prefix through import bindings, and looking up the member in the resolved file's prepared decl cache.

**Exactness model:** `ExactConcrete | ExactSymbolic | Incomplete` -- replaces old `Exact | LowerBound | OpaqueFallback`. Execution status (`Completed | Cancelled | HardStop`) is tracked separately from semantic exactness.

**Operators implemented:** keyof, indexed access, conditionals (with distributive distribution + infer binding collection), mapped types (with key remapping via `as` clause), template literals (iterative cartesian expansion with 10k guard), typeof, built-in utilities (Partial, Required, Readonly, Pick, Omit, Record, Extract, Exclude, NonNullable, ReturnType, Parameters, ConstructorParameters, InstanceType, Awaited, Uppercase, Lowercase, Capitalize, Uncapitalize, NoInfer).

**Parser-level `TypeResolutionContext`:** `TypeResolutionContext` (in `verter_parser::utils::oxc::vue::script::resolve_type`) no longer carries a per-context resolved-types cache or a recursion-depth `Cell`. Memoization is delegated to an injected `Arc<dyn NamedTypeCache + Send + Sync>` (trait in the public `verter_parser::utils::oxc::vue::resolve_type::cache_keys` module, alongside `ResolvedNamedTypeCacheKey` and `ResolvedTypeParamBindingCacheKey`). Recursion depth is tracked via a module-local `thread_local!` RAII guard; the `MAX_TYPE_RESOLUTION_DEPTH = 64` bound is enforced at each recursive call site. The host-owned cache lives inside `SemanticGraphStore` (accessed via `ProjectTypeStore::semantic_graph()`): entries are stored as `SemanticNodeData::VueMacroElements(Arc<ResolvedElements>)` behind a `DashMap<HostResolvedNamedTypeKey, SemanticNodeId>` identity map. The `HostNamedTypeCacheAdapter` (in `verter_session::host_manage`) calls `SemanticGraphStore::get_resolved_named_type` / `insert_resolved_named_type` directly on the hot path — reads are refcount-only (one `DashMap::get` + one arena read + one `Arc::clone`). Entries survive across requests within one workspace generation; they are cleared by `clear_thread_local_parsed_eval_program_cache` (which calls `semantic_graph().clear_resolved_named_types()` on `bump_store_view_epoch`) and per-canonical via `ProjectTypeStore::evict_canonical`. A debug-mode validation feature `parser_cache_audit` (not yet enabled by default -- deep `PartialEq` impls on `ResolvedElements`/`ResolvedProp`/`ResolvedEmit` are in place to support it) will re-run the slow-path resolver on cache hits and assert equality to lock in cache-key sufficiency.

**Key files:**

| File | Purpose |
| --- | --- |
| `crates/verter_semantic/src/analysis/type_solver/mod.rs` | Module root, re-exports |
| `crates/verter_semantic/src/analysis/type_solver/arena.rs` | QueryArena (node store) + SolverCaches (memo tables) |
| `crates/verter_semantic/src/analysis/type_solver/solve.rs` | Top-level `solve_type()` entry, `resolve_node`, operator resolution |
| `crates/verter_semantic/src/analysis/type_solver/relate.rs` | Tri-state relation engine (zero-clone, reads via `&QueryArena`) |
| `crates/verter_semantic/src/analysis/type_solver/host.rs` | `TypeSolverHost` trait, `ResolvedRootIdentity`, `EvalEnvSolverHost` |
| `crates/verter_semantic/src/analysis/type_solver/lower.rs` | `TypeExpr -> NodeId` lowering |
| `crates/verter_semantic/src/analysis/type_solver/prepared.rs` | `PreparedTypeDecl`, `PreparedValueDecl` |
| `crates/verter_semantic/src/analysis/type_solver/builtin.rs` | Built-in utility implementations |
| `crates/verter_semantic/src/analysis/type_solver/project.rs` | member/keyspace/surface projections |
| `crates/verter_semantic/src/analysis/type_solver/recursion.rs` | Tarjan SCC + RecursionTracker |
| `crates/verter_session/src/resolver_core/solver_host.rs` | `SessionSolverHost` (bridges host caches to solver) |
| `crates/verter_session/src/resolver_core/type_expansion_verter.rs` | `resolved_macro_to_expansion_via_solver()` (component-meta integration) |

**Cutover status:** The solver is the sole type expansion authority. The legacy evaluator body (`EvalLookup`, `evaluate_with_lookup()`, `ImportedEvalLookup`, `ImportedDeclEvalResolver`, `ExpansionBudget`, budget retry logic) and the legacy `ImportedEval*` trait hierarchy (`ImportedEvalInputs`, `ImportedEvalResolver`, `OwnerEvalEnvAssembler`, walker-based import pre-loading) have been fully deleted. `type_eval.rs` now contains only symbol table types (`TypeDeclInfo`, `EvalEnv`, `TypeExpr`, etc.) and a convenience `evaluate()` function that delegates to `solve_type()` via `EvalEnvSolverHost`. The `type_expand/` module retains only `expand_macro_types()`, `expand_object_shape()`, and `expand_normalized_expr()` -- all of which take `&dyn TypeSolverHost` instead of the deleted budget/lookup parameters.

## Type Evaluation Symbol Tables

`verter_semantic::analysis::type_eval` contains the shared type-representation types and symbol tables used by the solver and analysis layers. The legacy evaluator body has been deleted -- all evaluation now goes through `type_solver`.

- `TypeExpr` -- recursive Arc-backed type representation (`Arc<TypeExpr>`, `Arc<[TypeExpr]>`, `Arc<ObjectExpr>`, `Arc<FunctionExpr>`). Clones stay shallow.
- `EvalEnv` -- per-file symbol table: `type_bindings` (generic params), `type_symbols` (named declarations). Stores `Arc<TypeExpr>` so generic instantiation does not re-copy subtrees.
- `TypeDeclInfo` -- declaration metadata (body, type params, span).
- `evaluate()` -- convenience wrapper that delegates to `solve_type()` via `EvalEnvSolverHost`. No longer contains evaluation logic itself.

## Declaration Merging (CRITICAL)

Same-name TypeScript declaration merging is owned end-to-end by the shared layer; there is exactly ONE merge path.

**Carrier chain (owner → reducer):**

- `EvalEnv.type_symbols`/`value_symbols` are `FxHashMap<String, TypeDeclGroup>`/`ValueDeclGroup` — ordered contributor groups. `add_type`/`add_value` APPEND (`group.contributors.push`), never last-wins `insert`. `TypeDeclGroup::merged_body()` returns `TypeDeclBody::Merged { contributors, kinds }` for any ≥2-contributor group whose contributors are ALL `interface` or `class` (`interface`+`interface` AND `interface`+`class` fold — the interface members augment the class INSTANCE type; the class value/static/constructor side stays on a separate value declaration). Every other group — a single contributor, or any group containing a type `alias` (a duplicate-identifier error in TS) — returns `TypeDeclBody::Single` (the last-wins representative). `ValueDeclGroup::merged_signatures()` concatenates every contributor's signatures in source order.
- `ShallowTypeSymbol.body: TypeDeclBody`. Shallow same-file member readers consult `body.lookup_object()` (a member-union `Object` projection — an index view, NEVER an `Intersection`; it descends a heritage-carrying contributor's intersection arms to collect that contributor's OWN members). `PreparedTypeDecl` carries `merged_contributors: Vec<TypeExpr>` (empty = single).
- Body lowering (`lower_decl_body_with_provenance`) interns a merged interface as a distinct `SemanticNodeData::MergedDecl { contributors: Arc<[SemanticNodeId]> }` carrier; a single declaration lowers exactly as before.

**The load-bearing rule:** a merged declaration MUST reach the reducer as `MergedDecl`. A bare `TypeExpr::Intersection`/`SemanticNodeData::Intersection` is FORBIDDEN as the merged-decl representation — the intersection reducer applies own-body-shadows-heritage member precedence and cannot accumulate method overload groups (a same-named method in a later arm SHADOWS the earlier). `verter_session` must not synthesise `raw_body = TypeExpr::intersection(...)`.

**Peer-merge reducer** (`reduce_merged_decl_with_graph` + `merge_declaration_surfaces`, in `project_semantic_dispatch::walk`; routed through raise / expand / keyof / relation / substitute): (a) same-name methods/call-signatures ACCUMULATE into one ordered overload group (member value = ordered `Intersection` of the per-contributor function nodes); (b) conflicting non-method properties take deterministic first-contributor precedence (never `never`); (c) distinct members union; call/construct/index signatures concatenate. **`extends`/`implements` heritage is PRESERVED:** each contributor is split into its own-body surface(s) and its heritage `Ref` arms (a contributor lowers to `Intersection([heritage Ref…, own Object])`); own bodies peer-merge, then the reducer re-emits `Intersection([heritage…, merged_own_Object])` so the existing heritage-overlay path resolves inherited members lazily (own members shadow inherited same-name members). Dropping the heritage arms would lose inherited members from a `interface X extends Base {…}` + `interface X {…}` merge.

**One merge engine, two consumers — display is non-mutating.** The peer-merge precedence lives in a single pure helper `merge_declaration_surfaces_core` (member union + first-contributor precedence + ordered method-overload accumulation, NO interning, NO own-body-shadows-heritage branch — that shadowing is the intersection reducer's job over real heritage arms). `merge_declaration_surfaces` calls the core then interns the overload groups + `Object`; the `display()` projection (`reduce_merged_decl_display_surface`) calls the SAME core and renders the result WITHOUT interning, so rendering a `SemanticNodeData::MergedDecl` leaves `node_count` unchanged and is byte-identical to the canonical reduced surface (an accumulated overload renders as the property-intersection `m: (..) & (..)`, exactly as the reduced `Object` does — never as separate method signatures). Display must never call the interning reducer or dispatch (guard: `display_source_does_not_call_graph_interning_or_dispatch`).

**Overload visibility** is a projection-time rule (`build_typeof`): a lone signature is visible (even if bodied); a multi-signature group surfaces every bodiless overload in source order and HIDES the trailing implementation (`has_implementation_body == true`).

**Versioning:** same-file merged values root on the owner's single `FileWholeHash` self-root under a content-free query-identity key (R6) — no dedicated contributor-sequence fact. Cross-file ambient augmentation (`declare module`/`declare global`) is a separate concern documented under **Declaration Augmentation (CRITICAL)** below — it reuses this same `MergedDecl` peer-merge path, it is not a second merge engine.

**Guards** (registered in `critical_rules_have_guards.rs::CRITICAL_RULE_GUARDS`): `eval_env_type_symbols_are_grouped_not_last_wins_map`, `eval_env_add_decl_appends_not_overwrites`, `no_intersection_merge_synthesis_in_verter_session`, `merged_decl_lowers_to_distinct_carrier_not_intersection`, plus the discriminating `declaration_merge_facts` regression and the `declaration_merge` typeinfo oracles.

**Merge/augmentation WIRE domain (architecture decision).** The wire representation of a merged declaration / module augmentation is ALREADY modelled inside `GraphTypeNode`: kinds **21–25** (`GraphMergedDeclaration` `{merged_symbol, repeated GraphDeclarationPart parts}`, `GraphAmbientModule`, `GraphModuleAugmentation`, `GraphAmbientNamespace`, `GraphGlobalAugmentation`), with decl anchors as nested `GraphDeclarationPart`. The LIVE carrier behind that wire surface is the graph node `SemanticNodeData::MergedDecl { contributors }` — reduced to an `Object` / overload surface and emitted as a `GraphTypeNode`. The `SemanticQueryValue::DeclarationAnalysis(DeclarationAnalysisValue)` value variant is a **non-live shell** with NO producer and is NOT the wire carrier (its rustdoc points back here). A proposal to relocate merge to a distinct `DeclarationAnalysisGraph` wire message is **rejected**: the correct home already exists in the closed contract, so adding the message would be dead duplicate surface (new tags + TS bindings + validation + a `schema_version` bump + permanent compat). A structured-merge-graph producer (actual `GraphMergedDeclaration` emission) lands ONLY together with its first real FFI/TS consumer, as the existing kind 21 — never as a separate message. Do not re-flag "missing wire-proto relocation" as an incomplete deliverable.

## Declaration Augmentation (CRITICAL)

Ambient declaration augmentation (`declare module "X" { ... }` / `declare global { ... }`) is a RETAINED, addressable scoped inventory — never fingerprint-only facts and never file-scope pollution.

**Typed inventory (the single source of truth).** The binder retains every augmentation-block inner declaration on `EvalEnv`, keyed by `(AugmentationScopeKind {Global, Module(specifier)}, name)`:

- `EvalEnv.augmentation_scopes` → ordered `TypeDeclGroup` (interfaces / type aliases), mirrored on `ShallowFileState.augmentation_scopes` as `ShallowTypeSymbol`.
- `EvalEnv.augmentation_value_scopes` → ordered `ValueDeclGroup` (`const`/`let`/`var`, `function`, `class`), mirrored on `ShallowFileState.augmentation_value_scopes` as `ShallowValueSymbol`.

Inner declarations NEVER enter file-scope `type_symbols`/`value_symbols`. Parse-domain `ModuleAugmentationFact`s are DERIVED from this typed inventory (`fact_emission::collect_augmentations`) — one fact per `(scope, name)` carrying the augmented name, its symbol space, and a content-sensitive shape fingerprint. There is NO raw-source byte-scan (Build Philosophy: no stage rescans raw source to rediscover what shallow processing captured — guard `shallow_walk_invariant::fact_emission_reads_only_shallow_state_never_raw_source`). There is no `ScopeId.kind`/`semantic_query::ScopeKind` — the live addressing is `AugmentationScopeKind` alone.

**Cross-file augmentation merge is the SAME `MergedDecl` peer-merge path as same-file merging — NOT a second merge engine.** When a declaration `(canonical, name)` is instantiated, `stitch_module_augmentations` (in `project_semantic_dispatch::build`) finds every augmenter file via `FileArtifactStore::ensure_augmentation_index_populated`, fetches each augmenter's RETAINED inner body from the typed `augmentation_symbol(Module(spec), name)` inventory (typed-IR only — never a source/byte scan in the resolver), lowers it in the augmenter's own file context through `prepare_augmentation_type_decl` + `lower_decl_body_with_provenance`, and folds base ∪ augmenter contributions into ONE `SemanticNodeData::MergedDecl` carrier (flattening the base body if it is already a `MergedDecl`). Augmenter order is the stable `(canonical, parse_stable_hash)` key — discovery-order-independent. Relative-augmenter discovery loads the base's `reverse_deps_for` before the index scan.

**Overlay-aware index.** `AugmentationTargetKey.population: AugmentationPopulation {Base, Session(u64)}`. A `Base` scan reads `is_legacy()` artifacts only; a `Session` scan reads the session's overlay (non-legacy) artifacts (matched by the session overlay discriminator) UNIONED with base, keyed by the overlay-set fingerprint. **`Session(u64)` is the overlay-set fingerprint, NOT a raw session id** — both producers (the body stitch and `RouteDb::get_or_compute_effective_export_set`) derive `(population, overlay_discriminator)` through the SINGLE shared `session_view::augmentation_population_for_view`, so a session view can never be cached as a base-only set under a session key. Overlay augmenters NEVER poison the base index and NEVER cross sessions. There is NO base-only `assert!(view.compat_token().session.is_none(), …)` on the augmentation-index / `EffectiveExportSet` surface — a session view is accepted under `Session` population.

**Cross-file FACTS** reuse `get_or_compute_effective_export_set`'s rail: the cold stitch observes one `FactKey::ModuleAugmentationIndexShape` (the augmenter-set fingerprint) plus one `FileWholeHash` per contributing file (base ∪ augmenters), and records `self_root_canonicals = {base} ∪ {augmenters}`. A content edit to ANY contributor misses the warm read; torn/partial routes through `ReturnOnly`, never warmed. Query keys stay content-free (R6).

**Guards** (registered in `critical_rules_have_guards.rs::CRITICAL_RULE_GUARDS`): `session_overlay_augmenter_isolated_from_base_index`, `effective_export_set_session_view_stitches_overlay_augmenter`, `no_effective_export_set_base_only_session_assert`, plus the e2e overlay-isolation oracle.

## Cross-File Type Resolution (Compiler Integration)

External types for macros like `defineProps<ExternalType>()` are pre-resolved by the host:

1. Host detects type dependencies from imports
2. Host resolves types from its file store
3. Host passes resolved types via `VerterCompileOptions::external_types`
4. `script/process.rs` merges external types with companion `<script>` types

The Rust compiler never does file I/O -- all external resolution is the host's responsibility.

**Shallow file state and type solver integration:** `ShallowFileState` (authoritative shallow symbol/export surface per imported file, keyed by `(canonical_id, whole_hash)`) and `ExternalTypeFrontier` (single BFS engine for cross-file type deepening, level-by-level with dedup and export routing) are the shared cross-file resolution primitives. Local closure runs same-file deps iteratively without crossing import boundaries. Three budget domains (local_closure=500, frontier=2000, builder=5000) act as safety rails with structured failure. The frontier backs shared shallow-state reuse, merge-root traversal, and final external-type body production via `SessionSolverHost` and the native type solver. See `resolver_core/shallow_file_state.rs`, `resolver_core/external_type_frontier.rs`, `resolver_core/solver_host.rs`, and `host_resolve.rs` (`HostFrontierAdapter`).

All type expansion (macro types, component-meta, imported type aliases) goes through the native `type_solver::solve::solve_type()` pipeline. The old lightweight evaluator (`evaluate_with_lookup`, `EvalLookup` trait, `evaluate_inner`) has been fully removed from `type_eval.rs`. That module now contains only symbol table types (`TypeDeclInfo`, `EvalEnv`, `ValueDeclInfo`, `FunctionSignature`, `EvalLimits`) and a convenience `evaluate()` function that delegates to the solver via `EvalEnvSolverHost`. Expansion functions (`expand_object_shape`, `expand_normalized_expr` in `type_expand/mod.rs`) take `&dyn TypeSolverHost` and delegate to `solve_type()`. The old `ExpansionBudget` type and budget-retry logic have been removed -- the solver uses its own `SolveLimits`.

## Macro Type Traversal Rule

When resolving cross-file macro types (`defineProps<T>()`, `defineEmits<T>()`, and other shared host-backed queries), only follow the import graph reachable from the requested type's declaration graph.

- There is one shared cross-file type resolver. Consumer-specific ownership rules live in `/component-meta`.
- The resolver has exactly five query modes (see "Query Mode Contract" above):
  - `Identity`: declaration identity and canonical source location only. No body read, no shape materialization.
  - `Navigate`: minimum semantic work needed to continue a requested path. Intermediate hops run in this mode.
  - `Shallow`: one surface level of the requested node without recursive expansion.
  - `Expanded`: recursive materialization of the requested result.
  - `Skeleton`: specialised generic-helper/cycle traversal that keeps unbound type parameters as shells.
- `Skeleton` is a distinct mode, not a synonym for `Navigate`. It is currently scoped to cycle/generic-helper traversal such as `ref_root_reaches_transitive_cycle_node`'s BFS step. New call sites must justify why they need Skeleton semantics instead of `Navigate` / `Shallow`.
- Do not introduce ad hoc navigate/shallow flags; use the canonical modes and the path-precise projection surface.
- Do not walk unrelated imports from the same file.
- Do not treat plain imports as implicit exports.
- Keep direct re-exports (`export { X } from`, `export * from`) as an explicit separate path.
- Parsing a `.ts`/`.js`/declaration file for type resolution must cache discovered symbol name -> canonical location mappings.
- Re-exported names and barrel hops must also be cached once discovered. If traversal follows `export * from './foo'`, cache that result so later lookups do not rescan the same barrel chain.

If a file imports 20 modules but the requested macro type only references `AvatarProps` and `IconProps`, external resolution must only traverse those reachable dependencies.

**TS-first resolution priority:** TypeScript types always take priority over JavaScript files when resolving ambiguous dependency candidates. Verter is a type-strict compiler that relies on TS typing for correctness. JS files should only be used as a last resort when no TS type definition is available. When `DependencyResolution.possible_canonical_ids` contains multiple candidates, use `effective_target()` which selects the single highest-priority candidate: `.d.ts` > `.d.cts` > `.d.mts` > `.ts` > `.tsx` > `.js` > `.jsx` > `.cjs` > `.mjs`. Do not try remaining candidates if the selected one lacks the needed type -- treat as not found.

**Owned resolution is bounded by `workspace_root`:** For owned and project-scoped resolution, `node_modules` and package `#imports` ancestor walks stop at `IdeProjectConfig.workspace_root`. In monorepos, `workspace_root` may be above `project_root` to reach hoisted `node_modules`. In compat `createCheckerByJson()`, `workspace_root == project_root`. Unowned resolution (no owning project) remains unbounded. The boundary is passed via `ancestor_dirs(path, Some(&workspace_root))` and `ancestor_dirs_from_dir(start_dir, Some(&workspace_root))` in `verter_workspace::resolver`.

## Typed-IR-Only Resolver Rule (CRITICAL)

The native component-meta / typeinfo resolver pipeline drives every semantic decision from the typed IR (`TypeExpr` on the Rust side, `TypeDescriptor` from `@verter/type-ir` on the TS side). Source slicing, regex against type text, hand-rolled type-text splitters, `starts_with("Pick<")` shape sniffing, the synthesise-then-reparse pattern (`format!(...).parse_type_annotation(...)`), and `path.contains("/node_modules/")` classification are all forbidden inside that pipeline.

- OXC AST is lowered exactly once during shallow analysis via `lower_ts_type(ts_type, source)` (in `verter_semantic::analysis::type_expr_lower`). The analyzer takes the OXC `TSType` AST node it already has in scope; downstream stages walk the resulting `TypeExpr`.
- `parse_type_annotation` is reserved for JSDoc tag-type payloads (`{Type}` text inside `@type`/`@param`/`@returns`). Calling it from the resolver / projector / registry / policy / materialiser / compat pipeline is the bug.
- Raw display strings (`Analyzed*Field.type_annotation`, `ExpandedField.raw_type`, `ResolvedLocalType.expanded`, `PropMeta.rawType`) are display passthroughs only. Resolver and compat consumers MUST NOT parse them back into `TypeExpr` / `TypeDescriptor`.
- Workspace classification uses `ResolverContext::workspace_is_workspace_owned(canonical_id)` / `workspace_is_package_backed(canonical_id)`. Substring checks on canonical paths (`"/node_modules/"`, `"\\node_modules\\"`) are banned. The classification API is path-agnostic and handles symlinked / pnpm-hoisted / Windows-backslash / workspace-linked-package cases.
- Hand-rolled type-text parsers must not exist inside the resolver. Walk `TypeExpr` nodes — `IndexedAccess`, `Ref { name: "Pick", type_arguments }`, `Union`, `Intersection`, etc. — directly via Rust pattern matching.
- The JS compat layer reads `prop.type` (`TypeDescriptor`) for every semantic decision. `prop.rawType` is display passthrough only. Operator splits use union/intersection tag matching on `TypeDescriptor`, not hand-rolled string operator parsers.

If a new requirement appears to need text manipulation inside the resolver, fix the producer (lower the right OXC node, store the right typed field, extend `@verter/type-ir` with a missing variant) rather than reparsing or pattern-matching on text. Architecture-guard tests in `crates/verter_session/tests/architecture_guards.rs` and equivalents in `packages/component-meta` lock down this contract.

See `/component-meta` skill for the full producer-side schema (typed `*_expr` fields on `Analyzed*Field`, `ProjectedMacroSurfaces`, `ResolvedLocalType.type_expr` "always populated" invariant) and the post-cutover delete list.

### Typed Value Domain + Demand-Lattice Resolution (CRITICAL)

The U2 query-value-domain design (`docs/arch/u2-query-value-domain-design.md`) locks the typed value
domain and the demand-lattice that decides cache satisfaction/backfill. Resolution is typed end to
end; display is a projection; error and absorbing types ride existing carriers.

- **One key → one value arm.** Every `SemanticQueryKey` maps to exactly one `SemanticQueryValue` arm.
  No non-type value is smuggled into `GraphTypeNode`; the wire taxonomy stays a closed type taxonomy.
- **Demand lattice (presets, not enum order).** The five mode names (`Identity` / `Navigate` /
  `Shallow` / `Expanded` / `Skeleton`) are PRESETS over `(ProjectionDemand, EvalPolicy)`. Cache
  satisfaction and backfill are decided by lattice DOMINANCE over a RECORDED materialised
  `(path, point)` set — NOT by enum order, and NOT by a meet-derived nominal demand. `Skeleton` =
  `TypeParamShells` + carrier-stop; it is INCOMPARABLE to the expansion presets, a regime of its own.
- **Display is a projection.** Canonical display is computed at publish from the cached typed value,
  never a stored or re-parsed string. `display_needs` is display-only: it is masked OUT of every
  typed-value family key and never drives resolution. Two queries differing only in `display_needs`
  hit the SAME typed-value slot.
- **Error tolerance.** A result computed over torn / broken / mid-edit input is `ReturnOnly` and is
  never warm-admitted. A fact-rooted error (a recorded missing-dep fact) IS cacheable. `admit_decision`
  gates on the ROOTING FACT's presence in the `ReadSetSignature`, not on the taint class.
- **Error / any / never / unknown.** `unknown` = ⊤, `never` = ⊥; `any` is off-order (relates
  bidirectionally); `error` taints and rides the EXISTING `SemanticNodeData::Opaque(QueryError)`. NO
  new `GraphTypeNode::ErrorType` wire arm may be introduced — the wire-purity closure forbids it.
- **Planned STAGE-B guards (gap tracked here per the architecture-guard rule).** The discriminating
  behavioural guards land with STAGE-B: `cache_satisfaction_is_demand_lattice_not_enum_order` (U10),
  `cache_satisfaction_is_materialized_point_not_nominal_demand`,
  `display_needs_is_display_only_never_drives_resolution`,
  `error_tolerance_broken_input_is_returnonly_fact_rooted_error_is_cacheable`, and
  `error_any_never_propagation_lattice`. The design-gate guards landed NOW are
  `error_rides_opaque_no_new_error_type_wire_arm` and `u2_value_domain_design_doc_locks_invariants`
  (both in `crates/verter_session/tests/g_block/u2_value_domain_design_guards.rs`).

## Frontier Engine Tests

Tests in `crates/verter_session/src/frontier_tests.rs` cover diamond dedup, barrel ordering, cycle termination, budget enforcement, export routing, and store-view consistency. Run with `cargo test --package verter_session frontier_tests`.
