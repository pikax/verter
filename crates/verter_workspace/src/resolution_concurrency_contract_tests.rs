#![doc = include_str!("../../../docs/contributing/path-precise-resolution-currency.md")]

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;

use super::{
    resolution_test_hooks::{
        self, ResolutionAdmissionObservation, ResolutionPhase, ResolutionTransactionObservation,
        ResolutionWorldSignature,
    },
    Engine,
};
use crate::canonical_path::CanonicalPath;
use crate::project_graph::{ProjectGraph, ProjectRank, VfsProjectConfig};
use crate::resolver::take_input_resolution_budget_events_for_test;
use crate::traits::{WorkspaceAccess, WorkspaceRead};
use crate::types::{ExactResolution, ExactResolutionResult, ParsedEdge};
use verter_semantic::resolver_core::{
    normalize_canonical_id, AttemptFailure, IdeProjectCompilerOptions, InputResolutionBudgetMeter,
    InputResolutionBudgets, ProjectOwnership, ResolutionContext, ResolvePhase, ResolveRequestKind,
    ResolveResult, WorkspaceAlias,
};

const CONTEXT: ResolutionContext = ResolutionContext {
    phase: ResolvePhase::ProviderGraph,
    kind: ResolveRequestKind::EsmImport,
};

struct ConcurrentReader {
    files: RwLock<HashMap<String, Arc<str>>>,
    probe_hook_path: RwLock<Option<String>>,
    probe_hook_fired: AtomicBool,
}

impl ConcurrentReader {
    fn new(files: &[&str]) -> Self {
        let files = files
            .iter()
            .map(|path| {
                (
                    normalize_canonical_id(path),
                    Arc::<str>::from("// concurrency fixture"),
                )
            })
            .collect();
        Self {
            files: RwLock::new(files),
            probe_hook_path: RwLock::new(None),
            probe_hook_fired: AtomicBool::new(false),
        }
    }

    fn insert(&self, path: &str) {
        self.files.write().insert(
            normalize_canonical_id(path),
            Arc::from("// concurrency fixture"),
        );
    }

    fn remove(&self, path: &str) {
        self.files.write().remove(&normalize_canonical_id(path));
    }

    fn fire_hook_after_probe(&self, path: &str) {
        *self.probe_hook_path.write() = Some(normalize_canonical_id(path));
        self.probe_hook_fired.store(false, Ordering::Relaxed);
    }
}

impl WorkspaceRead for ConcurrentReader {
    fn preflight_resolution_inputs_bounded(
        &self,
        keys: &[verter_semantic::resolver_core::InputKey],
        basis: verter_semantic::resolver_core::ResolutionBasis,
    ) -> Result<crate::resolver::ResolutionInputReservationBatch, AttemptFailure> {
        crate::resolver::preflight_workspace_inputs_for_test(self, keys, basis)
    }

    fn load_preflighted_resolution_inputs(
        &self,
        reservation: &crate::resolver::ResolutionInputReservationBatch,
    ) -> Result<crate::resolver::LoadedResolutionInputBatch, AttemptFailure> {
        crate::resolver::load_workspace_inputs_for_test(self, reservation)
    }

    fn read_file(&self, canonical_id: &str) -> Option<Arc<str>> {
        self.files
            .read()
            .get(&normalize_canonical_id(canonical_id))
            .cloned()
    }

    fn file_exists(&self, canonical_id: &str) -> bool {
        self.probe_path(canonical_id) == verter_semantic::resolver_core::PathProbe::File
    }

    fn probe_path(&self, canonical_id: &str) -> verter_semantic::resolver_core::PathProbe {
        let normalized = normalize_canonical_id(canonical_id);
        let exists = self.files.read().contains_key(&normalized);
        let should_fire = self
            .probe_hook_path
            .read()
            .as_ref()
            .is_some_and(|path| path == &normalized)
            && !self.probe_hook_fired.swap(true, Ordering::Relaxed);
        if should_fire {
            resolution_test_hooks::fire(ResolutionPhase::FilesystemProbing);
        }
        if exists {
            verter_semantic::resolver_core::PathProbe::File
        } else {
            verter_semantic::resolver_core::PathProbe::Absent
        }
    }

