#![doc = include_str!("../../../docs/contributing/path-precise-resolution-currency.md")]

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;

use super::Engine;
use crate::changes::WorkspaceChange;
use crate::resolution_currency::{
    CanonicalResolutionId, ResolutionEpoch, ResolutionFactKey, ResolutionFactVersion,
};
use crate::traits::{WorkspaceAccess, WorkspaceRead};
use crate::types::{ExactResolution, ExactResolutionResult, ParsedEdge, VfsProvenanceSnapshot};
use verter_semantic::resolver_core::{
    normalize_canonical_id, AttemptFailure, IdeProjectConfig, PathProbe as ProbeOutcome,
    ResolutionContext, ResolutionPopulation, ResolvePhase, ResolveRequestKind, ResolveResult,
    SessionFingerprint, WorkspaceAlias,
};

const CONTEXT: ResolutionContext = ResolutionContext {
    phase: ResolvePhase::ProviderGraph,
    kind: ResolveRequestKind::EsmImport,
};

struct ContractReader {
    files: RwLock<HashMap<String, Arc<str>>>,
    realpaths: RwLock<HashMap<String, String>>,
    observed_probe_outcomes: RwLock<Vec<ProbeOutcome>>,
    directory_enumerated_by_probe: Option<String>,
    pending_directory_observations: RwLock<Vec<String>>,
    forced_probe_outcome: Option<ProbeOutcome>,
    bridge_checks_before_refusal: Option<usize>,
    bridge_checks: AtomicUsize,
}

impl ContractReader {
    fn new() -> Self {
        Self {
            files: RwLock::new(HashMap::new()),
            realpaths: RwLock::new(HashMap::new()),
            observed_probe_outcomes: RwLock::new(Vec::new()),
            directory_enumerated_by_probe: None,
            pending_directory_observations: RwLock::new(Vec::new()),
            forced_probe_outcome: None,
            bridge_checks_before_refusal: None,
            bridge_checks: AtomicUsize::new(0),
        }
    }

    fn with_probe_outcome(outcome: ProbeOutcome) -> Self {
        Self {
            forced_probe_outcome: Some(outcome),
            ..Self::new()
        }
    }

    fn with_directory_enumeration(directory: &str) -> Self {
        Self {
            directory_enumerated_by_probe: Some(normalize_canonical_id(directory)),
            ..Self::new()
        }
    }

    fn with_midflight_bridge_loss() -> Self {
        Self {
            bridge_checks_before_refusal: Some(1),
            ..Self::new()
        }
    }

    fn insert(&self, path: &str, source: &str) {
        self.files
            .write()
            .insert(normalize_canonical_id(path), Arc::from(source));
    }

    fn remove(&self, path: &str) {
        self.files.write().remove(&normalize_canonical_id(path));
    }

    fn set_realpath(&self, requested: &str, resolved: &str) {
        self.realpaths.write().insert(
            normalize_canonical_id(requested),
            normalize_canonical_id(resolved),
        );
    }

    fn observed_probe_count(&self, expected: ProbeOutcome) -> usize {
        self.observed_probe_outcomes
            .read()
            .iter()
            .filter(|outcome| **outcome == expected)
            .count()
    }

    fn observe_probe_path(&self, canonical_id: &str) -> ProbeOutcome {
        if let Some(directory) = self.directory_enumerated_by_probe.as_ref() {
            self.pending_directory_observations
                .write()
                .push(directory.clone());
        }
        let outcome = self.forced_probe_outcome.unwrap_or_else(|| {
            if self
                .files
                .read()
                .contains_key(&normalize_canonical_id(canonical_id))
            {
                ProbeOutcome::File
            } else {
                ProbeOutcome::Absent
            }
        });
        self.observed_probe_outcomes.write().push(outcome);
        outcome
    }
}

