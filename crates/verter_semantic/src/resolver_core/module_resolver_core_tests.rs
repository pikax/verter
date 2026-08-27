use std::sync::Arc;

use super::ModuleResolverCore;
use crate::resolver_core::{
    AttemptFailure, AttemptOutcome, CompletedAttempt, InputKey, InputResolutionBudgets,
    ResolutionBasis, ResolutionObservationSnapshot, ResolutionWorldBasis, ResolverAttemptView,
};

fn basis() -> ResolutionBasis {
    ResolutionBasis::new(
        ResolutionWorldBasis::new(
            crate::resolver_core::WorkspaceAuthorityId::test_only(1),
            crate::resolver_core::ResolutionPopulation::Base,
            crate::resolver_core::ResolutionWorldId::test_only(1),
            None,
        ),
        None,
    )
}

fn esm_import_ctx() -> crate::resolver_core::ResolutionContext {
    crate::resolver_core::ResolutionContext {
        phase: crate::resolver_core::ResolvePhase::CodegenBlocker,
        kind: crate::resolver_core::ResolveRequestKind::EsmImport,
    }
}

fn known_world_view(files: &[&str]) -> ResolverAttemptView {
    let mut snapshot = ResolutionObservationSnapshot::with_stable_absent_defaults_for_test();
    for path in files {
        snapshot.insert_path_probe((*path).to_string(), crate::resolver_core::PathProbe::File);
        snapshot.insert_real_path((*path).to_string(), Some(Arc::from(*path)));
    }
    ResolverAttemptView::from_resolution_snapshot(Arc::new(snapshot), basis())
}

fn configured_project(
    root: &str,
    tsconfig: &str,
    aliases: &[(&str, &str)],
) -> crate::resolver_core::IdeProjectConfig {
    let mut project = crate::resolver_core::IdeProjectConfig::new(
        root.to_string(),
        root.to_string(),
        Some(tsconfig.to_string()),
    );
    project.workspace_aliases = aliases
        .iter()
        .map(|(find, replacement)| crate::resolver_core::WorkspaceAlias {
            find: find.to_string(),
            replacement: replacement.to_string(),
        })
        .collect();
    project
}

#[test]
fn new_sorts_the_configs_by_precedence() {
    let core = ModuleResolverCore::new(vec![
        configured_project("/proj", "/proj/tsconfig.json", &[]),
        configured_project("/proj/pkg", "/proj/pkg/tsconfig.json", &[]),
    ]);

    let roots: Vec<&str> = core.configs().iter().map(|p| p.root.as_str()).collect();
    assert_eq!(roots, vec!["/proj/pkg", "/proj"]);
}

#[test]
fn nearest_config_for_path_delegates_correctly() {
    let core = ModuleResolverCore::new(vec![configured_project(
        "/proj",
        "/proj/tsconfig.json",
        &[],
    )]);

    assert_eq!(
        core.nearest_config_for_path("/proj/src/main.ts")
            .map(|p| p.root.as_str()),
        Some("/proj")
    );
    assert!(core.nearest_config_for_path("/elsewhere/main.ts").is_none());
}

#[test]
fn effective_configs_for_path_delegates_correctly() {
    let core = ModuleResolverCore::new(vec![configured_project(
        "/proj",
        "/proj/tsconfig.json",
        &[],
    )]);

    assert_eq!(
        core.effective_configs_for_path("/proj/src/main.ts").len(),
        1
    );
    assert!(core
        .effective_configs_for_path("/elsewhere/main.ts")
        .is_empty());
}

#[test]
fn project_for_ownership_delegates_correctly() {
    let core = ModuleResolverCore::new(vec![configured_project(
        "/proj",
        "/proj/tsconfig.json",
        &[],
    )]);
    let owner = crate::resolver_core::ProjectOwnership {
        project_root: "/proj".to_string(),
        tsconfig_path: Some("/proj/tsconfig.json".to_string()),
    };

    assert!(core.project_for_ownership(&owner).is_some());
}

