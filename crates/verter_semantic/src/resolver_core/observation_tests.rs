//! Exhaustive observation test double: `impl ResolverObservation for
//! TestDouble` must implement every trait method to compile. A method
//! added to the trait later that is not implemented here fails to
//! compile, which is the actual coverage proof — not a sampled runtime
//! assertion. This double is also a genuine I/O-free runtime harness:
//! every call below touches zero scheduler/host
//! state, only the fixed canned data the double was built with.

use std::collections::HashMap;

use super::sealed::Sealed;
use super::ResolverObservation;
use crate::analysis::flow::FunctionBodySkeleton;
use crate::analysis::types::Hash16;
use crate::resolver_core::{
    AttemptOutcome, AugmentationTargetKey, CanonicalId, EnvHashes, FlowFunctionObservationKey,
    LoweredTypeDecl, LoweredValueDecl, ModuleAugmentationIndexObservation,
    ResolutionPackageManifest, StoreViewProjectIdentity,
};
use std::sync::Arc;

type DeclKey = (verter_type_expr::TopLevelOwnerId, String);

/// A fixed, structurally-valid `ResolutionBasis` for `NeedInputs` payloads
/// this double emits — the double never interprets its contents, so one
/// fixed seed is sufficient here (unlike `attempt_outcome_tests.rs`'s
/// per-seed `basis(raw)`, which needs distinct bases per test case).
fn test_basis() -> crate::resolver_core::ResolutionBasis {
    crate::resolver_core::ResolutionBasis::new(
        crate::resolver_core::ResolutionWorldBasis::new(
            crate::resolver_core::WorkspaceAuthorityId::test_only(1),
            crate::resolver_core::ResolutionPopulation::Base,
            crate::resolver_core::ResolutionWorldId::test_only(1),
            None,
        ),
        None,
    )
}

/// A fixed, in-memory `ResolverObservation` implementation. Holds no
/// `VerterHost`/scheduler reference — only plain owned maps supplied at
/// construction.
#[derive(Default)]
struct TestDouble {
    env_hashes_by_canonical: HashMap<String, EnvHashes>,
    default_env_hashes: Option<EnvHashes>,
    project_identity_by_canonical: HashMap<String, StoreViewProjectIdentity>,
    default_project_identity: Option<StoreViewProjectIdentity>,
    whole_hash_by_canonical: HashMap<String, Option<Hash16>>,
    package_backed_by_canonical: HashMap<String, bool>,
    ambient_symbol_by_key: HashMap<
        (crate::resolver_core::ProjectStableKey, String),
        crate::resolver_core::AmbientSymbolHit,
    >,
    project_generation: Option<u64>,
    /// `None` entry = not inventoried/demanded at all (drives `NeedInputs`);
    /// `Some(None)` = demanded and genuinely absent (`Complete(None)`);
    /// `Some(Some(decl))` = demanded and present (`Complete(Some(decl))`).
    type_decl_by_key: HashMap<DeclKey, Option<Arc<LoweredTypeDecl>>>,
    value_decl_by_key: HashMap<DeclKey, Option<Arc<LoweredValueDecl>>>,
    /// Entry present = this target's index has been scanned in this
    /// attempt's content generation (drives `Complete`, possibly with an
    /// empty `contributors` — the stable "zero augmenters" fact); entry
    /// absent = not yet scanned (drives `NeedInputs`).
    module_augmentation_index_by_target:
        HashMap<AugmentationTargetKey, ModuleAugmentationIndexObservation>,
    /// `None` entry = not yet built for this content version (drives
    /// `NeedInputs`); `Some(None)` = demanded and a typed miss (the
    /// pinned version does not serve this position, `Complete(None)`);
    /// `Some(Some(skeleton))` = built and memoized (`Complete(Some(..))`).
    flow_function_skeleton_by_key:
        HashMap<FlowFunctionObservationKey, Option<Arc<FunctionBodySkeleton>>>,
    /// Entry absent = not yet observed (drives `NeedInputs`); entry
    /// present = the stable classification (`Complete`), including
    /// `PathProbe::Unknown`/`Inaccessible` as themselves stable facts.
    path_probe_by_path: HashMap<String, crate::resolver_core::PathProbe>,
    /// `None` entry = not yet observed (drives `NeedInputs`); `Some(None)`
    /// = demanded, no symlink to resolve (`Complete(None)`); `Some(Some(..))`
    /// = demanded and resolved (`Complete(Some(..))`).
    real_path_by_path: HashMap<String, Option<CanonicalId>>,
    /// `None` entry = not yet observed (drives `NeedInputs`); `Some(None)`
    /// = demanded, no manifest at this directory (`Complete(None)`);
    /// `Some(Some(manifest))` = demanded and present (`Complete(Some(..))`).
    package_manifest_by_directory: HashMap<String, Option<Arc<ResolutionPackageManifest>>>,
}

