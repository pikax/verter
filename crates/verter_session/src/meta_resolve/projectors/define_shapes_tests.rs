//! Tests for the `define_props` / `define_emits` / `define_slots` shape
//! publication — extracted sibling of `define_shapes.rs` (the `#[path]`
//! attach keeps private-item access and the `define_shapes::tests::*` test
//! paths).

use super::*;
use crate::meta::MetaProject;
use crate::project_semantic_dispatch::semantic_source::demand_semantic_source_type_expr_with_ctx;
use crate::resolver_core::{CanonicalCompletionOverlay, HostResolverContext};
use crate::types::HostConfig;
use crate::VerterHost;
use verter_type_expr::facts::{
    ClosedTypeFact, LeafTypeFact, ProjectedTypeFact, SemanticTypeSource,
};
use verter_type_expr::locators::AuthoredBodyLocator;
use verter_type_expr::TypeExpr;

fn project_with(canonical: &str, source: &str) -> std::sync::Arc<MetaProject> {
    let project = MetaProject::new(VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    }));
    project.upsert_base(canonical, source).unwrap();
    project
}

/// Drive `define_emits_shape` with the evaluated-field match WITHHELD and
/// return the published `save` property source POSITION.
fn evaluated_miss_save_position(project: &MetaProject) -> verter_type_expr::facts::SourcePosition {
    let host = project.host();
    let view = host.resolver_store_view_read().into_owned_view();
    let overlay = std::sync::Arc::new(CanonicalCompletionOverlay::new());
    let ctx = HostResolverContext::new(host, &view, overlay);
    let evaluated = ExpandedComponentTypes::default();
    let shape = define_emits_shape(&ctx, "/App.vue", 0, &evaluated)
        .expect("the emits macro surface resolves");
    shape
        .value
        .properties
        .iter()
        .find(|property| property.name == "save")
        .expect("the property event is published")
        .ty
        .clone()
}

/// Demand a published source through the ONE shared dispatch (raise +
/// observation reduce), materializing its `TypeExpr` under the base view.
fn demand_source(project: &MetaProject, source: &SemanticTypeSource) -> Option<TypeExpr> {
    let host = project.host();
    let view = host.resolver_store_view_read().into_owned_view();
    let overlay = std::sync::Arc::new(CanonicalCompletionOverlay::new());
    let ctx = HostResolverContext::new(host, &view, overlay);
    demand_semantic_source_type_expr_with_ctx(&ctx, "/App.vue", source)
}

/// The labels of a tuple-shaped `TypeExpr` (empty for any other shape).
fn tuple_labels(expr: &TypeExpr) -> Vec<String> {
    match expr {
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .filter_map(|element| element.label.clone())
            .collect(),
        _ => Vec::new(),
    }
}

/// A duplicate / intersection same-name property event (`{ save: A } &
/// { save: B }`) publishes the projected MEMBER-PATH source — never either
/// contributor's authored locator — and demanding it materializes the
/// MERGED member covering BOTH contributors.
#[test]
fn duplicate_intersection_emit_publishes_the_merged_member_not_a_contributor() {
    let project = project_with(
        "/App.vue",
        r#"<script setup lang="ts">
defineEmits<{ save: [id: number] } & { save: [name: string] }>()
</script>
<template><div /></template>"#,
    );
    let source = evaluated_miss_save_position(&project)
        .into_present()
        .expect("the fallback publishes a present source");
    assert!(
        matches!(
            &source,
            SemanticTypeSource::Projected(ProjectedTypeFact::MemberPath { .. })
        ),
        "a merged same-name member publishes the projected member-path \
         route; got {source:?}"
    );
    assert!(
        !matches!(&source, SemanticTypeSource::Authored(_)),
        "no single contributor's authored locator may represent the merged \
         member"
    );

    // Demanding the source materializes the MERGED member — both
    // contributors' payload tuples, never either alone.
    let demanded = demand_source(&project, &source)
        .expect("the projected member-path route raises through the one dispatch");
    let contributor_id = TypeExpr::Tuple {
        elements: std::sync::Arc::from(
            vec![verter_type_expr::TupleElement {
                label: Some("id".to_string()),
                ty: TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number),
                optional: false,
                rest: false,
            }]
            .into_boxed_slice(),
        ),
        readonly: false,
    };
    let contributor_name = TypeExpr::Tuple {
        elements: std::sync::Arc::from(
            vec![verter_type_expr::TupleElement {
                label: Some("name".to_string()),
                ty: TypeExpr::Primitive(verter_type_expr::PrimitiveName::String),
                optional: false,
                rest: false,
            }]
            .into_boxed_slice(),
        ),
        readonly: false,
    };
    assert_ne!(
        demanded, contributor_id,
        "the merged member is never the first contributor alone"
    );
    assert_ne!(
        demanded, contributor_name,
        "the merged member is never the second contributor alone"
    );
    let TypeExpr::Intersection(arms) = &demanded else {
        panic!(
            "the merged same-name member materializes as the intersection \
             of its contributors; got {demanded:?}"
        );
    };
    let labels: Vec<String> = arms.iter().flat_map(tuple_labels).collect();
    assert!(
        labels.iter().any(|label| label == "id") && labels.iter().any(|label| label == "name"),
        "both contributors' payloads participate in the merged member; got \
         arms {arms:?}"
    );
}

