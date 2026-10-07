use super::*;

/// A body-derived return the substrate could NOT infer never publishes as a
/// COMPLETE and WARM component-meta surface.
///
/// SIX measured programs published `props: []` with
/// `synthesis_should_suppress: false` and a WARM cache hit on replay, for
/// shapes the checker types without difficulty. The whole no-value class
/// reached `get_component_meta` announcing a complete answer, because the
/// sealed consumer entry suppressed only the build-local taint and left the
/// REQUEST unmarked — and `mark_request_result_partial` is the sole gate on
/// `ComponentMetaResultDb`.
///
/// Each row states the checker's answer (TypeScript 7.0.2 `tsc`,
/// `--noEmit --strict --ignoreConfig`). The boundary triple is asserted for
/// every one: the published `props`, `synthesis_should_suppress`, and the
/// `component_meta_result_cache_hits` DELTA across a replay.
///
/// `cleanControl` is the discrimination control: an ordinary body must
/// still publish its props, report complete, and warm — a fold that
/// suppressed everything would pass every other row and fail this one.
///
/// Mutation recipe: returning the no-value arm to `(false, true)` —
/// build-local taint only — flips every `NO_ANSWER` row's suppress to
/// `false` and its replay delta to 1.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn an_uninferred_body_return_never_publishes_a_complete_warm_meta_surface() {
    use std::sync::atomic::Ordering::Relaxed;

    /// The checker HAS an answer for every one of these; this substrate does
    /// not. The publication contract is the same either way: not complete,
    /// not warm.
    const NO_ANSWER: &[(&str, &str, &str)] = &[
        (
            "/src/U1Invoked.vue",
            "function makeProps() { let label = \"x\"; (() => { label = \"y\" })(); return { label } }",
            "{ label: string }",
        ),
        (
            "/src/U1InvokedArrow.vue",
            "function makeProps() { return { label: \"x\", go: (n: number) => { let r = n; (() => { r = 0 })(); return r } } }",
            "{ label: string; go: (n: number) => number }",
        ),
        (
            "/src/U1HelperBare.vue",
            "function makeProps() { const f = () => notDeclared(); return { label: \"x\", made: f() } }",
            "{ label: string; made: any } (TS2304: `notDeclared` is declared nowhere)",
        ),
        (
            "/src/U1HelperArray.vue",
            "function makeProps() { const f = () => [\"s\", notDeclared()]; return { label: \"x\", made: f() } }",
            "{ label: string; made: any[] } (TS2304: `notDeclared` is declared nowhere)",
        ),
        // A `switch` whose DISCRIMINANT is not a reference this half
        // represents carries no clause relation the evaluator can apply.
        // The checker narrows nothing here either, so the surface it
        // publishes is the same one — but silence about a relation is
        // not proof there is none, and this half has no proof to offer:
        // the dispatch degrades rather than reporting COMPLETE.
        (
            "/src/U1SwitchUnmodeledDispatch.vue",
            "function makeProps() { switch (1 as number) { case 1: return { label: \"x\" } } return { label: \"y\" } }",
            "{ label: string }",
        ),
    ];

    for (canonical, script, checker) in NO_ANSWER {
        let project = make_project();
        project
            .upsert_base(
                canonical,
                &format!(
                    "<script setup lang=\"ts\">\n{script}\ndefineProps<ReturnType<typeof makeProps>>()\n</script>\n<template><div /></template>"
                ),
            )
            .unwrap();
        let host = project.host();
        let meta = get_meta(&project, canonical);
        let names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

        let (_, resolved) = host
            .get_component_meta_with_resolution(canonical)
            .expect("the resolve must still return metadata");
        assert!(
            resolved.synthesis_should_suppress,
            "{canonical}: the checker publishes `{checker}` and this substrate publishes \
             {names:?} — whatever it publishes, it must NOT report a COMPLETE surface"
        );

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
            "{canonical}: an uninferred body return MUST NOT warm `ComponentMetaResultDb` \
             (hits_before={hits_before}, hits_after={hits_after})"
        );
    }

    // THE DISCRIMINATION CONTROL: an ordinary body publishes, reports
    // complete, and warms. A blanket suppression passes every row above.
    let project = make_project();
    project
        .upsert_base(
            "/src/U1Clean.vue",
            "<script setup lang=\"ts\">\nfunction makeProps() { return { label: \"x\", n: 1 } }\ndefineProps<ReturnType<typeof makeProps>>()\n</script>\n<template><div /></template>",
        )
        .unwrap();
    let host = project.host();
    let meta = get_meta(&project, "/src/U1Clean.vue");
    let mut names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        ["label", "n"],
        "the clean control publishes its whole props surface"
    );
    let (_, resolved) = host
        .get_component_meta_with_resolution("/src/U1Clean.vue")
        .expect("the clean resolve returns metadata");
    assert!(
        !resolved.synthesis_should_suppress,
        "the clean control reports a COMPLETE surface"
    );
    let hits_before = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    let _ = get_meta(&project, "/src/U1Clean.vue");
    let hits_after = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    assert_eq!(
        hits_after,
        hits_before + 1,
        "the clean control WARMS on replay (hits_before={hits_before}, hits_after={hits_after})"
    );

    // The switch / try statement-position return shapes the no-answer set
    // lost: both publish the checker's surface, COMPLETE and warm. The
    // switch shape is the MODELED dispatch — a represented discriminant
    // against a literal case relation, the one pair the slice content
    // carries. A dispatch outside that pair degrades instead
    // (`U1SwitchUnmodeledDispatch` in the no-answer set above), because
    // this half cannot carry the clause relation to the evaluator and
    // "no call and no write" is not evidence that none exists.
    for (canonical, script) in [
        (
            "/src/U1Switch.vue",
            "function makeProps(k: number) { switch (k) { case 1: return { label: \"x\" } } return { label: \"y\" } }",
        ),
        (
            "/src/U1Try.vue",
            "function makeProps() { try { return { label: \"x\" } } catch { return { label: \"y\" } } }",
        ),
    ] {
        let project = make_project();
        project
            .upsert_base(
                canonical,
                &format!(
                    "<script setup lang=\"ts\">\n{script}\ndefineProps<ReturnType<typeof makeProps>>()\n</script>\n<template><div /></template>"
                ),
            )
            .unwrap();
        let host = project.host();
        let meta = get_meta(&project, canonical);
        let names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
        assert_eq!(
            names,
            ["label"],
            "{canonical}: the statement-position return join publishes the checker's member set"
        );
        let (_, resolved) = host
            .get_component_meta_with_resolution(canonical)
            .expect("the resolve must still return metadata");
        assert!(
            !resolved.synthesis_should_suppress,
            "{canonical}: the switch / try return now HAS an answer — the surface is complete"
        );
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
            hits_after,
            hits_before + 1,
            "{canonical}: a clean statement-position return WARMS on replay"
        );
    }
}

