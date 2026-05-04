#![deny(missing_docs)]
//! [`WorkspacePayload`] — strongly-typed payload for
//! `RequestKind::Workspace`. Producer crates populate the data structure once they emit through the audit substrate.

use serde::{Deserialize, Serialize};

use crate::record::u64_as_decimal_string;

/// Workspace operation discriminator. Mirrors the surface
/// `verter_workspace::WorkspaceAccess::audit_op` will provide in
/// a future producer.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS, PartialEq, Eq, Hash)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum WorkspaceOp {
    /// `resolve(specifier, from)` — module specifier resolution.
    AuditResolve {
        /// Module specifier being resolved.
        specifier: String,
        /// Importer canonical id, if any.
        from: Option<String>,
    },
    /// Dependency-graph traversal from a root canonical id.
    DepGraphTraverse {
        /// Root canonical id to traverse from.
        root: String,
    },
    /// Resolver walk for a specifier.
    ResolverWalk {
        /// Module specifier being walked.
        specifier: String,
    },
}

/// Workspace request payload.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct WorkspacePayload {
    /// Number of files touched while servicing the operation.
    pub files_touched: u32,
    /// Wall-clock duration of the operation (ms).
    pub ms: f64,
    /// Number of dependency-graph edges traversed.
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub dep_edges_traversed: u64,
}
