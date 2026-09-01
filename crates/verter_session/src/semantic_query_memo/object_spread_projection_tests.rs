use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use super::family::family_and_slot;
use super::SemanticGraphStore;
use crate::semantic_query::object_spread_projection::test_support;
use crate::semantic_query::{
    DepSignature, DepVersion, ExactOptionalPropertyPolicy, MemberMergeRole,
    ObjectProjectionSelector, ProjectionMode, ProjectionReductionContext, PropertyKey, QueryError,
    QueryResult, SemanticNodeId, SemanticQueryKey, SemanticQueryValue, SemanticQueryValueTag,
    SubstitutionCanonicalHash, SurfaceProvenanceContext,
};
use crate::{HostConfig, VerterHost};

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn context(mode: ProjectionMode) -> crate::semantic_query::ObjectSpreadProjectionContext {
    test_support::context(
        ProjectionReductionContext::published(mode),
        [1; 16],
        [2; 16],
        [3; 16],
        [4; 16],
        SubstitutionCanonicalHash::distinct_for_test(1),
        ExactOptionalPropertyPolicy::Enabled,
    )
}

fn query(
    selector: ObjectProjectionSelector,
    context: crate::semantic_query::ObjectSpreadProjectionContext,
) -> SemanticQueryKey {
    SemanticQueryKey::ProjectObjectSpread {
        program: SemanticNodeId(41),
        selector,
        context,
    }
}

fn formula() -> crate::semantic_query::ObjectProjectionFormula {
    test_support::closed_formula([test_support::closed_alternative([])])
}

fn dep(canonical: &str, seed: u8) -> DepSignature {
    Arc::from([(
        Arc::<str>::from(canonical),
        DepVersion::WholeHash([seed; 16]),
    )])
}

fn empty_dep() -> DepSignature {
    Arc::from([])
}

fn join_within<T: Send + 'static>(handle: thread::JoinHandle<T>, label: &str) -> T {
    let (tx, rx) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let _ = tx.send(handle.join());
    });
    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(value)) => value,
        Ok(Err(_)) => panic!("{label} panicked"),
        Err(_) => panic!("{label} did not complete"),
    }
}