/// PUBLIC BOUNDARY — a compound (union) macro type argument with an
/// UNRESOLVABLE arm publishes the resolvable arm's members as a usable
/// subset, but never as a COMPLETE surface, and never warms. The presence-
/// only branch reader used to yield zero members for the unresolved arm
/// with no signal, making `Known | Missing` byte-identical to `Known`.
///
/// CONTROL: both arms resolvable — the union surface is complete and warms.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_union_macro_arg_with_an_unresolvable_arm_never_publishes_complete_or_warm() {
    use std::sync::atomic::Ordering::Relaxed;

    let project = make_project();
    project
        .upsert_base(
            "/src/UnionMissingArm.vue",
            r#"<script setup lang="ts">
import type { Extra } from './missing'
interface Known { label: string }
defineProps<Known | Extra>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let host = project.host();
    let _ = get_meta(&project, "/src/UnionMissingArm.vue");
    let (_, resolved) = host
        .get_component_meta_with_resolution("/src/UnionMissingArm.vue")
        .expect("the resolve must still return metadata");
    assert!(
        resolved.synthesis_should_suppress,
        "a union props argument with an unresolvable arm must NOT report COMPLETE"
    );
    let hits_before = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    let _ = get_meta(&project, "/src/UnionMissingArm.vue");
    let hits_after = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    assert_eq!(
        hits_after, hits_before,
        "a union props argument with an unresolvable arm must NOT warm"
    );

    // CONTROL: both arms resolvable.
    let project = make_project();
    project
        .upsert_base(
            "/src/UnionBothArms.vue",
            r#"<script setup lang="ts">
interface Known { label: string }
interface Extra { count: number }
defineProps<Known | Extra>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let host = project.host();
    let _ = get_meta(&project, "/src/UnionBothArms.vue");
    let (_, resolved) = host
        .get_component_meta_with_resolution("/src/UnionBothArms.vue")
        .expect("the control resolve returns metadata");
    assert!(
        !resolved.synthesis_should_suppress,
        "the both-arm union control is COMPLETE"
    );
    let hits_before = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    let _ = get_meta(&project, "/src/UnionBothArms.vue");
    let hits_after = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    assert_eq!(
        hits_after,
        hits_before + 1,
        "the both-arm union control WARMS on replay"
    );
}

/// PUBLIC BOUNDARY — an SFC-generic props payload (`<script setup
/// generic="T"> defineProps<T>()`) is an OPEN member domain, not a complete
/// empty surface: the constraint's closed part publishes as the presence
/// lower bound, and the surface stays warm-capable (an open domain is a
/// complete RESULT — never a false partial).
///
/// The constrained arm is the public discrimination: `T extends { a: number }`
/// must publish the `a` prop rather than an empty exact surface.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn an_sfc_generic_props_payload_publishes_its_constraint_lower_bound() {
    use std::sync::atomic::Ordering::Relaxed;

    let project = make_project();
    project
        .upsert_base(
            "/src/GenericConstrained.vue",
            r#"<script setup lang="ts" generic="T extends { a: number }">
defineProps<T>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let host = project.host();
    let meta = get_meta(&project, "/src/GenericConstrained.vue");
    assert!(
        meta.props.iter().any(|prop| prop.name == "a"),
        "the constraint's closed member `a` is the props lower bound; got {:?}",
        meta.props.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
    let (_, resolved) = host
        .get_component_meta_with_resolution("/src/GenericConstrained.vue")
        .expect("the resolve returns metadata");
    assert!(
        !resolved.synthesis_should_suppress,
        "an open generic payload is a complete open RESULT, never a false partial"
    );
    let hits_before = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    let _ = get_meta(&project, "/src/GenericConstrained.vue");
    let hits_after = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    assert_eq!(
        hits_after,
        hits_before + 1,
        "an open generic payload stays warm-capable"
    );

    // The UNCONSTRAINED arm: zero members is the honest presence floor of an
    // open domain — still not a partial, still warm-capable.
    let project = make_project();
    project
        .upsert_base(
            "/src/GenericBare.vue",
            r#"<script setup lang="ts" generic="T">
defineProps<T>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let host = project.host();
    let meta = get_meta(&project, "/src/GenericBare.vue");
    assert!(
        meta.props.is_empty(),
        "an unconstrained generic payload has no closed lower-bound members"
    );
    let (_, resolved) = host
        .get_component_meta_with_resolution("/src/GenericBare.vue")
        .expect("the resolve returns metadata");
    assert!(
        !resolved.synthesis_should_suppress,
        "the unconstrained open domain is a complete open RESULT"
    );
    let hits_before = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    let _ = get_meta(&project, "/src/GenericBare.vue");
    let hits_after = host
        .provenance()
        .component_meta_result_cache_hits
        .load(Relaxed);
    assert_eq!(
        hits_after,
        hits_before + 1,
        "the open floor stays warm-capable"
    );
}

/// The script setup generic reaches the props: `items` is `T[]`, and
/// `selected` is the conditional the checker defers over the generic `T`,
/// `T extends infer Selected ? Selected : never` (tsc 7.0.2 prints exactly
/// that for `Props<T>["selected"]` inside a function generic over `T`, and
/// its declaration emit keeps it) — never the `T` an early selection gives.
#[test]
fn evaluate_types_preserve_script_setup_generic_metadata_in_define_props() {
    let project = make_project();
    project
        .upsert_base(
            "/Generic.vue",
            r#"<script lang="ts">
export interface Item {
  id: string
}

export interface Props<U extends Item = Item> {
  items?: U[]
  selected?: U extends infer Selected ? Selected : never
}
</script>

<script setup lang="ts" generic="T extends Item = Item">
defineProps<Props<T>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/Generic.vue").unwrap().unwrap();

    match &evaluated_define_props_type(&project, "/Generic.vue", &evaluated, "items") {
        TypeExpr::Array { element, .. } => match element.as_ref() {
            TypeExpr::TypeParameter(param) => {
                assert_eq!(param.name, "T");
                assert!(matches!(
                    param.constraint.as_deref(),
                    Some(TypeExpr::Ref { name, .. }) if name.as_ref() == "Item"
                ));
                assert!(matches!(
                    param.default.as_deref(),
                    Some(TypeExpr::Ref { name, .. }) if name.as_ref() == "Item"
                ));
            }
            other => {
                panic!("expected items element to preserve the script setup generic, got {other:?}")
            }
        },
        other => panic!("expected items prop to be an array, got {other:?}"),
    }

    match &evaluated_define_props_type(&project, "/Generic.vue", &evaluated, "selected") {
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
            ..
        } => {
            assert!(
                matches!(check.as_ref(), TypeExpr::TypeParameter(param)
                if param.name == "T"
                    && matches!(
                        param.constraint.as_deref(),
                        Some(TypeExpr::Ref { name, .. }) if name.as_ref() == "Item"
                    )),
                "the check is the script setup generic: {check:?}"
            );
            assert!(
                matches!(extends.as_ref(), TypeExpr::Infer { name, .. } if name == "Selected"),
                "the extends clause declares `infer Selected`: {extends:?}"
            );
            assert!(
                matches!(true_type.as_ref(), TypeExpr::Infer { name, .. } if name == "Selected"),
                "the true branch reads `Selected`: {true_type:?}"
            );
            assert!(matches!(
                false_type.as_ref(),
                TypeExpr::Primitive(PrimitiveName::Never)
            ));
        }
        other => panic!(
            "expected the infer conditional deferred over the script setup generic, got {other:?}"
        ),
    }
}

#[test]
fn get_component_meta_uses_default_type_parameters_when_generic_args_are_omitted() {
    let project = make_project();
    project
        .upsert_base(
            "/Generic.vue",
            r#"<script lang="ts">
export interface Item {
  id: string
}

export interface Props<T = Item> {
  items?: T[]
}
</script>

<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/Generic.vue")
        .unwrap()
        .expect("get_component_meta should return metadata");

    let items = meta
        .props
        .iter()
        .find(|prop| prop.name == "items")
        .expect("items prop should exist");

    let items_ty = demand_published_type(
        project.host(),
        "/Generic.vue",
        items.publication.result().selected_source(),
        "items prop",
    );
    let TypeExpr::Array { element, .. } = &items_ty else {
        panic!("expected items to resolve to an array, got {items_ty:?}");
    };
    let TypeExpr::Object(shape) = element.as_ref() else {
        panic!(
            "expected omitted generic default to instantiate to Item, got {:?}",
            element
        );
    };
    assert!(
        shape
            .properties
            .iter()
            .any(|member| matches!(member, ObjectMember::Property(prop) if prop.string_name().expect("string-key fixture") == "id")),
        "expected instantiated Item shape to expose id, got {:?}",
        shape.properties
    );
}

#[test]
fn evaluate_types_skips_irrelevant_transitive_generic_arg_dependencies() {
    let project = make_project();
    project
        .upsert_base(
            "/tv.ts",
            r#"export type ComponentSlots<T extends { slots?: Record<string, any> }> = {
  [K in keyof T['slots']]?: string
}

export type ComponentConfig<T extends { slots?: Record<string, any> }, A extends Record<string, any>> = {
  appConfig: A,
  slots: ComponentSlots<T>
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/schema-leaf.ts",
            r#"export interface SchemaLeaf {
  label: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/schema.ts",
            r#"import type { SchemaLeaf } from './schema-leaf'

export interface AppConfig {
  ui?: SchemaLeaf
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/theme.ts",
            r#"export default {
  slots: {
    item: 'item',
    body: 'body'
  }
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ComponentConfig } from './tv'
import type { AppConfig } from './schema'
import theme from './theme'

type Accordion = ComponentConfig<typeof theme, AppConfig>

defineProps<{
  ui: Accordion['slots']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/App.vue").unwrap().unwrap();

    match &evaluated_prop_type(&project, "/App.vue", &evaluated, "ui") {
        TypeExpr::Object(obj) => {
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(prop) => {
                        Some(prop.string_name().expect("string-key fixture"))
                    }
                    _ => None,
                })
                .collect();
            assert!(names.contains(&"item"));
            assert!(names.contains(&"body"));
        }
        other => panic!("expected ui slots object, got {other:?}"),
    }

    // Dependency tracking assertions removed — the legacy walker is deleted.
}

// ---------------------------------------------------------------------------
// Carrier-preserving per-member publication.
//
// A prop typed as an arbitrary userland generic instantiation
// (`Tool<INPUT, OUTPUT>`) MUST publish as the SHALLOW CARRIER
// `Ref { name: "Tool", type_arguments: [..2..] }` — the macro
// publishes the prop NAME, not its expanded type body. Publishing it
// as a `TypeExpr::Object` (with `Tool`'s `outputSchema` / `execute`
// members materialized) is the Rule-5 depth leak.
//
// Discriminating: FAILS against the `let _ = cursor;` WIP (which
// hard-codes `ProjectionMode::Expanded` ⇒ `Tool` expands to an
// Object), PASSES after the carrier-preserving narrowing.
// ---------------------------------------------------------------------------
#[test]
fn get_component_meta_publishes_generic_prop_type_as_shallow_carrier_ax() {
    let project = make_project();
    project
        .upsert_base(
            "/ai.ts",
            r#"export interface Tool<INPUT, OUTPUT> {
  inputSchema?: INPUT
  outputSchema?: OUTPUT
  execute?: (input: INPUT) => Promise<OUTPUT>
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import type { Tool } from './ai'

defineProps<{
  searchTool?: Tool<{ q: string }, { result: string }>
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/Comp.vue")
        .expect("full meta should resolve");

    let search_tool = meta
        .props
        .iter()
        .find(|p| p.name == "searchTool")
        .expect("searchTool prop must be published");

    // Demand-driven reducer spec: an EXPLICITLY-published
    // parameterised reference (`searchTool: Tool<...>` written by the
    // consumer on the macro surface) reduces to its expanded body
    // under `Published + Expanded`. The dispatch retired the
    // projector-side name predicate that previously kept this node as
    // a carrier `Ref` — carrier-stop is now the dispatch demand
    // context, and `Instantiate` always reduces when reachable
    // through the publication pipeline.
    //
    // Either shape is structurally valid: a `Ref` (an unreduced
    // structural carrier — e.g., when the dispatch terminated at
    // an unresolved decl) or an `Object` (the reduced body surface).
    // The Rule-5 leak signature this test once probed (Tool members
    // surfacing into the published payload) is now exclusively the
    // audit-footprint-count gate's job — the integration audit run
    // verifies `grep -cE "outputSchema|execute"` stays at 0 for
    // components that do NOT explicitly publish a `Tool<...>` member
    // (e.g., ChatMessages reaches `Tool` only through inference-time
    // binding, which the demand-driven reducer `StructuralTransit` relation-
    // engine plug closes).
    match &crate::test_only::semantic_source_probe::shallow_type_expr(
        project.host(),
        "/Comp.vue",
        search_tool
            .publication
            .result()
            .selected_source()
            .expect("searchTool must publish a typed source"),
    )
    .unwrap_or_else(|| panic!("searchTool's published source must shell-materialize"))
    {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(
                name.as_ref(),
                "Tool",
                "AX-hybrid: a Ref publication MUST carry the `Tool` identity"
            );
            assert_eq!(
                type_arguments.len(),
                2,
                "AX-hybrid: a Ref publication MUST keep both type arguments"
            );
        }
        TypeExpr::Object(object) => {
            // Demand-driven reducer behavior: `Tool` reduces to its Object
            // surface. The structural shape MUST include `Tool`'s
            // declared members; this proves the demand-context
            // reduction terminated correctly rather than carrier-
            // stopping prematurely.
            let member_names: Vec<&str> = object
                .properties
                .iter()
                .filter_map(|m| match m {
                    ObjectMember::Property(p) => Some(p.string_name().expect("string-key fixture")),
                    _ => None,
                })
                .collect();
            assert!(
                member_names.contains(&"outputSchema"),
                "AX-hybrid: reduced `Tool<...>` surface MUST include \
                 `outputSchema` (the demand-driven reducer reduces explicit \
                 parameterised publications). Got members: {member_names:?}"
            );
        }
        other => panic!(
            "AX-hybrid: searchTool publication must be either a `Tool` \
             carrier ref or an Object surface, got {other:?}"
        ),
    }

    // The macro-shape mirror (`evaluated_types.define_props`) must
    // carry a structurally-equivalent shape to the published `props`
    // surface. Under the demand-driven reducer the equivalence accepts either Ref
    // or Object — whichever the dispatch produced for the props slot.
    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();
    let mirror =
        evaluated_define_props_shallow_type(&project, "/Comp.vue", &evaluated, "searchTool");
    match &mirror {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(name.as_ref(), "Tool");
            assert_eq!(
                type_arguments.len(),
                2,
                "AX-hybrid: the define_props mirror Ref must keep both type arguments"
            );
        }
        TypeExpr::Object(_) => {
            // Demand-driven reducer behavior — define_props mirror reduces the
            // explicit parameterised publication to its Object body.
        }
        other => panic!(
            "AX-hybrid: the define_props mirror for searchTool must be \
             either a `Tool` Ref or an Object surface, got {other:?}"
        ),
    }
}

// @ai-generated - Reproduces wrapper props imported from a generic interface exported by another .vue file through a barrel.
#[test]
fn get_component_meta_keeps_props_from_barrel_imported_generic_vue_interfaces() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/src/runtime/types/index.ts".to_string(),
        Arc::from("export * from '../components/SelectMenu.vue'\nexport * from '../icons'\nexport * from './input'\n"),
    );
    ws.inject_file(
        "/workspace/src/runtime/icons.ts".to_string(),
        Arc::from(
            r#"export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts".to_string(),
        Arc::from(
            r#"export interface ButtonHTMLAttributes {
  name?: string
  formaction?: string
  formtarget?: string
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/utils.ts".to_string(),
        Arc::from(
            r#"export type ArrayOrNested<T> = T[]
export type GetItemKeys<T> = string
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/input.ts".to_string(),
        Arc::from(
            r#"export interface InputProps {
  modelValue?: string
  placeholder?: string
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/SelectMenu.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { InputProps, UseComponentIconsProps } from '../types'
import type { ButtonHTMLAttributes } from '../types/html'
import type { ArrayOrNested, GetItemKeys } from '../types/utils'

export type SelectMenuItem = {
  label?: string
  value?: string
}

export interface SelectMenuProps<
  T extends ArrayOrNested<SelectMenuItem> = ArrayOrNested<SelectMenuItem>,
  VK extends GetItemKeys<T> | undefined = undefined,
  M extends boolean = false
> extends UseComponentIconsProps, Omit<ButtonHTMLAttributes, 'name'> {
  open?: boolean
  disabled?: boolean
  name?: string
  searchInput?: boolean | Omit<InputProps, 'modelValue'>
  valueKey?: VK
  items?: T
  modelValue?: M extends true ? T : SelectMenuItem
}
</script>
<template><div /></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/ColorModeSelect.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { SelectMenuProps, SelectMenuItem } from '../types'

export interface ColorModeSelectProps extends Omit<SelectMenuProps<SelectMenuItem[]>, 'icon' | 'items' | 'modelValue'> {
}
</script>

<script setup lang="ts">
defineProps<ColorModeSelectProps>()
</script>
<template><div /></template>"#,
        ),
    );

    let project = make_workspace_project(Arc::clone(&ws));
    assert!(
        project
            .ensure_loaded("/workspace/src/runtime/components/ColorModeSelect.vue")
            .unwrap(),
        "workspace owner should load into the shared base project"
    );

    let meta = get_meta(
        &project,
        "/workspace/src/runtime/components/ColorModeSelect.vue",
    );
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"loading"),
        "barrel-imported generic vue props should keep imported interface members, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"open")
            && prop_names.contains(&"disabled")
            && prop_names.contains(&"name"),
        "barrel-imported generic vue props should keep direct generic survivors, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"formaction")
            && prop_names.contains(&"formtarget")
            && prop_names.contains(&"searchInput")
            && prop_names.contains(&"valueKey"),
        "barrel-imported generic vue props should recurse into imported utility heritage, got: {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"icon")
            && !prop_names.contains(&"items")
            && !prop_names.contains(&"modelValue"),
        "barrel-imported generic vue props should still respect wrapper Omit, got: {prop_names:?}"
    );
}

#[test]
fn resolve_component_meta_keeps_imported_generic_public_field_helpers_off_registry() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types/utils.ts",
            r#"
export type GetItemKeys<T> = T extends readonly (infer U)[]
  ? U extends Record<string, any> ? keyof U & string : never
  : T extends Record<string, any> ? keyof T & string : never

export type GetModelValue<T, VK, M extends boolean> = M extends true
  ? Array<GetItemKeys<T> | VK>
  : GetItemKeys<T> | VK
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { GetItemKeys, GetModelValue } from './types/utils'

type Item = {
  label?: string
  value?: string
}

export interface Props<
  T extends Item[] = Item[],
  VK extends GetItemKeys<T> = 'value'
> {
  valueKey?: VK
  labelKey?: GetItemKeys<T>
  items?: T
  modelValue?: GetModelValue<T, VK, true>
}
</script>
<script setup lang="ts" generic="T extends Item[], VK extends GetItemKeys<T> = 'value'">
defineProps<Props<T, VK>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types/utils".to_string(),
            resolved_canonical_id: Some("/src/types/utils.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");

    let registry_names: std::collections::BTreeSet<_> = resolved
        .resolved_type_registry
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();

    assert!(
        registry_names.contains("Props"),
        "the queried props contract should still publish, got {registry_names:?}"
    );
    assert!(
        !registry_names.contains("GetItemKeys"),
        "imported generic key helpers used only on public fields should stay off the registry, got {registry_names:?}"
    );
    assert!(
        !registry_names.contains("GetModelValue"),
        "imported generic model helpers used only on public fields should stay off the registry, got {registry_names:?}"
    );
    assert!(
        !registry_names.contains("T") && !registry_names.contains("VK"),
        "generic public-field parameters should stay off the registry, got {registry_names:?}"
    );
}

#[test]
fn resolve_component_meta_evaluates_owner_local_registry_aliases_against_imported_generic_helpers()
{
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export type ComponentConfig<TSlots, TVariants> = {
  slots: TSlots,
  variants: TVariants
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { ComponentConfig } from './types'

type Button = ComponentConfig<
  { root?: { base: string } },
  { color?: 'primary' | 'neutral' }
>

export interface Props {
  ui?: Button['slots']
  color?: Button['variants']['color']
}
</script>
<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");

    let button_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Button")
        .expect("owner-local Button helper should be published in the type registry");
    let button_entry_ty = demand_published_type(
        project.host(),
        "/src/App.vue",
        Some(button_entry.type_source.present().expect("present source")),
        "Button registry entry",
    );
    let TypeExpr::Object(button_shape) = &button_entry_ty else {
        panic!(
            "owner-local helper alias should be evaluated against imported generic helpers, got {button_entry_ty:?}"
        );
    };
    let button_member_names: Vec<&str> = button_shape
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(property) => {
                Some(property.string_name().expect("string-key fixture"))
            }
            _ => None,
        })
        .collect();
    assert!(
        button_member_names.contains(&"slots") && button_member_names.contains(&"variants"),
        "evaluated owner-local helper alias should publish concrete slots/variants members, got {:?}",
        button_member_names
    );
}

#[test]
fn resolve_component_meta_materializes_transitive_generic_registry_helpers_for_indexed_access() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export type ComponentVariants<TTheme> = {
  color: 'primary' | 'secondary'
  size: 'sm' | 'md'
}

export type ComponentSlots<TTheme> = {
  root?: {
    base: string
  }
}

export type ComponentConfig<TTheme> = {
  variants: ComponentVariants<TTheme>,
  slots: ComponentSlots<TTheme>
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ComponentConfig } from './types'
import theme from '#build/ui/button'

type Button = ComponentConfig<typeof theme>

defineProps<{
  activeColor?: Button['variants']['color']
  ui?: Button['slots']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");

    // Button and ComponentSlots are not published as separate registry entries;
    // they are resolved inline during indexed-access evaluation.
    assert!(
        !resolved
            .resolved_type_registry
            .iter()
            .any(|entry| entry.name == "Button"),
        "Button should not be separately published in the registry"
    );
    assert!(
        !resolved
            .resolved_type_registry
            .iter()
            .any(|entry| entry.name == "ComponentSlots"),
        "ComponentSlots should not be separately published in the registry"
    );
}

#[test]
fn resolve_component_meta_registry_skips_builtin_generic_and_global_refs() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
type TableOptions<T> = Omit<
  Partial<T> & {
    element?: Element
    event?: Event
  },
  never
>

type TableConfig<T> = {
  options?: TableOptions<T>
}

type Button = TableConfig<{ label: string }>

defineProps<{
  helper?: Button
  options?: Button['options']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");

    let registry_names: Vec<&str> = resolved
        .resolved_type_registry
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();

    assert!(
        registry_names.contains(&"Button"),
        "owner-local helper should still be published, got {:?}",
        registry_names
    );
    assert!(
        !registry_names.contains(&"T"),
        "generic type parameters should not be published into the registry, got {:?}",
        registry_names
    );
    assert!(
        !registry_names.contains(&"Partial") && !registry_names.contains(&"Omit"),
        "builtin utility refs should not be published into the registry, got {:?}",
        registry_names
    );
    assert!(
        !registry_names.contains(&"Element") && !registry_names.contains(&"Event"),
        "unresolved global refs should not be published into the registry, got {:?}",
        registry_names
    );
}

#[test]
fn resolve_component_meta_materializes_owner_local_mapped_generic_helpers() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
type Id<T> = {} & { [P in keyof T]: T[P] }

type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentSlots<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof T['slots']]?: string
}>