impl WorkspaceRead for ContractReader {
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
        matches!(self.observe_probe_path(canonical_id), ProbeOutcome::File)
    }

    fn probe_path(&self, canonical_id: &str) -> ProbeOutcome {
        self.observe_probe_path(canonical_id)
    }

    fn resolution_event_bridge_complete(&self) -> bool {
        match self.bridge_checks_before_refusal {
            Some(complete_checks) => {
                self.bridge_checks.fetch_add(1, Ordering::AcqRel) < complete_checks
            }
            None => true,
        }
    }

    fn take_resolution_directory_observations(&self) -> Vec<String> {
        std::mem::take(&mut *self.pending_directory_observations.write())
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        let normalized = normalize_canonical_id(canonical_id);
        self.realpaths.read().get(&normalized).cloned().or_else(|| {
            self.files
                .read()
                .contains_key(&normalized)
                .then_some(normalized)
        })
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

impl WorkspaceAccess for ContractReader {
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

fn resolve(
    engine: &Engine,
    reader: &ContractReader,
    importer: &str,
    specifier: &str,
) -> Option<ResolveResult> {
    engine.resolve_import(reader, importer, specifier, CONTEXT)
}

fn provenance(engine: &Engine) -> VfsProvenanceSnapshot {
    engine.vfs_provenance.snapshot()
}

fn engine_with_fallback_project(root: &str) -> Engine {
    let engine = Engine::new();
    *engine.project_graph.write() = crate::project_graph::ProjectGraph::from_configs(vec![
        crate::project_graph::VfsProjectConfig {
            root: root.to_string(),
            rank: crate::project_graph::ProjectRank::Inferred,
            tsconfig_path: None,
            root_files: Vec::new(),
            extensions: vec![".ts".to_string()],
            workspace_root: root.to_string(),
            workspace_aliases: Vec::new(),
            compiler_options: verter_semantic::resolver_core::IdeProjectCompilerOptions::default(),
            references: Vec::new(),
            membership: crate::membership::configured_membership_match_all_under_root(
                &crate::CanonicalPath::new(root),
            ),
        },
    ]);
    engine.rebuild_and_publish();
    engine
}

fn assert_warm_hit(
    engine: &Engine,
    reader: &ContractReader,
    importer: &str,
    specifier: &str,
    expected_source: &str,
) {
    let before = provenance(engine);
    let result = resolve(engine, reader, importer, specifier)
        .unwrap_or_else(|| panic!("{importer} {specifier} must remain resolvable"));
    let after = provenance(engine);
    assert_eq!(result.source_id, expected_source);
    assert_eq!(
        after.import_resolution_cache_miss_count, before.import_resolution_cache_miss_count,
        "an unchanged witness must not rerun resolution"
    );
    assert_eq!(
        after.import_resolution_cache_hit_count,
        before.import_resolution_cache_hit_count + 1,
        "an unchanged witness must reuse its validating candidate"
    );
}

fn warm_positive(
    engine: &Engine,
    reader: &ContractReader,
    importer: &str,
    specifier: &str,
) -> String {
    resolve(engine, reader, importer, specifier)
        .unwrap_or_else(|| panic!("{importer} {specifier} must resolve"))
        .source_id
}

fn assert_recomputed_once(
    engine: &Engine,
    reader: &ContractReader,
    importer: &str,
    specifier: &str,
    expected_source: &str,
) {
    let before = provenance(engine);
    let result = resolve(engine, reader, importer, specifier)
        .unwrap_or_else(|| panic!("{importer} {specifier} must recompute"));
    let after = provenance(engine);
    assert_eq!(result.source_id, expected_source);
    assert_eq!(
        after.import_resolution_cache_miss_count,
        before.import_resolution_cache_miss_count + 1,
        "an affected witness must miss exactly once"
    );
    assert_eq!(
        after.import_resolution_cache_hit_count, before.import_resolution_cache_hit_count,
        "the invalidated candidate must not be reported as a warm hit"
    );
}

fn assert_typed_probe_return_only(expected: ProbeOutcome) {
    assert!(matches!(
        expected,
        ProbeOutcome::Inaccessible | ProbeOutcome::Unknown
    ));
    let engine = Engine::new();
    let reader = ContractReader::with_probe_outcome(expected);

    assert!(resolve(&engine, &reader, "/p/main.ts", "./dep").is_none());
    let probes_after_first = reader.observed_probe_count(expected);
    assert!(
        probes_after_first > 0,
        "the first request must actually observe typed {expected:?}"
    );
    let before_second = provenance(&engine);
    assert!(resolve(&engine, &reader, "/p/main.ts", "./dep").is_none());
    let after_second = provenance(&engine);
    assert!(
        reader.observed_probe_count(expected) > probes_after_first,
        "an observed {expected:?} must force typed non-admission, so a later \
         request probes again"
    );
    assert_eq!(
        after_second.import_resolution_cache_miss_count,
        before_second.import_resolution_cache_miss_count + 1
    );
    assert_eq!(
        after_second.import_resolution_cache_hit_count,
        before_second.import_resolution_cache_hit_count,
        "{expected:?} must never be admitted as a cacheable Absent"
    );
}

#[test]
fn exact_resolution_lookup_preserves_raw_specifier_identity() {
    let engine = Engine::new();
    let reader = ContractReader::new();
    reader.insert("/p/dep.ts", "export const value = 1");
    engine.set_exact_resolutions(
        "/p/main.ts",
        vec![ExactResolution {
            specifier: "./dep".to_string(),
            phase: CONTEXT.phase,
            kind: CONTEXT.kind,
            resolved_canonical_id: Some("/p/override.ts".to_string()),
            possible_canonical_ids: vec!["/p/override.ts".to_string()],
        }],
    );

    let result = engine
        .resolve_import_outcome(&reader, "/p/main.ts", "./dep/", CONTEXT)
        .into_transient_result()
        .expect("the raw-distinct request must fall through to ordinary resolution");

    assert_eq!(
        result.source_id, "/p/dep.ts",
        "exact-resolution lookup must not normalize two raw specifiers into one override key"
    );
}

#[test]
fn directory_probe_keeps_the_pre_typed_resolution_outcome_cacheable() {
    let engine = engine_with_fallback_project("/p");
    let reader = ContractReader::with_probe_outcome(ProbeOutcome::Directory);

    let outcome = engine.resolve_import_outcome(&reader, "/p/main.ts", "./dep", CONTEXT);

    assert_eq!(
        outcome.result().map(|result| result.source_id.as_str()),
        Some("/p/dep.ts"),
        "introducing typed probes must not turn a previously truthy exists probe into a miss"
    );
    assert!(
        outcome.is_cacheable(),
        "a known directory observation is stable evidence, not a ReturnOnly condition"
    );
}

#[test]
fn directory_enumeration_inside_probe_enters_the_transaction_signature() {
    let engine = engine_with_fallback_project("/p");
    let reader = ContractReader::with_directory_enumeration("/p");
    reader.insert("/p/dep.ts", "export const value = 1");

    let outcome = engine.resolve_import_outcome(&reader, "/p/main.ts", "./dep", CONTEXT);
    let crate::SignatureAdmission::Cacheable(_) = &outcome.admission else {
        panic!("a fully tracked resolver read must remain cacheable");
    };
    let directory_fact = ResolutionFactKey::DirectoryMembers {
        canonical: CanonicalResolutionId::new("/p"),
        population: ResolutionPopulation::Base,
    };
    let node = decision_node(&engine, "/p/main.ts", "./dep", ResolutionPopulation::Base)
        .expect("an admitted resolution publishes its decision node");
    let edges = engine
        .decision_direct_dependencies_for_test(ResolutionPopulation::Base, &node)
        .expect("a published decision carries its direct edge set");
    assert!(
        edges.contains(&directory_fact),
        "a directory enumeration performed inside a typed path probe must be visible to \
         TransactionReader and become a direct dependency edge of the decision — the \
         admitted witness names the decision, so this edge is what makes a member change \
         reach it"
    );

    // Mutation recipe: stop draining the reader's directory-observation
    // evidence after probe_path. The result remains correct, but this exact
    // DirectoryMembers edge disappears from the published decision.
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn filesystem_live_os_resolution_is_typed_return_only() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = normalize_canonical_id(
        &std::fs::canonicalize(temp.path())
            .expect("canonical tempdir")
            .to_string_lossy(),
    );
    let importer = std::path::Path::new(&root).join("main.ts");
    let dependency = std::path::Path::new(&root).join("dep.ts");
    std::fs::write(&dependency, "export const value = 1").expect("write dependency");
    let importer = normalize_canonical_id(&importer.to_string_lossy());
    let dependency = normalize_canonical_id(
        &std::fs::canonicalize(&dependency)
            .expect("canonical dependency")
            .to_string_lossy(),
    );
    let workspace =
        crate::filesystem::FilesystemWorkspace::new(crate::filesystem::FilesystemOptions {
            roots: vec![root.clone()],
            eager_preload: false,
        });
    WorkspaceAccess::configure_resolver(
        &workspace,
        vec![crate::resolver::ide_project_config(
            root.clone(),
            root,
            None,
        )],
    );

    let outcome = workspace
        .engine
        .resolve_import_outcome(&workspace, &importer, "./dep", CONTEXT);

    assert_eq!(
        outcome.result().map(|result| result.source_id.as_str()),
        Some(dependency.as_str()),
        "fail-closed admission must still return the correct live-OS result"
    );
    assert_eq!(
        outcome.non_admission_reason(),
        Some(verter_audit::NonAdmissionReason::ResolutionUntrackedBackend),
        "a backend that can observe unpublished live-OS state must never mint a cacheable witness"
    );
    assert_eq!(
        outcome
            .clone()
            .into_transient_result()
            .map(|result| result.source_id),
        Some(dependency),
        "a ReturnOnly target remains available to an explicitly transient consumer"
    );
    match outcome.into_publication() {
        crate::ResolutionPublication::Refused(refusal) => assert_eq!(
            refusal.reason(),
            verter_audit::NonAdmissionReason::ResolutionUntrackedBackend,
            "the sink refusal must retain the exact transaction admission reason"
        ),
        crate::ResolutionPublication::Admitted(_) => {
            panic!("the persistent-sink projection must reject every ReturnOnly target")
        }
    }

    // Mutation recipe: make FilesystemWorkspace claim a complete resolution
    // event bridge. The same live disk result becomes cacheable against an
    // immutable root that contains no publication for the disk observation.
    // Or make `into_publication` admit the stored result without
    // checking admission: the second assertion admits the unwitnessed target.
}

#[test]
fn bridge_loss_after_resolution_refuses_the_durable_publication() {
    let engine = engine_with_fallback_project("/p");
    let reader = ContractReader::with_midflight_bridge_loss();
    reader.insert("/p/dep.ts", "export const value = 1");

    let outcome = engine.resolve_import_outcome(&reader, "/p/main.ts", "./dep", CONTEXT);
    assert_eq!(
        outcome.result().map(|result| result.source_id.as_str()),
        Some("/p/dep.ts"),
        "the resolution itself completed before the bridge was lost"
    );
    match outcome.into_publication() {
        crate::ResolutionPublication::Refused(refusal) => assert_eq!(
            refusal.reason(),
            verter_audit::NonAdmissionReason::ResolutionUntrackedBackend,
            "the final bridge fence must refuse a world that became untracked mid-flight"
        ),
        crate::ResolutionPublication::Admitted(_) => {
            panic!("a mid-flight bridge loss must never reach a durable sink")
        }
    }
}

#[test]
fn missing_published_root_is_typed_incomplete_provenance() {
    let engine = Engine::new();
    let reader = ContractReader::new();
    reader.insert("/p/dep.ts", "export const value = 1");
    engine.mutate_resolution_world(|world| {
        world.published = None;
        ((), true)
    });

    let outcome = engine.resolve_import_outcome(&reader, "/p/main.ts", "./dep", CONTEXT);

    assert_eq!(
        outcome.non_admission_reason(),
        Some(verter_audit::NonAdmissionReason::ResolutionIncompleteProvenance),
        "a world with no published root cannot complete any context observation"
    );
    let population = reader.resolution_population();
    assert!(
        engine
            .cached_resolution_query_for_test("/p/main.ts", "./dep", CONTEXT, population)
            .is_none(),
        "a provenance-refused result must not enter the workspace resolution cache"
    );
    assert!(
        engine.dependency_snapshot("/p/main.ts").is_none(),
        "a provenance-refused result must not publish a lazy resolved edge"
    );

    // Mutation recipe: map NoPublishedRoot onto the unowned context instead
    // of mark_incomplete_provenance. The refusal disappears and an
    // unprovenanced candidate enters the cache.
}

#[test]
fn published_root_missing_project_tables_is_typed_incomplete_provenance() {
    let engine = engine_with_fallback_project("/p");
    let reader = ContractReader::new();
    reader.insert("/p/dep.ts", "export const value = 1");
    reader.insert("/outside/dep.ts", "export const value = 1");
    let snapshot = engine
        .load_published()
        .expect("configured engine publishes a snapshot")
        .snapshot
        .clone();
    engine.mutate_resolution_world(|world| {
        // Bypass publish_snapshot's table completion: the published root
        // carries NO project_identity_hashes / env_hashes_by_project rows.
        world.replace_published(
            Arc::new(crate::published_state::PublishedRoot::new_vfs_only(
                snapshot,
            )),
            &[],
            || engine.next_resolution_fact_version(),
        );
        ((), true)
    });

    let owned = engine.resolve_import_outcome(&reader, "/p/main.ts", "./dep", CONTEXT);
    assert_eq!(
        owned.result().map(|result| result.source_id.as_str()),
        Some("/p/dep.ts"),
        "the provenance refusal must not erase the useful transient result"
    );
    assert_eq!(
        owned.non_admission_reason(),
        Some(verter_audit::NonAdmissionReason::ResolutionIncompleteProvenance),
        "an owning project with no identity/environment projection is a genuine provenance gap"
    );
    let population = reader.resolution_population();
    assert!(
        engine
            .cached_resolution_query_for_test("/p/main.ts", "./dep", CONTEXT, population)
            .is_none(),
        "a provenance-refused result must not enter the workspace resolution cache"
    );
    assert!(
        engine.dependency_snapshot("/p/main.ts").is_none(),
        "a provenance-refused result must not publish a lazy resolved edge"
    );

    // The discriminating contrast: an importer NO configured project owns
    // reads the same incomplete-table root, but its selection is a complete
    // observation ("none owns this entry") — the stable unowned context —
    // and stays cacheable.
    let unowned = engine.resolve_import_outcome(&reader, "/outside/main.ts", "./dep", CONTEXT);
    assert_eq!(
        unowned.result().map(|result| result.source_id.as_str()),
        Some("/outside/dep.ts")
    );
    assert!(
        unowned.is_cacheable(),
        "no owning project is a complete observation of the published index, not a provenance gap"
    );

    // Mutation recipe: collapse the four ContextProvenanceError variants
    // into the unowned selection. The owned importer then admits a witness
    // whose project identity/environment provenance was never observed.
}

#[test]
fn root_project_supplies_real_context_to_top_level_package_importer() {
    let engine = engine_with_fallback_project("/");
    let reader = ContractReader::new();
    reader.insert(
        "/node_modules/pkg/package.json",
        r#"{"name":"pkg","types":"./index.d.ts"}"#,
    );
    reader.insert(
        "/node_modules/pkg/index.ts",
        "export { value } from './inner'",
    );
    reader.insert("/node_modules/pkg/inner.ts", "export const value = 1");

    assert_eq!(
        warm_positive(&engine, &reader, "/node_modules/pkg/index.ts", "./inner"),
        "/node_modules/pkg/inner.ts"
    );
    assert_warm_hit(
        &engine,
        &reader,
        "/node_modules/pkg/index.ts",
        "./inner",
        "/node_modules/pkg/inner.ts",
    );
}

#[cfg(unix)]
#[test]
fn overlay_open_advances_realpath_when_the_path_kind_stays_file() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("tempdir");
    let physical = temp.path().join("physical.ts");
    let linked = temp.path().join("linked.ts");
    std::fs::write(&physical, "export const disk = 1").expect("write physical target");
    symlink(&physical, &linked).expect("create symlink");

    let linked = normalize_canonical_id(&linked.to_string_lossy());
    let workspace =
        crate::filesystem::FilesystemWorkspace::new(crate::filesystem::FilesystemOptions {
            roots: vec![normalize_canonical_id(&temp.path().to_string_lossy())],
            eager_preload: false,
        });
    let disk_realpath =
        WorkspaceRead::realpath(&workspace, &linked).expect("disk symlink resolves");
    assert_ne!(
        disk_realpath, linked,
        "precondition: disk and overlay realpath meanings must differ"
    );

    workspace.engine.mutate_content_for(
        &linked,
        false,
        Some(ProbeOutcome::File),
        super::BaseRealpathTransition::Unknown,
        || ((), true),
    );
    let population = WorkspaceRead::resolution_population(&workspace);
    let realpath_key = ResolutionFactKey::Realpath {
        requested: CanonicalResolutionId::new(linked.clone()),
        population,
    };
    let path_key = ResolutionFactKey::PathProbe {
        canonical: CanonicalResolutionId::new(linked.clone()),
        population,
    };
    let realpath_before = workspace
        .engine
        .resolution_fact_version_for_test(population, &realpath_key);
    let path_before = workspace
        .engine
        .resolution_fact_version_for_test(population, &path_key);

    WorkspaceAccess::notify_upsert(&workspace, &linked, Arc::from("export const overlay = 1"));

    assert_eq!(
        WorkspaceRead::realpath(&workspace, &linked).as_deref(),
        Some(linked.as_str()),
        "an overlay resolves to its canonical overlay path, not the disk symlink target"
    );
    assert_ne!(
        workspace
            .engine
            .resolution_fact_version_for_test(population, &realpath_key),
        realpath_before,
        "opening an overlay must invalidate the old disk realpath witness even when both probes are File"
    );
    assert_eq!(
        workspace
            .engine
            .resolution_fact_version_for_test(population, &path_key),
        path_before,
        "the independent PathProbe fact must stay stable because File remained File"
    );
    let overlay_realpath_version = workspace
        .engine
        .resolution_fact_version_for_test(population, &realpath_key);

    WorkspaceAccess::notify_close(&workspace, &linked);

    assert_eq!(
        WorkspaceRead::realpath(&workspace, &linked).as_deref(),
        Some(disk_realpath.as_str()),
        "closing the overlay must reveal the disk symlink target"
    );
    let base_realpath_key = realpath_key.in_population(ResolutionPopulation::Base);
    let revealed_realpath_version = workspace
        .engine
        .resolution_fact_version_for_test(population, &realpath_key);
    assert_eq!(
        revealed_realpath_version,
        workspace
            .engine
            .resolution_fact_version_for_test(ResolutionPopulation::Base, &base_realpath_key),
        "closing the overlay must reveal the current base realpath fact"
    );
    assert_ne!(
        revealed_realpath_version, overlay_realpath_version,
        "overlay close must not retain the session realpath witness"
    );

    // Mutation recipe: seed every session path fact from the base whenever
    // PathProbe stays File. Realpath then reuses the disk version even though
    // overlay lookup changed the resolved target to the canonical overlay path.
}

#[test]
fn resolution_outcome_uses_the_shared_resolve_imports_signature_rail() {
    let engine = engine_with_fallback_project("/p");
    let reader = ContractReader::new();
    reader.insert("/p/dep.ts", "export const value = 1");

    let outcome = engine.resolve_import_outcome(&reader, "/p/main.ts", "./dep", CONTEXT);
    let crate::SignatureAdmission::Cacheable(signature) = &outcome.admission else {
        panic!("a fully tracked resolution must produce the shared cacheable admission");
    };
    assert!(!signature.facts.is_empty());
    assert!(signature.facts.iter().all(|fact| matches!(
        fact,
        crate::FactVersionRef::ResolveImports(crate::ResolveImportsFactRef::Resolution(_))
    )));

    // Mutation recipe: reintroduce a resolution-only signature/admission carrier
    // in Engine and store it directly in the lazy cache. This assertion then
    // fails because the cacheable product no longer consists solely of the
    // shared ResolveImports fact variant.
}

#[test]
fn every_closed_resolution_fact_family_has_a_live_mutation_rail() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    let engine = &workspace.engine;
    let base = ResolutionPopulation::Base;
    let package = "/p/package.json";
    let path_key = ResolutionFactKey::PathProbe {
        canonical: CanonicalResolutionId::new(package),
        population: base,
    };
    let manifest_key = ResolutionFactKey::Manifest {
        canonical: CanonicalResolutionId::new(package),
        population: base,
    };
    let realpath_key = ResolutionFactKey::Realpath {
        requested: CanonicalResolutionId::new(package),
        population: base,
    };
    let directory_key = ResolutionFactKey::DirectoryMembers {
        canonical: CanonicalResolutionId::new("/p"),
        population: base,
    };
    let recovery_key = ResolutionFactKey::RecoveryScope {
        canonical_prefix: CanonicalResolutionId::new("/p"),
        population: base,
    };
    let exact_key = ResolutionFactKey::exact_importer("/p/main.ts", "./dep", CONTEXT, base);
    let context_key = ResolutionFactKey::context_importer("/p/main.ts", base);
    let before_context = engine.resolution_fact_version_for_test(base, &context_key);

    workspace.inject_file(
        package.to_string(),
        Arc::from(r#"{"name":"p","types":"old.d.ts"}"#),
    );
    for (family, key) in [
        ("PathProbe", &path_key),
        ("Manifest", &manifest_key),
        ("Realpath", &realpath_key),
        ("DirectoryMembers", &directory_key),
    ] {
        assert_ne!(
            engine.resolution_fact_version_for_test(base, key),
            ResolutionFactVersion::INITIAL,
            "{family} must advance at its owning mutation chokepoint"
        );
    }
    assert_eq!(
        engine.resolution_fact_version_for_test(base, &recovery_key),
        ResolutionFactVersion::INITIAL,
        "a precise per-canonical appearance advances NO recovery scope; witnesses observe \
         recovery scopes, only imprecise watcher mutations advance them"
    );
    workspace.apply_changes(vec![WorkspaceChange::DirectoryTreeDirty {
        prefix: "/p".to_string(),
    }]);
    assert_ne!(
        engine.resolution_fact_version_for_test(base, &recovery_key),
        ResolutionFactVersion::INITIAL,
        "RecoveryScope must advance at its imprecise watcher-recovery chokepoint"
    );

    engine.set_exact_resolutions(
        "/p/main.ts",
        vec![ExactResolution {
            specifier: "./dep".to_string(),
            phase: CONTEXT.phase,
            kind: CONTEXT.kind,
            resolved_canonical_id: Some("/p/dep.ts".to_string()),
            possible_canonical_ids: vec!["/p/dep.ts".to_string()],
        }],
    );
    assert_ne!(
        engine.resolution_fact_version_for_test(base, &exact_key),
        ResolutionFactVersion::INITIAL
    );

    let project = crate::resolver::ide_project_config(
        "/p".to_string(),
        "/".to_string(),
        Some("/p/tsconfig.json".to_string()),
    );
    WorkspaceAccess::configure_resolver(&workspace, vec![project]);
    assert_ne!(
        engine.resolution_fact_version_for_test(base, &context_key),
        before_context,
        "ContextSelection must be a computed fact over the published resolver index"
    );

    // The two DERIVED families. A decision is minted by the resolve
    // fence's publication; an owner set by its single owner-surface
    // publisher. Neither is advanced by an observation.
    let resolve_population = WorkspaceRead::resolution_population(&workspace);
    let decision_before = engine
        .cached_resolution_query_for_test("/p/main.ts", "./dep", CONTEXT, resolve_population)
        .map(ResolutionFactKey::decision);
    assert!(
        decision_before.is_none(),
        "fixture invariant: no decision exists for this demand before it is resolved"
    );
    let outcome = engine.resolve_import_outcome_with_evidence(
        &workspace,
        crate::resolution_currency::ResolutionEvidenceSource::ReaderAuthoritative,
        "/p/main.ts",
        "./dep",
        CONTEXT,
    );
    assert!(matches!(
        outcome.admission,
        crate::SignatureAdmission::Cacheable(_)
    ));
    let decision_key = engine
        .cached_resolution_query_for_test("/p/main.ts", "./dep", CONTEXT, resolve_population)
        .map(ResolutionFactKey::decision)
        .expect("Decision must be published at its owning publication chokepoint");
    assert!(
        engine
            .decision_direct_dependencies_for_test(resolve_population, &decision_key)
            .is_some(),
        "Decision must acquire its direct edge set at its owning publication chokepoint — \
         the edges, not a minted version, are what a resolution publishes"
    );
    assert!(
        engine.remove_derived_node_for_test(resolve_population, &decision_key),
        "and its owning REMOVAL chokepoint must advance it"
    );
    assert_ne!(
        engine.resolution_fact_version_for_test(resolve_population, &decision_key),
        ResolutionFactVersion::INITIAL,
        "Decision must advance at its owning removal chokepoint"
    );

    // Exhaustiveness is a COMPILE rail, not a list: a new
    // `ResolutionFactKey` variant cannot be added without stating which
    // chokepoint above owns it.
    let inventory = [
        path_key.clone(),
        manifest_key.clone(),
        realpath_key.clone(),
        directory_key.clone(),
        recovery_key.clone(),
        exact_key.clone(),
        context_key.clone(),
        decision_key.clone(),
    ];
    for key in &inventory {
        match key {
            ResolutionFactKey::PathProbe { .. }
            | ResolutionFactKey::Manifest { .. }
            | ResolutionFactKey::Realpath { .. }
            | ResolutionFactKey::DirectoryMembers { .. }
            | ResolutionFactKey::RecoveryScope { .. }
            | ResolutionFactKey::ExactResolution { .. }
            | ResolutionFactKey::ContextSelection { .. }
            | ResolutionFactKey::Decision { .. } => {}
            // The owner-set family's publication chokepoint is asserted
            // by the owner-surface suite, which owns its single publisher.
            ResolutionFactKey::OwnerResolutionSet { .. } => {}
        }
    }
    assert_eq!(
        inventory.iter().collect::<BTreeSet<_>>().len(),
        inventory.len(),
        "the inventory must name each family once"
    );

    // Mutation recipe: delete any one call to update_base_path_facts,
    // update_base_manifest_fact, replace_world_exact_resolutions, the
    // DirectoryTreeDirty RecoveryScope advance, the computed
    // ContextSelection validator, or the resolve fence's
    // `publish_resolution_decision`. The matching family remains at its
    // old version and this inventory fails. Re-adding RecoveryScope advances to
    // the precise per-path chokepoint flips the negative assertion above.
}

#[test]
fn session_overlay_root_is_independent_and_shields_hidden_base_mutations() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    workspace.inject_file("/p/dep.ts".to_string(), Arc::from("export const base = 1"));
    let population = WorkspaceRead::resolution_population(&workspace);
    let ResolutionPopulation::Session(session) = population else {
        panic!("engine-backed editor workspaces must resolve through a session population");
    };
    let other_session = SessionFingerprint::from_raw(0x5E5510);
    let engine = &workspace.engine;
    let base_key = ResolutionFactKey::PathProbe {
        canonical: CanonicalResolutionId::new("/p/dep.ts"),
        population: ResolutionPopulation::Base,
    };
    let session_key = base_key.in_population(population);
    let base_epoch = engine.resolution_epoch_for_test(ResolutionPopulation::Base);
    let other_epoch =
        engine.resolution_epoch_for_test(ResolutionPopulation::Session(other_session));
    let visible_version =
        engine.resolution_fact_version_for_test(ResolutionPopulation::Base, &base_key);

    WorkspaceAccess::notify_upsert(
        &workspace,
        "/p/dep.ts",
        Arc::from("export const overlay = 1"),
    );
    assert_eq!(
        engine.resolution_epoch_for_test(ResolutionPopulation::Base),
        base_epoch,
        "a session-only edit must not advance the base epoch"
    );
    assert_ne!(
        engine.resolution_epoch_for_test(population),
        ResolutionEpoch::from_raw(0),
        "the owning session epoch must advance"
    );
    assert_eq!(
        engine.resolution_epoch_for_test(ResolutionPopulation::Session(other_session)),
        other_epoch,
        "one session edit must not advance another session domain"
    );
    assert_eq!(
        engine.resolution_fact_version_for_test(population, &session_key),
        visible_version,
        "opening an overlay over an existing file preserves the effective path fact"
    );

    workspace.remove_file("/p/dep.ts");
    assert_ne!(
        engine.resolution_fact_version_for_test(ResolutionPopulation::Base, &base_key),
        visible_version
    );
    assert_eq!(
        engine.resolution_fact_version_for_test(population, &session_key),
        visible_version,
        "the session root must shield a base mutation hidden by its overlay"
    );

    WorkspaceAccess::notify_close(&workspace, "/p/dep.ts");
    assert_eq!(
        engine.resolution_fact_version_for_test(population, &session_key),
        engine.resolution_fact_version_for_test(ResolutionPopulation::Base, &base_key),
        "closing the overlay must reveal the current base fact"
    );

    // Mutation recipe: store session facts in the base root, or omit the
    // fallback-version freeze on overlay open. The base/other epochs advance
    // or the hidden deletion leaks through before close.
    let _ = session;
}

#[test]
fn non_cacheable_parsed_resolution_never_enters_persistent_edges() {
    let engine = Engine::new();
    let reader = ContractReader::with_probe_outcome(ProbeOutcome::Unknown);
    engine.record_parsed_edges(
        &reader,
        "/p/main.ts",
        &[
            ParsedEdge::Relative {
                specifier: "./dep".to_string(),
                kind: ResolveRequestKind::EsmImport,
            },
            ParsedEdge::ExternalSrc {
                specifier: "./external.vue".to_string(),
                resolved_path: Some("/p/external.vue".to_string()),
            },
        ],
    );

    assert!(
        engine.forward_deps_for("/p/main.ts").is_empty(),
        "ReturnOnly parsed outcomes must not publish resolved or unresolved edge state"
    );

    // Mutation recipe: ignore ResolutionTransaction::finish() in
    // resolve_parsed_edge_in_world and persist its result/miss unconditionally.
    // The owner acquires a durable edge from an Unknown observation.
}

#[test]
fn later_parsed_edge_refusal_preserves_the_entire_previously_admitted_batch() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    workspace.inject_file("/p/main.ts".to_string(), Arc::from("import './dep';"));
    workspace.inject_file("/p/dep.ts".to_string(), Arc::from("export const dep = 1;"));
    WorkspaceAccess::configure_resolver(
        &workspace,
        vec![crate::resolver::ide_project_config(
            "/p".to_string(),
            "/".to_string(),
            Some("/p/tsconfig.json".to_string()),
        )],
    );
    let engine = &workspace.engine;
    engine.record_parsed_edges(
        &workspace,
        "/p/main.ts",
        &[ParsedEdge::Relative {
            specifier: "./dep".to_string(),
            kind: ResolveRequestKind::EsmImport,
        }],
    );
    assert_eq!(
        engine.forward_deps_for("/p/main.ts"),
        vec!["/p/dep.ts".to_string()],
        "precondition: the first complete batch is durable"
    );

    let refusing = ContractReader::with_probe_outcome(ProbeOutcome::Unknown);
    engine.record_parsed_edges(
        &refusing,
        "/p/main.ts",
        &[
            ParsedEdge::Bare {
                specifier: "pkg".to_string(),
                kind: ResolveRequestKind::EsmImport,
            },
            ParsedEdge::Relative {
                specifier: "./missing".to_string(),
                kind: ResolveRequestKind::EsmImport,
            },
        ],
    );

    assert_eq!(
        engine.forward_deps_for("/p/main.ts"),
        vec!["/p/dep.ts".to_string()],
        "a later constituent refusal must publish none of its batch and must not \
         erase the previously admitted graph"
    );
}

