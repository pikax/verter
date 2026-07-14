//! The GENERATED differential-parity oracle.
//!
//! A DETERMINISTIC generated differential harness: the committed generated
//! corpus (`tests/svelte_oracle_corpus/{fixtures,goldens}/generated`, produced by
//! `scripts/gen-svelte-diff-corpus.mjs`) pins the official `svelte@5.56.3`
//! NORMALIZED topology across a combinatorial axis enumeration, and this matrix
//! projects Verter's runtime IR into the SAME normalized schema and diffs them.
//!
//! The EXPANDED schema (over the hand-vendored helper/skeleton/import axes) covers:
//!
//! - `events` — per registered event: type + target-kind + delegation-kind. The
//!   delegation is Verter's ACTUAL `AttrIr::Event` flag for EVERY host (element,
//!   component, `<svelte:element>`, window/body/document) — never the official
//!   `forwarded_prop` answer re-derived from a host-kind rule.
//! - `nonStaticProperties` — the `cannot_be_set_statically` set: name + kind
//!   (autofocus / dom_property) + the value chunk-topology of the assigned RHS.
//! - `attrParts` — per dynamic / mixed attribute the value-part topology.
//! - `directiveExprs` — per `class:`/`style:`/`bind:`/`use:`/`on:` directive the
//!   inner-expression SHAPE (object / expr / none) Verter's stored expr source has.
//! - `delegatedEvents` — the module's collected `$.delegate([...])` set.
//! - `helperSet` — the owned structural-helper set (intersected with the owned
//!   universe).
//! - `staticHtml` — the serialized clone-template HTML bytes (factory-family
//!   independent, so an entity-decode / serialization divergence is isolated).
//! - `factory` — the clone-template factory family Verter emits (`from_html` /
//!   `from_tree`); a svg / mathml root diverges from official's `from_svg` /
//!   `from_mathml` (a recorded divergence — svg/mathml emission is deferred).
//! - `decodedText` — the text-first `$.text` seed: Verter's RAW seed vs official's
//!   entity-decoded seed.
//! - `nodePaths` — per region, the multiset of node-path step sequences.
//! - `dynamicSlots` — per-slot-kind dynamic-surface counts.
//!
//! The candidate projection (`project_*`) is a FAITHFUL READ-ONLY projection of
//! the EXISTING runtime IR + static-template plan + topology plan — it changes no
//! production behavior; it reads what `lower_parsed_svelte_to_ir` +
//! `plan_static_templates` + `plan_client_topology` already produce and
//! re-expresses it in the golden's normalized shape. Where Verter's IR genuinely
//! diverges from official, the projection reflects Verter's ACTUAL state (it is
//! NEVER pre-corrected to match official) — that is exactly how a divergence
//! surfaces as a failing axis (the masked-projector defect this harness removed).
//!
//! ## The honest allow-list (this phase ENUMERATES, does not fix)
//!
//! The matrix asserts every generated `(fixture, axis)` pair EXCEPT the pairs on
//! [`KNOWN_DIVERGENCES`] — the enumerated long tail of real divergences. Every
//! allow-list row is GUARDED by [`known_divergences_are_real`]: a row whose
//! `(fixture, axis)` no longer diverges FAILS the guard (a stale row cannot
//! linger), mirroring the hand-vendored `deferral_ledger_rows_are_justified_and_real`
//! pattern. The allow-list is grouped by ROOT CAUSE and mapped to the Y-labels.

use super::super::parser::parse_svelte;
use super::html::{
    cannot_be_set_statically, DynamicSlotKind, NodePathPlan, NodePathStep, PathBase,
    StaticTemplatePlan, TemplateFactory,
};
use super::ir::{
    AttrIr, BlockIr, ExprId, IrNode, MixedAttrPart, NodeId, NonStaticPropertyKind,
    NonStaticPropertyValue, RuntimeOp, SpecialKind, StyleDirectiveValue, SvelteRuntimeIr,
    TemplateScopeId,
};
use super::topology::plan_client_topology;
use super::SvelteFragments;
use super::{lower_parsed_svelte_to_ir, plan_static_templates, SvelteRuntimeOptions};
use oxc_allocator::Allocator;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Golden deserialization (the EXPANDED generated-corpus schema)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, PartialEq, Eq, Clone)]
struct GoldenEvent {
    #[serde(rename = "type")]
    event_type: String,
    target: String,
    delegation: String,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone)]
struct GoldenNonStaticProperty {
    name: String,
    kind: String,
    /// The VALUE chunk-topology of the assigned RHS (`["literal"]` for a static
    /// literal, `["literal","expr","literal"]` for a mixed template-literal value,
    /// `["boolean"]` for a valueless boolean, `["expr"]` for any other expression).
    #[serde(default)]
    value: Vec<String>,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone)]
struct GoldenDirectiveExpr {
    kind: String,
    shape: String,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone)]
struct GoldenTemplate {
    factory: String,
    #[serde(default)]
    html: String,
    #[serde(default)]
    flag: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone)]
struct GoldenAttrPart {
    helper: String,
    attr: String,
    chunks: Vec<String>,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone)]
struct GoldenNodePath {
    base: String,
    steps: Vec<String>,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone)]
struct GoldenRegion {
    paths: Vec<GoldenNodePath>,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Default)]
struct GoldenNodePaths {
    regions: Vec<GoldenRegion>,
}

#[derive(Debug, Deserialize)]
struct GeneratedGolden {
    slug: String,
    #[serde(default)]
    events: Vec<GoldenEvent>,
    #[serde(rename = "nonStaticProperties", default)]
    non_static_properties: Vec<GoldenNonStaticProperty>,
    #[serde(rename = "attrParts", default)]
    attr_parts: Vec<GoldenAttrPart>,
    #[serde(rename = "directiveExprs", default)]
    directive_exprs: Vec<GoldenDirectiveExpr>,
    #[serde(rename = "decodedText", default)]
    decoded_text: Vec<String>,
    #[serde(rename = "delegatedEvents", default)]
    delegated_events: Vec<String>,
    #[serde(rename = "helperSet", default)]
    helper_set: Vec<String>,
    #[serde(default)]
    templates: Vec<GoldenTemplate>,
    #[serde(rename = "nodePaths", default)]
    node_paths: GoldenNodePaths,
    #[serde(rename = "dynamicSlots", default)]
    dynamic_slots: BTreeMap<String, u32>,
}

fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/svelte_oracle_corpus")
}

