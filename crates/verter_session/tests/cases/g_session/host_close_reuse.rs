//! A closed host is empty, reusable and back at its pre-workload footprint.
//!
//! Every figure here is read through the public host: ordinary upserts and
//! named-symbol requests make the workload, `close` releases it, the parse
//! release boundary proves the queued parse releases were applied, and the
//! retention snapshot is compared with the one taken before the workload.
//! Readers a caller still holds survive the close, stay valid and keep their
//! own charges until their last handle drops.

use std::sync::{Arc, Mutex};

use verter_session::{HostConfig, HostRetentionSnapshot, UpsertRequest, VerterHost};
use verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch;
use verter_type_engine::semantic_query::SemanticNodeId;
use verter_type_engine::semantic_query::SignatureKind;
use verter_type_engine::semantic_query_memo::node_roots::NodeRoot;
use verter_type_engine::signature_kernel::{BorrowedSet, SemanticReadView, SignatureSetRef};

/// Every host charges the one process account, so tests comparing account
/// figures run one at a time, under the lock the occupancy-window suite
/// shares.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    super::active_occupancy_window::PROCESS_ACCOUNT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn upsert(host: &VerterHost, canonical_id: &str, source: &str) {
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical_id.to_string()),
        input_id: canonical_id.to_string(),
        source: Arc::from(source),
        file_language: verter_session::LanguageRegistry::global()
            .classify_static(canonical_id)
            .static_resolution(),
        aliases: Vec::new(),
    });
}

/// One document per cycle, with names distinct per cycle, so no cycle can
/// be served by what an earlier one left behind. `Out` resolves through a
/// conditional `infer` over a call signature, so the demand interns
/// signature records too. `Spare` is never demanded: reading it later lowers
/// a declaration through the retained parse.
fn document(cycle: usize) -> (String, [String; 2], String) {
    let canonical = format!("/src/types/shapes{cycle}.ts");
    let symbols = [format!("Box{cycle}"), format!("Out{cycle}")];
    let source = format!(
        "export interface Props{cycle} {{ name: string; size{cycle}: number }}\n\
         export type Run{cycle} = (input: Props{cycle}) => string;\n\
         export interface Box{cycle} {{ props: Props{cycle}; run: Run{cycle}; label: string }}\n\
         type Ret{cycle}<F> = F extends (...args: any[]) => infer R ? R : never;\n\
         export type Out{cycle} = Ret{cycle}<Run{cycle}>;\n\
         export interface Spare{cycle} {{ spare{cycle}: boolean }}\n"
    );
    (canonical, symbols, source)
}

/// Cold named-symbol demands through the ordinary public request, with the
/// answers rendered for comparison.
fn demand(host: &VerterHost, cycle: usize) -> (Vec<NodeRoot>, String) {
    let (canonical, symbols, source) = document(cycle);
    upsert(host, &canonical, &source);
    let answers: Vec<NodeRoot> = symbols
        .iter()
        .map(|symbol| {
            host.resolve_named_symbol(&canonical, symbol, None)
                .unwrap_or_else(|| panic!("{symbol} resolves"))
        })
        .collect();
    let rendered = render(host, &answers);
    (answers, rendered)
}