#[test]
fn query_context_records_target_provider_projection_identity() {
    fn projects(provider_root: &str) -> Vec<IdeProjectConfig> {
        let mut importer = crate::resolver::ide_project_config(
            "/app".to_string(),
            "/".to_string(),
            Some("/app/tsconfig.json".to_string()),
        );
        importer.workspace_aliases = vec![WorkspaceAlias {
            find: "@lib".to_string(),
            replacement: "/lib/dep".to_string(),
        }];
        let mut target = crate::resolver::ide_project_config(
            "/lib".to_string(),
            "/".to_string(),
            Some("/lib/tsconfig.json".to_string()),
        );
        target.provider_root = provider_root.to_string();
        vec![importer, target]
    }

    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    workspace.inject_file("/lib/dep.ts".to_string(), Arc::from("export const dep = 1"));
    WorkspaceAccess::configure_resolver(&workspace, projects("/provider/v1"));
    assert!(WorkspaceRead::resolve_import(&workspace, "/app/main.ts", "@lib", CONTEXT).is_some());
    let population = WorkspaceRead::resolution_population(&workspace);
    let first = workspace
        .engine
        .cached_resolution_query_for_test("/app/main.ts", "@lib", CONTEXT, population)
        .expect("cold cache publication must retain its complete query")
        .context()
        .identity_parts();

    WorkspaceAccess::configure_resolver(&workspace, projects("/provider/v2"));
    assert!(WorkspaceRead::resolve_import(&workspace, "/app/main.ts", "@lib", CONTEXT).is_some());
    let second = workspace
        .engine
        .cached_resolution_query_for_test("/app/main.ts", "@lib", CONTEXT, population)
        .expect("recomputed candidate must retain its complete query")
        .context()
        .identity_parts();

    assert_eq!(
        first.0, second.0,
        "the importer project identity did not change"
    );
    assert_eq!(
        first.1, second.1,
        "the importer resolver policy did not change"
    );
    assert_ne!(
        first.2, second.2,
        "the query must carry the target project's provider projection identity"
    );

    // Mutation recipe: derive ProviderPolicyIdentity from the importer project
    // or set the query before result projection. The third identity component
    // remains unchanged across the target-only provider mutation.
}

#[test]
fn publish_snapshot_completes_missing_project_context_projection() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    workspace.inject_file("/p/dep.ts".to_string(), Arc::from("export const dep = 1"));
    let project = crate::resolver::ide_project_config(
        "/p".to_string(),
        "/".to_string(),
        Some("/p/tsconfig.json".to_string()),
    );
    WorkspaceAccess::configure_resolver(&workspace, vec![project]);
    let snapshot = workspace
        .load_published()
        .expect("configured workspace publishes a snapshot")
        .snapshot
        .clone();
    workspace
        .engine
        .publish_snapshot(crate::published_state::PublishedRoot::new_vfs_only(
            snapshot,
        ));

    let outcome =
        workspace
            .engine
            .resolve_import_outcome(&workspace, "/p/main.ts", "./dep", CONTEXT);
    assert_eq!(
        outcome.result().map(|result| result.source_id.as_str()),
        Some("/p/dep.ts"),
        "ReturnOnly still returns the complete computed value"
    );
    assert!(
        outcome.is_cacheable(),
        "the publication authority must compose the project identity and environment \
         tables before exposing an externally supplied snapshot"
    );

    // Mutation recipe: publish the VFS-only root verbatim. The resolver still
    // returns the target, but the transaction refuses it because the selected
    // project has no identity/environment projection.
}

#[test]
fn query_context_uses_the_resolvers_actual_overlap_selection_policy() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    workspace.inject_file("/p/dep.ts".to_string(), Arc::from("export const dep = 1"));

    let mut solution = crate::resolver::ide_project_config(
        "/p".to_string(),
        "/".to_string(),
        Some("/p/tsconfig.json".to_string()),
    );
    solution.references = vec!["/p/z.json".to_string()];
    let leaf = crate::resolver::ide_project_config(
        "/p".to_string(),
        "/".to_string(),
        Some("/p/z.json".to_string()),
    );
    WorkspaceAccess::configure_resolver(&workspace, vec![solution, leaf]);

    let published = workspace.load_published().expect("published project graph");
    let nearest = published
        .snapshot
        .resolver
        .nearest_config_for_path("/p/main.ts")
        .expect("resolver must select a project");
    assert_eq!(nearest.tsconfig_path.as_deref(), Some("/p/tsconfig.json"));
    let nearest_project = published
        .snapshot
        .projects
        .iter()
        .find(|project| {
            matches!(
                &project.payload,
                crate::workspace_snapshot::ProjectPayload::Configured {
                    tsconfig_path,
                    ..
                } if tsconfig_path.as_str() == "/p/tsconfig.json"
            )
        })
        .expect("nearest resolver project has a snapshot projection");
    let nearest_identity = published.project_identity_hashes[&nearest_project.id];

    assert!(WorkspaceRead::resolve_import(&workspace, "/p/main.ts", "./dep", CONTEXT).is_some());
    let population = WorkspaceRead::resolution_population(&workspace);
    let query_identity = workspace
        .engine
        .cached_resolution_query_for_test("/p/main.ts", "./dep", CONTEXT, population)
        .expect("cacheable request stores its full query")
        .context()
        .identity_parts()
        .0;
    assert_eq!(
        query_identity, nearest_identity,
        "query provenance must use ModuleResolverCore::nearest_config_for_path, not the provider-default owner policy"
    );

    // Mutation recipe: replace selected_context_for_path with
    // WorkspaceSnapshot::default_configured_owner_for_file. This overlap
    // topology selects through a different policy and the query identity
    // diverges from the resolver that computed the result.
}

#[test]
fn resolution_currency_content_overwrite_preserves_unchanged_path_witness() {
    let engine = Engine::new();
    let reader = ContractReader::new();
    reader.insert("/p/dep.ts", "export const v = 1");
    assert_eq!(
        warm_positive(&engine, &reader, "/p/main.ts", "./dep"),
        "/p/dep.ts"
    );

    reader.insert("/p/dep.ts", "export const v = 2");
    engine.bump_content_generation_for("/p/dep.ts");

    assert_warm_hit(&engine, &reader, "/p/main.ts", "./dep", "/p/dep.ts");
}

#[test]
fn resolution_currency_relevant_path_appearance_invalidates_only_the_recorded_miss() {
    let engine = Engine::new();
    let reader = ContractReader::new();
    reader.insert("/p/stable.ts", "export const stable = 1");
    assert_eq!(
        warm_positive(&engine, &reader, "/p/stable-main.ts", "./stable"),
        "/p/stable.ts"
    );
    assert!(resolve(&engine, &reader, "/p/main.ts", "./appears").is_none());

    reader.insert("/p/appears.ts", "export const appears = 1");
    engine.bump_content_generation_for("/p/appears.ts");

    assert_eq!(
        warm_positive(&engine, &reader, "/p/main.ts", "./appears"),
        "/p/appears.ts"
    );
    assert_warm_hit(
        &engine,
        &reader,
        "/p/stable-main.ts",
        "./stable",
        "/p/stable.ts",
    );
}

#[test]
fn resolution_currency_irrelevant_path_appearance_keeps_positive_warm() {
    let engine = Engine::new();
    let reader = ContractReader::new();
    reader.insert("/p/dep.ts", "export const v = 1");
    warm_positive(&engine, &reader, "/p/main.ts", "./dep");

    reader.insert("/elsewhere/new.ts", "export const unrelated = 1");
    engine.bump_content_generation_for("/elsewhere/new.ts");

    assert_warm_hit(&engine, &reader, "/p/main.ts", "./dep", "/p/dep.ts");
}

#[test]
fn resolution_currency_higher_priority_positive_retarget_keeps_precedence_guards() {
    let engine = Engine::new();
    let reader = ContractReader::new();
    reader.insert("/p/mod.tsx", "export const lower = 1");
    reader.insert("/p/stable.ts", "export const stable = 1");
    assert_eq!(
        warm_positive(&engine, &reader, "/p/main.ts", "./mod.js"),
        "/p/mod.tsx"
    );
    warm_positive(&engine, &reader, "/p/stable-main.ts", "./stable");

    reader.insert("/p/mod.ts", "export const higher = 1");
    engine.bump_content_generation_for("/p/mod.ts");

    assert_eq!(
        warm_positive(&engine, &reader, "/p/main.ts", "./mod.js"),
        "/p/mod.ts"
    );
    assert_warm_hit(
        &engine,
        &reader,
        "/p/stable-main.ts",
        "./stable",
        "/p/stable.ts",
    );
}

#[test]
fn resolution_currency_deletion_falls_back_without_importer_fanout() {
    let engine = Engine::new();
    let reader = ContractReader::new();
    reader.insert("/p/mod.ts", "export const higher = 1");
    reader.insert("/p/mod.tsx", "export const lower = 1");
    reader.insert("/p/stable.ts", "export const stable = 1");
    assert_eq!(
        warm_positive(&engine, &reader, "/p/main.ts", "./mod.js"),
        "/p/mod.ts"
    );
    warm_positive(&engine, &reader, "/p/stable-main.ts", "./stable");

    reader.remove("/p/mod.ts");
    engine.bump_content_generation_for("/p/mod.ts");

    assert_eq!(
        warm_positive(&engine, &reader, "/p/main.ts", "./mod.js"),
        "/p/mod.tsx"
    );
    assert_warm_hit(
        &engine,
        &reader,
        "/p/stable-main.ts",
        "./stable",
        "/p/stable.ts",
    );
}

#[test]
fn resolution_currency_manifest_semantic_change_retargets_only_consulting_queries() {
    let engine = Engine::new();
    let reader = ContractReader::new();
    reader.insert(
        "/repo/node_modules/pkg/package.json",
        r#"{"name":"pkg","types":"old.d.ts"}"#,
    );
    reader.insert("/repo/node_modules/pkg/old.d.ts", "export interface Old {}");
    reader.insert("/repo/node_modules/pkg/new.d.ts", "export interface New {}");
    reader.insert("/repo/src/stable.ts", "export const stable = 1");
    assert_eq!(
        warm_positive(&engine, &reader, "/repo/src/main.ts", "pkg"),
        "/repo/node_modules/pkg/old.d.ts"
    );
    warm_positive(&engine, &reader, "/repo/src/stable-main.ts", "./stable");

    reader.insert(
        "/repo/node_modules/pkg/package.json",
        r#"{"name":"pkg","types":"new.d.ts"}"#,
    );
    engine.bump_content_generation_for("/repo/node_modules/pkg/package.json");

    assert_eq!(
        warm_positive(&engine, &reader, "/repo/src/main.ts", "pkg"),
        "/repo/node_modules/pkg/new.d.ts"
    );
    assert_warm_hit(
        &engine,
        &reader,
        "/repo/src/stable-main.ts",
        "./stable",
        "/repo/src/stable.ts",
    );
}

#[test]
fn resolution_currency_realpath_symlink_change_retargets_requested_and_resolved_chains() {
    let engine = Engine::new();
    let reader = ContractReader::new();
    reader.insert("/p/link.ts", "export const linked = 1");
    reader.insert("/p/stable.ts", "export const stable = 1");
    reader.set_realpath("/p/link.ts", "/store/v1/link.ts");
    assert_eq!(
        warm_positive(&engine, &reader, "/p/main.ts", "./link"),
        "/store/v1/link.ts"
    );
    warm_positive(&engine, &reader, "/p/stable-main.ts", "./stable");

    reader.set_realpath("/p/link.ts", "/store/v2/link.ts");
    engine.bump_content_generation_for("/p/link.ts");

    assert_eq!(
        warm_positive(&engine, &reader, "/p/main.ts", "./link"),
        "/store/v2/link.ts"
    );
    assert_warm_hit(
        &engine,
        &reader,
        "/p/stable-main.ts",
        "./stable",
        "/p/stable.ts",
    );
}

#[test]
fn resolution_currency_inaccessible_probe_is_typed_return_only() {
    assert_typed_probe_return_only(ProbeOutcome::Inaccessible);
}

#[test]
fn resolution_currency_unknown_probe_is_typed_return_only() {
    assert_typed_probe_return_only(ProbeOutcome::Unknown);
}

#[test]
fn resolution_currency_project_provider_change_retargets_selected_context_only() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    workspace.inject_file("/p/dep.ts".to_string(), Arc::from("export const dep = 1"));
    workspace.inject_file(
        "/outside/stable.ts".to_string(),
        Arc::from("export const stable = 1"),
    );
    let mut project = crate::resolver::ide_project_config(
        "/p".to_string(),
        "/".to_string(),
        Some("/p/tsconfig.json".to_string()),
    );
    project.workspace_aliases = vec![WorkspaceAlias {
        find: "@dep".to_string(),
        replacement: "/p/dep".to_string(),
    }];
    WorkspaceAccess::configure_resolver(&workspace, vec![project]);
    assert_eq!(
        WorkspaceRead::resolve_import(&workspace, "/p/main.ts", "@dep", CONTEXT)
            .expect("alias must resolve")
            .source_id,
        "/p/dep.ts"
    );
    assert_eq!(
        WorkspaceRead::resolve_import(&workspace, "/outside/main.ts", "./stable", CONTEXT)
            .expect("stable path must resolve")
            .source_id,
        "/outside/stable.ts"
    );

    let mut changed = crate::resolver::ide_project_config(
        "/p".to_string(),
        "/".to_string(),
        Some("/p/tsconfig.json".to_string()),
    );
    changed.provider_root = "/provider-v2".to_string();
    changed.workspace_aliases = vec![WorkspaceAlias {
        find: "@dep".to_string(),
        replacement: "/p/dep".to_string(),
    }];
    WorkspaceAccess::configure_resolver(&workspace, vec![changed]);

    let before = workspace.vfs_provenance_snapshot();
    assert!(WorkspaceRead::resolve_import(&workspace, "/p/main.ts", "@dep", CONTEXT).is_some());
    assert!(
        WorkspaceRead::resolve_import(&workspace, "/outside/main.ts", "./stable", CONTEXT)
            .is_some()
    );
    let after = workspace.vfs_provenance_snapshot();
    assert_eq!(
        after.import_resolution_cache_miss_count,
        before.import_resolution_cache_miss_count + 1,
        "only the query whose ContextSelection changed may miss"
    );
    assert_eq!(
        after.import_resolution_cache_hit_count,
        before.import_resolution_cache_hit_count + 1,
        "the unrelated context must stay warm"
    );
}

#[test]
fn resolution_currency_subtree_watcher_recovery_is_component_boundary_precise() {
    let engine = Engine::new();
    let reader = ContractReader::new();
    reader.insert("/a/b/dep.ts", "export const affected = 1");
    reader.insert("/a/b2/dep.ts", "export const dep = 1");
    warm_positive(&engine, &reader, "/a/b/main.ts", "./dep");
    warm_positive(&engine, &reader, "/a/b2/main.ts", "./dep");

    engine.apply_changes(vec![WorkspaceChange::DirectoryTreeDirty {
        prefix: "/a/b".to_string(),
    }]);

    assert_recomputed_once(&engine, &reader, "/a/b/main.ts", "./dep", "/a/b/dep.ts");
    assert_warm_hit(&engine, &reader, "/a/b2/main.ts", "./dep", "/a/b2/dep.ts");
}

fn overlay_workspace() -> crate::memory::MemoryWorkspace {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    workspace.inject_file("/p/dep.ts".to_string(), Arc::from("export const dep = 1"));
    workspace
}

fn assert_memory_workspace_warm(
    workspace: &crate::memory::MemoryWorkspace,
    before: VfsProvenanceSnapshot,
) {
    let result = WorkspaceRead::resolve_import(workspace, "/p/main.ts", "./dep", CONTEXT)
        .expect("the effective dep path must remain resolvable");
    assert_eq!(result.source_id, "/p/dep.ts");
    let after = workspace.vfs_provenance_snapshot();
    assert_eq!(
        after.import_resolution_cache_miss_count,
        before.import_resolution_cache_miss_count
    );
    assert_eq!(
        after.import_resolution_cache_hit_count,
        before.import_resolution_cache_hit_count + 1
    );
}

#[test]
fn resolution_currency_overlay_open_advances_only_changed_effective_facts() {
    let workspace = overlay_workspace();
    assert!(WorkspaceRead::resolve_import(&workspace, "/p/main.ts", "./dep", CONTEXT).is_some());

    WorkspaceAccess::notify_upsert(&workspace, "/p/dep.ts", Arc::from("export const dep = 2"));
    let before = workspace.vfs_provenance_snapshot();
    assert_memory_workspace_warm(&workspace, before);
}

#[test]
fn resolution_currency_overlay_close_advances_only_changed_effective_facts() {
    let workspace = overlay_workspace();
    WorkspaceAccess::notify_upsert(&workspace, "/p/dep.ts", Arc::from("export const dep = 2"));
    assert!(WorkspaceRead::resolve_import(&workspace, "/p/main.ts", "./dep", CONTEXT).is_some());

    WorkspaceAccess::notify_close(&workspace, "/p/dep.ts");
    let before = workspace.vfs_provenance_snapshot();
    assert_memory_workspace_warm(&workspace, before);
}

#[test]
fn resolution_currency_overlay_reveal_tracks_effective_population_value() {
    let workspace = overlay_workspace();
    WorkspaceAccess::notify_upsert(
        &workspace,
        "/p/dep.ts",
        Arc::from("export const overlay = 1"),
    );
    assert!(WorkspaceRead::resolve_import(&workspace, "/p/main.ts", "./dep", CONTEXT).is_some());

    workspace.inject_file(
        "/p/dep.ts".to_string(),
        Arc::from("export const hidden_base = 2"),
    );
    WorkspaceAccess::notify_close(&workspace, "/p/dep.ts");
    let before = workspace.vfs_provenance_snapshot();
    assert_memory_workspace_warm(&workspace, before);
}

