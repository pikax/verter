#![deny(missing_docs)]
//! The Svelte resolution leg — the executor-private resolver for one Svelte
//! source family.
//!
//! [`resolve_svelte_surface`] is the executor's `PlannedDemand::SvelteSurface`
//! arm (D-bh). It READS the owner's typed Svelte facts
//! ([`SvelteScriptFacts`](verter_semantic::analysis::framework_facts::svelte::SvelteScriptFacts))
//! — the SAME parse-domain inventory the synth consumed, provenance-validated —
//! and dispatches the captured `TypeExpr`(s) through the ONE shared resolver
//! (`navigate_param_to_object_surface` → `lower_type_expr_in_scope_with_context`
//! in `Navigate` + `project_shallow_surface_from_base(… published(Shallow))`),
//! then folds the result into a SINGLE-SOURCE [`MacroSurfaceDtos`] bundle. It is
//! NOT a second resolver: every type read routes through `ctx.dispatch()`.
//!
//! The legacy `<slot>` inventory ([`SvelteSurfaceSource::LegacySlotInventory`])
//! is the only source read from the typed parse CARRIER (the `<slot>` elements
//! live in template markup, not the script) — a structural walk over the typed
//! [`ParsedSvelte`](verter_compiler::svelte::ParsedSvelte) template tree, NEVER a
//! source-text scan.

use std::sync::Arc;

use verter_compiler::svelte::parser::template_ast::{
    SvelteAttributeKind, SvelteElementKind, SvelteNode,
};
use verter_semantic::analysis::framework_facts::svelte::{SvelteLegacyProp, SvelteScriptFacts};
use verter_semantic::analysis::types::{
    AnalyzedEmitField, AnalyzedMacroKind, AnalyzedPropField, AnalyzedSlotField,
    AnalyzedSlotFieldBinding, TypeResolutionSource,
};
use verter_type_expr::{PrimitiveName, TypeExpr};

use verter_protocol::typeinfo::graph::FrameworkSurfaceKind;

use crate::framework::surface_store::{FullKey, StoredSurfaceDto};
use crate::resolver_core::ResolverContext;
use crate::typeinfo::framework_surface::results::{
    EmitsSurface, MacroSurfaceDtos, ModelBinding, ModelSurface, PropsSurface, ResolvedMacroPayload,
    ResolvedOutcome,
};
use crate::typeinfo::framework_surface::vue_exec::{
    emits_from_typeinfo_surface, navigate_param_to_object_surface, props_from_typeinfo_surface,
    VueMacroSurface,
};
use crate::typeinfo::framework_surface::{SvelteSurfaceKey, SvelteSurfaceSource};
use crate::typeinfo::surface::TypeInfoSurface;
use crate::typeinfo::types::TypeInfoQueryLevel;
use crate::VerterHost;

/// The wire framework-surface kind a Svelte source family's DTO bundle is keyed
/// under in the surface store (the store column matches the wire kind the bundle
/// contributes to). The two SLOTS source families share the `Slots` column but
/// stay collision-free via the `source` remainder.
fn store_kind_for_source(source: SvelteSurfaceSource) -> FrameworkSurfaceKind {
    match source {
        SvelteSurfaceSource::RunesProps | SvelteSurfaceSource::LegacyExportLet => {
            FrameworkSurfaceKind::Props
        }
        SvelteSurfaceSource::Bindable => FrameworkSurfaceKind::Model,
        SvelteSurfaceSource::SnippetProps | SvelteSurfaceSource::LegacySlotInventory => {
            FrameworkSurfaceKind::Slots
        }
        SvelteSurfaceSource::LegacyDispatcher | SvelteSurfaceSource::CallbackPropEvents => {
            FrameworkSurfaceKind::Emits
        }
        SvelteSurfaceSource::InstanceExports => FrameworkSurfaceKind::Expose,
    }
}

/// Resolve ONE Svelte source family into a single-source [`MacroSurfaceDtos`]
/// bundle (D-bh).
///
/// The result outcome distinguishes:
/// - [`ResolvedOutcome::Resolved`] — the family is PRESENT (possibly empty); the
///   single relevant DTO slot is filled (supported-empty for a present-but-empty
///   family);
/// - [`ResolvedOutcome::Missing`] — the component has NO declaration site for
///   this family (no `$props()`, no dispatcher, no legacy slots, …).
#[must_use]
pub(crate) fn resolve_svelte_surface(
    host: &VerterHost,
    ctx: &dyn ResolverContext,
    owner: &str,
    source: SvelteSurfaceSource,
) -> ResolvedMacroPayload {
    // Load the CURRENT (overlay-aware) `IndexedReady` BEFORE touching the cache,
    // so the content-addressed key carries the live overlay `whole_hash`. An
    // unloaded owner has no surface and no cache entry.
    let Some(indexed) = ctx.ensure_indexed_ready(owner) else {
        return ResolvedOutcome::Missing;
    };
    let whole_hash = indexed.whole_hash;
    let owner_arc: Arc<str> = Arc::from(owner);

    // The framework-neutral key plus the Svelte adapter's typed remainder (one
    // source family per row, D-bc). Content-addressed via `owner_whole_hash`.
    let key = FullKey {
        kind: store_kind_for_source(source),
        query_level: TypeInfoQueryLevel::FullMetadata,
        canonical: Arc::clone(&owner_arc),
        owner_whole_hash: whole_hash,
        adapter_key: SvelteSurfaceKey { source },
    };
    let store = host.svelte_surface_store();
    let generation = ctx.project_type_store().current_project_generation();

    // Warm read against the SAME `ctx` view the surface resolves under — a
    // carrier edit (a cross-file dependency the captured `TypeExpr` reaches)
    // invalidates the entry lazily via the recorded fact signature + generation.
    if let Some(cached) = store.get_with_view(&key, ctx.store_view(), generation) {
        cached.read_set_signature.bubble_via_tls();
        return ResolvedOutcome::Resolved(Arc::clone(&cached.dto_bundle));
    }

    // Cold compute under an installed fact tracer so the CROSS-FILE facts the
    // captured-`TypeExpr` resolution reads enter the entry's `ReadSetSignature`.
    let (outcome, finalise) = crate::fact_signature_helpers::install_fact_tracer(host, || {
        compute_svelte_surface(host, ctx, owner, source)
    });

    // Only a complete `Resolved` bundle warms the cache, and only with a
    // non-overflowed observation set (the no-poison invariant). `Missing` /
    // `Partial` / `Unsupported` flow through without warming.
    if let ResolvedOutcome::Resolved(dtos) = &outcome {
        if let crate::resolver_core::FactReadSetFinalise::Ok(facts) = finalise {
            let entry = StoredSurfaceDto {
                dto_bundle: Arc::clone(dtos),
                read_set_signature: crate::fact_signature_helpers::ReadSetSignature::new(facts),
                validated_at_generation: generation,
            };
            return ResolvedOutcome::Resolved(Arc::clone(&store.insert(key, entry).dto_bundle));
        }
    }
    outcome
}

