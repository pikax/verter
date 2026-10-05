//! A content-pinned function identity and the immutable flow-graph bundle built for it.

use crate::analysis::types::Hash16;
use crate::flow::flow_graph::FunctionFlowGraph;
use crate::flow::{binding::FlowBindingMap, skeleton::FunctionBodySkeleton};
use crate::function_program::FunctionProgramKey;
use std::sync::Arc;
use verter_language::{FileLanguage, ParseKey};

/// The content-pinned function identity every flow-slice artifact keys
/// on: the canonical, the five-axis served-function identity, the
/// body-sensitive `flow_body_stable_hash` (NOT `parse_stable_hash` —
/// `return {{ b: 1 }}` vs `return {{ b: 2 }}` key distinct slices), the
/// EXACT per-function byte hash, the parse-env hash, the exact parse
/// identity ([`ParseKey`]) and runtime-authoritative language row
/// ([`FileLanguage`]) of the serving file — the same source axes
/// [`verter_session_query::source::artifact_key::FileArtifactKey`] carries — and the
/// parser version.
///
/// A public REQUEST/identity value, not a certificate: any crate may build
/// one, and a key does not prove its axes describe live source. Whatever
/// acquires or publishes a product under a key must bind that product to
/// the key's complete source identity itself.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FlowSliceFunctionKey {
    /// Canonical id of the file serving the function.
    pub canonical_id: Arc<str>,
    /// The five-axis function program identity.
    pub function: FunctionProgramKey,
    /// The whole-function body-sensitive / cosmetic-insensitive hash.
    pub flow_body_stable_hash: Hash16,
    /// The EXACT byte hash of the function's own source text.
    ///
    /// The artifacts this key addresses carry SOURCE POSITIONS, and the
    /// stable hash above cannot address those: it alpha-normalizes
    /// binding / reference identifiers and folds the AST rather than the
    /// text, so `const aa = 1` and `const aaaa = 1` share it while
    /// placing every position inside the body differently. Reuse across
    /// that boundary hands a plan positions that no longer address the
    /// code they were computed from.
    ///
    /// This is NOT a file-offset axis, and deliberately so: it covers
    /// the function's OWN bytes only, so an edit anywhere else in the
    /// file — a leading blank line, a sibling function's body — leaves
    /// it (and every anchor-relative position in the artifacts) intact.
    /// Reuse also requires the serving ParseKey below: that key pins the
    /// lexical source context of exact captured binding identities.
    pub flow_body_exact_hash: Hash16,
    /// Parse-domain env hash.
    pub parse_env_hash: Hash16,
    /// The exact parse identity (source bytes, language, compatibility
    /// domain/epoch, syntax profile) of the serving file — taken from the
    /// request-bound artifact identity, never reclassified from the path.
    pub parse_key: ParseKey,
    /// The runtime-authoritative [`FileLanguage`] row the serving file was
    /// parsed under — taken from the request-bound `IndexedReady`, never
    /// reclassified from the path.
    pub file_language: FileLanguage,
    /// Parser version.
    pub build_toolchain_fingerprint: crate::source::toolchain::BuildToolchainFingerprint,
}

/// One memoized per-function flow bundle: the skeleton, exact indexed
/// binding correspondence, and graph, shared by every demand against
/// the same content version.
pub struct FlowGraphBundle {
    /// The arena-free body skeleton.
    pub skeleton: Arc<FunctionBodySkeleton>,
    /// The authoritative indexed declaration map built once with the skeleton.
    pub bindings: Arc<FlowBindingMap>,
    /// The typed-edge dependence graph built once from the skeleton.
    pub graph: Arc<FunctionFlowGraph>,
}

/// A flow graph bundle SEALED to the store key it was built for. Fields
/// are private and the sole constructors are [`FunctionFlowGraphStore`]
/// methods, so a consumer can never plan over one graph while naming
/// another's body identity. This is the completeness-proof layer's graph
/// handle: the production demand preparation mints it from the memoized
/// bundle ([`FunctionFlowGraphStore::bound_graph`]); the gated test mint
/// below serves the hermetic fixtures.
pub struct BoundFlowGraph {
    pub key: FlowSliceFunctionKey,
    pub bundle: Arc<FlowGraphBundle>,
}

impl BoundFlowGraph {
    /// The content-pinned function identity the bundle was built for.
    pub fn key(&self) -> &FlowSliceFunctionKey {
        &self.key
    }

    /// The memoized bundle the key names.
    pub fn bundle(&self) -> &FlowGraphBundle {
        &self.bundle
    }
}