// ─── Resolution-currency invalidation invariants ────────────────────────
//
// These pin invariants that must survive replacing
// `ResolutionTransaction::absorb`'s whole-signature flattening with
// bounded direct-dependency records, and replacing over-threshold
// precise fact buckets with coarser terminal aggregates. Each is green
// today and must stay green; a coarser propagation seed, or a
// dependency record that forgets an intermediate observation, breaks a
// named one.

/// Each compaction domain's terminal aggregate has a LIVE producer, and
/// the domains do not ride each other's counters.
///
/// An aggregate that never advances is a witness nothing can invalidate —
/// permanent stale warm, and the exact poisoning class domain-wise
/// compaction exists to avoid. So each counter is asserted to move at its
/// own mutation AND to stay put at the other's.
///
/// The source-env / content split is the load-bearing half: a config
/// change republishes the env-hash tables (`parse_env_hash`,
/// `parse_key`, `file_language_id`) with NO content bump, so a
/// source-env fact folded into the content domain would survive it.
///
/// Mutation recipe, VERIFIED: delete the `bump_source_env_generation()`
/// call from the `WorkspaceChange::ConfigChanged` arm — the config arm's
/// `source_env_after > source_env_before` assertion fails. Deleting it
/// from `publish_snapshot` instead fails the publish arm.
#[test]
fn each_engine_owned_compaction_domain_has_a_live_producer() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    let engine = &workspace.engine;

    // Content mutation: content moves, source-env does not.
    let content_before = engine.current_content_generation();
    let source_env_before = engine.current_source_env_generation();
    workspace.inject_file("/p/dep.ts".to_string(), Arc::from("export const v = 1"));
    assert!(
        engine.current_content_generation() > content_before,
        "a content mutation must advance the content domain's generation"
    );
    assert_eq!(
        engine.current_source_env_generation(),
        source_env_before,
        "a content mutation must NOT advance the source-env domain — the two are \
         separate domains precisely so an entry compacted on one is not coarsened by \
         the other"
    );

    // Config change: source-env moves, and it does so WITHOUT a content
    // bump — the reason source-env cannot ride `content_generation`.
    let content_before = engine.current_content_generation();
    let source_env_before = engine.current_source_env_generation();
    workspace.apply_changes(vec![WorkspaceChange::ConfigChanged {
        canonical_id: "/p/tsconfig.json".to_string(),
    }]);
    assert!(
        engine.current_source_env_generation() > source_env_before,
        "a config change moves parse_env_hash / parse_key / file_language_id, so \
         the source-env domain's generation MUST advance"
    );
    assert_eq!(
        engine.current_content_generation(),
        content_before,
        "fixture invariant: a config change deliberately does NOT bump the content \
         generation — if this ever starts bumping, the source-env domain's separate \
         counter is no longer load-bearing and this rationale must be revisited"
    );

    // Project reconfiguration, which reaches `rebuild_and_publish`.
    let source_env_before = engine.current_source_env_generation();
    WorkspaceAccess::configure_resolver(
        &workspace,
        vec![crate::resolver::ide_project_config(
            "/p".to_string(),
            "/".to_string(),
            Some("/p/tsconfig.json".to_string()),
        )],
    );
    assert!(
        engine.current_source_env_generation() > source_env_before,
        "`rebuild_and_publish` recomposes env_hashes_by_project, so the source-env \
         domain's generation must advance"
    );

    // The OTHER env-table republication path, driven directly: an
    // externally-supplied snapshot handed to `publish_snapshot` without
    // going through `rebuild_and_publish`. `FilesystemWorkspace::publish_snapshot`
    // is the production caller. Without this arm the `publish_snapshot`
    // bump is an unfalsifiable claim — every other arm reaches
    // `rebuild_and_publish`, which bumps on its own.
    let snapshot = workspace
        .load_published()
        .expect("the configured workspace publishes a snapshot")
        .snapshot
        .clone();
    let source_env_before = engine.current_source_env_generation();
    let content_before = engine.current_content_generation();
    engine.publish_snapshot(crate::published_state::PublishedRoot::new_vfs_only(
        snapshot,
    ));
    assert!(
        engine.current_source_env_generation() > source_env_before,
        "`publish_snapshot` republishes the per-project env-hash / identity tables, so \
         the source-env domain's generation must advance on THIS path too"
    );
    assert_eq!(
        engine.current_content_generation(),
        content_before,
        "and it must do so without a content bump — the split is the whole point"
    );
}

/// The source-env counter is reachable through the SEAM the session
/// actually reads — `WorkspaceAccess` — and not only through the engine's
/// own inherent accessor.
///
/// `WorkspaceAccess::source_env_generation` defaults to `None`, which
/// means "this workspace tracks no source-env generation" and correctly
/// disarms the domain for a workspace that has no producer. That default
/// is the hazard: a production workspace that FORGOT to override it would
/// report `None`, the session would install no source-env stamp, and the
/// domain would silently stop compacting — no failure, just a permanent
/// silent regression to precise buckets. So each production workspace is
/// asserted to answer `Some` AND to track the live counter.
///
/// Mutation recipe, VERIFIED: delete the `source_env_generation`
/// override from `impl WorkspaceAccess for MemoryWorkspace`. The trait
/// method falls back to the `None` default and the first assertion fails.
#[test]
fn memory_workspace_exposes_the_source_env_generation_through_the_access_trait() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());

    let before = WorkspaceAccess::source_env_generation(&workspace)
        .expect("a production workspace must expose its source-env producer, not the None default");
    assert_eq!(
        before,
        workspace.engine.current_source_env_generation(),
        "the trait seam must report the ENGINE's live counter, not a private copy"
    );

    workspace.apply_changes(vec![WorkspaceChange::ConfigChanged {
        canonical_id: "/p/tsconfig.json".to_string(),
    }]);

    let after = WorkspaceAccess::source_env_generation(&workspace)
        .expect("still Some after a config change");
    assert!(
        after > before,
        "a config change advances the source-env domain, and the trait seam must SEE that \
         advance — a seam that reported a frozen or private value would let the session mint a \
         source-env aggregate that no config change can ever invalidate"
    );
    assert_eq!(
        after,
        workspace.engine.current_source_env_generation(),
        "the seam and the engine must not drift"
    );
}

/// The RESOLUTION domain's aggregate advances at the ledger chokepoint —
/// on a PRECISE per-path mutation — while the ancestor `RecoveryScope`
/// stays `INITIAL`.
///
/// This is why the resolution aggregate is a NEW family and not
/// `RecoveryScope`. The engine deliberately refuses to advance a recovery
/// scope on a precise mutation (advancing an ancestor would destroy every
/// sibling witness under it), so `RecoveryScope` can never serve as the
/// resolution domain's aggregate; the aggregate must do the opposite and
/// move on every resolution mutation, precise ones included.
///
/// Mutation recipe, VERIFIED: delete the
/// `replacement.id = ResolutionWorldId::from_raw(...)` mint from the
/// `WorldWrite::Publish` arm of `mutate_resolution_world_locked`. The
/// stamp stops moving and this fails with
/// `ResolutionWorldId(1) -> ResolutionWorldId(1)`.
///
/// The `RecoveryScope` assertion is UNAFFECTED by that plant — the two
/// are genuinely different families, not one renamed — but it is not
/// literally observable under it: the stamp `assert_ne!` precedes it and
/// panics first. To see it hold, comment out the stamp assertion and
/// re-run under the same plant.
#[test]
fn resolution_aggregate_advances_on_precise_mutation_leaving_recovery_scope_initial() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    let engine = &workspace.engine;
    let base = ResolutionPopulation::Base;
    let recovery = ResolutionFactKey::RecoveryScope {
        canonical_prefix: CanonicalResolutionId::new("/p"),
        population: base,
    };

    let before = engine
        .captured_resolution_stamp_for_test(base)
        .expect("the base world always reports a resolution stamp");
    workspace.inject_file("/p/appears.ts".to_string(), Arc::from("export const v = 1"));
    let after = engine
        .captured_resolution_stamp_for_test(base)
        .expect("the base world always reports a resolution stamp");

    assert_ne!(
        after, before,
        "a precise path appearance is a resolution mutation, so the resolution \
         domain's aggregate stamp MUST move ({before:?} -> {after:?})"
    );
    assert_eq!(
        engine.resolution_fact_version_for_test(base, &recovery),
        ResolutionFactVersion::INITIAL,
        "and it must do so WITHOUT advancing the ancestor RecoveryScope — the precise \
         per-path contract is intact, which is exactly why the resolution aggregate \
         cannot be RecoveryScope"
    );
}

/// The resolution stamp covers `ContextSelection`, which the fact LEDGER
/// does not.
///
/// This is the case that distinguishes the root-identity stamp from a
/// ledger-mutation counter, and nothing else in the suite does:
/// `resolution_aggregate_advances_on_precise_mutation_leaving_recovery_scope_initial`
/// passes under BOTH designs, because a precise path appearance mutates
/// the ledger as well.
///
/// `CapturedResolutionWorld::fact_version` routes a `ContextSelection`
/// key to `context_version(entry)` — the separate `context_versions` map
/// — instead of `ResolutionFactRoot::version`. So withdrawing a project
/// and re-publishing a byte-identical config moves the context version
/// while `advance`/`remove` are never called: a counter maintained at the
/// ledger's mutators would read UNCHANGED across it and a compacted
/// resolution signature would keep validating against a world whose
/// selected context had been replaced.
///
/// The third assertion is the negative control that pins the reason. It
/// fails the moment the stamp is re-derived from ledger mutation, because
/// then "the ledger did not move" and "the stamp did move" cannot both
/// hold.
///
/// This is the plan's mutation-matrix row "Project/context replacement".
///
/// Mutation recipe: re-derive the stamp from a counter advanced in
/// `ResolutionFactRoot::advance`/`remove` (the superseded design) — the
/// stamp assertion fails while the context-version and ledger assertions
/// keep passing, naming exactly which half is blind.
#[test]
fn resolution_stamp_moves_on_context_replacement_the_ledger_never_sees() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    let engine = &workspace.engine;
    let base = ResolutionPopulation::Base;
    let project = || {
        crate::resolver::ide_project_config(
            "/p".to_string(),
            "/".to_string(),
            Some("/p/tsconfig.json".to_string()),
        )
    };

    workspace.inject_file("/p/dep.ts".to_string(), Arc::from("export const v = 1"));
    WorkspaceAccess::configure_resolver(&workspace, vec![project()]);
    // Drive one resolution so the context node is registered and the
    // ledger holds a real path fact to watch.
    let _ = WorkspaceRead::resolve_import(&workspace, "/p/main.ts", "./dep", CONTEXT);

    let context_key = ResolutionFactKey::context_importer("/p/main.ts", base);
    // A LEDGER-backed key: its version comes from
    // `ResolutionFactRoot::version`, so it moves only when `advance` /
    // `remove` run.
    let ledger_key = ResolutionFactKey::PathProbe {
        canonical: CanonicalResolutionId::new("/p/dep.ts"),
        population: base,
    };

    let context_before = engine.resolution_fact_version_for_test(base, &context_key);
    let ledger_before = engine.resolution_fact_version_for_test(base, &ledger_key);
    let stamp_before = engine
        .captured_resolution_stamp_for_test(base)
        .expect("the base world always reports a resolution stamp");

    // Withdraw the project, then re-publish a BYTE-IDENTICAL config. No
    // file content moves; only the published context does.
    WorkspaceAccess::configure_resolver(&workspace, vec![]);
    WorkspaceAccess::configure_resolver(&workspace, vec![project()]);

    let context_after = engine.resolution_fact_version_for_test(base, &context_key);
    let ledger_after = engine.resolution_fact_version_for_test(base, &ledger_key);
    let stamp_after = engine
        .captured_resolution_stamp_for_test(base)
        .expect("the base world always reports a resolution stamp");

    assert_ne!(
        context_after, context_before,
        "fixture invariant: replacing the published context must mint a fresh \
         ContextSelection version, or this case exercises nothing"
    );
    assert_ne!(
        stamp_after, stamp_before,
        "the resolution domain's aggregate stamp MUST move across a context \
         replacement ({stamp_before:?} -> {stamp_after:?}) — a compacted signature \
         stands in for every resolution fact the scope observed, ContextSelection \
         included"
    );
    assert_eq!(
        ledger_after, ledger_before,
        "NEGATIVE CONTROL: the fact LEDGER was not mutated on this path. That is \
         precisely why the stamp cannot be a counter maintained in \
         `ResolutionFactRoot::advance`/`remove` — such a counter reads unchanged \
         here, and a resolution-compacted entry would stale-serve across a context \
         replacement"
    );
}

/// Two owners resolving the SAME helper share its resolution inputs.
///
/// Both stay warm across an unrelated mutation, and BOTH recompute when a
/// higher-priority sibling of the shared helper appears. The warm arm is
/// the one a coarse resolution-domain generation counter threatens: a
/// counter advanced by ANY resolution-fact change would recompute both
/// owners here, so any scheme that compacts this signature must still
/// leave the pair warm.
///
/// Mutation recipe, VERIFIED (invalidation arm): early-return from
/// `ResolutionTransaction::observe_path` when the outcome is
/// `PathProbe::Absent`. Both owners then keep resolving to
/// `/p/helper.tsx` after the higher-priority sibling appears, and this
/// fails at the first `assert_recomputed_once`.
///
/// Mutation recipe, STATED (warm arm, not yet plant-verified because the
/// mechanism does not exist on this tree): seed derived propagation from
/// a coarse resolution-domain counter instead of the changed leaves —
/// the `+2 hits / +0 misses` unrelated arm becomes `+0 / +2`. That is
/// the regression a domain-wide generation counter could introduce, and
/// this arm is here to catch it.
#[test]
fn resolution_currency_shared_helper_owners_invalidate_and_stay_warm_together() {
    let engine = Engine::new();
    let reader = ContractReader::new();
    reader.insert("/p/helper.tsx", "export const h = 1");
    assert_eq!(
        warm_positive(&engine, &reader, "/p/a.ts", "./helper.js"),
        "/p/helper.tsx"
    );
    assert_eq!(
        warm_positive(&engine, &reader, "/p/b.ts", "./helper.js"),
        "/p/helper.tsx"
    );

    // Unrelated appearance: BOTH owners stay warm.
    reader.insert("/elsewhere/new.ts", "export const unrelated = 1");
    engine.bump_content_generation_for("/elsewhere/new.ts");
    assert_warm_hit(&engine, &reader, "/p/a.ts", "./helper.js", "/p/helper.tsx");
    assert_warm_hit(&engine, &reader, "/p/b.ts", "./helper.js", "/p/helper.tsx");

    // The shared helper retargets: BOTH owners recompute, exactly once
    // each, onto the higher-priority sibling.
    reader.insert("/p/helper.ts", "export const h = 2");
    engine.bump_content_generation_for("/p/helper.ts");
    assert_recomputed_once(&engine, &reader, "/p/a.ts", "./helper.js", "/p/helper.ts");
    assert_recomputed_once(&engine, &reader, "/p/b.ts", "./helper.js", "/p/helper.ts");
}

/// A positively-resolving specifier re-resolved after warming takes the
/// REUSE arm — `ResolutionTransaction::absorb`, which inherits the warm
/// candidate's whole signature by reference.
///
/// This is the mechanism the owner-witness growth fixture in
/// `verter_session::resolution_signature_growth_tests` measures at scale:
/// nothing about the reused signature is scoped to the query, so a
/// consumer unioning many of them grows linearly. A bounded direct
/// dependency edge is what must replace the absorb; this case pins that
/// the second resolution is genuinely the reuse arm and not a silent
/// recompute, so a change to that arm cannot go unnoticed.
///
/// Mutation recipe, VERIFIED: replace the warm-candidate `reusable`
/// search in `resolve_import_outcome_in_published` with a hard `None`.
/// The second resolution becomes a miss (`import_resolution_cache_miss_count`
/// 1 -> 2) and `assert_warm_hit` fails.
#[test]
fn resolution_currency_declaration_companion_positive_reuses_its_warm_candidate() {
    let engine = Engine::new();
    let reader = ContractReader::new();
    // The corpus shape: the runtime `.mjs` is absent, the `.d.mts`
    // declaration companion is present.
    reader.insert("/p/_chunks/c0.d.mts", "export declare const V0: number;");
    assert_eq!(
        warm_positive(&engine, &reader, "/p/owner.ts", "./_chunks/c0.mjs"),
        "/p/_chunks/c0.d.mts"
    );
    assert_warm_hit(
        &engine,
        &reader,
        "/p/owner.ts",
        "./_chunks/c0.mjs",
        "/p/_chunks/c0.d.mts",
    );
}

/// An ancestor lookup path that was ABSENT when the resolution was
/// recorded invalidates that resolution the moment it appears.
///
/// The importer sits at `/repo/src/deep/nested/`, so resolving `pkg`
/// walks — and observes — every ancestor `node_modules` candidate,
/// including ones that do not exist. A nearer `node_modules/pkg`
/// appearing beneath a previously-absent ancestor must retarget the
/// resolution; the importer's own bytes never move, so nothing but the
/// recorded absent-ancestor observation can catch it.
///
/// The required re-observation set for a deep appearance is the exact
/// `PathProbe`, the parent `DirectoryMembers`, and every previously
/// recorded absent-ancestor `Realpath`. This pins the behaviour the tree
/// already has, so a rewrite of the observation set cannot lose it.
///
/// Mutation recipe, VERIFIED: early-return from
/// `ResolutionTransaction::observe_path` when the outcome is
/// `PathProbe::Absent`. The nearer package's appearance no longer
/// invalidates and the importer keeps resolving to
/// `/repo/node_modules/pkg/far.d.ts`.
#[test]
fn resolution_currency_absent_ancestor_appearance_retargets_a_deep_importer() {
    let engine = Engine::new();
    let reader = ContractReader::new();
    reader.insert(
        "/repo/node_modules/pkg/package.json",
        r#"{"name":"pkg","types":"far.d.ts"}"#,
    );
    reader.insert("/repo/node_modules/pkg/far.d.ts", "export interface Far {}");
    assert_eq!(
        warm_positive(&engine, &reader, "/repo/src/deep/nested/main.ts", "pkg"),
        "/repo/node_modules/pkg/far.d.ts"
    );

    // A nearer `node_modules` appears under an ancestor directory that
    // did not exist when the resolution above was recorded.
    reader.insert(
        "/repo/src/deep/node_modules/pkg/package.json",
        r#"{"name":"pkg","types":"near.d.ts"}"#,
    );
    reader.insert(
        "/repo/src/deep/node_modules/pkg/near.d.ts",
        "export interface Near {}",
    );
    engine.bump_content_generation_for("/repo/src/deep/node_modules/pkg/package.json");
    engine.bump_content_generation_for("/repo/src/deep/node_modules/pkg/near.d.ts");

    assert_recomputed_once(
        &engine,
        &reader,
        "/repo/src/deep/nested/main.ts",
        "pkg",
        "/repo/src/deep/node_modules/pkg/near.d.ts",
    );
}