    fn resolution_event_bridge_complete(&self) -> bool {
        true
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        let canonical = normalize_canonical_id(canonical_id);
        self.files
            .read()
            .contains_key(&canonical)
            .then_some(canonical)
    }

    fn reverse_deps_for(&self, _id: &str) -> Vec<String> {
        Vec::new()
    }

    fn forward_deps_for(&self, _id: &str) -> Vec<String> {
        Vec::new()
    }

    fn dependency_snapshot(
        &self,
        _id: &str,
    ) -> Option<crate::exact_resolution::DependencySnapshotView> {
        None
    }
}

impl WorkspaceAccess for ConcurrentReader {
    fn record_parsed_edges(&self, _canonical_id: &str, _edges: &[ParsedEdge]) {}

    fn set_exact_resolutions(
        &self,
        _canonical_id: &str,
        _resolutions: Vec<ExactResolution>,
    ) -> ExactResolutionResult {
        ExactResolutionResult::default()
    }

    fn record_parsed_edges_with_exact_resolutions(
        &self,
        _canonical_id: &str,
        _edges: &[ParsedEdge],
        _resolutions: Vec<ExactResolution>,
    ) -> ExactResolutionResult {
        ExactResolutionResult::default()
    }

    fn replace_semantic_transitive(&self, _canonical_id: &str, _deps: BTreeSet<String>) {}

    fn set_default_resolve_extensions(&self, _host_extensions: Vec<String>) {}

    fn record_ambient_dependency(&self, _consumer: &str, _virtual_id: &str) {}
}

fn exact(specifier: &str, target: &str) -> ExactResolution {
    ExactResolution {
        specifier: specifier.to_string(),
        phase: CONTEXT.phase,
        kind: CONTEXT.kind,
        resolved_canonical_id: Some(target.to_string()),
        possible_canonical_ids: vec![target.to_string()],
    }
}