/// The cold per-source resolution (no caching) — dispatched under the fact
/// tracer by [`resolve_svelte_surface`].
fn compute_svelte_surface(
    host: &VerterHost,
    ctx: &dyn ResolverContext,
    owner: &str,
    source: SvelteSurfaceSource,
) -> ResolvedMacroPayload {
    // The typed Svelte facts (provenance-validated) for every script-derived
    // family, resolved against the executor's ONE request view `ctx` (D-bh — NOT
    // a second `current_store_view_for_query`). The legacy-slot family reads the
    // content-addressed parse carrier instead.
    let facts = host.resolve_svelte_script_facts_with_ctx(ctx, owner);

    match source {
        SvelteSurfaceSource::RunesProps => resolve_runes_props(ctx, owner, facts.as_deref()),
        SvelteSurfaceSource::LegacyExportLet => resolve_legacy_export_let(owner, facts.as_deref()),
        SvelteSurfaceSource::Bindable => resolve_bindable(ctx, owner, facts.as_deref()),
        SvelteSurfaceSource::SnippetProps => resolve_snippet_props(ctx, owner, facts.as_deref()),
        SvelteSurfaceSource::LegacySlotInventory => resolve_legacy_slot_inventory(ctx, owner),
        SvelteSurfaceSource::LegacyDispatcher => resolve_dispatcher(ctx, owner, facts.as_deref()),
        SvelteSurfaceSource::CallbackPropEvents => {
            resolve_callback_prop_events(ctx, owner, facts.as_deref())
        }
        SvelteSurfaceSource::InstanceExports => resolve_instance_exports(facts.as_deref()),
    }
}

/// Wrap a resolved [`TypeInfoSurface`] in a [`VueMacroSurface`] shell so the
/// shared per-kind normalizers (`props_from_typeinfo_surface` / `emits…` /
/// `slots…`) consume it. The shell carries the Svelte owner so member scopes
/// fall back to the owner file; `macro_index` / `macro_call_span` are synthetic
/// (the surface members carry their own per-member spans).
fn macro_surface_shell(
    surface: TypeInfoSurface,
    macro_kind: AnalyzedMacroKind,
    owner: &str,
) -> VueMacroSurface {
    VueMacroSurface {
        surface,
        macro_kind,
        owner_canonical: Arc::from(owner),
        macro_index: 0,
        macro_call_span: verter_span::Span::default(),
        level: TypeInfoQueryLevel::FullMetadata,
    }
}

/// PROPS from the runes `$props()` type: navigate the captured props type to its
/// one-level object surface through the shared engine and normalize as props.
fn resolve_runes_props(
    ctx: &dyn ResolverContext,
    owner: &str,
    facts: Option<&SvelteScriptFacts>,
) -> ResolvedMacroPayload {
    let Some(facts) = facts else {
        return ResolvedOutcome::Missing;
    };
    let Some(props_type) = facts.props_type.as_ref() else {
        return ResolvedOutcome::Missing;
    };
    // Navigate the props type to its one-level object surface ONCE, then derive
    // BOTH the published prop fields (via the shared normalizer) AND the
    // per-member declaration ORIGINs from the SAME surface — the surface's
    // per-member `origin.canonical_file` is the heritage-aware declaration file
    // (the file the member's `name`/`: T` is written in), so a member inherited
    // from an imported base reports THAT base's file, not the props_type's.
    let surface = navigate_param_to_object_surface(ctx, owner, props_type);
    let (mut fields, prop_origins) = match surface {
        Some(surface) => {
            let prop_origins = prop_origins_from_surface(owner, &surface);
            let fields = props_from_typeinfo_surface(
                ctx,
                &macro_surface_shell(surface, AnalyzedMacroKind::DefineProps, owner),
            );
            (fields, prop_origins)
        }
        // A props type that does not project to an object surface (a primitive /
        // open generic) still establishes a PRESENT props surface — supported-
        // empty, never a Missing.
        None => (Vec::new(), Vec::new()),
    };
    // Apply runtime DEFAULTS DIRECTLY (the Svelte path does NOT use Vue's
    // analyzer default-merge path): a prop with a captured default is OPTIONAL
    // on the surface (`required = !is_optional` downstream), and the default
    // VALUE rides the framework-neutral `prop_defaults` SIDECAR.
    let default_keys: std::collections::HashSet<&str> =
        facts.prop_defaults.iter().map(|d| d.key.as_str()).collect();
    for field in &mut fields {
        if default_keys.contains(field.name.as_str()) {
            field.is_optional = true;
        }
    }
    let dtos = MacroSurfaceDtos {
        props: Some(PropsSurface {
            fields,
            index_signatures: Vec::new(),
            prop_defaults: facts.prop_defaults.clone(),
            prop_origins,
        }),
        ..Default::default()
    };
    ResolvedOutcome::Resolved(Arc::new(dtos))
}

