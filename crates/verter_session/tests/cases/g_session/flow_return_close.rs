//! A fresh host's first flow returns leave nothing behind once their readers
//! release and the host closes.
//!
//! Every host here starts unprimed: its baseline is read before any semantic
//! demand, so whatever the first demand creates — the flow-slice graph
//! bundles and lowered slices, the global primitive vocabulary it interns,
//! the sidecars describing them — must be released by `close` and the public
//! release boundary, not absorbed into the baseline. Each host is closed and
//! drained when its test ends, failing or not. Readers held across the close
//! keep their answers and exactly their own charges until their last copy
//! drops.

use std::sync::Arc;

use verter_execution::cancellation::CancellationToken;
use verter_session::host_flow_return_audit::FlowReturnError;
use verter_session::{HostConfig, HostRetentionSnapshot, UpsertRequest, VerterHost};
use verter_session_query::retention::{ChargeClass, SemanticRetentionAccount};
use verter_type_engine::semantic_query::{
    FlowReturnResult, ReturnProjectionDemand, SemanticNodeId,
};
use verter_type_engine::semantic_query_memo::charge_witness::HeldChargeWitness;
use verter_type_expr::facts::{FlowFunctionReturnIdentity, FunctionPartIdentity};
use verter_type_expr::locators::{AuthoredAnchor, LocatorSymbolSpace};

/// Tests reading the process account's totals run one at a time, under the
/// lock the occupancy-window suite shares.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    super::active_occupancy_window::PROCESS_ACCOUNT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A host closed and drained through the public completion boundary when it
/// drops — on a failing assertion's unwind too.
struct ClosingHost(VerterHost);

impl ClosingHost {
    fn fresh() -> Self {
        Self(VerterHost::new_standalone(HostConfig::default()))
    }

    /// Close the host and apply every release queued so far.
    fn retire(&self) {
        self.0.close();
        drain(&self.0);
    }
}

impl std::ops::Deref for ClosingHost {
    type Target = VerterHost;
    fn deref(&self) -> &VerterHost {
        &self.0
    }
}

impl Drop for ClosingHost {
    fn drop(&mut self) {
        self.0.close();
        let _ = self.0.release_completion().wait();
    }
}

fn drain(host: &VerterHost) {
    host.release_completion()
        .wait()
        .expect("the decl-lowering workers apply the queued releases");
}

/// One flow-return workload: a function whose inferred return the host
/// delivers.
#[derive(Clone, Copy, Debug)]
enum Workload {
    /// A call no overload accepts: the delivered result carries the
    /// checker's diagnostic.
    Diagnostic,
    /// An array literal of a parameter and a number.
    Tuple,
    /// Conditional object-literal returns, normalised to one union.
    ConditionalObjects,
    /// The control: a bare fresh literal.
    FreshLiteral,
}

const WORKLOADS: [Workload; 4] = [
    Workload::Diagnostic,
    Workload::Tuple,
    Workload::ConditionalObjects,
    Workload::FreshLiteral,
];

impl Workload {
    /// The workload's document, its names distinct per `cycle` so no cycle
    /// can be served by what an earlier one left behind.
    fn document(self, cycle: usize) -> (String, String, String) {
        let canonical = format!("/src/flow_{self:?}_{cycle}.ts").to_lowercase();
        let symbol = format!("g{cycle}");
        let source = match self {
            Workload::Diagnostic => format!(
                "declare function take(x: number): string;\n\
                 export function {symbol}() {{ return take(\"s\"); }}\n"
            ),
            Workload::Tuple => {
                format!("export function {symbol}(x: string) {{ return [x, 1]; }}\n")
            }
            Workload::ConditionalObjects => format!(
                "export function {symbol}(flag: boolean) {{\n\
                 \x20 if (flag) {{ return {{ a: 1 }}; }}\n\
                 \x20 return {{ b: \"s\" }};\n\
                 }}\n"
            ),
            Workload::FreshLiteral => {
                format!("export function {symbol}() {{ return \"a\"; }}\n")
            }
        };
        (canonical, symbol, source)
    }

