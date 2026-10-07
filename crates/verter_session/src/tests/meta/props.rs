use super::*;

/// `open_session()` defaults to interactive mode; `open_session_batch()`
/// returns a batch-mode session.
#[test]
fn open_session_defaults_to_interactive_mode() {
    let project = make_project();
    let interactive = project.open_session().expect("interactive session");
    let batch = project.open_session_batch().expect("batch session");
    assert_eq!(
        interactive.execution_mode(),
        crate::meta::MetaExecutionMode::Interactive,
        "open_session() default must be Interactive",
    );
    assert_eq!(
        batch.execution_mode(),
        crate::meta::MetaExecutionMode::Batch,
        "open_session_batch() must return Batch mode",
    );
}

/// PUBLIC BOUNDARY — `accepted_surface_completeness` is an EXHAUSTIVENESS
/// claim over the accepted surface. A component whose own props resolution is
/// PARTIAL (an unresolvable props import) cannot claim `Exact`: the accepted
/// set was computed against a props surface that may be missing members, so
/// the claim demotes to `LowerBound`.
///
/// CONTROL: a genuinely props-less component keeps `Exact` — the whole point
/// of the demotion is that it fires on partial COMPUTE, not on emptiness.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_partial_props_resolution_demotes_accepted_surface_exactness() {
    let project = make_project();
    project
        .upsert_base(
            "/src/AcceptedMissing.vue",
            r#"<script setup lang="ts">
import type { P } from './missing'
defineProps<P>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let meta = get_meta(&project, "/src/AcceptedMissing.vue");
    assert_eq!(
        meta.accepted_surface_completeness,
        verter_session_query::analysis::component_meta::AcceptedSurfaceCompleteness::LowerBound,
        "a partial props resolution must demote the accepted-surface claim: \
         `Exact` says every accepted member is known, and the declared props \
         that would subtract from the accepted set are unknown"
    );

    // CONTROL: props-less component — the empty declared surface is COMPLETE
    // and the accepted claim stays Exact.
    let project = make_project();
    project
        .upsert_base(
            "/src/AcceptedPropless.vue",
            r#"<script setup lang="ts">
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let meta = get_meta(&project, "/src/AcceptedPropless.vue");
    assert_eq!(
        meta.accepted_surface_completeness,
        verter_session_query::analysis::component_meta::AcceptedSurfaceCompleteness::Exact,
        "a genuinely props-less component keeps the Exact accepted claim"
    );
}

/// PUBLIC BOUNDARY, RENDERED BYTES — the runtime lane derives EACH member's
/// constructor set from THAT member's own evidence.
///
/// The flow substrate publishes a degraded success as a surface in which the
/// one position it could not type carries the typed unresolved marker and
/// every modelled sibling is exact. The runtime lane must read that surface
/// the same way: the marker-carrying member emits `type: null` (Vue's
/// "validation and casting off"), and a sibling the substrate typed exactly
/// keeps its real constructor.
///
/// The regression this pins: the per-member broad-runtime classification
/// short-circuited on the frame-level partial before it ever projected the
/// member, so BOTH members collapsed to `type: null` — `get_component_meta`
/// published `label` as `string` on the same tree while the module Vue
/// actually runs declared it untyped. Vue drives Boolean casting, `default`
/// factory handling, and dev validation off `type`, so an erased constructor
/// is a runtime behaviour change, not a cosmetic one.
///
/// Oracle (TypeScript 7.0.2 `tsc`, `--noEmit --strict --ignoreConfig`):
/// `ReturnType<typeof makeProps>` is `{ label: string; made: any }`, with
/// TS2304 at `notDeclared()` — a name declared nowhere, whose error type
/// the flow-return lane does not model.
///
/// Discrimination: restoring the short-circuit fails the `label: { type:
/// String` assertion; publishing a fabricated constructor for the unmodelled
/// member fails the `made: { type: null` assertion.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn runtime_props_derive_each_member_from_that_members_own_evidence() {
    let RenderedRuntime::Props(props) = render_runtime_props(
        "/src/R1Helper.vue",
        "function makeProps() { const f = () => notDeclared(); return { label: \"x\", made: f() } }",
    ) else {
        panic!(
            "a FAITHFUL degraded surface (marker at one position, every sibling exact) has a \
             member set — the runtime lane must publish it, not refuse"
        );
    };
    assert!(
        props.contains("label: { type: String"),
        "the substrate typed `label` exactly, so its runtime constructor must survive the \
         sibling's degradation:\n{props}"
    );
    assert!(
        !props.contains("label: { type: null"),
        "`label` must NOT be erased to `type: null` — that is the sibling-collapse \
         regression:\n{props}"
    );
    assert!(
        props.contains("made: { type: null"),
        "`made` carries the typed unresolved marker, so the runtime lane must emit it with \
         validation off rather than fabricate a constructor:\n{props}"
    );
}

