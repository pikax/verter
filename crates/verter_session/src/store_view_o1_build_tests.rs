//! O(1) store-view build: the capture does no per-owner work, and the
//! sealed roots make the view answer for its OWN world even for a
//! canonical it had never been asked about at capture time.
//!
//! The build used to copy an answer for every owner the host knew into
//! per-canonical maps — six terms linear in host size, paid on every
//! keystroke that moved the validation token. Those terms are gone; every
//! per-canonical answer is now an exact point lookup through the captured
//! roots, resolved on first demand.
//!
//! Two properties are asserted here, and the second is the one that killed
//! an earlier lazy-capture attempt:
//!
//! 1. **The build resolves nothing.** A view's per-view memo counts the
//!    canonicals it has actually resolved. Straight after a build it is
//!    empty at every host size — the build performed zero per-owner work.
//!    An earlier eager build would have resolved N.
//! 2. **A lease is not a hash.** A view captured before a mutation still
//!    answers the PRE-mutation world for a dependency it had never
//!    observed. Deferring the read is only sound because the roots keep
//!    the old world reachable; a lazy read of live state would answer the
//!    new world and silently validate post-mutation facts against a
//!    pre-mutation view.
//!
//! The memo is a cost mechanism, not a correctness one, so it is used here
//! only as a WITNESS of how much per-canonical work happened — and every
//! assertion that a count is zero is paired with a control proving the
//! count moves when work does run.

use std::sync::Arc;

use crate::resolver_core::StoreView;
use crate::types::FileLanguage;
use crate::{HostConfig, UpsertRequest, VerterHost};
use verter_scheduler::stage::Priority;

const DEP_ID: &str = "/proj/dep.ts";
const DEP_SRC: &str = "export const d = 1\nexport interface D { x: number }\n";
const MEMBER_SRC: &str =
    "import { d, type D } from './dep'\nexport const use = d\nexport type R = D\n";

/// Host sizes the O(1) claim is measured across. A 12x span: any surviving
/// per-owner term shows up as a 12x cost, not as noise. Every fixture file is
/// virtual: its canonical ID names an in-memory `MemoryWorkspace` overlay and
/// its source is supplied directly through `UpsertRequest`; this suite performs
/// no fixture filesystem I/O.
const HOST_SIZES: [usize; 3] = [250, 1000, 3000];

fn ts_request(id: impl Into<String>, source: &str) -> UpsertRequest {
    UpsertRequest {
        canonical_id: None,
        input_id: id.into(),
        source: Arc::from(source),
        file_language: FileLanguage::script_ts(),
        aliases: Vec::new(),
    }
}

fn upsert_ts(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(ts_request(id, source))
        .unwrap_or_else(|err| panic!("upsert {id} must succeed: {err:?}"));
}

/// Admit a complete synthetic project through the host's production bulk
/// boundary: one atomic scheduler submission and one batch wait. The logical
/// `/proj/...` IDs are virtual paths; all source bytes stay in memory. Building
/// a bulk fixture from singleton `upsert` calls would test the same final host
/// state while redundantly paying one scheduler transaction per file.
fn upsert_materialized_project_sources(host: &VerterHost, n: usize) {
    let mut requests = Vec::with_capacity(n + 1);
    requests.push(ts_request(DEP_ID, DEP_SRC));
    requests.extend((0..n).map(|i| ts_request(member_id(i), MEMBER_SRC)));

    let outcomes = host.upsert_many_with_priority(requests, Priority::Interactive);
    assert_eq!(
        outcomes.len(),
        n + 1,
        "bulk fixture admission must report one outcome per source"
    );
    for outcome in outcomes {
        let canonical = outcome.canonical_id;
        let _ = outcome
            .result
            .unwrap_or_else(|err| panic!("bulk upsert {canonical} must succeed: {err:?}"));
    }
}

fn member_id(i: usize) -> String {
    format!("/proj/f{i}.ts")
}

