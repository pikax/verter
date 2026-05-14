//! Discriminating test: cross-thread joiner bubbles the winner's fact-dep
//! signature into the joiner thread's active tracer (Correction 1 of the
//! Stage 7 cutover plan).
//!
//! ## Discrimination contract
//!
//! This test FAILS unless **all** of the following are wired end-to-end:
//!
//! 1. The cold winner writes `state.fact_dep_signature` on the
//!    in-flight state BEFORE notifying joiners
//!    (`semantic_query_memo/mod.rs::execute_cooperative_slow`, step 6).
//! 2. The joiner path reads `state.fact_dep_signature` after waking
//!    (`semantic_query_memo/mod.rs::execute_cooperative_slow`, joiner branch).
//! 3. The joiner fans the signature out via `observe_fan_out_borrowed`
//!    BEFORE returning the `CacheRead` so the joiner thread's outer
//!    `with_fact_tracer` scope captures the winner's observations.
//!
//! Reverting any one of those three sites makes the joiner's outer
//! tracer finalise without the winner's fact, failing the
//! `joiner_outer_tracer_contains_winner_fact` assertion below.
//!
//! ## Driver shape
//!
//! - **Winner thread** enters `SemanticGraphStore::execute_cooperative`
//!   on the shared key `K`. Its build closure blocks on an mpsc channel
//!   until the joiner has had time to register as a waiter, then
//!   returns a `DepSignature` carrying one `WholeHash` entry. That
//!   entry is converted to a `FactVersionRef::FileWholeHash` via
//!   `component_meta_materialize::fact_signature_from_fence` (the same
//!   helper `warm_publish_one` uses) and written onto
//!   `state.fact_dep_signature` by the winner-write path.
//! - **Joiner thread** installs its own outer fact tracer, then enters
//!   `execute_cooperative` on the same key `K`. Because the winner has
//!   already claimed the in-flight entry, the joiner blocks on the
//!   condvar — exercising the joiner branch (not a same-thread
//!   recursion or warm-hit). When the joiner wakes, the joiner-bubble
//!   code path fans the winner's signature into the joiner's outer
//!   tracer. The outer tracer's finalised set must then contain the
//!   winner's fact.
//!
//! ## Why this could not be expressed pre-Correction-1
//!
//! Before the winner-write, `state.fact_dep_signature` was always
//! `None`, so the joiner's read always observed `None` and the
//! fan-out branch at the joiner-bubble site never executed. The
//! joiner's outer tracer would finalise empty under that buggy tree
//! even though the winner observed a non-empty signature.

use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use verter_session::for_tests::{install_fact_tracer_for_tests, SemanticGraphStore};
use verter_session::resolver_core::{FactReadSetFinalise, FactVersionRef};
use verter_session::semantic_query::{
    DepVersion, PrimitiveKind, QueryResult, ResolveDeclKey, ScopeId, SemanticNodeData,
    SemanticQueryKey,
};
use verter_session::VerterHost;

fn scope(canonical: &str) -> ScopeId {
    ScopeId {
        canonical_id: Arc::from(canonical),
        local_scope: None,
    }
}

/// Construct the cross-thread query key. Both threads target the same
/// key so the joiner path is reached.
fn shared_key() -> SemanticQueryKey {
    SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/cross_thread_joiner_bubbles_facts/site.ts"),
        name: Arc::from("Target"),
    })
}

/// Build the winner's fact: a `FileWholeHash` over a synthetic
/// canonical with a recognisable 16-byte pattern. The hash + canonical
/// pair appears (via `fact_signature_from_fence`) on `InflightState`
/// when the winner-write path fires, and reappears in the joiner's
/// outer tracer when the joiner-bubble path fans it out.
fn winner_fact() -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: "winner_dep.ts".to_string(),
        hash: [0x77u8; 16],
    }
}

/// `DepSignature` referencing one `WholeHash` entry on the
/// `winner_fact()` canonical. The `fact_signature_from_fence` helper
/// converts `WholeHash` -> `FileWholeHash` so the fact threads
/// end-to-end from the winner's build output to the joiner's outer
/// tracer.
fn winner_dep_signature() -> verter_session::semantic_query::DepSignature {
    Arc::from(
        [(
            Arc::<str>::from("winner_dep.ts"),
            DepVersion::WholeHash([0x77u8; 16]),
        )]
        .as_slice(),
    )
}

