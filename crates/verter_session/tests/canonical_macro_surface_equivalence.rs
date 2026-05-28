//! Stage 1 consolidation — canonical macro-surface API equivalence
//! discriminators.
//!
//! # What these characterize
//!
//! Stage 1 builds the **canonical macro-surface API** (resolve ONE
//! payload type through the shared typed-IR dispatch, then read the
//! one-level surface through the shared surface reader) and proves it
//! **field-for-field equivalent** to the eager OXC rail on the same
//! inputs — the gate that lets Stage 2 flip production onto it.
//!
//! The comparison runs through the two production-relevant arms of the
//! `ResolvedMacroSurface` cutover seam:
//!
//! - `Eager(EagerResolvedMacro)` — driven by the REAL OXC producer
//!   (`host.resolve_macro_elements` → `project_macro_surfaces`).
//! - `LazyImported(ImportedMacroSurface)` — the canonical typed-IR
//!   path: resolve the imported declaration through the macro-aware
//!   shared resolver (`surface_view_from_base_node` evaluating the
//!   `DeclPlaceholder` under
//!   `SurfaceProvenanceContext::MacroTypeArgOwnBody` for props), then
//!   normalise.
//!
//! Both arms feed the SAME shared `prop_members` / `emit_members` /
//! `slot_members` interpretation, so each test asserts arm-to-arm
//! equivalence — prop names, order, per-field `TypeExpr`, optionality
//! (`is_optional`), `declared_in_macro_type_arg` provenance, emit
//! payloads (`payload_expr`), slot bindings (`binding_expr`), and slot
//! return types (`return_expr`, compared exactly). Note: there is NO
//! prop `readonly` field — `AnalyzedPropField` carries `is_optional`
//! only — so readonly is deliberately NOT among the asserted axes.
//!
//! # Why these discriminate (CLAUDE.md Stub Prevention)
//!
//! Each test would FAIL if the canonical path dropped or reordered a
//! field, lost the `declared_in_macro_type_arg` provenance, mis-handled
//! call-signature emits, admitted a non-function slot, dropped a slot's
//! return type, dropped an identity-alias shell, mis-synthesized a
//! union's common members, or walked an unrelated package import. The
//! discrimination proof for the provenance field: removing the
//! `MacroTypeArgOwnBody` context from the canonical surface resolver (so
//! the imported declaration's own-body members lower structurally) makes
//! [`canonical_props_equivalence_direct_imported_root`] and
//! [`canonical_props_equivalence_heritage_own_vs_inherited`] go RED on
//! the `declared_in_macro_type_arg` assertion. The alias-shell and
//! union-common-member fixtures
//! ([`canonical_props_equivalence_identity_utility_alias`],
//! [`canonical_props_equivalence_union_shared_member_published`]) go RED
//! against the pre-fix `surface_view_from_base_node` (which returned
//! `None` → empty surface for both shapes).
//!
//! Required fixtures (all hermetic, vendored — NO external corpus):
//! direct imported root + heritage (`extends`); call-signature emits
//! AND property-key emits (both forms); slots with NON-function members
//! AND exact slot-return comparison; `withDefaults` payload-surface
//! optionality through the lazy canonical path; identity/utility alias
//! shell (`NoInfer<Base>`); union with a SHARED member (common-members
//! synthesis) AND disjoint union (empty); intersection macro payload;
//! package-backed helper type with UNRELATED imports (must not be
//! walked); same-canonical-edit warm-cache rejection.

#![allow(clippy::too_many_lines)]

use std::sync::Arc;

use verter_semantic::analysis::AnalyzedMacroKind;
use verter_session::test_only::imported_macro_surface::{
    EagerMacroSurfaceProbe, ImportedMacroSurfaceProbe,
};
use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

const OWNER_VUE_PATH: &str = "/w/owner.vue";
const TYPES_TS_PATH: &str = "/w/types.ts";

/// Build a host with an owner SFC importing from `'./types'` and a
/// types file. Both injected (resolver reads) AND upserted (parsed +
/// shallow-indexed).
fn build_host(owner_vue: &'static str, types_source: &'static str) -> Arc<VerterHost> {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.inject_file(OWNER_VUE_PATH.into(), Arc::from(owner_vue));
    workspace.inject_file(TYPES_TS_PATH.into(), Arc::from(types_source));
    let ws: Arc<dyn WorkspaceAccess> = workspace;
    let host = Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            ..HostConfig::default()
        },
        ws,
    ));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(OWNER_VUE_PATH.into()),
        input_id: OWNER_VUE_PATH.into(),
        source: Arc::from(owner_vue),
        file_kind: FileKind::VueSfc,
        aliases: vec![],
    });
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(TYPES_TS_PATH.into()),
        input_id: TYPES_TS_PATH.into(),
        source: Arc::from(types_source),
        file_kind: FileKind::NonSfc,
        aliases: vec![],
    });
    host
}

