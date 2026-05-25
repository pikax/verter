# Fact-based cache architecture

This document is the per-field audit + per-cache-layer key composition
table for the fact-based cache architecture. The full rule set
(R1–R29) lives in the `/type-cache-architecture` skill.

For the next-generation cache runtime and performance migration plan, see
[`cache-runtime-overhaul-plan.md`](cache-runtime-overhaul-plan.md).

> AMENDMENT 2026-05-11-A — the integration branch for this work is
> `refactor/semantic-db-overhaul` (renamed from the older
> `fix/cutover-review-findings`). The two share the same baseline; the
> swap is documentation-only.

> AMENDMENT 2026-05-12-B — Four rows in the Legacy Deletions table
> were verified as "never existed at the substrate baseline" and
> should be read as **forbidden patterns to not introduce** rather
> than retirements:
>
> - **Row 2:** `project_config_hash` (single bundled hash) — never
>   existed. The 5-way env-hash split is a net-new addition.
> - **Row 3:** ambient-lib mixing inside `resolve_env_hash` — never
>   existed. R21 scoping rule prevents future introduction.
> - **Row 8:** `BothTypeValue` variant of `SymbolSpace` — never
>   existed. `SymbolSpace` was `{Type, Value}` at baseline; the
>   3-variant `{Type, Value, Namespace}` form was added directly.
> - **Row 21:** `decl_index_in_file` (round-1 proposal) — never
>   introduced; `merged_symbol_name` + `merged_parts` chosen
>   instead.

