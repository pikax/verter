#![deny(missing_docs)]
//! [`StructuredAuditEvent`] — typed structured events emitted by
//! audited request paths. The session-side macro
//! `component_meta_trace_structured!` constructs these variants and
//! pushes them onto the per-request accumulator.
//!
//! The enum is authoritative; producers MUST add a `// Custom
//! justified: <reason>` comment at every `Custom` construction site
//! (the
//! `every_custom_variant_construction_site_has_justification_comment`
//! architecture guard enforces this).

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::origin_graph::{
    DispatchKeyKind, MaterializationScopeAudit, MaterializationSubject, MaterializeSkipReason,
    ProjectionModeAudit, VfsLayer,
};
use crate::payloads::cache_outcomes::CacheOutcomeKind;
use crate::payloads::tags::{AdmissionRefusalReason, AugmentationTargetKindTag};
use crate::record::{u64_as_decimal_string, Hash16};

/// Typed structured event emitted by an audited request path.
///
/// All variants are `Serialize + Deserialize` so they can be written
/// to the TLS accumulator's event log and, later, to the footprint
/// miner's output without a trip through a format string.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum StructuredAuditEvent {
    /// Emitted at the entry of `get_component_meta_with_resolution`.
    RequestStart {
        /// Canonical id being resolved.
        canonical_id: Arc<str>,
        /// Stamped request id (decimal-string transport).
        #[serde(with = "u64_as_decimal_string")]
        #[ts(type = "string")]
        request_id: u64,
    },
    /// Emitted when `get_component_meta_with_resolution` returns.
    RequestEnd {
        /// Request id this event closes.
        #[serde(with = "u64_as_decimal_string")]
        #[ts(type = "string")]
        request_id: u64,
        /// `true` when the resolution produced `Some(...)`.
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
        /// `true` when served by an in-memory cache.
        cache_hit: bool,
        /// Number of bytes returned.
        #[serde(with = "u64_as_decimal_string")]
        #[ts(type = "string")]
        bytes_read: u64,
    },
    /// This request attached to a winner's in-flight slot.
    SharedLoadReuse {
        /// Canonical id of the shared artifact.
        canonical_id: Arc<str>,
        /// Winning request's id.
        #[serde(with = "u64_as_decimal_string")]
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
        /// Wall-clock duration in nanoseconds.
        #[serde(with = "u64_as_decimal_string")]
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
        #[serde(with = "u64_as_decimal_string")]
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
        #[serde(with = "u64_as_decimal_string")]
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
        #[serde(with = "u64_as_decimal_string")]
        #[ts(type = "string")]
        duration_ns: u64,
    },
    /// Entering `materialize_component_meta_structure`.
    MaterializeStructureEnter {
        /// Stable display key for the input semantic node.
        base: Arc<str>,
        /// Axis the input was lowered at.
        scope_axis: MaterializationScopeAudit,
        /// Caller-side projection mode the materialiser ran with.
        mode: ProjectionModeAudit,
        /// Materialiser stack depth at the entry (post-increment).
        depth: u16,
    },
    /// Leaving `materialize_component_meta_structure`.
    MaterializeStructureExit {
        /// Stable display key for the input semantic node.
        base: Arc<str>,
        /// Axis the input was lowered at.
        scope_axis: MaterializationScopeAudit,
        /// Caller-side projection mode the materialiser ran with.
        mode: ProjectionModeAudit,
        /// Cache outcome recorded for the materialiser entry.
        outcome: CacheOutcomeKind,
        /// Wall-clock duration (ns).
        #[serde(with = "u64_as_decimal_string")]
        #[ts(type = "string")]
        duration_ns: u64,
    },
    /// Policy gate fired before dispatch.
    MaterializeStructurePolicySkip {
        /// Stable display key for the input semantic node.
        base: Arc<str>,
        /// Axis the input was at when the gate fired.
        scope_axis: MaterializationScopeAudit,
        /// Specific policy arm that bailed.
        reason: MaterializeSkipReason,
    },
    /// Same-key re-entry detected on the materialiser's thread-local
    /// in-flight stack.
    MaterializeStructureCycleDetected {
        /// Stable display key for the input semantic node.
        base: Arc<str>,
        /// Axis the input was at when the cycle was detected.
        scope_axis: MaterializationScopeAudit,
        /// Caller-side projection mode the materialiser ran with.
        mode: ProjectionModeAudit,
        /// Materialiser stack depth at detection.
        depth: u16,
    },
    /// Defensive depth fuse tripped.
    MaterializeStructureDepthFuseTripped {
        /// Stable display key for the input semantic node.
        base: Arc<str>,
        /// Axis the input was at when the fuse tripped.
        scope_axis: MaterializationScopeAudit,
        /// Caller-side projection mode the materialiser ran with.
        mode: ProjectionModeAudit,
        /// Materialiser stack depth at trip.
        depth: u16,
    },
    /// One cache cascade drain at the per-canonical upsert layer.
    ///
    /// Emitted by the full `host.upsert(...)` path at every cache
    /// drain site enumerated in
    /// `crates/verter_session/tests/fixtures/cache_baseline/evict_canonical_inventory.json`.
    /// The quintuple-unchanged fast path (R1) MUST NOT emit this event;
    /// the structural-change path emits one event per draining
    /// instruction. Cache-reuse tests use the absence of these
    /// events to prove byte-identical re-upsert is a true no-op;
    /// invalidation-characterisation tests use the presence/order
    /// to characterise path-precise invalidation.
    CacheDrainedAtUpsert {
        /// Static identifier for the cache layer being drained
        /// (e.g. `"resolved_type_cache"`, `"eval_env_cache"`,
        /// `"compile_slots"`, `"derived_raw_cache"`,
        /// `"semantic_invalidate"`, `"workspace_parsed_edges"`,
        /// `"resolver_runtime"`, `"store_view_epoch"`,
        /// `"project_type_store"`, `"dependency_cache"`).
        layer: Arc<str>,
        /// Canonical id of the file whose upsert triggered the drain.
        canonical_id: Arc<str>,
    },
    /// R20 typed event: a `ValidatedFactCache` candidate's
    /// `fact_dep_signature` exceeded the
    /// `FACT_SIGNATURE_CAP` size cap and was admitted as
    /// `NonCacheable`. Producers fall back to cold recompute;
    /// correctness is preserved but the warm-cache slot is
    /// skipped for this candidate.
    FactSignatureOverflow {
        /// Number of `FactVersionRef` entries the producer
        /// attempted to admit.
        candidate_size: u32,
        /// Configured cap value at admission time. Today this
        /// equals `verter_session::resolver_core::FACT_SIGNATURE_CAP`
        /// (1024); the field is recorded explicitly so the audit
        /// trail survives future cap tuning.
        cap: u32,
    },
    /// R20 typed event: a `ValidatedFactCache` candidate failed the
    /// admission guard because its `fact_dep_signature` was empty and
    /// the cache is NOT a documented source-independent kind. The
    /// candidate was admitted as `NonCacheable`; correctness is
    /// preserved by falling back to cold recompute every time. Stage
    /// 6d / Stage 7 canary asserts this event fires for the
    /// synthetic empty-signature test only — production producers
    /// must observe at least one fact.
    FactSignatureAdmissionRefused {
        /// Static identifier for the cache layer that refused the
        /// admission (mirrors the `layer` discriminator on
        /// `CacheDrainedAtUpsert`). Values like `"materialize_structure"`,
        /// `"route_db_routes"`, `"validated_fact_cache_generic"`.
        cache_kind: Arc<str>,
        /// Reason for the refusal.
        reason: AdmissionRefusalReason,
    },
    /// Module-augmentation stitching produced an effective export
    /// set for an augmentation target (R29 + G1).
    ///
    /// Cold-path only — emitted once per `EffectiveExportSet`
    /// cold/stale compute, after the augmenter set has been folded
    /// into the consumer's effective surface. The `target_kind_tag`
    /// together with the parallel optional fields below identify the
    /// target. `augmenter_count` and `fingerprint` describe the
    /// `AugmenterSet` that contributed.
    ModuleAugmentationStitched {
        /// Discriminator for the augmentation target kind.
        target_kind_tag: AugmentationTargetKindTag,
        /// External-specifier text when
        /// `target_kind_tag == ExternalSpecifier`.
        external_specifier: Option<Arc<str>>,
        /// Resolved canonical path when
        /// `target_kind_tag == ResolvedRelativeCanonical`.
        resolved_relative_canonical: Option<Arc<str>>,
        /// Wildcard glob pattern when
        /// `target_kind_tag == WildcardAmbient`.
        wildcard_pattern: Option<Arc<str>>,
        /// Number of augmenters that contributed to the surface.
        augmenter_count: u32,
        /// `AugmenterSet.fingerprint` at stitch time. The
        /// `ModuleAugmentationIndexShape` fact recorded on the
        /// consumer's `fact_dep_signature` carries this same value
        /// as its `expected_hash`, so a future augmenter-set change
        /// invalidates the consumer.
        fingerprint: Hash16,
    },
    /// `FileArtifactStore.augmentation_index` entry was installed
    /// or refreshed (R29 + G1).
    ///
    /// Cold-path only — emitted by the index-population path when
    /// a new `AugmentationTargetKey` is inserted (`prev_fingerprint
    /// == None`) or an existing entry's `AugmenterSet.fingerprint`
    /// transitions because a new augmenter has entered or left
    /// `FileArtifactStore` (`prev_fingerprint == Some(...)`).
    /// Downstream consumers that observed the prior fingerprint
    /// fail their `fact_dep_signature` validation on the next read
    /// and recompute.
    ModuleAugmentationIndexShape {
        /// Discriminator for the augmentation target kind.
        target_kind_tag: AugmentationTargetKindTag,
        /// External-specifier text when
        /// `target_kind_tag == ExternalSpecifier`.
        external_specifier: Option<Arc<str>>,
        /// Resolved canonical path when
        /// `target_kind_tag == ResolvedRelativeCanonical`.
        resolved_relative_canonical: Option<Arc<str>>,
        /// Wildcard glob pattern when
        /// `target_kind_tag == WildcardAmbient`.
        wildcard_pattern: Option<Arc<str>>,
        /// Previous fingerprint when this is a refresh; `None` on
        /// first install.
        prev_fingerprint: Option<Hash16>,
        /// New fingerprint after install/refresh.
        new_fingerprint: Hash16,
        /// Number of augmenters in the post-install set.
        augmenter_count: u32,
    },
    /// Escape hatch for ad-hoc events. Every construction site MUST
    /// carry a `// Custom justified: <reason>` comment.
    Custom {
        /// Short identifier for the event kind.
        name: Arc<str>,
        /// Free-form detail payload.
        detail: Arc<str>,
    },
}