#[test]
fn resolve_attempt_delegates_to_resolve_with_reader() {
    let view = known_world_view(&["/proj/src/util.ts"]);
    let core = ModuleResolverCore::new(vec![configured_project(
        "/proj",
        "/proj/tsconfig.json",
        &[("@/", "/proj/src")],
    )]);
    let request = crate::resolver_core::ResolveRequest {
        importer_id: "/proj/src/main.ts".to_string(),
        specifier: "@/util".to_string(),
        kind: crate::resolver_core::ResolveRequestKind::EsmImport,
        phase: crate::resolver_core::ResolvePhase::CodegenBlocker,
    };

    let outcome = core.resolve_attempt(&view, basis(), &request);
    match outcome {
        AttemptOutcome::Complete(CompletedAttempt {
            value: Some(result),
            ..
        }) => {
            assert_eq!(result.source_id, "/proj/src/util.ts");
            assert_eq!(
                result.resolution_kind,
                crate::resolver_core::ResolutionKind::WorkspaceAlias
            );
        }
        other => panic!("expected Complete(Some(_)), got {other:?}"),
    }
}

#[test]
fn resolve_for_project_attempt_delegates_to_resolve_for_project_with_reader() {
    let view = known_world_view(&["/proj/sibling.ts"]);
    let core = ModuleResolverCore::new(vec![configured_project(
        "/proj",
        "/proj/tsconfig.json",
        &[],
    )]);
    let owner = crate::resolver_core::ProjectOwnership {
        project_root: "/proj".to_string(),
        tsconfig_path: Some("/proj/tsconfig.json".to_string()),
    };

    let outcome =
        core.resolve_for_project_attempt(&view, basis(), &owner, "./sibling.ts", esm_import_ctx());
    match outcome {
        AttemptOutcome::Complete(CompletedAttempt {
            value: Some(result),
            ..
        }) => assert_eq!(result.source_id, "/proj/sibling.ts"),
        other => panic!("expected Complete(Some(_)), got {other:?}"),
    }
}

#[test]
fn real_kernel_accounts_completed_path_target_witnesses_without_unique_key_misclassification() {
    let mut project = configured_project("/proj", "/proj/tsconfig.json", &[]);
    project.compiler_options.paths = vec![(
        "pkg/*".to_string(),
        (0..9).map(|index| format!("generated/{index}/*")).collect(),
    )];
    project.references = (0..9)
        .map(|index| format!("/refs/{index}/tsconfig.json"))
        .collect();
    let mut projects = vec![project];
    projects.extend((0..9).map(|index| {
        configured_project(
            &format!("/refs/{index}"),
            &format!("/refs/{index}/tsconfig.json"),
            &[],
        )
    }));
    let core = ModuleResolverCore::new(projects);
    let request = crate::resolver_core::ResolveRequest {
        importer_id: "/proj/src/main.ts".to_string(),
        specifier: "pkg/item".to_string(),
        kind: crate::resolver_core::ResolveRequestKind::EsmImport,
        phase: crate::resolver_core::ResolvePhase::ProviderGraph,
    };
    let budgets = InputResolutionBudgets::try_tightened_with_retention(32, 8, 32_768, 16, 4, 8, 8)
        .expect("test policy");
    let retention = crate::resolver_core::InputResolutionRetention::new(budgets);
    let view = ResolverAttemptView::from_resolution_snapshot_with_operation_retention(
        Arc::new(ResolutionObservationSnapshot::with_stable_absent_defaults_for_test()),
        basis(),
        budgets,
        retention.clone(),
    );
    assert!(matches!(
        retention.scope(|| core.resolve_attempt(&view, basis(), &request)),
        AttemptOutcome::Terminal(
            AttemptFailure::InputResolutionCompletedWitnessRetentionLimit { maximum: 8, .. }
        )
    ));
}