/// Derive each prop's MEMBER-DECLARATION origin from the resolved props object
/// surface (gap 2 — framework-neutral SIDECAR).
///
/// The origin is the member's DECLARATION provenance (where the prop's
/// `name`/`: T` is written), NOT its value-type provenance. The shared
/// resolver already records this per-member axis on
/// [`SurfaceMemberOrigin::canonical_file`](crate::typeinfo::surface::SurfaceMemberOrigin):
/// the heritage-aware file the member's declaration lives in. A member
/// inherited from an imported `Base` reports THAT base's file (an Import hop);
/// a member declared in a local/inline props type reports the owner file (a
/// Local hop), INCLUDING a primitive-typed member (it is still DECLARED
/// somewhere). A synthetic / multi-origin member (a union common-member, a
/// mapped-produced member) carries NO single declaration file → NO entry
/// (never guessed).
fn prop_origins_from_surface(
    owner: &str,
    surface: &TypeInfoSurface,
) -> Vec<crate::typeinfo::framework_surface::results::PropOriginEntry> {
    let mut origins = Vec::new();
    for member in surface.members.iter() {
        if !member.visibility.is_public() {
            continue;
        }
        let Some(origin) = member_declaration_origin(owner, member) else {
            continue;
        };
        origins.push(
            crate::typeinfo::framework_surface::results::PropOriginEntry {
                prop_name: member.name.as_ref().to_string(),
                origin,
            },
        );
    }
    origins
}

/// Build a [`PropOrigin`](crate::typeinfo::framework_surface::results::PropOrigin)
/// for ONE surface member from its DECLARATION provenance. Returns `None` when
/// the member has no single declaration file (a synthetic / multi-origin
/// member). The hop chain is derived purely from the per-member declaration
/// file vs the owner: same file ⇒ a `Local` hop; a different file ⇒ an `Import`
/// hop pointing at the declaring module (the member was reached cross-file —
/// an imported props interface, a heritage base in another file).
fn member_declaration_origin(
    owner: &str,
    member: &crate::typeinfo::surface::TypeInfoSurfaceMember,
) -> Option<crate::typeinfo::framework_surface::results::PropOrigin> {
    use crate::resolver_core::{ResolvedDeclarationKind, ResolvedTypeDeclaration};
    use crate::typeinfo::framework_surface::results::{OriginHop, PropOrigin};

    let canonical_file = member.origin.canonical_file.as_ref()?;
    let canonical_source = canonical_file.as_ref().to_string();
    let member_name = member.name.as_ref().to_string();
    let span = member
        .origin
        .declaration_span
        .as_ref()
        .map(|cspan| cspan.span)
        .or_else(|| member.name_span.as_ref().map(|cspan| cspan.span))
        .unwrap_or_default();

    let declaration = ResolvedTypeDeclaration {
        requested_name: member_name.clone(),
        declaration_id: None,
        resolved_name: member_name.clone(),
        canonical_source: canonical_source.clone(),
        span,
        // A surface member is not itself a named interface/alias/class
        // declaration — its declaration kind is the MEMBER declaration, which
        // the surface does not classify. Report Unknown rather than fabricate a
        // kind; the load-bearing provenance is the file + name + span + hop.
        kind: ResolvedDeclarationKind::Unknown,
        text: None,
    };

    let chain = if canonical_source == owner {
        vec![OriginHop::Local]
    } else {
        vec![OriginHop::Import {
            from: canonical_source,
            specifier: None,
            imported_name: member_name,
        }]
    };

    Some(PropOrigin { declaration, chain })
}

/// PROPS from legacy `export let` props. The legacy props carry no type
/// information at the script-fact layer, so each surfaces as an `any`-typed prop
/// (optional when it declares a default).
fn resolve_legacy_export_let(
    owner: &str,
    facts: Option<&SvelteScriptFacts>,
) -> ResolvedMacroPayload {
    let Some(facts) = facts else {
        return ResolvedOutcome::Missing;
    };
    if facts.legacy_props.is_empty() {
        return ResolvedOutcome::Missing;
    }
    let fields = facts
        .legacy_props
        .iter()
        .map(|prop| legacy_prop_field(owner, prop))
        .collect::<Vec<_>>();
    let dtos = MacroSurfaceDtos {
        props: Some(PropsSurface {
            fields,
            index_signatures: Vec::new(),
            ..Default::default()
        }),
        ..Default::default()
    };
    ResolvedOutcome::Resolved(Arc::new(dtos))
}

/// One legacy `export let` prop as an `any`-typed [`AnalyzedPropField`]. The
/// `any` value type is paired with the OWNER scope to uphold the
/// `type_expr.is_some() <=> type_expr_scope.is_some()` pairing invariant (a
/// primitive `any` carries no named refs, but the pairing must hold so the
/// component-meta consumer's pairing guard is never violated).
fn legacy_prop_field(owner: &str, prop: &SvelteLegacyProp) -> AnalyzedPropField {
    AnalyzedPropField {
        name: prop.name.clone(),
        // A prop with a default value is optional.
        is_optional: prop.has_default,
        span: verter_span::Span::default(),
        type_annotation: None,
        type_expr: Some(TypeExpr::Primitive(PrimitiveName::Any)),
        type_expr_scope: Some(verter_type_expr::TypeExprScope::new(owner)),
        description: None,
        tags: Vec::new(),
        resolution_source: TypeResolutionSource::Rust,
        resolution_error: None,
        declared_in_macro_type_arg: false,
    }
}

