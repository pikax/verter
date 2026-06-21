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
use super::client_shapes::{ClientBindShape, ClientEventHandlerShape, ClientInterpolationShape};
use super::client_surface::ClassifiedClientSurface;
use super::expr::{BindingRuntimeKind, ScopeId};
use super::expr_emit;
use super::expr_rewrite::{self, PropReads, ProxyInitMap};
use super::ir::{
    AttrIr, BindOp, EventOp, EventTarget, ExprId, IrNode, NodeId, RuntimeOp, SvelteRuntimeIr,
};
use verter_span::Span;

/// A node in the NARROW client node arena — the closed template-node vocabulary
/// the emitter walks. Every supported [`IrNode`] projects to exactly one of these;
/// the broad-IR variants (component / block / tag / non-options special) never
/// reach the plan (they were refused by the classifier).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ClientNode {
    /// A literal text run.
    Text {
        /// The source span.
        span: Span,
        /// The text content.
        text: String,
    },
    /// An HTML comment.
    Comment {
        /// The source span.
        span: Span,
        /// The comment text.
        text: String,
    },
    /// A reactive escaped interpolation (`{expr}`). The reactivity decision was
    /// made at build time (a non-reactive interpolation fails closed before the
    /// plan is built), so every `ReactiveText` node in the plan IS reactive.
    ReactiveText {
        /// The source span.
        span: Span,
        /// The interpolated expression id (into the IR expression arena; the plan
        /// reads it back through the build-time analysis for the op rewrite).
        expr: ExprId,
    },
    /// An intrinsic element. The element is a TYPED [`SupportedHtmlElement`] fact (the
    /// classifier's `try_from` proof), so the emitter reads the DOM var stem from
    /// [`SupportedHtmlElement::var_stem`] — never the raw tag string. The `tag` is
    /// retained for the template SERIALIZATION + the whitespace-context namespace
    /// decision (`for_children_of`), which are HTML-tag concerns, not var stems.
    Element {
        /// The typed accepted element (the SOLE source of the DOM var stem).
        element: SupportedHtmlElement,
        /// The tag name (for serialization + child-namespace context only).
        tag: String,
        /// The full open-tag source span.
        span: Span,
        /// The narrow attributes.
        attrs: Vec<ClientAttr>,
        /// The child node ids (into the plan's narrow node arena).
        children: Vec<ClientNodeId>,
    },
    /// The `<svelte:options>` compile-option marker — consumed, renders nothing
    /// (carried so the node arena mirrors the IR node-id space; the walk skips it).
    OptionsMarker {
        /// The source span.
        span: Span,
    },
}

/// A node id into the plan's narrow node arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct ClientNodeId(pub(super) u32);

/// A narrow supported attribute on a [`ClientNode::Element`]. The bind / event
/// REWRITES live on the [`ClientRuntimeOp`]s (the emitter sequences ops, not
/// element attrs); the element attr records the supported KIND so the narrow node
/// tree is a faithful structural mirror. The static attribute carries its literal
/// (folded into the template HTML by the planner).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ClientAttr {
    /// A truly-static attribute (folded into the static template HTML).
    Static {
        /// The attribute name.
        name: String,
        /// The literal value (`None` for a valueless boolean attribute).
        value: Option<String>,
    },
    /// `bind:value` on an `<input>` (or element `bind:this`) — the getter/setter
    /// rewrite is on the corresponding [`ClientRuntimeOp::Bind`].
    Bind {
        /// The bind target (`value` / `this`).
        target: ClientBindTarget,
    },
    /// A modern delegated DOM event — the handler rewrite is on the corresponding
    /// [`ClientRuntimeOp::Event`].
    DelegatedEvent {
        /// The normalized event type (`click`, …).
        event_type: String,
    },
}

/// The supported bind target kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ClientBindTarget {
    /// `bind:value`.
    Value,
    /// `bind:this`.
    This,
}

/// A narrow supported script item — a single emitted component-FUNCTION-BODY
/// statement (already lowered to its final client JS text). The supported instance
/// script is the strict finite [`SupportedInstanceScriptItem`](super::client_shapes::SupportedInstanceScriptItem)
/// allowlist; a `<script module>` / instance `import` / `export` is fail-closed
/// upstream (the script-hoisting deferral), so the closed body vocabulary is a single
/// `BodyStatement` variant — the plan carries the emitted string, the emitter only
/// sequences it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ClientScriptItem {
    /// An emitted component-FUNCTION-BODY statement (a supported `$state` declaration
    /// or a `bind:this` clone-root local) — already lowered to its final client JS
    /// text.
    BodyStatement {
        /// The emitted statement.
        code: String,
    },
}

