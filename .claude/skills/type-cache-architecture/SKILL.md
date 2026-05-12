---
name: type-cache-architecture
description: "Verter fact-based cache architecture — env hash split, FileArtifactStore, content-addressed caches, query-identity caches, R1–R29 architectural rules, module augmentation, multi-candidate storage"
---

# Verter Fact-Based Cache Architecture

> AMENDMENT 2026-05-11-A — the integration branch for the fact-based cache
> overhaul is `refactor/semantic-db-overhaul` (the user-mandated rename of
> the older `fix/cutover-review-findings` branch). The two are at the same
> baseline; the swap is documentation-only.

This skill is the reference for the post-cutover cache architecture in
`verter_session`. Every architectural rule referenced by tests, plans,
and code comments resolves here. See
`docs/arch/fact-based-cache.md` for the per-cache-layer key composition
table.

## Why this architecture exists

Verter's IR is shallow-Refs + content-hashed parse + typed-IR-only
resolver. The cache substrate must match: **lazy-validate,
fact-granular, read-side authoritative**. The old model was
**eager-evict, file-coarse, write-side authoritative**, which produced
~100 ms cold latency that failed to warm across the loop. The
rebuilt architecture moves cache validation from a write-side
side-effect into a read-side fact check.

Target end-state rule:

> Source updates produce semantic fact diffs. Cache entries are
> validated against the exact facts they read. Reuse is the default;
> recomputation is the exception. Overlays are views over the base
> host, not host mutations. Fact validation is the cache-correctness
> oracle; a view/content token remains the concurrency oracle.
> Shared semantic materialisations are keyed by resolved declaration
> **slot** identity (content-free); versioned declaration identity
> lives inside cached values.

## Architectural rules (R1–R29)

### Mutation semantics

**R1.** `host.upsert(canonical, source)` is a cache-state no-op iff the
`(canonical, content_hash, parse_env_hash, resolve_env_hash,
lib_env_hash)` quintuple is unchanged. No cache mutation, no semantic
invalidation, no `bump_store_view_epoch`, no scheduler round-trip
beyond the quintuple check.

**R2.** `upsert` means "the source changed." Cache eviction is an
explicit method with a stated scope; never a side effect of `upsert`.

**R3.** Eager dependent cache invalidation is forbidden. Cache entries
validate on read against the exact facts they recorded; staleness is
detected lazily. `smart_invalidate_dependents`, `evict_canonical`
cascades from upsert, and `*_db::invalidate_canonical` as a public API
are all banned in production code paths.

**R4.** Source-content changes produce a semantic fact diff (publishable
via `compute_upsert_changes_from_parse` for LSP / observability
callers). Invalidation is not propagated; the fact diff is the public
observability of what changed.

### Cache identity & validation

**R5.** Caches divide into two families:

- **Content-addressed artifact caches** — `FileArtifactStore`,
  `ResolvedImportFacts`, typed-IR resolve, `MemberSemanticFactStore`,
  `MemberDisplayFactStore`, `ModuleAugmentationIndex`. Keys include
  `content_hash` or a derived parse-stable hash.
- **Query-identity caches** — `RouteDb`, `MaterializeStructureDb`,
  `RefCycleResultDb`, `SemanticGraphStore` query nodes,
  `ComponentMetaResultDb`. Keys exclude version hashes; concurrent
  variants coexist as candidates inside one slot.

Version rooting for query-identity caches lives **inside the cached
value** as `VersionedDeclIdentity` + `fact_dep_signature`, not in
the key.

**R6.** Cache keys never include `fact_dep_signature` or content /
version hashes (for query-identity caches). Signatures and version
info live on the cached value.

**R7.** Shared semantic materialisations key by
`ResolvedDeclSlotIdentity` (content-free) as the cache slot identity.
The cached value carries `VersionedDeclIdentity` (with content /
version info) for fence seeding, scope identity, and version-aware
semantic operations. Cross-owner reuse is the architectural invariant
— a `ChatMessageProps` reached from N owners materialises once.