#[test]
fn family_retains_selector_and_every_non_slot_context_axis() {
    let base_context = context(ProjectionMode::Shallow);
    let base = query(ObjectProjectionSelector::Surface, base_context);
    let (base_family, base_slot) = family_and_slot(&base);

    let selector_mutation = query(
        ObjectProjectionSelector::Key(PropertyKey::identifier("x")),
        base_context,
    );
    assert_ne!(family_and_slot(&selector_mutation).0, base_family);
    let program_mutation = SemanticQueryKey::ProjectObjectSpread {
        program: SemanticNodeId(42),
        selector: ObjectProjectionSelector::Surface,
        context: base_context,
    };
    assert_ne!(family_and_slot(&program_mutation).0, base_family);

    let mode_mutation = query(
        ObjectProjectionSelector::Surface,
        context(ProjectionMode::Expanded),
    );
    let (mode_family, mode_slot) = family_and_slot(&mode_mutation);
    assert_eq!(mode_family, base_family, "mode is an established slot axis");
    assert_ne!(mode_slot, base_slot);

    let mut reduction = ProjectionReductionContext::published(ProjectionMode::Shallow);
    reduction.provenance = SurfaceProvenanceContext::MacroTypeArgOwnBody;
    let provenance_mutation = query(
        ObjectProjectionSelector::Surface,
        test_support::context(
            reduction,
            [1; 16],
            [2; 16],
            [3; 16],
            [4; 16],
            SubstitutionCanonicalHash::distinct_for_test(1),
            ExactOptionalPropertyPolicy::Enabled,
        ),
    );
    assert_ne!(family_and_slot(&provenance_mutation).0, base_family);

    reduction.provenance = SurfaceProvenanceContext::Structural;
    reduction.merge_role = MemberMergeRole::Heritage;
    let merge_role_mutation = query(
        ObjectProjectionSelector::Surface,
        test_support::context(
            reduction,
            [1; 16],
            [2; 16],
            [3; 16],
            [4; 16],
            SubstitutionCanonicalHash::distinct_for_test(1),
            ExactOptionalPropertyPolicy::Enabled,
        ),
    );
    assert_ne!(family_and_slot(&merge_role_mutation).0, base_family);

    let mutations = [
        test_support::context(
            ProjectionReductionContext::published(ProjectionMode::Shallow),
            [9; 16],
            [2; 16],
            [3; 16],
            [4; 16],
            SubstitutionCanonicalHash::distinct_for_test(1),
            ExactOptionalPropertyPolicy::Enabled,
        ),
        test_support::context(
            ProjectionReductionContext::published(ProjectionMode::Shallow),
            [1; 16],
            [9; 16],
            [3; 16],
            [4; 16],
            SubstitutionCanonicalHash::distinct_for_test(1),
            ExactOptionalPropertyPolicy::Enabled,
        ),
        test_support::context(
            ProjectionReductionContext::published(ProjectionMode::Shallow),
            [1; 16],
            [2; 16],
            [9; 16],
            [4; 16],
            SubstitutionCanonicalHash::distinct_for_test(1),
            ExactOptionalPropertyPolicy::Enabled,
        ),
        test_support::context(
            ProjectionReductionContext::published(ProjectionMode::Shallow),
            [1; 16],
            [2; 16],
            [3; 16],
            [9; 16],
            SubstitutionCanonicalHash::distinct_for_test(1),
            ExactOptionalPropertyPolicy::Enabled,
        ),
        test_support::context(
            ProjectionReductionContext::published(ProjectionMode::Shallow),
            [1; 16],
            [2; 16],
            [3; 16],
            [4; 16],
            SubstitutionCanonicalHash::distinct_for_test(9),
            ExactOptionalPropertyPolicy::Enabled,
        ),
        test_support::context(
            ProjectionReductionContext::published(ProjectionMode::Shallow),
            [1; 16],
            [2; 16],
            [3; 16],
            [4; 16],
            SubstitutionCanonicalHash::distinct_for_test(1),
            ExactOptionalPropertyPolicy::Disabled,
        ),
    ];
    for mutation in mutations {
        assert_ne!(
            family_and_slot(&query(ObjectProjectionSelector::Surface, mutation)).0,
            base_family
        );
    }
}

#[test]
fn equal_selector_and_context_share_one_singleflight() {
    let store = Arc::new(SemanticGraphStore::new());
    let key = query(
        ObjectProjectionSelector::Surface,
        context(ProjectionMode::Identity),
    );
    let builds = Arc::new(AtomicUsize::new(0));
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);

    let owner_store = Arc::clone(&store);
    let owner_key = key.clone();
    let owner_builds = Arc::clone(&builds);
    let owner = thread::spawn(move || {
        let ctx = host();
        owner_store.execute_cooperative_value(
            &ctx,
            owner_key,
            || SemanticNodeId(900),
            || {
                owner_builds.fetch_add(1, Ordering::SeqCst);
                started_tx.send(()).expect("signal owner start");
                release_rx.recv().expect("release owner");
                (
                    QueryResult::Value(SemanticQueryValue::ObjectProjection(formula())),
                    empty_dep(),
                )
            },
        )
    });
    started_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("owner must start");

    let joiner_store = Arc::clone(&store);
    let joiner_key = key.clone();
    let joiner_builds = Arc::clone(&builds);
    let joiner = thread::spawn(move || {
        let ctx = host();
        joiner_store.execute_cooperative_value(
            &ctx,
            joiner_key,
            || SemanticNodeId(901),
            || {
                joiner_builds.fetch_add(1, Ordering::SeqCst);
                (
                    QueryResult::Value(SemanticQueryValue::ObjectProjection(formula())),
                    empty_dep(),
                )
            },
        )
    });

    let deadline = Instant::now() + Duration::from_secs(10);
    while store.test_inflight_strong_count(&key) < 4 && Instant::now() < deadline {
        thread::yield_now();
    }
    assert!(
        store.test_inflight_strong_count(&key) >= 4,
        "second caller must join the projection family flight"
    );
    release_tx.send(()).expect("release owner");

    let owner_read = join_within(owner, "projection owner");
    let joiner_read = join_within(joiner, "projection joiner");
    assert!(matches!(
        owner_read.value,
        QueryResult::Value(SemanticQueryValue::ObjectProjection(_))
    ));
    assert!(matches!(
        joiner_read.value,
        QueryResult::Value(SemanticQueryValue::ObjectProjection(_))
    ));
    assert_eq!(builds.load(Ordering::SeqCst), 1);
}

