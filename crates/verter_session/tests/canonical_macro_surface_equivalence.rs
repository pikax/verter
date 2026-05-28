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
//! equivalence — names, order, types, optionality, readonly,
//! `declared_in_macro_type_arg` provenance, emit payloads, slot
//! bindings/returns.
//!
//! # Why these discriminate (CLAUDE.md Stub Prevention)
//!
//! Each test would FAIL if the canonical path dropped or reordered a
//! field, lost the `declared_in_macro_type_arg` provenance, mis-handled
//! call-signature emits, admitted a non-function slot, or walked an
//! unrelated package import. The discrimination proof for the
//! provenance field: removing the `MacroTypeArgOwnBody` context from
//! the canonical surface resolver (so the imported declaration's
//! own-body members lower structurally) makes
//! [`canonical_props_equivalence_direct_imported_root`] and
//! [`canonical_props_equivalence_heritage_own_vs_inherited`] go RED on
//! the `declared_in_macro_type_arg` assertion.
//!
//! Required fixtures (all hermetic, vendored — NO external corpus):
//! direct imported root + heritage (`extends`); call-signature emits
//! AND property-key emits (both forms); slots with NON-function
//! members; `withDefaults` (optionality changes); union / intersection
//! / conditional macro payloads; package-backed helper type with
//! UNRELATED imports (must not be walked); same-canonical-edit
//! warm-cache rejection.

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
    // Return type preserved on both arms.
    assert_eq!(
        e_default.return_expr.is_some(),
        l_default.return_expr.is_some(),
        "slot return_expr presence must match",
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
// withDefaults — defaults-object merge affects published optionality
// ===========================================================================
//
// `withDefaults` resolves the SAME props payload type as `defineProps`
// (its macro provenance is `MacroTypeArgOwnBody`, identical to
// `DefineProps` — see `macro_payload_surface_provenance`), then the thin
// normalizer merges the runtime defaults object: a prop with a default
// becomes NON-required on the published surface even when its type
// annotation is non-optional. This characterizes the withDefaults
// normalizer end-to-end through the production component-meta payload
// (the type-resolution half is proven field-for-field equivalent by the
// `canonical_props_equivalence_*` tests above).

#[test]
fn canonical_with_defaults_equivalence_defaults_relax_optionality() {
    use verter_session::component_meta_host::ComponentMetaHost;
    use verter_session::{CompileErrorPolicy, HostConfig};

    let mh = ComponentMetaHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    });
    // `size` is REQUIRED by type (`size: number`) but withDefaults
    // supplies a default, so the published prop must be non-required.
    // `label` has no default and stays required.
    let component = r#"
<script setup lang="ts">
interface Props { size: number; label: string }
withDefaults(defineProps<Props>(), { size: 10 });
</script>
<template><div /></template>
"#;
    mh.upsert_base("/src/WithDefaults.vue", component)
        .expect("WithDefaults.vue upsert");
    let meta = mh
        .host()
        .get_component_meta("/src/WithDefaults.vue")
        .expect("component meta resolves");

    let size = meta
        .props
        .iter()
        .find(|p| p.name == "size")
        .expect("meta.props contains `size`");
    let label = meta
        .props
        .iter()
        .find(|p| p.name == "label")
        .expect("meta.props contains `label`");

    // Discriminating: the withDefaults merge MUST relax `size` to
    // non-required (it has a default) while leaving `label` required.
    // A normalizer that ignored the defaults object would leave `size`
    // required; one that blanket-relaxed every prop would drop `label`'s
    // required flag.
    assert!(
        !size.required,
        "`size` has a withDefaults default → published prop MUST be non-required; \
         got required={}",
        size.required,
    );
    assert!(
        size.has_default,
        "`size` MUST carry has_default=true from the withDefaults object",
    );
    assert!(
        label.required,
        "`label` has NO default → MUST stay required; got required={}",
        label.required,
    );
    // NOTE: this test drives the EAGER production `get_component_meta`
    // path (Stage 1 does not flip the producer), so it characterizes the
    // withDefaults defaults-merge optionality only. The
    // `declared_in_macro_type_arg` provenance of the canonical path is
    // proven field-for-field equivalent to eager by the probe-based
    // `canonical_props_equivalence_*` tests above; it is intentionally
    // NOT re-asserted here against the unflipped production path.
}
