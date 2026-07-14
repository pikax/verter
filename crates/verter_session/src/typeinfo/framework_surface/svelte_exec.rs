#![deny(missing_docs)]
//! The Svelte resolution leg — the executor-private resolver for one Svelte
//! source family.
//!
//! [`resolve_svelte_surface`] is the executor's `PlannedDemand::SvelteSurface`
//! arm. It READS the owner's typed Svelte facts
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
use verter_type_expr::TypeExpr;

use verter_protocol::typeinfo::graph::FrameworkSurfaceKind;

use crate::framework::surface_store::{FullKey, StoredSurfaceDto};
use crate::meta_resolve::callable_view::{CallableNodeView, PositionalParamNode};
use crate::project_semantic_dispatch::output_materialization::OutputProjector;
use crate::resolver_core::ResolverContext;
use crate::typeinfo::framework_surface::resolved_surface_access::ResolvedSurfaceAccess;
use crate::typeinfo::framework_surface::results::{
    EmitsSurface, MacroSurfaceDtos, ModelBinding, ModelSurface, PropsSurface, ResolvedEmitField,
    ResolvedMacroPayload, ResolvedOutcome,
};
use crate::typeinfo::framework_surface::vue_exec::{
    emits_from_typeinfo_surface, navigate_param_to_object_surface, props_from_typeinfo_surface,
    VueMacroSurface,
};
use crate::typeinfo::framework_surface::{SvelteSurfaceKey, SvelteSurfaceSource};
use crate::typeinfo::surface::TypeInfoSurface;
use crate::typeinfo::types::TypeInfoQueryLevel;
use crate::VerterHost;