fn generated_fixtures_dir() -> PathBuf {
    corpus_root().join("fixtures/generated")
}

fn generated_goldens_dir() -> PathBuf {
    corpus_root().join("goldens/generated")
}

/// Discover every generated fixture slug (`generated/NNN_label.svelte`), sorted.
fn generated_corpus() -> Vec<String> {
    let dir = generated_fixtures_dir();
    let mut out: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read generated fixtures dir {}: {e}", dir.display()))
        .map(|e| e.expect("dir entry").path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("svelte"))
        .map(|p| format!("generated/{}", p.file_name().unwrap().to_string_lossy()))
        .collect();
    out.sort();
    assert!(
        !out.is_empty(),
        "the generated corpus is empty — run `node scripts/gen-svelte-diff-corpus.mjs`"
    );
    out
}

fn load_generated_golden(slug: &str) -> GeneratedGolden {
    let name = slug.strip_prefix("generated/").unwrap_or(slug);
    let path = generated_goldens_dir().join(format!("{}.client.json", name.replace(".svelte", "")));
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read generated golden {}: {e}", path.display()));
    serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("parse generated golden {}: {e}", path.display()))
}

fn load_generated_fixture(slug: &str) -> String {
    let name = slug.strip_prefix("generated/").unwrap_or(slug);
    let path = generated_fixtures_dir().join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read generated fixture {}: {e}", path.display()))
}

/// Lower a generated fixture to its IR + static plan (the candidate substrate).
fn lower_generated<'a>(
    source: &'a str,
    alloc: &'a Allocator,
) -> (SvelteRuntimeIr<'a>, StaticTemplatePlan) {
    let parsed = parse_svelte(source);
    let ir = lower_parsed_svelte_to_ir(source, &parsed, &SvelteRuntimeOptions::default(), alloc)
        .expect("generated fixture lowers");
    let plan = plan_static_templates(&ir, None);
    (ir, plan)
}

// ---------------------------------------------------------------------------
// The candidate projection — Verter's IR → the normalized golden schema.
//
// READ-ONLY: every `project_*` reads the IR / static plan and re-expresses it in
// the golden's normalized shape. It is NOT pre-corrected to match official; it
// reflects Verter's ACTUAL behavior, so a genuine divergence surfaces.
// ---------------------------------------------------------------------------

/// One normalized candidate event (mirrors [`GoldenEvent`]).
#[derive(Debug, Clone, PartialEq, Eq)]
struct CandEvent {
    event_type: String,
    target: String,
    delegation: String,
}

/// The delegation label Verter's IR produces for an `AttrIr::Event`: the IR's
/// `delegated`/`capture` flags decide. A capture handler is direct (never
/// delegated); otherwise `delegated` decides.
///
/// This is a FAITHFUL read of Verter's IR. A regular intrinsic element's `on*` is
/// an `AttrIr::Event` (delegated iff `can_delegate_event`); a window/body/document
/// `on*` is an `AttrIr::Event` with `delegated = false` (a direct global listener).
/// A `<Component>` / `<svelte:element>` `on*` is NOT an `AttrIr::Event` at all (it
/// lowers to a forwarded prop / an `$.attribute_effect` attribute) — see
/// [`project_events`] for how those are projected.
fn ir_event_delegation(delegated: bool, capture: bool) -> &'static str {
    if delegated && !capture {
        "delegated"
    } else {
        "direct"
    }
}

/// Whether an attribute NAME is an event-attribute name the official forwarded-prop
/// extractor reports (`on<lowercase>` — the `/^on([a-z]+)$/` shape the golden's
/// `extractForwardedPropEvents` keys on). The TYPE is `name[2..]`.
fn forwarded_event_type(name: &str) -> Option<&str> {
    let rest = name.strip_prefix("on")?;
    if !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_lowercase()) {
        Some(rest)
    } else {
        None
    }
}

/// Project the registered EVENTS from the IR, FAITHFULLY reflecting how each host
/// lowers an `on*` handler:
///
/// - a regular intrinsic element's `on*` is an `AttrIr::Event` → projected with its
///   actual `delegated`/`capture` flag (`delegated` / `direct`), target `element`;
/// - a window/body/document `on*` is an `AttrIr::Event` with `delegated = false` →
///   projected `direct`, target `window`/`body`/`document`;
/// - a `<Component>` `on*` is a FORWARDED PROP — Verter's IR carries it as an
///   `AttrIr::Dynamic` / `AttrIr::Mixed` under its ORIGINAL `on<name>` name; it is
///   projected as a `forwarded_prop` event (mirroring the official golden's
///   `extractForwardedPropEvents`, which reads the `on<name>:` key of a component
///   call's props object), target `element`;
/// - a `<svelte:element>` `on*` rides `$.attribute_effect` — Verter's IR carries it
///   as an `AttrIr::Dynamic` attribute; it is NOT an event (official emits no
///   `$.event`/forwarded-prop for it), so it contributes NO event row.
///
/// FAITHFUL: it reads Verter's ACTUAL stored attribute variant per host; it never
/// re-derives the official answer for an `AttrIr::Event`.
fn project_events(ir: &SvelteRuntimeIr) -> Vec<CandEvent> {
    let mut out = Vec::new();
    walk_nodes(ir, ir.root, &mut |node, parent| {
        let _ = parent;
        match node {
            IrNode::Element(el) => collect_event_attrs(&el.attrs, "element", &mut out),
            IrNode::Special(s) => {
                let target = match s.kind {
                    SpecialKind::Window => "window",
                    SpecialKind::Body => "body",
                    SpecialKind::Document => "document",
                    // `<svelte:element>` lowers to a real element — element target.
                    _ => "element",
                };
                collect_event_attrs(&s.attrs, target, &mut out);
            }
            IrNode::Component(c) => {
                // A component's `on*` handler is a FORWARDED PROP (an
                // `AttrIr::Dynamic`/`Mixed` under the `on<name>` name) — project it
                // as a `forwarded_prop` event, mirroring the golden extractor.
                for attr in &c.attrs {
                    let name = match attr {
                        AttrIr::Dynamic { name, .. } | AttrIr::Mixed { name, .. } => name.as_str(),
                        _ => continue,
                    };
                    if let Some(ty) = forwarded_event_type(name) {
                        out.push(CandEvent {
                            event_type: ty.to_string(),
                            target: "element".into(),
                            delegation: "forwarded_prop".into(),
                        });
                    }
                }
            }
            _ => {}
        }
    });
    out.sort_by(|a, b| {
        a.event_type
            .cmp(&b.event_type)
            .then(a.target.cmp(&b.target))
            .then(a.delegation.cmp(&b.delegation))
    });
    out
}

