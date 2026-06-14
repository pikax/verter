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
    AnalyzedMacroKind, AnalyzedPropField, AnalyzedSlotField, AnalyzedSlotFieldBinding,
    TypeResolutionSource,
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
        SvelteSurfaceSource::LegacyDispatcher => FrameworkSurfaceKind::Emits,
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
        SvelteSurfaceSource::LegacyExportLet => resolve_legacy_export_let(facts.as_deref()),
        SvelteSurfaceSource::Bindable => resolve_bindable(ctx, owner, facts.as_deref()),
        SvelteSurfaceSource::SnippetProps => resolve_snippet_props(ctx, owner, facts.as_deref()),
        SvelteSurfaceSource::LegacySlotInventory => resolve_legacy_slot_inventory(ctx, owner),
        SvelteSurfaceSource::LegacyDispatcher => resolve_dispatcher(ctx, owner, facts.as_deref()),
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
        }),
        ..Default::default()
    };
    ResolvedOutcome::Resolved(Arc::new(dtos))
}

/// One legacy `export let` prop as an `any`-typed [`AnalyzedPropField`].
fn legacy_prop_field(prop: &SvelteLegacyProp) -> AnalyzedPropField {
    AnalyzedPropField {
        name: prop.name.clone(),
        // A prop with a default value is optional.
        is_optional: prop.has_default,
        span: verter_span::Span::default(),
        type_annotation: None,
        type_expr: Some(TypeExpr::Primitive(PrimitiveName::Any)),
        type_expr_scope: None,
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
    collect_slot_elements(&parsed.template, indexed.raw_source.as_ref(), &mut slots);

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
fn collect_slot_elements(nodes: &[SvelteNode], raw_source: &str, out: &mut Vec<AnalyzedSlotField>) {
    for node in nodes {
        match node {
            SvelteNode::Element(element) => {
                if element.kind == SvelteElementKind::Intrinsic && element.name == "slot" {
                    let name =
                        slot_name(element, raw_source).unwrap_or_else(|| "default".to_string());
                    if !out.iter().any(|s| s.name == name) {
                        let bindings = slot_bindings(element);
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
                collect_slot_elements(&element.children, raw_source, out);
            }
            SvelteNode::Block(block) => {
                collect_slot_block(block, raw_source, out);
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
    out: &mut Vec<AnalyzedSlotField>,
) {
    collect_slot_elements(&block.children, raw_source, out);
    for clause in &block.clauses {
        collect_slot_elements(&clause.children, raw_source, out);
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
/// a slot binding (its value type stays shallow `any` — the legacy forwarding
/// type is not statically resolved at this layer).
fn slot_bindings(
    element: &verter_compiler::svelte::parser::template_ast::SvelteElement,
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
                binding_expr_scope: None,
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