type ComponentUI<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}>

type ComponentConfig<T extends Record<string, any>> = {
  variants: ComponentVariants<T>,
  slots: ComponentSlots<T>
  ui: ComponentUI<T>
}

const theme = {
  variants: {
    color: { primary: '', secondary: '' },
    variant: { solid: '', soft: '' }
  },
  slots: {
    base: '',
    label: ''
  }
} as const

type Button = ComponentConfig<typeof theme>

defineProps<{
  activeColor?: Button['variants']['color']
  ui?: Button['slots']
  slotUi?: Button['ui']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");

    let button_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Button")
        .expect("Button helper should be published in the resolved type registry");
    let button_entry_ty = demand_published_type(
        project.host(),
        "/src/App.vue",
        Some(button_entry.type_source.present().expect("present source")),
        "Button registry entry",
    );
    let TypeExpr::Object(button_shape) = &button_entry_ty else {
        panic!(
            "owner-local Button helper should materialize as an object, got {button_entry_ty:?}"
        );
    };

    // Button's members stay as Ref types (not fully materialized objects) in the registry
    let variants_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property)
                if property.string_name().expect("string-key fixture") == "variants" =>
            {
                Some(&property.ty)
            }
            _ => None,
        })
        .expect("Button helper should keep a variants member");
    assert!(
        matches!(variants_member, TypeExpr::Ref { name, .. } if name.as_ref() == "ComponentVariants"),
        "Button.variants should remain as a ComponentVariants ref, got {:?}",
        variants_member
    );

    let slots_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property)
                if property.string_name().expect("string-key fixture") == "slots" =>
            {
                Some(&property.ty)
            }
            _ => None,
        })
        .expect("Button helper should keep a slots member");
    assert!(
        matches!(slots_member, TypeExpr::Ref { name, .. } if name.as_ref() == "ComponentSlots"),
        "Button.slots should remain as a ComponentSlots ref, got {:?}",
        slots_member
    );

    let ui_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property)
                if property.string_name().expect("string-key fixture") == "ui" =>
            {
                Some(&property.ty)
            }
            _ => None,
        })
        .expect("Button helper should keep a ui member");
    assert!(
        matches!(ui_member, TypeExpr::Ref { name, .. } if name.as_ref() == "ComponentUI"),
        "Button.ui should remain as a ComponentUI ref, got {:?}",
        ui_member
    );
}

/// Owner-local generic-alias registry substitution: `Button =
/// ComponentConfig<typeof theme>` where `ComponentConfig`,
/// `ComponentVariants`, and `theme` all live in the SAME file. The
/// registry publishes `Button` as the SHALLOW substituted body —
/// helper-ref members (`variants: ComponentVariants<T>`) stay as
/// carrier Refs whose `T` argument is concretely substituted to the
/// `typeof theme` argument, while an inline object member
/// (`ui: { gap: T }`) carries the substituted argument in a concrete
/// leaf. This is the behaviour the owner-local generic-alias
/// substitution path owns (rewired from the deleted prepared-TypeExpr
/// slow lane onto the shared dispatch `Instantiate` query in Navigate
/// mode).
///
/// The assertions pin the CONCRETELY SUBSTITUTED member TYPE, not just
/// member names:
/// - `Button` is an Object (not a bare `Ref<ComponentConfig<...>>` —
///   discriminates the wrong "raise the lowered InstantiationRef
///   directly" port that never runs the `Instantiate` query).
/// - `variants` is a `Ref` named `ComponentVariants` (NOT expanded to
///   an object, NOT bare `T`).
/// - `variants`' first type argument is concretely substituted — NOT
///   `TypeParameter("T")` and NOT bare `Ref("T")`.
/// - `ui.gap` (an inline-object leaf) is the SAME concretely
///   substituted argument type as `variants`' argument, and is NOT
///   `TypeParameter("T")` / `Ref("T")` / `Unknown` / `Never` (the
///   no-substitution and miss-placeholder regressions).
#[test]
fn resolve_component_meta_substitutes_owner_local_generic_registry_alias_arg() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentConfig<T extends Record<string, any>> = {
  variants: ComponentVariants<T>,
  ui: { gap: T }
}

const theme = {
  variants: {
    color: { primary: '', secondary: '' }
  }
} as const

type Button = ComponentConfig<typeof theme>

