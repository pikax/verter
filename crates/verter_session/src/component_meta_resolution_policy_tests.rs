//! Unit tests for the Phase 4B policy pass.

use std::sync::Arc;

use verter_semantic::analysis::component_meta::{
    AcceptedSurfaceCompleteness, ComponentMetaAnalysis, ComponentMetaFlags, FallthroughSurface,
    NoFallthroughReason, PropAnalysis, PublicInstanceAnalysis, PublicInstanceCompleteness,
    ResolvedTypeAnalysis, RootReachability, SlotAnalysis, SlotBindingAnalysis,
};
use verter_type_expr::{
    LiteralValue, ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName, TypeExpr,
};

use crate::component_meta_resolution_policy::apply_component_meta_resolution_policy;
use crate::resolver_core::component_meta::ResolvedTypeRegistryMeta;
use crate::resolver_core::{ResolvedDeclarationKind, ResolvedTypeDeclaration};
use crate::types::HostConfig;
use crate::VerterHost;

fn empty_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn run_policy(
    meta: &mut ComponentMetaAnalysis,
    registry: &[ResolvedTypeAnalysis],
    registry_meta: &[ResolvedTypeRegistryMeta],
) {
    let host = empty_host();
    apply_component_meta_resolution_policy(meta, registry, registry_meta, &host, "/owner.vue");
}

fn empty_meta() -> ComponentMetaAnalysis {
    ComponentMetaAnalysis {
        props: vec![],
        events: vec![],
        slots: vec![],
        models: vec![],
        exposed: vec![],
        public_instance: None,
        sfc_blocks: None,
        type_registry: vec![],
        components: vec![],
        template_refs: vec![],
        imports: vec![],
        bindings: vec![],
        vue_api_calls: vec![],
        styles: vec![],
        flags: ComponentMetaFlags::default(),
        root_reachability: RootReachability::NoFallthrough {
            reason: NoFallthroughReason::NoTemplate,
        },
        accepted_props: vec![],
        accepted_events: vec![],
        accepted_surface_completeness: AcceptedSurfaceCompleteness::Exact,
        fallthrough_surface: FallthroughSurface::None {
            reason: NoFallthroughReason::NoTemplate,
        },
        macro_expansion_diagnostics: vec![],
        options_api: false,
        file_path: String::from("/fixture/Component.vue"),
    }
}

fn prop(name: &str, type_expr: TypeExpr) -> PropAnalysis {
    PropAnalysis {
        name: name.to_string(),
        type_expr,
        type_expansion: None,
        raw_type: None,
        raw_type_expr: None,
        required: false,
        has_default: false,
        default_value: None,
        description: None,
        tags: vec![],
    }
}

fn ref_zero(name: &str) -> TypeExpr {
    TypeExpr::Ref {
        name: Arc::from(name),
        type_arguments: Arc::from(Vec::<TypeExpr>::new()),
    }
}

fn registry_entry(name: &str, body: TypeExpr) -> ResolvedTypeAnalysis {
    ResolvedTypeAnalysis {
        name: name.to_string(),
        type_expr: body,
        type_expansion: None,
    }
}

fn meta_entry(name: &str, canonical_source: &str) -> ResolvedTypeRegistryMeta {
    ResolvedTypeRegistryMeta {
        name: name.to_string(),
        declaration: ResolvedTypeDeclaration {
            requested_name: name.to_string(),
            declaration_id: None,
            resolved_name: name.to_string(),
            canonical_source: canonical_source.to_string(),
            span: verter_span::Span::default(),
            kind: ResolvedDeclarationKind::TypeAlias,
            text: None,
        },
    }
}

fn object_with_property(prop_name: &str, ty: TypeExpr) -> TypeExpr {
    TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![ObjectMember::Property(ObjectProperty {
            name: prop_name.to_string(),
            ty,
            optional: false,
            readonly: false,
        })],
    }))
}

