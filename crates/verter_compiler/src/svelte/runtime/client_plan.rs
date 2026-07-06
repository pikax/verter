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
use super::client_codegen_helpers::op_target_node;
use super::client_shapes::{
    ClientBindShape, ClientDynamicAttrShape, ClientEventHandlerShape, ClientInterpolationShape,
};
use super::client_surface::ClassifiedClientSurface;
use super::events::{validate_event_modifiers, EventModifierError};
use super::expr::{BindingRuntimeKind, ExprRefKind, ScopeId};
use super::expr_emit;
use super::expr_rewrite::{PropReads, ProxyInitMap};
use super::ir::{AttrIr, AttrOpKind, ExprId, IrNode, NodeId, OpId, RuntimeOp, SvelteRuntimeIr};
use verter_span::Span;

// The narrow client-plan VOCABULARY (the closed node / attribute / op / value type set
// the emitter consumes) lives in the sibling `client_plan_types` module; this builder
// projects the broad IR onto it. Re-exported so existing consumers (`super::client`, …)
// keep importing the vocabulary as `super::client_plan::<Type>`.
pub(super) use super::client_plan_types::{
    AttrValue, ClientAttr, ClientBindTarget, ClientBlock, ClientNode, ClientNodeId,
    ClientRuntimeOp, ClientScriptItem, ElementLifecycleOp, EventEmit, EventEmitTarget, EventMode,
    RegionOps,
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
    /// The component-FUNCTION-BODY statements, in source order. (Static imports are
    /// NOT here — every top-level import from either script slot hoists to module
    /// scope on `user_imports`; the body carries the remaining instance script
    /// items.)
    pub(super) body_statements: Vec<ClientScriptItem>,
    /// The hoisted `$props.id()` declaration (`const <name> = $.props_id();`) —
    /// emitted at the ABSOLUTE function-body top, ABOVE the `$.push` frame line
    /// (the official hoist slot). `None` for the common no-`$props.id` component.
    pub(super) props_id_hoist: Option<String>,
    /// The narrow reactive ops, grouped by their owning template-scope REGION (the root
    /// region plus every block body / branch region), in source order within each region.
    /// A block body's reactive surface is its OWN region: the emitter builds each region's
    /// combined `$.template_effect` + binds + events from its region's ops.
    pub(super) region_ops: Vec<RegionOps>,
    /// The module-scope USER imports — EVERY top-level static import from BOTH script
    /// slots on the shared [`UserImport`](super::client_imports::UserImport) carrier
    /// (default / named / namespace / side-effect forms, `with { … }` attributes
    /// preserved), each slot in source order. Emitted in the official TWO-SLOT order:
    /// `<script module>` imports BEFORE the runtime namespace import (after
    /// disclose-version/flags), instance-script imports AFTER it. Empty for a
    /// component with no user imports.
    pub(super) user_imports: Vec<super::client_imports::UserImport>,
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
    /// The classified `$store` auto-subscriptions in first-seen order — the
    /// per-store accessor emission input (`const $NAME = () =>
    /// $.store_get(NAME, '$NAME', $$stores);`). Empty for a store-less
    /// component.
    pub(super) store_subscriptions: Vec<super::store_subscriptions::StoreSubscription>,
    /// Whether the component has at least one `$store` auto-subscription — the
    /// SOLE driver of the `$.setup_stores()` / accessor / trailing
    /// `$$cleanup();` emission. SEPARATE from [`needs_context`](Self::needs_context)
    /// by design: the component-context frame is driven by the EXISTING
    /// `needs_context` triggers (an imported call / `new` / unsafe member),
    /// NEVER by store presence — a clean local store emits setup/cleanup with
    /// NO frame (oracle-verified against svelte@5.56.3).
    pub(super) has_store_subscriptions: bool,
    /// The custom-element module-epilogue payload (`customElements.define(tag,
    /// $.create_custom_element(…))` / the bare create statement) — `Some` iff the
    /// component compiles as a custom element.
    pub(super) custom_element: Option<super::client_custom_element::CustomElementEmission>,
    /// The custom-element `$$exports` get/set accessor pairs (one per `$props()`
    /// member, declaration order). Non-empty forces the component context and
    /// flips the close to `return $.pop($$exports)`. Always empty for a
    /// non-custom-element component.
    pub(super) ce_exports: Vec<super::client_custom_element::CeExportAccessor>,
    /// The build-time analysis the emitter reads for the reactive-text rewrite (the
    /// memoizer consults the per-interpolation rewritten expression). Retained as a
    /// borrow so the plan stays the single emitter input without re-deriving.
    pub(super) build: SupportedClientIr<'a>,
}

