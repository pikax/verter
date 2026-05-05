#![deny(missing_docs)]
//! Per-file attribution for an audited request.
//!
//! [`FileAudit`] is the deduplicated per-file record attached to a
//! [`crate::record::RequestAuditRecord`]. Each entry carries:
//!
//! - **role** — why the file was visible to this request (entry,
//!   direct/transitive import, type-only dep, IndexedReady build,
//!   resolver walk, or referenced-but-not-loaded).
//! - **layer** — which VFS layer served the read (overlay / snapshot /
//!   disk / negative / missing).
//! - **bytes_read** — how many bytes the read returned. The
//!   [`crate::memory::RequestMemoryAudit::bytes_parsed`] aggregate is a
//!   sum of these for non-`NotLoaded` entries.
//! - **read-once-aware timing** — `read_ms`, `parse_ms`, `lower_ms` are
//!   `Some(value)` ONLY when the audited request triggered the
//!   corresponding I/O / parse. Files served from the existing
//!   `IndexedReady` cache report all three as `None` and
//!   `cache_hit = true`.
//! - **`triggered_by_this_request`** — explicit semantic flag mirroring
//!   the read-once invariant. `false` for warm-cache entries; `true`
//!   when the request paid for the read / parse / lower.
//!
//! Producers populate the `Vec<FileAudit>` at request finalisation
//! using the per-request bookkeeping captured from
//! [`crate::footprint::VfsReadRecord`] events,
//! [`crate::footprint::IndexedReadyBuildRecord`] events, and any
//! resolver-walk / not-loaded attributions the higher layer chooses to
//! emit.

use serde::{Deserialize, Serialize};

use crate::origin_graph::VfsLayer;
use crate::record::u64_as_decimal_string;

/// Why a file was visible to the audited request. Producers attach
/// the role at the point the file enters the request's bookkeeping;
/// the role is determined by which path the file came in through.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ts_rs::TS, PartialEq, Eq, Hash)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum FileRole {
    /// The explicit canonical id passed to the public audited entry
    /// point — the request's primary subject.
    Entry,
    /// First-level import from the `Entry` file. Reachable via one
    /// import hop.
    DirectImport,
    /// Reachable via two or more import hops from the `Entry`. Carries
    /// the broader transitive closure of the request.
    TransitiveImport,
    /// Imported with the `type` modifier (e.g. `import type { T } from
    /// "./types"`). Tracked separately from value imports so consumers
    /// can attribute type-only dependency cost.
    TypeDep,
    /// A fresh `IndexedReady` build was triggered for this file. Read
    /// in tandem with [`FileRole::Entry`] / [`FileRole::DirectImport`]
    /// / [`FileRole::TransitiveImport`] — `IndexedReadyBuild`
    /// indicates the parse path was paid for by THIS request.
    IndexedReadyBuild,
    /// Referenced by the resolver / dependency graph but the request
    /// did not load the file's source. Bytes read is zero; timings
    /// stay `None`.
    NotLoaded,
    /// Walked by the resolver during type resolution but not parsed.
    /// May overlap with `DirectImport` / `TransitiveImport` when the
    /// resolver visits a file without producing a parse for it.
    ResolverWalk,
}

/// Per-file attribution attached to a
/// [`crate::record::RequestAuditRecord::files`] entry.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS, PartialEq)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct FileAudit {
    /// Canonical id of the file the entry attributes.
    pub canonical_id: String,
    /// Why this file was visible to the request.
    pub role: FileRole,
    /// Which VFS layer served the read.
    pub layer: VfsLayer,
    /// Number of bytes returned by the read. Zero for `NotLoaded`,
    /// `Missing`, and `DirIndexNegative` reads.
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub bytes_read: u64,
    /// `true` when the read resolved from an in-memory cache (overlay
    /// / snapshot) OR from the existing `IndexedReady` cache. Cache
    /// hits report `triggered_by_this_request = false` and all
    /// `*_ms = None` per the read-once invariant.
    pub cache_hit: bool,
    /// `true` when this audited request triggered the I/O / parse /
    /// lower attributed to this file. `false` for warm-cache entries
    /// served by a prior request.
    pub triggered_by_this_request: bool,
    /// Wall-clock milliseconds the request spent reading the file.
    /// `Some(value)` only when this request triggered the read; `None`
    /// for warm-cache entries (read-once invariant).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_ms: Option<f64>,
    /// Wall-clock milliseconds the request spent parsing the file.
    /// `Some(value)` only when this request triggered the parse;
    /// `None` for warm-cache entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parse_ms: Option<f64>,
    /// Wall-clock milliseconds the request spent lowering the parsed
    /// AST into `IndexedReady`. `Some(value)` only when this request
    /// triggered the lower; `None` otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lower_ms: Option<f64>,
}

impl FileAudit {
    /// Construct a `FileAudit` representing a cache-hit (no work done
    /// by this request). All timings are `None`,
    /// `triggered_by_this_request = false`, `cache_hit = true`.
    #[must_use]
    pub fn cached(
        canonical_id: impl Into<String>,
        role: FileRole,
        layer: VfsLayer,
        bytes_read: u64,
    ) -> Self {
        Self {
            canonical_id: canonical_id.into(),
            role,
            layer,
            bytes_read,
            cache_hit: true,
            triggered_by_this_request: false,
            read_ms: None,
            parse_ms: None,
            lower_ms: None,
        }
    }

    /// Construct a `FileAudit` representing work this request paid
    /// for. Timings carry `Some(value)` when the timing flag is on;
    /// `None` when the flag is off.
    #[must_use]
    pub fn triggered(
        canonical_id: impl Into<String>,
        role: FileRole,
        layer: VfsLayer,
        bytes_read: u64,
        read_ms: Option<f64>,
        parse_ms: Option<f64>,
        lower_ms: Option<f64>,
    ) -> Self {
        Self {
            canonical_id: canonical_id.into(),
            role,
            layer,
            bytes_read,
            cache_hit: false,
            triggered_by_this_request: true,
            read_ms,
            parse_ms,
            lower_ms,
        }
    }
}