/// Collect the `AttrIr::Event` rows of one DOM host (element / global special) into
/// `out`, with the given target label and Verter's actual delegation flag.
fn collect_event_attrs(attrs: &[AttrIr], target: &str, out: &mut Vec<CandEvent>) {
    for attr in attrs {
        if let AttrIr::Event {
            event_type,
            delegated,
            capture,
            ..
        } = attr
        {
            out.push(CandEvent {
                event_type: event_type.clone(),
                target: target.into(),
                delegation: ir_event_delegation(*delegated, *capture).into(),
            });
        }
    }
}

/// One normalized candidate non-static property (mirrors [`GoldenNonStaticProperty`]).
#[derive(Debug, Clone, PartialEq, Eq)]
struct CandNonStaticProperty {
    name: String,
    kind: String,
    value: Vec<String>,
}

/// Project the NON-STATIC properties from the IR's runtime ops, INCLUDING the
/// value chunk-topology. The value is projected FAITHFULLY from Verter's
/// [`NonStaticPropertyValue`]: a `Boolean` → `["boolean"]`, a `Literal` →
/// `["literal"]`, and a single `Expr` → `["expr"]`. Verter collapses a MIXED
/// value (`defaultValue="a {x} b"`) to a SINGLE `Expr` — so its candidate value
/// is `["expr"]`, which diverges from official's preserved `["literal","expr",
/// "literal"]` alternation. The projector reads Verter's ACTUAL stored value
/// variant; it does NOT reconstruct the official literal/expr alternation.
fn project_non_static_properties(ir: &SvelteRuntimeIr) -> Vec<CandNonStaticProperty> {
    let mut out = Vec::new();
    for op in &ir.ops {
        if let RuntimeOp::NonStaticProperty { property, .. } = op {
            let kind = match property.kind {
                NonStaticPropertyKind::Autofocus => "autofocus",
                NonStaticPropertyKind::DomProperty => "dom_property",
            };
            let value = match &property.value {
                NonStaticPropertyValue::Boolean => vec!["boolean".to_string()],
                NonStaticPropertyValue::Literal(_) => vec!["literal".to_string()],
                NonStaticPropertyValue::Expr(_) => vec!["expr".to_string()],
                // A mixed value carries its FULL ordered literal/expr alternation —
                // project the chunk kinds faithfully (matching official's
                // `["literal","expr","literal"]` for `defaultValue="a {x} b"`).
                NonStaticPropertyValue::Mixed(parts) => mixed_chunks(parts),
            };
            out.push(CandNonStaticProperty {
                name: property.name.clone(),
                kind: kind.into(),
                value,
            });
        }
    }
    out.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then(a.kind.cmp(&b.kind))
            .then(a.value.join(",").cmp(&b.value.join(",")))
    });
    out
}

/// One normalized candidate attribute-value-part row (mirrors [`GoldenAttrPart`]).
#[derive(Debug, Clone, PartialEq, Eq)]
struct CandAttrPart {
    helper: String,
    attr: String,
    chunks: Vec<String>,
}

/// Project the dynamic / mixed ATTRIBUTE VALUE-PART topology from the IR. The
/// helper is derived from the attribute name the way official routes it: `class`
/// → `set_class`, `style` → `set_style`, `value` → `set_value`, anything else →
/// `set_attribute`. Chunks: a `Dynamic` attribute is a single `expr` chunk; a
/// `Mixed` attribute is the literal/expr alternation of its parts; a `Class` /
/// `Style` directive is a `directive` chunk under the typed setter.
fn project_attr_parts(ir: &SvelteRuntimeIr) -> Vec<CandAttrPart> {
    let mut out = Vec::new();
    walk_nodes(ir, ir.root, &mut |node, _| {
        let attrs: &[AttrIr] = match node {
            IrNode::Element(el) => &el.attrs,
            IrNode::Component(c) => &c.attrs,
            IrNode::Special(s) => &s.attrs,
            _ => return,
        };
        // A component routes its props through the component CALL, not a DOM
        // setter, so it contributes NO attr-part rows (mirroring official's
        // `Foo($$anchor, { … })`). Only DOM hosts (element / special-element)
        // emit `$.set_*` value setters.
        if matches!(node, IrNode::Component(_)) {
            return;
        }
        for attr in attrs {
            match attr {
                // A `cannot_be_set_statically` attribute (`defaultValue` / `muted` /
                // …) is a non-static PROPERTY write (captured on the
                // `non_static_properties` axis), NOT a DOM `$.set_*` value-setter —
                // official emits `node.defaultValue = …`, no `set_attribute`. Skip it
                // here so it does not double-count as an attr-part.
                AttrIr::Dynamic { name, .. } | AttrIr::Mixed { name, .. }
                    if cannot_be_set_statically(name) => {}
                AttrIr::Dynamic { name, .. } => {
                    out.push(CandAttrPart {
                        helper: setter_for(name).into(),
                        attr: typed_attr(name),
                        chunks: vec!["expr".into()],
                    });
                }
                AttrIr::Mixed { name, parts } => {
                    out.push(CandAttrPart {
                        helper: setter_for(name).into(),
                        attr: typed_attr(name),
                        chunks: mixed_chunks(parts),
                    });
                }
                AttrIr::Class { .. } => {
                    out.push(CandAttrPart {
                        helper: "set_class".into(),
                        attr: "class".into(),
                        chunks: vec!["directive".into()],
                    });
                }
                AttrIr::Style { .. } => {
                    out.push(CandAttrPart {
                        helper: "set_style".into(),
                        attr: "style".into(),
                        chunks: vec!["directive".into()],
                    });
                }
                _ => {}
            }
        }
    });
    out.sort_by(|a, b| {
        a.helper
            .cmp(&b.helper)
            .then(a.attr.cmp(&b.attr))
            .then(a.chunks.join(",").cmp(&b.chunks.join(",")))
    });
    out
}

/// The official value-setter helper for an attribute name.
fn setter_for(name: &str) -> &'static str {
    match name {
        "class" => "set_class",
        "style" => "set_style",
        "value" => "set_value",
        _ => "set_attribute",
    }
}