/// An imported referenced-tuple payload (`save: [payload: Payload]` — a
/// tuple whose element is a named reference, so no closed tuple fact
/// exists) publishes the projected MEMBER-PATH source and demands to the
/// referenced tuple — never the degraded Unknown leaf.
#[test]
fn inherited_referenced_tuple_emit_publishes_the_projected_member_path_route() {
    let project = project_with(
        "/emits.ts",
        "export interface SavePayload { id: number }\n\
         export interface ImportedEmits { save: [payload: SavePayload] }\n",
    );
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ImportedEmits } from './emits'
defineEmits<ImportedEmits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let source = evaluated_miss_save_position(&project)
        .into_present()
        .expect("the fallback publishes a present source");
    assert!(
        matches!(
            &source,
            SemanticTypeSource::Projected(ProjectedTypeFact::MemberPath { .. })
        ),
        "a non-closed inherited member with a stamped macro type-arg base \
         publishes the projected member-path route; got {source:?}"
    );
    assert_ne!(
        source,
        SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Primitive(
            verter_type_expr::PrimitiveName::Unknown,
        ))),
        "an inherited referenced tuple must not fall to the Unknown leaf"
    );
    let demanded = demand_source(&project, &source)
        .expect("the projected member-path route raises through the one dispatch");
    let TypeExpr::Tuple { elements, .. } = &demanded else {
        panic!("the referenced tuple materializes as the tuple; got {demanded:?}");
    };
    assert_eq!(elements.len(), 1);
    assert_eq!(
        elements[0].label.as_deref(),
        Some("payload"),
        "the authored tuple label is preserved"
    );
}

/// An imported OBJECT payload (`save: {{ id: number }}` — no closed fact
/// vocabulary covers an object member) publishes the projected MEMBER-PATH
/// source and demands to the object — never the degraded Unknown leaf.
#[test]
fn inherited_object_payload_emit_publishes_the_projected_member_path_route() {
    let project = project_with(
        "/emits.ts",
        "export interface ImportedEmits { save: { id: number } }\n",
    );
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ImportedEmits } from './emits'
defineEmits<ImportedEmits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let source = evaluated_miss_save_position(&project)
        .into_present()
        .expect("the fallback publishes a present source");
    assert!(
        matches!(
            &source,
            SemanticTypeSource::Projected(ProjectedTypeFact::MemberPath { .. })
        ),
        "a non-closed inherited object member publishes the projected \
         member-path route; got {source:?}"
    );
    let demanded = demand_source(&project, &source)
        .expect("the projected member-path route raises through the one dispatch");
    let TypeExpr::Object(object) = &demanded else {
        panic!("the object payload materializes as the object; got {demanded:?}");
    };
    assert!(
        object.properties.iter().any(|member| matches!(
            member,
            verter_type_expr::ObjectMember::Property(property) if property.name == "id"
        )),
        "the object payload's members materialize; got {object:?}"
    );
}

