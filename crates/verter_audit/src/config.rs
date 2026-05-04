#![deny(missing_docs)]
//! [`AuditConfig`] + the [`AuditConsumerFilter`] bitset that decides
//! which [`crate::record::RequestKind`] variants emit records.
//!
//! The filter is read ONCE at registration time (`AuditRequestRegistration::new`)
//! and CANNOT change for that request's lifetime. Filtered kinds skip
//! the `active_requests` map and never produce a record — the public
//! audited API returns `None` for them.

use serde::{Deserialize, Serialize};

use crate::record::RequestKind;

/// Audit-config snapshot owned by the host. The session layer's
/// `HostAuditRuntime` wraps this in an `Arc` and exposes it to
/// `AuditRequestRegistration::new`.
#[derive(Debug, Clone, Default)]
pub struct AuditConfig {
    /// Filter bitset deciding which `RequestKind` variants emit
    /// records. The default allows every kind.
    pub consumer_filter: AuditConsumerFilter,
}

/// Bitset deciding which [`RequestKind`] variants emit records.
///
/// Default-constructed filters allow every kind. Use [`Self::deny`]
/// to strip a kind from the allow-set, or [`Self::allow_only`] to
/// build a filter from scratch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AuditConsumerFilter {
    /// Each bit corresponds to one [`RequestKind`] discriminant; see
    /// [`KindBit`].
    bits: u32,
}

impl Default for AuditConsumerFilter {
    fn default() -> Self {
        Self::allow_all()
    }
}

impl AuditConsumerFilter {
    /// Allow every kind.
    #[must_use]
    pub const fn allow_all() -> Self {
        Self { bits: u32::MAX }
    }

    /// Deny every kind (no records will be emitted).
    #[must_use]
    pub const fn deny_all() -> Self {
        Self { bits: 0 }
    }

    /// Build a filter that allows only the kinds in `kinds`.
    #[must_use]
    pub fn allow_only<I: IntoIterator<Item = KindBit>>(kinds: I) -> Self {
        let mut bits: u32 = 0;
        for kind in kinds {
            bits |= 1 << (kind as u32);
        }
        Self { bits }
    }

    /// Set the bit for `kind` so it is allowed.
    #[must_use]
    pub const fn allow(mut self, kind: KindBit) -> Self {
        self.bits |= 1 << (kind as u32);
        self
    }

    /// Clear the bit for `kind` so it is denied.
    #[must_use]
    pub const fn deny(mut self, kind: KindBit) -> Self {
        self.bits &= !(1 << (kind as u32));
        self
    }

    /// `true` when the filter allows `kind`.
    #[must_use]
    pub fn allows(&self, kind: &RequestKind) -> bool {
        let bit = KindBit::from_kind(kind);
        (self.bits >> (bit as u32)) & 1 == 1
    }
}

/// Stable bit position for each [`RequestKind`] discriminant. Used by
/// [`AuditConsumerFilter`] to encode which kinds a host accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum KindBit {
    /// `RequestKind::ComponentMeta`.
    ComponentMeta = 0,
    /// `RequestKind::TypeResolution`.
    TypeResolution = 1,
    /// `RequestKind::SemanticAnalysis`.
    SemanticAnalysis = 2,
    /// `RequestKind::Compile`.
    Compile = 3,
    /// `RequestKind::Workspace`.
    Workspace = 4,
    /// `RequestKind::Lsp`.
    Lsp = 5,
    /// `RequestKind::Mcp`.
    Mcp = 6,
    /// `RequestKind::BundlerBatch`.
    BundlerBatch = 7,
    /// `RequestKind::Custom`.
    Custom = 8,
}

impl KindBit {
    /// Map a [`RequestKind`] to its stable bit position.
    #[must_use]
    pub fn from_kind(kind: &RequestKind) -> Self {
        match kind {
            RequestKind::ComponentMeta => Self::ComponentMeta,
            RequestKind::TypeResolution => Self::TypeResolution,
            RequestKind::SemanticAnalysis => Self::SemanticAnalysis,
            RequestKind::Compile { .. } => Self::Compile,
            RequestKind::Workspace { .. } => Self::Workspace,
            RequestKind::Lsp { .. } => Self::Lsp,
            RequestKind::Mcp { .. } => Self::Mcp,
            RequestKind::BundlerBatch { .. } => Self::BundlerBatch,
            RequestKind::Custom { .. } => Self::Custom,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_filter_allows_every_kind() {
        let filter = AuditConsumerFilter::default();
        assert!(filter.allows(&RequestKind::ComponentMeta));
        assert!(filter.allows(&RequestKind::TypeResolution));
        assert!(filter.allows(&RequestKind::SemanticAnalysis));
        assert!(filter.allows(&RequestKind::Custom {
            name: "x".to_string(),
        }));
    }

    #[test]
    fn deny_all_blocks_every_kind() {
        let filter = AuditConsumerFilter::deny_all();
        assert!(!filter.allows(&RequestKind::ComponentMeta));
        assert!(!filter.allows(&RequestKind::TypeResolution));
    }

    #[test]
    fn allow_only_subset_admits_specific_kinds() {
        let filter = AuditConsumerFilter::allow_only([KindBit::ComponentMeta]);
        assert!(filter.allows(&RequestKind::ComponentMeta));
        assert!(!filter.allows(&RequestKind::TypeResolution));
    }

    #[test]
    fn deny_specific_kind_keeps_others() {
        let filter = AuditConsumerFilter::default().deny(KindBit::ComponentMeta);
        assert!(!filter.allows(&RequestKind::ComponentMeta));
        assert!(filter.allows(&RequestKind::TypeResolution));
    }
}