/// Collect the prop LOCAL names WRITTEN anywhere — every reference of kind
/// `Reassign` / `DeepMutate` that resolves (scope-awarely, so a shadowing local
/// of the same name never counts) to a `Prop` / `BindableProp` binding. This is
/// the official `updated` flag axis in runes mode (`reassigned || mutated`) and
/// the write half of `is_prop_source`.
///
/// The accepted prop-WRITE surfaces are exactly TWO — this collection observes
/// both (any other instance-script prop reference is fail-closed upstream by
/// the prop-usage gate):
///
/// 1. TEMPLATE expressions (handlers / interpolations / binds) — the analyzed
///    expression arena, resolved through each expression's own scope;
/// 2. `$props()` DEFAULT expressions (a self write `{ a = (a = 1) }`, a sibling
///    write `{ a = (b.x++), b = {} }`, plain or `$bindable(...)`) — enumerated
///    through the SAME unified [`expr_emit::PropsDeclaratorPlan`] the `$.prop`
///    lowering consumes (its member default spans, no separate re-scan) and
///    harvested with the SAME shared reference collector the template arena uses
///    ([`super::expr::collect_expr_references`] — expression-local shadowers
///    such as an arrow param already excluded), resolved at the instance ROOT
///    scope (where the defaults lexically live).
fn collect_prop_updated_locals(
    ir: &SvelteRuntimeIr,
    decl_plan: Option<&expr_emit::PropsDeclaratorPlan>,
) -> rustc_hash::FxHashSet<String> {
    let mut updated = rustc_hash::FxHashSet::default();
    let mark_prop_writes =
        |scope: ScopeId,
         references: &[super::expr::ExprReference],
         updated: &mut rustc_hash::FxHashSet<String>| {
            for r in references {
                if !matches!(r.kind, ExprRefKind::Reassign | ExprRefKind::DeepMutate) {
                    continue;
                }
                if matches!(
                    ir.analysis
                        .bindings
                        .resolve_kind(&ir.analysis.scopes, scope, &r.name),
                    Some(BindingRuntimeKind::Prop | BindingRuntimeKind::BindableProp)
                ) {
                    updated.insert(r.name.clone());
                }
            }
        };
    // (1) The template expression arena.
    for expr in ir.analysis.expressions.all() {
        mark_prop_writes(expr.scope, &expr.references, &mut updated);
    }
    // (2) The `$props()` default expressions, read off the UNIFIED plan's member
    // default spans (no separate declarator re-scan). A default that fails the
    // wrapped reparse contributes nothing here — the same slice refuses the
    // shared rewriter at lowering, so the component never emits with a missed
    // mark.
    if let (Some(instance), Some(plan)) = (ir.analysis.scripts.instance_source, decl_plan) {
        let root_scope = ir.root_scope().scope;
        for member in &plan.members {
            let Some(default) = &member.default else {
                continue;
            };
            let Some(src) = instance.get(default.span.0 as usize..default.span.1 as usize) else {
                continue;
            };
            let Ok(facts) = super::expr::collect_expr_references(src) else {
                continue;
            };
            mark_prop_writes(root_scope, &facts.references, &mut updated);
        }
    }
    updated
}

