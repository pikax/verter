//! `verter_audit::published_surface` — unified
//! `PublishedSurfacePolicy` registry that defines the three views
//! of a Vue SFC's user-visible macro published surface.
//!
//! ## Why this lives in `verter_audit`
//!
//! `MemberEdgeProvenance::PublishedField` audit edges record the
//! producer's semantic truth — every name the macro projector
//! admitted onto the user-visible surface. Downstream consumers
//! (`@verter/component-meta/compat`, the benchmark refiner, the
//! Rule-5 validator) historically each maintained their own
//! independent name-blocklists / heuristic filters, which drifted
//! against the producer over time. The registry collapses those
//! consumer-side projections into one authoritative source.
//!
//! - [`PublishedSurfacePolicy::Native`] is the producer's raw
//!   truth. The `PublishedField` audit rail aligns to this view by
//!   construction (no producer-side filter).
//! - [`PublishedSurfacePolicy::Compat`] is the
//!   `@verter/component-meta/compat` projection: native minus the
//!   `COMPAT_BLOCKED_SLOT_NAMES` set on the slots surface only.
//! - [`PublishedSurfacePolicy::Refined`] is the benchmark refiner
//!   projection: Compat, minus props that structurally shadow a
//!   declared emit (`onSubmit` when `submit` is declared), minus
//!   Vue intrinsics not explicitly re-declared in the macro type
//!   argument, minus producer-flagged "global" (HTMLAttributes-
//!   derived) props.
//!
//! All policy decisions are STRUCTURAL — driven by per-name facts
//! on [`AnalyzedSurfaceItem`] (`declared_in_macro_type_arg`,
//! `global`) and the structural fingerprint of the declared emits.
//! No name-prefix heuristics, no thresholds, no ratios.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use ts_rs::TS;

/// One of three projection views of the published macro surface.
///
/// `PublishedField` audit edges are emitted in `Native` truth; the
/// `Compat` and `Refined` projections are downstream views
/// consumed by `@verter/component-meta/compat` and the benchmark
/// refiner respectively.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum PublishedSurfacePolicy {
    /// Native producer truth: every name the macro projector
    /// admitted onto the user-visible surface, with no filtering.
    /// The `PublishedField` rail in the audit graph aligns to this
    /// projection by construction.
    Native,
    /// `@verter/component-meta/compat` consumer-facing projection:
    /// native, minus the [`COMPAT_BLOCKED_SLOT_NAMES`] set on the
    /// `slots` surface only. The blocklist mirrors
    /// `vue-component-meta`'s slot-name suppression for VNode-only
    /// transport keys.
    Compat,
    /// Benchmark refiner projection: `Compat`, minus props that
    /// structurally shadow a declared emit (`on{Event}` form,
    /// camelCase), minus Vue intrinsics where the SFC author did
    /// NOT explicitly re-declare them in the macro type argument,
    /// minus producer-flagged global (HTMLAttributes-derived)
    /// props.
    Refined,
}

/// Per-name structural facts attached to a member of the
/// published macro surface. Consumed by [`names_for_policy`] to
/// produce structural projections (no name-string heuristics).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct AnalyzedSurfaceItem {
    /// Member name as published by the producer.
    pub name: String,
    /// True if the SFC author explicitly declared this member name
    /// in the macro's type argument (inline `defineProps<{ class?:
    /// any }>()` or its referenced generic helper's own body
    /// member). Distinguishes "this name is on the surface because
    /// the author wanted it" from "this name reaches the surface
    /// through heritage / HTMLAttributes intersection /
    /// utility-type expansion". `Refined` filters Vue intrinsics
    /// and `onX`-shadows-emit only when this is `false`.
    pub declared_in_macro_type_arg: bool,
    /// True if the producer flagged this prop as "global" — i.e.
    /// reaching the surface from a globally-declared
    /// HTMLAttributes-flavoured ancestor that the
    /// `@verter/component-meta/compat` and `vue-component-meta`
    /// projections never publish. Consulted only by `Refined`.
    /// Always `false` for non-prop surfaces (events / slots /
    /// exposed).
    pub global: bool,
}

