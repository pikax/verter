# Fact-based cache architecture

This document is the per-field audit + per-cache-layer key composition
table for the fact-based cache architecture. The full rule set
(R1–R31) lives in the `/type-cache-architecture` skill.

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
> `validates_route_surface_domain` /
> `validates_program_analysis_domain`).

> AMENDMENT 2026-05-12-D — The whole-hash retirement audit-test at
> `crates/verter_session/tests/g_misc1/whole_hash_migration_audit.rs` is a
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
| `EnvHashInputs.ts_semantic_version` | per-call | `type_env` | The TypeScript semantic-engine version the resolver mirrors (the pinned `tsgo` semantic version). A semantic-version change can alter inference / relation / reduction outcomes for the same source, so cached semantic values depend on it. Distinct from `parser_version` (a `parse_env` dimension on `FileArtifactStore`) and `resolver_version` (a `resolve_env` dimension on `RouteDb`). |
| `EnvHashInputs.jsx_mode` | per-call | `type_env` | `jsx` emit/semantic mode (`preserve` / `react` / `react-jsx` / `react-jsxdev` / `react-native`). Changes how JSX expressions type and which factory surface applies. |
| `EnvHashInputs.jsx_import_source` | per-call | `resolve_env` | `jsxImportSource` — the module the `jsx` / `jsxs` runtime factory resolves from. A resolution input (it changes WHERE the factory comes from). |
| `EnvHashInputs.jsx_factory` | per-call | `type_env` | `jsxFactory` / `jsxFragmentFactory` classic-runtime factory identifiers. Change which call surface a JSX element synthesises. |
| `EnvHashInputs.module_resolution` | per-call | `resolve_env` | `moduleResolution` strategy (`node10` / `node16` / `nodenext` / `bundler` / `classic`). Changes HOW specifiers resolve. |
| `EnvHashInputs.package_conditions` | per-call | `resolve_env` | Active package export/import `conditions` (the `exports` / `imports` condition set, e.g. `import` / `require` / `types` / `node` / `browser`). Order- and membership-sensitive; selects which conditional export target a specifier resolves to. |
| `EnvHashInputs.custom_conditions` | per-call | `resolve_env` | `customConditions` — user-declared additional resolution conditions layered onto `package_conditions`. |
| `EnvHashInputs.module_suffixes` | per-call | `resolve_env` | `moduleSuffixes` — the ordered specifier-suffix search list (e.g. `[".ios", ""]`). Changes which on-disk file a specifier resolves to. |
| `EnvHashInputs.decorator_semantics` | per-call | `type_env` | Decorator + class-field semantics: `experimentalDecorators` (legacy TS decorators) vs TS7 standard decorators, plus `emitDecoratorMetadata`. Changes the typed decorator/class surface. |
| `EnvHashInputs.use_define_for_class_fields` | per-call | `type_env` | `useDefineForClassFields` — `[[Define]]` vs `[[Set]]` class-field semantics. Changes whether a field shadows/overrides a base accessor and the declared class surface. |
| `WorldSnapshot` (request identity) | `crates/verter_session/src/cache_runtime/world_snapshot.rs` | n/a — request-concurrency identity | Carries all five env hashes plus `compat_token`, `compiler_version`, `plugin_versions`, source-map / public-API policy hashes, `overlay_identity`, and `generation`. Exposes per-layer dim accessors (`parse_dims`, `resolve_dims`, `type_dims`, `compile_dims`); NEVER enters a cache key as a whole (R21 — the static guard `crates/verter_session/tests/g_misc3/world_snapshot_is_not_a_cache_key.rs` rejects any cache-layer struct field of type `WorldSnapshot`). |

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

The same R21 discipline governs every dimension added above: each one
enters a cache key **only** when the cached value depends on it, and it
enters via the **split env hash of the dimension it belongs to** — there
is no bundled `project_config_hash` mega-hash that smuggles them all in
together. Concretely:

