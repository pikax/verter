---
name: component-meta
description: "Component metadata: native vs compat boundary, fallthrough/root inheritance, resolver rules, cache-owned hydration, registry publication"
---

# Component Meta

## Final-Result Cache (post-rewrite)

`get_component_meta(owner)` consults `ProjectTypeStore::component_meta_results()` before running the resolver. The cache is typed `ComponentMetaResultDb<ComponentMetaAnalysis>` and keyed by `(owner_canonical, owner_whole_hash, ComponentMetaQueryKind, options_fingerprint)`.

Flow on each call:

1. Look up `shallow_file_state(owner)` for the current whole-hash.
2. Build `ComponentMetaResultKey` with `component_meta_options_fingerprint(&ComponentMetaOptions::default())` — xxh3-128 over a manually-versioned encoding (schema + `compat` + `include_fallthrough`).
3. Try `component_meta_results().get(&key)`. On hit, revalidate the entry's `DepSignature` via `HostFenceValidator::validate` (walks every `(canonical, DepVersion)` pair against the live host). Stable signatures return `Arc<ComponentMetaAnalysis>` with zero resolver work.
4. On miss or stale signature, run the existing resolver and publish the result with the owner's whole-hash + transitive dep facts + current project generation as the dep-signature.

Cache eviction is automatic: `host.upsert(...)` calls `project_type_store.evict_canonical(owner)` which `invalidate_owner`s every key for the changed canonical. Workspace-shape shifts (tsconfig / SDK / project-graph) call `bump_project_generation_and_evict`, clearing all result entries.

Direct owner imports take the `OwnerImportSurfaceDb` path via `resolve_owner_direct_import`; no legacy `resolve_imported_type_root` caller survives for direct-owner resolution, though the helper remains the authority for transitive chain walks inside route/barrel code.

## Native Vs Compat (CRITICAL)

The official/native component-meta payload is the semantic authority. `@verter/component-meta/compat` is a projection layer for `vue-component-meta` interoperability, not a second semantic pipeline.