#[test]
fn joiner_outer_tracer_contains_winner_fact() {
    let store = Arc::new(SemanticGraphStore::new());
    let key = shared_key();

    // Channels coordinate the interleave. The winner waits inside its
    // build closure until the joiner has had time to register on the
    // in-flight entry; without this synchronisation the winner could
    // finish and warm-publish before the joiner enters
    // `execute_cooperative`, turning the would-be joiner into a
    // warm-hit fast-path caller and bypassing the joiner code path
    // we want to exercise.
    let (tx_winner_in_build, rx_winner_in_build) = mpsc::channel::<()>();
    let (tx_release_winner, rx_release_winner) = mpsc::channel::<()>();

    let winner_store = Arc::clone(&store);
    let winner_key = key.clone();
    let winner = thread::spawn(move || {
        winner_store.execute_cooperative(
            winner_key,
            || {
                winner_store.intern_node(SemanticNodeData::Opaque(
                    verter_session::semantic_query::QueryError::Miss,
                ))
            },
            || {
                // We have claimed the in-flight entry and are inside
                // the build closure. The joiner is free to enter and
                // will block on the condvar.
                tx_winner_in_build
                    .send(())
                    .expect("winner: signal in-build");
                // Wait for the test driver to release us once the
                // joiner has registered.
                rx_release_winner
                    .recv()
                    .expect("winner: released by driver");
                let id =
                    winner_store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                (QueryResult::Value(id), winner_dep_signature())
            },
        )
    });

    // Wait until the winner is inside its build closure (claimed,
    // before publish). This guarantees the joiner reaches the
    // condvar-wait branch rather than executing its own cold path.
    rx_winner_in_build.recv().expect("winner entered build");

    // Spawn the joiner thread. It installs its own outer fact tracer
    // BEFORE entering `execute_cooperative` so the joiner-bubble
    // fan-out has a tracer to deliver the winner's signature to.
    let joiner_store = Arc::clone(&store);
    let joiner_key = key.clone();
    let joiner = thread::spawn(move || {
        let host = VerterHost::new_standalone(Default::default());
        // Outer tracer scope spans the entire dispatch call so the
        // joiner-bubble target is the cell the joiner returns into.
        let ((), finalise) = install_fact_tracer_for_tests(&host, || {
            let cache_read = joiner_store.execute_cooperative(
                joiner_key,
                || {
                    joiner_store.intern_node(SemanticNodeData::Opaque(
                        verter_session::semantic_query::QueryError::Miss,
                    ))
                },
                || {
                    // This build MUST NOT run on the joiner — if it
                    // does, the test isn't exercising the joiner
                    // path. Returns a tagged sentinel so a failure
                    // mode would be observable.
                    let id = joiner_store
                        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
                    (
                        QueryResult::Value(id),
                        Arc::from([] as [(Arc<str>, DepVersion); 0]),
                    )
                },
            );
            // Return the cache_read out of the tracer scope so we can
            // confirm the joiner-side result identity.
            let _value = cache_read.value;
        });
        finalise
    });

    // Give the joiner time to register on the in-flight entry and
    // block on the condvar. 50 ms is the same heuristic used by
    // `audit_retry_counter_attributes_to_request.rs::
    // inflight_aborted_retry_attributes_to_per_request_context` for
    // the same scheduling concern. If a race ever surfaces under
    // load, the recommended fix is to expose
    // `inflight_table_contains_key` on the test surface and poll it;
    // for now the joined_waits assertion below catches a regression
    // where the joiner did not in fact wait.
    thread::sleep(Duration::from_millis(50));

    // Release the winner. It publishes the value + dep_signature,
    // writes `state.fact_dep_signature` on the in-flight state
    // (P0-A), and notifies the condvar. The joiner wakes, reads
    // `state.fact_dep_signature`, fans it out via
    // `observe_fan_out_borrowed`, and returns. The outer tracer
    // captures the winner's fact.
    tx_release_winner.send(()).expect("release winner");

    let _winner_read = winner.join().expect("winner joined");
    let joiner_finalise = joiner.join().expect("joiner joined");

    // The joiner-wait branch must have fired at least once. If this
    // counter is zero, the joiner never entered the wait branch and
    // the discrimination below would be vacuous.
    let snap = store.stats_snapshot();
    assert!(
        snap.joined_waits >= 1,
        "joiner must have hit the cooperative wait branch \
         (joined_waits={}); if this fails the joiner ran its own \
         cold build instead of entering the wait branch, and the \
         joiner-bubble code path was never exercised. Re-tune the \
         50 ms sleep or expose `inflight_table_contains_key` on the \
         test surface to poll deterministically.",
        snap.joined_waits
    );

    // The joiner's outer fact tracer must contain the winner's
    // fact. This is the discriminator: it FAILS if any of the three
    // sites listed in the module docstring regresses. In particular,
    // reverting the winner-write of `state.fact_dep_signature` at
    // `semantic_query_memo/mod.rs::execute_cooperative_slow` step 6
    // makes this assertion fail because the joiner's read returns
    // `None` and no fan-out occurs.
    let want = winner_fact();
    match joiner_finalise {
        FactReadSetFinalise::Ok(sig) => {
            assert!(
                sig.iter().any(|f| f == &want),
                "joiner thread's outer tracer must contain the \
                 winner's fact (got {sig:?}; expected to contain \
                 {want:?}). If this fails, either (a) the winner \
                 did not write state.fact_dep_signature at \
                 semantic_query_memo/mod.rs::execute_cooperative_slow \
                 step 6, (b) the joiner-bubble code path did not read \
                 it, or (c) the fan-out call did not deliver the fact \
                 to this thread's outer tracer."
            );
        }
        FactReadSetFinalise::Overflow => panic!("joiner outer tracer overflowed"),
    }
}