/// The normalized `attr` field for a setter row: the typed setters report their
/// fixed surface (`class`/`style`/`value`); `set_attribute` reports the name.
fn typed_attr(name: &str) -> String {
    match name {
        "class" => "class".into(),
        "style" => "style".into(),
        "value" => "value".into(),
        other => other.to_string(),
    }
}

/// The literal/expr chunk kinds of a mixed attribute value.
fn mixed_chunks(parts: &[MixedAttrPart]) -> Vec<String> {
    parts
        .iter()
        .map(|p| match p {
            MixedAttrPart::Literal(_) => "literal".to_string(),
            MixedAttrPart::Expr(_) => "expr".to_string(),
        })
        .collect()
}

/// One normalized candidate node-path (mirrors [`GoldenNodePath`]).
#[derive(Debug, Clone, PartialEq, Eq)]
struct CandNodePath {
    base: String,
    steps: Vec<String>,
}

/// Project the per-region NODE-PATH multiset from the static plan's
/// `client_paths`. Each path's base is `fragment` (descends from the region's
/// cloned fragment) or `node` (descends from another named node); the steps are
/// the ORDERED step KINDS (a `Text` materialize step is dropped — it is a
/// backend text-node strategy, not a DOM-walk hop, and the official extractor
/// likewise records only descent steps).
fn project_node_paths(plan: &StaticTemplatePlan) -> Vec<Vec<CandNodePath>> {
    // Group paths by region (scope) so the per-region multiset matches the
    // golden's `regions`.
    let mut by_region: BTreeMap<u32, Vec<CandNodePath>> = BTreeMap::new();
    for path in &plan.client_paths {
        let steps = path_step_kinds(path);
        if steps.is_empty() {
            continue;
        }
        let base = match path.base {
            PathBase::Fragment => "fragment".to_string(),
            PathBase::Node(_) => "node".to_string(),
        };
        by_region
            .entry(scope_index(path.scope))
            .or_default()
            .push(CandNodePath { base, steps });
    }
    let mut regions: Vec<Vec<CandNodePath>> = by_region
        .into_values()
        .map(|mut paths| {
            paths.sort_by(|a, b| {
                a.base
                    .cmp(&b.base)
                    .then(a.steps.join(">").cmp(&b.steps.join(">")))
            });
            paths
        })
        .filter(|paths| !paths.is_empty())
        .collect();
    // Stable region order: sort by the serialized path-set, matching the golden.
    regions.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
    regions
}

/// The descent step KINDS of a node-path (the `Text` materialize step dropped).
fn path_step_kinds(path: &NodePathPlan) -> Vec<String> {
    path.steps
        .iter()
        .filter_map(|s| match s {
            NodePathStep::FirstChild => Some("first_child".to_string()),
            NodePathStep::Child { .. } => Some("child".to_string()),
            NodePathStep::Sibling { .. } => Some("sibling".to_string()),
            // `reset` / `next` are cursor ops, not descent hops; `text` is a
            // text-node materialize, not a DOM walk — none are descent steps.
            NodePathStep::Reset | NodePathStep::Next | NodePathStep::Text { .. } => None,
        })
        .collect()
}

fn scope_index(scope: TemplateScopeId) -> u32 {
    scope.0
}

/// Project the per-slot-KIND dynamic-surface counts from the static plan's slots,
/// restricted to the CLIENT DOM-realized surfaces the golden's
/// `extractDynamicSlotCounts` measures (it keys off emitted `$.set_*` / `$.bind_*` /
/// `$.attribute_effect` / block helpers). A COMPONENT's dynamic props / events /
/// binds are realized as a component CALL argument (`Foo($$anchor, { … })`), NOT a
/// DOM setter — so Verter's plan slots that target a component node contribute NO
/// client DOM slot here (mirroring [`project_attr_parts`], which excludes components
/// for the same reason). The plan-level component slots are an SSR-surface concern,
/// not a client DOM surface. FAITHFUL: it reports the slots Verter's CLIENT backend
/// would realize as DOM surfaces, excluding the component-call-routed props.
fn project_dynamic_slots(ir: &SvelteRuntimeIr, plan: &StaticTemplatePlan) -> BTreeMap<String, u32> {
    let mut counts: BTreeMap<String, u32> = BTreeMap::new();
    for slot in &plan.slots {
        // A slot targeting a component node routes through the component call, not a
        // DOM setter — it is not a client DOM surface.
        if matches!(ir.node(slot.node), IrNode::Component(_)) {
            continue;
        }
        // A `cannot_be_set_statically` ATTRIBUTE slot (`defaultValue` / `muted` / …)
        // is realized as a non-static PROPERTY write (the `non_static_properties`
        // axis), NOT a DOM attribute slot — official emits `node.defaultValue = …`,
        // not `$.set_attribute`. Skip it so it does not double-count as a DOM slot.
        if let DynamicSlotKind::Attribute { name, .. } = &slot.kind {
            if cannot_be_set_statically(name) {
                continue;
            }
        }
        let kind = match &slot.kind {
            DynamicSlotKind::Text { .. } => "text",
            DynamicSlotKind::Html { .. } => "html",
            DynamicSlotKind::Attribute { .. } => "attribute",
            DynamicSlotKind::Class { .. } => "class",
            DynamicSlotKind::Style { .. } => "style",
            DynamicSlotKind::Spread { .. } => "spread",
            DynamicSlotKind::Bind { .. } => "bind",
            DynamicSlotKind::Block => "block",
        };
        *counts.entry(kind.to_string()).or_insert(0) += 1;
    }
    counts
}

/// Project the module's DELEGATED event-type set — the set Verter's topology
/// planner ACTUALLY collects (`plan_client_topology(...).delegated_events`), as a
/// sorted list. The planner registers a delegated type only for an `AttrIr::Event`
/// with `delegated = true`, which host-aware lowering sets exclusively on a regular
/// element (a component `on*` lowers to a forwarded `AttrIr::Dynamic` prop, a
/// `<svelte:element>`/global target to a non-delegated event), so a component-only
/// event correctly yields an empty set — matching official's empty `$.delegate([...])`.
/// FAITHFUL: it reads Verter's collected set, never the official rule.
fn project_delegated_events(ir: &SvelteRuntimeIr, plan: &StaticTemplatePlan) -> Vec<String> {
    let topo = plan_client_topology(ir, plan, None);
    let mut out: Vec<String> = topo.delegated_events.ordered().to_vec();
    out.sort();
    out
}