#[test]
fn selector_specific_invalidation_evicts_only_the_dependent_projection() {
    let store = SemanticGraphStore::new();
    let ctx = host();
    let surface = query(
        ObjectProjectionSelector::Surface,
        context(ProjectionMode::Identity),
    );
    let selected = query(
        ObjectProjectionSelector::Key(PropertyKey::identifier("x")),
        context(ProjectionMode::Identity),
    );

    for (key, canonical) in [(&surface, "/w/surface.ts"), (&selected, "/w/key.ts")] {
        let _ = store.execute_cooperative_value(
            &ctx,
            key.clone(),
            || SemanticNodeId(902),
            || {
                (
                    QueryResult::Value(SemanticQueryValue::ObjectProjection(formula())),
                    dep(canonical, 1),
                )
            },
        );
    }
    assert_eq!(store.memo_entry_count(), 2);
    assert_eq!(store.invalidate_canonical("/w/surface.ts"), 1);

    let surface_rebuilds = AtomicUsize::new(0);
    let key_rebuilds = AtomicUsize::new(0);
    let _ = store.execute_cooperative_value(
        &ctx,
        surface,
        || SemanticNodeId(903),
        || {
            surface_rebuilds.fetch_add(1, Ordering::SeqCst);
            (
                QueryResult::Value(SemanticQueryValue::ObjectProjection(formula())),
                dep("/w/surface.ts", 2),
            )
        },
    );
    let _ = store.execute_cooperative_value(
        &ctx,
        selected,
        || SemanticNodeId(904),
        || {
            key_rebuilds.fetch_add(1, Ordering::SeqCst);
            (
                QueryResult::Value(SemanticQueryValue::ObjectProjection(formula())),
                dep("/w/key.ts", 2),
            )
        },
    );
    assert_eq!(surface_rebuilds.load(Ordering::SeqCst), 1);
    assert_eq!(key_rebuilds.load(Ordering::SeqCst), 0);
}

#[test]
fn same_path_projection_recursion_never_publishes_a_placeholder() {
    let store = SemanticGraphStore::new();
    let ctx = host();
    let key = query(
        ObjectProjectionSelector::Surface,
        context(ProjectionMode::Identity),
    );
    let nested_builds = AtomicUsize::new(0);

    let read = store.execute_cooperative_value(
        &ctx,
        key.clone(),
        || SemanticNodeId(905),
        || {
            let nested = store.execute_cooperative_value(
                &ctx,
                key,
                || SemanticNodeId(906),
                || {
                    nested_builds.fetch_add(1, Ordering::SeqCst);
                    (QueryResult::Error(QueryError::Miss), empty_dep())
                },
            );
            (nested.value, empty_dep())
        },
    );
    assert!(matches!(
        read.value,
        QueryResult::Recursive(SemanticNodeId(906))
    ));
    assert_eq!(nested_builds.load(Ordering::SeqCst), 0);
    assert_eq!(
        store.memo_entry_count(),
        0,
        "recursive placeholders must remain return-only"
    );
}

