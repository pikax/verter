# C1 relocation identity map

Base `d1f3d50a948597f036868543b9bb21acacd730ff` → reviewed candidate
`c46c60c52f33784356a9f1d7fade31627486e874`, tree
`031c84419aaa1bc851c24e31add987c9ad678ba8` (see `landing-subject.md`). Derived by diffing every `pub` item name per production file at base against the same file at the candidate; each removed item is resolved to its destination by definition lookup in the candidate tree. Method-level rows come from the same procedure over `impl` blocks. Test-file relocations are in `s2f4/` and `test-relocation.md`.

| Source file (base) | Kind | Identifier | Destination / disposition |
|---|---|---|---|
| `crates/verter_lsp/src/lib.rs` | mod | `project_resolver` | DELETED — `verter_lsp/src/project_resolver.rs` shim removed (S2-F1); callers repointed to `verter_semantic::resolver_core::ModuleResolverCore` |
| `crates/verter_semantic/src/analysis/mod.rs` | mod | `project_resolver` | RENAMED → `resolver_core/module_reference_resolution.rs` (git R051) |
| `crates/verter_semantic/src/analysis/project_resolver.rs` | fn | `collect_resolvable_module_reference_specifiers` | `crates/verter_semantic/src/resolver_core/module_reference_resolution.rs` |
| `crates/verter_semantic/src/analysis/project_resolver.rs` | fn | `resolve_known_module_reference_dependencies` | `crates/verter_semantic/src/resolver_core/module_reference_resolution.rs` |
| `crates/verter_session/src/decl_body_memo.rs` | struct | `LoweredTypeDecl` | `crates/verter_semantic/src/resolver_core/lowered_decl.rs` |
| `crates/verter_session/src/decl_body_memo.rs` | struct | `LoweredValueDecl` | `crates/verter_semantic/src/resolver_core/lowered_decl.rs` |
| `crates/verter_session/src/decl_body_memo.rs` | struct | `ValueBodyHashFact` | `crates/verter_semantic/src/resolver_core/lowered_decl.rs` |
| `crates/verter_session/src/file_artifact_store.rs` | enum | `AugmentationPopulation` | `crates/verter_semantic/src/resolver_core/augmentation_key.rs` |
| `crates/verter_session/src/file_artifact_store.rs` | enum | `AugmentationTargetKind` | `crates/verter_semantic/src/resolver_core/augmentation_key.rs` |
| `crates/verter_session/src/file_artifact_store.rs` | struct | `AugmentationTargetKey` | `crates/verter_semantic/src/resolver_core/augmentation_key.rs` |
| `crates/verter_session/src/file_artifact_store.rs` | struct | `ProjectIdentity` | `crates/verter_semantic/src/resolver_core/augmentation_key.rs` / `crates/verter_semantic/src/facts/resolution.rs` / `crates/verter_workspace/src/resolution_currency.rs` |
| `crates/verter_session/src/resolver_store.rs` | struct | `OverlayIdentity` | `crates/verter_session/src/cache_runtime/world_snapshot.rs` |
| `crates/verter_session/src/resolver_store.rs` | struct | `StoreViewValidationToken` | `crates/verter_semantic/src/resolver_core/store_view_identity.rs` |
| `crates/verter_session/src/session_view.rs` | struct | `EnvHashes` | `crates/verter_semantic/src/resolver_core/env_hashes.rs` |
| `crates/verter_workspace/src/ambient_lib.rs` | struct | `AmbientSymbolHit` | `crates/verter_semantic/src/resolver_core/ambient_symbol_hit.rs` |
| `crates/verter_workspace/src/fact_registry.rs` | enum | `AugmentationScopeKindTag` | `crates/verter_semantic/src/facts/registry.rs` |
| `crates/verter_workspace/src/fact_registry.rs` | enum | `AugmentationTargetKindTag` | `crates/verter_semantic/src/facts/registry.rs` |
| `crates/verter_workspace/src/fact_registry.rs` | enum | `FactDomain` | `crates/verter_semantic/src/facts/registry.rs` |
| `crates/verter_workspace/src/fact_registry.rs` | enum | `FactKey` | `crates/verter_semantic/src/facts/registry.rs` |
| `crates/verter_workspace/src/fact_registry.rs` | enum | `FactLane` | `crates/verter_semantic/src/facts/registry.rs` |
| `crates/verter_workspace/src/fact_registry.rs` | enum | `MacroKind` | `crates/verter_semantic/src/analysis/template.rs` / `crates/verter_semantic/src/facts/registry.rs` |
| `crates/verter_workspace/src/fact_registry.rs` | enum | `MemberKind` | `crates/verter_semantic/src/facts/registry.rs` |
| `crates/verter_workspace/src/fact_registry.rs` | enum | `SymbolSpace` | `crates/verter_semantic/src/facts/registry.rs` / `crates/verter_session/src/resolver_core/route_demand.rs` / `crates/verter_session/src/typeinfo/typeinfo_tests/oracle_query_specs.rs` |
| `crates/verter_workspace/src/fact_registry.rs` | struct | `Fact` | `crates/verter_semantic/src/facts/registry.rs` |
| `crates/verter_workspace/src/fact_registry.rs` | struct | `FactRegistry` | `crates/verter_semantic/src/facts/registry.rs` |
| `crates/verter_workspace/src/fact_registry.rs` | struct | `InternedGlobPattern` | `crates/verter_semantic/src/facts/registry.rs` |
| `crates/verter_workspace/src/fact_registry.rs` | struct | `InternedName` | `crates/verter_semantic/src/facts/registry.rs` |
| `crates/verter_workspace/src/fact_registry.rs` | struct | `InternedSpecifier` | `crates/verter_semantic/src/facts/registry.rs` |
| `crates/verter_workspace/src/fact_registry.rs` | struct | `MacroTargetKey` | `crates/verter_semantic/src/facts/registry.rs` |
| `crates/verter_workspace/src/fact_registry.rs` | struct | `ObservedFact` | `crates/verter_semantic/src/facts/registry.rs` |
| `crates/verter_workspace/src/fact_registry.rs` | type | `FactHash` | `crates/verter_semantic/src/facts/registry.rs` |
| `crates/verter_workspace/src/lib.rs` | mod | `fact_registry` | DELETED module; contents MOVED → `verter_semantic/src/facts/registry.rs` (the former semantic re-export became the owner) |
| `crates/verter_workspace/src/lib.rs` | mod | `normalized_glob` | `crates/verter_semantic/src/resolver_core/mod.rs` |
| `crates/verter_workspace/src/membership.rs` | fn | `typescript_default_excludes` | `crates/verter_semantic/src/resolver_core/membership.rs` |
| `crates/verter_workspace/src/membership.rs` | struct | `ConfiguredMembership` | `crates/verter_semantic/src/resolver_core/membership.rs` |
| `crates/verter_workspace/src/membership.rs` | struct | `StaticMembershipSpec` | `crates/verter_semantic/src/resolver_core/membership.rs` |
| `crates/verter_workspace/src/normalized_glob.rs` | struct | `CompiledGlob` | `crates/verter_semantic/src/resolver_core/normalized_glob.rs` |
| `crates/verter_workspace/src/normalized_glob.rs` | struct | `NormalizedGlob` | `crates/verter_semantic/src/resolver_core/normalized_glob.rs` |
| `crates/verter_workspace/src/project_key.rs` | enum | `ProjectStableKey` | `crates/verter_semantic/src/resolver_core/project_stable_key.rs` |
| `crates/verter_workspace/src/resolution_currency.rs` | enum | `PathProbe` | `crates/verter_semantic/src/resolver_core/path_probe.rs` |
| `crates/verter_workspace/src/resolution_currency.rs` | enum | `ResolutionPopulation` | `crates/verter_semantic/src/resolver_core/resolution_world_identity.rs` |
| `crates/verter_workspace/src/resolution_currency.rs` | struct | `ResolutionWorldId` | `crates/verter_semantic/src/resolver_core/resolution_world_identity.rs` |
| `crates/verter_workspace/src/resolution_currency.rs` | struct | `SessionFingerprint` | `crates/verter_semantic/src/resolver_core/resolution_world_identity.rs` |
| `crates/verter_workspace/src/resolver.rs` | const | `CARRIER_API_MODULE_SPECIFIER_SUFFIX` | `crates/verter_semantic/src/resolver_core/path_utils.rs` |
| `crates/verter_workspace/src/resolver.rs` | const | `CARRIER_API_VIRTUAL_SUFFIX` | `crates/verter_semantic/src/resolver_core/path_utils.rs` |
| `crates/verter_workspace/src/resolver.rs` | enum | `ProjectMembership` | `crates/verter_workspace/src/membership.rs` (the existing workspace-owned file) |
| `crates/verter_workspace/src/resolver.rs` | fn | `build_known_file_index` | `crates/verter_semantic/src/resolver_core/path_utils.rs` |
| `crates/verter_workspace/src/resolver.rs` | fn | `carrier_api_provider_path` | `crates/verter_semantic/src/resolver_core/path_utils.rs` |
| `crates/verter_workspace/src/resolver.rs` | fn | `carrier_ide_provider_path` | `crates/verter_semantic/src/resolver_core/path_utils.rs` |
| `crates/verter_workspace/src/resolver.rs` | fn | `carrier_source_extensions` | `crates/verter_semantic/src/resolver_core/path_utils.rs` |
| `crates/verter_workspace/src/resolver.rs` | fn | `collapse_path` | `crates/verter_semantic/src/resolver_core/path_utils.rs` |
| `crates/verter_workspace/src/resolver.rs` | fn | `is_absolute_specifier` | `crates/verter_semantic/src/resolver_core/path_utils.rs` |
| `crates/verter_workspace/src/resolver.rs` | fn | `is_relative_specifier` | `crates/verter_semantic/src/resolver_core/path_utils.rs` |
| `crates/verter_workspace/src/resolver.rs` | fn | `join_paths` | `crates/verter_semantic/src/resolver_core/path_utils.rs` |
| `crates/verter_workspace/src/resolver.rs` | fn | `normalize_canonical_id` | `crates/verter_semantic/src/resolver_core/path_utils.rs` / `crates/verter_workspace/src/ambient_lib.rs` |
| `crates/verter_workspace/src/resolver.rs` | fn | `normalize_known_file_id` | `crates/verter_semantic/src/resolver_core/path_utils.rs` |
| `crates/verter_workspace/src/resolver.rs` | fn | `parent_dir` | `crates/verter_semantic/src/resolver_core/path_utils.rs` |
| `crates/verter_workspace/src/resolver.rs` | fn | `path_is_carrier` | `crates/verter_semantic/src/resolver_core/path_utils.rs` |
| `crates/verter_workspace/src/resolver.rs` | fn | `probe_extensions` | `crates/verter_semantic/src/resolver_core/mod.rs` |
| `crates/verter_workspace/src/resolver.rs` | fn | `resolve_known_dependency_base` | `crates/verter_semantic/src/resolver_core/path_utils.rs` |
| `crates/verter_workspace/src/resolver.rs` | fn | `resolve_known_dependency_id` | `crates/verter_semantic/src/resolver_core/path_utils.rs` |
| `crates/verter_workspace/src/resolver.rs` | fn | `strip_carrier_extension` | `crates/verter_semantic/src/resolver_core/path_utils.rs` |
| `crates/verter_workspace/src/resolver.rs` | struct | `IdeProjectCompilerOptions` | `crates/verter_semantic/src/resolver_core/project_config.rs` |
| `crates/verter_workspace/src/resolver.rs` | struct | `IdeProjectConfig` | `crates/verter_semantic/src/resolver_core/project_config.rs` |
| `crates/verter_workspace/src/resolver.rs` | struct | `ProjectResolver` | DELETED (S2-F1) → `verter_semantic::resolver_core::ModuleResolverCore` (`module_resolver_core.rs`); absence pinned by `tests/compile-fail/legacy_resolver_surface_is_absent.rs` |
| `crates/verter_workspace/src/resolver.rs` | struct | `WorkspaceAlias` | `crates/verter_semantic/src/resolver_core/project_config.rs` |
| `crates/verter_workspace/src/resolver.rs` | type | `NativeProjectResolver` | DELETED (S2-F1), no alias; absence pinned by the same compile-fail fixture |
| `crates/verter_workspace/src/types.rs` | enum | `ProviderTarget` | `crates/verter_semantic/src/resolver_core/dto.rs` |
| `crates/verter_workspace/src/types.rs` | enum | `ResolutionKind` | `crates/verter_semantic/src/resolver_core/dto.rs` |
| `crates/verter_workspace/src/types.rs` | enum | `ResolvePhase` | `crates/verter_semantic/src/resolver_core/dto.rs` |
| `crates/verter_workspace/src/types.rs` | enum | `ResolveRequestKind` | `crates/verter_semantic/src/resolver_core/dto.rs` |
| `crates/verter_workspace/src/types.rs` | struct | `ProjectOwnership` | `crates/verter_semantic/src/resolver_core/dto.rs` |
| `crates/verter_workspace/src/types.rs` | struct | `ResolutionContext` | `crates/verter_semantic/src/resolver_core/dto.rs` |
| `crates/verter_workspace/src/types.rs` | struct | `ResolveRequest` | `crates/verter_semantic/src/resolver_core/dto.rs` / `crates/verter_session/src/resolver_core/mod.rs` |
| `crates/verter_workspace/src/types.rs` | struct | `ResolveResult` | `crates/verter_semantic/src/resolver_core/dto.rs` |

