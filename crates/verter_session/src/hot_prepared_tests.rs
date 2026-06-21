//! Discriminating tests for the session hot prepared-decl CARRIERS.
//!
//! These pin that `HotPreparedTypeDecl::from_parts` /
//! `HotPreparedValueDecl::from_parts` and the focused hot accessors
//! ASSEMBLE the carrier faithfully: every handle field round-trips to the
//! EXACT `HotTypeRef` it was constructed with, and the merged/non-merged
//! discriminant is real. Each assertion is DISCRIMINATING — it would FAIL
//! if `from_parts` dropped a field, swapped two handle fields, or an
//! accessor returned the wrong member.
//!
//! The handles are REAL: a small TS source is upserted into a standalone
//! host and resolved through `resolve_named_symbol` (the same path the
//! `*_equivalence_tests.rs` use), so the captured `SemanticNodeId`s are
//! genuine interned graph nodes. The split between `semantic_body` and
//! `lookup_body` is exercised with TWO DISTINCT handles so a producer that
//! collapsed them is caught.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::resolver_core::hot_prepared::{
    HotEnumMemberValue, HotFunctionParam, HotFunctionSignature, HotPreparedClassifierMeta,
    HotPreparedMember, HotPreparedTypeDecl, HotPreparedValueDecl, HotPreparedValueMember,
    HotProjectionKind, HotTypeParamDecl, HotWrapperKind,
};
use crate::semantic_query::{HotTypeRef, ProjectionMode, SemanticNodeData, SemanticNodeId};
use crate::types::{FileLanguage, HostConfig, UpsertRequest};
use crate::VerterHost;
use verter_semantic::analysis::type_eval::{TypeDeclKind, ValueDeclKind};
use verter_semantic::analysis::type_solver::host::ResolvedRootIdentity;

fn make_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig::default()))
}

fn upsert_ts(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert ts");
}

fn node_data(host: &VerterHost, node: SemanticNodeId) -> Arc<SemanticNodeData> {
    host.project_type_store()
        .semantic_graph()
        .node_data(node)
        .expect("node interned during resolution")
}

fn resolve(host: &VerterHost, canonical: &str, name: &str, mode: ProjectionMode) -> SemanticNodeId {
    host.resolve_named_symbol(canonical, name, &[], Some(mode))
        .unwrap_or_else(|| panic!("`{name}` must resolve in {mode:?}"))
}

/// Resolve several DISTINCT real graph nodes from one fixture so a
/// field-swap in `from_parts` is detectable.
///
/// - `merged`: the `MergedDecl` node for `interface I {a} + interface I {b}`
///   — and its two distinct contributor nodes.
/// - `object`: the resolved `Object` body for `interface O { m: number }`.
/// - `alias`: the `DeclRef` carrier for `type A = O`.
struct Handles {
    host: Arc<VerterHost>,
    merged: HotTypeRef,
    contributor_a: HotTypeRef,
    contributor_b: HotTypeRef,
    object: HotTypeRef,
    alias: HotTypeRef,
}

fn build_handles() -> Handles {
    let host = make_host();
    upsert_ts(
        &host,
        "/h.ts",
        "export interface I { a: number }\n\
         export interface I { b: string }\n\
         export interface O { m: number }\n\
         export type A = O;\n",
    );

    // Merged interface → MergedDecl carrier with two distinct contributors.
    let merged_node = resolve(&host, "/h.ts", "I", ProjectionMode::Navigate);
    let (contributor_a, contributor_b) = match node_data(&host, merged_node).as_ref() {
        SemanticNodeData::MergedDecl { contributors } => {
            assert_eq!(contributors.len(), 2, "I must carry two contributors");
            (contributors[0], contributors[1])
        }
        other => panic!("merged interface I must lower to MergedDecl, got {other:?}"),
    };
    assert_ne!(
        contributor_a, contributor_b,
        "the two interface-I contributors must be DISTINCT graph nodes"
    );

    // Object body (Expanded reaches the resolved Object surface).
    let object_node = resolve(&host, "/h.ts", "O", ProjectionMode::Expanded);
    assert!(
        matches!(
            node_data(&host, object_node).as_ref(),
            SemanticNodeData::Object(_)
        ),
        "O must resolve to an Object body"
    );

    // Alias DeclRef carrier.
    let alias_node = resolve(&host, "/h.ts", "A", ProjectionMode::Navigate);
    assert!(
        matches!(
            node_data(&host, alias_node).as_ref(),
            SemanticNodeData::DeclRef { .. }
        ),
        "A must resolve to a DeclRef carrier"
    );

    Handles {
        merged: HotTypeRef::new(merged_node),
        contributor_a: HotTypeRef::new(contributor_a),
        contributor_b: HotTypeRef::new(contributor_b),
        object: HotTypeRef::new(object_node),
        alias: HotTypeRef::new(alias_node),
        host,
    }
}