/// The resolved `$props()` rest / whole-object capture hoist facts: the ALLOCATED
/// module-scope `rest_excludes` Set var name (collision-renamed through the SAME
/// seeded uniquifier the emitter's DOM vars use — the official `scope.root.unique`
/// equivalent), the ordered exclude keys the `new Set([…])` literal quotes, and
/// the local binding name the `$.rest_props($$props, rest_excludes)` declarator
/// binds. Resolved ONCE at plan build so the module hoist (emitter) and the body
/// declarator (plan) reference the SAME allocated name — never two independently
/// chosen names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RestPropsHoist {
    /// The rest / whole-object local binding name (`rest` / `all`).
    pub(super) local: String,
    /// The allocated module-scope `rest_excludes` Set var name.
    pub(super) set_name: String,
    /// The ordered exclude keys (the fixed prefix + each non-rest source key).
    pub(super) excludes: Vec<String>,
}

/// Seed the reserved-name union an allocated stem must avoid — the UNION of every
/// top-level script binding ([`SupportedClientIr::declared_roots`]), every recorded
/// binding-table name (template-scope bindings included), every free
/// template-expression reference, and the runtime-magic reserved literals. This is
/// the official `scope.generate` reservation set; both the emitter's DOM-var
/// allocator and the plan-time `rest_excludes` allocation seed from it so a
/// generated stem never collides with a user/template binding.
pub(super) fn seed_reserved_names(build: &SupportedClientIr<'_>) -> rustc_hash::FxHashSet<String> {
    let mut used = rustc_hash::FxHashSet::default();
    for name in &build.declared_roots {
        used.insert(name.clone());
    }
    for binding in build.ir.analysis.bindings.all() {
        used.insert(binding.name.clone());
    }
    for analyzed in build.ir.analysis.expressions.all() {
        for reference in &analyzed.references {
            used.insert(reference.name.clone());
        }
    }
    for reserved in ["$", "$$anchor", "$$props", "$$value"] {
        used.insert(reserved.to_string());
    }
    used
}

