#![deny(missing_docs)]
//! `StructuredComponentMetaEvent` — typed replacement for the legacy
//! free-form `component_meta_trace_event!` / `component_meta_trace_scope!`
//! macro format strings.
//!
//! Plan §2.3. The enum is authoritative; the `Display` implementation
//! is hand-authored fresh (legacy stderr format was deleted in
//! Commit 5). Every call-site in the allow-list (`host_manage`,
//! `host_resolve`, `meta_resolve`, `component_meta_host`,
//! `component_meta_audit`) emits one variant via the
//! `component_meta_trace_structured!` macro (defined in Commit 5).
//!
//! `Custom::name: Arc<str>` is intentionally `Arc<str>` rather than
//! `Cow<'static, str>` — serde and ts-rs integration is markedly
//! simpler and the allocation cost is negligible against a path that
//! also does component-meta solving. Any new `Custom` construction
//! site must carry a `// Custom justified: <reason>` comment; the
//! `every_custom_variant_construction_site_has_justification_comment`
//! test enforces this at CI time.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{
    CacheOutcomeKind, DispatchKeyKind, MaterializationScopeAudit, MaterializationSubject,
    MaterializeSkipReason, ProjectionModeAudit, VfsLayer,
};
use crate::types::Hash16;

/// Typed structured event emitted by the component-meta call chain.
///
/// All variants are `Serialize + Deserialize` so they can be written
/// to the TLS accumulator's event log and, later, to the footprint
/// miner's output without a trip through a format string.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum StructuredComponentMetaEvent {
    /// Emitted at the entry of `get_component_meta_with_resolution`.
    RequestStart {
        /// Canonical id being resolved.
        canonical_id: Arc<str>,
        /// Stamped request id (decimal-string transport).
        #[serde(with = "crate::u64_as_decimal_string")]
        #[ts(type = "string")]
        request_id: u64,
    },
    /// Emitted when `get_component_meta_with_resolution` returns.
    RequestEnd {
        /// Request id this event closes.
        #[serde(with = "crate::u64_as_decimal_string")]
        #[ts(type = "string")]
        request_id: u64,
        /// `true` when the resolution produced `Some(...)`; `false`
        /// on a `None`/error return.
        success: bool,
    },
    /// A fresh `IndexedReady` entry was installed.
    IndexedReadyBuilt {
        /// Canonical id of the newly-built entry.
        canonical_id: Arc<str>,
        /// Content hash of the source snapshot.
        whole_hash: Hash16,
    },
    /// One VFS read was observed by the session-side sink.
    VfsRead {
        /// Canonical id that was read.
        canonical_id: Arc<str>,
        /// VFS layer that served the read.
        layer: VfsLayer,
        /// `true` when served by an in-memory cache (overlay / snapshot).
        cache_hit: bool,
        /// Number of bytes returned.
        #[serde(with = "crate::u64_as_decimal_string")]
        #[ts(type = "string")]
        bytes_read: u64,
    },
    /// This request attached to a winner's in-flight slot.
    SharedLoadReuse {
        /// Canonical id of the shared artifact.
        canonical_id: Arc<str>,
        /// Winning request's id.
        #[serde(with = "crate::u64_as_decimal_string")]
        #[ts(type = "string")]
        winner_request_id: u64,
        /// `true` when the winner was itself audited.
        winner_audited: bool,
    },
    /// Entering a semantic-query dispatch envelope.
    DispatchEnter {
        /// Kind of dispatch key being resolved.
        key_kind: DispatchKeyKind,
        /// Nesting depth within the envelope stack.
        depth: u16,
    },
    /// Leaving a semantic-query dispatch envelope.
    DispatchExit {
        /// Kind of dispatch key that was resolved.
        key_kind: DispatchKeyKind,
        /// Cache outcome recorded for the dispatch.
        outcome: CacheOutcomeKind,
        /// Wall-clock duration in nanoseconds (decimal-string transport).
        #[serde(with = "crate::u64_as_decimal_string")]
        #[ts(type = "string")]
        duration_ns: u64,
    },
    /// Start envelope for member-route materialization.
    MaterializeMemberRouteStart {
        /// What is being materialized.
        subject: MaterializationSubject,
    },
    /// End envelope with captured duration.
    MaterializeMemberRouteEnd {
        /// Subject this event closes.
        subject: MaterializationSubject,
        /// Wall-clock duration (ns).
        #[serde(with = "crate::u64_as_decimal_string")]
        #[ts(type = "string")]
        duration_ns: u64,
    },
    /// Start envelope for public-prop-type rematerialization.
    RematerializePublicPropTypeStart {
        /// Subject (owner + prop).
        subject: MaterializationSubject,
    },
    /// End envelope with captured duration.
    RematerializePublicPropTypeEnd {
        /// Subject this event closes.
        subject: MaterializationSubject,
        /// Wall-clock duration (ns).
        #[serde(with = "crate::u64_as_decimal_string")]
        #[ts(type = "string")]
        duration_ns: u64,
    },
    /// `defineProps<…>()` member materialization event.
    MaterializeDefinePropsMember {
        /// Subject (owner + member).
        subject: MaterializationSubject,
    },
    /// Fallthrough-inheritance was computed for an owner file.
    FallthroughInheritanceComputed {
        /// Subject (owner).
        subject: MaterializationSubject,
    },
    /// Imported type-root resolution hop.
    ResolveImportedTypeRoot {
        /// Canonical id of the declaring file.
        canonical_id: Arc<str>,
        /// Symbol name that was being resolved.
        symbol_name: Arc<str>,
    },
    /// Eval-state checkpoint with captured duration.
    CurrentEvalState {
        /// Canonical id whose eval state was captured.
        canonical_id: Arc<str>,
        /// Wall-clock duration (ns).
        #[serde(with = "crate::u64_as_decimal_string")]
        #[ts(type = "string")]
        duration_ns: u64,
    },
    /// Entering `materialize_component_meta_structure`. Plan §3.3.
    MaterializeStructureEnter {
        /// Stable display key for the input `SemanticNodeId`.
        base: Arc<str>,
        /// Axis the input was lowered at.
        scope_axis: MaterializationScopeAudit,
        /// Caller-side projection mode the materialiser ran with.
        mode: ProjectionModeAudit,
        /// Materialiser stack depth at the entry (post-increment).
        depth: u16,
    },
    /// Leaving `materialize_component_meta_structure`. Plan §3.3.
    MaterializeStructureExit {
        /// Stable display key for the input `SemanticNodeId`.
        base: Arc<str>,
        /// Axis the input was lowered at.
        scope_axis: MaterializationScopeAudit,
        /// Caller-side projection mode the materialiser ran with.
        mode: ProjectionModeAudit,
        /// Cache outcome recorded for the materialiser entry.
        /// `Tainted` discriminates depth-fuse and scope-unloaded
        /// outcomes from regular Hit/Miss.
        outcome: CacheOutcomeKind,
        /// Wall-clock duration (ns).
        #[serde(with = "crate::u64_as_decimal_string")]
        #[ts(type = "string")]
        duration_ns: u64,
    },
    /// Policy gate fired before dispatch — input was rejected by
    /// shape policy. Plan §3.3.
    MaterializeStructurePolicySkip {
        /// Stable display key for the input `SemanticNodeId`.
        base: Arc<str>,
        /// Axis the input was at when the gate fired.
        scope_axis: MaterializationScopeAudit,
        /// Specific policy arm that bailed.
        reason: MaterializeSkipReason,
    },
    /// Same-key re-entry detected on the materialiser's thread-local
    /// in-flight stack. Plan §3.3.
    MaterializeStructureCycleDetected {
        /// Stable display key for the input `SemanticNodeId`.
        base: Arc<str>,
        /// Axis the input was at when the cycle was detected.
        scope_axis: MaterializationScopeAudit,
        /// Caller-side projection mode the materialiser ran with.
        mode: ProjectionModeAudit,
        /// Materialiser stack depth at detection.
        depth: u16,
    },
    /// Defensive depth fuse tripped (input depth exceeded the
    /// materialiser's hard cap). Plan §3.3.
    MaterializeStructureDepthFuseTripped {
        /// Stable display key for the input `SemanticNodeId`.
        base: Arc<str>,
        /// Axis the input was at when the fuse tripped.
        scope_axis: MaterializationScopeAudit,
        /// Caller-side projection mode the materialiser ran with.
        mode: ProjectionModeAudit,
        /// Materialiser stack depth at trip.
        depth: u16,
    },
    /// Escape hatch for ad-hoc events. Every construction site MUST
    /// carry a `// Custom justified: <reason>` comment — the grep
    /// test in Commit 5 enforces this.
    Custom {
        /// Short identifier for the event kind.
        name: Arc<str>,
        /// Free-form detail payload — kept `Arc<str>` rather than a
        /// typed struct because `Custom` exists precisely for the
        /// ad-hoc cases that have not yet been lifted into a named
        /// variant.
        detail: Arc<str>,
    },
}

