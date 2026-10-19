//! A closed host is empty, reusable and back at its pre-workload footprint.
//!
//! Every figure here is read through the public host: ordinary upserts and
//! named-symbol requests make the workload, `close` releases it, the parse
//! release boundary proves the queued parse releases were applied, and the
//! retention snapshot is compared with the one taken before the workload.
//! Readers a caller still holds survive the close, stay valid and keep their
//! own charges until their last handle drops.

use std::sync::{Arc, Barrier, Mutex};

use verter_session::{HostConfig, HostRetentionSnapshot, UpsertRequest, VerterHost};
use verter_type_engine::semantic_query::SemanticNodeId;
use verter_type_engine::semantic_query_memo::node_roots::NodeRoot;
use verter_type_engine::signature_kernel::SemanticReadView;

/// Every host charges the one process account, so tests comparing account
/// figures run one at a time.
static PROCESS_ACCOUNT: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    PROCESS_ACCOUNT
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

/// The charge a set of readers keeps alive across a close: a control
/// host runs the same workload, holds exactly the readers in `held` (bit 0
/// result, bit 1 signature view, bit 2 parse artifact), closes, and reports
/// the total it keeps above its baseline. Dropping them returns it to baseline.
fn held_set_charge(held: u8) -> usize {
    let host = VerterHost::new_standalone(HostConfig::default());
    let baseline = retained(&host.retention_snapshot());
    let base_total = baseline.active_bytes + baseline.retained_bytes + baseline.pinned_bytes;
    let (answer, _) = demand(&host, 3);
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
    let live = VerterHost::new_standalone(HostConfig::default());
    let (live_answer, live_rendered) = demand(&live, 7);
    let live_footprint = retained(&live.retention_snapshot());

    let closed = VerterHost::new_standalone(HostConfig::default());
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
        let graph = host.project_type_store().semantic_graph();
        let view = SemanticReadView::pin(graph.signature_store());
        let held_epoch = view.epoch();
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
                view.epoch(),
                held_epoch,
                "{order}: the held signature view keeps its epoch after the close"
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
    }
}

/// A close racing a request on another thread never leaves residue: once
/// both settle, a final close returns the host to its baseline and the host
/// answers again.
#[test]
fn a_close_racing_a_publication_settles_to_the_baseline() {
    let _serial = serial();
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let baseline = retained(&host.retention_snapshot());
    for round in 0..4 {
        let start = Arc::new(Barrier::new(2));
        let requester = {
            let (host, start) = (Arc::clone(&host), Arc::clone(&start));
            std::thread::spawn(move || {
                start.wait();
                let (canonical, symbols, source) = document(20 + round);
                upsert(&host, &canonical, &source);
                let answers: Option<Vec<NodeRoot>> = symbols
                    .iter()
                    .map(|symbol| host.resolve_named_symbol(&canonical, symbol, None))
                    .collect();
                answers.map(|answers| render(&host, &answers))
            })
        };
        start.wait();
        host.close();
        let answered = requester.join().expect("the request completes");
        if let Some(rendered) = answered {
            assert_eq!(rendered, fresh_answer(20 + round), "round {round}");
        }
        host.close();
        drain(&host);
        assert_at_baseline(&host, &baseline, &format!("after racing round {round}"));
    }
    let (answer, rendered) = demand(&host, 30);
    assert_eq!(rendered, fresh_answer(30));
    drop(answer);
}