fn eager_probe(host: &VerterHost, name: &str, kind: AnalyzedMacroKind) -> EagerMacroSurfaceProbe {
    EagerMacroSurfaceProbe::resolve(host, OWNER_VUE_PATH, "./types", name, kind).expect(
        "the eager OXC resolver MUST reach the imported declaration via \
         `./types` — a None here means the fixture's import route is broken \
         and the equivalence comparison would be vacuous",
    )
}

fn lazy_probe(name: &str) -> ImportedMacroSurfaceProbe {
    ImportedMacroSurfaceProbe::new(Arc::from(TYPES_TS_PATH), Arc::from(name), [0u8; 16])
}

// ===========================================================================
// PROPS — direct imported root: names, order, optionality, types, provenance
// ===========================================================================

const PROPS_DIRECT_TYPES: &str = r#"
export interface Props {
  a: string;
  b?: number;
  c: boolean;
}
"#;
const PROPS_DIRECT_OWNER: &str = r#"
<script setup lang="ts">
import type { Props } from './types';
defineProps<Props>();
</script>
<template><div /></template>
"#;

#[test]
fn canonical_props_equivalence_direct_imported_root() {
    let host = build_host(PROPS_DIRECT_OWNER, PROPS_DIRECT_TYPES);
    let eager =
        eager_probe(&host, "Props", AnalyzedMacroKind::DefineProps).eager_prop_members(&host);
    let lazy = lazy_probe("Props").lazy_prop_members(&host);

    assert_eq!(eager.len(), 3, "eager surface must have a, b, c");
    let eager_names: Vec<&str> = eager.iter().map(|p| p.name.as_str()).collect();
    let lazy_names: Vec<&str> = lazy.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(eager_names, lazy_names, "prop names/order must match");

    for (e, l) in eager.iter().zip(lazy.iter()) {
        assert_eq!(e.is_optional, l.is_optional, "optionality of `{}`", e.name);
        assert_eq!(e.type_expr, l.type_expr, "type_expr of `{}`", e.name);
        assert_eq!(
            e.declared_in_macro_type_arg, l.declared_in_macro_type_arg,
            "declared_in_macro_type_arg of `{}` must match (eager={}, lazy={})",
            e.name, e.declared_in_macro_type_arg, l.declared_in_macro_type_arg,
        );
    }
    // Every member of `Props`'s own body is author-declared in the
    // macro T → both arms report `true`.
    assert!(
        lazy.iter().all(|p| p.declared_in_macro_type_arg),
        "all own-body props carry declared_in_macro_type_arg=true on the \
         canonical surface; got {:?}",
        lazy.iter()
            .map(|p| (p.name.clone(), p.declared_in_macro_type_arg))
            .collect::<Vec<_>>(),
    );
}

// ===========================================================================
// PROPS — heritage (`extends`): own-body members `true`, inherited `false`
// ===========================================================================

const PROPS_HERITAGE_TYPES: &str = r#"
export interface Base {
  inherited: string;
}
export interface Props extends Base {
  own: number;
}
"#;
const PROPS_HERITAGE_OWNER: &str = r#"
<script setup lang="ts">
import type { Props } from './types';
defineProps<Props>();
</script>
<template><div /></template>
"#;

#[test]
fn canonical_props_equivalence_heritage_own_vs_inherited() {
    let host = build_host(PROPS_HERITAGE_OWNER, PROPS_HERITAGE_TYPES);
    let eager =
        eager_probe(&host, "Props", AnalyzedMacroKind::DefineProps).eager_prop_members(&host);
    let lazy = lazy_probe("Props").lazy_prop_members(&host);

    // Both arms surface `own` (own-body) and `inherited` (heritage).
    let lazy_own = lazy
        .iter()
        .find(|p| p.name == "own")
        .expect("canonical surface contains own-body `own`");
    let lazy_inherited = lazy
        .iter()
        .find(|p| p.name == "inherited")
        .expect("canonical surface contains heritage `inherited`");

    // The discriminating provenance split: `own` is author-declared in
    // the macro T body (`true`); `inherited` reaches via `extends`
    // (`false`). A canonical path that lost the macro-T provenance
    // would report `own=false`; one that leaked the body flag into
    // heritage descent would report `inherited=true`. Either flips this.
    assert!(
        lazy_own.declared_in_macro_type_arg,
        "own-body `own` MUST carry declared_in_macro_type_arg=true",
    );
    assert!(
        !lazy_inherited.declared_in_macro_type_arg,
        "heritage `inherited` MUST carry declared_in_macro_type_arg=false",
    );

    // Field-for-field against the eager rail.
    for name in ["own", "inherited"] {
        let e = eager.iter().find(|p| p.name == name).unwrap();
        let l = lazy.iter().find(|p| p.name == name).unwrap();
        assert_eq!(
            e.declared_in_macro_type_arg, l.declared_in_macro_type_arg,
            "declared_in_macro_type_arg of `{name}` must match eager",
        );
        assert_eq!(e.type_expr, l.type_expr, "type_expr of `{name}` must match");
    }
}