fn resolve(
    engine: &Engine,
    reader: &ConcurrentReader,
    importer: &str,
    specifier: &str,
) -> Option<ResolveResult> {
    engine.resolve_import(reader, importer, specifier, CONTEXT)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolutionWorldOracle {
    signature: ResolutionWorldSignature,
    result: Option<ResolveResult>,
}

fn oracle_for_world(
    signature: ResolutionWorldSignature,
    reader: &ConcurrentReader,
    importer: &str,
    specifier: &str,
    publish_world: impl FnOnce(&Engine),
) -> ResolutionWorldOracle {
    let isolated_world = Engine::new();
    publish_world(&isolated_world);
    ResolutionWorldOracle {
        signature,
        result: resolve(&isolated_world, reader, importer, specifier),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AllowedConcurrencyOutcome {
    WhollyOld,
    WhollyNew,
    RetryEquivalent,
    ReturnOnly,
}

fn assert_single_world_outcome(
    actual: &Option<ResolveResult>,
    old_oracle: &ResolutionWorldOracle,
    new_oracle: &ResolutionWorldOracle,
    observation: &ResolutionTransactionObservation,
) -> AllowedConcurrencyOutcome {
    assert_eq!(
        observation.final_world, new_oracle.signature,
        "the mutation fixture must publish the new world"
    );

    let Some(admission) = observation.admission.as_ref() else {
        panic!("the transaction must report Cacheable or typed ReturnOnly admission");
    };
    let ResolutionAdmissionObservation::Cacheable(admission) = admission else {
        return AllowedConcurrencyOutcome::ReturnOnly;
    };

    let matches_old = actual == &old_oracle.result;
    let matches_new = actual == &new_oracle.result;
    assert_eq!(
        admission.captured, admission.validated,
        "cacheable admission must validate against the same complete world \
         signature the attempt captured"
    );
    assert_eq!(
        observation.attempts.last(),
        Some(&admission.captured),
        "cacheable admission must belong to the result-producing captured attempt"
    );
    let admitted_wholly_old = admission.captured == old_oracle.signature && matches_old;
    let admitted_wholly_new = admission.captured == new_oracle.signature && matches_new;
    assert!(
        admitted_wholly_old || admitted_wholly_new,
        "the returned result and its admitted signature must belong to one \
         captured world; a result from one oracle under the other world's \
         signature is mixed-world"
    );

    if observation.attempts.len() > 1 {
        return AllowedConcurrencyOutcome::RetryEquivalent;
    }
    if admitted_wholly_old {
        return AllowedConcurrencyOutcome::WhollyOld;
    }
    if admitted_wholly_new {
        return AllowedConcurrencyOutcome::WhollyNew;
    }
    unreachable!("the single-world assertion above exhausts cacheable outcomes");
}

fn publish_alias(engine: &Engine, project_root: &str, alias_target: &str) {
    let membership = crate::membership::configured_membership_match_all_under_root(
        &CanonicalPath::new(project_root),
    );
    *engine.project_graph.write() = ProjectGraph::from_configs(vec![VfsProjectConfig {
        root: project_root.to_string(),
        rank: ProjectRank::Explicit,
        tsconfig_path: Some(format!("{project_root}/tsconfig.json")),
        root_files: Vec::new(),
        extensions: vec![".ts".to_string(), ".tsx".to_string(), ".vue".to_string()],
        workspace_root: "/".to_string(),
        workspace_aliases: vec![WorkspaceAlias {
            find: "@dep".to_string(),
            replacement: alias_target.to_string(),
        }],
        compiler_options: IdeProjectCompilerOptions::default(),
        references: Vec::new(),
        membership,
    }]);
    engine.rebuild_and_publish();
}

fn publish_unrelated_project(engine: &Engine) {
    publish_alias(engine, "/other", "/other/dep");
}

#[test]
fn resolution_concurrency_mutation_during_exact_table_lookup_never_admits_mixed_world() {
    const IMPORTER: &str = "/p/main.ts";
    let engine = Arc::new(Engine::new());
    let reader = ConcurrentReader::new(&["/p/ordinary.ts"]);
    let old_world = ResolutionWorldSignature::from_facts([
        "ExactResolution(./ordinary)=miss",
        "PathProbe(/p/ordinary.ts)=File",
    ]);
    let new_world = ResolutionWorldSignature::from_facts([
        "ExactResolution(./ordinary)=/p/exact.ts",
        "PathProbe(/p/ordinary.ts)=File",
    ]);
    let old_oracle = oracle_for_world(old_world.clone(), &reader, IMPORTER, "./ordinary", |_| {});
    let mutate = Arc::clone(&engine);
    let published_new = new_world.clone();

    let (result, observation) =
        resolution_test_hooks::with_world_contract(old_world.clone(), || {
            resolution_test_hooks::with_hook(
                ResolutionPhase::ExactTableLookup,
                move || {
                    assert!(
                        mutate
                            .set_exact_resolutions(
                                IMPORTER,
                                vec![exact("./ordinary", "/p/exact.ts")],
                            )
                            .changed
                    );
                    resolution_test_hooks::publish_world(published_new);
                },
                || resolve(&engine, &reader, IMPORTER, "./ordinary"),
            )
        });
    let new_oracle = oracle_for_world(
        new_world.clone(),
        &reader,
        IMPORTER,
        "./ordinary",
        |oracle_engine| {
            oracle_engine.set_exact_resolutions(IMPORTER, vec![exact("./ordinary", "/p/exact.ts")]);
        },
    );
    let _outcome = assert_single_world_outcome(&result, &old_oracle, &new_oracle, &observation);
}

#[test]
fn resolution_concurrency_mutation_during_project_selection_never_admits_mixed_world() {
    const IMPORTER: &str = "/p/main.ts";
    let engine = Arc::new(Engine::new());
    publish_alias(&engine, "/p", "/p/old");
    let reader = ConcurrentReader::new(&["/p/old.ts", "/p/new.ts"]);
    let old_world =
        ResolutionWorldSignature::from_facts(["ContextSelection(/p/main.ts)=alias:/p/old"]);
    let new_world =
        ResolutionWorldSignature::from_facts(["ContextSelection(/p/main.ts)=alias:/p/new"]);
    let old_oracle = oracle_for_world(
        old_world.clone(),
        &reader,
        IMPORTER,
        "@dep",
        |oracle_engine| publish_alias(oracle_engine, "/p", "/p/old"),
    );
    let mutate = Arc::clone(&engine);
    let published_new = new_world.clone();

    let (result, observation) =
        resolution_test_hooks::with_world_contract(old_world.clone(), || {
            resolution_test_hooks::with_hook(
                ResolutionPhase::ProjectSelection,
                move || {
                    publish_alias(&mutate, "/p", "/p/new");
                    resolution_test_hooks::publish_world(published_new);
                },
                || resolve(&engine, &reader, IMPORTER, "@dep"),
            )
        });
    let new_oracle = oracle_for_world(
        new_world.clone(),
        &reader,
        IMPORTER,
        "@dep",
        |oracle_engine| publish_alias(oracle_engine, "/p", "/p/new"),
    );
    let _outcome = assert_single_world_outcome(&result, &old_oracle, &new_oracle, &observation);
}

#[test]
fn resolution_concurrency_mutation_during_filesystem_probing_never_admits_mixed_world() {
    const IMPORTER: &str = "/p/main.ts";
    let engine = Arc::new(Engine::new());
    let reader = Arc::new(ConcurrentReader::new(&["/p/mod.tsx"]));
    let old_world = ResolutionWorldSignature::from_facts([
        "PathProbe(/p/mod.ts)=Absent",
        "PathProbe(/p/mod.tsx)=File",
    ]);
    let new_world = ResolutionWorldSignature::from_facts([
        "PathProbe(/p/mod.ts)=File",
        "PathProbe(/p/mod.tsx)=Absent",
    ]);
    let old_oracle = oracle_for_world(old_world.clone(), &reader, IMPORTER, "./mod.js", |_| {});
    reader.fire_hook_after_probe("/p/mod.ts");
    let mutate_engine = Arc::clone(&engine);
    let mutate_reader = Arc::clone(&reader);
    let published_new = new_world.clone();

    let (result, observation) =
        resolution_test_hooks::with_world_contract(old_world.clone(), || {
            resolution_test_hooks::with_hook(
                ResolutionPhase::FilesystemProbing,
                move || {
                    mutate_reader.remove("/p/mod.tsx");
                    mutate_reader.insert("/p/mod.ts");
                    mutate_engine.bump_content_generation_for("/p/mod.ts");
                    resolution_test_hooks::publish_world(published_new);
                },
                || resolve(&engine, &reader, IMPORTER, "./mod.js"),
            )
        });
    let new_oracle = oracle_for_world(new_world.clone(), &reader, IMPORTER, "./mod.js", |_| {});
    let _outcome = assert_single_world_outcome(&result, &old_oracle, &new_oracle, &observation);
}

#[test]
fn resolution_concurrency_mutation_during_provider_projection_never_admits_mixed_world() {
    const IMPORTER: &str = "/p/main.ts";
    let engine = Arc::new(Engine::new());
    publish_alias(&engine, "/p", "/p/Comp.vue");
    let reader = ConcurrentReader::new(&["/p/Comp.vue"]);
    let old_world = ResolutionWorldSignature::from_facts([
        "ContextSelection(/p/main.ts)=project:/p",
        "ProviderPolicy(/p/Comp.vue)=old",
    ]);
    let new_world = ResolutionWorldSignature::from_facts([
        "ContextSelection(/p/main.ts)=no-project",
        "ProviderPolicy(/p/Comp.vue)=new",
    ]);
    let old_oracle = oracle_for_world(
        old_world.clone(),
        &reader,
        IMPORTER,
        "@dep",
        |oracle_engine| publish_alias(oracle_engine, "/p", "/p/Comp.vue"),
    );
    let mutate = Arc::clone(&engine);
    let published_new = new_world.clone();

    let (result, observation) =
        resolution_test_hooks::with_world_contract(old_world.clone(), || {
            resolution_test_hooks::with_hook(
                ResolutionPhase::ProviderProjection,
                move || {
                    publish_unrelated_project(&mutate);
                    resolution_test_hooks::publish_world(published_new);
                },
                || resolve(&engine, &reader, IMPORTER, "@dep"),
            )
        });
    let new_oracle = oracle_for_world(
        new_world.clone(),
        &reader,
        IMPORTER,
        "@dep",
        publish_unrelated_project,
    );
    let _outcome = assert_single_world_outcome(&result, &old_oracle, &new_oracle, &observation);
}

#[test]
fn resolution_concurrency_mutation_during_pre_admission_validation_never_admits_mixed_world() {
    const IMPORTER: &str = "/p/main.ts";
    let engine = Arc::new(Engine::new());
    let reader = ConcurrentReader::new(&["/p/ordinary.ts"]);
    let old_world = ResolutionWorldSignature::from_facts([
        "ExactResolution(./ordinary)=miss",
        "PathProbe(/p/ordinary.ts)=File",
    ]);
    let new_world = ResolutionWorldSignature::from_facts([
        "ExactResolution(./ordinary)=/p/exact-at-admission.ts",
        "PathProbe(/p/ordinary.ts)=File",
    ]);
    let old_oracle = oracle_for_world(old_world.clone(), &reader, IMPORTER, "./ordinary", |_| {});
    let mutate = Arc::clone(&engine);
    let published_new = new_world.clone();

    let (result, observation) =
        resolution_test_hooks::with_world_contract(old_world.clone(), || {
            resolution_test_hooks::with_hook(
                ResolutionPhase::PreAdmissionValidation,
                move || {
                    mutate.set_exact_resolutions(
                        IMPORTER,
                        vec![exact("./ordinary", "/p/exact-at-admission.ts")],
                    );
                    resolution_test_hooks::publish_world(published_new);
                },
                || resolve(&engine, &reader, IMPORTER, "./ordinary"),
            )
        });
    let new_oracle = oracle_for_world(
        new_world.clone(),
        &reader,
        IMPORTER,
        "./ordinary",
        |oracle_engine| {
            oracle_engine.set_exact_resolutions(
                IMPORTER,
                vec![exact("./ordinary", "/p/exact-at-admission.ts")],
            );
        },
    );
    let _outcome = assert_single_world_outcome(&result, &old_oracle, &new_oracle, &observation);
}

#[test]
fn resolution_concurrency_mutation_during_request_completion_never_admits_mixed_world() {
    const IMPORTER: &str = "/p/main.ts";
    let engine = Arc::new(Engine::new());
    let reader = ConcurrentReader::new(&["/p/ordinary.ts"]);
    let old_world = ResolutionWorldSignature::from_facts([
        "ExactResolution(./ordinary)=miss",
        "CompletionPopulation=old",
    ]);
    let new_world = ResolutionWorldSignature::from_facts([
        "ExactResolution(./ordinary)=/p/exact-at-completion.ts",
        "CompletionPopulation=new",
    ]);
    let old_oracle = oracle_for_world(old_world.clone(), &reader, IMPORTER, "./ordinary", |_| {});
    let mutate = Arc::clone(&engine);
    let published_new = new_world.clone();

    let (result, observation) =
        resolution_test_hooks::with_world_contract(old_world.clone(), || {
            resolution_test_hooks::with_hook(
                ResolutionPhase::RequestCompletion,
                move || {
                    mutate.set_exact_resolutions(
                        IMPORTER,
                        vec![exact("./ordinary", "/p/exact-at-completion.ts")],
                    );
                    resolution_test_hooks::publish_world(published_new);
                },
                || resolve(&engine, &reader, IMPORTER, "./ordinary"),
            )
        });
    let new_oracle = oracle_for_world(
        new_world.clone(),
        &reader,
        IMPORTER,
        "./ordinary",
        |oracle_engine| {
            oracle_engine.set_exact_resolutions(
                IMPORTER,
                vec![exact("./ordinary", "/p/exact-at-completion.ts")],
            );
        },
    );
    let _outcome = assert_single_world_outcome(&result, &old_oracle, &new_oracle, &observation);
}

#[test]
fn conditional_commit_restarts_share_one_churn_ledger_and_a_new_request_resets_it() {
    const IMPORTER: &str = "/p/main.ts";
    const SPECIFIER: &str = "./ordinary";
    let budgets =
        InputResolutionBudgets::try_tightened(32, 128, 32_768, 16, 1).expect("test policy");
    let engine = Arc::new(Engine::new_with_input_resolution_budgets(budgets));
    let reader = ConcurrentReader::new(&["/p/ordinary.ts"]);
    let mutations = Arc::new(AtomicUsize::new(0));
    let mutate_engine = Arc::clone(&engine);
    let mutate_count = Arc::clone(&mutations);

    let refused = resolution_test_hooks::with_repeating_hook(
        ResolutionPhase::PreAdmissionValidation,
        move || {
            let ordinal = mutate_count.fetch_add(1, Ordering::AcqRel);
            let target = if ordinal % 2 == 0 {
                "/p/exact-a.ts"
            } else {
                "/p/exact-b.ts"
            };
            mutate_engine.set_exact_resolutions(IMPORTER, vec![exact(SPECIFIER, target)]);
        },
        || engine.resolve_import_outcome(&reader, IMPORTER, SPECIFIER, CONTEXT),
    );

    assert_eq!(mutations.load(Ordering::Acquire), 2);
    assert_eq!(
        refused.non_admission_reason(),
        Some(verter_audit::NonAdmissionReason::BudgetExceeded),
        "the second conditional-commit restart must breach the same operation ledger"
    );
    assert!(
        refused.result().is_none(),
        "a churn rejection must discard the answer computed from the superseded world"
    );
    assert!(engine
        .cached_resolution_query_for_test(
            IMPORTER,
            SPECIFIER,
            CONTEXT,
            reader.resolution_population(),
        )
        .is_none());
    assert!(
        engine.reverse_deps_for("/p/exact-a.ts").is_empty(),
        "the superseded answer must not enter the reverse index"
    );

    let fresh = engine.resolve_import_outcome(&reader, IMPORTER, SPECIFIER, CONTEXT);
    assert_eq!(
        fresh.result().map(|result| result.source_id.as_str()),
        Some("/p/exact-b.ts")
    );
    assert!(
        fresh.is_cacheable(),
        "a new operation must receive a fresh ledger"
    );

    // Mutation controls: recreating the ledger inside the outer retry loop,
    // or moving the churn charge after `continue`, makes the first assertion
    // run to the provider retry cap instead of producing BudgetExceeded.
}

#[test]
fn ratified_outer_churn_boundary_runs_the_ninth_attempt_and_rejects_only_its_restart() {
    const IMPORTER: &str = "/p/main.ts";
    const SPECIFIER: &str = "./ordinary";
    let reader = ConcurrentReader::new(&["/p/ordinary.ts"]);

    let accepted_engine = Arc::new(Engine::new());
    let accepted_hook_calls = Arc::new(AtomicUsize::new(0));
    let mutate_engine = Arc::clone(&accepted_engine);
    let hook_calls = Arc::clone(&accepted_hook_calls);
    let accepted = resolution_test_hooks::with_repeating_hook(
        ResolutionPhase::PreAdmissionValidation,
        move || {
            let ordinal = hook_calls.fetch_add(1, Ordering::AcqRel);
            if ordinal < InputResolutionBudgets::RATIFIED.churn() as usize {
                let target = if ordinal % 2 == 0 {
                    "/p/exact-a.ts"
                } else {
                    "/p/exact-b.ts"
                };
                mutate_engine.set_exact_resolutions(IMPORTER, vec![exact(SPECIFIER, target)]);
            }
        },
        || accepted_engine.resolve_import_outcome(&reader, IMPORTER, SPECIFIER, CONTEXT),
    );
    assert_eq!(accepted_hook_calls.load(Ordering::Acquire), 9);
    assert_eq!(
        accepted.result().map(|result| result.source_id.as_str()),
        Some("/p/exact-b.ts"),
        "eight inclusive restarts must still run and admit the ninth attempt"
    );
    assert!(accepted.is_cacheable());

    let _ = take_input_resolution_budget_events_for_test();
    let refused_engine = Arc::new(Engine::new());
    let refused_hook_calls = Arc::new(AtomicUsize::new(0));
    let mutate_engine = Arc::clone(&refused_engine);
    let hook_calls = Arc::clone(&refused_hook_calls);
    let refused = resolution_test_hooks::with_repeating_hook(
        ResolutionPhase::PreAdmissionValidation,
        move || {
            let ordinal = hook_calls.fetch_add(1, Ordering::AcqRel);
            let target = if ordinal % 2 == 0 {
                "/p/exact-a.ts"
            } else {
                "/p/exact-b.ts"
            };
            mutate_engine.set_exact_resolutions(IMPORTER, vec![exact(SPECIFIER, target)]);
        },
        || refused_engine.resolve_import_outcome(&reader, IMPORTER, SPECIFIER, CONTEXT),
    );
    assert_eq!(
        refused_hook_calls.load(Ordering::Acquire),
        9,
        "the ninth attempt must run before its prospective ninth restart is rejected"
    );
    assert_eq!(
        refused.non_admission_reason(),
        Some(verter_audit::NonAdmissionReason::BudgetExceeded)
    );
    assert!(refused.result().is_none());
    assert!(refused_engine
        .cached_resolution_query_for_test(
            IMPORTER,
            SPECIFIER,
            CONTEXT,
            reader.resolution_population(),
        )
        .is_none());
    assert!(
        refused_engine.reverse_deps_for("/p/exact-b.ts").is_empty(),
        "the answer computed before the final exact-A publication must not enter the reverse index"
    );

    let events = take_input_resolution_budget_events_for_test();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].meter, InputResolutionBudgetMeter::Churn);
    assert_eq!(events[0].consumed, 8);
    assert_eq!(events[0].prospective, 9);
    assert_eq!(events[0].maximum, 8);

    // Mutation control: restoring an independent `for _ in 0..8` world loop
    // stops after eight hook calls and reports retry exhaustion before this
    // exact semantic churn event can exist.
}

#[test]
fn parsed_edge_commit_restarts_share_one_churn_ledger_and_discard_stale_edges() {
    const IMPORTER: &str = "/p/main.ts";
    let budgets =
        InputResolutionBudgets::try_tightened(32, 128, 32_768, 16, 1).expect("test policy");
    let engine = Arc::new(Engine::new_with_input_resolution_budgets(budgets));
    let reader = ConcurrentReader::new(&["/p/ordinary.ts"]);
    let mutations = Arc::new(AtomicUsize::new(0));
    let mutate_engine = Arc::clone(&engine);
    let mutate_count = Arc::clone(&mutations);
    let edges = [ParsedEdge::Relative {
        specifier: "./ordinary".to_string(),
        kind: ResolveRequestKind::EsmImport,
    }];

    let _ = take_input_resolution_budget_events_for_test();
    resolution_test_hooks::with_repeating_hook(
        ResolutionPhase::ParsedEdgePreCommit,
        move || {
            let ordinal = mutate_count.fetch_add(1, Ordering::AcqRel);
            let target = if ordinal % 2 == 0 {
                "/p/exact-a.ts"
            } else {
                "/p/exact-b.ts"
            };
            mutate_engine.set_exact_resolutions(IMPORTER, vec![exact("./forced", target)]);
        },
        || engine.record_parsed_edges(&reader, IMPORTER, &edges),
    );

    assert_eq!(mutations.load(Ordering::Acquire), 2);
    assert!(
        !engine
            .forward_deps_for(IMPORTER)
            .iter()
            .any(|dependency| dependency == "/p/ordinary.ts"),
        "the superseded parsed-edge result must not enter the forward index"
    );
    let events = take_input_resolution_budget_events_for_test();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].meter, InputResolutionBudgetMeter::Churn);
    assert_eq!((events[0].consumed, events[0].prospective), (1, 2));

    engine.record_parsed_edges(&reader, IMPORTER, &edges);
    assert_eq!(engine.forward_deps_for(IMPORTER), vec!["/p/ordinary.ts"]);
}

