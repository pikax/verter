//! Imported macro-type element projection for the compile-facing legacy
//! `ResolvedElements` rail.
//!
//! The compile pipeline resolves an IMPORTED macro type argument
//! (`import type { Emits } from './types'; defineEmits<Emits>()`,
//! `import type { Props } from './types'; defineProps<Props>()`) into the
//! parser-consumed [`ResolvedElements`] map. The legacy frontier element
//! expander is severed (query-time member/type authority is owned by the ONE
//! shared dispatch), so this module is the sanctioned replacement for the
//! EMITS and PROPS positions: resolve the macro's type argument ONCE through
//! the shared macro-surface authority
//! ([`VerterHost::resolve_vue_macro_surface_with_ctx`] — heritage and
//! cross-file routes compose there), then run a THIN normalize into the
//! legacy DTO shape. It is a projection, NOT a resolver: every semantic
//! decision is made in the NODE domain (`CallableNodeView` /
//! `node_data_for`), and display text is minted ONCE at the terminal render
//! step.

use std::sync::Arc;

use verter_parser::utils::oxc::script::type_surface::{
    ResolvedCallPayloadForm, ResolvedElements, ResolvedMemberVisibility,
    ResolvedNamedCallSignature, ResolvedProp, RuntimeType,
};
use verter_semantic::analysis::AnalyzedMacroKind;

use super::{raise_member_value, TypeinfoVueSurfaceOutputCap, UnresolvedSurfaceArm};
use crate::meta_resolve::callable_view::CallableNodeView;
use crate::project_semantic_dispatch::node_data_for;
use crate::project_semantic_dispatch::output_materialization::OutputProjector;
use crate::resolver_core::surface_projector::{render_type_expr_display, ResolvedMacroElements};
use crate::resolver_core::ResolvedNativeProp;
use crate::semantic_query::{
    FunctionParam, ProjectionMode, ProjectionReductionContext, SemanticNodeData,
};
use crate::typeinfo::types::{TypeInfoQueryLevel, VueMacroSurfaceRequest};