> AMENDMENT 2026-05-12-C — The substrate uses
> `fact_dep_signature: Arc<[FactVersionRef]>` (not
> `Arc<[ObservedFact]>` as some earlier plan text spec'd). The two
> are structurally equivalent — `FactVersionRef::Parse(ParseFactRef
> { canonical_id, key, lane, expected_hash })` carries the same
> fields as the older `ObservedFact { canonical, key, lane,
> expected_hash }` shape (the only nominal difference is `String`
> vs `Arc<str>`). `FactVersionRef` is the chosen shape because it
> aligns with the per-domain dispatch surface (R26's
> `validates_parse_domain` / `validates_resolve_imports_domain` /
> `validates_route_surface_domain`).

> AMENDMENT 2026-05-12-D — The whole-hash retirement audit-test at
> `crates/verter_session/tests/whole_hash_migration_audit.rs` is a
> **count-bounded inventory** (each enumerated read site asserted
> to have ≤ N occurrences), not a per-site absence assertion. The
> R20 multi-candidate substrate + `VersionedDeclIdentity` +
> `fact_dep_signature` ship as the SUBSTRATE; consumer-path call
> sites migrate incrementally onto their documented replacements
> (`VersionedDeclIdentity.content_hash` for scope;
> `fact_dep_signature` for hashing; `SessionView::content_hash_for`
> for routed-expr tracking).

## `IdeProjectConfig` 5-way env hash audit (R21)

`IdeProjectConfig` is the source of truth for the resolve-domain and
project-identity dimensions of the env hash. The remaining dimensions
(parse, type, lib) take per-call inputs through `EnvHashInputs<'_>`
because they are surfaced from data not owned by `IdeProjectConfig`
(parser-flag set, TS compiler options, TS lib selection, ambient
corpus fingerprint).

| Field | Source | Dimension | Reason |
|---|---|---|---|
| `root` | `IdeProjectConfig` | `project_identity` | Identifies WHICH project the config describes. Same paths under a different root MUST produce a distinct identity. |
| `workspace_root` | `IdeProjectConfig` | `project_identity` | A3 sub-plan: two workspaces both containing `tsconfig.json` MUST produce distinct project identities; `workspace_root` is the discriminator. |
| `tsconfig_path` | `IdeProjectConfig` | `project_identity` | `Configured` vs `Fallback` projects share roots in some multi-tsconfig layouts; the `tsconfig_path` discriminates. |
| `provider_root` | `IdeProjectConfig` | `project_identity` | The provider-graph root the type provider sees; distinct provider roots are distinct projects. |
| `membership` | `IdeProjectConfig` | `project_identity` | `MatchAll` vs `IncludeExclude { files, include, exclude }` membership filters select different file sets. A membership change is a project-identity change, not a resolve change. |
| `workspace_aliases` | `IdeProjectConfig` | `resolve_env` | Affect HOW imports resolve. Order-sensitive (alias precedence matters). |
| `compiler_options.base_url` | `IdeProjectConfig` | `resolve_env` | Path-resolution input. |
| `compiler_options.paths` | `IdeProjectConfig` | `resolve_env` | Path-resolution input. |
| `references` | `IdeProjectConfig` | `resolve_env` | tsconfig project-reference graph; affects which projects participate in module resolution. |
| `EnvHashInputs.parser_flags` | per-call | `parse_env` | Parser feature flags + SFC compiler flags (e.g., `preserve_jsx`, `vue_macros_v3`). |
| `EnvHashInputs.resolve_extensions` | per-call | `resolve_env` | Extension priority for extensionless specifiers (order-sensitive). |
| `EnvHashInputs.type_strict` | per-call | `type_env` | `strict` mode changes type meaning. |
| `EnvHashInputs.type_no_implicit_any` | per-call | `type_env` | `noImplicitAny` changes type meaning. |
| `EnvHashInputs.lib_names` | per-call | `lib_env` | Selected TS `lib*.d.ts` set (e.g., `lib.dom.d.ts`, `lib.es2022.d.ts`). |
| `EnvHashInputs.type_roots` | per-call | `lib_env` | `typeRoots` directory list for ambient `@types`. |
| `EnvHashInputs.ambient_corpus_fingerprint` | per-call | `lib_env` | Identity of the resolved ambient library corpus (registered globals, module-augmentation declarations). |

### R21 scoping rule (the dimension that does NOT enter every key)

`lib_env_hash` enters a cache key only when the cached value depends
on lib data:

- `ResolvedImportFacts` does **NOT** include `lib_env_hash`. A lib
  update does not change where `./theme` resolves.
- `RouteDb`, typed-IR resolve, `MaterializeStructureDb`,
  `RefCycleResultDb`, `SemanticGraphStore`, `ComponentMetaResultDb`
  **DO** include `lib_env_hash` because semantic meaning depends on
  intrinsic types (`Array<T>`, `HTMLElement`, etc.) and / or module
  augmentations stitching into the effective surface.

The `FileArtifactStore` key does **NOT** carry `lib_env_hash`
(parse-domain only). The `AugmentationTargetKey` on the
`augmentation_index` skeleton on `FileArtifactStore` **DOES** carry
`lib_env_hash` because module augmentations are looked up against
the lib + ambient corpus.

## Cache layer key composition (final-state)

| Layer | Family | Key | Validation |
|---|---|---|---|
| `FileArtifactStore` | Content-addressed | `(canonical, content_hash, parse_env_hash, parser_version)` | Content-addressed; never invalidated |
| `ModuleAugmentationIndex` (on `FileArtifactStore`) | Content-addressed | `AugmentationTargetKey { project_identity, resolve_env_hash, lib_env_hash, target }` | Content-addressed; incrementally populated. **Base-only** — the key has no base/session discriminator, and the cold scan + refresh filter to base (`is_legacy`) artifacts, so an overlay never contributes |
| `ResolvedImportFacts` | Content-addressed | `(canonical, content_hash, parse_env_hash, resolve_env_hash, resolver_version)` — **no `lib_env_hash`** (R21) | Content + resolve-env addressed |
| Typed-IR resolve | Content-addressed | `(canonical, content_hash, parse_env_hash, type_env_hash, lib_env_hash, parser_version)` | Content + type-env + lib addressed |
| `MemberSemanticFactStore` | Content-addressed | `(canonical, parse_stable_hash, parse_env_hash, exporter, member_name, symbol_space)` | Keyed on `parse_stable_hash` so cosmetic edits do not recompute |
| `MemberDisplayFactStore` | Content-addressed | `(canonical, content_hash, parse_env_hash, exporter, member_name, symbol_space)` | Keyed on `content_hash`; cosmetic edits recompute display only |
| `RouteDb` per-name resolution | Query-identity (multi-candidate) | `(provider_canonical, exported_name, symbol_space, resolve_env_hash, lib_env_hash, resolver_version)` | Fact-validated; stable misses preserved |
| `RouteDb` effective barrel surface | Query-identity (multi-candidate) | `barrel_canonical` | Fact-validated per candidate via `BarrelRouteSurface.fact_dep_signature: Arc<[FactVersionRef]>` (Stage 6c) |
| `RouteDb` effective export set | Query-identity (multi-candidate) | `EffectiveExportSetKey { provider_canonical, project_identity, resolve_env_hash, lib_env_hash }` (R21 — lib_env enters because module augmentations live in libs) | Fact-validated per candidate via `EffectiveExportSetEntry.fact_dep_signature`, which records `RouteSurface(ModuleAugmentationIndexShape)` + per-contributor `FileWholeHash` anchors (R29 + G1; Stage 6c). **Base-only** — `get_or_compute_effective_export_set` fails closed on a session view; the augmentation index it stitches has no base/session population identity (deferred to a future block) |
| `MaterializeStructureDb` | Query-identity (multi-candidate) | `MaterializationCacheKey { decl: ResolvedDeclSlotIdentity, projection_path, projection_mode, normalized_type_args, options_hash }` | Fact-validated per candidate |
| `RefCycleResultDb`, `SemanticGraphStore` query nodes | Query-identity (multi-candidate) | `ResolvedDeclSlotIdentity` (slot) + `VersionedDeclIdentity` payload | Fact-validated per candidate |
| `ComponentMetaResultDb` | Query-identity (multi-candidate) | Owner identity (per R8) | Fact-validated per candidate |

`parse_stable_hash` is a structural hash over the file's
post-shallow-analysis decl skeleton (names, kinds, member name lists,
scope structure). Invariant under cosmetic edits (whitespace,
comments, JSDoc, generic param rename). Computed once per
`(canonical, content_hash, parse_env_hash)` and lives alongside
`IndexedReady` in `FileArtifactStore.FileArtifacts`.

## Fact registry shape

The per-file fact registry lives on
`verter_session::file_artifact_store::FileFacts.registry` and uses
the schema defined in `verter_semantic::facts::registry` (R10–R13,
R28, R29):

```rust
struct FactRegistry {
    facts: FxHashMap<FactKey, Fact>,
    syntactic_export_set: Option<Fact>,
}

struct Fact {
    key: FactKey,
    semantic_hash: FactHash,  // alpha-normalised, cosmetic-invariant
    display_hash: FactHash,   // cosmetic-sensitive
}

enum FactKey {
    // Parse-domain (R12; populated at parse time)
    Export { name, space },
    ExportAlias { exported_as, space },
    SyntacticExportSet,
    LocalDecl { name, space },
    Member { exporter, name, space },         // body — lazy
    MemberPresence { exporter, name, space }, // header — eager
    MemberShape { exporter, space },          // whole-surface — eager
    MacroSurface { kind, target },
    TemplateRoot,
    ImportRef { specifier, binding, space },
    SyntacticReexportRef { specifier, source_name, target_name, space },
    ModuleAugmentation { specifier, augmented_name, space },

    // Resolve-imports domain (R12; populated downstream by the resolver)
    ResolvedImportClause { specifier, binding, space, resolved_canonical, resolved_source_name },
    ResolvedReexportBinding { specifier, source_name, target_name, space, resolved_canonical, resolved_source_name },

    // Route-surface domain (R12; populated downstream by RouteDb)
    EffectiveExportSet,
    ModuleAugmentationIndexShape { target_kind_tag, external_specifier, resolved_relative_canonical, wildcard_pattern },
}

enum FactDomain { ParseFile, ResolveImports, RouteSurface }

impl FactKey {
    fn domain(&self) -> FactDomain;  // routes per-domain validator dispatch
}
```

`FactKey::domain()` routes validator lookups through the `StoreView`
trait surface:

```rust
trait StoreView {
    fn compat_token(&self) -> StoreViewCompatToken;
    fn validates(&self, fact: &FactVersionRef) -> bool;
    // R26 per-domain validators — default impls return `false`;
    // Stage 6 producers override.
    fn validates_parse_domain(&self, _fact: &ParseFactRef) -> bool { false }
    fn validates_resolve_imports_domain(&self, _fact: &ResolveImportsFactRef) -> bool { false }
    fn validates_route_surface_domain(&self, _fact: &RouteSurfaceFactRef) -> bool { false }
}
```

The dispatch table is bounded by `FactDomain` (3 variants), not by
`FactKey`. Adding a new `FactKey` extends a per-domain `*FactRef`
enum but does NOT widen the trait.

## Two-phase emission (R28)

Parse-time emission (eager, shallow, O(file_size)) populates the
parse-domain `FactRegistry` on the per-file `FileFacts`. The producer
is `verter_session::fact_emission::emit_parse_facts(&IndexedReady) ->
ParseFactsEmission { facts: FileFacts, augmentations: Vec<…> }`.

The lazy member-body emission is split into TWO host-owned stores
keyed differently so cosmetic edits hit only the display store:

| Store | Key | Lifecycle |
|---|---|---|
| `MemberSemanticFactStore` | `(canonical, parse_stable_hash, parse_env_hash, exporter, member_name, symbol_space)` | A cosmetic edit keeps the same key → the cached fact survives |
| `MemberDisplayFactStore` | `(canonical, content_hash, parse_env_hash, exporter, member_name, symbol_space)` | A cosmetic edit re-keys → the producer recomputes (may equal original under whitespace-only edit, may differ under JSDoc) |

Both stores admit through `entry().or_insert(...)`: insert-only-if-
absent, so producer races for the same key collapse to a single
canonical fact. Downstream consumers always observe pointer-equal
`Arc<Fact>` for the same key.

## Cycle-safe worklist hashing (R27)

`verter_semantic::facts::hashing::compute_semantic_hash` walks a
`TypeExpr` body and emits an alpha-normalised `Hash16`. Stack-safe
(explicit `depth` counter, `MAX_HASH_DEPTH = 64`), cycle-safe
(`VisitedSet` emits `CycleRef(visit_index)` on re-entry), path-
precise (cross-decl refs resolve through a `CrossDeclLens` and emit
reference-shape edges WITHOUT inlining the referent's body).

Over-budget walks set `HashOutcome.budget_exceeded = true`; producers
MUST admit the cache entry as `NonCacheable` (the admission guard
lives downstream).

Visit order is canonical: lexicographic by `(name, symbol_space)`
at each unresolved-neighbor expansion; tie-break by `(canonical,
name, symbol_space)`. The `CycleRef` placeholder is therefore
invariant under source-text reordering — the same cycle produces
byte-identical fingerprints regardless of declaration order.

## See also

- `.claude/skills/type-cache-architecture/SKILL.md` — full R1–R29
  rule set with semantic content.
- `crates/verter_workspace/src/env_hash.rs` — env-hash function
  implementations.
- `crates/verter_semantic/src/facts/registry.rs` — `FactKey` /
  `Fact` / `FactRegistry` / `SymbolSpace` definitions; the
  `Interned*` newtype set lives here.
- `crates/verter_semantic/src/facts/hashing.rs` —
  `compute_semantic_hash`, `compute_member_presence_hash`,
  `compute_member_shape_hash`, `CrossDeclLens`,
  `MAX_HASH_DEPTH`.
- `crates/verter_session/src/file_artifact_store.rs` —
  `FileArtifactStore`, `FileArtifactKey`, `FileArtifacts`,
  `FileFacts`, `AugmentationTargetKey`, supporting types.
- `crates/verter_session/src/fact_emission.rs` —
  `emit_parse_facts` parse-time producer + module-augmentation
  extraction.
- `crates/verter_session/src/member_semantic_fact_store.rs` —
  `MemberSemanticFactStore` (lazy, `parse_stable_hash`-keyed).
- `crates/verter_session/src/member_display_fact_store.rs` —
  `MemberDisplayFactStore` (lazy, `content_hash`-keyed).
- `crates/verter_session/src/resolver_core/mod.rs` —
  `FactVersionRef` per-domain variants + `StoreView` per-domain
  validator dispatch.
- `crates/verter_session/src/parse_stable_hash.rs` —
  `compute_parse_stable_hash` implementation.