/// `defineProps<{ [k: string]: string }>()` — a props type argument that is an
/// index-signature-only object literal. A props member is `properties + index
/// signatures`, so the published `define_props` shape MUST carry the index
/// signature even though there is NO named property member.
///
/// Discriminating: the pre-fix `define_props_shape` hardcoded
/// `index_signatures: Vec::new()`, so the published shape dropped the
/// signature and this test FAILS (empty `index_signatures`); the fix preserves
/// the DTO's `prop_index_signatures` so the `[k: string]: string` signature
/// surfaces. (The `properties` list legitimately stays empty — the surface has
/// no named member.)
#[test]
fn evaluate_types_define_props_preserves_index_signature_only_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/IndexProps.vue",
            r#"<script setup lang="ts">
defineProps<{ [key: string]: string }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/IndexProps.vue").unwrap().unwrap();

    let shape = evaluated
        .define_props
        .iter()
        .map(|entry| &entry.result.value)
        .next()
        .expect("an index-signature-only defineProps must still publish a define_props shape");

    assert_eq!(
        shape.index_signatures.len(),
        1,
        "defineProps<{{ [k: string]: string }}> must publish exactly its index \
         signature, got {} index signatures (a `Vec::new()` here means the \
         index signature was dropped — props = properties + index signatures)",
        shape.index_signatures.len(),
    );
    let sig = &shape.index_signatures[0];
    let key_ty = demand_published_type(
        project.host(),
        "/IndexProps.vue",
        sig.key_type.present(),
        "index signature key",
    );
    assert!(
        matches!(key_ty, TypeExpr::Primitive(PrimitiveName::String)),
        "index signature key type is `string`, got {key_ty:?}",
    );
    let value_ty = demand_published_type(
        project.host(),
        "/IndexProps.vue",
        sig.value_type.present(),
        "index signature value",
    );
    assert!(
        matches!(value_ty, TypeExpr::Primitive(PrimitiveName::String)),
        "index signature value type is `string`, got {value_ty:?}",
    );
    assert!(
        !sig.readonly,
        "the `[key: string]: string` signature is not readonly",
    );
}

/// The owner-local projectable-roots PRE-FILTER
/// (`projectable_owner_local_macro_roots`) resolves each candidate root through
/// the SOLE query-time resolver — the shared dispatch surface projection — NOT
/// the retired prepared-decl walker. This pass runs UPSTREAM of the owner-local
/// authority gate, so it is a
/// production resolution path in its own right; it must agree with the authority
/// gate on what "projectable" means.
///
/// An index-signature-only owner-local props root (`type Props = { [k: string]:
/// string }`) projects through dispatch to a shape whose only surface is an index
/// signature. The pre-filter MUST return `Props` as projectable.
///
/// Discriminating: the retargeted per-kind predicate admits props/model/slots
/// roots whose dispatch shape has members OR call-signatures OR index-signatures
/// (`|| !shape.index_signatures.is_empty()`). Reverting that index-signature
/// admission clause drops this index-sig-only root, so the pre-filter returns
/// `[]` and this assertion fails. (Proven by mutation: removing the
/// `index_signatures` clause makes the returned roots empty.)
#[test]
fn projectable_owner_local_pre_filter_admits_index_signature_only_props_root() {
    use crate::host_manage::jsdoc_resolve::HostComponentMetaResolver;
    use crate::resolver_core::component_meta::ComponentMetaResolverHost;
    use verter_session_query::analysis::types::AnalyzedMacroKind;

    let project = make_project();
    project
        .upsert_base(
            "/PreFilterIndex.vue",
            r#"<script setup lang="ts">
type Props = { [key: string]: string }
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // Prime the SFC's IndexedReady (and pull the resolved `AnalyzedMacro` the
    // production cold resolver feeds the pre-filter) through a real session.
    let session = project.open_session_batch().unwrap();
    let _ = session
        .evaluate_types("/PreFilterIndex.vue")
        .unwrap()
        .unwrap();
    let analysis = session
        .get_analysis("/PreFilterIndex.vue")
        .unwrap()
        .expect("analysis should exist");
    let define_props = analysis
        .macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
        .expect("defineProps macro should exist");

    let host = project.host();
    let dispatch =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
    let resolver_host = HostComponentMetaResolver {
        host,
        ctx: host,
        engine: crate::host_manage::jsdoc_resolve::ComponentMetaSemanticServices {
            dispatch: &dispatch,
            session_view: None,
        },
    };
    let roots =
        resolver_host.projectable_owner_local_macro_roots("/PreFilterIndex.vue", define_props);
    assert_eq!(
        roots,
        vec!["Props".to_string()],
        "the owner-local projectable pre-filter MUST admit an index-signature-only \
         props root `type Props = {{ [k: string]: string }}` through the dispatch \
         surface route; an empty result means the retargeted predicate dropped the \
         index-sig-only dispatch shape (the `|| !shape.index_signatures.is_empty()` \
         admission was removed) or the pre-filter regressed to the prepared walker, \
         got {roots:?}",
    );
}

#[test]
fn get_component_meta_uses_evaluated_define_props_from_split_script_sfc() {
    let project = make_project();
    project
        .upsert_base(
            "/types/index.ts",
            "export * from '../Link.vue'\nexport * from '../icons'",
        )
        .unwrap();
    project
        .upsert_base(
            "/icons.ts",
            r#"export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Link.vue",
            r#"<script lang="ts">
interface RouterLinkOptions {
  replace?: boolean
  activeClass?: string
  ariaCurrentValue?: string
}

interface RouterLinkProps extends RouterLinkOptions {
  custom?: boolean
}

export interface LinkProps extends RouterLinkProps {
  href?: string
  raw?: boolean
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Button.vue",
            r#"<script lang="ts">
import type { LinkProps, UseComponentIconsProps } from './types'

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw' | 'custom'> {
  label?: string
  color?: string
}
</script>

<script setup lang="ts">
defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/Button.vue");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"icon"),
        "split-script defineProps should include imported interface members, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"loading"),
        "split-script defineProps should include imported interface members, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"href"),
        "split-script defineProps should include imported Omit survivors, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"replace"),
        "split-script defineProps should include inherited base props, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"label") && prop_names.contains(&"color"),
        "split-script defineProps should keep local props, got: {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"raw") && !prop_names.contains(&"custom"),
        "split-script defineProps should respect Omit, got: {prop_names:?}"
    );
}

#[test]
fn evaluate_types_handles_shadowed_get_item_keys_defaults_without_hanging() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types/utils.ts",
            r#"export type NestedItem<T> = T extends Array<infer I> ? NestedItem<I> : T
export type GetItemKeys<I, T extends NestedItem<I> = NestedItem<I>> = keyof Extract<T, object> & string
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Accordion.vue",
            r#"<script lang="ts">
import type { GetItemKeys } from './src/types/utils'

export interface Item {
  label?: string
  value?: string
  [key: string]: any
}

export interface Props<T extends Item = Item> {
  valueKey?: GetItemKeys<T>
}
</script>

<script setup lang="ts" generic="T extends Item">
defineProps<Props<T>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session
        .evaluate_types("/Accordion.vue")
        .unwrap()
        .expect("evaluate_types should return a result");
    let value_key = evaluated
        .props
        .iter()
        .find(|field| field.name == "valueKey")
        .expect("valueKey should be present");

    assert_eq!(
        value_key.execution_status,
        verter_session_query::analysis::type_expand::ExpansionExecutionStatus::Completed,
    );
    assert!(
        matches!(
            value_key.exactness,
            verter_session_query::analysis::type_expand::ExpansionExactness::ExactSymbolic,
        ) || matches!(
            value_key.exactness,
            verter_session_query::analysis::type_expand::ExpansionExactness::ExactConcrete,
        ),
        "valueKey should resolve without hanging, got {:?}",
        value_key.exactness
    );
}

