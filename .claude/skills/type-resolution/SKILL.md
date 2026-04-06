---
name: type-resolution
description: "Cross-file type resolution: type solver, ShallowFileState, ExternalTypeFrontier, canonical cache rules, macro traversal, prepared declarations"
---

# Type Resolution

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
- Keep expansion lazy. We do not need to eagerly resolve every transitive type in a file up front. Preserve named references so later requests can expand them from cache when needed.
- Collected imported aliases stay shallow but must already be root-normalized. Store the defining file's canonical ID plus the final exported symbol name; do not keep unresolved barrel routes once the root is known, and do not eagerly materialize a prepared declaration during collection.
- Builder-owned shallow imported aliases should treat their stored canonical ID as the defining-file root. They may consult cached barrel/export state only when a canonical root is still unknown. Cache the prepared alias on the defining canonical file and hydrate from that file's host cache or base eval env. Do not synthesize barrel-local prepared aliases for symbols that resolve to another file.
- Whole-file hashes are for long-lived update handling and cache validation, not for repeated warm reads. Compute/store the hash once for the current source version, then reuse it until the VFS reports a newer content generation / file version.
- VFS is the authority for file-change invalidation. When a canonical file's version/hash changes, host caches derived from that canonical ID must be discarded together across source snapshots, parsed state, eval envs, and resolved-type/import caches.
- Invalidation must stay selective. If `/src/type.ts` changes, invalidate caches owned by `/src/type.ts` and downstream final expansion/query results that depend on it, but do not force reparsing or reshallowing unchanged owner files that merely import it. Those owner files should stay warm on their own-file caches and only re-resolve against the refreshed imported dependency state.
- A changed imported dependency may be reparsed once for its new hash, even if several owners or several later queries need it. That single refreshed canonical file state must then be shared across all of those requests.
- Concurrent cold requests that reach the same canonical imported file must collapse onto one host-owned materialization path. `Promise.all([MetaA, MetaB, MetaC])` is not allowed to produce three separate read/parse/shallow passes for the same `type.ts`.
- Prepared declarations are also host-owned warm artifacts. Once `(canonical_id, symbol_name, whole_hash)` has been prepared, later lookups from other owners and from later distinct queries on the same host must reuse that prepared declaration until invalidation.
- Reuse the current host-owned route/barrel cache path. Today that means `ImportTypeRouteEntry` on the importer side plus `BarrelResolutionState` on the imported-file side; do not add a second route-cache subsystem for the same work without explicit proof it is needed.
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

**Concrete performance contract:**

- If `MetaA`, `MetaB`, and `MetaC` all depend on `type.ts`, the first query batch may process each owner file once and `type.ts` once.
- If a later batch requests `MetaB` and `MetaC` again with no file changes, that later batch must reuse the warm cached state for both the owner files and `type.ts`.
- If `type.ts` changes between batches, `MetaB` and `MetaC` may keep their own-file caches, while `type.ts` is processed exactly once for the new hash and then shared by both later requests.

## Shallow File State and Frontier Engine

Cross-file type resolution for macros (`defineProps<T>()`, component-meta, etc.) is built on two shared primitives in `verter_session::resolver_core`:

**ShallowFileState** (`shallow_file_state.rs`) is the authoritative shallow symbol/export surface for one imported type file. Keyed by `(canonical_id, whole_hash)`. Contains:
- `exports` map (exported name -> `ExportTarget`: Local or Reexport)
- `wildcard_reexports` (`export * from` sources, in declaration order)
- `symbols` (all locally-declared type symbols with raw body, type params, local deps, external deps)
- `import_locals` / `import_targets` (import classification for closure)

Populated once through the shared host ensure-path and cached on `ImportedDependencyCacheEntry.shallow_file_state`. Invalidated when the file's whole-hash changes.

**ExternalTypeFrontier** (`external_type_frontier.rs`) is the single BFS engine for all cross-file type deepening. Level-by-level traversal:
1. Seed with initial `(canonical_id, exported_name)` pairs
2. For each pending symbol: load `ShallowFileState` via `FrontierHost` trait, route the export (direct > alias > wildcard in declared order), run local closure
3. Collect `ExternalSymbolRef` entries from unresolved external deps into the next level
4. Dedup on `(canonical_id, exported_name)` across the entire request via `seen` set
5. Repeat until frontier is empty or budget is exceeded