/// Removal followed by reintroduction mints a FRESH fact version — the
/// reintroduced path never validates against the version recorded before
/// the removal.
///
/// Any derived resolution record built over these primitives inherits
/// the property, so it is pinned here on the `PathProbe` leaf itself. A
/// version scheme that returned to the pre-removal value would let a
/// witness recorded before the removal validate against the world after
/// it.
///
/// Mutation recipe, VERIFIED: in `Engine::update_base_path_facts`, mint
/// the version from the probe OUTCOME (a content-addressed stamp:
/// `File => 11`, `Absent => 13`, …) instead of
/// `next_resolution_fact_version()`. The present and reintroduced
/// versions both become `ResolutionFactVersion(11)` and the ABA
/// assertion fails.
#[test]
fn resolution_currency_removal_then_reintroduction_mints_a_fresh_version() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    let engine = &workspace.engine;
    let base = ResolutionPopulation::Base;
    let probe = ResolutionFactKey::PathProbe {
        canonical: CanonicalResolutionId::new("/p/mod.ts"),
        population: base,
    };

    workspace.inject_file("/p/mod.ts".to_string(), Arc::from("export const v = 1"));
    let present = engine.resolution_fact_version_for_test(base, &probe);
    assert_ne!(
        present,
        ResolutionFactVersion::INITIAL,
        "fixture invariant: the appearance must advance the probe fact"
    );

    workspace.apply_changes(vec![WorkspaceChange::FileDeleted {
        canonical_id: "/p/mod.ts".to_string(),
    }]);
    let removed = engine.resolution_fact_version_for_test(base, &probe);
    assert_ne!(
        removed, present,
        "removal must advance the probe fact away from its present-version"
    );

    workspace.inject_file("/p/mod.ts".to_string(), Arc::from("export const v = 1"));
    let reintroduced = engine.resolution_fact_version_for_test(base, &probe);
    assert_ne!(
        reintroduced, removed,
        "reintroduction must advance the probe fact away from its removed-version"
    );
    assert_ne!(
        reintroduced, present,
        "ABA: reintroducing byte-identical content must mint a FRESH version, never return \
         to the pre-removal one — a witness recorded before the removal must not validate \
         against the world after it"
    );
}

// ---------------------------------------------------------------------
// Resolution decision DAG
// ---------------------------------------------------------------------

/// The published decision node for one importer/specifier demand, or
/// `None` when that demand never admitted a candidate.
fn decision_node(
    engine: &Engine,
    importer_id: &str,
    specifier: &str,
    population: ResolutionPopulation,
) -> Option<ResolutionFactKey> {
    engine
        .cached_resolution_query_for_test(importer_id, specifier, CONTEXT, population)
        .map(ResolutionFactKey::decision)
}

fn resolution_facts(signature: &crate::ReadSetSignature) -> Vec<ResolutionFactKey> {
    signature
        .facts
        .iter()
        .filter_map(|fact| match fact {
            crate::FactVersionRef::ResolveImports(crate::ResolveImportsFactRef::Resolution(
                fact,
            )) => Some(fact.key.clone()),
            _ => None,
        })
        .collect()
}

fn cacheable_signature(outcome: &crate::ResolutionOutcome) -> crate::ReadSetSignature {
    match &outcome.admission {
        crate::SignatureAdmission::Cacheable(signature) => signature.clone(),
        other => panic!("expected a cacheable resolution, got {other:?}"),
    }
}

/// `RC-2` / `DAG-1`: an admitted resolution's witness is its DECISION
/// node — one typed derived fact — and never the leaf set the attempt
/// observed, cold or warm.
///
/// This is the growth fix stated as an invariant. Before the DAG a warm
/// answer fed the reused candidate's ENTIRE signature into the caller's
/// witness by reference, so every consumer of a resolution inherited the
/// whole transitive leaf set and an owner's witness grew with the closure
/// of everything its specifiers reached.
///
/// The cold and warm witnesses must be IDENTICAL: a producer that dedupes
/// an identical recomputation compares witnesses, so a witness that
/// depended on cache warmth would make every second computation look like
/// a new one.
///
/// Mutation recipe: drop the decision-witness substitution at the resolve
/// fence, so a cacheable outcome carries the attempt's own observation set
/// again. The leaf-absence and cold/warm-equality assertions fail.
#[test]
fn resolution_decision_positive_reused_candidate_depends_on_child_decision() {
    let engine = engine_with_fallback_project("/p");
    let reader = ContractReader::new();
    reader.insert("/p/dep.ts", "export const value = 1");
    let base = ResolutionPopulation::Base;

    let cold = engine.resolve_import_outcome(&reader, "/p/main.ts", "./dep", CONTEXT);
    let cold_witness = cacheable_signature(&cold);
    let node = decision_node(&engine, "/p/main.ts", "./dep", base)
        .expect("an admitted cold resolution publishes its decision node");

    assert_eq!(
        resolution_facts(&cold_witness),
        vec![node.clone()],
        "an admitted resolution roots on its decision node ALONE"
    );

    let edges = engine
        .decision_direct_dependencies_for_test(base, &node)
        .expect("a published decision carries its direct edge set");
    let probes: Vec<_> = edges
        .iter()
        .filter(|key| matches!(key, ResolutionFactKey::PathProbe { .. }))
        .collect();
    assert!(
        !probes.is_empty(),
        "fixture invariant: the attempt must have observed primitive path probes, so the \
         leaf-absence assertion below has something to be about"
    );
    for probe in probes {
        assert!(
            !resolution_facts(&cold_witness).contains(probe),
            "RC-2: {probe:?} is a direct EDGE of the decision, not a member of the witness \
             — flattening it back into the witness is what makes a consumer's root grow \
             with the whole transitive closure"
        );
    }

    let warm = engine.resolve_import_outcome(&reader, "/p/main.ts", "./dep", CONTEXT);
    assert!(
        warm.trace().reused(),
        "fixture invariant: the second attempt must reuse the warm candidate"
    );
    assert_eq!(
        cacheable_signature(&warm).facts.as_ref(),
        cold_witness.facts.as_ref(),
        "the cold and warm witnesses for one demand must be IDENTICAL — a witness that \
         depends on cache warmth defeats every identical-recomputation dedupe downstream"
    );
}

/// `RC-2`: a decision's recorded direct edge set is exactly the primitive
/// facts the query observed plus the child decisions it reused — never a
/// child's transitive leaves, and never the node itself.
///
/// Mutation recipe: make `ResolutionTransaction::direct_edges` return the
/// whole observation set including terminal facts, or drop the
/// self-dependency guard in `publish_derived`. The set-equality and
/// self-edge assertions fail respectively.
#[test]
fn resolution_decision_records_only_direct_dependencies() {
    let engine = engine_with_fallback_project("/p");
    let reader = ContractReader::new();
    reader.insert("/p/dep.ts", "export const value = 1");
    let base = ResolutionPopulation::Base;

    let cold = engine.resolve_import_outcome(&reader, "/p/main.ts", "./dep", CONTEXT);
    assert!(matches!(
        cold.admission,
        crate::SignatureAdmission::Cacheable(_)
    ));
    let node = decision_node(&engine, "/p/main.ts", "./dep", base).expect("published decision");

    let edges: BTreeSet<ResolutionFactKey> = engine
        .decision_direct_dependencies_for_test(base, &node)
        .expect("a published decision carries its complete direct edge set")
        .into_iter()
        .collect();

    // Every edge names this demand's own inputs: the exact-resolution
    // row, the selected context, and the paths the resolver probed.
    assert!(
        edges.contains(&ResolutionFactKey::exact_importer(
            "/p/main.ts",
            "./dep",
            CONTEXT,
            base
        )),
        "the demand's own exact-resolution row is a direct dependency"
    );
    assert!(
        edges.contains(&ResolutionFactKey::context_importer("/p/main.ts", base)),
        "the demand's own selected context is a direct dependency"
    );
    assert!(
        edges
            .iter()
            .any(|key| matches!(key, ResolutionFactKey::PathProbe { .. })),
        "the paths the resolver probed are direct dependencies"
    );
    assert!(
        !edges.contains(&node),
        "a decision is never its own dependency: a self-edge would make propagation \
         advance the node that seeded it"
    );
    assert!(
        edges
            .iter()
            .all(|key| !matches!(key, ResolutionFactKey::Decision { .. })),
        "this query reused no child decision, so no derived edge may be recorded"
    );
}

/// `RC-3`: removing a decision ADVANCES it away from every version a
/// witness can hold, and a reintroduction keeps that tombstone rather
/// than reverting to it.
///
/// Publication mints nothing, so `INITIAL` is a version witnesses DO
/// hold — which is exactly why removal, not publication, is where the
/// fresh version is minted. Reverting to `INITIAL` on reintroduction
/// would re-validate every witness the removal invalidated.
///
/// Mutation recipe: make `ResolutionFactRoot::remove_derived` drop the
/// edges without advancing the version. The tombstone assertion fails.
#[test]
fn resolution_decision_reintroduction_mints_fresh_version() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    workspace.inject_file("/p/dep.ts".to_string(), Arc::from("export const value = 1"));
    let project = crate::resolver::ide_project_config("/p".to_string(), "/".to_string(), None);
    WorkspaceAccess::configure_resolver(&workspace, vec![project]);
    let engine = &workspace.engine;
    let population = WorkspaceRead::resolution_population(&workspace);
    let resolve = || {
        engine.resolve_import_outcome_with_evidence(
            &workspace,
            crate::resolution_currency::ResolutionEvidenceSource::ReaderAuthoritative,
            "/p/main.ts",
            "./dep",
            CONTEXT,
        )
    };

    let first = resolve();
    let first_witness = cacheable_signature(&first);
    let node = decision_node(engine, "/p/main.ts", "./dep", population)
        .expect("an admitted cold resolution publishes its decision node");
    assert_eq!(
        engine.resolution_fact_version_for_test(population, &node),
        ResolutionFactVersion::INITIAL,
        "publication mints no version: the node reads exactly what the publishing \
         request's own captured world says"
    );

    assert!(engine.remove_derived_node_for_test(population, &node));
    let tombstone = engine.resolution_fact_version_for_test(population, &node);
    assert_ne!(
        tombstone,
        ResolutionFactVersion::INITIAL,
        "removal must advance the node away from the version its witnesses hold"
    );
    assert!(
        engine
            .decision_direct_dependencies_for_test(population, &node)
            .is_none(),
        "removal must drop the node's edges in the same operation as its version advance"
    );
    let after_removal = engine
        .capture_published_resolution_world(population)
        .expect("a settled world");
    assert!(
        !first_witness.validates(after_removal.as_ref()),
        "the witness recorded before the removal must stop validating"
    );

    // Reintroduce the same demand: advance a leaf the candidate observed
    // so the slot stops validating and the next attempt recomputes.
    workspace.remove_file("/p/dep.ts");
    workspace.inject_file("/p/dep.ts".to_string(), Arc::from("export const value = 1"));
    resolve();

    let reintroduced_node = decision_node(engine, "/p/main.ts", "./dep", population)
        .expect("the recomputation republishes a decision");
    assert_eq!(
        reintroduced_node, node,
        "fixture invariant: the same demand keeps the same node IDENTITY, so the version \
         comparison below is about the version and not about two different nodes"
    );
    assert!(
        engine
            .decision_direct_dependencies_for_test(population, &node)
            .is_some(),
        "the reintroduction must republish the node's edges"
    );
    let reintroduced = engine.resolution_fact_version_for_test(population, &node);
    assert_ne!(
        reintroduced,
        ResolutionFactVersion::INITIAL,
        "ABA: a removed-and-reintroduced decision must NEVER revert to INITIAL — that is \
         the version every witness recorded before the removal holds"
    );
    assert!(
        !first_witness.validates(
            engine
                .capture_published_resolution_world(population)
                .expect("a settled world")
                .as_ref()
        ),
        "and the pre-removal witness must still be invalid after the reintroduction"
    );
}

/// `RC-3`: a witness recorded against a decision that is later removed
/// stops validating, without any cache entry being evicted.
///
/// Mutation recipe: make `remove_derived` a no-op for the version ledger.
/// The post-removal validation assertion fails.
#[test]
fn resolution_decision_removal_invalidates_old_witness() {
    let engine = engine_with_fallback_project("/p");
    let reader = ContractReader::new();
    reader.insert("/p/dep.ts", "export const value = 1");
    let base = ResolutionPopulation::Base;

    engine.resolve_import_outcome(&reader, "/p/main.ts", "./dep", CONTEXT);
    let warm = engine.resolve_import_outcome(&reader, "/p/main.ts", "./dep", CONTEXT);
    assert!(
        warm.trace().reused(),
        "fixture invariant: the second attempt reuses"
    );
    let witness = cacheable_signature(&warm);
    let node = decision_node(&engine, "/p/main.ts", "./dep", base).expect("published decision");
    assert!(resolution_facts(&witness).contains(&node));

    let before = engine
        .capture_published_resolution_world(base)
        .expect("a settled world");
    assert!(
        witness.validates(before.as_ref()),
        "fixture invariant: the warm witness must validate before the removal"
    );

    assert!(engine.remove_derived_node_for_test(base, &node));
    let after = engine
        .capture_published_resolution_world(base)
        .expect("a settled world");
    assert!(
        !witness.validates(after.as_ref()),
        "a witness rooted on a removed decision must stop validating"
    );
}

/// `RC-4`: a session-population decision is a different node from the
/// base-population decision for the same demand, and neither validates
/// as the other.
///
/// Mutation recipe: drop the `population` rewrite from
/// `ResolutionFactKey::in_population`'s `Decision` arm, or drop the
/// population component from `ResolutionQueryKey`. The two nodes then
/// collide and the inequality assertions fail.
#[test]
fn resolution_decision_overlay_never_validates_as_base() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    workspace.inject_file("/p/dep.ts".to_string(), Arc::from("export const base = 1"));
    let project = crate::resolver::ide_project_config("/p".to_string(), "/".to_string(), None);
    WorkspaceAccess::configure_resolver(&workspace, vec![project]);
    let engine = &workspace.engine;
    let population = WorkspaceRead::resolution_population(&workspace);
    let ResolutionPopulation::Session(_) = population else {
        panic!("an engine-backed editor workspace resolves through a session population");
    };

    let session_outcome = engine.resolve_import_outcome_with_evidence(
        &workspace,
        crate::resolution_currency::ResolutionEvidenceSource::ReaderAuthoritative,
        "/p/main.ts",
        "./dep",
        CONTEXT,
    );
    assert!(matches!(
        session_outcome.admission,
        crate::SignatureAdmission::Cacheable(_)
    ));

    let session_node = decision_node(engine, "/p/main.ts", "./dep", population)
        .expect("the session resolution publishes its decision node");
    let base_node = session_node.in_population(ResolutionPopulation::Base);
    assert_ne!(
        session_node, base_node,
        "RC-4: a decision node carries its population, so the base and session nodes for \
         one demand are distinct keys"
    );

    assert!(
        engine
            .decision_direct_dependencies_for_test(population, &session_node)
            .is_some(),
        "the session root must hold the session decision's edges"
    );

    // The witness the session resolution handed back names the SESSION
    // node. A base capture must refuse it outright rather than settle it
    // against a version it cannot answer for.
    let witness = cacheable_signature(&session_outcome);
    assert_eq!(resolution_facts(&witness), vec![session_node.clone()]);
    let base_capture = engine
        .capture_published_resolution_world(ResolutionPopulation::Base)
        .expect("a settled base world");
    assert!(
        !witness.validates(base_capture.as_ref()),
        "RC-4: a session-population witness must NOT validate against a base capture — a \
         base world composes no overlay, so answering for the session population at all \
         would settle the question with the never-advanced version and serve overlay-rooted \
         work to a base reader"
    );
    assert!(
        witness.validates(
            engine
                .capture_published_resolution_world(population)
                .expect("a settled session world")
                .as_ref()
        ),
        "and it must validate against its own session capture"
    );

    assert!(
        engine
            .decision_direct_dependencies_for_test(ResolutionPopulation::Base, &base_node)
            .is_none(),
        "the base graph must hold no edges for a session-only decision"
    );
}

// ---------------------------------------------------------------------
// Root-owned DAG propagation
// ---------------------------------------------------------------------

/// A memory workspace with one configured project and one dependency,
/// plus the session population its resolutions run under.
fn propagation_fixture() -> (crate::memory::MemoryWorkspace, ResolutionPopulation) {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    workspace.inject_file("/p/dep.ts".to_string(), Arc::from("export const value = 1"));
    workspace.inject_file(
        "/p/other.ts".to_string(),
        Arc::from("export const other = 1"),
    );
    let project = crate::resolver::ide_project_config("/p".to_string(), "/".to_string(), None);
    WorkspaceAccess::configure_resolver(&workspace, vec![project]);
    let population = WorkspaceRead::resolution_population(&workspace);
    (workspace, population)
}