#[test]
fn evaluate_types_materializes_package_reexported_route_aliases_for_component_props() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/vue-router/package.json".to_string(),
        Arc::from(
            r#"{ "name": "vue-router", "types": "./dist/vue-router.d.ts", "exports": { ".": { "types": "./dist/vue-router.d.ts", "import": "./dist/vue-router.js" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue-router/dist/vue-router.d.ts".to_string(),
        Arc::from(r#"export { Lt as RouteLocationRaw } from "./index-typed.js";"#),
    );
    ws.inject_file(
        "/workspace/node_modules/vue-router/dist/index-typed.d.ts".to_string(),
        Arc::from(
            r#"
export interface St { path: string }
export interface vt { name: string }
export type Lt = string | St | vt
"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue-router/dist/index-typed.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![verter_workspace::ide_project_config(
        "/workspace".to_string(),
        "/workspace".to_string(),
        Some("/workspace/tsconfig.json".to_string()),
    )]);

    let project = MetaProject::new(host);
    project
        .upsert_base(
            "/workspace/src/Link.vue",
            r#"<script lang="ts">
import type { RouteLocationRaw } from 'vue-router'

export interface Props {
  to?: RouteLocationRaw
  href?: Props['to']
}
</script>
<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // Type alias and registry assertions removed — cached_eval_inputs deleted with the legacy walker.

    let session = project.open_session_batch().unwrap();
    let evaluated = session
        .evaluate_types("/workspace/src/Link.vue")
        .unwrap()
        .expect("evaluate_types should return a result");

    // Architectural contract: package-imported alias names stay
    // shallow at the published surface. The `to` prop publishes the
    // bare `Ref { name: "RouteLocationRaw" }` (consumers re-resolve
    // through the package registry). The `href` prop publishes its
    // `IndexedAccess` route — the route stays symbolic because the
    // root resolves to a package-backed declaration.
    let to_ty =
        evaluated_define_props_shallow_type(&project, "/workspace/src/Link.vue", &evaluated, "to");
    assert!(
        matches!(
            &to_ty,
            TypeExpr::Ref { name, .. } if name.as_ref() == "RouteLocationRaw"
        ),
        "to prop should publish the bare RouteLocationRaw ref, got {to_ty:?}"
    );
    let href_ty = evaluated_define_props_shallow_type(
        &project,
        "/workspace/src/Link.vue",
        &evaluated,
        "href",
    );
    assert!(
        matches!(
            href_ty,
            TypeExpr::IndexedAccess { .. } | TypeExpr::Ref { .. } | TypeExpr::Unknown { .. }
        ),
        "href prop should publish the symbolic indexed access, bare ref, or Unknown carrier, got {href_ty:?}"
    );

    let meta = session
        .get_component_meta("/workspace/src/Link.vue")
        .unwrap()
        .expect("get_component_meta should return metadata");
    let to_prop = meta
        .props
        .iter()
        .find(|prop| prop.name == "to")
        .expect("to prop should exist");
    let href_prop = meta
        .props
        .iter()
        .find(|prop| prop.name == "href")
        .expect("href prop should exist");

    let to_prop_ty = shallow_published_type(
        project.host(),
        "/workspace/src/Link.vue",
        to_prop.publication.result().selected_source(),
        "to prop",
    );
    assert!(
        matches!(
            &to_prop_ty,
            TypeExpr::Ref { name, .. } if name.as_ref() == "RouteLocationRaw"
        ),
        "package re-exported route alias should publish the bare RouteLocationRaw ref: {to_prop_ty:?}"
    );
    let href_prop_ty = shallow_published_type(
        project.host(),
        "/workspace/src/Link.vue",
        href_prop.publication.result().selected_source(),
        "href prop",
    );
    assert!(
        matches!(
            &href_prop_ty,
            TypeExpr::IndexedAccess { .. } | TypeExpr::Ref { .. } | TypeExpr::Unknown { .. }
        ),
        "self indexed access through a package alias should publish the symbolic shape: {href_prop_ty:?}"
    );
}

