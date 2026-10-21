//! A held answer's allocation charges, observed allocation by allocation.
//!
//! The answers are real: a delivered flow return from the host's public
//! flow-return request, and a dispatch read over ordinary upserts. Caches are
//! retired only by the host's public `close`, and every host is closed and
//! drained through the public release boundary when its test ends, failing
//! or not. The witness names the allocations a held answer keeps charged —
//! its delivered carriers, a read's cost receipts and the delivery owner's
//! bookkeeping — with the bytes each charge holds and the bytes each
//! allocation measures, so an exact transfer is checked against the
//! allocations themselves rather than against another transfer or the
//! account's shared totals.

use std::sync::Arc;

use verter_session::{HostConfig, UpsertRequest, VerterHost};
use verter_session_query::retention::shared::CallerHold;
use verter_session_query::retention::witness::{AllocationCharge, Reach};
use verter_session_query::retention::{ChargeClass, SemanticRetentionAccount};
use verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch;
use verter_type_engine::semantic_query::{
    BinderScopeId, CacheRead, FlowReturnResult, QueryResult, ResolveDeclKey,
    ReturnProjectionDemand, ScopeId, SemanticNodeId, SemanticQueryKey,
};
use verter_type_engine::semantic_query_memo::charge_witness::HeldChargeWitness;
use verter_type_expr::facts::{FlowFunctionReturnIdentity, FunctionPartIdentity};
use verter_type_expr::locators::{AuthoredAnchor, LocatorSymbolSpace};

/// Tests reading the process account's totals run one at a time, under
/// the lock the occupancy-window suite shares.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    super::active_occupancy_window::PROCESS_ACCOUNT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A host closed and drained through the public completion boundary when
/// it drops — on a failing assertion's unwind too.
struct ClosingHost(VerterHost);

impl ClosingHost {
    /// A fresh host that already served and released one unrelated
    /// workload, so the figures a test reads start from a settled host.
    fn settled() -> Self {
        let host = Self(VerterHost::new_standalone(HostConfig::default()));
        drop(flow_return(&host, 0));
        host.retire();
        host
    }

    /// Retire every cache owner and apply the queued releases.
    fn retire(&self) {
        self.0.close();
        self.0
            .release_completion()
            .wait()
            .expect("the decl-lowering workers apply the queued releases");
    }