/// Resolve an imported `defineEmits<T>()` type argument to the legacy
/// [`ResolvedElements`] the compile-facing parser consumes.
///
/// Gated to the BY-NAME representable shape: the macro's type argument must be
/// the bare named reference `type_name` (the external-elements map is keyed by
/// name, so a composite type argument — a union / intersection / instantiation
/// — cannot be represented by one name's elements and fails closed to the
/// legacy unresolved outcome). The gate reads the macro type-arg NODE through
/// its sole sanctioned producer (`macro_type_arg_hot_ref`) — a node-domain
/// carrier match, never a type-text sniff.
///
/// The macro surface then resolves through the ONE shared macro-surface
/// authority (heritage composes there) and normalizes:
///
/// - each surface CALL SIGNATURE whose first param is a string-literal event
///   name (or a union of them) contributes one `Call { params_text }` row per
///   event name, with the leading event-name parameter STRIPPED;
/// - each PUBLIC property member contributes a [`ResolvedProp`] row (the
///   faithful object projection), and additionally an emit-signature row:
///   a function-like value contributes `Call { params_text }` (ALL params —
///   a property callable has no leading event-name parameter), an inline
///   tuple value contributes `Tuple { tuple_text }`; any other value shape is
///   NOT an emit signature (the compile-side emits-shape diagnostic reads the
///   emptiness of `call_signatures`);
/// - an unrenderable position fails ITS row closed (skipped) — never a
///   fabricated display.
///
/// `None` when the type argument is not the bare `type_name` reference or the
/// macro surface does not resolve — the caller keeps the legacy unresolved
/// outcome.
///
/// `unresolved_arms` receives the resolved surface's unresolvable
/// SURFACE-COMPOSITION arm facts (see
/// [`VueMacroSurface::unresolved_surface_arms`]) so the compile-facing
/// collector can tier import-backed misses.
pub(crate) fn imported_emits_resolved_elements(
    ctx: &dyn crate::resolver_core::ResolverContext,
    owner_canonical: &str,
    macro_index: usize,
    type_name: &str,
    unresolved_arms: &mut Vec<UnresolvedSurfaceArm>,
) -> Option<ResolvedElements> {
    let dispatch = ctx.dispatch();

    // By-name representability gate (node-domain): the macro type arg must be
    // the bare `type_name` reference, with NO type arguments.
    bare_named_ref_type_arg(ctx, owner_canonical, macro_index, type_name)?;

    // Resolve the macro surface through the shared macro-surface authority.
    let indexed = ctx
        .ensure_indexed_ready_serve(owner_canonical)
        .map(|serve| serve.indexed)?;
    let host = ctx.host_for_fact_tracer_install();
    let macro_surface = host.resolve_vue_macro_surface_with_ctx(
        ctx,
        &VueMacroSurfaceRequest {
            owner_canonical: Arc::from(owner_canonical),
            macro_index,
            macro_kind: AnalyzedMacroKind::DefineEmits,
            root_identity: indexed.whole_hash,
            level: TypeInfoQueryLevel::FullMetadata,
        },
    )?;
    unresolved_arms.extend(macro_surface.unresolved_surface_arms.iter().cloned());
    let surface = &macro_surface.surface;

    // Node-domain demand identity — `Navigate` carrier-resolves aliased
    // event-name unions, mirroring the emit normalizer.
    let context = ProjectionReductionContext::published(ProjectionMode::Navigate);

    let mut call_signatures: Vec<ResolvedNamedCallSignature> = Vec::new();
    let mut props: Vec<ResolvedProp> = Vec::new();

    // (1) Call-signature emits — the event name(s) and params are decided in
    // the NODE domain through the shared `CallableNodeView`; the leading
    // event-name param is stripped from the rendered params text.
    for sig in surface.call_signatures.iter() {
        let view = CallableNodeView::new(&dispatch, sig.node);
        let Some(names) = view.event_names(context) else {
            continue;
        };
        let Some(signature) = view.signature(context) else {
            continue;
        };
        let raw_params = signature.raw_params();
        let Some(params_text) = render_params_text(ctx, &raw_params[1..]) else {
            continue;
        };
        for name in names {
            call_signatures.push(named_signature_row(
                name.as_ref(),
                ResolvedCallPayloadForm::Call {
                    params_text: params_text.clone(),
                },
            ));
        }
    }

    // (2) Property members — every PUBLIC member projects a props row; the
    // emit-signature classification (function-like vs inline tuple) is a
    // NODE-domain decision, and the display text is minted once.
    for member in surface
        .members
        .iter()
        .filter(|member| member.visibility.is_public())
    {
        let raised = raise_member_value(ctx, member);
        let type_text = raised.as_ref().and_then(render_type_expr_display);
        props.push(ResolvedProp {
            span: verter_span::Span::default(),
            key: verter_span::Span::default(),
            key_name: Some(member.name.as_ref().to_string()),
            optional: member.optional,
            types: Vec::new(),
            visibility: ResolvedMemberVisibility::Public,
            type_span: None,
            type_text: type_text.clone(),
            map_local: false,
            span_is_absolute: false,
            declared_in_macro_type_arg: member.declared_in_macro_type_arg,
        });

        // Function-like member value → a named `Call` signature over ALL of
        // the callable's params (no event-name strip for property callables).
        let view = CallableNodeView::new(&dispatch, member.value);
        if let Some(signature) = view.signature(context) {
            let raw_params = signature.raw_params();
            if let Some(params_text) = render_params_text(ctx, &raw_params) {
                call_signatures.push(named_signature_row(
                    member.name.as_ref(),
                    ResolvedCallPayloadForm::Call { params_text },
                ));
            }
            continue;
        }
        // Inline tuple member value → the named-tuple shorthand emit. The
        // tuple-ness decision is NODE-domain; the display is the member's
        // already-rendered value text (`[id: number]`).
        if matches!(
            node_data_for(dispatch.ctx, member.value).as_deref(),
            Some(SemanticNodeData::Tuple { .. })
        ) {
            if let Some(tuple_text) = type_text {
                call_signatures.push(named_signature_row(
                    member.name.as_ref(),
                    ResolvedCallPayloadForm::Tuple { tuple_text },
                ));
            }
        }
    }

    Some(ResolvedElements {
        props,
        has_call_signature: !surface.call_signatures.is_empty(),
        call_signatures,
        // The name resolved and projected an object-like one-level surface
        // through the shared engine.
        root_runtime_types: vec![RuntimeType::Object],
    })
}

