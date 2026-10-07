use super::*;

#[test]
fn evaluate_types_expose_bindings_follow_validated_owner_visibility() {
    let project = make_project();
    project
        .upsert_base(
            "/OwnerBindings.vue",
            r#"<script lang="ts">
const shared: string = 'module'
const moduleOnly: boolean = true
</script>

<script setup lang="ts">
const shared: number = 1
defineExpose({ shared, moduleOnly })
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let evaluated = project
        .host()
        .evaluate_types("/OwnerBindings.vue")
        .expect("evaluated types should exist");
    let binding_type = |name: &str| {
        let field = evaluated
            .bindings
            .iter()
            .find(|field| field.name == name)
            .unwrap_or_else(|| panic!("missing exposed binding {name}"));
        crate::test_only::semantic_source_probe::demand_type_expr(
            project.host(),
            "/OwnerBindings.vue",
            field
                .authority
                .source_position()
                .present()
                .expect("present binding source"),
        )
        .unwrap_or_else(|| panic!("binding {name} must demand-materialize"))
    };

    assert_eq!(
        binding_type("shared"),
        TypeExpr::Primitive(PrimitiveName::Number),
        "the setup-local binding shadows the same-name module binding"
    );
    assert_eq!(
        binding_type("moduleOnly"),
        TypeExpr::Primitive(PrimitiveName::Boolean),
        "an instance exposure may see its sole validated module parent"
    );
}

#[test]
fn define_expose_object_literal_jsdoc_publishes() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { ref, computed } from 'vue'

const count = ref(0)
const label = computed(() => `n=${count.value}`)

defineExpose({
  /**
   * The live counter value.
   * @internal
   */
  count,
  label,
})
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project.host().get_component_meta("/App.vue").expect("meta");

    let count = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "count")
        .expect("count must be exposed");
    assert_eq!(
        count.description.as_deref(),
        Some("The live counter value."),
        "object-literal defineExpose member's JSDoc description must publish"
    );
    let internal = count
        .tags
        .iter()
        .find(|tag| tag.name == "internal")
        .expect("@internal tag must publish on the exposed member");
    assert_eq!(internal.text.as_deref(), None);

    // Negative: an undocumented exposed member publishes NO description and
    // NO tags (no fabrication).
    let label = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "label")
        .expect("label must be exposed");
    assert_eq!(label.description.as_deref(), None);
    assert!(label.tags.is_empty(), "got {:?}", label.tags);
}

#[test]
fn define_expose_type_argument_only_publishes_members_with_docs() {
    let project = make_project();
    project
        .upsert_base(
            "/src/api.ts",
            r#"
export interface PanelApi {
  /**
   * Open the panel.
   * @public
   */
  open(): void
  close(): void
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Panel.vue",
            r#"<script setup lang="ts">
import type { PanelApi } from './api'

defineExpose<PanelApi>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Panel.vue")
        .expect("component meta resolves");

    let open = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "open")
        .expect("type-argument-only defineExpose must publish its surface members");
    assert_eq!(
        open.description.as_deref(),
        Some("Open the panel."),
        "type-argument member's JSDoc must publish without an object literal"
    );
    assert!(
        open.tags.iter().any(|tag| tag.name == "public"),
        "type-argument member's @public tag must publish, got {:?}",
        open.tags
    );
    // The method-typed member value is KNOWN structure: the structural
    // member-source projection publishes the faithful PRESENT projected
    // MEMBER-PATH replay route (the macro's stamped type-argument base +
    // the member name) — never a fabricated `unknown` success and never a
    // typed failure for a representable shape.
    match open.type_source.present() {
        Some(verter_type_expr::facts::SemanticTypeSource::Projected(
            verter_type_expr::facts::ProjectedTypeFact::MemberPath { path, .. },
        )) => {
            assert_eq!(
                path.as_ref(),
                [verter_type_engine::semantic_query::PropertyKey::identifier(
                    "open"
                )],
                "one member hop"
            );
        }
        other => panic!("the open member publishes the MemberPath replay source, got {other:?}"),
    }

    // Negative: an undocumented exposed member publishes nothing fabricated.
    let close = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "close")
        .expect("undocumented type-argument member must still publish");
    assert_eq!(close.description.as_deref(), None);
    assert!(close.tags.is_empty(), "got {:?}", close.tags);

    // The public-instance sidecar derives from the exposed surface, so the
    // type-argument-only members surface there too.
    let sidecar = meta
        .public_instance
        .as_ref()
        .expect("exposed members imply a public-instance sidecar");
    let sidecar_open = sidecar
        .members
        .iter()
        .find(|member| member.name == "open")
        .expect("sidecar must carry the exposed member");
    assert_eq!(
        sidecar_open.kind,
        verter_session_query::analysis::component_meta::PublicInstanceMemberKind::Exposed
    );
    assert_eq!(sidecar_open.description.as_deref(), Some("Open the panel."));
}

