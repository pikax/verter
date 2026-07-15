//! Slot-binding registry-collection skip tests.
//!
//! Issue #8: When the registry-collection loop processes
//! `evaluated_types.slot_bindings`, calls to
//! `collect_component_meta_registry_public_field_refs` for slot
//! bindings whose raw type root is the owner's own
//! `defineProps<T>()` interface are wasted work — the defineProps
//! root is already authoritative for that surface. The skip
//! predicate fires only when:
//!
//! - The binding's raw type root resolves to a name in
//!   `collect_define_props_root_names(snapshot)` for the same owner.
//! - The binding does NOT introduce a new prop surface (no
//!   intersection / union broadening; not an imported root; not a
//!   peer SFC's defineProps interface).
//!
//! These tests use the per-request `CaptureToken` to discriminate
//! between the predicate firing (positive case, counter > 0) and
//! NOT firing (counterfixtures, counter == 0). Asserting on
//! `resolved_type_registry` names directly is not discriminating —
//! the slot-binding registry-collection loop in the present
//! implementation does not enqueue new names for any of the four
//! fixtures regardless of whether the predicate fires (the helper's
//! `prepared_type_decl(owner_canonical, ...)` lookup early-returns
//! for imported roots, and fully-expanded primitive `r#type` fields
//! produce no Ref to walk). The counter is the only observable that
//! changes between the four fixtures.

use std::sync::Arc;

use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

use crate::capture_token::CaptureToken;
use crate::meta_resolve::SLOT_BINDING_REGISTRY_COLLECTION_SKIP_COUNTER;
use crate::types::{HostConfig, ProjectionMode};
use crate::VerterHost;

/// Build a hermetic [`VerterHost`] backed by a [`MemoryWorkspace`]
/// pre-populated with the supplied files.
fn build_hermetic_host(files: &[(&str, &str)]) -> Arc<VerterHost> {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    for (canonical, content) in files {
        workspace.inject_file((*canonical).into(), Arc::from(*content));
    }
    let ws_access: Arc<dyn WorkspaceAccess> = workspace;
    Arc::new(VerterHost::new(HostConfig::default(), ws_access))
}

/// Drive the component-meta resolution path that exercises the
/// slot-binding registry-collection loop, returning the number of
/// times the skip predicate fired during this resolution. The
/// counter is captured via a per-request [`CaptureToken`] (no global
/// state) so parallel tests cannot pollute each other.
fn skip_count_for(host: &Arc<VerterHost>, canonical: &str) -> u64 {
    let guard = CaptureToken::start_for_query("slot_binding_registry_skip");
    let _ = host.get_component_meta(canonical);
    let _ = host.resolve_component_meta(canonical, ProjectionMode::Expanded);
    let snapshot = guard.end();
    snapshot.counter(SLOT_BINDING_REGISTRY_COLLECTION_SKIP_COUNTER)
}

// ── Positive: owner-rooted slot binding skips registry collection ──

const POSITIVE_OWNER_ROOTED_VUE: &str = r#"<script setup lang="ts">
interface Props {
  avatar: string;
  label: string;
  count: number;
}
defineProps<Props>();
defineSlots<{
  leading(props: { avatar: Props['avatar'] }): any;
}>();
</script>
<template><div /></template>
"#;

/// Positive case: slot binding `leading.avatar: Props['avatar']`
/// where `Props` is the owner's own defineProps root. The predicate
/// MUST fire — the slot binding's contribution to the registry is
/// redundant because `Props` is already authoritative.
#[test]
fn slot_binding_targets_define_props_root_skips_registry_collection() {
    let host = build_hermetic_host(&[("/A.vue", POSITIVE_OWNER_ROOTED_VUE)]);

    let skips = skip_count_for(&host, "/A.vue");

    assert!(
        skips >= 1,
        "expected the slot-binding registry-collection skip predicate to fire \
         at least once for an owner-rooted `Props['avatar']` binding (Issue #8); \
         got skip count {skips}",
    );
}

// ── Counterfixture 1: broadening intersection still seeds registry ──

const COUNTER_BROADENING_INTERSECTION_VUE: &str = r#"<script setup lang="ts">
interface Props {
  avatar: string;
  label: string;
  count: number;
}
interface Extra {
  extraProp: number;
}
defineProps<Props>();
defineSlots<{
  leading(props: Props & Extra): any;
}>();
</script>
<template><div /></template>
"#;