    fn accounts(&self) -> (usize, usize, usize) {
        let snapshot = self.0.retention_snapshot();
        (
            snapshot.active_bytes,
            snapshot.retained_bytes,
            snapshot.pinned_bytes,
        )
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

fn upsert(host: &VerterHost, canonical: &str, source: String) {
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

/// `pick{cycle}`'s inferred return through the host's public flow-return
/// request. Its body calls `take` with an argument no overload accepts, so
/// the delivered result carries the checker's diagnostic list — an owned
/// allocation every copy of the result shares — beside its delivery's own
/// bookkeeping.
fn flow_return(host: &VerterHost, cycle: usize) -> Arc<FlowReturnResult> {
    let canonical = format!("/src/flow{cycle}.ts");
    upsert(
        host,
        &canonical,
        format!(
            "declare function take(x: number): string;\n\
             export function pick{cycle}(flag: boolean) {{\n\
             \x20 if (flag) {{ return \"a\"; }}\n\
             \x20 return take(\"s\");\n\
             }}\n"
        ),
    );
    let function = FlowFunctionReturnIdentity {
        anchor: AuthoredAnchor {
            canonical_id: verter_type_expr::OwnedStr::from(canonical.as_str()),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: verter_type_expr::OwnedStr::from(format!("pick{cycle}").as_str()),
            space: LocatorSymbolSpace::Value,
        },
        function_part: FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    };
    host.get_flow_return_type_with_audit(&function, ReturnProjectionDemand::whole_return())
        .into_parts()
        .0
        .expect("the flow return resolves")
}

/// The declaration `Box{cycle}` read through the dispatch. Its dependency
/// rail names both files, so the rail carries owned children, and the read
/// is priced by a cost receipt.
fn read(host: &VerterHost, cycle: usize) -> CacheRead<QueryResult<SemanticNodeId>> {
    let canonical = format!("/src/witness{cycle}.ts");
    upsert(
        host,
        &canonical,
        format!(
            "import type {{ Shared{cycle} }} from './shared{cycle}';\n\
             export interface Box{cycle} {{ shared: Shared{cycle}; label: string }}\n"
        ),
    );
    upsert(
        host,
        &format!("/src/shared{cycle}.ts"),
        format!("export interface Shared{cycle} {{ size: number }}\n"),
    );
    let owner = verter_type_expr::TopLevelOwnerId::ordinary_file();
    ProjectSemanticDispatch::new(host).execute_read(SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: ScopeId {
            canonical_id: verter_type_expr::OwnedStr::from(canonical.as_str()),
            owner,
            local_scope: None,
            binder_scope_id: BinderScopeId::file_scope(owner),
        },
        name: verter_type_expr::OwnedStr::from(format!("Box{cycle}").as_str()),
    }))
}

/// One allocation's figures without its address.
type Figures = (
    Reach,
    Option<ChargeClass>,
    usize,
    usize,
    Option<(usize, usize)>,
);

/// A witness's figures without the allocations' addresses, which differ
/// from host to host: what two executions of the same answer must agree on.
type Shape = Vec<Vec<Figures>>;

fn shape(witness: &HeldChargeWitness) -> Shape {
    witness
        .sections()
        .iter()
        .map(|section| {
            let mut figures: Vec<Figures> = section
                .entries()
                .iter()
                .map(|entry| {
                    (
                        entry.reach,
                        entry.class,
                        entry.charged_bytes,
                        entry.measured_bytes,
                        entry
                            .shared_block
                            .map(|block| (block.charged_bytes, block.measured_bytes)),
                    )
                })
                .collect();
            figures.sort_by_key(|figure| format!("{figure:?}"));
            figures
        })
        .collect()
}

/// The classes a section's entries hold, each once.
fn classes(entries: &[AllocationCharge]) -> Vec<Option<ChargeClass>> {
    let mut classes: Vec<_> = entries.iter().map(|entry| entry.class).collect();
    classes.sort_by_key(|class| format!("{class:?}"));
    classes.dedup();
    classes
}

/// The exact-transfer oracle. `after` must name exactly the allocations
/// `before` named, each once, each charged precisely the bytes it measured
/// before the transfer, with the same block share. The expectation comes
/// from the allocations' own measurements, never from another transfer.
fn exact_transfer(before: &[AllocationCharge], after: &[AllocationCharge]) -> Result<(), String> {
    if after.len() != before.len() {
        return Err(format!(
            "{} allocations observed after the transfer, {} before",
            after.len(),
            before.len()
        ));
    }
    for expected in before {
        let matching: Vec<_> = after
            .iter()
            .filter(|entry| entry.allocation == expected.allocation)
            .collect();
        let [entry] = matching.as_slice() else {
            return Err(format!(
                "{:?} observed {} times after the transfer",
                expected.allocation,
                matching.len()
            ));
        };
        if entry.class.is_none()
            || entry.charged_bytes != expected.measured_bytes
            || entry.measured_bytes != expected.measured_bytes
            || entry.shared_block != expected.shared_block
        {
            return Err(format!("{entry:?} is not the charge of {expected:?}"));
        }
    }
    let charged: usize = after.iter().map(|entry| entry.charged_bytes).sum();
    let measured: usize = before.iter().map(|entry| entry.measured_bytes).sum();
    if charged != measured {
        return Err(format!("{charged} bytes charged for {measured} measured"));
    }
    Ok(())
}

fn exact_witness_transfer(
    before: &HeldChargeWitness,
    after: &HeldChargeWitness,
) -> Result<(), String> {
    for (before, after) in before.sections().iter().zip(after.sections()) {
        exact_transfer(before.entries(), after.entries())?;
    }
    Ok(())
}

/// What a settled host's cold flow return, and the warm one after it, keep
/// charged; the account's totals are unchanged by reading either witness
/// twice.
fn cold_and_warm() -> (Shape, Shape) {
    let host = ClosingHost::settled();
    let cold = flow_return(&host, 1);
    let warm = flow_return(&host, 1);
    let accounts = host.accounts();
    let (cold_witness, warm_witness) = (
        HeldChargeWitness::of_flow_return(&cold),
        HeldChargeWitness::of_flow_return(&warm),
    );
    assert_eq!(
        (
            HeldChargeWitness::of_flow_return(&cold),
            HeldChargeWitness::of_flow_return(&warm)
        ),
        (cold_witness.clone(), warm_witness.clone()),
        "a repeated read observes the same allocations and charges"
    );
    assert_eq!(
        host.accounts(),
        accounts,
        "reading a witness charges, adopts and releases nothing"
    );
    assert_eq!(
        warm_witness.payload(),
        cold_witness.payload(),
        "the warm answer shares the payload the cold one populated the cache with"
    );
    assert!(
        warm_witness
            .root_metadata()
            .allocations()
            .all(|allocation| cold_witness.root_metadata().get(allocation).is_none()),
        "each delivery's bookkeeping is its own"
    );
    (shape(&cold_witness), shape(&warm_witness))
}

/// A cold answer that populates the cache and a warm answer the cache
/// already charged both name their exact allocation charges: the payload the
/// cache retains, and each delivery's pinned bookkeeping. The figures are
/// the allocations' own: a repeat execution on a fresh host and an execution
/// beside a large unrelated live charge observe the same ones.
#[test]
fn a_held_answer_names_its_exact_allocation_charges_cold_and_warm() {
    let _serial = serial();
    let (cold, warm) = cold_and_warm();
    for (stage, shape) in [("cold", &cold), ("warm", &warm)] {
        let [payload, receipts, metadata] = shape.as_slice() else {
            unreachable!("a witness has three sections");
        };
        assert!(
            !payload.is_empty()
                && payload
                    .iter()
                    .all(|entry| entry.1 == Some(ChargeClass::Retained)),
            "{stage}: the delivered payload is the cache's, retained: {payload:?}"
        );
        assert!(
            receipts.is_empty(),
            "{stage}: a flow return carries no receipt"
        );
        assert!(
            !metadata.is_empty()
                && metadata
                    .iter()
                    .all(|entry| entry.1 == Some(ChargeClass::Pinned)),
            "{stage}: the delivery's bookkeeping is pinned for its reader: {metadata:?}"
        );
        for entry in shape.iter().flatten() {
            assert!(
                entry.2 == entry.3,
                "{stage}: every allocation is charged exactly what it measures: {entry:?}"
            );
        }
    }

    assert_eq!(
        cold_and_warm(),
        (cold.clone(), warm.clone()),
        "a repeat execution observes the same charges"
    );

    let account = SemanticRetentionAccount::process_local();
    let pinned_before = account.snapshot().pinned_bytes;
    let unrelated = CallerHold::pin(&account, 64 << 20);
    assert!(
        account.snapshot().pinned_bytes >= pinned_before + (64 << 20),
        "the unrelated allocation is live on the process account"
    );
    assert_eq!(
        cold_and_warm(),
        (cold, warm),
        "unrelated account occupancy changes no observed charge"
    );
    drop(unrelated);
}

/// Public copies of a held answer share every observed allocation and
/// charge. Through the cache's retirement — before or after the copy drops —
/// the remaining reader keeps its value and exactly the charges it measured:
/// the shared payload moves from retained to pinned and the bookkeeping
/// stays pinned. A second answer sharing the payload keeps it charged when
/// the first goes, and the last reader releases it.
#[test]
fn copies_keep_exactly_the_measured_charge_through_retirement_in_either_order() {
    let _serial = serial();
    for retire_first in [true, false] {
        let order = if retire_first {
            "retire-first"
        } else {
            "copy-first"
        };
        let host = ClosingHost::settled();
        let held = flow_return(&host, 1);
        let other = flow_return(&host, 1);
        let value = format!("{:?}", held.checker_diagnostics());
        let (shared, copy) = (Arc::clone(&held), FlowReturnResult::clone(&held));
        let cached = HeldChargeWitness::of_flow_return(&held);
        assert_eq!(
            (
                HeldChargeWitness::of_flow_return(&shared),
                HeldChargeWitness::of_flow_return(&copy)
            ),
            (cached.clone(), cached.clone()),
            "{order}: a copy shares every allocation and charge"
        );
        assert_eq!(
            classes(cached.payload().entries()),
            [Some(ChargeClass::Retained)],
            "{order}: the cache retains the payload it delivered"
        );
        assert_eq!(
            classes(cached.root_metadata().entries()),
            [Some(ChargeClass::Pinned)],
            "{order}: the delivery's bookkeeping is charged apart from the payload"
        );
        assert!(cached.is_exact(), "{order}: {cached:#?}");

        if retire_first {
            host.retire();
            assert_eq!(
                HeldChargeWitness::of_flow_return(&copy),
                HeldChargeWitness::of_flow_return(&held),
                "{order}: after the retirement the copies still share every charge"
            );
            drop((shared, copy));
        } else {
            drop((shared, copy));
            assert_eq!(
                HeldChargeWitness::of_flow_return(&held),
                cached,
                "{order}: dropping the copies moves no charge while the cache holds it"
            );
            host.retire();
        }

        let remaining = HeldChargeWitness::of_flow_return(&held);
        exact_witness_transfer(&cached, &remaining)
            .unwrap_or_else(|error| panic!("{order}: {error}"));
        assert_eq!(
            classes(remaining.payload().entries()),
            [Some(ChargeClass::Pinned)],
            "{order}: the payload is its readers' alone, pinned"
        );
        assert_eq!(
            classes(remaining.root_metadata().entries()),
            [Some(ChargeClass::Pinned)],
            "{order}: the bookkeeping stays pinned for its reader"
        );
        assert_eq!(
            format!("{:?}", held.checker_diagnostics()),
            value,
            "{order}: the remaining reader keeps its value"
        );
        held.return_root()
            .expect("the remaining reader keeps its return node live");
        assert_eq!(
            remaining.charged_bytes_in(ChargeClass::Retained),
            0,
            "{order}: with the cache retired nothing the answer keeps is retained"
        );

        // The other answer shares the payload: the first reader's drop
        // leaves it charged, exactly, for the other.
        let other_before = HeldChargeWitness::of_flow_return(&other);
        assert_eq!(other_before.payload(), remaining.payload());
        drop(held);
        let last = HeldChargeWitness::of_flow_return(&other);
        assert_eq!(
            last, other_before,
            "{order}: the last reader keeps exactly the payload charge"
        );
        let pinned = host.accounts().2;
        drop(other);
        host.retire();
        let pinned_after = host.accounts().2;
        assert!(
            pinned - pinned_after
                >= last.payload().charged_bytes() + last.root_metadata().charged_bytes(),
            "{order}: the last reader releases the payload and its bookkeeping"
        );
    }
}

/// A transfer corrupted the same way in two identical executions still
/// fails the oracle in each: the expectation is each allocation's own
/// measurement, so two transfers that agree with each other prove nothing.
#[test]
fn dropped_doubled_and_partial_transfers_fail_even_when_two_transfers_agree() {
    let _serial = serial();
    let transfer = || {
        let host = ClosingHost::settled();
        let held = flow_return(&host, 1);
        let before = HeldChargeWitness::of_flow_return(&held);
        host.retire();
        let after = HeldChargeWitness::of_flow_return(&held);
        exact_witness_transfer(&before, &after).expect("the real transfer is exact");
        (
            before.payload().entries().to_vec(),
            after.payload().entries().to_vec(),
        )
    };
    let ((before_a, after_a), (before_b, after_b)) = (transfer(), transfer());
    let figures = |entries: &[AllocationCharge]| {
        let mut figures: Vec<_> = entries
            .iter()
            .map(|entry| (entry.reach, entry.class, entry.charged_bytes))
            .collect();
        figures.sort_by_key(|figure| format!("{figure:?}"));
        figures
    };
    let carrier = |entries: &[AllocationCharge]| {
        entries
            .iter()
            .position(|entry| entry.reach == Reach::Direct)
            .expect("a carrier is observed")
    };
    let controls: [(&str, fn(&mut Vec<AllocationCharge>, usize)); 3] = [
        ("dropped", |entries, at| {
            entries.remove(at);
        }),
        ("doubled", |entries, at| {
            let entry = entries[at];
            entries.push(entry);
        }),
        ("partial", |entries, at| {
            entries[at].charged_bytes /= 2;
        }),
    ];
    for (name, corrupt) in controls {
        let mut corrupted_a = after_a.clone();
        corrupt(&mut corrupted_a, carrier(&after_a));
        let mut corrupted_b = after_b.clone();
        corrupt(&mut corrupted_b, carrier(&after_b));
        assert_ne!(corrupted_a, after_a, "{name}: the control applied");
        assert_ne!(corrupted_b, after_b, "{name}: the control applied");
        assert_eq!(
            figures(&corrupted_a),
            figures(&corrupted_b),
            "{name}: the two corrupted transfers agree with each other"
        );
        assert!(
            exact_transfer(&before_a, &corrupted_a).is_err(),
            "{name}: the oracle rejects the first corrupted transfer"
        );
        assert!(
            exact_transfer(&before_b, &corrupted_b).is_err(),
            "{name}: the oracle rejects the second corrupted transfer"
        );
    }
}

/// A dispatch read keeps a rail with owned children and a priced receipt.
/// Through the cache's retirement the read's copies share every charge, the
/// carriers move to pinned exactly, a child the receipt also holds and the
/// receipts stay retained — everything still retained is what the read
/// keeps — and the owner's bookkeeping comes back pinned.
#[test]
fn a_read_keeps_its_carriers_receipts_and_bookkeeping_exactly_through_retirement() {
    let _serial = serial();
    let host = ClosingHost::settled();
    let (_, retained_baseline, _) = host.accounts();
    drop(read(&host, 1));
    let held = read(&host, 1);
    let copy = held.clone();
    let cached = HeldChargeWitness::of_read(&held);
    assert_eq!(HeldChargeWitness::of_read(&copy), cached);
    assert_eq!(
        classes(cached.payload().entries()),
        [Some(ChargeClass::Retained)],
        "the cache retains the carriers it delivered"
    );
    assert!(
        cached
            .payload()
            .entries()
            .iter()
            .any(|entry| entry.reach == Reach::Nested),
        "the rail holds owned children: {cached:#?}"
    );
    assert!(!cached.cost_receipts().is_empty(), "the read is priced");
    assert!(cached.is_exact(), "{cached:#?}");

    host.retire();
    let remaining = HeldChargeWitness::of_read(&held);
    assert_eq!(HeldChargeWitness::of_read(&copy), remaining);
    drop(copy);
    exact_witness_transfer(&cached, &remaining).unwrap_or_else(|error| panic!("{error}"));
    let direct: Vec<AllocationCharge> = remaining
        .payload()
        .entries()
        .iter()
        .filter(|entry| entry.reach == Reach::Direct)
        .copied()
        .collect();
    assert_eq!(
        classes(&direct),
        [Some(ChargeClass::Pinned)],
        "the carriers are their reader's alone, pinned"
    );
    assert_eq!(
        classes(remaining.cost_receipts().entries()),
        [Some(ChargeClass::Retained)],
        "the receipts keep their retained reservation"
    );
    assert_eq!(
        classes(remaining.root_metadata().entries()),
        [Some(ChargeClass::Pinned)],
        "the owner's bookkeeping is handed back to its reader"
    );
    assert_eq!(
        host.accounts().1 - retained_baseline,
        remaining.charged_bytes_in(ChargeClass::Retained),
        "with the cache retired, what the read keeps is everything retained"
    );

    drop(held);
    host.retire();
    assert_eq!(
        host.accounts().1,
        retained_baseline,
        "the last reader releases every retained charge it kept"
    );
}
