//! WASM negative test for `current_process_rss()`.
//!
//! WASM has no concept of process working-set accounting — the
//! function explicitly falls through to `0` via
//! `#[cfg(target_arch = "wasm32")]`. This test pins that contract:
//! when the target_arch is wasm32, callers MUST observe `0` rather
//! than e.g. a panic, junk value, or a Linux/macOS arm leaking
//! through.
//!
//! Sibling positive: `process_rss_windows.rs` covers the Windows arm.

#![cfg(target_arch = "wasm32")]

use verter_session::component_meta_audit::current_process_rss;

#[test]
fn current_process_rss_returns_zero_on_wasm32() {
    let rss = current_process_rss();
    assert_eq!(
        rss, 0,
        "WASM has no process RSS; current_process_rss() must \
         return 0, got {rss}",
    );
}