#[test]
fn parsed_edge_exact_companion_rejects_the_ninth_restart_after_running_nine_attempts() {
    const IMPORTER: &str = "/p/main.ts";
    let engine = Arc::new(Engine::new());
    let reader = ConcurrentReader::new(&["/p/ordinary.ts"]);
    let hook_calls = Arc::new(AtomicUsize::new(0));
    let mutate_engine = Arc::clone(&engine);
    let calls = Arc::clone(&hook_calls);
    let edges = [ParsedEdge::Relative {
        specifier: "./ordinary".to_string(),
        kind: ResolveRequestKind::EsmImport,
    }];

    let _ = take_input_resolution_budget_events_for_test();
    let result = resolution_test_hooks::with_repeating_hook(
        ResolutionPhase::ParsedEdgePreCommit,
        move || {
            let ordinal = calls.fetch_add(1, Ordering::AcqRel);
            let target = if ordinal % 2 == 0 {
                "/p/exact-a.ts"
            } else {
                "/p/exact-b.ts"
            };
            mutate_engine.set_exact_resolutions(IMPORTER, vec![exact("./forced", target)]);
        },
        || {
            engine.record_parsed_edges_with_exact_resolutions(
                &reader,
                IMPORTER,
                &edges,
                vec![exact("./ordinary", "/p/ordinary.ts")],
            )
        },
    );

    assert_eq!(hook_calls.load(Ordering::Acquire), 9);
    assert!(!result.changed);
    assert!(result.newly_resolved.is_empty());
    assert!(
        !engine
            .forward_deps_for(IMPORTER)
            .iter()
            .any(|dependency| dependency == "/p/ordinary.ts"),
        "the parsed-edge result preceding the rejected ninth restart must not be admitted"
    );
    let events = take_input_resolution_budget_events_for_test();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].meter, InputResolutionBudgetMeter::Churn);
    assert_eq!((events[0].consumed, events[0].prospective), (8, 9));
}