impl ClientScriptItem {
    /// The emitted statement text for this script item.
    pub(super) fn code(&self) -> &str {
        match self {
            Self::BodyStatement { code } => code,
        }
    }
}

/// A narrow supported reactive runtime op — the closed op vocabulary the emitter
/// consumes. Every supported [`RuntimeOp`] projects to one of these (with its
/// expressions already rewritten); the broad-op variants never reach the plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ClientRuntimeOp {
    /// Reactive text content for an interpolation's text node. The interpolated
    /// expression was REWRITTEN at build time (the fallible rewrite — an `await` /
    /// destructuring write inside an interpolation fails closed BEFORE the plan is
    /// built), so the emit-time memoizer consumes the already-rewritten text.
    ReactiveText {
        /// The target node id (into the plan node arena).
        target: ClientNodeId,
        /// The interpolated expression id (the emit-time text-run partition reads it
        /// back through the build analysis for the mixed-run template assembly).
        expr: ExprId,
        /// The already-rewritten expression text (the value the memoizer routes
        /// inline or hoists into a `$N` placeholder).
        rewritten: String,
        /// Whether the expression `has_call` (drives the memoizer deps-array form).
        has_call: bool,
    },
    /// A `bind:value` / `bind:this` op.
    Bind {
        /// The target node id.
        target: ClientNodeId,
        /// The bind target kind.
        bind_target: ClientBindTarget,
        /// The accepted bind SHAPE fact (from the default-deny classifier) — the
        /// typed sub-shape the op was admitted as, carried so the op is a typed
        /// classification, not just a rewritten string pair.
        shape: ClientBindShape,
        /// The rewritten getter body.
        getter: String,
        /// The rewritten setter body.
        setter: String,
    },
    /// A delegated event registration.
    Event {
        /// The target node id.
        target: ClientNodeId,
        /// The normalized event type.
        event_type: String,
        /// The accepted handler SHAPE fact (from the default-deny classifier) — the
        /// typed sub-shape (arrow / function-expr / local-fn-ident) the handler was
        /// admitted as, carried so the emitter consumes a typed shape, not just a
        /// rewritten string.
        shape: ClientEventHandlerShape,
        /// The rewritten handler body.
        handler: String,
    },
}

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
            // Any other attribute kind was refused by the classifier — defensive.
            _ => Err(UnsupportedSvelteRuntimeSurface::DynamicAttribute {
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
                // A broad op the supported surface never produces — defensive
                // refusal (never silently dropped).
                _ => {
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

/// The DOM-node target of a runtime op (for the dead-options-attr skip). A
/// global-target event (`<svelte:window>` etc.) has no DOM node target — `None`
/// (such ops belong to a refused special surface anyway).
fn op_target_node(op: &RuntimeOp) -> Option<NodeId> {
    match op {
        RuntimeOp::ReactiveText { target, .. }
        | RuntimeOp::ReactiveAttr { target, .. }
        | RuntimeOp::SpreadAttrs { target, .. }
        | RuntimeOp::Binding { target, .. }
        | RuntimeOp::Attachment { target, .. }
        | RuntimeOp::Action { target, .. }
        | RuntimeOp::Transition { target, .. }
        | RuntimeOp::NonStaticProperty { target, .. } => Some(*target),
        RuntimeOp::Event { target, .. } => match target {
            EventTarget::Node(node) => Some(*node),
            _ => None,
        },
    }
}

/// Whether a binding kind is a reactive SIGNAL (read via `$.get`).
fn is_signal_kind(kind: BindingRuntimeKind) -> bool {
    matches!(
        kind,
        BindingRuntimeKind::StateSignal { .. }
            | BindingRuntimeKind::StateProxy
            | BindingRuntimeKind::Derived
            | BindingRuntimeKind::EachSignal
            | BindingRuntimeKind::AwaitSignal
            | BindingRuntimeKind::LegacyConstDerived
    )
}
