//! The DEFAULT-DENY client syntax classifier.
//!
//! [`ClientSyntaxSurface::classify`] walks the parsed component ([`ParsedSvelte`])
//! and the runtime IR ([`SvelteRuntimeIr`]) and decides, NODE BY NODE / ATTR BY
//! ATTR / SCRIPT-ITEM BY SCRIPT-ITEM, whether EVERY surface is in the supported
//! allowlist. It is the structural choke point of the refuse-by-default design: it
//! returns the typed [`ClassifiedClientSurface`] facts ONLY when the whole
//! component is supported; the FIRST unsupported surface returns a typed
//! [`UnsupportedSvelteRuntimeSurface`]. There is NO wildcard arm that accepts — an
//! unrecognised node / attribute / rune form is a refusal, never a pass.
//!
//! Because the downstream [`ClientModulePlan`](super::client_plan::ClientModulePlan)
//! is built ONLY from a [`ClassifiedClientSurface`], an unsupported surface has NO
//! emission type — emit-by-default is structurally impossible (the emitter never
//! sees the broad IR).
//!
//! The classifier drives EVERY decision from the typed parse tree + the typed IR +
//! the OXC script AST — never a raw-source scan.

use std::cell::RefCell;

use oxc_allocator::Allocator;

use super::client::UnsupportedSvelteRuntimeSurface;
use super::client_allowlist::{is_svelte_reserved_word, SupportedHtmlElement, SupportedStaticAttr};
use super::client_plan_types::UserImport;
use super::client_shapes::{
    self, ClientBindShape, ClientDynamicAttrShape, ClientEventHandlerShape,
    ClientInterpolationShape, ClientPropsUsage,
};
use super::client_surface_element_query::{
    element_carries_is_attribute, element_has_class_directive, element_has_group_bind,
    element_has_spread, element_has_style_directive, element_own_namespace,
};
use super::client_surface_refuse::{
    namespace_label, refuse_invalid_animate_placement, refuse_invalid_self_placement, refuse_tag,
    refuse_transition_conflicts, refuse_unsupported_special_content, special_label,
};
use super::client_surface_script::{classify_props_usage, classify_script_items};
use super::client_surface_special::{
    classify_special_host, classify_svelte_boundary, classify_svelte_element, classify_svelte_head,
};
use super::events::{validate_event_modifiers, EventModifierError};
use super::html::{synthesize_region, TemplateFactory};
use super::instance_items::{self, SupportedInstanceScriptItem};
use super::ir::{
    AttrIr, EscapeMode, ExprId, IrNode, NodeId, SpecialKind, SvelteRuntimeIr, TagIr,
    TemplateScopeId,
};
use super::whitespace::{
    clean_nodes, determine_namespace_for_children, CleanContext, CleanItem, Namespace,
};
use verter_span::Span;

/// The TYPED accepted facts the default-deny classifier produces when (and only
/// when) every surface of the component is in the supported allowlist.
///
/// This is intentionally a confirmation token the downstream
/// [`SupportedClientIr::build`](super::client_plan::SupportedClientIr) requires: a
/// `ClassifiedClientSurface` can ONLY be minted by a successful default-deny walk,
/// so the semantic-projection / plan stages are UNREACHABLE for an unsupported
/// component. It carries the typed accepted SHAPE FACTS (the per-handler
/// [`ClientEventHandlerShape`], the per-bind [`ClientBindShape`], the
/// [`ClientPropsUsage`] prop-usage fact) — the proof-of-classification PLUS the sub-shape the
/// downstream plan/emitter consumes, so emission reads a typed shape, never
/// re-classifies a generic string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClassifiedClientSurface {
    /// The accepted element fact per element node — the typed
    /// [`SupportedHtmlElement`] the strict element allowlist's `try_from` minted. The
    /// plan carries it onto each [`ClientNode::Element`] so the emitter reads the DOM
    /// var stem from [`SupportedHtmlElement::var_stem`], never the raw tag string.
    pub(super) element_facts: Vec<(NodeId, SupportedHtmlElement)>,
    /// The accepted event-handler shape per (target node, event type, handler expr) —
    /// the FACT an event op consumes. The key includes the event TYPE and the handler
    /// expression id because ONE element can carry MULTIPLE events
    /// (`<button onfocus={a} onclick={b}>`), each with its own handler and routing;
    /// keying on the node alone would collapse them onto the element's FIRST recorded
    /// event.
    pub(super) event_shapes: Vec<(NodeId, String, ExprId, ClientEventHandlerShape)>,
    /// The accepted bind shape per (target node, bind NAME) — the FACT a `bind:` op
    /// consumes. The bind NAME is part of the key because ONE element can carry
    /// MULTIPLE binds (e.g. `<video bind:currentTime bind:paused bind:duration>`), each
    /// with its own routing; keying on the node alone would collapse them.
    pub(super) bind_shapes: Vec<(NodeId, String, ClientBindShape)>,
    /// The `bind:group` VALUE literal per group-input node — the `value="X"` source the
    /// emitter writes as `input.value = input.__value = 'X'` (the static `value` is
    /// stripped from the skeleton). Empty for a non-group component.
    pub(super) group_values: Vec<(NodeId, String)>,
    /// The `bind:group` input nodes carrying a DYNAMIC/mixed `value={…}` — the plan reads
    /// each node's `value` attr from the IR and builds the structured `GroupDynamicValue`
    /// (the static-literal case stays in `group_values`). Empty for a non-group component or
    /// a group with only static values.
    pub(super) group_dynamic_value_nodes: Vec<NodeId>,
    /// The accepted interpolation shape per interpolation node — the FACT proving
    /// the interpolation is a bare signal / no-default-prop read (the §1.2-class
    /// reactive-text surface), carried so the plan reads a typed classification.
    pub(super) interp_shapes: Vec<(NodeId, ClientInterpolationShape)>,
    /// The accepted dynamic-attribute / class / style shape per (node, attribute
    /// index) — the FACT a reactive-attribute / `$.set_class` / `$.set_style` /
    /// `$.autofocus` op consumes. The attribute index is the position of
    /// the attribute in the element's `AttrIr` list, so a per-attribute op maps back
    /// to its accepted emission shape. The plan coalesces the class/style entries.
    pub(super) dynamic_attr_shapes: Vec<(NodeId, usize, ClientDynamicAttrShape)>,
    /// The accepted `{@html}` node ids — the FACT a `$.html` op consumes (the proof the
    /// raw-markup tag is supported in this position). The plan reads the payload
    /// expression + the only-child topology from the IR; this is the per-node acceptance
    /// proof so an unclassified `{@html}` cannot reach the plan.
    pub(super) html_nodes: Vec<NodeId>,
    /// The accepted spread-attribute element node ids — the FACT a `$.attribute_effect`
    /// op consumes. A spread on an element switches its WHOLE attribute strategy (every
    /// co-located attribute folds into the single effect); the plan reads the
    /// source-ordered attribute list from the IR, this is the per-element acceptance
    /// proof.
    pub(super) spread_elements: Vec<NodeId>,
    /// The accepted `$props()` usage fact — no INSTANCE-SCRIPT prop reference
    /// outside the `$props()` declaration itself, and no `bind:` target
    /// resolving to a prop local. TEMPLATE prop writes are supported: a written
    /// prop is a PROP SOURCE, lowered through the `$.prop` getter/setter.
    pub(super) props_usage: ClientPropsUsage,
    /// The TYPED supported instance-script items — the strict finite allowlist the
    /// downstream `SupportedClientIr::build_script_items` consumes (the SOLE
    /// instance-script lowering input). Minted ONLY by a successful
    /// `classify_supported_instance_items` walk, so the lowering can NEVER see an
    /// out-of-allowlist statement (a function / class / enum / `$:` label / plain
    /// local fails closed at the classifier, never reaches lowering).
    pub(super) script_items: Vec<SupportedInstanceScriptItem>,
    /// The admitted module-scope USER imports (the `.svelte`-component-default subset), in
    /// SOURCE ORDER — the typed prelude carrier the plan emits above the component function.
    /// Empty for a component with no `.svelte` imports.
    pub(super) user_imports: Vec<UserImport>,
    /// The `$host`-usage FACT the rune scan recorded: an ADMITTED zero-arg
    /// `$host()` call in the instance script or any template expression (the
    /// only accepted `$host` form — every other shape refused during the scan,
    /// and admission requires an active custom element). The plan build gates
    /// the `$$props`-parameter decision on this fact — the rewritten
    /// `$$props.$$host` member must never reference an unbound `$$props`, so a
    /// host use without an independent props-parameter trigger fails closed
    /// there.
    pub(super) uses_host: bool,
    /// The span of the FIRST admitted `$host()` call (in the scanned program's
    /// own coordinates — the same space the rune scan's refusal spans use),
    /// for the plan build's degenerate-host refusal.
    pub(super) first_host_span: Option<Span>,
}

