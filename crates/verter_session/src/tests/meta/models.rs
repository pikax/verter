use super::*;

/// `notDeclared` is declared nowhere, so `made`'s call is TS2304: the
/// checker's error type is recovery for a program that does not type-check,
/// which the flow-return lane does not model — an unmodelled position by
/// design.
// Reads optional audit capture: runs in the instrumentation lane.
#[cfg(feature = "semantic-observe")]
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn one_unmodeled_member_marks_its_prop_and_the_props_surface_survives() {
    use std::sync::atomic::Ordering::Relaxed;

    let project = make_project();
    project
        .upsert_base(
            "/src/C1.vue",
            r#"<script setup lang="ts">
function makeProps() {
  return { label: "x", made: notDeclared() }
}
defineProps<ReturnType<typeof makeProps>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    let canonical = "/src/C1.vue";
    let meta = get_meta(&project, canonical);

    let names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    // 1. The MODELLED member survives.
    assert!(
        names.contains(&"label"),
        "the modelled sibling `label` MUST be published — one unmodelled member \
         never discards the composite; got {names:?}"
    );
    // 2. The unmodelled member is present and MARKED, never a fabricated
    //    `any` and never a silently dropped key.
    assert!(
        names.contains(&"made"),
        "the unmodelled member `made` MUST be published as a marked slot, \
         never silently dropped; got {names:?}"
    );
    let made = meta
        .props
        .iter()
        .find(|prop| prop.name == "made")
        .expect("the `made` prop");
    // The published slot is a TYPED FAILURE, not a value: the boundary
    // says "this member's type is not known" rather than handing over a
    // fabricated `any`, which is indistinguishable from an authored one at
    // every downstream gate. The positional marker spelling
    // (`UNMODELED_POSITION`) is derived from the failure at display time;
    // the prop itself carries the failed publication.
    assert!(
        matches!(
            made.publication.result(),
            verter_type_expr::PublicationResult::Failed { .. }
        ),
        "`made` must NOT publish a usable type source — it carries the typed \
         unresolved marker; got {:?}",
        made.publication.result()
    );

    // 3. The result is reported PARTIAL: the resolve suppresses synthesis,
    //    which is the boundary's "this surface is not a complete answer"
    //    channel.
    let (_, resolved) = host
        .get_component_meta_with_resolution(canonical)
        .expect("the resolve must still return metadata");
    assert!(
        resolved.synthesis_should_suppress,
        "a props surface carrying an unmodelled member is reported PARTIAL"
    );

    // 4. Nothing warms: a replay is COLD.
    let hits_before = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    let _ = get_meta(&project, canonical);
    let hits_after = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    assert_eq!(
        hits_after, hits_before,
        "a props surface carrying an unmodelled member MUST NOT warm \
         `ComponentMetaResultDb` (hits_before={hits_before}, hits_after={hits_after})"
    );
}

/// SOLE-AUTHORITY discrimination for the MODEL path: the model prop/event
/// TYPE source comes from the NORMALIZED `defineModel` surface row — never
/// a same-name flat evaluated prop from a SIBLING macro. The fixture
/// declares a `defineProps` prop and a `defineModel` binding under the SAME
/// name: the legacy path adopted the sibling `defineProps` flat row's
/// source (anchored at the props macro); the model's own normalized surface
/// row (its authored type-argument payload, anchored at the MODEL macro)
/// is authoritative.
#[test]
fn model_type_source_comes_from_the_normalized_define_model_surface() {
    use verter_type_expr::facts::{SemanticTypeSource, SourcePosition};
    use verter_type_expr::locators::AuthoredBodyLocator;

    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ value: string }>()
const value = defineModel<number>("value")
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("component resolves");
    let model = meta
        .models
        .iter()
        .find(|model| model.name == "value")
        .expect("the model publishes");
    // The model's type source is the NORMALIZED defineModel surface row's
    // authored payload — anchored at the MODEL macro (index 1), never the
    // sibling defineProps macro (index 0).
    match &model.type_source {
        SourcePosition::Present(SemanticTypeSource::Authored(
            AuthoredBodyLocator::MacroPayload(payload),
        )) => {
            assert_eq!(
                payload.macro_index, 1,
                "the model type source anchors at the defineModel macro, \
                 never the sibling defineProps macro"
            );
        }
        other => panic!(
            "the model publishes its own normalized authored payload \
             source, got {other:?}"
        ),
    }
    // Demanding the model's published source materializes the MODEL's own
    // `number` — never the sibling prop's `string`.
    let demanded = demand_published_type(
        project.host(),
        "/App.vue",
        model.type_source.present(),
        "model value type",
    );
    assert_eq!(
        demanded,
        TypeExpr::Primitive(PrimitiveName::Number),
        "the model type is the defineModel type argument"
    );
}

/// A `defineModel<T>()` whose model type is imported from another file
/// publishes its model, model prop and update event from the shared
/// framework-surface props read of the macro, the only source of the model
/// type: the prop carries that read's provenance, the model and the prop
/// share its authored type-argument source, and demanding that source
/// resolves the imported declaration.
#[test]
fn imported_model_type_publishes_from_the_framework_surface_props_read() {
    use verter_type_expr::facts::{SemanticTypeSource, SourcePosition};
    use verter_type_expr::locators::AuthoredBodyLocator;

    let project = make_project();
    project
        .upsert_base("/choice.ts", "export interface Choice { id: number }\n")
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Choice } from './choice'
const choice = defineModel<Choice>("choice")
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("component resolves");
    let model = meta
        .models
        .iter()
        .find(|model| model.name == "choice")
        .expect("the model publishes");
    match &model.type_source {
        SourcePosition::Present(SemanticTypeSource::Authored(
            AuthoredBodyLocator::MacroPayload(payload),
        )) => assert_eq!(
            payload.macro_index, 0,
            "the model type source anchors at the defineModel macro"
        ),
        other => panic!("the model publishes the resolved row's authored source, got {other:?}"),
    }
    let prop = meta
        .props
        .iter()
        .find(|prop| prop.name == "choice")
        .expect("the model prop publishes");
    assert_eq!(
        prop.publication.authority().provenance(),
        verter_type_expr::ResolutionProvenance::FrameworkSurface,
        "the model prop publishes the framework-surface row, not the analyzer field"
    );
    assert_eq!(
        prop.publication.authority().source_position(),
        model.type_source,
        "the model prop and the model share one type source"
    );
    assert!(
        meta.events
            .iter()
            .any(|event| event.name == "update:choice"),
        "the model update event publishes"
    );

    let demanded = demand_published_type(
        project.host(),
        "/App.vue",
        model.type_source.present(),
        "model value type",
    );
    match &demanded {
        TypeExpr::Object(shape) => assert!(
            shape.properties.iter().any(|member| matches!(
                member,
                ObjectMember::Property(property)
                    if property.string_name().expect("string-key fixture") == "id"
            )),
            "the demanded model type expands to the imported members, got {demanded:?}"
        ),
        TypeExpr::Ref { name, .. } if name.as_ref() == "Choice" => {}
        other => panic!("the demanded model type resolves the imported `Choice`, got {other:?}"),
    }
}
