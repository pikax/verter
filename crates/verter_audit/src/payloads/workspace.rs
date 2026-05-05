#![deny(missing_docs)]
//! [`WorkspacePayload`] — strongly-typed payload for
//! `RequestKind::Workspace`. Producer crates populate the data structure once they emit through the audit substrate.

use serde::{Deserialize, Serialize};

use crate::record::u64_as_decimal_string;

/// Workspace operation discriminator. Mirrors the surface
/// `verter_workspace::WorkspaceAccess::audit_op` provides; the same
/// value is stored on the `RequestKind::Workspace { op }` discriminant
/// and on the [`WorkspacePayload::op`] field carried by the payload
/// (parallel to how `LspRequestPayload::method` mirrors the
/// `RequestKind::Lsp { method }` tag).
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS, PartialEq, Eq, Hash)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum WorkspaceOp {
    /// `resolve(specifier, from)` — module specifier resolution.
    AuditResolve {
        /// Module specifier being resolved.
        specifier: String,
        /// Importer canonical id; empty string when no importer is
        /// in scope (e.g. project-scoped lookup without a source
        /// file).
        from: String,
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
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct WorkspacePayload {
    /// Operation discriminator. Mirrors `RequestKind::Workspace { op }`
    /// on the envelope; carried inside the payload so consumers that
    /// only read the typed payload still see the operation type.
    pub op: WorkspaceOp,
    /// Number of files touched while servicing the operation.
    pub files_touched: u32,
    /// Wall-clock duration of the operation (ms).
    pub ms: f64,
    /// Number of dependency-graph edges traversed.
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub dep_edges_traversed: u64,
}

impl Default for WorkspacePayload {
    fn default() -> Self {
        Self {
            op: WorkspaceOp::AuditResolve {
                specifier: String::new(),
                from: String::new(),
            },
            files_touched: 0,
            ms: 0.0,
            dep_edges_traversed: 0,
        }
    }
}