## Whole-file moves (git rename detection at ≥40% similarity)

| From | To | Similarity |
|---|---|---|
| `crates/verter_semantic/src/analysis/project_resolver.rs` | `crates/verter_semantic/src/resolver_core/module_reference_resolution.rs` | R051 |
| `crates/verter_semantic/src/analysis/project_resolver_tests.rs` | `crates/verter_semantic/src/resolver_core/module_reference_resolution_tests.rs` | R100 |
| `crates/verter_workspace/src/normalized_glob.rs` | `crates/verter_semantic/src/resolver_core/normalized_glob.rs` | R090 |
| `crates/verter_workspace/src/fact_registry.rs` (897 lines, deleted) | `crates/verter_semantic/src/facts/registry.rs` (+890 lines) | content move below git rename threshold (the semantic file pre-existed as a re-export) |
| `crates/verter_workspace/src/resolver.rs` (2172 lines removed, 337 kept) | `crates/verter_semantic/src/resolver_core/{module_resolver_core,source_id_resolution,tsconfig_paths_resolution,project_references_resolution,node_modules_resolution,package_target_resolution,project_ownership_resolution,provider_projection_resolution,preferred_specifier_resolution,probe_path_resolution,top_level_resolution,path_utils,project_config,dto}.rs` | split; the retained 421-line `resolver.rs` is the workspace-owned tracked driver (`resolve_tracked`, `resolve_for_project_tracked`, `drive_attempt`, `ide_project_config`) |
| `crates/verter_lsp/src/project_resolver.rs` | — | DELETED shim (S2-F1) |
| `crates/verter_semantic/src/resolver_core/resolution_dual_runner_tests.rs` (1802 lines, deleted at `d31410791`) | `crates/verter_workspace/src/resolution_conversion_tests.rs` (24 cases) | converted; see `s2f4/correspondence.md` |