impl ClientSyntaxSurface {
    /// Run the DEFAULT-DENY classification over a parsed component + its runtime IR.
    ///
    /// Returns the typed accepted facts iff EVERY surface is supported; the first
    /// unsupported surface returns its typed [`UnsupportedSvelteRuntimeSurface`].
    pub(super) fn classify(
        ir: &SvelteRuntimeIr,
    ) -> Result<ClassifiedClientSurface, UnsupportedSvelteRuntimeSurface> {
        // (1) Mode: legacy (non-runes) is the 5i vertical.
        if ir.component.mode == super::ir::SvelteMode::Legacy {
            return Err(UnsupportedSvelteRuntimeSurface::LegacyMode {
                span: Span::new(0, 0),
            });
        }

        // (2) Script-item classification: scan every instance + module script
        // declarator/statement for an unsupported shape. The basic no-default
        // `$props()` form, ALL primitive-literal `$state` declarators
        // (multi-declarator scanned), and the advanced rune forms are gated here
        // BEFORE lowering. A default `.svelte` component import is ADMITTED (the
        // component-callee subset, returned as the typed prelude carrier); every other
        // instance-script import form + a `<script module>` are the broad
        // static-import-prelude deferral (not yet supported) and fail closed here.
        // The returned facts also carry `uses_host` — the admitted zero-arg
        // `$host()` usage the rune scan recorded (instance + template
        // expressions), consumed by the plan build.
        let script_facts = classify_script_items(ir)?;

        // (3) `$props()` USAGE: an INSTANCE-SCRIPT prop reference (outside the
        // `$props()` declaration itself) and a BOUND prop (official's 2-arg
        // `$.bind_value(input, label)` form) fail closed here. A TEMPLATE prop
        // WRITE is supported — it makes the prop a PROP SOURCE (the flag-4
        // `updated` axis), lowered through the getter/setter by the projection.
        let props_usage = classify_props_usage(ir)?;

        // (4) Template-node classification: walk every node + attribute, ACCUMULATING
        // the per-node accepted event/bind/interp shape facts. Every node maps to a
        // supported `ClientNodeKind` or REFUSES (no wildcard accept).
        //
        // The DECLARED module + instance top-level locals are computed ONCE here (they
        // are loop-invariant — they depend only on the module/instance script source,
        // which does not change during the walk) and threaded down to the per-attr bind
        // classifier, rather than re-parsing the scripts per bind attribute.
        let alloc = Allocator::default();
        let declared_root_names = super::reactive_analysis::collect_declared_root_names(
            &alloc,
            ir.analysis.scripts.module_source,
            ir.analysis.scripts.instance_source,
        );
        // (4a) `animate:` PLACEMENT gate (pre-pass): the official `AnimateDirective`
        // analyze rules — one `animate:` per element (`animation_duplicate`), the
        // animated element the ONLY significant child of an `{#each}` body
        // (`animation_invalid_placement`), and that each KEYED
        // (`animation_missing_key`). Runs BEFORE the node walk so a misplaced
        // `animate:` refuses with its placement identity (the per-attr classifier
        // accepts the directive itself), and the each-FLAG widening downstream only
        // ever sees a validated placement.
        if let Some(surface) = refuse_invalid_animate_placement(ir) {
            return Err(surface);
        }
        let facts = RefCell::new(SurfaceFacts::default());
        // The `slot`-attribute placement FACTS recorded at lowering: the STATIC
        // slot-FILLER hosts (the lowered ids of exactly the DIRECT static-slot
        // children of a component-family node, any kind), the DIRECT
        // component-child set (fillers plus implicit default content), and the DIRECT
        // `{#snippet}`-body child set (static-only component-owned placement).
        // Together they drive the official disposition in the unified choke-point —
        // filler routing, plain-prop acceptance, the snippet-static branch, and the
        // fail-closed rejects. The gate must NEVER key on lowered
        // slot-region-root membership: region roots include a transparent
        // `<svelte:fragment slot>`'s hoisted children, which do not inherit
        // direct-child slot placement.
        let slot_placement = SlotPlacementFacts {
            static_slot_filler_hosts: &ir.static_slot_filler_hosts,
            direct_slot_attr_child_hosts: &ir.direct_slot_attr_child_hosts,
            direct_snippet_slot_attr_child_hosts: &ir.direct_snippet_slot_attr_child_hosts,
        };
        for (idx, scope) in ir.template_scopes.iter().enumerate() {
            // A region root is the COMPONENT root or a BLOCK BODY root — the placement axis
            // the declaration-tag gate validates against. (A block body is its OWN scope, so
            // its roots are region roots, never `Nested`; element children recurse `Nested`.)
            let placement = if TemplateScopeId(idx as u32) == ir.root {
                NodePlacement::ComponentRoot
            } else {
                NodePlacement::BlockBodyRoot
            };
            for &root in &scope.roots {
                classify_node(
                    ir,
                    root,
                    Namespace::Html,
                    &declared_root_names,
                    slot_placement,
                    &facts,
                    placement,
                )?;
            }
        }

        // (5) ROOT-REGION emission shape: the root template region's clone-frame is
        // emitted as `var <region> = root();`, which calls `root()` as a FACTORY
        // FUNCTION. That is correct ONLY for a `from_html` factory (the cloned
        // element / multi-root fragment). The official text-first (`$.text()`) and
        // comment-anchor (`$.comment()`) root shapes bind `root` to a NODE, not a
        // factory — calling `root()` on a node is `TypeError: root is not a function`
        // — and reach the runtime via the official text-first (`$.text()` / `$.next()`)
        // topology, a distinct emission shape (5q). The node-level walk accepts the
        // bare-text / escaped-interpolation / empty SHAPE, so this region-level check
        // fails the non-`from_html` root shapes closed. (A `from_html` element /
        // fragment root and a standalone `<Component>` / `{@render}` root stay
        // supported.)
        if let Some(surface) = refuse_unsupported_root_region(ir) {
            return Err(surface);
        }

        // (5b) `<svelte:self>` PLACEMENT gate: the official `svelte_self_invalid_placement`
        // rule — a `<svelte:self>` may only appear inside an `{#if}` / `{#each}` /
        // `{#snippet}` block or a slot passed to a component. At the component root (or
        // nested only in elements at the root, or in an `{#await}` / `{#key}` block with no
        // valid ancestor) the official compiler HARD-ERRORS, so Verter fails it closed
        // rather than emitting the recursive self-call for an input the official rejects.
        if let Some(surface) = refuse_invalid_self_placement(ir) {
            return Err(surface);
        }

        // (6) Instance-script item allowlist: classify EVERY top-level instance-script
        // statement into the strict finite `SupportedInstanceScriptItem` set, or fail
        // closed on the first out-of-allowlist item (a function / class / enum /
        // namespace / interface / type / plain `let`-`const`-`var` / arbitrary
        // statement / `$:` label / `$`-`$$`-prefixed binding). A bare `let el;` is
        // admitted ONLY when its name is a supported `bind:this` target (collected from
        // the accepted bind shapes). This is the SOLE source of the lowering input —
        // `build_script_items` consumes ONLY these typed items, so the broad
        // statement-rewrite path is structurally unreachable.
        let script_items = if let Some(instance) = ir.analysis.scripts.instance_source {
            use super::bind_target_names::{
                collect_bind_function_pair_names, collect_bind_lvalue_roots,
                collect_bind_this_targets,
            };
            let bind_this_targets = collect_bind_this_targets(ir);
            let bind_lvalue_roots = collect_bind_lvalue_roots(ir);
            let bind_function_pair_names = collect_bind_function_pair_names(ir);
            instance_items::classify_supported_instance_items(
                instance,
                &bind_this_targets,
                &bind_lvalue_roots,
                &bind_function_pair_names,
            )?
        } else {
            Vec::new()
        };

        let facts = facts.into_inner();
        Ok(ClassifiedClientSurface {
            element_facts: facts.element_facts,
            event_shapes: facts.event_shapes,
            bind_shapes: facts.bind_shapes,
            group_values: facts.group_values,
            group_dynamic_value_nodes: facts.group_dynamic_value_nodes,
            interp_shapes: facts.interp_shapes,
            dynamic_attr_shapes: facts.dynamic_attr_shapes,
            html_nodes: facts.html_nodes,
            spread_elements: facts.spread_elements,
            props_usage,
            script_items,
            user_imports: script_facts.user_imports,
            uses_host: script_facts.uses_host,
            first_host_span: script_facts.first_host_span,
        })
    }
}

/// The per-node accepted shape facts accumulated during the template-node walk.
#[derive(Default)]
pub(super) struct SurfaceFacts {
    /// The accepted element fact per element node (the strict-allowlist `try_from`
    /// result — the SOLE source of the DOM var stem at emit time).
    element_facts: Vec<(NodeId, SupportedHtmlElement)>,
    /// The accepted event-handler shape per (target node, event type, handler expr) —
    /// keyed precisely so an element with multiple events keeps each event's fact
    /// distinct.
    pub(super) event_shapes: Vec<(NodeId, String, ExprId, ClientEventHandlerShape)>,
    /// The accepted bind shape per (target node, bind NAME) — keyed by name so an
    /// element with multiple binds keeps each bind's routing distinct.
    pub(super) bind_shapes: Vec<(NodeId, String, ClientBindShape)>,
    /// The `bind:group` value literal per group-input node.
    group_values: Vec<(NodeId, String)>,
    /// The `bind:group` input nodes carrying a DYNAMIC/mixed `value={…}`.
    group_dynamic_value_nodes: Vec<NodeId>,
    /// The accepted interpolation shape per interpolation node.
    interp_shapes: Vec<(NodeId, ClientInterpolationShape)>,
    /// The accepted dynamic-attr / class / style shape per (node, attribute index).
    dynamic_attr_shapes: Vec<(NodeId, usize, ClientDynamicAttrShape)>,
    /// The accepted `{@html}` node ids.
    html_nodes: Vec<NodeId>,
    /// The accepted spread-attribute element node ids.
    spread_elements: Vec<NodeId>,
}

