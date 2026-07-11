//! Tests for the Svelte resolution leg (extracted from `svelte_exec.rs`
//! to keep the production module under the oversize-file guard).

use super::*;
use verter_compiler::svelte::parser::parse_svelte;

/// Collect the legacy `<slot>` slot fields from a `.svelte` SOURCE through the
/// same structural walk the resolver uses (the typed template carrier).
fn legacy_slots(source: &str) -> Vec<AnalyzedSlotField> {
    let parsed = parse_svelte(source);
    let mut slots = Vec::new();
    collect_slot_elements(&parsed.template, source, "/Test.svelte", &mut slots);
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
                This test asserts the binding `binding_expr` is PRECISE (NOT `Primitive(Any)`); it \
                is RED today (the carve-out emits `any`) and flips green (ignore removed) when the \
                precise-capture follow-up lands."]
fn legacy_slot_let_binding_value_precision_is_a_followup() {
    // DISCRIMINATING: the forwarded `item={items[0]}` binding's value type must
    // be PRECISE (resolved from the forwarded expression), NOT the `any`
    // carve-out. Today `slot_bindings` emits `Primitive(Any)`, so this RED
    // assertion is ledgered behind `#[ignore]`. When the precise forwarded-
    // expression capture lands, `binding_expr` becomes the resolved type and
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
        !matches!(
            binding.binding_expr,
            Some(TypeExpr::Primitive(PrimitiveName::Any))
        ),
        "the legacy slot binding value must be PRECISE (not the `any` carve-out) — \
             follow-up: precise forwarded-expression capture"
    );
}

#[test]
fn legacy_slot_binding_expr_is_paired_with_a_scope() {
    // PAIRING INVARIANT: even the `any` carve-out value must be paired with a
    // `binding_expr_scope` (`binding_expr.is_some() <=> binding_expr_scope
    // .is_some()`). A `Some`-expr / `None`-scope mismatch violates the
    // documented `AnalyzedSlotFieldBinding` pairing invariant. This is
    // DISCRIMINATING: it FAILS if `slot_bindings` drops the scope back to
    // `None`.
    let slots = legacy_slots("<slot name=\"row\" item={x} />");
    let binding = slots
        .iter()
        .find(|s| s.name == "row")
        .and_then(|s| s.bindings.iter().find(|b| b.name == "item"))
        .expect("the forwarded `item` binding is collected");
    assert_eq!(
        binding.binding_expr.is_some(),
        binding.binding_expr_scope.is_some(),
        "binding_expr must be paired with binding_expr_scope (pairing invariant)"
    );
    assert!(
        binding.binding_expr_scope.is_some(),
        "the legacy slot binding's `any` value must carry an owner scope"
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
        .raise_node_to_type_expr(realized)
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
    let labels: Vec<Option<&str>> = elements.iter().map(|e| e.label.as_deref()).collect();
    assert_eq!(
        labels,
        vec![Some("item"), Some("index")],
        "the `Params` tuple carries BOTH labelled elements, got {labels:?}"
    );
    // And the normalizer over this realized value yields the two ordered
    // bindings (the integration of shape + normalizer).
    let scope = verter_type_expr::TypeExprScope::new(component);
    let bindings = snippet_callable_positional_bindings(&value, &scope)
        .expect("the realized snippet yields positional bindings");
    let names: Vec<&str> = bindings.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, vec!["item", "index"]);
}

/// Build the realized `Snippet`-callable shape the vendored
/// `Snippet<Params>` lowers to: `(this: void, ...args: <tuple>)`. The
/// normalizer must skip `this` and expand the rest-tuple element-wise.
fn snippet_callable(rest_tuple_elements: Vec<verter_type_expr::TupleElement>) -> TypeExpr {
    use verter_type_expr::{FunctionExpr, FunctionParam};
    TypeExpr::Function(Arc::new(FunctionExpr::synthetic(
        vec![
            FunctionParam::synthetic(
                Some("this".to_string()),
                TypeExpr::Primitive(PrimitiveName::Void),
                false,
                false,
            ),
            FunctionParam::synthetic(
                Some("args".to_string()),
                TypeExpr::Tuple {
                    elements: Arc::from(rest_tuple_elements.into_boxed_slice()),
                    readonly: false,
                },
                false,
                true,
            ),
        ],
        None,
        Vec::new(),
    )))
}

