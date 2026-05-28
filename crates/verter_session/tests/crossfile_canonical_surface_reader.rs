//! Stage 1.5 — cross-file canonical surface reader equivalence.
//!
//! # What these characterize
//!
//! Stage 1 proved the eager/lazy macro-authority equivalence
//! (`stage2b1_macro_authority_equivalence.rs`) using SAME-FILE-only
//! fixtures: each imported type was declared wholly within the imported
//! file, with NO cross-file heritage. That left a blind spot — a
//! `interface Props extends ImportedBase` whose `ImportedBase` lives in
//! ANOTHER file lowers its heritage arm as a lazy `DeclRef` /
//! `InstantiationRef` carrier in Navigate/Skeleton mode, and the
//! pre-Stage-1.5 `surface_view_from_base_node` dropped those carrier arms
//! at its catch-all `None`. The inherited members vanished from the lazy
//! reconstruction while own-body members still surfaced.
//!
//! These tests drive the CANONICAL-DIRECT path (the
//! `ImportedMacroSurfaceProbe::lazy_*_members` reader, identical to the
//! Stage-2 producer path) against cross-file shapes, asserting:
//!
//! - inherited members are PRESENT in the lazy reconstruction (class A);
//! - barrel-routed and `Omit`-on-imported heritage resolve (class B);
//! - display metadata (`type_annotation` / `payload_type` /
//!   `return_type` / JSDoc `description` + `tags`) is reattached for
//!   INHERITED members, field-for-field equivalent to the eager rail
//!   (class C).
//!
//! The eager arm (driven by the REAL OXC resolver) is the equivalence
//! oracle: it is the production-authoritative rail Stage 2's flip must
//! reproduce. Both arms flow through the shared `prop_members` /
//! `emit_members` / `slot_members` interpretation, so each assertion is
//! arm-to-arm, not arm-to-hand-built.
//!
//! # Why these discriminate
//!
//! Each cross-file fixture FAILS against the pre-Stage-1.5 tree (the
//! carrier-incomplete reader drops the inherited members / the display
//! sidecar does not exist) and PASSES post-fix. The discrimination proof
//! is recorded in the Stage 1.5 commit (temp-revert of the `DeclRef` /
//! `InstantiationRef` arms makes the inherited-member assertions fail;
//! temp-revert of the display sidecar makes the metadata assertions
//! fail).

#![allow(clippy::too_many_lines)]

use std::sync::Arc;

use verter_semantic::analysis::AnalyzedMacroKind;
use verter_session::test_only::imported_macro_surface::{
    EagerMacroSurfaceProbe, ImportedMacroSurfaceProbe,
};
use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

// ---------------------------------------------------------------------------
// Hermetic multi-file host harness
// ---------------------------------------------------------------------------

const OWNER_VUE_PATH: &str = "/w/owner.vue";

/// Build a host from a set of `(path, source, kind)` files. The first
/// file is treated as the owner SFC for the eager probe's import-route
/// entry point. Every file is injected into the workspace (so the
/// resolver can read it) AND upserted (so it is parsed + shallow-indexed).
fn build_host(files: &[(&'static str, &'static str, FileKind)]) -> Arc<VerterHost> {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    for (path, source, _kind) in files {
        workspace.inject_file((*path).into(), Arc::from(*source));
    }
    let ws: Arc<dyn WorkspaceAccess> = workspace;
    let host = Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            ..HostConfig::default()
        },
        ws,
    ));
    for (path, source, kind) in files {
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some((*path).into()),
            input_id: (*path).into(),
            source: Arc::from(*source),
            file_kind: *kind,
            aliases: vec![],
        });
    }
    host
}

/// Resolve the eager `ResolvedMacroMeta` for the imported declaration
/// `exported_name` reached from `OWNER_VUE_PATH` via `import_source`.
/// Panics with a discriminating message when the eager OXC rail cannot
/// reach the declaration (a vacuous equivalence comparison).
fn eager(
    host: &VerterHost,
    import_source: &str,
    exported_name: &str,
    kind: AnalyzedMacroKind,
) -> EagerMacroSurfaceProbe {
    EagerMacroSurfaceProbe::resolve(host, OWNER_VUE_PATH, import_source, exported_name, kind)
        .expect(
            "the eager OXC resolver MUST reach the imported declaration — a None \
             here means the fixture's import route is broken and the equivalence \
             comparison would be vacuous",
        )
}

/// Build a lazy bridge probe targeting `exported_name` at `canonical`.
fn lazy(canonical: &'static str, exported_name: &str) -> ImportedMacroSurfaceProbe {
    ImportedMacroSurfaceProbe::new(Arc::from(canonical), Arc::from(exported_name), [0u8; 16])
}

fn vue_importing(import_source: &str, macro_call: &str, type_name: &str) -> String {
    format!(
        "\n<script setup lang=\"ts\">\nimport type {{ {type_name} }} from '{import_source}';\n{macro_call}<{type_name}>();\n</script>\n<template><div /></template>\n"
    )
}

// ===========================================================================
// Class A — cross-file `interface Props extends BaseProps`
// ===========================================================================