- A `type_env` addition (`ts_semantic_version`, `jsx_mode`, `jsx_factory`,
  `decorator_semantics`, `use_define_for_class_fields`) folds into
  `type_env_hash`, so it enters every layer that already carries
  `type_env_hash` (typed-IR resolve, `MaterializeStructureDb`,
  `RefCycleResultDb`, `SemanticGraphStore`, `ComponentMetaResultDb`,
  `TypeInfoGraphResultDb`) and stays absent from `ResolvedImportFacts`
  (which carries no `type_env_hash`).
- A `resolve_env` addition (`jsx_import_source`, `module_resolution`,
  `package_conditions`, `custom_conditions`, `module_suffixes`) folds into
  `resolve_env_hash`, so it enters every resolution-dependent layer
  (`ResolvedImportFacts`, `RouteDb`, `RouteDb` effective export set,
  `AugmentationTargetKey`) and stays absent from a pure type-value layer
  that does not re-resolve specifiers.

These are split-env-hash **additions**, not query-identity content: none of
them is a field on any query-identity key struct. Query-identity keys stay
content-free (R6) — they carry a content-free declaration identity (the
env-bearing `ResolvedDeclSlotIdentity` for `Instantiate`/`ResolveMacroPayload`,
or the env-free `ResolveDeclKey` for `ResolveDecl`) plus the relevant
split env hashes; they never carry a content/version hash or
`fact_dep_signature`. A query-identity value is version-rooted on the cached
value (`ReadSetSignature.facts` + self-roots), not on the key.

### Session-only env identity + persistent-admission rule (R21 + R6)

Two dimensions are **session-scoped identity**, never persistent-cache key
material:

- **Overlay / session identity** (`overlay_identity`, the active editor
  overlay set) enters **session cache identity ONLY** — never the **base**
  population of any layer. A base/persistent slot (`FileArtifactStore`, the
  `Base` population of `ModuleAugmentationIndex`, the `Base` scope of
  `EffectiveExportSet`, and any pure artifact cache) **NEVER** admits an
  overlay-only result: an overlay edit produces a session-scoped value
  returned to the caller but routed into a SESSION-keyed slot, never the base
  one. `ModuleAugmentationIndex` and `EffectiveExportSet` are OVERLAY-AWARE:
  the index's `AugmentationPopulation::Session(overlay-set fingerprint)` slot
  unions the session's overlay augmenters with base (content-addressed
  compute cache → content fingerprint in the key), while
  `EffectiveExportSet`'s query-identity `EffectiveExportSetScope::Session(scope_id)`
  slot is keyed by the content-free session scope (R6) with overlay content
  rooted on the value's facts. Either way an overlay result populates the
  session-scoped slot, never the `Base` one. Pinned by
  **`persistent_caches_never_admit_overlay_only_results`**.
- **Instantiation-depth policy** (`InstantiationDepthPolicy` — the
  recursive-conditional / recursive-mapped instantiation-depth limit beyond
  the per-reducer budgets, parent §4.3 / §6) is part of the **cache
  identity + the recorded facts** of the depth-sensitive query-identity
  caches (`SemanticGraphStore` `Instantiate` / `Conditional` / `MappedType`
  nodes, `MaterializeStructureDb`, `RefCycleResultDb`, `TypeInfoGraphResultDb`).
  Two reductions of the same type under different depth policies reduce
  differently (one truncates at a shallower depth), so the policy is a
  meaning-affecting input. It folds into `type_env_hash` (the depth limit is a
  type-checking option), so it does **not** add a new key field — it enters
  the same layers `type_env_hash` enters, and the recorded `ReadSetSignature`
  validates against the depth policy in effect. Pinned by
  **`instantiation_depth_policy_in_identity_and_facts`** (owned at
  `U3.CACHE_FACT_MODEL`).

## Cache layer key composition (final-state)