#[test]
fn rule3_resolves_project_local_non_props_to_object() {
    let mut meta = empty_meta();
    meta.props.push(prop("user", ref_zero("ImportedUser")));

    let imported_user_body = object_with_property("id", TypeExpr::Primitive(PrimitiveName::Number));
    let registry = vec![registry_entry("ImportedUser", imported_user_body.clone())];
    let registry_meta = vec![meta_entry("ImportedUser", "/workspace/types.ts")];

    run_policy(&mut meta, &registry, &registry_meta);

    assert_eq!(
        meta.props[0].type_expr, imported_user_body,
        "Rule 3 should resolve project-local non-Props ref to its registry body"
    );
}

#[test]
fn rule3_resolves_project_local_alias_union_literal() {
    let mut meta = empty_meta();
    meta.props.push(prop("status", ref_zero("Status")));

    let status_body = TypeExpr::Union(Arc::from(vec![
        TypeExpr::Literal(LiteralValue::String("idle".to_string())),
        TypeExpr::Literal(LiteralValue::String("busy".to_string())),
    ]));
    let registry = vec![registry_entry("Status", status_body.clone())];
    let registry_meta = vec![meta_entry("Status", "/workspace/types.ts")];

    run_policy(&mut meta, &registry, &registry_meta);

    assert_eq!(
        meta.props[0].type_expr, status_body,
        "Rule 3 should resolve project-local non-Props alias to union of literals"
    );
}

#[test]
fn rule4_keeps_props_suffix_ref_symbolic() {
    let mut meta = empty_meta();
    meta.props.push(prop("avatar", ref_zero("AvatarProps")));

    // Even with a registry body present, *Props stays symbolic.
    let registry = vec![registry_entry(
        "AvatarProps",
        object_with_property("size", TypeExpr::Primitive(PrimitiveName::Number)),
    )];
    let registry_meta = vec![meta_entry("AvatarProps", "/workspace/avatar.ts")];

    run_policy(&mut meta, &registry, &registry_meta);

    assert!(
        matches!(
            &meta.props[0].type_expr,
            TypeExpr::Ref { name, type_arguments }
                if name.as_ref() == "AvatarProps" && type_arguments.is_empty()
        ),
        "Rule 4 should keep *Props refs symbolic; got {:?}",
        meta.props[0].type_expr,
    );
}

#[test]
fn rule4_keeps_array_of_props_symbolic() {
    let mut meta = empty_meta();
    meta.props.push(prop(
        "actions",
        TypeExpr::Array {
            element: Arc::new(ref_zero("ButtonProps")),
            readonly: false,
        },
    ));

    let registry = vec![registry_entry(
        "ButtonProps",
        object_with_property("label", TypeExpr::Primitive(PrimitiveName::String)),
    )];
    let registry_meta = vec![meta_entry("ButtonProps", "/workspace/button.ts")];

    run_policy(&mut meta, &registry, &registry_meta);

    assert!(
        matches!(
            &meta.props[0].type_expr,
            TypeExpr::Array { element, .. }
                if matches!(
                    element.as_ref(),
                    TypeExpr::Ref { name, type_arguments }
                        if name.as_ref() == "ButtonProps" && type_arguments.is_empty()
                )
        ),
        "Rule 4 + 5: Array<*Props> recurses but leaves the *Props leaf symbolic; got {:?}",
        meta.props[0].type_expr,
    );
}

#[test]
fn rule2_keeps_indexed_access_on_props_suffix_symbolic() {
    let mut meta = empty_meta();
    let indexed = TypeExpr::IndexedAccess {
        object: Arc::new(ref_zero("Button")),
        index: Arc::new(TypeExpr::Literal(LiteralValue::String("ui".to_string()))),
    };
    meta.props.push(prop("ui", indexed.clone()));

    run_policy(&mut meta, &[], &[]);

    // Note: Button (no Props suffix) — the IndexedAccess should still be
    // recursed via Rule 5 but Button itself has no registry body, so Rule 1
    // doesn't fire and Rule 4 doesn't apply (no Props suffix). Net: unchanged.
    assert_eq!(
        meta.props[0].type_expr, indexed,
        "IndexedAccess unchanged when no registry body and not Props-suffix"
    );
}