/// MODEL from `$bindable()` props: each bindable member's prop is its
/// `$props()`-typed member. The model name is the member name; the binding's
/// prop type is the member's value type (resolved shallow through the shared
/// engine via the runes-props surface).
fn resolve_bindable(
    ctx: &dyn ResolverContext,
    owner: &str,
    facts: Option<&SvelteScriptFacts>,
) -> ResolvedMacroPayload {
    let Some(facts) = facts else {
        return ResolvedOutcome::Missing;
    };
    if facts.bindable_members.is_empty() {
        return ResolvedOutcome::Missing;
    }
    // Resolve the props surface once and pick the bindable members from it.
    let props_fields: Vec<AnalyzedPropField> = facts
        .props_type
        .as_ref()
        .and_then(|props_type| navigate_param_to_object_surface(ctx, owner, props_type))
        .map(|surface| {
            props_from_typeinfo_surface(
                ctx,
                &macro_surface_shell(surface, AnalyzedMacroKind::DefineProps, owner),
            )
        })
        .unwrap_or_default();

    let bindings = facts
        .bindable_members
        .iter()
        .map(|name| {
            let prop = props_fields
                .iter()
                .find(|f| &f.name == name)
                .cloned()
                .unwrap_or_else(|| AnalyzedPropField {
                    name: name.clone(),
                    is_optional: false,
                    span: verter_span::Span::default(),
                    type_annotation: None,
                    type_expr: Some(TypeExpr::Primitive(PrimitiveName::Any)),
                    // Pair the `any` value with the OWNER scope to uphold the
                    // `type_expr.is_some() <=> type_expr_scope.is_some()` pairing
                    // invariant (a fallback for a `$bindable()` member with no
                    // resolved `$props` field).
                    type_expr_scope: Some(verter_type_expr::TypeExprScope::new(owner)),
                    description: None,
                    tags: Vec::new(),
                    resolution_source: TypeResolutionSource::Rust,
                    resolution_error: None,
                    declared_in_macro_type_arg: false,
                });
            ModelBinding {
                name: name.clone(),
                prop,
            }
        })
        .collect();
    let dtos = MacroSurfaceDtos {
        model: Some(ModelSurface { bindings }),
        ..Default::default()
    };
    ResolvedOutcome::Resolved(Arc::new(dtos))
}

/// SLOTS from snippet-typed `$props()` members: project the props surface,
/// retain ONLY the validated snippet members, and normalize as slots
/// (function-like Snippet members become slot fields). A userland `Snippet`
/// look-alike never appears (it is absent from `validated_snippet_members`).
fn resolve_snippet_props(
    ctx: &dyn ResolverContext,
    owner: &str,
    facts: Option<&SvelteScriptFacts>,
) -> ResolvedMacroPayload {
    let Some(facts) = facts else {
        return ResolvedOutcome::Missing;
    };
    if facts.validated_snippet_members.is_empty() {
        return ResolvedOutcome::Missing;
    }
    let Some(props_type) = facts.props_type.as_ref() else {
        return ResolvedOutcome::Missing;
    };
    let slots = navigate_param_to_object_surface(ctx, owner, props_type)
        .map(|surface| {
            // Retain only the snippet-validated members BEFORE the slot
            // normalizer (the other props are not slots).
            let filtered = retain_members(&surface, &facts.validated_snippet_members);
            // The SVELTE-SPECIFIC snippet normalizer (NOT Vue's shared
            // `slots_from_typeinfo_surface`): a Svelte `Snippet<[a, b]>`
            // contributes ALL positional parameters as ordered slot bindings,
            // whereas Vue's slot callable surfaces only its first-parameter
            // object. The two normalizers stay separate so neither regresses.
            svelte_snippet_slots_from_typeinfo_surface(
                ctx,
                &macro_surface_shell(filtered, AnalyzedMacroKind::DefineSlots, owner),
            )
        })
        .unwrap_or_default();
    let dtos = MacroSurfaceDtos {
        slots: Some(slots),
        ..Default::default()
    };
    ResolvedOutcome::Resolved(Arc::new(dtos))
}

/// The SVELTE-SPECIFIC snippet-slot normalizer (gap 3).
///
/// Unlike Vue's shared `slots_from_typeinfo_surface` (which surfaces ONLY a
/// slot callable's FIRST-parameter object), a Svelte `Snippet<[a, b]>` exposes
/// EVERY positional parameter as an ordered slot binding. For each validated
/// snippet member this:
///
/// 1. realizes the member value to a callable through the SHARED
///    callable-realization substrate (`realize_callable_member` +
///    `raise_node_to_type_expr`) — never a second resolver;
/// 2. iterates the realized `Function`'s parameters in positional ORDER,
///    SKIPPING the leading `this` parameter and EXPANDING a rest-tuple
///    parameter (`...args: [item: Item, index: number]`) into one binding per
///    tuple element (label from `TupleElement.label`, type from the element
///    type); a non-rest, non-`this` parameter contributes one positional
///    binding directly;
/// 3. combines a UNION / INTERSECTION of callable arms by index (intersecting
///    each positional binding's types), mirroring the Vue multi-arm rule.
///
/// The ordered `bindings` vector IS the positional order (no explicit position
/// field). The binding type is the typed-IR element type (typed-IR only — no
/// source slicing).
fn svelte_snippet_slots_from_typeinfo_surface(
    ctx: &dyn ResolverContext,
    macro_surface: &VueMacroSurface,
) -> Vec<AnalyzedSlotField> {
    let host = ctx.host_for_fact_tracer_install();
    macro_surface
        .surface
        .members
        .iter()
        .filter(|member| member.visibility.is_public())
        .filter_map(|member| {
            // Realize the member value to a callable through the SHARED
            // substrate (Alias / Conditional / InstantiationRef / DeclRef
            // carrier normalization), then raise to a TypeExpr.
            let dispatch = ctx.dispatch();
            let realized = crate::meta_resolve::dispatch_helpers::realize_callable_member(
                &dispatch,
                member.value,
                crate::semantic_query::ProjectionReductionContext::published(
                    crate::semantic_query::ProjectionMode::Navigate,
                ),
            )
            .unwrap_or(member.value);
            let value = dispatch.raise_node_to_type_expr(realized)?;
            let scope = crate::typeinfo::framework_surface::scope::member_value_expr_scope(
                host,
                member,
                macro_surface.owner_canonical.as_ref(),
            );
            let bindings = snippet_callable_positional_bindings(&value, &scope)?;
            Some(AnalyzedSlotField {
                name: member.name.as_ref().to_string(),
                is_required: !member.optional,
                span: verter_span::Span::default(),
                bindings,
                return_type: None,
                return_expr: None,
                return_expr_scope: None,
                description: None,
                tags: Vec::new(),
            })
        })
        .collect()
}