| Layer | Family | Key | Validation |
|---|---|---|---|
| `FileArtifactStore` | Content-addressed | `(canonical, content_hash, parse_env_hash, parser_version, file_language_id)` | Content-addressed; never invalidated. `file_language_id` is the file's `FileLanguage` row — the per-file classification dimension; every key producer currently derives it from the static registry resolution (identical to the host-resolved row while no gated registry rows exist), and the first gated row's producer wiring threads the host-resolved row so a capability flip misses exactly the affected files' slots |
| `ModuleAugmentationIndex` (on `FileArtifactStore`) | Content-addressed | `AugmentationTargetKey { project_identity, resolve_env_hash, lib_env_hash, population, target }` | Content-addressed; incrementally populated. `population: AugmentationPopulation {Base, Session(overlay-set fingerprint)}` — a `Base` scan reads base (`is_legacy`) artifacts only; a `Session` scan reads the session overlay (non-legacy) artifacts matched by the overlay discriminator UNIONED with base, so overlay augmenters never poison the base index. The `Session` slot's fingerprint is its content-view identity (self-invalidating on overlay content/membership change); a base augmenter add/edit invalidates the `Session` entries that include it via `refresh_augmentation_index_for_canonical` (the base set rebuilds in place, the session sets are dropped so the next session-scoped `ensure` cold-rescans base ∪ overlay). Distinct from the query-identity `EffectiveExportSetKey`, whose session dimension is the CONTENT-FREE `EffectiveExportSetScope` (R6) |
| `ResolvedImportFacts` | Content-addressed | `(canonical, content_hash, parse_env_hash, resolve_env_hash, resolver_version)` — **no `lib_env_hash`** (R21) | Content + resolve-env addressed |
| Typed-IR resolve | Content-addressed | `(canonical, content_hash, parse_env_hash, type_env_hash, lib_env_hash, parser_version)` | Content + type-env + lib addressed |
| `MemberSemanticFactStore` | Content-addressed | `(canonical, parse_stable_hash, parse_env_hash, exporter, member_name, symbol_space)` | Keyed on `parse_stable_hash` so cosmetic edits do not recompute |
| `MemberDisplayFactStore` | Content-addressed | `(canonical, content_hash, parse_env_hash, exporter, member_name, symbol_space)` | Keyed on `content_hash`; cosmetic edits recompute display only |
| `RouteDb` per-name resolution | Query-identity (multi-candidate) | `(provider_canonical, exported_name, symbol_space, resolve_env_hash, lib_env_hash, resolver_version)` | Fact-validated; stable misses preserved |
| `RouteDb` effective barrel surface | Query-identity (multi-candidate) | `barrel_canonical` | Fact-validated per candidate via `BarrelRouteSurface.fact_dep_signature: Arc<[FactVersionRef]>` (Stage 6c) |
| `RouteDb` effective export set | Query-identity (multi-candidate) | `EffectiveExportSetKey { provider_canonical, project_identity, resolve_env_hash, lib_env_hash, session_scope }` (R21 — lib_env enters because module augmentations live in libs; `session_scope: EffectiveExportSetScope {Base, Session(scope_id)}` is the CONTENT-FREE session scope, R6 — the overlay-set content fingerprint never enters the key) | Fact-validated per candidate via `EffectiveExportSetEntry.fact_dep_signature`, which records `RouteSurface(ModuleAugmentationIndexShape)` (the augmenter-set fingerprint) + per-contributor `FileWholeHash` anchors (R29 + G1). **Overlay-aware**: a session view stitches its own overlay augmenters (unioned with base) into a `Session(scope_id)` slot distinct from the `Base` slot; overlay CONTENT identity is validated on the value's facts (revalidated on every warm hit), NOT smuggled into the key. The content-addressed augmentation index it stitches keys its `Session` slot by the overlay-set fingerprint (compute input). |
| `MaterializeStructureDb` | Query-identity (multi-candidate) | `MaterializationCacheKey { decl: ResolvedDeclSlotIdentity, projection_path, projection_mode, normalized_type_args, options_hash }` | Fact-validated per candidate |
| `RefCycleResultDb`, `SemanticGraphStore` query nodes | Query-identity (multi-candidate) | `ResolvedDeclSlotIdentity` (slot) + `VersionedDeclIdentity` payload | Fact-validated per candidate |
| `ComponentMetaResultDb` | Query-identity (multi-candidate) | Owner identity (per R8) | Fact-validated per candidate |
| `CompileSlot.fact_dep_signature` (per-profile compile cache) | Profile-keyed (`CompileProfile`) on `compile_slots: FxHashMap<u64, CompileSlot>` | `(canonical, compile_profile_hash)` (no version hash in the key; `semantic_hash` + override-hashes ride on the slot value) | Carrier is `ReadSetSignature { facts, overflowed }`. Cold-build producer routes the finalised tracer through `SignatureAdmission::from_finalise(...)`; `Cacheable(sig)` → `compile_slots.insert(CompileSlot { fact_dep_signature: sig, .. })`; `NonCacheable` (overflow / unresolved provenance / self-root conflict / route-generation dependency) → skip-publish refusal, fresh value returned to caller without admitting. Empty (`facts.is_empty() && !overflowed`) is a valid admitted state — the warm-hit oracle validates vacuously and falls back to the `semantic_hash` / override-hash pre-filter |

