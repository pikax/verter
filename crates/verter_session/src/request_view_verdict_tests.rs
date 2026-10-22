//! A request view settles a validation answer only when the answer is a
//! function of what the view captured, and reuses it, valid or stale, only
//! in the exact context it was given in: the completion state and the
//! strict self-roots the validated evidence reaches.

use std::sync::Arc;

use verter_session_query::facts::fact_cache::{FactVersionRef, ResultReceipt};
use verter_session_query::facts::store_view::StoreView;

use crate::resolver_core::{CanonicalCompletionOverlay, RequestStoreView};
use crate::resolver_store::HostStoreView;
use crate::types::FileLanguage;
use crate::{HostConfig, UpsertRequest, VerterHost};

fn base_view() -> HostStoreView {
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/proj/button.ts".to_string(),
            source: Arc::from("export interface ButtonProps { label: string }\n"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert button.ts must succeed");
    host.resolver_store_view_read().into_owned_view()
}

fn whole(level: u8) -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: format!("/r/{level}.ts").into(),
        hash: [level; 16],
    }
}

/// `/r/0.ts` as an older version recorded it; the overlay holds `[0; 16]`.
fn stale_fact() -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: "/r/0.ts".into(),
        hash: [0xEE; 16],
    }
}

/// An overlay holding every `/r/{level}.ts` at its own version.
fn overlay_through(levels: u8) -> Arc<CanonicalCompletionOverlay> {
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    for level in 0..levels {
        overlay.insert_whole_hash_for_tests(&format!("/r/{level}.ts"), [level; 16]);
    }
    overlay
}

/// Level `n` consumed level `n - 1` and read `/r/{n}.ts`; level 0 read the
/// stale fact.
fn stale_chain(make: impl Fn(Vec<FactVersionRef>) -> ResultReceipt) -> Vec<ResultReceipt> {
    let mut levels = vec![make(vec![stale_fact()])];
    for level in 1..32u8 {
        let below = FactVersionRef::Receipt(levels.last().expect("a level below").clone());
        levels.push(make(vec![below, whole(level)]));
    }
    levels
}

fn reads(
    view: &RequestStoreView<'_>,
    signature: &[FactVersionRef],
    roots: &[&str],
) -> (Result<(), usize>, usize) {
    let before = view.leaf_validations_for_tests();
    let answer = view.validate_fact_signature(signature, roots);
    (answer, view.leaf_validations_for_tests() - before)
}

/// A fact that will not validate, deep under a chain of consumed results,
/// is read once per applicable context however many of its ancestors later
/// signatures reach it through: the failing walk settles every receipt it
/// proved reaches it, and a later walk stops at the first one it meets. A
/// strict root or a completion that can change what the receipts answer is
/// another context; a sibling the failing walk never finished is settled
/// neither way.
#[test]
fn a_deep_stale_fact_is_read_once_per_applicable_context() {
    let base = base_view();
    let overlay = overlay_through(33);
    let levels = stale_chain(ResultReceipt::new);
    let through = |level: usize| vec![FactVersionRef::Receipt(levels[level].clone())];
    let view = RequestStoreView::new(&base, Arc::clone(&overlay));

    assert_eq!(
        reads(&view, &through(31), &[]),
        (Err(0), 32),
        "the stale fact and every level's own fact above it"
    );
    assert_eq!(view.settled_receipts_for_tests(true), 32);
    for level in 0..32 {
        assert_eq!(
            reads(&view, &through(level), &[]),
            (Err(0), 0),
            "level {level} answers from its settled verdict"
        );
    }

    let sibling = ResultReceipt::new(vec![whole(32)]);
    let consumer = ResultReceipt::new(vec![
        FactVersionRef::Receipt(levels[17].clone()),
        FactVersionRef::Receipt(sibling.clone()),
    ]);
    assert_eq!(
        reads(&view, &[FactVersionRef::Receipt(consumer)], &[]),
        (Err(0), 0),
        "a new consumer of a stale level"
    );
    let sibling = [FactVersionRef::Receipt(sibling)];
    assert_eq!(
        reads(&view, &sibling, &[]),
        (Ok(()), 1),
        "the consumer's valid sibling was never proven stale"
    );
    assert_eq!(reads(&view, &sibling, &[]), (Ok(()), 0));

    // Levels 20 to 31 reach the strict root and are read again, down to the
    // first level below it, whose settled verdict still applies.
    assert_eq!(
        reads(&view, &through(31), &["/r/20.ts"]),
        (Err(0), 12),
        "a strict root the levels above 19 reach"
    );
    assert_eq!(reads(&view, &through(31), &["/r/20.ts"]), (Err(0), 0));
    assert_eq!(
        reads(&view, &through(31), &["/elsewhere.ts"]),
        (Err(0), 0),
        "a strict root no level reaches"
    );

    overlay.insert_whole_hash_for_tests("/moved.ts", [9; 16]);
    assert_eq!(
        reads(&view, &through(31), &[]),
        (Err(0), 32),
        "a completion moved the overlay"
    );
    assert_eq!(reads(&view, &through(31), &[]), (Err(0), 0));
    overlay.insert_whole_hash_for_tests("/r/0.ts", [0xEE; 16]);
    assert_eq!(
        reads(&view, &through(31), &[]),
        (Ok(()), 32),
        "a completion that restores the recorded version validates"
    );

    let fresh = RequestStoreView::new(&base, overlay_through(33));
    assert_eq!(
        reads(&fresh, &through(31), &[]),
        (Err(0), 32),
        "a new request"
    );
}