/// The default-deny classifier handle (a zero-size type the entry method hangs
/// off — `ClientSyntaxSurface::classify`).
pub(super) struct ClientSyntaxSurface;

/// Refuse a ROOT REGION whose emission shape is NOT a supported root shape. The
/// supported shapes are the `from_html` clone-root, a standalone `<Component>` /
/// `{@render}` root, and a `$.comment()`-anchored raw-markup (`{@html}`) or
/// single-block (`{#if}` / `{#each}` / `{#await}` / `{#key}`) root.
///
/// The root region's clone frame is emitted as `var <region> = root();` — a call of
/// `root` as a FACTORY FUNCTION. `synthesize_region` (the SAME factory decision the
/// emitter consumes through `plan_static_templates`) classifies the root into one of
/// four `TemplateFactory` shapes:
///
/// - [`TemplateFactory::FromHtml`] — `root` is the `$.from_html(...)` clone factory;
///   `root()` is a valid call. SUPPORTED.
/// - [`TemplateFactory::TextNode`] — `root` is bound to a `$.text(...)` NODE (the
///   official text-first topology: `$.next(); var text = $.text(...); $.append(...)`).
///   Calling `root()` on a node is `TypeError: root is not a function`. REFUSE (5q).
///   This covers BOTH a pure-static-text root (`hello world`) and a reactive
///   text-node root (`{count}`); the latter additionally has no element host for its
///   `$.set_text`.
/// - [`TemplateFactory::CommentAnchor`] — `root` is bound to a `$.comment()` NODE.
///   A RAW-MARKUP (`{@html}`, `AnchorReason::RawHtmlRoot`) or SINGLE-BLOCK
///   (`{#if}` / `{#each}` / `{#await}` / `{#key}`, `AnchorReason::BlockOnlyRoot`)
///   comment-anchor root is SUPPORTED — the client backend emits the raw-markup /
///   block helper against the `$.comment()` anchor. Only an EMPTY / comment-only
///   (`AnchorReason::EmptyRoot`) comment-anchor root has no `root()` clone frame and
///   REFUSES (5q) as a `RootTextRegion`.
/// - [`TemplateFactory::Standalone`] — a `<Component>` / `{@render}` root, already
///   refused by the node walk (5f); never reaches here on the accept path. Treated
///   as supported here so this check owns ONLY the node-vs-factory clone-frame
///   mismatch.
///
/// Returns `Some` to fail closed for a `TextNode` root or an EMPTY / comment-only
/// (`AnchorReason::EmptyRoot`) `CommentAnchor` root, or `None` for a `from_html` /
/// standalone / raw-markup-anchor / block-anchor root (a supported clone-frame shape).
// The official text-first root topology (a `$.text()` root reached via `$.next()`, then —
// for a reactive text root — a `$.template_effect` over it) and the empty-template
// comment-anchor shape are not yet emitted as a clone frame, so they fail closed here
// instead of materializing.
fn refuse_unsupported_root_region(ir: &SvelteRuntimeIr) -> Option<UnsupportedSvelteRuntimeSurface> {
    let scope = ir.root_scope();
    // A NO-BODY special (`<svelte:window|document|body>` / `<svelte:head>`) at root is
    // TRANSPARENT to root classification: it renders at the function-init level and clones NO
    // template, so the root's clone-frame shape is decided by the REMAINING (cleaned) content.
    // Only bypass the empty/text-root refusal when the cleaned root is EMPTY (a host-special-ONLY
    // root ⇒ the no-DOM init-only path); a MIXED root falls through so its sibling classifies
    // normally (an element sibling ⇒ `FromHtml` accept; a bare reactive/empty text sibling ⇒ the
    // `RootTextRegion` deferral, EXACTLY as a text root without a host special) — never silently
    // accepted-and-mis-emitted through the no-DOM path.
    if root_region_has_no_body_special_host(ir)
        && clean_nodes(ir, &scope.roots, CleanContext::region_root()).is_empty()
    {
        return None;
    }
    match synthesize_region(ir, scope) {
        // The supported clone-frame shapes — `root()` is a valid factory call (or
        // the standalone root, already refused upstream).
        TemplateFactory::FromHtml { .. } | TemplateFactory::Standalone { .. } => None,
        // A lone `{@html}` root is a SUPPORTED `$.comment()`-anchored raw-markup root
        // (`var fragment = $.comment(); var node = $.first_child(fragment); $.html(node,
        // () => h);`) — the client backend emits the raw-markup root frame for it.
        TemplateFactory::CommentAnchor {
            reason: super::html::AnchorReason::RawHtmlRoot,
        } => None,
        // A SINGLE control-flow block at the root (`{#if}`/`{#each}`/`{#await}`/`{#key}`)
        // serializes to a lone `<!>` comment anchor — the official `$.comment()` block root
        // frame (`var fragment = $.comment(); var node = $.first_child(fragment); $.if(node,
        // …);`). The client backend emits the block helper against this anchor, so it
        // is a SUPPORTED root shape (a MULTI-block root is a `from_html` of comment markers,
        // already a `FromHtml` above).
        TemplateFactory::CommentAnchor {
            reason: super::html::AnchorReason::BlockOnlyRoot,
        } => None,
        // A `$.text(...)` / `$.comment()` node root (an empty / reactive-text root) —
        // refuse, carrying the span of the first interpolation when the region is a
        // reactive text run (for a precise diagnostic), else the root region's first node
        // span.
        TemplateFactory::TextNode { .. } | TemplateFactory::CommentAnchor { .. } => {
            Some(UnsupportedSvelteRuntimeSurface::RootTextRegion {
                span: root_region_span(ir, scope),
            })
        }
    }
}

/// Whether the ROOT region directly hosts a NO-BODY special (`<svelte:window|document|body>` —
/// a global event/bind host — or `<svelte:head>` — the `$.head(...)` region), each of which
/// emits at the function-init level and clones no root template. A root that is otherwise empty
/// after dropping such a special takes the emitter's no-DOM region path rather than being
/// refused as an empty/text root. Structural over the typed root nodes, never a source scan.
fn root_region_has_no_body_special_host(ir: &SvelteRuntimeIr) -> bool {
    ir.root_scope().roots.iter().any(|&n| {
        matches!(
            ir.node(n),
            IrNode::Special(s) if matches!(
                s.kind,
                SpecialKind::Window | SpecialKind::Document | SpecialKind::Body | SpecialKind::Head
            )
        )
    })
}

/// The diagnostic span for a refused root region: the first interpolation's span
/// when the region cleans to a reactive text run (the precise reactive surface),
/// else the first root node's span, else an empty span (an empty template).
fn root_region_span(ir: &SvelteRuntimeIr, scope: &super::ir::TemplateScope) -> Span {
    let items = clean_nodes(ir, &scope.roots, CleanContext::region_root());
    if let [CleanItem::TextRun { interps, .. }] = items.as_slice() {
        if let Some(&first) = interps.first() {
            if let IrNode::Interpolation { span, .. } = ir.node(first) {
                return *span;
            }
        }
    }
    scope
        .roots
        .first()
        .map(|&n| match ir.node(n) {
            IrNode::Text { span, .. }
            | IrNode::Comment { span, .. }
            | IrNode::Interpolation { span, .. } => *span,
            IrNode::Element(el) => el.span,
            _ => Span::new(0, 0),
        })
        .unwrap_or_else(|| Span::new(0, 0))
}

/// Classify one template node + its descendants, ACCUMULATING the per-node accepted
/// event/bind/interp shape facts. `namespace` is the DOM namespace the node renders
/// in (HTML at the region root, propagated into an `<svg>` / `<math>` subtree).
/// `declared_root_names` is the loop-invariant set of declared module + instance
/// top-level locals (computed once per compile, threaded down to the per-attr bind
/// classifier). Every node maps to a supported [`ClientNodeKind`] or REFUSES — there is
/// NO wildcard accept arm.
/// Where a node sits relative to its region — the placement axis the non-rendering
/// DECLARATION tags (`{@const}` / `{const}` / `{let}`) validate against. A `{@debug}` is
/// placement-INDEPENDENT (it emits a reactive effect at any document position).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodePlacement {
    /// A direct child of a BLOCK BODY region (`{#if}` / `{:else}` / `{#each}` / `{:then}` /
    /// `{:catch}` / … body root). The only in-scope valid parent for a `{@const}`, and a
    /// valid parent for `{const}` / `{let}`.
    BlockBodyRoot,
    /// A direct child of the COMPONENT ROOT region. A valid parent for `{const}` / `{let}`;
    /// the official compiler REJECTS a `{@const}` here (the component root is not a
    /// `{@const}` valid parent).
    ComponentRoot,
    /// NESTED inside an element. The official REJECTS a `{@const}` here, and Verter matches
    /// that rejection. A nested `{const}` / `{let}` is DIFFERENT — the official ACCEPTS it by
    /// wrapping the element child-walk in a real JavaScript `BlockStatement` (element-local
    /// lexical scope + a `$.template_effect` split local to that scope). Verter does not emit
    /// that element-local lowering, so it fails CLOSED here — an honest typed refusal, never a
    /// silent drop or a mis-hoist to the region top. That nested emission is the nested
    /// element-scope codegen axis — an element-local `BlockStatement` scope plus a per-block
    /// `$.template_effect` split — not ordinary block-body lowering.
    Nested,
}