fn resolve_in(
    workspace: &crate::memory::MemoryWorkspace,
    importer: &str,
    specifier: &str,
) -> crate::ResolutionOutcome {
    workspace.engine.resolve_import_outcome_with_evidence(
        workspace,
        crate::resolution_currency::ResolutionEvidenceSource::ReaderAuthoritative,
        importer,
        specifier,
        CONTEXT,
    )
}

/// `RC-1` / `RC-3`: advancing a fact a decision directly depends on
/// advances that decision, and advances it EXACTLY ONCE for the whole
/// mutation batch even though the batch moves several of its edges.
///
/// The once-per-batch half is measured against a control workspace that
/// performs the identical mutation with no decision published: the
/// difference between the two observable-advance deltas is the number of
/// derived advances the propagation performed.
///
/// Mutation recipe: delete the `visited` guard in
/// `ResolutionFactRoot::propagate` so a node can be re-advanced through a
/// second reverse edge. The delta-difference assertion fails.
#[test]
fn resolution_decision_child_advance_advances_parent_once() {
    let (workspace, population) = propagation_fixture();
    let engine = &workspace.engine;

    let cold = resolve_in(&workspace, "/p/main.ts", "./dep");
    let witness = cacheable_signature(&cold);
    let node = decision_node(engine, "/p/main.ts", "./dep", population)
        .expect("an admitted cold resolution publishes its decision node");
    let edges = engine
        .decision_direct_dependencies_for_test(population, &node)
        .expect("edges");
    let dep_edges = edges
        .iter()
        .filter(|key| key.canonical_id() == Some("/p/dep.ts"))
        .count();
    assert!(
        dep_edges > 1,
        "fixture invariant: the batch below advances SEVERAL of this decision's edges \
         (probe, realpath, …) — with only one edge the once-per-batch claim would be \
         vacuous; got {dep_edges}"
    );

    // Control: an identical mutation on a workspace with no decision.
    let (control, _) = propagation_fixture();
    let control_before = control.engine.current_resolution_fact_generation();
    control.remove_file("/p/dep.ts");
    let control_delta = control.engine.current_resolution_fact_generation() - control_before;

    let before = engine.current_resolution_fact_generation();
    workspace.remove_file("/p/dep.ts");
    let delta = engine.current_resolution_fact_generation() - before;

    assert!(
        !witness.validates(
            engine
                .capture_published_resolution_world(population)
                .expect("a settled world")
                .as_ref()
        ),
        "RC-1: a mutation of a fact the decision depends on must invalidate every witness \
         rooted on that decision"
    );
    assert_eq!(
        delta - control_delta,
        1,
        "RC-3: the batch advanced {dep_edges} of the decision's direct edges and the \
         decision must advance ONCE for all of them; control delta {control_delta}, \
         observed {delta}"
    );
}

/// `RC-3`: a mutation that touches none of a decision's direct edges
/// leaves its witness valid.
///
/// Mutation recipe: seed propagation from every key in the reverse map
/// instead of the batch's own advanced keys. This fails.
#[test]
fn resolution_decision_unrelated_mutation_stays_valid() {
    let (workspace, population) = propagation_fixture();
    let engine = &workspace.engine;

    let cold = resolve_in(&workspace, "/p/main.ts", "./dep");
    let witness = cacheable_signature(&cold);

    workspace.inject_file(
        "/p/unrelated.ts".to_string(),
        Arc::from("export const unrelated = 1"),
    );

    assert!(
        witness.validates(
            engine
                .capture_published_resolution_world(population)
                .expect("a settled world")
                .as_ref()
        ),
        "a mutation of a path this decision never observed must leave its witness valid — \
         coarse invalidation here destroys exactly the warm reuse the DAG exists to give"
    );
}

/// `RC-1`: a BASE mutation advances a SESSION decision that depends on
/// it. The session graph records session-population edges whose versions
/// fall back to the base root, so the base publication protocol must
/// translate its changed keys into every live session population.
///
/// Mutation recipe: delete the
/// `propagate_base_changes_into_sessions(..)` call from
/// `mutate_resolution_world_locked`. This fails while the base-only and
/// session-only siblings stay green — which is what pins the fan-out
/// rather than propagation in general.
#[test]
fn resolution_base_mutation_advances_dependent_session_decision() {
    let (workspace, population) = propagation_fixture();
    let engine = &workspace.engine;
    assert!(
        matches!(population, ResolutionPopulation::Session(_)),
        "fixture invariant: the decision must live in a SESSION root"
    );

    let cold = resolve_in(&workspace, "/p/main.ts", "./dep");
    let witness = cacheable_signature(&cold);
    let node = decision_node(engine, "/p/main.ts", "./dep", population).expect("session decision");
    assert!(
        engine
            .decision_direct_dependencies_for_test(population, &node)
            .is_some(),
        "fixture invariant: the session root owns the decision's edges"
    );

    workspace.remove_file("/p/dep.ts");

    assert!(
        !witness.validates(
            engine
                .capture_published_resolution_world(population)
                .expect("a settled world")
                .as_ref()
        ),
        "a base mutation must reach a session decision that depends on it"
    );
}

/// A resolution admission that discovers newer base evidence while holding a
/// session publication gate must reuse that gate during base-to-session
/// propagation. `parking_lot::Mutex` is not reentrant, so losing the held
/// session witness deadlocks before the admission can retry.
///
/// Mutation recipe: pass `None` instead of the captured session fingerprint
/// to the evidence fold in `resolve_import_outcome_in_published`. The worker
/// never reports completion and the watchdog assertion fails.
#[test]
fn resolution_admission_conflict_reuses_held_session_gate() {
    let (done_tx, done_rx) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let (workspace, population) = propagation_fixture();
        assert!(
            matches!(population, ResolutionPopulation::Session(_)),
            "fixture invariant: admission must hold a session publication gate"
        );

        let cold = resolve_in(&workspace, "/p/main.ts", "./dep");
        assert!(
            cold.result().is_some(),
            "fixture invariant: the initial resolution must publish a session decision"
        );
        assert_eq!(
            workspace.engine.lazy_resolution_slot_len_for_test(
                "/p/main.ts",
                "./dep",
                CONTEXT,
                population,
            ),
            1,
            "fixture invariant: the first resolution must populate the candidate slot"
        );

        workspace.engine.lazy_resolution_cache.write().clear();
        assert!(
            workspace.engine.snapshot.write().remove("/p/dep.ts"),
            "fixture invariant: the reader must reveal evidence newer than the recorded base"
        );

        let outcome = resolve_in(&workspace, "/p/main.ts", "./dep");
        done_tx
            .send(outcome.result().is_none())
            .expect("test receiver must remain alive");
    });

    let resolved_absent = done_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("conflicting evidence admission must not re-lock its held session gate");
    assert!(
        resolved_absent,
        "after retrying against the newer base evidence, the removed dependency must not resolve"
    );
}

/// `RC-4`: a session-only mutation advances no base decision.
///
/// Mutation recipe: publish the session overlay's fact advances into the
/// base root. The base witness then stops validating and this fails.
#[test]
fn resolution_session_mutation_does_not_advance_base_decision() {
    let (workspace, session) = propagation_fixture();
    let engine = &workspace.engine;

    // A BASE-population decision, resolved through a base-population
    // reader rather than the workspace's session one.
    let reader = ContractReader::new();
    reader.insert("/p/dep.ts", "export const value = 1");
    let base_outcome = engine.resolve_import_outcome(&reader, "/p/main.ts", "./dep", CONTEXT);
    let base_witness = cacheable_signature(&base_outcome);
    let base_node = decision_node(engine, "/p/main.ts", "./dep", ResolutionPopulation::Base)
        .expect("base decision");
    assert_eq!(base_node.population(), ResolutionPopulation::Base);

    WorkspaceAccess::notify_upsert(&workspace, "/p/dep.ts", Arc::from("export const open = 1"));

    assert!(
        base_witness.validates(
            engine
                .capture_published_resolution_world(ResolutionPopulation::Base)
                .expect("a settled base world")
                .as_ref()
        ),
        "RC-4: an overlay edit lives in its own session root and must not advance a BASE \
         decision — a base reader sees no overlay at all"
    );
    assert!(
        matches!(session, ResolutionPopulation::Session(_)),
        "fixture invariant: the overlay edit landed in a session population"
    );
}

/// `RULE-1`: propagation advances derived versions and EVICTS NOTHING.
///
/// The reverse graph is currency propagation, not cache-invalidation
/// authority: the resolution slot keeps every candidate it held, and each
/// entry goes cold only when its own recorded facts fail ordinary
/// read-side validation.
///
/// Mutation recipe: make the propagation step drain the affected
/// `lazy_resolution_cache` slots. The retained-candidate assertion fails.
#[test]
fn reverse_decision_propagation_does_not_evict_cache_entries() {
    let (workspace, population) = propagation_fixture();
    let engine = &workspace.engine;

    resolve_in(&workspace, "/p/main.ts", "./dep");
    let before =
        engine.lazy_resolution_slot_len_for_test("/p/main.ts", "./dep", CONTEXT, population);
    assert_eq!(
        before, 1,
        "fixture invariant: the slot must hold the admitted candidate"
    );

    workspace.remove_file("/p/dep.ts");

    assert_eq!(
        engine.lazy_resolution_slot_len_for_test("/p/main.ts", "./dep", CONTEXT, population),
        before,
        "RULE-1: propagation must not drain a single dependent entry — it advances the \
         derived version and leaves every candidate exactly where it is"
    );
    let node = decision_node(engine, "/p/main.ts", "./dep", population).expect("decision");
    assert!(
        engine
            .decision_direct_dependencies_for_test(population, &node)
            .is_some(),
        "and the decision keeps its edges, so the next mutation still reaches it"
    );
}

/// The mutation-to-decision-family matrix: for every mutation family the
/// plan enumerates, a decision that depends on it is advanced and a
/// decision that does not is left alone.
///
/// Each row names the demand it depends on, so the appearance row roots
/// on a decision that genuinely probed the appearing path (a resolved
/// demand stops at its first hit and never probes it).
///
/// Mutation recipe: drop any one family's advance from its owning
/// chokepoint (see
/// `every_closed_resolution_fact_family_has_a_live_mutation_rail`). That
/// row's "must advance" assertion fails while the others stay green.
#[test]
fn resolution_decision_mutation_matrix_advances_exactly_the_dependent_decisions() {
    type Mutate = Box<dyn Fn(&crate::memory::MemoryWorkspace)>;
    let rows: Vec<(&str, &str, Mutate)> = vec![
        (
            "content edit of the resolved target",
            "./dep",
            Box::new(|workspace: &crate::memory::MemoryWorkspace| {
                workspace.remove_file("/p/dep.ts");
            }),
        ),
        (
            "precise appearance beneath a previously-absent probe",
            "./missing",
            Box::new(|workspace: &crate::memory::MemoryWorkspace| {
                workspace.inject_file(
                    "/p/missing.ts".to_string(),
                    Arc::from("export const missing = 1"),
                );
            }),
        ),
        (
            "DirectoryTreeDirty over the owner's scope",
            "./dep",
            Box::new(|workspace: &crate::memory::MemoryWorkspace| {
                workspace.apply_changes(vec![WorkspaceChange::DirectoryTreeDirty {
                    prefix: "/p".to_string(),
                }]);
            }),
        ),
        (
            "caller-supplied exact-resolution retarget",
            "./dep",
            Box::new(|workspace: &crate::memory::MemoryWorkspace| {
                workspace.engine.set_exact_resolutions(
                    "/p/main.ts",
                    vec![ExactResolution {
                        specifier: "./dep".to_string(),
                        phase: CONTEXT.phase,
                        kind: CONTEXT.kind,
                        resolved_canonical_id: Some("/p/other.ts".to_string()),
                        possible_canonical_ids: vec!["/p/other.ts".to_string()],
                    }],
                );
            }),
        ),
        (
            "project/context replacement",
            "./dep",
            Box::new(|workspace: &crate::memory::MemoryWorkspace| {
                WorkspaceAccess::configure_resolver(
                    workspace,
                    vec![crate::resolver::ide_project_config(
                        "/p".to_string(),
                        "/".to_string(),
                        Some("/p/tsconfig.json".to_string()),
                    )],
                );
            }),
        ),
    ];

    for (family, specifier, mutate) in rows {
        let (workspace, population) = propagation_fixture();
        let engine = &workspace.engine;

        let dependent = cacheable_signature(&resolve_in(&workspace, "/p/main.ts", specifier));
        // An owner in a DIFFERENT directory, so no ancestor scope, probe
        // or directory-member fact is shared with the row's mutation.
        workspace.inject_file("/q/far.ts".to_string(), Arc::from("export const far = 1"));
        workspace.inject_file(
            "/q/main.ts".to_string(),
            Arc::from("import { far } from './far'"),
        );
        let unrelated = cacheable_signature(&resolve_in(&workspace, "/q/main.ts", "./far"));

        mutate(&workspace);
        let world = engine
            .capture_published_resolution_world(population)
            .expect("a settled world");

        assert!(
            !dependent.validates(world.as_ref()),
            "{family}: a decision depending on this mutation must be advanced"
        );
        assert!(
            unrelated.validates(world.as_ref()),
            "{family}: a decision depending on none of it must stay valid"
        );
    }
}

// ---------------------------------------------------------------------
// Context-change and deep-appearance propagation
// ---------------------------------------------------------------------

/// `RC-1`: a publication that changes an entry's SELECTED context
/// advances every decision depending on that context leaf.
///
/// `ContextSelection` is versioned in a map the fact ledger's mutators
/// never touch, so nothing advances to seed from — the publication has to
/// enumerate the registered context leaves across the swap itself.
///
/// Mutation recipe: delete the before/after comparison loop at the end of
/// `ResolutionWorldRoot::replace_published`. This fails while its
/// unchanged-selection sibling stays green.
#[test]
fn resolution_decision_context_replace_advances_version() {
    let (workspace, population) = propagation_fixture();
    let engine = &workspace.engine;
    let witness = cacheable_signature(&resolve_in(&workspace, "/p/main.ts", "./dep"));

    WorkspaceAccess::configure_resolver(
        &workspace,
        vec![crate::resolver::ide_project_config(
            "/p".to_string(),
            "/".to_string(),
            Some("/p/tsconfig.json".to_string()),
        )],
    );

    assert!(
        !witness.validates(
            engine
                .capture_published_resolution_world(population)
                .expect("a settled world")
                .as_ref()
        ),
        "a changed selected context must advance the decisions that observed it"
    );
}

/// …and a republication that changes NO selection leaves them valid.
///
/// Mutation recipe: seed every registered context leaf unconditionally
/// instead of only the changed ones. This fails while its sibling above
/// stays green — the pair is what distinguishes "enumerates" from
/// "invalidates on every publish".
#[test]
fn resolution_decision_context_replace_unchanged_selection_stays_valid() {
    let (workspace, population) = propagation_fixture();
    let engine = &workspace.engine;
    let witness = cacheable_signature(&resolve_in(&workspace, "/p/main.ts", "./dep"));

    // The identical project set, republished.
    WorkspaceAccess::configure_resolver(
        &workspace,
        vec![crate::resolver::ide_project_config(
            "/p".to_string(),
            "/".to_string(),
            None,
        )],
    );

    assert!(
        witness.validates(
            engine
                .capture_published_resolution_world(population)
                .expect("a settled world")
                .as_ref()
        ),
        "a republication that changes no selection must leave every decision valid — a \
         publish-time blanket invalidation would make every project touch a full recompute"
    );
}

/// `RC-1`: a dependency APPEARING beneath a probe a miss recorded as
/// absent advances that miss's decision, with no `DirectoryTreeDirty` and
/// no recovery-scope advance anywhere.
///
/// Mutation recipe: stop advancing the exact `PathProbe` in
/// `update_base_path_facts`. This fails.
#[test]
fn resolution_decision_negative_deep_appearance_advances_without_tree_dirty() {
    let (workspace, population) = propagation_fixture();
    let engine = &workspace.engine;
    let base = ResolutionPopulation::Base;

    let miss = resolve_in(&workspace, "/p/main.ts", "./missing");
    assert!(
        miss.result().is_none(),
        "fixture invariant: the demand must be an admitted MISS"
    );
    let witness = cacheable_signature(&miss);

    let recovery = ResolutionFactKey::RecoveryScope {
        canonical_prefix: CanonicalResolutionId::new("/p"),
        population: base,
    };
    let recovery_before = engine.resolution_fact_version_for_test(base, &recovery);

    workspace.inject_file(
        "/p/missing.ts".to_string(),
        Arc::from("export const missing = 1"),
    );

    assert!(
        !witness.validates(
            engine
                .capture_published_resolution_world(population)
                .expect("a settled world")
                .as_ref()
        ),
        "the appearance moved the exact probe the miss observed, so its decision must advance"
    );
    assert_eq!(
        engine.resolution_fact_version_for_test(base, &recovery),
        recovery_before,
        "and it must reach the decision through the PRECISE probe — advancing the ancestor \
         recovery scope would destroy every sibling witness under /p"
    );
}

