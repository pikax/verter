//! Publication-policy tests over content-free SOURCES.
//!
//! The policy rewrites `SemanticTypeSource` driver fields node-domain:
//! fixtures upsert REAL files (declaration bodies come from the engine's
//! authored decl-body locators, exactly the production route) or carry
//! synthesized closed shapes, run the policy, and assert on the published
//! SOURCE plus the semantic-graph node it raises to through the one shared
//! dispatch — never on a materialized `TypeExpr`.

use std::sync::Arc;

use verter_semantic::analysis::component_meta::{
    AcceptedSurfaceCompleteness, ComponentMetaAnalysis, ComponentMetaFlags, FallthroughSurface,
    NoFallthroughReason, PropAnalysis, ResolvedTypeAnalysis, RootReachability, SlotAnalysis,
    SlotBindingAnalysis,
};
use verter_type_expr::facts::{
    ClosedTypeFact, FactOrLocator, LeafTypeFact, ResolvedLocalShape, SemanticTypeSource,
    SynthesizedMemberFact,
};
use verter_type_expr::locators::{
    AuthoredAnchor, AuthoredBodyLocator, LocatorSymbolSpace, TypeBodySlot,
};

use crate::component_meta_resolution_policy::apply_component_meta_resolution_policy;
use crate::resolver_core::component_meta::ResolvedTypeRegistryMeta;
use crate::resolver_core::{ResolvedDeclarationKind, ResolvedTypeDeclaration};
use crate::semantic_query::{SemanticNodeData, SemanticNodeId};
use crate::types::{HostConfig, UpsertRequest};
use crate::{FileLanguage, VerterHost};

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

fn empty_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn upsert_ts(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();
}

fn run_policy(
    host: &VerterHost,
    meta: &mut ComponentMetaAnalysis,
    registry: &[ResolvedTypeAnalysis],
    registry_meta: &[ResolvedTypeRegistryMeta],
) {
    crate::resolver_core::with_bare_host_ctx_for_test(host, |ctx| {
        apply_component_meta_resolution_policy(
            meta,
            registry,
            registry_meta,
            host,
            "/owner.vue",
            None,
            ctx,
        );
    });
}

/// Run the policy with a pre-built macro-participation set whose
/// identities are derived from the registry's canonical_source for
/// each entry in `macro_participating_names`. This is the §3.4
/// structural macro-participation classifier hook: any name in the
/// list will resolve to a `ResolvedRootIdentity` that the policy
/// treats as role-bearing (kept symbolic per Rules 2 / 4 + the
/// raw-restoration helpers).
///
/// Production code paths build the set from `snapshot.macros` via
/// `build_policy_macro_role_identities` — see
/// `apply_component_meta_resolution_policy`. The
/// `_with_participation` entry point exists for unit tests that don't
/// stand up analyzer snapshots but still want to exercise §3.4
/// structural classification.
fn run_policy_with_macro_participation(
    host: &VerterHost,
    meta: &mut ComponentMetaAnalysis,
    registry: &[ResolvedTypeAnalysis],
    registry_meta: &[ResolvedTypeRegistryMeta],
    macro_participating_names: &[&str],
) {
    use rustc_hash::FxHashSet;
    use verter_semantic::analysis::type_solver::host::ResolvedRootIdentity;

    let mut participating: FxHashSet<ResolvedRootIdentity> = FxHashSet::default();
    for name in macro_participating_names.iter() {
        // Derive the identity the same way the policy's registry-fallback
        // resolver derives it: the canonical_source from registry_meta is
        // the file declaring the type.
        if let Some(rm) = registry_meta.iter().find(|m| m.name == *name) {
            participating.insert(ResolvedRootIdentity::new(
                rm.declaration.canonical_source.as_str(),
                *name,
            ));
        } else {
            // No registry meta entry — treat as owner-local (the policy's
            // host path would resolve a locally-declared name to the
            // owner_canonical).
            participating.insert(ResolvedRootIdentity::new("/owner.vue", *name));
        }
    }
    crate::resolver_core::with_bare_host_ctx_for_test(host, |ctx| {
        crate::component_meta_resolution_policy::apply_component_meta_resolution_policy_with_participation(
            meta,
            registry,
            registry_meta,
            host,
            "/owner.vue",
            &participating,
            ctx,
        );
    });
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

fn prop(name: &str, type_source: SemanticTypeSource) -> PropAnalysis {
    PropAnalysis {
        name: name.to_string(),
        type_source: verter_type_expr::facts::SourcePosition::Present(type_source),
        type_expansion: None,
        raw_type: None,
        raw_type_source: None,
        required: false,
        has_default: false,
        default_value: None,
        description: None,
        tags: vec![],
        declared_in_macro_type_arg: false,
    }
}

/// The shallow named-reference SOURCE (`Closed(Leaf(Ref(name)))`) — the
/// same seed shape the production registry publishes.
fn ref_source(name: &str) -> SemanticTypeSource {
    SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Ref(name.to_string())))
}