#[test]
fn package_backed_omit_does_not_leak_omitted_editor_members_into_top_level_props() {
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
            "/src/drag.ts",
            r#"
import type { Editor } from 'editor-lib'

export interface DragHandleProps {
  class?: any
  editor?: Editor
  element?: object
  appendTo?: object
  onNodeChange?: () => void
  pluginKey?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { DragHandleProps } from './drag'

type Props = Omit<DragHandleProps, 'editor' | 'element' | 'onNodeChange' | 'class'> & {
  editor: object
}

defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./drag".to_string(),
            resolved_canonical_id: Some("/src/drag.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/drag.ts",
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
        prop_names.contains(&"editor"),
        "top-level editor prop should survive the local override, got {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"appendTo") && prop_names.contains(&"pluginKey"),
        "non-omitted drag props should still be present, got {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"$doc")
            && !prop_names.contains(&"chain")
            && !prop_names.contains(&"active")
            && !prop_names.contains(&"element")
            && !prop_names.contains(&"class")
            && !prop_names.contains(&"onNodeChange"),
        "Omit should not leak omitted package-backed editor members into top-level props: {:?}",
        prop_names
    );
}

#[test]
fn package_backed_object_prop_does_not_flatten_members_into_top_level_props() {
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
    assert!(
        project
            .host()
            .workspace_read()
            .is_package_backed("/node_modules/editor-lib/index.d.ts"),
        "fixture requires package-backed dependency classification"
    );
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Editor } from 'editor-lib'

defineProps<{
  editor: Editor
  label?: string
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
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
        prop_names.contains(&"editor") && prop_names.contains(&"label"),
        "declared props should remain present, got {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"$doc")
            && !prop_names.contains(&"chain")
            && !prop_names.contains(&"active"),
        "package-backed object props should stay nested instead of flattening their members into top-level props: {:?}",
        prop_names
    );
    assert!(
        !registry_names.contains(&"Editor"),
        "direct package-backed public field refs should stay symbolic on the prop instead of being published into the registry, got {registry_names:?}",
    );
}

#[test]
fn package_backed_object_prop_stays_symbolic_in_evaluated_types() {
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
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Editor } from 'editor-lib'

defineProps<{
  editor: Editor
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "editor-lib".to_string(),
            resolved_canonical_id: Some("/node_modules/editor-lib/index.d.ts".to_string()),
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
    let editor_field = resolved
        .evaluated_types
        .as_ref()
        .and_then(|types| types.props.iter().find(|field| field.name == "editor"))
        .expect("expanded evaluated types should keep the editor prop");

    let editor_field_ty = shallow_published_type(
        project.host(),
        "/src/App.vue",
        Some(
            editor_field
                .authority
                .source_position()
                .present()
                .expect("present source"),
        ),
        "editor prop",
    );
    assert!(
        matches!(
            &editor_field_ty,
            verter_type_expr::TypeExpr::Ref { name, type_arguments }
                if name.as_ref() == "Editor" && type_arguments.is_empty()
        ),
        "package-backed prop expansion should keep the raw symbolic ref instead of expanding the package object, got {editor_field_ty:?}"
    );
}

#[test]
fn package_backed_member_path_prop_stays_symbolic_in_evaluated_types() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/table-lib/index.d.ts",
            r#"
export interface CoreOptions<T> {
  state?: T
}

