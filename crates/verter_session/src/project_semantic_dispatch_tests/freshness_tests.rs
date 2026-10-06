//! Freshness derivation from canonical excess-origin facts:
//! `FreshOwn` / `SpreadTainted` / `NonLiteral` are the single
//! authority — a spread-tainted or non-literal member revokes
//! freshness; only an all-`FreshOwn` object surface is `Fresh`.

use crate::HostConfig;
use crate::VerterHost;
use std::sync::Arc;
use verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch;
use verter_type_engine::semantic_query::AuthoredPropertyKey;
use verter_type_engine::semantic_query::FreshnessKey;
use verter_type_engine::semantic_query::MacroOwnBodyStamp;
use verter_type_engine::semantic_query::MergeRoleStamp;
use verter_type_engine::semantic_query::SemanticNodeData;
use verter_type_engine::semantic_query::SemanticNodeId;
use verter_type_engine::semantic_query::SurfaceMember;
use verter_type_expr::ExcessPropertyOrigin;

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn member(origin: ExcessPropertyOrigin, value: SemanticNodeId) -> SurfaceMember {
    SurfaceMember {
        key: AuthoredPropertyKey::string("m"),
        value,
        optional: false,
        readonly: false,
        method_kind: None,
        has_implementation_body: false,
        visibility: verter_type_expr::MemberVisibility::Public,
        spans: Default::default(),
        declaration_origin: None,
        declared_in_macro_type_arg: MacroOwnBodyStamp::NEUTRAL,
        merge_role: MergeRoleStamp::NEUTRAL,
        excess_origin: origin,
    }
}

fn object_with(
    graph: &Arc<verter_type_engine::semantic_query_memo::SemanticGraphStore>,
    members: Vec<SurfaceMember>,
) -> SemanticNodeId {
    graph.intern_node(SemanticNodeData::Object(
        verter_type_engine::surface_view! {
            members: Arc::from(members.into_boxed_slice()),
            call_signatures: Arc::from(Vec::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
            index_signatures: Arc::from(Vec::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        },
    ))
}

#[test]
fn all_fresh_own_members_derive_fresh() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = dispatch.graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(
        verter_type_engine::semantic_query::PrimitiveKind::String,
    ));
    let node = object_with(
        graph,
        vec![
            member(ExcessPropertyOrigin::FreshOwn, string),
            member(ExcessPropertyOrigin::FreshOwn, string),
        ],
    );
    assert_eq!(
        dispatch.freshness_for_source_node(node),
        FreshnessKey::Fresh,
        "an all-FreshOwn object surface is a proven fresh literal"
    );
}

#[test]
fn empty_object_surface_derives_fresh() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = dispatch.graph();
    let node = object_with(graph, Vec::new());
    assert_eq!(
        dispatch.freshness_for_source_node(node),
        FreshnessKey::Fresh,
        "`{{}}` is a fresh literal (vacuously all-FreshOwn)"
    );
}

/// The spread-derived freshness row: a spread-tainted member revokes
/// freshness — excess checking must NOT apply.
#[test]
fn spread_tainted_member_revokes_freshness() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = dispatch.graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(
        verter_type_engine::semantic_query::PrimitiveKind::String,
    ));
    let node = object_with(
        graph,
        vec![
            member(ExcessPropertyOrigin::FreshOwn, string),
            member(ExcessPropertyOrigin::SpreadTainted, string),
        ],
    );
    assert_eq!(
        dispatch.freshness_for_source_node(node),
        FreshnessKey::Regular,
        "a spread-tainted member revokes freshness"
    );
}

#[test]
fn non_literal_member_revokes_freshness() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = dispatch.graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(
        verter_type_engine::semantic_query::PrimitiveKind::String,
    ));
    let node = object_with(
        graph,
        vec![member(ExcessPropertyOrigin::NonLiteral, string)],
    );
    assert_eq!(
        dispatch.freshness_for_source_node(node),
        FreshnessKey::Regular,
        "a non-literal origin is never fresh"
    );
}

#[test]
fn alias_chains_derive_through() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = dispatch.graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(
        verter_type_engine::semantic_query::PrimitiveKind::String,
    ));
    let object = object_with(graph, vec![member(ExcessPropertyOrigin::FreshOwn, string)]);
    let alias = graph.intern_node(SemanticNodeData::Alias(object));
    assert_eq!(
        dispatch.freshness_for_source_node(alias),
        FreshnessKey::Fresh,
        "freshness derives through alias chains"
    );
}

#[test]
fn non_object_nodes_are_regular() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = dispatch.graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(
        verter_type_engine::semantic_query::PrimitiveKind::String,
    ));
    assert_eq!(
        dispatch.freshness_for_source_node(string),
        FreshnessKey::Regular,
        "a primitive is never a fresh object literal"
    );
}
