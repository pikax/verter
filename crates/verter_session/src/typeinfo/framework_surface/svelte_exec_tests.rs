//! Tests for the Svelte resolution leg (extracted from `svelte_exec.rs`
//! to keep the production module under the oversize-file guard).

use super::*;
use verter_compiler::svelte::parser::parse_svelte;

/// The owner canonical id the legacy `<slot>` walk tests thread as the
/// binding resolution scope.
const LEGACY_SLOT_OWNER: &str = "/Component.svelte";

/// Collect the legacy `<slot>` slot fields from a `.svelte` SOURCE through the
/// same structural walk the resolver uses (the typed template carrier),
/// scoped to [`LEGACY_SLOT_OWNER`].
fn legacy_slots(source: &str) -> Vec<AnalyzedSlotField> {
    let parsed = parse_svelte(source);
    let mut slots = Vec::new();
    collect_slot_elements(&parsed.template, source, LEGACY_SLOT_OWNER, &mut slots);
    slots
}

#[test]
fn legacy_slot_names_are_exact_and_dedup_first_writer_wins() {
    // F9: the legacy `<slot>` inventory walk yields EXACT slot NAMES from the
    // typed template AST — precise, structural, never a source-text scan.
    let slots = legacy_slots(
        "<div><slot /></div><slot name=\"header\" /><slot name=\"header\" item={x} />",
    );
    let names: Vec<&str> = slots.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"default"), "the bare <slot> is `default`");
    assert!(names.contains(&"header"), "the named <slot> is `header`");
    // First-writer-wins on a duplicate name (the `header` slot appears once).
    assert_eq!(
        names.iter().filter(|n| **n == "header").count(),
        1,
        "duplicate slot names dedup first-writer-wins, got {names:?}"
    );
}

#[test]
#[ignore = "NAMED FOLLOW-UP (owner-decided carve-out): a legacy `<slot name=x let:b>` / \
                forwarded `<slot attr={expr}>` binding VALUE type is currently `any` — a \
                DOCUMENTED deprecated-path carve-out scoped to legacy-<slot> bindings ONLY (the \
                slot NAMES are precise). Precise parse-domain forwarded-expression capture (typing \
                each binding from its `attr={expr}` through the shared engine) is the follow-up. \
                This test asserts the binding's published `type_annotation` is PRECISE (NOT the \
                `any` display); it is RED today (the carve-out publishes `any`) and flips green \
                (ignore removed) when the precise-capture follow-up lands."]
fn legacy_slot_let_binding_value_precision_is_a_followup() {
    // DISCRIMINATING: the forwarded `item={items[0]}` binding's value type must
    // be PRECISE (resolved from the forwarded expression), NOT the `any`
    // carve-out. Today `slot_bindings` publishes the `any` display, so this RED
    // assertion is ledgered behind `#[ignore]`. When the precise forwarded-
    // expression capture lands, `type_annotation` renders the resolved type and
    // this assertion passes — the ignore is then removed.
    let slots = legacy_slots(
        "<script lang=\"ts\">let items: { id: number }[] = []; void items;</script>\n\
             <slot name=\"row\" item={items[0]} />",
    );
    let row = slots
        .iter()
        .find(|s| s.name == "row")
        .expect("the `row` slot is collected");
    let binding = row
        .bindings
        .iter()
        .find(|b| b.name == "item")
        .expect("the forwarded `item` binding is collected");
    assert!(
        binding.type_annotation.as_deref() != Some("any"),
        "the legacy slot binding value must be PRECISE (not the `any` carve-out) — \
             follow-up: precise forwarded-expression capture"
    );
}

#[test]
fn legacy_slot_binding_display_value_carries_its_resolution_scope() {
    // VALUE⇔SCOPE PAIRING INVARIANT: a locator-less display VALUE must carry
    // its resolution SCOPE (`type_annotation.is_some() <=>
    // binding_expr_scope.is_some()`), the documented
    // `AnalyzedSlotFieldBinding` rule for resolver-published display-only
    // values. Even the `any` carve-out value is a published display value, so
    // it rides with the owning component's scope. The binding stays
    // locator-less (`payload: None`): a template `<slot>` attribute has no
    // authored TYPE position to address — never a fabricated locator. This is
    // DISCRIMINATING: it FAILS if `slot_bindings` publishes the display value
    // with a `None` scope (or fabricates a payload locator).
    let slots = legacy_slots("<slot name=\"row\" item={x} />");
    let binding = slots
        .iter()
        .find(|s| s.name == "row")
        .and_then(|s| s.bindings.iter().find(|b| b.name == "item"))
        .expect("the forwarded `item` binding is collected");
    assert!(
        binding.type_annotation.is_some(),
        "the carve-out publishes a display value (the pairing assert below is non-vacuous)"
    );
    assert_eq!(
        binding.type_annotation.is_some(),
        binding.binding_expr_scope.is_some(),
        "a locator-less display value must carry its resolution scope (value⇔scope pairing)"
    );
    assert!(
        binding.payload.is_none(),
        "a template <slot> attribute has no authored TYPE position — locator-less, never fabricated"
    );
    assert_eq!(
        binding
            .binding_expr_scope
            .as_ref()
            .map(verter_type_expr::TypeExprScope::as_str),
        Some(LEGACY_SLOT_OWNER),
        "the resolution scope is the owning component's canonical id"
    );
}

/// Build a host carrying ONE `.svelte` source under `canonical`, returning
/// the host plus a PROVEN-CURRENT base view — the caller builds the request
/// `ResolverContext` inline (the ctx borrows both, so it cannot outlive a
/// helper that owns them).
fn host_with_svelte(
    canonical: &str,
    source: &str,
) -> (
    std::sync::Arc<VerterHost>,
    crate::resolver_store::CurrentHostStoreView,
) {
    use crate::{HostConfig, UpsertRequest};
    use verter_language::FileLanguage;
    let host = std::sync::Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::svelte(),
            aliases: Vec::new(),
        })
        .unwrap_or_else(|e| panic!("upsert: {e:?}"));
    let view = crate::typeinfo::current_store_view_for_query(&host).expect("current store view");
    (host, view)
}

#[test]
fn instance_export_type_resolution_uses_the_exact_binding_owner() {
    let canonical = "/OwnerExact.svelte";
    let source = "<script module lang=\"ts\">\n\
             export const shared: string = 'module';\n\
             </script>\n\
             <script lang=\"ts\">\n\
             export const shared: number = 1;\n\
             </script>\n\
             <div />";
    let (host, view) = host_with_svelte(canonical, source);
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let ctx = crate::resolver_core::HostResolverContext::from_current(&host, &view, overlay);

    let facts = host
        .resolve_svelte_script_facts_with_ctx(&ctx, canonical)
        .expect("svelte facts");
    let export = facts
        .instance_exports
        .iter()
        .find(|export| export.exported_name == "shared")
        .expect("instance shared export");
    assert_eq!(
        export.binding_key.owner,
        verter_type_expr::TopLevelOwnerId::instance(0),
        "the capture preserves the instance binding owner"
    );

    let outcome =
        resolve_svelte_surface(&host, &ctx, canonical, SvelteSurfaceSource::InstanceExports);
    let ResolvedOutcome::Resolved(dtos) = outcome else {
        panic!("the instance-export surface must resolve, got {outcome:?}");
    };
    let shared = dtos
        .expose
        .as_ref()
        .and_then(|surface| {
            surface
                .members
                .iter()
                .find(|member| member.name == "shared")
        })
        .expect("the instance `shared` member publishes");
    assert_eq!(
        shared.value,
        Some(
            crate::typeinfo::framework_surface::results::NamedTypeMemberOutput::Primitive(
                verter_type_expr::PrimitiveName::Number,
            )
        ),
        "the instance binding resolves as number, never the module string binding"
    );
    assert_eq!(shared.type_annotation.as_deref(), Some("number"));
}

#[test]
fn exported_route_closure_keeps_same_name_class_owners_disjoint() {
    let canonical = "/RouteOwnerExact.svelte";
    let source = "<script module lang=\"ts\">\n\
             import type { ModuleDep } from './module-dep';\n\
             class Shared { value!: ModuleDep }\n\
             export { Shared as ModuleShared };\n\
             </script>\n\
             <script lang=\"ts\">\n\
             import type { InstanceDep } from './instance-dep';\n\
             class Shared { value!: InstanceDep }\n\
             export { Shared as InstanceShared };\n\
             </script>\n\
             <div />";
    let (host, _view) = host_with_svelte(canonical, source);

    let state = host
        .routed_shallow_state(canonical)
        .expect("route-owner fixture indexes");
    let instance_owner = verter_type_expr::TopLevelOwnerId::instance(0);
    let module_owner = verter_type_expr::TopLevelOwnerId::module(0);
    assert_eq!(
        state.required_declaration_import_names_in(instance_owner, "Shared"),
        rustc_hash::FxHashSet::from_iter(["InstanceDep".to_string()]),
        "the instance declaration-carrier closure is exact-owner"
    );
    assert_eq!(
        state.required_declaration_import_names_in(module_owner, "Shared"),
        rustc_hash::FxHashSet::from_iter(["ModuleDep".to_string()]),
        "the module declaration-carrier closure is exact-owner"
    );

    let instance = host.required_import_routes_for_exported_route(
        canonical,
        "InstanceShared",
        &crate::resolver_core::RouteDemand::Whole,
    );
    assert_eq!(
        instance.get("InstanceDep"),
        Some(&crate::resolver_core::RouteDemand::Whole),
        "the instance-owner class supplement keeps its own imported dependency"
    );
    assert!(
        !instance.contains_key("ModuleDep"),
        "the same-name module-owner class must not contaminate instance closure"
    );

    let module = host.required_import_routes_for_exported_route(
        canonical,
        "ModuleShared",
        &crate::resolver_core::RouteDemand::Whole,
    );
    assert_eq!(
        module.get("ModuleDep"),
        Some(&crate::resolver_core::RouteDemand::Whole),
    );
    assert!(!module.contains_key("InstanceDep"));
}

