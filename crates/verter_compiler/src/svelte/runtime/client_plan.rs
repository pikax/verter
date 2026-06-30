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
    escape_template_text, js_single_quoted, object_key, op_target_node, style_object,
};
use super::client_shapes::{
    ClientBindShape, ClientDynamicAttrShape, ClientEventHandlerShape, ClientInterpolationShape,
};
use super::client_surface::ClassifiedClientSurface;
use super::entity_decode::decode_attr_entities;
use super::events::{validate_event_modifiers, EventModifierError};
use super::expr::{BindingRuntimeKind, ScopeId};
use super::expr_emit;
use super::expr_rewrite::{PropReads, ProxyInitMap};
use super::ir::{
    AttrIr, AttrOpKind, ExprId, IrNode, MixedAttrPart, NodeId, NonStaticPropertyKind,
    NonStaticPropertyValue, OpId, RuntimeOp, StyleDirectiveValue, SvelteRuntimeIr,
};
use verter_span::Span;

// The narrow client-plan VOCABULARY (the closed node / attribute / op / value type set
// the emitter consumes) lives in the sibling `client_plan_types` module; this builder
// projects the broad IR onto it. Re-exported so existing consumers (`super::client`, …)
// keep importing the vocabulary as `super::client_plan::<Type>`.
pub(super) use super::client_plan_types::{
    AttrValue, AttrValuePart, ClientAttr, ClientBindTarget, ClientBlock, ClientDynAttrEmit,
    ClientNode, ClientNodeId, ClientRuntimeOp, ClientScriptItem, EventEmit, EventEmitTarget,
    EventMode, RegionOps,
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
    /// The narrow reactive ops, grouped by their owning template-scope REGION (the root
    /// region plus every block body / branch region), in source order within each region.
    /// A block body's reactive surface is its OWN region: the emitter builds each region's
    /// combined `$.template_effect` + binds + events from its region's ops.
    pub(super) region_ops: Vec<RegionOps>,
    /// The module-scope USER imports (the `.svelte`-component-default subset), in SOURCE
    /// ORDER — emitted above the component function, immediately after the runtime namespace
    /// import. Empty for a component with no `.svelte` imports.
    pub(super) user_imports: Vec<super::client_plan_types::UserImport>,
    /// Top-level `{#snippet}` defs that CAN hoist (capture only their params) — emitted as
    /// MODULE-scope `const` declarations between the imports and the `$.from_html` hoists,
    /// in source order. The node ids index `nodes` / the IR.
    pub(super) module_snippets: Vec<NodeId>,
    /// Top-level `{#snippet}` defs that CAPTURE component state / props — emitted as
    /// INSTANCE-scope `const` declarations at the top of the component function body
    /// (before the script statements), in source order.
    pub(super) instance_snippets: Vec<NodeId>,
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
    /// The accepted event-handler shape per (target node, event type, handler expr) —
    /// the classifier's typed FACT, keyed precisely so an element with multiple events
    /// resolves each to its own shape; the op projection carries it onto each
    /// [`ClientRuntimeOp::Event`].
    pub(super) event_shapes: Vec<(NodeId, String, ExprId, ClientEventHandlerShape)>,
    /// The accepted bind shape per target node (the classifier's typed FACT) — the
    /// op projection carries it onto each [`ClientRuntimeOp::Bind`].
    pub(super) bind_shapes: Vec<(NodeId, String, ClientBindShape)>,
    /// The `bind:group` value literal per group-input node — the emitter writes
    /// `input.value = input.__value = '<value>'` per input and declares the
    /// component-fn-scoped `const binding_group = []` when this is non-empty.
    pub(super) group_values: Vec<(NodeId, String)>,
    /// The `bind:group` DYNAMIC/mixed `value={…}` per group-input node — the structured
    /// value + reactivity the emitter renders as the change-tracked `$.template_effect`
    /// update (reactive) or one-shot inline write (non-reactive), plus the group getter's
    /// dynamic-value dependency read. Built from `classified.group_dynamic_value_nodes` by
    /// reading each node's `value` attr through the shared `attr_value_for`.
    pub(super) group_dynamic_values: Vec<(NodeId, super::client_plan_types::GroupDynamicValue)>,
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
    pub(super) script_items: Vec<super::instance_items::SupportedInstanceScriptItem>,
    /// The accepted `{@html}` node ids (the classifier's typed FACT) — proves each
    /// raw-markup node is supported in its position, so the plan projects a
    /// [`ClientRuntimeOp::Html`] / [`ClientNode::RawHtml`] instead of refusing.
    pub(super) html_nodes: Vec<NodeId>,
    /// The accepted spread-attribute element node ids (the classifier's typed FACT) —
    /// each such element folds its WHOLE attribute set into a single
    /// [`ClientRuntimeOp::AttributeEffect`]; the per-attribute ops the IR also produced
    /// for these elements are suppressed.
    pub(super) spread_elements: Vec<NodeId>,
    /// The COMPONENT-FUNCTION-scoped pair INDEX counter for function-pair component binds
    /// (`bind:x={get, set}`). Each pair consumes one index in source order across EVERY
    /// component call in the component function; the emitter mints the `var bind_get` / `var
    /// bind_set` locals from that index through the shared scope-aware allocator (so two
    /// function-binds never alias the same `var`, AND the names never collide with a user
    /// binding — the official `state.scope.generate('bind_get')` per-function uniquing). This
    /// is a stable INDEX, NOT a name: the names are minted at emit time, never here.
    /// Interior-mutable because the projection walks nodes through `&self`.
    pub(super) fn_pair_bind_seq: std::cell::Cell<usize>,
}