export interface RowState {
  selected?: boolean
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { CoreOptions, RowState } from 'table-lib'

defineProps<{
  state?: CoreOptions<RowState>['state']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "table-lib".to_string(),
            resolved_canonical_id: Some("/node_modules/table-lib/index.d.ts".to_string()),
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
    let state_field = resolved
        .evaluated_types
        .as_ref()
        .and_then(|types| types.props.iter().find(|field| field.name == "state"))
        .expect("expanded evaluated types should keep the state prop");

    let state_field_ty = shallow_published_type(
        project.host(),
        "/src/App.vue",
        Some(
            state_field
                .authority
                .source_position()
                .present()
                .expect("present source"),
        ),
        "state prop",
    );
    assert!(
        matches!(
            &state_field_ty,
            verter_type_expr::TypeExpr::IndexedAccess { object, index }
                if matches!(
                    object.as_ref(),
                    verter_type_expr::TypeExpr::Ref { name, type_arguments }
                        if name.as_ref() == "CoreOptions" && type_arguments.len() == 1
                ) && matches!(
                    index.as_ref(),
                    verter_type_expr::TypeExpr::Literal(
                        verter_type_expr::LiteralValue::String(key),
                    ) if key == "state"
                )
        ),
        "package-backed indexed member paths should stay symbolic instead of expanding through package declarations, got {state_field_ty:?}"
    );
}

#[test]
fn jsdoc_descriptions_propagate_through_barrel_reexports() {
    let project = make_project();
    // Defining file: actual interface with JSDoc comments
    project
        .upsert_base(
            "/src/external-types.ts",
            r#"
interface TooltipRootProps {
  /**
   * The open state of the tooltip when it is initially rendered.
   * Use when you do not need to control its open state.
   */
  defaultOpen?: boolean;
  /**
   * The controlled open state of the tooltip.
   */
  open?: boolean;
  /**
   * Override the duration given to the `Provider` to customise
   * the open delay for a specific tooltip.
   *
   * @defaultValue 700
   */
  delayDuration?: number;
  /**
   * When `true`, clicking on trigger will not close the content.
   * @defaultValue false
   */
  disableClosingTrigger?: boolean;
  /**
   * When `true`, disable tooltip
   * @defaultValue false
   */
  disabled?: boolean;
}

export { TooltipRootProps }
"#,
        )
        .unwrap();
    // Barrel re-export file: imports from defining file and re-exports
    project
        .upsert_base(
            "/src/types.ts",
            r#"
import { TooltipRootProps } from "./external-types";
export { TooltipRootProps };
"#,
        )
        .unwrap();
    // Component imports from the barrel file and extends the type
    project
        .upsert_base(
            "/src/Tooltip.vue",
            r#"<script lang="ts">
import type { TooltipRootProps } from './types'

export interface TooltipProps extends TooltipRootProps {
  /** The text content of the tooltip. */
  text?: string
}
</script>
<script setup lang="ts">
const props = defineProps<TooltipProps>()
</script>
<template><div>{{ props.text }}</div></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Tooltip.vue")
        .expect("should return component meta");

    // Local prop should have its JSDoc
    let text = meta
        .props
        .iter()
        .find(|p| p.name == "text")
        .expect("text prop should exist");
    assert_eq!(
        text.description.as_deref(),
        Some("The text content of the tooltip.")
    );

    // Props inherited from the external package through barrel re-export
    // should also have their JSDoc descriptions
    let default_open = meta
        .props
        .iter()
        .find(|p| p.name == "defaultOpen")
        .expect("defaultOpen prop should exist");
    assert_eq!(
        default_open.description.as_deref(),
        Some("The open state of the tooltip when it is initially rendered.\nUse when you do not need to control its open state."),
        "defaultOpen JSDoc should propagate through barrel re-export"
    );

    let open = meta
        .props
        .iter()
        .find(|p| p.name == "open")
        .expect("open prop should exist");
    assert_eq!(
        open.description.as_deref(),
        Some("The controlled open state of the tooltip."),
        "open JSDoc should propagate through barrel re-export"
    );

    let delay = meta
        .props
        .iter()
        .find(|p| p.name == "delayDuration")
        .expect("delayDuration prop should exist");
    assert_eq!(
        delay.description.as_deref(),
        Some("Override the duration given to the `Provider` to customise\nthe open delay for a specific tooltip."),
        "delayDuration JSDoc should propagate through barrel re-export"
    );
    assert_eq!(
        delay.tags.len(),
        1,
        "delayDuration should have @defaultValue tag"
    );
    assert_eq!(delay.tags[0].name, "defaultValue");
    assert_eq!(delay.tags[0].text.as_deref(), Some("700"));

    let disabled = meta
        .props
        .iter()
        .find(|p| p.name == "disabled")
        .expect("disabled prop should exist");
    assert_eq!(
        disabled.description.as_deref(),
        Some("When `true`, disable tooltip"),
        "disabled JSDoc should propagate through barrel re-export"
    );
    assert_eq!(
        disabled.tags.len(),
        1,
        "disabled should have @defaultValue tag"
    );
    assert_eq!(disabled.tags[0].name, "defaultValue");
    assert_eq!(disabled.tags[0].text.as_deref(), Some("false"));

    // Negative assertion: props that don't exist should not appear
    assert!(
        meta.props.iter().all(|p| p.name != "nonexistent"),
        "no phantom props should be generated"
    );
}

#[test]
fn jsdoc_descriptions_propagate_through_barrel_reexports_with_defaults() {
    let project = make_project();
    project
        .upsert_base(
            "/src/external-types.ts",
            r#"
interface TooltipRootProps {
  /**
   * The open state of the tooltip when it is initially rendered.
   * Use when you do not need to control its open state.
   */
  defaultOpen?: boolean;
  /**
   * The controlled open state of the tooltip.
   */
  open?: boolean;
}

export { TooltipRootProps }
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
import { TooltipRootProps } from "./external-types";
export { TooltipRootProps };
"#,
        )
        .unwrap();
    // Uses withDefaults wrapping defineProps — macro_kind is WithDefaults
    project
        .upsert_base(
            "/src/Tooltip.vue",
            r#"<script lang="ts">
import type { TooltipRootProps } from './types'

export interface TooltipProps extends TooltipRootProps {
  /** The text content of the tooltip. */
  text?: string
  portal?: boolean
}
</script>
<script setup lang="ts">
const props = withDefaults(defineProps<TooltipProps>(), {
  portal: true
})
</script>
<template><div>{{ props.text }}</div></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Tooltip.vue")
        .expect("should return component meta");

    let default_open = meta
        .props
        .iter()
        .find(|p| p.name == "defaultOpen")
        .expect("defaultOpen prop should exist");
    assert_eq!(
        default_open.description.as_deref(),
        Some("The open state of the tooltip when it is initially rendered.\nUse when you do not need to control its open state."),
        "defaultOpen JSDoc should propagate through barrel with withDefaults"
    );

    let open = meta
        .props
        .iter()
        .find(|p| p.name == "open")
        .expect("open prop should exist");
    assert_eq!(
        open.description.as_deref(),
        Some("The controlled open state of the tooltip."),
        "open JSDoc should propagate through barrel with withDefaults"
    );
}

#[test]
fn jsdoc_descriptions_propagate_through_wildcard_reexport() {
    let project = make_project();
    // Defining file with JSDoc-annotated interface
    project
        .upsert_base(
            "/src/link-props.ts",
            r#"
export interface LinkProps {
  /**
   * Force the link to be active independent of the current route.
   */
  active?: boolean;
  /**
   * Class to apply when the link is active
   * @defaultValue ""
   */
  activeClass?: string;
  /**
   * The element or component this component should render as when not a link.
   */
  as?: string;
}
"#,
        )
        .unwrap();
    // Barrel using `export *` wildcard re-export
    project
        .upsert_base(
            "/src/types/index.ts",
            r#"
export * from '../link-props';
"#,
        )
        .unwrap();
    // Component that imports from the barrel
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { LinkProps } from './types/index'

export interface ButtonProps extends LinkProps {
  /** The button label. */
  label?: string
}
</script>
<script setup lang="ts">
const props = defineProps<ButtonProps>()
</script>
<template><button>{{ props.label }}</button></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Button.vue")
        .expect("should return component meta");

    let label = meta
        .props
        .iter()
        .find(|p| p.name == "label")
        .expect("label prop should exist");
    assert_eq!(label.description.as_deref(), Some("The button label."),);

    let active = meta
        .props
        .iter()
        .find(|p| p.name == "active")
        .expect("active prop should exist");
    assert_eq!(
        active.description.as_deref(),
        Some("Force the link to be active independent of the current route."),
        "active JSDoc should propagate through export * barrel"
    );

    let active_class = meta
        .props
        .iter()
        .find(|p| p.name == "activeClass")
        .expect("activeClass prop should exist");
    assert_eq!(
        active_class.description.as_deref(),
        Some("Class to apply when the link is active"),
        "activeClass JSDoc should propagate through export * barrel"
    );
    assert_eq!(
        active_class.tags.len(),
        1,
        "activeClass should have @defaultValue tag"
    );
    assert_eq!(active_class.tags[0].name, "defaultValue");

    let as_prop = meta
        .props
        .iter()
        .find(|p| p.name == "as")
        .expect("as prop should exist");
    assert_eq!(
        as_prop.description.as_deref(),
        Some("The element or component this component should render as when not a link."),
        "as JSDoc should propagate through export * barrel"
    );
}

/// AUDIT-FOOTPRINT BOUND: within the per-position publication finalize
/// (`finalize_published_prop_source` — the props half of
/// `reduce_published_field_types`), the shallow authored-locator raise
/// (`raise_authored_locator_to_hot` — the node the finalize node-compares
/// against the reduced carrier) happens exactly once. The emits /
/// slot_bindings / bindings loops NEVER
/// node-compare a shallow form, so they must not raise one: raising every
/// per-field shallow locator bloats the per-request audit footprint on wide
/// cyclic surfaces (the bound the former per-loop `stamp_shallow_node_id`
/// flag enforced).
#[test]
fn reduce_published_raises_the_shallow_form_only_in_the_props_loop() {
    use syn::visit::Visit;

    const PUBLISHED_FINALIZE_SRC: &str =
        include_str!("../../meta_resolve/projectors/output_sink/published_finalize.rs");

    /// Within `reduce_published_field_types`, count the
    /// `raise_authored_locator_to_hot` method calls.
    #[derive(Default)]
    struct ShallowRaiseCollector {
        depth: usize,
        raises: usize,
    }
    impl<'ast> Visit<'ast> for ShallowRaiseCollector {
        fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
            let hit = f.sig.ident == "finalize_published_prop_source";
            if hit {
                self.depth += 1;
            }
            syn::visit::visit_item_fn(self, f);
            if hit {
                self.depth -= 1;
            }
        }
        fn visit_expr_method_call(&mut self, c: &'ast syn::ExprMethodCall) {
            if self.depth > 0 && c.method == "raise_authored_locator_to_hot" {
                self.raises += 1;
            }
            syn::visit::visit_expr_method_call(self, c);
        }
    }

    let file = syn::parse_file(PUBLISHED_FINALIZE_SRC).expect("parse published_finalize.rs");
    let mut collector = ShallowRaiseCollector::default();
    collector.visit_file(&file);

    assert_eq!(
        collector.raises, 1,
        "reduce_published_field_types must raise the shallow authored form exactly once — in \
         the props loop (the node-compare subject); the emits / slot_bindings / bindings loops \
         never node-compare and must not raise per-field shallow forms (the audit-footprint \
         bound)"
    );
}

#[test]
fn published_default_value_and_default_value_tag_keep_verbatim_source_quoting() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
withDefaults(defineProps<{ orientation?: string, count?: number, active?: boolean, items?: string[] }>(), {
  orientation: 'vertical',
  count: 0,
  active: false,
  items: () => ['a'],
})
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("component meta resolves");

    let prop = |name: &str| {
        meta.props
            .iter()
            .find(|prop| prop.name == name)
            .unwrap_or_else(|| panic!("prop {name} must be published"))
    };

    let orientation = prop("orientation");
    assert_eq!(
        orientation.default_value.as_deref(),
        Some("'vertical'"),
        "string default must publish the verbatim quoted source text"
    );
    assert_ne!(
        orientation.default_value.as_deref(),
        Some("vertical"),
        "string default must not publish the unquoted inner value"
    );
    let default_tag = orientation
        .tags
        .iter()
        .find(|tag| tag.name == "defaultValue")
        .expect("synthesized @defaultValue tag must be present");
    assert_eq!(
        default_tag.text.as_deref(),
        Some("'vertical'"),
        "@defaultValue tag text must carry the verbatim quoted source text"
    );

    // Non-string defaults stay byte-identical — no quoting layer is added.
    assert_eq!(prop("count").default_value.as_deref(), Some("0"));
    assert_eq!(prop("active").default_value.as_deref(), Some("false"));
    assert_eq!(prop("items").default_value.as_deref(), Some("() => ['a']"));
    for name in ["count", "active", "items"] {
        let value = prop(name).default_value.as_deref().unwrap();
        assert!(
            !value.starts_with('\'') && !value.starts_with('"'),
            "non-string default {name} must not gain a quoting layer, got {value}"
        );
    }
}

#[test]
fn same_file_local_interface_prop_jsdoc_publishes_without_text_scan() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script lang="ts">
export interface Inner {
  /** Doc for foo */
  foo: string
  /** Doc for bar.
   * @deprecated use baz
   */
  bar?: number
}
</script>
<script setup lang="ts">
defineProps<Inner>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project.host().get_component_meta("/App.vue").expect("meta");