**Local closure** (`ShallowFileState::local_closure()`) resolves same-file transitive deps iteratively. Uses a visited set for cycle handling (revisited nodes are silently skipped). Never crosses import boundaries -- external deps become `ExternalSymbolRef` for the frontier.

**Budget contract** -- three domains with high ceilings (safety rails, not normal control flow):
- `local_closure_steps`: 500 (same-file symbols per closure)
- `frontier_symbol_visits`: 2000 (cross-file `(canonical_id, exported_name)` pairs)
- `builder_expansion_steps`: 5000 (symbolic expansion steps)

When a budget trips, the system returns a structured `BudgetExceededFailure` with domain, limit, actual count, and context -- never silently normalizes.

**Host integration**: `HostFrontierAdapter` (`host_resolve.rs`) bridges the frontier to the real `VerterHost`, resolving through compile_cache deps, imported_dependency_cache deps, then workspace fallback. Route discovery runs exclusively through the frontier/final-target path; once the defining symbol is selected, the shared source-body evaluator materializes the final `ResolvedElements`.

**Key files:**

| File | Purpose |
| --- | --- |
| `crates/verter_session/src/resolver_core/shallow_file_state.rs` | ShallowFileState, ExportTarget, ShallowTypeSymbol, ExternalSymbolRef, ResolutionBudgets, local_closure() |
| `crates/verter_session/src/resolver_core/external_type_frontier.rs` | ExternalTypeFrontier, FrontierHost trait, PendingExternalSymbol, ResolvedSymbol, RouteKind |
| `crates/verter_session/src/host_resolve.rs` | HostFrontierAdapter, resolve_external_type_from_loaded_files_in_view() |
| `crates/verter_session/src/frontier_tests.rs` | Behavioral invariant tests (diamond dedup, barrel ordering, cycle termination, budget enforcement, etc.) |

## Native Type Solver

`verter_semantic::analysis::type_solver` is the sole authority for all type expansion. It handles `defineProps<T>()`, `defineEmits<T>()`, component-meta type resolution, and cross-file generic instantiation.

**Architecture -- two separate structs to avoid cloning:**

- `QueryArena` -- append-only immutable node store. Nodes are interned as `NodeId` (u32). Once allocated, nodes are never mutated.
- `SolverCaches` -- mutable memoization tables (relation, instantiation, keyspace, member). Separate from arena so the relation engine can hold `&QueryArena` and `&mut SolverCaches` simultaneously.

**Pipeline:** `TypeExpr -> lower -> QueryArena -> resolve_node -> project_to_type_expr -> TypeExpr`

