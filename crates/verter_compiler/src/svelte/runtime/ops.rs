//! Reactive runtime-op population.
//!
//! Walks the lowered template nodes per template scope and emits the reactive
//! [`RuntimeOp`]s for every reactive surface the IR carries — reactive text, raw
//! `{@html}`, dynamic / class / style attributes, spreads, two-way binds, events,
//! and `{@attach}` attachments — attaching each [`OpId`] to its owning template
//! scope's `local_ops`.

use super::html::cannot_be_set_statically;
use super::ir::{
    ActionOp, AttrIr, AttrOp, AttrOpKind, BindOp, EventOp, EventTarget, IrNode, MixedAttrPart,
    NodeId, NonStaticPropertyKind, NonStaticPropertyOp, NonStaticPropertyValue, OpId, RuntimeOp,
    SpecialElementIr, SpecialKind, TagIr, TemplateScope, TransitionOp,
};

/// Populate the reactive runtime ops for every reactive surface the lowering
/// detected, attaching each [`OpId`] to its owning template scope's `local_ops`.
///
/// Each template scope owns the ops for the reactive surfaces of the nodes it
/// directly contains (descending into element / component / special-element
/// children, but NOT crossing a nested block body — that body is its own template
/// scope and owns its own ops). The emitted ops mirror the dynamic surfaces the
/// IR already carries: a `{expr}` interpolation → [`RuntimeOp::ReactiveText`]; a
/// dynamic / class / style attribute → [`RuntimeOp::ReactiveAttr`]; a spread →
/// [`RuntimeOp::SpreadAttrs`]; a `bind:` → [`RuntimeOp::Binding`]; an event →
/// [`RuntimeOp::Event`]; an `{@attach}` → [`RuntimeOp::Attachment`].
pub(super) fn populate_runtime_ops(
    nodes: &[IrNode],
    template_scopes: &mut [TemplateScope],
    ops: &mut Vec<RuntimeOp>,
) {
    for scope in template_scopes.iter_mut() {
        let mut local = Vec::new();
        for &node in &scope.roots {
            collect_node_ops(nodes, node, ops, &mut local);
        }
        scope.local_ops = local;
    }
}

/// Intern a runtime op into the arena, returning its id.
fn push_op(ops: &mut Vec<RuntimeOp>, op: RuntimeOp) -> OpId {
    let id = OpId(ops.len() as u32);
    ops.push(op);
    id
}

/// Emit the reactive ops for one node + its in-scope descendants, appending each
/// [`OpId`] to `local`. Stops at a nested block (its body is a separate scope).
fn collect_node_ops(
    nodes: &[IrNode],
    node_id: NodeId,
    ops: &mut Vec<RuntimeOp>,
    local: &mut Vec<OpId>,
) {
    match nodes[node_id.0 as usize].clone() {
        IrNode::Interpolation { expr, .. } => {
            let op = push_op(
                ops,
                RuntimeOp::ReactiveText {
                    target: node_id,
                    expr,
                },
            );
            local.push(op);
        }
        IrNode::Element(el) => {
            collect_attr_ops(node_id, &el.attrs, EventTarget::Node(node_id), ops, local);
            for child in el.children {
                collect_node_ops(nodes, child, ops, local);
            }
        }
        IrNode::Component(c) => {
            collect_attr_ops(node_id, &c.attrs, EventTarget::Node(node_id), ops, local);
            for child in c.children {
                collect_node_ops(nodes, child, ops, local);
            }
        }
        IrNode::Special(s) => {
            // A special element's event listeners target the global the element
            // represents (`<svelte:window>` ⇒ Window, `<svelte:document>` ⇒
            // Document, `<svelte:body>` ⇒ Body), NOT the node — verified against
            // svelte@5.56.3 (`$.window` / `$.document` / `$.document.body`).
            let event_target = special_event_target(&s, node_id);
            collect_attr_ops(node_id, &s.attrs, event_target, ops, local);
            for child in s.children {
                collect_node_ops(nodes, child, ops, local);
            }
        }
        IrNode::Tag(TagIr::Html { expr }) => {
            // `{@html}` is a reactive raw-text surface — model it as reactive text.
            let op = push_op(
                ops,
                RuntimeOp::ReactiveText {
                    target: node_id,
                    expr,
                },
            );
            local.push(op);
        }
        IrNode::Tag(TagIr::Attach { expr }) => {
            let op = push_op(
                ops,
                RuntimeOp::Attachment {
                    target: node_id,
                    expr,
                },
            );
            local.push(op);
        }
        // A block introduces a separate template scope (its own ops); other tags /
        // text / comments carry no reactive op here.
        IrNode::Block(_) | IrNode::Tag(_) | IrNode::Text { .. } | IrNode::Comment { .. } => {}
    }
}