fn tuple_el(label: &str, ty: TypeExpr) -> verter_type_expr::TupleElement {
    verter_type_expr::TupleElement {
        label: Some(label.to_string()),
        ty,
        optional: false,
        rest: false,
    }
}

/// A `Snippet<[..]>` carrier `Ref` — the PRIMARY realized shape (the
/// resolver keeps the structural `Snippet<Params>` interface as a Ref whose
/// single type argument is the `Params` tuple).
fn snippet_ref(params: Vec<verter_type_expr::TupleElement>) -> TypeExpr {
    TypeExpr::Ref {
        name: Arc::from("Snippet"),
        type_arguments: Arc::from(
            vec![TypeExpr::Tuple {
                elements: Arc::from(params.into_boxed_slice()),
                readonly: false,
            }]
            .into_boxed_slice(),
        ),
    }
}

#[test]
fn snippet_ref_carrier_expands_params_tuple_in_order() {
    // CORE (PRIMARY shape, DISCRIMINATING): a `Snippet<[item: Item,
    // index: number]>` carrier Ref expands its single `Params` tuple type
    // argument into TWO ordered bindings — `item` then `index`. The shared
    // Vue normalizer would never touch the carrier's type arguments; this
    // discriminates the Svelte-specific path.
    let scope = verter_type_expr::TypeExprScope::new("/Owner.svelte");
    let carrier = snippet_ref(vec![
        tuple_el(
            "item",
            TypeExpr::Ref {
                name: Arc::from("Item"),
                type_arguments: Arc::from(Vec::new().into_boxed_slice()),
            },
        ),
        tuple_el("index", TypeExpr::Primitive(PrimitiveName::Number)),
    ]);
    let bindings = snippet_callable_positional_bindings(&carrier, &scope)
        .expect("a Snippet carrier yields positional bindings");
    let names: Vec<&str> = bindings.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["item", "index"],
        "the carrier's `Params` tuple expands to ALL positional bindings in order, got {names:?}"
    );
    assert!(
        matches!(
            bindings[1].binding_expr,
            Some(TypeExpr::Primitive(PrimitiveName::Number))
        ),
        "the `index` binding type is `number`, got {:?}",
        bindings[1].binding_expr
    );
}

#[test]
fn snippet_ref_carrier_empty_params_yields_no_bindings() {
    // DISCRIMINATING: a `Snippet<[]>` carrier yields NO bindings.
    let scope = verter_type_expr::TypeExprScope::new("/Owner.svelte");
    let carrier = snippet_ref(Vec::new());
    let bindings = snippet_callable_positional_bindings(&carrier, &scope).unwrap();
    assert!(
        bindings.is_empty(),
        "a `Snippet<[]>` carrier yields no bindings"
    );
}

#[test]
fn snippet_ref_carrier_open_params_yields_no_bindings() {
    // A `Snippet<Params>` whose single type argument is NOT a tuple
    // (an open generic) carries no enumerable positional bindings.
    let scope = verter_type_expr::TypeExprScope::new("/Owner.svelte");
    let carrier = TypeExpr::Ref {
        name: Arc::from("Snippet"),
        type_arguments: Arc::from(
            vec![TypeExpr::Ref {
                name: Arc::from("Params"),
                type_arguments: Arc::from(Vec::new().into_boxed_slice()),
            }]
            .into_boxed_slice(),
        ),
    };
    assert!(
        snippet_callable_positional_bindings(&carrier, &scope).is_none_or(|b| b.is_empty()),
        "an open-generic `Snippet<Params>` yields no positional bindings"
    );
}