#[test]
fn real_kernel_collapses_duplicate_candidate_observations_in_first_seen_order() {
    let request = crate::resolver_core::ResolveRequest {
        importer_id: "/proj/src/main.ts".to_string(),
        specifier: "pkg/item".to_string(),
        kind: crate::resolver_core::ResolveRequestKind::EsmImport,
        phase: crate::resolver_core::ResolvePhase::ProviderGraph,
    };
    let view = known_world_view(&[]);
    let resolve = |target_count| {
        let mut project = configured_project("/proj", "/proj/tsconfig.json", &[]);
        project.compiler_options.paths = vec![(
            "pkg/*".to_string(),
            std::iter::repeat_n("generated/*".to_string(), target_count).collect(),
        )];
        let core = ModuleResolverCore::new(vec![project]);
        let AttemptOutcome::Complete(CompletedAttempt {
            value: None,
            output,
        }) = core.resolve_attempt(&view, basis(), &request)
        else {
            panic!("a fully-known absent world must produce an exhausted miss");
        };
        output
    };

    let _ =
        crate::resolver_core::tsconfig_paths_resolution::take_path_mapping_candidate_evaluations();
    let control = resolve(1);
    let control_evaluations =
        crate::resolver_core::tsconfig_paths_resolution::take_path_mapping_candidate_evaluations();
    let duplicates = resolve(2_048);
    let duplicate_evaluations =
        crate::resolver_core::tsconfig_paths_resolution::take_path_mapping_candidate_evaluations();
    assert_eq!(duplicates, control);
    assert_eq!(duplicate_evaluations, control_evaluations);
    let observations = duplicates.consumed_resolution_observations();
    let unique: std::collections::HashSet<_> = observations.iter().collect();
    assert_eq!(observations.len(), unique.len());
}

#[test]
fn real_kernel_streams_more_aliases_than_the_live_geometry_maximum_to_a_late_hit() {
    let aliases = (0..9)
        .map(|index| ("pkg/".to_string(), format!("/generated/{index}/")))
        .collect::<Vec<_>>();
    let mut project = configured_project("/proj", "/proj/tsconfig.json", &[]);
    project.workspace_aliases = aliases
        .iter()
        .map(|(find, replacement)| crate::resolver_core::WorkspaceAlias {
            find: find.clone(),
            replacement: replacement.clone(),
        })
        .collect();
    project.references = (0..9)
        .map(|index| format!("/refs/{index}/tsconfig.json"))
        .collect();
    let mut projects = vec![project];
    projects.extend((0..9).map(|index| {
        configured_project(
            &format!("/refs/{index}"),
            &format!("/refs/{index}/tsconfig.json"),
            &[],
        )
    }));
    let core = ModuleResolverCore::new(projects);
    let request = crate::resolver_core::ResolveRequest {
        importer_id: "/proj/src/main.ts".to_string(),
        specifier: "pkg/item".to_string(),
        kind: crate::resolver_core::ResolveRequestKind::EsmImport,
        phase: crate::resolver_core::ResolvePhase::ProviderGraph,
    };
    let budgets =
        InputResolutionBudgets::try_tightened_with_retention(32, 8, 32_768, 16, 4, 1, 1_024)
            .expect("test policy");
    let retention = crate::resolver_core::InputResolutionRetention::new(budgets);
    let view = ResolverAttemptView::from_resolution_snapshot_with_operation_retention(
        Arc::new({
            let mut snapshot =
                ResolutionObservationSnapshot::with_stable_absent_defaults_for_test();
            snapshot.insert_path_probe(
                "/generated/8/item.ts".to_string(),
                crate::resolver_core::PathProbe::File,
            );
            snapshot.insert_real_path(
                "/generated/8/item.ts".to_string(),
                Some(Arc::from("/generated/8/item.ts")),
            );
            snapshot
        }),
        basis(),
        budgets,
        retention.clone(),
    );

    let _ =
        crate::resolver_core::tsconfig_paths_resolution::take_path_mapping_candidate_evaluations();
    let outcome = retention.scope(|| core.resolve_attempt(&view, basis(), &request));
    let AttemptOutcome::Complete(CompletedAttempt {
        value: Some(result),
        ..
    }) = outcome
    else {
        panic!("late alias hit must complete");
    };
    assert_eq!(result.source_id, "/generated/8/item.ts");
    assert_eq!(
        retention.retained_for_test().1,
        1,
        "only one alias geometry bundle was live"
    );
}