/// Extract the ordered positional slot bindings from a realized snippet
/// callable `value`, handling a single `Function`, or a `Union` / `Intersection`
/// of callable arms (combined by positional index — intersecting types). Each
/// `this` param is skipped and each rest-tuple param is expanded into its
/// element bindings. Returns `None` when the value is not callable.
fn snippet_callable_positional_bindings(
    value: &TypeExpr,
    scope: &verter_type_expr::TypeExprScope,
) -> Option<Vec<AnalyzedSlotFieldBinding>> {
    match value {
        // A realized snippet call signature: `(this: void, ...args: Params)`.
        TypeExpr::Function(_) => Some(snippet_function_positional_bindings(value, scope)),
        // A `Snippet<Params>` carrier the resolver kept as a Ref (the common
        // case — the structural `Snippet<Params>` interface does not reduce to a
        // bare `Function` under Navigate). The member is ALREADY structurally
        // validated as snippet-typed, so its SINGLE tuple type-argument IS the
        // `Params` tuple — expand it element-wise. Typed-IR only (no nominal
        // name match, no source slicing): we read the carrier's first type
        // argument, which the validated `Snippet<Params>` contract fixes as the
        // positional-params tuple.
        TypeExpr::Ref { type_arguments, .. } => {
            // A validated snippet carrier is ALWAYS a slot; an open-generic /
            // non-tuple `Params` simply yields no enumerable bindings (a
            // present, binding-less slot — NOT a dropped slot).
            match single_tuple_type_argument(type_arguments) {
                Some(params) => Some(positional_bindings_from_tuple(params, scope)),
                None => Some(Vec::new()),
            }
        }
        TypeExpr::Parenthesized(inner) => snippet_callable_positional_bindings(inner, scope),
        TypeExpr::Union(arms) | TypeExpr::Intersection(arms) => {
            // Every arm must be a callable / snippet carrier; combine by index.
            let mut per_arm: Vec<Vec<AnalyzedSlotFieldBinding>> = Vec::new();
            for arm in arms.iter() {
                let arm_bindings = snippet_callable_positional_bindings(arm, scope)?;
                per_arm.push(arm_bindings);
            }
            Some(combine_positional_bindings_by_index(per_arm, scope))
        }
        _ => None,
    }
}

/// The single tuple `Params` argument of a `Snippet<Params>` carrier `Ref`'s
/// type-argument list, or `None` when the carrier has no single tuple argument
/// (an open `Params` generic / a non-tuple arg ⇒ no enumerable positional
/// bindings).
fn single_tuple_type_argument(
    type_arguments: &[TypeExpr],
) -> Option<&[verter_type_expr::TupleElement]> {
    let [TypeExpr::Tuple { elements, .. }] = type_arguments else {
        return None;
    };
    Some(elements)
}

/// Expand a `Params` tuple into ordered positional bindings (label →
/// `arg{index}` fallback, element type preserved).
fn positional_bindings_from_tuple(
    elements: &[verter_type_expr::TupleElement],
    scope: &verter_type_expr::TypeExprScope,
) -> Vec<AnalyzedSlotFieldBinding> {
    elements
        .iter()
        .enumerate()
        .map(|(index, element)| {
            positional_binding(element.label.clone(), index, &element.ty, scope)
        })
        .collect()
}

/// The ordered positional bindings of ONE realized snippet `Function`: skip the
/// leading `this` param, expand a rest-tuple param into element bindings, and
/// emit each remaining positional param directly.
fn snippet_function_positional_bindings(
    value: &TypeExpr,
    scope: &verter_type_expr::TypeExprScope,
) -> Vec<AnalyzedSlotFieldBinding> {
    let TypeExpr::Function(func) = value else {
        return Vec::new();
    };
    let mut bindings = Vec::new();
    for param in &func.parameters {
        // Skip the leading `this` parameter (the vendored `Snippet` call
        // signature is `(this: void, ...args: Params)`).
        if param.name.as_deref() == Some("this") {
            continue;
        }
        if param.rest {
            // A rest-tuple param spreads the snippet's `Params` tuple — expand
            // each tuple element into one ordered positional binding. A rest
            // param whose type is NOT a tuple (an open `Params` generic /
            // `unknown[]`) carries no enumerable positional bindings.
            if let TypeExpr::Tuple { elements, .. } = &param.ty {
                bindings.extend(positional_bindings_from_tuple(elements, scope));
            }
            continue;
        }
        let index = bindings.len();
        bindings.push(positional_binding(
            param.name.clone(),
            index,
            &param.ty,
            scope,
        ));
    }
    bindings
}

/// One positional slot binding: name from the label (fallback `arg{index}`),
/// the typed element/param type, scoped to the slot member's value-node file.
fn positional_binding(
    label: Option<String>,
    index: usize,
    ty: &TypeExpr,
    scope: &verter_type_expr::TypeExprScope,
) -> AnalyzedSlotFieldBinding {
    let name = label.unwrap_or_else(|| format!("arg{index}"));
    let type_annotation = crate::resolver_core::surface_projector::render_type_expr_display(ty);
    AnalyzedSlotFieldBinding {
        name,
        type_annotation,
        binding_expr: Some(ty.clone()),
        binding_expr_scope: Some(scope.clone()),
        span: verter_span::Span::default(),
    }
}