// ===========================================================================
// EMITS — call-signature form: event names from call signatures, not keyof
// ===========================================================================

const EMITS_CALLSIG_TYPES: &str = r#"
export interface Emits {
  (e: 'change', v: number): void;
  (e: 'submit'): void;
}
"#;
const EMITS_CALLSIG_OWNER: &str = r#"
<script setup lang="ts">
import type { Emits } from './types';
defineEmits<Emits>();
</script>
<template><div /></template>
"#;

#[test]
fn canonical_emits_equivalence_call_signature_form() {
    let host = build_host(EMITS_CALLSIG_OWNER, EMITS_CALLSIG_TYPES);
    let eager =
        eager_probe(&host, "Emits", AnalyzedMacroKind::DefineEmits).eager_emit_members(&host);
    let lazy = lazy_probe("Emits").lazy_emit_members(&host);

    let eager_names: Vec<&str> = eager.iter().map(|e| e.name.as_str()).collect();
    let lazy_names: Vec<&str> = lazy.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        eager_names,
        vec!["change", "submit"],
        "eager extracts event names from call signatures",
    );
    assert_eq!(
        lazy_names, eager_names,
        "canonical arm MUST extract event names from call signatures, not keyof",
    );
    // Negative: no numeric-index pseudo-events.
    assert!(
        !lazy_names
            .iter()
            .any(|n| n.chars().all(|c| c.is_ascii_digit())),
        "no numeric-index pseudo-events may leak: {lazy_names:?}",
    );
    for (e, l) in eager.iter().zip(lazy.iter()) {
        assert_eq!(
            e.payload_expr, l.payload_expr,
            "payload TypeExpr for `{}` must match (event-name param stripped)",
            e.name,
        );
    }
}

// ===========================================================================
// EMITS — property-key form: event names from member names
// ===========================================================================

const EMITS_PROP_TYPES: &str = r#"
export interface Emits {
  change: [value: number];
  submit: [];
}
"#;
const EMITS_PROP_OWNER: &str = r#"
<script setup lang="ts">
import type { Emits } from './types';
defineEmits<Emits>();
</script>
<template><div /></template>
"#;

#[test]
fn canonical_emits_equivalence_property_key_form() {
    let host = build_host(EMITS_PROP_OWNER, EMITS_PROP_TYPES);
    let eager =
        eager_probe(&host, "Emits", AnalyzedMacroKind::DefineEmits).eager_emit_members(&host);
    let lazy = lazy_probe("Emits").lazy_emit_members(&host);

    let eager_names: Vec<&str> = eager.iter().map(|e| e.name.as_str()).collect();
    let lazy_names: Vec<&str> = lazy.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(eager_names, vec!["change", "submit"]);
    assert_eq!(
        lazy_names, eager_names,
        "property-key emit names must match"
    );
    for (e, l) in eager.iter().zip(lazy.iter()) {
        assert_eq!(e.payload_expr, l.payload_expr, "payload for `{}`", e.name);
    }
}

// ===========================================================================
// SLOTS — non-function members filtered; function members keep bindings
// ===========================================================================

const SLOTS_FILTER_TYPES: &str = r#"
export interface Slots {
  default: (props: { item: string; index: number }) => any;
  notASlot: string;
}
"#;
const SLOTS_FILTER_OWNER: &str = r#"
<script setup lang="ts">
import type { Slots } from './types';
defineSlots<Slots>();
</script>
<template><div /></template>
"#;

#[test]
fn canonical_slots_equivalence_non_function_filtered_and_bindings() {
    let host = build_host(SLOTS_FILTER_OWNER, SLOTS_FILTER_TYPES);
    let eager =
        eager_probe(&host, "Slots", AnalyzedMacroKind::DefineSlots).eager_slot_members(&host);
    let lazy = lazy_probe("Slots").lazy_slot_members(&host);

    let eager_names: Vec<&str> = eager.iter().map(|s| s.name.as_str()).collect();
    let lazy_names: Vec<&str> = lazy.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        eager_names,
        vec!["default"],
        "eager keeps only the function slot"
    );
    assert_eq!(
        lazy_names, eager_names,
        "canonical arm filters non-function `notASlot`"
    );
    assert!(
        !lazy_names.contains(&"notASlot"),
        "non-function `notASlot` must NOT appear as a slot: {lazy_names:?}",
    );

    // Bindings of the function slot must match (item, index).
    let e_default = &eager[0];
    let l_default = &lazy[0];
    let e_bindings: Vec<&str> = e_default.bindings.iter().map(|b| b.name.as_str()).collect();
    let l_bindings: Vec<&str> = l_default.bindings.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(e_bindings, vec!["item", "index"], "eager slot bindings");
    assert_eq!(l_bindings, e_bindings, "canonical slot bindings must match");
    for (e, l) in e_default.bindings.iter().zip(l_default.bindings.iter()) {
        assert_eq!(
            e.binding_expr, l.binding_expr,
            "binding_expr for `{}`",
            e.name
        );
    }
    // Return type preserved on both arms — compared EXACTLY (not merely
    // presence): the slot's `return_expr` TypeExpr must round-trip
    // identically through the canonical surface reader. A presence-only
    // check would miss a canonical path that produced a DIFFERENT return
    // shape (e.g. `any` vs the declared return); the exact compare
    // discriminates that.
    assert_eq!(
        e_default.return_expr, l_default.return_expr,
        "slot `default` return_expr must match the eager arm EXACTLY: \
         eager={:?} lazy={:?}",
        e_default.return_expr, l_default.return_expr,
    );
}