#[test]
fn rule2_button_props_indexed_access_stays_symbolic() {
    let mut meta = empty_meta();
    // ButtonProps['ui'] — Rule 2 keeps member-path-on-Props symbolic.
    let indexed = TypeExpr::IndexedAccess {
        object: Arc::new(ref_zero("ButtonProps")),
        index: Arc::new(TypeExpr::Literal(LiteralValue::String("ui".to_string()))),
    };
    meta.props.push(prop("ui", indexed.clone()));

    let registry = vec![registry_entry(
        "ButtonProps",
        object_with_property(
            "ui",
            object_with_property("base", TypeExpr::Primitive(PrimitiveName::String)),
        ),
    )];
    let registry_meta = vec![meta_entry("ButtonProps", "/workspace/button.ts")];

    run_policy(&mut meta, &registry, &registry_meta);

    assert!(
        matches!(&meta.props[0].type_expr, TypeExpr::IndexedAccess { object, .. }
            if matches!(object.as_ref(), TypeExpr::Ref { name, .. } if name.as_ref() == "ButtonProps")),
        "Rule 2: IndexedAccess on *Props stays symbolic; got {:?}",
        meta.props[0].type_expr,
    );
}

#[test]
fn rule1_keeps_package_backed_refs_symbolic() {
    let mut meta = empty_meta();
    meta.props.push(prop("vnode", ref_zero("VNode")));

    let registry = vec![registry_entry(
        "VNode",
        object_with_property("type", TypeExpr::Primitive(PrimitiveName::String)),
    )];
    let registry_meta = vec![meta_entry(
        "VNode",
        "/workspace/node_modules/vue/dist/vue.d.ts",
    )];

    run_policy(&mut meta, &registry, &registry_meta);

    assert!(
        matches!(
            &meta.props[0].type_expr,
            TypeExpr::Ref { name, .. } if name.as_ref() == "VNode"
        ),
        "Rule 1: package-backed Ref stays symbolic; got {:?}",
        meta.props[0].type_expr,
    );
}

#[test]
fn rule3_does_not_fire_when_registry_body_is_just_another_ref() {
    let mut meta = empty_meta();
    meta.props.push(prop("a", ref_zero("AliasA")));

    // AliasA → AliasB: Rule 3 sees AliasA's body is a Ref and refuses to
    // chase (otherwise it would produce an opaque non-resolved Ref body).
    let registry = vec![registry_entry("AliasA", ref_zero("AliasB"))];
    let registry_meta = vec![meta_entry("AliasA", "/workspace/types.ts")];

    run_policy(&mut meta, &registry, &registry_meta);

    assert!(
        matches!(
            &meta.props[0].type_expr,
            TypeExpr::Ref { name, .. } if name.as_ref() == "AliasA"
        ),
        "Rule 3 must NOT chase a Ref-only body; got {:?}",
        meta.props[0].type_expr,
    );
}

#[test]
fn rule3_recurses_into_resolved_body_for_nested_imports() {
    // Component carries `prop: Container` where Container resolves to
    // `{ first: First }` and First resolves to `{ id: number }`. The pass
    // should recursively apply policy to the resolved body so the nested Ref
    // also resolves.
    let mut meta = empty_meta();
    meta.props.push(prop("data", ref_zero("Container")));

    let registry = vec![
        registry_entry(
            "Container",
            object_with_property("first", ref_zero("First")),
        ),
        registry_entry(
            "First",
            object_with_property("id", TypeExpr::Primitive(PrimitiveName::Number)),
        ),
    ];
    let registry_meta = vec![
        meta_entry("Container", "/workspace/types.ts"),
        meta_entry("First", "/workspace/types.ts"),
    ];

    run_policy(&mut meta, &registry, &registry_meta);

    // After Rule 3 + Rule 5: data.type_expr = { first: { id: number } }.
    let expected = object_with_property(
        "first",
        object_with_property("id", TypeExpr::Primitive(PrimitiveName::Number)),
    );
    assert_eq!(
        meta.props[0].type_expr, expected,
        "Rule 3 should recurse into resolved bodies",
    );
}