#[test]
fn interface_extends_imported_base_inherited_props_present() {
    // owner imports Props (props.ts); Props extends BaseProps (base.ts).
    let owner: &'static str =
        Box::leak(vue_importing("./props", "defineProps", "Props").into_boxed_str());
    let props = "import type { BaseProps } from './base'\n\
                 export interface Props extends BaseProps {\n  /** own field */\n  own: string\n}";
    let base = "export interface BaseProps {\n  /** base field */\n  base: number\n}";
    let host = build_host(&[
        (OWNER_VUE_PATH, owner, FileKind::VueSfc),
        ("/w/props.ts", props, FileKind::NonSfc),
        ("/w/base.ts", base, FileKind::NonSfc),
    ]);

    let eager_members =
        eager(&host, "./props", "Props", AnalyzedMacroKind::DefineProps).eager_prop_members(&host);
    let lazy_members = lazy("/w/props.ts", "Props").lazy_prop_members(&host);

    // The inherited `base` member MUST be present in BOTH arms. Pre-fix
    // the lazy reader dropped the cross-file heritage `DeclRef` arm.
    let eager_names: std::collections::BTreeSet<&str> =
        eager_members.iter().map(|p| p.name.as_str()).collect();
    let lazy_names: std::collections::BTreeSet<&str> =
        lazy_members.iter().map(|p| p.name.as_str()).collect();
    assert!(
        eager_names.contains("base") && eager_names.contains("own"),
        "eager arm must surface inherited `base` + own `own`; got {eager_names:?}",
    );
    assert_eq!(
        lazy_names, eager_names,
        "lazy arm prop name SET MUST match the eager arm — the inherited \
         cross-file `base` member must be present (carrier-complete reader)",
    );

    // Inherited members carry declared_in_macro_type_arg = false; own
    // members carry true.
    let own = lazy_members.iter().find(|p| p.name == "own").expect("own");
    let base_field = lazy_members
        .iter()
        .find(|p| p.name == "base")
        .expect("inherited base member must be present");
    assert!(
        own.declared_in_macro_type_arg,
        "own-body member `own` MUST carry declared_in_macro_type_arg=true",
    );
    assert!(
        !base_field.declared_in_macro_type_arg,
        "inherited member `base` MUST carry declared_in_macro_type_arg=false \
         (reached via cross-file heritage, not the macro-T own body)",
    );

    // Class C — display metadata for the INHERITED member.
    let eager_base = eager_members
        .iter()
        .find(|p| p.name == "base")
        .expect("eager base");
    assert_eq!(
        base_field.type_annotation, eager_base.type_annotation,
        "inherited member `base` type_annotation MUST match the eager rail \
         (display sidecar reattaches raw type text by member origin)",
    );
    assert_eq!(
        base_field.type_annotation.as_deref(),
        Some("number"),
        "inherited member `base` type_annotation MUST be the rendered display \
         text `number` (cache-owned typed → display projection)",
    );
    assert_eq!(
        base_field.type_expr, eager_base.type_expr,
        "inherited member `base` TypeExpr MUST match the eager rail",
    );

    // JSDoc reattachment by member ORIGIN (class C). The inherited `base`
    // member's JSDoc lives in the heritage BASE's file (`/w/base.ts`), not
    // the root `Props` declaration's file. The display sidecar resolves it
    // from the member value node's origin scope — proving metadata follows
    // the member origin, not the root declaration name.
    //
    // NOTE: the EAGER probe path resolves elements with `source: None`, so
    // its JSDoc is always empty — the lazy rail is STRICTLY MORE COMPLETE
    // here. We assert the JSDoc was correctly reattached on the lazy side
    // (NOT lazy==eager), per the project rule that neither rail is a
    // ground-truth oracle. The `get_component_meta` cross-file fixtures
    // below exercise the eager production rail WITH source.
    assert_eq!(
        base_field.description.as_deref(),
        Some("base field"),
        "inherited member `base` JSDoc description MUST be reattached from the \
         heritage base's file (`/w/base.ts`) by member origin; got {:?}",
        base_field.description,
    );
    let own = lazy_members.iter().find(|p| p.name == "own").expect("own");
    assert_eq!(
        own.description.as_deref(),
        Some("own field"),
        "own-body member `own` JSDoc description MUST be reattached from the \
         root declaration's file (`/w/props.ts`)",
    );
}

#[test]
fn class_extends_imported_base_inherited_props_present() {
    let owner: &'static str =
        Box::leak(vue_importing("./props", "defineProps", "Props").into_boxed_str());
    // class-form Props extends an imported base class.
    let props = "import { BaseProps } from './base'\n\
                 export class Props extends BaseProps {\n  own!: string\n}";
    let base = "export class BaseProps {\n  base!: number\n}";
    let host = build_host(&[
        (OWNER_VUE_PATH, owner, FileKind::VueSfc),
        ("/w/props.ts", props, FileKind::NonSfc),
        ("/w/base.ts", base, FileKind::NonSfc),
    ]);

    let eager_members =
        eager(&host, "./props", "Props", AnalyzedMacroKind::DefineProps).eager_prop_members(&host);
    let lazy_members = lazy("/w/props.ts", "Props").lazy_prop_members(&host);
    let eager_names: std::collections::BTreeSet<&str> =
        eager_members.iter().map(|p| p.name.as_str()).collect();
    let lazy_names: std::collections::BTreeSet<&str> =
        lazy_members.iter().map(|p| p.name.as_str()).collect();
    assert!(
        eager_names.contains("base") && eager_names.contains("own"),
        "eager arm must surface inherited class member `base` + own `own`; got {eager_names:?}",
    );
    assert_eq!(
        lazy_names, eager_names,
        "lazy arm class-heritage prop set MUST match the eager arm (inherited \
         `base` present via cross-file class heritage)",
    );
}

#[test]
fn interface_extends_imported_base_via_named_barrel() {
    // owner imports Props; Props extends BaseProps re-exported through a
    // NAMED barrel (`export { BaseProps } from './base'`).
    let owner: &'static str =
        Box::leak(vue_importing("./props", "defineProps", "Props").into_boxed_str());
    let props = "import type { BaseProps } from './barrel'\n\
                 export interface Props extends BaseProps {\n  own: string\n}";
    let barrel = "export { BaseProps } from './base'";
    let base = "export interface BaseProps {\n  base: number\n}";
    let host = build_host(&[
        (OWNER_VUE_PATH, owner, FileKind::VueSfc),
        ("/w/props.ts", props, FileKind::NonSfc),
        ("/w/barrel.ts", barrel, FileKind::NonSfc),
        ("/w/base.ts", base, FileKind::NonSfc),
    ]);

    let eager_members =
        eager(&host, "./props", "Props", AnalyzedMacroKind::DefineProps).eager_prop_members(&host);
    let lazy_members = lazy("/w/props.ts", "Props").lazy_prop_members(&host);
    let eager_names: std::collections::BTreeSet<&str> =
        eager_members.iter().map(|p| p.name.as_str()).collect();
    let lazy_names: std::collections::BTreeSet<&str> =
        lazy_members.iter().map(|p| p.name.as_str()).collect();
    assert!(
        eager_names.contains("base"),
        "eager arm must surface inherited `base` through the named barrel; got {eager_names:?}",
    );
    assert_eq!(
        lazy_names, eager_names,
        "lazy arm MUST surface inherited `base` reached through a NAMED barrel re-export",
    );
}