/// `RC-1`: a path appearing beneath an ancestor recorded as having NO
/// realpath advances that ancestor's `Realpath` fact.
///
/// Mutation recipe: delete the `advance_absent_realpath_ancestors` call
/// from `update_base_path_facts`. This fails.
#[test]
fn resolution_decision_absent_realpath_ancestor_appearance_advances() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    let project = crate::resolver::ide_project_config("/p".to_string(), "/".to_string(), None);
    WorkspaceAccess::configure_resolver(&workspace, vec![project]);
    let engine = &workspace.engine;
    let base = ResolutionPopulation::Base;
    let ancestor = ResolutionFactKey::Realpath {
        requested: CanonicalResolutionId::new("/p/nested"),
        population: base,
    };

    let miss = resolve_in(&workspace, "/p/main.ts", "./nested/dep");
    assert!(
        miss.result().is_none(),
        "fixture invariant: nothing exists under /p/nested yet"
    );
    // Record the ancestor as KNOWN-ABSENT. Staged explicitly rather than
    // hoped for: the resolver realpaths candidate FILES, so whether it
    // ever records a directory ancestor is incidental, and a fixture that
    // depended on it would silently stop staging the precondition. The
    // write is a Retain — recording a baseline advances nothing.
    engine.mutate_resolution_world(|world| {
        world.realpaths.insert("/p/nested".to_string(), None);
        ((), crate::engine::WorldWrite::Retain)
    });
    assert!(
        matches!(
            engine
                .capture_published_resolution_world(base)
                .expect("a settled world")
                .base
                .realpaths
                .get("/p/nested"),
            Some(None)
        ),
        "fixture invariant: the ancestor must be recorded as KNOWN-ABSENT — an unrecorded \
         ancestor contradicts nothing and must NOT advance"
    );
    let before = engine.resolution_fact_version_for_test(base, &ancestor);

    workspace.inject_file(
        "/p/nested/dep.ts".to_string(),
        Arc::from("export const nested = 1"),
    );

    assert_ne!(
        engine.resolution_fact_version_for_test(base, &ancestor),
        before,
        "the appearance made /p/nested exist, so an ancestor recorded as having NO realpath \
         must advance — a witness that observed it absent is otherwise never invalidated"
    );

    // The negative half: an ancestor that was never recorded contradicts
    // nothing and must stay untouched.
    let unrecorded = ResolutionFactKey::Realpath {
        requested: CanonicalResolutionId::new("/p/other-nested"),
        population: base,
    };
    assert_eq!(
        engine.resolution_fact_version_for_test(base, &unrecorded),
        ResolutionFactVersion::INITIAL,
        "an ancestor with no recorded value must not be advanced by an appearance"
    );
}

/// `RC-1`: an IMPRECISE watcher recovery advances the ancestor recovery
/// scope, and through it every decision beneath.
///
/// Mutation recipe: delete the `RecoveryScope` advance from
/// `mutate_content_subtree`. This fails while
/// `precise_path_mutation_preserves_recovery_scope_initial` stays green —
/// the pair is what keeps the two mutation classes apart.
#[test]
fn resolution_decision_directory_tree_dirty_advances_via_recovery_scope() {
    let (workspace, population) = propagation_fixture();
    let engine = &workspace.engine;
    let base = ResolutionPopulation::Base;
    let recovery = ResolutionFactKey::RecoveryScope {
        canonical_prefix: CanonicalResolutionId::new("/p"),
        population: base,
    };

    let witness = cacheable_signature(&resolve_in(&workspace, "/p/main.ts", "./dep"));
    let before = engine.resolution_fact_version_for_test(base, &recovery);

    workspace.apply_changes(vec![WorkspaceChange::DirectoryTreeDirty {
        prefix: "/p".to_string(),
    }]);

    assert_ne!(
        engine.resolution_fact_version_for_test(base, &recovery),
        before,
        "an imprecise recovery advances the scope it names"
    );
    assert!(
        !witness.validates(
            engine
                .capture_published_resolution_world(population)
                .expect("a settled world")
                .as_ref()
        ),
        "and the scope reaches every decision that observed it"
    );
}

/// The negative half: a PRECISE per-path mutation leaves every ancestor
/// recovery scope at `INITIAL`.
///
/// Mutation recipe: add the ancestor `RecoveryScope` keys to
/// `Engine::path_fact_keys`. This fails.
#[test]
fn precise_path_mutation_preserves_recovery_scope_initial() {
    let (workspace, _population) = propagation_fixture();
    let engine = &workspace.engine;
    let base = ResolutionPopulation::Base;

    resolve_in(&workspace, "/p/main.ts", "./dep");
    workspace.inject_file(
        "/p/precise.ts".to_string(),
        Arc::from("export const precise = 1"),
    );

    for prefix in ["/p", "/"] {
        assert_eq!(
            engine.resolution_fact_version_for_test(
                base,
                &ResolutionFactKey::RecoveryScope {
                    canonical_prefix: CanonicalResolutionId::new(prefix),
                    population: base,
                }
            ),
            ResolutionFactVersion::INITIAL,
            "a precise appearance must advance NO ancestor recovery scope ({prefix}) — \
             witnesses OBSERVE recovery scopes, and only an imprecise watcher mutation \
             advances them"
        );
    }
}

/// A precise mutation of a path no decision observed leaves it valid.
///
/// The sibling of `resolution_decision_unrelated_mutation_stays_valid`
/// scoped to the PATH families specifically: the appearance advances real
/// probe/realpath/directory facts, and none of them is an edge here.
///
/// Mutation recipe: advance the ancestor `RecoveryScope` on a precise
/// mutation. Every decision under `/` then depends on it and this fails.
#[test]
fn resolution_decision_unrelated_path_mutation_stays_valid() {
    let (workspace, population) = propagation_fixture();
    let engine = &workspace.engine;

    workspace.inject_file("/q/far.ts".to_string(), Arc::from("export const far = 1"));
    let witness = cacheable_signature(&resolve_in(&workspace, "/q/main.ts", "./far"));

    workspace.inject_file(
        "/p/elsewhere.ts".to_string(),
        Arc::from("export const elsewhere = 1"),
    );

    assert!(
        witness.validates(
            engine
                .capture_published_resolution_world(population)
                .expect("a settled world")
                .as_ref()
        ),
        "a precise appearance in an unrelated directory must leave this decision valid"
    );
}

// ---------------------------------------------------------------------
// Owner resolution set
// ---------------------------------------------------------------------

fn owner_set_key(owner: &str, population: ResolutionPopulation) -> ResolutionFactKey {
    ResolutionFactKey::owner_resolution_set(
        CanonicalResolutionId::new(normalize_canonical_id(owner)),
        population,
    )
}

fn owner_set_fact(
    workspace: &crate::memory::MemoryWorkspace,
    owner: &str,
) -> Option<crate::FactVersionRef> {
    WorkspaceAccess::publish_owner_resolution_set(workspace, owner)
}

/// The owner set records CHILD DECISIONS, never their flattened leaves,
/// and it exists only once its single publisher mints it.
///
/// Mutation recipe: make `Engine::publish_owner_resolution_set` record
/// each child's own direct edges instead of the child nodes. The
/// derived-only assertion fails.
#[test]
fn owner_resolution_set_records_child_decisions_not_flattened_leaves() {
    let (workspace, population) = propagation_fixture();
    let engine = &workspace.engine;
    let owner = "/p/main.ts";
    let node = owner_set_key(owner, population);

    resolve_in(&workspace, owner, "./dep");
    resolve_in(&workspace, owner, "./other");

    assert!(
        engine
            .decision_direct_dependencies_for_test(population, &node)
            .is_none(),
        "resolving the owner's specifiers must NOT mint an owner set as a side effect — \
         the node has exactly one publisher"
    );

    assert!(owner_set_fact(&workspace, owner).is_some());
    let edges = engine
        .decision_direct_dependencies_for_test(population, &node)
        .expect("the publisher mints the node's edges");

    assert!(
        !edges.is_empty(),
        "the owner set must record the owner's decisions"
    );
    for edge in &edges {
        assert!(
            matches!(edge, ResolutionFactKey::Decision { .. }),
            "every owner-set edge must be a child DECISION; got {edge:?}"
        );
    }
    assert_eq!(
        edges.len(),
        2,
        "one edge per resolved specifier — the witness is bounded by the owner's own \
         decision count, not by what those decisions transitively reach; got {edges:?}"
    );
    for specifier in ["./dep", "./other"] {
        let child = decision_node(engine, owner, specifier, population).expect("child decision");
        assert!(edges.contains(&child), "{specifier} must be a child edge");
    }
}

/// Any child decision advancing advances the owner set.
///
/// Mutation recipe: publish the owner set with an EMPTY edge set. The
/// advance assertion fails.
#[test]
fn owner_resolution_set_advances_with_any_child_decision() {
    let (workspace, population) = propagation_fixture();
    let engine = &workspace.engine;
    let owner = "/p/main.ts";
    let node = owner_set_key(owner, population);

    resolve_in(&workspace, owner, "./dep");
    resolve_in(&workspace, owner, "./other");
    let fact = owner_set_fact(&workspace, owner).expect("published owner set");
    let witness = crate::ReadSetSignature::new(Arc::from([fact]));
    assert!(witness.validates(
        engine
            .capture_published_resolution_world(population)
            .expect("a settled world")
            .as_ref()
    ));

    // Mutate a leaf under ONE of the two children.
    workspace.remove_file("/p/other.ts");

    assert!(
        !witness.validates(
            engine
                .capture_published_resolution_world(population)
                .expect("a settled world")
                .as_ref()
        ),
        "a mutation reaching ANY child decision must advance the owner set — the owner \
         witness stands in for every one of them"
    );
    assert_ne!(
        engine.resolution_fact_version_for_test(population, &node),
        ResolutionFactVersion::INITIAL
    );
}

/// A decision belonging to a DIFFERENT owner leaves this owner set alone.
///
/// Mutation recipe: drop the owner component from the owner-decision
/// index key, so every decision lands in one bucket. This fails.
#[test]
fn owner_resolution_set_unchanged_for_unrelated_decision() {
    let (workspace, population) = propagation_fixture();
    let engine = &workspace.engine;
    let owner = "/p/main.ts";

    resolve_in(&workspace, owner, "./dep");
    let fact = owner_set_fact(&workspace, owner).expect("published owner set");
    let witness = crate::ReadSetSignature::new(Arc::from([fact]));

    // A second owner, in its own directory, with its own decision.
    workspace.inject_file("/q/far.ts".to_string(), Arc::from("export const far = 1"));
    resolve_in(&workspace, "/q/main.ts", "./far");
    workspace.remove_file("/q/far.ts");

    assert!(
        witness.validates(
            engine
                .capture_published_resolution_world(population)
                .expect("a settled world")
                .as_ref()
        ),
        "another owner's decision advancing must leave this owner set valid"
    );
    let other = owner_set_key("/q/main.ts", population);
    assert!(
        engine
            .decision_direct_dependencies_for_test(population, &other)
            .is_none(),
        "and resolving for another owner must not mint an owner set for it either"
    );
}

/// The publication is idempotent on an unchanged child set, so asking for
/// a warm owner surface does not supersede the view that asked.
///
/// Mutation recipe: drop the `unchanged` early return in
/// `Engine::publish_owner_resolution_set`. The republication then
/// replaces the edges on every call; the world-identity assertion below
/// still holds (publication is a Retain), but the recorded-edge identity
/// churns — assert on the edge set to keep the check honest.
#[test]
fn owner_resolution_set_publication_is_idempotent_on_an_unchanged_child_set() {
    let (workspace, population) = propagation_fixture();
    let engine = &workspace.engine;
    let owner = "/p/main.ts";
    let node = owner_set_key(owner, population);

    resolve_in(&workspace, owner, "./dep");
    let first = owner_set_fact(&workspace, owner).expect("published owner set");
    let first_edges = engine
        .decision_direct_dependencies_for_test(population, &node)
        .expect("edges");

    let second = owner_set_fact(&workspace, owner).expect("republished owner set");
    let second_edges = engine
        .decision_direct_dependencies_for_test(population, &node)
        .expect("edges");

    assert_eq!(
        first, second,
        "an unchanged child set must yield the identical fact ref, so a consumer rooting \
         on it warm-hits through its own view"
    );
    assert_eq!(first_edges.len(), second_edges.len());

    // A NEW child decision does change it.
    resolve_in(&workspace, owner, "./other");
    owner_set_fact(&workspace, owner).expect("republished owner set");
    assert_eq!(
        engine
            .decision_direct_dependencies_for_test(population, &node)
            .expect("edges")
            .len(),
        2,
        "a new child decision must enter the owner set"
    );
}

// ---------------------------------------------------------------------
// Context-membership measurement
// ---------------------------------------------------------------------

/// `CTX-1`: one published index performs ONE project-membership walk per
/// canonical path, however many resolutions select a context for it.
///
/// Selecting a path's resolve context reads the published index's resolver
/// membership, its project list and its two per-project tables, and
/// nothing else — so it is a pure function of `(index, path)` and repeating
/// it is pure waste. Every resolution attempt from an importer selects that
/// importer's context, so `n` demands from one importer against one index
/// currently cost `n` walks.
///
/// The fixture drives TWO importers, each with two demands, so a service
/// that only ever recorded its first path would still be caught; and it
/// asserts a never-demanded path records ZERO, so a service that
/// pre-seeded or fabricated rows could not make the counts above look
/// right.
///
/// Before the memo this fixture measured SIX walks for every one of the
/// four paths its four demands touch.
///
/// Mutation recipe: delete `row.selected = Some(selected.clone());` from
/// `PublishedContextSelection::selected`. Every demand walks again, the
/// tally climbs, and this fails — while
/// `resolution_decision_context_replace_unchanged_selection_stays_valid`
/// stays green, because re-walking is waste, not wrong answers.
#[test]
fn context_selection_evaluates_membership_once_per_path_per_root() {
    let (workspace, _population) = propagation_fixture();
    let engine = &workspace.engine;

    resolve_in(&workspace, "/p/main.ts", "./dep");
    resolve_in(&workspace, "/p/main.ts", "./other");
    resolve_in(&workspace, "/p/second.ts", "./dep");
    resolve_in(&workspace, "/p/second.ts", "./other");

    let published = engine
        .load_published()
        .expect("a configured engine publishes an index");
    assert_eq!(
        published.context_membership_table_clears(),
        0,
        "fixture invariant: the per-path table must not have overflowed, or the counts \
         below are counts since the last clear rather than since publication"
    );
    for importer in ["/p/main.ts", "/p/second.ts"] {
        assert_eq!(
            published.context_membership_evaluations(importer),
            1,
            "{importer}: membership is a pure function of the published index and the \
             path, so every demand after the first must reuse the recorded selection",
        );
    }
    assert_eq!(
        published.context_membership_evaluations("/p/never-demanded.ts"),
        0,
        "and a path no demand selected must record no walk — a service that pre-seeded \
         or fabricated rows would make the counts above meaningless"
    );
}

/// `CTX-1`: the memo belongs to the published INDEX, so every immutable
/// world-root clone taken over that index's lifetime shares it.
///
/// `mutate_resolution_world` replaces the world root with a clone on every
/// mutation. A memo owned by the world root would be re-created — or, if
/// shared by `Arc`, would need its own reset discipline — on each of them.
/// Owning it on the index makes the sharing structural: the clone carries
/// the same `Arc<PublishedRoot>`, so a selection recorded before an
/// unrelated mutation still answers after it.
///
/// Mutation recipe: make `impl Default for PublishedContextSelection`
/// build `Self::with_cap(0)`, so every published index drops its rows on
/// the next insert and retains nothing across the clone. This fails —
/// alongside `context_selection_evaluates_membership_once_per_path_per_root`,
/// since a memo that retains nothing also walks per demand; what this test
/// adds is the asserted precondition that the immutable root really was
/// replaced while the index stayed put.
#[test]
fn context_selection_memo_shared_by_unrelated_root_clone() {
    let (workspace, population) = propagation_fixture();
    let engine = &workspace.engine;

    resolve_in(&workspace, "/p/main.ts", "./dep");
    let published = engine.load_published().expect("a published index");
    assert_eq!(
        published.context_membership_evaluations("/p/main.ts"),
        1,
        "fixture invariant: the first demand must warm this index's memo, or there is \
         nothing for the clone to share"
    );

    let before = engine
        .capture_published_resolution_world(population)
        .expect("a settled world");

    // An UNRELATED mutation: a path no decision here observed. It replaces
    // the immutable world root while leaving the published index alone.
    workspace.inject_file(
        "/p/unrelated.ts".to_string(),
        Arc::from("export const unrelated = 1"),
    );

    let after = engine
        .capture_published_resolution_world(population)
        .expect("a settled world");
    assert!(
        !Arc::ptr_eq(&before.base, &after.base),
        "fixture invariant: the mutation must actually replace the immutable root, or \
         the sharing claim is vacuous"
    );
    assert!(
        Arc::ptr_eq(&published, &engine.load_published().expect("index")),
        "fixture invariant: and it must leave the published index in place — this test \
         is about the clone, not about republication"
    );

    resolve_in(&workspace, "/p/main.ts", "./other");

    assert_eq!(
        published.context_membership_evaluations("/p/main.ts"),
        1,
        "the new world root shares the index's memo: a mutation that changed nothing \
         about project membership must not cost a second membership walk"
    );
}

/// `CTX-1`: a publication resets the memo, because the new index owns a
/// new one.
///
/// The reset is structural rather than performed: `replace_published`
/// installs a different `PublishedRoot`, and a `PublishedRoot`'s memo is
/// private, is absent from every constructor signature, and starts empty.
/// What is observable is the consequence — the incoming index answers the
/// publication's own context enumeration from ITS OWN walk, and the
/// outgoing index keeps its records.
///
/// Mutation recipe: replace `published.context_selection()` in
/// `selected_context_for_path` with a process-wide
/// `static SHARED: OnceLock<PublishedContextSelection>`. Every per-index
/// tally then reads zero and this fails at the first assertion.
#[test]
fn replace_published_resets_context_selection_memo() {
    let (workspace, _population) = propagation_fixture();
    let engine = &workspace.engine;

    resolve_in(&workspace, "/p/main.ts", "./dep");
    let outgoing = engine.load_published().expect("a published index");
    assert_eq!(
        outgoing.context_membership_evaluations("/p/main.ts"),
        1,
        "fixture invariant: the outgoing index must be warm for this importer"
    );

    // Republish the IDENTICAL project set: a new index all the same.
    WorkspaceAccess::configure_resolver(
        &workspace,
        vec![crate::resolver::ide_project_config(
            "/p".to_string(),
            "/".to_string(),
            None,
        )],
    );
    let incoming = engine.load_published().expect("a published index");

    assert!(
        !Arc::ptr_eq(&outgoing, &incoming),
        "fixture invariant: the republication must install a different index"
    );
    assert_eq!(
        incoming.context_membership_evaluations("/p/main.ts"),
        1,
        "the incoming index answered the publication's context enumeration from its OWN \
         walk — a memo carried across the swap would have replayed the outgoing index's \
         selection and recorded no walk at all"
    );
    assert_eq!(
        outgoing.context_membership_evaluations("/p/main.ts"),
        1,
        "and the outgoing index keeps its own records: the reset is per-index, not a \
         global flush, so the pre-swap half of the enumeration still hits its memo"
    );

    resolve_in(&workspace, "/p/main.ts", "./other");
    assert_eq!(
        incoming.context_membership_evaluations("/p/main.ts"),
        1,
        "and the incoming index memoizes from that first walk like any other"
    );
}