/// The authored decl-body SOURCE for `(canonical, symbol)` — the same
/// carrier the engine's `named_decl_body` route publishes.
fn decl_body_source(canonical: &str, symbol: &str) -> SemanticTypeSource {
    SemanticTypeSource::Authored(AuthoredBodyLocator::DeclBody(TypeBodySlot {
        anchor: AuthoredAnchor {
            canonical_id: Arc::from(canonical),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from(symbol),
            space: LocatorSymbolSpace::Type,
        },
        path: Arc::from([]),
    }))
}

/// A synthesized closed object SOURCE with leaf-fact member values.
fn synthesized_object(members: &[(&str, LeafTypeFact)]) -> SemanticTypeSource {
    SemanticTypeSource::Synthesized(ResolvedLocalShape::Object(Arc::from(
        members
            .iter()
            .map(|(name, leaf)| SynthesizedMemberFact {
                name: name.to_string(),
                ty: FactOrLocator::Leaf(leaf.clone()),
                optional: false,
                span_origin: verter_type_expr::span_origins::MemberSpansOrigin::Synthetic(
                    verter_type_expr::span_origins::SourceSynthetic,
                ),
            })
            .collect::<Vec<_>>(),
    )))
}

fn registry_entry(name: &str, type_source: SemanticTypeSource) -> ResolvedTypeAnalysis {
    ResolvedTypeAnalysis {
        name: name.to_string(),
        type_source: verter_type_expr::facts::SourcePosition::Present(type_source),
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
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            span: verter_span::Span::default(),
            kind: ResolvedDeclarationKind::TypeAlias,
            text: None,
        },
    }
}

/// Raise a published source through the one shared dispatch (owner scope)
/// and return the node.
fn raise(host: &VerterHost, source: &SemanticTypeSource) -> Option<SemanticNodeId> {
    let mut out = None;
    crate::resolver_core::with_bare_host_ctx_for_test(host, |ctx| {
        let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);
        out = dispatch
            .raise_semantic_type_source_to_hot(
                source,
                crate::project_semantic_dispatch::semantic_source::SourceRaiseContext {
                    scope_canonical_id: "/owner.vue",
                    scope_owner: verter_type_expr::TopLevelOwnerId::instance(0),
                    context:
                        crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                            crate::semantic_query::ProjectionMode::Navigate,
                        ),
                    interior_failures: None,
                },
            )
            .map(|hot| hot.node());
    });
    out
}

fn node_data(host: &VerterHost, node: SemanticNodeId) -> Option<Arc<SemanticNodeData>> {
    let mut out = None;
    crate::resolver_core::with_bare_host_ctx_for_test(host, |ctx| {
        out = crate::project_semantic_dispatch::node_data_for(ctx, node);
    });
    out
}

/// The raised node's object member names (empty when not an object root).
fn object_member_names(host: &VerterHost, node: SemanticNodeId) -> Vec<String> {
    match node_data(host, node).as_deref() {
        Some(SemanticNodeData::Object(surface)) => surface
            .members
            .iter()
            .map(|member| member.name.to_string())
            .collect(),
        _ => Vec::new(),
    }
}