#[test]
fn interface_extends_imported_base_via_wildcard_barrel() {
    // Heritage reached through a WILDCARD (`export *`) barrel.
    let owner: &'static str =
        Box::leak(vue_importing("./props", "defineProps", "Props").into_boxed_str());
    let props = "import type { BaseProps } from './barrel'\n\
                 export interface Props extends BaseProps {\n  own: string\n}";
    let barrel = "export * from './base'";
    let base = "export interface BaseProps {\n  base: number\n}";
    let host = build_host(&[
        (OWNER_VUE_PATH, owner, FileKind::VueSfc),
        ("/w/props.ts", props, FileKind::NonSfc),
        ("/w/barrel.ts", barrel, FileKind::NonSfc),
        ("/w/base.ts", base, FileKind::NonSfc),
    ]);

    let eager_members =
        eager(&host, "./props", "Props", AnalyzedMacroKind::DefineProps).eager_prop_members(&host);
    let lazy_members = lazy("/w/props.ts", "Props").lazy_prop_members(&host);
    let eager_names: std::collections::BTreeSet<&str> =
        eager_members.iter().map(|p| p.name.as_str()).collect();
    let lazy_names: std::collections::BTreeSet<&str> =
        lazy_members.iter().map(|p| p.name.as_str()).collect();
    assert!(
        eager_names.contains("base"),
        "eager arm must surface inherited `base` through the wildcard barrel; got {eager_names:?}",
    );
    assert_eq!(
        lazy_names, eager_names,
        "lazy arm MUST surface inherited `base` reached through a WILDCARD `export *` barrel",
    );
}

// ===========================================================================
// Class B — `Omit<ImportedBase, 'hidden'>` heritage
// ===========================================================================

#[test]
fn interface_extends_omit_imported_base_hidden_absent() {
    let owner: &'static str =
        Box::leak(vue_importing("./props", "defineProps", "Props").into_boxed_str());
    let props = "import type { ImportedBase } from './base'\n\
                 export interface Props extends Omit<ImportedBase, 'hidden'> {\n  own: string\n}";
    let base = "export interface ImportedBase {\n  kept: number\n  hidden: string\n}";
    let host = build_host(&[
        (OWNER_VUE_PATH, owner, FileKind::VueSfc),
        ("/w/props.ts", props, FileKind::NonSfc),
        ("/w/base.ts", base, FileKind::NonSfc),
    ]);

    let eager_members =
        eager(&host, "./props", "Props", AnalyzedMacroKind::DefineProps).eager_prop_members(&host);
    let lazy_members = lazy("/w/props.ts", "Props").lazy_prop_members(&host);
    let eager_names: std::collections::BTreeSet<&str> =
        eager_members.iter().map(|p| p.name.as_str()).collect();
    let lazy_names: std::collections::BTreeSet<&str> =
        lazy_members.iter().map(|p| p.name.as_str()).collect();
    assert!(
        eager_names.contains("kept") && eager_names.contains("own"),
        "eager arm must surface `kept` (Omit-kept) + own `own`; got {eager_names:?}",
    );
    assert!(
        !eager_names.contains("hidden"),
        "eager arm must NOT surface the omitted `hidden`; got {eager_names:?}",
    );
    assert_eq!(
        lazy_names, eager_names,
        "lazy arm Omit-on-imported MUST keep `kept`, drop `hidden`, identical to eager",
    );
    assert!(
        !lazy_names.contains("hidden"),
        "the omitted `hidden` member must be ABSENT from the lazy reconstruction: {lazy_names:?}",
    );
}

// ===========================================================================
// Class A — imported defineEmits forms
// ===========================================================================

#[test]
fn imported_emits_property_form_omitted_event_absent() {
    // Emits = Omit<BaseEmits, 'drop'> through cross-file heritage.
    let owner: &'static str =
        Box::leak(vue_importing("./emits", "defineEmits", "Emits").into_boxed_str());
    let emits = "import type { BaseEmits } from './base'\n\
                 export interface Emits extends Omit<BaseEmits, 'drop'> {\n  keep: [value: number]\n}";
    let base = "export interface BaseEmits {\n  drop: [x: string]\n  inherited: [y: boolean]\n}";
    let host = build_host(&[
        (OWNER_VUE_PATH, owner, FileKind::VueSfc),
        ("/w/emits.ts", emits, FileKind::NonSfc),
        ("/w/base.ts", base, FileKind::NonSfc),
    ]);

    let eager_members =
        eager(&host, "./emits", "Emits", AnalyzedMacroKind::DefineEmits).eager_emit_members(&host);
    let lazy_members = lazy("/w/emits.ts", "Emits").lazy_emit_members(&host);
    let eager_names: std::collections::BTreeSet<&str> =
        eager_members.iter().map(|e| e.name.as_str()).collect();
    let lazy_names: std::collections::BTreeSet<&str> =
        lazy_members.iter().map(|e| e.name.as_str()).collect();
    assert!(
        eager_names.contains("keep") && eager_names.contains("inherited"),
        "eager arm must surface own `keep` + inherited `inherited`; got {eager_names:?}",
    );
    assert!(
        !eager_names.contains("drop"),
        "eager arm must NOT surface the omitted event `drop`; got {eager_names:?}",
    );
    assert_eq!(
        lazy_names, eager_names,
        "lazy emit set MUST match the eager arm — inherited `inherited` present, omitted `drop` absent",
    );
}

