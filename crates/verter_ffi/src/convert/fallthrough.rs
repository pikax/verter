//! Root-reachability and fallthrough surface conversions: branches, target
//! refs, branch status, generic-resolution failures, and partial-resolution
//! reasons.

use crate::types::*;

use super::component_meta::require_lane_aligned;
use super::string_helpers::inherited_source_to_ffi;

pub(super) fn root_reachability_to_ffi(
    reachability: verter_semantic::analysis::component_meta::RootReachability,
) -> FfiRootReachability {
    match reachability {
        verter_semantic::analysis::component_meta::RootReachability::NoFallthrough { reason } => {
            FfiRootReachability::NoFallthrough {
                reason: no_fallthrough_reason_to_ffi(reason),
            }
        }
        verter_semantic::analysis::component_meta::RootReachability::Branches { branches } => {
            FfiRootReachability::Branches {
                branches: branches.into_iter().map(root_branch_to_ffi).collect(),
            }
        }
    }
}

pub(super) fn root_info_to_ffi(
    reachability: &verter_semantic::analysis::component_meta::RootReachability,
) -> FfiRootInfo {
    match reachability {
        verter_semantic::analysis::component_meta::RootReachability::NoFallthrough { reason } => {
            let kind = match reason {
                verter_semantic::analysis::component_meta::NoFallthroughReason::MultiRoot
                | verter_semantic::analysis::component_meta::NoFallthroughReason::RootVFor => {
                    FfiRootInfoKind::Multiple
                }
                verter_semantic::analysis::component_meta::NoFallthroughReason::BranchNotSingleRoot => {
                    FfiRootInfoKind::Conditional
                }
                verter_semantic::analysis::component_meta::NoFallthroughReason::InheritAttrsFalse
                | verter_semantic::analysis::component_meta::NoFallthroughReason::NoTemplate
                | verter_semantic::analysis::component_meta::NoFallthroughReason::EmptyTemplate
                | verter_semantic::analysis::component_meta::NoFallthroughReason::TextOrInterpolationRoot => {
                    FfiRootInfoKind::None
                }
            };
            FfiRootInfo {
                kind,
                reason: Some(no_fallthrough_reason_to_ffi(reason.clone())),
                targets: Vec::new(),
            }
        }
        verter_semantic::analysis::component_meta::RootReachability::Branches { branches } => {
            FfiRootInfo {
                kind: if branches.len() <= 1 {
                    FfiRootInfoKind::Single
                } else {
                    FfiRootInfoKind::Conditional
                },
                reason: None,
                targets: branches
                    .iter()
                    .map(|branch| root_target_ref_to_ffi(branch.target.clone()))
                    .collect(),
            }
        }
    }
}

pub(super) fn no_fallthrough_reason_to_ffi(
    reason: verter_semantic::analysis::component_meta::NoFallthroughReason,
) -> FfiNoFallthroughReason {
    match reason {
        verter_semantic::analysis::component_meta::NoFallthroughReason::InheritAttrsFalse => {
            FfiNoFallthroughReason::InheritAttrsFalse
        }
        verter_semantic::analysis::component_meta::NoFallthroughReason::MultiRoot => {
            FfiNoFallthroughReason::MultiRoot
        }
        verter_semantic::analysis::component_meta::NoFallthroughReason::BranchNotSingleRoot => {
            FfiNoFallthroughReason::BranchNotSingleRoot
        }
        verter_semantic::analysis::component_meta::NoFallthroughReason::RootVFor => {
            FfiNoFallthroughReason::RootVFor
        }
        verter_semantic::analysis::component_meta::NoFallthroughReason::NoTemplate => {
            FfiNoFallthroughReason::NoTemplate
        }
        verter_semantic::analysis::component_meta::NoFallthroughReason::EmptyTemplate => {
            FfiNoFallthroughReason::EmptyTemplate
        }
        verter_semantic::analysis::component_meta::NoFallthroughReason::TextOrInterpolationRoot => {
            FfiNoFallthroughReason::TextOrInterpolationRoot
        }
    }
}