/// The node's reference head (name, arg count), read through the shared
/// node-domain extractor.
fn ref_head(host: &VerterHost, node: SemanticNodeId) -> Option<(String, usize)> {
    let mut out = None;
    crate::resolver_core::with_bare_host_ctx_for_test(host, |ctx| {
        out = crate::resolver_core::component_meta_registry::component_meta_registry_node_ref_head(
            ctx, node,
        )
        .map(|(name, args)| (name, args.len()));
    });
    out
}

/// The published source is (still) the bare named-reference leaf.
fn source_is_bare_ref(source: Option<&SemanticTypeSource>, name: &str) -> bool {
    matches!(
        source,
        Some(SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Ref(n))))
            if n == name
    )
}

// ---------------------------------------------------------------------------
// Rule 3 — project-local non-participating refs chase to the declaration body
// ---------------------------------------------------------------------------

#[test]
fn rule3_resolves_project_local_non_props_to_object() {
    let host = empty_host();
    upsert_ts(
        &host,
        "/workspace/user.ts",
        "export type ImportedUser = { id: number };",
    );
    let mut meta = empty_meta();
    meta.props.push(prop("user", ref_source("ImportedUser")));

    let registry = vec![registry_entry("ImportedUser", ref_source("ImportedUser"))];
    let registry_meta = vec![meta_entry("ImportedUser", "/workspace/user.ts")];

    run_policy(&host, &mut meta, &registry, &registry_meta);

    // Rule 3 publishes the located declaration's authored body SOURCE; the
    // raised node is the concrete object surface with the `id` member.
    let published = meta.props[0].type_source.present().expect("typed prop");
    assert!(
        matches!(published, SemanticTypeSource::Authored(_)),
        "Rule 3 must publish the authored declaration-body source; got {published:?}",
    );
    let node = raise(&host, published).expect("published source must raise");
    assert_eq!(
        object_member_names(&host, node),
        vec!["id".to_string()],
        "the raised body must be the object surface with the `id` member",
    );
    // Negative: the published source is no longer the bare seed.
    assert!(
        !source_is_bare_ref(meta.props[0].type_source.present(), "ImportedUser"),
        "Rule 3 must replace the bare named-reference seed",
    );
}

#[test]
fn rule3_resolves_project_local_alias_union_literal() {
    let host = empty_host();
    upsert_ts(
        &host,
        "/workspace/types.ts",
        "export type Status = 'idle' | 'busy';",
    );
    let mut meta = empty_meta();
    meta.props.push(prop("status", ref_source("Status")));

    let registry = vec![registry_entry("Status", ref_source("Status"))];
    let registry_meta = vec![meta_entry("Status", "/workspace/types.ts")];

    run_policy(&host, &mut meta, &registry, &registry_meta);

    let published = meta.props[0].type_source.present().expect("typed prop");
    assert!(
        matches!(published, SemanticTypeSource::Authored(_)),
        "Rule 3 must publish the authored declaration-body source; got {published:?}",
    );
    let node = raise(&host, published).expect("published source must raise");
    let arms = match node_data(&host, node).as_deref() {
        Some(SemanticNodeData::Union(arms)) => arms.to_vec(),
        other => panic!("published body must raise to the union of literals; got {other:?}"),
    };
    let mut literals: Vec<String> = arms
        .iter()
        .filter_map(|arm| match node_data(&host, *arm).as_deref() {
            Some(SemanticNodeData::Literal(verter_type_expr::LiteralValue::String(text))) => {
                Some(text.clone())
            }
            _ => None,
        })
        .collect();
    literals.sort();
    assert_eq!(
        literals,
        vec!["busy".to_string(), "idle".to_string()],
        "publication policy must resolve the project-local alias body to its literal union",
    );
}