## Type relocations landed before the resolver move (same branch)

| Identifier | From | To |
|---|---|---|
| `StoreViewValidationToken` (+ `external_supersession_fingerprint`, `externally_superseded_by`, `lane_fingerprint`) | `verter_session/src/resolver_store.rs` | `verter_semantic/src/resolver_core/store_view_identity.rs` |
| `EnvHashes` | `verter_session/src/session_view.rs` | `verter_semantic/src/resolver_core/env_hashes.rs` |
| `LoweredTypeDecl`, `LoweredValueDecl`, `ValueBodyHashFact` (+ `to_outcome`) | `verter_session/src/decl_body_memo.rs` | `verter_semantic/src/resolver_core/lowered_decl.rs` |
| `ProjectIdentity` (+ `fold_u32`), `AugmentationPopulation`, `AugmentationTargetKind`, `AugmentationTargetKey` | `verter_session/src/file_artifact_store.rs` | `verter_semantic/src/resolver_core/augmentation_key.rs` |
| `AmbientSymbolHit` | `verter_workspace/src/ambient_lib.rs` | `verter_semantic/src/resolver_core/ambient_symbol_hit.rs` |
| `PathProbe`, `ResolutionPopulation`, `ResolutionWorldId`, `SessionFingerprint` | `verter_workspace/src/resolution_currency.rs` | `verter_semantic/src/resolver_core/{path_probe,resolution_world_identity}.rs` |
| `ProjectStableKey` | `verter_workspace/src/project_key.rs` | `verter_semantic/src/resolver_core/project_stable_key.rs` (`from_project` stays workspace-side) |
| `ConfiguredMembership`, `StaticMembershipSpec` | `verter_workspace/src/membership.rs` | `verter_semantic/src/resolver_core/membership.rs`; `ProjectMembership` moved from `verter_workspace/src/resolver.rs` into the existing `verter_workspace/src/membership.rs` (stays workspace-owned, no re-export) |