/// The three lowering-recorded `slot=` placement-fact sets the unified slot choke-point
/// keys on, borrowed from the IR for the classification walk (see
/// [`SvelteRuntimeIr::static_slot_filler_hosts`] /
/// [`SvelteRuntimeIr::direct_slot_attr_child_hosts`] /
/// [`SvelteRuntimeIr::direct_snippet_slot_attr_child_hosts`] for the membership
/// contracts).
#[derive(Clone, Copy)]
pub(super) struct SlotPlacementFacts<'a> {
    /// The DIRECT static-slot-declaring component children (any node kind).
    pub(super) static_slot_filler_hosts: &'a rustc_hash::FxHashSet<NodeId>,
    /// Every source-level DIRECT child of a component-family node.
    pub(super) direct_slot_attr_child_hosts: &'a rustc_hash::FxHashSet<NodeId>,
    /// Every lowered SOURCE-LEVEL direct child of a `{#snippet}` block body.
    pub(super) direct_snippet_slot_attr_child_hosts: &'a rustc_hash::FxHashSet<NodeId>,
}

/// The UNIFIED `slot`-attribute choke-point — the SOLE `slot=` validation authority,
/// applied to EVERY template node at [`classify_node`] entry, BEFORE any per-kind
/// accept / fold / prop projection. Covering every node kind by construction means no
/// attr-bearing host — regular element, component, or `<svelte:*>` special — can
/// quietly route a `slot` attribute past the official disposition.
///
/// The official `svelte@5.56.3` disposition (`validate_slot_attribute`, driven here
/// from the typed IR node kinds plus the three lowering-recorded placement-fact sets —
/// never name/text sniffing):
///
/// - **Filler (Class A)** — a STATIC `slot="x"` on a DIRECT slot-declaring component
///   child (the node id is in [`SvelteRuntimeIr::static_slot_filler_hosts`]) is
///   accepted on a FILLER host kind: a regular element, a component, a
///   `<svelte:component>` / `<svelte:self>`, or a `<svelte:element>`. The filler
///   routes into the parent's `$$slots.NAME` region; a component-family filler ALSO
///   keeps `slot` as an ordinary prop on its own call, and a `<svelte:element>`
///   filler folds it into `$.attribute_effect` — both are official output shapes.
///   Lowered slot-region-root membership is NOT the placement fact: a transparent
///   `<svelte:fragment slot>`'s hoisted children are region roots but never fillers.
/// - **Plain prop (Class B)** — a `slot` (static OR dynamic/mixed) on a
///   COMPONENT-FAMILY host (a component / `<svelte:component>` / `<svelte:self>`)
///   with NO direct-placement owner at all — neither a component parent
///   ([`SvelteRuntimeIr::direct_slot_attr_child_hosts`]) nor a `{#snippet}` body
///   ([`SvelteRuntimeIr::direct_snippet_slot_attr_child_hosts`]) — is an ordinary
///   prop: official validates a component host with `is_component = true` and accepts
///   it at every owner-less placement, top level included.
/// - **Snippet static** — a SINGLE static TEXT-VALUED `slot="x"` on a DIRECT
///   `{#snippet}`-body child
///   (the node id is in [`SvelteRuntimeIr::direct_snippet_slot_attr_child_hosts`])
///   is accepted on a filler-capable host kind: official validates a snippet child as
///   component-owned placement, so the `slot` stays an ordinary attr/prop on the host
///   itself — a snippet child is NOT a filler, never routes into `$$slots`, and never
///   enters the duplicate/default-slot checks. An element bakes it into the skeleton,
///   a component-family host keeps the plain prop, a `<svelte:element>` folds it into
///   `$.attribute_effect`. The text value is part of the acceptance (official
///   `is_text_attribute`): a VALUELESS/boolean `slot` on a direct snippet child
///   REJECTS (Class C).
/// - **Reject (Class C)** — everything else fails closed with the typed slot refusal:
///   a dynamic/mixed `slot` on a DIRECT component child, on a DIRECT snippet child, or
///   on any element-family host (official `slot_attribute_invalid` — "must be a
///   static value"), a VALUELESS/boolean `slot` on a DIRECT snippet child (the same
///   official `slot_attribute_invalid` — not a text-valued attribute), a static
///   `slot` on an element outside direct-filler /
///   direct-snippet placement (official `slot_attribute_invalid_placement`), and a
///   `slot` on a non-filler special (`<svelte:head>` / `<svelte:boundary>` /
///   `<svelte:fragment>` / the global hosts / `<svelte:options>` — each an official
///   per-host attribute reject, kind-gated even at snippet placement).
///
/// A node kind with no attribute surface (text / comment / interpolation / block /
/// tag) validates trivially.
pub(super) fn validate_slot_placement(
    node: &IrNode,
    node_id: NodeId,
    slot_placement: SlotPlacementFacts<'_>,
) -> Result<(), UnsupportedSvelteRuntimeSurface> {
    let SlotPlacementFacts {
        static_slot_filler_hosts,
        direct_slot_attr_child_hosts,
        direct_snippet_slot_attr_child_hosts,
    } = slot_placement;
    let (attrs, span) = match node {
        IrNode::Element(el) => (&el.attrs, el.span),
        IrNode::Component(c) => (&c.attrs, c.span),
        IrNode::Special(s) => (&s.attrs, s.span),
        // No attribute surface — nothing can carry a `slot=`.
        IrNode::Text { .. }
        | IrNode::Comment { .. }
        | IrNode::Interpolation { .. }
        | IrNode::Block(_)
        | IrNode::Tag(_) => return Ok(()),
    };
    // A PLAIN-PROP host receives a non-direct `slot` as an ordinary prop (official
    // validates these with `is_component = true`).
    let plain_component_slot_prop_host = matches!(node, IrNode::Component(_))
        || matches!(node, IrNode::Special(s) if matches!(s.kind, SpecialKind::Component | SpecialKind::SelfRef));
    // A FILLER host can be routed into the parent's `$$slots` as a DIRECT static-slot
    // child (the plain-prop hosts plus the element family).
    let slot_filler_host = plain_component_slot_prop_host
        || matches!(node, IrNode::Element(_))
        || matches!(node, IrNode::Special(s) if s.kind == SpecialKind::Element);
    let direct_component_child = direct_slot_attr_child_hosts.contains(&node_id);
    let direct_snippet_child = direct_snippet_slot_attr_child_hosts.contains(&node_id);
    // The PLAIN-PROP acceptance requires a host with NO direct-placement owner at all
    // — neither a component parent nor a `{#snippet}` body (a direct snippet child
    // carrying a dynamic `slot` must REJECT, never leak through the plain-prop path).
    let plain_prop =
        plain_component_slot_prop_host && !direct_component_child && !direct_snippet_child;
    for attr in attrs {
        if let AttrIr::Dynamic { name, .. } | AttrIr::Mixed { name, .. } = attr {
            // A dynamic/mixed `slot` is accepted ONLY as an owner-less plain prop; on
            // a direct component child, a direct snippet child, or any element-family
            // host it is the official `slot_attribute_invalid` compile error.
            if name == "slot" && !plain_prop {
                return Err(UnsupportedSvelteRuntimeSurface::DynamicAttribute {
                    name: name.clone(),
                    span,
                });
            }
        }
        if let AttrIr::Static { name, value } = attr {
            if name == "slot" {
                let component_filler =
                    slot_filler_host && static_slot_filler_hosts.contains(&node_id);
                // A static TEXT-VALUED `slot` on a DIRECT snippet child is
                // component-owned placement on a filler-capable host kind — accepted
                // as a plain attr/prop on the host itself, NEVER routed into
                // `$$slots`. The text value is REQUIRED (official `is_text_attribute`):
                // a valueless/boolean `slot` (`<span slot>` / `<Inner slot/>`) is the
                // official `slot_attribute_invalid` reject — snippet membership
                // already disables the plain-prop path, so it falls through to the
                // typed refusal below. The value gate is SNIPPET-ONLY: the owner-less
                // Class B plain prop (top-level `<Inner slot/>` → `{slot: true}`) is
                // a genuine official accept and stays untouched, and the Class A
                // filler set is value-gated at lowering (`static_slot_name` only
                // records text-valued slots).
                let snippet_static = direct_snippet_child && slot_filler_host && value.is_some();
                if !(component_filler || snippet_static || plain_prop) {
                    return Err(UnsupportedSvelteRuntimeSurface::DynamicAttribute {
                        name: name.clone(),
                        span,
                    });
                }
            }
        }
    }
    Ok(())
}