/// A host holding `n` already-materialised `.ts` files, each with a real
/// cross-file import edge — the state the eager builder had to walk.
fn host_with_n_materialized_files(n: usize) -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert_materialized_project_sources(&host, n);
    // This suite measures store-view capture, not declaration-index parsing.
    // Seed the minimum current-content IndexedReady row directly from each
    // scheduler-authoritative whole hash. Production bulk admission above has
    // already parsed every source and published the real workspace edges; a
    // second serial `ensure_indexed_ready_serve` pass would benchmark an
    // unrelated materializer N times before the O(1) assertion starts.
    let seed_indexed = |canonical: &str| {
        let whole_hash = host
            .scheduler
            .try_get_source(canonical)
            .unwrap_or_else(|| panic!("bulk admission must retain {canonical}"))
            .whole_hash;
        let canonical: Arc<str> = Arc::from(canonical);
        let indexed = Arc::new(crate::project_type_store::IndexedReady::new_for_test(
            whole_hash,
        ));
        let key = crate::file_artifact_store::FileArtifactKey::for_indexed(
            Arc::clone(&canonical),
            &indexed,
            crate::file_artifact_store::BASE_PARSE_ENV_HASH,
        );
        host.project_type_store().indexed().insert_artifacts(
            key,
            Arc::new(crate::file_artifact_store::FileArtifacts::with_indexed(
                indexed,
            )),
        );
    };
    seed_indexed(DEP_ID);
    for i in 0..n {
        seed_indexed(&member_id(i));
    }
    // Precondition: without published artifacts there would be nothing for
    // a per-owner term to walk, and "no work happened" would be vacuous.
    assert!(
        host.project_type_store()
            .indexed()
            .get_any(&member_id(n - 1))
            .is_some(),
        "precondition: every preloaded member must be materialised, else \
         the zero-work assertions below prove nothing"
    );
    assert_eq!(
        host.ws().forward_deps_for(&member_id(n - 1)),
        vec![DEP_ID.to_string()],
        "precondition: the last owner must retain its real workspace import edge"
    );
    host
}

/// Bulk source admission must leave ordinary import resolution to the
/// workspace's batched parsed-edge transaction. `DependencyState` retains the
/// authored relative stem for bookkeeping; resolving that stem again through
/// `normalized_analysis_canonical` once per owner duplicates the workspace
/// resolver and makes this fixture scale with N resolver walks before the
/// store-view assertion even begins.
#[test]
fn bulk_fixture_admission_performs_no_per_owner_host_resolution() {
    let host = VerterHost::new_standalone(HostConfig::default());
    host.begin_resolution_currency_observation();

    upsert_materialized_project_sources(&host, 32);

    assert_eq!(
        host.take_resolution_currency_observations(),
        Vec::<crate::host_test_audit::ResolutionCurrencyObservation>::new(),
        "ordinary imports must be resolved by the one workspace parsed-edge batch, not once per owner through the host resolver"
    );
    assert_eq!(
        host.dependency_cache()
            .get(&member_id(0))
            .expect("bulk admission must publish dependency state")
            .dependencies,
        std::collections::BTreeSet::from(["/proj/dep".to_string()]),
        "DependencyState keeps the parse-derived joined stem; the workspace graph owns the resolved target"
    );
    assert_eq!(
        host.ws().forward_deps_for(&member_id(0)),
        vec![DEP_ID.to_string()],
        "the workspace batch must still publish the fully resolved dependency edge"
    );
}

/// Force a fresh build (the manager serves a token-stable view from cache
/// otherwise) and hand back the built view.
fn freshly_built_view(host: &VerterHost) -> crate::resolver_store::HostStoreView {
    host.bump_store_view_epoch();
    host.resolver_store_view_read().into_owned_view()
}

// ── 1. The build performs no per-owner work, at any host size ──

#[test]
fn store_view_build_resolves_zero_canonicals_at_any_host_size() {
    for n in HOST_SIZES {
        let host = host_with_n_materialized_files(n);
        host.bump_store_view_epoch();
        crate::store_view_roots::reset_store_view_owner_visits();
        let view = host.resolver_store_view_read().into_owned_view();
        assert_eq!(
            view.memo_len_for_tests(),
            0,
            "host size {n}: a build must resolve ZERO canonicals — it captures \
             roots and nothing else. A per-owner term would show up here as a \
             count proportional to {n}."
        );
        let visits = crate::store_view_roots::store_view_owner_visits();
        assert_eq!(
            visits, 0,
            "host size {n}: the build touched {visits} owner(s) through its \
             captured roots. A build must be a fixed number of scalar reads \
             and `Arc` clones — any owner visit means a per-owner term is \
             back, and a build that does this at N={n} would do it at every \
             host size the O(1) contract is measured across."
        );
    }
}

/// Anti-vacuity control for the assertion above: the witness has a
/// producer. Without this, "the count is zero" could mean "nothing ever
/// increments it" — the exact failure this program has already recorded
/// once.
#[test]
fn the_resolved_canonical_witness_moves_when_a_canonical_is_actually_resolved() {
    let host = host_with_n_materialized_files(4);
    let view = freshly_built_view(&host);
    assert_eq!(
        view.memo_len_for_tests(),
        0,
        "precondition: build resolved nothing"
    );

    let hash = view
        .whole_hash_for_tests(&member_id(0))
        .expect("a materialised member must resolve through the captured roots");
    assert_ne!(
        hash, [0u8; 16],
        "the resolved whole hash must be a real content hash, not a sentinel"
    );
    assert_eq!(
        view.memo_len_for_tests(),
        1,
        "resolving ONE canonical must move the witness by exactly one"
    );

    view.whole_hash_for_tests(&member_id(1));
    assert_eq!(
        view.memo_len_for_tests(),
        2,
        "each distinct canonical resolved contributes exactly one entry"
    );

    // Re-asking is memoized, not re-counted.
    view.whole_hash_for_tests(&member_id(1));
    assert_eq!(
        view.memo_len_for_tests(),
        2,
        "a repeated demand for the same canonical must not add a second entry"
    );
}