## Explicit nonexistent-path check

Both `crates/verter_workspace/src/project_membership.rs` and
`crates/verter_semantic/src/resolver_core/project_membership.rs` are absent at the base and at the
reviewed candidate. Neither path is an addition, deletion, rename destination, or semantic owner.
`ProjectMembership` is declared at `crates/verter_workspace/src/membership.rs` in the candidate.
| 8 resolver DTOs (`ProjectOwnership`, `ResolveRequestKind`, `ResolvePhase`, `ResolutionContext`, `ProviderTarget`, `ResolutionKind`, `ResolveRequest`, `ResolveResult`) | `verter_workspace/src/types.rs` | `verter_semantic/src/resolver_core/dto.rs` |
| `IdeProjectConfig`, `IdeProjectCompilerOptions`, `WorkspaceAlias` | `verter_workspace/src/resolver.rs` | `verter_semantic/src/resolver_core/project_config.rs` |
| `FactKey`, `Fact`, `FactDomain`, `FactRegistry`, `FactLane`, `SymbolSpace`, `MemberKind`, `MacroKind`, `MacroTargetKey`, `ObservedFact`, `Interned{Specifier,Name,GlobPattern}`, `AugmentationScopeKindTag`, `AugmentationTargetKindTag`, `FactHash` | `verter_workspace/src/fact_registry.rs` | `verter_semantic/src/facts/registry.rs` |
| 16 path/carrier helpers (`normalize_canonical_id`, `collapse_path`, `join_paths`, `parent_dir`, `is_relative_specifier`, `is_absolute_specifier`, `path_is_carrier`, `carrier_ide_provider_path`, `carrier_api_provider_path`, `carrier_source_extensions`, `strip_carrier_extension`, `build_known_file_index`, `normalize_known_file_id`, `resolve_known_dependency_base`, `resolve_known_dependency_id`, `CARRIER_API_*` consts) | `verter_workspace/src/resolver.rs` | `verter_semantic/src/resolver_core/path_utils.rs` (`probe_extensions` → `resolver_core/mod.rs`) |
