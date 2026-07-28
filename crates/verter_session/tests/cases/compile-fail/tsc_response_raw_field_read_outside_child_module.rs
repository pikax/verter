//! Compile-FAIL fixture: `TscResponse`'s two rendering fields are private to
//! the `tsc_response` CHILD module, not merely to `crate::types`.
//!
//! Rust privacy is per-module INCLUDING descendants: a field declared
//! directly in `types.rs` stays readable from every sibling item and test
//! module in that file with no compiler objection — the raw-read wall would
//! have an inside. An out-of-crate fixture cannot see the difference (the
//! fields are private either way from here), so this fixture compiles the
//! REAL production module source (`src/types/tsc_response.rs`, spliced via
//! `include!` — the two cannot drift) into the same parent/child module
//! shape as `crate::types`, and attempts the exact sibling-position accesses
//! the boundary must reject:
//!
//! a raw read of `code` / `ts_carrier_code` from the parent module — the
//! position of any future helper added to `types.rs` — must be E0616. (The
//! sibling struct-literal vector is pinned separately in
//! `tsc_response_struct_literal_outside_child_module.rs`: rustc suppresses
//! the literal's E0451 privacy error once a typeck E0616 exists, so one
//! fixture cannot pin both.)
//!
//! DISCRIMINATING: widening either field to `pub(crate)` makes every access
//! below compile (crate-wide visibility in this includer crate too), failing
//! the trybuild expectation; moving the struct back into `types.rs` proper
//! breaks the `include!` path and fails the expected-stderr match.
#![allow(dead_code)]

mod types {
    mod tsc_response {
        include!("../../../src/types/tsc_response.rs");
    }
    pub use tsc_response::TscResponse;

    /// The exact position the child-module boundary exists to police: a
    /// helper living beside the re-export, OUTSIDE `tsc_response`.
    fn sibling_raw_dialect_channel_read(response: &TscResponse) -> std::sync::Arc<str> {
        std::sync::Arc::clone(&response.code)
    }

    fn sibling_raw_ts_channel_read(response: &TscResponse) -> Option<std::sync::Arc<str>> {
        response.ts_carrier_code.clone()
    }
}

fn main() {}