/// By-name representability gate shared by the imported-macro element
/// projections: the macro's type argument must be the bare `type_name`
/// reference with NO type arguments (the external-elements map is keyed by
/// name). Node-domain carrier match through the sole sanctioned producer
/// (`macro_type_arg_hot_ref`) — never a type-text sniff. Returns the type-arg
/// handle on a match, `None` otherwise.
fn bare_named_ref_type_arg(
    ctx: &dyn crate::resolver_core::ResolverContext,
    owner_canonical: &str,
    macro_index: usize,
    type_name: &str,
) -> Option<crate::semantic_query::HotTypeRef> {
    let dispatch = ctx.dispatch();
    let type_arg = crate::structural_carrier_producer::macro_type_arg_hot_ref(
        ctx,
        owner_canonical,
        macro_index,
    )?;
    let is_bare_named_ref = node_data_for(dispatch.ctx, type_arg.node())
        .as_deref()
        .is_some_and(|data| {
            if let Some((name, _scope)) = data.bare_ref_head() {
                return name.as_ref() == type_name && data.carrier_type_args().is_empty();
            }
            if let SemanticNodeData::DeclRef { identity } = data {
                return identity.decl_name.as_ref() == type_name;
            }
            false
        });
    is_bare_named_ref.then_some(type_arg)
}

/// Resolve an imported `defineProps<T>()` type argument to the legacy
/// [`ResolvedElements`] the compile-facing parser consumes.
///
/// Same two-step rail as [`imported_emits_resolved_elements`]: gate on the
/// bare-named-ref representable shape, resolve the macro surface ONCE through
/// the shared macro-surface authority
/// ([`VerterHost::resolve_vue_macro_surface_with_ctx`] — heritage and
/// cross-file routes compose there), then run a THIN normalize into the legacy
/// DTO shape:
///
/// - every surface member (public AND non-public — class-backed props keep
///   their visibility; the one keep-all projection core runs elements-only
///   here, [`NativeProjection::Skip`]) contributes a [`ResolvedProp`] row
///   with its rendered display text and the node-domain runtime-constructor
///   classification;
/// - the object surface stamps `root_runtime_types: [Object]` (the compile
///   object-like check accepts an empty-but-object-like props type);
/// - a type argument that RESOLVES but does not project an object surface
///   (e.g. `export type Props = string`) returns the RESOLVED-non-object
///   elements — empty props, non-`Object` root constructors — so the compile
///   diagnostics path reports the object-like violation instead of a false
///   "could not be resolved";
/// - an UNRESOLVED type argument (missing file / missing symbol / undecidable
///   carrier) returns `None` — the caller keeps the legacy unresolved outcome.
///
/// `unresolved_arms` receives the resolved surface's unresolvable
/// SURFACE-COMPOSITION arm facts (see
/// [`VueMacroSurface::unresolved_surface_arms`]) so the compile-facing
/// collector can tier import-backed misses.
pub(crate) fn imported_props_resolved_elements(
    ctx: &dyn crate::resolver_core::ResolverContext,
    owner_canonical: &str,
    macro_index: usize,
    type_name: &str,
    unresolved_arms: &mut Vec<UnresolvedSurfaceArm>,
) -> Option<ResolvedElements> {
    let dispatch = ctx.dispatch();

    let type_arg = bare_named_ref_type_arg(ctx, owner_canonical, macro_index, type_name)?;

    // Resolve the macro surface through the shared macro-surface authority.
    let indexed = ctx
        .ensure_indexed_ready_serve(owner_canonical)
        .map(|serve| serve.indexed)?;
    let host = ctx.host_for_fact_tracer_install();
    let macro_surface = host.resolve_vue_macro_surface_with_ctx(
        ctx,
        &VueMacroSurfaceRequest {
            owner_canonical: Arc::from(owner_canonical),
            macro_index,
            macro_kind: AnalyzedMacroKind::DefineProps,
            root_identity: indexed.whole_hash,
            level: TypeInfoQueryLevel::FullMetadata,
        },
    );
    if let Some(macro_surface) = &macro_surface {
        unresolved_arms.extend(macro_surface.unresolved_surface_arms.iter().cloned());
        let surface = &macro_surface.surface;
        // A MEMBERLESS projected surface is ambiguous: the macro-object
        // synthesis can produce an empty object for a NON-object alias
        // (`export type Props = string` contributes no object-arm members).
        // Fall through to the node-domain root classification below to
        // distinguish a genuinely-empty object type from a resolved
        // non-object — trusting `Object` here would silence the compile
        // object-like diagnostic.
        if !surface.members.is_empty() || !surface.call_signatures.is_empty() {
            return Some(props_elements_from_surface(ctx, &dispatch, surface));
        }
    }

    // No members surfaced. Distinguish RESOLVED (object-like or not — the
    // compile object-like diagnostic needs the difference) from UNRESOLVED
    // (the legacy unresolved outcome) by resolving the type-arg carrier ONE
    // Navigate hop through the shared dispatch and classifying the
    // node-domain result. An unresolved carrier (the head stays a bare ref /
    // opaque) and every undecidable shape fail closed to `None` — never a
    // fabricated resolution.
    let resolved = dispatch.resolve_hot_handle_with_context(
        crate::semantic_query::HotTypeRef::new(type_arg.node()),
        ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
    );
    classified_root_elements(&dispatch, resolved)
}