#[test]
fn imported_call_signature_emits_through_extends() {
    // Emits extends an imported call-signature emits interface.
    let owner: &'static str =
        Box::leak(vue_importing("./emits", "defineEmits", "Emits").into_boxed_str());
    let emits = "import type { BaseEmits } from './base'\n\
                 export interface Emits extends BaseEmits {\n  (e: 'own', v: number): void\n}";
    let base = "export interface BaseEmits {\n  (e: 'inherited', v: string): void\n}";
    let host = build_host(&[
        (OWNER_VUE_PATH, owner, FileKind::VueSfc),
        ("/w/emits.ts", emits, FileKind::NonSfc),
        ("/w/base.ts", base, FileKind::NonSfc),
    ]);

    let eager_members =
        eager(&host, "./emits", "Emits", AnalyzedMacroKind::DefineEmits).eager_emit_members(&host);
    let lazy_members = lazy("/w/emits.ts", "Emits").lazy_emit_members(&host);
    let eager_names: std::collections::BTreeSet<&str> =
        eager_members.iter().map(|e| e.name.as_str()).collect();
    let lazy_names: std::collections::BTreeSet<&str> =
        lazy_members.iter().map(|e| e.name.as_str()).collect();
    assert!(
        eager_names.contains("inherited"),
        "eager arm must extract the inherited call-signature event `inherited`; got {eager_names:?}",
    );
    assert_eq!(
        lazy_names, eager_names,
        "lazy arm MUST extract call-signature event names through cross-file `extends` (inherited present)",
    );
    assert!(
        !lazy_names
            .iter()
            .any(|n| n.chars().all(|c| c.is_ascii_digit())),
        "no numeric-index pseudo-events may leak into the lazy emit set: {lazy_names:?}",
    );
}

// ===========================================================================
// Class A — generic cross-file heritage with substitution
// ===========================================================================

#[test]
fn interface_extends_imported_generic_base_with_substitution() {
    // Props extends Base<string> where Base<T> { value: T } lives cross-file.
    let owner: &'static str =
        Box::leak(vue_importing("./props", "defineProps", "Props").into_boxed_str());
    let props = "import type { Base } from './base'\n\
                 export interface Props extends Base<string> {\n  own: number\n}";
    let base = "export interface Base<T> {\n  value: T\n}";
    let host = build_host(&[
        (OWNER_VUE_PATH, owner, FileKind::VueSfc),
        ("/w/props.ts", props, FileKind::NonSfc),
        ("/w/base.ts", base, FileKind::NonSfc),
    ]);

    let eager_members =
        eager(&host, "./props", "Props", AnalyzedMacroKind::DefineProps).eager_prop_members(&host);
    let lazy_members = lazy("/w/props.ts", "Props").lazy_prop_members(&host);
    let eager_names: std::collections::BTreeSet<&str> =
        eager_members.iter().map(|p| p.name.as_str()).collect();
    let lazy_names: std::collections::BTreeSet<&str> =
        lazy_members.iter().map(|p| p.name.as_str()).collect();
    assert!(
        eager_names.contains("value") && eager_names.contains("own"),
        "eager arm must surface substituted generic member `value` + own `own`; got {eager_names:?}",
    );
    assert_eq!(
        lazy_names, eager_names,
        "lazy arm MUST surface the substituted generic heritage member `value`",
    );

    // The substituted member's type must be the INSTANTIATED `string`
    // (generic substitution is semantic meaning: `Base<string>` → the
    // heritage member `value: T` becomes `value: string`). The carrier-
    // complete reader instantiates the `InstantiationRef{Base, [string]}`
    // heritage arm in Skeleton mode, which substitutes `T → string`.
    //
    // NOTE (eager-rail divergence): the EAGER OXC rail leaves this member
    // as the unsubstituted `Ref { name: "T" }` for cross-file generic
    // heritage — the lazy reader is STRICTLY MORE CORRECT here. We assert
    // TS-correct behavior on the lazy side (NOT lazy==eager), per the
    // project rule that neither rail is a ground-truth oracle. The Stage-2
    // flip's equivalence harness should special-case / accept this as a
    // lazy-side improvement when it lands.
    let lazy_value = lazy_members
        .iter()
        .find(|p| p.name == "value")
        .expect("lazy value member must be present");
    assert_eq!(
        lazy_value.type_expr,
        Some(verter_type_expr::TypeExpr::Primitive(
            verter_type_expr::PrimitiveName::String
        )),
        "substituted generic member `value` MUST be the instantiated `string` \
         (Base<string> → value: string); the carrier reader substitutes \
         `T → string` through the Skeleton InstantiationRef unwrap",
    );
    // Discriminate against the unsubstituted carrier leak: `value` must NOT
    // remain a bare `Ref { name: \"T\" }`.
    assert_ne!(
        lazy_value.type_expr,
        Some(verter_type_expr::TypeExpr::named("T")),
        "the substituted member `value` must NOT leak the unsubstituted type \
         parameter `Ref {{ name: \"T\" }}`",
    );
}

// ===========================================================================
// Class C — inherited-member `*_expr_scope` follows the member ORIGIN file
// ===========================================================================

