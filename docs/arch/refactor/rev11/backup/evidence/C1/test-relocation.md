# C1 test relocation and coverage-preservation map (S2-F4)

Procedure: for every `.rs` file changed between base `d1f3d50a9` and the candidate, the set of
`#[test]` function names at base was diffed against the same file at the candidate; every name that
left its file was looked up across the candidate tree.

| Base file | Tests at base → now | Names that left | Destination / disposition |
|---|---|---|---|
| `crates/verter_semantic/src/resolver_core/resolution_dual_runner_tests.rs` (deleted at `d31410791`) | 24 → 0 | all 24 | `crates/verter_workspace/src/resolution_conversion_tests.rs`, same 24 names, production driver + kernel + total historical ledger — `s2f4/correspondence.md` |
| `crates/verter_workspace/src/resolver_tests.rs` | 95 → 86 | `preferred_specifier_returns_tsconfig_alias`, `preferred_specifier_returns_none_when_no_match`, `preferred_specifier_prefers_shortest`, `preferred_specifier_round_trips`, `preferred_specifier_none_for_provider_paths`, `preferred_specifier_multi_target_first_wins`, `preferred_specifier_multi_target_shadowed`, `preferred_specifier_workspace_alias_fallback`, `preferred_specifier_workspace_alias_no_double_slash` | **Were deleted without a destination (S2-F4 defect, repaired here).** Converted with unchanged assertions onto the production composite `Engine::preferred_specifier` over `ModuleResolverCore::preferred_specifier_candidates`, driven through `MemoryWorkspace`: `crates/verter_workspace/src/preferred_specifier_round_trip_tests.rs` (9 tests, same names; commit `3a1533cd3`). |
| `crates/verter_workspace/src/resolver_tests.rs` | | `native_project_resolver_alias_works` | **Explicit exclusion.** The test asserted that the `NativeProjectResolver` alias exists; S2-F1 mandates its deletion with no forwarding alias. Its inverse is now pinned by `crates/verter_workspace/tests/compile-fail/legacy_resolver_surface_is_absent.rs`. |
| `crates/verter_workspace/src/fact_registry.rs` (deleted) | 8 → 0 | `empty_registry_round_trip`, `fact_key_domain_routes_correctly`, `insert_and_get_round_trip`, `member_kind_tags_discriminate_by_modifier`, `module_augmentation_keys_are_partitioned_by_lexical_owner`, `symbol_space_tags_are_distinct_and_stable`, `syntactic_export_set_cache_is_kept_in_sync`, `type_and_value_namespace_keys_are_distinct` | all 8 present in `crates/verter_semantic/src/facts/registry.rs::registry_tests` |
| `crates/verter_semantic/src/facts/registry.rs` | 1 → 0 (→ 9 now) | `macro_kind_round_trips_with_template_kind` | **Was deleted when the registry became the owner (S2-F4 defect, repaired here).** Restored in `registry_tests` with the original pairs plus an injectivity assertion (commit `3a1533cd3`). |
| `crates/verter_semantic/src/analysis/project_resolver_tests.rs` | R100 rename | — | `crates/verter_semantic/src/resolver_core/module_reference_resolution_tests.rs` |
| `crates/verter_workspace/src/normalized_glob_tests.rs` | moved | — | `crates/verter_semantic/src/resolver_core/normalized_glob_tests.rs` |
| every other changed test file | no test name left its file | — | — |

New test surface on the branch (not relocations): 26 `verter_semantic/src/resolver_core/*_tests.rs`
files (256 tests) covering the split resolver modules, `verter_semantic/tests/cases/{resolver_core_ownership,
resolver_observation_compile_fail}.rs`, `verter_workspace/tests/cases/{legacy_resolver_absence_compile_fail,
resolver_core_private_helpers_compile_fail,resolver_stay_ownership}.rs`, `verter_workspace/src/
{resolution_conversion_registration_tests,resolution_driver_tests}.rs`, `verter_session/src/
{route_analysis_inputs_tests,lifecycle_answer_equivalence_tests}.rs`.
