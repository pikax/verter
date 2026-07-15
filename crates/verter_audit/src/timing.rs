#![deny(missing_docs)]
//! Phase timings (wall-clock, milliseconds) carried on every audit
//! record envelope.

use serde::{Deserialize, Serialize};

/// Per-phase wall-clock timings in milliseconds. Producers initialise
/// to zero (default) and the request driver fills the matching block
/// at each phase boundary.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
pub struct RequestTimingAudit {
    /// End-to-end wall-clock for the request.
    pub total_ms: f64,
    /// Time spent capturing inputs (request args, config).
    pub capture_inputs_ms: f64,
    /// Time spent reading the project type store.
    pub store_read_ms: f64,
    /// Time spent merging new store data with the overlay view.
    pub store_merge_ms: f64,
    /// Time spent proving direct imports from the owner file.
    pub direct_import_proof_ms: f64,
    /// Time spent proving transitively-imported type roots.
    pub imported_root_proof_ms: f64,
    /// Time spent inside the type solver.
    pub solver_ms: f64,
    /// Time spent materializing member routes + public types.
    pub materialize_ms: f64,
    /// Time spent serializing the final component-meta payload.
    pub serialize_ms: f64,
    /// Sum of per-file `read_ms + parse_ms + lower_ms` across files
    /// THIS request triggered (i.e. read-once-aware critical path
    /// accounting). Always present at the envelope so consumers can
    /// observe a per-request critical path independently of the
    /// per-file [`crate::footprint::RequestFootprintAudit`] vector.
    /// Defaults to `0.0` when no producer has populated it.
    #[serde(default, skip_serializing_if = "is_default_f64")]
    pub request_critical_path_ms: f64,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_default_f64(value: &f64) -> bool {
    *value == 0.0
}
