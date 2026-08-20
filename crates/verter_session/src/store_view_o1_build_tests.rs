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

const DEP_ID: &str = "/proj/dep.ts";
const DEP_SRC: &str = "export const d = 1\nexport interface D { x: number }\n";
const MEMBER_SRC: &str =
    "import { d, type D } from './dep'\nexport const use = d\nexport type R = D\n";

/// Host sizes the O(1) claim is measured across. A 12x span: any surviving
/// per-owner term shows up as a 12x cost, not as noise.
const HOST_SIZES: [usize; 3] = [250, 1000, 3000];

fn upsert_ts(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap_or_else(|err| panic!("upsert {id} must succeed: {err:?}"));
}

fn member_id(i: usize) -> String {
    format!("/proj/f{i}.ts")
}

/// A host holding `n` already-materialised `.ts` files, each with a real
/// cross-file import edge — the state the eager builder had to walk.
fn host_with_n_materialized_files(n: usize) -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert_ts(&host, DEP_ID, DEP_SRC);
    for i in 0..n {
        upsert_ts(&host, &member_id(i), MEMBER_SRC);
    }
    let _ = host.ensure_indexed_ready_serve(DEP_ID);
    for i in 0..n {
        let _ = host.ensure_indexed_ready_serve(&member_id(i));
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
    host
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
        let view = freshly_built_view(&host);
        assert_eq!(
            view.memo_len_for_tests(),
            0,
            "host size {n}: a build must resolve ZERO canonicals — it captures \
             roots and nothing else. A per-owner term would show up here as a \
             count proportional to {n}."
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

/// Deterministic companion to the structural assertion above: the build
/// must not touch a SINGLE owner through its captured roots, at any host
/// size. This used to be measured as a wall-clock ratio (a 12x host span
/// must not produce a ~12x build), but `Instant`-based timing flakes under
/// parallel/loaded test execution and correctness tests in this crate must
/// be deterministic — wall-clock budgets belong in `verter_bench`.
///
/// `store_view_owner_visits` (`crate::store_view_roots`) is a THREAD-LOCAL
/// counter that increments on every read through a view's captured roots
/// while a store-view BUILD scope is active — the exact production build
/// path (`HostStoreView::build` enters the scope itself), not a test-only
/// variant. It is proven to have a live producer and to be scope-gated
/// (counts build-time reads, not demand-time ones) by
/// `store_view_marginal_admit_tests::the_owner_visit_counter_moves_only_inside_a_build_scope`.
/// A restored per-owner term in the builder — even one that copies data
/// without going through `memo_len_for_tests` — visits at least one owner
/// per already-materialised file and moves this counter by N; a correct
/// O(1) build moves it by exactly zero, at every host size.
#[test]
fn store_view_build_touches_no_owner_at_any_host_size() {
    for n in HOST_SIZES {
        let host = host_with_n_materialized_files(n);
        host.bump_store_view_epoch();
        crate::store_view_roots::reset_store_view_owner_visits();
        let view = host.resolver_store_view_read().into_owned_view();
        let visits = crate::store_view_roots::store_view_owner_visits();
        assert_eq!(
            visits, 0,
            "host size {n}: the build touched {visits} owner(s) through its \
             captured roots. A build must be a fixed number of scalar reads \
             and `Arc` clones — any owner visit means a per-owner term is \
             back, and a build that does this at N={n} would do it at every \
             host size the O(1) contract is measured across."
        );
        drop(view);
    }
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