/// Resolve ONE named imported PROPS-position type (`dep.type_name` imported
/// from `dep.import_source` in `owner_canonical`) to the legacy
/// [`ResolvedElements`] the compile-facing parser folds per NAME.
///
/// The COMPOSITE-type-argument companion to
/// [`imported_props_resolved_elements`]: `defineProps<Left & Right>()`
/// references TWO named imported types; the macro-argument route cannot
/// represent the composite under one name, but the parser's companion fold
/// resolves EACH referenced name independently against the external map — so
/// each per-name dep resolves here: route the import to its declaring file
/// (the shared import-route authority — barrels and re-exports compose),
/// `ResolveDecl` the declaration carrier through the ONE shared dispatch,
/// synthesise its one-level `Shallow` surface, and run the SAME thin
/// props-row normalize. Resolution, routing, and surface synthesis all run
/// through the shared engine; this is a projection, not a resolver.
///
/// The same RESOLVED-non-object vs UNRESOLVED distinction as the
/// macro-argument route applies to the memberless outcome.
///
/// `unresolved_arms` receives the resolved surface's unresolvable
/// SURFACE-COMPOSITION arm facts, exactly as on the macro-argument route.
pub(crate) fn imported_named_props_resolved_elements(
    ctx: &dyn crate::resolver_core::ResolverContext,
    owner_canonical: &str,
    import_source: &str,
    type_name: &str,
    unresolved_arms: &mut Vec<UnresolvedSurfaceArm>,
) -> Option<ResolvedElements> {
    let host = ctx.host_for_fact_tracer_install();

    let dep_canonical = host.resolve_loaded_dependency_canonical(
        owner_canonical,
        import_source,
        verter_workspace::ResolveRequestKind::TypeImport,
    )?;
    let (root_canonical, root_name) =
        ctx.resolve_imported_type_root(dep_canonical.as_str(), type_name);

    named_type_resolved_elements(
        ctx,
        root_canonical.as_str(),
        root_name.as_str(),
        unresolved_arms,
    )
}

/// Whether the combined projection also builds the keep-all `native_props`
/// rows. The compile-facing `.elements` routes pass [`Skip`] — they consume
/// only the legacy elements DTO, so building native rows there would be pure
/// per-member allocation waste (a discarded second vector plus name /
/// display-text clones); the component-meta macro-elements rail passes
/// [`Include`]. The elements value is byte-identical under both.
///
/// [`Skip`]: NativeProjection::Skip
/// [`Include`]: NativeProjection::Include
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeProjection {
    /// Elements-only caller: leave `native_props` empty (no allocation).
    Skip,
    /// Build the keep-all `native_props` rows alongside the elements.
    Include,
}

/// QueryResult-style outcome of the `(canonical, name)` elements projection —
/// the dispatch-owned terminal the component-meta macro-elements rail
/// consumes. Distinguishes a resolved VALUE from a transient RECURSIVE
/// back-edge (which the caller must NOT negative-cache) and from a GENUINE
/// unresolved route/declaration miss.
pub(crate) enum NamedTypeElementsOutcome {
    /// The root resolved through the shared dispatch and projected (or
    /// root-classified) into the macro-elements payload: the legacy elements
    /// value PLUS — under [`NativeProjection::Include`] — the keep-all
    /// `native_props` rows built directly from the same surface resolution
    /// (empty under [`NativeProjection::Skip`]).
    Resolved(ResolvedMacroElements),
    /// The declaration is on an active resolution chain (a dispatch
    /// recursion back-edge) and produced no projectable surface — an honest
    /// TRANSIENT non-result; never a cacheable negative.
    Recursive,
    /// Genuine unresolved route/declaration (or an undecidable root).
    Miss,
}