/// Build a WORKSPACE host (rooted at `/workspace`) carrying one `.svelte`
/// component plus extra supporting files injected into the VFS. A bare
/// `svelte` import laid out under `/workspace/node_modules/svelte/` (with a
/// `package.json`) resolves PACKAGE-BACKED, so a snippet member validates;
/// a relative import (`./types`) resolves workspace-owned. Returns the host
/// + a proven-current base view.
fn workspace_host_with_svelte(
    component_canonical: &str,
    component_source: &str,
    extra: &[(&str, &str)],
) -> (
    std::sync::Arc<VerterHost>,
    crate::resolver_store::CurrentHostStoreView,
) {
    use crate::HostConfig;
    use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

    #[allow(deprecated)]
    let project_graph =
        verter_workspace::ProjectGraph::from_configs(vec![verter_workspace::VfsProjectConfig {
            root: "/workspace".to_string(),
            rank: verter_workspace::ProjectRank::Explicit,
            tsconfig_path: Some("/workspace/tsconfig.json".to_string()),
            root_files: vec![],
            extensions: vec![],
            workspace_root: "/workspace".to_string(),
            workspace_aliases: vec![],
            compiler_options: verter_workspace::IdeProjectCompilerOptions::default(),
            references: vec![],
            membership: verter_workspace::ConfiguredMembership::match_all_under_root(
                &verter_workspace::CanonicalPath::new("/workspace"),
            ),
        }]);
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.set_project_graph(project_graph);
    for (canonical, content) in extra {
        workspace.inject_file((*canonical).into(), Arc::from(*content));
    }
    workspace.inject_file(component_canonical.into(), Arc::from(component_source));
    let ws_access: Arc<dyn WorkspaceAccess> = workspace;
    let host = Arc::new(VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws_access,
    ));
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    let view = crate::typeinfo::current_store_view_for_query(&host).expect("current store view");
    (host, view)
}

#[test]
fn public_api_resolves_local_dispatcher_interface_through_shared_surface() {
    // The dispatcher macro mirrors `$props()`: capture owns its authored type
    // locator and the public projector consumes the shared LegacyDispatcher
    // surface. A local interface is intentionally not available by name in the
    // generated declaration module, so the carrier must render its resolved
    // event map rather than `Events`, `{}`, or `unknown`.
    let component = "/workspace/Eventful.svelte";
    let source = r#"<script lang="ts">
      import { createEventDispatcher } from 'svelte';
      let { label }: { label: string } = $props();
      interface Events { save: string; update: [id: number] }
      const dispatch = createEventDispatcher<Events>();
      void dispatch; void label;
    </script>
    <button>save</button>"#;
    let (host, _view) = workspace_host_with_svelte(
        component,
        source,
        &[
            (
                "/workspace/node_modules/svelte/package.json",
                r#"{"name":"svelte","version":"5.56.3","types":"index.d.ts"}"#,
            ),
            (
                "/workspace/node_modules/svelte/index.d.ts",
                "export declare function createEventDispatcher<E>(): (name: keyof E, detail: E[keyof E]) => void;\n",
            ),
        ],
    );
    let _ = host
        .upsert(crate::UpsertRequest {
            canonical_id: Some(component.to_string()),
            input_id: component.to_string(),
            source: Arc::from(source),
            file_language: verter_language::FileLanguage::svelte(),
            aliases: Vec::new(),
        })
        .expect("load the component into the public-API runtime");
    let facts = host
        .resolve_svelte_script_facts(component)
        .expect("resolved Svelte script facts");
    assert!(
        facts.dispatcher_events.is_some(),
        "the package-backed createEventDispatcher import must validate before public projection: \
         {facts:?}"
    );

    let declaration = host
        .get_public_api_with_mode(component, crate::PublicApiMode::Declaration, None)
        .expect("Svelte public API projection")
        .expect("a dispatcher-bearing Svelte component projects a public API")
        .code
        .to_string();

    assert!(
        declaration.contains("save: string") && declaration.contains("update: [id: number]"),
        "the local dispatcher interface must resolve into the public event map:\n{declaration}"
    );
    assert!(
        declaration.contains("CustomEvent<")
            && !declaration.contains("CustomEvent<any>")
            && !declaration.contains("keyof (Events)"),
        "the public event handlers must carry concrete dispatcher payloads without a local-name leak:\n{declaration}"
    );
}

#[test]
fn realized_snippet_call_signature_is_this_plus_rest_tuple() {
    // IMPL-VERIFY (discriminating): confirm the ACTUAL realized shape
    // a `Snippet<[item: Item, index: number]>` member lowers to through the
    // shared resolver — the vendored `Snippet<Params>` call signature is
    // `(this: void, ...args: Params)`, so the realized callable MUST carry a
    // LEADING `this` param AND a trailing REST param whose type is the tuple
    // `[item: Item, index: number]`. The Svelte normalizer's this-skip +
    // rest-tuple-expansion depends on exactly this shape; this test pins it
    // so a future resolver change that pre-expands the rest (or drops `this`)
    // is caught. Uses a WORKSPACE-LOCAL `Snippet` interface (no package
    // gate) so the realization path is exercised directly.
    let component = "/workspace/RealizedShape.svelte";
    let source = "<script lang=\"ts\">\n\
             import type { Snippet } from './snippet';\n\
             interface Item { id: number }\n\
             interface Props { row: Snippet<[item: Item, index: number]> }\n\
             let { row }: Props = $props();\n\
             void row;\n\
             </script>\n\
             <div />";
    let (host, view) = workspace_host_with_svelte(
        component,
        source,
        &[(
            "/workspace/snippet.ts",
            "export interface Snippet<Params extends unknown[] = []> {\n\
                 (this: void, ...args: Params): { __brand: 'snippet' };\n\
                 }\n",
        )],
    );
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let ctx = crate::resolver_core::HostResolverContext::from_current(&host, &view, overlay);

    // Resolve the props surface, find the `row` member, realize its value to
    // the callable through the SAME shared substrate the normalizer uses.
    let facts = host
        .resolve_svelte_script_facts_with_ctx(&ctx, component)
        .expect("svelte facts");
    let props_type = facts.props_type.as_ref().expect("props type");
    let surface =
        navigate_param_to_object_surface(&ctx, component, props_type).expect("props surface");
    let row_member = surface
        .members
        .iter()
        .find(|m| m.name.as_ref() == "row")
        .expect("the `row` member is present");
    let dispatch = ctx.dispatch();
    let realized = crate::meta_resolve::dispatch_helpers::realize_callable_member(
        &dispatch,
        row_member.value,
        crate::semantic_query::ProjectionReductionContext::published(
            crate::semantic_query::ProjectionMode::Navigate,
        ),
    )
    .unwrap_or(row_member.value);
    let value = dispatch
        .materialize_output_type_expr_for_test(realized)
        .expect("the realized snippet member raises to a TypeExpr");
    // VERIFIED ACTUAL SHAPE: the resolver keeps the `Snippet<Params>` carrier
    // as a `Ref` whose SINGLE type argument is the `Params` tuple
    // `[item: Item, index: number]` (it does NOT reduce the structural
    // interface call signature to a bare `Function` under Navigate). The
    // normalizer therefore reads the carrier's tuple type argument directly.
    let TypeExpr::Ref { type_arguments, .. } = &value else {
        panic!("the realized snippet member is a `Snippet<Params>` Ref carrier, got {value:?}");
    };
    let [TypeExpr::Tuple { elements, .. }] = type_arguments.as_ref() else {
        panic!("the carrier's single type argument is the `Params` tuple, got {type_arguments:?}");
    };
    let element_labels: Vec<Option<&str>> = elements.iter().map(|e| e.label.as_deref()).collect();
    assert_eq!(
        element_labels,
        vec![Some("item"), Some("index")],
        "the `Params` tuple carries BOTH labelled elements, got {element_labels:?}"
    );
    // And the node-domain snippet reader over the SAME member yields the two
    // ordered positional binding NODES, which the terminal DTO sink
    // materializes into the published bindings (the integration of shape +
    // reader + sink).
    let context = crate::semantic_query::ProjectionReductionContext::published(
        crate::semantic_query::ProjectionMode::Navigate,
    );
    let params = CallableNodeView::new(&dispatch, row_member.value)
        .validated_snippet_positional_params(context)
        .expect("the snippet member yields positional params");
    let labels: Vec<Option<&str>> = params.iter().map(|p| p.label.as_deref()).collect();
    assert_eq!(labels, vec![Some("item"), Some("index")]);
    let bindings = materialize_snippet_slot_bindings(
        &ctx,
        &verter_type_expr::TypeExprScope::new(component),
        &params,
    );
    let names: Vec<&str> = bindings.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, vec!["item", "index"]);
}

// ─────────── node-domain snippet reader + terminal DTO sink (golden) ───────────
//
// These drive the SAME production pair the snippet-slot normalizer composes:
// `CallableNodeView::validated_snippet_positional_params` (the node-domain
// positional reader) + `materialize_snippet_slot_bindings` (the terminal DTO
// sink). Fixtures are interned graph nodes; every assertion is an exact node /
// DTO fact (labels, order, exact `TypeExpr` values, `arg{index}` fallback, the
// pairing invariant).

fn snippet_graph(
    host: &VerterHost,
) -> std::sync::Arc<crate::semantic_query_memo::SemanticGraphStore> {
    Arc::clone(host.project_type_store().semantic_graph())
}

fn nav_context() -> crate::semantic_query::ProjectionReductionContext {
    crate::semantic_query::ProjectionReductionContext::published(
        crate::semantic_query::ProjectionMode::Navigate,
    )
}

fn nprim(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    kind: crate::semantic_query::PrimitiveKind,
) -> crate::semantic_query::SemanticNodeId {
    graph.intern_node(crate::semantic_query::SemanticNodeData::Primitive(kind))
}