/// Allocate a deterministic variable name from a preferred stem, appending a `_N`
/// suffix on collision (mirroring the official allocator's stem + counter). Shared
/// by the plan-time `rest_excludes` allocation and the emitter's DOM-var allocator
/// so both uniquify identically.
pub(super) fn alloc_unique_name(used: &mut rustc_hash::FxHashSet<String>, stem: &str) -> String {
    if used.insert(stem.to_string()) {
        return stem.to_string();
    }
    let mut n = 1;
    loop {
        let candidate = format!("{stem}_{n}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        n += 1;
    }
}

/// The semantic projection stage — it attaches the reactivity / lvalue / prop-read
/// facts the narrow plan needs, then builds the [`ClientModulePlan`].
pub(super) struct SupportedClientIr<'a> {
    /// The runtime IR (read for the structural template walk + the reactive-text
    /// rewrite at emit time).
    pub(super) ir: &'a SvelteRuntimeIr<'a>,
    /// The UNIFIED `$props()` declarator plan — the SINGLE scan authority (built
    /// ONCE in [`Self::build`]) that produces the prop-read forms, feeds the
    /// prop-updated harvest, and drives the `$.prop` destructure lowering. `None`
    /// for a component with no `$props()`.
    pub(super) decl_plan: Option<expr_emit::PropsDeclaratorPlan>,
    /// The component's `$props()` read forms.
    pub(super) prop_reads: PropReads,
    /// The prop LOCAL names WRITTEN anywhere (a template-expression or
    /// `$props()`-default reassign / deep-mutate resolving scope-awarely to a
    /// `Prop` / `BindableProp` binding) — the official `updated` flag axis
    /// (bit 4) and the `is_prop_source` write half.
    pub(super) prop_updated: rustc_hash::FxHashSet<String>,
    /// The per-instance one-hop proxy-init map (threaded into the TEMPLATE-side
    /// rewrite so a handler reassignment matches the official `should_proxy(rhs)`).
    pub(super) proxy_inits: ProxyInitMap,
    /// The component-declared root names (the `has_call` memoizer `is_pure` input).
    pub(super) declared_roots: rustc_hash::FxHashSet<String>,
    /// The resolved `$props()` rest / whole-object capture hoist (the allocated
    /// `rest_excludes` Set name + ordered exclude keys + local binding name), or
    /// `None` for a component with no rest / whole-object `$props()` capture. Read
    /// by BOTH the body declarator ([`Self::lower_props_destructure`]) and the
    /// emitter's module hoist so they reference the SAME allocated name.
    pub(super) rest_props: Option<RestPropsHoist>,
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
        // A CUSTOM ELEMENT compiles with `accessors` (the official
        // `is_custom_element` force) and carries the extra `'$$host'` rest-exclude
        // key — resolved once and threaded to the declarator plan + read forms.
        let ce_accessors = ir.component.custom_element.is_some();
        // The UNIFIED `$props()` declarator plan — ONE scan of the accepted
        // declarator, built here after upstream validation and threaded to EVERY
        // consumer (the prop-updated harvest, the read forms, the `$.rest_props`
        // hoist, and the `$.prop` destructure lowering). `None` when there is no
        // `$props()`.
        let decl_plan = ir
            .analysis
            .scripts
            .instance_source
            .and_then(|src| expr_emit::PropsDeclaratorPlan::build(&alloc, src, ce_accessors));
        // The prop WRITE facts (a template-expression or `$props()`-default
        // reassign / deep-mutate resolving to a prop binding) — the `updated` flag
        // axis. Computed BEFORE the read forms so a written prop's reads flip to
        // the getter (`is_prop_source`); harvested from the unified plan's member
        // default spans. A CUSTOM ELEMENT compiles with `accessors` (the official
        // `is_custom_element` force), so EVERY member is a prop source there.
        let prop_updated = collect_prop_updated_locals(ir, decl_plan.as_ref());
        let prop_reads = decl_plan
            .as_ref()
            .map(|plan| plan.prop_reads(&prop_updated, ce_accessors))
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
            &ir.analysis.script_imports,
        );
        let mut projection = SupportedClientIr {
            ir,
            decl_plan,
            prop_reads,
            prop_updated,
            proxy_inits,
            declared_roots,
            rest_props: None,
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

        // (0) Resolve the `$props()` rest / whole-object capture hoist BEFORE the
        // body statements: allocate the module-scope `rest_excludes` Set name once
        // through the seeded uniquifier (the official `scope.root.unique`
        // equivalent), so the body declarator ([`lower_props_destructure`], reached
        // through `build_script_items` next) and the emitter's module hoist
        // reference the SAME name. The capture facts come from the UNIFIED plan (no
        // re-scan); the owned `(local, excludes)` is lifted out first so the
        // seed-reservation borrow of `projection` does not overlap the plan borrow.
        let rest_capture = projection
            .decl_plan
            .as_ref()
            .and_then(|plan| plan.rest.as_ref())
            .map(|rest| (rest.local.clone(), rest.excludes.clone()));
        projection.rest_props = rest_capture.map(|(local, excludes)| {
            let mut used = seed_reserved_names(&projection);
            let set_name = alloc_unique_name(&mut used, "rest_excludes");
            RestPropsHoist {
                local,
                set_name,
                excludes,
            }
        });

        // (1) The component-body statements from the TYPED instance-script item
        // allowlist (a `<script module>` / instance import is fail-closed upstream, so
        // there are no module-scope imports / hoists). A function-pair function body
        // lowers through the fallible rewriter, so this is fallible. A `$props.id()`
        // item ALSO yields the body-top hoist (`const <name> = $.props_id();`).
        let (body_statements, props_id_hoist) = projection.build_script_items()?;

        // (2) The narrow node arena (mirrors the supported IR node space). The
        // reactivity decision for each interpolation is made here: a non-reactive
        // interpolation fails closed (the official compiler static-folds it).
        let nodes = projection.build_nodes()?;

        // (3) The narrow ops (reactive text / binds / events), grouped per region, with
        // bind/event expressions rewritten through the fallible rewriter.
        let region_ops = projection.build_ops(&nodes)?;

        // (4) The custom-element payload: the module-epilogue create/define facts
        // plus the `$$exports` accessor pairs (one get/set per `$props()` member —
        // the official `analysis.accessors` force under a custom element). BOTH
        // are fact-driven: a no-props custom element carries an emission payload
        // but NO accessors (and so no `$.push`/`$.pop($$exports)` frame).
        let ce_members = projection
            .decl_plan
            .as_ref()
            .map(|plan| plan.members.as_slice())
            .unwrap_or_default();
        let custom_element = ir.component.custom_element.as_ref().map(|descriptor| {
            super::client_custom_element::build_custom_element_emission(descriptor, ce_members)
        });
        let ce_exports = if custom_element.is_some() {
            super::client_custom_element::build_ce_export_accessors(
                ce_members,
                ir.analysis.scripts.instance_source,
            )
        } else {
            Vec::new()
        };

        // (5) Component context + props-param facts. The `$$exports` accessor
        // frame opens the component context (the official `should_inject_context`
        // arm for a non-empty component-returned object). `$host` usage inside a
        // custom element does NOT force the `$$props` parameter by itself:
        // official binds the parameter only when an INDEPENDENT
        // props-parameter trigger exists — a REAL props binder (`$props()` /
        // `$bindable(...)` / legacy prop, i.e. `Prop | BindableProp`) or a
        // `needs_context` reason. A member on the `$host()` call result IS such
        // a reason (a call-result-rooted member is never a "safe identifier"),
        // in a handler (`$host().x`) and in a `{@render}` dynamic callee
        // (`{@render $host().snip()}` — the peeled callee scans) alike; an
        // alias like `const h = $host(); h.x` is NOT a trigger. With NEITHER,
        // official emits the DEGENERATE-UNBOUND residue (`function
        // App($$anchor)` whose body still reads `$$props.$$host` — a runtime
        // `ReferenceError`); this backend refuses that residue BEFORE emission
        // instead of silently repairing the binding. `props_param_bound` is
        // THE single props-parameter-bound fact: any future `$$props`-binding
        // trigger extends it, never this gate. All inputs are
        // classifier/analysis FACTS (the rune scan's `$host()` admission over
        // the instance script + every template expression, the shared
        // `needs_context` analysis, the binding table) — no re-parse here.
        let needs_context = projection.needs_context(&alloc) || !ce_exports.is_empty();
        let host_used = custom_element.is_some() && classified.uses_host;
        let real_props_binder = ir.analysis.bindings.all().iter().any(|b| {
            matches!(
                b.kind,
                BindingRuntimeKind::Prop | BindingRuntimeKind::BindableProp
            )
        });
        let props_param_bound = real_props_binder || needs_context;
        if host_used && !props_param_bound {
            return Err(UnsupportedSvelteRuntimeSurface::HostOrCustomElement {
                surface: "$host",
                span: classified.first_host_span.unwrap_or(Span::new(0, 0)),
            });
        }
        let uses_props = props_param_bound;

        // The `$store` auto-subscription facts (classifier-owned; the plan reads
        // them for the setup_stores/accessor/$$cleanup emission ONLY — store
        // presence NEVER feeds `needs_context` above: a clean local store emits
        // setup/cleanup with NO `$.push`/`$.pop` frame, oracle-verified). When a
        // store component ALSO carries custom-element `$$exports` prop accessors,
        // the close uses the official PRE-RETURN finalizer slot — `var $$pop =
        // $.pop($$exports); $$cleanup(); return $$pop;` (the returned object is
        // captured, the store `$$cleanup()` runs, then the captured value
        // returns), emitted by the close in `client.rs`. The combination is
        // SUPPORTED, not fail-closed.
        let store_subscriptions = classified.store_subscriptions.clone();
        let has_store_subscriptions = !store_subscriptions.is_empty();

        let (module_snippets, instance_snippets) = projection.collect_top_level_snippets();

        Ok(ClientModulePlan {
            component: ir.component.clone(),
            nodes,
            body_statements,
            props_id_hoist,
            region_ops,
            user_imports: classified.user_imports.clone(),
            module_snippets,
            instance_snippets,
            needs_context,
            uses_props,
            store_subscriptions,
            has_store_subscriptions,
            custom_element,
            ce_exports,
            build: projection,
        })
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
            // The GLOBAL-host specials (`<svelte:window|document|body>`) — NON-RENDERING
            // init-only hosts. Their events/binds ride the region ops (projected against the
            // global host); the node itself clones no template.
            IrNode::Special(s)
                if matches!(
                    s.kind,
                    super::ir::SpecialKind::Window
                        | super::ir::SpecialKind::Document
                        | super::ir::SpecialKind::Body
                ) =>
            {
                Ok(ClientNode::SpecialHost {
                    kind: s.kind,
                    span: s.span,
                })
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
            // A `<svelte:element this={…}>` dynamic element — projected to the comment-anchored
            // `$.element(node, get_tag, is_svg, callback)` renderable.
            IrNode::Special(s) if s.kind == super::ir::SpecialKind::Element => {
                self.project_svelte_element(s, id)
            }
            // A `<svelte:boundary>` — projected to the comment-anchored `$.boundary(node, props,
            // callback)` renderable.
            IrNode::Special(s) if s.kind == super::ir::SpecialKind::Boundary => {
                self.project_svelte_boundary(s)
            }
            // A `<svelte:head>` — projected to the `$.head('<hash>', ($$anchor) => { <body> })`
            // head-region call (the title effect + the non-title body region).
            IrNode::Special(s) if s.kind == super::ir::SpecialKind::Head => {
                self.project_svelte_head(s)
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
            // An element LIFECYCLE directive (`use:` / `transition:` / `animate:` /
            // element `{@attach}`) — the emission lives on the corresponding
            // `ClientRuntimeOp::Lifecycle`; the element attr records the supported
            // KIND only (the structural mirror).
            AttrIr::Use { .. }
            | AttrIr::Transition { .. }
            | AttrIr::Animate { .. }
            | AttrIr::Attach { .. } => Ok(ClientAttr::Lifecycle),
            // A `let:` on an ordinary element was refused by the classifier — defensive.
            AttrIr::Let { .. } => Err(UnsupportedSvelteRuntimeSurface::DynamicAttribute {
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
            // A spread element's ATTRIBUTE-domain ops are absorbed into the fold; its
            // LIFECYCLE ops (`use:` / `transition:` / `animate:` / `{@attach}`) are
            // NOT — official emits them ALONGSIDE the fold (`$.attribute_effect` →
            // `$.action` → `$.transition`, the fixture-#18 order), so they project
            // normally.
            if self.spread_elements.contains(&target)
                && !matches!(
                    self.ir.op(op_id),
                    RuntimeOp::SpreadAttrs { .. }
                        | RuntimeOp::Action { .. }
                        | RuntimeOp::Transition { .. }
                        | RuntimeOp::Animation { .. }
                        | RuntimeOp::Attachment { .. }
                )
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
            // A `use:` action — the callee is the rewritten action expression (an
            // identifier / member path; a signal callee reads `$.get(fn)`), the
            // optional argument the concise-arrow-wrapped getter-thunk body.
            RuntimeOp::Action { target, action } => {
                let analyzed = self.ir.analysis.expressions.get(action.expr);
                let callee = self.rewrite(action.expr, analyzed.scope)?;
                let arg = match action.arg {
                    Some(arg) => Some(self.rewrite_arrow_body_value(arg)?),
                    None => None,
                };
                out = Some(ClientRuntimeOp::Lifecycle(ElementLifecycleOp::Action {
                    target: ClientNodeId(target.0),
                    callee,
                    arg,
                }));
            }
            // A `transition:` / `in:` / `out:` — the FLAG integer is precomputed from
            // the typed kind + `|global` (the official TRANSITION_IN|OUT|GLOBAL
            // arithmetic); the fn expression is the directive NAME rewritten in the
            // op's scope (so a signal / prop transition fn lowers).
            RuntimeOp::Transition { target, transition } => {
                let kind_flags = match transition.kind {
                    super::ir::TransitionKind::Transition => 3,
                    super::ir::TransitionKind::In => 1,
                    super::ir::TransitionKind::Out => 2,
                };
                let flags = kind_flags | if transition.global { 4 } else { 0 };
                let get_fn = self.rewrite_source(&transition.name, scope_lexical)?;
                let params = match transition.expr {
                    Some(expr) => Some(self.rewrite_arrow_body_value(expr)?),
                    None => None,
                };
                out = Some(ClientRuntimeOp::Lifecycle(ElementLifecycleOp::Transition {
                    target: ClientNodeId(target.0),
                    flags,
                    get_fn,
                    params,
                }));
            }
            // An `animate:` — its OWN `$.animation` family (validated keyed-each-only
            // by the classifier's placement pre-pass; never a `$.transition`).
            RuntimeOp::Animation { target, animation } => {
                let get_fn = self.rewrite_source(&animation.name, scope_lexical)?;
                let params = match animation.expr {
                    Some(expr) => Some(self.rewrite_arrow_body_value(expr)?),
                    None => None,
                };
                out = Some(ClientRuntimeOp::Lifecycle(ElementLifecycleOp::Animation {
                    target: ClientNodeId(target.0),
                    get_fn,
                    params,
                }));
            }
            // An element-position `{@attach expr}` — the payload is the
            // concise-arrow-wrapped getter-thunk body (an inline arrow / object
            // payload stays a valid expression body).
            RuntimeOp::Attachment { target, expr } => {
                let payload = self.rewrite_arrow_body_value(*expr)?;
                out = Some(ClientRuntimeOp::Lifecycle(ElementLifecycleOp::Attachment {
                    target: ClientNodeId(target.0),
                    payload,
                }));
            }
        }
        Ok(out)
    }

    /// Whether a template expression references a reactive SIGNAL (the official
    /// `metadata.expression.has_state`). A dynamic attribute / class / style value
    /// with state joins the combined `$.template_effect`; a stateless value is a
    /// one-shot init (`RegularElement.js`'s `has_state ? update : init`). A
    /// `<svelte:head>`'s `<title>` reads this to pick `$.effect` (static) vs
    /// `$.deferred_template_effect` (stateful).
    pub(super) fn expr_has_state(&self, expr_id: ExprId) -> bool {
        let analyzed = self.ir.analysis.expressions.get(expr_id);
        // Official `has_state` is set by a reactive signal/prop reference OR by a BINDING
        // IMPURITY: a MEMBER access rooted at any declared binding
        // (`MemberExpression.js`'s `!is_pure(node)` rule — a member on a demoted `$state`
        // / plain local is impure ⇒ has_state, so `{d.x}` joins the `$.template_effect`
        // even though `d` is not a live signal) OR an assignment/update MUTATION (a write
        // is not pure, so `{obj.x = 1}` / `{plain++}` also join the `$.template_effect`).
        super::reactive_analysis::expr_references_signal(
            analyzed.source,
            analyzed.scope,
            &self.ir.analysis.bindings,
            &self.ir.analysis.scopes,
        ) || super::reactive_analysis::expr_has_binding_impurity(
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
}