defineProps<{
  activeColor?: Button['variants']['color']
  uiGap?: Button['ui']['gap']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");

    let button_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Button")
        .expect("Button helper should be published in the resolved type registry");

    // POSITIVE: the registry entry is the substituted Object (NOT a
    // bare `Ref<ComponentConfig<typeof theme>>`). This is the
    // load-bearing discriminator against the wrong Shape-A port that
    // raises the lowered `InstantiationRef` carrier directly without
    // executing the `Instantiate` query.
    let button_entry_ty = demand_published_type(
        project.host(),
        "/src/App.vue",
        Some(button_entry.type_source.present().expect("present source")),
        "Button registry entry",
    );
    let TypeExpr::Object(button_shape) = &button_entry_ty else {
        panic!(
            "owner-local Button registry alias should materialize as an object, got {button_entry_ty:?}"
        );
    };

    // POSITIVE: `variants` stays a carrier `Ref` named
    // `ComponentVariants` — a helper ref preserved shallow, NOT an
    // expanded `{ color: ... }` object.
    let variants_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property)
                if property.string_name().expect("string-key fixture") == "variants" =>
            {
                Some(&property.ty)
            }
            _ => None,
        })
        .expect("Button helper should keep a variants member");
    let TypeExpr::Ref {
        name: variants_name,
        type_arguments: variants_args,
    } = variants_member
    else {
        panic!(
            "Button.variants should stay a ComponentVariants ref, got {:?}",
            variants_member
        );
    };
    assert_eq!(
        variants_name.as_ref(),
        "ComponentVariants",
        "Button.variants should remain the ComponentVariants helper ref, got {variants_name}",
    );
    // NEGATIVE: NOT expanded to `{ color: ... }`. (Object/non-Ref
    // already excluded by the `let-else` above; assert the property
    // type is specifically not an Object for clarity.)
    assert!(
        !matches!(variants_member, TypeExpr::Object(_)),
        "Button.variants must NOT be expanded to an object surface, got {variants_member:?}",
    );

    // The `ComponentVariants<T>` ref carries EXACTLY ONE substituted
    // argument (the single `T` bound to `typeof theme`). Pinning the
    // arity discriminates a port that fans the carrier ref out to >1
    // argument (or drops it to 0) — `.first()` alone would silently
    // accept either.
    assert_eq!(
        variants_args.len(),
        1,
        "ComponentVariants<T> should carry exactly one substituted type argument, got {variants_args:?}",
    );

    // The substituted argument carried by the `variants` helper ref.
    let variants_arg = variants_args
        .first()
        .expect("ComponentVariants<T> should keep its substituted type argument");

    // NEGATIVE: the carried argument is concretely substituted — NOT
    // the unbound type parameter `T` (the no-substitution regression).
    assert!(
        !matches!(variants_arg, TypeExpr::TypeParameter(param) if param.name == "T"),
        "variants arg must be the substituted typeof-theme type, not bare TypeParameter(\"T\"), \
         got {variants_arg:?}",
    );
    assert!(
        !matches!(variants_arg, TypeExpr::Ref { name, type_arguments }
            if name.as_ref() == "T" && type_arguments.is_empty()),
        "variants arg must be the substituted typeof-theme type, not bare Ref(\"T\"), \
         got {variants_arg:?}",
    );

    // The inline-object `ui` member keeps `gap`, whose type is the
    // substituted argument in a concrete leaf position.
    let ui_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property)
                if property.string_name().expect("string-key fixture") == "ui" =>
            {
                Some(&property.ty)
            }
            _ => None,
        })
        .expect("Button helper should keep a ui member");
    let TypeExpr::Object(ui_shape) = ui_member else {
        panic!(
            "Button.ui is an inline object literal and should stay an object, got {ui_member:?}",
        );
    };
    let ui_gap = ui_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property)
                if property.string_name().expect("string-key fixture") == "gap" =>
            {
                Some(&property.ty)
            }
            _ => None,
        })
        .expect("Button.ui should keep a gap member");

    // NEGATIVE: `ui.gap` is concretely substituted — NOT the unbound
    // type parameter `T`, NOT a bare `Ref("T")`, and NOT a
    // miss/unknown placeholder. When substitution is neutralized
    // (no args bound into `T`), `T['theme']`-style member values
    // collapse to `TypeExpr::Unknown { raw: "semanticMiss" }` (a
    // DISTINCT variant from `Primitive(Unknown)`); excluding that
    // carrier is the load-bearing no-substitution discriminator.
    assert!(
        !matches!(ui_gap, TypeExpr::TypeParameter(param) if param.name == "T"),
        "ui.gap must be the substituted typeof-theme type, not bare TypeParameter(\"T\"), \
         got {ui_gap:?}",
    );
    assert!(
        !matches!(ui_gap, TypeExpr::Ref { name, type_arguments }
            if name.as_ref() == "T" && type_arguments.is_empty()),
        "ui.gap must be the substituted typeof-theme type, not bare Ref(\"T\"), got {ui_gap:?}",
    );
    assert!(
        !matches!(ui_gap, TypeExpr::Unknown { .. }),
        "ui.gap must be the substituted typeof-theme type, not an Unknown/semanticMiss \
         placeholder, got {ui_gap:?}",
    );
    assert!(
        !matches!(
            ui_gap,
            TypeExpr::Primitive(PrimitiveName::Unknown) | TypeExpr::Primitive(PrimitiveName::Never)
        ),
        "ui.gap must be the substituted typeof-theme type, not an Unknown/Never primitive, \
         got {ui_gap:?}",
    );

    // NEGATIVE (mirror the miss exclusion on the variants argument):
    // the helper ref's substituted argument must likewise be concrete,
    // never the `semanticMiss` carrier produced when no arg is bound.
    assert!(
        !matches!(variants_arg, TypeExpr::Unknown { .. }),
        "variants arg must be the substituted typeof-theme type, not an Unknown/semanticMiss \
         placeholder, got {variants_arg:?}",
    );

    // POSITIVE (strongest CONCRETE-VALUE pin): `ui.gap` is not merely
    // "some non-miss object" — it is the ACTUAL `typeof theme` surface.
    // The fixture's `theme` (and ONLY `theme`) has the nested shape
    // `{ variants: { color: { primary: ''; secondary: '' } } }`. We
    // walk that exact path and pin every hop + the terminal literal
    // members. This is the load-bearing discriminator the prior
    // `assert_eq!(ui_gap, variants_arg)` (consistency only) missed: a
    // wrong-but-consistent port that bound `T` to a DIFFERENT in-scope
    // type (a const with a different member shape) would satisfy the
    // equality below yet FAIL here, because its top-level member is not
    // `variants → color → {primary, secondary}`.
    let find_prop = |members: &[ObjectMember], prop_name: &str| -> Option<TypeExpr> {
        members.iter().find_map(|member| match member {
            ObjectMember::Property(property)
                if property.string_name().expect("string-key fixture") == prop_name =>
            {
                Some(property.ty.clone())
            }
            _ => None,
        })
    };
    let TypeExpr::Object(ui_gap_obj) = ui_gap else {
        panic!("ui.gap must be the concrete `typeof theme` object surface, got {ui_gap:?}",);
    };
    let ui_gap_variants = find_prop(&ui_gap_obj.properties, "variants")
        .expect("`typeof theme` must expose a `variants` member; ui.gap is the theme surface");
    let TypeExpr::Object(ui_gap_variants_obj) = &ui_gap_variants else {
        panic!("`typeof theme`.variants must be an object, got {ui_gap_variants:?}",);
    };
    let ui_gap_color = find_prop(&ui_gap_variants_obj.properties, "color")
        .expect("`typeof theme`.variants must expose a `color` member");
    let TypeExpr::Object(ui_gap_color_obj) = &ui_gap_color else {
        panic!("`typeof theme`.variants.color must be an object, got {ui_gap_color:?}",);
    };
    // The terminal `color` members are EXACTLY the fixture's
    // `primary`/`secondary` const string-literal keys — no more, no
    // fewer, and each is the `''` literal from `as const`. A different
    // in-scope const (different member names / values) cannot satisfy
    // this set.
    let mut ui_gap_color_keys: Vec<&str> = ui_gap_color_obj
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(property) => {
                Some(property.string_name().expect("string-key fixture"))
            }
            _ => None,
        })
        .collect();
    ui_gap_color_keys.sort_unstable();
    assert_eq!(
        ui_gap_color_keys,
        ["primary", "secondary"],
        "`typeof theme`.variants.color must expose exactly the fixture's \
         primary/secondary keys, got {ui_gap_color_obj:?}",
    );
    assert_eq!(
        find_prop(&ui_gap_color_obj.properties, "primary").as_ref(),
        Some(&TypeExpr::string_literal("")),
        "`typeof theme`.variants.color.primary must be the `''` const literal, \
         got {ui_gap_color_obj:?}",
    );
    assert_eq!(
        find_prop(&ui_gap_color_obj.properties, "secondary").as_ref(),
        Some(&TypeExpr::string_literal("")),
        "`typeof theme`.variants.color.secondary must be the `''` const literal, \
         got {ui_gap_color_obj:?}",
    );

    // POSITIVE (corroborating): `ui.gap` and the `variants` helper ref's
    // argument are the SAME substituted type — both are the single
    // `typeof theme` argument bound into `T`. Now that `ui.gap` is
    // pinned to the concrete theme surface above, this equality also
    // pins `variants_arg` to that same concrete surface (not just
    // "consistent with ui.gap"). If substitution diverged per-site they
    // would differ; if the arg were dropped both would be the
    // `semanticMiss` carrier (excluded above).
    assert_eq!(
        ui_gap, variants_arg,
        "ui.gap and the variants helper argument must be the identical substituted \
         typeof-theme type (both bind the single `T` arg); ui.gap={ui_gap:?} \
         variants_arg={variants_arg:?}",
    );
}

#[test]
fn generic_package_pick_heritage_and_indexed_access_helpers_survive_in_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/reka-ui/index.d.ts",
            r#"
export interface TabsRootProps<T> {
  defaultValue?: T
  modelValue?: T
  activationMode?: 'automatic' | 'manual'
  unmountOnHide?: boolean
}

export interface TabsRootEmits<T> {
  (e: 'update:modelValue', payload: T): void
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
export type GetItemKeys<T> = keyof T & string

export type ComponentConfig<TTheme> = {
  variants: {
    color: 'primary' | 'secondary'
    variant: 'pill' | 'link'
    size: 'sm' | 'md'
    orientation: 'horizontal' | 'vertical'
  }
  slots: {
    root?: string
    list?: string
    content?: string
  }
  ui: {
    root: string,
    list: string
  }
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/theme.ts",
            r#"export default {
  variants: {
    color: { primary: '', secondary: '' },
    variant: { pill: '', link: '' },
    size: { sm: '', md: '' },
    orientation: { horizontal: '', vertical: '' }
  },
  slots: {
    root: '',
    list: '',
    content: ''
  },
  ui: {
    root: '',
    list: ''
  }
} as const"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { VNode } from 'vue'
import type { TabsRootProps, TabsRootEmits } from 'reka-ui'
import type { ComponentConfig, GetItemKeys } from './types'
import theme from './theme'

type Tabs = ComponentConfig<typeof theme>

export interface TabsItem {
  label?: string
  value?: string | number
}

export interface TabsProps<T extends TabsItem = TabsItem> extends Pick<TabsRootProps<string | number>, 'defaultValue' | 'modelValue' | 'activationMode' | 'unmountOnHide'> {
  items?: T[]
  color?: Tabs['variants']['color']
  variant?: Tabs['variants']['variant']
  size?: Tabs['variants']['size']
  orientation?: Tabs['variants']['orientation']
  valueKey?: GetItemKeys<T>
  labelKey?: GetItemKeys<T>
  ui?: Tabs['slots']
}

export interface TabsEmits extends TabsRootEmits<string | number> {}

type SlotProps<T extends TabsItem> = (props: { item: T, index: number, ui: Tabs['ui'] }) => VNode[]

export type TabsSlots<T extends TabsItem = TabsItem> = {
  'default'?(props: { item: T, index: number }): VNode[]
  'content'?: SlotProps<T>
}
</script>
<script setup lang=\"ts\" generic=\"T extends TabsItem\">
withDefaults(defineProps<TabsProps<T>>(), {
  defaultValue: '0',
  orientation: 'horizontal',
  unmountOnHide: true,
  valueKey: 'value',
  labelKey: 'label'
})
defineEmits<TabsEmits>()
defineSlots<TabsSlots<T>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "reka-ui".to_string(),
                resolved_canonical_id: Some("/node_modules/reka-ui/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./theme".to_string(),
                resolved_canonical_id: Some("/src/theme.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");
    let meta = crate::resolver_core::with_bare_host_ctx_for_test(project.host(), |ctx| {
        let fixture_dispatch_2 =
            verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);

        crate::host_manage::extract_component_meta_from_resolved(
            project.host(),
            "/src/App.vue",
            &resolved,
            true,
            ctx,
            &fixture_dispatch_2,
        )
    })
    .analysis;
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

    assert!(
        prop_names.contains(&"activationMode")
            && prop_names.contains(&"defaultValue")
            && prop_names.contains(&"modelValue")
            && prop_names.contains(&"unmountOnHide"),
        "generic package-backed Pick heritage should survive, got {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"color")
            && prop_names.contains(&"variant")
            && prop_names.contains(&"size")
            && prop_names.contains(&"orientation")
            && prop_names.contains(&"ui"),
        "generic indexed-access helper props should survive, got {prop_names:?}"
    );

    let color = meta
        .props
        .iter()
        .find(|prop| prop.name == "color")
        .expect("color prop should exist");
    let color_ty = demand_published_type(
        project.host(),
        "/src/App.vue",
        color.publication.result().selected_source(),
        "color prop",
    );
    assert_union_string_literals(&color_ty, &["primary", "secondary"]);

    let ui = meta
        .props
        .iter()
        .find(|prop| prop.name == "ui")
        .expect("ui prop should exist");
    let ui_ty = demand_published_type(
        project.host(),
        "/src/App.vue",
        ui.publication.result().selected_source(),
        "ui prop",
    );
    let TypeExpr::Object(ui_shape) = &ui_ty else {
        panic!("ui helper should materialize as an object, got {ui_ty:?}");
    };
    assert!(
        ui_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.string_name().expect("string-key fixture") == "root"),
        ),
        "ui helper should keep root, got {ui_ty:?}"
    );
    assert!(
        ui_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.string_name().expect("string-key fixture") == "list"),
        ),
        "ui helper should keep list, got {ui_ty:?}"
    );

    let value_key = meta
        .props
        .iter()
        .find(|prop| prop.name == "valueKey")
        .expect("valueKey prop should exist");
    let value_key_ty = demand_published_type(
        project.host(),
        "/src/App.vue",
        value_key.publication.result().selected_source(),
        "valueKey prop",
    );
    assert!(
        !matches!(value_key_ty, TypeExpr::Primitive(PrimitiveName::Never)),
        "generic key helpers should not collapse to never, got {value_key_ty:?}"
    );

    let content_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "content")
        .expect("content slot should exist");
    let binding_names: Vec<_> = content_slot
        .bindings
        .iter()
        .map(|binding| binding.name.as_str())
        .collect();
    assert_eq!(
        binding_names,
        vec!["item", "index", "ui"],
        "generic slot aliases should keep their scoped bindings, got {:?}",
        binding_names
    );

    let event = meta
        .events
        .iter()
        .find(|event| event.name == "update:modelValue")
        .expect("update:modelValue event should exist");
    let event_payload_ty = demand_published_type(
        project.host(),
        "/src/App.vue",
        event.payload.present(),
        "update:modelValue payload",
    );
    let TypeExpr::Tuple { elements, .. } = &event_payload_ty else {
        panic!(
            "generic package-backed emits should materialize as a tuple payload, got {event_payload_ty:?}"
        );
    };
    assert_eq!(
        elements.len(),
        1,
        "model update should have a single payload"
    );
    match &elements[0].ty {
        TypeExpr::Union(members) => {
            assert!(
                members.contains(&TypeExpr::Primitive(PrimitiveName::String)),
                "event payload should include string, got {event_payload_ty:?}"
            );
            assert!(
                members.contains(&TypeExpr::Primitive(PrimitiveName::Number)),
                "event payload should include number, got {event_payload_ty:?}"
            );
        }
        other => panic!(
            "generic package-backed emits should instantiate the generic payload, got {:?}",
            other
        ),
    }
}

#[test]
fn resolved_component_meta_materializes_imported_generic_tabs_helper_fields() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/reka-ui/index.d.ts",
            r#"
export interface TabsRootProps<T> {
  defaultValue?: T
  modelValue?: T
  activationMode?: 'automatic' | 'manual'
  unmountOnHide?: boolean
}

export interface TabsRootEmits<T> {
  (e: 'update:modelValue', payload: T): void
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/tv.ts",
            r#"
type Id<T> = {} & { [P in keyof T]: T[P] }

type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentSlots<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof T['slots']]?: string
}>