/// Structural view of a SFC's published macro surface, fed to
/// [`names_for_policy`] for projection.
///
/// One [`AnalyzedSurfaceItem`] per published member per surface.
/// The order of the items inside each vector is preserved by
/// projection — callers wanting deterministic ordering should sort
/// upstream.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct AnalyzedSurface {
    /// Props published by `defineProps` (+ `withDefaults` /
    /// `defineModel` contributions).
    pub props: Vec<AnalyzedSurfaceItem>,
    /// Events published by `defineEmits`.
    pub events: Vec<AnalyzedSurfaceItem>,
    /// Slots published by `defineSlots` (and template-side
    /// `<slot>` usage).
    pub slots: Vec<AnalyzedSurfaceItem>,
    /// Members published by `defineExpose`.
    pub exposed: Vec<AnalyzedSurfaceItem>,
}

/// Result of applying a [`PublishedSurfacePolicy`] to an
/// [`AnalyzedSurface`]. One name list per surface.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct PolicyNamesResult {
    /// Prop names retained by the policy.
    pub props: Vec<String>,
    /// Event names retained by the policy.
    pub events: Vec<String>,
    /// Slot names retained by the policy.
    pub slots: Vec<String>,
    /// Exposed member names retained by the policy.
    pub exposed: Vec<String>,
}

/// VNode-only transport keys that `vue-component-meta` suppresses
/// on the slots surface — `@verter/component-meta/compat` mirrors
/// this contract.
///
/// Used by `PublishedSurfacePolicy::Compat` and
/// `PublishedSurfacePolicy::Refined`.
pub const COMPAT_BLOCKED_SLOT_NAMES: &[&str] = &[
    "type",
    "props",
    "key",
    "ref",
    "scopeId",
    "children",
    "component",
    "dirs",
    "transition",
    "el",
    "placeholder",
    "anchor",
    "target",
    "targetStart",
    "targetAnchor",
    "suspense",
    "shapeFlag",
    "patchFlag",
    "appContext",
];

/// Vue intrinsic attribute names — always merged through
/// fallthrough on the runtime and never published on the
/// consumer-facing macro surface UNLESS the SFC author explicitly
/// re-declared the name in the macro's type argument.
///
/// Used by `PublishedSurfacePolicy::Refined` (filtered only when
/// `AnalyzedSurfaceItem.declared_in_macro_type_arg` is `false`).
pub const VUE_INTRINSIC_ATTR_NAMES: &[&str] = &["class", "style", "key", "ref"];