// ===========================================================================
// PROPS — intersection literal payload (own-body in BOTH arms)
// ===========================================================================

const PROPS_INTERSECTION_TYPES: &str = r#"
export interface A { a: string; }
export interface B { b: number; }
export type Props = A & B;
"#;
const PROPS_INTERSECTION_OWNER: &str = r#"
<script setup lang="ts">
import type { Props } from './types';
defineProps<Props>();
</script>
<template><div /></template>
"#;

#[test]
fn canonical_props_equivalence_intersection_payload() {
    let host = build_host(PROPS_INTERSECTION_OWNER, PROPS_INTERSECTION_TYPES);
    let eager =
        eager_probe(&host, "Props", AnalyzedMacroKind::DefineProps).eager_prop_members(&host);
    let lazy = lazy_probe("Props").lazy_prop_members(&host);

    let mut eager_names: Vec<&str> = eager.iter().map(|p| p.name.as_str()).collect();
    let mut lazy_names: Vec<&str> = lazy.iter().map(|p| p.name.as_str()).collect();
    eager_names.sort_unstable();
    lazy_names.sort_unstable();
    assert_eq!(
        eager_names,
        vec!["a", "b"],
        "intersection surfaces both members"
    );
    assert_eq!(
        lazy_names, eager_names,
        "canonical intersection surface must match"
    );
    for name in ["a", "b"] {
        let e = eager.iter().find(|p| p.name == name).unwrap();
        let l = lazy.iter().find(|p| p.name == name).unwrap();
        assert_eq!(e.type_expr, l.type_expr, "type_expr of `{name}`");
        assert_eq!(
            e.declared_in_macro_type_arg, l.declared_in_macro_type_arg,
            "provenance of `{name}` must match",
        );
    }
}

// ===========================================================================
// PROPS — union payload carrier (bare disjoint union)
// ===========================================================================
//
// CHARACTERIZATION (not strict eager==lazy equality): a bare union of
// two DISJOINT object types (`A | B` with no common members) is a
// known canonical-vs-eager DIVERGENCE that predates this change and is
// orthogonal to the `declared_in_macro_type_arg` provenance work:
//
// - The eager OXC rail flattens `A | B` into the UNION of members
//   (`["a", "b"]`) — i.e. it exposes every member of either arm.
// - The canonical typed-IR surface reader (`surface_view_from_base_node`)
//   returns no single member surface for a bare `Union` carrier (a union
//   has no single object surface a macro payload reads), so the canonical
//   prop set is the COMMON members only — empty for a disjoint union.
//   This is the TS-correct shallow reading: `(A | B)['k']` is valid only
//   when `k` is in BOTH arms.
//
// `vue-component-meta` is NOT a ground-truth oracle (the eager rail's
// union-of-members behaviour is itself questionable), so this test does
// NOT assert the canonical path must reproduce the eager over-production.
// It pins the canonical contract (common-members-only) AND records the
// eager divergence so Stage 2's producer flip reconciles it deliberately
// rather than silently. The lazy arm NEVER read bare unions (by design),
// so this is not a regression introduced by the provenance work.

const PROPS_UNION_TYPES: &str = r#"
export interface A { a: string; }
export interface B { b: number; }
export type Props = A | B;
"#;
const PROPS_UNION_OWNER: &str = r#"
<script setup lang="ts">
import type { Props } from './types';
defineProps<Props>();
</script>
<template><div /></template>
"#;

