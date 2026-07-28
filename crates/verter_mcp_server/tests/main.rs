//! Single consolidated integration-test binary for `verter_mcp_server`.
//!
//! Per the anti-binary-growth layout rule, this is the crate's ONLY
//! top-level `tests/*.rs`; every entry lives as a private submodule under
//! [`cases`].

mod cases;