/// A retained signature answers a repeated probe from its settled verdict,
/// stale as well as valid, without reading its facts again; a borrowed
/// signature, which no cache retains, is read each time.
#[test]
fn a_retained_signature_answers_its_settled_verdict() {
    let base = base_view();
    let overlay = overlay_through(32);
    overlay.insert_whole_hash_for_tests("/stale.ts", [1; 16]);
    let mut facts: Vec<FactVersionRef> = (1..32).map(whole).collect();
    facts.push(FactVersionRef::FileWholeHash {
        canonical_id: "/stale.ts".into(),
        hash: [2; 16],
    });
    let stale: Arc<[FactVersionRef]> = Arc::from(facts);
    let valid: Arc<[FactVersionRef]> = (1..32).map(whole).collect();
    let view = RequestStoreView::new(&base, overlay);
    let probe = |signature: &Arc<[FactVersionRef]>| {
        let before = view.leaf_validations_for_tests();
        let answer = view.validates_retained_signature(signature, &[]);
        (answer, view.leaf_validations_for_tests() - before)
    };

    assert_eq!(probe(&stale), (false, 32));
    assert_eq!(probe(&stale), (false, 0), "a settled stale signature");
    assert_eq!(probe(&valid), (true, 31));
    assert_eq!(probe(&valid), (true, 0), "a settled valid signature");
    assert_eq!(view.settled_signatures_for_tests(), 2);
    assert_eq!(
        reads(&view, &stale, &[]),
        (Err(31), 32),
        "the same facts borrowed"
    );
}

