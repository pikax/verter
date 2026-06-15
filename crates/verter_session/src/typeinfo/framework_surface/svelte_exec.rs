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
    slots_from_typeinfo_surface, VueMacroSurface,
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
    let Some(props_type) = facts.and_then(|f| f.props_type.as_ref()) else {
        return ResolvedOutcome::Missing;
    };
    let fields = navigate_param_to_object_surface(ctx, owner, props_type)
        .map(|surface| {
            props_from_typeinfo_surface(
                ctx,
                &macro_surface_shell(surface, AnalyzedMacroKind::DefineProps, owner),
            )
        })
        // A props type that does not project to an object surface (a primitive /
        // open generic) still establishes a PRESENT props surface — supported-
        // empty, never a Missing.
        .unwrap_or_default();
    let dtos = MacroSurfaceDtos {
        props: Some(PropsSurface {
            fields,
            index_signatures: Vec::new(),
        }),
        ..Default::default()
    };
    ResolvedOutcome::Resolved(Arc::new(dtos))
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
            slots_from_typeinfo_surface(
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
mod tests {
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
        let view =
            crate::typeinfo::current_store_view_for_query(&host).expect("current store view");
        (host, view)
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
}