#[test]
fn rule3_does_not_fire_when_registry_body_is_just_another_ref() {
    // AliasA → AliasB where AliasB does NOT resolve: the alias-spine
    // descent finds no located body for AliasB, so AliasA's published
    // source stays the bare seed (no opaque half-chased publication).
    let host = empty_host();
    upsert_ts(&host, "/workspace/types.ts", "export type AliasA = AliasB;");
    let mut meta = empty_meta();
    meta.props.push(prop("a", ref_source("AliasA")));

    let registry = vec![registry_entry("AliasA", ref_source("AliasA"))];
    let registry_meta = vec![meta_entry("AliasA", "/workspace/types.ts")];

    run_policy(&host, &mut meta, &registry, &registry_meta);

    assert!(
        source_is_bare_ref(meta.props[0].type_source.present(), "AliasA"),
        "Rule 3 must NOT publish a half-chased alias spine; got {:?}",
        meta.props[0].type_source,
    );
}

#[test]
fn rule3_alias_spine_descends_to_the_resolvable_body() {
    // AliasA → AliasB → { x: 1 }: the alias-spine descent (guarded per
    // declaration) adopts the first structurally-resolvable body on the
    // spine.
    let host = empty_host();
    upsert_ts(
        &host,
        "/workspace/types.ts",
        "export type AliasB = { x: number };\nexport type AliasA = AliasB;",
    );
    let mut meta = empty_meta();
    meta.props.push(prop("a", ref_source("AliasA")));

    let registry = vec![registry_entry("AliasA", ref_source("AliasA"))];
    let registry_meta = vec![meta_entry("AliasA", "/workspace/types.ts")];

    run_policy(&host, &mut meta, &registry, &registry_meta);

    let published = meta.props[0].type_source.present().expect("typed prop");
    let node = raise(&host, published).expect("published source must raise");
    assert_eq!(
        object_member_names(&host, node),
        vec!["x".to_string()],
        "the alias spine must chase through AliasB to the object body",
    );
}

#[test]
fn rule3_publishes_nested_refs_shallow() {
    // `Container = { first: First }` — Rule 3 publishes Container's body;
    // the nested `First` member VALUE stays a shallow reference carrier
    // (consumers re-resolve it on demand). Eagerly inlining `First`'s body
    // into the published surface is the forbidden shape
    // (shallow-by-default).
    let host = empty_host();
    upsert_ts(
        &host,
        "/workspace/types.ts",
        "export type First = { id: number };\nexport type Container = { first: First };",
    );
    let mut meta = empty_meta();
    meta.props.push(prop("data", ref_source("Container")));

    let registry = vec![
        registry_entry("Container", ref_source("Container")),
        registry_entry("First", ref_source("First")),
    ];
    let registry_meta = vec![
        meta_entry("Container", "/workspace/types.ts"),
        meta_entry("First", "/workspace/types.ts"),
    ];

    run_policy(&host, &mut meta, &registry, &registry_meta);

    let published = meta.props[0].type_source.present().expect("typed prop");
    let node = raise(&host, published).expect("published source must raise");
    let member_value = match node_data(&host, node).as_deref() {
        Some(SemanticNodeData::Object(surface)) => {
            assert_eq!(
                surface
                    .members
                    .iter()
                    .map(|m| m.name.to_string())
                    .collect::<Vec<_>>(),
                vec!["first".to_string()],
                "Container's body must surface the `first` member",
            );
            surface.members[0].value
        }
        other => panic!("published body must raise to Container's object; got {other:?}"),
    };
    // Shallow-by-default: the member value is a reference carrier to
    // `First`, NOT an eagerly-inlined object body.
    assert!(
        matches!(ref_head(&host, member_value), Some((ref name, _)) if name == "First"),
        "the nested member value must stay the shallow `First` reference carrier",
    );
}

// ---------------------------------------------------------------------------
// Rule 4 — macro-participating refs stay symbolic (§3.4 structural)
// ---------------------------------------------------------------------------