#[test]
fn canonical_props_equivalence_union_payload_carrier_common_members_only() {
    let host = build_host(PROPS_UNION_OWNER, PROPS_UNION_TYPES);
    let eager =
        eager_probe(&host, "Props", AnalyzedMacroKind::DefineProps).eager_prop_members(&host);
    let lazy = lazy_probe("Props").lazy_prop_members(&host);

    let eager_names: Vec<&str> = eager.iter().map(|p| p.name.as_str()).collect();
    let lazy_names: Vec<&str> = lazy.iter().map(|p| p.name.as_str()).collect();

    // Canonical contract: a disjoint object union has NO common members,
    // so the canonical surface is empty. This discriminates a canonical
    // path that wrongly breadth-flattened the union arms (it would
    // surface `a`/`b`) from the correct common-members-only reading.
    assert!(
        lazy_names.is_empty(),
        "canonical surface for a bare DISJOINT union `A | B` exposes only \
         COMMON members (none here) — got {lazy_names:?}. A non-empty set \
         means the canonical reader wrongly flattened the union arms.",
    );
    // Record the eager divergence (the eager rail flattens to the union
    // of members). This is the deliberate Stage-2 reconciliation point;
    // if the eager rail ever stops over-producing, this assertion flags
    // it so the divergence note can be retired.
    assert_eq!(
        eager_names,
        vec!["a", "b"],
        "eager rail flattens the disjoint union to the union of members \
         (the documented over-production); got {eager_names:?}",
    );
}

// ===========================================================================
// PACKAGE-BACKED helper with UNRELATED imports — must NOT be walked
// ===========================================================================

const PKG_UNRELATED_OWNER: &str = r#"
<script setup lang="ts">
import type { Props } from './types';
defineProps<Props>();
</script>
<template><div /></template>
"#;
const PKG_UNRELATED_TYPES: &str = r#"
import type { Unrelated } from 'some-pkg/unrelated';
export interface Props {
  label: string;
  // `helper` references an unrelated package type; the canonical
  // surface must keep it shallow (a carrier Ref), NOT breadth-walk
  // `Unrelated`'s own members into this surface.
  helper: Unrelated;
}
"#;

#[test]
fn canonical_props_equivalence_unrelated_package_import_not_walked() {
    let host = build_host(PKG_UNRELATED_OWNER, PKG_UNRELATED_TYPES);
    let eager =
        eager_probe(&host, "Props", AnalyzedMacroKind::DefineProps).eager_prop_members(&host);
    let lazy = lazy_probe("Props").lazy_prop_members(&host);

    let eager_names: Vec<&str> = eager.iter().map(|p| p.name.as_str()).collect();
    let lazy_names: Vec<&str> = lazy.iter().map(|p| p.name.as_str()).collect();
    // The macro surface is `{ label, helper }` — exactly two members.
    // `Unrelated`'s OWN members (from the unresolved package) must NOT
    // appear on this surface; that would be a Rule-5 breadth leak.
    assert_eq!(
        lazy_names, eager_names,
        "canonical surface member set MUST match eager (no unrelated-package \
         member leak): eager={eager_names:?}, lazy={lazy_names:?}",
    );
    assert!(
        lazy.iter().any(|p| p.name == "label") && lazy.iter().any(|p| p.name == "helper"),
        "canonical surface must keep the declared members `label` + `helper`",
    );
    assert_eq!(
        lazy.len(),
        2,
        "exactly the two declared members — `Unrelated`'s members must not \
         be breadth-walked into the surface: {lazy_names:?}",
    );
}

// ===========================================================================
// SAME-CANONICAL EDIT — warm-cache rejection (edit types.ts, surface updates)
// ===========================================================================

const SAME_CANON_OWNER: &str = r#"
<script setup lang="ts">
import type { Props } from './types';
defineProps<Props>();
</script>
<template><div /></template>
"#;
const SAME_CANON_TYPES_V1: &str = r#"
export interface Props {
  a: string;
}
"#;
const SAME_CANON_TYPES_V2: &str = r#"
export interface Props {
  a: string;
  added: number;
}
"#;

#[test]
fn canonical_props_equivalence_same_canonical_edit_rejects_warm_cache() {
    let host = build_host(SAME_CANON_OWNER, SAME_CANON_TYPES_V1);

    // Warm the canonical surface against V1.
    let v1 = lazy_probe("Props").lazy_prop_members(&host);
    let v1_names: Vec<&str> = v1.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(v1_names, vec!["a"], "V1 surface has only `a`");

    // Edit types.ts in place (same canonical id, new content).
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(TYPES_TS_PATH.into()),
        input_id: TYPES_TS_PATH.into(),
        source: Arc::from(SAME_CANON_TYPES_V2),
        file_kind: FileKind::NonSfc,
        aliases: vec![],
    });

    // The canonical surface MUST reflect the edit (warm-cache rejected on
    // same-canonical content change). A stale warm hit would still report
    // only `a`.
    let v2 = lazy_probe("Props").lazy_prop_members(&host);
    let mut v2_names: Vec<&str> = v2.iter().map(|p| p.name.as_str()).collect();
    v2_names.sort_unstable();
    assert_eq!(
        v2_names,
        vec!["a", "added"],
        "after a same-canonical edit the canonical surface MUST include the \
         newly-added member `added` — a stale warm-cache hit would still \
         report only `a`",
    );
}