fn classify_node(
    ir: &SvelteRuntimeIr,
    node_id: NodeId,
    namespace: Namespace,
    declared_root_names: &rustc_hash::FxHashSet<String>,
    slot_placement: SlotPlacementFacts<'_>,
    facts: &RefCell<SurfaceFacts>,
    placement: NodePlacement,
) -> Result<(), UnsupportedSvelteRuntimeSurface> {
    // The unified `slot`-attribute choke-point runs for EVERY node kind BEFORE the
    // per-kind arms — no host arm can accept, fold, or prop-project a `slot=` the
    // gate refuses.
    validate_slot_placement(ir.node(node_id), node_id, slot_placement)?;
    match ir.node(node_id) {
        // A text node's literal chunk must be SIMPLE ASCII (no HTML entity, no
        // tab/newline/repeated-space, no escaping need). A complex chunk needs the
        // official boundary-trimming / entity-decode / escaping path (5u). Comments
        // serialize verbatim.
        IrNode::Comment { .. } => Ok(()),
        IrNode::Text { text, span } => {
            if client_shapes::text_chunk_is_simple_ascii(text) {
                Ok(())
            } else {
                Err(UnsupportedSvelteRuntimeSurface::ComplexTextChunk { span: *span })
            }
        }
        IrNode::Interpolation { escape, span, expr } => {
            if *escape == EscapeMode::Raw {
                // A raw-markup interpolation (`{@html}` in interpolation form) is the
                // `$.html` surface — accept it as a raw-html node (the same emission the
                // `TagIr::Html` node takes). The template lowering produces every `{@html}`
                // as a `TagIr::Html` node, so this arm is a defensive mirror; it accepts
                // rather than refuses so the raw-markup surface is never split.
                let _ = expr;
                facts.borrow_mut().html_nodes.push(node_id);
                return Ok(());
            }
            // The interpolation expression must be a BARE signal / no-default-prop
            // read (the §1.2-class reactive-text surface). A complex expression
            // (binary / call / member / conditional / …) fails closed — its breadth is
            // owned by the reactive-text/interpolation completion surface. The accepted
            // shape is recorded as a fact for the plan.
            let analyzed = ir.analysis.expressions.get(*expr);
            let shape = client_shapes::classify_interpolation_shape(
                analyzed.source,
                analyzed.scope,
                &ir.analysis.bindings,
                &ir.analysis.scopes,
                *span,
            )?;
            facts.borrow_mut().interp_shapes.push((node_id, shape));
            Ok(())
        }
        IrNode::Element(el) => {
            // The STRICT FINITE element allowlist gate. The element is accepted ONLY by
            // `SupportedHtmlElement::try_from`; every other tag fails closed BY
            // CONSTRUCTION (there is no approximating blocklist).
            //
            // (1) Reject a NON-HTML namespace. An SVG / MathML subtree inherits its
            // namespace; otherwise an `<svg>` / `<math>` introduces one. An element in
            // a non-HTML namespace needs the `$.from_svg` / `$.from_mathml` factory
            // (a distinct root-helper layer Verter does not emit), so it fails closed
            // (5a).
            let element_namespace = element_own_namespace(namespace, &el.tag);
            if element_namespace != Namespace::Html {
                return Err(UnsupportedSvelteRuntimeSurface::DynamicAttribute {
                    name: format!(
                        "<{}> ({} namespace)",
                        el.tag,
                        namespace_label(element_namespace)
                    ),
                    span: el.span,
                });
            }
            // (2) Reject a HYPHENATED custom element (`<my-widget>`) OR any element
            // carrying an `is` attribute (a customized built-in `<button is="x">`):
            // both are the web-components surface (official clones via `importNode` +
            // `$.set_custom_element_data`). This fires BEFORE the attr walk so a
            // no-attribute custom element does not leak. The custom-element output
            // (`importNode` clone + `$.set_custom_element_data`, with a sanitized DOM
            // local name) is not yet emitted, so a custom element fails closed here.
            if el.tag.contains('-') || element_carries_is_attribute(el) {
                return Err(UnsupportedSvelteRuntimeSurface::HostOrCustomElement {
                    surface: "custom element",
                    span: el.span,
                });
            }
            // (3) Reject a raw `<slot>` explicitly. Verter's parser does not model the
            // official `SlotElement` (it parses `<slot>` as a regular intrinsic), so a
            // `<slot>` must NEVER reach intrinsic emission — it would clone a bare
            // `<slot>` element instead of the official slot-fallback topology. Routed
            // to the regular-element refusal (5a).
            // TODO(follow-up): model the official `SlotElement` + its slot-fallback
            // lowering instead of failing closed. Owned by the slot / component block.
            if el.tag == "slot" {
                return Err(UnsupportedSvelteRuntimeSurface::Element {
                    tag: el.tag.clone(),
                    span: el.span,
                });
            }
            // (4) Reject a SVELTE-RESERVED-WORD tag (`<var>` / `<class>` / `<for>` /
            // `<interface>` / …) — its synthesized DOM local var name (`var var = …`)
            // is an invalid/reserved JS binding the official compiler collision-renames
            // (`var_1`). The membership uses Svelte's STRICT `RESERVED_WORDS` (NOT
            // OXC's narrower `is_keyword`, which omits `arguments` / `eval` /
            // `interface` / `package` / `implements` / …), so the diagnostic split is
            // precise. Kept as the distinct naming-completion refusal (5v).
            // TODO(follow-up): collision-rename a reserved-word element's DOM local var
            // (`var` → `var_1`) through the shared name allocator instead of failing
            // closed. Owned by the naming-completion block (5v).
            if is_svelte_reserved_word(&el.tag) {
                return Err(UnsupportedSvelteRuntimeSurface::ElementName {
                    tag: el.tag.clone(),
                    span: el.span,
                });
            }
            // (5) Accept ONLY a tag in the finite `SupportedHtmlElement` allowlist;
            // (6) everything else fails closed with the regular-element refusal (5a).
            // The accepted element is recorded as a TYPED fact so the emitter reads the
            // DOM var stem from `SupportedHtmlElement::var_stem`, never the raw tag.
            let Some(element) = SupportedHtmlElement::try_from(&el.tag) else {
                return Err(UnsupportedSvelteRuntimeSurface::Element {
                    tag: el.tag.clone(),
                    span: el.span,
                });
            };
            facts.borrow_mut().element_facts.push((node_id, element));
            // (5c) SPECIAL-CONTENT-MODEL gate for the bindings-breadth hosts
            // (`textarea` / `select` / `option`). These elements have a SPECIAL
            // official content model (raw-text for `<textarea>`, the `__value`/
            // option-tracking surface for `<select>`/`<option>`) that 5c does NOT
            // emit: 5c emits them ONLY as the `bind:value` host shapes the pinned
            // oracle proves (`<textarea bind:value></textarea>` cleared empty;
            // `<select bind:value><option>static</option></select>`). Any other
            // interior content — a `<textarea>` with text/interpolation children, an
            // `<option>` with an INTERPOLATION child (the `option.__value` tracking
            // surface), a `<select>` child that is not a static `<option>` — is the
            // special-content surface 5c does not own; it fails closed HERE (before
            // the per-attr / child walk) so a divergent module is never emitted.
            refuse_unsupported_special_content(ir, element, el, el.span)?;
            // A SPREAD on the element switches its WHOLE attribute strategy to the single
            // `$.attribute_effect` fold. The fold models only the directly-foldable attr
            // set; an event / `bind:` / `use:` / `transition:` / `let:` directive on a
            // spread element fails closed HERE (before the per-attr loop), regardless of
            // attribute order — so a delegated event on a spread element does not silently
            // pass its own `classify_attr` arm and then diverge at the fold.
            if element_has_spread(el) {
                if let Some(surface) = refuse_spread_incompatible_attr(el, el.span) {
                    return Err(surface);
                }
            }
            // OVERLAPPING transition directives (`in:`/`out:`/`transition:` halves) are
            // the official `transition_duplicate` / `transition_conflict` compile
            // errors — refused here (before the per-attr loop, which accepts each
            // transition individually).
            refuse_transition_conflicts(el, el.span)?;
            // The static attributes are classified against the strict per-element attr
            // allowlist (the typed `SupportedHtmlElement` is the per-tag key). A custom
            // element already failed closed at step (2), so the attr walk sees only an
            // allowlisted element.
            for (attr_idx, attr) in el.attrs.iter().enumerate() {
                classify_attr(
                    ir,
                    node_id,
                    attr_idx,
                    element,
                    attr,
                    el.span,
                    declared_root_names,
                    facts,
                )?;
            }
            let child_namespace = determine_namespace_for_children(element_namespace, &el.tag);
            for &child in &el.children {
                // An element's children are NESTED — a declaration tag here is not a region
                // root, so `{@const}` / `{const}` / `{let}` fail closed (placement gate).
                classify_node(
                    ir,
                    child,
                    child_namespace,
                    declared_root_names,
                    slot_placement,
                    facts,
                    NodePlacement::Nested,
                )?;
            }
            Ok(())
        }
        // A component invocation (`<Foo …/>`) is ACCEPTED. Its props / events / binds /
        // `let:` are validated + rewritten at projection (fallible — an unsupported form
        // fails closed there); its slot-content regions (default / named) are CLASSIFIED
        // INDEPENDENTLY by the outer scope loop (they are their own template scopes).
        // Children are NOT recursed here — the slot regions own that. Component `let:` is the
        // component-context slot-prop path (NOT the element-context `let:` refusal, which
        // stays closed). A `slot=` on the component itself was already dispositioned by
        // the unified choke-point above: a DIRECT-filler or non-direct placement passed
        // through (official ACCEPTS both — the filler routes into the parent's `$$slots`
        // AND keeps the prop; the non-direct form is an ordinary plain prop the
        // projection emits), and a dynamic/mixed `slot` on a direct child was refused.
        IrNode::Component(_) => Ok(()),
        // `<svelte:options>` is a compile-option carrier. The component-INVOCATION specials
        // (`<svelte:component>` / `<svelte:self>`) are ACCEPTED, projected to a component
        // call. A `<svelte:fragment>` reaching here is a STANDALONE transparent wrapper (the
        // supported fragment surface is the `slot=`-bearing NAMED slot, which is ABSORBED
        // into its parent component's `$$slots` at lowering and never becomes a node) — the
        // standalone transparent-fragment surface stays CLOSED. Every OTHER `<svelte:*>`
        // special — the host / renderable specials (`Head` / `Window` / `Document` / `Body` /
        // `Boundary`) — refuses a `slot=` at the unified choke-point above (each is an
        // official per-host attribute reject); the component-family specials and
        // `<svelte:element>` take the same three-class disposition as a component.
        IrNode::Special(s) if s.kind == SpecialKind::Options => Ok(()),
        IrNode::Special(s) if matches!(s.kind, SpecialKind::Component | SpecialKind::SelfRef) => {
            Ok(())
        }
        // The GLOBAL-host specials (`<svelte:window|document|body>`) are ACCEPTED with
        // per-host attribute validation: events classify against the regular event surface
        // (recorded host-keyed), binds resolve through the HOST-SCOPED bind contract (a
        // wrong-host / unknown bind name fails closed). No DOM is rendered.
        IrNode::Special(s)
            if matches!(
                s.kind,
                SpecialKind::Window | SpecialKind::Document | SpecialKind::Body
            ) =>
        {
            classify_special_host(ir, node_id, s, declared_root_names, facts)
        }
        // A `<svelte:element this={…}>` dynamic element is ACCEPTED with per-attribute
        // validation: its binds resolve through the HOST-SCOPED bind contract (the
        // `svelte:element` generic-element host — `bind:value` / `bind:devicePixelRatio`
        // fail closed, the §1.8 negatives), its events validate against the inline-handler
        // surface, and its attributes fold into the runtime `$.attribute_effect`. Its
        // children are its OWN body region (classified independently by the scope loop).
        IrNode::Special(s) if s.kind == SpecialKind::Element => {
            classify_svelte_element(ir, node_id, s, declared_root_names, facts)
        }
        // A `<svelte:boundary>` is ACCEPTED: its `onerror` handler validates against the
        // inline-handler surface (an ASYNC handler fails closed via `ExperimentalAsync`), its
        // body + `failed`/`pending` snippets are classified independently by the scope loop.
        // An async-runtime construct inside the boundary (`await` / `$effect.pending` / async
        // `$derived` / async handler) fails closed via the shared `ExperimentalAsync` paths.
        IrNode::Special(s) if s.kind == SpecialKind::Boundary => classify_svelte_boundary(ir, s),
        // A `<svelte:head>` is ACCEPTED: it emits `$.head(...)`, its `<title>` drives
        // `$.document.title`, and its non-title children are its OWN body region (scope-loop
        // classified). Official rejects head attributes (`svelte_head_illegal_attribute`) — parity.
        IrNode::Special(s) if s.kind == SpecialKind::Head => classify_svelte_head(s),
        IrNode::Special(s) => Err(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
            construct: special_label(s.kind),
            span: s.span,
        }),
        // The control-flow blocks (`{#if}`/`{#each}`/`{#await}`/`{#key}`) are ACCEPTED.
        // Each block body is its OWN template scope, which the outer scope loop
        // (`for scope in &ir.template_scopes`) classifies independently — so an out-of-scope
        // child INSIDE a block body (a `<Component>`, a `{#snippet}`, a renderable
        // `<svelte:*>`) is still refused by its own node arm. The block-head expressions
        // (the `{#if}` test, the `{#each}` source/key, the `{#await}` promise, the `{#key}`
        // expression) are rewritten at plan time (an `await` expression / async rune inside
        // them fails closed at the rewrite). A `{#snippet}` is a callable DEFINITION — the
        // snippet surface, ACCEPTED; its body region is classified by the scope loop.
        IrNode::Block(_) => Ok(()),
        // A `{@html expr}` raw-markup tag is the `$.html` surface — accept it, recording
        // the per-node acceptance proof. Its payload expression + the only-child topology
        // are read from the IR at plan time.
        IrNode::Tag(TagIr::Html { .. }) => {
            facts.borrow_mut().html_nodes.push(node_id);
            Ok(())
        }
        // `{@debug}` is placement-INDEPENDENT: its reactive snapshot effect emits at ANY
        // document position (region root or nested in an element, interleaved into the
        // walk). Always accepted.
        IrNode::Tag(TagIr::Debug { .. }) => Ok(()),
        // `{@const}` is valid ONLY as a direct child of a BLOCK BODY (the official
        // valid-parents set, restricted to Verter's supported surface). The official rejects
        // it at the component root and nested in an element — both fail closed here, never a
        // silent drop / mis-hoist.
        IrNode::Tag(tag @ TagIr::LegacyConst { .. }) => {
            if placement == NodePlacement::BlockBodyRoot {
                Ok(())
            } else {
                Err(refuse_tag(tag))
            }
        }
        // `{const …}` / `{let …}` are valid at any region ROOT (hoisted block-local
        // declarations) and are accepted here. A NESTED placement (inside an element) fails
        // CLOSED — an honest typed refusal, never the silent drop the roots-only hoist
        // produced. The official compiler ACCEPTS a nested DeclarationTag by wrapping the
        // element's child-walk in a real JavaScript `BlockStatement`, emitting the
        // declaration (and its `$.template_effect` split) inside that scope. That
        // element-local lowering is the nested element-scope codegen axis — an element-local
        // `BlockStatement` scope plus a per-block `$.template_effect` split; the nested
        // placement is refused, not mis-emitted.
        IrNode::Tag(tag @ TagIr::Declaration { .. }) => {
            if placement == NodePlacement::Nested {
                Err(refuse_tag(tag))
            } else {
                Ok(())
            }
        }
        // A `{@render}` tag is the snippet-render surface — ACCEPTED (its callee + args are
        // validated + rewritten at projection), EXCEPT a render call carrying a SPREAD
        // argument (`{@render row(...xs)}`), which official `svelte@5.56.3` HARD-ERRORS
        // (`render_tag_invalid_spread_argument`). It fails closed here rather than silently
        // dropping the spread and emitting a wrong-arity `$.snippet` call. A
        // CHILD-position `{@attach}` stays CLOSED via `refuse_tag` (official
        // `expected_tag` parity — attribute-position-only).
        IrNode::Tag(TagIr::Render {
            spread_arg_span, ..
        }) => match spread_arg_span {
            Some(span) => Err(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                construct: "{@render} spread argument",
                span: *span,
            }),
            None => Ok(()),
        },
        // `{@html}` is accepted above; a CHILD-position `{@attach}` (officially
        // attribute-position-only — the element form is the supported `AttrIr::Attach`)
        // stays refused via `refuse_tag`.
        IrNode::Tag(tag) => Err(refuse_tag(tag)),
    }
}

