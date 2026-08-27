#![doc = include_str!("../../../docs/arch/path-precise-resolution-currency.md")]

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use parking_lot::Mutex;

use super::ModuleResolverCoreTestExt;
use crate::traits::WorkspaceRead;
use crate::types::{ResolutionKind, ResolvePhase, ResolveRequest, ResolveRequestKind};
use verter_semantic::resolver_core::{normalize_canonical_id, AttemptFailure, ModuleResolverCore};

#[derive(Debug, Clone, PartialEq, Eq)]
enum ResolverObservation {
    Probe {
        requested: String,
        outcome: ProbeOutcome,
    },
    Realpath {
        requested: String,
        resolved: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(dead_code)]
enum ProbeOutcome {
    File,
    Directory,
    Absent,
    Inaccessible,
    Unknown,
}

struct TraceReader {
    files: BTreeSet<String>,
    realpaths: HashMap<String, String>,
    observations: Mutex<Vec<ResolverObservation>>,
}

impl TraceReader {
    fn new(files: &[&str]) -> Self {
        Self {
            files: files
                .iter()
                .map(|path| normalize_canonical_id(path))
                .collect(),
            realpaths: HashMap::new(),
            observations: Mutex::new(Vec::new()),
        }
    }

    fn with_realpath(mut self, requested: &str, resolved: &str) -> Self {
        self.realpaths.insert(
            normalize_canonical_id(requested),
            normalize_canonical_id(resolved),
        );
        self
    }

    fn observations(&self) -> Vec<ResolverObservation> {
        self.observations.lock().clone()
    }

    fn probe_path(&self, canonical_id: &str) -> ProbeOutcome {
        let requested = normalize_canonical_id(canonical_id);
        let outcome = if self.files.contains(&requested) {
            ProbeOutcome::File
        } else {
            ProbeOutcome::Absent
        };
        self.observations
            .lock()
            .push(ResolverObservation::Probe { requested, outcome });
        outcome
    }
}

impl WorkspaceRead for TraceReader {
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
            .contains(&normalize_canonical_id(canonical_id))
            .then(|| Arc::from("// resolution witness fixture"))
    }