/// Project the OWNED structural-helper SET Verter's topology planner records
/// (`plan_client_topology(...).helpers.helper_set()`), as sorted `$.`-idents,
/// RESTRICTED to the owned universe (the planner records some helpers that the
/// official compiler routes as backend DOM-walk strategy — notably the text-first
/// root `$.text` factory — and the universe excludes those so BOTH sides compare
/// like-for-like, mirroring the hand-vendored matrix's `OWNED_STRUCTURAL_HELPERS`
/// intersection). FAITHFUL: it reports the OWNED helpers Verter's planner actually
/// called — a component-event `delegated` / `delegate` leak, or a window/comment
/// region helper Verter plans where official folds it, surfaces here.
fn project_helper_set(ir: &SvelteRuntimeIr, plan: &StaticTemplatePlan) -> Vec<String> {
    let topo = plan_client_topology(ir, plan, None);
    let mut out: Vec<String> = topo
        .helpers
        .helper_set()
        .iter()
        .map(|h| h.ident().to_string())
        .filter(|h| OWNED_HELPER_UNIVERSE.contains(&h.as_str()))
        .collect();
    out.sort();
    out
}

/// The OWNED structural-helper universe — the helpers the helper-set axis compares
/// on BOTH sides. EXCLUDES the fine-grained DOM-walk helpers (`first_child` /
/// `child` / `sibling` / `reset` / `next` / `text`) and the script read-rewrite
/// helpers (`get` / `set` / `template_effect` / `set_text` / `state` / `proxy` /
/// …) — those are the emitting backend's, never planned by the topology, so the
/// official set is restricted to this universe too (mirrors the hand-vendored
/// `OWNED_STRUCTURAL_HELPERS`). The text-first root `$.text` factory IS recorded by
/// Verter's planner but is excluded here (it is the `$.text` DOM family official
/// also uses for interior text nodes, so comparing it would conflate the root
/// factory with backend interior-text strategy).
const OWNED_HELPER_UNIVERSE: &[&str] = &[
    "from_html",
    "comment",
    "append",
    "if",
    "each",
    "await",
    "key",
    "html",
    "snippet",
    "delegated",
    "event",
    "delegate",
    "head",
    "bind_this",
    "bind_value",
    "attribute_effect",
];

/// The golden helper set restricted to the owned universe (so the helper-set axis
/// compares like-for-like against [`project_helper_set`]).
fn golden_owned_helper_set(golden: &GeneratedGolden) -> Vec<String> {
    let mut out: Vec<String> = golden
        .helper_set
        .iter()
        .filter(|h| OWNED_HELPER_UNIVERSE.contains(&h.as_str()))
        .cloned()
        .collect();
    out.sort();
    out
}

/// Project the serialized static-HTML BYTES of each clone-template region, in plan
/// order, as a sorted multiset (each region's normalized `html` string). This axis
/// compares the serialized template STRING irrespective of the factory FAMILY (the
/// `from_svg` / `from_mathml` family choice is the separate [`project_factory_kinds`]
/// axis) — so a `<svg>` root whose html BYTES match official's `from_svg` html does
/// NOT double-count a divergence here. FAITHFUL: it serializes Verter's ACTUAL
/// `TemplateFactory::FromHtml { html }` bytes (entity-preservation, void-element
/// shape, whitespace), so an entity-decode / serialization divergence in the
/// template string surfaces here while a pure factory-family divergence does not.
fn project_static_html(plan: &StaticTemplatePlan) -> Vec<String> {
    let mut out: Vec<String> = plan
        .templates
        .iter()
        .filter_map(|t| match t {
            TemplateFactory::FromHtml { html, .. } => Some(html.clone()),
            _ => None,
        })
        .collect();
    out.sort();
    out
}

/// The golden's static-HTML byte multiset: the `html` of EVERY golden clone-
/// template row (every `from_*` factory family — `from_html` / `from_svg` /
/// `from_mathml` / `from_tree`), sorted. Comparing the html bytes across all clone
/// families isolates a serialization / entity-decode divergence from a pure
/// factory-family divergence (an SVG root's bytes match even though its factory
/// family differs).
fn golden_static_html(golden: &GeneratedGolden) -> Vec<String> {
    let mut out: Vec<String> = golden
        .templates
        .iter()
        .filter(|t| t.factory.starts_with("from_"))
        .map(|t| t.html.clone())
        .collect();
    out.sort();
    out
}

/// Project the CLONE-TEMPLATE factory family of each region, in plan order, as a
/// sorted multiset. The golden `templates` array (the official `$.from_html` /
/// `$.from_tree` clone factories) is the comparison subject, so ONLY Verter's
/// `TemplateFactory::FromHtml` (Verter's sole clone-template family) is projected
/// here — `text` / `comment` / `standalone` regions emit NO clone-template factory
/// (matching official, whose `$.text` / `$.comment` / standalone-mount emit no
/// `templates` row), so they contribute nothing on this axis. Roots are always
/// html-namespaced (a non-`html` namespace is refused at the resolver, and svg /
/// mathml elements fail closed), so the html-fragments family is always `from_html`;
/// a `<svelte:element>` root (Verter clones a `<!>` `from_html`, official uses the
/// `$.element` wrapper with no clone) surfaces as a `from_html` against official's
/// empty templates.
fn project_factory_kinds(plan: &StaticTemplatePlan) -> Vec<String> {
    let mut out: Vec<String> = plan
        .templates
        .iter()
        .filter_map(|t| match t {
            TemplateFactory::FromHtml { fragments, .. } => Some(
                match fragments {
                    SvelteFragments::Tree => "from_tree",
                    SvelteFragments::Html => "from_html",
                }
                .to_string(),
            ),
            // Not a clone-template factory — official's `templates` array likewise
            // excludes its equivalent (`$.text` / `$.comment` / standalone mount).
            TemplateFactory::TextNode { .. }
            | TemplateFactory::CommentAnchor { .. }
            | TemplateFactory::Standalone { .. } => None,
        })
        .collect();
    out.sort();
    out
}

/// The golden's clone-template factory-kind multiset — the `factory` of every
/// golden `templates` row (only the `from_*` clone factories the official
/// `templates` array carries; official `$.text` / `$.comment` / standalone-mount
/// regions emit no `templates` row, matching [`project_factory_kinds`]).
fn golden_factory_kinds(golden: &GeneratedGolden) -> Vec<String> {
    let mut out: Vec<String> = golden.templates.iter().map(|t| t.factory.clone()).collect();
    out.sort();
    out
}