    let foo = meta
        .props
        .iter()
        .find(|prop| prop.name == "foo")
        .expect("foo must surface");
    assert_eq!(foo.description.as_deref(), Some("Doc for foo"));

    let bar = meta
        .props
        .iter()
        .find(|prop| prop.name == "bar")
        .expect("bar must surface");
    assert_eq!(bar.description.as_deref(), Some("Doc for bar."));
    let deprecated = bar
        .tags
        .iter()
        .find(|tag| tag.name == "deprecated")
        .expect("@deprecated tag must publish");
    assert_eq!(deprecated.text.as_deref(), Some("use baz"));
}

/// SOLE-AUTHORITY discrimination: the `define_props` SHAPE lane publishes
/// the NORMALIZED prop row's member-value source — the flat
/// `evaluated_types.props` projection contributes metadata only and can
/// never shadow it. The imported function-valued member is a production
/// divergence case: the legacy flat-field preference published the flat
/// row's value onto the lane here, while the normalized row carries the
/// faithful projected MEMBER-PATH replay source — so a lane publishing the
/// normalized value proves the authority order, and the extracted meta
/// position agrees with the lane (one authority end to end).
#[test]
fn normalized_prop_rows_are_the_published_source_authority() {
    let project = make_project();
    project
        .upsert_base(
            "/props.ts",
            "export interface Props { onClick: () => void }\n",
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Props } from './props'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let (analysis, state) = project
        .host()
        .get_component_meta_with_resolution("/App.vue")
        .expect("component resolves");
    let evaluated = state
        .evaluated_types
        .as_ref()
        .expect("evaluated types present");
    let lane = &evaluated.define_props[0].result.value;
    let lane_prop = lane
        .properties
        .iter()
        .find(|property| property.name == "onClick")
        .expect("the lane publishes the onClick property");
    // The lane position IS the normalized row's source (the projected
    // member-path replay route) — never the flat projection's value.
    let lane_source = match lane_prop.ty.present() {
        Some(verter_type_expr::facts::SemanticTypeSource::Projected(
            verter_type_expr::facts::ProjectedTypeFact::MemberPath { path, .. },
        )) => {
            assert_eq!(
                path.as_ref(),
                [verter_type_engine::semantic_query::PropertyKey::identifier(
                    "onClick"
                )],
                "one member hop"
            );
            lane_prop.ty.clone()
        }
        other => panic!(
            "the define_props lane publishes the NORMALIZED member-path \
             source, got {other:?}"
        ),
    };
    // The extracted meta position agrees with the lane — one authority end
    // to end (extraction publishes the normalized-derived lane, no
    // post-extract overwrite).
    let meta_prop = analysis
        .props
        .iter()
        .find(|prop| prop.name == "onClick")
        .expect("the meta publishes the onClick prop");
    assert_eq!(
        meta_prop.publication.source_position(),
        lane_source,
        "the extracted position IS the lane's normalized source"
    );
}