/// Combine per-arm positional bindings by index: a binding at index `i` is the
/// INTERSECTION of every arm's `i`-th binding type (a template can rely on a
/// positional binding only if EVERY arm supplies it). Bindings present in only
/// some arms are dropped (the shortest arm caps the count).
fn combine_positional_bindings_by_index(
    per_arm: Vec<Vec<AnalyzedSlotFieldBinding>>,
    scope: &verter_type_expr::TypeExprScope,
) -> Vec<AnalyzedSlotFieldBinding> {
    let Some(min_len) = per_arm.iter().map(|a| a.len()).min() else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(min_len);
    for i in 0..min_len {
        let mut types: Vec<TypeExpr> = Vec::new();
        // Use the first arm's binding NAME for the position.
        let name = per_arm[0][i].name.clone();
        for arm in &per_arm {
            if let Some(expr) = arm[i].binding_expr.as_ref() {
                types.push(expr.clone());
            }
        }
        let combined = match types.len() {
            0 => TypeExpr::Primitive(PrimitiveName::Unknown),
            1 => types.into_iter().next().unwrap(),
            _ => TypeExpr::Intersection(Arc::from(types.into_boxed_slice())),
        };
        let type_annotation =
            crate::resolver_core::surface_projector::render_type_expr_display(&combined);
        out.push(AnalyzedSlotFieldBinding {
            name,
            type_annotation,
            binding_expr: Some(combined),
            binding_expr_scope: Some(scope.clone()),
            span: verter_span::Span::default(),
        });
    }
    out
}

/// A [`TypeInfoSurface`] keeping only the members whose name is in `keep`.
fn retain_members(surface: &TypeInfoSurface, keep: &[String]) -> TypeInfoSurface {
    let members: Vec<_> = surface
        .members
        .iter()
        .filter(|m| keep.iter().any(|k| k.as_str() == m.name.as_ref()))
        .cloned()
        .collect();
    TypeInfoSurface {
        members: Arc::from(members.into_boxed_slice()),
        call_signatures: Arc::clone(&surface.call_signatures),
        construct_signatures: Arc::clone(&surface.construct_signatures),
        index_signatures: Arc::clone(&surface.index_signatures),
        keyspace: surface.keyspace,
        has_index_signature: surface.has_index_signature,
    }
}

/// SLOTS from the legacy `<slot>` template inventory — a structural walk over
/// the typed [`ParsedSvelte`] template tree. Each `<slot>` element contributes a
/// slot named by its `name` attribute (default `"default"`); the forwarded prop
/// attributes (every plain attribute other than `name`) become its bindings.
fn resolve_legacy_slot_inventory(ctx: &dyn ResolverContext, owner: &str) -> ResolvedMacroPayload {
    // Resolve the owner's `IndexedReady` through the request view `ctx` ONCE and
    // read EVERYTHING (carrier + raw source + whole-hash) from THIS snapshot, so
    // the cache key, the observed fact, the parsed template, and the slot-name
    // slice all see the SAME owner version (no host-current version mix under
    // churn).
    let Some(indexed) = ctx.ensure_indexed_ready(owner) else {
        return ResolvedOutcome::Missing;
    };
    // Root the cached slot bundle to the owner's CONTENT so a content edit to the
    // `.svelte` misses the warm SLOTS entry.
    crate::resolver_core::resolver_context::observe_fan_out(
        crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: owner.to_string(),
            hash: indexed.whole_hash,
        },
    );
    // Read the typed carrier FROM THE SNAPSHOT — never a source-text scan, never
    // a separate host-current carrier read.
    let Some(artifact) = indexed.framework_parse.as_ref() else {
        return ResolvedOutcome::Missing;
    };
    let Some(parsed) = crate::typeinfo::adapters::svelte::svelte_parse(artifact) else {
        return ResolvedOutcome::Missing;
    };

    let mut slots: Vec<AnalyzedSlotField> = Vec::new();
    collect_slot_elements(
        &parsed.template,
        indexed.raw_source.as_ref(),
        owner,
        &mut slots,
    );

    if slots.is_empty() {
        return ResolvedOutcome::Missing;
    }
    let dtos = MacroSurfaceDtos {
        slots: Some(slots),
        ..Default::default()
    };
    ResolvedOutcome::Resolved(Arc::new(dtos))
}

/// Recursively collect `<slot>` elements from the template tree into slot
/// fields. Deduplicated by slot name (first-writer-wins). `raw_source` is the
/// owner's snapshot raw source (the slot-name slice indexes it directly).
/// `owner` is the `.svelte` canonical, used to scope the slot bindings'
/// `binding_expr`.
fn collect_slot_elements(
    nodes: &[SvelteNode],
    raw_source: &str,
    owner: &str,
    out: &mut Vec<AnalyzedSlotField>,
) {
    for node in nodes {
        match node {
            SvelteNode::Element(element) => {
                if element.kind == SvelteElementKind::Intrinsic && element.name == "slot" {
                    let name =
                        slot_name(element, raw_source).unwrap_or_else(|| "default".to_string());
                    if !out.iter().any(|s| s.name == name) {
                        let bindings = slot_bindings(element, owner);
                        out.push(AnalyzedSlotField {
                            name,
                            is_required: false,
                            span: verter_span::Span::default(),
                            bindings,
                            return_type: None,
                            return_expr: None,
                            return_expr_scope: None,
                            description: None,
                            tags: Vec::new(),
                        });
                    }
                }
                // A `<slot>` may nest fallback content; recurse into children.
                collect_slot_elements(&element.children, raw_source, owner, out);
            }
            SvelteNode::Block(block) => {
                collect_slot_block(block, raw_source, owner, out);
            }
            _ => {}
        }
    }
}

/// Recurse into a template block's child node runs (the primary body plus every
/// branch clause) collecting `<slot>` elements.
fn collect_slot_block(
    block: &verter_compiler::svelte::parser::template_ast::SvelteBlock,
    raw_source: &str,
    owner: &str,
    out: &mut Vec<AnalyzedSlotField>,
) {
    collect_slot_elements(&block.children, raw_source, owner, out);
    for clause in &block.clauses {
        collect_slot_elements(&clause.children, raw_source, owner, out);
    }
}