pub(super) fn root_branch_to_ffi(
    branch: verter_semantic::analysis::component_meta::RootBranch,
) -> FfiRootBranch {
    FfiRootBranch {
        branch_index: branch.branch_index,
        condition_text: branch.condition_text,
        target: root_target_ref_to_ffi(branch.target),
        consumed: FfiConsumedRootBindings {
            attrs: branch.consumed.attrs,
            listeners: branch.consumed.listeners,
            has_dynamic_attr_name: branch.consumed.has_dynamic_attr_name,
            has_dynamic_listener_name: branch.consumed.has_dynamic_listener_name,
        },
        has_unknown_spread: branch.has_unknown_spread,
    }
}

pub(super) fn root_target_ref_to_ffi(
    target: verter_semantic::analysis::component_meta::RootTargetRef,
) -> FfiRootTargetRef {
    match target {
        verter_semantic::analysis::component_meta::RootTargetRef::NativeElement {
            element_index,
            tag,
        } => FfiRootTargetRef::NativeElement { element_index, tag },
        verter_semantic::analysis::component_meta::RootTargetRef::DynamicComponentUsage {
            element_index,
            usage_index,
        } => FfiRootTargetRef::DynamicComponentUsage {
            element_index,
            usage_index,
        },
        verter_semantic::analysis::component_meta::RootTargetRef::ComponentUsage {
            element_index,
            usage_index,
            name,
            import_source,
        } => FfiRootTargetRef::ComponentUsage {
            element_index,
            usage_index,
            name,
            import_source,
        },
        verter_semantic::analysis::component_meta::RootTargetRef::UnresolvedTarget {
            element_index,
            tag,
            reason,
        } => FfiRootTargetRef::UnresolvedTarget {
            element_index,
            tag,
            reason: unresolved_root_target_reason_to_ffi(reason),
        },
    }
}

pub(super) fn unresolved_root_target_reason_to_ffi(
    reason: verter_semantic::analysis::component_meta::UnresolvedRootTargetReason,
) -> FfiUnresolvedRootTargetReason {
    match reason {
        verter_semantic::analysis::component_meta::UnresolvedRootTargetReason::DynamicComponentIs => {
            FfiUnresolvedRootTargetReason::DynamicComponentIs
        }
        verter_semantic::analysis::component_meta::UnresolvedRootTargetReason::SlotOutlet => {
            FfiUnresolvedRootTargetReason::SlotOutlet
        }
        verter_semantic::analysis::component_meta::UnresolvedRootTargetReason::UnsupportedBuiltin { tag } => {
            FfiUnresolvedRootTargetReason::UnsupportedBuiltin { tag }
        }
        verter_semantic::analysis::component_meta::UnresolvedRootTargetReason::MissingUsageLink => {
            FfiUnresolvedRootTargetReason::MissingUsageLink
        }
        verter_semantic::analysis::component_meta::UnresolvedRootTargetReason::UnresolvedImport => {
            FfiUnresolvedRootTargetReason::UnresolvedImport
        }
        verter_semantic::analysis::component_meta::UnresolvedRootTargetReason::UnknownRootTarget => {
            FfiUnresolvedRootTargetReason::UnknownRootTarget
        }
    }
}