```rust
struct ResolvedDeclSlotIdentity {
    defining_canonical: Arc<str>,
    merged_symbol_name: InternedName,
    symbol_space: SymbolSpace,
    project_identity: ProjectId,
    type_env_hash: Hash16,
    lib_env_hash: Hash16,
}

struct VersionedDeclIdentity {
    slot: ResolvedDeclSlotIdentity,
    content_hash: Hash16,
    parse_env_hash: Hash16,
    merged_parts: SmallVec<[(DeclPartId, FactHash); 2]>,
}
```

`merged_symbol_name` is **stable across declaration reordering and
TypeScript declaration merging**. Same-scope `interface Foo` parts
merge into one `merged_symbol_name`. Per-part fingerprints live in
`VersionedDeclIdentity.merged_parts` for diagnostics and overload
surfacing — they are NOT the cache validation oracle.

**R8.** Only final per-owner payloads (`ComponentMetaResultDb`) are
owner-keyed: `(owner_canonical, project_identity, type_env_hash,
lib_env_hash, projection_mode, options_hash)`.

**R9.** Reuse is the default; recomputation is the exception. A cache
miss under steady-state load is treated as evidence of a fact-graph
bug or a real semantic change — never as a routine event.

### Fact model

**R10.** Facts use stable `FactKey`s, not `Vec` indices. Removed facts
validate as misses (registry lookup returns `None`). Reordering a file
does not change `FactKey`s.

**R11.** Every binding-naming fact key carries `SymbolSpace ∈ {Type,
Value, Namespace}`. **`BothTypeValue` is forbidden.** A `class Foo`
declaration that occupies both spaces emits TWO distinct facts:
`Export("Foo", Type)` and `Export("Foo", Value)`.

**R12.** Facts split strictly by domain. **Parse-domain facts
(`FileFacts`, parse_env keyed) never reference resolved canonicals.**
**Resolve-domain facts (`ResolvedImportFacts`, `RouteDb`, resolve_env
keyed) carry resolutions.** The producer of a parse-domain fact does
not run the resolver; it only emits the syntactic shape.
`FactKey::domain()` routes validator lookups to the correct store.

**R13.** Each `Fact` carries `semantic_hash` (alpha-normalised
structural fingerprint) and `display_hash` (cosmetic — JSDoc, param
names, comments). Signatures record per-observation `lane: Semantic |
Display` so cosmetic edits invalidate display-bearing materialisations
only. Storage of semantic vs display facts is **physically split** at
the cache layer (`MemberSemanticFactStore` keyed on
`parse_stable_hash`, `MemberDisplayFactStore` keyed on `content_hash`)
so a cosmetic edit hits only the display store and does not recompute
semantic facts.

**R14 (path-precise).** `Export(name).semantic_hash` is computed over
the **export's body alone**, with every cross-decl reference (same-file
local decl, same-file member, imported binding) recorded as a
**reference-shape edge** by name + space, NOT by inlining the
referent's body. Editing an unused local in the same file does NOT
invalidate consumers of an export that does not reach it. Editing a
member that `Pick<Foo, "a">` does not select does NOT invalidate that
consumer.

**R15.** `SyntacticExportSet` (parse-domain) records local exports and
bare re-export specifiers only — no resolution. `EffectiveExportSet`
(resolve-domain, owned by `RouteDb`) records post-wildcard-expansion,
post-module-augmentation visible names with resolved canonicals. The
two cannot be merged.

**R16.** Semantic fingerprints are alpha-normalised structural hashes.
Source-text hashes, span-based hashes, position offsets, or any hash
that changes under cosmetic edits (whitespace, comments, generic param
rename, declaration reordering) are forbidden as semantic hashes.
Cosmetic changes live in `display_hash` only.

### Sessions & concurrency