/// The event-registration target for a special element: the global the element
/// represents. An intrinsic / component element targets its node.
fn special_event_target(s: &SpecialElementIr, node_id: NodeId) -> EventTarget {
    match s.kind {
        SpecialKind::Window => EventTarget::Window,
        SpecialKind::Document => EventTarget::Document,
        SpecialKind::Body => EventTarget::Body,
        _ => EventTarget::Node(node_id),
    }
}

/// Emit the reactive attribute / bind / event / spread / class / style / action /
/// transition ops for one element's attributes, targeting `target` (events use
/// `event_target`).
///
/// Every reactive attribute surface the IR can represent emits an op, in
/// ATTRIBUTE SOURCE ORDER (spreads are NOT hoisted to the end — a `{...rest}`
/// before a `class` writes before the class op). No dynamic surface is silently
/// dropped: shorthand `class:`/`style:`/`bind:` directives carry their
/// synthesized expression; `Mixed` values emit one `ReactiveAttr` per expression
/// run; `use:` / `transition:` / `in:` / `out:` / `animate:` emit `Action` /
/// `Transition` ops.
fn collect_attr_ops(
    target: NodeId,
    attrs: &[AttrIr],
    event_target: EventTarget,
    ops: &mut Vec<RuntimeOp>,
    local: &mut Vec<OpId>,
) {
    // Source order is preserved: each spread emits its own SpreadAttrs op AT its
    // attribute position, accumulating no run — so the relative order of a spread
    // vs an adjacent attribute matches the source.
    for attr in attrs {
        // A "cannot be set statically" attribute (`autofocus` / `muted` /
        // `defaultValue` / `defaultChecked`) is applied at runtime via the
        // `NonStaticProperty` op (a property write, or `$.autofocus`), NOT a plain
        // `set_attribute`, and is excluded from the static skeleton. Intercept it
        // before the normal attribute handling. (A `class:`/`style:`/`bind:`
        // directive shares no name with these, so only `Static` / `Dynamic` /
        // `Mixed` plain attributes are affected.)
        if let Some(op) = non_static_property_op(target, attr) {
            push_local(ops, local, op);
            continue;
        }
        match attr {
            AttrIr::Dynamic { name, expr } => push_local(
                ops,
                local,
                RuntimeOp::ReactiveAttr {
                    target,
                    attr: AttrOp {
                        name: name.clone(),
                        expr: *expr,
                        kind: AttrOpKind::Plain,
                    },
                },
            ),
            AttrIr::Mixed { name, parts } => {
                // A concatenated value (`class="a {b}"`) is a dynamic attribute
                // surface; emit one ReactiveAttr per expression run in order.
                for part in parts {
                    if let MixedAttrPart::Expr(expr) = part {
                        push_local(
                            ops,
                            local,
                            RuntimeOp::ReactiveAttr {
                                target,
                                attr: AttrOp {
                                    name: name.clone(),
                                    expr: *expr,
                                    kind: attr_op_kind_for_name(name),
                                },
                            },
                        );
                    }
                }
            }
            // A `class:`/`style:` directive — shorthand carries the synthesized
            // expression, so a `None` here is a genuinely value-less form that
            // emits no op (defensive; lowering always synthesizes one).
            AttrIr::Class {
                name,
                condition: Some(expr),
            } => push_local(
                ops,
                local,
                RuntimeOp::ReactiveAttr {
                    target,
                    attr: AttrOp {
                        name: name.clone(),
                        expr: *expr,
                        kind: AttrOpKind::Class,
                    },
                },
            ),
            AttrIr::Style {
                property,
                value: Some(expr),
                ..
            } => push_local(
                ops,
                local,
                RuntimeOp::ReactiveAttr {
                    target,
                    attr: AttrOp {
                        name: property.clone(),
                        expr: *expr,
                        kind: AttrOpKind::Style,
                    },
                },
            ),
            AttrIr::Bind {
                target: bind_target,
                expr: Some(expr),
            } => push_local(
                ops,
                local,
                RuntimeOp::Binding {
                    target,
                    bind: BindOp {
                        target: bind_target.clone(),
                        expr: *expr,
                    },
                },
            ),
            AttrIr::Event {
                event_type,
                handler,
                delegated,
                capture,
                modifiers,
            } => push_local(
                ops,
                local,
                RuntimeOp::Event {
                    target: event_target,
                    event: EventOp {
                        event_type: event_type.clone(),
                        handler: *handler,
                        delegated: *delegated,
                        capture: *capture,
                        modifiers: modifiers.clone(),
                    },
                },
            ),
            AttrIr::Spread { expr } => push_local(
                ops,
                local,
                RuntimeOp::SpreadAttrs {
                    target,
                    spreads: vec![*expr],
                },
            ),
            AttrIr::Use { expr, arg } => push_local(
                ops,
                local,
                RuntimeOp::Action {
                    target,
                    action: ActionOp {
                        expr: *expr,
                        arg: *arg,
                    },
                },
            ),
            AttrIr::Transition { kind, name, expr } => push_local(
                ops,
                local,
                RuntimeOp::Transition {
                    target,
                    transition: TransitionOp {
                        kind: *kind,
                        name: name.clone(),
                        expr: *expr,
                    },
                },
            ),
            // A value-less shorthand whose lowering produced no expression, a
            // `let:` slot-prop directive (a slot-scope concern, not a node-reactive
            // surface), and a static attribute carry no node-reactive op here.
            AttrIr::Class {
                condition: None, ..
            }
            | AttrIr::Style { value: None, .. }
            | AttrIr::Bind { expr: None, .. }
            | AttrIr::Let { .. }
            | AttrIr::Static { .. } => {}
        }
    }
}

