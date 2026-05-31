//! Consolidated integration-test group `session`: each module below was
//! a separate top-level tests/*.rs binary, merged to cut test-link count.
#[path = "g_session/session_meta_store_view_regression.rs"]
mod session_meta_store_view_regression;
#[path = "g_session/session_view_dep_overlay_invalidates_warm.rs"]
mod session_view_dep_overlay_invalidates_warm;
#[path = "g_session/session_view_isolation.rs"]
mod session_view_isolation;
#[path = "g_session/session_view_smoke.rs"]
mod session_view_smoke;
#[path = "g_session/session_view_warm_reuse.rs"]
mod session_view_warm_reuse;