/// Positional-lane fallthrough conversion: `prop_lanes[i]` / `event_lanes[i]`
/// carry the materialized types for `branches[i]`'s rows, inner-aligned with
/// each branch's `props` / `events` vectors (the session envelope guarantees
/// the alignment; a `None` surface carries empty lanes).
///
/// HARD wire-boundary alignment guards (the [`require_lane_aligned`] class,
/// active in EVERY build profile) validate every dimension BEFORE any zip:
/// the outer branch count against both lanes, each branch's inner prop/event
/// counts 1:1, and the `None`-surface empty-lane invariant — a mismatch means
/// the envelope is torn, and the positional `zip`s below would SILENTLY
/// TRUNCATE the wire payload. A debug-only assert would let a release build
/// ship the truncated payload.
pub(super) fn fallthrough_surface_to_ffi(
    surface: verter_semantic::analysis::component_meta::FallthroughSurface,
    prop_lanes: Vec<Vec<verter_type_expr::TypeExpr>>,
    event_lanes: Vec<Vec<verter_type_expr::TypeExpr>>,
) -> FfiFallthroughSurface {
    match surface {
        verter_semantic::analysis::component_meta::FallthroughSurface::None { reason } => {
            assert!(
                prop_lanes.is_empty() && event_lanes.is_empty(),
                "component-meta FFI conversion refused: a `None` fallthrough surface must \
                 carry EMPTY `fallthrough-props`/`fallthrough-event-payloads` lanes; got \
                 {props} prop lane(s) and {events} event lane(s) — materialized values for \
                 branches that do not exist mean the envelope is torn",
                props = prop_lanes.len(),
                events = event_lanes.len(),
            );
            FfiFallthroughSurface::None {
                reason: no_fallthrough_reason_to_ffi(reason),
            }
        }
        verter_semantic::analysis::component_meta::FallthroughSurface::Branches { branches } => {
            require_lane_aligned("fallthrough-props", branches.len(), prop_lanes.len());
            require_lane_aligned(
                "fallthrough-event-payloads",
                branches.len(),
                event_lanes.len(),
            );
            for (index, (branch, (props, events))) in branches
                .iter()
                .zip(prop_lanes.iter().zip(event_lanes.iter()))
                .enumerate()
            {
                assert_eq!(
                    branch.props.len(),
                    props.len(),
                    "component-meta FFI conversion refused: fallthrough branch #{index} \
                     (`{key}`) carries {lane_len} materialized prop value(s) for \
                     {analysis_len} analysis prop row(s) — inner prop lanes are positional \
                     1:1 and a zip would silently truncate",
                    key = branch.branch_key,
                    lane_len = props.len(),
                    analysis_len = branch.props.len(),
                );
                assert_eq!(
                    branch.events.len(),
                    events.len(),
                    "component-meta FFI conversion refused: fallthrough branch #{index} \
                     (`{key}`) carries {lane_len} materialized event payload(s) for \
                     {analysis_len} analysis event row(s) — inner event lanes are \
                     positional 1:1 and a zip would silently truncate",
                    key = branch.branch_key,
                    lane_len = events.len(),
                    analysis_len = branch.events.len(),
                );
            }
            FfiFallthroughSurface::Branches {
                branches: branches
                    .into_iter()
                    .zip(prop_lanes.into_iter().zip(event_lanes))
                    .map(|(branch, (props, events))| {
                        fallthrough_branch_to_ffi(branch, props, events)
                    })
                    .collect(),
            }
        }
    }
}

pub(super) fn fallthrough_branch_to_ffi(
    branch: verter_semantic::analysis::component_meta::FallthroughBranch,
    prop_types: Vec<verter_type_expr::TypeExpr>,
    event_payloads: Vec<verter_type_expr::TypeExpr>,
) -> FfiFallthroughBranch {
    FfiFallthroughBranch {
        branch_key: branch.branch_key,
        condition_text: branch.condition_text,
        props: branch
            .props
            .into_iter()
            .zip(prop_types)
            .map(|(p, r#type)| FfiFallthroughPropEntry {
                name: p.name,
                r#type,
                raw_type: p.raw_type,
                sources: p.sources.into_iter().map(inherited_source_to_ffi).collect(),
            })
            .collect(),
        events: branch
            .events
            .into_iter()
            .zip(event_payloads)
            .map(|(e, payload)| FfiFallthroughEventEntry {
                name: e.name,
                payload,
                raw_signature: e.raw_signature,
                sources: e.sources.into_iter().map(inherited_source_to_ffi).collect(),
            })
            .collect(),
        root_chain: branch
            .root_chain
            .into_iter()
            .map(resolved_root_step_to_ffi)
            .collect(),
        status: branch_status_to_ffi(branch.status),
    }
}

pub(super) fn resolved_root_step_to_ffi(
    step: verter_semantic::analysis::component_meta::ResolvedRootStep,
) -> FfiResolvedRootStep {
    match step {
        verter_semantic::analysis::component_meta::ResolvedRootStep::NativeTag { tag } => {
            FfiResolvedRootStep::NativeTag { tag }
        }
        verter_semantic::analysis::component_meta::ResolvedRootStep::Component {
            canonical_id,
            component_name,
        } => FfiResolvedRootStep::Component {
            canonical_id,
            component_name,
        },
        verter_semantic::analysis::component_meta::ResolvedRootStep::Unresolved { tag, reason } => {
            FfiResolvedRootStep::Unresolved {
                tag,
                reason: unresolved_branch_reason_to_ffi(reason),
            }
        }
    }
}

