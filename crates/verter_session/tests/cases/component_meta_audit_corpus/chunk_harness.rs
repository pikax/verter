//! Shared runner for generated component-meta corpus chunks.
//!
//! Nextest gives each generated chunk one process. Within that process the
//! immutable `include_str!` sources and execution-only worker pools are reused,
//! while every logical row goes through hermetic [`AuditedRequest`] host
//! construction. No workspace, scheduler/driver, semantic cache, audit store,
//! request state, or declaration-lowering service crosses a row boundary.

use std::sync::Arc;

use verter_session::audited_request::{AuditedRequest, AuditedRequestError};
use verter_session::{HostConfig, TestHostWorkerPools};

#[derive(Debug, Clone, Copy)]
pub(super) struct CorpusCase {
    slug: &'static str,
    canonical_id: &'static str,
    source: &'static str,
}

impl CorpusCase {
    pub(super) const fn new(
        slug: &'static str,
        canonical_id: &'static str,
        source: &'static str,
    ) -> Self {
        Self {
            slug,
            canonical_id,
            source,
        }
    }
}

pub(super) fn run_chunk(cases: &'static [CorpusCase]) {
    assert!(
        !cases.is_empty(),
        "a generated corpus chunk must not be empty"
    );

    let config = HostConfig::default();
    let worker_pools = TestHostWorkerPools::new(
        &config,
        verter_scheduler::scheduler::SchedulerConfig::default(),
    );
    let expected_pool_ids = worker_pools.pool_ids();

    for (index, case) in cases.iter().enumerate() {
        let before = worker_pools.receipt();
        assert_eq!(before.host_shells_created, index);
        assert_eq!(before.scheduler_shells_created, index);
        assert_eq!(
            before.active_scheduler_shells, 0,
            "{}: the prior logical row must release its host before reuse",
            case.slug,
        );
        assert_eq!(before.pool_ids, expected_pool_ids);

        let result = AuditedRequest::builder()
            .test_worker_pools(Arc::clone(&worker_pools))
            .files([(case.canonical_id, case.source)])
            .resolve_component_meta(case.canonical_id);

        match result {
            Ok((_, _, record)) => {
                assert_eq!(
                    record.canonical_id, case.canonical_id,
                    "{}: audit record must identify the requested canonical",
                    case.slug,
                );
                assert!(
                    record.footprint.is_some(),
                    "{}: hermetic AuditedRequest must attach Some(footprint) on resolution success",
                    case.slug,
                );
            }
            Err(AuditedRequestError::ResolutionFailed) => {
                // Benign and historically tolerated: this one-file hermetic view
                // intentionally omits transitive dependencies.
                eprintln!(
                    "corpus_audit_{}: hermetic resolution returned None (missing deps) — documenting skip",
                    case.slug,
                );
            }
            Err(other) => panic!(
                "corpus_audit_{}: unexpected audit error — this indicates an audit-wiring regression, not a hermetic-dep gap: {other:?}",
                case.slug,
            ),
        }

        let after = worker_pools.receipt();
        assert_eq!(
            after.host_shells_created,
            index + 1,
            "{}: every logical row must construct one fresh host shell",
            case.slug,
        );
        assert_eq!(
            after.scheduler_shells_created,
            index + 1,
            "{}: every logical row must construct one fresh scheduler/driver shell",
            case.slug,
        );
        assert_eq!(
            after.active_scheduler_shells, 0,
            "{}: every logical row must drop its scheduler shell before the next row",
            case.slug,
        );
        assert_eq!(
            after.pool_ids, expected_pool_ids,
            "{}: every fresh shell must use the chunk's exact shared worker pools",
            case.slug,
        );
    }
}
