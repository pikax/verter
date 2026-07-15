//! Override-identity cache-key dimension for fallthrough.
//!
//! The fallthrough cache key's `overrides` dimension records ONLY whether the
//! request carries a non-empty child prop-type override set. A non-empty set is
//! WHOLESALE uncacheable: override-bearing fallthrough is recomputed cold on
//! every request and never warms — or shares a singleflight lane with — the
//! shared cache, so two genuinely-different override sets can never alias one
//! warm surface. No-override fallthrough, intrinsic surfaces, the semantic-graph
//! caches, and the final component-meta result cache carry the warm-state value.
//!
//! This is an INTERNAL cache-key type (no public / wire DTO, no new CRITICAL
//! rule). It carries no `whole_hash` / `content_hash` / raw
//! [`crate::semantic_query::SemanticNodeId`] (R6) — it is a content-free unit
//! discriminator.

/// Override-identity cache-key dimension: either the request carries no
/// overrides (cacheable) or it carries a non-empty override set, which is
/// wholesale [`Self::Uncacheable`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub enum FallthroughOverrideIdentity {
    /// No overrides — a `None` set or an empty set. The request is cacheable.
    #[default]
    NoOverrides,
    /// A non-empty override set. Override-bearing fallthrough is wholesale
    /// uncacheable: the request skips warm lookup, cache admission, AND
    /// singleflight (computed cold, returned-only).
    Uncacheable,
}

impl FallthroughOverrideIdentity {
    /// `None` or an empty set → [`Self::NoOverrides`]; any non-empty set →
    /// [`Self::Uncacheable`].
    #[must_use]
    pub fn for_overrides(
        overrides: Option<&crate::resolver_core::FallthroughPropOverrideSet>,
    ) -> Self {
        match overrides {
            Some(set) if !set.is_empty() => Self::Uncacheable,
            _ => Self::NoOverrides,
        }
    }
}