#[test]
fn define_expose_owner_local_type_argument_publishes_members_with_docs() {
    let project = make_project();
    project
        .upsert_base(
            "/src/Input.vue",
            r#"<script setup lang="ts">
interface LocalApi {
  /**
   * Focus the input.
   * @public
   */
  focus(): void
  blur(): void
}

defineExpose<LocalApi>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Input.vue")
        .expect("component meta resolves");

    let focus = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "focus")
        .expect("owner-local type-argument defineExpose must publish its surface members");
    assert_eq!(
        focus.description.as_deref(),
        Some("Focus the input."),
        "owner-local type-argument member's JSDoc must publish without an object literal"
    );
    assert!(
        focus.tags.iter().any(|tag| tag.name == "public"),
        "owner-local type-argument member's @public tag must publish, got {:?}",
        focus.tags
    );
    // The method-typed member value is KNOWN structure: the structural
    // member-source projection publishes the faithful PRESENT projected
    // MEMBER-PATH replay route (the macro's stamped type-argument base +
    // the member name) — never a fabricated `unknown` success and never a
    // typed failure for a representable shape.
    match focus.type_source.present() {
        Some(verter_type_expr::facts::SemanticTypeSource::Projected(
            verter_type_expr::facts::ProjectedTypeFact::MemberPath { path, .. },
        )) => {
            assert_eq!(
                path.as_ref(),
                [verter_type_engine::semantic_query::PropertyKey::identifier(
                    "focus"
                )],
                "one member hop"
            );
        }
        other => panic!("the focus member publishes the MemberPath replay source, got {other:?}"),
    }

    // Negative: an undocumented exposed member publishes nothing fabricated.
    let blur = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "blur")
        .expect("undocumented owner-local type-argument member must still publish");
    assert_eq!(blur.description.as_deref(), None);
    assert!(blur.tags.is_empty(), "got {:?}", blur.tags);

    // The public-instance sidecar derives from the exposed surface, so the
    // owner-local type-argument members surface there too.
    let sidecar = meta
        .public_instance
        .as_ref()
        .expect("exposed members imply a public-instance sidecar");
    let sidecar_focus = sidecar
        .members
        .iter()
        .find(|member| member.name == "focus")
        .expect("sidecar must carry the exposed member");
    assert_eq!(
        sidecar_focus.kind,
        verter_session_query::analysis::component_meta::PublicInstanceMemberKind::Exposed
    );
    assert_eq!(
        sidecar_focus.description.as_deref(),
        Some("Focus the input.")
    );
}

#[test]
fn define_expose_owner_local_type_argument_with_imported_heritage_publishes() {
    let project = make_project();
    project
        .upsert_base(
            "/src/base.ts",
            r#"
export interface BaseApi {
  /**
   * Reset the control.
   */
  reset(): void
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Control.vue",
            r#"<script setup lang="ts">
import type { BaseApi } from './base'

interface LocalApi extends BaseApi {
  /**
   * Focus the control.
   * @public
   */
  focus(): void
}

defineExpose<LocalApi>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Control.vue")
        .expect("component meta resolves");

    let focus = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "focus")
        .expect("owner-local heritage-bearing defineExpose must publish its own-body members");
    assert_eq!(focus.description.as_deref(), Some("Focus the control."));
    assert!(
        focus.tags.iter().any(|tag| tag.name == "public"),
        "own-body member's @public tag must publish, got {:?}",
        focus.tags
    );

    let reset = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "reset")
        .expect("heritage member from the imported base must publish");
    assert_eq!(reset.description.as_deref(), Some("Reset the control."));
}