impl std::fmt::Display for StructuredComponentMetaEvent {
    /// Hand-authored `Display` — NOT a recreation of the legacy
    /// `format!("k=v")` detail strings. Produces a compact
    /// single-line representation suitable for the structured-event
    /// snapshot tests (plan §3 Commit 5).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RequestStart {
                canonical_id,
                request_id,
            } => {
                write!(f, "RequestStart({canonical_id}, #{request_id})")
            }
            Self::RequestEnd {
                request_id,
                success,
            } => {
                write!(f, "RequestEnd(#{request_id}, success={success})")
            }
            Self::IndexedReadyBuilt {
                canonical_id,
                whole_hash,
            } => {
                write!(
                    f,
                    "IndexedReadyBuilt({canonical_id}, hash={})",
                    short_hash(whole_hash)
                )
            }
            Self::VfsRead {
                canonical_id,
                layer,
                cache_hit,
                bytes_read,
            } => {
                write!(
                    f,
                    "VfsRead({canonical_id}, {layer:?}, hit={cache_hit}, bytes={bytes_read})"
                )
            }
            Self::SharedLoadReuse {
                canonical_id,
                winner_request_id,
                winner_audited,
            } => {
                write!(
                    f,
                    "SharedLoadReuse({canonical_id}, winner=#{winner_request_id}, audited={winner_audited})"
                )
            }
            Self::DispatchEnter { key_kind, depth } => {
                write!(f, "DispatchEnter({key_kind:?}, depth={depth})")
            }
            Self::DispatchExit {
                key_kind,
                outcome,
                duration_ns,
            } => {
                write!(
                    f,
                    "DispatchExit({key_kind:?}, {outcome:?}, {duration_ns}ns)"
                )
            }
            Self::MaterializeMemberRouteStart { subject } => {
                write!(f, "MaterializeMemberRouteStart({subject:?})")
            }
            Self::MaterializeMemberRouteEnd {
                subject,
                duration_ns,
            } => {
                write!(f, "MaterializeMemberRouteEnd({subject:?}, {duration_ns}ns)")
            }
            Self::RematerializePublicPropTypeStart { subject } => {
                write!(f, "RematerializePublicPropTypeStart({subject:?})")
            }
            Self::RematerializePublicPropTypeEnd {
                subject,
                duration_ns,
            } => {
                write!(
                    f,
                    "RematerializePublicPropTypeEnd({subject:?}, {duration_ns}ns)"
                )
            }
            Self::MaterializeDefinePropsMember { subject } => {
                write!(f, "MaterializeDefinePropsMember({subject:?})")
            }
            Self::FallthroughInheritanceComputed { subject } => {
                write!(f, "FallthroughInheritanceComputed({subject:?})")
            }
            Self::ResolveImportedTypeRoot {
                canonical_id,
                symbol_name,
            } => {
                write!(f, "ResolveImportedTypeRoot({canonical_id}::{symbol_name})")
            }
            Self::CurrentEvalState {
                canonical_id,
                duration_ns,
            } => {
                write!(f, "CurrentEvalState({canonical_id}, {duration_ns}ns)")
            }
            Self::MaterializeStructureEnter {
                base,
                scope_axis,
                mode,
                depth,
            } => write!(
                f,
                "MaterializeStructureEnter({base}, {scope_axis:?}, {mode:?}, depth={depth})"
            ),
            Self::MaterializeStructureExit {
                base,
                scope_axis,
                mode,
                outcome,
                duration_ns,
            } => write!(
                f,
                "MaterializeStructureExit({base}, {scope_axis:?}, {mode:?}, {outcome:?}, {duration_ns}ns)"
            ),
            Self::MaterializeStructurePolicySkip {
                base,
                scope_axis,
                reason,
            } => write!(
                f,
                "MaterializeStructurePolicySkip({base}, {scope_axis:?}, {reason:?})"
            ),
            Self::MaterializeStructureCycleDetected {
                base,
                scope_axis,
                mode,
                depth,
            } => write!(
                f,
                "MaterializeStructureCycleDetected({base}, {scope_axis:?}, {mode:?}, depth={depth})"
            ),
            Self::MaterializeStructureDepthFuseTripped {
                base,
                scope_axis,
                mode,
                depth,
            } => write!(
                f,
                "MaterializeStructureDepthFuseTripped({base}, {scope_axis:?}, {mode:?}, depth={depth})"
            ),
            Self::Custom { name, detail } => write!(f, "Custom({name}, {detail})"),
        }
    }
}

fn short_hash(h: &Hash16) -> String {
    let mut s = String::with_capacity(8);
    for b in &h[..4] {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_event_custom_with_arc_str_deserializes_from_serde_json() {
        // Custom justified: round-trip test probe — exercises the
        // Custom variant's serde impl.
        let event = StructuredComponentMetaEvent::Custom {
            name: Arc::from("test_event"),
            detail: Arc::from("x=42"),
        };
        let json = serde_json::to_string(&event).expect("serialize");
        let back: StructuredComponentMetaEvent = serde_json::from_str(&json).expect("deserialize");
        match back {
            StructuredComponentMetaEvent::Custom { name, detail } => {
                assert_eq!(name.as_ref(), "test_event");
                assert_eq!(detail.as_ref(), "x=42");
            }
            other => panic!("expected Custom, got {other:?}"),
        }
    }
}
