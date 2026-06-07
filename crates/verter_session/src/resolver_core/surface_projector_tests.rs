//! Sibling tests for `surface_projector`.
//!
//! Extracted from the inline `#[cfg(test)] mod tests` block per the
//! CLAUDE.md "Rust test file organization" rule (~400 line cap on
//! inline test modules). The parent's `#[cfg(test)] mod
//! surface_projector_tests` declaration gates compilation; no inner
//! `#![cfg(test)]` is needed.
//!
//! The surface projector is
//! responsible for ONLY the native-only `native_props` carrier (the
//! private/protected class-member visibility surface). The published
//! props/emits/slots surface is owned by the typeinfo macro-surface path
//! (`vue_macro_dtos` → `props/emits/slots_from_typeinfo_surface`); its
//! behaviour is characterized in
//! `typeinfo/typeinfo_tests/vue_adapter.rs` and the component-meta e2e
//! suites, NOT here. These tests pin the `native_props` projection.

use super::surface_projector::*;
use verter_compiler::utils::oxc::vue::resolve_type::{
    ResolvedElements, ResolvedMemberVisibility, ResolvedProp,
};
use verter_semantic::analysis::types::AnalyzedMacroKind;

fn prop(
    name: &str,
    optional: bool,
    visibility: ResolvedMemberVisibility,
    type_text: Option<&str>,
    span_start: u32,
) -> ResolvedProp {
    ResolvedProp {
        span: verter_span::Span::new(span_start, span_start + 8),
        key: verter_span::Span::new(span_start, span_start + 3),
        key_name: Some(name.to_string()),
        optional,
        types: Vec::new(),
        visibility,
        type_span: None,
        type_text: type_text.map(str::to_string),
        map_local: false,
        span_is_absolute: true,
        type_expr: None,
        type_expr_scope: None,
        declared_in_macro_type_arg: false,
    }
}

/// `native_props` captures EVERY member regardless of visibility (it is the
/// class-member visibility surface), each carrying its own visibility tag.
/// This is the discriminator vs the published props surface, which filters to
/// public members and is owned by the typeinfo path.
#[test]
fn native_props_capture_all_members_with_visibility_and_span() {
    let elements = ResolvedElements {
        props: vec![
            prop(
                "label",
                false,
                ResolvedMemberVisibility::Public,
                Some("string"),
                0,
            ),
            prop(
                "secret",
                true,
                ResolvedMemberVisibility::Private,
                Some("number"),
                10,
            ),
        ],
        ..ResolvedElements::default()
    };

    let projected = project_macro_surfaces(None, AnalyzedMacroKind::DefineProps, &elements);

    // Both members surface as native props (visibility is carried, not filtered).
    assert_eq!(projected.native_props.len(), 2);

    let label = projected
        .native_props
        .iter()
        .find(|p| p.name == "label")
        .expect("label native prop present");
    assert_eq!(label.visibility, ResolvedMemberVisibility::Public);
    assert!(!label.is_optional);
    assert_eq!(label.type_annotation.as_deref(), Some("string"));
    assert_eq!(label.span, verter_span::Span::new(0, 8));

    let secret = projected
        .native_props
        .iter()
        .find(|p| p.name == "secret")
        .expect("secret native prop present");
    assert_eq!(secret.visibility, ResolvedMemberVisibility::Private);
    assert!(secret.is_optional);
    assert_eq!(secret.type_annotation.as_deref(), Some("number"));
}

/// `native_props` is projected for every macro kind from the resolved
/// elements' member set — it is independent of the macro kind (the projector no
/// longer branches on kind for props/emits/slots). A `DefineSlots`/`DefineEmits`
/// invocation still yields the native member surface.
#[test]
fn native_props_are_kind_independent() {
    let elements = ResolvedElements {
        props: vec![
            prop("a", false, ResolvedMemberVisibility::Public, Some("A"), 0),
            prop(
                "b",
                false,
                ResolvedMemberVisibility::Protected,
                Some("B"),
                10,
            ),
        ],
        ..ResolvedElements::default()
    };

    for kind in [
        AnalyzedMacroKind::DefineProps,
        AnalyzedMacroKind::DefineEmits,
        AnalyzedMacroKind::DefineSlots,
        AnalyzedMacroKind::WithDefaults,
        AnalyzedMacroKind::DefineModel,
    ] {
        let projected = project_macro_surfaces(None, kind, &elements);
        let names: Vec<&str> = projected
            .native_props
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["a", "b"],
            "native_props must surface every resolved member for {kind:?}"
        );
    }
}

/// Empty resolved elements yield an empty `native_props` surface (no panic, no
/// synthetic members).
#[test]
fn native_props_empty_for_no_members() {
    let elements = ResolvedElements::default();
    let projected = project_macro_surfaces(None, AnalyzedMacroKind::DefineProps, &elements);
    assert!(projected.native_props.is_empty());
}