/// Counterfixture: the slot binding intersects `Props` with `Extra`,
/// broadening the surface beyond what the owner's defineProps
/// exposes (`extraProp` is not in `Props`). After enrichment the
/// expanded slot bindings have `raw_type: None` (the binding fields
/// are produced by walking the expanded intersection, not by
/// preserving its source text), so the predicate observes a
/// fully-expanded primitive expression and does NOT fire — the
/// registry-collection call still runs for each binding.
#[test]
fn slot_binding_intersection_broadens_surface_still_seeds_registry() {
    let host = build_hermetic_host(&[("/A.vue", COUNTER_BROADENING_INTERSECTION_VUE)]);

    let skips = skip_count_for(&host, "/A.vue");

    assert_eq!(
        skips, 0,
        "broadening intersection `Props & Extra` MUST NOT trigger the \
         slot-binding skip predicate; skipping would lose the `Extra` \
         arm beyond what defineProps exposes; got skip count {skips}",
    );
}

// ── Counterfixture 2: imported target still seeds registry ──

const COUNTER_IMPORTED_BUTTON_PROPS_TS: &str = r#"export interface ButtonProps {
  avatar: string;
  label: string;
}
"#;

const COUNTER_IMPORTED_TARGET_VUE: &str = r#"<script setup lang="ts">
import type { ButtonProps } from './button-props'
interface Props {
  count: number;
}
defineProps<Props>();
defineSlots<{
  leading(props: { avatar: ButtonProps['avatar'] }): any;
}>();
</script>
<template><div /></template>
"#;

/// Counterfixture: the slot binding targets an IMPORTED type
/// (`ButtonProps`) — not the owner's own defineProps root (`Props`).
/// The predicate's `define_props_roots` set contains only `Props`,
/// so a binding rooted at `ButtonProps['avatar']` MUST NOT fire the
/// skip — the owner's defineProps Props is not authoritative for
/// the imported `ButtonProps` surface.
#[test]
fn slot_binding_imported_target_still_seeds_registry() {
    let host = build_hermetic_host(&[
        ("/A.vue", COUNTER_IMPORTED_TARGET_VUE),
        ("/button-props.ts", COUNTER_IMPORTED_BUTTON_PROPS_TS),
    ]);

    let skips = skip_count_for(&host, "/A.vue");

    assert_eq!(
        skips, 0,
        "slot-binding root `ButtonProps` (imported, not the owner's defineProps \
         root) MUST NOT trigger the skip predicate — the owner's defineProps \
         Props is not authoritative for the imported `ButtonProps` surface; \
         got skip count {skips}",
    );
}

// ── Counterfixture 3: different-owner defineProps target ──

const COUNTER_PEER_PROPS_VUE: &str = r#"<script setup lang="ts">
export interface PeerProps {
  avatar: string;
  label: string;
}
defineProps<PeerProps>();
</script>
<template><div /></template>
"#;

const COUNTER_DIFFERENT_OWNER_VUE: &str = r#"<script setup lang="ts">
import type { PeerProps } from './Peer.vue'
interface Props {
  count: number;
}
defineProps<Props>();
defineSlots<{
  leading(props: { avatar: PeerProps['avatar'] }): any;
}>();
</script>
<template><div /></template>
"#;

/// Counterfixture: the slot binding targets a DIFFERENT owner's
/// defineProps root — `PeerProps` is the peer SFC's defineProps
/// interface, re-imported and used in `/A.vue`. The owner of
/// `/A.vue` declares its own `Props` interface for its own
/// defineProps. `PeerProps` is NOT in `define_props_roots` for
/// `/A.vue` (only `Props` is) so the predicate MUST NOT fire — a
/// peer SFC's defineProps interface is not authoritative for this
/// owner's slot bindings.
#[test]
fn slot_binding_targets_different_owner_define_props_still_seeds_registry() {
    let host = build_hermetic_host(&[
        ("/A.vue", COUNTER_DIFFERENT_OWNER_VUE),
        ("/Peer.vue", COUNTER_PEER_PROPS_VUE),
    ]);

    let skips = skip_count_for(&host, "/A.vue");

    assert_eq!(
        skips, 0,
        "slot-binding root `PeerProps` (a peer SFC's defineProps interface, \
         not /A.vue's own) MUST NOT trigger the skip predicate — only the \
         current owner's defineProps roots authorise the skip; \
         got skip count {skips}",
    );
}
