# Fact-based cache architecture

This document is the per-field audit + per-cache-layer key composition
table for the fact-based cache architecture. The full rule set
(R1–R29) lives in the `/type-cache-architecture` skill.

> AMENDMENT 2026-05-11-A — the integration branch for this work is
> `refactor/semantic-db-overhaul` (renamed from the older
> `fix/cutover-review-findings`). The two share the same baseline; the
> swap is documentation-only.

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

## Cache layer key composition (post-cutover end-state)

| Layer | Family | Key | Validation |
|---|---|---|---|
| `FileArtifactStore` | Content-addressed | `(canonical, content_hash, parse_env_hash, parser_version)` | Content-addressed; never invalidated |
| `ModuleAugmentationIndex` (on `FileArtifactStore`) | Content-addressed | `AugmentationTargetKey { project_identity, resolve_env_hash, lib_env_hash, target }` | Content-addressed; incrementally populated |
| `ResolvedImportFacts` | Content-addressed | `(canonical, content_hash, parse_env_hash, resolve_env_hash, resolver_version)` — **no `lib_env_hash`** (R21) | Content + resolve-env addressed |
| Typed-IR resolve | Content-addressed | `(canonical, content_hash, parse_env_hash, type_env_hash, lib_env_hash, parser_version)` | Content + type-env + lib addressed |
| `MemberSemanticFactStore` | Content-addressed | `(canonical, parse_stable_hash, parse_env_hash, exporter, member_name, symbol_space)` | Keyed on `parse_stable_hash` so cosmetic edits do not recompute |
| `MemberDisplayFactStore` | Content-addressed | `(canonical, content_hash, parse_env_hash, exporter, member_name, symbol_space)` | Keyed on `content_hash`; cosmetic edits recompute display only |
| `RouteDb` per-name resolution | Query-identity (multi-candidate) | `(provider_canonical, exported_name, symbol_space, resolve_env_hash, lib_env_hash, resolver_version)` | Fact-validated; stable misses preserved |
| `RouteDb` effective barrel surface | Query-identity (multi-candidate) | `(provider_canonical, resolve_env_hash, lib_env_hash, resolver_version)` | Fact-validated |
| `MaterializeStructureDb` | Query-identity (multi-candidate) | `MaterializationCacheKey { decl: ResolvedDeclSlotIdentity, projection_path, projection_mode, normalized_type_args, options_hash }` | Fact-validated per candidate |
| `RefCycleResultDb`, `SemanticGraphStore` query nodes | Query-identity (multi-candidate) | `ResolvedDeclSlotIdentity` (slot) + `VersionedDeclIdentity` payload | Fact-validated per candidate |
| `ComponentMetaResultDb` | Query-identity (multi-candidate) | Owner identity (per R8) | Fact-validated per candidate |

`parse_stable_hash` is a structural hash over the file's
post-shallow-analysis decl skeleton (names, kinds, member name lists,
scope structure). Invariant under cosmetic edits (whitespace,
comments, JSDoc, generic param rename). Computed once per
`(canonical, content_hash, parse_env_hash)` and lives alongside
`IndexedReady` in `FileArtifactStore.FileArtifacts`.

## See also

- `.claude/skills/type-cache-architecture/SKILL.md` — full R1–R29
  rule set with semantic content.
- `crates/verter_workspace/src/env_hash.rs` — env-hash function
  implementations.
- `crates/verter_session/src/file_artifact_store.rs` —
  `FileArtifactStore`, `FileArtifactKey`, `FileArtifacts`,
  `AugmentationTargetKey`, supporting types.
- `crates/verter_session/src/parse_stable_hash.rs` —
  `compute_parse_stable_hash` implementation.