#[test]
fn rule4_keeps_macro_participating_ref_symbolic() {
    let host = empty_host();
    upsert_ts(
        &host,
        "/workspace/button.ts",
        "export type ButtonProps = { size: number };",
    );
    let mut meta = empty_meta();
    meta.props.push(prop("button", ref_source("ButtonProps")));

    let registry = vec![registry_entry("ButtonProps", ref_source("ButtonProps"))];
    let registry_meta = vec![meta_entry("ButtonProps", "/workspace/button.ts")];

    run_policy_with_macro_participation(
        &host,
        &mut meta,
        &registry,
        &registry_meta,
        &["ButtonProps"],
    );

    assert!(
        source_is_bare_ref(meta.props[0].type_source.present(), "ButtonProps"),
        "Rule 4: macro-participating reference must stay the symbolic seed; got {:?}",
        meta.props[0].type_source,
    );
}

#[test]
fn fixture_non_participating_props_suffix_name_gets_expanded() {
    // §3.4: role classification is STRUCTURAL, not nominal — a name ending
    // in "Props" that no macro consumes expands like any project-local
    // alias. The owner SFC is a REAL upserted file whose import makes
    // `XyzProps` resolvable in the owner scope, so the policy raise runs
    // against production-shaped host state — a file-level scope
    // (`local_scope: None`), exactly like a live `get_component_meta`
    // owner. No macro consumes `XyzProps`, so the participation set stays
    // empty and the "Props" suffix alone must not classify.
    let host = empty_host();
    upsert_ts(
        &host,
        "/workspace/xyz.ts",
        "export type XyzProps = { value: number };",
    );
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/owner.vue".to_string(),
            source: Arc::from(
                "<script setup lang=\"ts\">\n\
                 import type { XyzProps } from '/workspace/xyz.ts';\n\
                 const xyz: XyzProps = { value: 1 };\n\
                 </script>\n\
                 <template><div /></template>\n",
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    let mut meta = empty_meta();
    meta.props.push(prop("xyz", ref_source("XyzProps")));

    let registry = vec![registry_entry("XyzProps", ref_source("XyzProps"))];
    let registry_meta = vec![meta_entry("XyzProps", "/workspace/xyz.ts")];

    // Empty participation set — the "Props" suffix must NOT keep it
    // symbolic.
    run_policy_with_macro_participation(&host, &mut meta, &registry, &registry_meta, &[]);

    let published = meta.props[0].type_source.present().expect("typed prop");
    assert!(
        matches!(published, SemanticTypeSource::Authored(_)),
        "a non-participating *Props name must expand (nominal suffix must not classify); got {published:?}",
    );
    let node = raise(&host, published).expect("published source must raise");
    assert_eq!(
        object_member_names(&host, node),
        vec!["value".to_string()],
        "the published body must be XyzProps' object surface",
    );
    // Negative: the published source must not remain the bare seed.
    assert!(
        !source_is_bare_ref(meta.props[0].type_source.present(), "XyzProps"),
        "the bare seed must have been replaced",
    );
}

#[test]
fn fixture_macro_participating_non_props_suffix_name_stays_symbolic() {
    // §3.4: a name WITHOUT the "Props" suffix that a macro consumes stays
    // symbolic — participation, not spelling, classifies.
    let host = empty_host();
    upsert_ts(
        &host,
        "/workspace/cfg.ts",
        "export type WidgetConfig = { label: string };",
    );
    let mut meta = empty_meta();
    meta.props.push(prop("cfg", ref_source("WidgetConfig")));

    let registry = vec![registry_entry("WidgetConfig", ref_source("WidgetConfig"))];
    let registry_meta = vec![meta_entry("WidgetConfig", "/workspace/cfg.ts")];

    run_policy_with_macro_participation(
        &host,
        &mut meta,
        &registry,
        &registry_meta,
        &["WidgetConfig"],
    );

    assert!(
        source_is_bare_ref(meta.props[0].type_source.present(), "WidgetConfig"),
        "a macro-participating non-*Props name must stay symbolic; got {:?}",
        meta.props[0].type_source,
    );
}

#[test]
fn fixture_macro_participating_props_suffix_baseline_stays_symbolic() {
    let host = empty_host();
    upsert_ts(
        &host,
        "/workspace/av.ts",
        "export type AvatarProps = { url: string };",
    );
    let mut meta = empty_meta();
    meta.props.push(prop("avatar", ref_source("AvatarProps")));

    let registry = vec![registry_entry("AvatarProps", ref_source("AvatarProps"))];
    let registry_meta = vec![meta_entry("AvatarProps", "/workspace/av.ts")];

    run_policy_with_macro_participation(
        &host,
        &mut meta,
        &registry,
        &registry_meta,
        &["AvatarProps"],
    );

    assert!(
        source_is_bare_ref(meta.props[0].type_source.present(), "AvatarProps"),
        "macro-participating *Props baseline must stay symbolic; got {:?}",
        meta.props[0].type_source,
    );
}

#[test]
fn rule4_keeps_array_of_macro_participating_symbolic() {
    // The published source is the authored `ButtonProps[]` annotation body;
    // the array's ELEMENT is macro-participating. The policy leaves the
    // structural root untouched (only reference-headed roots rewrite), so
    // the symbolic composition survives.
    let host = empty_host();
    upsert_ts(
        &host,
        "/workspace/button.ts",
        "export type ButtonProps = { size: number };",
    );
    upsert_ts(
        &host,
        "/workspace/owner-annos.ts",
        "import type { ButtonProps } from \"/workspace/button.ts\";\nexport type Actions = ButtonProps[];",
    );
    let mut meta = empty_meta();
    meta.props.push(prop(
        "actions",
        decl_body_source("/workspace/owner-annos.ts", "Actions"),
    ));

    let registry = vec![registry_entry("ButtonProps", ref_source("ButtonProps"))];
    let registry_meta = vec![meta_entry("ButtonProps", "/workspace/button.ts")];

    let before = meta.props[0].type_source.clone();
    run_policy_with_macro_participation(
        &host,
        &mut meta,
        &registry,
        &registry_meta,
        &["ButtonProps"],
    );

    assert_eq!(
        meta.props[0].type_source, before,
        "an array-of-participating composition must stay the authored source",
    );
}

// ---------------------------------------------------------------------------
// Rule 2 — indexed access
// ---------------------------------------------------------------------------

#[test]
fn rule2_keeps_indexed_access_on_non_participating_symbolic() {
    // `Button['ui']` where Button has no located declaration: the
    // indexed-access root keeps its source (nothing to chase, nothing
    // rewritten).
    let host = empty_host();
    upsert_ts(
        &host,
        "/workspace/routes.ts",
        "export type Route = Button['ui'];",
    );
    let mut meta = empty_meta();
    meta.props.push(prop(
        "ui",
        decl_body_source("/workspace/routes.ts", "Route"),
    ));

    let before = meta.props[0].type_source.clone();
    run_policy(&host, &mut meta, &[], &[]);

    assert_eq!(
        meta.props[0].type_source, before,
        "IndexedAccess stays unchanged when the root is not macro-participating and has no body",
    );
}

#[test]
fn rule2_indexed_access_on_macro_participating_stays_symbolic() {
    // `ButtonProps['ui']` — Rule 2 keeps a member-path on a
    // macro-participating root symbolic per §3.4 structural
    // classification.
    let host = empty_host();
    upsert_ts(
        &host,
        "/workspace/button.ts",
        "export type ButtonProps = { ui: { base: string } };",
    );
    upsert_ts(
        &host,
        "/workspace/routes.ts",
        "import type { ButtonProps } from \"/workspace/button.ts\";\nexport type Route = ButtonProps['ui'];",
    );
    let mut meta = empty_meta();
    meta.props.push(prop(
        "ui",
        decl_body_source("/workspace/routes.ts", "Route"),
    ));

    let registry = vec![registry_entry("ButtonProps", ref_source("ButtonProps"))];
    let registry_meta = vec![meta_entry("ButtonProps", "/workspace/button.ts")];

    let before = meta.props[0].type_source.clone();
    run_policy_with_macro_participation(
        &host,
        &mut meta,
        &registry,
        &registry_meta,
        &["ButtonProps"],
    );

    assert_eq!(
        meta.props[0].type_source, before,
        "Rule 2: IndexedAccess on macro-participating root stays symbolic",
    );
}

// ---------------------------------------------------------------------------
// Rule 1 — package-backed refs stay symbolic
// ---------------------------------------------------------------------------

#[test]
fn rule1_keeps_package_backed_refs_symbolic() {
    let host = empty_host();
    let mut meta = empty_meta();
    meta.props.push(prop("vnode", ref_source("VNode")));

    // The registry carries a synthesized body for VNode (so the chase
    // WOULD fire), but the declaration's canonical source is
    // package-backed — Rule 1 wins.
    let registry = vec![registry_entry(
        "VNode",
        synthesized_object(&[(
            "type",
            LeafTypeFact::Primitive(verter_type_expr::PrimitiveName::String),
        )]),
    )];
    let registry_meta = vec![meta_entry(
        "VNode",
        "/workspace/node_modules/vue/dist/vue.d.ts",
    )];

    run_policy(&host, &mut meta, &registry, &registry_meta);

    assert!(
        source_is_bare_ref(meta.props[0].type_source.present(), "VNode"),
        "Rule 1: package-backed reference stays symbolic; got {:?}",
        meta.props[0].type_source,
    );
}

// ---------------------------------------------------------------------------
// Sidecar recompute
// ---------------------------------------------------------------------------

#[test]
fn pass_recomputes_public_instance_after_rewrite() {
    let host = empty_host();
    upsert_ts(
        &host,
        "/workspace/user.ts",
        "export type ImportedUser = { id: number };",
    );
    let mut meta = empty_meta();
    meta.props.push(prop("user", ref_source("ImportedUser")));

    let registry = vec![registry_entry("ImportedUser", ref_source("ImportedUser"))];
    let registry_meta = vec![meta_entry("ImportedUser", "/workspace/user.ts")];

    run_policy(&host, &mut meta, &registry, &registry_meta);

    // The rewrite fired (Rule 3), so the public-instance sidecar is
    // repopulated.
    assert!(
        meta.public_instance.is_some(),
        "a fired rewrite must repopulate the public-instance sidecar",
    );
}

#[test]
fn pass_does_not_touch_public_instance_when_no_rewrite() {
    let host = empty_host();
    let mut meta = empty_meta();
    meta.props.push(prop(
        "plain",
        SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Primitive(
            verter_type_expr::PrimitiveName::String,
        ))),
    ));

    run_policy(&host, &mut meta, &[], &[]);

    assert!(
        meta.public_instance.is_none(),
        "no rewrite → the sidecar stays untouched",
    );
}

