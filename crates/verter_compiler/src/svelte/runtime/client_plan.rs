//! The NARROW client module plan — the ONLY input the client emitter consumes.
//!
//! [`SupportedClientIr::build`] is the semantic-projection stage: it takes the
//! TYPED [`ClassifiedClientSurface`] (the proof the default-deny classifier
//! accepted every surface) plus the broad [`SvelteRuntimeIr`], and projects a
//! NARROW [`ClientModulePlan`] over a closed vocabulary — [`ClientNode`],
//! [`ClientAttr`], [`ClientScriptItem`], [`ClientRuntimeOp`]. It decides whether
//! each interpolation is ACTUALLY reactive (a non-reactive interpolation fails
//! closed — the official compiler static-folds it), validates each bind lvalue, and
//! rewrites every script item + op through the FALLIBLE expression rewriter (a
//! refusal short-circuits the whole build).
//!
//! Because the emitter ([`super::client`]) matches ONLY the narrow plan, no broad
//! [`IrNode`] / [`AttrIr`] / [`RuntimeOp`] variant reaches emission — emit-by-default
//! is structurally impossible (a future broad-IR variant cannot become
//! emit-capable).

use oxc_allocator::Allocator;

use super::client::UnsupportedSvelteRuntimeSurface;
use super::client_allowlist::SupportedHtmlElement;
use super::client_codegen_helpers::{
    escape_template_text, is_signal_kind, js_single_quoted, object_key, op_target_node,
    style_object,
};
use super::client_shapes::{
    ClientBindShape, ClientDynamicAttrShape, ClientEventHandlerShape, ClientInterpolationShape,
};
use super::client_surface::ClassifiedClientSurface;
use super::entity_decode::decode_attr_entities;
use super::expr::{BindingRuntimeKind, ScopeId};
use super::expr_emit;
use super::expr_rewrite::{self, PropReads, ProxyInitMap};
use super::ir::{
    AttrIr, AttrOpKind, BindOp, EventOp, EventTarget, ExprId, IrNode, MixedAttrPart, NodeId,
    NonStaticPropertyKind, NonStaticPropertyValue, RuntimeOp, SvelteRuntimeIr,
};
use verter_span::Span;

// The narrow client-plan VOCABULARY (the closed node / attribute / op / value type set
// the emitter consumes) lives in the sibling `client_plan_types` module; this builder
// projects the broad IR onto it. Re-exported so existing consumers (`super::client`, …)
// keep importing the vocabulary as `super::client_plan::<Type>`.
pub(super) use super::client_plan_types::{
    AttrValue, AttrValuePart, ClientAttr, ClientBindTarget, ClientDynAttrEmit, ClientNode,
    ClientNodeId, ClientRuntimeOp, ClientScriptItem,
};

/// The narrow client module plan — the SOLE emitter input.
pub(super) struct ClientModulePlan<'a> {
    /// The component identity + mode.
    pub(super) component: super::ir::ComponentIr,
    /// The narrow node arena, indexed by [`ClientNodeId`] (mirrors the supported
    /// IR node space 1:1) — the EMISSION-decision view of every template node (the
    /// walk reads each named position's KIND / tag from here). Building it is also
    /// where the per-interpolation reactivity fail-closed decision is made.
    pub(super) nodes: Vec<ClientNode>,
    /// The component-FUNCTION-BODY statements, in source order. (A `<script module>`
    /// / instance import is fail-closed upstream, so there are no module-scope
    /// imports / hoists — the body is the only script-item slot.)
    pub(super) body_statements: Vec<ClientScriptItem>,
    /// The narrow reactive ops in source order.
    pub(super) ops: Vec<ClientRuntimeOp>,
    /// Whether the component opens a component context (`$.push`/`$.pop`).
    pub(super) needs_context: bool,
    /// Whether the component function takes `$$props`.
    pub(super) uses_props: bool,
    /// The build-time analysis the emitter reads for the reactive-text rewrite (the
    /// memoizer consults the per-interpolation rewritten expression). Retained as a
    /// borrow so the plan stays the single emitter input without re-deriving.
    pub(super) build: SupportedClientIr<'a>,
}

/// The semantic projection stage — it attaches the reactivity / lvalue / prop-read
/// facts the narrow plan needs, then builds the [`ClientModulePlan`].
pub(super) struct SupportedClientIr<'a> {
    /// The runtime IR (read for the structural template walk + the reactive-text
    /// rewrite at emit time).
    pub(super) ir: &'a SvelteRuntimeIr<'a>,
    /// The component's `$props()` read forms.
    pub(super) prop_reads: PropReads,
    /// The per-instance one-hop proxy-init map (threaded into the TEMPLATE-side
    /// rewrite so a handler reassignment matches the official `should_proxy(rhs)`).
    pub(super) proxy_inits: ProxyInitMap,
    /// The component-declared root names (the `has_call` memoizer `is_pure` input).
    pub(super) declared_roots: rustc_hash::FxHashSet<String>,
    /// The accepted event-handler shape per target node (the classifier's typed
    /// FACT) — the op projection carries it onto each [`ClientRuntimeOp::Event`].
    pub(super) event_shapes: Vec<(NodeId, ClientEventHandlerShape)>,
    /// The accepted bind shape per target node (the classifier's typed FACT) — the
    /// op projection carries it onto each [`ClientRuntimeOp::Bind`].
    pub(super) bind_shapes: Vec<(NodeId, ClientBindShape)>,
    /// The accepted interpolation shape per interpolation node (the classifier's
    /// typed FACT) — proves each `ReactiveText` node is a bare signal /
    /// no-default-prop read, so the plan reads a typed classification instead of
    /// re-deriving reactivity.
    pub(super) interp_shapes: Vec<(NodeId, ClientInterpolationShape)>,
    /// The accepted element fact per element node (the strict-allowlist `try_from`
    /// proof) — projected onto each [`ClientNode::Element`] so the emitter reads the
    /// DOM var stem from [`SupportedHtmlElement::var_stem`], never the raw tag.
    pub(super) element_facts: Vec<(NodeId, SupportedHtmlElement)>,
    /// The TYPED supported instance-script items (the strict finite allowlist) —
    /// the SOLE input `build_script_items` lowers. The broad statement-rewrite path
    /// is gone; this is the only instance-script source.
    pub(super) script_items: Vec<super::client_shapes::SupportedInstanceScriptItem>,
}