// ════════════════════════════════════════════════════════════════════
// HotPreparedTypeDecl — from_parts round-trip + discriminating accessors.
// ════════════════════════════════════════════════════════════════════

#[test]
fn hot_prepared_type_decl_from_parts_round_trips_every_handle() {
    let h = build_handles();

    // Build a member index with ONE member whose value is the object handle.
    let mut member_index: FxHashMap<Arc<str>, HotPreparedMember> = FxHashMap::default();
    member_index.insert(
        Arc::from("the_member"),
        HotPreparedMember {
            ty: h.object,
            optional: false,
            readonly: false,
            is_method: false,
            visibility: verter_type_expr::MemberVisibility::Public,
            spans: verter_type_expr::MemberSpans::default(),
        },
    );

    // One type-parameter carrying a constraint handle.
    let type_parameters = vec![HotTypeParamDecl {
        name: Arc::from("T"),
        constraint: Some(h.alias),
        default: None,
    }];

    let carrier = HotPreparedTypeDecl::from_parts(
        ResolvedRootIdentity::new("/h.ts", "Subject"),
        Some("Subject".to_string()),
        TypeDeclKind::Interface,
        type_parameters,
        // semantic_body and lookup_body are DELIBERATELY DIFFERENT handles
        // so the split is proven real — a producer that fed one to both
        // (or swapped them) is caught.
        h.merged,        // semantic_body
        h.contributor_a, // lookup_body (a different handle)
        vec![h.contributor_b],
        member_index,
        Vec::new(),
        Vec::new(),
        FxHashMap::default(),
        Default::default(),
        Default::default(),
        HotPreparedClassifierMeta {
            wrapper_kind: HotWrapperKind::None,
            projection_kind: HotProjectionKind::DirectMembers,
        },
    );

    // semantic_body round-trips AND is NOT lookup_body (the split is real).
    assert_eq!(
        carrier.semantic_body_handle(),
        h.merged,
        "semantic_body must round-trip to the merged handle"
    );
    assert_ne!(
        carrier.semantic_body_handle(),
        h.contributor_a,
        "semantic_body must NOT equal lookup_body — a producer that fed one \
         handle to both would be caught here"
    );

    // lookup_body round-trips AND is NOT semantic_body.
    assert_eq!(
        carrier.lookup_body_handle(),
        h.contributor_a,
        "lookup_body must round-trip to its own handle"
    );
    assert_ne!(
        carrier.lookup_body_handle(),
        h.merged,
        "lookup_body must NOT equal semantic_body"
    );

    // Member handle resolves to the exact member value handle.
    assert_eq!(
        carrier.member_handle("the_member"),
        Some(h.object),
        "the indexed member must round-trip to the object handle"
    );
    // NEGATIVE control: an absent member is None (no fabrication).
    assert_eq!(
        carrier.member_handle("nonexistent"),
        None,
        "an absent member must return None"
    );

    // merged_contributors round-trip in order AND is_merged() is true.
    assert_eq!(
        carrier.merged_contributors(),
        &[h.contributor_b],
        "merged_contributors must round-trip the contributor handle"
    );
    assert!(
        carrier.is_merged(),
        "a carrier with non-empty merged_contributors is merged"
    );

    // Type-parameter constraint handle round-trips.
    assert_eq!(
        carrier.type_parameters[0].constraint,
        Some(h.alias),
        "the type-parameter constraint must round-trip to the alias handle"
    );
    assert_eq!(carrier.type_parameters[0].default, None);

    // The handles point to the EXPECTED node data via the live graph: the
    // semantic_body handle is a MergedDecl (the merged interface we fed in),
    // and the member handle is the Object body.
    assert!(
        matches!(
            node_data(&h.host, carrier.semantic_body_handle().node()).as_ref(),
            SemanticNodeData::MergedDecl { .. }
        ),
        "semantic_body handle must point to the MergedDecl node it was built from"
    );
    assert!(
        matches!(
            node_data(&h.host, carrier.member_handle("the_member").unwrap().node()).as_ref(),
            SemanticNodeData::Object(_)
        ),
        "the member handle must point to the Object body node"
    );
}

#[test]
fn hot_prepared_type_decl_non_merged_reports_not_merged() {
    let h = build_handles();
    let carrier = HotPreparedTypeDecl::from_parts(
        ResolvedRootIdentity::new("/h.ts", "Plain"),
        None,
        TypeDeclKind::Alias,
        Vec::new(),
        h.alias,    // semantic_body
        h.alias,    // lookup_body (identical is allowed for a plain alias)
        Vec::new(), // empty contributors
        FxHashMap::default(),
        Vec::new(),
        Vec::new(),
        FxHashMap::default(),
        Default::default(),
        Default::default(),
        HotPreparedClassifierMeta {
            wrapper_kind: HotWrapperKind::None,
            projection_kind: HotProjectionKind::ForwardSubject,
        },
    );
    assert!(
        !carrier.is_merged(),
        "a carrier with empty merged_contributors must report is_merged() == false"
    );
    assert!(
        carrier.merged_contributors().is_empty(),
        "merged_contributors must be empty for a non-merged decl"
    );
}

