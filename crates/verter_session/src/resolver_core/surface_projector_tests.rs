//! Sibling tests for `surface_projector`.
//!
//! Extracted from the inline `#[cfg(test)] mod tests` block per the
//! CLAUDE.md "Rust test file organization" rule (~400 line cap on
//! inline test modules). The parent's `#[cfg(test)] mod
//! surface_projector_tests` declaration gates compilation; no inner
//! `#![cfg(test)]` is needed.
//!
//! These tests pin the surface-based `native_props` row construction
//! ([`ResolvedNativeProp::from_surface_member`]): the keep-all
//! class-member visibility rows built DIRECTLY from the shared-dispatch
//! one-level surface members
//! ([`crate::typeinfo::surface::TypeInfoSurfaceMember`]). The published
//! props/emits/slots/exposed surface is owned by the typeinfo
//! macro-surface path (`vue_macro_dtos` → the
//! `props/emits/slots/exposed_from_typeinfo_surface` normalizers); its
//! behaviour is characterized in `typeinfo/typeinfo_tests/vue_adapter.rs`
//! and the component-meta e2e suites, NOT here.
//!
//! Macro-KIND independence is structural on this path: the constructor
//! takes only the member (no macro-kind parameter exists anywhere on the
//! surface→native rail — `named_type_elements_outcome` resolves the named
//! root identically for every macro kind), so there is no kind axis left
//! to vary here. The e2e keep-all discriminators over the REAL projection
//! loop are
//! `meta_resolve_tests::native_props_fidelity_for_directly_declared_class_keeps_all_visibilities`
//! (own-body members), its heritage sibling, and the empty-member-set
//! projection e2e
//! (`meta_resolve_tests::native_props_empty_for_call_signature_only_imported_type`).

use std::sync::Arc;

use super::surface_projector::*;
use crate::semantic_query::{MemberMergeRole, SemanticNodeId};
use crate::typeinfo::surface::{CanonicalSpan, SurfaceMemberOrigin, TypeInfoSurfaceMember};
use verter_span::Span;
use verter_type_expr::MemberVisibility;

fn member(
    name: &str,
    optional: bool,
    visibility: MemberVisibility,
    name_span: Option<Span>,
) -> TypeInfoSurfaceMember {
    TypeInfoSurfaceMember {
        name: Arc::from(name),
        name_span: name_span.map(|span| CanonicalSpan::new(Arc::from("/types.ts"), span)),
        value: SemanticNodeId(1),
        type_annotation_span: None,
        optional,
        readonly: false,
        is_method: false,
        visibility,
        declared_in_macro_type_arg: false,
        jsdoc_description_span: None,
        jsdoc_tag_spans: Arc::from(Vec::new().into_boxed_slice()),
        origin: SurfaceMemberOrigin {
            canonical_file: None,
            declaration_span: None,
            merge_role: MemberMergeRole::OwnBody,
        },
    }
}

/// `native_props` rows carry EVERY visibility VERBATIM (they are the
/// class-member visibility surface): mapping a public, a protected, and a
/// private member each yields a row with its own visibility tag. This is the
/// discriminator vs the published props surface, which filters to public
/// members at the publication boundary: a visibility coercion (or a
/// `Public`-only short-circuit) inside the constructor would mislabel or
/// drop `shielded`/`secret` and FAIL this test.
///
/// Every row's `span` is the wire-honest `Span::default()` EVEN THOUGH the
/// members carry real declaration-site `name_span`s: the FFI/proto row emits
/// only `span_start`/`span_end` with no declaration-file id, so a
/// declaration-site offset would be unanchored on the wire. Re-introducing
/// `name_span` sourcing into the constructor would surface `Span::new(10,
/// 15)` here and FAIL this test.
#[test]
fn native_prop_rows_keep_all_visibilities_with_wire_default_spans() {
    let members = vec![
        member(
            "label",
            false,
            MemberVisibility::Public,
            Some(Span::new(10, 15)),
        ),
        member(
            "shielded",
            false,
            MemberVisibility::Protected,
            Some(Span::new(20, 28)),
        ),
        member(
            "secret",
            true,
            MemberVisibility::Private,
            Some(Span::new(30, 36)),
        ),
    ];

    let native: Vec<ResolvedNativeProp> = members
        .iter()
        .map(|member| {
            ResolvedNativeProp::from_surface_member(member, Some(format!("T_{}", member.name)))
        })
        .collect();

    assert_eq!(
        native.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        vec!["label", "shielded", "secret"],
        "every member maps to a native row, in member order"
    );

    let label = &native[0];
    assert_eq!(label.visibility, MemberVisibility::Public);
    assert!(!label.is_optional);
    assert_eq!(label.type_annotation.as_deref(), Some("T_label"));

    let shielded = &native[1];
    assert_eq!(
        shielded.visibility,
        MemberVisibility::Protected,
        "protected visibility carried verbatim, never coerced"
    );

    let secret = &native[2];
    assert_eq!(
        secret.visibility,
        MemberVisibility::Private,
        "private visibility carried verbatim, never coerced"
    );
    assert!(secret.is_optional);
    assert_eq!(secret.type_annotation.as_deref(), Some("T_secret"));

    for row in &native {
        assert_eq!(
            row.span,
            Span::default(),
            "native prop `{}` must publish the wire-honest default span even \
             when its surface member carries a declaration-site name_span — \
             the wire has no declaration-file anchor for the offsets",
            row.name
        );
    }
}

/// A member whose value the caller rendered no display text for surfaces
/// with a `None` annotation — never a fabricated display — and the same
/// wire-default span as every row.
#[test]
fn native_prop_row_unrendered_annotation_stays_none() {
    let synthesized = member("synthesized", false, MemberVisibility::Public, None);

    let row = ResolvedNativeProp::from_surface_member(&synthesized, None);

    assert_eq!(row.name, "synthesized");
    assert_eq!(row.span, Span::default());
    assert_eq!(
        row.type_annotation, None,
        "an unrendered member value must stay None, never fabricated"
    );
}