crate::project_semantic_dispatch::output_materialization::define_output_capability! {
    /// The Svelte framework-surface executor's output-sink capability: the
    /// Svelte resolution leg here holds this to materialize a graph node into
    /// a sealed output carrier and unwrap it. Its constructor is visible ONLY
    /// within `crate::typeinfo::framework_surface::svelte_exec` — NOT the
    /// whole `typeinfo` subtree — so no `typeinfo` sibling can mint it
    /// (planted `TypeinfoSvelteSurfaceOutputCap::new` outside this leaf is
    /// `E0624`).
    pub(crate) struct TypeinfoSvelteSurfaceOutputCap;
    mint: pub(in crate::typeinfo::framework_surface::svelte_exec)
}

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
/// bundle.
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
    let Some(indexed) = ctx
        .ensure_indexed_ready_serve(owner)
        .map(|serve| serve.indexed)
    else {
        return ResolvedOutcome::Missing;
    };
    let whole_hash = indexed.whole_hash;
    let owner_arc: Arc<str> = Arc::from(owner);

    // The framework-neutral key plus the Svelte adapter's typed remainder (one
    // source family per row). Content-addressed via `owner_whole_hash`.
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
    let (outcome, finalise, non_cacheable_read_observed) =
        crate::fact_signature_helpers::install_fact_tracer(host, || {
            compute_svelte_surface(host, ctx, owner, source)
        });

    // ReturnOnly never publishes — a surface resolved from a served-without-
    // publication (fenced) artifact must not enter the shared store (its
    // cross-file facts validate against the live view). Serve the freshly
    // computed bundle WITHOUT caching.
    if non_cacheable_read_observed {
        return outcome;
    }
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
    // family, resolved against the executor's ONE request view `ctx` (NOT
    // a second `current_store_view_for_query`). The legacy-slot family reads the
    // content-addressed parse carrier instead.
    let facts = host.resolve_svelte_script_facts_with_ctx(ctx, owner);

    match source {
        SvelteSurfaceSource::RunesProps => resolve_runes_props(ctx, owner, facts.as_deref()),
        SvelteSurfaceSource::LegacyExportLet => resolve_legacy_export_let(facts.as_deref()),
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

/// Svelte-private mint seal. With no constructor reachable outside
/// `svelte_exec`, the only code that can place a `SvelteSurfaceSeal` into a
/// [`SvelteResolvedSurface`] — and therefore the only code that can mint one —
/// is the `svelte_exec`-private [`macro_surface_shell`].
struct SvelteSurfaceSeal;

/// A RESOLVED Svelte macro surface — Svelte's OWN sealed resolved-surface
/// token, minted ONLY inside `svelte_exec` after its resolver produced the
/// surface. It is NOT a [`ResolvedVueSurface`]: Svelte does not mint the Vue
/// token from a public `VueMacroSurface` shell (that would re-open the
/// forgeability the Vue minter's privatization closed).
///
/// It wraps a resolution-derived [`VueMacroSurface`] carrier (the shared
/// surface carrier both frameworks normalize off) plus a private
/// [`SvelteSurfaceSeal`], and drives the SAME shared per-kind normalizers
/// through the sealed [`ResolvedSurfaceAccess`] trait — no second normalizer,
/// no extra allocation (the accessor borrows).
///
/// The type is `pub(in crate::typeinfo::framework_surface)` so the trait module
/// [`crate::typeinfo::framework_surface::resolved_surface_access`] (the SOLE
/// implementor of [`ResolvedSurfaceAccess`]) can name it for the impl, while the
/// CONSTRUCTOR ([`macro_surface_shell`], module-private) and the private
/// [`SvelteSurfaceSeal`] field keep `svelte_exec` the only minter — a sibling
/// cannot construct one (private fields) and cannot implement the accessor (the
/// supertrait seal is private to the trait module).
pub(in crate::typeinfo::framework_surface) struct SvelteResolvedSurface {
    surface: VueMacroSurface,
    _seal: SvelteSurfaceSeal,
}

impl SvelteResolvedSurface {
    /// The resolution-derived carrier the shared normalizers read, by borrow.
    /// Exposed to the trait module
    /// [`crate::typeinfo::framework_surface::resolved_surface_access`] while the
    /// private `surface` field + the [`SvelteSurfaceSeal`] keep the token
    /// unmintable and unmodifiable outside `svelte_exec`.
    pub(in crate::typeinfo::framework_surface) fn surface_carrier(&self) -> &VueMacroSurface {
        &self.surface
    }
}

/// Wrap a resolution-derived [`TypeInfoSurface`] in the carrier and mint
/// Svelte's OWN sealed [`SvelteResolvedSurface`] token so the shared per-kind
/// normalizers (`props_from_typeinfo_surface` / `emits…` /
/// `svelte_snippet_slots…`) consume it through [`ResolvedSurfaceAccess`]. The
/// carrier holds the Svelte owner so member scopes fall back to the owner file;
/// `macro_index` / `macro_call_span` are synthetic (the surface members carry
/// their own per-member spans).
///
/// The Svelte path is a RESOLUTION sink too: the `surface` is a navigated /
/// filtered surface this module resolved, so minting the sealed token here
/// (rather than handing a bare `&VueMacroSurface` to the now-token-gated
/// normalizers, or minting the Vue token from a forgeable shell) keeps both Vue
/// and Svelte on ONE sealed-surface discipline while each owns its own token.
fn macro_surface_shell(
    surface: TypeInfoSurface,
    macro_kind: AnalyzedMacroKind,
    owner: &str,
) -> SvelteResolvedSurface {
    SvelteResolvedSurface {
        surface: VueMacroSurface {
            surface,
            macro_kind,
            owner_canonical: Arc::from(owner),
            macro_index: 0,
            macro_call_span: verter_span::Span::default(),
            level: TypeInfoQueryLevel::FullMetadata,
        },
        _seal: SvelteSurfaceSeal,
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
    for row in &mut fields {
        if default_keys.contains(row.analysis.name.as_str()) {
            row.analysis.is_optional = true;
        }
    }
    let dtos = MacroSurfaceDtos {
        props: Some(PropsSurface {
            fields,
            index_signatures: Vec::new(),
            prop_defaults: facts.prop_defaults.to_vec(),
            prop_origins,
        }),
        ..Default::default()
    };
    ResolvedOutcome::Resolved(Arc::new(dtos))
}

/// Derive each prop's MEMBER-DECLARATION origin from the resolved props object
/// surface (the framework-neutral sidecar).
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
/// information at the script-fact layer, so each surfaces as an
/// annotation-less prop (optional when it declares a default).
fn resolve_legacy_export_let(facts: Option<&SvelteScriptFacts>) -> ResolvedMacroPayload {
    let Some(facts) = facts else {
        return ResolvedOutcome::Missing;
    };
    if facts.legacy_props.is_empty() {
        return ResolvedOutcome::Missing;
    }
    let fields = facts
        .legacy_props
        .iter()
        .map(legacy_prop_field)
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

/// One legacy `export let` prop as an annotation-less row. The legacy
/// capture carries no authored type annotation, so the field is locator-less
/// (`payload: None` paired with a `None` scope — never a fabricated
/// position) and its source is the PROVEN unannotated absence; consumers
/// treat the annotation-less prop as loose (`any`-equivalent).
fn legacy_prop_field(
    prop: &SvelteLegacyProp,
) -> crate::typeinfo::framework_surface::results::ResolvedPropField {
    crate::typeinfo::framework_surface::results::ResolvedPropField {
        analysis: AnalyzedPropField {
            name: prop.name.clone(),
            // A prop with a default value is optional.
            is_optional: prop.has_default,
            span: verter_span::Span::default(),
            type_annotation: None,
            payload: None,
            type_expr_scope: None,
            description: None,
            tags: Vec::new(),
            resolution_source: TypeResolutionSource::Rust,
            resolution_error: None,
            declared_in_macro_type_arg: false,
        },
        type_source: verter_type_expr::facts::SourcePosition::unannotated(),
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
    let props_fields: Vec<crate::typeinfo::framework_surface::results::ResolvedPropField> = facts
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
                .find(|row| &row.analysis.name == name)
                .map(|row| row.analysis.clone())
                .unwrap_or_else(|| AnalyzedPropField {
                    name: name.clone(),
                    is_optional: false,
                    span: verter_span::Span::default(),
                    type_annotation: None,
                    // A `$bindable()` member with no resolved `$props` field
                    // has no authored annotation position — the honest
                    // locator-less form (`payload: None` paired with a `None`
                    // scope), never a fabricated position.
                    payload: None,
                    type_expr_scope: None,
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

/// The SVELTE-SPECIFIC snippet-slot normalizer.
///
/// Unlike Vue's shared `slots_from_typeinfo_surface` (which surfaces ONLY a
/// slot callable's FIRST-parameter object), a Svelte `Snippet<[a, b]>` exposes
/// EVERY positional parameter as an ordered slot binding. For each validated
/// snippet member this:
///
/// 1. reads the snippet's positional binding NODES through the shared
///    [`CallableNodeView::validated_snippet_positional_params`] — the
///    carrier-preserving peel reads an un-instantiated `Snippet<Params>`
///    carrier's `Params` directly, a realized `Function` fallback skips the
///    leading `this` parameter and expands a rest-tuple parameter
///    (`...args: [item: Item, index: number]`) into one binding per tuple
///    element, and a UNION / INTERSECTION of arms combines by index
///    (intersecting each positional binding's types, the Vue multi-arm rule);
/// 2. materializes each binding node ONCE at the terminal
///    [`materialize_snippet_slot_bindings`] DTO sink (label from the
///    element/param label, `arg{index}` fallback) — this normalizer makes NO
///    decision on any materialized value.
///
/// The ordered `bindings` vector IS the positional order (no explicit position
/// field). The binding type is the typed-IR element type (typed-IR only — no
/// source slicing).
fn svelte_snippet_slots_from_typeinfo_surface(
    ctx: &dyn ResolverContext,
    resolved: &impl ResolvedSurfaceAccess,
) -> Vec<AnalyzedSlotField> {
    let macro_surface = resolved.macro_surface();
    let host = ctx.host_for_fact_tracer_install();
    let dispatch = ctx.dispatch();
    // Node-domain demand identity. `Navigate` so a carrier-wrapped snippet
    // (`Snippet<Args>` with `Args` a `DeclRef`-to-tuple) resolves its `Params`.
    // The binding types are minted shallow at the terminal sink regardless.
    let context = crate::semantic_query::ProjectionReductionContext::published(
        crate::semantic_query::ProjectionMode::Navigate,
    );
    macro_surface
        .surface
        .members
        .iter()
        .filter(|member| member.visibility.is_public())
        .filter_map(|member| {
            // The validated-snippet positional binding NODES, decided ENTIRELY in
            // the node domain through the shared `CallableNodeView` (which
            // carrier-preserving-peels the `Snippet<Params>` carrier and reads its
            // uninstantiated `Params` — resolving a `DeclRef`-to-tuple `Params`
            // to its ordered elements).
            // A fail-closed `None` (an unresolved `Params` carrier) drops the slot.
            let params = CallableNodeView::new(&dispatch, member.value)
                .validated_snippet_positional_params(context)?;
            // The slot member's scope (the shared member-value-scope rule) —
            // each published binding display value is paired with it.
            let member_scope = crate::typeinfo::framework_surface::scope::member_value_expr_scope(
                host,
                member,
                macro_surface.owner_canonical.as_ref(),
            );
            // Materialize each binding node ONCE at the terminal DTO sink; this
            // normalizer makes NO decision on any materialized value.
            let bindings = materialize_snippet_slot_bindings(ctx, &member_scope, &params);
            Some(AnalyzedSlotField {
                name: member.name.as_ref().to_string(),
                is_required: !member.optional,
                span: verter_span::Span::default(),
                bindings,
                return_type: None,
                payload: None,
                return_expr_scope: None,
                description: None,
                tags: Vec::new(),
            })
        })
        .collect()
}

/// Materialize the validated Svelte snippet-slot bindings from NODE-DOMAIN
/// positional params — a GENUINE decide-free terminal one-shot sink (the
/// snippet-slot twin of [`materialize_payload_tuple`]). Each
/// [`PositionalParamNode::ty`] is minted ONCE through the sealed Svelte output
/// capability; the binding NAME is the element/param label (fallback
/// `arg{index}`), the display `type_annotation` is rendered from the minted
/// value via the by-name `.and_then(render_type_expr_display)` form and
/// paired with the caller-derived slot MEMBER scope (`member_scope` — the
/// shared member-value-scope rule), so a binding's named refs resolve in the
/// member's file. Value⇔scope pairing: an unrendered value carries no scope.
///
/// The published binding is locator-less (`payload: None`): the flat
/// field-position vocabulary cannot address a nested (slot, binding) position
/// honestly, so typed binding demand is host-raised.
/// It makes NO decision on any materialized value (no branch / match /
/// shape-extract) and takes NO `&TypeExpr` param (node ids + the active
/// `ctx`). The mint cap is constructed INTERNALLY from `ctx` (the
/// `raise_member_value` pattern) — a cap is a mint AUTHORITY and must not cross
/// the boundary from the non-terminal caller.
pub(in crate::typeinfo::framework_surface::svelte_exec) fn materialize_snippet_slot_bindings(
    ctx: &dyn ResolverContext,
    member_scope: &verter_type_expr::TypeExprScope,
    params: &[PositionalParamNode],
) -> Vec<AnalyzedSlotFieldBinding> {
    let dispatch = ctx.dispatch();
    let cap = TypeinfoSvelteSurfaceOutputCap::new(&dispatch);
    params
        .iter()
        .enumerate()
        .map(|(index, param)| {
            // Mint the binding's type node ONCE. Display renders through the
            // by-name `.and_then` form (never a direct read-of-mint decide).
            let raised = cap
                .materialize_output_type_expr(param.ty)
                .map(|raised| raised.into_type_expr(&cap));
            let type_annotation = raised
                .as_ref()
                .and_then(crate::resolver_core::surface_projector::render_type_expr_display);
            // Value⇔scope pairing: the rendered display rides with the slot
            // member's scope; an unrendered value carries no scope.
            let binding_expr_scope = type_annotation.as_ref().map(|_| member_scope.clone());
            let name = param
                .label
                .as_ref()
                .map(|label| label.to_string())
                .unwrap_or_else(|| format!("arg{index}"));
            AnalyzedSlotFieldBinding {
                name,
                type_annotation,
                payload: None,
                binding_expr_scope,
                span: verter_span::Span::default(),
            }
        })
        .collect()
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
    let Some(indexed) = ctx
        .ensure_indexed_ready_serve(owner)
        .map(|serve| serve.indexed)
    else {
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
/// owner's snapshot raw source (the slot-name slice indexes it directly);
/// `owner` is the owning component's canonical id — the resolution scope every
/// published binding display value is paired with.
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
                            payload: None,
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
/// The binding VALUE displays as `any` — a DOCUMENTED, owner-decided
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
/// The binding is locator-less (`payload: None`): a template `<slot>`
/// attribute has no authored TYPE position to address — never a fabricated
/// position. Value⇔scope pairing: the published display VALUE rides with its
/// resolution SCOPE — the owning component's canonical id (`owner`) — so even
/// the carve-out display satisfies the documented
/// [`AnalyzedSlotFieldBinding`] pairing invariant
/// (`type_annotation.is_some() <=> binding_expr_scope.is_some()`).
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
            let type_annotation = Some("any".to_string());
            // Value⇔scope pairing: a published display value carries the
            // owning component's resolution scope; an unpublished value
            // carries no scope.
            let binding_expr_scope = type_annotation
                .as_ref()
                .map(|_| verter_type_expr::TypeExprScope::new(owner));
            Some(AnalyzedSlotFieldBinding {
                name: name.clone(),
                type_annotation,
                payload: None,
                binding_expr_scope,
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
        .map(|surface| callback_events_from_props_surface(ctx, &surface))
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
/// function-like type becomes a [`ResolvedEmitField`] named `E` whose payload is
/// the callback's parameters as a labelled tuple (every parameter — NO strip).
///
/// The value is function-like in two shapes:
///
/// - a BARE callable — a required prop `onselect: (r: Row) => void`, and a
///   member-OPTIONAL prop `onselect?: (r: Row) => void` (the `?` is carried by
///   the surface member `optional` flag, so the VALUE raises to a bare
///   `Function`, not a union);
/// - a callable arm of an EXPLICIT nullish UNION VALUE —
///   `onselect: ((r: Row) => void) | undefined` raises to
///   `Union([Function, Undefined])`, and the node-domain
///   `CallableNodeView::signature` (via `single_callable_arm`) strips the nullish
///   arm and pulls out the single callable. A nullish INTERSECTION
///   (`Fn & undefined` = `never`) is deliberately REFUSED, matching the shared
///   `realize_callable_member`.
fn callback_events_from_props_surface(
    ctx: &dyn ResolverContext,
    surface: &TypeInfoSurface,
) -> Vec<ResolvedEmitField> {
    let dispatch = ctx.dispatch();
    // Publication sink (DTO event payload tuples): the callable-arm decide and the
    // payload param selection are made ENTIRELY in the node domain through the
    // shared `CallableNodeView`; materialization happens ONCE at the terminal
    // `materialize_payload_tuple` sink (which constructs its own mint cap from
    // `ctx`). This normalizer calls NO mint verb and holds NO cap.
    let context = crate::semantic_query::ProjectionReductionContext::published(
        crate::semantic_query::ProjectionMode::Navigate,
    );
    let mut events: Vec<ResolvedEmitField> = Vec::new();
    for member in surface.members.iter().filter(|m| m.visibility.is_public()) {
        // Structural `on${E}` callback convention: `on` prefix + a NON-EMPTY
        // suffix. The suffix is the event name (NO strip applied to the payload).
        let Some(event_name) = member.name.as_ref().strip_prefix("on") else {
            continue;
        };
        if event_name.is_empty() {
            continue;
        }
        // The value MUST realize to a single FUNCTION-LIKE type. The node-domain
        // `single_callable_arm` (via `signature`) resolves the member's carrier(s)
        // through the shared structural-fact demand primitive, strips an EXPLICIT
        // nullish (`undefined` / `null`) arm an `onselect: ((r) => void) | undefined`
        // value carries, and REFUSES two distinct callable arms. (A
        // member-`?`-optional callback keeps its `?` on the surface `optional`
        // flag, so the value is a bare `Function` here.) `None` — a non-callable
        // prop (`onclick: string` / `label?: string`), or a union with no single
        // callable arm — is NOT an event.
        let view = CallableNodeView::new(&dispatch, member.value);
        let Some(signature) = view.signature(context) else {
            continue;
        };
        // Payload = the callback's PARAMETERS as a labelled tuple (ALL of them — a
        // callback prop's parameters ARE the event payload; there is no leading
        // event-name parameter to strip), materialized ONCE at the terminal sink
        // for DISPLAY ONLY (the by-name `render_type_expr_display` form — this
        // normalizer NEVER decides on the materialized value). The tuple is a
        // per-event SYNTHESIS over the callback's params — it has no flat
        // authored macro-payload position, so `payload` stays the honest `None`
        // (paired with a `None` scope); typed payload demand is host-raised
        // through the graph surface.
        let raw_params = signature.raw_params();
        let payload_tuple = materialize_payload_tuple(ctx, &raw_params);
        let payload_type =
            crate::resolver_core::surface_projector::render_type_expr_display(&payload_tuple);
        // The payload SOURCE: the closed tuple over the SAME callback params,
        // projected in the node domain through the shared dispatch (leaf /
        // leaf-union element facts, order preserved; NO strip). `None` when
        // any param is richer than the closed element vocabulary.
        // The realized event's payload-tuple position is REQUIRED: params
        // richer than the closed element vocabulary have no faithful source,
        // so the position is the typed source-construction FAILURE — never a
        // fabricated `unknown` success.
        let payload_source = dispatch
            .closed_params_tuple_source(&raw_params)
            .map(verter_type_expr::facts::SourcePosition::Present)
            .unwrap_or(verter_type_expr::facts::SourcePosition::Failed(
                verter_type_expr::facts::SemanticSourceFailure::UnrepresentableRequiredPayload,
            ));
        events.push(ResolvedEmitField {
            analysis: AnalyzedEmitField {
                name: event_name.to_string(),
                span: verter_span::Span::default(),
                payload_type,
                payload: None,
                payload_expr_scope: None,
                description: None,
                tags: Vec::new(),
            },
            payload_source,
        });
    }
    // De-duplicate by event name, first-writer-wins.
    let mut seen = std::collections::HashSet::new();
    events.retain(|e| seen.insert(e.analysis.name.clone()));
    events
}

/// Materialize a Svelte callback-event payload tuple from NODE-DOMAIN params — a
/// GENUINE decide-free terminal one-shot sink (the Svelte-cap twin of the Vue
/// `vue_exec::normalize::materialize_payload_tuple`). Each `param.ty` node is
/// minted ONCE through the sealed Svelte output capability into a labelled
/// `TupleElement` that preserves the param's name / optional / rest; the result
/// is the payload `TypeExpr::Tuple`. It makes NO decision on any materialized
/// value (no branch / match / shape-extract), takes NO `&TypeExpr` param (node
/// ids + the active `ctx`), and lives inside the Svelte cap's
/// `pub(in …::svelte_exec)` mint scope. The mint cap is constructed INTERNALLY
/// from `ctx` (the `raise_member_value` pattern) — a cap is a mint AUTHORITY and
/// must not cross the boundary from the non-terminal caller. A callback prop has
/// NO leading event-name param, so ALL params enter the tuple (no `[1..]` skip).
///
/// Materialization is POSITION-PRESERVING: the params are `.map`ped (never
/// `filter_map`ped), so a param whose node does not materialize keeps its tuple
/// SLOT with the opaque `Unknown` raise-miss value instead of shifting the
/// subsequent payload elements. This does not arise in practice — the realized
/// signature's param nodes ARE the callback's own declared parameter types, which
/// all materialize — so the fallback is position-safety robustness only.
pub(in crate::typeinfo::framework_surface::svelte_exec) fn materialize_payload_tuple(
    ctx: &dyn ResolverContext,
    params: &[crate::semantic_query::FunctionParam],
) -> TypeExpr {
    use verter_type_expr::TupleElement;
    // Construct the mint cap INTERNALLY from the active `ctx` (the
    // `raise_member_value` pattern): a cap is a genuine mint AUTHORITY that must
    // not cross into a `TypeExpr`-producing sink from the non-terminal caller.
    let dispatch = ctx.dispatch();
    let cap = TypeinfoSvelteSurfaceOutputCap::new(&dispatch);
    let elements = params
        .iter()
        .map(|param| {
            // Position-preserving: mint the param's `ty` node ONCE; a node that
            // does not materialize keeps its tuple SLOT with the opaque `Unknown`
            // raise-miss value (the `output_sink::raise_node_to_sealed_carrier`
            // convention) so subsequent payload params never shift. A declared
            // param's `ty` always mints, so the fallback is robustness only.
            let ty = cap
                .materialize_output_type_expr(param.ty)
                .map(|raised| raised.into_type_expr(&cap))
                .unwrap_or_else(|| TypeExpr::Unknown { raw: String::new() });
            TupleElement {
                // Node-domain `FunctionParam.name` (`Option<Arc<str>>`) → the
                // display-facing tuple `label` (`Option<String>`).
                label: param.name.as_ref().map(|n| n.to_string()),
                ty,
                optional: param.optional,
                rest: param.rest,
            }
        })
        .collect();
    TypeExpr::Tuple {
        elements,
        readonly: false,
    }
}

/// EXPOSE from the exported instance-script members. Each export is a named
/// member of the public instance; the member type stays a shallow `Ref` to the
/// exported binding (shallow-by-default — the consumer re-resolves on demand).
fn resolve_instance_exports(facts: Option<&SvelteScriptFacts>) -> ResolvedMacroPayload {
    use crate::typeinfo::framework_surface::results::{
        ExposeSurface, NamedTypeMember, NamedTypeMemberOutput,
    };
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
            value: Some(NamedTypeMemberOutput::Ref {
                name: Arc::from(name.as_str()),
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