pub(super) fn branch_status_to_ffi(
    status: verter_semantic::analysis::component_meta::BranchStatus,
) -> FfiBranchStatus {
    match status {
        verter_semantic::analysis::component_meta::BranchStatus::Resolved => {
            FfiBranchStatus::Resolved
        }
        verter_semantic::analysis::component_meta::BranchStatus::PartiallyUnresolved {
            reasons,
        } => FfiBranchStatus::PartiallyUnresolved {
            reasons: reasons
                .into_iter()
                .map(partial_branch_reason_to_ffi)
                .collect(),
        },
        verter_semantic::analysis::component_meta::BranchStatus::Unresolved { reason } => {
            FfiBranchStatus::Unresolved {
                reason: unresolved_branch_reason_to_ffi(reason),
            }
        }
    }
}

pub(super) fn generic_resolution_failure_to_ffi(
    failure: verter_semantic::analysis::component_meta::GenericResolutionFailure,
) -> FfiGenericResolutionFailure {
    match failure {
        verter_semantic::analysis::component_meta::GenericResolutionFailure::SpreadInput => {
            FfiGenericResolutionFailure::SpreadInput
        }
        verter_semantic::analysis::component_meta::GenericResolutionFailure::DynamicKey => {
            FfiGenericResolutionFailure::DynamicKey
        }
        verter_semantic::analysis::component_meta::GenericResolutionFailure::MissingType => {
            FfiGenericResolutionFailure::MissingType
        }
        verter_semantic::analysis::component_meta::GenericResolutionFailure::UnsupportedExpression => {
            FfiGenericResolutionFailure::UnsupportedExpression
        }
        verter_semantic::analysis::component_meta::GenericResolutionFailure::MissingUsageLink => {
            FfiGenericResolutionFailure::MissingUsageLink
        }
        verter_semantic::analysis::component_meta::GenericResolutionFailure::UnresolvedChildGenericSurface => {
            FfiGenericResolutionFailure::UnresolvedChildGenericSurface
        }
    }
}

pub(super) fn partial_branch_reason_to_ffi(
    reason: verter_semantic::analysis::component_meta::PartialBranchReason,
) -> FfiPartialBranchReason {
    match reason {
        verter_semantic::analysis::component_meta::PartialBranchReason::DynamicAttrName => {
            FfiPartialBranchReason::DynamicAttrName
        }
        verter_semantic::analysis::component_meta::PartialBranchReason::DynamicListenerName => {
            FfiPartialBranchReason::DynamicListenerName
        }
        verter_semantic::analysis::component_meta::PartialBranchReason::UnknownSpread => {
            FfiPartialBranchReason::UnknownSpread
        }
        verter_semantic::analysis::component_meta::PartialBranchReason::GenericResolution {
            failure,
        } => FfiPartialBranchReason::GenericResolution {
            failure: generic_resolution_failure_to_ffi(failure),
        },
    }
}

pub(super) fn unresolved_branch_reason_to_ffi(
    reason: verter_semantic::analysis::component_meta::UnresolvedBranchReason,
) -> FfiUnresolvedBranchReason {
    match reason {
        verter_semantic::analysis::component_meta::UnresolvedBranchReason::Cycle { canonical_id } => {
            FfiUnresolvedBranchReason::Cycle { canonical_id }
        }
        verter_semantic::analysis::component_meta::UnresolvedBranchReason::DynamicComponentIs => {
            FfiUnresolvedBranchReason::DynamicComponentIs
        }
        verter_semantic::analysis::component_meta::UnresolvedBranchReason::ChildResolutionFailed => {
            FfiUnresolvedBranchReason::ChildResolutionFailed
        }
        verter_semantic::analysis::component_meta::UnresolvedBranchReason::UnresolvedChildImport {
            import_source,
        } => FfiUnresolvedBranchReason::UnresolvedChildImport { import_source },
        verter_semantic::analysis::component_meta::UnresolvedBranchReason::RootTarget { reason } => {
            FfiUnresolvedBranchReason::RootTarget {
                reason: unresolved_root_target_reason_to_ffi(reason),
            }
        }
        verter_semantic::analysis::component_meta::UnresolvedBranchReason::GenericResolution { failure } => {
            FfiUnresolvedBranchReason::GenericResolution {
                failure: generic_resolution_failure_to_ffi(failure),
            }
        }
    }
}