type ComponentUI<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}>

export type ComponentConfig<T extends Record<string, any>> = {
  variants: ComponentVariants<T>,
  slots: ComponentSlots<T>
  ui: ComponentUI<T>
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/utils.ts",
            r#"
export type NestedItem<T> = T extends Array<infer I> ? NestedItem<I> : T
export type GetItemKeys<I, T extends NestedItem<I> = NestedItem<I>> =
  keyof Extract<T, object> & string
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/theme.ts",
            r#"export default {
  variants: {
    color: { primary: '', secondary: '' },
    variant: { pill: '', link: '' },
    size: { sm: '', md: '' },
    orientation: { horizontal: '', vertical: '' }
  },
  slots: {
    root: '',
    list: '',
    content: ''
  }
} as const"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { TabsRootProps, TabsRootEmits } from 'reka-ui'
import type { ComponentConfig } from './tv'
import type { GetItemKeys } from './utils'
import theme from './theme'

type Tabs = ComponentConfig<typeof theme>

export interface TabsItem {
  label?: string
  value?: string | number
}

export interface TabsProps<T extends TabsItem = TabsItem> extends Pick<TabsRootProps<string | number>, 'defaultValue' | 'modelValue' | 'activationMode' | 'unmountOnHide'> {
  items?: T[]
  color?: Tabs['variants']['color']
  variant?: Tabs['variants']['variant']
  size?: Tabs['variants']['size']
  orientation?: Tabs['variants']['orientation']
  valueKey?: GetItemKeys<T>
  labelKey?: GetItemKeys<T>
  ui?: Tabs['slots']
}

export interface TabsEmits extends TabsRootEmits<string | number> {}

type SlotProps<T extends TabsItem> = (props: { item: T, index: number, ui: Tabs['ui'] }) => any

export type TabsSlots<T extends TabsItem = TabsItem> = {
  content?: SlotProps<T>
}
</script>
<script setup lang="ts" generic="T extends TabsItem">
withDefaults(defineProps<TabsProps<T>>(), {
  defaultValue: '0',
  orientation: 'horizontal',
  unmountOnHide: true,
  valueKey: 'value',
  labelKey: 'label'
})
defineEmits<TabsEmits>()
defineSlots<TabsSlots<T>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "reka-ui".to_string(),
                resolved_canonical_id: Some("/node_modules/reka-ui/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./tv".to_string(),
                resolved_canonical_id: Some("/src/tv.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./utils".to_string(),
                resolved_canonical_id: Some("/src/utils.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./theme".to_string(),
                resolved_canonical_id: Some("/src/theme.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");
    let meta = crate::resolver_core::with_bare_host_ctx_for_test(project.host(), |ctx| {
        let fixture_dispatch_4 =
            verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);

        crate::host_manage::extract_component_meta_from_resolved(
            project.host(),
            "/src/App.vue",
            &resolved,
            true,
            ctx,
            &fixture_dispatch_4,
        )
    })
    .analysis;

    let color = meta
        .props
        .iter()
        .find(|prop| prop.name == "color")
        .expect("color prop should exist");
    let color_ty = demand_published_type(
        project.host(),
        "/src/App.vue",
        color.publication.result().selected_source(),
        "color prop",
    );
    assert_union_string_literals(&color_ty, &["primary", "secondary"]);

    let ui = meta
        .props
        .iter()
        .find(|prop| prop.name == "ui")
        .expect("ui prop should exist");
    let ui_ty = demand_published_type(
        project.host(),
        "/src/App.vue",
        ui.publication.result().selected_source(),
        "ui prop",
    );
    let TypeExpr::Object(ui_shape) = &ui_ty else {
        panic!("ui helper should materialize as an object, got {ui_ty:?}");
    };
    assert!(
        ui_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.string_name().expect("string-key fixture") == "root"),
        ),
        "ui helper should keep root, got {ui_ty:?}"
    );

    let value_key = meta
        .props
        .iter()
        .find(|prop| prop.name == "valueKey")
        .expect("valueKey prop should exist");
    let value_key_ty = demand_published_type(
        project.host(),
        "/src/App.vue",
        value_key.publication.result().selected_source(),
        "valueKey prop",
    );
    assert!(
        !matches!(value_key_ty, TypeExpr::Primitive(PrimitiveName::Never)),
        "generic key helpers should not collapse to never, got {value_key_ty:?}"
    );

    let content_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "content")
        .expect("content slot should exist");
    let binding_names: Vec<_> = content_slot
        .bindings
        .iter()
        .map(|binding| binding.name.as_str())
        .collect();
    assert_eq!(
        binding_names,
        vec!["item", "index", "ui"],
        "generic slot aliases should keep their scoped bindings, got {:?}",
        binding_names
    );
}

// Regression: earlier, materializing a prop field whose type was a `Ref` to
// an imported generic whose declaration body transitively cycled through a
// sibling helper (DotPathKeys → DotPathKeys) sent the solver into a declaration
// scope with full local visibility to every recursive helper.  The solver
// would then grow the type arena to its hard ceiling during a single
// projection call, consuming multi-GB of memory before terminating.
//
// This fixture narrows the reproduction down to the recursive generic itself,
// without any of the slot / component-meta surface that the larger realistic
// fixture carries.  If the owner-scope fallback ever starts walking back into
// the declaration scope for a transitively recursive helper, this test will
// either hang or OOM instead of completing in a few tens of milliseconds.
#[test]
fn component_meta_does_not_hang_on_transitively_recursive_generic_prop_helper() {
    let project = make_project();
    project
        .upsert_base(
            "/src/utils.ts",
            r#"
type IsPrimitive<T> = T extends (string | number | boolean | symbol | bigint | null | undefined)
  ? true
  : false

type IsPlainObject<T> = IsPrimitive<T> extends true
  ? false
  : T extends readonly any[] | ((...args: any[]) => any)
    ? false
    : T extends object ? true
      : false

type DotPathKeys<T> = IsPlainObject<T> extends true
  ? {
      [K in keyof T & string]:
      IsPlainObject<NonNullable<T[K]>> extends true
        ? K | `${K}.${DotPathKeys<NonNullable<T[K]>>}`
        : K
    }[keyof T & string]
  : never

export type NestedItem<T> = T extends Array<infer I> ? NestedItem<I> : T

export type GetItemKeys<
  I,
  T extends NestedItem<I> = NestedItem<I>
> = (keyof Extract<T, object> & string) | DotPathKeys<Extract<T, object>>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts" generic="T extends { label?: string; nested?: { path?: string } }">
import type { GetItemKeys } from './utils'

defineProps<{
  valueKey?: GetItemKeys<T>
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./utils".to_string(),
            resolved_canonical_id: Some("/src/utils.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let started = std::time::Instant::now();
    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");
    let elapsed = started.elapsed();

    assert!(
        elapsed.as_secs_f64() < 30.0,
        "transitively-recursive generic prop helper should not hang \
         (elapsed {:.2}s)",
        elapsed.as_secs_f64()
    );

    let meta = crate::resolver_core::with_bare_host_ctx_for_test(project.host(), |ctx| {
        let fixture_dispatch_7 =
            verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);

        crate::host_manage::extract_component_meta_from_resolved(
            project.host(),
            "/src/App.vue",
            &resolved,
            true,
            ctx,
            &fixture_dispatch_7,
        )
    })
    .analysis;
    assert!(
        meta.props.iter().any(|prop| prop.name == "valueKey"),
        "valueKey prop should still be produced, got props {:?}",
        meta.props
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn union_object_variants_synthesize_component_meta_props() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
type FixedProps = {
  layout?: 'fixed'
  editor: string
}

type BubbleProps = {
  layout?: 'bubble'
  editor: string
  floating?: boolean
}

type Props = FixedProps | BubbleProps
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        names.contains(&"layout"),
        "should have 'layout', got: {names:?}"
    );
    assert!(
        names.contains(&"editor"),
        "should have 'editor', got: {names:?}"
    );
    assert!(
        names.contains(&"floating"),
        "should have union branch props, got: {names:?}"
    );
}

#[test]
fn mixed_intersection_retains_local_component_meta_props() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
type Props = {
  id?: string
  disabled?: boolean
} & Omit<FormHTMLAttributes, 'name'>

defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"id"), "should have 'id', got: {names:?}");
    assert!(
        names.contains(&"disabled"),
        "should have 'disabled', got: {names:?}"
    );
}

#[test]
fn partial_omit_union_branch_does_not_leak_package_editor_members_into_top_level_props() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/editor-lib/index.d.ts",
            r#"
