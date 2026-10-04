## Canonical Dependency Cache Rule

Host-backed type/import resolution must treat the canonical file ID as cache identity. Contract:

- Load a dependency source at most once per canonical ID per workspace content generation. Parse immediately and cache raw source, parsed/OXC snapshot, and reusable eval/build state right away.
- On a cold miss materializing an imported dependency, derive the AST-backed bundle from that single parse and cache together: file snapshot, semantic `ScriptShallowIndex`, lazy declaration-body memo, and any other reusable per-file analysis. Do not let later resolver stages trigger a second parse of the same canonical file just to build another artifact.
- Host-owned imported-file caches are long-lived for the `VerterHost` lifetime. Distinct queries on the same host reuse the same cached canonical file state until that file's content hash or workspace generation changes.
- Cache named declarations from that parsed file by name, not just exported entrypoints. Internal named types/interfaces/aliases still matter because exported declarations in the same file may depend on them later.
- Treat named-node discovery as local symbol lookup. Once a file is parsed for a canonical ID/version, future lookups hit cached symbol/export maps instead of rewalking the full AST to rediscover names.
- Treat AST ownership as single-pass work. For a canonical ID/version, do at most one full top-level AST walk to discover named symbols/exports, then cache lookup entries and leave deeper expansion lazy per symbol. Do not rewalk the full file to rediscover the same symbol later.
- Imported-file analysis exposes one shallow symbol graph keyed by `(canonical_id, symbol_name)` — authoritative source for local symbol kind/span, local import targets, direct reexports, local export aliases. Resolver stages consume that graph, not parallel rediscovery paths.
- Resolve the requested import from the cached parsed file first. If the name is absent, only then BFS through explicit barrel/re-export hops. Do not rescan the same file graph on the second request.
- Imported-file traversal stays shallow-first. After a canonical file is read/processed once for the current version, inspect only that file's shallow/export surface first. Do not navigate into an imported target just because the file imports it.
- Direct imported-file navigation allowed only when the requested symbol is present in the current shallow file's direct export/import route info, or when the current file is a barrel and the symbol was not found locally.
- For barrel files, if the requested symbol is absent from the shallow/export surface and the file has wildcard barrel reexports, enqueue all barrel targets as one BFS layer, shallow each target once, then check each shallow surface for the symbol before descending deeper. Do not deepen one barrel branch ahead of same-layer siblings.
- Barrel traversal stays symbol-directed. If the symbol is absent in a shallow barrel child, only then continue from that child to its own barrel reexports under the same BFS rule. Do not eagerly open unrelated imported files or non-matching sibling branches.
- Keep expansion lazy. Do not eagerly resolve every transitive type in a file up front. Preserve named references so later requests expand from cache when needed.
- Collected imported aliases stay shallow. Root normalization is demand-driven: once a demand resolves the defining root through the shared route authority, reuse that memoized root (do not re-walk the barrel chain per touch), and do not eagerly materialize a prepared declaration during collection.
- Builder-owned shallow imported aliases treat their stored canonical ID as the defining-file root. They consult cached barrel/export state only when a canonical root is still unknown. Cache the prepared alias on the defining canonical file and hydrate from that file's host cache or its lazy declaration-body memo. Do not synthesize barrel-local prepared aliases for symbols that resolve to another file.
- Whole-file hashes are for long-lived update handling and cache validation, not repeated warm reads. Compute/store the hash once for the current source version, reuse until VFS reports a newer content generation / file version.
- VFS is the authority for file-change invalidation. When a canonical file's version/hash changes, host caches derived from that canonical ID must be discarded together across source snapshots, parsed state, declaration-body memos, and resolved-type/import caches.
- Invalidation stays selective. If `/src/type.ts` changes, invalidate caches owned by `/src/type.ts` and downstream final expansion/query results depending on it, but do not reparse or reshallow unchanged owner files that merely import it. Those owners stay warm on their own-file caches and only re-resolve against the refreshed imported dependency state.
- A changed imported dependency may be reparsed once for its new hash, even if several owners or later queries need it. That single refreshed canonical file state is then shared across all requests.
- Concurrent cold requests reaching the same canonical imported file must collapse onto one host-owned materialization path. `Promise.all([MetaA, MetaB, MetaC])` must not produce three separate read/parse/shallow passes for the same `type.ts`.
- Prepared declarations are host-owned warm artifacts. Once `(canonical_id, symbol_name, whole_hash)` is prepared, later lookups from other owners and later distinct queries on the same host reuse that prepared declaration until invalidation.
- `OwnedLowering::prepared_type_decl` preserves `Result<Option<Arc<PreparedTypeDecl>>, PreparationFailure>` end to end. `MissingExternalOwner` and `AuthoredOrdinalOverflow` are typed failures, never declaration absence: the prepared slot stays vacant, and an Option-shaped semantic boundary may serve them only through the single ReturnOnly adapter that marks the enclosing derivation non-cacheable. `LeaseMiss` remains the distinct recoverable `Ok(None)` + non-cacheable rail.
- Prepared import canonicalization is DEMAND-DRIVEN. Bundle build (`build_prepared_import_canonicalization`) walks NO import chain: it records each resolvable binding's DIRECT hop as `(local owner, local name) → (direct target canonical, ordinary-file owner, imported name)` — the `ordinary_file()` owner is the provisional final-resolution-owed marker. The FINAL `(canonical, owner, symbol)` resolves at the first decl-prepare / ref-head demand through the shared route authority (the type-export rail `resolve_imported_type_root_with_facts*`, memoized in `ImportedRootDb` under an R6 content-free key; the graph-native value-export rail with the terminal alias peel for value demands), and every demand site observes the chain hops' `FileWholeHash` + `Route` facts into the ACTIVE fact tracer AT DEMAND TIME — so the CONSUMING query's read-set (a `LowerLocator` shape memo, an `Instantiate` memo, a component-meta proof) invalidates on a barrel retarget or leaf edit anywhere on the chain. Chain facts are never pinned on the bundle's fact rail. Never default the target owner, substitute the source owner, or recover by name/span; an UNRESOLVABLE specifier records no entry and remains `MissingExternalOwner` and non-cacheable at prepare.
- A member-value-position reference to an unresolved AUTHORED IMPORT stays an honest `BareRef` carrier — it never poisons the root object's completeness (authored-partial preparation is declaration-wide, Instantiate completeness is demand-local; the member consumer that actually demands the value degrades it member-locally, e.g. the Vue runtime constructor's per-member `null` degradation). The unresolved-head site observes the owner's request-bound path-precise resolution witness into the active tracer (the demand-time recovery rail, same as `build_typeof`'s import-miss arm), so carrier-bearing surfaces stay COMPLETE + cacheable and every consuming warm entry invalidates the moment the missing dependency appears. Root-alias / heritage / authored intersection-union-arm reaches remain authoritative missing-dependency debt (typed partial + ReturnOnly).
- A traced compute observes a consumed file's `DerivedFactHash{Route}` fact at the `ensure_indexed_ready_serve` demand point — content-pinned from the served artifact's `route_hash` under the exact store-view publish predicate (store-published + edge-current + resolvable surface), so warm validation round-trips by construction. This is how a cross-file dep's Route fact reaches the published component-meta signature even though the direct-import fast path resolves the dep before it is indexed (a get, never an ensure).
- Reuse the current host-owned route/barrel cache path: `RouteDb` for barrel/export route facts and `ImportedRootDb` for imported-root proofs. Do not add a second route-cache subsystem for the same work without explicit proof it is needed.
- Route discovery stays lazy and demand-driven. First-hit discovery may follow barrel/reexport hops only until the symbol is found (or proven absent under the current negative-cache policy). Do not require a full scan of all barrel exports on every first hit.
- Warm same-owner lookups reuse the existing valid importer-local route entry rather than replaying the full barrel chain.
- Cross-owner reuse should come primarily from shared imported-file state, shared barrel/export surfaces, and prepared declarations. Do not assume canonical cross-owner route-fact backfill exists unless a later change explicitly adds it.
- Stable negative route answers are gated by `BarrelResolutionState.fully_resolved` plus tracked dependency/store-view freshness. Richer persisted completeness states, if ever needed, are an explicit follow-up, not an existing invariant.
- If in-flight dedup is needed for concurrent cold route work, model it separately from persisted barrel state. Do not overload `fully_resolved` to mean "currently being built".
- Do not use `Arc` next-hop chains as the primary barrel cache shape if a future route-cache redesign is introduced.
- Route caches and prepared-declaration caches invalidate independently. If a leaf file body changes but its export surface stays the same, the route fact may remain valid while prepared declarations and downstream final results refresh.
- On file update, eagerly recompute the changed file's own parse/shallow/export surface once. That write-path cost is acceptable and keeps later reads fast.
- Do not eagerly rewrite every upstream barrel/route fact on every changed-file update. After the changed file's fresh shallow/export snapshot is available, let upstream route facts validate lazily against tracked route participants/generations on demand.
- Prefer comparing old vs new shallow/export surface for the changed file. If the export surface is unchanged, keep route generations stable and refresh only body/prepared-declaration/final-result layers. If it changed, bump the route/export-surface generation so affected warm route facts become stale and lazily rebuild on next access.
- Route invalidation is not file-hash-only. tsconfig path changes, vite alias changes, workspace graph changes, package target changes, and barrel export-surface changes must invalidate affected route facts even if the owner file text did not change.
- Negative route/cache misses may be cached only against a concrete snapshot (hash/generation/store-view context). Cancelled or interrupted results must never be promoted to warm reusable cache entries.
- One query resolves against one coherent host/store snapshot. Resolver stages must not mix captured stale owner routes with newer live dependency routes within a single query flow.
- Legacy fallback paths that reparse or rewalk imported dependency files on warm requests should be removed, not preserved behind alternative code paths. Default behavior must go through the cache-aware host/VFS path.
- Architectural cache/resolver changes land as one clean cutover. No temporary shims, compatibility wrappers, feature flags, or duplicated old/new paths. Delete the superseded path in the same change, or upgrade the surviving path to first-class shared ownership with the same invariants and tests.
- Imported dependency loading, type-resolution source materialization, and dependency canonical resolution should be host-owned single entry points. Do not add request-local cache layers or alternative parser/import paths on top of the host cache for the same work.
- Imported type root/declaration resolution and prepared imported-type alias caching should also be host-owned single entry points keyed by canonical ID plus current file version/hash. Do not rebuild the same imported symbol route or prepared alias body per request when the host cache already has it.
- Do not add new request-scoped lookup memos over host-owned resolver work in the final architecture. Existing request-view-era memos are legacy and must be removed as part of the project-global cache cutover.
- `source_type` for downstream cache keys is authoritative from the scheduler: `HostSourceData::source_type` is computed once at `execute_source` time with full access to the parsed SFC; readers consume via `VerterHost::authoritative_source_type_for(canonical)`. Recomputing from `(canonical_id, framework_parse)` is unstable when the `framework_parse` artifact is dropped mid-resolution. Carrier files read the neutral `FrameworkParseCommon.script_regions[].source_type` (populated by the owning adapter's producer — Vue: `verter_compiler::framework_common::vue_bridge::build_vue_parse_artifact`); plain scripts derive from the classified `FileLanguage` row (`verter_language` registry — the SOLE plain-script dialect authority, `.d.ts`-family included via the `Dts` rows; `ScriptSourceType` carries `JsModuleKind` fidelity for JS: `.js` unambiguous, `.mjs` module, `.cjs` commonjs, `.jsx` JSX — session parse code never re-sniffs path extensions; guard: `plain_script_dialect_from_file_language`).

**Concrete performance contract:**

- If `MetaA`, `MetaB`, `MetaC` all depend on `type.ts`, the first query batch may process each owner file once and `type.ts` once.
- If a later batch requests `MetaB` and `MetaC` again with no file changes, it must reuse the warm cached state for both the owner files and `type.ts`.
- If `type.ts` changes between batches, `MetaB` and `MetaC` may keep their own-file caches, while `type.ts` is processed exactly once for the new hash and then shared by both later requests.

### Import-Route Admission Ownership

`DerivedRawState.import_routes` is exclusively the CALLER-SUPPLIED
authoritative route table — a bundler telling the host how ITS resolver
resolves. The host memoises NO resolution there: the workspace's own
bounded owner-edge candidate slot is the one resolution memo, validated
per-reader against a captured immutable resolution world.

- **Complete caller-supplied snapshot** — `VerterHost::set_import_dependencies`
  is the **single producer**. Its entries serve until the caller replaces them;
  their currency rides the `ExactResolution` facts the same push installs, and
  their KEYS join the owner's authored specifiers in the import-route witness
  inventory, so a caller-pushed specifier with no authored counterpart is
  witnessed like any other resolution.
- **Lifecycle reset** — `VerterHost::configure_projects` (project-graph
  reconfiguration) and `VerterHost::upsert_via_scheduler_with_priority` (owner
  source update) may `.clear()` the table.
- **Deleted** — the host-side positive-route memo
  (`cache_positive_import_route_result`), its `PositiveRouteStamp` /
  `import_routes_positive_recorded_at_generation` sidecar, the known-miss
  generation sidecar, and the per-entry oracles
  `import_route_is_generation_current` /
  `import_route_entry_is_generation_current`. A host-side memo duplicated the
  workspace candidate slot and, having no witness of its own, needed a global
  `content_generation` equality to decide whether it was still true — the last
  such warm-resolution validity test in the session. What remains host-owned is
  the resolved dependency EDGE set (`record_resolved_dependency_edge` writing
  `DependencyState.dependencies`), which is reverse-dependency bookkeeping, not
  a resolution answer.

A KNOWN-MISS entry is never served warm: a negative answer is not evidence that
the answer is still negative, so the reader refuses it and the specifier
re-resolves through the one owner-edge authority (where a warm candidate whose
exhausted probe set is unchanged is reused, so the re-resolve is cheap).

Architectural rules carried by the writer guard at `crates/verter_session/tests/cases/g_misc3/import_route_writer_guard.rs`:

- A direct `derived_raw_cache().entry(...).import_routes.insert(...)` outside
  `set_import_dependencies` and the lifecycle reset methods is rejected — that
  is how a host-side route memo would be reintroduced.
- `positive_route_memo_producer_is_deleted` asserts the deleted producer and
  stamp are absent from production source.

### Parse/Resolve Ownership

`IndexedReady` and `ShallowFileState` are content-addressed PARSE/INDEX
artifacts. They retain authored import/export syntax, specifiers, shallow
declarations, locators, and parse-domain facts — and NO resolved canonical.
Materialising an artifact performs ZERO import resolution.

The engine reads cached pure input projections through `IndexedInputs` and
requests body/dependency/preparation work through `OwnedLowering`. The private
source backend retains the exact observed artifacts; input records do not expose
workers or cache mutation. `RouteLookup` owns routing demands. Source memo DBs
hold passive slots, while request-local source drivers perform the existing
singleflight walk, validation and admission in their original order.

- The complete reuse gate is `indexed_surface_is_current` = the owner's
  `parse_env_hash` equals its live parse environment. No route-resolution
  mutation can stale a parse artifact, so there is no edge-currency oracle and
  no route-only edge-refresh materialise lane.
- `IndexedReady.built_at_content_generation` is a CONTENT-domain stamp with one
  consumer, `artifact_only_candidate_is_fresh` (a per-canonical comparison
  against the workspace content-transition ledger). Never a global-generation
  equality, never a route currency oracle.
- `Route` is a pure parse-domain digest of the AUTHORED routing surface. The
  resolve-domain half of a route answer rides the import-route resolution
  WITNESS: a route walk collects a PATH-PRECISE witness through
  `ResolutionWitnessScope` over exactly the edges it TRAVERSED, never the
  owner's whole authored inventory (which for a barrel is far broader than any
  route through it).
