//! The out-of-band parse release is a test coordination hook: it breaks the
//! lease invariant on purpose. A build without `test-support` — this
//! feature-less lane — cannot reach it.

use verter_semantic_source::decl_lowering::DeclLoweringService;
use verter_session_query::source::snapshot::SnapshotKey;

fn release_out_of_band(service: &DeclLoweringService, key: &SnapshotKey) {
    service.release_retained_snapshot_for_test(key);
}

fn main() {}