#[test]
fn workspace_alias_candidate_memo_does_not_outlive_its_geometry_lease_across_waves() {
    let core = ModuleResolverCore::new(vec![configured_project(
        "/proj",
        "/proj/tsconfig.json",
        &[("pkg/", "/generated/")],
    )]);
    let request = crate::resolver_core::ResolveRequest {
        importer_id: "/proj/src/main.ts".to_string(),
        specifier: "pkg/item".to_string(),
        kind: crate::resolver_core::ResolveRequestKind::EsmImport,
        phase: crate::resolver_core::ResolvePhase::ProviderGraph,
    };
    let budgets = InputResolutionBudgets::try_tightened_with_retention(16, 32, 4_096, 8, 2, 1, 64)
        .expect("test policy");
    let retention = crate::resolver_core::InputResolutionRetention::new(budgets);
    let frame = core.resolve_frame(&request);
    let empty = ResolverAttemptView::from_resolution_snapshot_with_operation_retention(
        Arc::new(ResolutionObservationSnapshot::default()),
        basis(),
        budgets,
        retention.clone(),
    );

    assert!(matches!(
        retention.scope(|| frame.attempt(&empty, basis())),
        AttemptOutcome::NeedInputs(_)
    ));
    assert_eq!(retention.retained_for_test().0, 0);
    assert!(
        !frame
            .memo
            .retains_probe_base_for_test("/generated/item"),
        "candidate-exclusive alias memo entries cannot survive after their live geometry lease is released"
    );

    let mut snapshot = ResolutionObservationSnapshot::with_stable_absent_defaults_for_test();
    snapshot.insert_path_probe(
        "/generated/item.ts".to_string(),
        crate::resolver_core::PathProbe::File,
    );
    snapshot.insert_real_path(
        "/generated/item.ts".to_string(),
        Some(Arc::from("/generated/item.ts")),
    );
    let complete = ResolverAttemptView::from_resolution_snapshot_with_operation_retention(
        Arc::new(snapshot),
        basis(),
        budgets,
        retention.clone(),
    );
    let AttemptOutcome::Complete(CompletedAttempt {
        value: Some(result),
        ..
    }) = retention.scope(|| frame.attempt(&complete, basis()))
    else {
        panic!("the same-basis next wave must preserve the real alias answer");
    };
    assert_eq!(result.source_id, "/generated/item.ts");
    assert_eq!(retention.retained_for_test().1, 1);

    // Mutation recipe: route the alias candidate through `frame.memo` again.
    // The first assertion turns RED because the memo then outlives the lease.
}

#[test]
fn real_kernel_completed_witness_limit_is_typed_at_max_plus_one() {
    let aliases = (0..8)
        .map(|index| ("pkg/".to_string(), format!("/generated/{index}/")))
        .collect::<Vec<_>>();
    let mut project = configured_project("/proj", "/proj/tsconfig.json", &[]);
    project.workspace_aliases = aliases
        .iter()
        .map(|(find, replacement)| crate::resolver_core::WorkspaceAlias {
            find: find.clone(),
            replacement: replacement.clone(),
        })
        .collect();
    let core = ModuleResolverCore::new(vec![project]);
    let request = crate::resolver_core::ResolveRequest {
        importer_id: "/proj/src/main.ts".to_string(),
        specifier: "pkg/item".to_string(),
        kind: crate::resolver_core::ResolveRequestKind::EsmImport,
        phase: crate::resolver_core::ResolvePhase::ProviderGraph,
    };
    let budgets = InputResolutionBudgets::try_tightened_with_retention(32, 8, 32_768, 16, 4, 1, 8)
        .expect("test policy");
    let retention = crate::resolver_core::InputResolutionRetention::new(budgets);
    let view = ResolverAttemptView::from_resolution_snapshot_with_operation_retention(
        Arc::new(ResolutionObservationSnapshot::with_stable_absent_defaults_for_test()),
        basis(),
        budgets,
        retention.clone(),
    );
    assert!(matches!(
        retention.scope(|| core.resolve_attempt(&view, basis(), &request)),
        AttemptOutcome::Terminal(
            AttemptFailure::InputResolutionCompletedWitnessRetentionLimit {
                retained: 8,
                prospective: 9,
                maximum: 8,
            }
        )
    ));
}