#[test]
fn explicit_project_churn_rejection_discards_the_superseded_result() {
    let budgets =
        InputResolutionBudgets::try_tightened(32, 128, 32_768, 16, 1).expect("test policy");
    let engine = Arc::new(Engine::new_with_input_resolution_budgets(budgets));
    publish_alias(&engine, "/p", "/p/old");
    let reader = ConcurrentReader::new(&["/p/old.ts", "/p/new.ts"]);
    let owner = ProjectOwnership {
        project_root: "/p".to_string(),
        tsconfig_path: Some("/p/tsconfig.json".to_string()),
    };
    let mutations = Arc::new(AtomicUsize::new(0));
    let mutate_engine = Arc::clone(&engine);
    let mutate_count = Arc::clone(&mutations);

    let refused = resolution_test_hooks::with_repeating_hook(
        ResolutionPhase::PreAdmissionValidation,
        move || {
            let ordinal = mutate_count.fetch_add(1, Ordering::AcqRel);
            let target = if ordinal % 2 == 0 { "/p/new" } else { "/p/old" };
            publish_alias(&mutate_engine, "/p", target);
        },
        || engine.resolve_import_for_project_outcome(&reader, &owner, "@dep", CONTEXT),
    );

    assert_eq!(mutations.load(Ordering::Acquire), 2);
    assert_eq!(
        refused.non_admission_reason(),
        Some(verter_audit::NonAdmissionReason::BudgetExceeded)
    );
    assert!(
        refused.result().is_none(),
        "the explicit-project facade must not expose its superseded attempt output"
    );

    // Mutation control: passing `result` through either explicit-path churn
    // rejection makes the final assertion expose the old alias target.
}
