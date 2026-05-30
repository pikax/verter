//! Verter-native `TypeExpander` implementation.
//!
//! Wraps the existing OXC-based cross-file resolver behind the [`TypeExpander`]
//! trait. No external processes — pure Rust.
//!
//! The async boundary exists at host/source-loading edges. Core semantic work
//! stays synchronous.

use std::sync::Arc;

use verter_type_expr::{PrimitiveName, TypeExpr};

use crate::resolver_core::type_expansion::{
    ExpandedMember, ExpanderFuture, ExpansionCompleteness, TypeExpander, TypeExpansionError,
    TypeExpansionRequest, TypeExpansionResult,
};
use crate::resolver_core::ResolvedMacroMeta;

// ---------------------------------------------------------------------------
// VerterComponentMetaProvider trait
// ---------------------------------------------------------------------------

/// Trait for hosts that can provide component-meta results.
///
/// Implemented by `ComponentMetaHost` in `verter_session`. This allows
/// `VerterTypeExpander` to call into the existing resolution pipeline
/// without depending on `verter_session` directly.
pub trait VerterComponentMetaProvider: Send + Sync {
    /// Get full component metadata for a file (uses Verter's native resolver).
    fn get_component_meta(
        &self,
        canonical_id: &str,
    ) -> Option<verter_semantic::analysis::component_meta::ComponentMetaAnalysis>;

    /// Get the resolved macro metadata for a file (for type expansion).
    fn get_resolved_macros(&self, canonical_id: &str) -> Vec<ResolvedMacroMeta> {
        // Default: not available. Implementations override.
        let _ = canonical_id;
        vec![]
    }
}

// ---------------------------------------------------------------------------
// VerterTypeExpander
// ---------------------------------------------------------------------------

/// Native Verter `TypeExpander` — wraps the existing OXC-based resolver.
///
/// Resolves types using Verter's cross-file import graph traversal and
/// type evaluation. Does not spawn external processes.
///
/// Calls `VerterComponentMetaProvider::get_component_meta()` and extracts
/// the matching macro's resolved data for the requested span.
pub struct VerterTypeExpander {
    provider: Arc<dyn VerterComponentMetaProvider>,
}

impl VerterTypeExpander {
    pub fn new(provider: Arc<dyn VerterComponentMetaProvider>) -> Self {
        Self { provider }
    }
}

impl TypeExpander for VerterTypeExpander {
    fn expand_type<'a>(
        &'a self,
        request: &'a TypeExpansionRequest,
    ) -> ExpanderFuture<'a, TypeExpansionResult> {
        Box::pin(async move {
            let meta = self
                .provider
                .get_component_meta(&request.canonical_id)
                .ok_or(TypeExpansionError::SourceUnavailable)?;

            // Return all props as expanded members (the Verter backend
            // resolves the full component surface, not a single type).
            let members: Vec<ExpandedMember> = meta
                .props
                .iter()
                .map(|p| ExpandedMember {
                    name: p.name.clone(),
                    type_expr: p.type_expr.clone(),
                    raw_type: p.raw_type.clone(),
                    optional: !p.required,
                    description: p.description.clone(),
                })
                .collect();

            let completeness = if members.is_empty() {
                ExpansionCompleteness::LowerBound
            } else {
                ExpansionCompleteness::Exact
            };

            Ok(TypeExpansionResult {
                type_expr: TypeExpr::primitive(PrimitiveName::Unknown),
                members,
                completeness,
            })
        })
    }
}