impl<'a> ClientModulePlan<'a> {
    /// The reactive ops owned by a specific template-scope region (empty when the region
    /// has no ops). The emitter reads this per region so a block body's effect is built
    /// from the body's ops, never the root's.
    pub(super) fn ops_in(&self, scope: super::ir::TemplateScopeId) -> &[ClientRuntimeOp] {
        self.region_ops
            .iter()
            .find(|r| r.scope_id == scope)
            .map_or(&[], |r| r.ops.as_slice())
    }

    /// Every reactive op across all regions, in region-then-source order. Used by the
    /// by-unique-target lookups (a node lives in one region, so a flat scan resolves it).
    pub(super) fn all_ops(&self) -> impl Iterator<Item = &ClientRuntimeOp> {
        self.region_ops.iter().flat_map(|r| r.ops.iter())
    }
}

/// The per-element first-op dedup state threaded through `project_scope_op` (one
/// coalesced `$.set_class` / `$.set_style` / `$.set_attribute` / `$.attribute_effect`
/// per element). Global across regions — an element lives in exactly one region.
#[derive(Default)]
struct OpDedup {
    /// Targets whose coalesced `$.set_class` has been emitted.
    class_done: rustc_hash::FxHashSet<NodeId>,
    /// Targets whose coalesced `$.set_style` has been emitted.
    style_done: rustc_hash::FxHashSet<NodeId>,
    /// `(target, attr-name)` pairs whose whole plain-attribute value has been emitted.
    plain_attr_done: rustc_hash::FxHashSet<(NodeId, String)>,
    /// Spread elements whose `$.attribute_effect` fold has been emitted.
    spread_attrs_done: rustc_hash::FxHashSet<NodeId>,
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
        let mut projection = SupportedClientIr {
            ir,
            prop_reads,
            proxy_inits,
            declared_roots,
            event_shapes: classified.event_shapes.clone(),
            bind_shapes: classified.bind_shapes.clone(),
            group_values: classified.group_values.clone(),
            group_dynamic_values: Vec::new(),
            interp_shapes: classified.interp_shapes.clone(),
            element_facts: classified.element_facts.clone(),
            script_items: classified.script_items.clone(),
            html_nodes: classified.html_nodes.clone(),
            spread_elements: classified.spread_elements.clone(),
            fn_pair_bind_seq: std::cell::Cell::new(0),
        };
        // The `bind:group` DYNAMIC/mixed values — built here (not in the classifier) because
        // it needs the rewriter + reactivity analysis the projection owns. Each node's `value`
        // attr is read through the shared `attr_value_for`; a non-emittable value fails closed.
        projection.group_dynamic_values =
            projection.collect_group_dynamic_values(&classified.group_dynamic_value_nodes)?;

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
        // there are no module-scope imports / hoists). A function-pair function body
        // lowers through the fallible rewriter, so this is fallible.
        let body_statements = projection.build_script_items()?;

        // (2) The narrow node arena (mirrors the supported IR node space). The
        // reactivity decision for each interpolation is made here: a non-reactive
        // interpolation fails closed (the official compiler static-folds it).
        let nodes = projection.build_nodes()?;