// ---------------------------------------------------------------------------
// Raw-annotation restoration + slot preservation
// ---------------------------------------------------------------------------

#[test]
fn w2_4_restore_macro_participating_from_typed_annotation_replaces_eager_object() {
    // The resolved source is the eagerly-expanded object body (the
    // evaluator inlined `ButtonProps` away); the authored annotation
    // source is the symbolic `ButtonProps[]` the user wrote. The policy
    // must restore the authored source because `ButtonProps` is
    // macro-participating (§3.4 structural classification) and imported.
    let host = empty_host();
    upsert_ts(
        &host,
        "/workspace/button.ts",
        "export type ButtonProps = { label: string };",
    );
    upsert_ts(
        &host,
        "/workspace/annos.ts",
        "import type { ButtonProps } from \"/workspace/button.ts\";\nexport type ActionsAnno = ButtonProps[];",
    );

    let eager = synthesized_object(&[(
        "label",
        LeafTypeFact::Primitive(verter_type_expr::PrimitiveName::String),
    )]);
    let authored = decl_body_source("/workspace/annos.ts", "ActionsAnno");

    let mut meta = empty_meta();
    meta.props.push(PropAnalysis {
        name: "actions".to_string(),
        type_source: verter_type_expr::facts::SourcePosition::Present(eager),
        type_expansion: None,
        raw_type: None,
        // The authored annotation source — the analyzer's payload
        // position for the user's own `ButtonProps[]` text.
        raw_type_source: Some(authored.clone()),
        required: false,
        has_default: false,
        default_value: None,
        description: None,
        tags: vec![],
        declared_in_macro_type_arg: false,
    });

    let registry = vec![registry_entry("ButtonProps", ref_source("ButtonProps"))];
    let registry_meta = vec![meta_entry("ButtonProps", "/workspace/button.ts")];

    run_policy_with_macro_participation(
        &host,
        &mut meta,
        &registry,
        &registry_meta,
        &["ButtonProps"],
    );

    assert_eq!(
        meta.props[0].type_source.present(),
        Some(&authored),
        "restore_props_suffix_from_raw must publish the authored annotation source",
    );
    // Negative: the raised published node is the symbolic array over the
    // `ButtonProps` reference carrier — not an inlined object body.
    let node = raise(&host, meta.props[0].type_source.present().unwrap())
        .expect("restored source must raise");
    let element = match node_data(&host, node).as_deref() {
        Some(SemanticNodeData::Array { element, .. }) => *element,
        other => panic!("restored annotation must raise to an array; got {other:?}"),
    };
    assert!(
        matches!(ref_head(&host, element), Some((ref name, _)) if name == "ButtonProps"),
        "the array element must be the symbolic ButtonProps reference, not an inlined object",
    );
}