/// `CTX-1`: a typed provenance error and a complete "no owning project"
/// are BOTH memoized, as themselves.
///
/// The two are opposite kinds of answer — one is a gap in the index, one
/// is a complete read of a complete index — and a memo that collapsed
/// either into the other, or declined to store errors and retried them per
/// demand, would be a correctness bug rather than a slow path: the error
/// is what refuses admission.
///
/// Mutation recipe: store `Ok(ResolveContextId::unowned())` instead of
/// `selected` in `PublishedContextSelection::selected`. The second demand
/// then admits a witness whose project identity was never observed, and
/// both the typed-variant and the non-admission assertions fail.
#[test]
fn context_selection_error_and_no_project_are_memoized_typed() {
    let engine = engine_with_fallback_project("/p");
    let reader = ContractReader::new();
    reader.insert("/p/dep.ts", "export const value = 1");
    reader.insert("/outside/dep.ts", "export const value = 1");
    let snapshot = engine
        .load_published()
        .expect("configured engine publishes a snapshot")
        .snapshot
        .clone();
    // An index with NO project identity / environment rows: an owning
    // project it cannot complete, and an unowned path it can.
    engine.mutate_resolution_world(|world| {
        world.replace_published(
            Arc::new(crate::published_state::PublishedRoot::new_vfs_only(
                snapshot,
            )),
            &[],
            || engine.next_resolution_fact_version(),
        );
        ((), true)
    });
    let world = engine
        .capture_published_resolution_world(ResolutionPopulation::Base)
        .expect("a settled world");
    let published = world
        .base
        .published
        .clone()
        .expect("the table-less index is installed");

    let owned_first =
        crate::resolution_currency::selected_context_for_path(world.base.as_ref(), "/p/main.ts");
    assert_eq!(
        owned_first,
        Err(crate::resolution_currency::ContextProvenanceError::ProjectIdentityMissing),
        "fixture invariant: an owning project with no identity row is a typed gap"
    );
    let owned_second =
        crate::resolution_currency::selected_context_for_path(world.base.as_ref(), "/p/main.ts");
    assert_eq!(
        owned_second, owned_first,
        "the memoized answer must be the SAME typed variant, not a re-derived or \
         collapsed one"
    );
    assert_eq!(
        published.context_membership_evaluations("/p/main.ts"),
        1,
        "and it must be memoized rather than re-walked per demand"
    );

    let unowned_first = crate::resolution_currency::selected_context_for_path(
        world.base.as_ref(),
        "/outside/main.ts",
    );
    assert_eq!(
        unowned_first,
        Ok(crate::resolution_currency::ResolveContextId::unowned()),
        "fixture invariant: no owning project is a complete observation, not a gap"
    );
    assert_eq!(
        crate::resolution_currency::selected_context_for_path(
            world.base.as_ref(),
            "/outside/main.ts"
        ),
        unowned_first,
        "\"no owning project\" is memoized as the unowned context, not as absence"
    );
    assert_eq!(
        published.context_membership_evaluations("/outside/main.ts"),
        1,
        "negative selection is memoized on the same terms as positive selection"
    );

    // The public boundary: the memoized error must keep refusing admission.
    for pass in ["first", "second"] {
        let outcome = engine.resolve_import_outcome(&reader, "/p/main.ts", "./dep", CONTEXT);
        assert_eq!(
            outcome.non_admission_reason(),
            Some(verter_audit::NonAdmissionReason::ResolutionIncompleteProvenance),
            "{pass} demand: a memoized typed provenance gap must refuse admission every \
             time — a memo that answered Ok would admit an unprovenanced witness"
        );
    }
    assert_eq!(
        published.context_membership_evaluations("/p/main.ts"),
        1,
        "and those demands rode the memo rather than re-walking membership"
    );
}

/// `CTX-1` with `RC-1`: a publication that changes an entry's selected
/// context still advances the decision that depends on it, WITH the memo
/// warm beforehand.
///
/// This is the memo's regression: `replace_published` decides what
/// changed by asking for the selection before and after the swap, so a
/// memo that survived the swap would answer the after-question with the
/// before-answer, find no change, seed nothing, and leave every dependent
/// decision valid against a world whose selection had moved. The
/// pre-publication warm-up is the load-bearing part of the fixture — a
/// cold memo could not stale-answer even if it were shared.
///
/// It rides the inherited machinery end to end: the publication's context
/// enumeration seeds `ResolutionFactRoot::seed_propagation`, the base
/// publication protocol propagates over the reverse edges into the live
/// session graph, and the decision node's own version is what moves.
///
/// Two mutation recipes, one per half.
///
/// Propagation: replace the whole `for (key, was) in registered … ` seed
/// loop at the end of `ResolutionWorldRoot::replace_published` with
/// `let _ = (registered, before);`. The enumeration then asks the incoming
/// index nothing, so the fresh-walk assertion fires first; neutralise that
/// one and the decision-node assertion fires next, `ResolutionFactVersion(0)`
/// against `ResolutionFactVersion(0)` — the decision never leaves INITIAL.
/// Both halves were run.
///
/// Memo scoping: the static-`OnceLock` shared memo from
/// `replace_published_resets_context_selection_memo`. This test then fails
/// on its warm-memo precondition, while the inherited
/// `resolution_decision_context_replace_advances_version` stays GREEN —
/// that inherited test reaches its verdict through the fresh
/// `ResolveContextId` the reconfiguration mints, so it cannot see a memo
/// that is not index-scoped, and this test can. Under both recipes
/// `resolution_decision_context_replace_unchanged_selection_stays_valid`
/// stays green: the failure mode is an invisible change, not a noisy one.
#[test]
fn context_selection_change_advances_dependent_decision() {
    let (workspace, population) = propagation_fixture();
    let engine = &workspace.engine;

    let witness = cacheable_signature(&resolve_in(&workspace, "/p/main.ts", "./dep"));
    let node = decision_node(engine, "/p/main.ts", "./dep", population)
        .expect("an admitted cold resolution publishes its decision node");
    let version_before = engine.resolution_fact_version_for_test(population, &node);

    let outgoing = engine.load_published().expect("a published index");
    assert_eq!(
        outgoing.context_membership_evaluations("/p/main.ts"),
        1,
        "fixture invariant: the memo must be WARM for this importer before the \
         publication, or a stale-answering memo would have nothing stale to answer with"
    );

    // The same project root with a tsconfig: a different selected context.
    WorkspaceAccess::configure_resolver(
        &workspace,
        vec![crate::resolver::ide_project_config(
            "/p".to_string(),
            "/".to_string(),
            Some("/p/tsconfig.json".to_string()),
        )],
    );

    let incoming = engine.load_published().expect("a published index");
    assert!(
        !Arc::ptr_eq(&outgoing, &incoming),
        "fixture invariant: the reconfiguration must install a different index"
    );
    assert_eq!(
        incoming.context_membership_evaluations("/p/main.ts"),
        1,
        "the enumeration asked the INCOMING index and it walked: that fresh walk is what \
         lets the comparison see a change at all"
    );
    assert_ne!(
        engine.resolution_fact_version_for_test(population, &node),
        version_before,
        "the changed selection must advance the DECISION NODE itself, through the \
         reverse-edge propagation the publication seeds"
    );
    assert!(
        !witness.validates(
            engine
                .capture_published_resolution_world(population)
                .expect("a settled world")
                .as_ref()
        ),
        "and every witness rooted on that decision must stop validating"
    );
}

/// Builds an Engine whose configured projects form a `chain0 -> chain1 -> …`
/// project-reference chain of `len` projects. Every link declares the next
/// link's tsconfig in `references`; the LAST link resolves `specifier` to its
/// own `src/index` through `baseUrl` + `paths`, so the chain is resolvable
/// end-to-end and the ONLY thing that can stop the walk is the traversal's
/// own stack-safety fuse.
fn engine_with_project_reference_chain(len: usize, specifier: &str) -> Engine {
    engine_with_chain_tail(len, specifier, Vec::new())
}

/// As [`engine_with_project_reference_chain`], but the LAST link declares
/// `tail_references` instead of nothing — so a test can put a specific edge
/// shape exactly where the fuse lands.
fn engine_with_chain_tail(len: usize, specifier: &str, tail_references: Vec<String>) -> Engine {
    let configs = (0..len)
        .map(|index| {
            let root = format!("/workspace/packages/chain{index}");
            let references = if index + 1 == len {
                tail_references.clone()
            } else {
                vec![format!(
                    "/workspace/packages/chain{}/tsconfig.json",
                    index + 1
                )]
            };
            let compiler_options = if index + 1 == len {
                verter_semantic::resolver_core::IdeProjectCompilerOptions {
                    base_url: Some(format!("{root}/src")),
                    paths: vec![(specifier.to_string(), vec!["index".to_string()])],
                    ..Default::default()
                }
            } else {
                verter_semantic::resolver_core::IdeProjectCompilerOptions::default()
            };
            crate::project_graph::VfsProjectConfig {
                root: root.clone(),
                rank: crate::project_graph::ProjectRank::Discovered,
                tsconfig_path: Some(format!("{root}/tsconfig.json")),
                root_files: Vec::new(),
                extensions: vec![".ts".to_string()],
                workspace_root: "/workspace".to_string(),
                workspace_aliases: Vec::new(),
                compiler_options,
                references,
                membership: crate::configured_membership_match_all_under_root(
                    &crate::CanonicalPath::new(&root),
                ),
            }
        })
        .collect();
    let engine = Engine::new();
    *engine.project_graph.write() = crate::project_graph::ProjectGraph::from_configs(configs);
    engine.rebuild_and_publish();
    engine
}

fn chain_reader(len: usize) -> ContractReader {
    let reader = ContractReader::new();
    reader.insert(
        &format!("/workspace/packages/chain{}/src/index.ts", len - 1),
        "export const value = 1;",
    );
    reader
}

const CHAIN_IMPORTER: &str = "/workspace/packages/chain0/src/App.ts";

/// A project-reference walk that terminates on its stack-safety depth fuse has
/// NOT proven the specifier absent — it abandoned a branch that, walked to the
/// end, resolves. Publishing that `None` caches a WRONG negative, and worse:
/// the fact signature it publishes under never observed the projects the fuse
/// cut off, so editing them cannot invalidate it. Budget exhaustion is
/// `ReturnOnly`.
///
/// This covers the REASON. That the negative is also kept out of the cache is
/// asserted separately, in
/// [`budget_refused_negative_never_enters_the_resolution_cache`], because an
/// assertion sitting after this one would never run under a control.
#[test]
fn project_reference_depth_fuse_refuses_admission() {
    const SPECIFIER: &str = "chain-lib";

    // Fixture invariant: the SAME chain shape, short enough to walk to the
    // end, resolves. So the long chain's `None` below is a fuse artifact and
    // not a genuine miss.
    let short_len = 10;
    let short_engine = engine_with_project_reference_chain(short_len, SPECIFIER);
    let short_reader = chain_reader(short_len);
    let short =
        short_engine.resolve_import_outcome(&short_reader, CHAIN_IMPORTER, SPECIFIER, CONTEXT);
    assert_eq!(
        short.result().map(|result| result.source_id.as_str()),
        Some("/workspace/packages/chain9/src/index.ts"),
        "fixture invariant: a chain inside the depth fuse must resolve end-to-end"
    );

    // The same chain, past the fuse.
    let long_len = 300;
    let engine = engine_with_project_reference_chain(long_len, SPECIFIER);
    let reader = chain_reader(long_len);
    let outcome = engine.resolve_import_outcome(&reader, CHAIN_IMPORTER, SPECIFIER, CONTEXT);

    assert!(
        outcome.result().is_none(),
        "fixture invariant: the depth fuse must cut the walk short of the resolving link"
    );
    assert_eq!(
        outcome.non_admission_reason(),
        Some(verter_audit::NonAdmissionReason::BudgetExceeded),
        "a walk that stopped on its own budget never proved absence, so its \
         negative must never be admitted"
    );
}

/// The refusal has to keep the wrong negative OUT of the cache, not merely
/// stamp a reason on it — that is the whole severity claim, so it is asserted
/// where a revert can reach it.
///
/// It lived inside the refusal test until the reason assertion there was found
/// to panic first, which meant this one never executed under any control at
/// all. Standing alone on the same fixture, it is red under a full production
/// revert directly.
///
/// The body is a lone negative assertion, and it does NOT need a positive half
/// bolted on: the revert control supplies one. Under the revert the fence does
/// not exist, the negative is admitted, the query IS cached, and this reddens —
/// which is simultaneously the proof that this accessor can return `Some` for
/// this fixture, i.e. that the `is_none()` is not passing vacuously.
///
/// It also fails safe if the fixture drifts: a chain too short to reach the
/// fuse resolves, the positive is cached, and this goes red rather than
/// quietly green.
#[test]
fn budget_refused_negative_never_enters_the_resolution_cache() {
    const SPECIFIER: &str = "chain-lib";

    let long_len = 300;
    let engine = engine_with_project_reference_chain(long_len, SPECIFIER);
    let reader = chain_reader(long_len);
    let outcome = engine.resolve_import_outcome(&reader, CHAIN_IMPORTER, SPECIFIER, CONTEXT);
    assert!(
        outcome.result().is_none(),
        "fixture invariant: the depth fuse must cut the walk short of the resolving link"
    );

    let population = reader.resolution_population();
    assert!(
        engine
            .cached_resolution_query_for_test(CHAIN_IMPORTER, SPECIFIER, CONTEXT, population)
            .is_none(),
        "a budget-refused negative must not enter the workspace resolution cache"
    );
}

/// The fence must not over-fire, and the place that can only be tested is the
/// fuse boundary itself.
///
/// Reaching `remaining_depth == 0` does NOT mean work was dropped. By the time
/// that branch runs, the current project's own aliases, `paths` and `baseUrl`
/// have already been checked in the same iteration; the ONLY thing the fuse
/// suppresses is recursion into that project's `references`. When the deepest
/// project the walk enters has no onward references, nothing was skipped, the
/// `None` is a proven absence, and it must stay cacheable.
///
/// All three cases below trip the fuse at the SAME depth and differ only in
/// the shape of the deepest-entered project's `references` — no onward edge,
/// a self-edge the descent would have skipped as a back-edge, and a real
/// onward edge — which is exactly the predicate under test. The last case
/// also pins the boundary: if the chain were too short to reach the fuse at
/// all, it could not report `BudgetExceeded`, and this test would stop
/// characterizing anything.
#[test]
fn fuse_reached_with_nothing_left_to_walk_still_admits_its_negative() {
    const UNUSED: &str = "chain-lib";
    const ABSENT: &str = "absent-lib";

    // The walk enters `chain257` (the last link, no onward references) and
    // hits the fuse there with nothing left to descend into.
    let at_boundary = 258;
    let engine = engine_with_project_reference_chain(at_boundary, UNUSED);
    let reader = chain_reader(at_boundary);
    let outcome = engine.resolve_import_outcome(&reader, CHAIN_IMPORTER, ABSENT, CONTEXT);
    assert!(
        outcome.result().is_none(),
        "fixture invariant: nothing in the chain resolves this specifier"
    );
    assert_eq!(
        outcome.non_admission_reason(),
        None,
        "the fuse suppressed no reference the walk had not already skipped, so \
         absence was fully proven and stays cacheable"
    );

    // Same length, but the link the fuse lands on references ITSELF. The real
    // descent inserts that edge into the active set before recursing, so it
    // would be skipped as a back-edge and nothing would be walked — the
    // predicate has to reason about the active set the descent WOULD have
    // had, not the one in hand when the fuse is checked. A degenerate config,
    // but a legal one.
    let self_edge = vec![format!(
        "/workspace/packages/chain{}/tsconfig.json",
        at_boundary - 1
    )];
    let engine = engine_with_chain_tail(at_boundary, UNUSED, self_edge);
    let reader = chain_reader(at_boundary);
    let outcome = engine.resolve_import_outcome(&reader, CHAIN_IMPORTER, ABSENT, CONTEXT);
    assert_eq!(
        outcome.non_admission_reason(),
        None,
        "the only onward edge is a self-edge the descent would have skipped as \
         a back-edge, so nothing was dropped and absence stays proven"
    );

    // One link longer: the fuse now lands on a project that DOES have an
    // onward reference, so a branch really is dropped and the negative is
    // refused. Same fuse depth, opposite verdict.
    let past_boundary = at_boundary + 1;
    let engine = engine_with_project_reference_chain(past_boundary, UNUSED);
    let reader = chain_reader(past_boundary);
    let outcome = engine.resolve_import_outcome(&reader, CHAIN_IMPORTER, ABSENT, CONTEXT);
    assert_eq!(
        outcome.non_admission_reason(),
        Some(verter_audit::NonAdmissionReason::BudgetExceeded),
        "fixture invariant: this chain really does reach the fuse — an onward \
         reference left unwalked must be refused"
    );
}