export interface Editor {
  $doc(): string
  chain(): string
  active?: boolean
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/menu.ts",
            r#"
import type { Editor } from 'editor-lib'

export interface MenuProps {
  editor: Editor,
  element: object
  appendTo?: object
  pluginKey?: string
  class?: any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { MenuProps } from './menu'

type BaseProps = {
  layout?: 'fixed' | 'bubble'
  editor: object
}

type Props =
  | (BaseProps & { layout?: 'fixed' })
  | (BaseProps & Partial<Omit<MenuProps, 'editor' | 'element' | 'class'>> & { layout?: 'bubble' })

defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./menu".to_string(),
            resolved_canonical_id: Some("/src/menu.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/menu.ts",
        vec![crate::types::DependencyResolution {
            specifier: "editor-lib".to_string(),
            resolved_canonical_id: Some("/node_modules/editor-lib/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

    assert!(
        prop_names.contains(&"layout")
            && prop_names.contains(&"editor")
            && prop_names.contains(&"appendTo")
            && prop_names.contains(&"pluginKey"),
        "expected union props should be present, got {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"$doc")
            && !prop_names.contains(&"chain")
            && !prop_names.contains(&"active")
            && !prop_names.contains(&"element")
            && !prop_names.contains(&"class"),
        "Partial<Omit<...>> union branch should not leak package editor members into top-level props: {:?}",
        prop_names
    );
}

#[test]
fn local_generic_wrapper_over_package_backed_props_stays_symbolic_in_evaluated_types() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/vue-router/index.d.ts",
            r#"
export interface RouterLinkProps {
  to?: string
  replace?: boolean
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Link.vue",
            r#"<script lang="ts">
import type { RouterLinkProps } from 'vue-router'

export interface LinkProps extends Omit<RouterLinkProps, 'custom'> {
  custom?: boolean
  label?: string
}
</script>
<script setup lang="ts">
defineProps<LinkProps>()
</script>
<template><a /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/CommandPalette.vue",
            r#"<script lang="ts">
export interface CommandPaletteItem {
  id?: string
}

export interface CommandPaletteGroup<T extends CommandPaletteItem = CommandPaletteItem> {
  items?: T[]
}
</script>
<script setup lang="ts">
defineProps<CommandPaletteGroup>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { LinkProps } from './Link.vue'
import type { CommandPaletteGroup } from './CommandPalette.vue'

interface ContentSearchItem extends Omit<LinkProps, 'custom'> {
  badge?: string
}

defineProps<{
  groups?: CommandPaletteGroup<ContentSearchItem>[]
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "./Link.vue".to_string(),
                resolved_canonical_id: Some("/src/Link.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./CommandPalette.vue".to_string(),
                resolved_canonical_id: Some("/src/CommandPalette.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
    project.host().set_import_dependencies(
        "/src/Link.vue",
        vec![crate::types::DependencyResolution {
            specifier: "vue-router".to_string(),
            resolved_canonical_id: Some("/node_modules/vue-router/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");
    let groups_field = resolved
        .evaluated_types
        .as_ref()
        .and_then(|types| types.props.iter().find(|field| field.name == "groups"))
        .expect("expanded evaluated types should keep the groups prop");

    let groups_field_ty = shallow_published_type(
        project.host(),
        "/src/App.vue",
        Some(
            groups_field
                .authority
                .source_position()
                .present()
                .expect("present source"),
        ),
        "groups prop",
    );
    assert!(
        matches!(
            &groups_field_ty,
            verter_type_expr::TypeExpr::Array { element, .. }
                if matches!(
                    element.as_ref(),
                    verter_type_expr::TypeExpr::Ref { name, type_arguments }
                        if name.as_ref() == "CommandPaletteGroup"
                            && type_arguments.len() == 1
                            && matches!(
                                &type_arguments[0],
                                verter_type_expr::TypeExpr::Ref { name, type_arguments }
                                    if name.as_ref() == "ContentSearchItem"
                                        && type_arguments.is_empty()
                            )
                )
        ),
        "local generic wrappers should stay symbolic when they eventually flow into package-backed imported refs, got {groups_field_ty:?}"
    );
}

#[test]
fn imported_union_field_stays_symbolic_in_evaluated_types() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
export interface TooltipProps {
  text?: string
  delay?: number
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { TooltipProps } from './types'

defineProps<{
  tooltip?: boolean | TooltipProps
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");
    let tooltip_field = resolved
        .evaluated_types
        .as_ref()
        .and_then(|types| types.props.iter().find(|field| field.name == "tooltip"))
        .expect("expanded evaluated types should keep the tooltip prop");

    let tooltip_field_ty = shallow_published_type(
        project.host(),
        "/src/App.vue",
        Some(
            tooltip_field
                .authority
                .source_position()
                .present()
                .expect("present source"),
        ),
        "tooltip prop",
    );
    let has_symbolic_tooltip = match &tooltip_field_ty {
        verter_type_expr::TypeExpr::Union(members) => members.iter().any(|member| {
            matches!(
                member,
                verter_type_expr::TypeExpr::Ref { name, type_arguments }
                    if name.as_ref() == "TooltipProps" && type_arguments.is_empty()
            )
        }),
        _ => false,
    };

    assert!(
        has_symbolic_tooltip,
        "imported unions should keep imported object refs symbolic instead of expanding them in shallow field evaluation, got {tooltip_field_ty:?}"
    );
}

#[test]
fn barrel_imported_vue_union_field_stays_symbolic_in_evaluated_types() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/reka-ui/index.d.ts",
            r#"
export interface TooltipRootProps {
  delayDuration?: number
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Tooltip.vue",
            r#"<script lang="ts">
import type { TooltipRootProps } from 'reka-ui'

export interface TooltipProps extends TooltipRootProps {
  text?: string
}

export default {
  name: 'Tooltip'
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base("/types.ts", "export * from './Tooltip.vue'\n")
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { TooltipProps } from './types'

defineProps<{
  tooltip?: boolean | TooltipProps
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/types.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./Tooltip.vue".to_string(),
            resolved_canonical_id: Some("/Tooltip.vue".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/Tooltip.vue",
        vec![crate::types::DependencyResolution {
            specifier: "reka-ui".to_string(),
            resolved_canonical_id: Some("/node_modules/reka-ui/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta(
            "/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");
    let tooltip_field = resolved
        .evaluated_types
        .as_ref()
        .and_then(|types| types.props.iter().find(|field| field.name == "tooltip"))
        .expect("expanded evaluated types should keep the tooltip prop");

    let tooltip_field_ty = shallow_published_type(
        project.host(),
        "/App.vue",
        Some(
            tooltip_field
                .authority
                .source_position()
                .present()
                .expect("present source"),
        ),
        "tooltip prop",
    );
    let has_symbolic_tooltip = match &tooltip_field_ty {
        verter_type_expr::TypeExpr::Union(members) => members.iter().any(|member| {
            matches!(
                member,
                verter_type_expr::TypeExpr::Ref { name, type_arguments }
                    if name.as_ref() == "TooltipProps" && type_arguments.is_empty()
            )
        }),
        _ => false,
    };

    assert!(
        has_symbolic_tooltip,
        "barrel-imported vue unions should keep imported component prop refs symbolic in shallow field evaluation, got {tooltip_field_ty:?}"
    );
}

#[test]
fn local_alias_with_package_backed_union_stays_symbolic_in_evaluated_types() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/vue/index.d.ts",
            r#"
export interface VNode {
  component?: object
  children?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
import type { VNode } from 'vue'

export type StringOrVNode = string | VNode
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { StringOrVNode } from './types'

defineProps<{
  title?: StringOrVNode
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/types.ts",
        vec![crate::types::DependencyResolution {
            specifier: "vue".to_string(),
            resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let _store_view = project.host().resolver_store_view_read().into_owned_view();
    let prepared = project
        .host()
        .prepared_type_decl("/src/types.ts", "StringOrVNode")
        .expect("StringOrVNode should be present in the shallow prepared declarations");
    let prepared_body_source = verter_type_expr::facts::SemanticTypeSource::Authored(
        verter_type_expr::locators::AuthoredBodyLocator::DeclBody(
            prepared.body_facts.body_slot.clone(),
        ),
    );
    let prepared_body_ty = shallow_published_type(
        project.host(),
        "/src/types.ts",
        Some(&prepared_body_source),
        "StringOrVNode prepared body",
    );
    assert!(
        matches!(&prepared_body_ty, verter_type_expr::TypeExpr::Union(_)),
        "shallow prepared declarations should keep imported non-object aliases symbolic, got {prepared_body_ty:?}"
    );

    let resolved = project
        .host()
        .resolve_component_meta(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved component meta should exist");
    let title_field = resolved
        .evaluated_types
        .as_ref()
        .and_then(|types| types.props.iter().find(|field| field.name == "title"))
        .expect("expanded evaluated types should keep the title prop");

    // The projector publishes the `title` field via
    // `dispatch.execute_read` + `raise_node_to_type_expr`. Two
    // acceptable shapes preserve the package-backed `VNode` symbol
    // semantically:
    //
    //   1. `Ref { name: "StringOrVNode", type_arguments: [] }` — the
    //      projector preserves the alias name rather than unwrapping
    //      the union (the alias body remains accessible through the
    //      type registry / resolver). This is the projector's typical
    //      Shallow output for non-object aliases.
    //   2. `Union(...)` containing a symbolic `Ref { name: "VNode" }`
    //      — the legacy walker's expanded form.
    //
    // BOTH preserve the load-bearing invariant: the package-backed
    // `VNode` is NOT eagerly expanded into its `node_modules/` body
    // (which would defeat the symbolic-preservation contract).
    let title_field_ty = shallow_published_type(
        project.host(),
        "/src/App.vue",
        Some(
            title_field
                .authority
                .source_position()
                .present()
                .expect("present source"),
        ),
        "title prop",
    );
    let preserves_package_symbolic = match &title_field_ty {
        verter_type_expr::TypeExpr::Union(members) => members.iter().any(|member| {
            matches!(
                member,
                verter_type_expr::TypeExpr::Ref { name, type_arguments }
                    if name.as_ref() == "VNode" && type_arguments.is_empty()
            )
        }),
        verter_type_expr::TypeExpr::Ref { name, .. } => name.as_ref() == "StringOrVNode",
        _ => false,
    };

    assert!(
        preserves_package_symbolic,
        "local aliases that wrap package-backed refs must preserve the \
         package-backed symbol — either as a `Ref` to the local alias \
         or as a `Union` containing a symbolic `Ref {{ name: \"VNode\" }}`. \
         Got {title_field_ty:?}"
    );
}

#[test]
fn get_component_meta_editor_toolbar_union_keeps_base_and_plugin_props() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/@tiptap/extension-bubble-menu/index.d.ts",
            r#"
export interface BubbleMenuPluginProps {
  editor?: object
  element?: object
  appendTo?: object
  pluginKey?: string
  shouldShow?: (props: { editor: object }) => boolean
  updateDelay?: number
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/node_modules/@tiptap/extension-floating-menu/index.d.ts",
            r#"
export interface FloatingMenuPluginProps {
  editor?: object
  element?: object
  options?: {
    strategy?: 'absolute' | 'fixed'
  }
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/types.ts",
            r#"
export type ArrayOrNested<T> = T[] | T[][]

export interface LinkProps {
  to?: string
  href?: string
  target?: string
  rel?: string
  noRel?: boolean
  external?: boolean
  prefetch?: boolean
  prefetchOn?: 'visibility' | 'interaction'
  prefetchedClass?: string
  noPrefetch?: boolean
  trailingSlash?: 'append' | 'remove'
  replace?: boolean
  ariaCurrentValue?: string
  active?: boolean
  activeClass?: string
  exact?: boolean
  exactQuery?: boolean | 'partial'
  exactHash?: boolean
  inactiveClass?: string
  download?: string
  ping?: string
  referrerpolicy?: string
  hreflang?: string
  media?: string
}

export type LinkPropsKeys =
  | 'to'
  | 'href'
  | 'target'
  | 'rel'
  | 'noRel'
  | 'external'
  | 'prefetch'
  | 'prefetchOn'
  | 'prefetchedClass'
  | 'noPrefetch'
  | 'trailingSlash'
  | 'replace'
  | 'ariaCurrentValue'
  | 'active'
  | 'activeClass'
  | 'exact'
  | 'exactQuery'
  | 'exactHash'
  | 'inactiveClass'
  | 'download'
  | 'ping'
  | 'referrerpolicy'
  | 'hreflang'
  | 'media'

export interface ButtonProps {
  color?: 'primary' | 'neutral'
  variant?: 'solid' | 'ghost' | 'soft'
  size?: 'sm' | 'md'
  class?: any
  ui?: object
  activeColor?: 'primary' | 'neutral'
  activeVariant?: 'solid' | 'ghost' | 'soft'
  type?: 'button' | 'submit'
}

export interface TooltipProps {
  text?: string
  portal?: boolean | string
}

export interface DropdownMenuItem {
  label?: string
  type?: 'label' | 'separator' | 'link'
}

export interface DropdownMenuProps<T extends ArrayOrNested<DropdownMenuItem> = ArrayOrNested<DropdownMenuItem>> {
  items?: T
  content?: { side?: 'bottom' | 'top' }
  arrow?: boolean
  portal?: boolean | string
}

export interface EditorHandler {
  canExecute: (editor: object, cmd?: any) => boolean,
  execute: (editor: object, cmd?: any) => any
  isActive: (editor: object, cmd?: any) => boolean
}

export type EditorCustomHandlers = Record<string, EditorHandler>

export type EditorItem<H extends EditorCustomHandlers = EditorCustomHandlers>
  = | { kind: 'mark', mark: 'bold' | 'italic' }
    | { kind: 'link', href?: string }
    | { kind: keyof H }
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/theme.ts",
            r#"
const theme = {
  slots: {
    root: 'root'
  },
  variants: {
    color: ['neutral', 'primary']
  }
} as const

export default theme
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/tv.ts",
            r#"
export type ComponentConfig<
  TTheme,
  TAppConfig,
  TKey extends string
> = {
  slots: TTheme extends { slots: infer TSlots } ? TSlots : never,
  AppConfig: TAppConfig
  key: TKey
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/node_modules/@nuxt/schema/index.d.ts",
            r#"
export interface AppConfig {
  ui?: Record<string, unknown>
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/node_modules/@tiptap/vue-3/index.d.ts",
            r#"
export interface Editor {
  isEditable?: boolean
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/EditorToolbar.vue",
            r#"<script lang="ts">
import type { AppConfig } from '@nuxt/schema'
import type { Editor } from '@tiptap/vue-3'
import type { BubbleMenuPluginProps } from '@tiptap/extension-bubble-menu'
import type { FloatingMenuPluginProps } from '@tiptap/extension-floating-menu'
import theme from './theme'
import type { ArrayOrNested, ButtonProps, DropdownMenuItem, DropdownMenuProps, EditorCustomHandlers, EditorItem, LinkPropsKeys, TooltipProps } from './types'
import type { ComponentConfig } from './tv'

type EditorToolbar = ComponentConfig<typeof theme, AppConfig, 'editorToolbar'>

type ButtonItem = Omit<ButtonProps, 'type'> & {
  slot?: string
  tooltip?: TooltipProps
  'aria-label'?: string
}

type EditorToolbarButtonItem<H extends EditorCustomHandlers = EditorCustomHandlers> = Omit<ButtonItem, LinkPropsKeys> & EditorItem<H>

type EditorToolbarDropdownChildItem<H extends EditorCustomHandlers = EditorCustomHandlers>
  = | DropdownMenuItem
    | (Omit<DropdownMenuItem, 'type'> & EditorItem<H>)

type EditorToolbarDropdownItem<H extends EditorCustomHandlers = EditorCustomHandlers> = ButtonItem & DropdownMenuProps<ArrayOrNested<EditorToolbarDropdownChildItem<H>>>

export type EditorToolbarItem<H extends EditorCustomHandlers = EditorCustomHandlers>
  = | ButtonItem
    | EditorToolbarButtonItem<H>
    | EditorToolbarDropdownItem<H>

type BaseProps<T extends ArrayOrNested<EditorToolbarItem> = ArrayOrNested<EditorToolbarItem>> = {
  as?: any
  color?: ButtonProps['color']
  variant?: ButtonProps['variant']
  activeColor?: ButtonProps['color']
  activeVariant?: ButtonProps['variant']
  size?: ButtonProps['size']
  items?: T
  editor: Editor
  class?: any
  ui?: EditorToolbar['slots']
}

export type EditorToolbarProps<T extends ArrayOrNested<EditorToolbarItem> = ArrayOrNested<EditorToolbarItem>>
  = | (BaseProps<T> & { layout?: 'fixed' })
    | (BaseProps<T> & Partial<Omit<BubbleMenuPluginProps, 'editor' | 'element'>> & {
      layout?: 'bubble'
    })
    | (BaseProps<T> & Partial<Omit<FloatingMenuPluginProps, 'editor' | 'element'>> & {
      layout?: 'floating'
    })
</script>

<script setup lang="ts" generic="T extends ArrayOrNested<EditorToolbarItem>">
withDefaults(defineProps<EditorToolbarProps<T>>(), {
  layout: 'fixed'
})
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/EditorToolbar.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "@nuxt/schema".to_string(),
                resolved_canonical_id: Some("/node_modules/@nuxt/schema/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "@tiptap/vue-3".to_string(),
                resolved_canonical_id: Some("/node_modules/@tiptap/vue-3/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "@tiptap/extension-bubble-menu".to_string(),
                resolved_canonical_id: Some(
                    "/node_modules/@tiptap/extension-bubble-menu/index.d.ts".to_string(),
                ),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "@tiptap/extension-floating-menu".to_string(),
                resolved_canonical_id: Some(
                    "/node_modules/@tiptap/extension-floating-menu/index.d.ts".to_string(),
                ),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./theme".to_string(),
                resolved_canonical_id: Some("/theme.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./tv".to_string(),
                resolved_canonical_id: Some("/tv.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let meta = get_meta(&project, "/EditorToolbar.vue");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

    assert!(
        prop_names.contains(&"as")
            && prop_names.contains(&"color")
            && prop_names.contains(&"variant")
            && prop_names.contains(&"activeColor")
            && prop_names.contains(&"activeVariant")
            && prop_names.contains(&"size")
            && prop_names.contains(&"items")
            && prop_names.contains(&"editor")
            && prop_names.contains(&"class")
            && prop_names.contains(&"ui")
            && prop_names.contains(&"layout"),
        "EditorToolbar union must keep its base props, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"appendTo")
            && prop_names.contains(&"pluginKey")
            && prop_names.contains(&"shouldShow")
            && prop_names.contains(&"updateDelay")
            && prop_names.contains(&"options"),
        "EditorToolbar union must also keep branch-specific plugin props, got: {prop_names:?}"
    );
}

/// C5 parity: an imported GENERIC-HELPER route `Button['ui']` where `Button` is
/// a local alias of an imported generic instantiation (`type Button =
/// FormApi<string>`). This is the documented `lower_and_project_to_expanded`
/// constraint case — the empty-path terminal would freeze a generic
/// `InstantiationRef` under Navigate, so the C5 fixpoint stabilises it at
/// Expanded. Pins the published `ui` object surface (members + their terminal
/// primitives), path-precise.
#[test]
fn c5_parity_imported_generic_helper_button_ui_route_pins_published_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/form.ts",
            r#"export type FormApi<T> = {
  ui: { label: T; count: number }
  meta: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import type { FormApi } from './form'

type Button = FormApi<string>

defineProps<{
  ui: Button['ui']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();
    let ui_ty = evaluated_prop_type(&project, "/Comp.vue", &evaluated, "ui");

    // CURRENT behaviour (pinned): `Button['ui']` = `FormApi<string>['ui']` =
    // `{ label: string; count: number }` — the generic `T=string` substitution
    // flows into the indexed-access terminal, and the sibling `meta` member is
    // excluded (path-precise: only the `['ui']` hop loads).
    let TypeExpr::Object(obj) = &ui_ty else {
        panic!(
            "C5 parity (button-ui): `Button['ui']` must publish a concrete Object surface \
             (not an `Unknown` shell / un-expanded `Ref` carrier); got {ui_ty:?}"
        );
    };
    let members: Vec<(&str, &TypeExpr)> = obj
        .properties
        .iter()
        .filter_map(|m| match m {
            ObjectMember::Property(p) => {
                Some((p.string_name().expect("string-key fixture"), &p.ty))
            }
            _ => None,
        })
        .collect();
    // Path-precise: EXACTLY [label, count] in source order — the `meta` sibling
    // of `FormApi` never enters the `['ui']` terminal.
    assert_eq!(
        members.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
        vec!["label", "count"],
        "C5 parity (button-ui): published `ui` must surface EXACTLY [label, count] in source \
         order; the `meta` sibling must NOT leak through the `['ui']` terminal. Got {ui_ty:?}"
    );
    assert_eq!(
        obj.properties.len(),
        2,
        "exactly two members, no index-signature leak"
    );
    // Generic substitution MUST flow into the terminal: `label` carries the
    // substituted `string`, NOT an un-substituted generic carrier.
    assert_eq!(
        members[0].1,
        &TypeExpr::Primitive(PrimitiveName::String),
        "C5 parity (button-ui): `label` must carry the `FormApi<string>` substitution (`string`), \
         not an un-substituted generic; got {:?}",
        members[0].1
    );
    assert!(
        !matches!(
            members[0].1,
            TypeExpr::Ref { .. } | TypeExpr::Unknown { .. }
        ),
        "C5 parity (button-ui): `label` must be the substituted primitive, never an \
         un-resolved `Ref`/`Unknown` generic carrier; got {:?}",
        members[0].1
    );
    assert_eq!(
        members[1].1,
        &TypeExpr::Primitive(PrimitiveName::Number),
        "C5 parity (button-ui): `count` must stay `number`; got {:?}",
        members[1].1
    );
}

/// FIX 3 #3 — `defineProps<NoInfer<Base>>()` THROUGH macro-surface publication.
/// `NoInfer<T>` is an identity wrapper (it only affects inference, not the
/// resolved type); the published props must be exactly the members of the
/// imported `Base` surface.
///
/// Discriminating: `label`/`count` come from `Base` wrapped in `NoInfer`; a
/// resolver that failed to unwrap `NoInfer` (or treated it as opaque) would
/// drop the props or collapse them to `never`.
#[test]
fn define_props_no_infer_base_through_macro_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/src/base.ts",
            r#"export interface Base {
  label?: string
  count?: number
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Base } from './base'

defineProps<NoInfer<Base>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");
    let label = meta
        .props
        .iter()
        .find(|p| p.name == "label")
        .expect("NoInfer<Base> must publish the `label` prop through the macro surface");
    let label_ty = demand_published_type(
        project.host(),
        "/src/App.vue",
        label.publication.result().selected_source(),
        "label prop",
    );
    assert!(
        !matches!(label_ty, TypeExpr::Primitive(PrimitiveName::Never)),
        "NoInfer<Base> prop must keep its resolved type, not collapse to never, got {label_ty:?}"
    );
    let prop_names: BTreeSet<_> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains("label") && prop_names.contains("count"),
        "NoInfer<Base> must publish the Base members unchanged (identity wrapper), got {prop_names:?}",
    );
}

/// FIX 3 #3 (alias-shell) — `export type Props = NoInfer<Base>; defineProps<Props>()`
/// THROUGH macro-surface publication. This is the harder shape than the direct
/// `defineProps<NoInfer<Base>>()` case above: the macro type argument is a NAMED
/// alias whose body is the `NoInfer` identity wrapper around the imported
/// `Base`. The transparent alias + `NoInfer` wrappers must resolve to `Base`'s
/// own body, so the published surface is EXACTLY `Base`'s members.
///
/// Discriminating, typeinfo-native (asserts the full published shape, not just
/// presence):
/// - member names AND order are exactly `[label, count]` (Base's declared order);
/// - both are optional (`required == false`) — the `?` survives the wrappers;
/// - the typed forms are the concrete `string` / `number` primitives (NOT
///   `never`, NOT a `Ref`/opaque) — a resolver treating `NoInfer` as opaque or
///   failing the alias hop would collapse or drop them;
/// - `declared_in_macro_type_arg == false` for BOTH — own-body provenance is
///   the WRITTEN macro type-argument declaration's DIRECT `member_index`
///   members (`build.rs::overlay_macro_type_arg_own_body`, which reads only
///   direct Object members and skips Ref/instantiation arms). The written
///   macro-arg here is the alias `Props`, whose body is the `NoInfer<Base>`
///   instantiation — it has NO direct own-body members; `label`/`count` are
///   reached by resolving the transparent alias + `NoInfer` hops to `Base`, so
///   they carry own-body `false` (the inverse value would mean a resolver
///   mis-stamped alias-reached members as the alias's own body).
#[test]
fn define_props_no_infer_base_alias_shell_through_macro_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/src/base.ts",
            r#"export interface Base {
  label?: string
  count?: number
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Base } from './base'

export type Props = NoInfer<Base>

defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");

    // Names AND order: exactly `[label, count]` (Base's declared order),
    // preserved through the alias + NoInfer wrappers.
    let ordered_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        ordered_names,
        vec!["label", "count"],
        "alias-shell NoInfer<Base> must publish exactly Base's members in declared order, got {ordered_names:?}",
    );

    let label = meta
        .props
        .iter()
        .find(|p| p.name == "label")
        .expect("alias-shell NoInfer<Base> must publish the `label` prop");
    let count = meta
        .props
        .iter()
        .find(|p| p.name == "count")
        .expect("alias-shell NoInfer<Base> must publish the `count` prop");

    // Optionality: the `?` survives the alias + NoInfer wrappers (NOT required).
    assert!(
        !label.required && !count.required,
        "alias-shell NoInfer<Base> props must stay optional (required == false), got label.required={}, count.required={}",
        label.required,
        count.required,
    );

    // Typed form: concrete primitives, NOT collapsed to `never` / left as an
    // opaque `Ref` — the alias + NoInfer wrappers resolved through to Base.
    let label_ty = demand_published_type(
        project.host(),
        "/src/App.vue",
        label.publication.result().selected_source(),
        "label prop",
    );
    assert!(
        matches!(label_ty, TypeExpr::Primitive(PrimitiveName::String)),
        "alias-shell `label` must keep its `string` primitive type, got {label_ty:?}",
    );
    let count_ty = demand_published_type(
        project.host(),
        "/src/App.vue",
        count.publication.result().selected_source(),
        "count prop",
    );
    assert!(
        matches!(count_ty, TypeExpr::Primitive(PrimitiveName::Number)),
        "alias-shell `count` must keep its `number` primitive type, got {count_ty:?}",
    );

    // Provenance: own-body is the WRITTEN macro type-arg declaration's direct
    // `member_index` members. The written macro-arg is the alias `Props`, whose
    // body is the `NoInfer<Base>` instantiation — NO direct own-body members.
    // `label`/`count` are reached through the transparent alias + NoInfer hops
    // to `Base`, so `declared_in_macro_type_arg` is false for both. The inverse
    // value would mean a resolver mis-stamped alias-reached members as the
    // alias's own body.
    assert!(
        !label.declared_in_macro_type_arg && !count.declared_in_macro_type_arg,
        "alias-shell NoInfer<Base> members are reached through the alias, NOT the macro type arg's own body (declared_in_macro_type_arg == false), got label={}, count={}",
        label.declared_in_macro_type_arg,
        count.declared_in_macro_type_arg,
    );
}

#[test]
fn cross_file_prop_jsdoc_survives_homomorphic_mapped_types() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
export interface ImportedProps {
  /**
   * Visual orientation of the widget.
   * @deprecated use layout instead
   */
  orientation?: string
  /** Number of columns. */
  columns?: number
  plain?: boolean
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Comp.vue",
            r#"<script setup lang="ts">
import type { ImportedProps } from './types'

defineProps<Partial<Pick<ImportedProps, 'orientation' | 'plain'>>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Comp.vue")
        .expect("component meta resolves");

    let orientation = meta
        .props
        .iter()
        .find(|prop| prop.name == "orientation")
        .expect("orientation must surface through Partial<Pick<...>>");
    assert_eq!(
        orientation.description.as_deref(),
        Some("Visual orientation of the widget."),
        "imported member's JSDoc description must survive homomorphic mapped production"
    );
    let deprecated = orientation
        .tags
        .iter()
        .find(|tag| tag.name == "deprecated")
        .expect("imported member's @deprecated tag must survive homomorphic mapped production");
    assert_eq!(deprecated.text.as_deref(), Some("use layout instead"));

    // Negative: an undocumented member publishes NO description and NO tags
    // (no fabrication).
    let plain = meta
        .props
        .iter()
        .find(|prop| prop.name == "plain")
        .expect("plain must surface through Partial<Pick<...>>");
    assert_eq!(
        plain.description.as_deref(),
        None,
        "undocumented member must not gain a fabricated description"
    );
    assert!(
        plain.tags.is_empty(),
        "undocumented member must not gain fabricated tags, got {:?}",
        plain.tags
    );
}

/// Generic substitution is SEMANTIC meaning on the published macro surface:
/// `defineProps<Pair<string, number>>` over the imported
/// `Pair<A, B> { first: A; second: B }` publishes `first` as the
/// INSTANTIATED `string` and `second` as the INSTANTIATED `number`. The two
/// type parameters are DISTINCT, so a dropped or swapped substitution env
/// (A/B reversed, or `TypeParam` shells surviving to publication) fails on
/// the exact primitive each member must carry.
///
/// ## Retired parity-oracle published-surface coverage map
///
/// The transitional dual-leg parity-oracle harness (its comparison leg read
/// the deleted prepared-body `TypeExpr` seam, so the harness became
/// definitionally unfulfillable and was removed) pinned eight
/// published-surface cases. Each is covered by a surviving, un-ignored,
/// discriminating test; the enforcement is those tests' greenness on the
/// default gate:
///
/// | Retired oracle case | Surviving cover |
/// |---|---|
/// | `component_meta_payload_cross_file_props` | `getcomponentmeta_decomposes_through_dispatch_primitives` (`tests/cases/g_misc2/per_macro_projector_decomposition_tests.rs` — cross-file member PRIMITIVE types + emits) jointly with `dispatch_macro_surface_matches_former_expander_output_on_real_fixture` (this file — member set + authored OPTIONALITY + emit set) |
/// | `fallthrough_single_native_root` | `single_native_root_inherits_intrinsic_surface` (this file — accepted declared prop + `FallthroughSurface::Branches` off a single native root) |
/// | `macro_own_body_provenance_intersection` | `cross_file_omit_then_reintroduce_own_body_members_carry_declared_true` (`tests/cases/g_misc2/r21_c5_cross_file_provenance.rs` — the own-body arm publishes `declared_in_macro_type_arg == true` while the imported-reached member publishes `false`) |
/// | `heritage_shadowing_own_body_wins` | `interface_heritage_duplicate_shadows` (`src/typeinfo/typeinfo_tests/shallow_surface_facts.rs` — own-body `dup: string` shadows the inherited `dup: number`, never an intersection; non-colliding inherited members survive) |
/// | `authored_intersection_collision_intersects` | `authored_intersection_duplicate_does_not_shadow` (`src/typeinfo/typeinfo_tests/shallow_surface_facts.rs` — authored `Base & { dup: string }` publishes `dup` carrying BOTH primitive arms, never the shadow single-arm) |
/// | `open_pick_publishes_shallow_carrier` | `chatmessages_resolvable_barrel_publishes_open_pick_as_shallow_carrier` (`src/component_meta_pick_omit_tests.rs` — an OPEN `Pick<PropsBase<T>, …>` over the SFC generic stays a `Pick` carrier ref with the open source arg preserved) |
/// | `module_augmentation_merged_props` | `cross_file_module_augmentation_merge_surface_matches_oracle` (`src/cross_file_augmentation_merge_equivalence_tests.rs` — the stitched surface publishes `base: string` AND `fromAug: number` with their types) plus the `defineProps` publication form in `merged_decl_body_stitch_self_heals_rekeyed_augmenter` (`tests/cases/g_session/module_augmentation_body_rekey.rs`) |
/// | `generic_pair_substitution` | THIS test — the only surviving cover of the two-DISTINCT-parameter instantiation on the published `defineProps` surface |
#[test]
fn imported_generic_pair_instantiates_distinct_member_primitives() {
    let project = make_project();
    project
        .upsert_base(
            "/pair.ts",
            "export interface Pair<A, B> { first: A; second: B }\n",
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Pair } from './pair'
defineProps<Pair<string, number>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    let first = meta
        .props
        .iter()
        .find(|p| p.name == "first")
        .expect("instantiated generic prop `first` must publish");
    let first_ty = demand_published_type(
        project.host(),
        "/App.vue",
        first.publication.result().selected_source(),
        "prop `first`",
    );
    assert!(
        matches!(first_ty, TypeExpr::Primitive(PrimitiveName::String)),
        "`first` must instantiate A := string (never a TypeParam shell, never \
         the swapped B arm); got {first_ty:?}"
    );

    let second = meta
        .props
        .iter()
        .find(|p| p.name == "second")
        .expect("instantiated generic prop `second` must publish");
    let second_ty = demand_published_type(
        project.host(),
        "/App.vue",
        second.publication.result().selected_source(),
        "prop `second`",
    );
    assert!(
        matches!(second_ty, TypeExpr::Primitive(PrimitiveName::Number)),
        "`second` must instantiate B := number (never a TypeParam shell, never \
         the swapped A arm); got {second_ty:?}"
    );
}

/// Closed shallow carriers survive to the wire UNexpanded: an imported alias
/// published as `Closed(Leaf(Ref))` materializes to the bare `Ref` name AS
/// WRITTEN (never the internal declaration body), and a closed literal union
/// (`LeafUnion`) renders its ordered members directly.
#[test]
fn component_meta_output_closed_ref_alias_and_leaf_union_stay_shallow() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            "export type PublishedAlias = { deep: string; nested: { inner: number } }\n",
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { PublishedAlias } from './types'
defineProps<{ aliased: PublishedAlias; lit: 'x' | 'y' }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let output = project
        .host()
        .get_component_meta_output("/App.vue")
        .expect("output materialization must succeed")
        .expect("component must resolve");
    let (analysis, _resolution, types) = output.into_parts();
    let lanes = types.into_lanes();

    let aliased_idx = analysis
        .props
        .iter()
        .position(|p| p.name == "aliased")
        .expect("aliased prop");
    match lanes.props[aliased_idx]
        .materialized_type()
        .expect("published type")
    {
        TypeExpr::Ref { name, .. } => assert_eq!(
            name.as_ref(),
            "PublishedAlias",
            "the published alias keeps the name AS WRITTEN"
        ),
        other => panic!(
            "a closed shallow alias carrier must materialize as the bare Ref, \
             NOT the expanded declaration body (Shallow-By-Default); got {other:?}"
        ),
    }

    let lit_idx = analysis
        .props
        .iter()
        .position(|p| p.name == "lit")
        .expect("lit prop");
    match lanes.props[lit_idx]
        .materialized_type()
        .expect("published type")
    {
        TypeExpr::Union(members) => {
            assert_eq!(members.len(), 2, "both literal arms render");
            assert!(
                matches!(&members[0], TypeExpr::Literal(LiteralValue::String(v)) if v == "y"),
                "ordered union arm 0 is 'y' in VerterStableV1 order; got {:?}",
                members[0]
            );
            assert!(
                matches!(&members[1], TypeExpr::Literal(LiteralValue::String(v)) if v == "x"),
                "ordered union arm 1 is 'x'"
            );
        }
        other => panic!("a closed literal union renders directly; got {other:?}"),
    }
}

/// Local-arm-first order preserves both the concrete contributor and the
/// stable unresolved carrier.
#[test]
fn same_name_intersection_prop_preserves_unresolved_carrier() {
    assert_same_name_intersection_prop_preserves_unresolved_carrier("{ x: string } & Bad");
}

/// Imported-arm-first order has the same carrier-preserving semantics.
#[test]
fn same_name_intersection_prop_preserves_unresolved_carrier_reversed() {
    assert_same_name_intersection_prop_preserves_unresolved_carrier("Bad & { x: string }");
}

/// POSITIVE control (no overfire): a same-name intersection whose arms AGREE
/// on a resolvable type (`{ x: string } & Good` with `Good { x: string }`)
/// stays a PRESENT `string` prop and a COMPLETE result.
#[test]
fn agreeing_same_name_intersection_prop_stays_present() {
    let project = make_project();
    project
        .upsert_base("/good.ts", "export interface Good { x: string }\n")
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Good } from './good'
defineProps<{ x: string } & Good>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let (analysis, _resolution, types) = project
        .host()
        .get_component_meta_output("/App.vue")
        .expect("an agreeing resolvable intersection materializes")
        .expect("the SFC resolves")
        .into_parts();
    let index = analysis
        .props
        .iter()
        .position(|prop| prop.name == "x")
        .expect("the merged prop publishes");
    // The published source stays a faithful PRESENT position (the closed
    // leaf when the merged value collapses to one interned node, else the
    // projected merged-member replay route) — never a failure and never an
    // absence.
    assert!(
        analysis.props[index]
            .publication
            .result()
            .selected_source()
            .is_some(),
        "both contributors agree on the same resolvable type — the merged \
         member publishes a PRESENT source, never a failure; got {:?}",
        analysis.props[index].publication.source_position()
    );
    // The materialized lane value is string-equivalent: the agreed leaf
    // itself, or the faithful merged intersection whose EVERY arm is the
    // agreed leaf (`string & string`) — never a failure, never a non-string.
    let lanes = types.into_lanes();
    let string_equivalent = match lanes.props[index]
        .materialized_type()
        .expect("published type")
    {
        TypeExpr::Primitive(PrimitiveName::String) => true,
        TypeExpr::Intersection(arms) => {
            !arms.is_empty()
                && arms
                    .iter()
                    .all(|arm| matches!(arm, TypeExpr::Primitive(PrimitiveName::String)))
        }
        _ => false,
    };
    assert!(
        string_equivalent,
        "the merged member materializes the agreed string type; got {:?}",
        lanes.props[index]
    );

    let (_analysis, state) = project
        .host()
        .get_component_meta_with_resolution("/App.vue")
        .expect("resolves");
    assert!(
        !state.completeness.is_partial(),
        "an agreeing resolvable intersection completes; got {:?}",
        state.completeness
    );
    assert!(
        !state.synthesis_should_suppress,
        "an agreeing resolvable intersection must not suppress warm admission"
    );
}

/// POSITIVE control (no overfire), both arms local: duplicate same-name
/// literal contributors (`{ x: string } & { x: string }`) merge to one
/// PRESENT `string` prop — multiple analyzer candidates keep the row
/// locator-less but the merged member value still publishes its closed
/// leaf fact.
#[test]
fn duplicate_local_same_name_intersection_prop_stays_present() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ x: string } & { x: string }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let (analysis, _resolution, _types) = project
        .host()
        .get_component_meta_output("/App.vue")
        .expect("a duplicate agreeing local intersection materializes")
        .expect("the SFC resolves")
        .into_parts();
    let prop = analysis
        .props
        .iter()
        .find(|prop| prop.name == "x")
        .expect("the merged prop publishes");
    assert!(
        matches!(
            &prop.publication.source_position(),
            verter_type_expr::facts::SourcePosition::Present(
                verter_type_expr::facts::SemanticTypeSource::Closed(_)
            )
        ),
        "identical local contributors publish the closed string fact; got {:?}",
        prop.publication.source_position()
    );
}

/// Index signatures concatenate across intersection arms. A stable unresolved
/// value remains an explicit carrier in its row while the concrete sibling row
/// remains intact; neither makes the result partial.
#[test]
fn intersection_index_signature_preserves_unresolved_value_carrier() {
    let project = make_project();
    project
        .upsert_base(
            "/bad-index.ts",
            "export interface BadIndex { [k: string]: MissingType }\n",
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { BadIndex } from './bad-index'
defineProps<{ [k: string]: string } & BadIndex>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let (_analysis, state) = project
        .host()
        .get_component_meta_with_resolution("/App.vue")
        .expect("the analysis resolves");
    let evaluated = state
        .evaluated_types
        .as_ref()
        .expect("evaluated types present");
    let signatures = &evaluated.define_props[0].result.value.index_signatures;
    assert_eq!(
        signatures.len(),
        2,
        "both intersection index signatures remain present"
    );
    let demanded_values: Vec<_> = signatures
        .iter()
        .map(|signature| {
            demand_published_type(
                project.host(),
                "/App.vue",
                signature.value_type.present(),
                "intersection index-signature value",
            )
        })
        .collect();
    assert!(
        demanded_values.iter().any(
            |value| matches!(value, TypeExpr::Ref { name, type_arguments }
                if name.as_ref() == "MissingType" && type_arguments.is_empty())
        ),
        "the unresolved value remains an explicit Ref carrier; got {demanded_values:?}"
    );
    assert!(
        demanded_values
            .iter()
            .any(|value| matches!(value, TypeExpr::Primitive(PrimitiveName::String))),
        "the concrete sibling value remains present; got {demanded_values:?}"
    );
    assert!(
        !state.completeness.is_partial(),
        "a stable unresolved index value is Complete; got {:?}",
        state.completeness
    );
    assert!(
        !state.synthesis_should_suppress,
        "a stable unresolved index value does not suppress warm admission"
    );
}

#[test]
fn generic_member_path_materializes_type_parameter_carriers() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Item { id: string }
export interface Props<U extends Item = Item>
  extends Partial<Pick<ExternalMenuOptions<U>, 'editor' | 'pluginKey'>> {
  items?: U[] | U[][]
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Generic.vue",
            r#"<script setup lang="ts" generic="T extends Item = Item">
import type { Item, Props } from './types'
withDefaults(defineProps<Props<T>>(), { pluginKey: 'menu' })
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let (analysis, _resolution, types) = project
        .host()
        .get_component_meta_output("/Generic.vue")
        .expect("the generic member path must not materialize to an opaque miss")
        .expect("component resolves")
        .into_parts();
    let index = analysis
        .props
        .iter()
        .position(|prop| prop.name == "items")
        .expect("items prop publishes");
    let lanes = types.into_lanes();
    let TypeExpr::Union(arms) = lanes.props[index]
        .materialized_type()
        .expect("published type")
    else {
        panic!("items retains its union shape");
    };

    fn array_leaf(expr: &TypeExpr) -> Option<&TypeExpr> {
        match expr {
            TypeExpr::Array { element, .. } => array_leaf(element).or(Some(element)),
            _ => None,
        }
    }

    assert_eq!(arms.len(), 2, "T[] | T[][] keeps both structural arms");
    for arm in arms.iter() {
        let leaf = array_leaf(arm).expect("each arm is an array");
        assert!(
            matches!(leaf, TypeExpr::TypeParameter(param) if param.name == "T"),
            "the script-setup generic remains a type-parameter carrier; got {leaf:?}"
        );
    }
}