#[test]
fn inherited_member_type_expr_scope_is_origin_file_not_root() {
    // owner imports Props (props.ts); Props extends BaseProps (base.ts);
    // BaseProps's `base` member is typed as a `LocalAlias` declared IN the
    // base file. The inherited member raises to `Ref("LocalAlias")`, and its
    // paired `type_expr_scope` MUST be the BASE file (`/w/base.ts`) — the file
    // where `LocalAlias` is declared and where later typed-IR resolution must
    // look. Pre-fix the scope was stamped with the ROOT declaration's file
    // (`/w/props.ts`), where `LocalAlias` does NOT exist → a wrong-file lookup
    // (Miss / cross-file mis-binding). This fixture pins the scope to the
    // member origin.
    let owner: &'static str =
        Box::leak(vue_importing("./props", "defineProps", "Props").into_boxed_str());
    let props = "import type { BaseProps } from './base'\n\
                 export interface Props extends BaseProps {\n  own: string\n}";
    // The base file declares a LOCAL alias and types `base` as that alias.
    let base = "type LocalAlias = { nested: number }\n\
                export interface BaseProps {\n  base: LocalAlias\n}";
    let host = build_host(&[
        (OWNER_VUE_PATH, owner, FileKind::VueSfc),
        ("/w/props.ts", props, FileKind::NonSfc),
        ("/w/base.ts", base, FileKind::NonSfc),
    ]);

    let lazy_members = lazy("/w/props.ts", "Props").lazy_prop_members(&host);
    let base_field = lazy_members
        .iter()
        .find(|p| p.name == "base")
        .expect("inherited `base` member must be present");
    let own = lazy_members.iter().find(|p| p.name == "own").expect("own");

    // The inherited member's type-expr is the shallow `Ref("LocalAlias")`
    // (shallow-by-default): the alias body is NOT inlined at the surface.
    assert_eq!(
        base_field.type_expr,
        Some(verter_type_expr::TypeExpr::named("LocalAlias")),
        "inherited `base` must raise to the shallow alias `Ref` (the alias body \
         stays un-inlined); got {:?}",
        base_field.type_expr,
    );

    // THE DISCRIMINATOR: the paired `type_expr_scope` is the member ORIGIN
    // file (`/w/base.ts`), NOT the root declaration's file (`/w/props.ts`).
    // Pre-fix this was `/w/props.ts` and `LocalAlias` would resolve in the
    // WRONG file.
    assert_eq!(
        base_field.type_expr_scope.as_ref().map(|s| s.as_str()),
        Some("/w/base.ts"),
        "inherited `base` member's type_expr_scope MUST be the heritage base's \
         file (`/w/base.ts`) where `LocalAlias` is declared — NOT the root \
         declaration's file; got {:?}",
        base_field.type_expr_scope,
    );
    // The OWN member stays scoped to the root declaration's file.
    assert_eq!(
        own.type_expr_scope.as_ref().map(|s| s.as_str()),
        Some("/w/props.ts"),
        "own-body `own` member's type_expr_scope MUST be the root declaration's \
         file (`/w/props.ts`); got {:?}",
        own.type_expr_scope,
    );

    // Proof the chosen scope is the RESOLVABLE one: `LocalAlias` is declared
    // ONLY in `/w/base.ts`. A `ResolveDecl` for `LocalAlias` in the member's
    // scope file (`/w/base.ts`) RESOLVES; the same `ResolveDecl` in the ROOT
    // declaration's file (`/w/props.ts`) MISSES. So stamping the inherited
    // member with the root scope (the pre-fix bug) would route alias
    // resolution into the wrong file → Miss; the origin scope routes it to
    // the file where the alias actually lives.
    let scope_file = base_field
        .type_expr_scope
        .as_ref()
        .expect("inherited base has a scope")
        .as_str();
    let resolves_in_scope_file = matches!(
        lazy(
            Box::leak(scope_file.to_string().into_boxed_str()),
            "LocalAlias"
        )
        .resolve_root(&host),
        verter_session::semantic_query::QueryResult::Value(_)
    );
    let resolves_in_root_file = matches!(
        lazy("/w/props.ts", "LocalAlias").resolve_root(&host),
        verter_session::semantic_query::QueryResult::Value(_)
    );
    assert!(
        resolves_in_scope_file,
        "`LocalAlias` MUST resolve in the inherited member's scope file ({scope_file}) — \
         the scope points at the file where the alias is declared",
    );
    assert!(
        !resolves_in_root_file,
        "`LocalAlias` MUST NOT resolve in the ROOT declaration's file (/w/props.ts) — \
         this is exactly why the pre-fix root-file scope produced a wrong-file lookup",
    );
}

// ===========================================================================
// Class C — inherited-member JSDoc keys on DECLARATION provenance
// ===========================================================================

#[test]
fn inherited_member_jsdoc_keys_on_declaring_declaration_not_file_first_match() {
    // The base file declares the SAME property name (`base`) in TWO
    // declarations with DIFFERENT JSDoc and DIFFERENT types. Only `BaseProps`
    // is the heritage base of `Props`; `Decoy` is an unrelated declaration
    // that appears FIRST in the file. A file-wide first-match JSDoc lookup
    // would attach `Decoy`'s JSDoc (the first textual `base:`); the
    // declaration-provenance lookup MUST attach `BaseProps`'s JSDoc.
    let owner: &'static str =
        Box::leak(vue_importing("./props", "defineProps", "Props").into_boxed_str());
    let props = "import type { BaseProps } from './base'\n\
                 export interface Props extends BaseProps {\n  own: string\n}";
    // `Decoy` is declared FIRST (its `base:` is the first textual match) and
    // carries a DECOY JSDoc + a different type (`string`) so the inherited
    // member's value node (`number`) disambiguates structurally.
    let base = "export interface Decoy {\n  /** DECOY base doc */\n  base: string\n}\n\
                export interface BaseProps {\n  /** correct base doc */\n  base: number\n}";
    let host = build_host(&[
        (OWNER_VUE_PATH, owner, FileKind::VueSfc),
        ("/w/props.ts", props, FileKind::NonSfc),
        ("/w/base.ts", base, FileKind::NonSfc),
    ]);

    let lazy_members = lazy("/w/props.ts", "Props").lazy_prop_members(&host);
    let base_field = lazy_members
        .iter()
        .find(|p| p.name == "base")
        .expect("inherited `base` member must be present");

    // Sanity: the inherited member is BaseProps's `base: number`, not Decoy's
    // `base: string` — the carrier-complete reader followed the heritage edge.
    assert_eq!(
        base_field.type_expr,
        Some(verter_type_expr::TypeExpr::Primitive(
            verter_type_expr::PrimitiveName::Number
        )),
        "inherited `base` must be BaseProps's `number`, not Decoy's `string`; got {:?}",
        base_field.type_expr,
    );

    // THE DISCRIMINATOR: the JSDoc is BaseProps's (the DECLARING declaration),
    // NOT Decoy's first-textual-match JSDoc. Pre-fix the file-wide search
    // attached `DECOY base doc` (the first `base:` site in the file).
    assert_eq!(
        base_field.description.as_deref(),
        Some("correct base doc"),
        "inherited `base` JSDoc MUST come from the DECLARING declaration \
         (`BaseProps`), NOT the file-first `Decoy.base`; got {:?}",
        base_field.description,
    );
    assert_ne!(
        base_field.description.as_deref(),
        Some("DECOY base doc"),
        "inherited `base` JSDoc must NOT be the decoy declaration's doc \
         (file-wide first-match bug)",
    );
}