#[test]
fn w2_4_slot_binding_preserve_typed_indexed_access_via_imported_root() {
    // The slot binding's published source was widened by the evaluator;
    // the authored annotation is the symbolic `AppProps['avatar']`. The
    // root `AppProps` resolves to an imported file and its `avatar`
    // property value contains an imported `Avatar` reference — the
    // guard's imported-root condition holds, so the authored source is
    // restored verbatim.
    let host = empty_host();
    upsert_ts(
        &host,
        "/workspace/avatar.ts",
        "export type Avatar = { url: string };",
    );
    upsert_ts(
        &host,
        "/workspace/app.ts",
        "import type { Avatar } from \"/workspace/avatar.ts\";\nexport type AppProps = { avatar: Avatar };",
    );
    upsert_ts(
        &host,
        "/workspace/annos.ts",
        "import type { AppProps } from \"/workspace/app.ts\";\nexport type AvatarAnno = AppProps['avatar'];",
    );

    let widened = SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Primitive(
        verter_type_expr::PrimitiveName::Unknown,
    )));
    let authored = decl_body_source("/workspace/annos.ts", "AvatarAnno");

    let mut meta = empty_meta();
    meta.slots.push(SlotAnalysis {
        name: "default".to_string(),
        is_scoped: true,
        bindings: vec![SlotBindingAnalysis {
            name: "avatar".to_string(),
            type_source: verter_type_expr::facts::SourcePosition::Present(widened.clone()),
            type_expansion: None,
            raw_type: None,
            raw_type_source: Some(authored.clone()),
        }],
        is_required: false,
        return_type: None,
        return_source: None,
        return_source_scope: None,
        description: None,
        tags: vec![],
        declared_in_macro_type_arg: true,
    });

    let registry = vec![
        registry_entry("AppProps", ref_source("AppProps")),
        registry_entry("Avatar", ref_source("Avatar")),
    ];
    let registry_meta = vec![
        meta_entry("AppProps", "/workspace/app.ts"),
        meta_entry("Avatar", "/workspace/avatar.ts"),
    ];

    run_policy_with_macro_participation(&host, &mut meta, &registry, &registry_meta, &["AppProps"]);

    let binding = &meta.slots[0].bindings[0];
    assert_eq!(
        binding.type_source.present(),
        Some(&authored),
        "slot_binding_should_preserve_symbolic_raw_type must restore the authored indexed-access source",
    );
    // Negative: the binding must not stay the widened leaf.
    assert_ne!(
        binding.type_source.present(),
        Some(&widened),
        "the widened published source must have been replaced",
    );
}
