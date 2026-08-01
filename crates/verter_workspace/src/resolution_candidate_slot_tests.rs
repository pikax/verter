//! One owner-edge candidate authority: bounded multi-candidate slot,
//! caller-supplied exact results, and the observer scope.
//!
//! These pin the resolve-domain store contract that the caller-supplied
//! (exact-resolution) producer, the resolver producer, and the
//! cache-validation oracle all share:
//!
//! - an exact-table result publishes and is reused through the SAME slot
//!   as a resolver-derived result — the exact lookup is a cold compute of
//!   the one authority, not a bypass of it;
//! - a superseded candidate stays retained (bounded, FIFO) so the demand
//!   that supersedes it can still name the witness it rejected;
//! - the slot never grows past the shared per-slot candidate cap.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use parking_lot::RwLock;

use super::Engine;
use crate::resolution_currency::{PathProbe, ResolutionPopulation};
use crate::resolver::normalize_canonical_id;
use crate::traits::{WorkspaceAccess, WorkspaceRead};
use crate::types::{
    ExactResolution, ExactResolutionResult, ParsedEdge, ResolutionContext, ResolvePhase,
    ResolveRequestKind,
};

const CONTEXT: ResolutionContext = ResolutionContext {
    phase: ResolvePhase::ProviderGraph,
    kind: ResolveRequestKind::EsmImport,
};

const IMPORTER: &str = "/p/main.ts";
const SPECIFIER: &str = "./dep";
const RESOLVER_TARGET: &str = "/p/dep.ts";
const EXACT_A: &str = "/p/a.ts";
const EXACT_B: &str = "/p/b.ts";
const EXACT_C: &str = "/p/c.ts";
const EXACT_D: &str = "/p/d.ts";
const EXACT_E: &str = "/p/e.ts";

struct SlotReader {
    files: RwLock<HashMap<String, Arc<str>>>,
}

impl SlotReader {
    fn new() -> Self {
        let mut files = HashMap::new();
        for path in [
            IMPORTER,
            RESOLVER_TARGET,
            EXACT_A,
            EXACT_B,
            EXACT_C,
            EXACT_D,
            EXACT_E,
        ] {
            files.insert(path.to_string(), Arc::from("export const x = 1\n"));
        }
        Self {
            files: RwLock::new(files),
        }
    }
}

impl WorkspaceRead for SlotReader {
    fn read_file(&self, canonical_id: &str) -> Option<Arc<str>> {
        self.files
            .read()
            .get(&normalize_canonical_id(canonical_id))
            .cloned()
    }

    fn file_exists(&self, canonical_id: &str) -> bool {
        matches!(self.probe_path(canonical_id), PathProbe::File)
    }

    fn probe_path(&self, canonical_id: &str) -> PathProbe {
        if self
            .files
            .read()
            .contains_key(&normalize_canonical_id(canonical_id))
        {
            PathProbe::File
        } else {
            PathProbe::Absent
        }
    }

    fn resolution_event_bridge_complete(&self) -> bool {
        true
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        let normalized = normalize_canonical_id(canonical_id);
        self.files
            .read()
            .contains_key(&normalized)
            .then_some(normalized)
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

impl WorkspaceAccess for SlotReader {
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

fn engine_for(root: &str) -> Engine {
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

fn set_exact(engine: &Engine, target: Option<&str>) {
    let resolutions = target
        .map(|target| {
            vec![ExactResolution {
                specifier: SPECIFIER.to_string(),
                phase: CONTEXT.phase,
                kind: CONTEXT.kind,
                resolved_canonical_id: Some(target.to_string()),
                possible_canonical_ids: vec![target.to_string()],
            }]
        })
        .unwrap_or_default();
    engine.set_exact_resolutions(IMPORTER, resolutions);
}

fn resolve(engine: &Engine, reader: &SlotReader) -> crate::resolution_currency::ResolutionOutcome {
    engine.resolve_import_outcome(reader, IMPORTER, SPECIFIER, CONTEXT)
}

fn slot_len(engine: &Engine) -> usize {
    engine.lazy_resolution_slot_len_for_test(
        IMPORTER,
        SPECIFIER,
        CONTEXT,
        ResolutionPopulation::Base,
    )
}

#[test]
fn exact_result_publishes_and_is_reused_through_the_one_candidate_slot() {
    let engine = engine_for("/p");
    let reader = SlotReader::new();
    set_exact(&engine, Some(EXACT_A));

    let cold = resolve(&engine, &reader);
    let warm = resolve(&engine, &reader);

    assert_eq!(
        (
            cold.result().map(|r| r.source_id.as_str()),
            cold.trace().published(),
            cold.trace().reused(),
        ),
        (Some(EXACT_A), true, false),
        "a caller-supplied exact result must be a cold compute of the one \
         owner-edge authority and publish a candidate"
    );
    assert_eq!(
        (
            warm.result().map(|r| r.source_id.as_str()),
            warm.trace().published(),
            warm.trace().reused(),
        ),
        (Some(EXACT_A), false, true),
        "the next demand must reuse the published exact candidate instead of \
         bypassing the slot"
    );
}

#[test]
fn superseded_candidate_witness_survives_in_the_bounded_slot() {
    let engine = engine_for("/p");
    let reader = SlotReader::new();

    // Resolver-derived candidate first, then two exact retargets.
    let first = resolve(&engine, &reader);
    assert_eq!(
        first.result().map(|r| r.source_id.as_str()),
        Some(RESOLVER_TARGET),
        "precondition: the resolver-derived leg must resolve"
    );
    set_exact(&engine, Some(EXACT_A));
    let _retarget = resolve(&engine, &reader);
    set_exact(&engine, Some(EXACT_B));
    let second_retarget = resolve(&engine, &reader);

    let rejected: Vec<Option<&str>> = second_retarget
        .trace()
        .rejected_exact_targets()
        .iter()
        .map(|target| target.as_deref())
        .collect();
    assert_eq!(
        second_retarget.result().map(|r| r.source_id.as_str()),
        Some(EXACT_B),
        "the second retarget must serve the new exact target"
    );
    assert!(
        rejected.contains(&Some(RESOLVER_TARGET)) && rejected.contains(&Some(EXACT_A)),
        "a superseded candidate must stay retained so the superseding demand \
         names every witness it rejected; got {rejected:?}"
    );
}

#[test]
fn slot_retains_at_most_the_shared_candidate_cap() {
    let engine = engine_for("/p");
    let reader = SlotReader::new();

    let _resolver_derived = resolve(&engine, &reader);
    for target in [EXACT_A, EXACT_B, EXACT_C, EXACT_D, EXACT_E] {
        set_exact(&engine, Some(target));
        let outcome = resolve(&engine, &reader);
        assert_eq!(
            outcome.result().map(|r| r.source_id.as_str()),
            Some(target),
            "each retarget must serve its own exact target"
        );
        assert!(
            slot_len(&engine) <= crate::CANDIDATE_CAP,
            "the slot must never exceed the shared per-slot candidate cap \
             ({}); got {}",
            crate::CANDIDATE_CAP,
            slot_len(&engine)
        );
    }
    assert_eq!(
        slot_len(&engine),
        crate::CANDIDATE_CAP,
        "six admissions must leave the slot saturated at the cap, not below it"
    );
}