#[test]
fn normalization_collision_keeps_the_more_specific_alias_winner() {
    let core = ModuleResolverCore::new(vec![configured_project(
        "/proj",
        "/proj/tsconfig.json",
        &[
            ("pkg/", "/target/../target/specific/"),
            ("pkg/special/", "/target/specific/special/"),
        ],
    )]);
    let request = crate::resolver_core::ResolveRequest {
        importer_id: "/proj/src/main.ts".to_string(),
        specifier: "pkg/special/item".to_string(),
        kind: crate::resolver_core::ResolveRequestKind::EsmImport,
        phase: crate::resolver_core::ResolvePhase::ProviderGraph,
    };
    let view = known_world_view(&["/target/specific/special/item.ts"]);
    let _ = crate::resolver_core::tsconfig_paths_resolution::take_workspace_alias_evaluations();
    let AttemptOutcome::Complete(CompletedAttempt {
        value: Some(result),
        ..
    }) = core.resolve_attempt(&view, basis(), &request)
    else {
        panic!("specific alias must resolve");
    };
    assert_eq!(result.source_id, "/target/specific/special/item.ts");
    assert_eq!(
        crate::resolver_core::tsconfig_paths_resolution::take_workspace_alias_evaluations(),
        vec!["pkg/special/"],
        "priority ordering precedes normalized-target deduplication",
    );
}

#[test]
fn pinned_live_alias_geometry_rejects_before_candidate_evaluation() {
    let core = ModuleResolverCore::new(vec![configured_project(
        "/proj",
        "/proj/tsconfig.json",
        &[("pkg/", "/target/")],
    )]);
    let request = crate::resolver_core::ResolveRequest {
        importer_id: "/proj/src/main.ts".to_string(),
        specifier: "pkg/item".to_string(),
        kind: crate::resolver_core::ResolveRequestKind::EsmImport,
        phase: crate::resolver_core::ResolvePhase::ProviderGraph,
    };
    let budgets = InputResolutionBudgets::try_tightened_with_retention(8, 8, 128, 4, 2, 2, 64)
        .expect("test policy");
    let retention = crate::resolver_core::InputResolutionRetention::new(budgets);
    let view = ResolverAttemptView::from_resolution_snapshot_with_operation_retention(
        Arc::new(ResolutionObservationSnapshot::with_stable_absent_defaults_for_test()),
        basis(),
        budgets,
        retention.clone(),
    );
    retention.force_alias_retained_for_test(1);
    let _ =
        crate::resolver_core::tsconfig_paths_resolution::take_path_mapping_candidate_evaluations();
    let _ = retention.scope(|| core.resolve_attempt(&view, basis(), &request));
    assert!(
        crate::resolver_core::tsconfig_paths_resolution::take_path_mapping_candidate_evaluations()
            > 0
    );

    retention.force_alias_retained_for_test(2);
    let _ =
        crate::resolver_core::tsconfig_paths_resolution::take_path_mapping_candidate_evaluations();
    assert_eq!(
        retention.scope(|| core.resolve_attempt(&view, basis(), &request)),
        AttemptOutcome::Terminal(AttemptFailure::InputResolutionAliasGeometryRetentionLimit {
            retained: 2,
            prospective: 3,
            maximum: 2,
        })
    );
    assert_eq!(
        crate::resolver_core::tsconfig_paths_resolution::take_path_mapping_candidate_evaluations(),
        0,
        "the rejected bundle cannot allocate probes or invoke the loader",
    );
    retention.force_alias_retained_for_test(0);
}

#[test]
fn preferred_specifier_candidates_delegates_correctly() {
    let mut project = configured_project("/proj", "/proj/tsconfig.json", &[("@/", "/proj/src")]);
    project.compiler_options.paths = vec![];
    let core = ModuleResolverCore::new(vec![project]);

    let candidates = core
        .preferred_specifier_candidates("/proj/src/main.ts", "/proj/src/util.ts")
        .expect("importer is owned");
    assert_eq!(candidates, vec!["@/util.ts".to_string()]);
}

#[test]
fn project_exact_result_delegates_correctly() {
    let core = ModuleResolverCore::new(vec![configured_project(
        "/proj",
        "/proj/tsconfig.json",
        &[],
    )]);

    let result = core.project_exact_result(
        "/proj/src/main.ts",
        "whatever",
        "/proj/src/exact.ts".to_string(),
        esm_import_ctx(),
    );
    assert_eq!(result.source_id, "/proj/src/exact.ts");
    assert_eq!(
        result.resolution_kind,
        crate::resolver_core::ResolutionKind::Bundler
    );
}