/// The accepted dynamic-attr shape for a STATIC non-static-property attribute
/// (`autofocus` / `muted`), or `None` for any other static attribute (which the
/// caller routes to the strict static-attr allowlist).
///
/// `autofocus` (valueless or valued) is the init-only `$.autofocus` surface on ANY
/// element. A valueless / valued `muted` is a DOM-PROPERTY write on ANY element —
/// official `is_dom_property('muted')` is element-agnostic (`muted` is in
/// `DOM_BOOLEAN_ATTRIBUTES` → `DOM_PROPERTIES` with no host check), so `<div muted>`
/// emits `div.muted = true` exactly like `<video muted>`. `defaultValue` /
/// `defaultChecked` are the form-default family (5c) and are NOT accepted here (they
/// fall through to the static-attr allowlist, which refuses them).
fn static_non_static_property_shape(name: &str) -> Option<ClientDynamicAttrShape> {
    match name {
        "autofocus" => Some(ClientDynamicAttrShape::Autofocus),
        "muted" => Some(ClientDynamicAttrShape::DomProperty {
            prop: "muted".to_string(),
        }),
        _ => None,
    }
}

/// Refuse a spread element that carries an attribute the `$.attribute_effect` fold does
/// not model: an event handler / `bind:` / `let:` directive (the handler-hoist /
/// two-way-binding surface a spread fold leaves to its owning vertical). The foldable /
/// co-existing set — static / dynamic / mixed / `class:` / `style:` / a plain `class` /
/// `style` attribute / further spreads, PLUS the lifecycle directives (`use:` /
/// `transition:` / `animate:` / `{@attach}`, which official emits ALONGSIDE the fold:
/// `$.attribute_effect` → `$.action` → `$.transition` in source order) — returns `None`.
/// Driven from the typed `AttrIr` inventory, never a source scan.
fn refuse_spread_incompatible_attr(
    el: &super::ir::ElementIr,
    el_span: Span,
) -> Option<UnsupportedSvelteRuntimeSurface> {
    for attr in &el.attrs {
        match attr {
            AttrIr::Static { .. }
            | AttrIr::Dynamic { .. }
            | AttrIr::Mixed { .. }
            | AttrIr::Spread { .. }
            | AttrIr::Class { .. }
            | AttrIr::Style { .. }
            | AttrIr::Use { .. }
            | AttrIr::Transition { .. }
            | AttrIr::Animate { .. }
            | AttrIr::Attach { .. } => {}
            AttrIr::Event { event_type, .. } => {
                return Some(UnsupportedSvelteRuntimeSurface::NonDelegatedEvent {
                    event_type: event_type.clone(),
                    span: el_span,
                });
            }
            AttrIr::Bind { target, .. } => {
                return Some(UnsupportedSvelteRuntimeSurface::Binding {
                    target: target.clone(),
                    span: el_span,
                });
            }
            AttrIr::Let { .. } => {
                return Some(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
                    construct: "let-directive",
                    span: el_span,
                });
            }
        }
    }
    None
}

