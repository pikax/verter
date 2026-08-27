//! `verter_semantic::resolver_core::ResolverObservation::function_body_skeleton`'s
//! query key.
//!
//! Dependency-neutral, narrowed mirror of `verter_session::cache_runtime::
//! flow_slice_node::FlowSliceFunctionKey`: same content-pinned function
//! identity (canonical, five-axis function program identity,
//! `flow_body_stable_hash`, `flow_body_exact_hash`, `parse_env_hash`), but
//! OMITS the session-private `build_toolchain_fingerprint` dimension
//! entirely — same treatment as `FileArtifactKey`'s narrow observation-side
//! mirror. The session side reconstructs the full
//! `FlowSliceFunctionKey` (adding its own live `build_toolchain_fingerprint`)
//! before doing the actual store lookup; that reconstruction, and the
//! store lookup itself, are session-only concerns this type never needs to
//! know about.

use std::sync::Arc;

use crate::analysis::function_program::FunctionProgramKey;
use crate::analysis::Hash16;

/// The content-pinned function identity a `function_body_skeleton`
/// observation is keyed on.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FlowFunctionObservationKey {
    /// Canonical id of the file serving the function.
    pub canonical_id: Arc<str>,
    /// The five-axis function program identity.
    pub function: FunctionProgramKey,
    /// The whole-function body-sensitive / cosmetic-insensitive hash.
    pub flow_body_stable_hash: Hash16,
    /// The exact byte hash of the function's own source text — see
    /// `FlowSliceFunctionKey`'s doc comment for why both hashes are
    /// required together (neither alone is a sound content-addressed
    /// oracle for position-carrying artifacts).
    pub flow_body_exact_hash: Hash16,
    /// Parse-domain env hash.
    pub parse_env_hash: Hash16,
}
