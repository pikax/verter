//! In-flight semantic work observed while GENUINELY in flight, through the
//! accepted flight-rendezvous event coordination: a producer parked at its
//! cold-build boundary holds real ACTIVE bytes on the public account the
//! host's retention snapshot reports, its completion settles them, and a
//! cancelled participant's flight releases them. A host torn down to its
//! release boundary leaves no stale occupancy or provenance behind: a
//! reopened host's window opens at zero and attributes fresh work.

use std::sync::Arc;

use verter_session::{HostConfig, UpsertRequest, VerterHost};
use verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch;
use verter_type_engine::semantic_query::{
    BinderScopeId, CacheRead, QueryResult, ResolveDeclKey, ScopeId, SemanticNodeId,
    SemanticQueryKey,
};
use verter_type_expr::TopLevelOwnerId;

/// Every host charges the one process account, so tests that read its
/// ACTIVE transitions run one at a time, under the lock the
/// occupancy-window suite shares.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    super::active_occupancy_window::PROCESS_ACCOUNT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

const FILE: &str = "/src/in-flight-decl.ts";
const HANG_BOUND: std::time::Duration = std::time::Duration::from_secs(20);
/// One full granule of the arena's declared storage backing (64 KiB):
/// resolving `Target` freshly CONSTRUCTS that much graph storage while
/// only the construction owns it, so a window peak crossing this bound
/// measured the workload's real in-flight payload — the flight's own
/// storage and this delivery's metadata (a few KiB together) cannot
/// reach it.
const FRESH_CONSTRUCTION_FLOOR: usize = 64 * 1024;

/// A host closed and drained through the public completion boundary when
/// it drops — on a failing assertion's unwind too.
struct ClosingHost(VerterHost);

impl ClosingHost {
    fn new() -> Self {
        let host = Self(VerterHost::new_standalone(HostConfig::default()));
        let _ = host.0.upsert(UpsertRequest {
            canonical_id: Some(FILE.to_string()),
            input_id: FILE.to_string(),
            source: Arc::from(
                "import type { Base } from './in-flight-base';\n\
                 export interface Target extends Base { label: string }\n",
            ),
            file_language: verter_session::LanguageRegistry::global()
                .classify_static(FILE)
                .static_resolution(),
            aliases: Vec::new(),
        });
        let _ = host.0.upsert(UpsertRequest {
            canonical_id: Some("/src/in-flight-base.ts".to_string()),
            input_id: "/src/in-flight-base.ts".to_string(),
            source: Arc::from("export interface Base { size: number }\n"),
            file_language: verter_session::LanguageRegistry::global()
                .classify_static("/src/in-flight-base.ts")
                .static_resolution(),
            aliases: Vec::new(),
        });
        host
    }

    fn settle(&self) {
        self.0.close();
        self.0
            .release_completion()
            .wait()
            .expect("the decl-lowering workers apply the queued releases");
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
        self.settle();
    }
}

fn key() -> SemanticQueryKey {
    let owner = TopLevelOwnerId::ordinary_file();
    SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: ScopeId {
            canonical_id: verter_type_expr::OwnedStr::from(FILE),
            owner,
            local_scope: None,
            binder_scope_id: BinderScopeId::file_scope(owner),
        },
        name: verter_type_expr::OwnedStr::from("Target"),
    })
}

/// The declaration read through the dispatcher's synchronous path — the
/// path a flight rendezvous parks.
fn dispatch_read(host: &VerterHost) -> CacheRead<QueryResult<SemanticNodeId>> {
    ProjectSemanticDispatch::new(host).execute_read(key())
}

/// A producer parked at its cold-build boundary — its flight claimed, its
/// raw computation not begun — already holds real ACTIVE bytes on the
/// public account: the flight's own storage, charged when the dispatcher
/// admitted it. Released, it completes and settles; the account the
/// public snapshot reports carries the occupancy throughout.
#[test]
fn a_parked_producer_holds_real_active_bytes_while_genuinely_in_flight() {
    let _serial = serial();
    let host = ClosingHost::new();
    let graph = host.project_type_store().semantic_graph();
    let before = host.retention_snapshot();
    assert_eq!(
        before.active_bytes, 0,
        "a settled host holds no ACTIVE work"
    );

    let parked = std::thread::scope(|scope| {
        // The releasing guard is owned INSIDE the scope: a failing
        // assertion's unwind drops it — releasing the parked producer —
        // before the scope joins the producer's thread, so the failure
        // terminates and drains instead of hanging on the join.
        let rendezvous = graph.arm_flight_rendezvous(key());
        let producer = scope.spawn(|| dispatch_read(&host));
        rendezvous
            .wait_producer_parked(HANG_BOUND)
            .expect("the cold producer parks at its build boundary");
        let mid_flight = host.retention_snapshot();
        assert!(
            mid_flight.active_bytes > 0,
            "the parked flight holds real ACTIVE bytes while genuinely in flight: \
                 {mid_flight:?}"
        );
        rendezvous.release();
        let read = producer.join().expect("the producer completes");
        (host.retention_snapshot(), read)
    });
    let (mid_flight, read) = parked;
    assert!(mid_flight.active_bytes <= mid_flight.peak_total_bytes);
    assert!(
        matches!(read.value(), QueryResult::Value(_)),
        "the released producer delivers its complete answer"
    );
    let settled = host.retention_snapshot();
    assert!(
        settled.retained_bytes > before.retained_bytes,
        "the completed publication retained its answer: {settled:?}"
    );
    drop(read);
    host.settle();
    let drained = host.retention_snapshot();
    assert_eq!(
        drained.active_bytes, 0,
        "the settled flight leaves no ACTIVE occupancy"
    );
}

/// A host torn down to its release boundary leaves no stale occupancy or
/// provenance on the process account: a reopened host's window opens at
/// zero — no ACTIVE bytes of either provenance — and its own fresh
/// workload attributes a new positive peak to the new owner alone.
#[test]
fn a_reopened_host_opens_its_window_at_zero_and_attributes_fresh_work() {
    let _serial = serial();
    {
        let first = ClosingHost::new();
        drop(dispatch_read(&first));
        first.settle();
        assert_eq!(
            first.retention_snapshot().active_bytes,
            0,
            "the closed host drained its in-flight work through the public boundary"
        );
    }
    let second = ClosingHost::new();
    let owner = second.mint_active_occupancy_owner();
    let window = second.open_active_occupancy_window(&owner);
    let measured = dispatch_read(&second);
    assert!(matches!(measured.value(), QueryResult::Value(_)));
    let report = window.finish();
    let peak = report
        .attributed()
        .expect("the reopened host's workload is attributable to its own owner");
    assert_eq!(
        report.active_at_open, 0,
        "no stale occupancy from the torn-down host"
    );
    assert_eq!(
        report.unknown_active_at_open, 0,
        "no stale untagged provenance from the torn-down host"
    );
    assert!(
        peak.crossed(FRESH_CONSTRUCTION_FLOOR),
        "the reopened host's fresh work charged its real in-flight payload \
         — one full granule of freshly constructed graph storage — which \
         flight storage and delivery metadata (a few KiB together) cannot \
         explain: peak {}",
        peak.peak_active_bytes
    );
    assert_eq!(report.active_at_close, 0);
    drop(measured);
}