/// An unraisable projected member-path route (a base addressing a macro
/// that does not exist) is an honest `None` at the demand boundary — the
/// existing typed-failure / Unknown policy applies downstream; a partial
/// fact is never fabricated.
#[test]
fn unraisable_projected_member_path_demands_to_honest_none() {
    let project = project_with(
        "/App.vue",
        r#"<script setup lang="ts">
defineEmits<{ save: [id: number] }>()
</script>
<template><div /></template>"#,
    );
    let source = SemanticTypeSource::Projected(ProjectedTypeFact::MemberPath {
        base: AuthoredBodyLocator::MacroPayload(verter_type_expr::locators::MacroPayloadLocator {
            anchor: verter_type_expr::locators::AuthoredAnchor {
                canonical_id: std::sync::Arc::from("/App.vue"),
                symbol: std::sync::Arc::from("default"),
                space: verter_type_expr::locators::LocatorSymbolSpace::Value,
            },
            // Out of range: no such macro exists, so the base has no live
            // graph representation.
            macro_index: 7,
            payload: verter_type_expr::locators::MacroPayloadPosition::TypeArgument,
        }),
        path: std::sync::Arc::from(vec!["save".to_string()].into_boxed_slice()),
    });
    assert_eq!(
        demand_source(&project, &source),
        None,
        "an unroutable projected member-path base is an honest None — never \
         a fabricated body or a partial fact"
    );
}

/// A property-style emit whose projected MEMBER-PATH source has a VALID
/// base but whose path projection lands on the walker's interned
/// failed-query node (`Opaque(Miss)` — the payload declares no such
/// member) FAILS the output with the typed
/// `ComponentMetaOutputFailure::UnraisableSource` — a
/// PRESENT-but-unraisable source must NEVER silently publish as
/// `Unknown`. DISTINCT: a genuinely schema-ABSENT payload position
/// (source `None`) is NOT a failure — the central missing-source policy
/// keeps rendering the canonical typed `Unknown`.
#[test]
fn member_path_projection_onto_interned_miss_fails_output_typed_never_silent_unknown() {
    let project = project_with(
        "/App.vue",
        r#"<script setup lang="ts">
defineEmits<{ save: [id: number] }>()
</script>
<template><div /></template>"#,
    );
    let host = project.host();
    let analysis = host
        .get_component_meta("/App.vue")
        .expect("component must resolve");
    assert_eq!(analysis.events.len(), 1, "fixture declares exactly 1 event");

    // Sanity: the UNTAMPERED analysis materializes cleanly — the typed
    // failure asserted below is not unconditional.
    crate::meta_resolve::projectors::build_component_meta_output(
        host,
        "/App.vue",
        analysis.clone(),
        None,
    )
    .expect("the untampered analysis must materialize");

    // A VALID base (macro 0's authored type argument) with a path that
    // projects onto a member the payload does NOT declare: the
    // `ProjectPath` projection returns the walker's interned
    // `Opaque(Miss)` as a VALUE — a failed projection, not a resolved
    // member.
    let miss_projection_source = SemanticTypeSource::Projected(ProjectedTypeFact::MemberPath {
        base: AuthoredBodyLocator::MacroPayload(verter_type_expr::locators::MacroPayloadLocator {
            anchor: verter_type_expr::locators::AuthoredAnchor {
                canonical_id: std::sync::Arc::from("/App.vue"),
                symbol: std::sync::Arc::from("default"),
                space: verter_type_expr::locators::LocatorSymbolSpace::Value,
            },
            macro_index: 0,
            payload: verter_type_expr::locators::MacroPayloadPosition::TypeArgument,
        }),
        path: std::sync::Arc::from(vec!["not_a_member".to_string()].into_boxed_slice()),
    });
    let mut tampered = analysis.clone();
    tampered.events[0].payload =
        verter_type_expr::facts::SourcePosition::Present(miss_projection_source.clone());
    let err = crate::meta_resolve::projectors::build_component_meta_output(
        host, "/App.vue", tampered, None,
    )
    .expect_err(
        "a member-path projection landing on the interned failed-query node must FAIL \
         the output with a typed error — never silently publish Unknown",
    );
    assert_eq!(
        err.lane,
        crate::meta_resolve::ComponentMetaOutputLane::EventPayload,
        "the error must name the failed lane"
    );
    assert_eq!(err.index, 0, "the error must carry the failed lane index");
    assert_eq!(
        *err.position,
        verter_type_expr::facts::SourcePosition::Present(miss_projection_source),
        "the error must carry the failed position"
    );
    assert_eq!(
        err.failure,
        crate::meta_resolve::ComponentMetaOutputFailure::UnraisableSource,
        "the failure class must be the raise miss, got {:?}",
        err.failure
    );

    // DISTINCT: a genuinely ABSENT payload position (no typed source at
    // all) is NOT a failure — the output materializes and the lane
    // renders the canonical typed `Unknown`.
    let mut absent = analysis;
    absent.events[0].payload = verter_type_expr::facts::SourcePosition::unannotated();
    let output = crate::meta_resolve::projectors::build_component_meta_output(
        host, "/App.vue", absent, None,
    )
    .expect("a schema-ABSENT payload position must keep materializing the output");
    let (_analysis, _resolution, types) = output.into_parts();
    let payloads = types.into_lanes().event_payloads;
    assert_eq!(
        payloads[0],
        TypeExpr::Unknown { raw: String::new() },
        "the schema-ABSENT position renders the canonical typed Unknown, got {:?}",
        payloads[0]
    );
}

