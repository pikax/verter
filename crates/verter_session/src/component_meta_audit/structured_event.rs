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

use super::{DispatchKeyKind, MaterializationSubject, VfsLayer};
use crate::types::Hash16;

/// Typed structured event emitted by the component-meta call chain.
///
/// All variants are `Serialize + Deserialize` so they can be written
/// to the TLS accumulator's event log and, later, to the footprint
/// miner's output without a trip through a format string.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum StructuredComponentMetaEvent {
    RequestStart {
        canonical_id: Arc<str>,
        #[serde(with = "crate::u64_as_decimal_string")]
        #[ts(type = "string")]
        request_id: u64,
    },
    RequestEnd {
        #[serde(with = "crate::u64_as_decimal_string")]
        #[ts(type = "string")]
        request_id: u64,
        success: bool,
    },
    IndexedReadyBuilt {
        canonical_id: Arc<str>,
        whole_hash: Hash16,
    },
    VfsRead {
        canonical_id: Arc<str>,
        layer: VfsLayer,
        cache_hit: bool,
        #[serde(with = "crate::u64_as_decimal_string")]
        #[ts(type = "string")]
        bytes_read: u64,
    },
    SharedLoadReuse {
        canonical_id: Arc<str>,
        #[serde(with = "crate::u64_as_decimal_string")]
        #[ts(type = "string")]
        winner_request_id: u64,
        winner_audited: bool,
    },
    DispatchEnter {
        key_kind: DispatchKeyKind,
        depth: u16,
    },
    DispatchExit {
        key_kind: DispatchKeyKind,
        outcome: super::CacheOutcomeKind,
        #[serde(with = "crate::u64_as_decimal_string")]
        #[ts(type = "string")]
        duration_ns: u64,
    },
    MaterializeMemberRouteStart {
        subject: MaterializationSubject,
    },
    MaterializeMemberRouteEnd {
        subject: MaterializationSubject,
        #[serde(with = "crate::u64_as_decimal_string")]
        #[ts(type = "string")]
        duration_ns: u64,
    },
    RematerializePublicPropTypeStart {
        subject: MaterializationSubject,
    },
    RematerializePublicPropTypeEnd {
        subject: MaterializationSubject,
        #[serde(with = "crate::u64_as_decimal_string")]
        #[ts(type = "string")]
        duration_ns: u64,
    },
    MaterializeDefinePropsMember {
        subject: MaterializationSubject,
    },
    FallthroughInheritanceComputed {
        subject: MaterializationSubject,
    },
    ResolveImportedTypeRoot {
        canonical_id: Arc<str>,
        symbol_name: Arc<str>,
    },
    CurrentEvalState {
        canonical_id: Arc<str>,
        #[serde(with = "crate::u64_as_decimal_string")]
        #[ts(type = "string")]
        duration_ns: u64,
    },
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
