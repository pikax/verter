//! Stage 2B.1 — eager/lazy macro-authority equivalence discriminators.
//!
//! # What these characterize
//!
//! Stage 2B.1 migrated the macro-shape producers
//! (`macro_shapes.rs`) and the slot-binding graph
//! (`slot_binding_graph.rs`) to read their `defineProps` /
//! `defineEmits` / `defineSlots` member sets through the
//! [`ResolvedMacroSurface`](verter_session) enum's shared
//! interpretation accessors (`prop_members` / `emit_members` /
//! `slot_members`) rather than the direct `.props` / `.emits` /
//! `.slots` fields on `ResolvedMacroMeta`.
//!
//! The enum has two production-relevant arms:
//!
//! - `Eager(EagerResolvedMacro)` — the OXC-resolved-elements surface
//!   the cold resolver produces today. Its accessor returns the
//!   stored field vector verbatim, so it is bit-identical to the
//!   pre-migration direct field read.
//! - `LazyImported(ImportedMacroSurface)` — the typed-IR bridge. Its
//!   accessor resolves the imported declaration's one-level surface
//!   through dispatch and reconstructs the `Analyzed*Field` set.
//!
//! **The contract under test:** for the SAME imported declaration,
//! both arms must produce a bit-identical member set. The eager arm
//! is driven by the REAL OXC producer
//! (`host.resolve_macro_elements` → `project_macro_surfaces`); the
//! lazy arm is driven by the real typed-IR reconstruction. Both flow
//! through the shared accessor, so each test asserts arm-to-arm
//! equivalence — not arm-to-hand-built.
//!
//! # Why these discriminate
//!
//! - The tests reference `EagerMacroSurfaceProbe` /
//!   `ImportedMacroSurfaceProbe::lazy_*_members`, symbols that do not
//!   exist on a tree without the Stage 2B.1 migration — so the file
//!   does not compile against the pre-migration tree.
//! - Each assertion would FAIL if the lazy arm's macro interpretation
//!   diverged from the eager arm's. The two codex-flagged cases are
//!   explicitly characterized:
//!   - [`define_emits_call_signature_extraction_eager_lazy_equivalent`]
//!     — the lazy arm MUST extract event names from call signatures,
//!     not from `keyof` (which would surface numeric tuple indices).
//!   - [`define_slots_non_function_filtering_eager_lazy_equivalent`]
//!     — the lazy arm MUST filter non-function slot members.
//!
//!   A temporary revert of the lazy arm's macro transform (returning
//!   raw `keyof` names without the call-signature / function-filter
//!   transform) makes tests 2 and 4 FAIL — the divergence-detection
//!   proof recorded in the migration commit.

#![allow(clippy::too_many_lines)]

use std::sync::Arc;

use verter_semantic::analysis::AnalyzedMacroKind;
use verter_session::test_only::imported_macro_surface::{
    EagerMacroSurfaceProbe, ImportedMacroSurfaceProbe,
};
use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

// ---------------------------------------------------------------------------
// Hermetic host harness
// ---------------------------------------------------------------------------

const OWNER_VUE_PATH: &str = "/w/owner.vue";
const TYPES_TS_PATH: &str = "/w/types.ts";

/// Build a host with an owner SFC at [`OWNER_VUE_PATH`] that imports
/// from `'./types'` and a types file at [`TYPES_TS_PATH`] holding
/// `types_source`. Both files are injected into the workspace (so the
/// resolver can read them) AND upserted (so they are parsed +
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

/// Resolve the eager `ResolvedMacroMeta` for the imported declaration
/// `exported_name` (from `'./types'`) on the owner SFC. Panics with a
/// discriminating message if the eager OXC rail cannot reach the
/// declaration — that would make the equivalence comparison vacuous.
fn eager_probe(
    host: &VerterHost,
    exported_name: &str,
    kind: AnalyzedMacroKind,
) -> EagerMacroSurfaceProbe {
    EagerMacroSurfaceProbe::resolve(host, OWNER_VUE_PATH, "./types", exported_name, kind).expect(
        "the eager OXC resolver MUST reach the imported declaration via \
         `./types` — a None here means the fixture's import route is \
         broken and the equivalence comparison would be vacuous",
    )
}

/// Build a lazy bridge probe targeting the imported declaration
/// `exported_name` at the canonical types-file path.
fn lazy_probe(exported_name: &str) -> ImportedMacroSurfaceProbe {
    ImportedMacroSurfaceProbe::new(
        Arc::from(TYPES_TS_PATH),
        Arc::from(exported_name),
        [0u8; 16],
    )
}