impl Sealed for TestDouble {}

impl ResolverObservation for TestDouble {
    fn env_hashes(&self, canonical: Option<&str>) -> AttemptOutcome<EnvHashes> {
        let found = match canonical {
            Some(id) => self.env_hashes_by_canonical.get(id).copied(),
            None => self.default_env_hashes,
        };
        match found {
            Some(env) => AttemptOutcome::Complete(env),
            None => AttemptOutcome::NeedInputs(crate::resolver_core::LoadSet::empty(test_basis())),
        }
    }

    fn project_identity(
        &self,
        canonical: Option<&str>,
    ) -> AttemptOutcome<StoreViewProjectIdentity> {
        let found = match canonical {
            Some(id) => self.project_identity_by_canonical.get(id).copied(),
            None => self.default_project_identity,
        };
        match found {
            Some(identity) => AttemptOutcome::Complete(identity),
            None => AttemptOutcome::NeedInputs(crate::resolver_core::LoadSet::empty(test_basis())),
        }
    }

    fn whole_hash(&self, canonical: &str) -> AttemptOutcome<Option<Hash16>> {
        match self.whole_hash_by_canonical.get(canonical) {
            Some(hash) => AttemptOutcome::Complete(*hash),
            None => AttemptOutcome::NeedInputs(crate::resolver_core::LoadSet::empty(test_basis())),
        }
    }

    fn workspace_is_package_backed(&self, canonical: &str) -> AttemptOutcome<bool> {
        match self.package_backed_by_canonical.get(canonical) {
            Some(flag) => AttemptOutcome::Complete(*flag),
            None => AttemptOutcome::NeedInputs(crate::resolver_core::LoadSet::empty(test_basis())),
        }
    }

    fn lookup_ambient_symbol(
        &self,
        consumer_project: crate::resolver_core::ProjectStableKey,
        symbol: &str,
    ) -> AttemptOutcome<Option<crate::resolver_core::AmbientSymbolHit>> {
        // An ambient lookup is always synchronously answerable from
        // already-published ambient-lib state (no load-on-demand):
        // `Complete(None)` is a genuine "no such ambient symbol", never
        // `NeedInputs`.
        AttemptOutcome::Complete(
            self.ambient_symbol_by_key
                .get(&(consumer_project, symbol.to_string()))
                .cloned(),
        )
    }

    fn project_generation(&self) -> AttemptOutcome<u64> {
        match self.project_generation {
            Some(gen) => AttemptOutcome::Complete(gen),
            None => AttemptOutcome::NeedInputs(crate::resolver_core::LoadSet::empty(test_basis())),
        }
    }