/// POSITIVE CONTROL: a shallow prop whose value IS recoverable — an
/// authored inline annotation (`msg: string`), a local alias reference
/// (`alias: MyAlias`), and an authored inline `unknown` — still completes
/// as `Present`. The member-value fail-close must touch ONLY the
/// no-faithful-source residue, never a recoverable member.
#[test]
fn recoverable_shallow_prop_values_still_complete_as_present() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
type MyAlias = number
defineProps<{ msg: string, alias: MyAlias, open: unknown }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let output = project
        .host()
        .get_component_meta_output("/App.vue")
        .expect("recoverable shallow member values complete as Present, never a failure")
        .expect("component must resolve");
    let (analysis, _resolution, types) = output.into_parts();
    let lanes = types.into_lanes();
    let position = |name: &str| {
        analysis
            .props
            .iter()
            .position(|prop| prop.name == name)
            .unwrap_or_else(|| panic!("the {name} prop publishes"))
    };
    for prop in analysis.props.iter() {
        assert!(
            prop.publication.source_position().is_present(),
            "the recoverable {} prop publishes a PRESENT source; got {:?}",
            prop.name,
            prop.publication.source_position()
        );
    }
    assert_eq!(
        published_type(&lanes.props[position("msg")]),
        &TypeExpr::Primitive(PrimitiveName::String),
        "the authored primitive renders as written"
    );
    assert!(
        matches!(
            lanes.props[position("alias")].materialized_type().expect("published type"),
            TypeExpr::Ref { name, .. } if name.as_ref() == "MyAlias"
        ),
        "the local alias stays the shallow authored reference; got {:?}",
        lanes.props[position("alias")]
    );
    assert_eq!(
        published_type(&lanes.props[position("open")]),
        &TypeExpr::Primitive(PrimitiveName::Unknown),
        "an AUTHORED `unknown` is a PRESENT success rendered as the author wrote it"
    );
    let (_analysis, state) = project
        .host()
        .get_component_meta_with_resolution("/App.vue")
        .expect("resolves");
    assert!(
        !state.completeness.is_partial(),
        "recoverable members complete; the fail-close must not overfire"
    );
}