// ---------------------------------------------------------------------------
// Test 1 — defineProps shape equivalence (names, order, optionality, type)
// ---------------------------------------------------------------------------

const PROPS_TYPES_TS: &str = r#"
export interface Props {
  a: string;
  b?: number;
  c: boolean;
}
"#;

const PROPS_OWNER_VUE: &str = r#"
<script setup lang="ts">
import type { Props } from './types';
defineProps<Props>();
</script>
<template><div /></template>
"#;

/// The eager and lazy arms must produce a bit-identical
/// `defineProps` member set for `Props { a: string; b?: number;
/// c: boolean }`: same names, same order, same optionality (`b?`),
/// same per-field `TypeExpr`, same `declared_in_macro_type_arg`.
#[test]
fn define_props_shape_eager_lazy_equivalent() {
    let host = build_host(PROPS_OWNER_VUE, PROPS_TYPES_TS);

    let eager =
        eager_probe(&host, "Props", AnalyzedMacroKind::DefineProps).eager_prop_members(&host);
    let lazy = lazy_probe("Props").lazy_prop_members(&host);

    // Discriminator: the eager rail must actually have produced a
    // non-empty surface, else equivalence is vacuous.
    assert_eq!(
        eager.len(),
        3,
        "eager defineProps surface must have exactly 3 props (a, b, c); \
         got {:?}",
        eager.iter().map(|p| &p.name).collect::<Vec<_>>()
    );

    // Names + order must match exactly.
    let eager_names: Vec<&str> = eager.iter().map(|p| p.name.as_str()).collect();
    let lazy_names: Vec<&str> = lazy.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        eager_names, lazy_names,
        "lazy arm prop names/order MUST match the eager arm exactly",
    );

    // Per-field optionality + TypeExpr + `declared_in_macro_type_arg`
    // equivalence.
    //
    // `declared_in_macro_type_arg` (codex BINDING consolidation Stage
    // 1): the canonical typed-IR surface now CARRIES this bit. The lazy
    // arm resolves the imported declaration through the macro-aware
    // shared path (`surface_view_from_base_node` evaluating the
    // `DeclPlaceholder` under `SurfaceProvenanceContext::MacroTypeArgOwnBody`),
    // so the imported declaration's OWN-body members surface with
    // `declared_in_macro_type_arg = true` — field-for-field equivalent
    // to the eager OXC rail's `from_root_body` stamping. The earlier
    // "parser-side-only / lazy reports false" characterisation is
    // RETIRED: the dispatch is now the canonical producer of the bit.
    for (e, l) in eager.iter().zip(lazy.iter()) {
        assert_eq!(
            e.is_optional, l.is_optional,
            "optionality mismatch for prop `{}` (eager={}, lazy={}) — `b?` \
             must round-trip identically",
            e.name, e.is_optional, l.is_optional,
        );
        assert_eq!(
            e.type_expr, l.type_expr,
            "TypeExpr mismatch for prop `{}`: eager={:?} lazy={:?}",
            e.name, e.type_expr, l.type_expr,
        );
        assert_eq!(
            e.declared_in_macro_type_arg, l.declared_in_macro_type_arg,
            "declared_in_macro_type_arg mismatch for prop `{}` \
             (eager={}, lazy={}) — the canonical surface MUST carry the \
             own-body provenance field-for-field equivalent to the eager \
             rail (codex BINDING Stage 1)",
            e.name, e.declared_in_macro_type_arg, l.declared_in_macro_type_arg,
        );
    }

    // Positive own-body assertion: every member of `Props`'s own body
    // is author-declared in the macro T argument, so BOTH arms MUST
    // report `declared_in_macro_type_arg = true`. A lazy arm that lost
    // the provenance (the pre-Stage-1 `surface_view_from_base_node`
    // without the `MacroTypeArgOwnBody` context) reported `false` here
    // — this is the discrimination guard for the bit.
    assert!(
        lazy.iter().all(|p| p.declared_in_macro_type_arg),
        "every own-body prop of `Props` MUST carry \
         declared_in_macro_type_arg=true on the canonical surface; got {:?}",
        lazy.iter()
            .map(|p| (p.name.clone(), p.declared_in_macro_type_arg))
            .collect::<Vec<_>>(),
    );
    assert!(
        eager.iter().all(|p| p.declared_in_macro_type_arg),
        "eager arm own-body props must also report \
         declared_in_macro_type_arg=true (the equivalence baseline)",
    );

    // Negative: `b` is the ONLY optional prop. Rules out a lazy arm
    // that dropped optionality entirely (all-false) or inverted it.
    let lazy_optional: Vec<&str> = lazy
        .iter()
        .filter(|p| p.is_optional)
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(
        lazy_optional,
        vec!["b"],
        "exactly `b` is optional in the lazy reconstruction",
    );
}