/// Only a failure the view would answer the same way again is negative
/// validity. A fact read from a store live writers share fails without a
/// verdict and is read again; a validation the overlay moved under, or one
/// that met a completion writer in flight, settles nothing; a base not
/// proven current refuses without reading or settling anything.
#[test]
fn uncertain_torn_and_refused_failures_settle_nothing() {
    use verter_session_query::facts::fact_cache::RouteSurfaceFactRef;
    use verter_session_query::facts::registry::FactLane;
    use verter_session_query::facts::FactKey;

    let base = base_view();
    let levels = stale_chain(ResultReceipt::new);
    let deep = vec![FactVersionRef::Receipt(levels[31].clone())];

    let live = ResultReceipt::new(vec![
        FactVersionRef::RouteSurface(RouteSurfaceFactRef {
            canonical_id: "/r/1.ts".into(),
            key: FactKey::ProgramFileOrder {
                program: Arc::from("/unpublished/tsconfig.json"),
            },
            lane: FactLane::Semantic,
            expected_hash: [3; 16],
        }),
        whole(1),
    ]);
    let live = [FactVersionRef::Receipt(live)];
    let view = RequestStoreView::new(&base, overlay_through(33));
    for _ in 0..2 {
        let (answer, read) = reads(&view, &live, &[]);
        assert!(answer.is_err());
        assert!(read > 0, "a live-store failure is read again");
    }
    assert_eq!(view.settled_receipts_for_tests(true), 0);

    let moving = overlay_through(33);
    let mover = Arc::clone(&moving);
    let torn = RequestStoreView::new(&base, moving).with_validation_step_hook_for_tests(Arc::new(
        move |step| {
            if step == 0 {
                mover.insert_whole_hash_for_tests("/moved.ts", [9; 16]);
            }
        },
    ));
    assert!(torn.validate_fact_signature(&deep, &[]).is_err());
    assert_eq!(
        (
            torn.settled_receipts_for_tests(true),
            torn.settled_receipts_for_tests(false)
        ),
        (0, 0),
        "a validation the overlay moved under settles nothing"
    );

    let writing = overlay_through(33);
    let in_flight = RequestStoreView::new(&base, Arc::clone(&writing));
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(0);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(0);
    let writer = std::thread::spawn(move || {
        writing.hold_revision_in_flight_for_tests(entered_tx, release_rx);
    });
    entered_rx.recv().expect("writer must enter its bracket");
    let answer = reads(&in_flight, &deep, &[]);
    release_tx.send(()).expect("writer must still be waiting");
    writer.join().expect("writer must finish");
    assert_eq!(answer, (Err(0), 0), "a writer in flight refuses unread");
    assert_eq!(in_flight.settled_receipts_for_tests(true), 0);
    assert_eq!(
        reads(&in_flight, &deep, &[]),
        (Err(0), 32),
        "the restored state reads the chain"
    );

    let unproven = RequestStoreView::new_cold_seed(&base, overlay_through(33), false);
    assert_eq!(reads(&unproven, &deep, &[]), (Err(0), 0));
    let retained: Arc<[FactVersionRef]> = Arc::from(deep);
    assert!(!unproven.validates_retained_signature(&retained, &[]));
    assert_eq!(
        (
            unproven.settled_receipts_for_tests(true),
            unproven.settled_signatures_for_tests()
        ),
        (0, 0),
        "an unproven base settles nothing"
    );
}

