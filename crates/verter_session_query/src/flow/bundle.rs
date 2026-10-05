//! A content-pinned function identity and the immutable flow-graph bundle built for it.

use crate::analysis::types::Hash16;
use crate::flow::flow_graph::{build_function_flow_graph, FunctionFlowGraph};
use crate::flow::{
    binding::FlowBindingMap,
    skeleton::{FunctionBodySkeleton, PreparedFunctionBodySkeleton},
};
use crate::function_program::{FunctionProgramEntry, FunctionProgramKey};
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
/// one, and a key does not prove its axes describe live source. A product
/// is bound to a key only through [`KeyedFunctionStructure::bind`], which
/// admits every axis against the acquired source identity; a bundle then
/// carries that key, and publication under any other key is refused.
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

/// The complete live source identity a flow structure is acquired under:
/// the serving file's canonical, its parse environment, exact parse
/// identity and language row, and the toolchain that built it.
///
/// Filled by the acquiring host; a key is admitted only when every one of
/// its source axes equals this identity ([`FlowSourceIdentity::admits`]).
/// The parse environment, parse identity and language row come from the
/// serving artifact, as do the function and body hashes (from its indexed
/// entry). The canonical and the toolchain are request-side axes: the
/// canonical is the id the artifact was served under and the toolchain is
/// this process's own fingerprint, so those two refuse a key minted for
/// another file or another build, not a stale artifact.
#[derive(Debug, Clone, Copy)]
pub struct FlowSourceIdentity<'a> {
    pub canonical_id: &'a str,
    pub parse_env_hash: Hash16,
    pub parse_key: &'a ParseKey,
    pub file_language: &'a FileLanguage,
    pub build_toolchain_fingerprint: crate::source::toolchain::BuildToolchainFingerprint,
}

/// The first key axis that does not describe the acquired source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowKeyMismatch {
    Canonical,
    Function,
    StableBodyHash,
    ExactBodyHash,
    ParseEnv,
    ParseKey,
    Language,
    Toolchain,
}

impl FlowSourceIdentity<'_> {
    /// Whether `key` names exactly this source's version of `entry`: every
    /// key axis — canonical, function, both body hashes, parse
    /// environment, parse identity, language row and toolchain — must
    /// match.
    pub fn admits(
        &self,
        key: &FlowSliceFunctionKey,
        entry: &FunctionProgramEntry,
    ) -> Result<(), FlowKeyMismatch> {
        if key.canonical_id.as_ref() != self.canonical_id {
            return Err(FlowKeyMismatch::Canonical);
        }
        if &key.function != entry.key() {
            return Err(FlowKeyMismatch::Function);
        }
        if key.flow_body_stable_hash != entry.flow_body_stable_hash() {
            return Err(FlowKeyMismatch::StableBodyHash);
        }
        if Some(key.flow_body_exact_hash) != entry.flow_body_exact_hash() {
            return Err(FlowKeyMismatch::ExactBodyHash);
        }
        if key.parse_env_hash != self.parse_env_hash {
            return Err(FlowKeyMismatch::ParseEnv);
        }
        if &key.parse_key != self.parse_key {
            return Err(FlowKeyMismatch::ParseKey);
        }
        if &key.file_language != self.file_language {
            return Err(FlowKeyMismatch::Language);
        }
        if key.build_toolchain_fingerprint != self.build_toolchain_fingerprint {
            return Err(FlowKeyMismatch::Toolchain);
        }
        Ok(())
    }
}

/// A prepared function structure bound to the content key it was
/// validated against. Both fields are private: [`Self::bind`] is the one
/// production constructor, and it admits the key against the acquired
/// source identity and the indexed function the structure was prepared
/// for.
pub struct KeyedFunctionStructure {
    key: FlowSliceFunctionKey,
    prepared: PreparedFunctionBodySkeleton,
}