// ---------------------------------------------------------------------------
// Test 2 (codex-flagged) — defineEmits call-signature event extraction
// ---------------------------------------------------------------------------

const EMITS_CALLSIG_TYPES_TS: &str = r#"
export interface Emits {
  (e: 'change', v: number): void;
  (e: 'submit'): void;
}
"#;

const EMITS_CALLSIG_OWNER_VUE: &str = r#"
<script setup lang="ts">
import type { Emits } from './types';
defineEmits<Emits>();
</script>
<template><div /></template>
"#;

/// **Codex BINDING case.** Call-signature emits carry the event name
/// in the first call-signature PARAMETER, never in the member-name
/// (`keyof`) set. Both arms MUST extract `['change', 'submit']` from
/// the call signatures.
///
/// A lazy arm that read event names from `keyof` would surface the
/// numeric tuple indices (or nothing) instead of the event names —
/// this test would fail.
#[test]
fn define_emits_call_signature_extraction_eager_lazy_equivalent() {
    let host = build_host(EMITS_CALLSIG_OWNER_VUE, EMITS_CALLSIG_TYPES_TS);

    let eager =
        eager_probe(&host, "Emits", AnalyzedMacroKind::DefineEmits).eager_emit_members(&host);
    let lazy = lazy_probe("Emits").lazy_emit_members(&host);

    let eager_names: Vec<&str> = eager.iter().map(|e| e.name.as_str()).collect();
    let lazy_names: Vec<&str> = lazy.iter().map(|e| e.name.as_str()).collect();

    // Positive: BOTH arms extract the event names from call signatures.
    assert_eq!(
        eager_names,
        vec!["change", "submit"],
        "eager arm must extract event names from call signatures",
    );
    assert_eq!(
        lazy_names, eager_names,
        "lazy arm MUST extract `['change', 'submit']` from the call \
         signatures, identical to the eager arm — NOT from `keyof` \
         (which would surface numeric indices or an empty set)",
    );

    // Negative: NO numeric-index "event" leaked in (the keyof-of-a-
    // call-signature-surface failure mode). Rules out a lazy arm that
    // enumerated member names instead of walking call signatures.
    assert!(
        !lazy_names
            .iter()
            .any(|n| n.chars().all(|c| c.is_ascii_digit())),
        "no numeric-index pseudo-events may leak into the lazy emit set: {lazy_names:?}",
    );

    // Per-event payload TypeExpr equivalence.
    for (e, l) in eager.iter().zip(lazy.iter()) {
        assert_eq!(
            e.payload_expr, l.payload_expr,
            "payload TypeExpr mismatch for event `{}`: eager={:?} lazy={:?}",
            e.name, e.payload_expr, l.payload_expr,
        );
    }
}

// ---------------------------------------------------------------------------
// Test 3 — defineEmits property-style equivalence
// ---------------------------------------------------------------------------

const EMITS_PROP_TYPES_TS: &str = r#"
export interface Emits {
  change: [value: number];
  submit: [];
}
"#;

const EMITS_PROP_OWNER_VUE: &str = r#"
<script setup lang="ts">
import type { Emits } from './types';
defineEmits<Emits>();
</script>
<template><div /></template>
"#;

/// Property-style emits (`{ change: [number]; submit: [] }`) surface
/// their event names as member names. Both arms must produce
/// `['change', 'submit']` with matching payloads.
#[test]
fn define_emits_property_style_eager_lazy_equivalent() {
    let host = build_host(EMITS_PROP_OWNER_VUE, EMITS_PROP_TYPES_TS);

    let eager =
        eager_probe(&host, "Emits", AnalyzedMacroKind::DefineEmits).eager_emit_members(&host);
    let lazy = lazy_probe("Emits").lazy_emit_members(&host);

    let eager_names: Vec<&str> = eager.iter().map(|e| e.name.as_str()).collect();
    let lazy_names: Vec<&str> = lazy.iter().map(|e| e.name.as_str()).collect();

    assert_eq!(
        eager_names,
        vec!["change", "submit"],
        "eager arm must surface property-style emit names",
    );
    assert_eq!(
        lazy_names, eager_names,
        "lazy arm property-style emit names MUST match the eager arm",
    );

    for (e, l) in eager.iter().zip(lazy.iter()) {
        assert_eq!(
            e.payload_expr, l.payload_expr,
            "payload TypeExpr mismatch for property-style event `{}`",
            e.name,
        );
    }
}