#[test]
fn define_expose_owner_local_mixed_literal_and_type_argument_publishes_union() {
    let project = make_project();
    project
        .upsert_base(
            "/src/Field.vue",
            r#"<script setup lang="ts">
interface LocalApi {
  /**
   * Select the whole value.
   * @public
   */
  selectAll(): void
  /**
   * Type-argument doc for focus.
   */
  focus(): void
  clear(): void
}

const focus = () => {}
defineExpose<LocalApi>({
  /**
   * Focus the field.
   */
  focus,
})
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Field.vue")
        .expect("component meta resolves");

    // Object-literal member: its OWN leading JSDoc wins over the overlapping
    // type-argument member's doc (first-wins pairing).
    let focus = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "focus")
        .expect("object-literal member must be exposed");
    assert_eq!(
        focus.description.as_deref(),
        Some("Focus the field."),
        "literal member's own JSDoc must win over the type-argument member's doc"
    );

    // Type-argument-only member: publishes with its span-sliced doc + tag even
    // though an object literal is present alongside the owner-local type
    // argument (the mixed form rides the same owner-local discovery rail).
    let select_all = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "selectAll")
        .expect("mixed owner-local defineExpose must publish its type-argument-only members");
    assert_eq!(
        select_all.description.as_deref(),
        Some("Select the whole value."),
        "type-argument-only member's JSDoc must publish in the mixed local form"
    );
    assert!(
        select_all.tags.iter().any(|tag| tag.name == "public"),
        "type-argument-only member's @public tag must publish, got {:?}",
        select_all.tags
    );
    // The method-typed member value is KNOWN structure: the structural
    // member-source projection publishes the faithful PRESENT projected
    // MEMBER-PATH replay route (the macro's stamped type-argument base +
    // the member name) — never a fabricated `unknown` success and never a
    // typed failure for a representable shape.
    match select_all.type_source.present() {
        Some(verter_type_expr::facts::SemanticTypeSource::Projected(
            verter_type_expr::facts::ProjectedTypeFact::MemberPath { path, .. },
        )) => {
            assert_eq!(
                path.as_ref(),
                [verter_type_engine::semantic_query::PropertyKey::identifier(
                    "selectAll"
                )],
                "one member hop"
            );
        }
        other => {
            panic!("the selectAll member publishes the MemberPath replay source, got {other:?}")
        }
    }

    // Negative: an undocumented type-argument-only member publishes nothing
    // fabricated.
    let clear = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "clear")
        .expect("undocumented type-argument-only member must still publish");
    assert_eq!(clear.description.as_deref(), None);
    assert!(clear.tags.is_empty(), "got {:?}", clear.tags);

    // The public-instance sidecar derives from the exposed surface: both the
    // literal member and the type-argument-only member surface there.
    let sidecar = meta
        .public_instance
        .as_ref()
        .expect("exposed members imply a public-instance sidecar");
    let sidecar_select_all = sidecar
        .members
        .iter()
        .find(|member| member.name == "selectAll")
        .expect("sidecar must carry the type-argument-only exposed member");
    assert_eq!(
        sidecar_select_all.kind,
        verter_session_query::analysis::component_meta::PublicInstanceMemberKind::Exposed
    );
    assert_eq!(
        sidecar_select_all.description.as_deref(),
        Some("Select the whole value.")
    );
    let sidecar_focus = sidecar
        .members
        .iter()
        .find(|member| member.name == "focus")
        .expect("sidecar must carry the literal exposed member");
    assert_eq!(
        sidecar_focus.kind,
        verter_session_query::analysis::component_meta::PublicInstanceMemberKind::Exposed
    );
}

