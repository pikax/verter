#![doc = include_str!("../../../docs/arch/path-precise-resolution-currency.md")]

//! Compile-checked model of the path-precise resolution-currency contract.
//!
//! Everything in this module is test-only: a compile-checked mirror of the
//! contract's taxonomy, observation table, mutation table, query identity,
//! and publication protocol. Each table is an enum the tests match
//! exhaustively, so extending any of them fails to compile until the new
//! variant is classified — the addition cannot enter the contract
//! unreviewed or untested.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::cache_runtime::{CacheAdmission, NonAdmissionReason, SignatureAdmission};
use crate::fact_signature_helpers::ReadSetSignature;
use crate::resolved_import_facts::{ResolvedImportFacts, ResolvedImportFactsKey};
use crate::resolver_core::{
    FactReadSetFinalise, FactVersionRef, ResolveImportsFactRef, ValidatedFactCache,
    FACT_SIGNATURE_CAP,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct CanonicalId(&'static str);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct NormalizedSpecifier(&'static str);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ProjectIdentity([u8; 16]);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ResolverPolicyIdentity([u8; 16]);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ProviderPolicyIdentity([u8; 16]);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ResolveEnvHash([u8; 16]);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct SessionFingerprint([u8; 16]);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum ResolutionPopulation {
    Base,
    Session(SessionFingerprint),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum ResolutionEntry {
    Importer(CanonicalId),
    ExplicitProject(ProjectIdentity),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum ResolvePhase {
    CodegenBlocker,
    ProviderGraph,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[allow(dead_code)]
enum ResolveRequestKind {
    EsmImport,
    TypeImport,
    RequireCall,
    SfcSrcAttr,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ResolveContextId {
    project_identity: ProjectIdentity,
    resolver_policy_identity: ResolverPolicyIdentity,
    provider_policy_identity: ProviderPolicyIdentity,
    resolve_env_hash: ResolveEnvHash,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ResolutionQueryKey {
    entry: ResolutionEntry,
    normalized_specifier: NormalizedSpecifier,
    phase: ResolvePhase,
    request_kind: ResolveRequestKind,
    context: ResolveContextId,
    population: ResolutionPopulation,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum ResolutionFactKey {
    PathProbe {
        canonical: CanonicalId,
        population: ResolutionPopulation,
    },
    Manifest {
        canonical: CanonicalId,
        population: ResolutionPopulation,
    },
    Realpath {
        requested: CanonicalId,
        population: ResolutionPopulation,
    },
    ExactResolution {
        entry: ResolutionEntry,
        specifier: NormalizedSpecifier,
        phase: ResolvePhase,
        kind: ResolveRequestKind,
        population: ResolutionPopulation,
    },
    DirectoryMembers {
        canonical: CanonicalId,
        population: ResolutionPopulation,
    },
    RecoveryScope {
        canonical_prefix: CanonicalId,
        population: ResolutionPopulation,
    },
    ContextSelection {
        entry: ResolutionEntry,
        population: ResolutionPopulation,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum ProbeOutcome {
    File,
    Directory,
    Absent,
    Inaccessible,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Observation {
    ExactLookup {
        entry: ResolutionEntry,
        specifier: NormalizedSpecifier,
        phase: ResolvePhase,
        kind: ResolveRequestKind,
    },
    ContextSelection {
        entry: ResolutionEntry,
    },
    PathProbe {
        requested: CanonicalId,
        outcome: ProbeOutcome,
    },
    Manifest {
        canonical: CanonicalId,
    },
    Realpath {
        requested: CanonicalId,
        resolved: Option<CanonicalId>,
    },
    DirectoryMembers {
        canonical: CanonicalId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum FactFamily {
    PathProbe,
    Manifest,
    Realpath,
    ExactResolution,
    DirectoryMembers,
    RecoveryScope,
    ContextSelection,
}

fn family(key: &ResolutionFactKey) -> FactFamily {
    match key {
        ResolutionFactKey::PathProbe { .. } => FactFamily::PathProbe,
        ResolutionFactKey::Manifest { .. } => FactFamily::Manifest,
        ResolutionFactKey::Realpath { .. } => FactFamily::Realpath,
        ResolutionFactKey::ExactResolution { .. } => FactFamily::ExactResolution,
        ResolutionFactKey::DirectoryMembers { .. } => FactFamily::DirectoryMembers,
        ResolutionFactKey::RecoveryScope { .. } => FactFamily::RecoveryScope,
        ResolutionFactKey::ContextSelection { .. } => FactFamily::ContextSelection,
    }
}

fn ancestor_scopes(path: &CanonicalId) -> Vec<CanonicalId> {
    let mut out = Vec::new();
    let mut current = path.0;
    while let Some(index) = current.rfind('/') {
        let prefix = if index == 0 { "/" } else { &current[..index] };
        out.push(CanonicalId(prefix));
        if prefix == "/" {
            break;
        }
        current = prefix;
    }
    out
}

fn observation_facts(
    observation: Observation,
    population: ResolutionPopulation,
) -> BTreeSet<ResolutionFactKey> {
    let mut facts = BTreeSet::new();
    match observation {
        Observation::ExactLookup {
            entry,
            specifier,
            phase,
            kind,
        } => {
            facts.insert(ResolutionFactKey::ExactResolution {
                entry,
                specifier,
                phase,
                kind,
                population,
            });
        }
        Observation::ContextSelection { entry } => {
            facts.insert(ResolutionFactKey::ContextSelection { entry, population });
        }
        Observation::PathProbe { requested, outcome } => {
            let _typed_outcome = outcome;
            facts.insert(ResolutionFactKey::PathProbe {
                canonical: requested.clone(),
                population: population.clone(),
            });
            for prefix in ancestor_scopes(&requested) {
                facts.insert(ResolutionFactKey::RecoveryScope {
                    canonical_prefix: prefix,
                    population: population.clone(),
                });
            }
        }
        Observation::Manifest { canonical } => {
            facts.insert(ResolutionFactKey::Manifest {
                canonical,
                population,
            });
        }
        Observation::Realpath {
            requested,
            resolved,
        } => {
            facts.insert(ResolutionFactKey::Realpath {
                requested: requested.clone(),
                population: population.clone(),
            });
            for prefix in ancestor_scopes(&requested) {
                facts.insert(ResolutionFactKey::RecoveryScope {
                    canonical_prefix: prefix,
                    population: population.clone(),
                });
            }
            if let Some(resolved) = resolved {
                for prefix in ancestor_scopes(&resolved) {
                    facts.insert(ResolutionFactKey::RecoveryScope {
                        canonical_prefix: prefix,
                        population: population.clone(),
                    });
                }
            }
        }
        Observation::DirectoryMembers { canonical } => {
            facts.insert(ResolutionFactKey::DirectoryMembers {
                canonical,
                population,
            });
        }
    }
    facts
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum Mutation {
    PathValueChanged,
    OrdinarySourceOverwrite,
    ConsultedManifestSemanticChanged,
    OverlayEffectiveValueChanged,
    RealpathChanged,
    ExactOverrideChanged,
    ContextPolicyChanged,
    ImpreciseWatcherRecovery,
}

fn advanced_families(mutation: Mutation) -> BTreeSet<FactFamily> {
    use FactFamily::{
        ContextSelection, DirectoryMembers, ExactResolution, Manifest, PathProbe, Realpath,
        RecoveryScope,
    };
    match mutation {
        Mutation::PathValueChanged => [PathProbe, DirectoryMembers, RecoveryScope].into(),
        Mutation::OrdinarySourceOverwrite => BTreeSet::new(),
        Mutation::ConsultedManifestSemanticChanged => [Manifest, ContextSelection].into(),
        Mutation::OverlayEffectiveValueChanged => [
            PathProbe,
            Manifest,
            Realpath,
            DirectoryMembers,
            RecoveryScope,
        ]
        .into(),
        Mutation::RealpathChanged => [Realpath, RecoveryScope].into(),
        Mutation::ExactOverrideChanged => [ExactResolution].into(),
        Mutation::ContextPolicyChanged => [ContextSelection].into(),
        Mutation::ImpreciseWatcherRecovery => [RecoveryScope].into(),
    }
}

fn fact_families<const N: usize>(families: [FactFamily; N]) -> BTreeSet<FactFamily> {
    families.into_iter().collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResolutionEpoch(u64);

impl ResolutionEpoch {
    fn is_stable(self) -> bool {
        self.0.is_multiple_of(2)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResolutionWorldId(u128);

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolutionWorldRoot {
    id: ResolutionWorldId,
}

#[derive(Debug)]
struct PublicationModel {
    gate_held: bool,
    epoch: ResolutionEpoch,
    current: Arc<ResolutionWorldRoot>,
    replacement: Option<Arc<ResolutionWorldRoot>>,
}

impl PublicationModel {
    fn begin(&mut self) {
        assert!(!self.gate_held);
        assert!(self.epoch.is_stable());
        self.gate_held = true;
        self.epoch.0 += 1;
        assert!(!self.epoch.is_stable());
    }

    fn construct_replacement(&mut self, replacement: Arc<ResolutionWorldRoot>) {
        assert!(self.gate_held);
        assert!(!self.epoch.is_stable());
        self.replacement = Some(replacement);
    }

    fn publish(&mut self) {
        assert!(self.gate_held);
        assert!(!self.epoch.is_stable());
        self.current = self
            .replacement
            .take()
            .expect("step 2 must construct a replacement before publication");
    }

    fn finish(&mut self) {
        assert!(self.gate_held);
        assert!(!self.epoch.is_stable());
        assert!(self.replacement.is_none());
        self.epoch.0 += 1;
        self.gate_held = false;
        assert!(self.epoch.is_stable());
    }
}

#[test]
fn resolution_fact_key_taxonomy_is_closed_over_seven_families() {
    let population = ResolutionPopulation::Base;
    let entry = ResolutionEntry::Importer(CanonicalId("/p/main.ts"));
    let keys = [
        ResolutionFactKey::PathProbe {
            canonical: CanonicalId("/p/dep.ts"),
            population: population.clone(),
        },
        ResolutionFactKey::Manifest {
            canonical: CanonicalId("/p/package.json"),
            population: population.clone(),
        },
        ResolutionFactKey::Realpath {
            requested: CanonicalId("/p/link.ts"),
            population: population.clone(),
        },
        ResolutionFactKey::ExactResolution {
            entry: entry.clone(),
            specifier: NormalizedSpecifier("./dep"),
            phase: ResolvePhase::ProviderGraph,
            kind: ResolveRequestKind::EsmImport,
            population: population.clone(),
        },
        ResolutionFactKey::DirectoryMembers {
            canonical: CanonicalId("/p"),
            population: population.clone(),
        },
        ResolutionFactKey::RecoveryScope {
            canonical_prefix: CanonicalId("/p"),
            population: population.clone(),
        },
        ResolutionFactKey::ContextSelection { entry, population },
    ];
    assert_eq!(
        keys.iter().map(family).collect::<BTreeSet<_>>(),
        fact_families([
            FactFamily::PathProbe,
            FactFamily::Manifest,
            FactFamily::Realpath,
            FactFamily::ExactResolution,
            FactFamily::DirectoryMembers,
            FactFamily::RecoveryScope,
            FactFamily::ContextSelection,
        ])
    );
}

#[test]
fn observation_to_fact_table_records_requested_and_resolved_recovery_chains() {
    let facts = [
        Observation::PathProbe {
            requested: CanonicalId("/a/link/pkg/index.ts"),
            outcome: ProbeOutcome::File,
        },
        Observation::Realpath {
            requested: CanonicalId("/a/link/pkg/index.ts"),
            resolved: Some(CanonicalId("/store/pkg/index.ts")),
        },
    ]
    .into_iter()
    .flat_map(|observation| observation_facts(observation, ResolutionPopulation::Base))
    .collect::<BTreeSet<_>>();
    for prefix in ["/a/link/pkg", "/a/link", "/a", "/store/pkg", "/store"] {
        assert!(
            facts.contains(&ResolutionFactKey::RecoveryScope {
                canonical_prefix: CanonicalId(prefix),
                population: ResolutionPopulation::Base,
            }),
            "missing recovery scope {prefix}"
        );
    }
    assert!(
        !facts.contains(&ResolutionFactKey::RecoveryScope {
            canonical_prefix: CanonicalId("/a/link2"),
            population: ResolutionPopulation::Base,
        }),
        "component-prefix siblings must not enter the recovery chain"
    );
}

#[test]
fn observation_to_fact_table_covers_every_observation_kind() {
    let entry = ResolutionEntry::Importer(CanonicalId("/p/main.ts"));
    let observations = [
        Observation::ExactLookup {
            entry: entry.clone(),
            specifier: NormalizedSpecifier("./dep"),
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::TypeImport,
        },
        Observation::ContextSelection {
            entry: entry.clone(),
        },
        Observation::PathProbe {
            requested: CanonicalId("/p/dep.ts"),
            outcome: ProbeOutcome::Absent,
        },
        Observation::Manifest {
            canonical: CanonicalId("/p/package.json"),
        },
        Observation::Realpath {
            requested: CanonicalId("/p/link"),
            resolved: Some(CanonicalId("/real/target")),
        },
        Observation::DirectoryMembers {
            canonical: CanonicalId("/p"),
        },
    ];
    let actual = observations
        .into_iter()
        .flat_map(|observation| observation_facts(observation, ResolutionPopulation::Base))
        .map(|fact| family(&fact))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        actual,
        fact_families([
            FactFamily::PathProbe,
            FactFamily::Manifest,
            FactFamily::Realpath,
            FactFamily::ExactResolution,
            FactFamily::DirectoryMembers,
            FactFamily::RecoveryScope,
            FactFamily::ContextSelection,
        ])
    );
}

#[test]
fn mutation_to_fact_table_is_value_sensitive_and_path_precise() {
    assert!(advanced_families(Mutation::OrdinarySourceOverwrite).is_empty());
    assert_eq!(
        advanced_families(Mutation::PathValueChanged),
        fact_families([
            FactFamily::PathProbe,
            FactFamily::DirectoryMembers,
            FactFamily::RecoveryScope,
        ])
    );
    assert_eq!(
        advanced_families(Mutation::RealpathChanged),
        fact_families([FactFamily::Realpath, FactFamily::RecoveryScope])
    );
    assert_eq!(
        advanced_families(Mutation::ExactOverrideChanged),
        fact_families([FactFamily::ExactResolution])
    );
    assert_eq!(
        advanced_families(Mutation::ContextPolicyChanged),
        fact_families([FactFamily::ContextSelection])
    );
}

#[test]
fn resolution_query_key_and_context_preserve_complete_split_identity() {
    let key = ResolutionQueryKey {
        entry: ResolutionEntry::ExplicitProject(ProjectIdentity([1; 16])),
        normalized_specifier: NormalizedSpecifier("pkg/subpath"),
        phase: ResolvePhase::ProviderGraph,
        request_kind: ResolveRequestKind::RequireCall,
        context: ResolveContextId {
            project_identity: ProjectIdentity([2; 16]),
            resolver_policy_identity: ResolverPolicyIdentity([3; 16]),
            provider_policy_identity: ProviderPolicyIdentity([4; 16]),
            resolve_env_hash: ResolveEnvHash([5; 16]),
        },
        population: ResolutionPopulation::Session(SessionFingerprint([6; 16])),
    };
    let mut changed = key.clone();
    changed.context.provider_policy_identity = ProviderPolicyIdentity([7; 16]);
    assert_ne!(key, changed);
    changed = key.clone();
    changed.context.resolve_env_hash = ResolveEnvHash([8; 16]);
    assert_ne!(key, changed);
    changed = key.clone();
    changed.population = ResolutionPopulation::Base;
    assert_ne!(key, changed);
}

#[test]
fn resolution_world_publication_is_odd_during_all_four_write_steps() {
    let old = Arc::new(ResolutionWorldRoot {
        id: ResolutionWorldId(1),
    });
    let new = Arc::new(ResolutionWorldRoot {
        id: ResolutionWorldId(2),
    });
    let mut model = PublicationModel {
        gate_held: false,
        epoch: ResolutionEpoch(8),
        current: Arc::clone(&old),
        replacement: None,
    };
    model.begin();
    model.construct_replacement(Arc::clone(&new));
    assert_eq!(model.current, old, "construction cannot publish early");
    model.publish();
    assert_eq!(model.current, new);
    assert!(
        !model.epoch.is_stable(),
        "the replacement is not visible as stable until step 4"
    );
    model.finish();
    assert_eq!(model.epoch, ResolutionEpoch(10));
}

#[test]
fn signature_bound_and_typed_non_admission_use_the_existing_substrate() {
    assert_eq!(FACT_SIGNATURE_CAP, 1_024);
    let empty = SignatureAdmission::from_finalise(FactReadSetFinalise::Ok(Arc::from([])));
    let overflow = SignatureAdmission::from_finalise(FactReadSetFinalise::Overflow);
    assert!(matches!(
        empty,
        SignatureAdmission::Cacheable(ReadSetSignature {
            overflowed: false,
            ..
        })
    ));
    assert!(matches!(
        overflow,
        SignatureAdmission::NonCacheable(NonAdmissionReason::SignatureOverflow)
    ));
    let return_only: CacheAdmission<()> = CacheAdmission::ReturnOnly {
        value: (),
        reason: NonAdmissionReason::UnresolvedProvenance,
    };
    assert!(matches!(return_only, CacheAdmission::ReturnOnly { .. }));
}

#[test]
fn existing_resolution_fact_rail_is_fact_version_ref_resolve_imports() {
    fn require_resolve_imports_variant(fact: FactVersionRef) -> ResolveImportsFactRef {
        match fact {
            FactVersionRef::ResolveImports(fact) => fact,
            _ => panic!("resolution facts must use FactVersionRef::ResolveImports"),
        }
    }
    fn require_validated_fact_cache<K: Eq + std::hash::Hash, V>(
        cache: &ValidatedFactCache<K, V>,
    ) -> &ValidatedFactCache<K, V> {
        cache
    }

    let fact = ResolveImportsFactRef::Semantic {
        canonical_id: "/p/main.ts".to_string(),
        key: verter_semantic::facts::FactKey::ResolvedImportClause {
            specifier: verter_semantic::facts::registry::InternedSpecifier::from("./dep"),
            binding: verter_semantic::facts::registry::InternedName::from("Dep"),
            space: verter_semantic::facts::SymbolSpace::Type,
            resolved_canonical: Arc::from("/p/dep.ts"),
            resolved_source_name: verter_semantic::facts::registry::InternedName::from("Dep"),
        },
        lane: verter_semantic::facts::FactLane::Semantic,
        expected_hash: [0; 16],
    };
    assert_eq!(
        require_resolve_imports_variant(FactVersionRef::ResolveImports(fact.clone())),
        fact
    );
    let cache = ValidatedFactCache::<ResolvedImportFactsKey, ResolvedImportFacts>::default();
    let _: &ValidatedFactCache<ResolvedImportFactsKey, ResolvedImportFacts> =
        require_validated_fact_cache(&cache);
}

#[test]
fn path_component_boundary_does_not_match_byte_prefix_sibling() {
    let prefix = verter_workspace::CanonicalPath::new("/a/b");
    assert!(verter_workspace::CanonicalPath::new("/a/b/file.ts").starts_with_dir(&prefix));
    assert!(verter_workspace::CanonicalPath::new("/a/b").starts_with_dir(&prefix));
    assert!(!verter_workspace::CanonicalPath::new("/a/b2/file.ts").starts_with_dir(&prefix));
}