`parse_stable_hash` is a structural hash over the file's
post-shallow-analysis decl skeleton (names, kinds, member name lists,
scope structure). Invariant under cosmetic edits (whitespace,
comments, JSDoc, generic param rename). Computed once per
`(canonical, content_hash, parse_env_hash)` and lives alongside
`IndexedReady` in `FileArtifactStore.FileArtifacts`.

## Multi-candidate `FamilySlots` — per-family adaptive caps + eviction

Each query-identity cache slot in the multi-candidate `FamilySlots`
substrate (`crates/verter_session/src/semantic_query_memo/mod.rs`;
`SemanticGraphStore` family memo) holds a **candidate list**: concurrent
version/env variants of the same query identity coexist as candidates, and
validity is decided per-candidate by `ReadSetSignature.validate_with_self_roots`
against the caller's live view (`validated_at_generation` is recency metadata
only, never a validity oracle). The candidate-list **capacity and eviction
policy is per-family, not a uniform cap of 4 with FIFO eviction**:

- **`candidate_cap()` is per-family.** Each query family declares its own
  candidate cap via a `candidate_cap()` function on the family descriptor
  rather than a single global `FAMILY_SLOT_CANDIDATE_CAP = 4`. The
  inference/substitution-heavy families — **`Relate`, `ResolveCall`,
  `Instantiate`, `Conditional`, `MappedType`, `FlowReturn`** — get **higher**
  adaptive caps (the same identity legitimately coexists across many live
  substitution / inference-context / env variants, so a small cap would thrash
  a hot inference loop). Content-light families (e.g. `ResolveEnum`,
  `KeyOf`, `ResolveOverloadSet`) keep a **small** cap. The cap is *adaptive*:
  it may grow toward the family's ceiling under sustained valid-hit pressure
  and shrink back, never exceeding the family ceiling or the global memory
  ceiling below.
- **Eviction = invalid-first, then LRU-by-valid-hit.** When a slot is at its
  cap and a new candidate must be admitted, eviction is **two-tier**: (1) evict
  any candidate that is **invalid** under the current live view first (an
  invalid candidate can never warm-hit, so it is pure overhead); (2) only if
  every candidate is still valid, evict the **least-recently valid-hit**
  candidate (LRU keyed on the last generation at which the candidate served a
  validated warm hit), not the oldest-inserted (FIFO). FIFO evicts a
  freshly-inserted-but-hot candidate; LRU-by-valid-hit retains the candidates
  that are actually serving the workload. Same-discriminant re-publish
  (matching `validated_at_generation` + `facts`) replaces in place and does
  not consume a slot.