// ===========================================================================
// withDefaults — payload-surface optionality through the LAZY canonical path
// ===========================================================================
//
// `withDefaults(defineProps<T>(), …)` resolves the SAME props payload type
// `T` as `defineProps<T>()`, under the SAME macro provenance
// (`macro_payload_surface_provenance(WithDefaults)` ==
// `SurfaceProvenanceContext::MacroTypeArgOwnBody`, identical to
// `DefineProps`). The runtime defaults-object merge (relaxing a typed-
// required prop to non-required) is a SEPARATE downstream normalizer step
// applied to the published meta — it is NOT part of the macro payload
// surface. This test therefore characterizes the lazy CANONICAL payload
// surface for the `WithDefaults` macro kind: it drives the lazy bridge
// (`lazy_prop_members`) against the REAL eager OXC producer
// (`eager_prop_members` resolved under `AnalyzedMacroKind::WithDefaults`),
// exactly as the other `canonical_*_equivalence` tests drive lazy vs eager.
//
// The discriminating signal is the payload surface's TYPE-LEVEL optionality
// + own-body provenance under the withDefaults macro kind: a lazy canonical
// path that mishandled withDefaults optionality (dropped the `?` of an
// optional payload member, or read the surface under `Structural` instead
// of `MacroTypeArgOwnBody` so own-body members lost `declared_in_macro_type_arg`)
// would diverge from the eager arm here. The production defaults-merge
// (`get_component_meta`) is exercised by the host-level meta tests in
// `crate::meta_tests`, not re-characterized here.

const WITH_DEFAULTS_TYPES: &str = r#"
export interface Props {
  size: number;
  variant?: 'a' | 'b';
  label: string;
}
"#;
const WITH_DEFAULTS_OWNER: &str = r#"
<script setup lang="ts">
import type { Props } from './types';
withDefaults(defineProps<Props>(), { size: 10 });
</script>
<template><div /></template>
"#;

#[test]
fn canonical_with_defaults_equivalence_payload_surface_optionality() {
    let host = build_host(WITH_DEFAULTS_OWNER, WITH_DEFAULTS_TYPES);

    // Eager arm driven under the `WithDefaults` macro kind — the REAL OXC
    // producer projects the `Props` payload surface for withDefaults.
    let eager =
        eager_probe(&host, "Props", AnalyzedMacroKind::WithDefaults).eager_prop_members(&host);
    // Lazy canonical arm: resolve the imported `Props` declaration through
    // the macro-aware shared surface reader. (`lazy_prop_members` enters
    // under `MacroTypeArgOwnBody`, the SAME provenance the withDefaults
    // projector uses — `macro_payload_surface_provenance(WithDefaults)`.)
    let lazy = lazy_probe("Props").lazy_prop_members(&host);

    // Discriminator: the eager rail must produce the full surface.
    assert_eq!(
        eager.len(),
        3,
        "eager withDefaults payload surface must have size, variant, label; got {:?}",
        eager.iter().map(|p| &p.name).collect::<Vec<_>>(),
    );

    let eager_names: Vec<&str> = eager.iter().map(|p| p.name.as_str()).collect();
    let lazy_names: Vec<&str> = lazy.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        eager_names, lazy_names,
        "lazy withDefaults payload member names/order MUST match the eager arm",
    );

    // Type-level optionality + TypeExpr + provenance, field-for-field. A
    // lazy-canonical withDefaults optionality bug (lost `variant?`, or all
    // members flipped) diverges here against the eager arm.
    for (e, l) in eager.iter().zip(lazy.iter()) {
        assert_eq!(
            e.is_optional, l.is_optional,
            "optionality mismatch for withDefaults payload prop `{}` \
             (eager={}, lazy={})",
            e.name, e.is_optional, l.is_optional,
        );
        assert_eq!(
            e.type_expr, l.type_expr,
            "TypeExpr mismatch for withDefaults payload prop `{}`",
            e.name,
        );
        assert_eq!(
            e.declared_in_macro_type_arg, l.declared_in_macro_type_arg,
            "declared_in_macro_type_arg mismatch for withDefaults payload prop \
             `{}` (eager={}, lazy={}) — the WithDefaults surface MUST carry the \
             own-body provenance, identical to DefineProps",
            e.name, e.declared_in_macro_type_arg, l.declared_in_macro_type_arg,
        );
    }

    // Negative: exactly `variant` is the optional payload member. Rules out
    // a lazy arm that dropped optionality (all-required) or inverted it.
    let lazy_optional: Vec<&str> = lazy
        .iter()
        .filter(|p| p.is_optional)
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(
        lazy_optional,
        vec!["variant"],
        "exactly `variant` is optional in the lazy withDefaults payload surface; \
         got {lazy_optional:?}",
    );

    // Positive provenance: every own-body member of the withDefaults payload
    // `Props` carries declared_in_macro_type_arg=true (the WithDefaults macro
    // kind enters under MacroTypeArgOwnBody, identical to DefineProps).
    assert!(
        lazy.iter().all(|p| p.declared_in_macro_type_arg),
        "every own-body withDefaults payload prop MUST carry \
         declared_in_macro_type_arg=true; got {:?}",
        lazy.iter()
            .map(|p| (p.name.clone(), p.declared_in_macro_type_arg))
            .collect::<Vec<_>>(),
    );
}