impl<'a> SupportedClientIr<'a> {
    /// Build the semantic projection and the narrow plan from the classified
    /// surface and the broad IR. A refusal (a non-reactive interpolation, an
    /// unsupported expression in a script item / op) short-circuits the build.
    pub(super) fn build(
        classified: &ClassifiedClientSurface,
        ir: &'a SvelteRuntimeIr<'a>,
    ) -> Result<ClientModulePlan<'a>, UnsupportedSvelteRuntimeSurface> {
        let alloc = Allocator::default();
        let prop_reads = ir
            .analysis
            .scripts
            .instance_source
            .map(|src| expr_emit::collect_prop_reads(&alloc, src))
            .unwrap_or_default();
        // The per-instance proxy-init map — threaded into the TEMPLATE-side rewrite
        // so a handler `o = primitiveVar` does NOT proxy (the one-hop follow).
        let proxy_inits = ir
            .analysis
            .scripts
            .instance_source
            .and_then(|src| super::expr::reparse_module(&alloc, src))
            .map(|program| super::state_scan::collect_proxy_inits(&program))
            .unwrap_or_default();
        let declared_roots = super::reactive_analysis::collect_declared_root_names(
            &alloc,
            ir.analysis.scripts.module_source,
            ir.analysis.scripts.instance_source,
        );
        let projection = SupportedClientIr {
            ir,
            prop_reads,
            proxy_inits,
            declared_roots,
            event_shapes: classified.event_shapes.clone(),
            bind_shapes: classified.bind_shapes.clone(),
            interp_shapes: classified.interp_shapes.clone(),
            element_facts: classified.element_facts.clone(),
            script_items: classified.script_items.clone(),
        };

        // Divergence guard: the op projection re-derives a plain dynamic attribute's
        // emission shape through the shared `classify_dynamic_attr_shape` (the SAME
        // function the classifier used to ACCEPT it). Assert the recorded
        // `SetAttribute` / `DomProperty` shapes still re-derive to the same FAMILY, so a
        // future table edit that desynced acceptance from emission fails closed here
        // rather than silently mis-emitting (a property write as a `set_attribute`, or
        // vice versa). `Class` / `Style` / `Autofocus` shapes carry no re-derivable
        // name and are trusted as recorded.
        for (_node, _idx, shape) in &classified.dynamic_attr_shapes {
            let recorded_name = match shape {
                ClientDynamicAttrShape::SetAttribute { name }
                | ClientDynamicAttrShape::DomProperty { prop: name } => name,
                _ => continue,
            };
            // Re-classify the recorded (already-normalized) name; it must land in the
            // SAME family. (A normalized name round-trips: `normalize_attribute` of an
            // already-normalized name is idempotent.)
            let re =
                super::client_shapes::classify_dynamic_attr_shape(recorded_name, Span::new(0, 0));
            let same_family = matches!(
                (shape, &re),
                (
                    ClientDynamicAttrShape::SetAttribute { .. },
                    Ok(ClientDynamicAttrShape::SetAttribute { .. })
                ) | (
                    ClientDynamicAttrShape::DomProperty { .. },
                    Ok(ClientDynamicAttrShape::DomProperty { .. })
                )
            );
            if !same_family {
                return Err(UnsupportedSvelteRuntimeSurface::DynamicAttribute {
                    name: recorded_name.clone(),
                    span: Span::new(0, 0),
                });
            }
        }

        // (1) The component-body statements from the TYPED instance-script item
        // allowlist (a `<script module>` / instance import is fail-closed upstream, so
        // there are no module-scope imports / hoists).
        let body_statements = projection.build_script_items();

        // (2) The narrow node arena (mirrors the supported IR node space). The
        // reactivity decision for each interpolation is made here: a non-reactive
        // interpolation fails closed (the official compiler static-folds it).
        let nodes = projection.build_nodes()?;

        // (3) The narrow ops (reactive text / binds / events), with bind/event
        // expressions rewritten through the fallible rewriter.
        let ops = projection.build_ops(&nodes)?;

        // (4) Component context + props-param facts.
        let needs_context = projection.needs_context(&alloc);
        let uses_props = ir
            .analysis
            .bindings
            .all()
            .iter()
            .any(|b| b.kind == BindingRuntimeKind::Prop)
            || needs_context;