#[test]
fn pass_recomputes_public_instance_after_rewrite() {
    let mut meta = empty_meta();
    meta.props.push(prop("x", ref_zero("Status")));

    let body = TypeExpr::Primitive(PrimitiveName::String);
    let registry = vec![registry_entry("Status", body.clone())];
    let registry_meta = vec![meta_entry("Status", "/workspace/types.ts")];

    assert!(meta.public_instance.is_none());
    run_policy(&mut meta, &registry, &registry_meta);

    let public = meta
        .public_instance
        .as_ref()
        .expect("policy pass must rebuild public_instance after rewrite");
    let x_member = public
        .members
        .iter()
        .find(|m| m.name == "x")
        .expect("x prop should be in public_instance");
    assert_eq!(x_member.type_expr, body);
}

#[test]
fn pass_does_not_touch_public_instance_when_no_rewrite() {
    let mut meta = empty_meta();
    meta.props
        .push(prop("simple", TypeExpr::Primitive(PrimitiveName::String)));
    // Pre-populate public_instance with a sentinel; ensure it survives.
    meta.public_instance = Some(PublicInstanceAnalysis {
        members: vec![],
        completeness: PublicInstanceCompleteness::Partial,
    });
    let before = meta.public_instance.clone();
    run_policy(&mut meta, &[], &[]);
    assert_eq!(
        format!("{:?}", meta.public_instance),
        format!("{:?}", before),
        "no-op policy must not rewrite public_instance"
    );
}

// W2.4 discriminating fixtures ───────────────────────────────────────────
//
// Locks down the typed-IR-only contract for the two policy helpers
// `restore_props_suffix_from_raw` and `slot_binding_should_preserve_symbolic_raw_type`.
// Both helpers consume `Option<&TypeExpr>` (the analyzer's lowered
// source-annotation typed form) — never the raw text.
//
// These tests intentionally pass `raw_type_expr: Some(...)` while
// leaving `raw_type: None` — that combination would not type-check
// against the pre-W2.4 signature (`Option<&str>`), so the test FAILS
// pre-cutover by construction and PASSES post-cutover.

#[test]
fn w2_4_restore_props_suffix_from_typed_annotation_replaces_eager_object() {
    // Resolved `type_expr` is the eagerly-expanded Object body (the
    // evaluator inlined `ButtonProps` into `{ label: string }`); the
    // typed source annotation is the symbolic `Array<ButtonProps>` the
    // user wrote. Policy must restore the symbolic form.
    let mut meta = empty_meta();

    let eager_array = TypeExpr::Array {
        element: Arc::new(object_with_property(
            "label",
            TypeExpr::Primitive(PrimitiveName::String),
        )),
        readonly: false,
    };
    let symbolic_array = TypeExpr::Array {
        element: Arc::new(ref_zero("ButtonProps")),
        readonly: false,
    };

    meta.props.push(PropAnalysis {
        name: "actions".to_string(),
        type_expr: eager_array,
        type_expansion: None,
        raw_type: None,
        // `raw_type_expr` is the typed form of the user's source
        // annotation, lowered by the analyzer's `lower_ts_type` pass.
        raw_type_expr: Some(symbolic_array.clone()),
        required: false,
        has_default: false,
        default_value: None,
        description: None,
        tags: vec![],
    });

    // `ButtonProps` lives in an imported file — the policy needs
    // canonical_source != owner_canonical to fire.
    let registry = vec![registry_entry(
        "ButtonProps",
        object_with_property("label", TypeExpr::Primitive(PrimitiveName::String)),
    )];
    let registry_meta = vec![meta_entry("ButtonProps", "/workspace/button.ts")];

    run_policy(&mut meta, &registry, &registry_meta);

    // The resolved `type_expr` was rewritten back to the symbolic
    // `Array<ButtonProps>` — the policy walked the typed annotation
    // directly without ever stringifying it.
    assert_eq!(
        meta.props[0].type_expr, symbolic_array,
        "restore_props_suffix_from_raw should rewrite eager Array<{{label}}> back to typed Array<ButtonProps>"
    );
    // Negative assertion: the resolved form must not contain a literal
    // Object body — that would mean the symbolic restore was bypassed.
    let TypeExpr::Array { element, .. } = &meta.props[0].type_expr else {
        panic!("expected Array; got {:?}", meta.props[0].type_expr);
    };
    assert!(
        matches!(
            element.as_ref(),
            TypeExpr::Ref { name, type_arguments }
                if name.as_ref() == "ButtonProps" && type_arguments.is_empty()
        ),
        "Array element must be the symbolic ButtonProps ref, not an inlined Object; got {:?}",
        element,
    );
}