/// Push an op into the arena and attach its id to the scope's local op list.
fn push_local(ops: &mut Vec<RuntimeOp>, local: &mut Vec<OpId>, op: RuntimeOp) {
    let id = push_op(ops, op);
    local.push(id);
}

/// The reactive-attribute op kind implied by a mixed-value attribute NAME (`class`
/// ⇒ Class, `style` ⇒ Style, else Plain). Structural (the attribute name), not a
/// text heuristic.
fn attr_op_kind_for_name(name: &str) -> AttrOpKind {
    match name {
        "class" => AttrOpKind::Class,
        "style" => AttrOpKind::Style,
        _ => AttrOpKind::Plain,
    }
}

/// Build the [`RuntimeOp::NonStaticProperty`] op for a plain attribute whose name
/// `cannot_be_set_statically` (`autofocus` / `muted` / `defaultValue` /
/// `defaultChecked`), or `None` for any other attribute. The op carries the
/// autofocus-vs-DOM-property kind and the init value (boolean for a valueless
/// attribute, a literal for a static value, or an expression for a dynamic value).
///
/// Only a plain `Static` / `Dynamic` / `Mixed` attribute reaches here — a `Mixed`
/// `defaultValue="a{b}"` is unusual but its concatenated runtime value is still a
/// non-static property; for simplicity (and matching the official, which sends the
/// whole concatenated value through `build_attribute_value`) we carry only its
/// FIRST expression run as the dynamic value. The directive forms (`class:` /
/// `style:` / `bind:` / `on:` / …) never share these names.
fn non_static_property_op(target: NodeId, attr: &AttrIr) -> Option<RuntimeOp> {
    let (name, value) = match attr {
        AttrIr::Static { name, value } if cannot_be_set_statically(name) => {
            let v = match value {
                Some(v) => NonStaticPropertyValue::Literal(v.value.clone()),
                None => NonStaticPropertyValue::Boolean,
            };
            (name.clone(), v)
        }
        AttrIr::Dynamic { name, expr } if cannot_be_set_statically(name) => {
            (name.clone(), NonStaticPropertyValue::Expr(*expr))
        }
        AttrIr::Mixed { name, parts } if cannot_be_set_statically(name) => {
            // A mixed non-static property carries the FULL ordered chunk list (the
            // official `build_attribute_value` concatenates the literal/expr
            // alternation into the property write — `input.defaultValue = `a ${x} b``).
            // The literal chunks are RETAINED, not dropped to a lone expression.
            (name.clone(), NonStaticPropertyValue::Mixed(parts.clone()))
        }
        _ => return None,
    };
    // `autofocus` → the `$.autofocus(node, value)` helper; the others → a DOM
    // property write `node.<name> = value`.
    let kind = if name == "autofocus" {
        NonStaticPropertyKind::Autofocus
    } else {
        NonStaticPropertyKind::DomProperty
    };
    Some(RuntimeOp::NonStaticProperty {
        target,
        property: NonStaticPropertyOp { name, kind, value },
    })
}
