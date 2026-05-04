#![deny(missing_docs)]
//! [`SemanticAnalysisPayload`] — strongly-typed payload for
//! `RequestKind::SemanticAnalysis`. Producer crates populate the data structure once they emit through the audit substrate.

use serde::{Deserialize, Serialize};

/// Semantic-analysis request payload.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct SemanticAnalysisPayload {
    /// Number of imports recorded by the analysis.
    pub num_imports: u32,
    /// Number of exports.
    pub num_exports: u32,
    /// Number of type declarations.
    pub num_type_decls: u32,
    /// Number of value declarations.
    pub num_value_decls: u32,
    /// Number of macro calls (`defineProps`, `defineEmits`, …).
    pub num_macro_calls: u32,
    /// Number of root-reachability edges captured.
    pub num_root_reachability_edges: u32,
    /// `true` when this request triggered a fresh `IndexedReady`
    /// build (false on warm-cache reuse).
    pub indexed_ready_built: bool,
}