/// The `(canonical, name)` → macro-elements projection core shared by the
/// per-name imported-props route and the component-meta macro-elements rail
/// (whose route target is already resolved when it reaches this projection):
/// `ResolveDecl` the declaration carrier through the ONE shared dispatch,
/// request its EMPTY-PATH one-level `Shallow` surface, and run the thin
/// combined normalize ([`macro_elements_from_surface`] — the legacy
/// props-row projection plus, under [`NativeProjection::Include`], the
/// keep-all `native_props` rows, one pass over the same surface; member
/// values stay semantic carriers — no recursive member-value
/// materialization). The compile-facing elements-only adapter passes
/// [`NativeProjection::Skip`]. The RESOLVED-non-object vs UNRESOLVED
/// distinction applies to the memberless outcome.
///
/// `unresolved_arms` receives the surface projection's unresolvable
/// SURFACE-COMPOSITION arm facts (name-sorted, deduplicated); callers that
/// don't consume them pass a throwaway vec.
pub(crate) fn named_type_elements_outcome(
    ctx: &dyn crate::resolver_core::ResolverContext,
    root_canonical: &str,
    root_name: &str,
    native: NativeProjection,
    unresolved_arms: &mut Vec<UnresolvedSurfaceArm>,
) -> NamedTypeElementsOutcome {
    let dispatch = ctx.dispatch();
    let host = ctx.host_for_fact_tracer_install();

    // The declaration CARRIER (a `DeclPlaceholder`), NOT a pre-instantiated
    // body: the empty-path Shallow synthesiser's decl-root unwrap
    // re-establishes the declaration's KIND and classifies heritage arms.
    let read = dispatch.execute_read(crate::semantic_query::SemanticQueryKey::ResolveDecl(
        crate::semantic_query::ResolveDeclKey {
            scope: crate::semantic_query::ScopeId {
                canonical_id: Arc::from(root_canonical),
                local_scope: None,
            },
            name: Arc::from(root_name),
        },
    ));
    crate::meta_resolve::emit_dispatch_dep_signature_facts(dispatch.ctx, &read.dep_signature);
    let (base, recursive) = match read.value {
        crate::semantic_query::QueryResult::Value(id) => (id, false),
        crate::semantic_query::QueryResult::Recursive(id) => (id, true),
        crate::semantic_query::QueryResult::Error(_) => return NamedTypeElementsOutcome::Miss,
    };

    // One-level surface through the SAME shared synthesiser the macro-surface
    // authority uses. The `MacroTypeArgOwnBody` provenance stamps the
    // declaration's own-body members `declared_in_macro_type_arg = true` and
    // heritage-reached members `false` — the parser's per-name companion fold
    // consumes those per-prop facts as-is. The walker's side-band diagnostics
    // carry any unresolvable SURFACE-COMPOSITION arm the synthesis dropped.
    let mut walker_diagnostics = Vec::new();
    if let Some(surface) = host.project_shallow_surface_from_base(
        ctx,
        &dispatch,
        base,
        Arc::from(Vec::<crate::semantic_query::PathSegment>::new().into_boxed_slice()),
        ProjectionReductionContext::macro_object_surface(
            ProjectionMode::Shallow,
            crate::semantic_query::SurfaceProvenanceContext::MacroTypeArgOwnBody,
        ),
        Some(&mut walker_diagnostics),
    ) {
        unresolved_arms.extend(super::unresolved_surface_arms_from_diags(
            &walker_diagnostics,
        ));
        if !surface.members.is_empty() || !surface.call_signatures.is_empty() {
            return NamedTypeElementsOutcome::Resolved(macro_elements_from_surface(
                ctx, &dispatch, &surface, native,
            ));
        }
    }

    match classified_root_elements(&dispatch, base) {
        Some(elements) => NamedTypeElementsOutcome::Resolved(ResolvedMacroElements {
            elements,
            // A memberless root-classified outcome projects no member
            // surface — no native rows.
            native_props: Vec::new(),
        }),
        None if recursive => NamedTypeElementsOutcome::Recursive,
        None => NamedTypeElementsOutcome::Miss,
    }
}

/// Compile-facing `Option` adapter over [`named_type_elements_outcome`] for
/// the per-name imported-props route (whose legacy contract folds both the
/// transient recursive back-edge and the genuine miss into the unresolved
/// outcome, and consumes only the legacy elements value — so the native
/// rows are never built here, [`NativeProjection::Skip`]).
fn named_type_resolved_elements(
    ctx: &dyn crate::resolver_core::ResolverContext,
    root_canonical: &str,
    root_name: &str,
    unresolved_arms: &mut Vec<UnresolvedSurfaceArm>,
) -> Option<ResolvedElements> {
    match named_type_elements_outcome(
        ctx,
        root_canonical,
        root_name,
        NativeProjection::Skip,
        unresolved_arms,
    ) {
        NamedTypeElementsOutcome::Resolved(resolution) => Some(resolution.elements),
        NamedTypeElementsOutcome::Recursive | NamedTypeElementsOutcome::Miss => None,
    }
}

