//! Framework adapter plumbing owned by the compiler.
//!
//! Hosts the per-framework carrier bridges between the parser's typed
//! parse results and the framework-neutral
//! [`verter_language::FrameworkParseArtifact`]. The compiler is the one
//! crate BOTH producers (parse pipelines) and the session (carrier
//! consumers) can name without dependency cycles, so the concrete
//! `CarrierParse` wrappers live here rather than in `verter_parser`
//! (the wrapper is adapter plumbing, not parser data) or
//! `verter_session` (unnameable from compiler-side producers).

pub mod vue_bridge;