#[test]
fn inherited_method_style_member_jsdoc_is_reattached() {
    // A method-style member (`default(props): any`) carries leading JSDoc.
    // The pre-fix name-search matched only `name:` / `name?:` and MISSED
    // method-style members entirely (their declaration site is `name(`), so
    // their JSDoc was dropped. This fixture pins method-style JSDoc
    // reattachment for an inherited slot member.
    let owner: &'static str =
        Box::leak(vue_importing("./slots", "defineSlots", "Slots").into_boxed_str());
    let slots = "import type { BaseSlots } from './base'\n\
                 export interface Slots extends BaseSlots {}";
    let base = "export interface BaseSlots {\n  \
                /** the default slot */\n  default(props: { item: string }): any\n}";
    let host = build_host(&[
        (OWNER_VUE_PATH, owner, FileKind::VueSfc),
        ("/w/slots.ts", slots, FileKind::NonSfc),
        ("/w/base.ts", base, FileKind::NonSfc),
    ]);

    let lazy_members = lazy("/w/slots.ts", "Slots").lazy_slot_members(&host);
    let default_slot = lazy_members
        .iter()
        .find(|s| s.name == "default")
        .expect("inherited method-style slot `default` must be present");

    // THE DISCRIMINATOR: the method-style member's JSDoc is reattached.
    // Pre-fix the `name:`-only matcher never matched `default(` so the
    // description was None.
    assert_eq!(
        default_slot.description.as_deref(),
        Some("the default slot"),
        "method-style inherited slot `default(props): any` MUST get its leading \
         JSDoc reattached (the matcher accepts `name(`); got {:?}",
        default_slot.description,
    );
}

// ===========================================================================
// Class C — imported defineSlots binding display metadata
// ===========================================================================

#[test]
fn imported_slots_binding_display_metadata_preserved() {
    // defineSlots<Slots> imported; Slots has a slot with a binding.
    let owner: &'static str =
        Box::leak(vue_importing("./slots", "defineSlots", "Slots").into_boxed_str());
    let slots =
        "export interface Slots {\n  default(props: { item: string; index: number }): any\n}";
    let host = build_host(&[
        (OWNER_VUE_PATH, owner, FileKind::VueSfc),
        ("/w/slots.ts", slots, FileKind::NonSfc),
    ]);

    let eager_members =
        eager(&host, "./slots", "Slots", AnalyzedMacroKind::DefineSlots).eager_slot_members(&host);
    let lazy_members = lazy("/w/slots.ts", "Slots").lazy_slot_members(&host);
    assert_eq!(eager_members.len(), 1, "eager: one slot (`default`)");
    assert_eq!(
        lazy_members.len(),
        eager_members.len(),
        "lazy slot count MUST match eager",
    );
    let eager_default = &eager_members[0];
    let lazy_default = &lazy_members[0];

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
        lazy_bindings, eager_bindings,
        "lazy slot bindings MUST match eager",
    );

    // Class C — binding display metadata (type_annotation) must match the
    // eager rail member-for-member.
    for (e, l) in eager_default
        .bindings
        .iter()
        .zip(lazy_default.bindings.iter())
    {
        assert_eq!(
            l.type_annotation, e.type_annotation,
            "binding `{}` type_annotation MUST match the eager rail (display sidecar)",
            e.name,
        );
        assert_eq!(
            l.binding_expr, e.binding_expr,
            "binding `{}` binding_expr MUST match the eager rail",
            e.name,
        );
    }
}

// ===========================================================================
// Edge cases (no-deferral mandate)
// ===========================================================================

#[test]
fn alias_chain_to_imported_heritage() {
    // Props extends Alias; Alias = BaseProps (alias chain to imported heritage).
    let owner: &'static str =
        Box::leak(vue_importing("./props", "defineProps", "Props").into_boxed_str());
    let props = "import type { Alias } from './alias'\n\
                 export interface Props extends Alias {\n  own: string\n}";
    let alias = "import type { BaseProps } from './base'\nexport type Alias = BaseProps";
    let base = "export interface BaseProps {\n  base: number\n}";
    let host = build_host(&[
        (OWNER_VUE_PATH, owner, FileKind::VueSfc),
        ("/w/props.ts", props, FileKind::NonSfc),
        ("/w/alias.ts", alias, FileKind::NonSfc),
        ("/w/base.ts", base, FileKind::NonSfc),
    ]);

    let eager_members =
        eager(&host, "./props", "Props", AnalyzedMacroKind::DefineProps).eager_prop_members(&host);
    let lazy_members = lazy("/w/props.ts", "Props").lazy_prop_members(&host);
    let eager_names: std::collections::BTreeSet<&str> =
        eager_members.iter().map(|p| p.name.as_str()).collect();
    let lazy_names: std::collections::BTreeSet<&str> =
        lazy_members.iter().map(|p| p.name.as_str()).collect();
    assert!(
        eager_names.contains("base"),
        "eager arm must surface `base` through the alias chain; got {eager_names:?}",
    );
    assert_eq!(
        lazy_names, eager_names,
        "lazy arm MUST resolve the alias chain to imported heritage (`base` present)",
    );
}