/// Public macro regression: the signature utilities are KIND-aware
/// over a retained direct constructor type in a `defineProps` payload —
/// `ConstructorParameters<new (x: string) => object>` materialises the
/// `[x: string]` tuple and `InstanceType<new () => { made: string }>` the
/// instance type (never `Opaque(Miss)`).
#[test]
fn define_props_constructor_parameters_and_instance_type_over_direct_constructor() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{
  args: ConstructorParameters<new (x: string) => object>,
  inst: InstanceType<new () => { made: string }>,
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session.evaluate_types("/App.vue").unwrap().unwrap();

    // `args` = [x: string].
    match &evaluated_define_props_type(&project, "/App.vue", &evaluated, "args") {
        TypeExpr::Tuple { elements, .. } => {
            assert_eq!(elements.len(), 1, "ConstructorParameters tuple arity");
            assert_eq!(
                elements[0].ty,
                TypeExpr::Primitive(PrimitiveName::String),
                "the constructor's parameter type flows into the tuple"
            );
        }
        other => panic!(
            "ConstructorParameters<new (x: string) => object> must evaluate to \
             a tuple, got {other:?}"
        ),
    }

    // `inst` = { made: string }.
    match &evaluated_define_props_type(&project, "/App.vue", &evaluated, "inst") {
        TypeExpr::Object(obj) => {
            assert_eq!(obj.properties.len(), 1, "InstanceType instance surface");
        }
        other => panic!(
            "InstanceType<new () => {{ made: string }}> must evaluate to the \
             instance object, got {other:?}"
        ),
    }
}