fn ntuple(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    elements: Vec<(Option<&str>, crate::semantic_query::SemanticNodeId)>,
) -> crate::semantic_query::SemanticNodeId {
    let elements: Vec<crate::semantic_query::TupleElement> = elements
        .into_iter()
        .map(|(label, value)| crate::semantic_query::TupleElement {
            label: label.map(Arc::from),
            value,
            optional: false,
            rest: false,
        })
        .collect();
    graph.intern_node(crate::semantic_query::SemanticNodeData::Tuple {
        elements: Arc::from(elements.into_boxed_slice()),
        readonly: false,
    })
}

/// A synthetic `Snippet<args...>` `InstantiationRef` carrier (the shape a
/// validated `Snippet<Params>` member value lowers to). The `__builtin__` base
/// makes the carrier un-instantiable, so the carrier-preserving peel reads its
/// `args` directly — exactly the validated-snippet read path.
fn nsnippet(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    args: Vec<crate::semantic_query::SemanticNodeId>,
) -> crate::semantic_query::SemanticNodeId {
    graph.intern_node(crate::semantic_query::SemanticNodeData::InstantiationRef {
        base: crate::semantic_query::DeclIdentity {
            canonical_id: Arc::from("__builtin__"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: crate::semantic_query::HashValue::default(),
            decl_name: Arc::from("Snippet"),
        },
        args: Arc::from(args.into_boxed_slice()),
    })
}

/// The realized snippet call-signature shape `(this: void, ...args: <tuple>)`
/// as a `Function` NODE — the fallback shape the reader handles when a
/// snippet's call signature reduced.
fn nsnippet_function(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    rest_tuple: crate::semantic_query::SemanticNodeId,
) -> crate::semantic_query::SemanticNodeId {
    let void = nprim(graph, crate::semantic_query::PrimitiveKind::Void);
    graph.intern_node(crate::semantic_query::SemanticNodeData::Function {
        params: Arc::from(
            vec![
                crate::semantic_query::FunctionParam::synthetic(
                    Some(Arc::from("this")),
                    void,
                    false,
                    false,
                ),
                crate::semantic_query::FunctionParam::synthetic(
                    Some(Arc::from("args")),
                    rest_tuple,
                    false,
                    true,
                ),
            ]
            .into_boxed_slice(),
        ),
        return_type: void,
        type_parameters: Arc::from(Vec::new().into_boxed_slice()),
        signature_span: None,
        return_type_span: None,
    })
}

#[test]
fn snippet_carrier_params_tuple_expands_to_ordered_dto_bindings() {
    // CORE (PRIMARY shape): a `Snippet<[item: string, index: number]>` carrier
    // expands its single `Params` tuple into TWO ordered positional NODES —
    // `item` then `index` — and the terminal DTO sink publishes them with
    // exact names, exact types, and the pairing invariant.
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    let host = VerterHost::new_standalone(crate::types::HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = snippet_graph(&host);

    let item_ty = nprim(&graph, crate::semantic_query::PrimitiveKind::String);
    let index_ty = nprim(&graph, crate::semantic_query::PrimitiveKind::Number);
    let params_tuple = ntuple(
        &graph,
        vec![(Some("item"), item_ty), (Some("index"), index_ty)],
    );
    let snippet = nsnippet(&graph, vec![params_tuple]);

    let params = CallableNodeView::new(&dispatch, snippet)
        .validated_snippet_positional_params(nav_context())
        .expect("a Snippet carrier yields positional params");
    let labels: Vec<Option<&str>> = params.iter().map(|p| p.label.as_deref()).collect();
    assert_eq!(
        labels,
        vec![Some("item"), Some("index")],
        "the carrier's `Params` tuple expands to ALL positions in order"
    );
    assert_eq!(
        params[0].ty, item_ty,
        "position 0 is the exact element node"
    );
    assert_eq!(
        params[1].ty, index_ty,
        "position 1 is the exact element node"
    );

    let bindings = materialize_snippet_slot_bindings(
        &host,
        &verter_type_expr::TypeExprScope::new("/Owner.svelte"),
        &params,
    );
    let names: Vec<&str> = bindings.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["item", "index"],
        "the DTO bindings keep the order"
    );
    assert_eq!(
        bindings[0].type_annotation.as_deref(),
        Some("string"),
        "the `item` binding materializes to `string` (rendered at the terminal sink), got {:?}",
        bindings[0].type_annotation
    );
    assert_eq!(
        bindings[1].type_annotation.as_deref(),
        Some("number"),
        "the `index` binding materializes to `number` (rendered at the terminal sink), got {:?}",
        bindings[1].type_annotation
    );
    let scope = verter_type_expr::TypeExprScope::new("/Owner.svelte");
    // Pairing invariant: every binding value rides with the member scope.
    assert!(
        bindings
            .iter()
            .all(|b| b.binding_expr_scope.as_ref() == Some(&scope)),
        "each binding value is paired with the slot member's scope"
    );
}

#[test]
fn snippet_carrier_empty_params_tuple_yields_present_bindingless_slot() {
    // A `Snippet<[]>` carrier is a PRESENT slot with NO bindings: the reader
    // yields `Some(vec![])` (never `None` — the slot must not be dropped) and
    // the DTO sink publishes an empty binding list.
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    let host = VerterHost::new_standalone(crate::types::HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = snippet_graph(&host);

    let snippet = nsnippet(&graph, vec![ntuple(&graph, Vec::new())]);
    let params = CallableNodeView::new(&dispatch, snippet)
        .validated_snippet_positional_params(nav_context())
        .expect("a `Snippet<[]>` is a PRESENT slot (not dropped)");
    assert!(
        params.is_empty(),
        "an empty `Params` tuple has no positions"
    );

    assert!(
        materialize_snippet_slot_bindings(
            &host,
            &verter_type_expr::TypeExprScope::new("/Owner.svelte"),
            &params,
        )
        .is_empty(),
        "a `Snippet<[]>` publishes NO bindings"
    );
}

#[test]
fn snippet_carrier_open_generic_params_is_present_bindingless() {
    // A `Snippet<Params>` whose single arg is an OPEN generic (`TypeParam`) is
    // a PRESENT, binding-less slot: the resolved non-tuple `Params` yields
    // `Some(vec![])`, never `None` (dropped) and never fabricated bindings.
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    let host = VerterHost::new_standalone(crate::types::HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = snippet_graph(&host);

    let open = graph.intern_node(crate::semantic_query::SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("Params"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("Params"),
    });
    let snippet = nsnippet(&graph, vec![open]);
    assert_eq!(
        CallableNodeView::new(&dispatch, snippet)
            .validated_snippet_positional_params(nav_context()),
        Some(Vec::new()),
        "an open-generic `Snippet<Params>` is a PRESENT, binding-less slot"
    );
}

#[test]
fn snippet_function_fallback_skips_this_and_expands_rest_tuple_to_dto_bindings() {
    // The realized `Function` fallback `(this: void, ...args: [item: string,
    // index: number])`: the reader SKIPS the leading `this` and EXPANDS the
    // rest-tuple into TWO ordered positions; the DTO sink publishes exact
    // names + types. A first-param-only reader (the Vue slot rule) would
    // surface only `this` and FAIL every assertion below.
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    let host = VerterHost::new_standalone(crate::types::HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = snippet_graph(&host);

    let item_ty = nprim(&graph, crate::semantic_query::PrimitiveKind::String);
    let index_ty = nprim(&graph, crate::semantic_query::PrimitiveKind::Number);
    let rest_tuple = ntuple(
        &graph,
        vec![(Some("item"), item_ty), (Some("index"), index_ty)],
    );
    let callable = nsnippet_function(&graph, rest_tuple);

    let params = CallableNodeView::new(&dispatch, callable)
        .validated_snippet_positional_params(nav_context())
        .expect("a realized snippet callable yields positional params");
    let labels: Vec<Option<&str>> = params.iter().map(|p| p.label.as_deref()).collect();
    assert_eq!(
        labels,
        vec![Some("item"), Some("index")],
        "`this` is skipped and the rest-tuple expands in order"
    );

    let bindings = materialize_snippet_slot_bindings(
        &host,
        &verter_type_expr::TypeExprScope::new("/Owner.svelte"),
        &params,
    );
    let names: Vec<&str> = bindings.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["item", "index"],
        "ALL positional params in order"
    );
    assert!(
        !names.contains(&"this"),
        "the leading `this` param must be skipped"
    );
    assert_eq!(
        bindings[0].type_annotation.as_deref(),
        Some("string"),
        "binding 0 is `string`, got {:?}",
        bindings[0].type_annotation
    );
    assert_eq!(
        bindings[1].type_annotation.as_deref(),
        Some("number"),
        "binding 1 is `number`, got {:?}",
        bindings[1].type_annotation
    );
    assert!(
        bindings.iter().all(|b| b.binding_expr_scope.is_some()),
        "each binding is paired with a scope (pairing invariant)"
    );
}

#[test]
fn snippet_function_empty_rest_tuple_yields_no_dto_bindings() {
    // `(this: void, ...args: [])`: `this` is skipped and the empty rest-tuple
    // expands to nothing — a present, binding-less slot.
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    let host = VerterHost::new_standalone(crate::types::HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = snippet_graph(&host);

    let callable = nsnippet_function(&graph, ntuple(&graph, Vec::new()));
    let params = CallableNodeView::new(&dispatch, callable)
        .validated_snippet_positional_params(nav_context())
        .expect("an empty snippet callable still yields a (zero-length) param list");
    assert!(
        params.is_empty(),
        "a `Snippet<[]>` callable has no positions"
    );

    assert!(
        materialize_snippet_slot_bindings(
            &host,
            &verter_type_expr::TypeExprScope::new("/Owner.svelte"),
            &params,
        )
        .is_empty(),
        "no positions ⇒ no published bindings"
    );
}

#[test]
fn snippet_unlabelled_tuple_elements_fall_back_to_arg_index_names() {
    // Unlabelled tuple elements (`Snippet<[string, number]>`): the reader keeps
    // `label: None` per position and the DTO sink applies the `arg{index}`
    // name fallback while preserving order + exact types.
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    let host = VerterHost::new_standalone(crate::types::HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = snippet_graph(&host);

    let a = nprim(&graph, crate::semantic_query::PrimitiveKind::String);
    let b = nprim(&graph, crate::semantic_query::PrimitiveKind::Number);
    let snippet = nsnippet(&graph, vec![ntuple(&graph, vec![(None, a), (None, b)])]);

    let params = CallableNodeView::new(&dispatch, snippet)
        .validated_snippet_positional_params(nav_context())
        .expect("an unlabelled `Params` tuple still yields positional params");
    assert!(
        params.iter().all(|p| p.label.is_none()),
        "the reader keeps unlabelled positions label-less (no fabricated name)"
    );

    let bindings = materialize_snippet_slot_bindings(
        &host,
        &verter_type_expr::TypeExprScope::new("/Owner.svelte"),
        &params,
    );
    let names: Vec<&str> = bindings.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["arg0", "arg1"],
        "unlabelled tuple elements fall back to `arg{{index}}` at the DTO sink"
    );
}

#[test]
fn snippet_union_arms_combine_by_index_into_intersection_binding() {
    // A UNION of two snippet carriers combines positions by index:
    // `Snippet<[a: string]> | Snippet<[a: number, b: boolean]>` yields ONE
    // position (the SHORTEST arm caps the count) whose type is the EXACT
    // interned `Intersection([string, number])` node, labelled by the FIRST
    // arm; the DTO sink publishes the intersection binding.
    use crate::project_semantic_dispatch::{node_data_for, ProjectSemanticDispatch};
    let host = VerterHost::new_standalone(crate::types::HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = snippet_graph(&host);

    let a_ty = nprim(&graph, crate::semantic_query::PrimitiveKind::String);
    let b_ty = nprim(&graph, crate::semantic_query::PrimitiveKind::Number);
    let extra = nprim(&graph, crate::semantic_query::PrimitiveKind::Boolean);
    let arm_a = nsnippet(&graph, vec![ntuple(&graph, vec![(Some("a"), a_ty)])]);
    let arm_b = nsnippet(
        &graph,
        vec![ntuple(&graph, vec![(Some("x"), b_ty), (Some("b"), extra)])],
    );
    let union = graph.intern_node(crate::semantic_query::SemanticNodeData::Union(Arc::from(
        vec![arm_a, arm_b].into_boxed_slice(),
    )));

    let params = CallableNodeView::new(&dispatch, union)
        .validated_snippet_positional_params(nav_context())
        .expect("a union of snippet carriers yields combined positional params");
    assert_eq!(
        params.len(),
        1,
        "one position across both arms (the SHORTEST arm caps the count)"
    );
    assert_eq!(
        params[0].label.as_deref(),
        Some("a"),
        "the FIRST arm's label names the position"
    );
    match node_data_for(dispatch.ctx, params[0].ty).as_deref() {
        Some(crate::semantic_query::SemanticNodeData::Intersection(arms)) => {
            assert_eq!(
                arms.as_ref(),
                &[a_ty, b_ty][..],
                "the combined position type intersects both arms' exact nodes, in arm order"
            );
        }
        other => panic!("the combined position type is an `Intersection` node, got {other:?}"),
    }

    let bindings = materialize_snippet_slot_bindings(
        &host,
        &verter_type_expr::TypeExprScope::new("/Owner.svelte"),
        &params,
    );
    assert_eq!(bindings.len(), 1, "one published binding across both arms");
    assert_eq!(bindings[0].name, "a");
    assert_eq!(
        bindings[0].type_annotation.as_deref(),
        Some("string & number"),
        "the published binding type is the intersection of both arms, got {:?}",
        bindings[0].type_annotation
    );
}

#[test]
fn snippet_non_callable_root_fails_closed() {
    // NEGATIVE: a non-snippet, non-callable root is NOT a snippet — the reader
    // fails closed (`None`, the slot is dropped), never a fabricated binding
    // list.
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    let host = VerterHost::new_standalone(crate::types::HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = snippet_graph(&host);

    let scalar = nprim(&graph, crate::semantic_query::PrimitiveKind::String);
    assert_eq!(
        CallableNodeView::new(&dispatch, scalar).validated_snippet_positional_params(nav_context()),
        None,
        "a primitive root is not a snippet callable"
    );
}

#[test]
fn userland_snippet_lookalike_is_not_published_as_a_slot() {
    // NEGATIVE: a `Snippet` imported from a userland module (NOT the
    // `svelte` package) is NOT validated, so the snippet-slots surface is
    // Missing — the structural package check is upheld end-to-end.
    let component = "/workspace/FakeSnippet.svelte";
    let source = "<script lang=\"ts\">\n\
             import type { Snippet } from './fake-svelte';\n\
             interface Props { row: Snippet<[item: number]> }\n\
             let { row }: Props = $props();\n\
             void row;\n\
             </script>\n\
             <div />";
    let (host, view) = workspace_host_with_svelte(
        component,
        source,
        &[(
            "/workspace/fake-svelte.ts",
            "export interface Snippet<P extends unknown[] = []> { (this: void, ...a: P): void }\n",
        )],
    );
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let ctx = crate::resolver_core::HostResolverContext::from_current(&host, &view, overlay);

    let outcome = resolve_svelte_surface(&host, &ctx, component, SvelteSurfaceSource::SnippetProps);
    assert!(
        matches!(outcome, ResolvedOutcome::Missing),
        "a userland `Snippet` look-alike must NOT publish a slot surface, got {outcome:?}"
    );
}

#[test]
fn inline_local_props_carry_a_local_member_declaration_origin() {
    // MEMBER-DECLARATION provenance: every prop member DECLARED in a
    // local/inline props type carries a `Local` origin hop whose declaration
    // file is the OWNER. This is MEMBER-declaration provenance, NOT value-type
    // provenance — so the PRIMITIVE prop `count: number` ALSO carries a `Local`
    // origin (it is still DECLARED in the local `Props`), and the origin's
    // declaration name is the MEMBER name (`count`), never the value type. The
    // declaration file is the owner for BOTH members.
    let component = "/workspace/LocalOrigin.svelte";
    let source = "<script lang=\"ts\">\n\
             interface Item { id: number }\n\
             interface Props { item: Item; count: number }\n\
             let { item, count }: Props = $props();\n\
             void item; void count;\n\
             </script>\n\
             <div />";
    let (host, view) = workspace_host_with_svelte(component, source, &[]);
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let ctx = crate::resolver_core::HostResolverContext::from_current(&host, &view, overlay);

    let outcome = resolve_svelte_surface(&host, &ctx, component, SvelteSurfaceSource::RunesProps);
    let ResolvedOutcome::Resolved(dtos) = outcome else {
        panic!("the PROPS surface must resolve, got {outcome:?}");
    };
    let origins = dtos.prop_origins();
    use crate::typeinfo::framework_surface::results::OriginHop;
    for prop in ["item", "count"] {
        let entry = origins
            .iter()
            .find(|o| o.prop_name == prop)
            .unwrap_or_else(|| panic!("the `{prop}` prop carries a member-declaration origin"));
        assert_eq!(
            entry.origin.chain,
            vec![OriginHop::Local],
            "`{prop}` is declared in the local inline `Props` ⇒ a Local hop, got {:?}",
            entry.origin.chain
        );
        assert_eq!(
            entry.origin.declaration.canonical_source, component,
            "`{prop}`'s member declaration lives in the owner file"
        );
        // DISCRIMINATING: the origin describes the MEMBER (its name), not the
        // value type — `count`'s origin name is `count`, never `number`/`Item`.
        assert_eq!(
            entry.origin.declaration.resolved_name, prop,
            "the member-declaration origin names the MEMBER, not its value type"
        );
    }
}

#[test]
fn prop_defaults_sidecar_carries_default_values_on_the_resolved_bundle() {
    // resolved-bundle DATA (DISCRIMINATING): the resolved
    // `MacroSurfaceDtos.prop_defaults` SIDECAR carries the default VALUE source
    // text keyed by prop — `size = 'md'` -> "'md'", `disabled = $bindable(false)`
    // -> "false" — while a prop WITHOUT a default (`label`) has NO entry. The
    // framework-surface graph wire only carries `required`; the VALUE lives on
    // this sidecar, so this is where default-value presence is asserted.
    let component = "/workspace/Defaults.svelte";
    let source = "<script lang=\"ts\">\n\
             interface Props { size?: string; disabled?: boolean; label: string }\n\
             let { size = 'md', disabled = $bindable(false), label }: Props = $props();\n\
             void size; void disabled; void label;\n\
             </script>\n\
             <div />";
    let (host, view) = workspace_host_with_svelte(component, source, &[]);
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let ctx = crate::resolver_core::HostResolverContext::from_current(&host, &view, overlay);

    let outcome = resolve_svelte_surface(&host, &ctx, component, SvelteSurfaceSource::RunesProps);
    let ResolvedOutcome::Resolved(dtos) = outcome else {
        panic!("the PROPS surface must resolve, got {outcome:?}");
    };
    let defaults = dtos.prop_defaults();
    let size = defaults
        .iter()
        .find(|d| d.key == "size")
        .expect("the `size` default is on the sidecar");
    assert_eq!(
        size.value, "'md'",
        "the destructuring default VALUE is captured"
    );
    let disabled = defaults
        .iter()
        .find(|d| d.key == "disabled")
        .expect("the `$bindable(false)` default is on the sidecar");
    assert_eq!(
        disabled.value, "false",
        "the `$bindable` first-arg default VALUE"
    );
    // A prop without a default has NO sidecar entry (discriminating negative).
    assert!(
        !defaults.iter().any(|d| d.key == "label"),
        "a prop without a default has no sidecar entry, got {defaults:?}"
    );
    // And the resolved prop fields reflect optionality: defaulted props are
    // optional, the non-defaulted `label` is required.
    let field = |name: &str| {
        dtos.prop_fields()
            .iter()
            .find(|f| f.analysis.name == name)
            .cloned()
    };
    assert!(
        field("size").expect("size prop").analysis.is_optional,
        "size is optional"
    );
    assert!(
        field("disabled")
            .expect("disabled prop")
            .analysis
            .is_optional,
        "disabled is optional"
    );
    assert!(
        !field("label").expect("label prop").analysis.is_optional,
        "label (no default) stays required"
    );
}

#[test]
fn imported_props_members_carry_an_import_member_declaration_origin() {
    // MEMBER-DECLARATION provenance (DISCRIMINATING): when the props type
    // itself is an IMPORTED interface, EVERY prop member is DECLARED in that
    // imported module — so each member's origin is an IMPORT hop pointing at the
    // declaring module, with the member's declaration on THAT file. This holds
    // even for a primitive-typed member (`width: number`): the member-DECLARATION
    // file is the imported module, not the owner. DISCRIMINATING: the prior
    // value-type model would classify `width` (a primitive) as having NO origin;
    // a Local-only origin would carry the owner as the canonical source.
    let component = "/workspace/ImportedOrigin.svelte";
    let source = "<script lang=\"ts\">\n\
             import type { Props } from './types';\n\
             let { box, width }: Props = $props();\n\
             void box; void width;\n\
             </script>\n\
             <div />";
    let (host, view) = workspace_host_with_svelte(
        component,
        source,
        &[(
            "/workspace/types.ts",
            "export interface Box { w: number }\n\
             export interface Props { box: Box; width: number }\n",
        )],
    );
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let ctx = crate::resolver_core::HostResolverContext::from_current(&host, &view, overlay);

    let outcome = resolve_svelte_surface(&host, &ctx, component, SvelteSurfaceSource::RunesProps);
    let ResolvedOutcome::Resolved(dtos) = outcome else {
        panic!("the PROPS surface must resolve, got {outcome:?}");
    };
    let origins = dtos.prop_origins();
    use crate::typeinfo::framework_surface::results::OriginHop;
    for prop in ["box", "width"] {
        let entry = origins
            .iter()
            .find(|o| o.prop_name == prop)
            .unwrap_or_else(|| panic!("the `{prop}` prop carries a member-declaration origin"));
        // The member declaration lives in the imported `/workspace/types.ts`.
        assert_eq!(
            entry.origin.declaration.canonical_source, "/workspace/types.ts",
            "`{prop}`'s member declaration resolves to the imported module, got {:?}",
            entry.origin.declaration.canonical_source
        );
        assert_eq!(
            entry.origin.chain.len(),
            1,
            "one cross-file hop for `{prop}`, got {:?}",
            entry.origin.chain
        );
        match &entry.origin.chain[0] {
            OriginHop::Import {
                from,
                imported_name,
                ..
            } => {
                assert_eq!(from, "/workspace/types.ts", "`{prop}` import source module");
                // The imported name on a member hop is the MEMBER name (its
                // declaration is the member inside the imported interface).
                assert_eq!(
                    imported_name, prop,
                    "`{prop}`'s member-declaration import names the member"
                );
            }
            other => panic!("expected an Import hop for `{prop}`, got {other:?}"),
        }
    }
    // DISCRIMINATING negative: the primitive `width` is NOT dropped — the
    // value-type model would have omitted it.
    assert!(
        origins.iter().any(|o| o.prop_name == "width"),
        "a primitive member of an imported props type still carries an origin"
    );
}

/// Demand a callback prop's first-param object surface THROUGH THE GRAPH
/// SURFACE (props surface -> `member_name` member -> realized callable
/// signature -> first param node -> one-level object surface) and assert it
/// carries member `id` — the precise named-ref (`Row`) resolution a published
/// callback event's synthesized payload defers to graph demand (the published
/// `AnalyzedEmitField` is display + honest locator-less `None`s by contract).
fn assert_callback_row_param_resolves_precisely(
    host: &VerterHost,
    ctx: &dyn crate::resolver_core::ResolverContext,
    canonical: &str,
    member_name: &str,
) {
    let facts = host
        .resolve_svelte_script_facts_with_ctx(ctx, canonical)
        .expect("svelte script facts");
    let props_type = facts.props_type.as_ref().expect("props type payload");
    let props_surface = navigate_param_to_object_surface(ctx, canonical, props_type)
        .expect("the `$props` object surface resolves");
    let member = props_surface
        .members
        .iter()
        .find(|m| m.name.as_ref() == member_name)
        .unwrap_or_else(|| panic!("the `{member_name}` member is on the props surface"));
    let dispatch = ctx.dispatch();
    let signature = CallableNodeView::new(&dispatch, member.value)
        .signature(nav_context())
        .expect("the callback member realizes to a callable signature");
    let row_param_ty = signature
        .raw_params()
        .first()
        .map(|p| p.ty)
        .expect("the `(row: Row)` callback has one parameter");
    let resolved = host
        .project_shallow_surface_from_base(
            ctx,
            &dispatch,
            row_param_ty,
            Arc::from(Vec::<crate::semantic_query::PathSegment>::new().into_boxed_slice()),
            crate::semantic_query::ProjectionReductionContext::published(
                crate::semantic_query::ProjectionMode::Shallow,
            ),
            None,
        )
        .expect("`Row` resolves to an object surface in its declaring scope");
    assert!(
        resolved.members.iter().any(|m| m.name.as_ref() == "id"),
        "the resolved `Row` surface carries member `id` (precise named-ref \
         resolution via graph demand), got members {:?}",
        resolved
            .members
            .iter()
            .map(|m| m.name.as_ref())
            .collect::<Vec<_>>()
    );
}

#[test]
fn callback_event_payload_named_ref_resolves_on_the_component_meta_surface() {
    // P1 (COMPONENT-META surface, not IDE-TSX): a callback-prop event
    // `onselect: (row: Row) => void` (with `Row` a same-module interface)
    // resolves through the framework-surface resolver to an `AnalyzedEmitField`
    // whose payload display renders the labelled tuple. The `Row` reference is
    // PRECISE: the typed payload is a graph-surface demand (the synthesized
    // tuple has no authored position), so the demand below re-resolves `Row`
    // to its object surface. DISCRIMINATING: an imprecise param type could
    // not surface `Row`'s member `id`.
    let canonical = "/CbScope.svelte";
    let source = "<script lang=\"ts\">\n\
             interface Row { id: number }\n\
             interface Props { onselect: (row: Row) => void }\n\
             let { onselect }: Props = $props();\n\
             void onselect;\n\
             </script>\n\
             <button onclick={() => onselect({ id: 1 })} />";
    let (host, view) = host_with_svelte(canonical, source);
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let ctx = crate::resolver_core::HostResolverContext::from_current(&host, &view, overlay);

    let outcome = resolve_svelte_surface(
        &host,
        &ctx,
        canonical,
        SvelteSurfaceSource::CallbackPropEvents,
    );
    let ResolvedOutcome::Resolved(dtos) = outcome else {
        panic!("the callback-prop EMITS surface must resolve, got {outcome:?}");
    };
    let emits = dtos.emits.as_ref().expect("emits surface present");
    let select = emits
        .fields
        .iter()
        .find(|e| e.analysis.name == "select")
        .expect("the `onselect` callback prop surfaces as event `select`");

    // The `select` event carries the `(row: Row)` payload tuple (rendered at
    // the terminal sink).
    assert_eq!(
        select.analysis.payload_type.as_deref(),
        Some("[row: Row]"),
        "the `select` event carries a payload tuple"
    );
    // The payload tuple is a per-event SYNTHESIS over the callback's params —
    // it has no authored macro-payload position, so the published locator and
    // scope are the honest paired `None`s (`payload_type` above is display).
    assert!(
        select.analysis.payload.is_none() && select.analysis.payload_expr_scope.is_none(),
        "a synthesized callback payload publishes no authored locator/scope"
    );

    // DISCRIMINATING named-ref resolution: demand the callback's `(row: Row)`
    // param through the graph surface and re-resolve `Row` to its object
    // surface (member `id`).
    assert_callback_row_param_resolves_precisely(&host, &ctx, canonical, "onselect");
}

#[test]
fn optional_callback_prop_classifies_as_event_with_precise_payload() {
    // P1-importance (COMPONENT-META surface): a member-OPTIONAL callback prop
    // `onselect?: (row: Row) => void`. The `?` is factored into the surface
    // member `optional` flag, so the VALUE raises to a BARE `Function` (NOT a
    // union — that is the explicit-union case below). It MUST classify as event
    // `select` with a PRECISE `(row: Row)` payload. A NON-callable optional prop
    // (`label?: string`) is NOT an event; a non-`on` prop is never mined.
    let canonical = "/OptCb.svelte";
    let source = "<script lang=\"ts\">\n\
             interface Row { id: number }\n\
             interface Props {\n\
               onselect?: (row: Row) => void;\n\
               label?: string;\n\
               plain: number;\n\
             }\n\
             let { onselect, label, plain }: Props = $props();\n\
             void onselect; void label; void plain;\n\
             </script>\n\
             <div />";
    let (host, view) = host_with_svelte(canonical, source);
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let ctx = crate::resolver_core::HostResolverContext::from_current(&host, &view, overlay);

    let outcome = resolve_svelte_surface(
        &host,
        &ctx,
        canonical,
        SvelteSurfaceSource::CallbackPropEvents,
    );
    let ResolvedOutcome::Resolved(dtos) = outcome else {
        panic!("the callback-prop EMITS surface must resolve, got {outcome:?}");
    };
    let emits = dtos.emits.as_ref().expect("emits surface present");
    let names: Vec<&str> = emits
        .fields
        .iter()
        .map(|e| e.analysis.name.as_str())
        .collect();

    // (a) the OPTIONAL callback prop IS event `select`.
    let select = emits
        .fields
        .iter()
        .find(|e| e.analysis.name == "select")
        .unwrap_or_else(|| {
            panic!(
                "an OPTIONAL `onselect?:` callback prop must classify as event \
                     `select` (its value raises to a bare `Function`), got {names:?}"
            )
        });
    // (c) the non-callable optional prop `label?: string` is NOT an event
    // (neither the prop name nor the `on`-strip residue).
    assert!(
        !names.contains(&"label") && !names.contains(&"abel"),
        "a non-callable optional prop must NOT be an event, got {names:?}"
    );
    // a non-`on` prop is never mined.
    assert!(
        !names.contains(&"plain"),
        "a non-`on` prop must NOT be an event, got {names:?}"
    );

    // The optional callback's payload is PRECISE — the display renders the
    // labelled tuple, the locator/scope stay the honest paired `None`s (a
    // synthesized tuple has no authored position), and `Row` resolves through
    // the graph-surface demand.
    assert_eq!(
        select.analysis.payload_type.as_deref(),
        Some("[row: Row]"),
        "the optional callback publishes the `[row: Row]` payload display"
    );
    assert!(
        select.analysis.payload.is_none() && select.analysis.payload_expr_scope.is_none(),
        "a synthesized callback payload publishes no authored locator/scope"
    );
    assert_callback_row_param_resolves_precisely(&host, &ctx, canonical, "onselect");
}

#[test]
fn union_with_no_callable_arm_is_not_an_event() {
    // P1-importance edge: an `on`-prefixed prop whose value is a union with NO
    // callable arm (`onmode: \"a\" | \"b\"`) is NOT an event — the shared
    // callable-arm extractor returns `None` for a non-callable union.
    // DISCRIMINATING: a classifier that accepted any union would mis-mine it.
    let canonical = "/UnionNoCb.svelte";
    let source = "<script lang=\"ts\">\n\
             interface Props { onmode: \"a\" | \"b\" }\n\
             let { onmode }: Props = $props();\n\
             void onmode;\n\
             </script>\n\
             <div />";
    let (host, view) = host_with_svelte(canonical, source);
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let ctx = crate::resolver_core::HostResolverContext::from_current(&host, &view, overlay);

    let outcome = resolve_svelte_surface(
        &host,
        &ctx,
        canonical,
        SvelteSurfaceSource::CallbackPropEvents,
    );
    let ResolvedOutcome::Resolved(dtos) = outcome else {
        panic!("the EMITS surface must resolve, got {outcome:?}");
    };
    let emits = dtos.emits.as_ref().expect("emits surface present");
    assert!(
        !emits.fields.iter().any(|e| e.analysis.name == "mode"),
        "an `on`-prefixed union with no callable arm must NOT be an event, got {:?}",
        emits
            .fields
            .iter()
            .map(|e| e.analysis.name.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn optional_alias_callback_prop_classifies_as_event_with_precise_payload() {
    // P1-importance WHOLE-CLASS edge: an OPTIONAL callback prop whose value is
    // an ALIAS (`type Handler = (row: Row) => void; onselect?: Handler`). The
    // member-`?` rides the surface `optional` flag, and the alias `Ref` carrier
    // is realised through the SHARED resolver (`realize_callable_member`) to its
    // bare `Function` body. It MUST classify as event `select` with a PRECISE
    // `(row: Row)` payload. DISCRIMINATING: a classifier that only matched a
    // bare post-raise `Function` arm WITHOUT realising the alias `Ref` carrier
    // first would DROP it (the value is a `Ref`, not a `Function`, before
    // realisation).
    let canonical = "/OptAliasCb.svelte";
    let source = "<script lang=\"ts\">\n\
             interface Row { id: number }\n\
             type Handler = (row: Row) => void;\n\
             interface Props { onselect?: Handler }\n\
             let { onselect }: Props = $props();\n\
             void onselect;\n\
             </script>\n\
             <div />";
    let (host, view) = host_with_svelte(canonical, source);
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let ctx = crate::resolver_core::HostResolverContext::from_current(&host, &view, overlay);

    let outcome = resolve_svelte_surface(
        &host,
        &ctx,
        canonical,
        SvelteSurfaceSource::CallbackPropEvents,
    );
    let ResolvedOutcome::Resolved(dtos) = outcome else {
        panic!("the callback-prop EMITS surface must resolve, got {outcome:?}");
    };
    let emits = dtos.emits.as_ref().expect("emits surface present");
    let select = emits
        .fields
        .iter()
        .find(|e| e.analysis.name == "select")
        .unwrap_or_else(|| {
            panic!(
                "an OPTIONAL alias callback prop `onselect?: Handler` must classify as \
                 event `select` (the alias arm is realised, the `| undefined` arm stripped), \
                 got {:?}",
                emits
                    .fields
                    .iter()
                    .map(|e| e.analysis.name.as_str())
                    .collect::<Vec<_>>()
            )
        });
    // The alias callback's payload is PRECISE — the display renders the
    // labelled tuple, the locator/scope stay the honest paired `None`s, and
    // `Row` resolves through the graph-surface demand.
    assert_eq!(
        select.analysis.payload_type.as_deref(),
        Some("[row: Row]"),
        "the optional alias callback publishes the `[row: Row]` payload display"
    );
    assert!(
        select.analysis.payload.is_none() && select.analysis.payload_expr_scope.is_none(),
        "a synthesized callback payload publishes no authored locator/scope"
    );
    assert_callback_row_param_resolves_precisely(&host, &ctx, canonical, "onselect");
}

#[test]
fn explicit_union_callback_prop_value_classifies_as_event_with_precise_payload() {
    // P2 (COMPONENT-META surface): a prop whose WRITTEN VALUE is an EXPLICIT
    // union containing a callable arm — `onselect: ((row: Row) => void) |
    // undefined` (NOT member-`?` optionality, which is carried by the surface
    // `optional` flag and resolves to a BARE `Function`). The explicit union
    // resolves to `Union(Function, undefined)`; the node-domain
    // `CallableNodeView::single_callable_arm` strips the nullish arm and pulls
    // out the single callable. It MUST classify as event `select` with a
    // PRECISE `(row: Row)` payload.
    //
    // DISCRIMINATING (the whole point): this exercises the composite arm of
    // the callable-arm classifier. A classifier reduced to a bare
    // `Function`-only match goes RED here (no `select` event) while the
    // member-`?` tests above stay GREEN (they resolve to a bare `Function`).
    // A non-callable explicit-union prop (`onmode: "a" | "b"`) is NOT an
    // event (asserted negatively here too).
    let canonical = "/ExplicitUnionCb.svelte";
    let source = "<script lang=\"ts\">\n\
             interface Row { id: number }\n\
             interface Props {\n\
               onselect: ((row: Row) => void) | undefined;\n\
               onmode: \"a\" | \"b\";\n\
             }\n\
             let { onselect, onmode }: Props = $props();\n\
             void onselect; void onmode;\n\
             </script>\n\
             <div />";
    let (host, view) = host_with_svelte(canonical, source);
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let ctx = crate::resolver_core::HostResolverContext::from_current(&host, &view, overlay);

    let outcome = resolve_svelte_surface(
        &host,
        &ctx,
        canonical,
        SvelteSurfaceSource::CallbackPropEvents,
    );
    let ResolvedOutcome::Resolved(dtos) = outcome else {
        panic!("the callback-prop EMITS surface must resolve, got {outcome:?}");
    };
    let emits = dtos.emits.as_ref().expect("emits surface present");
    let names: Vec<&str> = emits
        .fields
        .iter()
        .map(|e| e.analysis.name.as_str())
        .collect();

    // (a) the EXPLICIT-union callable VALUE IS event `select` (this is the
    // branch the member-`?` tests do NOT cover — they raise to a bare
    // `Function`, this raises to a `Union`).
    let select = emits
        .fields
        .iter()
        .find(|e| e.analysis.name == "select")
        .unwrap_or_else(|| {
            panic!(
                "an EXPLICIT-union callable prop VALUE `onselect: ((row: Row) => void) | \
                 undefined` must classify as event `select` (the `| undefined` arm is \
                 stripped from the union), got {names:?}"
            )
        });
    // (b) NEGATIVE: an explicit union with NO callable arm is NOT an event.
    assert!(
        !names.contains(&"mode"),
        "an explicit union with no callable arm (`onmode: \"a\" | \"b\"`) must NOT be \
             an event, got {names:?}"
    );

    // The payload is PRECISE — the display renders the labelled tuple, the
    // locator/scope stay the honest paired `None`s, and `Row` resolves
    // through the graph-surface demand (member `id`).
    assert_eq!(
        select.analysis.payload_type.as_deref(),
        Some("[row: Row]"),
        "the explicit-union callback publishes the `[row: Row]` payload display"
    );
    assert!(
        select.analysis.payload.is_none() && select.analysis.payload_expr_scope.is_none(),
        "a synthesized callback payload publishes no authored locator/scope"
    );
    assert_callback_row_param_resolves_precisely(&host, &ctx, canonical, "onselect");
}

#[test]
fn explicit_union_with_two_distinct_callable_arms_refuses() {
    // P2 (COMPONENT-META surface): the ambiguity branch of the node-domain
    // callable-arm classifier. An `on`-prefixed prop whose explicit-union
    // VALUE has TWO DISTINCT callable arms — `onselect: ((row: Row) => void) |
    // ((id: number) => void)` — is AMBIGUOUS: the classifier must REFUSE rather
    // than fabricate a single payload from divergent signatures. No `select`
    // event is mined.
    //
    // DISCRIMINATING: the union-arm loop returns `None` when a second, distinct
    // callable arm appears. A classifier that picked the first callable arm
    // would wrongly mine `select`; this asserts it does NOT.
    let canonical = "/AmbiguousUnionCb.svelte";
    let source = "<script lang=\"ts\">\n\
             interface Row { id: number }\n\
             interface Props { onselect: ((row: Row) => void) | ((id: number) => void) }\n\
             let { onselect }: Props = $props();\n\
             void onselect;\n\
             </script>\n\
             <div />";
    let (host, view) = host_with_svelte(canonical, source);
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let ctx = crate::resolver_core::HostResolverContext::from_current(&host, &view, overlay);

    let outcome = resolve_svelte_surface(
        &host,
        &ctx,
        canonical,
        SvelteSurfaceSource::CallbackPropEvents,
    );
    let ResolvedOutcome::Resolved(dtos) = outcome else {
        panic!("the EMITS surface must resolve, got {outcome:?}");
    };
    let emits = dtos.emits.as_ref().expect("emits surface present");
    assert!(
        !emits.fields.iter().any(|e| e.analysis.name == "select"),
        "an explicit union with TWO distinct callable arms is ambiguous and must NOT be \
             mined as an event, got {:?}",
        emits
            .fields
            .iter()
            .map(|e| e.analysis.name.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn carrier_wrapped_nullish_callback_prop_classifies_as_event_with_precise_payload() {
    // Node-domain callable-arm characterization: a callback prop whose value is
    // an ALIAS whose BODY is a nullish union — `type Handler = ((row: Row) => void) |
    // undefined; onselect: Handler`. The value node is a `DeclRef(Handler)`
    // carrier wrapping `Union([Function, undefined])`. The node-domain
    // `CallableNodeView::signature` (`single_callable_arm`) resolves the
    // `DeclRef` through the shared structural-fact demand primitive FIRST, strips
    // the `undefined` arm, and realizes the surviving `Function` — surfacing
    // event `select` with a PRECISE `(row: Row)` payload.
    //
    // DISCRIMINATING (fails on a wrong projection): a reader that decided on the
    // UN-resolved `DeclRef` carrier (never normalizing it), or that failed the
    // whole-composite realize on the `undefined` arm without stripping it, would
    // surface NO `select` event — the `select` assertion below FAILS against
    // either. A non-callable `on*` union (`onmode: "a" | "b"`) stays NOT an event.
    let canonical = "/CarrierNullishCb.svelte";
    let source = "<script lang=\"ts\">\n\
             interface Row { id: number }\n\
             type Handler = ((row: Row) => void) | undefined;\n\
             interface Props {\n\
               onselect: Handler;\n\
               onmode: \"a\" | \"b\";\n\
             }\n\
             let { onselect, onmode }: Props = $props();\n\
             void onselect; void onmode;\n\
             </script>\n\
             <div />";
    let (host, view) = host_with_svelte(canonical, source);
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let ctx = crate::resolver_core::HostResolverContext::from_current(&host, &view, overlay);

    let outcome = resolve_svelte_surface(
        &host,
        &ctx,
        canonical,
        SvelteSurfaceSource::CallbackPropEvents,
    );
    let ResolvedOutcome::Resolved(dtos) = outcome else {
        panic!("the callback-prop EMITS surface must resolve, got {outcome:?}");
    };
    let emits = dtos.emits.as_ref().expect("emits surface present");
    let names: Vec<&str> = emits
        .fields
        .iter()
        .map(|e| e.analysis.name.as_str())
        .collect();

    // (a) the carrier-wrapped nullish callable alias IS event `select`.
    let select = emits
        .fields
        .iter()
        .find(|e| e.analysis.name == "select")
        .unwrap_or_else(|| {
            panic!(
                "a carrier-wrapped nullish callback alias `onselect: Handler` (Handler = \
                 `((row: Row) => void) | undefined`) must classify as event `select` (the \
                 `DeclRef` carrier is resolved, the `| undefined` arm stripped), got {names:?}"
            )
        });
    // (b) NEGATIVE: the non-callable `on*` union is NOT an event.
    assert!(
        !names.contains(&"mode"),
        "an `on`-prefixed non-callable union (`onmode: \"a\" | \"b\"`) must NOT be an event, \
         got {names:?}"
    );

    // The payload is PRECISE — the display renders the labelled tuple, the
    // locator/scope stay the honest paired `None`s, and `Row` resolves
    // through the graph-surface demand (member `id`).
    assert_eq!(
        select.analysis.payload_type.as_deref(),
        Some("[row: Row]"),
        "the carrier-nullish callback publishes the `[row: Row]` payload display"
    );
    assert!(
        select.analysis.payload.is_none() && select.analysis.payload_expr_scope.is_none(),
        "a synthesized callback payload publishes no authored locator/scope"
    );
    assert_callback_row_param_resolves_precisely(&host, &ctx, canonical, "onselect");
}

#[test]
fn svelte_snippet_slots_normalizer_publishes_node_domain_bindings() {
    // END-TO-END (the CONVERTED node-domain normalizer): a
    // `Snippet<[item: Item, index: number]>` member publishes ordered slot
    // bindings `item` + `index` through `svelte_snippet_slots_from_typeinfo_surface`
    // — the node-domain path (no materialize-then-decide). Uses a WORKSPACE-LOCAL
    // `Snippet` interface + a direct `retain_members` (bypassing package-backed
    // validation) so the CONVERTED normalizer is exercised directly.
    let component = "/workspace/SnippetSlots.svelte";
    let source = "<script lang=\"ts\">\n\
             import type { Snippet } from './snippet';\n\
             interface Item { id: number }\n\
             interface Props { row: Snippet<[item: Item, index: number]> }\n\
             let { row }: Props = $props();\n\
             void row;\n\
             </script>\n\
             <div />";
    let (host, view) = workspace_host_with_svelte(
        component,
        source,
        &[(
            "/workspace/snippet.ts",
            "export interface Snippet<Params extends unknown[] = []> {\n\
                 (this: void, ...args: Params): { __brand: 'snippet' };\n\
                 }\n",
        )],
    );
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let ctx = crate::resolver_core::HostResolverContext::from_current(&host, &view, overlay);

    let facts = host
        .resolve_svelte_script_facts_with_ctx(&ctx, component)
        .expect("svelte facts");
    let props_type = facts.props_type.as_ref().expect("props type");
    let surface =
        navigate_param_to_object_surface(&ctx, component, props_type).expect("props surface");
    let filtered = retain_members(&surface, &["row".to_string()]);
    let resolved = macro_surface_shell(filtered, AnalyzedMacroKind::DefineSlots, component);

    let slots = svelte_snippet_slots_from_typeinfo_surface(&ctx, &resolved);
    let row = slots
        .iter()
        .find(|s| s.name == "row")
        .expect("the `row` snippet slot publishes");
    let names: Vec<&str> = row.bindings.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["item", "index"],
        "the snippet slot bindings come from the `Params` tuple, in order"
    );
    assert!(
        row.bindings.iter().all(|b| b.type_annotation.is_some()),
        "each binding carries a display `type_annotation` rendered from the value minted at \
         the terminal sink"
    );
    assert!(
        row.bindings.iter().all(|b| b.binding_expr_scope.is_some()),
        "each binding value is paired with a scope (pairing invariant)"
    );
}

#[test]
fn snippet_declref_tuple_params_resolve_to_ordered_dto_bindings() {
    // EMPIRICAL, end-to-end — a `Snippet<Args>` whose `Args` is a
    // DeclRef-to-tuple (`type Args = [item: Item, index: number]`): the node
    // reader (`validated_snippet_positional_params`) RESOLVES the `DeclRef` to
    // its `Tuple` through the shared structural-fact demand primitive, and the
    // terminal DTO sink publishes the two ordered bindings `item` + `index`.
    // DISCRIMINATING: a reader that required a LITERAL `Tuple` type-argument
    // (never resolving the `DeclRef`) would surface NO bindings and FAIL both
    // halves below.
    let component = "/workspace/FlipSnippetSurface.svelte";
    let source = "<script lang=\"ts\">\n\
             import type { Snippet } from './snippet';\n\
             import type { Args } from './types';\n\
             interface Props { row: Snippet<Args> }\n\
             let { row }: Props = $props();\n\
             void row;\n\
             </script>\n\
             <div />";
    let (host, view) = workspace_host_with_svelte(
        component,
        source,
        &[
            (
                "/workspace/snippet.ts",
                "export interface Snippet<Params extends unknown[] = []> {\n\
                     (this: void, ...args: Params): { __brand: 'snippet' };\n\
                     }\n",
            ),
            (
                "/workspace/types.ts",
                "export interface Item { id: number }\n\
                 export type Args = [item: Item, index: number];\n",
            ),
        ],
    );
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let ctx = crate::resolver_core::HostResolverContext::from_current(&host, &view, overlay);

    let facts = host
        .resolve_svelte_script_facts_with_ctx(&ctx, component)
        .expect("svelte facts");
    let props_type = facts.props_type.as_ref().expect("props type");
    let surface =
        navigate_param_to_object_surface(&ctx, component, props_type).expect("props surface");
    let row = surface
        .members
        .iter()
        .find(|m| m.name.as_ref() == "row")
        .expect("the `row` member is present");
    let dispatch = ctx.dispatch();
    let context = crate::semantic_query::ProjectionReductionContext::published(
        crate::semantic_query::ProjectionMode::Navigate,
    );

    // NODE reader: resolves the DeclRef-to-tuple `Params`.
    let params = CallableNodeView::new(&dispatch, row.value)
        .validated_snippet_positional_params(context)
        .expect("the node reader resolves the DeclRef-to-tuple `Params`");
    let node_labels: Vec<Option<&str>> = params.iter().map(|p| p.label.as_deref()).collect();
    assert_eq!(
        node_labels,
        vec![Some("item"), Some("index")],
        "the NODE reader resolves the two ordered positions"
    );

    // Terminal DTO sink: the SAME nodes publish as the two ordered Svelte slot
    // bindings (exact names, each paired with the member scope).
    let bindings = materialize_snippet_slot_bindings(
        &ctx,
        &verter_type_expr::TypeExprScope::new(component),
        &params,
    );
    let names: Vec<&str> = bindings.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["item", "index"],
        "the DTO sink publishes the resolved bindings in order"
    );
    assert!(
        bindings.iter().all(|b| b.type_annotation.is_some()),
        "each published binding carries a display rendered from its materialized value"
    );
    assert!(
        bindings.iter().all(|b| b.binding_expr_scope.is_some()),
        "each published binding value is paired with a scope"
    );
}

#[test]
fn snippet_unresolved_params_carrier_drops_the_slot_at_the_dto_surface() {
    // FAIL-CLOSED, end-to-end — a `Snippet<Args>` whose `Args` import does NOT
    // resolve (the `./missing-types` module is absent): the node reader fails
    // closed (`None` — the `Params` could still be a tuple we could not reach)
    // and the snippet-slot normalizer DROPS the slot from the published DTO
    // surface, while a resolvable sibling snippet still publishes (positive
    // contrast: the drop is the unresolved carrier, not a blanket drop).
    let component = "/workspace/UnresolvedSnippet.svelte";
    let source = "<script lang=\"ts\">\n\
             import type { Snippet } from './snippet';\n\
             import type { Args } from './missing-types';\n\
             interface Props { bad: Snippet<Args>; good: Snippet<[item: number]> }\n\
             let { bad, good }: Props = $props();\n\
             void bad; void good;\n\
             </script>\n\
             <div />";
    let (host, view) = workspace_host_with_svelte(
        component,
        source,
        &[(
            "/workspace/snippet.ts",
            "export interface Snippet<Params extends unknown[] = []> {\n\
                 (this: void, ...args: Params): { __brand: 'snippet' };\n\
                 }\n",
        )],
    );
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let ctx = crate::resolver_core::HostResolverContext::from_current(&host, &view, overlay);

    let facts = host
        .resolve_svelte_script_facts_with_ctx(&ctx, component)
        .expect("svelte facts");
    let props_type = facts.props_type.as_ref().expect("props type");
    let props_owner = verter_type_expr::TopLevelOwnerId::instance(0);
    let preparation = ctx
        .prepared_decl_bundle(component)
        .expect("prepared declaration bundle")
        .prepared_type_decls
        .get_in_for_projection(props_owner, "Props");
    match preparation {
        crate::resolver_core::prepared_decl::PreparedTypeDeclResolution::AuthoredPartial {
            root_identity,
            declaration,
            failure:
                crate::resolver_core::prepared_decl::PreparationFailure::MissingExternalOwner {
                    local_name,
                },
        } => {
            assert_eq!(root_identity.owner, props_owner);
            assert_eq!(root_identity.symbol_name.as_ref(), "Props");
            assert_eq!(local_name, "Args");
            assert!(
                declaration.member_index.contains_key("bad")
                    && declaration.member_index.contains_key("good"),
                "the exact authored declaration survives as a partial carrier"
            );
        }
        other => panic!("expected exact authored partial preparation, got {other:?}"),
    }

    let _completeness_scope = crate::request_context::ColdComputeCompletenessScope::enter();
    let surface =
        navigate_param_to_object_surface(&ctx, component, props_type).expect("props surface");
    let completeness = crate::request_context::current_cold_compute_completeness();
    assert!(
        completeness.is_partial()
            && completeness
                .reasons()
                .contains(crate::semantic_query::PartialReasonSet::MISSING_DEPENDENCY),
        "an unresolved imported owner produces typed MissingDependency partiality, got {completeness:?}"
    );
    let dispatch = ctx.dispatch();
    let context = crate::semantic_query::ProjectionReductionContext::published(
        crate::semantic_query::ProjectionMode::Navigate,
    );

    // NODE reader half: the unresolved `Args` carrier fails closed.
    let bad = surface
        .members
        .iter()
        .find(|m| m.name.as_ref() == "bad")
        .expect("the `bad` member is present");
    let graph = ctx.project_type_store().semantic_graph();
    let bad_data = graph.node_data(bad.value).expect("bad member graph node");
    let crate::semantic_query::SemanticNodeData::InstantiationRef { base, args } =
        bad_data.as_ref()
    else {
        panic!("the authored Snippet application remains a carrier, got {bad_data:?}");
    };
    assert_eq!(base.decl_name.as_ref(), "Snippet");
    let [args_node] = args.as_ref() else {
        panic!("Snippet carries exactly one Params argument, got {args:?}");
    };
    let args_data = graph.node_data(*args_node).expect("Args graph node");
    assert_eq!(
        args_data
            .bare_ref_head()
            .map(|(name, _scope)| name.as_ref()),
        Some("Args"),
        "the unresolved exact-owner import stays an authored BareRef"
    );
    assert_eq!(
        CallableNodeView::new(&dispatch, bad.value).validated_snippet_positional_params(context),
        None,
        "an unresolved `Params` carrier fails closed (never a present slot \
         presented as binding-complete)"
    );

    let base = dispatch
        .raise_semantic_type_source_to_hot(
            &verter_type_expr::facts::SemanticTypeSource::Authored(props_type.locator.clone()),
            crate::project_semantic_dispatch::semantic_source::SourceRaiseContext {
                scope_canonical_id: component,
                scope_owner: props_owner,
                context:
                    crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                        crate::semantic_query::ProjectionMode::Navigate,
                    ),
                interior_failures: None,
            },
        )
        .expect("props payload raises")
        .node();
    let read = dispatch.execute_read(crate::semantic_query::SemanticQueryKey::ProjectPath {
        base,
        path: Arc::from(Vec::<crate::semantic_query::PathSegment>::new().into_boxed_slice()),
        context: crate::semantic_query::ProjectionReductionContext::published(
            crate::semantic_query::ProjectionMode::Shallow,
        ),
    });
    assert!(
        read.result_is_partial && read.cache_suppress,
        "the usable carrier is Partial and ReturnOnly, never complete/cacheable"
    );

    // DTO surface half: the normalizer DROPS `bad` and keeps `good`.
    let filtered = retain_members(&surface, &["bad".to_string(), "good".to_string()]);
    let resolved = macro_surface_shell(filtered, AnalyzedMacroKind::DefineSlots, component);
    let slots = svelte_snippet_slots_from_typeinfo_surface(&ctx, &resolved);
    let slot_names: Vec<&str> = slots.iter().map(|s| s.name.as_str()).collect();
    assert!(
        !slot_names.contains(&"bad"),
        "the unresolved-`Params` snippet slot is DROPPED from the DTO surface, got {slot_names:?}"
    );
    let good = slots
        .iter()
        .find(|s| s.name == "good")
        .expect("the resolvable sibling snippet still publishes (positive contrast)");
    assert_eq!(
        good.bindings
            .iter()
            .map(|b| b.name.as_str())
            .collect::<Vec<_>>(),
        vec!["item"],
        "the resolvable snippet publishes its ordered binding"
    );
}

#[test]
fn snippet_resolved_params_preparation_stays_complete_and_cacheable() {
    let component = "/workspace/ResolvedSnippet.svelte";
    let source = "<script lang=\"ts\">\n\
             import type { Snippet } from './snippet';\n\
             import type { Args } from './types';\n\
             interface Props { row: Snippet<Args> }\n\
             let { row }: Props = $props();\n\
             void row;\n\
             </script>\n\
             <div />";
    let (host, view) = workspace_host_with_svelte(
        component,
        source,
        &[
            (
                "/workspace/snippet.ts",
                "export interface Snippet<Params extends unknown[] = []> {\n\
                     (this: void, ...args: Params): { __brand: 'snippet' };\n\
                     }\n",
            ),
            (
                "/workspace/types.ts",
                "export type Args = [item: string];\n",
            ),
        ],
    );
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let ctx = crate::resolver_core::HostResolverContext::from_current(&host, &view, overlay);
    let props_owner = verter_type_expr::TopLevelOwnerId::instance(0);

    let preparation = ctx
        .prepared_decl_bundle(component)
        .expect("prepared declaration bundle")
        .prepared_type_decls
        .get_in_for_projection(props_owner, "Props");
    let crate::resolver_core::prepared_decl::PreparedTypeDeclResolution::Complete(declaration) =
        preparation
    else {
        panic!("fully resolved Props must prepare completely, got {preparation:?}");
    };
    assert_eq!(declaration.root_identity.owner, props_owner);
    assert!(declaration.member_index.contains_key("row"));

    let facts = host
        .resolve_svelte_script_facts_with_ctx(&ctx, component)
        .expect("svelte facts");
    let props_type = facts.props_type.as_ref().expect("props type");
    let _completeness_scope = crate::request_context::ColdComputeCompletenessScope::enter();
    let surface =
        navigate_param_to_object_surface(&ctx, component, props_type).expect("props surface");
    assert_eq!(
        crate::request_context::current_cold_compute_completeness(),
        crate::semantic_query::ResultCompleteness::Complete,
        "a fully resolved declaration remains Complete"
    );

    let row = surface
        .members
        .iter()
        .find(|member| member.name.as_ref() == "row")
        .expect("resolved row member");
    let dispatch = ctx.dispatch();
    let params = CallableNodeView::new(&dispatch, row.value)
        .validated_snippet_positional_params(
            crate::semantic_query::ProjectionReductionContext::published(
                crate::semantic_query::ProjectionMode::Navigate,
            ),
        )
        .expect("resolved Args tuple validates");
    assert_eq!(params.len(), 1);

    let base = dispatch
        .raise_semantic_type_source_to_hot(
            &verter_type_expr::facts::SemanticTypeSource::Authored(props_type.locator.clone()),
            crate::project_semantic_dispatch::semantic_source::SourceRaiseContext {
                scope_canonical_id: component,
                scope_owner: props_owner,
                context:
                    crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                        crate::semantic_query::ProjectionMode::Navigate,
                    ),
                interior_failures: None,
            },
        )
        .expect("props payload raises")
        .node();
    let read = dispatch.execute_read(crate::semantic_query::SemanticQueryKey::ProjectPath {
        base,
        path: Arc::from(Vec::<crate::semantic_query::PathSegment>::new().into_boxed_slice()),
        context: crate::semantic_query::ProjectionReductionContext::published(
            crate::semantic_query::ProjectionMode::Shallow,
        ),
    });
    assert!(
        !read.result_is_partial && !read.cache_suppress,
        "the fully resolved path remains Complete and cacheable"
    );
}