**Host boundary:** `TypeSolverHost` trait (in `type_solver::host`) is the seam between `verter_session` (file readiness, frontier, caches) and the solver (arena, relations, projections). The solver never reopens route discovery -- it only accepts resolved root identities. Two implementations exist: `SessionSolverHost` in `resolver_core/solver_host.rs` (production, bridges host caches) and `EvalEnvSolverHost` in `type_solver/host.rs` (standalone, wraps an `EvalEnv`'s type_symbols for local-only resolution without a session).

**Request-scoped engine ownership:** `TypeQueryEngine` is the single request-scoped mutable solver owner for component-meta queries. One engine is created per `get_component_meta()` request and shared across all solves in that request. Declaration-scoped solves reuse the shared engine via `TypeQueryEngine::solve_scoped()` -- they share the arena, instantiation cache, and solver caches while using a different `TypeSolverHost` (scoped to the declaration file). The `solve_scoped` method partitions the op-cache key and bare-name root_identity cache by `scope_canonical_id` so results from one declaration scope do not alias with results from another scope. Do not construct fresh `TypeQueryEngine` instances for declaration-scoped solves.

**Scope-aware bare-name caching:** The `SolverCaches.root_identity` cache uses `(canonical_id, symbol_name)` as its key. For bare-name lookups (empty `canonical_id`), `resolve_root_identity_cached()` in `solve.rs` substitutes the `SolveState.scope_canonical_id` as the cache key. This prevents cross-scope poisoning: a bare-name miss in scope A does not prevent scope B from resolving the same name.

**Ambient/global environment (deferred):** Names like `Function`, `Promise`, `ThisType`, and DOM globals currently fall through as unresolved bare-name misses. The long-term target is explicit ambient/global declaration support as a first-class input to the host/engine boundary, modeled from the project's TypeScript configuration (`compilerOptions.lib`). This is deferred correctness work, not a speculative enhancement -- it remains a required follow-up in the same architectural track as the shared-engine cutover.

**Declaration context propagation:** `PreparedTypeDecl` and `PreparedValueDecl` carry a `name_resolution: FxHashMap<String, ResolvedRootIdentity>` field that maps bare names appearing in their bodies to resolved root identities. Built at preparation time from the defining file's local and import scope (local deps -> same-file identity, external deps -> resolved canonical_id via dep_edges). The solver's `SolveState` maintains `type_decl_context_stack` and `value_decl_context_stack`. When `resolve_prepared_ref` enters a declaration body, it pushes the prepared decl onto the stack. The `resolve_name_in_context` helper checks only the INNERMOST context (topmost stack entry) -- bare names in an imported type body resolve in that declaration's defining file scope, not in parent scopes.

**Barrel re-export following:** `prepared_type_decl_in_view` and `prepared_value_decl_in_view` follow barrel re-exports when a symbol is not found in the target file's local prepared decl cache. For named re-exports (`export { Foo } from './bar'`), the source specifier is resolved and the lookup continues in the target file. For wildcard re-exports (`export * from './bar'`), all wildcard sources are tried in declaration order with depth-limited recursion (max 20 hops).

**Namespace import resolution:** `SessionSolverHost::root_identity` handles dotted names (`Ns.Member`) by splitting on the first dot, resolving the prefix through import bindings, and looking up the member in the resolved file's prepared decl cache.

**Exactness model:** `ExactConcrete | ExactSymbolic | Incomplete` -- replaces old `Exact | LowerBound | OpaqueFallback`. Execution status (`Completed | Cancelled | HardStop`) is tracked separately from semantic exactness.

**Operators implemented:** keyof, indexed access, conditionals (with distributive distribution + infer binding collection), mapped types (with key remapping via `as` clause), template literals (iterative cartesian expansion with 10k guard), typeof, built-in utilities (Partial, Required, Readonly, Pick, Omit, Record, Extract, Exclude, NonNullable, ReturnType, Parameters, ConstructorParameters, InstanceType, Awaited, Uppercase, Lowercase, Capitalize, Uncapitalize, NoInfer).

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

- There is one shared cross-file type resolver with two modes. Consumer-specific ownership rules live in `/component-meta`.
- That resolver has exactly two modes:
  - `Type`: resolve the requested symbol identity and canonical source location only. Do not expand the shape.
  - `Expanded`: resolve the same symbol through the same traversal, then materialize the expanded shape / expanded text.
- Do not walk unrelated imports from the same file.
- Do not treat plain imports as implicit exports.
- Keep direct re-exports (`export { X } from`, `export * from`) as an explicit separate path.
- Parsing a `.ts`/`.js`/declaration file for type resolution must cache discovered symbol name -> canonical location mappings.
- Re-exported names and barrel hops must also be cached once discovered. If traversal follows `export * from './foo'`, cache that result so later lookups do not rescan the same barrel chain.

If a file imports 20 modules but the requested macro type only references `AvatarProps` and `IconProps`, external resolution must only traverse those reachable dependencies.

**TS-first resolution priority:** TypeScript types always take priority over JavaScript files when resolving ambiguous dependency candidates. Verter is a type-strict compiler that relies on TS typing for correctness. JS files should only be used as a last resort when no TS type definition is available. When `DependencyResolution.possible_canonical_ids` contains multiple candidates, use `effective_target()` which selects the single highest-priority candidate: `.d.ts` > `.d.cts` > `.d.mts` > `.ts` > `.tsx` > `.js` > `.jsx` > `.cjs` > `.mjs`. Do not try remaining candidates if the selected one lacks the needed type -- treat as not found.

**Owned resolution is bounded by `workspace_root`:** For owned and project-scoped resolution, `node_modules` and package `#imports` ancestor walks stop at `IdeProjectConfig.workspace_root`. In monorepos, `workspace_root` may be above `project_root` to reach hoisted `node_modules`. In compat `createCheckerByJson()`, `workspace_root == project_root`. Unowned resolution (no owning project) remains unbounded. The boundary is passed via `ancestor_dirs(path, Some(&workspace_root))` and `ancestor_dirs_from_dir(start_dir, Some(&workspace_root))` in `verter_workspace::resolver`.

## Frontier Engine Tests

Tests in `crates/verter_session/src/frontier_tests.rs` cover diamond dedup, barrel ordering, cycle termination, budget enforcement, export routing, and store-view consistency. Run with `cargo test --package verter_session frontier_tests`.