**R17.** Sessions are views over the base host. A `SessionView` never
mutates the host. `host.upsert` is not called from any query path.
Overlay artifacts are stored under the overlay's content hash and
coexist with base artifacts under different keys. Byte-identical
overlay collapses to base hash automatically.

**R18.** `SessionView` is passed explicitly through `ResolverContext`.
Thread-local "current view" globals remain forbidden.

**R19.** Fact validation is the **cache-correctness oracle**.
`StoreViewCompatToken` is the **concurrency oracle**: singleflight lane
separation, mid-query change detection, write admission against
superseded computations. The two are orthogonal and must not be
conflated.

**R20.** Multi-candidate storage isolates concurrent overlay variants
in the same query-identity slot. Default cap = 4 candidates per slot.
**Eviction is insertion-order (FIFO) on write only.** No LRU bookkeeping
on read; warm reads are `&self` shared borrows with zero atomic write
or lock contention. Concurrent sessions never overwrite each other's
results.

Concrete substrate:

- Outer table: `DashMap<K, Arc<CacheEntry<V>>>`.
- `CacheEntry<V> { candidates: ArcSwap<SmallVec<[Arc<Candidate<V>>; 2]>> }`.
- Admission writes use RCU; write contention serialised by
  `StoreViewCompatToken`-keyed singleflight.

Bounded signature size: a `fact_dep_signature` is capped at 1024
entries. Beyond that the producer consumes a hierarchical fact
(downstream materialisation `semantic_hash`) instead of flattening
transitive facts. Overflow produces `FactSignatureOverflow` audit
event + candidate is admitted as `NonCacheable`.

### Environment & GC

**R21.** Environment hashes split into **five** orthogonal dimensions
(this is the most-cited rule):

| Hash | Captures |
|---|---|
| `parse_env_hash` | Parser / SFC / compiler feature flags, syntax mode, language target |
| `resolve_env_hash` | `base_url`, `paths`, workspace aliases, project references, module resolution mode, package `exports`/`imports`, default extension order |
| `type_env_hash` | TS semantic options that change type meaning (`strict`, `noImplicitAny`, etc.) |
| `lib_env_hash` | TS built-in lib selection (`lib.dom.d.ts`, `lib.es*.d.ts`), `types`, `typeRoots`, registered ambient libs, global / module-augmentation corpus identity |
| `project_identity` | Project root, tsconfig path, provider root, workspace root, membership / owner selection |

**Scoping rule for `lib_env_hash` (per R21).** `lib_env_hash` enters a
cache key only when the cached value depends on lib data:

- `ResolvedImportFacts` (base import resolution) does **NOT** include
  `lib_env_hash`. A lib update does not change where `./theme` resolves.
- `RouteDb` per-name and effective-set caches **DO** include
  `lib_env_hash` because module augmentations stitch into the
  effective surface.
- Typed-IR resolve, `MaterializeStructureDb`, `RefCycleResultDb`,
  `SemanticGraphStore`, `ComponentMetaResultDb` **DO** include
  `lib_env_hash` because semantic meaning depends on intrinsic
  types (`Array<T>`, `HTMLElement`, etc.).

A single bundled `project_config_hash` is forbidden.

The five hash functions live on `verter_workspace::resolver::IdeProjectConfig`
+ per-call inputs surfaced through `EnvHashInputs<'_>`:

```rust
cfg.parse_env_hash(&inputs);   // Hash16
cfg.resolve_env_hash(&inputs); // Hash16
cfg.type_env_hash(&inputs);    // Hash16
cfg.lib_env_hash(&inputs);     // Hash16
cfg.project_identity();        // Hash16
```

Each function mixes a per-dimension salt so the five hashes derived
from identical baseline state never collide.

**R22.** Eviction is memory-bound, not correctness-bound. The reverse
import graph is content-addressed and serves reachability GC + LSP
affected-files reporting + diagnostics; it is never wired to cache
invalidation. Live-content reachability + a global LRU floor are the
sole GC mechanisms.