/// Apply the projection policy and return the names that survive.
///
/// All projection decisions are structural (driven by
/// [`AnalyzedSurfaceItem`] facts). No thresholds, no ratios, no
/// name-prefix heuristics.
pub fn names_for_policy(
    policy: PublishedSurfacePolicy,
    surface: &AnalyzedSurface,
) -> PolicyNamesResult {
    match policy {
        PublishedSurfacePolicy::Native => PolicyNamesResult {
            props: surface.props.iter().map(|i| i.name.clone()).collect(),
            events: surface.events.iter().map(|i| i.name.clone()).collect(),
            slots: surface.slots.iter().map(|i| i.name.clone()).collect(),
            exposed: surface.exposed.iter().map(|i| i.name.clone()).collect(),
        },
        PublishedSurfacePolicy::Compat => {
            let blocked: HashSet<&str> = COMPAT_BLOCKED_SLOT_NAMES.iter().copied().collect();
            PolicyNamesResult {
                props: surface.props.iter().map(|i| i.name.clone()).collect(),
                events: surface.events.iter().map(|i| i.name.clone()).collect(),
                slots: surface
                    .slots
                    .iter()
                    .filter(|s| !blocked.contains(s.name.as_str()))
                    .map(|s| s.name.clone())
                    .collect(),
                exposed: surface.exposed.iter().map(|i| i.name.clone()).collect(),
            }
        }
        PublishedSurfacePolicy::Refined => {
            // The shadow-event-prop set is derived structurally
            // from the declared emits: for every emit `X` the
            // bench refiner historically formed the `on{X}`
            // (camelCase) prop name to suppress. The set is
            // membership-tested below so prefix-matching is never
            // used.
            let shadow_event_props: HashSet<String> = surface
                .events
                .iter()
                .map(|e| event_name_to_on_prop_name(&e.name))
                .collect();
            let intrinsics: HashSet<&str> = VUE_INTRINSIC_ATTR_NAMES.iter().copied().collect();
            let blocked: HashSet<&str> = COMPAT_BLOCKED_SLOT_NAMES.iter().copied().collect();

            PolicyNamesResult {
                props: surface
                    .props
                    .iter()
                    .filter(|p| {
                        // Drop producer-flagged "global" props
                        // (HTMLAttributes-derived).
                        if p.global {
                            return false;
                        }
                        // Drop `on{X}` props that structurally
                        // shadow a declared emit, UNLESS the
                        // author explicitly re-declared the prop
                        // name in the macro type arg.
                        if shadow_event_props.contains(&p.name) && !p.declared_in_macro_type_arg {
                            return false;
                        }
                        // Drop Vue intrinsics not explicitly
                        // re-declared in the macro type arg.
                        if intrinsics.contains(p.name.as_str()) && !p.declared_in_macro_type_arg {
                            return false;
                        }
                        true
                    })
                    .map(|p| p.name.clone())
                    .collect(),
                events: surface.events.iter().map(|i| i.name.clone()).collect(),
                slots: surface
                    .slots
                    .iter()
                    .filter(|s| !blocked.contains(s.name.as_str()))
                    .map(|s| s.name.clone())
                    .collect(),
                exposed: surface.exposed.iter().map(|i| i.name.clone()).collect(),
            }
        }
    }
}

/// Convert an emit name to its `on{Event}` (camelCase) prop name
/// equivalent — the structural shadow form the `Refined` policy
/// filters.
///
/// Mirrors the bench refiner's prior JS-side derivation
/// `camelCase("on_" + event.name)`. Kept here so Rust and TS
/// consumers compute the same structural fingerprint.
///
/// Examples:
/// - `submit` → `onSubmit`
/// - `state-change` → `onStateChange`
/// - `update:modelValue` → `onUpdateModelValue`
pub fn event_name_to_on_prop_name(event_name: &str) -> String {
    // Algorithm: `"on_" + event_name`, then camelCase (collapse
    // runs of non-alphanumeric, capitalizing the following
    // alphanumeric; lowercase the very first character of the
    // result).
    let raw = format!("on_{event_name}");
    let bytes = raw.as_bytes();
    let mut out = String::with_capacity(bytes.len());

    // Skip leading non-alphanumerics (matches JS's first replace).
    let mut i = 0usize;
    while i < bytes.len() && !bytes[i].is_ascii_alphanumeric() {
        i += 1;
    }
    if i >= bytes.len() {
        return String::new();
    }

    // Second replace: collapse non-alphanumeric runs, capitalize
    // the following alphanumeric.
    let mut next_is_upper = false;
    while i < bytes.len() {
        let c = raw.as_bytes()[i] as char;
        if !c.is_ascii_alphanumeric() {
            next_is_upper = true;
        } else if next_is_upper {
            for cu in c.to_uppercase() {
                out.push(cu);
            }
            next_is_upper = false;
        } else {
            out.push(c);
        }
        i += 1;
    }

    // Third replace: lowercase the very first character if it is
    // uppercase ASCII (matches the JS `replace(/^[A-Z]/, ...)`).
    let mut chars = out.chars();
    if let Some(first) = chars.next() {
        if first.is_ascii_uppercase() {
            let mut lowered = String::with_capacity(out.len());
            for cl in first.to_lowercase() {
                lowered.push(cl);
            }
            lowered.push_str(chars.as_str());
            return lowered;
        }
    }
    out
}