// ── 2. The captured roots answer for the view's own world ──

/// THE defect that killed the earlier lazy-capture attempt.
///
/// The view is captured, then the host mutates a canonical the view has
/// NOT yet been asked about, and only then is the view asked. Memoization
/// cannot save this: there is nothing memoized to return. The answer is
/// correct only because the source root is a LEASE — it still selects the
/// version that was current at the capture epoch.
///
/// A lazy read of live scheduler state would return the post-mutation
/// hash, and every fact recorded against the pre-mutation content would
/// then be validated against a view that silently describes the new world.
#[test]
fn view_answers_the_premutation_world_for_a_dependency_it_never_observed() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert_ts(&host, DEP_ID, DEP_SRC);
    upsert_ts(&host, &member_id(0), MEMBER_SRC);
    let _ = host.ensure_indexed_ready_serve(DEP_ID);
    let _ = host.ensure_indexed_ready_serve(&member_id(0));

    let before = host
        .ensure_indexed_ready(DEP_ID)
        .expect("dependency must materialise")
        .whole_hash;

    // Capture. Nothing is asked about `DEP_ID` through this view.
    let view = freshly_built_view(&host);
    assert_eq!(
        view.memo_len_for_tests(),
        0,
        "precondition: the view must not have resolved anything yet — otherwise \
         a memo hit, not the lease, would be under test"
    );

    // The host moves on.
    upsert_ts(
        &host,
        DEP_ID,
        "export const d = 2\nexport interface D { x: string }\n",
    );
    let after = host
        .ensure_indexed_ready(DEP_ID)
        .expect("mutated dependency must re-materialise")
        .whole_hash;
    assert_ne!(
        before, after,
        "the mutation must actually change the content version, else this proves nothing"
    );

    // FIRST observation of this canonical through the view — after the
    // mutation.
    let observed = view
        .whole_hash_for_tests(DEP_ID)
        .expect("the captured source root must still place the canonical");
    assert_eq!(
        observed, before,
        "a view must answer for the world it captured: the first observation of \
         a canonical after a mutation must resolve the PRE-mutation content, not \
         the live one"
    );
    assert_ne!(
        observed, after,
        "the view must not have read live scheduler state"
    );

    // And the fact rail agrees: the pre-mutation self-root validates, the
    // post-mutation one does not.
    assert!(
        view.validates_self_root_whole_hash(DEP_ID, &before),
        "a fact recorded against the captured world must validate under it"
    );
    assert!(
        !view.validates_self_root_whole_hash(DEP_ID, &after),
        "a fact recorded against the POST-mutation world must NOT validate under \
         a view captured before it"
    );
}

// ── 3. A point miss is a miss — nothing is enumerated to recover it ──

#[test]
fn a_point_miss_rejects_and_enumerates_nothing() {
    use verter_semantic::facts::registry::{FactLane, InternedName, SymbolSpace};
    use verter_semantic::facts::FactKey;
    use verter_type_expr::facts::FactPropertyKey;

    let host = host_with_n_materialized_files(250);
    let view = freshly_built_view(&host);
    const ABSENT: &str = "/proj/never-existed.ts";

    // Strict self-root: an untracked canonical rejects.
    assert!(
        !view.validates_self_root_whole_hash(ABSENT, &[7u8; 16]),
        "a self-root whole hash for a canonical the roots do not place must reject"
    );
    // Parse domain: a REAL observed hash for an unplaced canonical rejects
    // (only the zero sentinel — "the producer saw nothing" — is consistent
    // with absence).
    assert!(
        !StoreView::validates_parse_domain(
            &view,
            &crate::resolver_core::ParseFactRef {
                canonical_id: ABSENT.to_string(),
                key: FactKey::MemberPresence {
                    exporter: InternedName::from("Missing"),
                    name: FactPropertyKey::identifier("x"),
                    space: SymbolSpace::Type,
                },
                lane: FactLane::Semantic,
                expected_hash: [7u8; 16],
            },
        ),
        "a real parse-fact hash for a canonical the roots do not place must reject"
    );
    assert!(
        !view.tracks_file(ABSENT),
        "an unplaced canonical is not tracked"
    );

    // The whole miss cost EXACTLY ONE resolved canonical — the one asked
    // about. A fallback enumeration to "find" the canonical would have
    // resolved more than the single point that was demanded.
    assert_eq!(
        view.memo_len_for_tests(),
        1,
        "a point miss must resolve exactly the demanded canonical and nothing \
         else — any owner scan on the miss path would resolve more"
    );
}