/// Entrance-matrix completion: mixed-IMPORTED `defineExpose<ImportedApi>({
/// ... })` — an imported type argument alongside an object literal. Mirrors
/// the mixed-local test above with the interface moved cross-file: the
/// literal member's own leading JSDoc wins on name overlap (first-wins
/// pairing), the imported type-argument-only member publishes its doc +
/// tag, an undocumented type-only member publishes nothing fabricated, and
/// the public-instance sidecar derives coherently from the union.
#[test]
fn define_expose_imported_mixed_literal_and_type_argument_publishes_union() {
    let project = make_project();
    project
        .upsert_base(
            "/src/imported_api.ts",
            r#"
export interface ImportedApi {
  /**
   * Select the whole value.
   * @public
   */
  selectAll(): void
  /**
   * Type-argument doc for focus.
   */
  focus(): void
  clear(): void
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Field.vue",
            r#"<script setup lang="ts">
import type { ImportedApi } from './imported_api'

const focus = () => {}
defineExpose<ImportedApi>({
  /**
   * Focus the field.
   */
  focus,
})
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Field.vue")
        .expect("component meta resolves");

    // Object-literal member: its OWN leading JSDoc wins over the overlapping
    // imported type-argument member's doc (first-wins pairing).
    let focus = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "focus")
        .expect("object-literal member must be exposed");
    assert_eq!(
        focus.description.as_deref(),
        Some("Focus the field."),
        "literal member's own JSDoc must win over the imported type-argument \
         member's doc"
    );

    // Imported type-argument-only member: publishes with its doc + tag even
    // though an object literal is present alongside the imported type
    // argument.
    let select_all = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "selectAll")
        .expect("mixed imported defineExpose must publish its type-argument-only members");
    assert_eq!(
        select_all.description.as_deref(),
        Some("Select the whole value."),
        "imported type-argument-only member's JSDoc must publish in the mixed \
         imported form"
    );
    assert!(
        select_all.tags.iter().any(|tag| tag.name == "public"),
        "imported type-argument-only member's @public tag must publish, got {:?}",
        select_all.tags
    );
    // The method-typed member value is KNOWN structure: the structural
    // member-source projection publishes the faithful PRESENT projected
    // MEMBER-PATH replay route (the macro's stamped type-argument base +
    // the member name) — never a fabricated `unknown` success and never a
    // typed failure for a representable shape.
    match select_all.type_source.present() {
        Some(verter_type_expr::facts::SemanticTypeSource::Projected(
            verter_type_expr::facts::ProjectedTypeFact::MemberPath { path, .. },
        )) => {
            assert_eq!(
                path.as_ref(),
                [verter_type_engine::semantic_query::PropertyKey::identifier(
                    "selectAll"
                )],
                "one member hop"
            );
        }
        other => {
            panic!("the selectAll member publishes the MemberPath replay source, got {other:?}")
        }
    }

    // Negative: an undocumented imported type-argument-only member publishes
    // nothing fabricated.
    let clear = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "clear")
        .expect("undocumented type-argument-only member must still publish");
    assert_eq!(clear.description.as_deref(), None);
    assert!(clear.tags.is_empty(), "got {:?}", clear.tags);

    // The public-instance sidecar derives from the exposed surface: both the
    // literal member and the imported type-argument-only member surface there.
    let sidecar = meta
        .public_instance
        .as_ref()
        .expect("exposed members imply a public-instance sidecar");
    let sidecar_select_all = sidecar
        .members
        .iter()
        .find(|member| member.name == "selectAll")
        .expect("sidecar must carry the imported type-argument-only exposed member");
    assert_eq!(
        sidecar_select_all.kind,
        verter_session_query::analysis::component_meta::PublicInstanceMemberKind::Exposed
    );
    assert_eq!(
        sidecar_select_all.description.as_deref(),
        Some("Select the whole value.")
    );
    let sidecar_focus = sidecar
        .members
        .iter()
        .find(|member| member.name == "focus")
        .expect("sidecar must carry the literal exposed member");
    assert_eq!(
        sidecar_focus.kind,
        verter_session_query::analysis::component_meta::PublicInstanceMemberKind::Exposed
    );
}