/// Project the DECODED-TEXT seeds of every text-first region
/// (`TemplateFactory::TextNode { seed: Some(text) }`), as a sorted multiset of the
/// seed STRINGS. FAITHFUL: the seed is the text Verter stores after text-context
/// entity decoding — a `&copy;` source lowers to the decoded `©` seed here, matching
/// official. A dynamic text-first node (`seed: None`) carries no seed (matching
/// official's empty `$.text()`).
fn project_decoded_text(plan: &StaticTemplatePlan) -> Vec<String> {
    let mut out: Vec<String> = plan
        .templates
        .iter()
        .filter_map(|t| match t {
            TemplateFactory::TextNode { seed: Some(text) } => Some(text.clone()),
            _ => None,
        })
        .collect();
    out.sort();
    out
}

/// One normalized candidate directive inner-expression shape (mirrors
/// [`GoldenDirectiveExpr`]).
#[derive(Debug, Clone, PartialEq, Eq)]
struct CandDirectiveExpr {
    kind: String,
    shape: String,
}

/// Project the directive inner-expression SHAPE of every `class:`/`style:`/
/// `bind:`/`use:`/`on:` directive, from the directive value expression's ACTUAL
/// source text. The shape is `object` when Verter's stored expr source is a
/// brace-wrapped object literal `{…}`, `none` for a value-less directive, `expr`
/// otherwise. FAITHFUL: it reads Verter's stored expression `source` — a QUOTED
/// `class:active="{dx}"` / `bind:value="{dv}"` / `use:fn="{foo}"` keeps the braces
/// in Verter's IR (an object literal), so its candidate shape is `object`, which
/// diverges from official's unwrapped `expr`. The event path correctly unwraps a
/// quoted handler, so a quoted `onclick="{h}"` reports `expr` (no divergence).
fn project_directive_exprs(ir: &SvelteRuntimeIr) -> Vec<CandDirectiveExpr> {
    let mut out = Vec::new();
    let shape_of = |e: Option<ExprId>| -> &'static str {
        match e {
            None => "none",
            Some(id) => {
                let src = ir.analysis.expressions.get(id).source.trim();
                if src.starts_with('{') && src.ends_with('}') {
                    "object"
                } else {
                    "expr"
                }
            }
        }
    };
    walk_nodes(ir, ir.root, &mut |node, _| {
        let attrs: &[AttrIr] = match node {
            IrNode::Element(el) => &el.attrs,
            IrNode::Component(c) => &c.attrs,
            IrNode::Special(s) => &s.attrs,
            _ => return,
        };
        for attr in attrs {
            let (kind, shape) = match attr {
                AttrIr::Class { condition, .. } => ("class", shape_of(*condition)),
                AttrIr::Style { value, .. } => (
                    "style",
                    match value {
                        StyleDirectiveValue::Expr(e) => shape_of(Some(*e)),
                        // A static-text OR mixed value has no SINGLE directive expression
                        // (a mixed value's parts are not one `ExprId`).
                        StyleDirectiveValue::Text(_) | StyleDirectiveValue::Mixed(_) => {
                            shape_of(None)
                        }
                    },
                ),
                AttrIr::Bind { expr, .. } => ("bind", shape_of(*expr)),
                AttrIr::Use { arg, .. } => ("use", shape_of(*arg)),
                AttrIr::Event { handler, .. } => ("on", shape_of(Some(*handler))),
                _ => continue,
            };
            out.push(CandDirectiveExpr {
                kind: kind.into(),
                shape: shape.into(),
            });
        }
    });
    out.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.shape.cmp(&b.shape)));
    out
}

/// Walk every IR node reachable from a template scope (depth-first, descending
/// element / component / special children and block bodies), invoking `f` with
/// each node and its parent node id.
fn walk_nodes(
    ir: &SvelteRuntimeIr,
    scope: TemplateScopeId,
    f: &mut impl FnMut(&IrNode, Option<NodeId>),
) {
    let roots: Vec<NodeId> = ir.template_scope(scope).roots.clone();
    for node in roots {
        walk_node(ir, node, None, f);
    }
}

fn walk_node(
    ir: &SvelteRuntimeIr,
    node: NodeId,
    parent: Option<NodeId>,
    f: &mut impl FnMut(&IrNode, Option<NodeId>),
) {
    let n = ir.node(node);
    f(n, parent);
    match n {
        IrNode::Element(el) => {
            for &c in &el.children {
                walk_node(ir, c, Some(node), f);
            }
        }
        IrNode::Component(c) => {
            for &ch in &c.children {
                walk_node(ir, ch, Some(node), f);
            }
        }
        IrNode::Special(s) => {
            for &c in &s.children {
                walk_node(ir, c, Some(node), f);
            }
        }
        IrNode::Slot(slot) => walk_nodes(ir, slot.fallback, f),
        IrNode::Block(block) => match block {
            BlockIr::If { branches } => {
                for b in branches {
                    walk_nodes(ir, b.body, f);
                }
            }
            BlockIr::Each {
                body, else_body, ..
            } => {
                walk_nodes(ir, *body, f);
                if let Some(eb) = else_body {
                    walk_nodes(ir, *eb, f);
                }
            }
            BlockIr::Await {
                pending,
                then_body,
                catch_body,
                ..
            } => {
                for ts in [pending, then_body, catch_body].into_iter().flatten() {
                    walk_nodes(ir, *ts, f);
                }
            }
            BlockIr::Key { body, .. } => walk_nodes(ir, *body, f),
            BlockIr::Snippet { body, .. } => walk_nodes(ir, *body, f),
        },
        IrNode::Text { .. }
        | IrNode::Comment { .. }
        | IrNode::Interpolation { .. }
        | IrNode::Tag(_) => {}
    }
}

// ---------------------------------------------------------------------------
// The differential axes
// ---------------------------------------------------------------------------

/// A generated-corpus differential axis the matrix asserts each fixture on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffAxis {
    /// The registered-event set (type + target + delegation kind).
    Events,
    /// The `cannot_be_set_statically` property set (name + kind + value topology).
    NonStaticProperties,
    /// The dynamic / mixed attribute value-part topology.
    AttrParts,
    /// The per-directive inner-expression shape (class/style/bind/use/on).
    DirectiveExprs,
    /// The module's delegated event-type set.
    DelegatedEvents,
    /// The owned structural-helper set (intersected with the owned universe).
    HelperSet,
    /// The serialized `from_html` static-template HTML multiset.
    StaticHtml,
    /// The template factory-kind multiset (from_html / text / svg / mathml / …).
    Factory,
    /// The text-first decoded-text seed multiset.
    DecodedText,
    /// The per-region node-path step-sequence multiset.
    NodePaths,
    /// The per-slot-kind dynamic-surface counts.
    DynamicSlots,
}