        // (3) The narrow ops (reactive text / binds / events), grouped per region, with
        // bind/event expressions rewritten through the fallible rewriter.
        let region_ops = projection.build_ops(&nodes)?;

        // (4) Component context + props-param facts.
        let needs_context = projection.needs_context(&alloc);
        let uses_props = ir
            .analysis
            .bindings
            .all()
            .iter()
            .any(|b| b.kind == BindingRuntimeKind::Prop)
            || needs_context;

        let (module_snippets, instance_snippets) = projection.collect_top_level_snippets();

        Ok(ClientModulePlan {
            component: ir.component.clone(),
            nodes,
            body_statements,
            region_ops,
            user_imports: classified.user_imports.clone(),
            module_snippets,
            instance_snippets,
            needs_context,
            uses_props,
            build: projection,
        })
    }

    /// Lower the TYPED supported instance-script items into the narrow
    /// [`ClientScriptItem`] component-body statements, IN SOURCE ORDER.
    ///
    /// The instance script is the strict finite [`SupportedInstanceScriptItem`](super::instance_items::SupportedInstanceScriptItem)
    /// allowlist (minted by the default-deny classifier); the lowering is a thin
    /// per-variant transform over that enum — there is NO broad statement-rewrite path. A
    /// `<script module>` and an instance `import` / `export` were already refused at the
    /// classifier (the script-hoisting deferral), so this stage emits NO module-script
    /// imports / hoists; it produces ONLY the component-FUNCTION-BODY statements.
    ///
    /// Every variant except `FunctionDecl` is a rewriter-FREE transform
    /// ([`lower_simple_instance_item`](expr_emit::lower_simple_instance_item)). A
    /// `FunctionDecl` (a named function referenced by a DOM function-pair bind) lowers its
    /// BODY through the FALLIBLE expression rewriter ([`rewrite_source`](Self::rewrite_source))
    /// rooted at the instance-script scope — so a signal read/write inside the body
    /// becomes `$.get`/`$.set` (`function get() { return $.get(value); }`), NEVER verbatim.
    /// FALLIBLE: a function body using an unsupported form refuses.
    fn build_script_items(&self) -> Result<Vec<ClientScriptItem>, UnsupportedSvelteRuntimeSurface> {
        use super::instance_items::SupportedInstanceScriptItem as Item;
        use expr_emit::SimpleItemLowering;
        let root_scope = self.ir.root_scope().scope;
        let mut items = Vec::new();
        for item in &self.script_items {
            match expr_emit::lower_simple_instance_item(item, &self.ir.analysis.bindings) {
                SimpleItemLowering::Statement(code) => {
                    items.push(ClientScriptItem::BodyStatement { code });
                }
                SimpleItemLowering::None => {}
                SimpleItemLowering::NeedsRewriter => {
                    let Item::FunctionDecl { source, .. } = item else {
                        // `NeedsRewriter` is produced ONLY for `FunctionDecl`; any other
                        // item reaching here is a classifier/lowering divergence.
                        unreachable!("only FunctionDecl needs the rewriter")
                    };
                    // The function declaration lowers through the shared rewriter (its
                    // body's signal reads/writes rewrite; the `function name(...) {}`
                    // structure is preserved). The rewriter wraps the source as an
                    // expression internally, so a declaration's source round-trips as a
                    // function expression with the body edits applied.
                    let code = self.rewrite_source(source, root_scope)?;
                    items.push(ClientScriptItem::BodyStatement { code });
                }
            }
        }
        Ok(items)
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
            IrNode::Interpolation { span, expr, escape } => {
                // A RAW interpolation (`{@html}` in interpolation form — accepted as a
                // raw-html node) projects to a `RawHtml` node. (The template lowering
                // produces every `{@html}` as a `TagIr::Html` node, so this is a defensive
                // mirror of the dominant raw-html path.)
                if *escape == super::ir::EscapeMode::Raw {
                    return Ok(ClientNode::RawHtml {
                        span: *span,
                        expr: *expr,
                    });
                }
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
            // A `{@html expr}` raw-markup tag (accepted by the classifier) projects to a
            // `RawHtml` node so the DOM walk can reach its `<!>` anchor (or recognise it
            // as the controlled sole child of its parent).
            IrNode::Tag(super::ir::TagIr::Html { expr }) => Ok(ClientNode::RawHtml {
                span: Span::new(id.0, id.0),
                expr: *expr,
            }),
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
            // A static `<Foo …/>` component invocation — projected to a `Component` node.
            IrNode::Component(c) => self.project_component(c),
            // The component-invocation specials (`<svelte:component>` / `<svelte:self>`) —
            // projected to a `Component` node. A standalone `<svelte:fragment>` (the
            // transparent-wrapper surface) + the host / renderable `<svelte:*>` specials (not
            // yet supported) are refused below.
            IrNode::Special(s)
                if matches!(
                    s.kind,
                    super::ir::SpecialKind::Component | super::ir::SpecialKind::SelfRef
                ) =>
            {
                self.project_special_component(s)
            }
            // A `{@render}` tag — projected to a `Render` node.
            IrNode::Tag(super::ir::TagIr::Render { callee, args, .. }) => {
                self.project_render(callee, args)
            }
            // The control-flow blocks (`{#if}`/`{#each}`/`{#await}`/`{#key}`) — projected
            // (head expressions rewritten, child regions carried by scope id) in
            // `client_block_plan`. A `{#snippet}` is the component/snippet surface, refused
            // upstream.
            IrNode::Block(block) if !matches!(block, super::ir::BlockIr::Snippet { .. }) => {
                self.project_block(block)
            }
            // A `{#snippet}` DECLARATION — non-rendering (dropped from the walk); its const
            // is emitted by `emit_snippet_decl`. The arena placeholder mirrors the node id.
            IrNode::Block(super::ir::BlockIr::Snippet { .. }) => Ok(ClientNode::SnippetDecl {
                span: Span::new(id.0, id.0),
            }),
            // The declaration / debug tags: `{@const}` (block-local derived), the
            // `{const}/{let}` declaration tag (inert), and `{@debug}` (reactive snapshot
            // effect).
            IrNode::Tag(super::ir::TagIr::LegacyConst { pattern, init }) => {
                self.project_const_tag(*pattern, *init)
            }
            IrNode::Tag(super::ir::TagIr::Declaration { kind, declarators }) => {
                self.project_declaration_tag(*kind, declarators)
            }
            IrNode::Tag(super::ir::TagIr::Debug { args }) => self.project_debug_tag(args),
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
                // The COARSE structural-mirror kind: `bind:this` (render-side, emitted
                // inline) vs any DOM value/property bind (post-walk, routed by its
                // op's `RuntimeBindRouting`). The PRECISE helper routing + getter/setter
                // rewrite live on the corresponding `ClientRuntimeOp::Bind` shape; this
                // narrow attr records the family only. Acceptance is owned by the
                // classifier (the op carries the recorded shape); an unsupported bind
                // never reaches here (it failed closed at classification).
                let bind_target = if target == "this" {
                    ClientBindTarget::This
                } else {
                    ClientBindTarget::DomValue
                };
                let _ = tag;
                Ok(ClientAttr::Bind {
                    target: bind_target,
                })
            }
            AttrIr::Event {
                event_type,
                delegated,
                modifiers,
                ..
            } => {
                // The legacy modifier set is validated against the official rules (an
                // unknown modifier / `passive`+`preventDefault` / `passive`+`nonpassive`
                // is refused). The PRECISE emission (capture / passive / wrapper stack /
                // rewritten handler) lives on the corresponding `ClientRuntimeOp::Event`'s
                // `EventEmit`; this narrow attr records the coarse KIND only (the event
                // type + the delegated-vs-direct mode), mirroring `ClientAttr::Bind`.
                if matches!(
                    validate_event_modifiers(modifiers),
                    Err(EventModifierError::Unknown(_)
                        | EventModifierError::InvalidPassiveCombination { .. })
                ) {
                    return Err(UnsupportedSvelteRuntimeSurface::NonDelegatedEvent {
                        event_type: event_type.clone(),
                        span: Span::new(0, 0),
                    });
                }
                let mode = if *delegated {
                    EventMode::Delegated
                } else {
                    EventMode::Direct
                };
                Ok(ClientAttr::Event {
                    event_type: event_type.clone(),
                    mode,
                })
            }
            // A dynamic attribute / `class={…}` / `style={…}` / `class:` / `style:`
            // directive, OR a spread `{...x}` — the emission lives on the corresponding op
            // (a per-attribute write, or the coalesced `$.attribute_effect` fold for a
            // spread element); the element attr records the supported KIND only. (The
            // classifier already accepted these.)
            AttrIr::Dynamic { .. }
            | AttrIr::Mixed { .. }
            | AttrIr::Class { .. }
            | AttrIr::Style { .. }
            | AttrIr::Spread { .. } => Ok(ClientAttr::Dynamic),
            // Any other attribute kind was refused by the classifier — defensive.
            AttrIr::Use { .. } | AttrIr::Transition { .. } | AttrIr::Let { .. } => {
                Err(UnsupportedSvelteRuntimeSurface::DynamicAttribute {
                    name: "unsupported-attr".to_string(),
                    span: Span::new(0, 0),
                })
            }
        }
    }

    /// Build the narrow ops from the IR reactive ops, mapping each supported
    /// [`RuntimeOp`] to a [`ClientRuntimeOp`] (bind/event expressions rewritten). A
    /// broad op variant the supported surface never produces is a defensive
    /// refusal.
    fn build_ops(
        &self,
        nodes: &[ClientNode],
    ) -> Result<Vec<super::client_plan_types::RegionOps>, UnsupportedSvelteRuntimeSurface> {
        // The per-element first-op dedup sets are GLOBAL across regions: an element lives
        // in exactly one region, so a per-region set would be identical. (One coalesced
        // `$.set_class` / `$.set_style` / `$.set_attribute` / `$.attribute_effect` per
        // element — official `RegularElement.js`.)
        let mut dedup = OpDedup::default();
        // Project EVERY template-scope region's ops — the root region PLUS every block
        // body / branch region. Each op's expressions were rewritten in the op's OWN
        // recorded scope, so a body op reads `$.get(item)` while a root op reads the root
        // signal; the per-region grouping lets the emitter build each region's combined
        // `$.template_effect` + binds + events independently.
        // The owning region of every node (so a `{@html}` tag's `$.html` op is projected
        // into ITS region — root-level OR a block body — not always the root).
        let node_region = self.build_node_region_map();
        let mut regions = Vec::with_capacity(self.ir.template_scopes.len());
        for scope_idx in 0..self.ir.template_scopes.len() {
            let scope_id = super::ir::TemplateScopeId(scope_idx as u32);
            let region = &self.ir.template_scopes[scope_idx];
            let scope_lexical = region.scope;
            let mut ops = Vec::new();
            for &op_id in &region.local_ops {
                if let Some(op) = self.project_scope_op(op_id, scope_lexical, &mut dedup, nodes)? {
                    ops.push(op);
                }
            }
            // The `{@html}` raw-markup nodes have NO IR runtime op (they are tag NODES), so
            // their `$.html` ops are projected into THIS region when they belong to it, in IR
            // node-id (source) order. Each is a distinct `ClientRuntimeOp::Html` carrying its
            // already-assembled payload (a `() => expr` thunk, or the bare elided callee) and
            // its only-child topology flag.
            let mut html_ids: Vec<NodeId> = self
                .html_nodes
                .iter()
                .copied()
                .filter(|n| node_region.get(n).copied() == Some(scope_id))
                .collect();
            html_ids.sort_by_key(|n| n.0);
            for node_id in html_ids {
                ops.push(self.project_html_op(node_id)?);
            }
            regions.push(super::client_plan_types::RegionOps { scope_id, ops });
        }
        Ok(regions)
    }

    /// Map every template node to its OWNING template-scope region — the scope whose
    /// roots-subtree reaches it WITHOUT crossing a nested block boundary (a block body is a
    /// separate region). Used to route a `{@html}` op into its region.
    fn build_node_region_map(&self) -> rustc_hash::FxHashMap<NodeId, super::ir::TemplateScopeId> {
        let mut map = rustc_hash::FxHashMap::default();
        for scope_idx in 0..self.ir.template_scopes.len() {
            let scope_id = super::ir::TemplateScopeId(scope_idx as u32);
            for root in self.ir.template_scopes[scope_idx].roots.clone() {
                self.assign_node_region(root, scope_id, &mut map);
            }
        }
        map
    }

    /// Assign `node` (and its same-region element/component/special children) to `scope_id`.
    /// A nested block's body is a SEPARATE region (already assigned from its own scope), so
    /// the descent does not cross into it.
    fn assign_node_region(
        &self,
        node: NodeId,
        scope_id: super::ir::TemplateScopeId,
        map: &mut rustc_hash::FxHashMap<NodeId, super::ir::TemplateScopeId>,
    ) {
        map.insert(node, scope_id);
        match self.ir.node(node) {
            IrNode::Element(el) => {
                for &child in &el.children {
                    self.assign_node_region(child, scope_id, map);
                }
            }
            // A component-family node's SLOT content lives in its slot REGIONS (their own
            // scopes), so it self-assigns via the per-scope loop in
            // `build_node_region_map` — NOT here (recursing children would mis-assign the
            // slot content to the PARENT region). The component node itself stays in this
            // region.
            IrNode::Component(_) | IrNode::Special(_) => {}
            _ => {}
        }
    }

    /// Project ONE supported `RuntimeOp` into its narrow `ClientRuntimeOp` (or `None`
    /// when it is a dead options-marker op, a spread-absorbed per-attribute op, or a
    /// dedup'd later class/style/attr/spread op). The op's expressions are rewritten in
    /// their OWN recorded scope; `scope_lexical` is the op's region lexical scope (passed
    /// to the bind/event projectors). Fail closed on a broad op the surface never produces.
    fn project_scope_op(
        &self,
        op_id: OpId,
        scope_lexical: ScopeId,
        dedup: &mut OpDedup,
        nodes: &[ClientNode],
    ) -> Result<Option<ClientRuntimeOp>, UnsupportedSvelteRuntimeSurface> {
        // Skip an op targeting the `<svelte:options>` compile-option MARKER (a dead attr
        // that never reaches the DOM), and absorb a spread element's per-attribute ops
        // into the single `$.attribute_effect` fold (the `SpreadAttrs` op is the trigger).
        if let Some(target) = op_target_node(self.ir.op(op_id)) {
            if matches!(
                nodes.get(target.0 as usize),
                Some(ClientNode::OptionsMarker { .. })
            ) {
                return Ok(None);
            }
            if self.spread_elements.contains(&target)
                && !matches!(self.ir.op(op_id), RuntimeOp::SpreadAttrs { .. })
            {
                return Ok(None);
            }
        }
        let mut out = None;
        match self.ir.op(op_id) {
            RuntimeOp::ReactiveText { target, expr } => {
                // Rewrite the interpolation expression at BUILD time (fallible — an
                // `await` / destructuring write inside `{…}` fails closed here, before
                // the plan exists). Compute `has_call` for the memoizer.
                let analyzed = self.ir.analysis.expressions.get(*expr);
                let rewritten = self.rewrite(*expr, analyzed.scope)?;
                let has_call = super::reactive_analysis::expr_has_call(
                    analyzed.source,
                    analyzed.scope,
                    &self.ir.analysis.bindings,
                    &self.ir.analysis.scopes,
                    &self.declared_roots,
                );
                out = Some(ClientRuntimeOp::ReactiveText {
                    target: ClientNodeId(target.0),
                    expr: *expr,
                    rewritten,
                    has_call,
                });
            }
            RuntimeOp::Binding { target, bind } => {
                out = Some(self.project_bind_op(*target, bind, scope_lexical)?);
            }
            RuntimeOp::Event { target, event } => {
                out = Some(self.project_event_op(*target, event, scope_lexical)?);
            }
            // A dynamic attribute / class / style write.
            RuntimeOp::ReactiveAttr { target, attr } => match attr.kind {
                AttrOpKind::Plain => {
                    // A `bind:group` input's DYNAMIC/mixed `value` is NOT a generic
                    // reactive attr — it is the group-value source, emitted as the
                    // change-tracked `$.template_effect` update + the bind getter
                    // dependency read (see `group_dynamic_values` / the `Bind` op). Skip
                    // the generic reactive-attr projection for it (which would mis-route
                    // `value` through the form-control refusal).
                    let is_group_value = attr.name == "value"
                        && self.group_dynamic_values.iter().any(|(n, _)| *n == *target);
                    // The first op for this `(target, name)` builds the WHOLE attribute
                    // value (the full `Dynamic` / `Mixed` concatenation); a Mixed
                    // attribute's later per-part ops are folded into it.
                    if !is_group_value && dedup.plain_attr_done.insert((*target, attr.name.clone()))
                    {
                        out = Some(self.project_reactive_attr_op(*target, &attr.name)?);
                    }
                }
                AttrOpKind::Class => {
                    // The first class op for this element materializes the WHOLE coalesced
                    // `$.set_class`; later class ops are folded into it.
                    if dedup.class_done.insert(*target) {
                        out = Some(self.project_set_class_op(*target)?);
                    }
                }
                AttrOpKind::Style => {
                    if dedup.style_done.insert(*target) {
                        out = Some(self.project_set_style_op(*target)?);
                    }
                }
            },
            // A non-single-expression style directive trigger (static-text OR mixed) — the
            // coalesced `$.set_style` projection fires once per element (same `style_done`
            // dedup as the reactive style path), reading every style directive.
            RuntimeOp::StyleDirectiveTrigger { target } => {
                if dedup.style_done.insert(*target) {
                    out = Some(self.project_set_style_op(*target)?);
                }
            }
            // A "cannot be set statically" attribute init (`autofocus` / media `muted`) —
            // the §1.2-class non-static-property surface (5a).
            RuntimeOp::NonStaticProperty { target, property } => {
                out = Some(self.project_non_static_property_op(*target, property)?);
            }
            // A spread element folds its WHOLE attribute set (in source order) into a
            // single `$.attribute_effect`. The IR emits one `SpreadAttrs` op per spread;
            // the FIRST one materializes the whole fold, later ones are skipped.
            RuntimeOp::SpreadAttrs { target, .. } => {
                if dedup.spread_attrs_done.insert(*target) {
                    out = Some(self.project_attribute_effect_op(*target)?);
                }
            }
            // A broad op the supported surface never produces — defensive refusal (never
            // silently dropped).
            RuntimeOp::Attachment { .. }
            | RuntimeOp::Action { .. }
            | RuntimeOp::Transition { .. } => {
                return Err(UnsupportedSvelteRuntimeSurface::DynamicAttribute {
                    name: "unsupported-op".to_string(),
                    span: Span::new(0, 0),
                });
            }
        }
        Ok(out)
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
    pub(super) fn expr_has_call(&self, expr_id: ExprId) -> bool {
        let analyzed = self.ir.analysis.expressions.get(expr_id);
        super::reactive_analysis::expr_has_call(
            analyzed.source,
            analyzed.scope,
            &self.ir.analysis.bindings,
            &self.ir.analysis.scopes,
            &self.declared_roots,
        )
    }

    /// Build the `bind:group` DYNAMIC/mixed value ([`GroupDynamicValue`]) for each recorded
    /// group-input node — the structured value (via the shared [`attr_value_for`](Self::attr_value_for))
    /// plus its reactivity (`has_state || has_call`, the official `RegularElement.js` rule). A
    /// node whose `value` attr is not an emittable dynamic/mixed value fails closed (the
    /// classifier only records a node that carried one, so the `?` is defensive).
    ///
    /// [`GroupDynamicValue`]: super::client_plan_types::GroupDynamicValue
    fn collect_group_dynamic_values(
        &self,
        nodes: &[NodeId],
    ) -> Result<
        Vec<(NodeId, super::client_plan_types::GroupDynamicValue)>,
        UnsupportedSvelteRuntimeSurface,
    > {
        let mut out = Vec::with_capacity(nodes.len());
        for &node in nodes {
            let IrNode::Element(el) = self.ir.node(node) else {
                continue;
            };
            let (value, has_state) = self.attr_value_for(el, "value")?;
            let reactive = has_state || value.has_call();
            // The outer `?? ''` group-value coercion is gated on DEFINEDNESS (official
            // `evaluated.is_defined`), NOT single-vs-mixed: a provably-defined SINGLE value
            // omits it. Reuse the SAME `mixed_chunk_nullish_wrap` definedness analysis the
            // mixed-attribute parts run (no new analysis path) — meaningful only for a single
            // value (a mixed value is already a string and never carries the outer coercion).
            let single_value_defined =
                matches!(value, AttrValue::Single { .. }) && self.group_value_single_is_defined(el);
            out.push((
                node,
                super::client_plan_types::GroupDynamicValue {
                    value,
                    reactive,
                    single_value_defined,
                },
            ));
        }
        Ok(out)
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
                    // A VALUE-position rewrite: source-preserving (author parens kept; a
                    // top-level sequence is wrapped once so it stays one value).
                    let rewritten = self.rewrite_value_preserving_source(*expr)?;
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
    pub(super) fn mixed_attr_value(
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
                    // The single-chunk quoted value is a VALUE position — source-preserving
                    // (author parens kept; a top-level sequence is wrapped once).
                    let rewritten = self.rewrite_value_preserving_source(*e)?;
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
                    // A template-literal `${…}` interpolation is a VALUE position —
                    // source-preserving (author parens kept; a top-level sequence is wrapped
                    // once). The `?? ''` coalesce decision below peels parens of its own
                    // (`unwrap_parens`) for its precedence check, so it is unaffected.
                    let rewritten = self.rewrite_value_preserving_source(*e)?;
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
    pub(super) fn element_for(
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
                // A VALUE position (`defaultValue={…}`; the `$.autofocus(node, value)` init
                // likewise) — source-preserving (author parens kept; sequence wrapped once).
                let rewritten = self.rewrite_value_preserving_source(*expr)?;
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
                    // literal chunks already use (`class="a&amp;b"` → base `'a&b'`). A
                    // VALUELESS `class` (`value: None` — `<div class class:on={c}>`) is the
                    // RAW boolean base `true`, distinct from a present empty-string `class=""`
                    // (`Some("")`) which stays `''`.
                    base_value = Some(match value {
                        None => AttrValue::Const("true".to_string()),
                        Some(v) => {
                            AttrValue::Const(js_single_quoted(&decode_attr_entities(&v.value)))
                        }
                    });
                }
                AttrIr::Dynamic { name, expr } if name == "class" => {
                    // The `$.set_class` BASE value is a VALUE position — source-preserving
                    // (author parens kept; sequence wrapped once). The `needs_clsx` decision
                    // below reads the UNWRAPPED-ROOT KIND fact (computed on the
                    // transparent-paren-unwrapped root), so a parenthesized literal / binary /
                    // template is correctly classified as no-clsx.
                    let v = self.rewrite_value_preserving_source(*expr)?;
                    // Official `Attribute.js` sets `needs_clsx` for a single-expression
                    // `class={…}` UNLESS the value is a `Literal` / `TemplateLiteral` /
                    // `BinaryExpression`: a `class={a + b}` string-concatenation, a
                    // `class={'x'}` literal, and a `` class={`a${b}`} `` template emit the
                    // value RAW (no `$.clsx` wrap); every other shape IS wrapped. When
                    // wrapped, the whole `$.clsx(expr)` wrap is the base value — a
                    // `has_call` base memoizes the WHOLE wrap (`[() => $.clsx(call)]`, the
                    // official `build_set_class`).
                    let analyzed = self.ir.analysis.expressions.get(*expr);
                    let rewritten = if super::reactive_analysis::class_value_needs_clsx(
                        analyzed.unwrapped_root_kind,
                    ) {
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
                            // The directive condition is a VALUE position — source-preserving
                            // (author parens kept; sequence wrapped once).
                            self.rewrite_value_preserving_source(*e)?
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
                    // HTML entities DECODE (`style="q:'&quot;'"` → base `'q:\'"\''`). A
                    // VALUELESS `style` (`value: None` — `<div style style:color={c}>`) is the
                    // RAW boolean base `true`, distinct from a present empty-string `style=""`
                    // (`Some("")`) which stays `''`.
                    base_value = Some(match value {
                        None => AttrValue::Const("true".to_string()),
                        Some(v) => {
                            AttrValue::Const(js_single_quoted(&decode_attr_entities(&v.value)))
                        }
                    });
                }
                AttrIr::Dynamic { name, expr } if name == "style" => {
                    // The `$.set_style` BASE value is a VALUE position — source-preserving
                    // (author parens kept; sequence wrapped once).
                    let v = self.rewrite_value_preserving_source(*expr)?;
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
                        StyleDirectiveValue::Expr(e) => {
                            dir_has_state |= self.expr_has_state(*e);
                            directives_has_call |= self.expr_has_call(*e);
                            // A VALUE position — source-preserving (author parens kept;
                            // sequence wrapped once).
                            self.rewrite_value_preserving_source(*e)?
                        }
                        // A static-text style value folds as a quoted string literal
                        // (`{ color: 'red' }`) — no state / call flags.
                        StyleDirectiveValue::Text(text) => js_single_quoted(text),
                        // A MIXED text+interpolation value (`style:color="a{x}b"`) folds as
                        // the template-literal `` `a${x ?? ''}b` ``, built through the shared
                        // mixed-value + fold-text path with NO memoizer (the effect re-runs).
                        StyleDirectiveValue::Mixed(parts) => {
                            let (mixed, st) = self.mixed_attr_value(parts)?;
                            dir_has_state |= st;
                            // A `has_call` interpolation inside the template forces the
                            // effect (the official memoizer rule), exactly as a base mixed
                            // value does.
                            directives_has_call |= mixed.has_call();
                            self.fold_attr_value_text(&mixed)
                        }
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
}
