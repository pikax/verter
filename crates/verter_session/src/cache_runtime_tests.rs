//! Session-composed suites for the shared cache runtime: each drives the
//! runtime through a live session host or session-owned fixtures.

pub(crate) mod flow_slice_node_tests;
mod node_tests;
mod singleflight_tests;
mod world_snapshot_tests;