impl KeyedFunctionStructure {
    /// Bind `prepared` (prepared for `entry`) to `key`, refusing a key any
    /// of whose axes does not describe `source`'s version of `entry`.
    pub fn bind(
        key: FlowSliceFunctionKey,
        prepared: PreparedFunctionBodySkeleton,
        entry: &FunctionProgramEntry,
        source: FlowSourceIdentity<'_>,
    ) -> Result<Self, FlowKeyMismatch> {
        source.admits(&key, entry)?;
        if prepared.bindings().function() != entry.key() {
            return Err(FlowKeyMismatch::Function);
        }
        Ok(Self { key, prepared })
    }

    /// Bind a hermetic fixture's structure to its fixture key. The fixture
    /// has no serving artifact, so only the function relationship is
    /// checked.
    #[cfg(any(test, feature = "test-support"))]
    pub fn bind_fixture(key: FlowSliceFunctionKey, prepared: PreparedFunctionBodySkeleton) -> Self {
        assert_eq!(
            &key.function,
            prepared.bindings().function(),
            "prepared fixture must match its content key"
        );
        Self { key, prepared }
    }

    /// The content key the structure was validated against.
    pub fn key(&self) -> &FlowSliceFunctionKey {
        &self.key
    }
}

/// One memoized per-function flow bundle: the content key it was built
/// for, the skeleton, the exact indexed binding correspondence, and the
/// graph, shared by every demand against the same content version.
///
/// Every field is private. [`FlowGraphBundle::build`] is the one
/// constructor: it takes a [`KeyedFunctionStructure`] and derives the graph
/// and the binding map from that one prepared structure, so a bundle's key,
/// graph and binding map always describe the same structure.
pub struct FlowGraphBundle {
    key: FlowSliceFunctionKey,
    skeleton: Arc<FunctionBodySkeleton>,
    bindings: Arc<FlowBindingMap>,
    graph: Arc<FunctionFlowGraph>,
}

impl FlowGraphBundle {
    /// Build the bundle of one keyed structure: the graph consumes the
    /// sealed prepared structure, whose binding map rides along unchanged.
    pub fn build(structure: KeyedFunctionStructure) -> Self {
        let KeyedFunctionStructure { key, prepared } = structure;
        let graph = build_function_flow_graph(&prepared);
        let (skeleton, bindings) = prepared.into_parts();
        Self {
            key,
            skeleton: Arc::new(skeleton),
            bindings: Arc::new(bindings),
            graph: Arc::new(graph),
        }
    }

    /// The content key the bundle was built for.
    pub fn key(&self) -> &FlowSliceFunctionKey {
        &self.key
    }

    /// The arena-free body skeleton.
    pub fn skeleton(&self) -> &Arc<FunctionBodySkeleton> {
        &self.skeleton
    }

    /// The authoritative indexed declaration map built once with the skeleton.
    pub fn bindings(&self) -> &Arc<FlowBindingMap> {
        &self.bindings
    }

    /// The typed-edge dependence graph built once from the skeleton.
    pub fn graph(&self) -> &Arc<FunctionFlowGraph> {
        &self.graph
    }
}

/// An immutable handle on one keyed flow bundle: the completeness-proof
/// layer's graph handle. Its key is the bundle's own, so a consumer can
/// never plan over one graph while naming another's body identity. There
/// is no constructor taking a separately supplied key.
pub struct BoundFlowGraph {
    bundle: Arc<FlowGraphBundle>,
}

impl BoundFlowGraph {
    /// The handle on a memoized keyed bundle.
    pub fn new(bundle: Arc<FlowGraphBundle>) -> Self {
        Self { bundle }
    }

    /// The content-pinned function identity the bundle was built for.
    pub fn key(&self) -> &FlowSliceFunctionKey {
        self.bundle.key()
    }

    /// The memoized bundle the key names.
    pub fn bundle(&self) -> &FlowGraphBundle {
        &self.bundle
    }
}