#[test]
fn snippet_normalizer_expands_rest_tuple_and_skips_this() {
    // CORE (DISCRIMINATING): the realized `Snippet<[item: Item, index:
    // number]>` callable is `(this: void, ...args: [item: Item, index:
    // number])`. The Svelte normalizer must SKIP `this` and EXPAND the
    // rest-tuple into TWO ordered bindings — `item: Item` then `index:
    // number`. The shared Vue normalizer (`func.parameters.first()`) would
    // take only `this` (the first param), dropping every real binding — so
    // this discriminates the Svelte-specific path.
    let scope = verter_type_expr::TypeExprScope::new("/Owner.svelte");
    let callable = snippet_callable(vec![
        tuple_el(
            "item",
            TypeExpr::Ref {
                name: Arc::from("Item"),
                type_arguments: Arc::from(Vec::new().into_boxed_slice()),
            },
        ),
        tuple_el("index", TypeExpr::Primitive(PrimitiveName::Number)),
    ]);
    let bindings = snippet_callable_positional_bindings(&callable, &scope)
        .expect("a snippet callable yields positional bindings");
    let names: Vec<&str> = bindings.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["item", "index"],
        "ALL positional params in order; `this` skipped, got {names:?}"
    );
    assert!(
        !names.contains(&"this"),
        "the leading `this` param must be skipped"
    );
    // The element types are preserved precisely.
    assert!(
        matches!(&bindings[0].binding_expr, Some(TypeExpr::Ref { name, .. }) if name.as_ref() == "Item"),
        "binding 0 is the named ref `Item`, got {:?}",
        bindings[0].binding_expr
    );
    assert!(
        matches!(
            bindings[1].binding_expr,
            Some(TypeExpr::Primitive(PrimitiveName::Number))
        ),
        "binding 1 is `number`, got {:?}",
        bindings[1].binding_expr
    );
    // Each binding is paired with a scope (pairing invariant).
    assert!(bindings.iter().all(|b| b.binding_expr_scope.is_some()));
}

#[test]
fn snippet_normalizer_empty_tuple_yields_no_bindings() {
    // DISCRIMINATING: a `Snippet<[]>` realizes to `(this: void,
    // ...args: [])` — the `this` is skipped and the empty rest-tuple
    // expands to nothing, so NO bindings.
    let scope = verter_type_expr::TypeExprScope::new("/Owner.svelte");
    let callable = snippet_callable(Vec::new());
    let bindings = snippet_callable_positional_bindings(&callable, &scope)
        .expect("an empty snippet callable still yields a (zero-length) binding list");
    assert!(
        bindings.is_empty(),
        "a `Snippet<[]>` must yield NO bindings, got {:?}",
        bindings.iter().map(|b| &b.name).collect::<Vec<_>>()
    );
}

#[test]
fn snippet_normalizer_unlabelled_tuple_elements_fall_back_to_arg_index() {
    // An unlabelled tuple element (`Snippet<[Item, number]>`) falls
    // back to `arg{index}` binding names while preserving order + types.
    let scope = verter_type_expr::TypeExprScope::new("/Owner.svelte");
    let mut e0 = tuple_el("", TypeExpr::Primitive(PrimitiveName::String));
    e0.label = None;
    let mut e1 = tuple_el("", TypeExpr::Primitive(PrimitiveName::Number));
    e1.label = None;
    let callable = snippet_callable(vec![e0, e1]);
    let bindings = snippet_callable_positional_bindings(&callable, &scope).unwrap();
    let names: Vec<&str> = bindings.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["arg0", "arg1"],
        "unlabelled tuple elements fall back to `arg{{index}}`, got {names:?}"
    );
}

#[test]
fn snippet_normalizer_union_arms_combine_by_index() {
    // A UNION of two snippet callables combines positional bindings
    // by index (intersecting types). `Snippet<[a: A]> | Snippet<[a: B]>`
    // yields one binding `a: A & B`.
    let scope = verter_type_expr::TypeExprScope::new("/Owner.svelte");
    let arm_a = snippet_callable(vec![tuple_el(
        "a",
        TypeExpr::Ref {
            name: Arc::from("A"),
            type_arguments: Arc::from(Vec::new().into_boxed_slice()),
        },
    )]);
    let arm_b = snippet_callable(vec![tuple_el(
        "a",
        TypeExpr::Ref {
            name: Arc::from("B"),
            type_arguments: Arc::from(Vec::new().into_boxed_slice()),
        },
    )]);
    let union = TypeExpr::Union(Arc::from(vec![arm_a, arm_b].into_boxed_slice()));
    let bindings = snippet_callable_positional_bindings(&union, &scope).unwrap();
    assert_eq!(bindings.len(), 1, "one positional binding across both arms");
    assert_eq!(bindings[0].name, "a");
    assert!(
        matches!(&bindings[0].binding_expr, Some(TypeExpr::Intersection(arms)) if arms.len() == 2),
        "the combined binding type is the intersection of both arms, got {:?}",
        bindings[0].binding_expr
    );
}