#[test]
fn w2_4_slot_binding_preserve_typed_indexed_access_via_imported_root() {
    // Slot binding's `type_expr` was widened by the evaluator to
    // `unknown`; the typed source annotation is the symbolic
    // `AppProps['avatar']`. The root `AppProps` lives in an imported
    // file and its `avatar` property type contains an imported `Avatar`
    // ref — the policy guard's "imported root" condition holds. The
    // typed annotation is restored verbatim.
    let mut meta = empty_meta();

    let symbolic_indexed = TypeExpr::IndexedAccess {
        object: Arc::new(ref_zero("AppProps")),
        index: Arc::new(TypeExpr::Literal(LiteralValue::String(
            "avatar".to_string(),
        ))),
    };

    meta.slots.push(SlotAnalysis {
        name: "default".to_string(),
        is_scoped: true,
        bindings: vec![SlotBindingAnalysis {
            name: "avatar".to_string(),
            // Eagerly widened to `unknown` by the evaluator.
            type_expr: TypeExpr::Unknown {
                raw: "unknown".to_string(),
            },
            type_expansion: None,
            raw_type: None,
            // The typed source annotation walks the symbolic indexed
            // access; the post-W2.4 helper inspects this directly.
            raw_type_expr: Some(symbolic_indexed.clone()),
        }],
        is_required: false,
        return_type: None,
        return_expr: None,
        return_expr_scope: None,
        description: None,
        tags: vec![],
    });

    // `AppProps.avatar: Avatar`; both `AppProps` and `Avatar` live in
    // imported files — the guard's imported-root + imported-leaf
    // condition holds.
    let registry = vec![
        registry_entry(
            "AppProps",
            object_with_property("avatar", ref_zero("Avatar")),
        ),
        registry_entry(
            "Avatar",
            object_with_property("url", TypeExpr::Primitive(PrimitiveName::String)),
        ),
    ];
    let registry_meta = vec![
        meta_entry("AppProps", "/workspace/app.ts"),
        meta_entry("Avatar", "/workspace/avatar.ts"),
    ];

    run_policy(&mut meta, &registry, &registry_meta);

    let binding = &meta.slots[0].bindings[0];
    assert_eq!(
        binding.type_expr, symbolic_indexed,
        "slot_binding_should_preserve_symbolic_raw_type should restore the symbolic IndexedAccess from raw_type_expr"
    );
    // Negative assertion: the binding must not stay `Unknown` — that
    // would mean the typed-IR guard never fired.
    assert!(
        !matches!(&binding.type_expr, TypeExpr::Unknown { .. }),
        "binding.type_expr must not remain Unknown after preservation; got {:?}",
        binding.type_expr,
    );
}