- Fix missing or incorrect metadata in the shared/native owner layer first. Compat should only remap representation when the native payload is already correct enough.
- Rust is the component-meta semantic authority. Resolution, declaration routing, recursion handling, graph construction, payload shaping, and the full Verter API response belong on the native side.
- `@verter/component-meta` must issue one async native request per query and receive the full Verter API response needed for that query. Do not introduce JS/native follow-up calls to progressively resolve missing types, missing graph nodes, or deferred declarations.
- The only intentional off-spec difference versus `vue-component-meta` is that Verter's native boundary is async instead of sync. Apart from that boundary difference, compat should behave like a projection over one completed native response, not a coordinator of extra native work.
- Rust should send the smallest semantically complete payload possible. Prefer compact symbolic graph structure, shared arenas/tables, and explicit recursion nodes over oversized expanded trees, raw-text-only fallbacks, or `unknown` when a graph-backed representation is possible.
- JS may decode, reconstruct recursive descriptors, memoize shared graph nodes, and adapt the native response into compat output, but that work must stay mechanical. JS may transform structure, but it must not recover meaning that native code failed to provide.
- Do not let JS become a second resolver, a second expander, or a second semantic authority. Compat must not reinterpret unresolved symbols, invent fallback semantics, or silently repair native payload gaps.
- Do not weaken native TypeScript meaning to imitate Volar formatting. Example: keep native `boolean` as `boolean`; any `true | false` expansion belongs only in compat-specific display/schema logic if we choose to support it.
- Indexed-access members may be resolved/expanded when that improves real type fidelity. Targeted compat expansion such as `Alert['variants']['color']` to the concrete color union is acceptable; blanket ref flattening is not.
- Compat `exposed` parity should be derived from a shared cached public-instance surface (for example a `ComponentPublicInstance` extraction owned by the host/public-instance path). Do not redefine native `exposed` to mean public-instance unless the public API is deliberately expanded.
- Native-only extensions such as `models`, `acceptedProps`, `acceptedEvents`, `acceptedSurfaceCompleteness`, `rootReachability`, and `fallthroughSurface` are part of Verter's official API. Benchmark them separately from Volar-surface parity instead of treating them as regressions.
- Component-meta type recovery must stay cache-owned. When changing `verter_session`, `verter_session::resolver_core`, or `packages/component-meta` type paths, rely on cached lookup/eval state and expand only on demand; do not rewalk AST/source as a fallback to recover missing types.
- Component-meta registry publication must stay shallow. Publish only the symbols demanded by the current query path, and do not eagerly materialize unrelated owner/package helpers just to populate the registry.
- Component-meta companion/file-target selection must stay shallow too. Choosing between runtime and declaration companions may probe cached raw source existence, but must not build export analysis, snapshots, or eval envs just to decide the target file.
- Imported component-meta hydration must stay cache-owned too. Once shallow imported dependency state exists, later alias/registry/fallthrough resolver stages must read only from that cache-owned state and must not jump back to raw snapshot/source builders for imported files.
- Component-meta resolvers must deepen in exactly one place per requested symbol/query path. Do not let a file-level helper widen into sibling symbols/files that are not on the active declaration route.
- Component-meta metadata/fallthrough projection must stay query-scoped. Reuse the resolved state plus captured `HostStoreView`/session view; do not re-enter a fresh top-level meta/fallthrough query when a resolved query already exists.
- Imported symbol collection for component-meta must stay single-path and lazy. Do not introduce eager collection modes or reparsing fallbacks from stored source text; selected imported symbols must be hydrated through the host-owned cache and resolved via the solver only.
- Resolver, solver, recursion tracking, and graph interning should share one stable semantic identity model: defining symbol identity plus type arguments and any relevant conditional context. Do not let cache keys drift by layer.
- Component-meta output must be deterministic for a coherent snapshot/query. The same inputs should produce the same graph identities, metadata shape, and compat-visible meaning.
- If native code truly cannot represent part of a type, encode that explicitly as a structured unsupported/HardStop-style result with diagnostics/provenance sidecars. Do not silently degrade a representable type to raw text or `unknown`.
- The native response contract must remain explicit and versioned. If the component-meta payload shape changes, do it as a deliberate schema/API cutover rather than compat-side drift.
- Host-owned resolver artifacts, graph artifacts, and encoded payload caches must share one invalidation story. Do not add a second ownership path for the same component-meta query state.
- Raw graph cycles reaching JS without explicit recursion nodes are native bugs, not a normal compat fallback path.

## Fallthrough / Root Inheritance (CRITICAL)

The shared Rust pipeline owns all fallthrough and root inheritance semantics. `verter_semantic::analysis` extracts root reachability facts only. `verter_session` owns the single inheritance resolver, recursion, conditional branch composition, generic propagation, caching, and final metadata projection.

**Public contract** (on `ComponentMetaAnalysis` / `FfiComponentMeta` / `ComponentMeta`):

- `props` / `events` -- declared component surface only (unchanged)
- `acceptedProps` / `acceptedEvents` -- computed call-site acceptance surface (declared + inherited)
- `acceptedSurfaceCompleteness` -- `Exact` or `LowerBound` (if any branch is partial/unresolved)
- `rootReachability` -- structural root classification before inheritance resolution
- `fallthroughSurface` -- branch-structured inherited surface after host resolution

**Semantic rules**:

- `inheritAttrs: false` -- no inherited surface
- Unconditional multi-root (fragment) -- no inherited surface
- Root `v-for` -- no inherited surface
- Single native root -- intrinsic attrs/listeners minus declared props/events minus consumed root bindings
- Single component root -- recursive propagation through the child's full public surface
- Conditional single-root branches -- exact union of branch surfaces
- Cycles -- terminate safely as unresolved branches, no invented members
- Unsupported roots (`<component :is>`, `<slot>`, Vue built-ins) -- unresolved branches
- `class` and `style` are never consumed (Vue always merges them)
- `@click` and `:onClick` normalize to the same canonical listener name (`click`)
- Declared props/events always take precedence over inherited names