    fn type_decl(
        &self,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> AttemptOutcome<Option<Arc<LoweredTypeDecl>>> {
        match self.type_decl_by_key.get(&(owner, name.to_string())) {
            Some(decl) => AttemptOutcome::Complete(decl.clone()),
            None => AttemptOutcome::NeedInputs(crate::resolver_core::LoadSet::new(
                vec![crate::resolver_core::InputKey::DeclBody {
                    canonical: Arc::from(canonical),
                    owner,
                    name: Arc::from(name),
                    space: crate::resolver_core::DeclarationSpace::Type,
                }],
                test_basis(),
            )),
        }
    }

    fn value_decl(
        &self,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> AttemptOutcome<Option<Arc<LoweredValueDecl>>> {
        match self.value_decl_by_key.get(&(owner, name.to_string())) {
            Some(decl) => AttemptOutcome::Complete(decl.clone()),
            None => AttemptOutcome::NeedInputs(crate::resolver_core::LoadSet::new(
                vec![crate::resolver_core::InputKey::DeclBody {
                    canonical: Arc::from(canonical),
                    owner,
                    name: Arc::from(name),
                    space: crate::resolver_core::DeclarationSpace::Value,
                }],
                test_basis(),
            )),
        }
    }

    fn module_augmentation_index(
        &self,
        target: &AugmentationTargetKey,
    ) -> AttemptOutcome<ModuleAugmentationIndexObservation> {
        match self.module_augmentation_index_by_target.get(target) {
            Some(observation) => AttemptOutcome::Complete(observation.clone()),
            None => AttemptOutcome::NeedInputs(crate::resolver_core::LoadSet::new(
                vec![crate::resolver_core::InputKey::ModuleAugmentationIndex {
                    target: target.clone(),
                }],
                test_basis(),
            )),
        }
    }

    fn function_body_skeleton(
        &self,
        key: &FlowFunctionObservationKey,
    ) -> AttemptOutcome<Option<Arc<FunctionBodySkeleton>>> {
        match self.flow_function_skeleton_by_key.get(key) {
            Some(skeleton) => AttemptOutcome::Complete(skeleton.clone()),
            None => AttemptOutcome::NeedInputs(crate::resolver_core::LoadSet::new(
                vec![crate::resolver_core::InputKey::FlowFunctionSkeleton { key: key.clone() }],
                test_basis(),
            )),
        }
    }

    fn path_probe(&self, path: &str) -> AttemptOutcome<crate::resolver_core::PathProbe> {
        match self.path_probe_by_path.get(path) {
            Some(probe) => AttemptOutcome::Complete(*probe),
            None => AttemptOutcome::NeedInputs(crate::resolver_core::LoadSet::new(
                vec![crate::resolver_core::InputKey::PathProbe {
                    path: Arc::from(path),
                }],
                test_basis(),
            )),
        }
    }

    fn real_path(&self, path: &str) -> AttemptOutcome<Option<CanonicalId>> {
        match self.real_path_by_path.get(path) {
            Some(real) => AttemptOutcome::Complete(real.clone()),
            None => AttemptOutcome::NeedInputs(crate::resolver_core::LoadSet::new(
                vec![crate::resolver_core::InputKey::RealPath {
                    path: Arc::from(path),
                }],
                test_basis(),
            )),
        }
    }

    fn package_manifest(
        &self,
        directory: &str,
    ) -> AttemptOutcome<Option<Arc<ResolutionPackageManifest>>> {
        match self.package_manifest_by_directory.get(directory) {
            Some(manifest) => AttemptOutcome::Complete(manifest.clone()),
            None => AttemptOutcome::NeedInputs(crate::resolver_core::LoadSet::new(
                vec![crate::resolver_core::InputKey::PackageManifest {
                    directory: Arc::from(directory),
                }],
                test_basis(),
            )),
        }
    }
}

fn env(byte: u8) -> EnvHashes {
    EnvHashes {
        parse_env_hash: [byte; 16],
        resolve_env_hash: [byte; 16],
        type_env_hash: [byte; 16],
        lib_env_hash: [byte; 16],
    }
}

#[test]
fn env_hashes_hits_per_canonical_entry_before_default() {
    let mut double = TestDouble::default();
    double
        .env_hashes_by_canonical
        .insert("a.ts".to_string(), env(1));
    double.default_env_hashes = Some(env(9));

    assert_eq!(
        double.env_hashes(Some("a.ts")),
        AttemptOutcome::Complete(env(1))
    );
    assert_eq!(double.env_hashes(None), AttemptOutcome::Complete(env(9)));
}

#[test]
fn env_hashes_reports_need_inputs_on_miss() {
    let double = TestDouble::default();
    // Discriminates: a buggy double that defaulted to `Complete` on a miss
    // would hide the "attempt needs to load this" signal the observation
    // contract exists to surface.
    assert!(double.env_hashes(Some("missing.ts")).is_need_inputs());
}

#[test]
fn whole_hash_distinguishes_stable_none_from_need_inputs() {
    let mut double = TestDouble::default();
    double
        .whole_hash_by_canonical
        .insert("tracked.ts".to_string(), Some([7; 16]));
    double
        .whole_hash_by_canonical
        .insert("untracked.ts".to_string(), None);

    assert_eq!(
        double.whole_hash("tracked.ts"),
        AttemptOutcome::Complete(Some([7; 16]))
    );
    // A recorded-but-empty entry is the honest "genuinely untracked"
    // structural fact -- Complete(None), NOT NeedInputs.
    assert_eq!(
        double.whole_hash("untracked.ts"),
        AttemptOutcome::Complete(None)
    );
    // An UNRECORDED canonical is a different case: the attempt doesn't
    // know yet, so it's NeedInputs.
    assert!(double.whole_hash("never-asked.ts").is_need_inputs());
}

#[test]
fn workspace_is_package_backed_reads_canned_classification() {
    let mut double = TestDouble::default();
    double
        .package_backed_by_canonical
        .insert("node_modules/x/index.ts".to_string(), true);
    double
        .package_backed_by_canonical
        .insert("src/local.ts".to_string(), false);

    assert_eq!(
        double.workspace_is_package_backed("node_modules/x/index.ts"),
        AttemptOutcome::Complete(true)
    );
    assert_eq!(
        double.workspace_is_package_backed("src/local.ts"),
        AttemptOutcome::Complete(false)
    );
}

#[test]
fn lookup_ambient_symbol_is_scoped_by_project_and_symbol() {
    let mut double = TestDouble::default();
    let project = crate::resolver_core::ProjectStableKey::Fallback([1; 16]);
    let other_project = crate::resolver_core::ProjectStableKey::Fallback([2; 16]);
    let hit = crate::resolver_core::AmbientSymbolHit {
        project,
        canonical_id: std::sync::Arc::from("lib.es5.d.ts"),
        virtual_id: std::sync::Arc::from("ambient:/F0101.../lib.es5.d.ts"),
        lib_order: 0,
    };
    double
        .ambient_symbol_by_key
        .insert((project, "Array".to_string()), hit.clone());

    assert_eq!(
        double.lookup_ambient_symbol(project, "Array"),
        AttemptOutcome::Complete(Some(hit))
    );
    // Discriminates: a lookup that ignored the project scope would find
    // "Array" under a DIFFERENT project too.
    assert_eq!(
        double.lookup_ambient_symbol(other_project, "Array"),
        AttemptOutcome::Complete(None)
    );
    assert_eq!(
        double.lookup_ambient_symbol(project, "NotAmbient"),
        AttemptOutcome::Complete(None)
    );
}

#[test]
fn project_generation_reports_need_inputs_until_captured() {
    let mut double = TestDouble::default();
    // Discriminates: a double that defaulted to Complete(0) would hide the
    // "attempt hasn't captured a generation yet" signal from a genuine
    // NeedInputs.
    assert!(double.project_generation().is_need_inputs());

    double.project_generation = Some(42);
    assert_eq!(double.project_generation(), AttemptOutcome::Complete(42));
}

#[test]
fn type_decl_reports_need_inputs_until_demanded() {
    let double = TestDouble::default();
    let owner = verter_type_expr::TopLevelOwnerId::ordinary_file();

    // Discriminates: never-demanded must be NeedInputs, not a fabricated
    // Complete(None) — the caller (the eventual session-side driver) needs
    // to know to trigger the blocking lowering path, not treat this as a
    // stable "no such declaration" fact.
    let outcome = double.type_decl("/ws/a.ts", owner, "Foo");
    assert!(outcome.is_need_inputs());
}

#[test]
fn type_decl_need_inputs_names_the_missing_decl_body() {
    let double = TestDouble::default();
    let owner = verter_type_expr::TopLevelOwnerId::ordinary_file();

    match double.type_decl("/ws/a.ts", owner, "Foo") {
        AttemptOutcome::NeedInputs(load_set) => {
            assert_eq!(
                load_set.keys(),
                &[crate::resolver_core::InputKey::DeclBody {
                    canonical: Arc::from("/ws/a.ts"),
                    owner,
                    name: Arc::from("Foo"),
                    space: crate::resolver_core::DeclarationSpace::Type,
                }]
            );
        }
        other => panic!("expected NeedInputs, got {other:?}"),
    }
}

#[test]
fn type_decl_reports_stable_complete_none_for_a_demanded_absence() {
    let mut double = TestDouble::default();
    let owner = verter_type_expr::TopLevelOwnerId::ordinary_file();
    // A demanded-but-genuinely-absent declaration is recorded as
    // Some(None) — distinct from "never demanded" (absent from the map
    // entirely, tested above).
    double
        .type_decl_by_key
        .insert((owner, "Missing".to_string()), None);

    // Discriminates: a lookup that collapsed "demanded absent" into the
    // same NeedInputs bucket as "never demanded" would make the caller
    // retry forever against a declaration that can never resolve.
    // `LoweredTypeDecl` has no `PartialEq` (see decl_body_memo.rs — it's
    // never compared, only identity-shared via `Arc`), so match the shape
    // directly rather than `assert_eq!`.
    assert!(matches!(
        double.type_decl("/ws/a.ts", owner, "Missing"),
        AttemptOutcome::Complete(None)
    ));
}

#[test]
fn value_decl_mirrors_type_decl_semantics() {
    let mut double = TestDouble::default();
    let owner = verter_type_expr::TopLevelOwnerId::ordinary_file();

    assert!(double.value_decl("/ws/a.ts", owner, "x").is_need_inputs());

    double
        .value_decl_by_key
        .insert((owner, "x".to_string()), None);
    assert!(matches!(
        double.value_decl("/ws/a.ts", owner, "x"),
        AttemptOutcome::Complete(None)
    ));
}

fn augmentation_target(byte: u8) -> AugmentationTargetKey {
    AugmentationTargetKey {
        project_identity: crate::resolver_core::ProjectIdentity([byte; 16]),
        resolve_env_hash: [byte; 16],
        lib_env_hash: [byte; 16],
        population: crate::resolver_core::AugmentationPopulation::Base,
        target: crate::resolver_core::AugmentationTargetKind::ExternalSpecifier(
            test_module_specifier(),
        ),
    }
}

fn test_module_specifier() -> crate::facts::registry::InternedSpecifier {
    crate::facts::registry::InternedSpecifier(Arc::from("vue"))
}

#[test]
fn module_augmentation_index_reports_need_inputs_until_scanned() {
    let double = TestDouble::default();
    let target = augmentation_target(1);

    // Discriminates: never-scanned must be NeedInputs, not a fabricated
    // Complete with zero contributors -- the caller needs to know to
    // trigger the session-side cold scan, not treat this as the stable
    // "zero augmenters" fact.
    assert!(double.module_augmentation_index(&target).is_need_inputs());
}

#[test]
fn module_augmentation_index_need_inputs_names_the_missing_target() {
    let double = TestDouble::default();
    let target = augmentation_target(1);

    match double.module_augmentation_index(&target) {
        AttemptOutcome::NeedInputs(load_set) => {
            assert_eq!(
                load_set.keys(),
                &[crate::resolver_core::InputKey::ModuleAugmentationIndex {
                    target: target.clone(),
                }]
            );
        }
        other => panic!("expected NeedInputs, got {other:?}"),
    }
}

#[test]
fn module_augmentation_index_reports_stable_complete_for_a_scanned_empty_index() {
    let mut double = TestDouble::default();
    let target = augmentation_target(2);
    // A scanned-but-genuinely-empty index is recorded with an empty
    // `contributors` slice -- distinct from "never scanned" (absent from
    // the map entirely, tested above).
    double.module_augmentation_index_by_target.insert(
        target.clone(),
        ModuleAugmentationIndexObservation {
            fingerprint: [0; 16],
            contributors: Arc::from([]),
        },
    );

    // Discriminates: a lookup that collapsed "scanned, zero augmenters"
    // into the same NeedInputs bucket as "never scanned" would make the
    // caller retry the cold scan forever against a target that genuinely
    // has no augmenters.
    match double.module_augmentation_index(&target) {
        AttemptOutcome::Complete(observation) => {
            assert!(observation.contributors.is_empty());
        }
        other => panic!("expected Complete, got {other:?}"),
    }
}

#[test]
fn module_augmentation_index_reports_ordered_contributors_when_scanned() {
    let mut double = TestDouble::default();
    let target = augmentation_target(3);
    let contributors: Arc<[_]> = Arc::from([
        crate::resolver_core::AugmentationContributorObservation {
            canonical: Arc::from("a.ts"),
            parse_stable_hash: [1; 16],
        },
        crate::resolver_core::AugmentationContributorObservation {
            canonical: Arc::from("b.ts"),
            parse_stable_hash: [2; 16],
        },
    ]);
    double.module_augmentation_index_by_target.insert(
        target.clone(),
        ModuleAugmentationIndexObservation {
            fingerprint: [9; 16],
            contributors: contributors.clone(),
        },
    );

    assert_eq!(
        double.module_augmentation_index(&target),
        AttemptOutcome::Complete(ModuleAugmentationIndexObservation {
            fingerprint: [9; 16],
            contributors,
        })
    );
}

fn flow_function_key(byte: u8) -> FlowFunctionObservationKey {
    use crate::analysis::function_program::{FunctionDeclarationRef, FunctionProgramKey};
    use verter_type_expr::facts::FunctionPartIdentity;

    FlowFunctionObservationKey {
        canonical_id: Arc::from("/ws/a.ts"),
        function: FunctionProgramKey {
            declaration: FunctionDeclarationRef {
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                name: Arc::from("f"),
                space: crate::facts::SymbolSpace::Value,
            },
            part: FunctionPartIdentity::DeclarationBody,
            overload_ordinal: 0,
        },
        flow_body_stable_hash: [byte; 16],
        flow_body_exact_hash: [byte; 16],
        parse_env_hash: [byte; 16],
    }
}

fn empty_skeleton() -> FunctionBodySkeleton {
    FunctionBodySkeleton {
        names: Arc::from([]),
        regions: Arc::from([]),
        bindings: Arc::from([]),
        expr_sites: Arc::from([]),
        return_sites: Arc::from([]),
        writes: Arc::from([]),
    }
}

#[test]
fn function_body_skeleton_reports_need_inputs_until_built() {
    let double = TestDouble::default();
    let key = flow_function_key(1);

    // Discriminates: never-built must be NeedInputs, not a fabricated
    // Complete(None) — the caller needs to know to trigger the session's
    // graph-store cold build, not treat this as a proven typed miss.
    assert!(double.function_body_skeleton(&key).is_need_inputs());
}

#[test]
fn function_body_skeleton_need_inputs_names_the_missing_key() {
    let double = TestDouble::default();
    let key = flow_function_key(1);

    match double.function_body_skeleton(&key) {
        AttemptOutcome::NeedInputs(load_set) => {
            assert_eq!(
                load_set.keys(),
                &[crate::resolver_core::InputKey::FlowFunctionSkeleton { key: key.clone() }]
            );
        }
        other => panic!("expected NeedInputs, got {other:?}"),
    }
}

#[test]
fn function_body_skeleton_reports_stable_complete_none_for_a_built_typed_miss() {
    let mut double = TestDouble::default();
    let key = flow_function_key(2);
    // A built-but-genuinely-absent position (the pinned content version
    // does not serve this function, or a live entry's hash no longer
    // matches) is recorded as Some(None) — distinct from "never built"
    // (absent from the map entirely, tested above).
    double
        .flow_function_skeleton_by_key
        .insert(key.clone(), None);

    // Discriminates: a lookup that collapsed "built, typed miss" into the
    // same NeedInputs bucket as "never built" would make the caller retry
    // the cold build forever against a position that can never resolve.
    assert!(matches!(
        double.function_body_skeleton(&key),
        AttemptOutcome::Complete(None)
    ));
}

#[test]
fn function_body_skeleton_reports_the_memoized_skeleton_when_built() {
    let mut double = TestDouble::default();
    let key = flow_function_key(3);
    let skeleton = Arc::new(empty_skeleton());
    double
        .flow_function_skeleton_by_key
        .insert(key.clone(), Some(skeleton.clone()));

    assert_eq!(
        double.function_body_skeleton(&key),
        AttemptOutcome::Complete(Some(skeleton))
    );
}

#[test]
fn path_probe_reports_need_inputs_until_observed() {
    let double = TestDouble::default();
    // Discriminates: never-observed must be NeedInputs, not a fabricated
    // Complete(Absent) or Complete(Unknown) -- the caller needs to know
    // to trigger a real probe, not treat this as a proven classification.
    assert!(double.path_probe("/ws/src/a.ts").is_need_inputs());
}

#[test]
fn path_probe_need_inputs_names_the_missing_path() {
    let double = TestDouble::default();

    match double.path_probe("/ws/src/a.ts") {
        AttemptOutcome::NeedInputs(load_set) => {
            assert_eq!(
                load_set.keys(),
                &[crate::resolver_core::InputKey::PathProbe {
                    path: Arc::from("/ws/src/a.ts"),
                }]
            );
        }
        other => panic!("expected NeedInputs, got {other:?}"),
    }
}

#[test]
fn path_probe_folds_every_stable_classification_into_complete() {
    use crate::resolver_core::PathProbe;

    let mut double = TestDouble::default();
    // Discriminates: an I/O-error classification (Unknown/Inaccessible) is
    // ITSELF a stable, cacheable fact once observed -- distinct from
    // "not yet queried" -- so it must fold into Complete like every other
    // PathProbe variant, never collapse to NeedInputs or a fabricated
    // Absent (the CLAUDE.md file_exists boolean-folding rule this
    // observation must not silently reintroduce).
    for (path, probe) in [
        ("/ws/file.ts", PathProbe::File),
        ("/ws/dir", PathProbe::Directory),
        ("/ws/missing.ts", PathProbe::Absent),
        ("/ws/perm-denied.ts", PathProbe::Inaccessible),
        ("/ws/racy.ts", PathProbe::Unknown),
    ] {
        double.path_probe_by_path.insert(path.to_string(), probe);
        assert_eq!(double.path_probe(path), AttemptOutcome::Complete(probe));
    }
}

#[test]
fn real_path_distinguishes_stable_none_from_need_inputs() {
    let mut double = TestDouble::default();
    double.real_path_by_path.insert(
        "/ws/symlinked.ts".to_string(),
        Some(Arc::from("/ws/real.ts")),
    );
    double
        .real_path_by_path
        .insert("/ws/plain.ts".to_string(), None);

    assert_eq!(
        double.real_path("/ws/symlinked.ts"),
        AttemptOutcome::Complete(Some(Arc::from("/ws/real.ts")))
    );
    // A recorded-but-empty entry is the honest "no symlink to resolve"
    // stable fact -- Complete(None), NOT NeedInputs.
    assert_eq!(
        double.real_path("/ws/plain.ts"),
        AttemptOutcome::Complete(None)
    );
    // An UNRECORDED path is a different case: not yet observed.
    assert!(double.real_path("/ws/never-asked.ts").is_need_inputs());
}

#[test]
fn real_path_need_inputs_names_the_missing_path() {
    let double = TestDouble::default();

    match double.real_path("/ws/a.ts") {
        AttemptOutcome::NeedInputs(load_set) => {
            assert_eq!(
                load_set.keys(),
                &[crate::resolver_core::InputKey::RealPath {
                    path: Arc::from("/ws/a.ts"),
                }]
            );
        }
        other => panic!("expected NeedInputs, got {other:?}"),
    }
}

fn manifest_with_main(main: &str) -> ResolutionPackageManifest {
    ResolutionPackageManifest {
        main: Some(main.to_string()),
        module: None,
        types: None,
        typings: None,
        exports: None,
        imports: None,
    }
}

#[test]
fn package_manifest_distinguishes_stable_none_from_need_inputs() {
    let mut double = TestDouble::default();
    let manifest = Arc::new(manifest_with_main("index.js"));
    double
        .package_manifest_by_directory
        .insert("/ws/node_modules/pkg".to_string(), Some(manifest.clone()));
    double
        .package_manifest_by_directory
        .insert("/ws/node_modules/no-manifest".to_string(), None);

    assert_eq!(
        double.package_manifest("/ws/node_modules/pkg"),
        AttemptOutcome::Complete(Some(manifest))
    );
    // A recorded-but-empty entry is the honest "no manifest at this
    // directory" stable fact -- Complete(None), NOT NeedInputs.
    assert_eq!(
        double.package_manifest("/ws/node_modules/no-manifest"),
        AttemptOutcome::Complete(None)
    );
    // An UNRECORDED directory is a different case: not yet observed.
    assert!(double
        .package_manifest("/ws/node_modules/never-asked")
        .is_need_inputs());
}

#[test]
fn package_manifest_need_inputs_names_the_missing_directory() {
    let double = TestDouble::default();

    match double.package_manifest("/ws/node_modules/pkg") {
        AttemptOutcome::NeedInputs(load_set) => {
            assert_eq!(
                load_set.keys(),
                &[crate::resolver_core::InputKey::PackageManifest {
                    directory: Arc::from("/ws/node_modules/pkg"),
                }]
            );
        }
        other => panic!("expected NeedInputs, got {other:?}"),
    }
}

/// The observation projects only
/// `main`/`module`/`types`/`typings`/`exports`/`imports`;
/// a manifest with `exports` set (the modern resolution path) round-trips
/// exactly, proving the DTO carries the fields resolution actually needs.
#[test]
fn package_manifest_round_trips_exports_field() {
    let mut double = TestDouble::default();
    let manifest = Arc::new(ResolutionPackageManifest {
        main: None,
        module: None,
        types: None,
        typings: None,
        exports: Some(serde_json::json!({ ".": "./dist/index.js" })),
        imports: None,
    });
    double
        .package_manifest_by_directory
        .insert("/ws/node_modules/pkg".to_string(), Some(manifest.clone()));

    match double.package_manifest("/ws/node_modules/pkg") {
        AttemptOutcome::Complete(Some(observed)) => {
            assert_eq!(observed.exports, manifest.exports);
        }
        other => panic!("expected Complete(Some(..)), got {other:?}"),
    }
}

#[test]
fn project_identity_hits_per_canonical_entry_before_default() {
    let mut double = TestDouble::default();
    let a = StoreViewProjectIdentity([1; 16]);
    let default = StoreViewProjectIdentity([9; 16]);
    double
        .project_identity_by_canonical
        .insert("a.ts".to_string(), a);
    double.default_project_identity = Some(default);

    assert_eq!(
        double.project_identity(Some("a.ts")),
        AttemptOutcome::Complete(a)
    );
    assert_eq!(
        double.project_identity(None),
        AttemptOutcome::Complete(default)
    );
}