// ===========================================================================
// PROPS — identity/utility alias shell (`NoInfer<Base>`)
// ===========================================================================
//
// `build_instantiate` lowers an identity / utility alias such as
// `NoInfer<T>` to `SemanticNodeData::Alias(source)`. Before the alias-shell
// fix the canonical surface reader (`surface_view_from_base_node`) handled
// only `Object` / `Intersection` / `DeclPlaceholder` and the catch-all
// returned `None` — which `resolve_surface_view` turns into an EMPTY macro
// surface. So `type Props = NoInfer<Base>` lost every member.
//
// `NoInfer<Base>` IS `Base` (an identity utility — no member transformation),
// so the canonical surface of the alias MUST equal the canonical surface of
// the un-aliased `Base`: same member names/order, same per-field TypeExpr,
// same optionality, AND the same `declared_in_macro_type_arg` provenance
// (own-body members stay `true` at the macro-T root — the alias is a
// transparent indirection, matching the eager same-file rail's identity-
// utility provenance propagation for `Partial` / `Required` in
// `verter_semantic::analysis::macros::resolve_type_to_prop_fields`).
//
// EAGER-ORACLE NOTE: the `EagerMacroSurfaceProbe` cross-file path
// (`resolve_macro_elements`) does NOT resolve a named alias-to-`NoInfer`
// (it returns an empty surface for `export type Props = NoInfer<Base>`),
// so it is NOT a usable oracle for THIS shape. The ground truth used here
// is the un-aliased `Base` surface produced by the SAME canonical reader
// (which IS field-for-field equivalent to eager — proven by
// `canonical_props_equivalence_direct_imported_root`). The production
// `get_component_meta` path independently resolves `NoInfer<Base>` to
// Base's two members with the matching required/optional flags, confirming
// the alias is transparent in production.

const PROPS_ALIAS_SHELL_TYPES: &str = r#"
export interface Base {
  a: string;
  b?: number;
  c: boolean;
}
export type Props = NoInfer<Base>;
"#;
const PROPS_ALIAS_SHELL_OWNER: &str = r#"
<script setup lang="ts">
import type { Props } from './types';
defineProps<Props>();
</script>
<template><div /></template>
"#;

#[test]
fn canonical_props_equivalence_identity_utility_alias() {
    let host = build_host(PROPS_ALIAS_SHELL_OWNER, PROPS_ALIAS_SHELL_TYPES);

    // Ground truth: the un-aliased `Base` surface through the SAME
    // canonical reader (proven eager-equivalent by
    // `canonical_props_equivalence_direct_imported_root`).
    let base = lazy_probe("Base").lazy_prop_members(&host);
    // The aliased `type Props = NoInfer<Base>` surface.
    let alias = lazy_probe("Props").lazy_prop_members(&host);

    // Pre-fix discrimination: before the `Alias` arm landed,
    // `surface_view_from_base_node` returned `None` for the alias shell,
    // so `alias` was EMPTY. This assertion (non-empty + equal to Base)
    // FAILS against the pre-fix reader and PASSES post-fix.
    assert_eq!(
        base.len(),
        3,
        "ground-truth Base surface must have a, b, c; got {:?}",
        base.iter().map(|p| &p.name).collect::<Vec<_>>(),
    );
    assert_eq!(
        alias.len(),
        base.len(),
        "the identity-alias `NoInfer<Base>` surface MUST carry every member \
         of `Base` — an empty/short surface means the alias shell was dropped \
         (the pre-fix `None` → empty-surface bug). got alias={:?}",
        alias.iter().map(|p| &p.name).collect::<Vec<_>>(),
    );

    let base_names: Vec<&str> = base.iter().map(|p| p.name.as_str()).collect();
    let alias_names: Vec<&str> = alias.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        alias_names, base_names,
        "alias-shell member names/order MUST equal the un-aliased Base surface",
    );

    for (b, a) in base.iter().zip(alias.iter()) {
        assert_eq!(
            a.is_optional, b.is_optional,
            "optionality of `{}` must match the un-aliased Base surface",
            b.name,
        );
        assert_eq!(
            a.type_expr, b.type_expr,
            "type_expr of `{}` must match the un-aliased Base surface",
            b.name,
        );
        assert_eq!(
            a.declared_in_macro_type_arg, b.declared_in_macro_type_arg,
            "declared_in_macro_type_arg of `{}` must match (alias is a \
             transparent identity indirection): Base={}, alias={}",
            b.name, b.declared_in_macro_type_arg, a.declared_in_macro_type_arg,
        );
    }

    // Positive provenance assertion: `NoInfer<Base>` is the macro-T own
    // body, so every member carries `declared_in_macro_type_arg = true`
    // (identical to `defineProps<Base>()`). A reader that downgraded the
    // alias to structural would report `false` here.
    assert!(
        alias.iter().all(|p| p.declared_in_macro_type_arg),
        "every member surfaced through the identity alias `NoInfer<Base>` \
         MUST carry declared_in_macro_type_arg=true (the alias is transparent \
         at the macro-T root); got {:?}",
        alias
            .iter()
            .map(|p| (p.name.clone(), p.declared_in_macro_type_arg))
            .collect::<Vec<_>>(),
    );
}