### Observability & cost

**R23.** Audit events emitted by this refactor on cache subsystem call
paths use typed `StructuredAuditEvent` variants (`FileArtifactCache`,
`FactRegistryWrite`, `FactValidationSummary`, `ExportRouteResolved`,
`FactSignatureAdmissionRefused`, `FactSignatureOverflow`,
`ModuleAugmentationStitched`, `ModuleAugmentationIndexShape`,
`CacheDrainedAtUpsert`). `Custom` events for the new emissions on
these paths are forbidden.

The `CacheDrainedAtUpsert { layer, canonical_id }` variant fires at
every cache-cascade drain site reached by the full
`host.upsert(...)` path. The quintuple-unchanged fast path (R1)
emits ZERO `CacheDrainedAtUpsert` events; that absence is the
direct read-side proof that the fast path is a cache-state no-op.
The variant is emitted via the in-tree helper
`verter_session::host_manage::push_cache_drained_at_upsert(layer,
canonical_id)` so all drain sites in `host_upsert.rs` flow through
one construction point. Layer-string vocabulary
(each is a `&'static str`): `compile_cache_overrides`,
`compile_slots`, `derived_raw_cache`, `dependency_cache`,
`resolver_runtime`, `project_type_store`, `resolved_type_cache`,
`semantic_invalidate`, `workspace_parsed_edges`,
`store_view_epoch`.

**R24.** Warm cache validation is counter-only — zero allocation, zero
structured payload emission per hit. Structured events emit only on
cold / stale / admit-refused paths or when an observer is explicitly
sampling. Warm-hit validation must stay under 50µs p99 and must not
allocate (asserted via test-allocator counter).

**R25.** The fact read tracer (`FactReadSet` in `ResolverContext`) is
active on cold-compute and write-admission paths only. Warm hits
validate stored signatures directly without instantiating a tracer.

### Substrate, stack-safety, granularity

**R26.** `ValidatedFactCache<K, V>` is the substrate. Multi-candidate
(R20), the extended `FactVersionRef` variants, and the new lane / space
metadata land **inside** this primitive — not as a parallel cache type.
No second fact-validation infrastructure may be introduced.

`StoreView` trait surface evolves with explicit per-domain validator
methods routed via `FactKey::domain()`:

```rust
trait StoreView {
    fn compat_token(&self) -> StoreViewCompatToken;
    fn validates(&self, fact: &FactVersionRef) -> bool;
    fn validates_parse_domain(&self, fact: &ParseFactRef) -> bool;
    fn validates_resolve_imports_domain(&self, fact: &ResolveImportsFactRef) -> bool;
    fn validates_route_surface_domain(&self, fact: &RouteSurfaceFactRef) -> bool;
}
```

The dispatch table is bounded by `FactDomain` (3 variants), not by
`FactKey`. Adding a new `FactKey` extends the per-domain `*FactRef`
enum but does NOT widen the trait.

**R27.** All semantic fingerprint computation is **stack-safe**:
implemented as an explicit worklist with a `VisitedSet`. Cycles emit
a stable `CycleRef(visit_index)` placeholder. Visit order is canonical:
lexicographic by `(name, symbol_space)` at each unresolved-neighbor
expansion; depth-budget tie-break by `(canonical, name, symbol_space)`.
`CycleRef` placeholder identity is invariant under source-text
reordering. Depth budget = 64; over-budget paths emit
`Opaque(BudgetExceeded)` and the cache entry is admitted as
`NonCacheable`.

**R28.** Path-precise fact granularity is mandatory. Every cache that
observes a member, local, import edge, or member-presence must observe
the corresponding `Member` / `LocalDecl` / `MemberPresence` /
`ImportRef` fact directly. Folding a whole-file or whole-export
closure into a single `semantic_hash` is forbidden.

**`MemberPresence` vs `Member` two-fact model** — they have orthogonal
purposes and orthogonal cache lifecycles. Consumers that select a
specific member observe BOTH:

| Fact | Purpose | Lifecycle | `semantic_hash` content |
|---|---|---|---|
| `MemberPresence(exporter, name, space)` | Existence + header | Phase 1 eager (parse-time) | `(name, member_kind, exporter_qualifier_salt)` |
| `Member(exporter, name, space)` | Canonical reusable member body fingerprint | Phase 2 lazy | Body alpha-normalised TypeExpr with cross-decl references kept as references |
| `MemberShape(exporter, space)` | Whole-surface fingerprint | Phase 1 eager | `sorted_by_name([(name, kind)])` |

Why both `MemberPresence` and `Member` exist: their cache lifecycles
differ. `MemberPresence` is cheap and eager — its body fingerprint is
NOT included, so adding member `b` does not force re-walking member
`a`'s body. `Member` is the canonical reusable navigation result —
computed once per `(parse_stable_hash, exporter, name, space)` and
reused for every consumer that walks into the member body.

Literal-key projections like `Pick<Foo, "a">` observe **`MemberPresence(Foo, "a")` + `Member(Foo, "a")`**:

- Adding `Foo.b` → new `MemberPresence(Foo, "b")` emitted; existing
  `MemberPresence(Foo, "a")` unchanged; `Member(Foo, "a")` unchanged
  → consumer NOT invalidated. `MemberShape` changes but is not in
  this consumer's signature.
- Editing `Foo.a` body → `MemberPresence(Foo, "a")` unchanged (header
  invariant); `Member(Foo, "a")` changes → consumer IS invalidated.
- Removing `Foo.a` → `MemberPresence(Foo, "a")` becomes a registry
  miss → consumer IS invalidated.

**R29 (Module augmentation).** Module augmentation is a fact-graph
completeness requirement. Parse-domain emits a syntactic fact
`ModuleAugmentation { specifier, augmented_name, space }` per
`declare module 'x' { ... }` block. Resolution of the augmentation
target happens at the resolver stage, producing a typed
`AugmentationTargetKey`:

```rust
enum AugmentationTargetKind {
    ExternalSpecifier(InternedSpecifier),     // declare module "vue" {}
    ResolvedRelativeCanonical(Arc<str>),      // declare module "./local" {}
    WildcardAmbient(InternedGlobPattern),     // declare module "*.css" {}
    GlobalAugmentation,                       // declare global {}
}

struct AugmentationTargetKey {
    project_identity: ProjectId,
    resolve_env_hash: Hash16,
    lib_env_hash: Hash16,
    target: AugmentationTargetKind,
}
```

A `ModuleAugmentationIndex { entries: DashMap<AugmentationTargetKey,
Arc<AugmenterSet>> }` lives on `FileArtifactStore`, providing the
inverse lookup "which augmenters target X under env E?".
Project / env isolation prevents cross-project poisoning.

**Index population semantics.** The augmentation index is populated
**incrementally** as files enter `FileArtifactStore`. There is NO
workspace-wide eager scan; out-of-program files contribute nothing —
matching TypeScript's own augmentation visibility rule. A reachability-
anchored query that needs `EffectiveExportSet(specifier)` triggers
`ResolvedImportFacts` resolution for the relevant import graph, which
pulls in any augmenter that is part of that reachability.

**Augmenter-set identity fact.** A parse-domain-derived fact
`ModuleAugmentationIndexShape { target: AugmentationTargetKey,
augmenter_set_fingerprint }` is observed by
`EffectiveExportSet(specifier)` consumers.
`augmenter_set_fingerprint = stable_hash(sorted([(augmenter_canonical,
augmenter_parse_stable_hash)]))`. Adding / removing an augmenter
changes the fingerprint → existing `EffectiveExportSet` candidates
invalidate.

## Cache layer key composition

See `docs/arch/fact-based-cache.md` for the canonical per-cache-layer
key composition table. Summary:

