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
    /// Per-host gate on the timing-capture surface.
    ///
    /// When `true`, the session-side `HostAuditRuntime` spawns the
    /// host-owned peak-RSS sampler thread (native only) on the
    /// first audit-enabled request and per-file timing helpers run
    /// their `Instant::now()` captures. When `false`, the sampler
    /// does not spawn, per-request peak slots stay at `0`, and
    /// timing helpers short-circuit.
    ///
    /// Validation in the session layer requires
    /// `audit_enabled = true` whenever this flag is enabled (see
    /// `HostConfigError::TimingCaptureWithoutAudit`). Default
    /// `false`.
    pub audit_timing_capture: bool,
}

/// Per-category caps on the unbounded `Vec` lanes inside
/// `RequestFootprintAudit` / `AccumulatorState`. Each cap bounds the
/// `Vec::push` count at the accumulator surface; once a cap is
/// reached, subsequent push attempts increment the matching
/// `*_truncated` counter on the audit record and the item is dropped.
///
/// The caps protect the LSP / MCP host from the unbounded-growth OOM
/// observed on pathological fixtures (ChatMessages.vue produced an
/// 8.4 GB audit JSON pre-cap). Every cap is generous by default so
/// typical requests are unaffected and only the pathological cases
/// see truncation.
///
/// All fields are `Option<usize>` so callers can override individual
/// caps without restating the defaults. `None` ⇒ use the default for
/// that category. A cap of `Some(0)` is honored literally (drop every
/// push) — production callers that want "no cap" should use a very
/// large value (e.g. `usize::MAX`) explicitly.
#[derive(Debug, Clone, Default)]
pub struct AuditCaps {
    /// Cap on `RequestFootprintAudit::structured_events`. Default:
    /// `Self::DEFAULT_STRUCTURED_EVENTS`.
    pub structured_events: Option<usize>,
    /// Cap on `DerivationSubgraph::nodes`. Default:
    /// `Self::DEFAULT_DERIVATION_NODES`. Enforced post-mining
    /// because nodes are derived from the canonicalised edge set.
    pub derivation_nodes: Option<usize>,
    /// Cap on `DerivationSubgraph::edges` (raw push at the
    /// accumulator). Default: `Self::DEFAULT_DERIVATION_EDGES`.
    /// The miner's existing `max_edges` cap continues to operate
    /// on the post-canonicalised edge set.
    pub derivation_edges: Option<usize>,
    /// Cap on `RequestFootprintAudit::vfs_reads`. Default:
    /// `Self::DEFAULT_VFS_READS`.
    pub vfs_reads: Option<usize>,
    /// Cap on `RequestFootprintAudit::indexed_ready_builds`. Default:
    /// `Self::DEFAULT_INDEXED_READY_BUILDS`. This corresponds to
    /// the "files" lane in the OOM analysis.
    pub indexed_ready_builds: Option<usize>,
    /// Cap on `RequestFootprintAudit::materializations`. Default:
    /// `Self::DEFAULT_MATERIALIZATIONS`.
    pub materializations: Option<usize>,
    /// Cap on `RequestFootprintAudit::instantiations`. Default:
    /// `Self::DEFAULT_INSTANTIATIONS`.
    pub instantiations: Option<usize>,
    /// Cap on `RequestFootprintAudit::substitutions`. Default:
    /// `Self::DEFAULT_SUBSTITUTIONS`.
    pub substitutions: Option<usize>,
    /// Cap on `RequestFootprintAudit::projections`. Default:
    /// `Self::DEFAULT_PROJECTIONS`.
    pub projections: Option<usize>,
    /// Cap on `RequestFootprintAudit::conditional_decisions`. Default:
    /// `Self::DEFAULT_CONDITIONAL_DECISIONS`.
    pub conditional_decisions: Option<usize>,
    /// Cap on `RequestFootprintAudit::alias_resolutions`. Default:
    /// `Self::DEFAULT_ALIAS_RESOLUTIONS`.
    pub alias_resolutions: Option<usize>,
    /// Cap on `RequestFootprintAudit::shared_load_reuses`. Default:
    /// `Self::DEFAULT_SHARED_LOAD_REUSES`.
    pub shared_load_reuses: Option<usize>,
}