- **Global memory ceiling.** Per-family caps are bounded by a process-wide
  **global memory ceiling** over the whole multi-candidate substrate: the sum
  of admitted candidates across all families and slots cannot exceed the
  ceiling. When the global ceiling is reached, admission applies the same
  invalid-first / LRU-by-valid-hit eviction **across** slots (globally, not
  just within the target slot) before admitting, so one hot family cannot
  starve memory from the rest. A candidate that cannot be admitted without
  breaching the ceiling and whose eviction victims are all still valid +
  more-recently-hit is **not admitted** — the value is returned to the caller
  through the typed `ReturnOnly`/`ComputeAdmission` path, never published
  (consistent with the substrate's existing non-admission discipline).
- **Benched fallback-count bound per family.** Each family carries a
  **benchmarked fallback-count bound** — the maximum tolerated cold-recompute
  ("fallback") rate for that family's representative workload — regression-
  gated through the existing `BenchResultRow`, which already reports cache
  mode, hit count, and fallback count. The bench asserts the per-family
  fallback count stays at or below its declared bound for the representative
  batch; a cap regression (e.g. silently reverting a hot family to a small cap)
  shows up as a fallback-count regression and fails the bench gate. This makes
  the per-family caps an empirically-tuned, regression-protected contract
  rather than a hand-picked constant.

The validity rail is unchanged: the per-family cap + eviction policy governs
only *which* candidates a slot retains; *whether* a retained candidate may
warm-hit is still decided exclusively by `ReadSetSignature.validate_with_self_roots`
against the caller's live view. Pinned by
**`cache_candidate_cap_is_per_family_not_uniform`**,
**`family_eviction_prefers_invalid_then_lru_valid_hit`**, and the benched
per-family fallback-count bound (owned at `U3.CACHE_FACT_MODEL`; the result-DB
candidate storage at `U10.RESULT_DB` rides the same substrate).

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

    // Program-analysis domain (populated by the demand-sliced flow engine)
    FlowSlice { function_slot, projection_path, slice_hash, selected_binding_ids, selected_effect_ids, selected_control_region_ids, closure_summary_ids },
}

enum FactDomain { ParseFile, ResolveImports, RouteSurface, ProgramAnalysis }

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
    // ProgramAnalysis domain — owns the `FlowSlice` fact (the demand-sliced flow
    // engine). Validates against the current region/function-body identity
    // (`flow_body_stable_hash`) + the stored `FlowSlice` semantic hash. Fail-closed:
    // a missing / overflowed / stale / unrooted `FlowSlice` fact returns `false`.
    fn validates_program_analysis_domain(&self, _fact: &ProgramAnalysisFactRef) -> bool { false }
}
```

The dispatch table is bounded by `FactDomain` (4 variants), not by
`FactKey`. Adding a new `FactKey` extends a per-domain `*FactRef`
enum but does NOT widen the trait.

The `ProgramAnalysis` domain is the fourth closed `FactDomain`. It owns
the `FlowSlice` fact produced by the demand-sliced flow engine — `FlowSlice`
is NOT a parse / resolve-imports / route-surface fact. Its
`FactVersionRef::ProgramAnalysis(ProgramAnalysisFactRef { .. })` carries the
flow-region identity (`function_slot`, `projection_path`, `flow_body_stable_hash`)
plus the stored `FlowSlice` semantic hash. `StoreView::validates_program_analysis_domain`
re-derives the live region's `flow_body_stable_hash` and the recorded slice's
semantic hash and validates BOTH gates; it FAILS CLOSED on a missing, overflowed,
stale (body changed → `flow_body_stable_hash` differs), or unrooted fact — a
fail-closed miss recomputes rather than serving a torn slice. `flow_body_stable_hash`
is content-derived flow node/fact identity, NOT a query-identity-key dimension:
query-identity keys stay content-free (R6); the flow result is version-rooted via
this `FlowSlice` fact, exactly as the other query-identity caches version-root
through their recorded facts.

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

- `.claude/skills/type-cache-architecture/SKILL.md` — full R1–R31
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