- The layer-ordered wildcard walk keeps each descendant as an unresolved
  `(owner, source_specifier)` edge and resolves it only when visited, so a
  barrel's later-declared `export *` siblings are never resolved or loaded when
  an earlier-declared one carries the requested export.
- Resolution is SESSION-SCOPED for a session-bound consumer
  (`SessionResolverContext::resolve_type_dependency_canonical` resolves through
  the session's own overlay): with nothing baked into the artifact, a base-host
  resolve would make an overlay-only dependency disappear.
- A session request's overlay is ONE resolution snapshot
  (`ResolutionOverlaySnapshot`), built once by `with_session_overlay` and kept
  on the store view. The context resolves through it
  (`RequestBoundLifecycle::resolution_overlay`), and the view validates every
  resolution fact against the effective world it composes. Facts the overlay
  changes take versions from a reserved overlay space, so an overlay answer
  never serves the workspace and a workspace answer the overlay reaches never
  serves the session. A workspace answer the overlay cannot reach is reused.
  The type route is one policy for both views
  (`resolve_type_dependency_canonical_in`), and overlay answers are cached in
  the Engine's overlay lane, never in a workspace slot. Never rebuild the
  snapshot per call or add an overlay-keyed resolution memo beside the Engine.

Full normative text: `docs/contributing/path-precise-resolution-currency.md`.

### Module-resolution keying (split env)

Import probing distinguishes recognized source extensions from dotted filename stems. Imports such as `./types.d` and `./Button.types` still probe appended script/declaration extensions; registered carrier suffixes retain literal carrier ownership. Literal SFC `src` attributes do not gain this dotted-stem import fallback.

Import/module resolution is keyed on the **split** env dimensions — see
`### Module-Resolution Keying (CRITICAL)` in the `/type-cache-architecture`
skill (the owner). `resolve_env_hash` carries the resolve-domain inputs (the
`moduleResolution` mode, the `exports`/`imports` `ConditionSet`,
`base_url`/`paths`, aliases, references, extension order); the lib corpus
(`lib_names`/`typeRoots`/ambient fingerprint) is NEVER folded into
`resolve_env_hash` — it keys `lib_env_hash` only (R21: resolve and lib are
orthogonal dimensions). The module-resolution SHAPE vocabulary
(`ModuleResolutionMode`, `SpecifierKind`, `ConditionSet`) lives in
`verter_workspace::module_resolution`; the FORK-C resolution matrix walker
that consumes it is U0 `verter_session::resolver_core`.