#[test]
fn define_expose_type_argument_jsdoc_publishes_cross_file() {
    let project = make_project();
    project
        .upsert_base(
            "/src/api.ts",
            r#"
export interface WidgetApi {
  /**
   * Focus the widget.
   * @public
   */
  focus(): void
  blur(): void
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Widget.vue",
            r#"<script setup lang="ts">
import type { WidgetApi } from './api'

const focus = () => {}
const blur = () => {}
defineExpose<WidgetApi>({ focus, blur })
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Widget.vue")
        .expect("component meta resolves");

    let focus = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "focus")
        .expect("focus must be exposed");
    assert_eq!(
        focus.description.as_deref(),
        Some("Focus the widget."),
        "imported type-argument member's JSDoc must publish on the exposed member"
    );
    assert!(
        focus.tags.iter().any(|tag| tag.name == "public"),
        "imported type-argument member's @public tag must publish, got {:?}",
        focus.tags
    );

    // Negative: an undocumented exposed member publishes nothing.
    let blur = meta
        .exposed
        .iter()
        .find(|exposed| exposed.name == "blur")
        .expect("blur must be exposed");
    assert_eq!(blur.description.as_deref(), None);
    assert!(blur.tags.is_empty(), "got {:?}", blur.tags);
}

/// ECRV8-AC3: `UnraisableSource` on Exposed / PublicInstanceMember publishes
/// the centralized typed unsupported outcome for that slot and keeps sibling
/// members. The whole request stays `Ok`.
#[test]
fn component_meta_output_exposed_unraisable_source_degrades_per_member() {
    use verter_session_query::analysis::component_meta as cm;
    let project = make_project();
    project
        .upsert_base("/App.vue", "<template><div /></template>")
        .unwrap();
    let host = project.host();
    let bad = authored_decl_body_source("/definitely-missing.ts", "NoSuchType");
    let healthy = closed_ref_source("Healthy");

    let mut analysis = blank_output_analysis();
    analysis.exposed.push(cm::ExposedAnalysis {
        name: "bad".to_string(),
        type_source: verter_type_expr::facts::SourcePosition::Present(bad.clone()),
        type_expansion: None,
        description: None,
        tags: Vec::new(),
    });
    analysis.exposed.push(cm::ExposedAnalysis {
        name: "ok".to_string(),
        type_source: verter_type_expr::facts::SourcePosition::Present(healthy.clone()),
        type_expansion: None,
        description: None,
        tags: Vec::new(),
    });
    analysis.public_instance = Some(cm::PublicInstanceAnalysis {
        members: vec![
            cm::PublicInstanceMemberAnalysis {
                name: "bad".to_string(),
                kind: cm::PublicInstanceMemberKind::Exposed,
                type_source: verter_type_expr::facts::SourcePosition::Present(bad),
                type_expansion: None,
                raw_type: None,
                description: None,
                tags: Vec::new(),
            },
            cm::PublicInstanceMemberAnalysis {
                name: "ok".to_string(),
                kind: cm::PublicInstanceMemberKind::Exposed,
                type_source: verter_type_expr::facts::SourcePosition::Present(healthy),
                type_expansion: None,
                raw_type: None,
                description: None,
                tags: Vec::new(),
            },
        ],
        completeness: cm::PublicInstanceCompleteness::Exact,
    });

    let fixture_dispatch_14 =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
    let output = crate::meta_resolve::projectors::build_component_meta_output(
        host,
        &fixture_dispatch_14,
        "/App.vue",
        analysis,
        None,
        crate::meta_resolve::PublishedCompleteness::COMPLETE,
    )
    .expect("UnraisableSource on Exposed/PublicInstanceMember must not fail the request");
    let lanes = output.into_parts().2.into_lanes();
    let unsupported = TypeExpr::Unknown(UnknownValue::missing_output());
    let healthy_ty = TypeExpr::Ref {
        name: Arc::from("Healthy"),
        type_arguments: Arc::from([]),
    };
    assert_eq!(
        lanes.exposed,
        vec![unsupported.clone(), healthy_ty.clone()],
        "unraisable exposed[0] is typed unsupported; sibling stays live"
    );
    assert_eq!(
        lanes.public_instance_members,
        vec![unsupported, healthy_ty],
        "unraisable public-instance[0] is typed unsupported; sibling stays live"
    );
}

/// ECRV8-AC3 boundary: a non-`UnraisableSource` failure on Exposed still
/// fails the whole output; the per-member degrade does not swallow it.
#[test]
fn component_meta_output_exposed_required_source_unavailable_still_fails_closed() {
    use verter_session_query::analysis::component_meta as cm;
    let project = make_project();
    project
        .upsert_base("/App.vue", "<template><div /></template>")
        .unwrap();
    let host = project.host();

    let mut analysis = blank_output_analysis();
    analysis.exposed.push(cm::ExposedAnalysis {
        name: "failed".to_string(),
        type_source: verter_type_expr::facts::SourcePosition::Failed(
            verter_type_expr::facts::SemanticSourceFailure::UnrepresentableRequiredPayload,
        ),
        type_expansion: None,
        description: None,
        tags: Vec::new(),
    });
    analysis.exposed.push(cm::ExposedAnalysis {
        name: "ok".to_string(),
        type_source: verter_type_expr::facts::SourcePosition::Present(closed_ref_source("Healthy")),
        type_expansion: None,
        description: None,
        tags: Vec::new(),
    });

    let fixture_dispatch_15 =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
    let err = crate::meta_resolve::projectors::build_component_meta_output(
        host,
        &fixture_dispatch_15,
        "/App.vue",
        analysis,
        None,
        crate::meta_resolve::PublishedCompleteness::COMPLETE,
    )
    .expect_err("RequiredSourceUnavailable on Exposed must still fail the output");
    assert_eq!(
        err.lane,
        crate::meta_resolve::ComponentMetaOutputLane::Exposed
    );
    assert_eq!(err.index, 0);
    assert!(
        matches!(
            err.failure,
            crate::meta_resolve::ComponentMetaOutputFailure::RequiredSourceUnavailable { .. }
        ),
        "degrade arm is UnraisableSource-only, got {:?}",
        err.failure
    );
}

/// The EXPOSED surface likewise preserves a stable unresolved arm beside the
/// concrete `string` arm.
#[test]
fn same_name_intersection_exposed_preserves_unresolved_carrier() {
    let project = make_project();
    project
        .upsert_base("/bad.ts", "export interface Bad { x: MissingType }\n")
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Bad } from './bad'
defineExpose<{ x: string } & Bad>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let (analysis, _resolution, types) = project
        .host()
        .get_component_meta_output("/App.vue")
        .expect("the stable unresolved exposed carrier materializes")
        .expect("the SFC resolves")
        .into_parts();
    let index = analysis
        .exposed
        .iter()
        .position(|member| member.name == "x")
        .expect("x exposed member publishes");
    let lanes = types.into_lanes();
    let TypeExpr::Intersection(arms) = &lanes.exposed[index] else {
        panic!(
            "the exposed member preserves both contributors; got {:?}",
            lanes.exposed[index]
        );
    };
    assert!(
        arms.iter()
            .any(|arm| matches!(arm, TypeExpr::Ref { name, type_arguments }
                if name.as_ref() == "MissingType" && type_arguments.is_empty()))
            && arms
                .iter()
                .any(|arm| matches!(arm, TypeExpr::Primitive(PrimitiveName::String))),
        "the exposed intersection retains the carrier and concrete arm; got {arms:?}"
    );
    let (_analysis, state) = project
        .host()
        .get_component_meta_with_resolution("/App.vue")
        .expect("component resolves");
    assert!(
        !state.completeness.is_partial() && !state.synthesis_should_suppress,
        "stable unresolved exposed carriers remain complete and cacheable; got {:?}",
        state.completeness
    );
}