#[test]
fn projection_family_rejects_and_does_not_publish_a_type_node_payload() {
    let store = SemanticGraphStore::new();
    let ctx = host();
    let key = query(
        ObjectProjectionSelector::Surface,
        context(ProjectionMode::Identity),
    );
    let read = store.execute_cooperative_value(
        &ctx,
        key,
        || SemanticNodeId(907),
        || {
            (
                QueryResult::Value(SemanticQueryValue::TypeNode(SemanticNodeId(908))),
                empty_dep(),
            )
        },
    );
    assert!(matches!(
        read.value,
        QueryResult::Error(QueryError::ValueDomainMismatch {
            expected: SemanticQueryValueTag::ObjectProjection,
            actual: SemanticQueryValueTag::TypeNode,
        })
    ));
    assert_eq!(store.memo_entry_count(), 0);
}

fn formula_with_key(value: SemanticNodeId) -> crate::semantic_query::ObjectProjectionFormula {
    test_support::closed_formula([test_support::closed_alternative([
        test_support::positive_key(
            PropertyKey::identifier("x"),
            crate::semantic_query::PositiveKeyPresence::Required,
            crate::semantic_query::ProjectionEvidence::Proven(value),
        ),
    ])])
}

#[test]
fn cross_view_projection_joiner_forks_when_winner_carrier_fails_follower_validation() {
    use crate::fact_signature_helpers::ReadSetSignature;
    use crate::resolver_core::{FactVersionRef, SessionResolverContext};
    use crate::session_view::OverlaidViewRef;
    use rustc_hash::FxHashMap;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicBool, Ordering};

    let keyed_canonical = "/p10/keyed.ts";
    let host = host();
    let _ = host
        .upsert(crate::UpsertRequest {
            canonical_id: Some(keyed_canonical.to_string()),
            input_id: keyed_canonical.to_string(),
            source: Arc::from("export interface Keyed { base: number; }\n"),
            file_language: crate::LanguageRegistry::global()
                .classify_static(keyed_canonical)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .expect("upsert of the keyed file succeeds");
    let base_hash = host
        .ensure_indexed_ready(keyed_canonical)
        .expect("keyed-file base IndexedReady must materialise")
        .whole_hash;
    let host = Arc::new(host);

    let store = Arc::new(SemanticGraphStore::new());
    let key = query(
        ObjectProjectionSelector::Key(PropertyKey::identifier("x")),
        context(ProjectionMode::Identity),
    );

    let winner_fact = FactVersionRef::FileWholeHash {
        canonical_id: keyed_canonical.to_string(),
        hash: base_hash,
    };
    let base_value = store.intern_node(crate::semantic_query::SemanticNodeData::Primitive(
        crate::semantic_query::PrimitiveKind::Number,
    ));
    let overlay_value = store.intern_node(crate::semantic_query::SemanticNodeData::Primitive(
        crate::semantic_query::PrimitiveKind::String,
    ));

    let (tx_winner_in_build, rx_winner_in_build) = mpsc::channel::<()>();
    let (tx_release_winner, rx_release_winner) = mpsc::channel::<()>();

    let winner_store = Arc::clone(&store);
    let winner_host = Arc::clone(&host);
    let winner_key = key.clone();
    let winner = thread::spawn(move || {
        let host: &dyn crate::resolver_core::ResolverContext = winner_host.as_ref();
        winner_store.execute_cooperative_value(
            host,
            winner_key,
            || SemanticNodeId(910),
            || {
                tx_winner_in_build
                    .send(())
                    .expect("winner: signal in-build");
                rx_release_winner.recv().expect("winner: released");
                let carrier = ReadSetSignature::new(Arc::from(vec![winner_fact.clone()]));
                crate::project_semantic_dispatch::walk::QueryBuildOutput {
                    result: QueryResult::Value(SemanticQueryValue::ObjectProjection(
                        formula_with_key(base_value),
                    )),
                    dep_signature: Arc::from(Vec::new().into_boxed_slice()),
                    walker_diagnostics: Vec::new(),
                    cache_suppress: false,
                    result_is_partial: false,
                    partial_reasons: crate::semantic_query::PartialReasonSet::empty(),
                    taint: crate::semantic_query::ResultTaint::Clean,
                    observed_self_roots: Vec::new(),
                    graph_carrier: Some(Box::new(carrier)),
                    self_root_canonicals: Arc::from([Arc::<str>::from(keyed_canonical)]),
                    pending_prefix_backfills: Vec::new(),
                    satisfied_projection: crate::semantic_query::demand::MaterializedSet::empty(),
                    flow_completion: None,
                }
            },
        )
    });
    rx_winner_in_build.recv().expect("winner entered build");

    let follower_cold_ran = Arc::new(AtomicBool::new(false));
    let follower_store = Arc::clone(&store);
    let follower_host = Arc::clone(&host);
    let follower_key = key.clone();
    let follower_flag = Arc::clone(&follower_cold_ran);
    let follower = thread::spawn(move || {
        let overlay_hash: crate::types::Hash16 = [0xA5u8; 16];
        assert_ne!(overlay_hash, base_hash);
        let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
        overlays.insert(
            keyed_canonical.to_string(),
            Arc::from("export interface Keyed { overlaid: string; }\n"),
        );
        let mut overlay_hashes: FxHashMap<String, crate::types::Hash16> = FxHashMap::default();
        overlay_hashes.insert(keyed_canonical.to_string(), overlay_hash);
        let tombstones: HashSet<String> = HashSet::new();
        let view = OverlaidViewRef::new(
            follower_host.as_ref(),
            &overlays,
            &overlay_hashes,
            &tombstones,
        );
        let session_store_view = follower_host
            .as_ref()
            .resolver_store_view_read()
            .into_owned_view()
            .with_session_overlay(follower_host.as_ref(), &view);
        let session_ctx = SessionResolverContext::new(
            follower_host.as_ref(),
            &view,
            &session_store_view,
            std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
        );
        let read = follower_store.execute_cooperative_value(
            &session_ctx,
            follower_key,
            || SemanticNodeId(911),
            || {
                follower_flag.store(true, Ordering::SeqCst);
                (
                    QueryResult::Value(SemanticQueryValue::ObjectProjection(formula_with_key(
                        overlay_value,
                    ))),
                    empty_dep(),
                )
            },
        );
        read.value
    });

    let deadline = Instant::now() + Duration::from_secs(10);
    while store.test_inflight_strong_count(&key) < 4 && Instant::now() < deadline {
        thread::yield_now();
    }
    assert!(
        store.test_inflight_strong_count(&key) >= 4,
        "the overlay follower must join the in-flight projection"
    );
    tx_release_winner.send(()).expect("release winner");

    let winner_read = join_within(winner, "projection base winner");
    let follower_value = join_within(follower, "projection overlay follower");

    assert!(
        follower_cold_ran.load(Ordering::SeqCst),
        "the winner's base-rooted carrier must not validate under the \
         follower's overlay view — the follower must fork and recompute"
    );
    let key_fact = |value: &QueryResult<SemanticQueryValue>| match value {
        QueryResult::Value(SemanticQueryValue::ObjectProjection(formula)) => {
            match formula.alternatives()[0].selected_key(&PropertyKey::identifier("x")) {
                crate::semantic_query::OpenSafeKeyEvidence::Positive(fact) => fact.value().clone(),
                other => panic!("expected positive x, got {other:?}"),
            }
        }
        other => panic!("expected an object projection, got {other:?}"),
    };
    assert!(matches!(
        key_fact(&winner_read.value),
        crate::semantic_query::ProjectionEvidence::Proven(node) if node == base_value
    ));
    assert!(matches!(
        key_fact(&follower_value),
        crate::semantic_query::ProjectionEvidence::Proven(node) if node == overlay_value
    ));
}