#[test]
fn two_level_deep_cross_file_heritage() {
    // Props extends Mid (mid.ts); Mid extends BaseProps (base.ts).
    let owner: &'static str =
        Box::leak(vue_importing("./props", "defineProps", "Props").into_boxed_str());
    let props = "import type { Mid } from './mid'\n\
                 export interface Props extends Mid {\n  own: string\n}";
    let mid = "import type { BaseProps } from './base'\n\
               export interface Mid extends BaseProps {\n  mid: boolean\n}";
    let base = "export interface BaseProps {\n  base: number\n}";
    let host = build_host(&[
        (OWNER_VUE_PATH, owner, FileKind::VueSfc),
        ("/w/props.ts", props, FileKind::NonSfc),
        ("/w/mid.ts", mid, FileKind::NonSfc),
        ("/w/base.ts", base, FileKind::NonSfc),
    ]);

    let eager_members =
        eager(&host, "./props", "Props", AnalyzedMacroKind::DefineProps).eager_prop_members(&host);
    let lazy_members = lazy("/w/props.ts", "Props").lazy_prop_members(&host);
    let eager_names: std::collections::BTreeSet<&str> =
        eager_members.iter().map(|p| p.name.as_str()).collect();
    let lazy_names: std::collections::BTreeSet<&str> =
        lazy_members.iter().map(|p| p.name.as_str()).collect();
    assert!(
        eager_names.contains("base") && eager_names.contains("mid") && eager_names.contains("own"),
        "eager arm must surface 2-level heritage (`base`, `mid`, `own`); got {eager_names:?}",
    );
    assert_eq!(
        lazy_names, eager_names,
        "lazy arm MUST surface 2-level-deep cross-file heritage members",
    );
}

#[test]
fn intersection_of_two_imported_refs() {
    // Props = A & B, both imported from distinct files.
    let owner: &'static str =
        Box::leak(vue_importing("./props", "defineProps", "Props").into_boxed_str());
    let props = "import type { A } from './a'\nimport type { B } from './b'\n\
                 export type Props = A & B";
    let a = "export interface A {\n  a: number\n}";
    let b = "export interface B {\n  b: string\n}";
    let host = build_host(&[
        (OWNER_VUE_PATH, owner, FileKind::VueSfc),
        ("/w/props.ts", props, FileKind::NonSfc),
        ("/w/a.ts", a, FileKind::NonSfc),
        ("/w/b.ts", b, FileKind::NonSfc),
    ]);

    let eager_members =
        eager(&host, "./props", "Props", AnalyzedMacroKind::DefineProps).eager_prop_members(&host);
    let lazy_members = lazy("/w/props.ts", "Props").lazy_prop_members(&host);
    let eager_names: std::collections::BTreeSet<&str> =
        eager_members.iter().map(|p| p.name.as_str()).collect();
    let lazy_names: std::collections::BTreeSet<&str> =
        lazy_members.iter().map(|p| p.name.as_str()).collect();
    assert!(
        eager_names.contains("a") && eager_names.contains("b"),
        "eager arm must surface both imported intersection arms (`a`, `b`); got {eager_names:?}",
    );
    assert_eq!(
        lazy_names, eager_names,
        "lazy arm MUST surface members from an intersection of two imported refs",
    );
}

#[test]
fn duplicate_member_across_heritage_and_own_body_first_writer_wins() {
    // Props extends BaseProps, both declare `dup`. Own body wins (TS
    // member precedence: the derived declaration's member shadows the base).
    let owner: &'static str =
        Box::leak(vue_importing("./props", "defineProps", "Props").into_boxed_str());
    let props = "import type { BaseProps } from './base'\n\
                 export interface Props extends BaseProps {\n  dup: string\n}";
    let base = "export interface BaseProps {\n  dup: number\n}";
    let host = build_host(&[
        (OWNER_VUE_PATH, owner, FileKind::VueSfc),
        ("/w/props.ts", props, FileKind::NonSfc),
        ("/w/base.ts", base, FileKind::NonSfc),
    ]);

    let lazy_members = lazy("/w/props.ts", "Props").lazy_prop_members(&host);

    // TS member precedence: a derived declaration's member shadows the
    // inherited one. `interface Props extends BaseProps { dup: string }`
    // exposes exactly ONE `dup`, typed `string` (the own-body
    // declaration), NOT two. The carrier-complete reader's Intersection
    // accumulation applies first-writer-wins, with the own-body Object arm
    // ordered after the heritage arm in `lower_decl_body_with_provenance`'s
    // class/interface fold — wait, the interface fold pushes heritage
    // first then body, so the OWN body is the LATER arm. The reader's
    // first-writer-wins keeps the FIRST seen; the own-body member must
    // still win. We assert the TS-correct outcome directly.
    //
    // NOTE (eager-rail divergence): the EAGER OXC rail emits TWO `dup`
    // entries for this cross-file heritage shape (it does not dedup the
    // inherited duplicate). The lazy reader is STRICTLY MORE CORRECT (one
    // `dup`). Per the project rule that neither rail is a ground-truth
    // oracle, we assert TS-correct behavior on the lazy side, not
    // lazy==eager.
    let lazy_dup: Vec<_> = lazy_members.iter().filter(|p| p.name == "dup").collect();
    assert_eq!(
        lazy_dup.len(),
        1,
        "lazy: exactly one `dup` member (TS member precedence — no duplicate \
         across heritage + own body); got {:?}",
        lazy_members
            .iter()
            .map(|p| p.name.clone())
            .collect::<Vec<_>>(),
    );
    // The own-body declaration (`dup: string`) wins over inherited
    // (`dup: number`).
    assert_eq!(
        lazy_dup[0].type_expr,
        Some(verter_type_expr::TypeExpr::Primitive(
            verter_type_expr::PrimitiveName::String
        )),
        "the own-body `dup: string` MUST win over the inherited `dup: number` \
         (TS derived-member precedence)",
    );
    assert!(
        lazy_dup[0].declared_in_macro_type_arg,
        "the winning `dup` is the own-body declaration → declared_in_macro_type_arg=true",
    );
}