/// The slot name from a `<slot name="x">` element's `name` attribute, sliced
/// from the typed value span out of the owner's SNAPSHOT raw source. `None` when
/// the element has no `name` attribute (the default slot).
fn slot_name(
    element: &verter_compiler::svelte::parser::template_ast::SvelteElement,
    raw_source: &str,
) -> Option<String> {
    for attr in &element.attributes {
        if let SvelteAttributeKind::Plain { name, value, .. } = &attr.kind {
            if name == "name" {
                return value.as_ref().and_then(|v| slice_attr_value(raw_source, v));
            }
        }
    }
    None
}

/// The forwarded slot bindings: every plain attribute other than `name` becomes
/// a slot binding.
///
/// The binding VALUE type is typed `any` — a DOCUMENTED, owner-decided
/// deprecated-path carve-out scoped to legacy-`<slot>` bindings ONLY (the slot
/// NAMES are precise; only the let:-binding values are loose). Precise
/// parse-domain forwarded-expression capture (typing each binding from its
/// `attr={expr}` forwarded expression through the shared engine) is a NAMED
/// FOLLOW-UP, ledgered by the discriminating `#[ignore]` test
/// `legacy_slot_let_binding_value_precision_is_a_followup` below — it asserts the
/// binding is PRECISE (not `any`) and is RED until the follow-up lands. Every
/// other Svelte surface stays precisely typed; this carve-out applies to nothing
/// else (mirroring the F12 `$$props` legacy-magic `any` exception).
///
/// The `any` binding value is paired with the OWNER scope (`owner`) to uphold
/// the `binding_expr.is_some() <=> binding_expr_scope.is_some()` pairing
/// invariant — a primitive `any` carries no named refs, but the pairing must
/// hold so the component-meta slot-binding pairing guards are never violated.
fn slot_bindings(
    element: &verter_compiler::svelte::parser::template_ast::SvelteElement,
    owner: &str,
) -> Vec<AnalyzedSlotFieldBinding> {
    element
        .attributes
        .iter()
        .filter_map(|attr| {
            let SvelteAttributeKind::Plain { name, .. } = &attr.kind else {
                return None;
            };
            if name == "name" {
                return None;
            }
            Some(AnalyzedSlotFieldBinding {
                name: name.clone(),
                type_annotation: Some("any".to_string()),
                binding_expr: Some(TypeExpr::Primitive(PrimitiveName::Any)),
                binding_expr_scope: Some(verter_type_expr::TypeExprScope::new(owner)),
                span: verter_span::Span::default(),
            })
        })
        .collect()
}