impl DiffAxis {
    fn label(self) -> &'static str {
        match self {
            DiffAxis::Events => "events",
            DiffAxis::NonStaticProperties => "non_static_properties",
            DiffAxis::AttrParts => "attr_parts",
            DiffAxis::DirectiveExprs => "directive_exprs",
            DiffAxis::DelegatedEvents => "delegated_events",
            DiffAxis::HelperSet => "helper_set",
            DiffAxis::StaticHtml => "static_html",
            DiffAxis::Factory => "factory",
            DiffAxis::DecodedText => "decoded_text",
            DiffAxis::NodePaths => "node_paths",
            DiffAxis::DynamicSlots => "dynamic_slots",
        }
    }
}

/// Compare ONE axis of a fixture's candidate against its golden. Returns `Ok(())`
/// when they match, `Err(summary)` (official-vs-Verter) when they diverge.
fn compare_axis(
    axis: DiffAxis,
    ir: &SvelteRuntimeIr,
    plan: &StaticTemplatePlan,
    golden: &GeneratedGolden,
) -> Result<(), String> {
    match axis {
        DiffAxis::Events => {
            let cand = project_events(ir);
            let want: Vec<CandEvent> = golden
                .events
                .iter()
                .map(|e| CandEvent {
                    event_type: e.event_type.clone(),
                    target: e.target.clone(),
                    delegation: e.delegation.clone(),
                })
                .collect();
            if cand == want {
                Ok(())
            } else {
                Err(format!("official events {want:?} != Verter {cand:?}"))
            }
        }
        DiffAxis::NonStaticProperties => {
            let cand = project_non_static_properties(ir);
            let want: Vec<CandNonStaticProperty> = golden
                .non_static_properties
                .iter()
                .map(|p| CandNonStaticProperty {
                    name: p.name.clone(),
                    kind: p.kind.clone(),
                    value: p.value.clone(),
                })
                .collect();
            if cand == want {
                Ok(())
            } else {
                Err(format!(
                    "official non-static-properties {want:?} != Verter {cand:?}"
                ))
            }
        }
        DiffAxis::DirectiveExprs => {
            let cand = project_directive_exprs(ir);
            let want: Vec<CandDirectiveExpr> = golden
                .directive_exprs
                .iter()
                .map(|d| CandDirectiveExpr {
                    kind: d.kind.clone(),
                    shape: d.shape.clone(),
                })
                .collect();
            if cand == want {
                Ok(())
            } else {
                Err(format!(
                    "official directive-exprs {want:?} != Verter {cand:?}"
                ))
            }
        }
        DiffAxis::DelegatedEvents => {
            let cand = project_delegated_events(ir, plan);
            let mut want = golden.delegated_events.clone();
            want.sort();
            if cand == want {
                Ok(())
            } else {
                Err(format!(
                    "official delegated-events {want:?} != Verter {cand:?}"
                ))
            }
        }
        DiffAxis::HelperSet => {
            let cand = project_helper_set(ir, plan);
            let want = golden_owned_helper_set(golden);
            if cand == want {
                Ok(())
            } else {
                Err(format!(
                    "official owned-helper-set {want:?} != Verter {cand:?}"
                ))
            }
        }
        DiffAxis::StaticHtml => {
            let cand = project_static_html(plan);
            let want = golden_static_html(golden);
            if cand == want {
                Ok(())
            } else {
                Err(format!("official static-html {want:?} != Verter {cand:?}"))
            }
        }
        DiffAxis::Factory => {
            let cand = project_factory_kinds(plan);
            let want = golden_factory_kinds(golden);
            if cand == want {
                Ok(())
            } else {
                Err(format!(
                    "official factory-kinds {want:?} != Verter {cand:?}"
                ))
            }
        }
        DiffAxis::DecodedText => {
            let cand = project_decoded_text(plan);
            let mut want = golden.decoded_text.clone();
            want.sort();
            if cand == want {
                Ok(())
            } else {
                Err(format!("official decoded-text {want:?} != Verter {cand:?}"))
            }
        }
        DiffAxis::AttrParts => {
            let cand = project_attr_parts(ir);
            let want: Vec<CandAttrPart> = golden
                .attr_parts
                .iter()
                .map(|p| CandAttrPart {
                    helper: p.helper.clone(),
                    attr: p.attr.clone(),
                    chunks: p.chunks.clone(),
                })
                .collect();
            if cand == want {
                Ok(())
            } else {
                Err(format!("official attr-parts {want:?} != Verter {cand:?}"))
            }
        }
        DiffAxis::NodePaths => {
            let cand = project_node_paths(plan);
            let want: Vec<Vec<CandNodePath>> = golden
                .node_paths
                .regions
                .iter()
                .map(|r| {
                    r.paths
                        .iter()
                        .map(|p| CandNodePath {
                            base: p.base.clone(),
                            steps: p.steps.clone(),
                        })
                        .collect()
                })
                .collect();
            if cand == want {
                Ok(())
            } else {
                Err(format!("official node-paths {want:?} != Verter {cand:?}"))
            }
        }
        DiffAxis::DynamicSlots => {
            let cand = project_dynamic_slots(ir, plan);
            if cand == golden.dynamic_slots {
                Ok(())
            } else {
                Err(format!(
                    "official dynamic-slots {:?} != Verter {cand:?}",
                    golden.dynamic_slots
                ))
            }
        }
    }
}

const ALL_AXES: &[DiffAxis] = &[
    DiffAxis::Events,
    DiffAxis::NonStaticProperties,
    DiffAxis::AttrParts,
    DiffAxis::DirectiveExprs,
    DiffAxis::DelegatedEvents,
    DiffAxis::HelperSet,
    DiffAxis::StaticHtml,
    DiffAxis::Factory,
    DiffAxis::DecodedText,
    DiffAxis::NodePaths,
    DiffAxis::DynamicSlots,
];

// ---------------------------------------------------------------------------
// The honest KNOWN_DIVERGENCES allow-list (this phase ENUMERATES, does not fix)
// ---------------------------------------------------------------------------