// ===========================================================================
// `get_component_meta` cross-file fixtures (Stage-2 flip equivalence target)
// ===========================================================================
//
// These drive the PUBLIC production entry point
// (`get_component_meta_with_resolution`) end-to-end over cross-file
// heritage shapes. Production is EAGER in Stage 1.5, so they PASS via the
// eager rail today; they become the equivalence target the Stage-2
// producer flip must reproduce on the corrected (carrier-complete) reader.

/// Collect `(name, raw_type, declared_in_macro_type_arg)` for the expanded
/// props of a component via the production `get_component_meta` path.
fn meta_expanded_props(host: &VerterHost, owner: &str) -> Vec<(String, Option<String>, bool)> {
    let (_meta, resolved) = host
        .get_component_meta_with_resolution(owner)
        .expect("component meta resolves");
    let evaluated = resolved
        .evaluated_types
        .expect("expanded component types present");
    evaluated
        .props
        .iter()
        .map(|p| {
            (
                p.name.clone(),
                p.raw_type.clone(),
                p.declared_in_macro_type_arg,
            )
        })
        .collect()
}

#[test]
fn get_component_meta_interface_extends_imported_base() {
    let owner: &'static str =
        Box::leak(vue_importing("./props", "defineProps", "Props").into_boxed_str());
    let props = "import type { BaseProps } from './base'\n\
                 export interface Props extends BaseProps {\n  /** own field */\n  own: string\n}";
    let base = "export interface BaseProps {\n  /** base field */\n  base: number\n}";
    let host = build_host(&[
        (OWNER_VUE_PATH, owner, FileKind::VueSfc),
        ("/w/props.ts", props, FileKind::NonSfc),
        ("/w/base.ts", base, FileKind::NonSfc),
    ]);
    let props_meta = meta_expanded_props(&host, OWNER_VUE_PATH);
    let names: std::collections::BTreeSet<&str> =
        props_meta.iter().map(|(n, _, _)| n.as_str()).collect();
    assert!(
        names.contains("base") && names.contains("own"),
        "get_component_meta MUST surface inherited `base` + own `own` for cross-file \
         heritage; got {names:?}",
    );
    // NOTE (eager-vs-lazy provenance divergence): the EAGER production path
    // currently marks BOTH the inherited `base` and the own `own` member
    // `declared_in_macro_type_arg = true` for cross-file heritage (it does
    // not decay the heritage member to `false` here). The Stage-1.5
    // carrier-complete LAZY reader correctly reports `base = false` (see
    // `interface_extends_imported_base_inherited_props_present`), so the
    // Stage-2 producer flip will CHANGE this bit for inherited members —
    // a lazy-side correctness improvement. We pin the eager status quo here
    // (own-body member is `true`) so the flip's behavior delta is visible,
    // and assert the core cross-file heritage requirement (both members
    // present) which the flip must preserve.
    let own_entry = props_meta.iter().find(|(n, _, _)| n == "own").unwrap();
    assert!(
        own_entry.2,
        "own-body `own` MUST carry declared_in_macro_type_arg=true through get_component_meta",
    );
}

#[test]
fn get_component_meta_interface_extends_omit_imported_base() {
    let owner: &'static str =
        Box::leak(vue_importing("./props", "defineProps", "Props").into_boxed_str());
    let props = "import type { ImportedBase } from './base'\n\
                 export interface Props extends Omit<ImportedBase, 'hidden'> {\n  own: string\n}";
    let base = "export interface ImportedBase {\n  kept: number\n  hidden: string\n}";
    let host = build_host(&[
        (OWNER_VUE_PATH, owner, FileKind::VueSfc),
        ("/w/props.ts", props, FileKind::NonSfc),
        ("/w/base.ts", base, FileKind::NonSfc),
    ]);
    let names: std::collections::BTreeSet<String> = meta_expanded_props(&host, OWNER_VUE_PATH)
        .into_iter()
        .map(|(n, _, _)| n)
        .collect();
    assert!(
        names.contains("kept") && names.contains("own"),
        "get_component_meta MUST surface Omit-kept `kept` + own `own`; got {names:?}",
    );
    assert!(
        !names.contains("hidden"),
        "get_component_meta MUST drop the omitted `hidden`; got {names:?}",
    );
}

#[test]
fn get_component_meta_interface_extends_via_wildcard_barrel() {
    let owner: &'static str =
        Box::leak(vue_importing("./props", "defineProps", "Props").into_boxed_str());
    let props = "import type { BaseProps } from './barrel'\n\
                 export interface Props extends BaseProps {\n  own: string\n}";
    let barrel = "export * from './base'";
    let base = "export interface BaseProps {\n  base: number\n}";
    let host = build_host(&[
        (OWNER_VUE_PATH, owner, FileKind::VueSfc),
        ("/w/props.ts", props, FileKind::NonSfc),
        ("/w/barrel.ts", barrel, FileKind::NonSfc),
        ("/w/base.ts", base, FileKind::NonSfc),
    ]);
    let names: std::collections::BTreeSet<String> = meta_expanded_props(&host, OWNER_VUE_PATH)
        .into_iter()
        .map(|(n, _, _)| n)
        .collect();
    assert!(
        names.contains("base") && names.contains("own"),
        "get_component_meta MUST surface inherited `base` through a wildcard barrel; got {names:?}",
    );
}