**Authority chain**: analysis extracts `RootReachability` -- host resolves `FallthroughResolution` -- `get_component_meta()` populates `accepted_*` and `fallthrough_surface` -- FFI maps to JSON -- TS consumes.

**Compat**: mapping-only. Flat Volar `props/events` stay on declared surfaces. Branch-structured inherited data is on `_verter`.

**Key files**:

| File | Purpose |
| --- | --- |
| `crates/verter_semantic/src/analysis/component_meta.rs` | Types + root extraction |
| `crates/verter_semantic/src/analysis/html_intrinsics.rs` | Native intrinsic catalog |
| `crates/verter_session/src/host_manage.rs` | Resolver + cache |
| `crates/verter_protocol/src/types.rs` | Schema DTOs |
| `crates/verter_ffi/src/convert.rs` | Adapter conversion |
| `packages/component-meta/src/types.ts` | TS types |

## Component-Meta Resolver Rules

These are the canonical component-meta resolver rules. They govern how the shared cross-file resolver operates when serving component-meta queries.

**Resolver ownership rule:** host-backed component-meta and analysis must share one cross-file resolver. Do not build separate resolver logic for script-setup macros, Options API metadata, compat wrappers, or consumer-specific adapters.

**IndexedReady target rule:** the foundational per-file input for component-meta is the post-parse `IndexedReady` artifact, not a separate request-local type-ready layer. Component-meta should start from canonical imports/exports and the owned lowered shallow symbol representation stored in that artifact, then deepen only on demand for the active symbol route.

**Shared-expansion rule:** when component-meta expands a shallow symbol, it must populate the same host-owned route, prepared-declaration, owner-import, and projection caches used by other resolver consumers. Do not create a component-meta-only expansion cache or helper path when the shared resolver can own the work.

**Path-independent cache rule:** component-meta must benefit from and contribute to the same shared semantic caches regardless of entry path. If a projected member or expanded symbol was already computed from another consumer path, component-meta should reuse it. If component-meta computes a reusable result successfully, it should populate the shared cache for later callers.

**Backfill rule:** broader successful results may backfill narrower caches they actually satisfied, but narrower results must not pretend broader work is cached. Cancelled, partial, or budget-exceeded work must not be promoted as warm shared cache entries.

**Navigation-not-expansion rule:** component-meta should navigate intermediate type paths as narrowly as possible. When resolving a path like `A['c']['full']['bar']`, the intermediate hops should stay in navigation or shallow projection mode; only the terminal requested projection should expand unless limited normalization is required to continue.

**Generic-instantiation rule:** component-meta navigation and expansion must respect generic substitutions. Member projection or indexed access on instantiated generic types must operate on the substituted meaning, and the shared cache keys must include the relevant type arguments or substitution environment.

**Navigator-boundary rule:** component-meta path walkers are not allowed to become a private resolver. They may do non-owning normalization and choose the next hop, but any operation that could recurse, cross files, instantiate a new semantic node, or populate reusable caches must enter through the shared semantic query API.

**Component-meta rule:** all metadata-producing macro and Options API surfaces must go through the shared resolver in `Expanded` mode. That includes props, emits, slots, data, computed, and expose-style members.

**Traversal rule:** only follow the import graph reachable from the requested type's declaration graph. Unrelated imports in the same file are out of scope.

**Caching rule:** when parsing a `.ts` / `.js` / declaration file for type resolution, cache discovered symbol name to canonical location mappings. Cache direct re-exports, barrelled exports, and any discovered `export *` hops as well, because repeated wildcard-barrel scanning is expensive.

**Component-meta cache rule:** when changing `verter_session`, `verter_session::resolver_core`, `verter_semantic`, or `packages/component-meta` type paths, use cached lookup/eval state as the only source of truth after the cache-owning pass. Do not rewalk AST/source as a fallback to recover or expand types.

**Component-meta publication rule:** keep registry publication shallow and demand-driven. Publish and expand only the symbols required by the active metadata query; do not eagerly materialize unrelated owner-local or package-local helpers.