/// The answer's payload with every child node id replaced by the child's
/// own rendering, so two hosts that number their nodes differently render
/// equal answers equally.
fn render(host: &VerterHost, answers: &[NodeRoot]) -> String {
    answers
        .iter()
        .map(|answer| render_node(host, answer.id(), 4))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_node(host: &VerterHost, id: SemanticNodeId, depth: usize) -> String {
    let data = host
        .project_type_store()
        .semantic_graph()
        .node_data(id)
        .expect("a held answer stays live");
    let text = format!("{data:?}");
    let mut out = String::new();
    let mut rest = text.as_str();
    while let Some(at) = rest.find("SemanticNodeId(") {
        out.push_str(&rest[..at]);
        let tail = &rest[at + "SemanticNodeId(".len()..];
        let end = tail.find(')').expect("an id closes");
        let child = SemanticNodeId(tail[..end].parse().expect("an id is numeric"));
        if depth == 0 {
            out.push('#');
        } else {
            out.push('<');
            out.push_str(&render_node(host, child, depth - 1));
            out.push('>');
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

/// What the answer to cycle `cycle` must read: a fresh host's answer. The
/// fresh host is closed and drained before it drops, so its parse pins are
/// gone from the shared account before the caller reads it.
fn fresh_answer(cycle: usize) -> String {
    let host = VerterHost::new_standalone(HostConfig::default());
    let (answers, rendered) = demand(&host, cycle);
    drop(answers);
    host.close();
    drain(&host);
    rendered
}

/// Apply every parse release issued so far.
fn drain(host: &VerterHost) {
    host.release_completion()
        .wait()
        .expect("the decl-lowering workers apply the queued releases");
}

/// The figures a released workload must return to: the account's charges
/// and every required object count. Monotonic history (`peak_total_bytes`,
/// refusal totals, close statistics) is not a retained figure.
fn retained(snapshot: &HostRetentionSnapshot) -> HostRetentionSnapshot {
    HostRetentionSnapshot {
        peak_total_bytes: 0,
        refusals_pressure: 0,
        refusals_oversized: 0,
        refusals_active: 0,
        closes: Default::default(),
        ..snapshot.clone()
    }
}

/// An identity index's occupancy without its backing capacities.
fn occupied(
    indexes: verter_type_engine::semantic_query_memo::IdentityIndexSnapshot,
) -> verter_type_engine::semantic_query_memo::IdentityIndexSnapshot {
    let occupied = |index: verter_type_engine::semantic_query_memo::InternIndexOccupancy| {
        verter_type_engine::semantic_query_memo::InternIndexOccupancy {
            digest_capacity: 0,
            collision_entry_capacity: 0,
            ..index
        }
    };
    verter_type_engine::semantic_query_memo::IdentityIndexSnapshot {
        intersection_recipes: occupied(indexes.intersection_recipes),
        semantic_contexts: occupied(indexes.semantic_contexts),
        order_domains: occupied(indexes.order_domains),
        relate_keys: occupied(indexes.relate_keys),
        resolve_call_keys: occupied(indexes.resolve_call_keys),
    }
}

/// The `field: value` pairs where two snapshots differ, for a readable
/// failure.
fn differing(left: &HostRetentionSnapshot, right: &HostRetentionSnapshot) -> Vec<String> {
    let fields = |snapshot: &HostRetentionSnapshot| {
        format!("{snapshot:?}")
            .split(", ")
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    let (left, right) = (fields(left), fields(right));
    left.iter()
        .zip(&right)
        .filter(|(l, r)| l != r)
        .map(|(l, r)| format!("{l}  (expected {r})"))
        .collect()
}

#[track_caller]
fn assert_at_baseline(host: &VerterHost, baseline: &HostRetentionSnapshot, context: &str) {
    let now = retained(&host.retention_snapshot());
    assert!(
        now == *baseline,
        "{context}: the closed host must hold exactly its pre-workload footprint; differing: {:#?}",
        differing(&now, baseline)
    );
}

/// The workload a test runs must actually charge the figures the baseline
/// comparison covers, or the comparison proves nothing.
#[track_caller]
fn assert_workload_charged(during: &HostRetentionSnapshot, baseline: &HostRetentionSnapshot) {
    assert!(
        during.retained_bytes > baseline.retained_bytes
            && during.pinned_bytes > baseline.pinned_bytes,
        "the workload charges retained and pinned bytes: {during:#?}"
    );
    assert!(
        during.identity_pool_entries > baseline.identity_pool_entries,
        "the workload pools identity strings"
    );
    assert!(
        during.snapshot_leases > baseline.snapshot_leases,
        "the workload retains a parse"
    );
}

fn account(host: &VerterHost) -> (usize, usize, usize) {
    let s = host.retention_snapshot();
    (s.active_bytes, s.retained_bytes, s.pinned_bytes)
}

fn total(host: &VerterHost) -> usize {
    let (active, retained, pinned) = account(host);
    active + retained + pinned
}

/// The undemanded declaration lowered through a held artifact.
fn spare_text(artifact: &verter_session::project_type_store::IndexedReady, cycle: usize) -> String {
    format!(
        "{:?}",
        artifact
            .shallow_state
            .type_decl(&format!("Spare{cycle}"))
            .expect("the undemanded declaration lowers through the retained parse")
    )
}

/// The call-signature set the workload's `Run` alias settles to: a handle
/// the signature store issued for a real published signature. The value that
/// carried it is dropped, so the handle is the only thing a reader keeps.
fn issued_signature_set(host: &VerterHost, cycle: usize) -> SignatureSetRef {
    let (canonical, ..) = document(cycle);
    let run = host
        .resolve_named_symbol(&canonical, &format!("Run{cycle}"), None)
        .expect("the call alias resolves");
    let value = ProjectSemanticDispatch::new(host)
        .signature_set_value(run.id(), SignatureKind::Call)
        .expect("the alias has a call signature set");
    value.set()
}

/// The signature records `set` names, read through `view`.
fn read_signatures(view: &SemanticReadView, set: SignatureSetRef) -> String {
    match view.read_set(set).expect("the issued set reads") {
        BorrowedSet::Empty => panic!("the alias publishes a call signature"),
        BorrowedSet::One {
            candidate,
            descriptor,
            provenance,
        } => format!("{candidate:?} {descriptor:?} {provenance:?}"),
        BorrowedSet::Many(candidates) => format!("{candidates:?}"),
    }
}

/// The pinned growth of a live (never closed) workload plus what lowering
/// an undemanded declaration adds: an upper bound, established without any
/// close, on what a held parse artifact may keep charged.
fn live_artifact_bound(cycle: usize) -> usize {
    let host = VerterHost::new_standalone(HostConfig::default());
    let base = total(&host);
    let base_pinned = host.retention_snapshot().pinned_bytes;
    let (answer, _) = demand(&host, cycle);
    let artifact = host
        .project_type_store()
        .indexed()
        .get_any(&document(cycle).0)
        .expect("the demanded document has a live artifact");
    let pinned = host.retention_snapshot().pinned_bytes - base_pinned;
    let before_lowering = total(&host);
    let _ = spare_text(&artifact, cycle);
    let lowering = total(&host) - before_lowering;
    drop((answer, artifact));
    host.close();
    drain(&host);
    assert_eq!(total(&host), base, "the live probe returns to its baseline");
    pinned + lowering
}

/// Live (never closed) figures for a lone result and a lone signature view:
/// an upper bound from the non-pinned account growth, and the signature
/// store's own exact epoch charge.
fn live_reader_bounds(cycle: usize) -> (usize, usize, usize) {
    let host = VerterHost::new_standalone(HostConfig::default());
    let base = total(&host);
    let base_pinned = host.retention_snapshot().pinned_bytes;
    let (answer, _) = demand(&host, cycle);
    let pinned = host.retention_snapshot().pinned_bytes - base_pinned;
    let result_bound = total(&host) - base - pinned;
    let _ = issued_signature_set(&host, cycle);
    let view_bound = host
        .project_type_store()
        .semantic_graph()
        .signature_store()
        .current_epoch_storage_charge();
    drop(answer);
    host.close();
    drain(&host);
    (result_bound, view_bound, pinned)
}

/// The charge a set of readers keeps alive across a close: a control
/// host runs the same workload, holds exactly the readers in `held` (bit 0
/// result, bit 1 signature view, bit 2 parse artifact), closes, and reports
/// the total it keeps above its baseline. Dropping them returns it to baseline.
fn held_set_charge(held: u8) -> usize {
    let host = VerterHost::new_standalone(HostConfig::default());
    let baseline = retained(&host.retention_snapshot());
    let base_total = baseline.active_bytes + baseline.retained_bytes + baseline.pinned_bytes;
    let (answer, _) = demand(&host, 3);
    let _ = issued_signature_set(&host, 3);
    let graph = host.project_type_store().semantic_graph();
    let view = SemanticReadView::pin(graph.signature_store());
    let artifact = host
        .project_type_store()
        .indexed()
        .get_any(&document(3).0)
        .expect("the demanded document has a live artifact");
    let (answer, view, artifact) = (
        (held & 1 != 0).then_some(answer),
        (held & 2 != 0).then_some(view),
        (held & 4 != 0).then_some(artifact),
    );
    host.close();
    drain(&host);
    if let Some(artifact) = &artifact {
        let _ = spare_text(artifact, 3);
    }
    let charge = total(&host) - base_total;
    drop((answer, view, artifact));
    drain(&host);
    assert_at_baseline(&host, &baseline, "a lone reader's control");
    charge
}

#[test]
fn one_cold_demand_then_close_returns_the_host_to_its_pre_workload_footprint() {
    let _serial = serial();
    let host = VerterHost::new_standalone(HostConfig::default());
    let baseline = retained(&host.retention_snapshot());

    let (answer, rendered) = demand(&host, 0);
    assert_eq!(rendered, fresh_answer(0));
    assert_workload_charged(&host.retention_snapshot(), &baseline);
    drop(answer);

    host.close();
    // Close itself releases every owner and object; only the parse pins
    // wait for their workers, which the release boundary below proves.
    let closed = HostRetentionSnapshot {
        pinned_bytes: baseline.pinned_bytes,
        ..retained(&host.retention_snapshot())
    };
    assert!(
        closed == baseline,
        "close releases everything but the queued parse pins; differing: {:#?}",
        differing(&closed, &baseline)
    );
    drain(&host);
    assert_at_baseline(&host, &baseline, "after one cold demand");

    let (answer, rendered) = demand(&host, 0);
    assert_eq!(
        rendered,
        fresh_answer(0),
        "the closed host answers the same demand again"
    );
    drop(answer);
}

#[test]
fn repeated_distinct_demand_close_cycles_return_to_the_same_footprint() {
    let _serial = serial();
    let host = VerterHost::new_standalone(HostConfig::default());
    let baseline = retained(&host.retention_snapshot());

    for cycle in 0..5 {
        let (answer, rendered) = demand(&host, cycle);
        assert_eq!(rendered, fresh_answer(cycle), "cycle {cycle} answers");
        assert_workload_charged(&host.retention_snapshot(), &baseline);
        drop(answer);
        host.close();
        drain(&host);
        assert_at_baseline(&host, &baseline, &format!("after cycle {cycle}"));
    }
}

#[test]
fn closing_one_host_keeps_an_unrelated_live_hosts_data_and_charges() {
    let _serial = serial();
    // Both hosts exist before either is measured: a host charges its own
    // store storage from construction, and the account is the process's.
    let live = VerterHost::new_standalone(HostConfig::default());
    let closed = VerterHost::new_standalone(HostConfig::default());
    let (live_answer, live_rendered) = demand(&live, 7);
    let live_footprint = retained(&live.retention_snapshot());

    let closed_baseline = retained(&closed.retention_snapshot());
    let (answer, _) = demand(&closed, 8);
    drop(answer);
    closed.close();
    drain(&closed);

    // The identity indexes are process-wide and the live host still occupies
    // them: their records and digests return exactly, while a collision
    // bucket both hosts shared may keep the amortised capacity the index
    // keeps for its remaining occupant.
    let shared_capacity = |snapshot: HostRetentionSnapshot| HostRetentionSnapshot {
        identity_indexes: occupied(snapshot.identity_indexes),
        ..snapshot
    };
    let closed_now = shared_capacity(retained(&closed.retention_snapshot()));
    let closed_expected = shared_capacity(closed_baseline);
    assert!(
        closed_now == closed_expected,
        "the closed host returns to its baseline; differing: {:#?}",
        differing(&closed_now, &closed_expected)
    );
    let live_now = shared_capacity(retained(&live.retention_snapshot()));
    let live_expected = shared_capacity(live_footprint);
    assert!(
        live_now == live_expected,
        "the live host keeps every charge and object it held; differing: {:#?}",
        differing(&live_now, &live_expected)
    );
    assert_eq!(
        render(&live, &live_answer),
        live_rendered,
        "the live host's held answer reads the same"
    );
    let (again, again_rendered) = demand(&live, 7);
    assert_eq!(
        again_rendered, live_rendered,
        "the live host still serves its document"
    );
    drop((again, live_answer));
}

/// A result, a signature read view and a parse artifact held across the
/// close stay valid and charged, a second handle on the same allocation
/// adds no charge, and the last drop — before or after the close — brings
/// the account back to baseline.
#[test]
fn held_readers_survive_close_and_release_once_in_either_order() {
    let _serial = serial();
    for cache_first in [true, false] {
        let order = if cache_first {
            "cache-first"
        } else {
            "caller-first"
        };
        let host = VerterHost::new_standalone(HostConfig::default());
        let baseline = retained(&host.retention_snapshot());
        let (answer, rendered) = demand(&host, 3);
        let canonical = document(3).0;
        let issued = issued_signature_set(&host, 3);
        let graph = host.project_type_store().semantic_graph();
        let view = SemanticReadView::pin(graph.signature_store());
        let held_epoch = view.epoch();
        let signatures_before = read_signatures(&view, issued);
        let artifact = host
            .project_type_store()
            .indexed()
            .get_any(&canonical)
            .expect("the demanded document has a live artifact");
        let held = account(&host);
        let (clone, artifact_clone, view_again) = (
            answer.clone(),
            Arc::clone(&artifact),
            SemanticReadView::pin(graph.signature_store()),
        );
        assert_eq!(
            account(&host),
            held,
            "{order}: another handle on a held allocation adds no charge"
        );
        drop((clone, artifact_clone, view_again));
        assert_eq!(account(&host), held, "{order}: dropping it frees nothing");

        let total_before_close = total(&host);
        if cache_first {
            host.close();
            drain(&host);
            let base_total =
                baseline.active_bytes + baseline.retained_bytes + baseline.pinned_bytes;
            // The parse reader still serves an undemanded declaration after
            // the close, identical to a clean host's.
            let control = VerterHost::new_standalone(HostConfig::default());
            let (control_answer, _) = demand(&control, 3);
            let control_artifact = control
                .project_type_store()
                .indexed()
                .get_any(&canonical)
                .expect("the control has a live artifact");
            assert_eq!(
                spare_text(&artifact, 3),
                spare_text(&control_artifact, 3),
                "{order}: the held artifact lowers an undemanded declaration after the close"
            );
            drop((control_answer, control_artifact));
            control.close();
            drain(&control);
            // The control goes with its own store storage once compared.
            drop(control);
            let readers_only = total(&host);
            assert!(
                readers_only <= total_before_close,
                "{order}: the close creates no charge"
            );
            assert_eq!(
                render(&host, &answer),
                rendered,
                "{order}: the held result reads the same after the close"
            );
            assert_eq!(
                read_signatures(&view, issued),
                signatures_before,
                "{order}: the issued signature set reads the same records after the close"
            );
            assert_ne!(
                SemanticReadView::pin(graph.signature_store()).epoch(),
                held_epoch,
                "{order}: the close replaced the signature epoch the view pins"
            );
            assert_eq!(
                retained(&host.retention_snapshot()).snapshot_leases,
                baseline.snapshot_leases + 1,
                "{order}: the close revokes no lease a reader holds"
            );
            // Bounds established on a live host that never closed, so a close
            // that charged a reader again cannot hide behind a control that
            // closes the same way.
            let lone_artifact = held_set_charge(0b100);
            assert!(
                lone_artifact > 0 && lone_artifact <= live_artifact_bound(3),
                "{order}: a lone held artifact keeps a charge no larger than its live one"
            );
            assert!(
                held_set_charge(0b111) <= total_before_close - base_total,
                "{order}: no reader is charged more than the live workload held"
            );
            // Distinct allocations: each reader's charge is its own, so the
            // set's charge never exceeds the sum of the lone readers' (shared
            // structure is charged once).
            let (lone_result, lone_view) = (held_set_charge(0b001), held_set_charge(0b010));
            let (result_bound, view_bound, live_pinned_bound) = live_reader_bounds(3);
            assert!(
                lone_result > 0 && lone_result <= result_bound,
                "{order}: a lone held result keeps a charge no larger than its live one \
                 ({lone_result} vs {result_bound})"
            );
            assert!(
                lone_view >= view_bound && lone_view <= live_pinned_bound,
                "{order}: a lone held signature view keeps at least the epoch charge the live store reports and no more than the live pinned growth ({lone_view} vs {view_bound}, result {lone_result} vs {result_bound})"
            );
            assert!(
                held_set_charge(0b111) <= lone_result + lone_view + lone_artifact,
                "{order}: the held set keeps no more than one charge per allocation"
            );
            // Each release leaves exactly the charge of the readers still held.
            for (name, remaining, release) in [
                (
                    "result",
                    0b110u8,
                    Box::new(|| drop(answer)) as Box<dyn FnOnce()>,
                ),
                ("signature view", 0b100, Box::new(|| drop(view))),
                ("parse artifact", 0b000, Box::new(|| drop(artifact))),
            ] {
                release();
                drain(&host);
                assert_eq!(
                    total(&host) - base_total,
                    held_set_charge(remaining),
                    "{order}: after dropping the held {name} exactly the others stay charged"
                );
            }
        } else {
            drain(&host);
            let owned = account(&host);
            drop(answer);
            assert_eq!(
                account(&host),
                owned,
                "{order}: the cache still owns the result's charge"
            );
            drop(view);
            assert_eq!(
                account(&host),
                owned,
                "{order}: the store still owns the epoch's charge"
            );
            drop(artifact);
            drain(&host);
            assert_eq!(
                account(&host),
                owned,
                "{order}: a non-last artifact handle's drop frees nothing"
            );
            assert_eq!(
                retained(&host.retention_snapshot()).snapshot_leases,
                baseline.snapshot_leases + 1,
                "{order}: the store still owns the artifact's lease"
            );
            host.close();
            drain(&host);
        }
        assert_at_baseline(&host, &baseline, order);
        let (again, rendered_again) = demand(&host, 3);
        assert_eq!(
            rendered_again, rendered,
            "{order}: the same owner is reused"
        );
        drop(again);
        // The account is process-wide: this host's parse pins must be gone
        // before the next iteration reads its baseline.
        host.close();
        drain(&host);
    }
}

/// A close issued while a request is parked inside a cold build never
/// leaves residue: the close runs at an observed rendezvous inside the
/// publishing producer, the request is then released, and once it settles a
/// final close returns the host to its baseline and the host answers again.
#[test]
fn a_close_racing_a_publication_settles_to_the_baseline() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::time::Duration;

    let _serial = serial();
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let baseline = retained(&host.retention_snapshot());
    let graph = host.project_type_store().semantic_graph();
    struct HookGuard<'s>(&'s verter_type_engine::semantic_query_memo::SemanticGraphStore);
    impl Drop for HookGuard<'_> {
        fn drop(&mut self) {
            self.0.set_produce_hook_for_tests(None);
        }
    }
    for round in 0..4 {
        let (canonical, symbols, source) = document(20 + round);
        upsert(&host, &canonical, &source);
        let (ready_tx, ready_rx) = channel::<()>();
        let (release_tx, release_rx) = channel::<()>();
        let release_rx = Mutex::new(release_rx);
        let park_once = AtomicBool::new(true);
        graph.set_produce_hook_for_tests(Some(Arc::new(move |_key, _nested| {
            if park_once.swap(false, Ordering::SeqCst) {
                let _ = ready_tx.send(());
                let _ = release_rx
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(10));
            }
        })));
        let _hook = HookGuard(graph);
        let requester = {
            let host = Arc::clone(&host);
            std::thread::spawn(move || {
                let answers: Option<Vec<NodeRoot>> = symbols
                    .iter()
                    .map(|symbol| host.resolve_named_symbol(&canonical, symbol, None))
                    .collect();
                answers.map(|answers| render(&host, &answers))
            })
        };
        let parked = ready_rx.recv_timeout(Duration::from_secs(10));
        if parked.is_err() {
            let _ = release_tx.send(());
            let _ = requester.join();
            panic!("round {round}: the request must park inside its cold build");
        }
        host.close();
        let _ = release_tx.send(());
        let answered = requester.join().expect("the request completes");
        if let Some(rendered) = answered {
            assert_eq!(rendered, fresh_answer(20 + round), "round {round}");
        }
        graph.set_produce_hook_for_tests(None);
        host.close();
        drain(&host);
        assert_at_baseline(&host, &baseline, &format!("after racing round {round}"));
    }
    let (answer, rendered) = demand(&host, 30);
    assert_eq!(rendered, fresh_answer(30));
    drop(answer);
}

/// A parse artifact held across the close keeps the parse snapshot pin it
/// held live, as observed on the owning worker, and releases it once with
/// the artifact: the fall is that pin plus the artifact's own small charge,
/// never a second pin.
#[test]
fn a_held_artifact_keeps_and_releases_its_inspected_parse_pin_once() {
    let _serial = serial();
    let host = VerterHost::new_standalone(HostConfig::default());
    let baseline = retained(&host.retention_snapshot());
    let (answer, _) = demand(&host, 3);
    drop(answer);
    let artifact = host
        .project_type_store()
        .indexed()
        .get_any(&document(3).0)
        .expect("the demanded document has a live artifact");
    let live_pin = host.parse_pin_bytes_for_tests();
    assert!(live_pin > 0, "the workload retains a parse snapshot");

    host.close();
    drain(&host);
    assert_eq!(
        host.parse_pin_bytes_for_tests(),
        live_pin,
        "the close neither duplicates nor loses the held snapshot's pin"
    );

    let before = total(&host);
    drop(artifact);
    drain(&host);
    let fall = before - total(&host);
    assert_eq!(host.parse_pin_bytes_for_tests(), 0, "the snapshot is freed");
    assert!(
        fall >= live_pin && fall < 2 * live_pin,
        "the artifact's release returns its pin once ({live_pin}) plus its own charge, not {fall}"
    );
    assert_at_baseline(&host, &baseline, "after the artifact's release");
}

/// A release is exact when the account falls by each inspected allocation
/// charge once plus a remainder smaller than the smallest of them (the held
/// nodes' own small records): a charge duplicated while held falls with the
/// reader and pushes the remainder past that bound, a lost one pulls the
/// fall under the inspected sum.
fn release_is_exact(fall: usize, inspected: &[usize]) -> bool {
    let sum: usize = inspected.iter().sum();
    let smallest = inspected.iter().copied().min().unwrap_or(0);
    fall >= sum && fall - sum < smallest
}

/// A signature read view held across the close keeps, and its last drop
/// returns, each allocation it keeps alive exactly once: the epoch storage
/// and the node arena storage its records root, both read from the
/// allocations themselves before the close and after it.
#[test]
fn a_held_view_keeps_and_releases_each_inspected_allocation_once() {
    let _serial = serial();
    let host = VerterHost::new_standalone(HostConfig::default());
    let baseline = retained(&host.retention_snapshot());
    let graph = host.project_type_store().semantic_graph();
    let (answer, _) = demand(&host, 3);
    let _ = issued_signature_set(&host, 3);
    drop(answer);
    let view = SemanticReadView::pin(graph.signature_store());
    let live = (
        view.epoch_storage_charge_for_tests(),
        graph.graph_storage_charge_for_tests(),
    );
    assert!(
        live.0 > 0 && live.1 > 0,
        "the workload charges both allocations"
    );

    host.close();
    drain(&host);
    // The close drops what only the cache held, so the arena shrinks to the
    // nodes the view's records root; the epoch is the same allocation.
    let held = (
        view.epoch_storage_charge_for_tests(),
        graph.graph_storage_charge_for_tests(),
    );
    assert_eq!(held.0, live.0, "the close keeps the held epoch's charge");
    assert!(
        held.1 > 0 && held.1 <= live.1,
        "the close creates no arena charge and loses none the view roots"
    );

    let before = total(&host);
    drop(view);
    drain(&host);
    let fall = before - total(&host);
    assert!(
        release_is_exact(fall, &[held.0, held.1]),
        "the last drop returns the inspected {held:?} once, not {fall}"
    );
    assert_eq!(graph.graph_storage_charge_for_tests(), 0);
    assert_at_baseline(&host, &baseline, "after the view's release");

    assert!(
        !release_is_exact(fall + held.0, &[held.0, held.1]),
        "a duplicated epoch charge is rejected"
    );
    assert!(
        !release_is_exact(fall + held.1, &[held.0, held.1]),
        "a duplicated arena charge is rejected"
    );
    assert!(
        !release_is_exact(fall - held.0, &[held.0, held.1]),
        "a lost epoch charge is rejected"
    );
}