impl std::fmt::Display for StructuredAuditEvent {
    /// Hand-authored `Display` — produces a compact single-line
    /// representation matching the snapshot test pinning in
    /// `verter_session::component_meta_audit::expected_display_snapshots`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RequestStart {
                canonical_id,
                request_id,
            } => write!(f, "RequestStart({canonical_id}, #{request_id})"),
            Self::RequestEnd {
                request_id,
                success,
            } => write!(f, "RequestEnd(#{request_id}, success={success})"),
            Self::IndexedReadyBuilt {
                canonical_id,
                whole_hash,
            } => write!(
                f,
                "IndexedReadyBuilt({canonical_id}, hash={})",
                short_hash(whole_hash)
            ),
            Self::VfsRead {
                canonical_id,
                layer,
                cache_hit,
                bytes_read,
            } => write!(
                f,
                "VfsRead({canonical_id}, {layer:?}, hit={cache_hit}, bytes={bytes_read})"
            ),
            Self::SharedLoadReuse {
                canonical_id,
                winner_request_id,
                winner_audited,
            } => write!(
                f,
                "SharedLoadReuse({canonical_id}, winner=#{winner_request_id}, audited={winner_audited})"
            ),
            Self::DispatchEnter { key_kind, depth } => {
                write!(f, "DispatchEnter({key_kind:?}, depth={depth})")
            }
            Self::DispatchExit {
                key_kind,
                outcome,
                duration_ns,
            } => write!(f, "DispatchExit({key_kind:?}, {outcome:?}, {duration_ns}ns)"),
            Self::MaterializeMemberRouteStart { subject } => {
                write!(f, "MaterializeMemberRouteStart({subject:?})")
            }
            Self::MaterializeMemberRouteEnd {
                subject,
                duration_ns,
            } => write!(f, "MaterializeMemberRouteEnd({subject:?}, {duration_ns}ns)"),
            Self::RematerializePublicPropTypeStart { subject } => {
                write!(f, "RematerializePublicPropTypeStart({subject:?})")
            }
            Self::RematerializePublicPropTypeEnd {
                subject,
                duration_ns,
            } => write!(
                f,
                "RematerializePublicPropTypeEnd({subject:?}, {duration_ns}ns)"
            ),
            Self::MaterializeDefinePropsMember { subject } => {
                write!(f, "MaterializeDefinePropsMember({subject:?})")
            }
            Self::FallthroughInheritanceComputed { subject } => {
                write!(f, "FallthroughInheritanceComputed({subject:?})")
            }
            Self::ResolveImportedTypeRoot {
                canonical_id,
                symbol_name,
            } => write!(f, "ResolveImportedTypeRoot({canonical_id}::{symbol_name})"),
            Self::CurrentEvalState {
                canonical_id,
                duration_ns,
            } => write!(f, "CurrentEvalState({canonical_id}, {duration_ns}ns)"),
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
            Self::CacheDrainedAtUpsert {
                layer,
                canonical_id,
            } => write!(f, "CacheDrainedAtUpsert({layer}, {canonical_id})"),
            Self::FactSignatureOverflow {
                candidate_size,
                cap,
            } => write!(
                f,
                "FactSignatureOverflow(size={candidate_size}, cap={cap})"
            ),
            Self::FactSignatureAdmissionRefused { cache_kind, reason } => write!(
                f,
                "FactSignatureAdmissionRefused({cache_kind}, {reason:?})"
            ),
            Self::ModuleAugmentationStitched {
                target_kind_tag,
                external_specifier,
                resolved_relative_canonical,
                wildcard_pattern,
                augmenter_count,
                fingerprint,
            } => {
                let target = format_augmentation_target(
                    *target_kind_tag,
                    external_specifier.as_deref(),
                    resolved_relative_canonical.as_deref(),
                    wildcard_pattern.as_deref(),
                );
                write!(
                    f,
                    "ModuleAugmentationStitched({target}, n={augmenter_count}, fp={})",
                    short_hash(fingerprint)
                )
            }
            Self::ModuleAugmentationIndexShape {
                target_kind_tag,
                external_specifier,
                resolved_relative_canonical,
                wildcard_pattern,
                prev_fingerprint,
                new_fingerprint,
                augmenter_count,
            } => {
                let target = format_augmentation_target(
                    *target_kind_tag,
                    external_specifier.as_deref(),
                    resolved_relative_canonical.as_deref(),
                    wildcard_pattern.as_deref(),
                );
                match prev_fingerprint {
                    Some(prev) => write!(
                        f,
                        "ModuleAugmentationIndexShape({target}, prev={}, new={}, n={augmenter_count})",
                        short_hash(prev),
                        short_hash(new_fingerprint),
                    ),
                    None => write!(
                        f,
                        "ModuleAugmentationIndexShape({target}, install={}, n={augmenter_count})",
                        short_hash(new_fingerprint),
                    ),
                }
            }
            Self::Custom { name, detail } => write!(f, "Custom({name}, {detail})"),
        }
    }
}

fn short_hash(hash: &Hash16) -> String {
    let mut s = String::with_capacity(8);
    for byte in hash.iter().take(4) {
        s.push_str(&format!("{byte:02x}"));
    }
    s
}

/// Format an augmentation target for `Display`. Picks the parallel
/// optional field that matches `target_kind_tag` and renders it
/// concisely.
fn format_augmentation_target(
    target_kind_tag: AugmentationTargetKindTag,
    external_specifier: Option<&str>,
    resolved_relative_canonical: Option<&str>,
    wildcard_pattern: Option<&str>,
) -> String {
    match target_kind_tag {
        AugmentationTargetKindTag::ExternalSpecifier => match external_specifier {
            Some(spec) => format!("ext={spec}"),
            None => "ext=?".to_owned(),
        },
        AugmentationTargetKindTag::ResolvedRelativeCanonical => match resolved_relative_canonical {
            Some(canon) => format!("rel={canon}"),
            None => "rel=?".to_owned(),
        },
        AugmentationTargetKindTag::WildcardAmbient => match wildcard_pattern {
            Some(pat) => format!("wild={pat}"),
            None => "wild=?".to_owned(),
        },
        AugmentationTargetKindTag::GlobalAugmentation => "global".to_owned(),
    }
}