        Ok(ClientModulePlan {
            component: ir.component.clone(),
            nodes,
            body_statements,
            ops,
            needs_context,
            uses_props,
            build: projection,
        })
    }

    /// Lower the TYPED supported instance-script items into the narrow
    /// [`ClientScriptItem`] component-body statements.
    ///
    /// The instance script is the strict finite [`SupportedInstanceScriptItem`](super::client_shapes::SupportedInstanceScriptItem)
    /// allowlist (minted by the default-deny classifier); `lower_supported_instance_items`
    /// is a thin per-variant transform over that enum — there is NO broad
    /// statement-rewrite path. A `<script module>` and an instance `import` / `export`
    /// were already refused at the classifier (the script-hoisting deferral), so this
    /// stage emits NO module-script imports / hoists; it produces ONLY the
    /// component-FUNCTION-BODY statements. Infallible — every item was already proven
    /// lowerable by the allowlist classifier.
    fn build_script_items(&self) -> Vec<ClientScriptItem> {
        expr_emit::lower_supported_instance_items(&self.script_items, &self.ir.analysis.bindings)
            .into_iter()
            .map(|code| ClientScriptItem::BodyStatement { code })
            .collect()
    }

    /// Build the narrow node arena (one `ClientNode` per supported IR node, indexed
    /// by the SAME numeric id space so an op's `NodeId` maps to the same
    /// `ClientNodeId`). The reactivity decision per interpolation is made here.
    fn build_nodes(&self) -> Result<Vec<ClientNode>, UnsupportedSvelteRuntimeSurface> {
        // The arena mirrors the IR node arena index-for-index (so `NodeId(n)` →
        // `ClientNodeId(n)`), letting the op projection map node ids trivially and
        // the emitter's walk read each named position's narrow node by IR id.
        let mut nodes = Vec::with_capacity(self.ir.nodes.len());
        for (idx, node) in self.ir.nodes.iter().enumerate() {
            nodes.push(self.project_node(NodeId(idx as u32), node)?);
        }
        Ok(nodes)
    }

    /// Project one supported IR node into its narrow [`ClientNode`]. A node kind the
    /// classifier already refused (component / block / tag / non-options special)
    /// is unreachable here, but is mapped to a defensive refusal rather than a
    /// silent placeholder so a classifier/plan divergence fails loudly.
    fn project_node(
        &self,
        id: NodeId,
        node: &IrNode,
    ) -> Result<ClientNode, UnsupportedSvelteRuntimeSurface> {
        match node {
            IrNode::Text { span, text } => Ok(ClientNode::Text {
                span: *span,
                text: text.clone(),
            }),
            IrNode::Comment { span, text } => Ok(ClientNode::Comment {
                span: *span,
                text: text.clone(),
            }),
            IrNode::Interpolation { span, expr, .. } => {
                // The classifier already proved this interpolation is a bare reactive
                // signal / no-default-prop read (recorded as a `ClientInterpolationShape`
                // fact); a non-reactive or complex interpolation failed closed there.
                // A `ReactiveText` node with NO recorded shape is a classifier/plan
                // divergence — fail closed defensively (never emit an unclassified
                // interpolation).
                if !self.interp_shapes.iter().any(|(n, _)| *n == id) {
                    return Err(UnsupportedSvelteRuntimeSurface::ComplexInterpolation {
                        span: *span,
                    });
                }
                Ok(ClientNode::ReactiveText {
                    span: *span,
                    expr: *expr,
                })
            }
            IrNode::Element(el) => {
                // The classifier already minted the typed `SupportedHtmlElement` fact
                // for this element (the strict-allowlist `try_from` proof). An element
                // node with NO recorded fact is a classifier/plan divergence — fail
                // closed defensively (never project an unclassified element whose tag
                // could become a raw var stem).
                let Some((_, element)) = self.element_facts.iter().find(|(n, _)| *n == id) else {
                    return Err(UnsupportedSvelteRuntimeSurface::Element {
                        tag: el.tag.clone(),
                        span: el.span,
                    });
                };
                let attrs = el
                    .attrs
                    .iter()
                    .map(|a| self.project_attr(&el.tag, a))
                    .collect::<Result<Vec<_>, _>>()?;
                let children = el.children.iter().map(|c| ClientNodeId(c.0)).collect();
                Ok(ClientNode::Element {
                    element: *element,
                    tag: el.tag.clone(),
                    span: el.span,
                    attrs,
                    children,
                })
            }
            IrNode::Special(s) if s.kind == super::ir::SpecialKind::Options => {
                Ok(ClientNode::OptionsMarker { span: s.span })
            }
            // A node the classifier refused — unreachable on the accept path.
            // Fail closed loudly (never a silent placeholder).
            _ => Err(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                construct: "unsupported-node",
                span: Span::new(id.0, id.0),
            }),
        }
    }

    /// Project one supported attribute into its narrow [`ClientAttr`]. The bind /
    /// event expressions are rewritten through the fallible rewriter (a refusal
    /// short-circuits). A reactive (dynamic) attribute was already refused by the
    /// classifier and is mapped to a defensive refusal here.
    fn project_attr(
        &self,
        tag: &str,
        attr: &AttrIr,
    ) -> Result<ClientAttr, UnsupportedSvelteRuntimeSurface> {
        match attr {
            AttrIr::Static { name, value } => Ok(ClientAttr::Static {
                name: name.clone(),
                value: value.as_ref().map(|v| v.value.clone()),
            }),
            AttrIr::Bind { target, .. } => {
                let bind_target = match target.as_str() {
                    "value" => ClientBindTarget::Value,
                    "this" => ClientBindTarget::This,
                    other => {
                        return Err(UnsupportedSvelteRuntimeSurface::Binding {
                            target: other.to_string(),
                            span: Span::new(0, 0),
                        });
                    }
                };
                // The getter/setter rewrite lives on the corresponding op; the
                // element attr records the supported kind only.
                let _ = tag;
                Ok(ClientAttr::Bind {
                    target: bind_target,
                })
            }
            AttrIr::Event {
                event_type,
                delegated,
                capture,
                modifiers,
                ..
            } => {
                if !*delegated || *capture || !modifiers.is_empty() {
                    return Err(UnsupportedSvelteRuntimeSurface::NonDelegatedEvent {
                        event_type: event_type.clone(),
                        span: Span::new(0, 0),
                    });
                }
                // The handler rewrite lives on the corresponding op.
                Ok(ClientAttr::DelegatedEvent {
                    event_type: event_type.clone(),
                })
            }
            // A dynamic attribute / `class={…}` / `style={…}` / `class:` / `style:`
            // directive — the emission lives on the corresponding op; the
            // element attr records the supported KIND only. (The classifier already
            // accepted these, recording the per-attribute dynamic-attr shape.)
            AttrIr::Dynamic { .. }
            | AttrIr::Mixed { .. }
            | AttrIr::Class { .. }
            | AttrIr::Style { .. } => Ok(ClientAttr::Dynamic),
            // Any other attribute kind was refused by the classifier — defensive.
            AttrIr::Spread { .. }
            | AttrIr::Use { .. }
            | AttrIr::Transition { .. }
            | AttrIr::Let { .. } => Err(UnsupportedSvelteRuntimeSurface::DynamicAttribute {
                name: "unsupported-attr".to_string(),
                span: Span::new(0, 0),
            }),
        }
    }

    /// Build the narrow ops from the IR reactive ops, mapping each supported
    /// [`RuntimeOp`] to a [`ClientRuntimeOp`] (bind/event expressions rewritten). A
    /// broad op variant the supported surface never produces is a defensive
    /// refusal.
    fn build_ops(
        &self,
        nodes: &[ClientNode],
    ) -> Result<Vec<ClientRuntimeOp>, UnsupportedSvelteRuntimeSurface> {
        let scope = self.ir.root_scope();
        let scope_lexical = scope.scope;
        // An op whose target is the `<svelte:options>` compile-option MARKER (which
        // renders nothing) is dead — the options attributes (`runes={…}`) lower to a
        // reactive-attr op that never reaches the DOM. Skip those ops (they are the
        // one legitimately-ignorable op the supported surface produces); every OTHER
        // broad op variant is a refusal.
        let is_options_marker = |target: NodeId| {
            matches!(
                nodes.get(target.0 as usize),
                Some(ClientNode::OptionsMarker { .. })
            )
        };
        // The first `class` / `style` op for a target builds the WHOLE coalesced
        // `$.set_class` / `$.set_style` call (reading the element's class/style attrs);
        // subsequent class/style ops for the same target are skipped (one merged call
        // per element, official `RegularElement.js`). These sets track which targets
        // have already emitted their coalesced class/style op.
        let mut class_done: rustc_hash::FxHashSet<NodeId> = rustc_hash::FxHashSet::default();
        let mut style_done: rustc_hash::FxHashSet<NodeId> = rustc_hash::FxHashSet::default();
        // A `Mixed` plain attribute (`id="a{x}b{y}"`) lowers to ONE `ReactiveAttr`
        // op PER expression part in the IR, but the official compiler builds ONE
        // `$.set_attribute` over the WHOLE concatenated value. Dedup by `(target,
        // name)` so the first op for a plain attribute builds the full value and the
        // rest are folded into it.
        let mut plain_attr_done: rustc_hash::FxHashSet<(NodeId, String)> =
            rustc_hash::FxHashSet::default();
        let mut ops = Vec::new();
        for &op_id in &scope.local_ops {
            // Skip any op targeting the options marker (a dead compile-option attr).
            if let Some(target) = op_target_node(self.ir.op(op_id)) {
                if is_options_marker(target) {
                    continue;
                }
            }
            match self.ir.op(op_id) {
                RuntimeOp::ReactiveText { target, expr } => {
                    // Rewrite the interpolation expression at BUILD time (fallible —
                    // an `await` / destructuring write inside `{…}` fails closed
                    // here, before the plan exists). Compute `has_call` for the
                    // memoizer.
                    let analyzed = self.ir.analysis.expressions.get(*expr);
                    let rewritten = self.rewrite(*expr, analyzed.scope)?;
                    let has_call = super::reactive_analysis::expr_has_call(
                        analyzed.source,
                        analyzed.scope,
                        &self.ir.analysis.bindings,
                        &self.ir.analysis.scopes,
                        &self.declared_roots,
                    );
                    ops.push(ClientRuntimeOp::ReactiveText {
                        target: ClientNodeId(target.0),
                        expr: *expr,
                        rewritten,
                        has_call,
                    });
                }
                RuntimeOp::Binding { target, bind } => {
                    let op = self.project_bind_op(*target, bind, scope_lexical)?;
                    ops.push(op);
                }
                RuntimeOp::Event { target, event } => {
                    let op = self.project_event_op(*target, event, scope_lexical)?;
                    ops.push(op);
                }
                // A dynamic attribute / class / style write .
                RuntimeOp::ReactiveAttr { target, attr } => match attr.kind {
                    AttrOpKind::Plain => {
                        // The first op for this `(target, name)` builds the WHOLE
                        // attribute value (the full `Dynamic` / `Mixed` concatenation);
                        // a Mixed attribute's later per-part ops are folded into it.
                        if plain_attr_done.insert((*target, attr.name.clone())) {
                            let op = self.project_reactive_attr_op(*target, &attr.name)?;
                            ops.push(op);
                        }
                    }
                    AttrOpKind::Class => {
                        // The first class op for this element materializes the WHOLE
                        // coalesced `$.set_class`; later class ops are folded into it.
                        if class_done.insert(*target) {
                            let op = self.project_set_class_op(*target)?;
                            ops.push(op);
                        }
                    }
                    AttrOpKind::Style => {
                        if style_done.insert(*target) {
                            let op = self.project_set_style_op(*target)?;
                            ops.push(op);
                        }
                    }
                },
                // A "cannot be set statically" attribute init (`autofocus` /
                // media `muted`) — the §1.2-class non-static-property surface (5a).
                RuntimeOp::NonStaticProperty { target, property } => {
                    let op = self.project_non_static_property_op(*target, property)?;
                    ops.push(op);
                }
                // A broad op the supported surface never produces — defensive
                // refusal (never silently dropped).
                RuntimeOp::SpreadAttrs { .. }
                | RuntimeOp::Attachment { .. }
                | RuntimeOp::Action { .. }
                | RuntimeOp::Transition { .. } => {
                    return Err(UnsupportedSvelteRuntimeSurface::DynamicAttribute {
                        name: "unsupported-op".to_string(),
                        span: Span::new(0, 0),
                    });
                }
            }
        }
        Ok(ops)
    }

    /// Project a `bind:` op into the narrow [`ClientRuntimeOp::Bind`], carrying the
    /// classifier's accepted [`ClientBindShape`] fact.
    fn project_bind_op(
        &self,
        target: NodeId,
        bind: &BindOp,
        _scope: ScopeId,
    ) -> Result<ClientRuntimeOp, UnsupportedSvelteRuntimeSurface> {
        let bind_target = match bind.target.as_str() {
            "value" => ClientBindTarget::Value,
            "this" => ClientBindTarget::This,
            other => {
                return Err(UnsupportedSvelteRuntimeSurface::Binding {
                    target: other.to_string(),
                    span: Span::new(0, 0),
                });
            }
        };
        // The accepted bind SHAPE the classifier recorded for this target node. A
        // bind op with NO recorded shape is a classifier/plan divergence — fail
        // closed defensively (never emit an unclassified bind).
        let shape = self
            .bind_shapes
            .iter()
            .find(|(n, _)| *n == target)
            .map(|(_, s)| s.clone())
            .ok_or_else(|| UnsupportedSvelteRuntimeSurface::Binding {
                target: bind.target.clone(),
                span: Span::new(0, 0),
            })?;
        let (getter, setter) = self.bind_getter_setter(bind.expr, &bind.target)?;
        Ok(ClientRuntimeOp::Bind {
            target: ClientNodeId(target.0),
            bind_target,
            shape,
            getter,
            setter,
        })
    }

    /// Project an event op into the narrow [`ClientRuntimeOp::Event`], carrying the
    /// classifier's accepted [`ClientEventHandlerShape`] fact.
    fn project_event_op(
        &self,
        target: EventTarget,
        event: &EventOp,
        _scope: ScopeId,
    ) -> Result<ClientRuntimeOp, UnsupportedSvelteRuntimeSurface> {
        let EventTarget::Node(node_id) = target else {
            return Err(UnsupportedSvelteRuntimeSurface::NonDelegatedEvent {
                event_type: event.event_type.clone(),
                span: Span::new(0, 0),
            });
        };
        // The accepted handler SHAPE the classifier recorded for this target node. An
        // event op with NO recorded shape is a classifier/plan divergence — fail
        // closed defensively (never emit an unclassified, possibly-non-function
        // handler).
        let shape = self
            .event_shapes
            .iter()
            .find(|(n, _)| *n == node_id)
            .map(|(_, s)| s.clone())
            .ok_or_else(|| UnsupportedSvelteRuntimeSurface::NonDelegatedEvent {
                event_type: event.event_type.clone(),
                span: Span::new(0, 0),
            })?;
        let analyzed = self.ir.analysis.expressions.get(event.handler);
        let handler = self.rewrite(event.handler, analyzed.scope)?;
        Ok(ClientRuntimeOp::Event {
            target: ClientNodeId(node_id.0),
            event_type: event.event_type.clone(),
            shape,
            handler,
        })
    }

    /// The getter + setter bodies for a `bind:` target. The getter is the bound
    /// expression rewritten as a value read. The setter is shaped by the target's
    /// STRUCTURAL lvalue kind (never a raw-text assignment):
    ///
    /// - a bare-IDENTIFIER signal target sets the signal directly
    ///   (`$.set(name, $$value)`);
    /// - a bare-IDENTIFIER non-signal target (a plain local) assigns it directly
    ///   (`name = $$value`);
    /// - a MEMBER target (`obj.x`, `a[i]`) writes through the SAME fallible
    ///   lvalue-aware rewrite as the getter, so a signal-wrapped-proxy member write
    ///   is `$.get(obj).x = $$value` (NOT a raw `obj.x = $$value` — the raw form
    ///   would read the unproxied object and miss the reactive write).
    ///
    /// The lvalue kind is decided STRUCTURALLY from the parsed OXC node (never a
    /// `source.contains('.')` text scan); a non-lvalue target was already refused by
    /// the classifier.
    fn bind_getter_setter(
        &self,
        expr: ExprId,
        _target: &str,
    ) -> Result<(String, String), UnsupportedSvelteRuntimeSurface> {
        let analyzed = self.ir.analysis.expressions.get(expr);
        let getter = self.rewrite(expr, analyzed.scope)?;
        let text = analyzed.source.trim();
        let mut alloc = Allocator::default();
        let setter = match super::expr::classify_bind_target(&alloc, analyzed.source) {
            // A bare identifier: a signal sets directly; a plain local assigns
            // directly. The signal vs plain decision reads the resolved binding kind.
            Some(super::expr::BindTargetKind::Identifier) => {
                let is_signal = self
                    .ir
                    .analysis
                    .bindings
                    .resolve_kind(&self.ir.analysis.scopes, analyzed.scope, text)
                    .is_some_and(is_signal_kind);
                if is_signal {
                    format!("$.set({text}, $$value)")
                } else {
                    format!("{text} = $$value")
                }
            }
            // A member target: the LHS is the bound member expression rewritten as a
            // value read (identical to the getter — `$.get(obj).x`), assigned to.
            // Routing through the rewriter is the fix for the signal-wrapped-proxy
            // member write.
            Some(super::expr::BindTargetKind::Member) => {
                let lvalue = self.rewrite(expr, analyzed.scope)?;
                format!("{lvalue} = $$value")
            }
            // A non-lvalue target was refused by the classifier (`bind:value={f()}`);
            // defensive — fail closed rather than emit `f() = $$value`.
            None => {
                return Err(UnsupportedSvelteRuntimeSurface::Binding {
                    target: text.to_string(),
                    span: Span::new(0, 0),
                });
            }
        };
        alloc.reset();
        Ok((getter, setter))
    }

    /// Whether a template expression references a reactive SIGNAL (the official
    /// `metadata.expression.has_state`). A dynamic attribute / class / style value
    /// with state joins the combined `$.template_effect`; a stateless value is a
    /// one-shot init (`RegularElement.js`'s `has_state ? update : init`).
    fn expr_has_state(&self, expr_id: ExprId) -> bool {
        let analyzed = self.ir.analysis.expressions.get(expr_id);
        // Official `has_state` is set by a reactive signal/prop reference OR by a MEMBER
        // access rooted at any declared binding (`MemberExpression.js`'s `!is_pure(node)`
        // rule — a member on a demoted `$state` / plain local is impure ⇒ has_state, so
        // `{d.x}` joins the `$.template_effect` even though `d` is not a live signal).
        super::reactive_analysis::expr_references_signal(
            analyzed.source,
            analyzed.scope,
            &self.ir.analysis.bindings,
            &self.ir.analysis.scopes,
        ) || super::reactive_analysis::expr_member_roots_at_binding(
            analyzed.source,
            analyzed.scope,
            &self.ir.analysis.bindings,
            &self.ir.analysis.scopes,
        )
    }

    /// Whether a template expression `has_call` (the official
    /// `metadata.expression.has_call`) — the same predicate the reactive-text memoizer
    /// uses. A dynamic attribute / property value that `has_call` is MEMOIZED into the
    /// `$.template_effect(($N) => …, [() => expr])` deps-array form (the official
    /// `build_template_chunk` memoize rule), so the call runs once per dep change.
    fn expr_has_call(&self, expr_id: ExprId) -> bool {
        let analyzed = self.ir.analysis.expressions.get(expr_id);
        super::reactive_analysis::expr_has_call(
            analyzed.source,
            analyzed.scope,
            &self.ir.analysis.bindings,
            &self.ir.analysis.scopes,
            &self.declared_roots,
        )
    }

    /// Build the STRUCTURED dynamic-attribute value for the attribute named `name` on
    /// element `el` — a [`AttrValue::Single`] for a `Dynamic` single expression, or a
    /// [`AttrValue::Mixed`] for a `Mixed` literal+expr value — plus its `has_state`
    /// (whether the value joins the combined effect). Each expression carries its
    /// `has_call` fact so the emitter memoizes it (the official deps-array rule); the
    /// literal chunks of a mixed value are entity-decoded at IR-lowering time.
    fn attr_value_for(
        &self,
        el: &super::ir::ElementIr,
        name: &str,
    ) -> Result<(AttrValue, bool), UnsupportedSvelteRuntimeSurface> {
        for attr in &el.attrs {
            match attr {
                AttrIr::Dynamic { name: n, expr } if n == name => {
                    let rewritten =
                        self.rewrite(*expr, self.ir.analysis.expressions.get(*expr).scope)?;
                    let has_state = self.expr_has_state(*expr);
                    let has_call = self.expr_has_call(*expr);
                    return Ok((
                        AttrValue::Single {
                            rewritten,
                            has_call,
                        },
                        has_state,
                    ));
                }
                AttrIr::Mixed { name: n, parts } if n == name => {
                    return self.mixed_attr_value(parts);
                }
                _ => {}
            }
        }
        Err(UnsupportedSvelteRuntimeSurface::DynamicAttribute {
            name: name.to_string(),
            span: Span::new(0, 0),
        })
    }

    /// Build the value of a `Mixed` (quoted) attribute, mirroring official's
    /// `build_attribute_value`. The chunk count decides the path EXACTLY as official's
    /// `value.length` does:
    ///
    /// - ONE chunk (`id="{d}"` / `class="{d}"`) routes the SINGLE-expression path — a raw
    ///   `build_expression` value ([`AttrValue::Single`], no evaluate-fold, no `?? ''`
    ///   wrap), with `has_state` the expression's own. (A lone literal chunk — a quoted
    ///   value with no interpolation cannot reach here as `Mixed`, but is handled as a
    ///   `Const` defensively.) Official's `value.length === 1` branch does NOT call
    ///   `build_template_chunk`, so it never evaluate-folds.
    /// - MULTI chunk (`id="a {d} b"`) routes `build_template_chunk` — each interpolation is
    ///   evaluate-folded when statically KNOWN (`scope.evaluate`), else kept as a live
    ///   `` ${expr ?? ''} `` part; an all-literal result collapses to a single `Const`.
    ///
    /// Returns the structured value + whether ANY surviving (un-folded) part references
    /// state.
    fn mixed_attr_value(
        &self,
        parts: &[MixedAttrPart],
    ) -> Result<(AttrValue, bool), UnsupportedSvelteRuntimeSurface> {
        // SINGLE-chunk quoted value — the official `value.length === 1` branch: the raw
        // single expression, NOT evaluate-folded and NOT `?? ''`-wrapped.
        if parts.len() == 1 {
            return match &parts[0] {
                MixedAttrPart::Literal(text) => {
                    Ok((AttrValue::Const(js_single_quoted(text)), false))
                }
                MixedAttrPart::Expr(e) => {
                    let analyzed = self.ir.analysis.expressions.get(*e);
                    let rewritten = self.rewrite(*e, analyzed.scope)?;
                    let has_state = self.expr_has_state(*e);
                    Ok((
                        AttrValue::Single {
                            rewritten,
                            has_call: self.expr_has_call(*e),
                        },
                        has_state,
                    ))
                }
            };
        }

        // MULTI-chunk value — the official `build_template_chunk` evaluate-fold path.
        let mut value_parts = Vec::with_capacity(parts.len());
        let mut has_state = false;
        for part in parts {
            match part {
                MixedAttrPart::Literal(text) => {
                    value_parts.push(AttrValuePart::Literal(text.clone()));
                }
                MixedAttrPart::Expr(e) => {
                    let analyzed = self.ir.analysis.expressions.get(*e);
                    let has_call = self.expr_has_call(*e);
                    // Official `build_template_chunk` constant-folds a KNOWN interpolation
                    // into the cooked literal text (`id="a {d + 1} b"` over a demoted
                    // `$state(5)` → `'a 6 b'`) via `scope.evaluate` — but it evaluates the
                    // chunk AFTER memoization (`shared/utils.js`: `memoize(...)` then
                    // `scope.evaluate(value)`). A `has_call` chunk is replaced by a synthetic
                    // `$N` slot BEFORE the evaluate, and `evaluate($N)` resolves to no binding
                    // ⇒ UNKNOWN ⇒ never folds (so `String(d)` over a demoted `$state` stays a
                    // live `String(d)` effect, NOT a folded literal). Only a NON-`has_call`
                    // chunk can fold; an unknown chunk stays live either way.
                    //
                    // The const-fold tri-state contract: `Fold` → the cooked literal;
                    // `Live` (a plain not-foldable chunk OR a ledgered live-fallback) → the
                    // live `?? ''` path; `Refuse` → a deterministic compile refusal (a
                    // compile-time JS throw official also compile-fails — never emit live
                    // code that would crash at runtime).
                    if !has_call {
                        match super::reactive_fold::mixed_chunk_fold(
                            analyzed.source,
                            analyzed.scope,
                            &self.ir.analysis.bindings,
                            &self.ir.analysis.scopes,
                            self.ir.analysis.scripts.instance_source,
                        ) {
                            super::reactive_fold::ChunkFold::Fold(folded) => {
                                value_parts.push(AttrValuePart::Literal(folded));
                                continue;
                            }
                            // Both a plain not-foldable chunk and a ledgered live-fallback
                            // emit the live expression (below); the ledger reason is recorded
                            // in the checked-in `LiveFallbackReason` table.
                            super::reactive_fold::ChunkFold::Live { .. } => {}
                            super::reactive_fold::ChunkFold::Refuse(reason) => {
                                // The span is unused on the accept-path refusal (matching
                                // the other 5a `mixed_attr_value` refusals); the `ExprId`
                                // arena does not carry a source span.
                                return Err(UnsupportedSvelteRuntimeSurface::ConstFoldThrow {
                                    reason: reason.label(),
                                    span: Span::new(0, 0),
                                });
                            }
                        }
                    }
                    let rewritten = self.rewrite(*e, analyzed.scope)?;
                    has_state |= self.expr_has_state(*e);
                    // The `?? ''` coercion for this LIVE part — official's
                    // `build_template_chunk` `is_defined`/precedence rule. A memoized part
                    // (`has_call`) is a `$N` identifier slot, so the paren decision
                    // collapses; a provably-defined part is emitted RAW (no `?? ''`).
                    let coalesce = super::reactive_fold::mixed_chunk_nullish_wrap(
                        analyzed.source,
                        analyzed.scope,
                        &self.ir.analysis.bindings,
                        &self.ir.analysis.scopes,
                        self.ir.analysis.scripts.instance_source,
                        has_call,
                    );
                    value_parts.push(AttrValuePart::Expr {
                        rewritten,
                        has_call,
                        coalesce,
                    });
                }
            }
        }
        // If EVERY part is a literal (every interpolation folded to a known constant),
        // the value is a single STRING literal — official `build_template_chunk` emits
        // `b.literal(cooked)` (`'a 6 b'`) when `expressions.length === 0`, NOT a template
        // literal. Concatenate the cooked text and emit a single-quoted `Const`.
        if value_parts
            .iter()
            .all(|p| matches!(p, AttrValuePart::Literal(_)))
        {
            let cooked: String = value_parts
                .iter()
                .map(|p| match p {
                    AttrValuePart::Literal(t) => t.as_str(),
                    AttrValuePart::Expr { .. } => "",
                })
                .collect();
            return Ok((AttrValue::Const(js_single_quoted(&cooked)), has_state));
        }
        Ok((AttrValue::Mixed(value_parts), has_state))
    }

    /// The IR element node for a target [`NodeId`] (a non-element target is a
    /// classifier/plan divergence — fail closed defensively).
    fn element_for(
        &self,
        target: NodeId,
    ) -> Result<&super::ir::ElementIr, UnsupportedSvelteRuntimeSurface> {
        match self.ir.node(target) {
            IrNode::Element(el) => Ok(el),
            _ => Err(UnsupportedSvelteRuntimeSurface::DynamicAttribute {
                name: "non-element-attr-target".to_string(),
                span: Span::new(0, 0),
            }),
        }
    }

    /// Project a dynamic PLAIN attribute (`AttrIr::Dynamic` / `AttrIr::Mixed`,
    /// `AttrOpKind::Plain`) into its narrow [`ClientRuntimeOp::ReactiveAttr`]. The
    /// emission shape is re-derived from the (deterministic) name classifier — a DOM
    /// property write vs `$.set_attribute`. The value is the WHOLE attribute value
    /// (read from the element's `Dynamic` / `Mixed` attr, not the single op expr) —
    /// a `Dynamic` single expression or a `Mixed` `` `lit${expr ?? ''}lit` ``
    /// template literal. `has_state` is the official `metadata.expression.has_state`.
    fn project_reactive_attr_op(
        &self,
        target: NodeId,
        name: &str,
    ) -> Result<ClientRuntimeOp, UnsupportedSvelteRuntimeSurface> {
        let el = self.element_for(target)?;
        // The element's `Dynamic` / `Mixed` attribute under this name → its STRUCTURED
        // value (each expression carrying its `has_call` fact for the emit-time
        // memoizer) + `has_state`.
        let (value, has_state) = self.attr_value_for(el, name)?;
        // The write is REACTIVE when it references state OR `has_call` (a `has_call`
        // value is memoized into a `$N` placeholder that only the effect can bind — the
        // official `Memoizer.add` rule, which forces even a pure `String(plain_let)`
        // into the render `$.template_effect`).
        let reactive = has_state || value.has_call();
        // Re-derive the emission shape from the name (deterministic, matches the
        // classifier's recorded fact). The span is unused on the accept path.
        let shape = super::client_shapes::classify_dynamic_attr_shape(name, Span::new(0, 0))?;
        let emit = match shape {
            ClientDynamicAttrShape::SetAttribute { name } => {
                ClientDynAttrEmit::SetAttribute { name, value }
            }
            ClientDynamicAttrShape::DomProperty { prop } => {
                ClientDynAttrEmit::Property { prop, value }
            }
            // A PLAIN-kind op is never `autofocus` (autofocus is a `NonStaticProperty`
            // op, projected by `project_non_static_property_op`) — defensively refuse
            // rather than mis-emit it as a reactive write.
            ClientDynamicAttrShape::Autofocus
            // Class / style never reach here (they are `AttrOpKind::Class` / `Style`).
            | ClientDynamicAttrShape::Class
            | ClientDynamicAttrShape::Style => {
                return Err(UnsupportedSvelteRuntimeSurface::DynamicAttribute {
                    name: name.to_string(),
                    span: Span::new(0, 0),
                });
            }
        };
        Ok(ClientRuntimeOp::ReactiveAttr {
            target: ClientNodeId(target.0),
            emit,
            reactive,
        })
    }

    /// Project a `NonStaticProperty` op (`autofocus` / media `muted`, static or
    /// dynamic) into its narrow [`ClientRuntimeOp::ReactiveAttr`]. `autofocus` →
    /// init-only `$.autofocus(node, value)`; a DOM property (`muted`) → `node.<name> =
    /// value`. The init value is `true` (a valueless attr), a literal, or the rewritten
    /// expression.
    fn project_non_static_property_op(
        &self,
        target: NodeId,
        property: &super::ir::NonStaticPropertyOp,
    ) -> Result<ClientRuntimeOp, UnsupportedSvelteRuntimeSurface> {
        // The STRUCTURED value + whether it is reactive. A `Mixed` value retains its
        // FULL ordered literal+expr run; each expression carries `has_call` for the
        // emit-time memoizer.
        let (value, has_state) = self.non_static_property_value(&property.value)?;
        // `autofocus` is init-only regardless of state; a property write (`muted`)
        // joins the effect when its value is stateful OR `has_call` (the official rule
        // — a `has_call` value is memoized and can only live in the effect).
        let init_only = matches!(property.kind, NonStaticPropertyKind::Autofocus);
        let reactive = (has_state || value.has_call()) && !init_only;
        let emit = match property.kind {
            // `autofocus` is ALWAYS init-only `$.autofocus(node, value)` — even a
            // dynamic value (`autofocus={v}`) is read once at init, so it is NEVER
            // memoized; flatten the structured value to a plain emit string.
            NonStaticPropertyKind::Autofocus => ClientDynAttrEmit::Autofocus {
                value: self.flatten_init_attr_value(&value),
            },
            // A DOM property write (`video.muted = value`) — carries the structured
            // value so a `has_call` reactive value memoizes at emit time.
            NonStaticPropertyKind::DomProperty => ClientDynAttrEmit::Property {
                prop: super::client_allowlist::normalize_attribute(&property.name),
                value,
            },
        };
        Ok(ClientRuntimeOp::ReactiveAttr {
            target: ClientNodeId(target.0),
            emit,
            reactive,
        })
    }

    /// Build the STRUCTURED value of a non-static-property op (`autofocus` / `muted`),
    /// plus its `has_state`. A `Boolean` valueless attr is the constant `true`; a
    /// static literal is a quoted constant; a single `Expr` carries its `has_call`; a
    /// `Mixed` value retains its full literal+expr run (each expr with `has_call`).
    fn non_static_property_value(
        &self,
        value: &NonStaticPropertyValue,
    ) -> Result<(AttrValue, bool), UnsupportedSvelteRuntimeSurface> {
        match value {
            NonStaticPropertyValue::Boolean => Ok((AttrValue::Const("true".to_string()), false)),
            NonStaticPropertyValue::Literal(text) => {
                Ok((AttrValue::Const(js_single_quoted(text)), false))
            }
            NonStaticPropertyValue::Expr(expr) => {
                let rewritten =
                    self.rewrite(*expr, self.ir.analysis.expressions.get(*expr).scope)?;
                Ok((
                    AttrValue::Single {
                        rewritten,
                        has_call: self.expr_has_call(*expr),
                    },
                    self.expr_has_state(*expr),
                ))
            }
            NonStaticPropertyValue::Mixed(parts) => self.mixed_attr_value(parts),
        }
    }

    /// Flatten a structured [`AttrValue`] for an INIT-only (`$.autofocus`) emit, where
    /// no effect-side memoizer runs. A `Single` value emits its bare expression; a
    /// `Const` emits verbatim; a `Mixed` value builds the `` `lit${expr ?? ''}lit` ``
    /// template inline (no memoization, since an init-only value is read once).
    fn flatten_init_attr_value(&self, value: &AttrValue) -> String {
        match value {
            AttrValue::Const(text) => text.clone(),
            AttrValue::Single { rewritten, .. } => rewritten.clone(),
            AttrValue::Mixed(parts) => {
                let mut tmpl = String::from("`");
                for part in parts {
                    match part {
                        AttrValuePart::Literal(text) => tmpl.push_str(&escape_template_text(text)),
                        AttrValuePart::Expr { rewritten, .. } => {
                            tmpl.push_str(&format!("${{{rewritten} ?? ''}}"));
                        }
                    }
                }
                tmpl.push('`');
                tmpl
            }
        }
    }

    /// Project the coalesced `$.set_class(node, is_html, value, css_hash, prev, next)`
    /// op for an element. Merges the `class={…}` base attribute (if any) with EVERY
    /// `class:` directive into ONE call, matching the official `build_set_class`. The
    /// supported surface is HTML-only (`is_html = 1`); scoped CSS is refused upstream
    /// (5l), so `css_hash` is `null` only when directives are present (the official
    /// `!css_hash && next` rule), else absent. Produces the SEMANTIC pieces; the
    /// emitter assembles the final call with the real DOM var + accumulator name.
    fn project_set_class_op(
        &self,
        target: NodeId,
    ) -> Result<ClientRuntimeOp, UnsupportedSvelteRuntimeSurface> {
        let el = self.element_for(target)?;
        // The base `class` attribute (a `Static` / `Dynamic` / `Mixed` named `class`),
        // and every `class:` directive, in source order.
        let mut base_value: Option<AttrValue> = None;
        let mut base_has_state = false;
        let mut directives: Vec<(String, String)> = Vec::new();
        let mut dir_has_state = false;
        let mut directives_has_call = false;
        for attr in &el.attrs {
            match attr {
                AttrIr::Static { name, value } if name == "class" => {
                    // A static `class` consumed as the `$.set_class` BASE value is a
                    // runtime JS-STRING argument (NOT a baked skeleton attr), so its
                    // HTML entities DECODE — the same `decode_attr_entities` the mixed
                    // literal chunks already use (`class="a&amp;b"` → base `'a&b'`).
                    let lit = value.as_ref().map(|v| v.value.as_str()).unwrap_or("");
                    base_value = Some(AttrValue::Const(js_single_quoted(&decode_attr_entities(
                        lit,
                    ))));
                }
                AttrIr::Dynamic { name, expr } if name == "class" => {
                    let v = self.rewrite(*expr, self.ir.analysis.expressions.get(*expr).scope)?;
                    // Official `Attribute.js` sets `needs_clsx` for a single-expression
                    // `class={…}` UNLESS the value is a `Literal` / `TemplateLiteral` /
                    // `BinaryExpression`: a `class={a + b}` string-concatenation, a
                    // `class={'x'}` literal, and a `` class={`a${b}`} `` template emit the
                    // value RAW (no `$.clsx` wrap); every other shape IS wrapped. When
                    // wrapped, the whole `$.clsx(expr)` wrap is the base value — a
                    // `has_call` base memoizes the WHOLE wrap (`[() => $.clsx(call)]`, the
                    // official `build_set_class`).
                    let analyzed = self.ir.analysis.expressions.get(*expr);
                    let rewritten =
                        if super::reactive_analysis::class_value_needs_clsx(analyzed.source) {
                            format!("$.clsx({v})")
                        } else {
                            v
                        };
                    base_value = Some(AttrValue::Single {
                        rewritten,
                        has_call: self.expr_has_call(*expr),
                    });
                    base_has_state |= self.expr_has_state(*expr);
                }
                AttrIr::Mixed { name, parts } if name == "class" => {
                    // A MIXED-string class (`class="a {x} b"`) is already a string
                    // template — official `needs_clsx` is FALSE for it, so it is NOT
                    // wrapped in `$.clsx` (verified against svelte@5.56.3). The
                    // structured value memoizes each EXPRESSION PART at emit time, not
                    // the whole rendered template.
                    let (mixed, st) = self.mixed_attr_value(parts)?;
                    base_value = Some(mixed);
                    base_has_state |= st;
                }
                AttrIr::Class { name, condition } => {
                    let cond = match condition {
                        Some(e) => {
                            dir_has_state |= self.expr_has_state(*e);
                            directives_has_call |= self.expr_has_call(*e);
                            self.rewrite(*e, self.ir.analysis.expressions.get(*e).scope)?
                        }
                        // A value-less shorthand `class:foo` with no synthesized
                        // condition is a defensive empty (lowering always synthesizes
                        // one) — skip it.
                        None => continue,
                    };
                    directives.push((object_key(name), cond));
                }
                _ => {}
            }
        }
        let has_directives = !directives.is_empty();
        // The `value` arg: the structured base value, or `''` when only directives are
        // present.
        let value = base_value.unwrap_or_else(|| AttrValue::Const("''".to_string()));
        let directives_has_call = directives_has_call && has_directives;
        // The op is REACTIVE when any contributor references state OR `has_call` (the
        // base or any directive) — the official rule that forces the effect +
        // memoization (and the accumulator) even over a pure-call/plain-let surface.
        let reactive = base_has_state || dir_has_state || value.has_call() || directives_has_call;
        // css_hash: `null` when directives are present (scoped CSS is refused upstream,
        // so there is never a real hash); absent otherwise.
        let css_hash = has_directives.then(|| "null".to_string());
        // The directives object `{ foo: cond, ... }`; absent when no directives.
        let directives_obj = has_directives.then(|| {
            let entries = directives
                .iter()
                .map(|(k, v)| format!("{k}: {v}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{ {entries} }}")
        });
        // The reactive-directive path needs the `let classes;` accumulator (used for
        // BOTH the `prev` arg and the `<name> =` assignment); a non-reactive directive
        // path passes `{}` as `prev` (no accumulator).
        let accumulator_stem = (has_directives && reactive).then_some("classes");
        Ok(ClientRuntimeOp::SetClass {
            target: ClientNodeId(target.0),
            value,
            css_hash,
            directives: directives_obj,
            directives_has_call,
            reactive,
            accumulator_stem,
        })
    }

    /// Project the coalesced `$.set_style(node, value, prev, next)` op for an element
    /// . Merges the `style={…}` base attribute (if any) with EVERY `style:`
    /// directive into ONE call, matching the official `build_set_style`. The
    /// `|important` directives split into the `[normal, important]` array `next`;
    /// custom / hyphenated property keys are quoted.
    fn project_set_style_op(
        &self,
        target: NodeId,
    ) -> Result<ClientRuntimeOp, UnsupportedSvelteRuntimeSurface> {
        let el = self.element_for(target)?;
        let mut base_value: Option<AttrValue> = None;
        let mut base_has_state = false;
        // Normal + important directive entries (key already quoted as needed).
        let mut normal: Vec<(String, String)> = Vec::new();
        let mut important: Vec<(String, String)> = Vec::new();
        let mut dir_has_state = false;
        let mut directives_has_call = false;
        for attr in &el.attrs {
            match attr {
                AttrIr::Static { name, value } if name == "style" => {
                    // A static `style` consumed as the `$.set_style` BASE value is a
                    // runtime JS-STRING argument (NOT a baked skeleton attr), so its
                    // HTML entities DECODE (`style="q:'&quot;'"` → base `'q:\'"\''`).
                    let lit = value.as_ref().map(|v| v.value.as_str()).unwrap_or("");
                    base_value = Some(AttrValue::Const(js_single_quoted(&decode_attr_entities(
                        lit,
                    ))));
                }
                AttrIr::Dynamic { name, expr } if name == "style" => {
                    let v = self.rewrite(*expr, self.ir.analysis.expressions.get(*expr).scope)?;
                    // The whole dynamic expression is the base value; a `has_call` base
                    // memoizes the whole expression.
                    base_value = Some(AttrValue::Single {
                        rewritten: v,
                        has_call: self.expr_has_call(*expr),
                    });
                    base_has_state |= self.expr_has_state(*expr);
                }
                AttrIr::Mixed { name, parts } if name == "style" => {
                    // The structured mixed value memoizes each EXPRESSION PART at emit
                    // time, not the whole rendered template.
                    let (mixed, st) = self.mixed_attr_value(parts)?;
                    base_value = Some(mixed);
                    base_has_state |= st;
                }
                AttrIr::Style {
                    property,
                    value,
                    important: is_important,
                } => {
                    let v = match value {
                        Some(e) => {
                            dir_has_state |= self.expr_has_state(*e);
                            directives_has_call |= self.expr_has_call(*e);
                            self.rewrite(*e, self.ir.analysis.expressions.get(*e).scope)?
                        }
                        None => continue,
                    };
                    let entry = (object_key(property), v);
                    if *is_important {
                        important.push(entry);
                    } else {
                        normal.push(entry);
                    }
                }
                _ => {}
            }
        }
        let has_directives = !normal.is_empty() || !important.is_empty();
        // The `value` arg: the structured base value, or `''` when only directives are
        // present (the official `build_set_style` passes an empty-string base then).
        let value = base_value.unwrap_or_else(|| AttrValue::Const("''".to_string()));
        let directives_has_call = directives_has_call && has_directives;
        // The op is REACTIVE when any contributor references state OR `has_call`.
        let reactive = base_has_state || dir_has_state || value.has_call() || directives_has_call;
        // The directives object, or the `[normal, important]` array when any
        // `|important` directive is present (the official `build_style_directives_object`).
        let directives_obj = has_directives.then(|| {
            let normal_obj = style_object(&normal);
            if important.is_empty() {
                normal_obj
            } else {
                format!("[{}, {}]", normal_obj, style_object(&important))
            }
        });
        let accumulator_stem = (has_directives && reactive).then_some("styles");
        Ok(ClientRuntimeOp::SetStyle {
            target: ClientNodeId(target.0),
            value,
            directives: directives_obj,
            directives_has_call,
            reactive,
            accumulator_stem,
        })
    }

    /// Rewrite one template expression to its emitted client form through the
    /// FALLIBLE rewriter, threading the per-instance proxy-init map (so a
    /// template-side reassignment matches the official `should_proxy(rhs)`).
    pub(super) fn rewrite(
        &self,
        expr_id: ExprId,
        _scope: ScopeId,
    ) -> Result<String, UnsupportedSvelteRuntimeSurface> {
        let analyzed = self.ir.analysis.expressions.get(expr_id);
        expr_rewrite::rewrite_expression_full(
            analyzed.source,
            analyzed.scope,
            &self.ir.analysis.bindings,
            &self.ir.analysis.scopes,
            &self.prop_reads,
            &self.proxy_inits,
        )
        .map(|r| r.text)
    }

    /// Whether the component needs a component context (`$.push`/`$.pop`) — the
    /// official `needs_context` analysis over the instance script + every template
    /// expression.
    fn needs_context(&self, alloc: &Allocator) -> bool {
        let template_expr_sources: Vec<&str> = self
            .ir
            .analysis
            .expressions
            .all()
            .iter()
            .map(|e| e.source)
            .collect();
        super::reactive_analysis::needs_context(
            alloc,
            self.ir.analysis.scripts.instance_source,
            &template_expr_sources,
        )
    }
}