/// Thin props-row adapter over [`macro_elements_from_surface`] for the
/// compile-facing per-name route's terminal, which consumes only the legacy
/// elements value — so the native rows are never built here
/// ([`NativeProjection::Skip`]).
fn props_elements_from_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    dispatch: &crate::project_semantic_dispatch::ProjectSemanticDispatch<'_>,
    surface: &crate::typeinfo::surface::TypeInfoSurface,
) -> ResolvedElements {
    macro_elements_from_surface(ctx, dispatch, surface, NativeProjection::Skip).elements
}

/// Combined keep-all projection of a one-level surface into the
/// macro-elements payload ([`ResolvedMacroElements`]) — the SINGLE
/// projection core for both consumers: the legacy props rows AND (under
/// [`NativeProjection::Include`]) the `native_props` rows, built in ONE
/// pass over the members so each member's value is raised + rendered
/// exactly once and BOTH projections are guaranteed to read the SAME
/// surface resolution. Every member (public AND non-public — class-backed
/// members keep their visibility) contributes a props row with its rendered
/// display text and node-domain runtime-constructor classification; the
/// object surface stamps `root_runtime_types: [Object]`. The native rows
/// are built DIRECTLY from the surface members via the
/// [`ResolvedNativeProp::from_surface_member`] constructor (visibility
/// carried verbatim, no `.is_public()` filter) — no `ResolvedElements`
/// round-trip; an elements-only caller passes [`NativeProjection::Skip`]
/// and no native row (or display-text clone) is built at all. Each rendered
/// display text feeds ONLY the published rows (a publication, never a
/// decision); the runtime-constructor classification decides on the
/// member's NODE (`runtime_types_for_node`), never the minted value.
fn macro_elements_from_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    dispatch: &crate::project_semantic_dispatch::ProjectSemanticDispatch<'_>,
    surface: &crate::typeinfo::surface::TypeInfoSurface,
    native: NativeProjection,
) -> ResolvedMacroElements {
    let mut props: Vec<ResolvedProp> = Vec::with_capacity(surface.members.len());
    let mut native_props: Vec<ResolvedNativeProp> = match native {
        NativeProjection::Skip => Vec::new(),
        NativeProjection::Include => Vec::with_capacity(surface.members.len()),
    };
    for member in surface.members.iter() {
        let raised = raise_member_value(ctx, member);
        let type_text = raised.as_ref().and_then(render_type_expr_display);
        if native == NativeProjection::Include {
            native_props.push(ResolvedNativeProp::from_surface_member(
                member,
                type_text.clone(),
            ));
        }
        props.push(ResolvedProp {
            span: verter_span::Span::default(),
            key: verter_span::Span::default(),
            key_name: Some(member.name.as_ref().to_string()),
            optional: member.optional,
            types: runtime_types_for_node(dispatch, member.value, false),
            visibility: resolved_member_visibility(member.visibility),
            type_span: None,
            type_text,
            map_local: false,
            span_is_absolute: false,
            declared_in_macro_type_arg: member.declared_in_macro_type_arg,
        });
    }
    ResolvedMacroElements {
        elements: ResolvedElements {
            props,
            has_call_signature: !surface.call_signatures.is_empty(),
            call_signatures: Vec::new(),
            // The name resolved and projected an object-like one-level
            // surface through the shared engine.
            root_runtime_types: vec![RuntimeType::Object],
        },
        native_props,
    }
}

/// The RESOLVED-non-object vs UNRESOLVED distinguisher for a memberless
/// outcome: classify the node's runtime-constructor kinds; a classified
/// result returns the root-classified elements (the compile object-like
/// diagnostic reads the non-`Object` root), an unclassifiable node returns
/// `None` (the legacy unresolved outcome).
fn classified_root_elements(
    dispatch: &crate::project_semantic_dispatch::ProjectSemanticDispatch<'_>,
    node: crate::semantic_query::SemanticNodeId,
) -> Option<ResolvedElements> {
    let root_runtime_types = runtime_types_for_node(dispatch, node, true);
    if root_runtime_types.is_empty() {
        // Undecidable or unresolved — keep the legacy unresolved outcome.
        return None;
    }
    Some(ResolvedElements {
        props: Vec::new(),
        has_call_signature: false,
        call_signatures: Vec::new(),
        root_runtime_types,
    })
}

/// Map a node-domain member visibility onto the legacy
/// [`ResolvedMemberVisibility`] row stamp.
fn resolved_member_visibility(
    visibility: verter_type_expr::MemberVisibility,
) -> ResolvedMemberVisibility {
    match visibility {
        verter_type_expr::MemberVisibility::Public => ResolvedMemberVisibility::Public,
        verter_type_expr::MemberVisibility::Protected => ResolvedMemberVisibility::Protected,
        verter_type_expr::MemberVisibility::Private => ResolvedMemberVisibility::Private,
    }
}