    fn file_exists(&self, canonical_id: &str) -> bool {
        matches!(self.probe_path(canonical_id), ProbeOutcome::File)
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        let requested = normalize_canonical_id(canonical_id);
        let resolved = self
            .realpaths
            .get(&requested)
            .cloned()
            .or_else(|| self.files.contains(&requested).then(|| requested.clone()));
        self.observations
            .lock()
            .push(ResolverObservation::Realpath {
                requested,
                resolved: resolved.clone(),
            });
        resolved
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum WitnessFact {
    PathProbe {
        canonical: String,
        outcome: ProbeOutcome,
    },
    Realpath {
        requested: String,
        resolved: Option<String>,
    },
    RecoveryScope {
        canonical_prefix: String,
    },
}

fn ancestor_scopes(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = path;
    while let Some(index) = current.rfind('/') {
        let prefix = if index == 0 { "/" } else { &current[..index] };
        out.push(prefix.to_string());
        if prefix == "/" {
            break;
        }
        current = prefix;
    }
    out
}

fn retained_witness(observations: &[ResolverObservation]) -> BTreeSet<WitnessFact> {
    let mut facts = BTreeSet::new();
    for observation in observations {
        match observation {
            ResolverObservation::Probe { requested, outcome } => {
                facts.insert(WitnessFact::PathProbe {
                    canonical: requested.clone(),
                    outcome: *outcome,
                });
                for prefix in ancestor_scopes(requested) {
                    facts.insert(WitnessFact::RecoveryScope {
                        canonical_prefix: prefix,
                    });
                }
            }
            ResolverObservation::Realpath {
                requested,
                resolved,
            } => {
                facts.insert(WitnessFact::Realpath {
                    requested: requested.clone(),
                    resolved: resolved.clone(),
                });
                for prefix in ancestor_scopes(requested) {
                    facts.insert(WitnessFact::RecoveryScope {
                        canonical_prefix: prefix,
                    });
                }
                if let Some(resolved) = resolved {
                    for prefix in ancestor_scopes(resolved) {
                        facts.insert(WitnessFact::RecoveryScope {
                            canonical_prefix: prefix,
                        });
                    }
                }
            }
        }
    }
    facts
}

fn request(specifier: &str) -> ResolveRequest {
    ResolveRequest {
        importer_id: "/p/main.ts".to_string(),
        specifier: specifier.to_string(),
        phase: ResolvePhase::ProviderGraph,
        kind: ResolveRequestKind::EsmImport,
    }
}

#[test]
fn resolution_witness_positive_retains_every_precedence_guard_and_both_recovery_chains() {
    let resolver = ModuleResolverCore::new(Vec::new());
    let reader =
        TraceReader::new(&["/p/mod.tsx"]).with_realpath("/p/mod.tsx", "/store/pkg/mod.tsx");

    let result = resolver
        .resolve_with_reader(&reader, &request("./mod.js"))
        .expect("the lower-priority .tsx source sibling must resolve");
    assert_eq!(result.source_id, "/store/pkg/mod.tsx");
    assert_eq!(result.resolution_kind, ResolutionKind::Relative);

    let witness = retained_witness(&reader.observations());
    assert!(
        witness.contains(&WitnessFact::PathProbe {
            canonical: "/p/mod.ts".to_string(),
            outcome: ProbeOutcome::Absent,
        }),
        "the absent higher-priority .ts candidate must be retained; recording \
         only the selected .tsx target would serve a stale positive after \
         /p/mod.ts appears"
    );
    assert!(witness.contains(&WitnessFact::PathProbe {
        canonical: "/p/mod.tsx".to_string(),
        outcome: ProbeOutcome::File,
    }));
    assert!(witness.contains(&WitnessFact::Realpath {
        requested: "/p/mod.tsx".to_string(),
        resolved: Some("/store/pkg/mod.tsx".to_string()),
    }));
    for prefix in ["/p", "/store", "/store/pkg"] {
        assert!(
            witness.contains(&WitnessFact::RecoveryScope {
                canonical_prefix: prefix.to_string(),
            }),
            "missing requested/resolved ancestor recovery fact {prefix}"
        );
    }
}

#[test]
fn resolution_witness_miss_retains_the_complete_exhausted_probe_set() {
    let resolver = ModuleResolverCore::new(Vec::new());
    let reader = TraceReader::new(&[]);

    assert!(resolver
        .resolve_with_reader(&reader, &request("./missing"))
        .is_none());

    let observed_probes = reader
        .observations()
        .into_iter()
        .filter_map(|observation| match observation {
            ResolverObservation::Probe { requested, outcome } => {
                assert_eq!(
                    outcome,
                    ProbeOutcome::Absent,
                    "the miss fixture must contain no candidate"
                );
                Some(requested)
            }
            ResolverObservation::Realpath { .. } => None,
        })
        .collect::<Vec<_>>();
    let expected = [
        "/p/missing.ts",
        "/p/missing.tsx",
        "/p/missing.js",
        "/p/missing.jsx",
        "/p/missing.mts",
        "/p/missing.mjs",
        "/p/missing.cts",
        "/p/missing.cjs",
        "/p/missing.vue",
        "/p/missing.d.ts",
        "/p/missing.d.mts",
        "/p/missing.d.cts",
        "/p/missing/index.ts",
        "/p/missing/index.tsx",
        "/p/missing/index.js",
        "/p/missing/index.jsx",
        "/p/missing/index.mts",
        "/p/missing/index.mjs",
        "/p/missing/index.cts",
        "/p/missing/index.cjs",
        "/p/missing/index.vue",
        "/p/missing/index.d.ts",
        "/p/missing/index.d.mts",
        "/p/missing/index.d.cts",
    ]
    .map(str::to_string);
    assert_eq!(
        observed_probes, expected,
        "a miss witness must retain every exhausted candidate in precedence order"
    );

    let witness = retained_witness(
        &observed_probes
            .iter()
            .map(|requested| ResolverObservation::Probe {
                requested: requested.clone(),
                outcome: ProbeOutcome::Absent,
            })
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        witness
            .iter()
            .filter(|fact| matches!(fact, WitnessFact::PathProbe { .. }))
            .count(),
        expected.len(),
        "no exhausted miss probe may be dropped from the retained witness"
    );
    assert!(witness.contains(&WitnessFact::RecoveryScope {
        canonical_prefix: "/p/missing".to_string(),
    }));
    assert!(!witness.contains(&WitnessFact::RecoveryScope {
        canonical_prefix: "/p/missing2".to_string(),
    }));
}