| Layer | Family | Key dimensions |
|---|---|---|
| `FileArtifactStore` | Content-addressed | `canonical, content_hash, parse_env_hash, parser_version` |
| `ModuleAugmentationIndex` (on `FileArtifactStore`) | Content-addressed | `project_identity, resolve_env_hash, lib_env_hash, target` |
| `ResolvedImportFacts` | Content-addressed | `canonical, content_hash, parse_env_hash, resolve_env_hash, resolver_version` (**no `lib_env_hash`** — R21) |
| Typed-IR resolve | Content-addressed | `canonical, content_hash, parse_env_hash, type_env_hash, lib_env_hash, parser_version` |
| `MemberSemanticFactStore` | Content-addressed | `canonical, parse_stable_hash, parse_env_hash, exporter, member_name, symbol_space` |
| `MemberDisplayFactStore` | Content-addressed | `canonical, content_hash, parse_env_hash, exporter, member_name, symbol_space` |
| `RouteDb` per-name | Query-identity (multi-candidate) | `provider_canonical, exported_name, symbol_space, resolve_env_hash, lib_env_hash, resolver_version` |
| `RouteDb` effective barrel surface | Query-identity (multi-candidate) | `provider_canonical, resolve_env_hash, lib_env_hash, resolver_version` |
| `MaterializeStructureDb` | Query-identity (multi-candidate) | `MaterializationCacheKey { decl: ResolvedDeclSlotIdentity, projection_path, projection_mode, normalized_type_args, options_hash }` |
| `RefCycleResultDb`, `SemanticGraphStore` query nodes | Query-identity (multi-candidate) | `ResolvedDeclSlotIdentity` (slot) + `VersionedDeclIdentity` inside value |
| `ComponentMetaResultDb` | Query-identity (multi-candidate) | Owner identity (per R8) |

## Key concrete files

- `crates/verter_workspace/src/env_hash.rs` — five env-hash functions on
  `IdeProjectConfig` + `EnvHashInputs<'_>`.
- `crates/verter_session/src/file_artifact_store.rs` — `FileArtifactStore`,
  `FileArtifactKey`, `FileArtifacts`, `AugmentationTargetKey`,
  `AugmentationTargetKind`, `AugmenterSet`, `FileFacts`, `ParsedEdges`,
  `ModuleAugmentationFact`, `ProjectIdentity`, `InternedSpecifier`,
  `InternedName`, `InternedGlobPattern`, `SymbolSpace`.
- `crates/verter_session/src/parse_stable_hash.rs` —
  `compute_parse_stable_hash(&IndexedReady) -> Hash16`.
- `crates/verter_session/src/resolver_core/mod.rs` —
  `ValidatedFactCache`, `FactVersionRef`, `StoreView`,
  `StoreViewCompatToken`.
- `crates/verter_session/src/semantic_query.rs` — `DeclIdentity`
  (migrating to `ResolvedDeclSlotIdentity`).

## Discriminating tests

- `crates/verter_workspace/src/env_hash_tests.rs` — 15 unit tests on
  the env-hash split (R21).
- `crates/verter_session/src/file_artifact_store_tests.rs` — 13 unit
  tests on the store (R5, R6, R28, R29).
- `crates/verter_session/tests/env_hash_isolation.rs` — 4 R21
  scoping rule tests.
- `crates/verter_session/tests/cache_key_invariants.rs` — 6 R5 / R6
  key-shape tests.
- `crates/verter_session/tests/parse_stable_hash_invariance.rs` — 9
  cosmetic-invariant + decl-shape-discriminating tests.
- `crates/verter_session/tests/file_artifact_store_smoke.rs` — 4
  consumer-side smoke tests.

## Related skills

- `/architecture` — high-level module map
- `/host-session` — `VerterHost` lifecycle + `ProjectTypeStore`
- `/type-resolution` — cross-file type resolver + macro traversal
- `/component-meta` — downstream consumer
- `/audit-infrastructure` — `StructuredAuditEvent` extensions