/// Refuse a legacy `on:` directive carrying an OFFICIAL-INVALID modifier set — an
/// unrecognized modifier (`event_handler_invalid_modifier`) or `passive` co-occurring
/// with `nonpassive` / `preventDefault` (`event_handler_invalid_modifier_combination`).
/// These are official COMPILE ERRORS; Verter keeps them fail-closed/refused (routed
/// through the event channel) so an invalid event surface never emits. (A modern
/// attribute carries no modifiers, so it validates trivially.)
pub(super) fn refuse_invalid_event_modifiers(
    modifiers: &[String],
    event_type: &str,
    el_span: Span,
) -> Result<(), UnsupportedSvelteRuntimeSurface> {
    match validate_event_modifiers(modifiers) {
        Ok(()) => Ok(()),
        Err(
            EventModifierError::Unknown(_) | EventModifierError::InvalidPassiveCombination { .. },
        ) => Err(UnsupportedSvelteRuntimeSurface::NonDelegatedEvent {
            event_type: event_type.to_string(),
            span: el_span,
        }),
    }
}

/// Classify one attribute / directive into the narrow supported vocabulary,
/// ACCUMULATING the accepted bind / event / dynamic-attribute SHAPE fact, or REFUSE.
/// The supported surface: a directly-serializable static attribute, a static
/// non-static-property (`autofocus` / media `muted`), a DYNAMIC attribute / `class={…}`
/// / `style={…}` / `class:` / `style:` directive , a delegated `onclick`
/// with a §1.2-class `$state`-write arrow handler, `bind:value` on an `<input>` to a
/// reactive `$state` ident, and intrinsic-element `bind:this` to a non-prop
/// identifier. There is NO wildcard accept arm — every `AttrIr` variant is matched
/// explicitly. `attr_idx` is the attribute's position in the element's `AttrIr` list,
/// recorded with the accepted dynamic-attr shape so the plan maps each op back to its
/// emission decision.
#[allow(clippy::too_many_arguments)]
fn classify_attr(
    ir: &SvelteRuntimeIr,
    node_id: NodeId,
    attr_idx: usize,
    element: SupportedHtmlElement,
    attr: &AttrIr,
    el_span: Span,
    declared_root_names: &rustc_hash::FxHashSet<String>,
    facts: &RefCell<SurfaceFacts>,
) -> Result<(), UnsupportedSvelteRuntimeSurface> {
    // The accepted element's tag string (for the bind classifier's `input` host
    // check). `var_stem()` is the exact lowercase tag for every allowlist element.
    let tag = element.var_stem();
    // NOTE: `slot` attribute validity (dynamic/mixed refusal + static placement) is
    // owned by the unified choke-point ([`validate_slot_placement`] at
    // [`classify_node`] entry) — an attr reaching this classifier carries either no
    // `slot` or a VALID owner-placed static `slot` (which the spread fold or the
    // static-attr arm accepts below).
    // A SPREAD on the element switches its WHOLE attribute strategy to the single
    // `$.attribute_effect` fold: every co-located FOLDABLE attribute (static / dynamic /
    // mixed / `class:` / `style:` / a plain `class` / `style`) moves into the runtime
    // object literal, so it is NOT classified against the per-element static-attr
    // allowlist (which would refuse a non-allowlisted name like `a` / `b`) nor recorded as
    // a per-attribute op shape (the plan reads the source-ordered fold from the IR). The
    // spread-INCOMPATIBLE directives (event / `bind:` / `use:` / `transition:` / `let:`)
    // were already refused at the element level before this loop, so reaching here for a
    // foldable attr is an accept. The spread itself is recorded by its own arm below.
    if matches!(ir.node(node_id), IrNode::Element(el) if element_has_spread(el))
        && matches!(
            attr,
            AttrIr::Static { .. }
                | AttrIr::Dynamic { .. }
                | AttrIr::Mixed { .. }
                | AttrIr::Class { .. }
                | AttrIr::Style { .. }
        )
    {
        return Ok(());
    }
    match attr {
        AttrIr::Static { name, value } => {
            // (a) A static `autofocus` / media `muted` is a NON-STATIC-PROPERTY
            // (`cannot_be_set_statically`) applied at runtime via `$.autofocus` / a
            // property write — NOT a baked skeleton attr. Accept it ,
            // recording the dynamic-attr shape so the plan emits the `NonStaticProperty`
            // op. `defaultValue` / `defaultChecked` are the form-default family (5c) and
            // are NOT accepted here — they fall through to the static-attr allowlist,
            // which refuses them.
            if let Some(shape) = static_non_static_property_shape(name) {
                facts
                    .borrow_mut()
                    .dynamic_attr_shapes
                    .push((node_id, attr_idx, shape));
                return Ok(());
            }
            // (a2) A static `class` / `style` whose element ALSO carries a `class:` /
            // `style:` directive is the BASE value of the merged `$.set_class` /
            // `$.set_style` (it is pulled OUT of the skeleton). Accept it as the
            // Class / Style surface — the plan reads the static base when coalescing.
            // (Without this, a static `style="x"` would fail the static-attr allowlist
            // even though the `style:` directive makes the whole element a 5a surface.)
            // The NAME matches case-insensitively — official normalizes HTML attribute
            // names (`get_attribute_name` → `normalize_attribute`) before routing.
            if (name.eq_ignore_ascii_case("class") && element_has_class_directive(ir, node_id))
                || (name.eq_ignore_ascii_case("style") && element_has_style_directive(ir, node_id))
            {
                let shape = if name.eq_ignore_ascii_case("class") {
                    ClientDynamicAttrShape::Class
                } else {
                    ClientDynamicAttrShape::Style
                };
                facts
                    .borrow_mut()
                    .dynamic_attr_shapes
                    .push((node_id, attr_idx, shape));
                return Ok(());
            }
            // (a3) A static `value="X"` on an `<input>` that ALSO carries a
            // `bind:group` is the GROUP VALUE source (the official `bind:group` form
            // emits `input.value = input.__value = 'X'` as a per-input runtime write,
            // NOT a baked static `value` attr). 5c accepts it as the group-value fact;
            // the serializer strips it from the skeleton and the emitter writes the
            // `__value`. (A static `value` on a NON-group input is still the
            // form-control deferral and fails closed at (b).)
            //
            // The RAW attribute span is ENTITY-DECODED here (the storage site) before it
            // becomes the group-value fact — official runs the static value through the
            // attribute-value entity decoder, so `value="a&amp;b"` writes `'a&b'`. The
            // emitter (`client_bind.rs`) is the QUOTING point; storing the decoded value
            // keeps decoding owned at one place and matches every other static attribute.
            if name == "value"
                && element == SupportedHtmlElement::Input
                && element_has_group_bind(ir, node_id)
            {
                let literal = value
                    .as_ref()
                    .map(|v| super::entity_decode::decode_attr_entities(&v.value))
                    .unwrap_or_default();
                facts.borrow_mut().group_values.push((node_id, literal));
                return Ok(());
            }
            // (a4) A static `defaultValue` / `defaultChecked` CO-LOCATED with its MATCHING
            // bind is the form-default write the official compiler emits as a property
            // write (`input.defaultValue = 'x'` / `input.defaultChecked = true`) BEFORE the
            // bind — and the default attr SUPPRESSES the `remove_input_defaults` prelude.
            // `defaultValue` pairs with `bind:value` (on an `<input>` OR `<textarea>`);
            // `defaultChecked` pairs with `bind:checked` (on an `<input>`). 5c accepts ONLY
            // the co-located form — the property-write op is already projected from the IR
            // (`NonStaticProperty`), so the accept just lets the attr through the gate. A
            // STANDALONE default (no matching bind) stays the form-default deferral and
            // fails closed at (b); a MISMATCHED default+bind (`defaultChecked` with
            // `bind:value`) is a CONSERVATIVE refusal — NARROWER than official, which
            // accepts the mixed form, but 5c keeps the strict co-location boundary.
            if super::bind_target_names::default_attr_has_matching_bind(name, element, ir, node_id)
            {
                return Ok(());
            }
            // (a5) A STATIC `slot="x"` at VALID component-child slot placement (the
            // unified choke-point — `validate_slot_placement` at `classify_node` entry
            // — already validated the node is a SOURCE-LEVEL static slot FILLER: a
            // direct slot-declaring regular-element component child) bakes into the
            // cloned skeleton verbatim — the official output keeps the slot attribute
            // in the template HTML (`<span slot="foo-bar"> </span>`).
            if name == "slot" {
                return Ok(());
            }
            // (b) The STRICT FINITE static-attr allowlist is the SOLE acceptance
            // authority for a baked static attr: `SupportedStaticAttr::classify`
            // accepts ONLY the enumerated `(name, element, value)` shapes. EVERY other
            // name (`is`, a standalone `defaultValue`/`defaultChecked`, `dir`, `style`,
            // input `value`/`checked`, …) fails closed BEFORE emission — so an accepted
            // attr can NEVER be the one the serializer would silently drop.
            let literal = value.as_ref().map(|v| v.value.as_str());
            if SupportedStaticAttr::classify(name, element, literal).is_none() {
                return Err(UnsupportedSvelteRuntimeSurface::DynamicAttribute {
                    name: name.clone(),
                    span: el_span,
                });
            }
            Ok(())
        }
        AttrIr::Dynamic { name, .. } | AttrIr::Mixed { name, .. } => {
            // A DYNAMIC/mixed `value={…}` on an `<input>` that ALSO carries a `bind:group` is
            // the group-value source (official emits the per-input `input.value = input.__value
            // = …` write — change-tracked via `$.template_effect` when reactive — NOT a generic
            // dynamic form-control attr). Record the node; the plan reads the `value` attr from
            // the IR and builds the structured `GroupDynamicValue` (it owns the rewriter +
            // reactivity analysis). The static-literal case is handled by the `AttrIr::Static`
            // arm (`group_values`).
            if name == "value"
                && element == SupportedHtmlElement::Input
                && element_has_group_bind(ir, node_id)
            {
                facts.borrow_mut().group_dynamic_value_nodes.push(node_id);
                return Ok(());
            }
            // A dynamic / mixed `class` / `style` PLAIN attribute (`class={c}` /
            // `class="a {b}"` / `style={s}`) is the class / style surface (`$.set_class`
            // / `$.set_style`), NOT a generic attribute — route it to the Class / Style
            // shape so the plan coalesces it. (The directive forms reach the
            // `AttrIr::Class` / `AttrIr::Style` arms below.)
            if name == "class" {
                facts.borrow_mut().dynamic_attr_shapes.push((
                    node_id,
                    attr_idx,
                    ClientDynamicAttrShape::Class,
                ));
                return Ok(());
            }
            if name == "style" {
                facts.borrow_mut().dynamic_attr_shapes.push((
                    node_id,
                    attr_idx,
                    ClientDynamicAttrShape::Style,
                ));
                return Ok(());
            }
            // A DYNAMIC attribute (`id={x}` / `id="a{x}"`) is the dynamic-attribute / class / style surface:
            // classify its emission shape (a DOM-property write, `$.set_attribute`,
            // `$.autofocus`), refusing the form-control setters (5c) and the `dir`
            // reflected-attr quirk. `muted` is a DOM property on ANY element (official
            // `is_dom_property('muted')` is element-agnostic), so `<div muted={x}>`
            // emits `div.muted = $.get(x)` exactly like a `<video>` host.
            let shape = client_shapes::classify_dynamic_attr_shape(name, el_span)?;
            facts
                .borrow_mut()
                .dynamic_attr_shapes
                .push((node_id, attr_idx, shape));
            Ok(())
        }
        AttrIr::Class { .. } => {
            // A `class={…}` attribute or `class:` directive → `$.set_class` .
            facts.borrow_mut().dynamic_attr_shapes.push((
                node_id,
                attr_idx,
                ClientDynamicAttrShape::Class,
            ));
            Ok(())
        }
        AttrIr::Style { .. } => {
            // A `style={…}` attribute or `style:` directive → `$.set_style` .
            facts.borrow_mut().dynamic_attr_shapes.push((
                node_id,
                attr_idx,
                ClientDynamicAttrShape::Style,
            ));
            Ok(())
        }
        AttrIr::Spread { .. } => {
            // A spread switches the element's WHOLE attribute strategy: every co-located
            // attribute folds — in source order — into the single `$.attribute_effect`
            // object literal. The spread-incompatible directives (event / `bind:` / `use:`
            // / `transition:` / `let:`) were already refused at the element level (before
            // this per-attr loop), so reaching here means the element's whole attribute
            // set is foldable. Record the element as a spread element ONCE (dedup, since a
            // multi-spread element hits this arm per spread); the plan reads the
            // source-ordered fold from the IR.
            if !facts.borrow().spread_elements.contains(&node_id) {
                facts.borrow_mut().spread_elements.push(node_id);
            }
            Ok(())
        }
        AttrIr::Bind { target, expr } => {
            // The narrow bind classifier: `bind:value` on an `<input>` to a
            // signal/plain identifier or a public member, and element `bind:this` to
            // an identifier. A PROP target, a non-lvalue (`{f()}`), a sequence
            // get/set pair, or a member `bind:this` fails closed (5c). Drives the
            // SCOPE-AWARE prop/signal resolution from the binding table.
            // The analyzed bound expression carries its source AND the shared
            // bind-target fact; the classifier reads both from it (no reparse).
            let analyzed = expr.map(|e| ir.analysis.expressions.get(e));
            let scope = analyzed
                .map(|a| a.scope)
                .unwrap_or_else(|| ir.root_scope().scope);
            // The DECLARED instance + module script top-level locals — a `bind:this`
            // target must name one of these (the §1.2-core shape-3 `let el;` local);
            // a free / undeclared target fails closed (5c), mooting the DOM-local
            // collision an undeclared target would otherwise cause. Computed ONCE per
            // compile and threaded in (loop-invariant), never re-parsed per bind attr.
            // The host element's typed attribute inventory — the input to the
            // official host-attribute bind gates (`bind:checked` needs a static
            // `type="checkbox"`, contenteditable binds need a static
            // `contenteditable`, `<select multiple bind:value>` needs a static
            // `multiple`). Read from the typed `ElementIr`, never a source scan.
            let host_attrs: &[AttrIr] = match ir.node(node_id) {
                IrNode::Element(el) => &el.attrs,
                _ => &[],
            };
            let shape = client_shapes::classify_bind_shape(
                target,
                tag,
                host_attrs,
                analyzed,
                scope,
                &ir.analysis.bindings,
                &ir.analysis.scopes,
                declared_root_names,
                el_span,
            )?;
            facts
                .borrow_mut()
                .bind_shapes
                .push((node_id, target.clone(), shape));
            Ok(())
        }
        AttrIr::Event {
            event_type,
            handler,
            delegated,
            capture: _,
            modifiers,
            passive: _,
            origin: _,
        } => {
            // A regular intrinsic DOM element hosts the full event surface: a delegated
            // modern attribute (`$.delegated`), a non-delegated / capture-phase / legacy
            // modifier-bearing event (`$.event` + the 4th/5th positional capture/passive
            // args + the modifier wrappers). The legacy modifier set is validated against
            // the official `validate_element` rules — an unknown modifier or an
            // official-invalid combo (`passive` + `preventDefault` / `passive` +
            // `nonpassive`) is refused, matching official's
            // `event_handler_invalid_modifier[_combination]` compile errors.
            refuse_invalid_event_modifiers(modifiers, event_type, el_span)?;
            // The DIRECT (`$.event`) path admits any non-async inline arrow / function
            // expression; a DELEGATED (`$.delegated`) handler keeps the NARROW §1.2
            // `$state`-write arrow boundary (no regression).
            let direct = !*delegated;
            let analyzed = ir.analysis.expressions.get(*handler);
            let shape = client_shapes::classify_event_handler_shape(
                analyzed.source,
                event_type,
                el_span,
                analyzed.scope,
                &ir.analysis.bindings,
                &ir.analysis.scopes,
                direct,
            )?;
            // Key the fact by (node, event type, handler expr) so an element with
            // multiple events resolves EACH to its own shape at projection time.
            facts
                .borrow_mut()
                .event_shapes
                .push((node_id, event_type.clone(), *handler, shape));
            Ok(())
        }
        // The element LIFECYCLE directives — `use:` actions, `transition:`/`in:`/`out:`
        // transitions, `animate:` animations, and element-position `{@attach}`
        // attachments — are ACCEPTED on a regular intrinsic element. Their emission is
        // owned by the corresponding `ClientRuntimeOp::Lifecycle` projection; the
        // transition-overlap conflict and the `animate:` keyed-each placement are
        // validated by the dedicated element/pre-pass gates
        // ([`refuse_transition_conflicts`] / [`refuse_invalid_animate_placement`]),
        // which run BEFORE this per-attr accept.
        AttrIr::Use { .. }
        | AttrIr::Transition { .. }
        | AttrIr::Animate { .. }
        | AttrIr::Attach { .. } => Ok(()),
        AttrIr::Let { .. } => Err(UnsupportedSvelteRuntimeSurface::ComponentOrSnippet {
            construct: "let-directive",
            span: el_span,
        }),
    }
}