/// A fact whose read was refused its lease proves nothing about the fact: it
/// fails without a verdict, for it or for the receipts that reach it, and a
/// probe once the lease can be had reads it again. The function-program
/// index walk is refused its walk-stack lease once (the fault is armed for
/// this source's reservation size alone, and the source is forced onto the
/// region path so the walk reserves on any thread's stack); the retry
/// through the same view at the same completion state validates.
#[test]
fn a_refused_index_walk_settles_nothing_and_its_retry_validates() {
    use oxc_span::SourceType;
    use verter_parser::oxc_parse::faults::{
        fail_reservations_needing, force_reservations, reservations_needing, Reservation,
    };
    use verter_session_query::facts::fact_cache::{
        ProgramAnalysisFactRef, ProgramAnalysisFunctionRef,
    };

    const ID: &str = "/ws/refused_flow.ts";
    // A nesting no other test in this binary forces, so the armed fault
    // reaches this source's walk alone.
    let source = format!(
        "export function alpha(n: number) {{\n  return {}n{};\n}}\n",
        "(".repeat(73),
        ")".repeat(73)
    );
    let upsert = |host: &VerterHost| {
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(ID.to_string()),
                input_id: ID.to_string(),
                source: Arc::from(source.as_str()),
                file_language: FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .expect("upsert must succeed");
    };
    let function = ProgramAnalysisFunctionRef {
        canonical_id: verter_type_expr::OwnedStr::from(ID),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        merged_symbol_name: verter_type_expr::OwnedStr::from("alpha"),
        symbol_space: verter_session_query::facts::SymbolSpace::Value,
        function_part: verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    };
    let live_hash = {
        let host = VerterHost::new_standalone(HostConfig::default());
        upsert(&host);
        host.ensure_indexed_ready(ID)
            .expect("indexed")
            .shallow_state
            .decl_bodies()
            .function_program_index()
            .value
            .value_function(
                function.owner,
                "alpha",
                &function.function_part,
                function.overload_ordinal,
            )
            .expect("alpha is indexed")
            .entry()
            .flow_body_stable_hash()
    };
    let needed = verter_parser::oxc_parse::parse_stack_bytes(&source, SourceType::ts());
    let _forcing = force_reservations(&[Reservation::Lease], needed);
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(&host);
    host.ensure_indexed_ready(ID).expect("indexed");
    let base = host.resolver_store_view_read().into_owned_view();
    let receipt = ResultReceipt::new(vec![
        FactVersionRef::ProgramAnalysis(ProgramAnalysisFactRef::FlowBody {
            function,
            flow_body_stable_hash: live_hash,
        }),
        whole(1),
    ]);
    let signature = [FactVersionRef::Receipt(receipt)];
    let view = RequestStoreView::new(&base, overlay_through(2));

    let leases = reservations_needing(Reservation::Lease, needed);
    fail_reservations_needing(Reservation::Lease, needed, 1);
    let refused = reads(&view, &signature, &[]);
    fail_reservations_needing(Reservation::Lease, needed, 0);
    assert!(
        reservations_needing(Reservation::Lease, needed) > leases,
        "the index walk asked for its lease"
    );
    assert_eq!(refused, (Err(0), 2), "the refused walk fails the receipt");
    assert_eq!(
        view.settled_receipts_for_tests(true),
        0,
        "a refused read is not negative validity"
    );
    assert_eq!(
        reads(&view, &signature, &[]),
        (Ok(()), 2),
        "the retry reads the fact again and validates"
    );
    assert_eq!(reads(&view, &signature, &[]), (Ok(()), 0));
}

/// A parse fact whose lazy declaration-body read misses its lease reads as
/// absent without being absent: it fails, settles nothing, and is read
/// again. That holds for the zero sentinel too: the refused read is not the
/// consistent absence the sentinel records, so neither the request view nor
/// the base view it reads through accepts it.
#[test]
fn a_parse_fact_whose_body_read_misses_its_lease_settles_nothing() {
    use verter_session_query::facts::fact_cache::ParseFactRef;
    use verter_session_query::facts::registry::{FactLane, InternedName};
    use verter_session_query::facts::{FactKey, SymbolSpace};

    const ID: &str = "/ws/lease_missed.ts";
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(ID.to_string()),
            input_id: ID.to_string(),
            source: Arc::from(
                "export type A = { x: number };
export type B = { y: string };
",
            ),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert must succeed");
    let indexed = host.ensure_indexed_ready(ID).expect("indexed");
    assert!(
        indexed.shallow_state.type_decl("A").is_some(),
        "A lowers under a live lease, pinning the retained snapshot"
    );
    indexed
        .shallow_state
        .decl_bodies()
        .release_retained_snapshot_for_test();
    let base = host.resolver_store_view_read().into_owned_view();
    for expected_hash in [[7; 16], [0; 16]] {
        let fact = ParseFactRef {
            canonical_id: ID.into(),
            key: FactKey::Export {
                name: InternedName(Arc::from("B")),
                space: SymbolSpace::Type,
            },
            lane: FactLane::Semantic,
            expected_hash,
        };
        assert!(
            !base.validates_parse_domain(&fact),
            "the base view rejects the refused read of {expected_hash:?}"
        );
        let signature = [FactVersionRef::Receipt(ResultReceipt::new(vec![
            FactVersionRef::Parse(fact),
        ]))];
        let view = RequestStoreView::new(&base, overlay_through(1));
        for probe in 0..2 {
            assert_eq!(
                reads(&view, &signature, &[]),
                (Err(0), 1),
                "probe {probe} reads the lease-missed fact recorded as {expected_hash:?}"
            );
        }
        assert_eq!(view.settled_receipts_for_tests(true), 0);
    }
}