/// Classify a semantic node's RUNTIME constructor kinds ([`RuntimeType`]) in
/// the NODE domain — the imported-props analogue of the parser's syntax-side
/// runtime-constructor classification (unknowns are dropped, mirroring the
/// parser's `runtime_types` filter). `Alias` hops and `Union` arms flatten
/// (bounded).
///
/// TWO demand strengths, chosen by position (Component-Meta
/// Shallow-By-Default): the ROOT distinguisher (`executing = true`) may
/// EXECUTE `DeclRef` / `DeclPlaceholder` carriers through the shared dispatch
/// — the route target's declaration IS the demand; a MEMBER-VALUE
/// classification (`executing = false`) is SHALLOW-ONLY — member values are
/// reference carriers that must NOT demand their declaring file, so carriers
/// classify as no kinds.
fn runtime_types_for_node(
    dispatch: &crate::project_semantic_dispatch::ProjectSemanticDispatch<'_>,
    node: crate::semantic_query::SemanticNodeId,
    executing: bool,
) -> Vec<RuntimeType> {
    let mut kinds = Vec::new();
    collect_runtime_types_for_node(dispatch, node, 0, executing, &mut kinds);
    kinds
}

fn collect_runtime_types_for_node(
    dispatch: &crate::project_semantic_dispatch::ProjectSemanticDispatch<'_>,
    node: crate::semantic_query::SemanticNodeId,
    depth: usize,
    executing: bool,
    kinds: &mut Vec<RuntimeType>,
) {
    // Alias / union flattening is bounded: a pathological alias cycle stops
    // classifying instead of recursing.
    if depth > 8 {
        return;
    }
    let Some(data) = node_data_for(dispatch.ctx, node) else {
        return;
    };
    fn push(kinds: &mut Vec<RuntimeType>, kind: RuntimeType) {
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }
    match data.as_ref() {
        SemanticNodeData::Alias(inner) => {
            collect_runtime_types_for_node(dispatch, *inner, depth + 1, executing, kinds);
        }
        SemanticNodeData::Union(arms) => {
            for arm in arms.iter() {
                collect_runtime_types_for_node(dispatch, *arm, depth + 1, executing, kinds);
            }
        }
        // DeclRef — ROOT positions resolve the declaration through the shared
        // dispatch, then classify the resolved node (the canonical
        // ResolveDecl dispatch pattern, mirroring
        // `realize_callable_member_inner`). MEMBER-VALUE positions keep the
        // carrier shallow (no kinds) — Shallow-By-Default.
        SemanticNodeData::DeclRef { identity } => {
            if !executing {
                return;
            }
            let read = dispatch.execute_read(crate::semantic_query::SemanticQueryKey::ResolveDecl(
                crate::semantic_query::ResolveDeclKey {
                    scope: crate::semantic_query::ScopeId {
                        canonical_id: Arc::clone(&identity.canonical_id),
                        local_scope: None,
                    },
                    name: Arc::clone(&identity.decl_name),
                },
            ));
            crate::meta_resolve::emit_dispatch_dep_signature_facts(
                dispatch.ctx,
                &read.dep_signature,
            );
            if let crate::semantic_query::QueryResult::Value(id) = read.value {
                collect_runtime_types_for_node(dispatch, id, depth + 1, executing, kinds);
            }
        }
        // DeclPlaceholder — the shallow ResolveDecl of an alias / interface
        // declaration returns this carrier rather than the resolved body;
        // ROOT positions Instantiate the placeholder to obtain the body, then
        // classify. MEMBER-VALUE positions keep the carrier shallow.
        SemanticNodeData::Opaque(crate::semantic_query::QueryError::DeclPlaceholder {
            canonical_id,
            name,
            whole_hash: _,
        }) => {
            if !executing {
                return;
            }
            let slot = dispatch.type_slot_for(Arc::clone(canonical_id), Arc::clone(name));
            let owner_canonical = Arc::clone(canonical_id);
            let read = dispatch.execute_read(crate::semantic_query::SemanticQueryKey::Instantiate(
                crate::semantic_query::InstantiateKey::new(
                    slot,
                    Arc::from(
                        Vec::<crate::semantic_query::SemanticNodeId>::new().into_boxed_slice(),
                    ),
                    dispatch.instantiate_context_for(
                        &owner_canonical,
                        ProjectionReductionContext::structural_transit_with_mode(
                            ProjectionMode::Navigate,
                        ),
                    ),
                ),
            ));
            crate::meta_resolve::emit_dispatch_dep_signature_facts(
                dispatch.ctx,
                &read.dep_signature,
            );
            if let crate::semantic_query::QueryResult::Value(id) = read.value {
                collect_runtime_types_for_node(dispatch, id, depth + 1, executing, kinds);
            }
        }
        SemanticNodeData::Primitive(primitive) => {
            use crate::semantic_query::PrimitiveKind;
            match primitive {
                PrimitiveKind::String => push(kinds, RuntimeType::String),
                PrimitiveKind::Number | PrimitiveKind::BigInt => {
                    push(kinds, RuntimeType::Number);
                }
                PrimitiveKind::Boolean => push(kinds, RuntimeType::Boolean),
                PrimitiveKind::Symbol => push(kinds, RuntimeType::Symbol),
                PrimitiveKind::Null | PrimitiveKind::Undefined => {
                    push(kinds, RuntimeType::Null);
                }
                PrimitiveKind::Object => push(kinds, RuntimeType::Object),
                // `any` / `unknown` / `void` / `never` have no runtime
                // constructor — dropped, mirroring the parser's Unknown
                // filter.
                PrimitiveKind::Any
                | PrimitiveKind::Unknown
                | PrimitiveKind::Void
                | PrimitiveKind::Never => {}
            }
        }
        SemanticNodeData::Literal(literal) => {
            use crate::semantic_query::LiteralValue;
            match literal {
                LiteralValue::String(_) => push(kinds, RuntimeType::String),
                LiteralValue::Number(_) | LiteralValue::BigInt(_) => {
                    push(kinds, RuntimeType::Number);
                }
                LiteralValue::Boolean(_) => push(kinds, RuntimeType::Boolean),
            }
        }
        SemanticNodeData::TemplateLiteral { .. } => push(kinds, RuntimeType::String),
        SemanticNodeData::Object(_)
        | SemanticNodeData::Intersection(_)
        | SemanticNodeData::Mapped { .. }
        | SemanticNodeData::MergedDecl { .. } => push(kinds, RuntimeType::Object),
        SemanticNodeData::Array { .. } | SemanticNodeData::Tuple { .. } => {
            push(kinds, RuntimeType::Array);
        }
        SemanticNodeData::Function { .. } => push(kinds, RuntimeType::Function),
        // Carriers, opaque results, and every other undecidable shape
        // classify as no kinds — never a fabricated constructor.
        _ => {}
    }
}