impl AuditCaps {
    /// Default cap on structured events. Generous (production traces
    /// of normal requests are well under this) but bounded for the
    /// pathological case.
    pub const DEFAULT_STRUCTURED_EVENTS: usize = 10_000;
    /// Default cap on canonicalised derivation nodes.
    pub const DEFAULT_DERIVATION_NODES: usize = 10_000;
    /// Default cap on raw derivation edges captured at the
    /// accumulator. The miner's `max_derivation_edges` continues
    /// to apply post-canonicalisation.
    pub const DEFAULT_DERIVATION_EDGES: usize = 10_000;
    /// Default cap on VFS reads.
    pub const DEFAULT_VFS_READS: usize = 10_000;
    /// Default cap on `IndexedReady` build records (the "files"
    /// lane).
    pub const DEFAULT_INDEXED_READY_BUILDS: usize = 10_000;
    /// Default cap on materialization envelopes.
    pub const DEFAULT_MATERIALIZATIONS: usize = 10_000;
    /// Default cap on instantiation steps.
    pub const DEFAULT_INSTANTIATIONS: usize = 10_000;
    /// Default cap on substitution steps.
    pub const DEFAULT_SUBSTITUTIONS: usize = 10_000;
    /// Default cap on projection steps.
    pub const DEFAULT_PROJECTIONS: usize = 10_000;
    /// Default cap on conditional-branch decisions.
    pub const DEFAULT_CONDITIONAL_DECISIONS: usize = 10_000;
    /// Default cap on alias-resolve hops.
    pub const DEFAULT_ALIAS_RESOLUTIONS: usize = 10_000;
    /// Default cap on shared-load reuse records.
    pub const DEFAULT_SHARED_LOAD_REUSES: usize = 10_000;

    /// Resolved cap for `structured_events` (override → default).
    #[must_use]
    pub fn structured_events(&self) -> usize {
        self.structured_events
            .unwrap_or(Self::DEFAULT_STRUCTURED_EVENTS)
    }
    /// Resolved cap for `derivation_nodes`.
    #[must_use]
    pub fn derivation_nodes(&self) -> usize {
        self.derivation_nodes
            .unwrap_or(Self::DEFAULT_DERIVATION_NODES)
    }
    /// Resolved cap for `derivation_edges`.
    #[must_use]
    pub fn derivation_edges(&self) -> usize {
        self.derivation_edges
            .unwrap_or(Self::DEFAULT_DERIVATION_EDGES)
    }
    /// Resolved cap for `vfs_reads`.
    #[must_use]
    pub fn vfs_reads(&self) -> usize {
        self.vfs_reads.unwrap_or(Self::DEFAULT_VFS_READS)
    }
    /// Resolved cap for `indexed_ready_builds`.
    #[must_use]
    pub fn indexed_ready_builds(&self) -> usize {
        self.indexed_ready_builds
            .unwrap_or(Self::DEFAULT_INDEXED_READY_BUILDS)
    }
    /// Resolved cap for `materializations`.
    #[must_use]
    pub fn materializations(&self) -> usize {
        self.materializations
            .unwrap_or(Self::DEFAULT_MATERIALIZATIONS)
    }
    /// Resolved cap for `instantiations`.
    #[must_use]
    pub fn instantiations(&self) -> usize {
        self.instantiations.unwrap_or(Self::DEFAULT_INSTANTIATIONS)
    }
    /// Resolved cap for `substitutions`.
    #[must_use]
    pub fn substitutions(&self) -> usize {
        self.substitutions.unwrap_or(Self::DEFAULT_SUBSTITUTIONS)
    }
    /// Resolved cap for `projections`.
    #[must_use]
    pub fn projections(&self) -> usize {
        self.projections.unwrap_or(Self::DEFAULT_PROJECTIONS)
    }
    /// Resolved cap for `conditional_decisions`.
    #[must_use]
    pub fn conditional_decisions(&self) -> usize {
        self.conditional_decisions
            .unwrap_or(Self::DEFAULT_CONDITIONAL_DECISIONS)
    }
    /// Resolved cap for `alias_resolutions`.
    #[must_use]
    pub fn alias_resolutions(&self) -> usize {
        self.alias_resolutions
            .unwrap_or(Self::DEFAULT_ALIAS_RESOLUTIONS)
    }
    /// Resolved cap for `shared_load_reuses`.
    #[must_use]
    pub fn shared_load_reuses(&self) -> usize {
        self.shared_load_reuses
            .unwrap_or(Self::DEFAULT_SHARED_LOAD_REUSES)
    }
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