    /// Whether `answer` is this workload's valid delivered content.
    #[track_caller]
    fn assert_valid(self, answer: &Answer) {
        let diagnostics: Vec<u32> = answer.diagnostics.iter().map(|d| d.0).collect();
        match self {
            Workload::Diagnostic => {
                assert_eq!(diagnostics, [2345], "{self:?}: {answer:#?}");
                assert_eq!(answer.rendered, "Primitive(String)", "{self:?}");
            }
            Workload::Tuple => {
                assert!(diagnostics.is_empty(), "{self:?}: {answer:#?}");
                assert!(
                    answer.rendered.starts_with("Array {")
                        && answer.rendered.contains("Primitive(Number)")
                        && answer.rendered.contains("Primitive(String)"),
                    "{self:?}: {answer:#?}"
                );
            }
            Workload::ConditionalObjects => {
                assert!(diagnostics.is_empty(), "{self:?}: {answer:#?}");
                assert!(
                    answer.rendered.starts_with("Union(")
                        && answer.rendered.matches("<Object(").count() == 2
                        && answer.rendered.contains("key: String(\"a\")")
                        && answer.rendered.contains("key: String(\"b\")"),
                    "{self:?}: {answer:#?}"
                );
            }
            Workload::FreshLiteral => {
                assert!(diagnostics.is_empty(), "{self:?}: {answer:#?}");
                assert_eq!(answer.rendered, "Primitive(String)", "{self:?}");
            }
        }
    }
}

fn function(canonical: &str, symbol: &str) -> FlowFunctionReturnIdentity {
    FlowFunctionReturnIdentity {
        anchor: AuthoredAnchor {
            canonical_id: verter_type_expr::OwnedStr::from(canonical),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: verter_type_expr::OwnedStr::from(symbol),
            space: LocatorSymbolSpace::Value,
        },
        function_part: FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    }
}

fn upsert(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical.to_string()),
        input_id: canonical.to_string(),
        source: Arc::from(source),
        file_language: verter_session::LanguageRegistry::global()
            .classify_static(canonical)
            .static_resolution(),
        aliases: Vec::new(),
    });
}

/// `workload`'s inferred return through the host's public flow-return
/// request.
fn demand(host: &VerterHost, workload: Workload, cycle: usize) -> Arc<FlowReturnResult> {
    let (canonical, symbol, source) = workload.document(cycle);
    upsert(host, &canonical, &source);
    host.get_flow_return_type_with_audit(
        &function(&canonical, &symbol),
        ReturnProjectionDemand::whole_return(),
    )
    .into_parts()
    .0
    .unwrap_or_else(|error| panic!("{workload:?} resolves: {error:?}"))
}

/// What a delivered result says, comparable across hosts that number their
/// nodes differently.
#[derive(Debug, PartialEq, Eq)]
struct Answer {
    diagnostics: Vec<(u32, String)>,
    rendered: String,
}

fn answer(host: &VerterHost, result: &FlowReturnResult) -> Answer {
    Answer {
        diagnostics: result
            .checker_diagnostics()
            .iter()
            .map(|diagnostic| (diagnostic.code().code(), diagnostic.full_message()))
            .collect(),
        rendered: render_node(host, result.return_type(), 4),
    }
}

/// The node's payload with every child id replaced by the child's own
/// rendering.
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

/// What `workload` answers on a host of its own, closed and drained before
/// it drops.
fn fresh_answer(workload: Workload, cycle: usize) -> Answer {
    let host = ClosingHost::fresh();
    let result = demand(&host, workload, cycle);
    answer(&host, &result)
}