/// One external-convention named-signature row: no local spans, `map_local:
/// false` (mirroring `finalize_external_resolution`'s external stamps), the
/// consumer maps the row onto the macro type-argument span.
fn named_signature_row(
    name: &str,
    signature: ResolvedCallPayloadForm,
) -> ResolvedNamedCallSignature {
    ResolvedNamedCallSignature {
        span: verter_span::Span::default(),
        name: name.to_string(),
        name_span: None,
        signature,
        map_local: false,
        span_is_absolute: false,
    }
}

/// Render a params list (`name: T, flag?: boolean, ...rest: R[]`) from
/// NODE-DOMAIN params — a decide-free terminal display render. Each param's
/// `ty` node is minted ONCE through the sealed Vue output capability and
/// rendered by name; the `...` / `?` / name come from the node-domain
/// [`FunctionParam`] flags (fallback name `arg{index}`). `None` when any
/// position does not render — the caller fails the whole row closed rather
/// than fabricate a partial signature.
fn render_params_text(
    ctx: &dyn crate::resolver_core::ResolverContext,
    params: &[FunctionParam],
) -> Option<String> {
    let dispatch = ctx.dispatch();
    let cap = TypeinfoVueSurfaceOutputCap::new(&dispatch);
    let mut parts = Vec::with_capacity(params.len());
    for (index, param) in params.iter().enumerate() {
        let ty = cap
            .materialize_output_type_expr(param.ty)
            .map(|raised| raised.into_type_expr(&cap))?;
        let ty_text = render_type_expr_display(&ty)?;
        let name = param
            .name
            .as_ref()
            .map(|name| name.to_string())
            .unwrap_or_else(|| format!("arg{index}"));
        let mut part = String::new();
        if param.rest {
            part.push_str("...");
        }
        part.push_str(&name);
        if param.optional {
            part.push('?');
        }
        part.push_str(": ");
        part.push_str(&ty_text);
        parts.push(part);
    }
    Some(parts.join(", "))
}
