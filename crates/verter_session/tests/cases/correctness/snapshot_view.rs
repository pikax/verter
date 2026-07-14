//! Test-only stable projection of `ComponentMetaAnalysis`.
//!
//! This view exists ONLY in tests. It does NOT add `Serialize` to
//! production types. The
//! projection drops fields that are known-non-deterministic (raw
//! spans, source positions tied to byte offsets, internal cache
//! identifiers) and sorts every collection by a stable key so the
//! JSON output is identical between runs and across machines.
//!
//! Every field that affects type-resolution correctness is
//! included — the discriminating self-test row table is the
//! coverage contract: adding a new SnapshotView field requires
//! adding a `MutationKind` row in `correctness.rs`. Without that, the
//! gate is silently blind to the new field.

use std::collections::BTreeMap;

use serde::Serialize;
use verter_semantic::analysis::component_meta::{
    ComponentMetaAnalysis, EventAnalysis, ExposedAnalysis, FallthroughEventEntry,
    FallthroughPropEntry, FallthroughSurface, InheritedSource, ModelAnalysis, NoFallthroughReason,
    PropAnalysis, SlotAnalysis, SlotBindingAnalysis,
};
use verter_session::VerterHost;
use verter_type_expr::facts::SemanticTypeSource;
use verter_type_expr::{LiteralValue, MappedModifier, ObjectMember, PrimitiveName, TypeExpr};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct SnapshotView {
    pub component_name: String,
    pub props: Vec<PropView>,
    pub events: Vec<EventView>,
    pub slots: Vec<SlotView>,
    pub models: Vec<ModelView>,
    pub exposed: Vec<ExposedView>,
    pub fallthrough: Option<FallthroughView>,
    pub flags: FlagsView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct PropView {
    pub name: String,
    /// Canonical string form of the resolved [`TypeExpr`]. The
    /// renderer below produces a deterministic, TS-spec-equivalent
    /// signature that is independent of internal field ordering.
    pub type_signature: String,
    pub required: bool,
    pub has_default: bool,
    pub default_signature: Option<String>,
    pub doc: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct SlotView {
    pub name: String,
    pub payload_signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct EventView {
    pub name: String,
    pub params_signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct ModelView {
    pub name: String,
    pub type_signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct ExposedView {
    pub name: String,
    pub type_signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct FallthroughView {
    pub inherit_attrs: bool,
    pub surface_signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct FlagsView {
    pub async_setup: bool,
    pub has_inherit_attrs_false: bool,
}

impl SnapshotView {
    pub fn from_analysis(host: &VerterHost, analysis: &ComponentMetaAnalysis) -> Self {
        let component_name = derive_component_name(&analysis.file_path);
        let owner = analysis.file_path.as_str();

        let mut props: Vec<PropView> = analysis
            .props
            .iter()
            .map(|prop| prop_view_from(host, owner, prop))
            .collect();
        props.sort_by(|a, b| a.name.cmp(&b.name));

        let mut events: Vec<EventView> = analysis
            .events
            .iter()
            .map(|event| event_view_from(host, owner, event))
            .collect();
        events.sort_by(|a, b| a.name.cmp(&b.name));

        let mut slots: Vec<SlotView> = analysis
            .slots
            .iter()
            .map(|slot| slot_view_from(host, owner, slot))
            .collect();
        slots.sort_by(|a, b| a.name.cmp(&b.name));

        let mut models: Vec<ModelView> = analysis
            .models
            .iter()
            .map(|model| model_view_from(host, owner, model))
            .collect();
        models.sort_by(|a, b| a.name.cmp(&b.name));

        let mut exposed: Vec<ExposedView> = analysis
            .exposed
            .iter()
            .map(|exposed| exposed_view_from(host, owner, exposed))
            .collect();
        exposed.sort_by(|a, b| a.name.cmp(&b.name));

        let fallthrough = build_fallthrough_view(host, analysis);

        SnapshotView {
            component_name,
            props,
            events,
            slots,
            models,
            exposed,
            fallthrough,
            flags: FlagsView {
                async_setup: analysis.flags.async_setup,
                has_inherit_attrs_false: analysis.flags.has_inherit_attrs_false,
            },
        }
    }
}

fn derive_component_name(file_path: &str) -> String {
    let base = file_path.rsplit(['\\', '/']).next().unwrap_or(file_path);
    let stem = base.split('.').next().unwrap_or(base);
    pascalize(stem)
}

fn pascalize(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut capitalize_next = true;
    for ch in input.chars() {
        if ch == '_' || ch == '-' {
            capitalize_next = true;
        } else if capitalize_next {
            out.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Demand-render a published `SemanticTypeSource` through the shared SHALLOW
/// probe — the published (shallow-by-default) shape, rendered canonically.
///
/// A `None` source or a raise miss renders a LOUD marker that can never
/// silently equal a pinned snapshot signature, so a dropped source surfaces
/// as a snapshot diff instead of a silent pass.
fn render_source_signature(
    host: &VerterHost,
    owner: &str,
    source: Option<&SemanticTypeSource>,
) -> String {
    let Some(source) = source else {
        return "/*no published source*/".to_string();
    };
    match verter_session::test_only::semantic_source_probe::shallow_type_expr(host, owner, source) {
        Some(expr) => render_type_signature(&expr),
        None => "/*source raise miss*/".to_string(),
    }
}

/// Native prop projection: the published `SemanticTypeSource` carrier is
/// rendered BARE (shallow-by-default). Optionality is carried by the TYPED
/// flags (`required` / `has_default`) — the `T | undefined` optional-model
/// display is a compat-layer projection, never native snapshot truth.
fn prop_view_from(host: &VerterHost, owner: &str, prop: &PropAnalysis) -> PropView {
    PropView {
        name: prop.name.clone(),
        type_signature: render_source_signature(host, owner, prop.type_source.present()),
        required: prop.required,
        has_default: prop.has_default,
        default_signature: prop.default_value.clone(),
        doc: prop.description.clone(),
    }
}

fn event_view_from(host: &VerterHost, owner: &str, event: &EventAnalysis) -> EventView {
    let params = event
        .raw_signature
        .clone()
        .unwrap_or_else(|| render_source_signature(host, owner, event.payload.present()));
    EventView {
        name: event.name.clone(),
        params_signature: params,
    }
}

fn slot_view_from(host: &VerterHost, owner: &str, slot: &SlotAnalysis) -> SlotView {
    let payload = if slot.bindings.is_empty() {
        slot.return_type.clone().unwrap_or_else(|| "{}".to_string())
    } else {
        let mut entries: Vec<String> = slot
            .bindings
            .iter()
            .map(|binding| slot_binding_signature(host, owner, binding))
            .collect();
        entries.sort();
        format!("{{ {} }}", entries.join("; "))
    };
    SlotView {
        name: slot.name.clone(),
        payload_signature: payload,
    }
}

fn slot_binding_signature(host: &VerterHost, owner: &str, binding: &SlotBindingAnalysis) -> String {
    // A graph-raised binding row publishes the first-class SYNTHETIC
    // carrier (its shallow identity); the binding's VALUE is read through
    // the ONE sanctioned explicit-deepen demand route
    // (`project_slot_binding_member`'s 3-hop composition behind the
    // synthetic raise) — the snapshot renders the resolved value
    // (`item: string`), never the carrier's opaque identity. Every other
    // published source renders its shallow-by-default shape.
    let rendered = match binding.type_source.present() {
        Some(source @ SemanticTypeSource::SyntheticSlotBinding(_)) => {
            match verter_session::test_only::semantic_source_probe::demand_type_expr(
                host, owner, source,
            ) {
                Some(expr) => render_type_signature(&expr),
                None => "/*source raise miss*/".to_string(),
            }
        }
        other => render_source_signature(host, owner, other),
    };
    format!("{}: {}", binding.name, rendered)
}

fn model_view_from(host: &VerterHost, owner: &str, model: &ModelAnalysis) -> ModelView {
    ModelView {
        name: model.name.clone(),
        type_signature: render_source_signature(host, owner, model.type_source.present()),
    }
}

fn exposed_view_from(host: &VerterHost, owner: &str, exposed: &ExposedAnalysis) -> ExposedView {
    ExposedView {
        name: exposed.name.clone(),
        type_signature: render_source_signature(host, owner, exposed.type_source.present()),
    }
}

fn build_fallthrough_view(
    host: &VerterHost,
    analysis: &ComponentMetaAnalysis,
) -> Option<FallthroughView> {
    let owner = analysis.file_path.as_str();
    // Projection rule: the fallthrough view
    // is emitted ONLY when the SFC's fallthrough surface is
    // *meaningful* for component-meta semantics — that is, either
    // (a) the SFC explicitly opted out via `inheritAttrs: false`, or
    // (b) the inherited surface includes a `Component`-sourced
    // entry (the propagation case from §"Fallthrough / Root
    // Inheritance" in CLAUDE.md). The default native-tag fallthrough
    // (e.g., a `<div />` root surfacing every HTMLAttributes member)
    // is suppressed because it is identical for every default-root SFC
    // and would dominate the snapshot diff. The
    // `fixture_fallthrough_inherit` and `fixture_fallthrough_root_inherit`
    // fixtures both trigger one of (a) or (b).
    match &analysis.fallthrough_surface {
        FallthroughSurface::None { reason } => match reason {
            NoFallthroughReason::InheritAttrsFalse => Some(FallthroughView {
                inherit_attrs: false,
                surface_signature: "{}".to_string(),
            }),
            // No-template / multi-root / etc.: not a meaningful
            // fallthrough surface, skip in the projection.
            _ => None,
        },
        FallthroughSurface::Branches { branches } => {
            let mut props_by_name: BTreeMap<String, FallthroughPropEntry> = BTreeMap::new();
            let mut events_by_name: BTreeMap<String, FallthroughEventEntry> = BTreeMap::new();
            let mut has_component_source = false;
            for branch in branches {
                for prop in &branch.props {
                    if prop
                        .sources
                        .iter()
                        .any(|s| matches!(s, InheritedSource::Component { .. }))
                    {
                        has_component_source = true;
                    }
                    props_by_name
                        .entry(prop.name.clone())
                        .or_insert_with(|| prop.clone());
                }
                for event in &branch.events {
                    if event
                        .sources
                        .iter()
                        .any(|s| matches!(s, InheritedSource::Component { .. }))
                    {
                        has_component_source = true;
                    }
                    events_by_name
                        .entry(event.name.clone())
                        .or_insert_with(|| event.clone());
                }
            }
            // Suppress the projection when the surface is purely
            // native (e.g., `<div />` root) AND inherit is on. Only
            // emit when the user opted out, OR when the inheritance
            // chain crosses a child component.
            if !analysis.flags.has_inherit_attrs_false && !has_component_source {
                return None;
            }
            let mut entries: Vec<String> = props_by_name
                .values()
                .map(|prop| fallthrough_prop_entry_signature(host, owner, prop))
                .chain(
                    events_by_name
                        .values()
                        .map(|event| fallthrough_event_entry_signature(host, owner, event)),
                )
                .collect();
            entries.sort();
            let surface_signature = format!("{{ {} }}", entries.join("; "));
            Some(FallthroughView {
                inherit_attrs: !analysis.flags.has_inherit_attrs_false,
                surface_signature,
            })
        }
    }
}

fn fallthrough_prop_entry_signature(
    host: &VerterHost,
    owner: &str,
    prop: &FallthroughPropEntry,
) -> String {
    format!(
        "{}{}: {}{}",
        prop.name,
        "",
        render_source_signature(host, owner, prop.type_source.present()),
        format_inherited_sources(&prop.sources),
    )
}

fn fallthrough_event_entry_signature(
    host: &VerterHost,
    owner: &str,
    event: &FallthroughEventEntry,
) -> String {
    format!(
        "@{}: {}{}",
        event.name,
        render_source_signature(host, owner, event.payload.present()),
        format_inherited_sources(&event.sources),
    )
}

fn format_inherited_sources(sources: &[InheritedSource]) -> String {
    if sources.is_empty() {
        return String::new();
    }
    let mut tags: Vec<String> = sources
        .iter()
        .map(|src| match src {
            InheritedSource::NativeTag { tag } => format!("native:{tag}"),
            InheritedSource::Component { canonical_id } => format!("component:{canonical_id}"),
        })
        .collect();
    tags.sort();
    format!(" /* from {} */", tags.join(", "))
}

// ═══════════════════════════════════════════════════════════════════════════
// Canonical type-signature rendering.
// ═══════════════════════════════════════════════════════════════════════════
//
// The renderer produces a deterministic string form of every
// `TypeExpr` variant. It is intentionally permissive about
// non-evaluated forms (Refs, Mapped, Conditional, IndexedAccess) —
// when the analysis pipeline does not collapse those to a flat
// shape, the renderer surfaces the structured form so the gate
// captures the difference. Hand-authored expected values in
// `expected.rs` use the same renderer's canonical form.

pub(crate) fn render_type_signature(expr: &TypeExpr) -> String {
    let mut buf = String::new();
    write_type_expr(&mut buf, expr);
    buf
}

fn write_type_expr(buf: &mut String, expr: &TypeExpr) {
    match expr {
        TypeExpr::Primitive(prim) => buf.push_str(primitive_name(*prim)),
        TypeExpr::Literal(lit) => write_literal(buf, lit),
        TypeExpr::Union(types) => {
            let parts: Vec<String> = types.iter().map(render_type_signature).collect();
            buf.push_str(&parts.join(" | "));
        }
        TypeExpr::Intersection(types) => {
            let parts: Vec<String> = types.iter().map(render_type_signature).collect();
            buf.push_str(&parts.join(" & "));
        }
        TypeExpr::Array { element, readonly } => {
            if *readonly {
                buf.push_str("readonly ");
            }
            let inner = render_type_signature(element);
            if needs_parens_for_array(element) {
                buf.push('(');
                buf.push_str(&inner);
                buf.push(')');
            } else {
                buf.push_str(&inner);
            }
            buf.push_str("[]");
        }
        TypeExpr::Tuple { elements, readonly } => {
            if *readonly {
                buf.push_str("readonly ");
            }
            buf.push('[');
            let mut first = true;
            for el in elements.iter() {
                if !first {
                    buf.push_str(", ");
                }
                first = false;
                if let Some(label) = &el.label {
                    buf.push_str(label);
                    if el.optional {
                        buf.push('?');
                    }
                    buf.push_str(": ");
                }
                if el.rest {
                    buf.push_str("...");
                }
                buf.push_str(&render_type_signature(&el.ty));
            }
            buf.push(']');
        }
        TypeExpr::Object(obj) => {
            // Sort named members alphabetically for stable rendering.
            // Note: this differs from declaration order, but the goal
            // is byte-deterministic snapshots — the discriminating
            // self-test catches semantic drift, not ordering.
            let mut named: Vec<(String, String, bool, bool)> = Vec::new();
            let mut other: Vec<String> = Vec::new();
            for member in &obj.properties {
                match member {
                    ObjectMember::Property(prop) => {
                        let key = prop.name.clone();
                        let value = render_type_signature(&prop.ty);
                        named.push((key, value, prop.optional, prop.readonly));
                    }
                    ObjectMember::IndexSignature(sig) => {
                        let v = render_type_signature(&sig.value_type);
                        let kt = render_type_signature(&sig.key_type);
                        let key_name = if sig.key_name.is_empty() {
                            "key"
                        } else {
                            &sig.key_name
                        };
                        other.push(format!("[{key_name}: {kt}]: {v}"));
                    }
                    ObjectMember::CallSignature(sig) => {
                        let mut s = String::new();
                        write_function(&mut s, sig);
                        other.push(s);
                    }
                    ObjectMember::ConstructSignature(sig) => {
                        let mut s = String::from("new ");
                        write_function(&mut s, sig);
                        other.push(s);
                    }
                    ObjectMember::Method(method) => {
                        let mut s = String::new();
                        s.push_str(&method.name);
                        if method.optional {
                            s.push('?');
                        }
                        write_function_after_name(&mut s, &method.function);
                        other.push(s);
                    }
                }
            }
            named.sort_by(|a, b| a.0.cmp(&b.0));
            let mut entries: Vec<String> = Vec::new();
            for (k, v, optional, readonly) in named {
                let mut s = String::new();
                if readonly {
                    s.push_str("readonly ");
                }
                s.push_str(&k);
                if optional {
                    s.push('?');
                }
                s.push_str(": ");
                s.push_str(&v);
                entries.push(s);
            }
            other.sort();
            entries.extend(other);
            if entries.is_empty() {
                buf.push_str("{}");
            } else {
                buf.push_str("{ ");
                buf.push_str(&entries.join("; "));
                buf.push_str(" }");
            }
        }
        TypeExpr::Function(sig) => {
            write_function(buf, sig);
        }
        // A constructor type renders as its function signature with a leading
        // `new ` so the snapshot distinguishes `new () => R` from `() => R`.
        TypeExpr::ConstructorType(sig) => {
            buf.push_str("new ");
            write_function(buf, sig);
        }
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            buf.push_str(name);
            if !type_arguments.is_empty() {
                buf.push('<');
                let parts: Vec<String> = type_arguments.iter().map(render_type_signature).collect();
                buf.push_str(&parts.join(", "));
                buf.push('>');
            }
        }
        TypeExpr::TypeParameter(param) => buf.push_str(&param.name),
        TypeExpr::KeyOf(inner) => {
            buf.push_str("keyof ");
            buf.push_str(&render_type_signature(inner));
        }
        TypeExpr::TypeOf(value_ref) => {
            buf.push_str("typeof ");
            buf.push_str(&value_ref.path.join("."));
        }
        TypeExpr::IndexedAccess { object, index } => {
            buf.push_str(&render_type_signature(object));
            buf.push('[');
            buf.push_str(&render_type_signature(index));
            buf.push(']');
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            buf.push_str(&render_type_signature(check));
            buf.push_str(" extends ");
            buf.push_str(&render_type_signature(extends));
            buf.push_str(" ? ");
            buf.push_str(&render_type_signature(true_type));
            buf.push_str(" : ");
            buf.push_str(&render_type_signature(false_type));
        }
        TypeExpr::Mapped {
            parameter,
            source,
            value,
            optional,
            readonly,
            name_type,
        } => {
            buf.push_str("{ ");
            match readonly {
                MappedModifier::Add => buf.push_str("readonly "),
                MappedModifier::Remove => buf.push_str("-readonly "),
                MappedModifier::None => {}
            }
            buf.push('[');
            buf.push_str(parameter);
            buf.push_str(" in ");
            buf.push_str(&render_type_signature(source));
            if let Some(name_ty) = name_type {
                buf.push_str(" as ");
                buf.push_str(&render_type_signature(name_ty));
            }
            buf.push(']');
            match optional {
                MappedModifier::Add => buf.push('?'),
                MappedModifier::Remove => buf.push_str("-?"),
                MappedModifier::None => {}
            }
            buf.push_str(": ");
            buf.push_str(&render_type_signature(value));
            buf.push_str(" }");
        }
        TypeExpr::TemplateLiteral {
            quasis,
            expressions,
        } => {
            buf.push('`');
            for (i, quasi) in quasis.iter().enumerate() {
                buf.push_str(quasi);
                if i < expressions.len() {
                    buf.push_str("${");
                    buf.push_str(&render_type_signature(&expressions[i]));
                    buf.push('}');
                }
            }
            buf.push('`');
        }
        TypeExpr::Infer { name } => {
            buf.push_str("infer ");
            buf.push_str(name);
        }
        TypeExpr::Rest(inner) => {
            buf.push_str("...");
            buf.push_str(&render_type_signature(inner));
        }
        TypeExpr::Parenthesized(inner) => {
            buf.push('(');
            buf.push_str(&render_type_signature(inner));
            buf.push(')');
        }
        TypeExpr::RecursiveRef {
            name,
            type_arguments,
            ..
        } => {
            // Drop conditional context (purely diagnostic — its
            // serialization is not stable across runs).
            buf.push_str("/*recursive*/ ");
            buf.push_str(name);
            if !type_arguments.is_empty() {
                buf.push('<');
                let parts: Vec<String> = type_arguments.iter().map(render_type_signature).collect();
                buf.push_str(&parts.join(", "));
                buf.push('>');
            }
        }
        TypeExpr::SyntheticSlotBinding(key) => {
            // Snapshot rendering for the synthetic slot-binding carrier:
            // display as the binding name (parser-equivalent surface),
            // matching the TS bridge's display contract.
            buf.push_str(key.binding_name.as_ref());
        }
        TypeExpr::ImportType {
            specifier,
            qualifier,
            typeof_query,
            type_arguments,
        } => {
            // Canonical TS surface: `typeof import("spec")` for a value-space
            // query, `import("spec")` otherwise, with the dotted qualifier and
            // any applied type arguments appended.
            if *typeof_query {
                buf.push_str("typeof ");
            }
            buf.push_str("import(\"");
            buf.push_str(specifier);
            buf.push_str("\")");
            for q in qualifier.iter() {
                buf.push('.');
                buf.push_str(q);
            }
            if !type_arguments.is_empty() {
                buf.push('<');
                let parts: Vec<String> = type_arguments.iter().map(render_type_signature).collect();
                buf.push_str(&parts.join(", "));
                buf.push('>');
            }
        }
        TypeExpr::Unknown { raw } => {
            // For Unknown, fall back to the raw source so the gate
            // can still distinguish "Verter could not lower X" from
            // "Verter could not lower Y".
            buf.push_str("/*unknown*/ ");
            buf.push_str(raw);
        }
    }
}

fn primitive_name(prim: PrimitiveName) -> &'static str {
    match prim {
        PrimitiveName::String => "string",
        PrimitiveName::Number => "number",
        PrimitiveName::Boolean => "boolean",
        PrimitiveName::Symbol => "symbol",
        PrimitiveName::BigInt => "bigint",
        PrimitiveName::Any => "any",
        PrimitiveName::Unknown => "unknown",
        PrimitiveName::Void => "void",
        PrimitiveName::Never => "never",
        PrimitiveName::Null => "null",
        PrimitiveName::Undefined => "undefined",
        PrimitiveName::Object => "object",
    }
}

fn write_literal(buf: &mut String, lit: &LiteralValue) {
    match lit {
        LiteralValue::String(value) => {
            // Use double-quoted form for stable rendering — matches
            // TS `"hello"` literal syntax.
            buf.push('"');
            buf.push_str(value);
            buf.push('"');
        }
        LiteralValue::Number(value) => {
            // Render integers without trailing `.0`; everything else
            // via Display (Rust's default).
            if value.fract() == 0.0 && value.is_finite() {
                buf.push_str(&format!("{}", *value as i64));
            } else {
                buf.push_str(&value.to_string());
            }
        }
        LiteralValue::Boolean(value) => buf.push_str(if *value { "true" } else { "false" }),
        LiteralValue::BigInt(value) => {
            buf.push_str(value);
            buf.push('n');
        }
    }
}

fn write_function(buf: &mut String, sig: &verter_type_expr::FunctionExpr) {
    write_type_parameters(buf, &sig.type_parameters);
    buf.push('(');
    let mut first = true;
    for (i, param) in sig.parameters.iter().enumerate() {
        if !first {
            buf.push_str(", ");
        }
        first = false;
        if param.rest {
            buf.push_str("...");
        }
        let pname: std::borrow::Cow<'_, str> = match &param.name {
            Some(n) if !n.is_empty() => std::borrow::Cow::Borrowed(n.as_str()),
            _ => std::borrow::Cow::Owned(format!("arg{i}")),
        };
        buf.push_str(&pname);
        if param.optional {
            buf.push('?');
        }
        buf.push_str(": ");
        buf.push_str(&render_type_signature(&param.ty));
    }
    buf.push(')');
    if let Some(ret) = &sig.return_type {
        buf.push_str(" => ");
        buf.push_str(&render_type_signature(ret));
    } else {
        buf.push_str(" => void");
    }
}

fn write_function_after_name(buf: &mut String, sig: &verter_type_expr::FunctionExpr) {
    write_type_parameters(buf, &sig.type_parameters);
    buf.push('(');
    let mut first = true;
    for (i, param) in sig.parameters.iter().enumerate() {
        if !first {
            buf.push_str(", ");
        }
        first = false;
        if param.rest {
            buf.push_str("...");
        }
        let pname: std::borrow::Cow<'_, str> = match &param.name {
            Some(n) if !n.is_empty() => std::borrow::Cow::Borrowed(n.as_str()),
            _ => std::borrow::Cow::Owned(format!("arg{i}")),
        };
        buf.push_str(&pname);
        if param.optional {
            buf.push('?');
        }
        buf.push_str(": ");
        buf.push_str(&render_type_signature(&param.ty));
    }
    buf.push(')');
    if let Some(ret) = &sig.return_type {
        buf.push_str(": ");
        buf.push_str(&render_type_signature(ret));
    } else {
        buf.push_str(": void");
    }
}

fn write_type_parameters(buf: &mut String, params: &[verter_type_expr::TypeParam]) {
    if params.is_empty() {
        return;
    }
    buf.push('<');
    let mut first = true;
    for p in params {
        if !first {
            buf.push_str(", ");
        }
        first = false;
        buf.push_str(&p.name);
        if let Some(constraint) = &p.constraint {
            buf.push_str(" extends ");
            buf.push_str(&render_type_signature(constraint));
        }
        if let Some(default_ty) = &p.default {
            buf.push_str(" = ");
            buf.push_str(&render_type_signature(default_ty));
        }
    }
    buf.push('>');
}

fn needs_parens_for_array(expr: &TypeExpr) -> bool {
    matches!(
        expr,
        TypeExpr::Union(_)
            | TypeExpr::Intersection(_)
            | TypeExpr::Function(_)
            // A constructor type `new () => R` needs parens as an array element
            // exactly like a function type.
            | TypeExpr::ConstructorType(_)
            | TypeExpr::Conditional { .. }
    )
}

// ───────────────────────────── Self-tests ─────────────────────────────

#[cfg(test)]
mod self_tests {
    use super::*;
    use std::sync::Arc;
    use verter_type_expr::{
        FunctionExpr, FunctionParam, ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName,
        TypeExpr,
    };

    fn obj(props: Vec<(&str, TypeExpr, bool)>) -> TypeExpr {
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: props
                .into_iter()
                .map(|(name, ty, optional)| {
                    ObjectMember::Property(ObjectProperty::synthetic_public(
                        name.to_string(),
                        ty,
                        optional,
                        false,
                    ))
                })
                .collect(),
        }))
    }

    #[test]
    fn renders_primitives_and_literals() {
        assert_eq!(
            render_type_signature(&TypeExpr::Primitive(PrimitiveName::String)),
            "string"
        );
        assert_eq!(
            render_type_signature(&TypeExpr::string_literal("hi")),
            "\"hi\""
        );
        assert_eq!(render_type_signature(&TypeExpr::number_literal(7.0)), "7");
        assert_eq!(
            render_type_signature(&TypeExpr::boolean_literal(true)),
            "true"
        );
    }

    #[test]
    fn renders_union_and_object_in_canonical_order() {
        let u = TypeExpr::Union(Arc::from(vec![
            TypeExpr::string_literal("a"),
            TypeExpr::string_literal("b"),
        ]));
        assert_eq!(render_type_signature(&u), "\"a\" | \"b\"");

        let o = obj(vec![
            ("beta", TypeExpr::Primitive(PrimitiveName::Number), false),
            ("alpha", TypeExpr::Primitive(PrimitiveName::String), false),
        ]);
        // Alphabetic ordering — mutation-detection still works
        // because the discriminating self-test injects extras with
        // distinct names.
        assert_eq!(render_type_signature(&o), "{ alpha: string; beta: number }",);
    }

    #[test]
    fn renders_array_with_parens_for_unions() {
        let inner = TypeExpr::Union(Arc::from(vec![
            TypeExpr::Primitive(PrimitiveName::String),
            TypeExpr::Primitive(PrimitiveName::Number),
        ]));
        let arr = TypeExpr::Array {
            element: Arc::new(inner),
            readonly: false,
        };
        assert_eq!(render_type_signature(&arr), "(string | number)[]");
    }

    #[test]
    fn renders_function_signature_with_void_default() {
        let func = TypeExpr::Function(Arc::new(FunctionExpr::synthetic(
            vec![FunctionParam::synthetic(
                Some("value".to_string()),
                TypeExpr::Primitive(PrimitiveName::Number),
                false,
                false,
            )],
            None,
            Vec::new(),
        )));
        assert_eq!(render_type_signature(&func), "(value: number) => void");
    }

    // Negative: same shape under different ordering must still
    // produce IDENTICAL signatures (canonical-by-construction).
    #[test]
    fn object_field_order_is_irrelevant() {
        let a = obj(vec![
            ("a", TypeExpr::Primitive(PrimitiveName::String), false),
            ("b", TypeExpr::Primitive(PrimitiveName::Number), false),
        ]);
        let b = obj(vec![
            ("b", TypeExpr::Primitive(PrimitiveName::Number), false),
            ("a", TypeExpr::Primitive(PrimitiveName::String), false),
        ]);
        assert_eq!(render_type_signature(&a), render_type_signature(&b));
    }
}