**Component-meta target-selection rule:** keep runtime/declaration companion selection shallow. Canonicalization may probe cached raw source existence, but must not build export analysis, snapshots, or eval envs just to choose a target file.

**Component-meta imported-state rule:** after shallow imported dependency seeding, resolver stages must consume only the imported dependency cache for imported file snapshots/envs/analysis. Do not bounce from alias or registry hydration back into raw snapshot/source builders for imported files.

**Component-meta analysis interop rule:** LSP or other consumers that request richer analysis but still need shallow type retrieval should build on the same `IndexedReady` and shared resolver artifacts. Analysis is an additive layer over the indexed base; it must not bypass the component-meta/type cache path for shallow symbol expansion.

**Component-meta deepening rule:** resolve one requested symbol/query path at a time. Do not let a file-level resolver widen into unrelated sibling symbols/files while chasing a single metadata request.

**Component-meta imported-file rule:** imported files stay shallow-first and symbol-directed. After an imported canonical file is read/processed for the current version, consume only its shallow/export surface first. Do not navigate into its imports unless the requested symbol is present on a direct route from that shallow state, or the file is acting as a barrel and the symbol was not found locally.

**Component-meta barrel BFS rule:** when the current imported file is a barrel and the requested symbol is not present in its shallow/export surface, follow wildcard barrel exports breadth-first by layer. Shallow all barrel children in the current layer, check each for the requested symbol, and only then continue to the next barrel layer. Do not descend one barrel branch depth-first ahead of same-layer siblings, and do not open unrelated imported files while searching that symbol route.

**Component-meta projection rule:** when projecting metadata/fallthrough from an already-resolved query, reuse that resolved state plus the captured store/session view. Do not bounce back out to a fresh top-level fallthrough/meta query.

**Component-meta collection rule:** imported-eval collection must stay lazy/BFS over the active symbol route. Do not add eager collector modes or source-text reparsing fallbacks in shared resolver code.

## Semantic Dispatch Integration (Post Phase-D)

All type expansion for component-meta goes through `ProjectSemanticDispatch::execute(SemanticQueryKey::...)` against the shared `SemanticGraphStore` (see `/type-resolution` for the full authority contract). The component-meta layer is a pure `SemanticQueryApi` consumer — it does not own a separate solver, a separate relation engine, or a separate lowering path.

**Call-site pattern** (plan §9 appendix — canonical migration shape):

```rust
// Retired: owner_engine.solve_scoped(host, scope, &expr) / .solve(&expr)
// Post-cutover:
let base = dispatch.shallow_lower_type_expr(&expr, &env, &scope_node, &name_resolution, &mut substitutions);
let path: Arc<[PathSegment]> = Arc::from([]);
dispatch.execute(SemanticQueryKey::ProjectPath { base, path, mode: Expanded });
// Then `semantic_node_to_type_expr(host, result)` to round-trip to TypeExpr.
```

`env` (type-parameter bindings) and `name_resolution` (import map) must be preserved per Gemini's substitution-environment warning — dropping either causes bare-name misses in declaration-scoped resolution.

**Retired identifiers** (per plan §9 / §5.7 WIP-C):

- `owner_engine.solve_scoped(...)` / `.solve(...)` → `dispatch.execute(ProjectPath { ..., mode: Expanded })`
- `owner_engine.project_expr_surface_as_type_expr(...)` → same as above
- `engine.solve_expr_type_expr(...)` → same as above
- `engine.project_expr_surface_shape(...)` → `dispatch.execute(ProjectPath { ..., mode: Shallow })` + surface-shape reader helper
- `engine.expand_local_generic_ref_expr(...)` → `dispatch.execute(SemanticQueryKey::Instantiate { base, args })`
- `TypeSurfaceDb::{get, publish, evict_*}` → DELETED; identity lives in `SemanticGraphStore`'s node memo
- `TypeSolverHost`, `EvalEnvSolverHost`, `SessionSolverHost` traits/structs → DELETED; dispatch called directly
- `TypeQueryEngine` → DELETED; `ProjectSemanticDispatch::new(host)` replaces

