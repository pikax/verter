#![doc = include_str!("../../../docs/arch/path-precise-resolution-currency.md")]

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;

use super::Engine;
use crate::changes::WorkspaceChange;
use crate::resolution_currency::{
    CanonicalResolutionId, PathProbe as ProbeOutcome, ResolutionEpoch, ResolutionFactKey,
    ResolutionFactVersion, ResolutionPopulation, SessionFingerprint,
};
use crate::resolver::{normalize_canonical_id, IdeProjectConfig, WorkspaceAlias};
use crate::traits::{WorkspaceAccess, WorkspaceRead};
use crate::types::{
    ExactResolution, ExactResolutionResult, ParsedEdge, ResolutionContext, ResolvePhase,
    ResolveRequestKind, ResolveResult, VfsProvenanceSnapshot,
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
            compiler_options: crate::resolver::IdeProjectCompilerOptions::default(),
            references: Vec::new(),
            membership: crate::ConfiguredMembership::match_all_under_root(
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
    let crate::SignatureAdmission::Cacheable(signature) = &outcome.admission else {
        panic!("a fully tracked resolver read must remain cacheable");
    };
    let directory_fact = ResolutionFactKey::DirectoryMembers {
        canonical: CanonicalResolutionId::new("/p"),
        population: ResolutionPopulation::Base,
    };
    assert!(
        signature
            .resolution_fact_version(&directory_fact)
            .is_some(),
        "a directory enumeration performed inside a typed path probe must be visible to TransactionReader"
    );

    // Mutation recipe: stop draining the reader's directory-observation
    // evidence after probe_path. The result remains correct, but this exact
    // DirectoryMembers fact disappears from the admitted signature.
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
        vec![IdeProjectConfig::new(root.clone(), root, None)],
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

    let project = IdeProjectConfig::new(
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

    // Mutation recipe: delete any one call to update_base_path_facts,
    // update_base_manifest_fact, replace_world_exact_resolutions, the
    // DirectoryTreeDirty RecoveryScope advance, or the computed
    // ContextSelection validator. The matching family remains at its old
    // version and this seven-family inventory fails. Re-adding RecoveryScope
    // advances to the precise per-path chokepoint flips the negative
    // assertion above.
}

#[test]
fn session_overlay_root_is_independent_and_shields_hidden_base_mutations() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    workspace.inject_file("/p/dep.ts".to_string(), Arc::from("export const base = 1"));
    let population = WorkspaceRead::resolution_population(&workspace);
    let ResolutionPopulation::Session(session) = population else {
        panic!("engine-backed editor workspaces must resolve through a session population");
    };
    let other_session = SessionFingerprint::fresh(0x5E5510);
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
        vec![IdeProjectConfig::new(
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
        let mut importer = IdeProjectConfig::new(
            "/app".to_string(),
            "/".to_string(),
            Some("/app/tsconfig.json".to_string()),
        );
        importer.workspace_aliases = vec![WorkspaceAlias {
            find: "@lib".to_string(),
            replacement: "/lib/dep".to_string(),
        }];
        let mut target = IdeProjectConfig::new(
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
    let project = IdeProjectConfig::new(
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

    let mut solution = IdeProjectConfig::new(
        "/p".to_string(),
        "/".to_string(),
        Some("/p/tsconfig.json".to_string()),
    );
    solution.references = vec!["/p/z.json".to_string()];
    let leaf = IdeProjectConfig::new(
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
        "query provenance must use ProjectResolver::nearest_config_for_path, not the provider-default owner policy"
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
    let mut project = IdeProjectConfig::new(
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

    let mut changed = IdeProjectConfig::new(
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
