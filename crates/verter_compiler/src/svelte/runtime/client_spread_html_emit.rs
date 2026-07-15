//! The element-spread `$.attribute_effect` + the `{@html}` `$.html` EMISSION helpers.
//!
//! Extracted from `client.rs` (the file-size guard boundary): these are the
//! [`ClientEmitter`] methods that build the emitted call text for the two coalesced
//! runtime surfaces a spread element / a `{@html}` tag produces. They read the narrow
//! plan op set + the walk-populated DOM var maps (never a broad-IR emission decision).
//! The fold ITEMS render through ONE ordered per-effect memoizer (the official
//! per-attribute/spread `Memoizer` topology) shared by the regular-element spread
//! fold and the `<svelte:element>` fold.

use super::client::ClientEmitter;
use super::client_codegen_helpers::concise_arrow_expr_body;
use super::client_effect::Memoizer;
use super::client_legacy_value::PreparedTemplateValue;
use super::client_plan::ClientRuntimeOp;
use super::client_plan_types::{AttributeEffectItem, HtmlGetterForm};
use super::ir::{IrNode, NodeId};

/// Build one `$.attribute_effect(<host>, (params) => ({ <body> })[, [deps…] | void 0,
/// void 0, void 0, <css_hash | void 0>[, true]])` call — the official argument row
/// `(el, fn, sync, async, blockers, css_hash, remove_defaults)` with missing trailing
/// arguments dropped: the memoized deps ride the `sync` slot (each a `() => <expr>`
/// arrow, params `$0 … $N-1` on the fold arrow); a SCOPED host passes its scope-hash
/// literal at the `css_hash` slot; a void / self-closing element (an `<input>`, whose
/// value/defaultValue handling the trailing `true` flags) keeps the tail through
/// `remove_defaults`. HOST-INDEPENDENT — the ONE tail builder the regular-element
/// spread fold and the `<svelte:element>` fold share, so the argument topology never
/// drifts between the two emitters.
pub(super) fn attribute_effect_call(
    var: &str,
    fold_body: &str,
    deps: &[String],
    input_trailing: bool,
    css_hash: Option<&str>,
) -> String {
    let params = (0..deps.len())
        .map(|i| format!("${i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let fun = format!("({params}) => ({{ {fold_body} }})");
    // The sync (deps) slot: the memoized `() => <expr>` arrows, in placeholder
    // order (each routed through the shared concise-arrow body wrap so an
    // object/sequence dep stays a valid expression body).
    let sync = (!deps.is_empty()).then(|| {
        let arrows = deps
            .iter()
            .map(|d| format!("() => {}", concise_arrow_expr_body(d)))
            .collect::<Vec<_>>()
            .join(", ");
        format!("[{arrows}]")
    });
    match (sync, css_hash, input_trailing) {
        (Some(sync), Some(hash), true) => {
            format!("$.attribute_effect({var}, {fun}, {sync}, void 0, void 0, {hash}, true)")
        }
        (Some(sync), Some(hash), false) => {
            format!("$.attribute_effect({var}, {fun}, {sync}, void 0, void 0, {hash})")
        }
        (Some(sync), None, true) => {
            format!("$.attribute_effect({var}, {fun}, {sync}, void 0, void 0, void 0, true)")
        }
        (Some(sync), None, false) => format!("$.attribute_effect({var}, {fun}, {sync})"),
        (None, Some(hash), true) => {
            format!("$.attribute_effect({var}, {fun}, void 0, void 0, void 0, {hash}, true)")
        }
        (None, Some(hash), false) => {
            format!("$.attribute_effect({var}, {fun}, void 0, void 0, void 0, {hash})")
        }
        (None, None, true) => {
            format!("$.attribute_effect({var}, {fun}, void 0, void 0, void 0, void 0, true)")
        }
        (None, None, false) => format!("$.attribute_effect({var}, {fun})"),
    }
}

/// The `{@html}` getter (the `$.html` second argument) from the PREPARED payload +
/// its plan-decided [`HtmlGetterForm`]: the elided bare callee, the rebuilt
/// `() => <callee>()` thunk, or the general `() => <prepared>` getter — a
/// legacy-wrapped payload always takes the general getter over the parenthesized
/// sequence (preparation PRECEDES thunk-elision eligibility).
pub(super) fn html_payload_getter(
    payload: &PreparedTemplateValue,
    form: &HtmlGetterForm,
) -> String {
    match form {
        HtmlGetterForm::ElidedCallee(callee) => callee.clone(),
        HtmlGetterForm::RebuiltCallThunk(callee) => format!("() => {callee}()"),
        HtmlGetterForm::PreparedThunk => format!("() => {}", payload.arrow_body()),
    }
}

impl ClientEmitter<'_> {
    /// Render the TYPED fold items of one `$.attribute_effect` through ONE ordered
    /// per-effect memoizer (the official `Memoizer`): each `has_call` value hoists
    /// into a `$N` placeholder + a dependency; event-attribute handlers hoist to
    /// stable `var <name> = <handler>;` locals appended to `hoists` (the official
    /// `context.state.init` order — before the effect). Returns the assembled
    /// object-literal BODY and the ordered dependency bodies. SHARED by the
    /// regular-element spread fold and the `<svelte:element>` fold.
    pub(super) fn render_attribute_effect_items(
        &mut self,
        items: &[AttributeEffectItem],
        hoists: &mut String,
    ) -> (String, Vec<String>) {
        let mut memoizer = Memoizer::default();
        let mut entries: Vec<String> = Vec::with_capacity(items.len());
        for item in items {
            match item {
                AttributeEffectItem::Entry(entry) => entries.push(entry.clone()),
                AttributeEffectItem::Attr { prop, value } => {
                    let v = self.build_attr_value(value, &mut Some(&mut memoizer));
                    entries.push(format!("{prop}: {v}"));
                }
                AttributeEffectItem::Spread { value } => {
                    // RAW w.r.t. `build_expression`, but the memoizer still receives
                    // it (official `SpreadAttribute` + `Memoizer.add`).
                    let v = memoizer.add(value.effect_value(), value.has_call());
                    entries.push(format!("...{v}"));
                }
                AttributeEffectItem::Event { prop, handler } => {
                    // The official attribute-effect handler-stability hoist: a stable
                    // `var event_handler = <handler>;` referenced by name in the fold.
                    let name = self.alloc_name("event_handler");
                    hoists.push_str(&format!(
                        "\tvar {name} = {};\n",
                        handler.inline_expression()
                    ));
                    entries.push(format!("{prop}: {name}"));
                }
                AttributeEffectItem::ClassDirectives(obj) => {
                    let v = memoizer.add(obj.raw_text().to_string(), obj.has_call());
                    entries.push(format!("[$.CLASS]: {v}"));
                }
                AttributeEffectItem::StyleDirectives(obj) => {
                    let v = memoizer.add(obj.raw_text().to_string(), obj.has_call());
                    entries.push(format!("[$.STYLE]: {v}"));
                }
            }
        }
        (entries.join(", "), memoizer.into_deps())
    }

    /// Emit the only-child `$.html(el, payload, true)` raw-markup call (the `$.reset(el)`
    /// follows via the parent's child walk). The `el` is the PARENT element var.
    pub(super) fn emit_html_only_child(&self, parent: NodeId, getter: &str) -> String {
        let var = self.dom_var(parent);
        format!("$.html({var}, {getter}, true)")
    }

    /// The `$.html` GETTER (second argument) of the `{@html}` op targeting `node`, or
    /// `None` when no such op exists (a non-`{@html}` node). Read from the narrow op set.
    pub(super) fn html_op_payload(&self, node: NodeId) -> Option<String> {
        self.plan().all_ops().find_map(|op| match op {
            ClientRuntimeOp::Html {
                target,
                payload,
                getter_form,
                ..
            } if NodeId(target.0) == node => Some(html_payload_getter(payload, getter_form)),
            _ => None,
        })
    }

    /// Whether the `{@html}` op targeting `node` is the only-child (controlled) form.
    pub(super) fn html_op_is_only_child(&self, node: NodeId) -> bool {
        self.plan().all_ops().any(|op| {
            matches!(op, ClientRuntimeOp::Html { target, only_child: true, .. }
                if NodeId(target.0) == node)
        })
    }

    /// The PARENT element node id whose direct children contain the `{@html}` node — used
    /// to place the only-child `$.html(parent, …, true)` at the parent's init position.
    /// `None` for a root-level `{@html}` (no parent element).
    pub(super) fn html_only_child_parent(&self, html_node: NodeId) -> Option<NodeId> {
        for (idx, node) in self.ir().nodes.iter().enumerate() {
            if let IrNode::Element(el) = node {
                if el.children.contains(&html_node) {
                    return Some(NodeId(idx as u32));
                }
            }
        }
        None
    }
}
