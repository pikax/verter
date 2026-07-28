//! Compile-FAIL fixture: the struct-literal half of the `TscResponse`
//! child-module seal — see
//! `tsc_response_raw_field_read_outside_child_module.rs` for the raw-read
//! half and the full rationale. A from-parts construction that bypasses
//! `TscResponse::new`, written in the sibling position a future helper in
//! `types.rs` would occupy, must fail E0451: the rendering fields are
//! private to the `tsc_response` CHILD module, so the literal cannot name
//! them even from directly beside the re-export. Pinned in its own fixture
//! because rustc suppresses the literal's E0451 privacy error whenever a
//! typeck E0616 (a raw field read) exists in the same crate.
//!
//! DISCRIMINATING: widening the fields to `pub(crate)` makes this literal
//! compile, failing the trybuild expectation.
#![allow(dead_code)]

mod types {
    mod tsc_response {
        include!("../../../src/types/tsc_response.rs");
    }
    pub use tsc_response::TscResponse;

    fn sibling_struct_literal() -> TscResponse {
        TscResponse {
            code: std::sync::Arc::from(""),
            ts_carrier_code: None,
            source_map: None,
            dialect: verter_compiler::tsc::SfcScriptDialect::TypeScript,
        }
    }
}

fn main() {}