#[test]
fn snippet_normalizer_non_callable_value_yields_none() {
    // NEGATIVE: a non-callable member value is not a snippet — the
    // normalizer returns None (no bindings).
    let scope = verter_type_expr::TypeExprScope::new("/Owner.svelte");
    assert!(
        snippet_callable_positional_bindings(&TypeExpr::Primitive(PrimitiveName::String), &scope)
            .is_none(),
        "a primitive value is not a snippet callable"
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
    let field = |name: &str| dtos.prop_fields().iter().find(|f| f.name == name).cloned();
    assert!(
        field("size").expect("size prop").is_optional,
        "size is optional"
    );
    assert!(
        field("disabled").expect("disabled prop").is_optional,
        "disabled is optional"
    );
    assert!(
        !field("label").expect("label prop").is_optional,
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

#[test]
fn callback_event_payload_named_ref_resolves_on_the_component_meta_surface() {
    // P1 (COMPONENT-META surface, not IDE-TSX): a callback-prop event
    // `onselect: (row: Row) => void` (with `Row` a same-module interface)
    // resolves through the framework-surface resolver to an `AnalyzedEmitField`
    // whose payload `Row` reference is PRECISE — its `payload_expr_scope`
    // anchors the SAME module so a consumer re-resolves `Row` to its object
    // surface. DISCRIMINATING: if the scope is dropped (`None`), the pairing
    // breaks and the `Row` re-resolution below cannot anchor.
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
        .find(|e| e.name == "select")
        .expect("the `onselect` callback prop surfaces as event `select`");

    // PAIRING: a `Some` payload_expr MUST carry a `Some` payload_expr_scope.
    assert!(
        select.payload_expr.is_some(),
        "the `select` event carries a payload tuple"
    );
    let scope = select
        .payload_expr_scope
        .as_ref()
        .expect("payload_expr_scope must be Some when payload_expr is Some (P1 pairing)");
    // The scope anchors the OWNER module where `Row` is declared.
    assert_eq!(
        scope.as_str(),
        canonical,
        "the callback payload scope anchors the `$props` member's value-node file \
             (where `Row` is declared)"
    );

    // DISCRIMINATING named-ref resolution: take the payload tuple's `Row`
    // element type and re-resolve it THROUGH THE SHARED RESOLVER in `scope`.
    // A precise scope yields `Row`'s object surface (member `id`); a dropped
    // scope could not anchor this resolution.
    let TypeExpr::Tuple { elements, .. } = select.payload_expr.as_ref().expect("payload tuple")
    else {
        panic!("the callback payload is a labelled tuple");
    };
    let row_ty = elements
        .first()
        .map(|el| el.ty.clone())
        .expect("the `(row: Row)` callback has one parameter");
    assert!(
        matches!(&row_ty, TypeExpr::Ref { name, .. } if name.as_ref() == "Row"),
        "the payload element is the named `Row` ref, got {row_ty:?}"
    );
    let resolved = navigate_param_to_object_surface(&ctx, scope.as_str(), &row_ty)
        .expect("`Row` resolves to an object surface in its declaring scope");
    assert!(
        resolved.members.iter().any(|m| m.name.as_ref() == "id"),
        "the resolved `Row` surface carries member `id` (precise named-ref \
             resolution via the payload scope), got members {:?}",
        resolved
            .members
            .iter()
            .map(|m| m.name.as_ref())
            .collect::<Vec<_>>()
    );
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
    let names: Vec<&str> = emits.fields.iter().map(|e| e.name.as_str()).collect();

    // (a) the OPTIONAL callback prop IS event `select`.
    let select = emits
        .fields
        .iter()
        .find(|e| e.name == "select")
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

    // The optional callback's payload is PRECISE — `Row` resolves in scope.
    let scope = select
        .payload_expr_scope
        .as_ref()
        .expect("optional callback payload_expr_scope is Some (pairing)");
    let TypeExpr::Tuple { elements, .. } = select.payload_expr.as_ref().expect("payload tuple")
    else {
        panic!("optional callback payload is a tuple");
    };
    let row_ty = elements
        .first()
        .map(|el| el.ty.clone())
        .expect("the `(row: Row)` callback has one parameter");
    let resolved = navigate_param_to_object_surface(&ctx, scope.as_str(), &row_ty)
        .expect("`Row` resolves through the optional callback payload scope");
    assert!(
        resolved.members.iter().any(|m| m.name.as_ref() == "id"),
        "the optional callback's `Row` payload resolves precisely (member `id`)"
    );
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
        !emits.fields.iter().any(|e| e.name == "mode"),
        "an `on`-prefixed union with no callable arm must NOT be an event, got {:?}",
        emits
            .fields
            .iter()
            .map(|e| e.name.as_str())
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
        .find(|e| e.name == "select")
        .unwrap_or_else(|| {
            panic!(
                "an OPTIONAL alias callback prop `onselect?: Handler` must classify as \
                 event `select` (the alias arm is realised, the `| undefined` arm stripped), \
                 got {:?}",
                emits
                    .fields
                    .iter()
                    .map(|e| e.name.as_str())
                    .collect::<Vec<_>>()
            )
        });
    let scope = select
        .payload_expr_scope
        .as_ref()
        .expect("optional alias callback payload_expr_scope is Some (pairing)");
    let TypeExpr::Tuple { elements, .. } = select.payload_expr.as_ref().expect("payload tuple")
    else {
        panic!("optional alias callback payload is a tuple");
    };
    let row_ty = elements
        .first()
        .map(|el| el.ty.clone())
        .expect("the `(row: Row)` callback has one parameter");
    let resolved = navigate_param_to_object_surface(&ctx, scope.as_str(), &row_ty)
        .expect("`Row` resolves through the optional alias callback payload scope");
    assert!(
        resolved.members.iter().any(|m| m.name.as_ref() == "id"),
        "the optional alias callback's `Row` payload resolves precisely (member `id`)"
    );
}

#[test]
fn explicit_union_callback_prop_value_classifies_as_event_with_precise_payload() {
    // P2 (COMPONENT-META surface): a prop whose WRITTEN VALUE is an EXPLICIT
    // union containing a callable arm — `onselect: ((row: Row) => void) |
    // undefined` (NOT member-`?` optionality, which is carried by the surface
    // `optional` flag and raises to a BARE `Function`). The explicit union
    // raises to `Union([Function, Primitive(Undefined)])`; the shared
    // callable-arm extractor strips the nullish arm and pulls out the single
    // callable. It MUST classify as event `select` with a PRECISE `(row: Row)`
    // payload.
    //
    // DISCRIMINATING (the whole point): this exercises the
    // `Union`/`Intersection` arm of `callable_arm_from_raised`. If that helper
    // is reverted to a bare `TypeExpr::Function(func)` match, this test goes
    // RED (no `select` event) while the member-`?` tests above stay GREEN
    // (they raise to a bare `Function`). A non-callable explicit-union prop
    // (`onmode: "a" | "b"`) is NOT an event (asserted negatively here too).
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
    let names: Vec<&str> = emits.fields.iter().map(|e| e.name.as_str()).collect();

    // (a) the EXPLICIT-union callable VALUE IS event `select` (this is the
    // branch the member-`?` tests do NOT cover — they raise to a bare
    // `Function`, this raises to a `Union`).
    let select = emits
        .fields
        .iter()
        .find(|e| e.name == "select")
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

    // The payload is PRECISE — `Row` resolves in scope (member `id`).
    let scope = select
        .payload_expr_scope
        .as_ref()
        .expect("explicit-union callback payload_expr_scope is Some (pairing)");
    let TypeExpr::Tuple { elements, .. } = select.payload_expr.as_ref().expect("payload tuple")
    else {
        panic!("explicit-union callback payload is a tuple");
    };
    let row_ty = elements
        .first()
        .map(|el| el.ty.clone())
        .expect("the `(row: Row)` callback has one parameter");
    let resolved = navigate_param_to_object_surface(&ctx, scope.as_str(), &row_ty)
        .expect("`Row` resolves through the explicit-union callback payload scope");
    assert!(
        resolved.members.iter().any(|m| m.name.as_ref() == "id"),
        "the explicit-union callback's `Row` payload resolves precisely (member `id`)"
    );
}

#[test]
fn explicit_union_with_two_distinct_callable_arms_refuses() {
    // P2 (COMPONENT-META surface): the ambiguity branch of
    // `callable_arm_from_raised`. An `on`-prefixed prop whose explicit-union
    // VALUE has TWO DISTINCT callable arms — `onselect: ((row: Row) => void) |
    // ((id: number) => void)` — is AMBIGUOUS: the extractor must REFUSE rather
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
        !emits.fields.iter().any(|e| e.name == "select"),
        "an explicit union with TWO distinct callable arms is ambiguous and must NOT be \
             mined as an event, got {:?}",
        emits
            .fields
            .iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>()
    );
}