/// Slice a typed attribute value span out of the owner's SNAPSHOT raw source
/// (the value spans are SFC-absolute over the position-preserving source — the
/// SAME owner version the cache key + observed fact see).
fn slice_attr_value(
    raw_source: &str,
    value: &verter_compiler::svelte::parser::template_ast::SvelteAttributeValue,
) -> Option<String> {
    use verter_compiler::svelte::parser::template_ast::SvelteAttributeValue;
    let span = match value {
        SvelteAttributeValue::Text(span) => *span,
        // An expression / mixed `name={…}` slot name is not a static slot name.
        SvelteAttributeValue::Expression(_) | SvelteAttributeValue::Mixed(_) => return None,
    };
    raw_source
        .get(span.start as usize..span.end as usize)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// EMITS from the legacy `createEventDispatcher<E>` event map: navigate the
/// captured event-map type to its one-level object surface and normalize as
/// emits (the event-map property keys are the events; the property value is the
/// payload). Present only when provenance-validated (`dispatcher_events` is
/// `Some` only for a `svelte`-resolved dispatcher).
fn resolve_dispatcher(
    ctx: &dyn ResolverContext,
    owner: &str,
    facts: Option<&SvelteScriptFacts>,
) -> ResolvedMacroPayload {
    let Some(event_map) = facts.and_then(|f| f.dispatcher_events.as_ref()) else {
        return ResolvedOutcome::Missing;
    };
    let fields = navigate_param_to_object_surface(ctx, owner, event_map)
        .map(|surface| {
            emits_from_typeinfo_surface(
                ctx,
                &macro_surface_shell(surface, AnalyzedMacroKind::DefineEmits, owner),
            )
        })
        .unwrap_or_default();
    let dtos = MacroSurfaceDtos {
        emits: Some(EmitsSurface {
            fields,
            index_signatures: Vec::new(),
        }),
        ..Default::default()
    };
    ResolvedOutcome::Resolved(Arc::new(dtos))
}

/// EMITS from the modern callback-prop event convention (a DERIVED,
/// NON-AUTHORITATIVE compatibility index — `$props` stays authoritative).
///
/// Svelte 5 replaced `createEventDispatcher` with callback props: a component
/// declares `onEvent` props the parent passes handlers to. There is no Events
/// generic on Svelte 5's `Component` type, so this index is derived
/// STRUCTURALLY from the `$props` object surface (resolved through the SHARED
/// engine in `Navigate`): a static prop key matching the `on${E}` convention
/// (a NON-EMPTY suffix `E` after the `on` prefix) whose value realises to a
/// FUNCTION-LIKE type contributes ONE event named `E` whose payload is the
/// callback's PARAMETERS directly (NO leading-event-name strip — that strip is
/// dispatcher-only). An arbitrary non-`on` function prop is NEVER mined. A
/// component with no `$props` type, or no `on*` function members, yields a
/// present-but-empty EMITS surface (the runes-event compatibility surface is
/// supported, even when no callback events exist).
fn resolve_callback_prop_events(
    ctx: &dyn ResolverContext,
    owner: &str,
    facts: Option<&SvelteScriptFacts>,
) -> ResolvedMacroPayload {
    let Some(props_type) = facts.and_then(|f| f.props_type.as_ref()) else {
        return ResolvedOutcome::Missing;
    };
    let fields = navigate_param_to_object_surface(ctx, owner, props_type)
        .map(|surface| callback_events_from_props_surface(ctx, owner, &surface))
        .unwrap_or_default();
    let dtos = MacroSurfaceDtos {
        emits: Some(EmitsSurface {
            fields,
            index_signatures: Vec::new(),
        }),
        ..Default::default()
    };
    ResolvedOutcome::Resolved(Arc::new(dtos))
}

/// Extract the callback-prop events from a resolved `$props` object surface: each
/// public member named `on${E}` (NON-EMPTY `E`) whose value realises to a
/// function-like type becomes an [`AnalyzedEmitField`] named `E` whose payload is
/// the callback's parameters as a labelled tuple (every parameter — NO strip).
///
/// The value is function-like in two shapes:
///
/// - a BARE callable — a required prop `onselect: (r: Row) => void`, and a
///   member-OPTIONAL prop `onselect?: (r: Row) => void` (the `?` is carried by
///   the surface member `optional` flag, so the VALUE raises to a bare
///   `Function`, not a union);
/// - a callable arm of an EXPLICIT nullish union/intersection VALUE —
///   `onselect: ((r: Row) => void) | undefined` raises to
///   `Union([Function, Undefined])`, and `callable_arm_from_raised` strips the
///   nullish arm and pulls out the single callable.
fn callback_events_from_props_surface(
    ctx: &dyn ResolverContext,
    owner: &str,
    surface: &TypeInfoSurface,
) -> Vec<AnalyzedEmitField> {
    use verter_type_expr::TupleElement;

    let host = ctx.host_for_fact_tracer_install();
    let dispatch = ctx.dispatch();
    let mut events: Vec<AnalyzedEmitField> = Vec::new();
    for member in surface.members.iter().filter(|m| m.visibility.is_public()) {
        // Structural `on${E}` callback convention: `on` prefix + a NON-EMPTY
        // suffix. The suffix is the event name (NO strip applied to the payload).
        let Some(event_name) = member.name.as_ref().strip_prefix("on") else {
            continue;
        };
        if event_name.is_empty() {
            continue;
        }
        // The value MUST realise to a FUNCTION-LIKE type (an arbitrary non-`on`
        // function prop is excluded above; an `on*` prop whose value is NOT a
        // function — `onclick: string` — is excluded here).
        let realized = crate::meta_resolve::dispatch_helpers::realize_callable_member(
            &dispatch,
            member.value,
            crate::semantic_query::ProjectionReductionContext::published(
                crate::semantic_query::ProjectionMode::Navigate,
            ),
        )
        .unwrap_or(member.value);
        // `TypeExpr` implements `Drop`, so the function cannot be moved out of
        // the raised value; bind it and borrow. The shared callable-arm
        // extractor strips the nullish (`undefined` / `null`) arms an EXPLICIT
        // nullish union VALUE (`onselect: ((r) => void) | undefined`) carries and
        // pulls out the single callable arm. (A member-`?`-optional callback raises
        // to a bare `Function` — the `?` rides the surface `optional` flag.) A
        // non-callable prop (`label?: string`) and a union with no callable arm
        // both yield `None` (NOT an event).
        let raised = dispatch.raise_node_to_type_expr(realized);
        let Some(func) = raised
            .as_ref()
            .and_then(crate::meta_resolve::dispatch_helpers::callable_arm_from_raised)
        else {
            continue;
        };
        // Payload = the callback's PARAMETERS as a labelled tuple (all of them —
        // a callback prop's parameters ARE the event payload; there is no leading
        // event-name parameter to strip).
        let payload_tuple = TypeExpr::Tuple {
            elements: func
                .parameters
                .iter()
                .map(|param| TupleElement {
                    label: param.name.clone(),
                    ty: param.ty.clone(),
                    optional: param.optional,
                    rest: param.rest,
                })
                .collect(),
            readonly: false,
        };
        let payload_type =
            crate::resolver_core::surface_projector::render_type_expr_display(&payload_tuple);
        // Scope the payload to the `$props` member's VALUE-NODE file (the SAME
        // scope the shared resolver navigated the member in), so a payload `Ref`
        // (a callback parameter typed against a same-module `interface Row`)
        // resolves precisely. `payload_expr` paired with `Some(scope)` upholds
        // the `AnalyzedEmitField` pairing invariant (a `Some`-expr / `None`-scope
        // mismatch would degrade the named ref to opaque on the component-meta
        // surface). Routes through the SHARED member-scope owner
        // (`member_value_expr_scope`).
        let payload_expr_scope = Some(
            crate::typeinfo::framework_surface::scope::member_value_expr_scope(host, member, owner),
        );
        events.push(AnalyzedEmitField {
            name: event_name.to_string(),
            span: verter_span::Span::default(),
            payload_type,
            payload_expr: Some(payload_tuple),
            payload_expr_scope,
            description: None,
            tags: Vec::new(),
        });
    }
    // De-duplicate by event name, first-writer-wins.
    let mut seen = std::collections::HashSet::new();
    events.retain(|e| seen.insert(e.name.clone()));
    events
}

/// EXPOSE from the exported instance-script members. Each export is a named
/// member of the public instance; the member type stays a shallow `Ref` to the
/// exported binding (shallow-by-default — the consumer re-resolves on demand).
fn resolve_instance_exports(facts: Option<&SvelteScriptFacts>) -> ResolvedMacroPayload {
    use crate::typeinfo::framework_surface::results::{ExposeSurface, NamedTypeMember};
    let Some(facts) = facts else {
        return ResolvedOutcome::Missing;
    };
    if facts.instance_exports.is_empty() {
        return ResolvedOutcome::Missing;
    }
    let members = facts
        .instance_exports
        .iter()
        .map(|name| NamedTypeMember {
            name: name.clone(),
            is_optional: false,
            type_expr: Some(TypeExpr::Ref {
                name: Arc::from(name.as_str()),
                type_arguments: Arc::from(Vec::new().into_boxed_slice()),
            }),
        })
        .collect();
    let dtos = MacroSurfaceDtos {
        expose: Some(ExposeSurface { members }),
        ..Default::default()
    };
    ResolvedOutcome::Resolved(Arc::new(dtos))
}

#[cfg(test)]
#[path = "svelte_exec_tests.rs"]
mod tests;