// ---------------------------------------------------------------------------
// Test 4 (codex-flagged) — defineSlots non-function filtering
// ---------------------------------------------------------------------------

const SLOTS_FILTER_TYPES_TS: &str = r#"
export interface Slots {
  default: (props: { item: string }) => any;
  notASlot: string;
}
"#;

const SLOTS_FILTER_OWNER_VUE: &str = r#"
<script setup lang="ts">
import type { Slots } from './types';
defineSlots<Slots>();
</script>
<template><div /></template>
"#;

/// **Codex BINDING case.** A `defineSlots` surface may carry
/// non-function members; only function-like members are slots. Both
/// arms MUST keep `default` (function-like) and FILTER `notASlot`
/// (a `string`, non-function).
///
/// A lazy arm that admitted every member name as a slot would
/// surface `notASlot` — this test would fail.
#[test]
fn define_slots_non_function_filtering_eager_lazy_equivalent() {
    let host = build_host(SLOTS_FILTER_OWNER_VUE, SLOTS_FILTER_TYPES_TS);

    let eager =
        eager_probe(&host, "Slots", AnalyzedMacroKind::DefineSlots).eager_slot_members(&host);
    let lazy = lazy_probe("Slots").lazy_slot_members(&host);

    let eager_names: Vec<&str> = eager.iter().map(|s| s.name.as_str()).collect();
    let lazy_names: Vec<&str> = lazy.iter().map(|s| s.name.as_str()).collect();

    // Positive: BOTH arms keep exactly `default`.
    assert_eq!(
        eager_names,
        vec!["default"],
        "eager arm must keep the function-like `default` slot and filter \
         the non-function `notASlot`",
    );
    assert_eq!(
        lazy_names, eager_names,
        "lazy arm MUST keep `default` (function-like) and FILTER \
         `notASlot` (non-function), identical to the eager arm",
    );

    // Negative: `notASlot` must be ABSENT from the lazy set. Rules out
    // a lazy arm that admitted every member name as a slot.
    assert!(
        !lazy_names.contains(&"notASlot"),
        "the non-function member `notASlot` must NOT appear as a slot: {lazy_names:?}",
    );
}

// ---------------------------------------------------------------------------
// Test 5 — slot bindings equivalence (parser-path precedence + shape)
// ---------------------------------------------------------------------------

const SLOTS_BIND_TYPES_TS: &str = r#"
export interface Slots {
  default: (props: { item: string; index: number }) => any;
}
"#;

const SLOTS_BIND_OWNER_VUE: &str = r#"
<script setup lang="ts">
import type { Slots } from './types';
defineSlots<Slots>();
</script>
<template><div /></template>
"#;

/// A `defineSlots` surface whose slot carries binding parameters
/// (`{ item: string; index: number }`) must reconstruct the same slot
/// binding set in both arms: same binding names, same per-binding
/// `binding_expr` TypeExpr.
#[test]
fn slot_bindings_graph_eager_lazy_equivalent() {
    let host = build_host(SLOTS_BIND_OWNER_VUE, SLOTS_BIND_TYPES_TS);

    let eager =
        eager_probe(&host, "Slots", AnalyzedMacroKind::DefineSlots).eager_slot_members(&host);
    let lazy = lazy_probe("Slots").lazy_slot_members(&host);

    assert_eq!(
        eager.len(),
        1,
        "eager arm must have exactly one slot (`default`)",
    );
    assert_eq!(
        lazy.len(),
        eager.len(),
        "lazy arm slot count MUST match the eager arm",
    );

    let eager_default = &eager[0];
    let lazy_default = &lazy[0];
    assert_eq!(eager_default.name, "default");
    assert_eq!(lazy_default.name, eager_default.name);

    // Binding names + order must match.
    let eager_bindings: Vec<&str> = eager_default
        .bindings
        .iter()
        .map(|b| b.name.as_str())
        .collect();
    let lazy_bindings: Vec<&str> = lazy_default
        .bindings
        .iter()
        .map(|b| b.name.as_str())
        .collect();
    assert_eq!(
        eager_bindings,
        vec!["item", "index"],
        "eager arm must extract both slot bindings in declaration order",
    );
    assert_eq!(
        lazy_bindings, eager_bindings,
        "lazy arm slot bindings MUST match the eager arm exactly",
    );

    // Per-binding TypeExpr equivalence.
    for (e, l) in eager_default
        .bindings
        .iter()
        .zip(lazy_default.bindings.iter())
    {
        assert_eq!(
            e.binding_expr, l.binding_expr,
            "binding_expr mismatch for slot binding `{}`: eager={:?} lazy={:?}",
            e.name, e.binding_expr, l.binding_expr,
        );
    }
}