/// One allow-list row: a `(fixture, axis)` pair where Verter's IR-derived
/// topology GENUINELY diverges from official, plus a root-cause label (a Y-label
/// — see [`KNOWN_DIVERGENCES`] grouping) and a ground-truthed official-vs-Verter
/// summary. The matrix skips ONLY these pairs; every other axis is asserted
/// exactly. [`known_divergences_are_real`] proves every row STILL diverges (a
/// stale row that no longer diverges fails the guard).
struct DivergenceRow {
    /// The fixture slug (`generated/NNN_label.svelte`).
    fixture: &'static str,
    /// The diverging axis.
    axis: DiffAxis,
    /// The root-cause Y-label (Y1-Y9 inherited, Y10+ new in this harness).
    root_cause: &'static str,
    /// A short ground-truthed official-vs-Verter summary.
    summary: &'static str,
}

/// The full divergence enumeration — the COMPLETE set of `(fixture, axis)` pairs
/// the generated differential matrix skips, each pinned to its root-cause label.
/// Grouped by ROOT CAUSE. Every divergence the matrix surfaces that is NOT here
/// is an unaccounted regression that FAILS the matrix.
///
/// This is populated by [`enumerate_divergences`] (the discovery pass) and then
/// frozen here. Each row is GUARDED by [`known_divergences_are_real`].
const KNOWN_DIVERGENCES: &[DivergenceRow] = &KNOWN_DIVERGENCES_DATA;

include!("diff_oracle_divergences.rs");

/// Whether `(fixture, axis)` is on the divergence allow-list.
fn is_known_divergence(fixture: &str, axis: DiffAxis) -> bool {
    KNOWN_DIVERGENCES
        .iter()
        .any(|r| r.fixture == fixture && r.axis == axis)
}

// ---------------------------------------------------------------------------
// The matrix
// ---------------------------------------------------------------------------

#[test]
fn generated_differential_matrix_matches_oracle() {
    let mut unaccounted: Vec<String> = Vec::new();
    for slug in &generated_corpus() {
        let source = load_generated_fixture(slug);
        let alloc = Allocator::default();
        let (ir, plan) = lower_generated(&source, &alloc);
        let golden = load_generated_golden(slug);
        assert_eq!(&golden.slug, slug, "golden identity matches fixture slug");

        for &axis in ALL_AXES {
            if is_known_divergence(slug, axis) {
                continue;
            }
            if let Err(summary) = compare_axis(axis, &ir, &plan, &golden) {
                unaccounted.push(format!(
                    "{slug} :: {} — {summary} (not on KNOWN_DIVERGENCES — fix it or enumerate it)",
                    axis.label()
                ));
            }
        }
    }
    assert!(
        unaccounted.is_empty(),
        "{} unaccounted generated-corpus divergence(s) (each must be FIXED, or \
         added to KNOWN_DIVERGENCES with a Y-label + ground-truthed summary):\n{}",
        unaccounted.len(),
        unaccounted.join("\n")
    );
    // Emit the enumerated divergences (visible at every run) so the long tail is
    // never silent.
    for row in KNOWN_DIVERGENCES {
        eprintln!(
            "KNOWN-DIVERGENCE {} :: {} → {} ({})",
            row.fixture,
            row.axis.label(),
            row.root_cause,
            row.summary
        );
    }
}

#[test]
#[ignore = "discovery harness: run with --ignored to dump every generated-corpus divergence"]
fn enumerate_divergences_discovery() {
    // The discovery pass that BUILDS the KNOWN_DIVERGENCES allow-list: run ALL
    // axes against ALL generated fixtures and dump every divergence (grouped by
    // axis). Not a gate — `#[ignore]`d — but the source of truth for the frozen
    // allow-list. Run: `cargo test -p verter_compiler --lib enumerate_divergences_discovery -- --ignored --nocapture`.
    let mut lines: Vec<String> = Vec::new();
    for slug in &generated_corpus() {
        let source = load_generated_fixture(slug);
        let alloc = Allocator::default();
        let (ir, plan) = lower_generated(&source, &alloc);
        let golden = load_generated_golden(slug);
        for &axis in ALL_AXES {
            if let Err(summary) = compare_axis(axis, &ir, &plan, &golden) {
                lines.push(format!("DIVERGE\t{slug}\t{}\t{summary}", axis.label()));
            }
        }
    }
    eprintln!("=== GENERATED-CORPUS DIVERGENCES ({}) ===", lines.len());
    for l in &lines {
        eprintln!("{l}");
    }
}

#[test]
fn known_divergences_are_real() {
    // Every allow-list row MUST genuinely diverge against the pinned compiler: a
    // row whose `(fixture, axis)` now MATCHES is stale (the divergence was fixed
    // or never existed) and FAILS here — the allow-list cannot hide a real pass.
    // Mirrors the hand-vendored `deferral_ledger_rows_are_justified_and_real`.
    let mut stale: Vec<String> = Vec::new();
    for row in KNOWN_DIVERGENCES {
        let source = load_generated_fixture(row.fixture);
        let alloc = Allocator::default();
        let (ir, plan) = lower_generated(&source, &alloc);
        let golden = load_generated_golden(row.fixture);
        if compare_axis(row.axis, &ir, &plan, &golden).is_ok() {
            stale.push(format!(
                "{} :: {} — labeled {} but the candidate now MATCHES official (stale row — remove it)",
                row.fixture,
                row.axis.label(),
                row.root_cause
            ));
        }
    }
    assert!(
        stale.is_empty(),
        "{} stale KNOWN_DIVERGENCES row(s) (no longer divergent — remove them):\n{}",
        stale.len(),
        stale.join("\n")
    );
}

#[test]
fn known_divergence_fixtures_and_axes_are_valid() {
    // Every allow-list fixture exists in the generated corpus and every row's
    // root-cause label is non-empty (no typo'd slug silently never-asserts).
    let corpus: std::collections::BTreeSet<String> = generated_corpus().into_iter().collect();
    for row in KNOWN_DIVERGENCES {
        assert!(
            corpus.contains(row.fixture),
            "KNOWN_DIVERGENCES references a fixture absent from the generated corpus: {}",
            row.fixture
        );
        assert!(
            !row.root_cause.is_empty() && !row.summary.is_empty(),
            "KNOWN_DIVERGENCES row for {} has an empty root_cause/summary",
            row.fixture
        );
    }
}