/// WITHHOLD the evaluated-field match (an EMPTY `ExpandedComponentTypes`)
/// and drive `define_emits_shape` directly: the DIRECT-AUTHORED
/// property-style event's fallback `ty` must be its authored
/// macro-payload source (`Authored(MacroPayload(..))`) — NEVER the
/// degraded Unknown leaf. This pins the exact fallback arm the
/// evaluated-field miss takes.
#[test]
fn authored_emit_fallback_publishes_the_macro_payload_source_without_evaluated_match() {
    let project = project_with(
        "/App.vue",
        r#"<script setup lang="ts">
defineEmits<{ save: [id: number] }>()
</script>
<template><div /></template>"#,
    );
    let host = project.host();
    let view = host.resolver_store_view_read().into_owned_view();
    let overlay = std::sync::Arc::new(CanonicalCompletionOverlay::new());
    let ctx = HostResolverContext::new(host, &view, overlay);

    // Deliberately WITHHELD: no evaluated emits field for `save`.
    let evaluated = ExpandedComponentTypes::default();
    let shape = define_emits_shape(&ctx, "/App.vue", 0, &evaluated)
        .expect("the authored emits macro surface resolves");
    let save = shape
        .value
        .properties
        .iter()
        .find(|property| property.name == "save")
        .expect("the authored property event is published");
    assert!(
        matches!(
            save.ty.present(),
            Some(SemanticTypeSource::Authored(
                AuthoredBodyLocator::MacroPayload(_)
            ))
        ),
        "with the evaluated match withheld the fallback publishes the \
         authored macro-payload source; got {:?}",
        save.ty
    );
    assert_ne!(
        save.ty,
        verter_type_expr::facts::SourcePosition::Present(SemanticTypeSource::Closed(
            ClosedTypeFact::Leaf(LeafTypeFact::Primitive(
                verter_type_expr::PrimitiveName::Unknown
            ))
        )),
        "an authored property event must never fall to the Unknown leaf"
    );
}

/// WITHHOLD the evaluated-field match for an IMPORTED / inherited
/// property-style event: the fallback `ty` must be the graph-native
/// complete closed TUPLE source — NEVER a fabricated authored locator
/// and NEVER the degraded Unknown leaf.
#[test]
fn inherited_emit_fallback_publishes_the_graph_native_source_without_evaluated_match() {
    let project = project_with(
        "/emits.ts",
        "export interface ImportedEmits { save: [id: number] }\n",
    );
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ImportedEmits } from './emits'
defineEmits<ImportedEmits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let host = project.host();
    let view = host.resolver_store_view_read().into_owned_view();
    let overlay = std::sync::Arc::new(CanonicalCompletionOverlay::new());
    let ctx = HostResolverContext::new(host, &view, overlay);

    // Deliberately WITHHELD: no evaluated emits field for `save`.
    let evaluated = ExpandedComponentTypes::default();
    let shape = define_emits_shape(&ctx, "/App.vue", 0, &evaluated)
        .expect("the imported emits macro surface resolves");
    let save = shape
        .value
        .properties
        .iter()
        .find(|property| property.name == "save")
        .expect("the imported property event is published");
    assert!(
        matches!(
            save.ty.present(),
            Some(SemanticTypeSource::Closed(ClosedTypeFact::Tuple(_)))
        ),
        "with the evaluated match withheld the fallback publishes the \
         graph-native closed tuple source; got {:?}",
        save.ty
    );
    assert!(
        !matches!(save.ty.present(), Some(SemanticTypeSource::Authored(_))),
        "no authored locator may be fabricated for a cross-file member"
    );
    assert_ne!(
        save.ty,
        verter_type_expr::facts::SourcePosition::Present(SemanticTypeSource::Closed(
            ClosedTypeFact::Leaf(LeafTypeFact::Primitive(
                verter_type_expr::PrimitiveName::Unknown
            ))
        )),
        "an inherited typed event must not fall to the Unknown leaf"
    );
}