**Key resolver files (post-cutover):**

| File | Purpose |
| --- | --- |
| `crates/verter_session/src/project_semantic_dispatch/` | `ProjectSemanticDispatch`, `SemanticQueryApi` impl, build/walk/relate/lower/substitute/evaluate/guards/enumerate sub-modules |
| `crates/verter_session/src/semantic_query_memo.rs` | `SemanticGraphStore` (node memo + relation memo) |
| `crates/verter_session/src/host_manage.rs` | `get_component_meta()` entry point, `HostNamedTypeCacheAdapter` (reads/writes `SemanticGraphStore` directly for Vue macro results) |
| `crates/verter_session/src/host_resolve.rs` | `HostFrontierAdapter`, cross-file type resolution |
| `crates/verter_session/src/resolver_core/component_meta_query_engine.rs` | `ComponentMetaQueryEngine` — pure `SemanticQueryApi` consumer; no `owner_engine` field, no private resolver/expander state |

## Component-Meta Perf / Debug Workflow

When debugging component-meta performance or validating resolver invariants, use the repository-owned benchmark and trace workflow first and treat trace as a correctness/attribution tool, not as the final latency source of truth.

### Tool order

1. Use no-trace benchmark runs to measure real request latency.
2. Use trace runs to validate resolver rules and identify which stage owns the time.
3. Use native profiling only after trace/no-trace runs identify a stable hotspot.

### Benchmark authority

- `query_ms_from_stdout` from `scripts/benchmark/trace-component-corpus.mjs --no-trace` is the best lightweight request-latency number for component-meta iteration work.
- `wall_ms` includes Node/process/bootstrap/teardown overhead and is useful for harness cost, not resolver-only cost.
- Traced runs add overhead; use them to understand route behavior and stage ownership, then confirm wins again with `--no-trace`.

### Trace interpretation

- `trace_resolve_ms` is the primary root `resolve_component_meta` span only.
- `trace_query_ms` is the sum of all root traced spans in the request. Use this when later imported-local work or extraction/fallthrough work is not inside the primary resolve span.
- Trace logs under `<trace-dir>/traces/` are the authority for route correctness:
  - imported files should stay shallow-first
  - imports should deepen only on the requested symbol route
  - wildcard barrels should stay BFS by layer
  - unrelated imported siblings should not be promoted/materialized

### Real-project component-meta commands

```bash
# Targeted no-trace timing for a real nuxt-ui component
node scripts/benchmark/trace-component-corpus.mjs \
  --output-dir=tmp/cm-notrace \
  --filter=Accordion.vue \
  --no-trace

# Targeted traced run for route validation and stage attribution
node scripts/benchmark/trace-component-corpus.mjs \
  --output-dir=tmp/cm-trace \
  --filter=Accordion.vue

# Full no-trace corpus run
node scripts/benchmark/trace-component-corpus.mjs \
  --output-dir=tmp/cm-full \
  --no-trace

# Validate traced output against strict rules / expected artifacts
npx tsx packages/benchmark/src/trace-check.ts \
  tmp/cm-trace \
  --batch "Accordion" \
  --strict \
  --check-expected
```

### Native profiling

For native hotspot attribution on a real `nuxt/ui` component, use the real-project profiler:

```bash
cargo run -p verter_bench --example profile_real_component_meta --release --features=hotpath -- Accordion
```

Useful environment variables:

- `VERTER_PROFILE_PROJECT_ROOT` - override the project root (defaults to `.integration-tests/repos/nuxt-ui`)
- `VERTER_PROFILE_REPEATS` - repeat the request multiple times
- `HOTPATH_METRICS_PORT` - choose a non-default hotpath port if another run is active
- `HOTPATH_METRICS_SERVER_OFF=1` - disable the HTTP metrics server when only local timing output is needed

Practical rule:

- use hotpath/native profiling for relative attribution inside one run
- use the no-trace benchmark as the final guard against real-world perf regressions

For repository-owned component-meta benchmark and profiling commands, see `/build-and-profiling`.
