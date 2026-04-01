//! Host/source-loading boundary for type expansion.
//!
//! [`TypeExpansionHost`] is a synchronous trait implemented by the source-of-truth
//! owner (likely `verter_session`). It provides coherent snapshots of SFC state.
//!
//! The host trait is sync on purpose:
//! - it reads already-materialized host state
//! - async I/O belongs before publication into the snapshot layer
//! - artifact generation must never observe mismatched source and structure
//!   from different revisions

use verter_span::Span;

use crate::resolver_core::type_expansion::TypeExpansionError;

// ---------------------------------------------------------------------------
// Host Trait
// ---------------------------------------------------------------------------

/// Provides coherent SFC snapshots for artifact construction.
///
/// Implemented by the source-of-truth owner (e.g., `verter_session::VerterHost`).
/// Defined in `verter_resolver` so the resolver can consume it without
/// depending on `verter_session`.
pub trait TypeExpansionHost: Send + Sync {
    /// Obtain a coherent snapshot of the SFC at the given canonical ID.
    ///
    /// Returns `Err(SourceUnavailable)` if the file doesn't exist or isn't loaded.
    fn snapshot_view(
        &self,
        canonical_id: &str,
    ) -> Result<TypeExpansionSnapshot, TypeExpansionError>;
}

// ---------------------------------------------------------------------------
// Snapshot DTOs (resolver-owned, not host-owned)
// ---------------------------------------------------------------------------

/// A coherent snapshot of an SFC's state at a point in time.
///
/// `revision` is monotonically increasing per `canonical_id` within a host session.
/// It drives per-session artifact cache invalidation.
#[derive(Debug, Clone)]
pub struct TypeExpansionSnapshot {
    /// The SFC source text.
    pub source: SourceSnapshot,
    /// Structural information about the SFC's blocks.
    pub sfc_structure: SfcStructure,
    /// Monotonically increasing revision per canonical_id.
    pub revision: u64,
}

/// SFC source content.
#[derive(Debug, Clone)]
pub struct SourceSnapshot {
    /// Full SFC source text.
    pub text: String,
    /// Script language (ts, js, tsx, jsx).
    pub lang: ScriptLang,
}

/// Script language of the SFC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ScriptLang {
    #[default]
    Ts,
    Js,
    Tsx,
    Jsx,
}

/// Structural description of an SFC's blocks.
///
/// Spans are SFC-absolute. This is a neutral DTO — it does not reference
/// parser-internal types from `verter_compiler`.
#[derive(Debug, Clone)]
pub struct SfcStructure {
    /// `<script>` block content span (SFC-absolute), if present.
    pub script: Option<SfcBlockSpan>,
    /// `<script setup>` block content span (SFC-absolute), if present.
    pub script_setup: Option<SfcBlockSpan>,
    /// `<template>` block content span (SFC-absolute), if present.
    pub template: Option<SfcBlockSpan>,
}

/// A block's content span within the SFC.
#[derive(Debug, Clone, Copy)]
pub struct SfcBlockSpan {
    /// SFC-absolute span of the block's content (between tags).
    pub content: Span,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sfc_structure_can_represent_dual_script_sfc() {
        let structure = SfcStructure {
            script: Some(SfcBlockSpan {
                content: Span::new(20, 150),
            }),
            script_setup: Some(SfcBlockSpan {
                content: Span::new(170, 300),
            }),
            template: Some(SfcBlockSpan {
                content: Span::new(320, 500),
            }),
        };
        assert!(structure.script.is_some());
        assert!(structure.script_setup.is_some());
        assert!(structure.template.is_some());
    }

    #[test]
    fn sfc_structure_can_represent_setup_only_sfc() {
        let structure = SfcStructure {
            script: None,
            script_setup: Some(SfcBlockSpan {
                content: Span::new(20, 200),
            }),
            template: Some(SfcBlockSpan {
                content: Span::new(220, 400),
            }),
        };
        assert!(structure.script.is_none());
        assert!(structure.script_setup.is_some());
    }

    #[test]
    fn snapshot_revision_is_monotonic_contract() {
        let snap1 = TypeExpansionSnapshot {
            source: SourceSnapshot {
                text: "v1".to_string(),
                lang: ScriptLang::Ts,
            },
            sfc_structure: SfcStructure {
                script: None,
                script_setup: Some(SfcBlockSpan {
                    content: Span::new(0, 10),
                }),
                template: None,
            },
            revision: 1,
        };
        let snap2 = TypeExpansionSnapshot {
            source: SourceSnapshot {
                text: "v2".to_string(),
                lang: ScriptLang::Ts,
            },
            sfc_structure: snap1.sfc_structure.clone(),
            revision: 2,
        };
        // revision must be monotonically increasing
        assert!(snap2.revision > snap1.revision);
    }

    #[test]
    fn script_lang_default_is_ts() {
        assert_eq!(ScriptLang::default(), ScriptLang::Ts);
    }
}