/// The figures a released workload must return to: the account's charges
/// and every object count. Monotonic history (peaks, refusal totals, close
/// statistics) is not a retained figure.
fn retained(host: &VerterHost) -> HostRetentionSnapshot {
    HostRetentionSnapshot {
        peak_total_bytes: 0,
        refusals_pressure: 0,
        refusals_oversized: 0,
        refusals_active: 0,
        closes: Default::default(),
        ..host.retention_snapshot()
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
    let now = retained(host);
    assert!(
        now == *baseline,
        "{context}: the closed host must hold exactly its pre-demand footprint; differing: {:#?}",
        differing(&now, baseline)
    );
}

/// `snapshot` without the process-wide identity indexes' backing
/// capacities. While another live host shares those indexes, their records
/// and digests return exactly, but a collision bucket both hosts used may
/// keep the amortised capacity the index keeps for its remaining occupant.
fn shared_capacity(snapshot: HostRetentionSnapshot) -> HostRetentionSnapshot {
    use verter_type_engine::semantic_query_memo::{IdentityIndexSnapshot, InternIndexOccupancy};
    let occupied = |index: InternIndexOccupancy| InternIndexOccupancy {
        digest_capacity: 0,
        collision_entry_capacity: 0,
        ..index
    };
    let indexes = snapshot.identity_indexes;
    HostRetentionSnapshot {
        identity_indexes: IdentityIndexSnapshot {
            intersection_recipes: occupied(indexes.intersection_recipes),
            semantic_contexts: occupied(indexes.semantic_contexts),
            order_domains: occupied(indexes.order_domains),
            relate_keys: occupied(indexes.relate_keys),
            resolve_call_keys: occupied(indexes.resolve_call_keys),
        },
        ..snapshot
    }
}

/// [`assert_at_baseline`] while another live host shares the process-wide
/// identity indexes.
#[track_caller]
fn assert_at_baseline_beside_live_host(
    host: &VerterHost,
    baseline: &HostRetentionSnapshot,
    context: &str,
) {
    let (now, expected) = (
        shared_capacity(retained(host)),
        shared_capacity(baseline.clone()),
    );
    assert!(
        now == expected,
        "{context}: the closed host must hold exactly its pre-demand footprint; differing: {:#?}",
        differing(&now, &expected)
    );
}

/// The demand must charge what the baseline comparison covers — semantic
/// nodes, flow-slice entries and account bytes — or the comparison proves
/// nothing.
#[track_caller]
fn assert_workload_charged(host: &VerterHost, baseline: &HostRetentionSnapshot, context: &str) {
    let during = retained(host);
    assert!(
        during.semantic_nodes > baseline.semantic_nodes
            && during.flow_graphs > baseline.flow_graphs
            && during.flow_lowered_entries > baseline.flow_lowered_entries
            && during.retained_bytes > baseline.retained_bytes
            && during.pinned_bytes > baseline.pinned_bytes,
        "{context}: the demand charges the figures the baseline covers: {during:#?}"
    );
}

/// Each workload's first demand on an unprimed host — a diagnostic-bearing
/// call, an array literal, conditional object literals and the fresh-literal
/// control — answers correctly and, once its reader drops and the host
/// closes and drains, leaves the host exactly at the footprint it had before
/// any demand. The closed host stays alive and answers the same demand again,
/// and closes back to the same footprint.
#[test]
fn a_fresh_hosts_first_flow_return_closes_back_to_its_pre_demand_footprint() {
    let _serial = serial();
    for workload in WORKLOADS {
        let expected = fresh_answer(workload, 0);
        workload.assert_valid(&expected);

        let host = ClosingHost::fresh();
        let baseline = retained(&host);
        let result = demand(&host, workload, 0);
        assert_eq!(answer(&host, &result), expected, "{workload:?} answers");
        assert_workload_charged(&host, &baseline, &format!("{workload:?}"));
        drop(result);
        host.retire();
        assert_at_baseline(
            &host,
            &baseline,
            &format!("{workload:?} after its first demand"),
        );

        let again = demand(&host, workload, 0);
        assert_eq!(
            answer(&host, &again),
            expected,
            "{workload:?}: the closed host answers the same demand again"
        );
        drop(again);
        host.retire();
        assert_at_baseline(&host, &baseline, &format!("{workload:?} after reuse"));
    }
}

/// Repeated demand/close cycles over every workload, with names distinct per
/// cycle, return to the same footprint each time: nothing accumulates.
#[test]
fn repeated_flow_return_close_cycles_accumulate_nothing() {
    let _serial = serial();
    let host = ClosingHost::fresh();
    let baseline = retained(&host);
    for cycle in 0..4 {
        let results: Vec<_> = WORKLOADS
            .iter()
            .map(|&workload| (workload, demand(&host, workload, cycle)))
            .collect();
        for (workload, result) in &results {
            workload.assert_valid(&answer(&host, result));
        }
        assert_workload_charged(&host, &baseline, &format!("cycle {cycle}"));
        drop(results);
        host.retire();
        assert_at_baseline(&host, &baseline, &format!("after cycle {cycle}"));
    }
}

/// The process account's charges, read without any host.
fn process_account() -> (usize, usize, usize) {
    let snapshot = SemanticRetentionAccount::process_local().snapshot();
    (
        snapshot.active_bytes,
        snapshot.retained_bytes,
        snapshot.pinned_bytes,
    )
}

/// Dropping a closed, drained host gives back every byte its lifetime
/// charged to the process account, its construction included: the
/// closed-retained reading is not hiding backing the host's drop would still
/// owe. An empty host already alive mints the process-permanent records any
/// store's construction needs, so the figure read before the measured host
/// exists is one it must return to.
#[test]
fn dropping_the_closed_host_returns_the_process_account_to_before_it_existed() {
    let _serial = serial();
    let _empty = ClosingHost::fresh();
    let before = process_account();
    {
        let host = ClosingHost::fresh();
        let results: Vec<_> = WORKLOADS
            .iter()
            .map(|&workload| demand(&host, workload, 0))
            .collect();
        assert_ne!(
            process_account(),
            before,
            "the workload charges the account"
        );
        drop(results);
        host.retire();
    }
    assert_eq!(
        process_account(),
        before,
        "the dropped host leaves no charge on the process account"
    );
}

/// Readers held across the close keep their answers — the return node, the
/// primitive vocabulary it may be, and the diagnostics — and exactly the
/// charges they measured: copies share every allocation, the shared payload
/// moves from retained to pinned once, and nothing is charged twice. Their
/// drops, in either order, bring the host to its pre-demand footprint, and
/// an unrelated live host and its reader are untouched throughout.
#[test]
fn held_readers_survive_close_and_release_in_either_order() {
    let _serial = serial();
    let unrelated = ClosingHost::fresh();
    let unrelated_reader = demand(&unrelated, Workload::Diagnostic, 9);
    let unrelated_answer = answer(&unrelated, &unrelated_reader);
    let unrelated_footprint = retained(&unrelated);

    for workload in [Workload::Diagnostic, Workload::FreshLiteral] {
        for copy_first in [true, false] {
            let context = format!(
                "{workload:?}, {}",
                if copy_first {
                    "copy dropped first"
                } else {
                    "original dropped first"
                }
            );
            let host = ClosingHost::fresh();
            let baseline = retained(&host);
            let held = demand(&host, workload, 0);
            let value = answer(&host, &held);
            workload.assert_valid(&value);
            let copy = Arc::new(FlowReturnResult::clone(&held));
            let cached = HeldChargeWitness::of_flow_return(&held);
            assert!(cached.is_exact(), "{context}: {cached:#?}");
            assert_eq!(
                HeldChargeWitness::of_flow_return(&copy),
                cached,
                "{context}: a copy shares every allocation and charge"
            );

            host.retire();
            for (name, reader) in [("original", &held), ("copy", &copy)] {
                assert_eq!(
                    answer(&host, reader),
                    value,
                    "{context}: the {name} keeps its answer through the close"
                );
                reader
                    .return_root()
                    .expect("a held reader keeps its return node live");
            }
            let closed = HeldChargeWitness::of_flow_return(&held);
            assert_eq!(
                HeldChargeWitness::of_flow_return(&copy),
                closed,
                "{context}: after the close the copies still share every charge"
            );
            assert_eq!(
                closed.payload().allocations().count(),
                cached.payload().allocations().count(),
                "{context}: the close moves the payload, it does not copy it"
            );
            assert!(
                closed.payload().entries().iter().all(|entry| {
                    entry.class == Some(ChargeClass::Pinned)
                        && entry.charged_bytes == entry.measured_bytes
                }),
                "{context}: the payload is its readers' alone, pinned once: {closed:#?}"
            );
            assert_eq!(
                closed.charged_bytes_in(ChargeClass::Retained),
                0,
                "{context}: with the cache retired nothing the readers keep is retained"
            );

            let (first, last) = if copy_first {
                (copy, held)
            } else {
                (held, copy)
            };
            drop(first);
            drain(&host);
            assert_eq!(
                answer(&host, &last),
                value,
                "{context}: the last reader keeps its answer"
            );
            assert_eq!(
                HeldChargeWitness::of_flow_return(&last),
                closed,
                "{context}: dropping a copy moves no shared charge"
            );
            drop(last);
            drain(&host);
            assert_at_baseline_beside_live_host(&host, &baseline, &context);
            // The account is process-wide: the closed host's own tables stay
            // allocated, and charged, until it drops. It goes with its own
            // store storage before the unrelated host, measured on the same
            // process account, is compared with its footprint.
            drop(host);

            assert_eq!(
                answer(&unrelated, &unrelated_reader),
                unrelated_answer,
                "{context}: the unrelated reader keeps its answer"
            );
            let (now, expected) = (
                shared_capacity(retained(&unrelated)),
                shared_capacity(unrelated_footprint.clone()),
            );
            assert!(
                now == expected,
                "{context}: the unrelated host keeps its data and charges; differing: {:#?}",
                differing(&now, &expected)
            );
        }
    }
}

/// A request that delivers no value — cancelled before it starts, or naming
/// a function the document does not declare — leaves an unprimed host
/// exactly at its pre-demand footprint after the close, and the host still
/// answers a valid demand correctly afterwards.
#[test]
fn a_cancelled_or_failed_first_request_closes_back_to_the_pre_demand_footprint() {
    let _serial = serial();
    let expected = fresh_answer(Workload::Diagnostic, 0);
    for cancelled in [true, false] {
        let host = ClosingHost::fresh();
        let baseline = retained(&host);
        let (canonical, symbol, source) = Workload::Diagnostic.document(0);
        upsert(&host, &canonical, &source);
        let outcome = if cancelled {
            let token = CancellationToken::new();
            token.cancel();
            host.get_flow_return_type_with_audit_cancellable(
                &function(&canonical, &symbol),
                ReturnProjectionDemand::whole_return(),
                token,
            )
        } else {
            host.get_flow_return_type_with_audit(
                &function(&canonical, "absent"),
                ReturnProjectionDemand::whole_return(),
            )
        }
        .into_parts()
        .0;
        match (cancelled, &outcome) {
            (true, Err(FlowReturnError::Cancelled)) | (false, Err(FlowReturnError::Failure(_))) => {
            }
            _ => panic!("cancelled={cancelled}: unexpected outcome {outcome:?}"),
        }
        drop(outcome);
        host.retire();
        assert_at_baseline(&host, &baseline, &format!("cancelled={cancelled}"));

        let result = demand(&host, Workload::Diagnostic, 0);
        assert_eq!(answer(&host, &result), expected, "cancelled={cancelled}");
        drop(result);
        host.retire();
        assert_at_baseline(&host, &baseline, &format!("cancelled={cancelled}, reused"));
    }
}

/// A request cancelled while its work is under way — after the flow-slice
/// graph bundles and primitive vocabulary it demands are acquired — still
/// closes the host back to its pre-demand footprint. Cancelling as the Nth
/// query family opens sweeps the cancellation across the request's work; the
/// sweep must land at least one cancellation that left flow owners charged
/// before the close, so the baseline comparison has residue to release.
#[test]
fn a_request_cancelled_mid_work_closes_back_to_the_pre_demand_footprint() {
    let _serial = serial();
    let expected = fresh_answer(Workload::Diagnostic, 0);
    let mut cancelled_holding_owners = 0usize;
    let mut completed = false;
    for nth in [1usize, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233, 377, 610] {
        let host = ClosingHost::fresh();
        let baseline = retained(&host);
        let (canonical, symbol, source) = Workload::Diagnostic.document(0);
        upsert(&host, &canonical, &source);
        let token = CancellationToken::new();
        let outcome = {
            let _cancel =
                verter_type_engine::cancel_trace::cancel_at_family_entry(token.clone(), nth);
            host.get_flow_return_type_with_audit_cancellable(
                &function(&canonical, &symbol),
                ReturnProjectionDemand::whole_return(),
                token,
            )
        }
        .into_parts()
        .0;
        let context = format!("cancelled as family {nth} opens");
        match &outcome {
            Err(FlowReturnError::Cancelled) => {
                let during = retained(&host);
                if during.flow_graphs > baseline.flow_graphs
                    || during.flow_lowered_entries > baseline.flow_lowered_entries
                    || during.semantic_nodes > baseline.semantic_nodes
                {
                    cancelled_holding_owners += 1;
                }
            }
            Ok(result) => {
                Workload::Diagnostic.assert_valid(&answer(&host, result));
                completed = true;
            }
            Err(other) => panic!("{context}: unexpected outcome {other:?}"),
        }
        drop(outcome);
        host.retire();
        assert_at_baseline(&host, &baseline, &context);

        let result = demand(&host, Workload::Diagnostic, 0);
        assert_eq!(answer(&host, &result), expected, "{context}");
        drop(result);
        host.retire();
        assert_at_baseline(&host, &baseline, &format!("{context}, reused"));
    }
    assert!(
        cancelled_holding_owners > 0,
        "some cancellation landed after the request acquired flow owners"
    );
    assert!(
        completed,
        "the sweep reaches past the request's last family"
    );
}