#[derive(Default)]
struct GrowingResolutionSnapshot {
    observations: Arc<ResolutionObservationSnapshot>,
}

impl GrowingResolutionSnapshot {
    fn view(&self, basis: ResolutionBasis) -> ResolverAttemptView {
        ResolverAttemptView::from_resolution_snapshot(Arc::clone(&self.observations), basis)
    }

    fn load(&mut self, keys: &[InputKey]) {
        for key in keys {
            match key {
                InputKey::PathProbe { path } => {
                    let probe = if path.as_ref() == "/proj/src/util.ts" {
                        crate::resolver_core::PathProbe::File
                    } else {
                        crate::resolver_core::PathProbe::Absent
                    };
                    Arc::make_mut(&mut self.observations)
                        .insert_path_probe(path.to_string(), probe);
                }
                InputKey::RealPath { path } => {
                    Arc::make_mut(&mut self.observations)
                        .insert_real_path(path.to_string(), Some(Arc::clone(path)));
                }
                InputKey::PackageManifest { directory } => {
                    Arc::make_mut(&mut self.observations)
                        .insert_package_manifest(directory.to_string(), None);
                }
                other => panic!("unexpected module-resolution input: {other:?}"),
            }
        }
    }
}

#[test]
fn one_resolution_frame_preserves_answers_and_load_sets_across_input_waves() {
    let core = ModuleResolverCore::new(vec![configured_project(
        "/proj",
        "/proj/tsconfig.json",
        &[("@/", "/proj/src")],
    )]);
    let request = crate::resolver_core::ResolveRequest {
        importer_id: "/proj/src/main.ts".to_string(),
        specifier: "@/util".to_string(),
        kind: crate::resolver_core::ResolveRequestKind::EsmImport,
        phase: crate::resolver_core::ResolvePhase::ProviderGraph,
    };
    let basis = basis();
    let mut baseline_snapshot = GrowingResolutionSnapshot::default();
    let mut baseline_load_sets = Vec::new();
    let (baseline_value, baseline_output) = loop {
        let view = baseline_snapshot.view(basis);
        let outcome = core.resolve_attempt(&view, basis, &request);
        drop(view);
        match outcome {
            AttemptOutcome::Complete(CompletedAttempt { value, output }) => break (value, output),
            AttemptOutcome::NeedInputs(load_set) => {
                baseline_load_sets.push(load_set.keys().to_vec());
                baseline_snapshot.load(load_set.keys());
            }
            AttemptOutcome::Terminal(failure) => {
                panic!("one-shot retry attempt failed: {failure:?}")
            }
        }
    };
    let mut snapshot = GrowingResolutionSnapshot::default();
    let mut frame_load_sets = Vec::new();
    let frame = core.resolve_frame(&request);
    let (incremental_value, incremental_output) = loop {
        let view = snapshot.view(basis);
        let outcome = frame.attempt(&view, basis);
        drop(view);
        match outcome {
            AttemptOutcome::Complete(CompletedAttempt { value, output }) => break (value, output),
            AttemptOutcome::NeedInputs(load_set) => {
                frame_load_sets.push(load_set.keys().to_vec());
                snapshot.load(load_set.keys());
            }
            AttemptOutcome::Terminal(failure) => {
                panic!("incremental frame attempt failed: {failure:?}")
            }
        }
    };

    assert!(
        frame_load_sets.len() >= 3,
        "the fixture must exercise at least three missing-input waves: {frame_load_sets:?}"
    );
    assert_eq!(frame_load_sets, baseline_load_sets);
    assert_eq!(incremental_value, baseline_value);
    assert_eq!(
        incremental_output.consumed_resolution_observations(),
        baseline_output.consumed_resolution_observations()
    );

    let full_view = snapshot.view(basis);
    let CompletedAttempt {
        value: one_shot_value,
        output: one_shot_output,
    } = core
        .resolve_attempt(&full_view, basis, &request)
        .complete()
        .expect("the fully loaded one-shot attempt must complete");
    assert_eq!(incremental_value, one_shot_value);
    assert_eq!(
        incremental_output.consumed_resolution_observations(),
        one_shot_output.consumed_resolution_observations(),
        "incremental retries must preserve the complete ordered consumed-selector replay"
    );
}