// ===========================================================================
// PROPS — union with a SHARED member (common-members synthesis)
// ===========================================================================
//
// `surface_view_from_base_node` previously collapsed ALL unions to `None`,
// so `A | B` published nothing. The existing disjoint-union test
// (`canonical_props_equivalence_union_payload_carrier_common_members_only`)
// passed only because disjoint → empty is coincidentally the correct
// common-members result. This test uses an OVERLAPPING union: `A | B`
// where both arms declare `shared`. The TS-correct shallow surface of a
// union-typed macro payload is its COMMON members, so `shared` MUST be
// published (typed as the union of the per-arm member types), while the
// arm-exclusive members (`onlyA`, `onlyB`) must NOT appear.

const PROPS_UNION_SHARED_TYPES: &str = r#"
export interface A { shared: string; onlyA: number; }
export interface B { shared: string; onlyB: boolean; }
export type Props = A | B;
"#;
const PROPS_UNION_SHARED_OWNER: &str = r#"
<script setup lang="ts">
import type { Props } from './types';
defineProps<Props>();
</script>
<template><div /></template>
"#;

#[test]
fn canonical_props_equivalence_union_shared_member_published() {
    let host = build_host(PROPS_UNION_SHARED_OWNER, PROPS_UNION_SHARED_TYPES);
    let lazy = lazy_probe("Props").lazy_prop_members(&host);

    let lazy_names: Vec<&str> = lazy.iter().map(|p| p.name.as_str()).collect();

    // Pre-fix discrimination: before the `Union` arm landed,
    // `surface_view_from_base_node` returned `None` for ANY union, so the
    // surface was EMPTY even for an overlapping union. This assertion
    // (the shared member IS published) FAILS pre-fix and PASSES post-fix.
    assert!(
        lazy_names.contains(&"shared"),
        "the union `A | B` shares member `shared` (present in BOTH arms), so \
         the canonical common-members surface MUST publish it — an empty \
         surface means the union collapsed to None (pre-fix bug). got {lazy_names:?}",
    );

    // Negative: arm-exclusive members must NOT leak into the common-members
    // surface. `(A | B)['onlyA']` is not well-typed (absent from B), so the
    // shallow union surface must exclude it.
    assert!(
        !lazy_names.contains(&"onlyA") && !lazy_names.contains(&"onlyB"),
        "arm-exclusive members (`onlyA`, `onlyB`) MUST NOT appear on the \
         common-members surface of `A | B` — only members present in EVERY \
         arm are published. got {lazy_names:?}",
    );
    assert_eq!(
        lazy_names,
        vec!["shared"],
        "the common-members surface of `A | B` is exactly `{{ shared }}`; \
         got {lazy_names:?}",
    );
}

#[test]
fn canonical_props_equivalence_union_disjoint_still_empty() {
    // Disjoint-union companion to the shared-member test: a union with NO
    // common key still publishes an EMPTY surface (the common-members
    // contract — disjoint → empty is the correct, not coincidental,
    // result). Guards against a regression that wrongly flattened a
    // disjoint union to the union of arm members.
    const TYPES: &str = r#"
export interface A { a: string; }
export interface B { b: number; }
export type Props = A | B;
"#;
    const OWNER: &str = r#"
<script setup lang="ts">
import type { Props } from './types';
defineProps<Props>();
</script>
<template><div /></template>
"#;
    let host = build_host(OWNER, TYPES);
    let lazy = lazy_probe("Props").lazy_prop_members(&host);
    let lazy_names: Vec<&str> = lazy.iter().map(|p| p.name.as_str()).collect();
    assert!(
        lazy_names.is_empty(),
        "a DISJOINT union `A | B` (no shared key) has no common members, so \
         the canonical surface MUST be empty — a non-empty set means the \
         union arms were wrongly flattened. got {lazy_names:?}",
    );
}