/// A flow-body fact whose function-program index walk finds its retained
/// snapshot's lease pin broken reads an empty index that proves nothing
/// about the function: it fails, settles nothing, and is read again.
#[test]
fn a_flow_body_fact_whose_index_walk_misses_its_lease_settles_nothing() {
    use verter_session_query::facts::fact_cache::{
        ProgramAnalysisFactRef, ProgramAnalysisFunctionRef,
    };

    const ID: &str = "/ws/lease_missed_flow.ts";
    const SOURCE: &str = "export type A = { x: number };
export function alpha(n: number) {
  return n;
}
";
    let upsert = |host: &VerterHost| {
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(ID.to_string()),
                input_id: ID.to_string(),
                source: Arc::from(SOURCE),
                file_language: FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .expect("upsert must succeed");
    };
    let function = ProgramAnalysisFunctionRef {
        canonical_id: verter_type_expr::OwnedStr::from(ID),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        merged_symbol_name: verter_type_expr::OwnedStr::from("alpha"),
        symbol_space: verter_session_query::facts::SymbolSpace::Value,
        function_part: verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    };
    let live_hash = {
        let host = VerterHost::new_standalone(HostConfig::default());
        upsert(&host);
        host.ensure_indexed_ready(ID)
            .expect("indexed")
            .shallow_state
            .decl_bodies()
            .function_program_index()
            .value
            .value_function(
                function.owner,
                "alpha",
                &function.function_part,
                function.overload_ordinal,
            )
            .expect("alpha is indexed")
            .entry()
            .flow_body_stable_hash()
    };
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(&host);
    let indexed = host.ensure_indexed_ready(ID).expect("indexed");
    assert!(
        indexed.shallow_state.type_decl("A").is_some(),
        "A lowers under a live lease, pinning the retained snapshot"
    );
    indexed
        .shallow_state
        .decl_bodies()
        .release_retained_snapshot_for_test();
    let base = host.resolver_store_view_read().into_owned_view();
    let signature = [FactVersionRef::Receipt(ResultReceipt::new(vec![
        FactVersionRef::ProgramAnalysis(ProgramAnalysisFactRef::FlowBody {
            function,
            flow_body_stable_hash: live_hash,
        }),
    ]))];
    let view = RequestStoreView::new(&base, overlay_through(1));
    for probe in 0..2 {
        assert_eq!(
            reads(&view, &signature, &[]),
            (Err(0), 1),
            "probe {probe} reads the lease-missed flow-body fact"
        );
    }
    assert_eq!(
        view.settled_receipts_for_tests(true),
        0,
        "a broken lease pin is not negative validity"
    );
}

/// Settled verdicts belong to the request: they keep the evidence they
/// name alive while the view lives and release every byte of it with the
/// view.
#[test]
fn settled_verdicts_release_their_evidence_with_the_view() {
    use verter_session_query::retention::{RetentionLimits, SemanticRetentionAccount};

    let account = SemanticRetentionAccount::new(RetentionLimits::default());
    let base = base_view();
    let view = RequestStoreView::new(&base, overlay_through(33));
    {
        let levels = stale_chain(|facts| ResultReceipt::new_on(facts, &account));
        let valid = ResultReceipt::new_on(vec![whole(32)], &account);
        let retained: Arc<[FactVersionRef]> = Arc::from(vec![
            FactVersionRef::Receipt(levels[31].clone()),
            FactVersionRef::Receipt(valid.clone()),
        ]);
        assert!(view
            .validate_fact_signature(&[FactVersionRef::Receipt(valid)], &[])
            .is_ok());
        assert!(!view.validates_retained_signature(&retained, &[]));
        assert_eq!(view.settled_receipts_for_tests(true), 32);
        assert_eq!(view.settled_receipts_for_tests(false), 1);
    }
    assert!(
        account.snapshot().pinned_bytes > 0,
        "the verdicts hold their evidence for the request"
    );
    drop(view);
    assert_eq!(
        account.snapshot().pinned_bytes,
        0,
        "the request's verdicts drain with it"
    );
}