// ════════════════════════════════════════════════════════════════════
// HotPreparedValueDecl — from_parts round-trip + discriminating accessors.
// ════════════════════════════════════════════════════════════════════

#[test]
fn hot_prepared_value_decl_from_parts_round_trips_every_handle() {
    let h = build_handles();

    // One object member whose value is the object handle.
    let mut object_member_index: FxHashMap<Arc<str>, HotPreparedValueMember> = FxHashMap::default();
    object_member_index.insert(
        Arc::from("field"),
        HotPreparedValueMember {
            ty: h.object,
            optional: false,
            readonly: false,
            is_method: false,
            visibility: verter_type_expr::MemberVisibility::Public,
            spans: verter_type_expr::MemberSpans::default(),
        },
    );

    // One signature whose RETURN TYPE is a handle (the alias handle); a
    // scalar param (no handle); one type-parameter with a default handle.
    let signatures = vec![HotFunctionSignature {
        parameters: vec![HotFunctionParam {
            name: Arc::from("p"),
            type_annotation: Some(Arc::from("number")),
            is_optional: false,
            has_default: false,
            span: verter_span::Span::default(),
        }],
        return_type: Some(h.alias),
        type_parameters: Arc::from(vec![HotTypeParamDecl {
            name: Arc::from("U"),
            constraint: None,
            default: Some(h.object),
        }]),
        has_implementation_body: false,
    }];

    // One enum member carrying a handle value.
    let enum_members = Some(vec![(
        Arc::<str>::from("Red"),
        HotEnumMemberValue::Folded(h.contributor_a),
    )]);

    let carrier = HotPreparedValueDecl::from_parts(
        ResolvedRootIdentity::new("/h.ts", "val"),
        Some("val".to_string()),
        ValueDeclKind::Const,
        Some(h.merged), // type_annotation handle
        signatures,
        object_member_index,
        enum_members,
        Vec::new(),
        FxHashMap::default(),
        Default::default(),
        Default::default(),
    );

    // type_annotation handle round-trips.
    assert_eq!(
        carrier.type_annotation_handle(),
        Some(h.merged),
        "type_annotation must round-trip to the merged handle"
    );

    // The signature's return type is the alias handle (NOT collapsed to the
    // type_annotation handle — a swap would be caught).
    assert_eq!(
        carrier.signatures[0].return_type,
        Some(h.alias),
        "the signature return type must round-trip to the alias handle"
    );
    assert_ne!(
        carrier.signatures[0].return_type,
        carrier.type_annotation_handle(),
        "the signature return type and the value type_annotation are DISTINCT \
         handles — a producer that crossed them would be caught"
    );
    // The signature's scalar param stays a display string, never a handle.
    assert_eq!(
        carrier.signatures[0].parameters[0]
            .type_annotation
            .as_deref(),
        Some("number"),
        "the param type_annotation is a scalar display string, not a handle"
    );
    // The signature type-parameter default handle round-trips.
    assert_eq!(
        carrier.signatures[0].type_parameters[0].default,
        Some(h.object),
        "the signature type-parameter default must round-trip to the object handle"
    );

    // The object member handle round-trips; a missing member is None.
    let field = carrier
        .object_member_index
        .get("field")
        .expect("the indexed object member must be present");
    assert_eq!(
        field.ty, h.object,
        "the object member value must round-trip to the object handle"
    );
    assert!(
        !carrier.object_member_index.contains_key("missing"),
        "an absent object member must not be present"
    );

    // The enum member handle round-trips.
    let enum_members = carrier
        .enum_members
        .as_ref()
        .expect("enum_members must be present");
    assert_eq!(enum_members.len(), 1);
    assert_eq!(enum_members[0].0.as_ref(), "Red");
    match enum_members[0].1 {
        HotEnumMemberValue::Folded(handle) => assert_eq!(
            handle, h.contributor_a,
            "the enum member handle must round-trip to the contributor handle"
        ),
        HotEnumMemberValue::Deferred(_) => {
            panic!("the enum member must stay Folded, not become Deferred")
        }
    }

    // The type_annotation handle points to the MergedDecl node it was built from.
    assert!(
        matches!(
            node_data(&h.host, carrier.type_annotation_handle().unwrap().node()).as_ref(),
            SemanticNodeData::MergedDecl { .. }
        ),
        "the type_annotation handle must point to the MergedDecl node"
    );
}
