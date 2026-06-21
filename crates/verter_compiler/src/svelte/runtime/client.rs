//! The Svelte `svelte/internal/client` JS emission backend.
//!
//! Consumes the runtime IR ([`SvelteRuntimeIr`]), the static-template plan
//! ([`StaticTemplatePlan`]), and the client topology ([`ClientTopologyPlan`])
//! and emits the executable client module — the `import * as $ from
//! 'svelte/internal/client'` shape the official `svelte@5.56.3` compiler
//! produces. It owns the four output regions:
//!
//! 1. module imports (`import 'svelte/internal/disclose-version'` + the runtime
//!    namespace),
//! 2. module hoists (the `$.from_html(...)` template factories),
//! 3. the component body (the rune declarations, the DOM walk, the reactive
//!    ops, the mount),
//! 4. the `$.delegate([...])` epilogue.
//!
//! The emitter drives EVERY semantic decision from the typed IR + the scope-aware
//! binding table — never a source-text scan. Each ORIGINAL expression segment
//! (a handler body, an interpolation expression, a `$state` initializer) is
//! rewritten through its own [`CodeTransform`] over the expression's source
//! slice (the read/write rewrites are mapped `overwrite` ops on the OXC AST
//! spans), so the load-bearing source-derived edits stay on the CodeTransform
//! authority; the surrounding synthesized scaffolding is unmapped.
//!
//! Anything outside the supported subset FAILS CLOSED with a typed
//! [`UnsupportedSvelteRuntimeSurface`] carrying its owning vertical — never a
//! silent empty module, never a panic.

use super::client_plan::{ClientBindTarget, ClientModulePlan, ClientNode, ClientRuntimeOp};
use super::entity_decode::decode_text_entities;
use super::helpers::{ImportPlan, RuntimeImport};
use super::html::{StaticTemplatePlan, TemplateFactory};
use super::ir::{AttrIr, IrNode, NodeId, SvelteRuntimeIr, TemplateScopeId};
use super::topology::ClientTopologyPlan;
use super::whitespace::{
    clean_nodes, cleaned_text_run_parts, CleanContext, CleanItem, RunTextPart,
};

pub use super::unsupported::UnsupportedSvelteRuntimeSurface;

/// The emitted client module: the JS source plus the structural facts a caller
/// (a topology gate / the carrier) reads back without re-parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientModule {
    /// The full emitted JS module source.
    pub code: String,
}

/// Emit the `svelte/internal/client` module for a NARROW client module plan.
///
/// The emitter consumes ONLY the [`ClientModulePlan`] — the closed-vocabulary
/// projection a default-deny classifier + semantic projection produced. It never
/// sees the broad [`SvelteRuntimeIr`] taxonomy as an emission input (the broad IR is
/// reachable only through the plan's retained `SupportedClientIr` for the DOM-walk
/// GEOMETRY — node positions — never for an emit-by-default decision). Because an
/// unsupported surface has NO place in the narrow plan, emit-by-default is
/// structurally impossible.
///
/// Returns the emitted module — the plan was built only from a classified surface,
/// so emission itself is infallible (every fail-closed decision happened upstream).
pub(super) fn emit_client_module(
    plan: &ClientModulePlan,
    html_plan: &StaticTemplatePlan,
    topology: &ClientTopologyPlan,
) -> ClientModule {
    let mut emitter = ClientEmitter::new(plan);
    emitter.emit(html_plan, topology)
}

// ---------------------------------------------------------------------------
// The emitter
// ---------------------------------------------------------------------------

/// The client-module emitter — holds the NARROW plan and the deterministic name
/// allocator state.
struct ClientEmitter<'a> {
    /// The narrow plan — the SOLE emission input.
    plan: &'a ClientModulePlan<'a>,
    /// Reserved + already-allocated names (collision avoidance).
    used: rustc_hash::FxHashSet<String>,
    /// The emitted var name reaching each named DOM node (populated by the walk,
    /// read by the op emission).
    node_var: rustc_hash::FxHashMap<NodeId, String>,
    /// The emitted var name reaching each interpolation's text node (populated by
    /// the walk, read by the reactive-text op emission).
    interp_var: rustc_hash::FxHashMap<NodeId, String>,
}

impl<'a> ClientEmitter<'a> {
    fn new(plan: &'a ClientModulePlan<'a>) -> Self {
        let mut used = rustc_hash::FxHashSet::default();
        // Reserve every user script binding name + the runtime-magic identifiers
        // so a generated stem never collides with a user binding (matching the
        // official `scope.generate`'s reservation of declared names). The GENERATED
        // stems (`root`, `fragment`, tag names, `text`) are NOT pre-reserved — they
        // are the names we want to allocate; a user binding of the same name pushes
        // the generated one to a `_N` suffix via `alloc_name`.
        //
        // The seed is the COMPLETE top-level user-binding set (`declared_roots`):
        // imports, every `let`/`const`/`var`, function / class declarations, and the
        // `$props()` destructure names across BOTH the module and instance scripts —
        // NOT just the reactive `bindings.all()` rows (which omit a PLAIN non-rune
        // local like `let fragment = 1`). A plain local that shares a generated stem
        // (`let fragment` vs the multi-root clone frame, `let div` vs a `<div>` clone
        // root) would otherwise emit a duplicate declaration (invalid JS); seeding the
        // full set makes `alloc_name` rename the synthesized local instead.
        for name in &plan.build.declared_roots {
            used.insert(name.clone());
        }
        for reserved in ["$", "$$anchor", "$$props", "$$value"] {
            used.insert(reserved.to_string());
        }
        Self {
            plan,
            used,
            node_var: rustc_hash::FxHashMap::default(),
            interp_var: rustc_hash::FxHashMap::default(),
        }
    }

    /// The runtime IR, reached through the plan's retained semantic projection —
    /// read ONLY for the DOM-walk GEOMETRY (the `clean_nodes` whitespace/run
    /// partition + sibling sequences operate on the IR node arena), never for an
    /// emission DECISION. Every emission decision (the node KIND, the element tag,
    /// the supported-attr classification, the reactive-text body, the op set) reads
    /// the NARROW plan vocabulary via [`Self::client_node`] / `self.plan.ops`.
    ///
    /// Returns the `'a`-lifetime reference (copied out of the plan) so it does NOT
    /// borrow `self` — the walk reads IR geometry while still mutating `self`'s
    /// var-name maps.
    fn ir(&self) -> &'a SvelteRuntimeIr<'a> {
        self.plan.build.ir
    }

    /// The NARROW node for an IR node id (the plan node arena mirrors the IR node
    /// arena index-for-index). This is the EMISSION-decision view: the walk reads a
    /// node's KIND / tag / supported attrs from here, never from the broad
    /// [`IrNode`] taxonomy.
    fn client_node(&self, id: NodeId) -> &'a ClientNode {
        &self.plan.nodes[id.0 as usize]
    }

    /// Allocate a deterministic variable name from a preferred stem, appending a
    /// `_N` suffix on collision (mirroring the official allocator's stem +
    /// counter).
    fn alloc_name(&mut self, stem: &str) -> String {
        if self.used.insert(stem.to_string()) {
            return stem.to_string();
        }
        let mut n = 1;
        loop {
            let candidate = format!("{stem}_{n}");
            if self.used.insert(candidate.clone()) {
                return candidate;
            }
            n += 1;
        }
    }

    /// Emit the full client module.
    fn emit(
        &mut self,
        html_plan: &StaticTemplatePlan,
        topology: &ClientTopologyPlan,
    ) -> ClientModule {
        let mut out = String::new();

        // (1) Module imports — disclose-version + flags + the runtime namespace, in
        // official order. (A `<script module>` / instance `import` is fail-closed
        // upstream — the script-hoisting deferral — so there are no user module
        // imports to emit at module scope.)
        emit_imports(&mut out, &topology.imports);
        out.push('\n');

        // (2) Module hoists — the `$.from_html(...)` template factories. The
        // supported surface is exactly the single-region (root) component; a
        // multi-region component (a nested block body) is a block surface already
        // refused upstream.
        let root_factory = html_plan.templates.first();
        let root_var = self.alloc_name("root");
        let mounts_fragment = emit_root_hoist(&mut out, &root_var, root_factory);
        out.push('\n');

        // (3) The component body.
        self.emit_body(&mut out, &root_var, mounts_fragment);

        // (4) The `$.delegate([...])` epilogue.
        if !topology.delegated_events.is_empty() {
            out.push('\n');
            emit_delegate_epilogue(&mut out, topology.delegated_events.ordered());
        }

        ClientModule { code: out }
    }

    /// Emit the component function body.
    fn emit_body(&mut self, out: &mut String, root_var: &str, mounts_fragment: bool) {
        // The component-context (`$.push`/`$.pop`) + props-param facts were decided
        // by the semantic projection (`SupportedClientIr::build`); the emitter reads
        // the narrow decision, never re-derives it.
        let needs_push = self.plan.needs_context;
        let params = if self.plan.uses_props {
            "$$anchor, $$props"
        } else {
            "$$anchor"
        };
        let name = &self.plan.component.name;
        out.push_str(&format!("export default function {name}({params}) {{\n"));

        if needs_push {
            // The RUNES form carries the trailing `true` (`$.push($$props, true)`)
            // — the runes-mode flag the official `5.56.3` compiler emits (a legacy
            // component would be `$.push($$props)`, but legacy fails closed at 5i).
            out.push_str("\t$.push($$props, true);\n");
        }

        // The component-function BODY statements (already lowered by the plan).
        for item in &self.plan.body_statements {
            out.push('\t');
            out.push_str(item.code());
            out.push('\n');
        }

        // The clone frame: `var fragment = root();` for a multi-root fragment,
        // `var <stem> = root();` for a single-element clone-root. EVERY synthesized
        // DOM local — including the multi-root `fragment` stem — routes through the
        // collision-aware allocator (seeded with the user's top-level bindings), so a
        // user binding of the same name (`let fragment`) pushes it to `fragment_1`
        // instead of emitting a duplicate declaration.
        let root_scope_id = self.ir().root;
        // The official PRE-CLONE cursor advance: when the component-ROOT region is a
        // multi-root fragment whose FIRST cleaned position is a TEXT / interpolation
        // run (the `is_text_first` case — `phases/3-transform/.../Fragment.js`), the
        // official compiler skips over the inserted leading anchor with a bare
        // `$.next();` emitted BEFORE `var fragment = root();`. (Inside an element the
        // leading text does NOT get this — the in-element walk advances via
        // `$.first_child` / `$.child`; only the root fragment is text-first-aware.) The
        // trailing static-run `$.next(skipped - 1)` in the walk is a SEPARATE cursor
        // advance and stays as-is.
        if mounts_fragment && self.root_region_is_text_first(root_scope_id) {
            out.push_str("\t$.next();\n");
        }
        let region_var = if mounts_fragment {
            self.alloc_name("fragment")
        } else {
            // The single clone-root element's own var (named by its tag).
            self.single_root_var_name(root_scope_id)
        };
        out.push_str(&format!("\tvar {region_var} = {root_var}();\n"));

        // The DOM walk populates the node/interp var maps the ops read.
        self.emit_walk(out, root_scope_id, &region_var, mounts_fragment);

        // The reactive ops (template_effect for reactive text, binds, events).
        self.emit_ops(out, root_scope_id);

        // The mount: `$.append($$anchor, <region_var>);`.
        out.push_str(&format!("\t$.append($$anchor, {region_var});\n"));

        if needs_push {
            out.push_str("\t$.pop();\n");
        }
        out.push_str("}\n");
    }

    /// Whether the component-ROOT region is TEXT-FIRST: its first cleaned DOM
    /// position is a TEXT / interpolation run (a leading static text or `{expr}`
    /// before the first element). This is the official `is_text_first` predicate
    /// (`clean_nodes` → `Fragment.js`) restricted to the component-root fragment — it
    /// drives the PRE-CLONE `$.next();` cursor advance. A leading PURE-whitespace node
    /// is already trimmed by `clean_nodes`, so it does not count (matching official's
    /// trim). Only the root fragment consults this; an in-element leading text is NOT
    /// text-first.
    fn root_region_is_text_first(&self, scope_id: TemplateScopeId) -> bool {
        let scope = self.ir().template_scope(scope_id);
        let ctx = CleanContext::region_root();
        let items = clean_nodes(self.ir(), &scope.roots, ctx);
        matches!(items.first(), Some(CleanItem::TextRun { .. }))
    }

    /// The variable name for a single-element clone-root region (named by the
    /// root element's tag, e.g. `button`).
    fn single_root_var_name(&mut self, scope_id: TemplateScopeId) -> String {
        let scope = self.ir().template_scope(scope_id);
        let ctx = CleanContext::region_root();
        let items = clean_nodes(self.ir(), &scope.roots, ctx);
        if let [CleanItem::Node(only)] = items.as_slice() {
            if let ClientNode::Element { element, .. } = self.client_node(*only) {
                // The DOM var stem comes from the TYPED element fact
                // (`SupportedHtmlElement::var_stem`), NEVER the raw tag — every
                // accepted element's stem is a valid, non-reserved JS identifier.
                return self.alloc_name(element.var_stem());
            }
        }
        self.alloc_name("root_node")
    }

    /// Emit the DOM walk for a region, populating the node/interp var maps.
    ///
    /// The walk is the official CHAINED form: a single-element clone-root descends
    /// into the clone var via `$.child`; a multi-root fragment reaches the first
    /// dynamic position via `$.first_child`, then each subsequent position via
    /// `$.sibling(prev, delta)` (delta = the cleaned-sequence index difference,
    /// omitted when 1).
    fn emit_walk(
        &mut self,
        out: &mut String,
        scope_id: TemplateScopeId,
        region_var: &str,
        mounts_fragment: bool,
    ) {
        let scope = self.ir().template_scope(scope_id);
        let ctx = CleanContext::region_root();
        let items = clean_nodes(self.ir(), &scope.roots, ctx);

        if mounts_fragment {
            // Multi-root fragment: the clone var IS the fragment.
            self.emit_walk_over_items(out, &items, WalkBase::Fragment(region_var), ctx);
        } else {
            // Single-element clone-root: the clone var IS the element; descend into
            // its children directly, and the element itself is reachable as
            // `region_var` (a dynamic op on it operates on the clone var directly).
            let [CleanItem::Node(only)] = items.as_slice() else {
                return;
            };
            let only = *only;
            self.node_var.insert(only, region_var.to_string());
            // The element TAG is the narrow emission decision; the children are the
            // IR geometry the whitespace cleaner partitions (the narrow children
            // mirror the IR ids 1:1).
            let ClientNode::Element { tag, .. } = self.client_node(only) else {
                return;
            };
            let child_ctx = ctx.for_children_of(tag);
            let IrNode::Element(el) = self.ir().node(only) else {
                return;
            };
            // The clone-root element's own input cleanup + `bind:this` are emitted
            // right after the clone frame named it (mirroring the chained path's
            // per-node setup + the official single-root order).
            if input_needs_remove_defaults(el) {
                out.push_str(&format!("\t$.remove_input_defaults({region_var});\n"));
            }
            self.emit_inline_bind_this(out, only);
            let child_items = clean_nodes(self.ir(), &el.children, child_ctx);
            self.emit_walk_over_items(out, &child_items, WalkBase::Element(region_var), child_ctx);
            // `$.reset(region_var)` after the clone-root element's children, when
            // any child was named (matches official's innermost-first reset order).
            if any_item_needs_name(self.ir(), &child_items) {
                out.push_str(&format!("\t$.reset({region_var});\n"));
            }
        }
    }

    /// Emit a chained walk over a cleaned DOM-position sequence, populating the
    /// node/interp var maps.
    fn emit_walk_over_items(
        &mut self,
        out: &mut String,
        items: &[CleanItem],
        base: WalkBase,
        ctx: CleanContext,
    ) {
        // The "previous named sibling" cursor — the last DOM position we named at
        // this level (its var name + cleaned index). The first descent comes from
        // `base`.
        let mut prev: Option<(usize, String)> = None;
        // The official `process_children` `skipped` counter (`shared/fragment.js`):
        // it counts consecutive STATIC positions (a static element, or an all-text
        // run) since the last NAMED position, resetting to 1 each time a dynamic
        // position is named. After the loop, a `skipped > 1` means there is a
        // TRAILING static run the hydration cursor must advance past — emitted as
        // `$.next(skipped - 1)` (the literal present when `skipped - 1 != 1`).
        let mut skipped = 0usize;
        // Whether ANY position at THIS level was named (a dynamic walk happened).
        // Mirrors the element-fragment `metadata.dynamic` gate: the official discards
        // the whole child walk (including a trailing `$.next()`) for a static-only
        // element fragment, but a multi-root FRAGMENT always walks. So the trailing
        // `$.next()` is emitted for a fragment base unconditionally, but for an
        // element base only when this level actually named a position.
        let mut named_any = false;
        for (idx, item) in items.iter().enumerate() {
            // Decide whether this position needs a name (it is dynamic, or hosts a
            // dynamic descendant we must reach). A non-named position is a STATIC
            // skip — count it for the trailing `$.next()` cursor advance.
            if !item_needs_name(self.ir(), item) {
                skipped += 1;
                continue;
            }
            // A NAMED (dynamic) position resets the static-skip run to 1 (the next
            // sibling descends via `$.sibling(id)`), matching official `flush_node`.
            skipped = 1;
            named_any = true;
            // Build the descent expression for this position. The official `is_text`
            // boolean trails the descent helper when the descended-to position is a
            // text DOM node that is a PURE single interpolation (`$.child(p, true)` /
            // `$.sibling(prev, N, true)`, the offset forced explicit even when 1) —
            // CSR-inert but required for hydration parity. A mixed run / element gets
            // no flag.
            let is_text = self.item_is_pure_interp_text(item);
            let var_name = self.descent_var_name(item);
            let var = self.alloc_name(&var_name);
            let descent = match &prev {
                None => first_descent(base, idx, is_text),
                Some((prev_idx, prev_name)) => {
                    let delta = idx - *prev_idx;
                    sibling_descent(prev_name, delta, is_text)
                }
            };
            out.push_str(&format!("\tvar {var} = {descent};\n"));
            prev = Some((idx, var.clone()));

            // Record the var for the interpolations / element it names.
            match item {
                CleanItem::TextRun { interps, .. } => {
                    // The text node is shared by all interps in the run.
                    for &interp in interps {
                        self.interp_var.insert(interp, var.clone());
                    }
                }
                CleanItem::Node(node) => {
                    self.node_var.insert(*node, var.clone());
                    // The element is a NARROW `ClientNode::Element` (the emission
                    // decision); its children are the IR geometry the cleaner
                    // partitions. A non-element narrow node has no children to walk.
                    if let ClientNode::Element { tag, .. } = self.client_node(*node) {
                        // An `<input>` bearing a value/checked/group bind and no
                        // static default needs `$.remove_input_defaults(input)`,
                        // emitted right after the input is named and BEFORE its
                        // `$.bind_value` (matching the official emission order). The
                        // bind/default facts are read from the IR element.
                        if let IrNode::Element(el) = self.ir().node(*node) {
                            if input_needs_remove_defaults(el) {
                                out.push_str(&format!("\t$.remove_input_defaults({var});\n"));
                            }
                            // `bind:this` is a RENDER-side binding emitted inline,
                            // right after the node is named (and after the input
                            // cleanup), BEFORE the next sibling — matching official.
                            self.emit_inline_bind_this(out, *node);
                            let child_ctx = ctx.for_children_of(tag);
                            let child_items = clean_nodes(self.ir(), &el.children, child_ctx);
                            self.emit_walk_over_items(
                                out,
                                &child_items,
                                WalkBase::Element(&var),
                                child_ctx,
                            );
                            if any_item_needs_name(self.ir(), &child_items) {
                                out.push_str(&format!("\t$.reset({var});\n"));
                            }
                        }
                    }
                }
            }
        }

        // The TRAILING static-run cursor advance (`$.next(skipped - 1)`): the
        // hydration cursor must skip past the static positions following the last
        // named one. The official `process_children` emits it when `skipped > 1`.
        // The base gate mirrors official's `metadata.dynamic` discard rule: a
        // multi-root FRAGMENT always walks (so a fully-static fragment like
        // `<p>a</p><p>b</p>` still advances), but an element's child walk — and its
        // trailing `$.next()` — is kept only when the child fragment is dynamic
        // (`named_any`). A single-element clone-root with all-static children emits
        // no `$.next()`.
        let emit_next = skipped > 1
            && match base {
                WalkBase::Fragment(_) => true,
                WalkBase::Element(_) => named_any,
            };
        if emit_next {
            // The literal is present when `skipped - 1 != 1` (official: `skipped !==
            // 1 && b.literal(skipped)` after the `skipped -= 1`).
            let advance = skipped - 1;
            if advance == 1 {
                out.push_str("\t$.next();\n");
            } else {
                out.push_str(&format!("\t$.next({advance});\n"));
            }
        }
    }

    /// The preferred stem for the var naming a cleaned DOM position (a tag name
    /// for an element, `text` for a text run). The element TAG is read from the
    /// NARROW node, never the broad IR.
    fn descent_var_name(&self, item: &CleanItem) -> String {
        match item {
            CleanItem::TextRun { .. } => "text".to_string(),
            CleanItem::Node(node) => match self.client_node(*node) {
                // The DOM var stem comes from the TYPED element fact
                // (`SupportedHtmlElement::var_stem`), NEVER the raw tag — every accepted
                // element's stem is a valid, non-reserved JS identifier.
                ClientNode::Element { element, .. } => element.var_stem().to_string(),
                _ => "node".to_string(),
            },
        }
    }

    /// Whether a cleaned DOM position is a TEXT node that is a PURE single
    /// interpolation (`<p>{count}</p>` → `$.child(p, true)`), the official
    /// `is_text` flag condition. A run is pure iff it is exactly ONE interpolation
    /// with NO literal text (the SAME predicate the `?? ''` text-effect decision
    /// uses); a mixed run (`x {count}` / `{a}{b}` / `{count}!`) is NOT pure. A
    /// non-text node is never `is_text`.
    fn item_is_pure_interp_text(&self, item: &CleanItem) -> bool {
        let CleanItem::TextRun { interps, .. } = item else {
            return false;
        };
        let [interp] = interps.as_slice() else {
            return false;
        };
        // SHAPE-only query: the owning run is pure iff it is exactly one
        // interpolation with no literal text.
        matches!(
            self.owning_text_run(*interp).as_slice(),
            [RunPart::Interp(_)]
        )
    }

    /// Emit the reactive ops for a region (the grouped `$.template_effect` of all
    /// reactive-text writes, then the binds + events in source order). Every op is
    /// the NARROW [`ClientRuntimeOp`] vocabulary — no broad `RuntimeOp` is matched.
    fn emit_ops(&mut self, out: &mut String, scope_id: TemplateScopeId) {
        let _ = scope_id;

        // (a) Group all reactive-text writes into ONE `$.template_effect`. Multiple
        // interpolations that share ONE DOM text node (a mixed run like
        // `{box.a} {box.b}`) collapse to a SINGLE `$.set_text` over the whole-run
        // template — dedup the per-interp ops by their shared text-node var (the
        // official `flush_sequence` one-text-node-per-run behavior).
        //
        // A reactive-text chunk that `has_call` is MEMOIZED through the official
        // deps-array form: the (already-rewritten) call expression is hoisted into a
        // `$N` placeholder and a `() => <expr>` dep, collected on ONE memoizer
        // SHARED across the whole effect (`$0, $1, …` assigned in order). A bare
        // read stays inline. When any chunk is memoized, the effect takes the
        // `($0, …) => <body>, [() => dep0, …]` shape.
        let mut memoizer = Memoizer::default();
        let mut text_writes = Vec::new();
        let mut seen_text_vars = rustc_hash::FxHashSet::default();
        for op in &self.plan.ops {
            if let ClientRuntimeOp::ReactiveText { target, .. } = op {
                let node = NodeId(target.0);
                let var = self
                    .interp_var
                    .get(&node)
                    .cloned()
                    .unwrap_or_else(|| "text".to_string());
                if !seen_text_vars.insert(var) {
                    // Another interpolation in the same text run already emitted the
                    // whole-run `$.set_text` — skip the duplicate.
                    continue;
                }
                let body = self.emit_set_text(node, &mut memoizer);
                text_writes.push(body);
            }
        }
        let deps = memoizer.into_deps();
        emit_text_effect(out, &text_writes, &deps);

        // (b) The POST-walk binds + events in source order — every op a NARROW
        // `ClientRuntimeOp` (already-rewritten getter / setter / handler bodies). A
        // `bind:this` is NOT emitted here: it is a render-side binding emitted INLINE
        // during the walk (see `emit_inline_bind_this`), BEFORE this grouped text
        // effect — matching the official op order. Only `bind:value` and delegated
        // events are post-walk.
        for op in &self.plan.ops {
            match op {
                ClientRuntimeOp::Bind {
                    bind_target: ClientBindTarget::This,
                    ..
                } => {
                    // Emitted inline in the walk; skip here.
                }
                ClientRuntimeOp::Bind {
                    target,
                    bind_target,
                    getter,
                    setter,
                    ..
                } => self.emit_bind(out, NodeId(target.0), *bind_target, getter, setter),
                ClientRuntimeOp::Event {
                    target,
                    event_type,
                    handler,
                    ..
                } => self.emit_event(out, NodeId(target.0), event_type, handler),
                // The reactive-text ops were grouped above.
                ClientRuntimeOp::ReactiveText { .. } => {}
            }
        }
    }

    /// Emit the `$.set_text(...)` call body for the reactive-text node `target`.
    ///
    /// The official `?? ''` rule is CONTENT-driven, not per-op: a text DOM node
    /// whose content is a PURE single interpolation emits `$.set_text(var, EXPR)`;
    /// a text node mixing static text with interpolation(s) emits the
    /// `` `..${EXPR ?? ''}..` `` template literal. The text node's content is
    /// recovered from the owning element's cleaned child run.
    fn emit_set_text(&self, target: NodeId, memoizer: &mut Memoizer) -> String {
        let var = self
            .interp_var
            .get(&target)
            .cloned()
            .unwrap_or_else(|| "text".to_string());
        // The owning text run drives the pure-vs-mixed shape (the interp the op
        // targets is found by node id within the run).
        let interp = self.interp_node_for_text(target);
        match self.text_node_template(interp, memoizer) {
            // A pure single interpolation → the (possibly memoized) value.
            TextNodeShape::PureInterp => {
                let value = self.memoized_interp(interp, memoizer);
                format!("$.set_text({var}, {value})")
            }
            // A mixed text run → a template literal with `?? ''` on each interp.
            TextNodeShape::Mixed(template) => {
                format!("$.set_text({var}, {template})")
            }
        }
    }

    /// The interpolation node whose text node is `target`. The reactive-text op's
    /// `target` IS the interpolation node id (see `SupportedClientIr::build_ops`),
    /// so this is the identity — kept as a named seam for the run partition.
    fn interp_node_for_text(&self, target: NodeId) -> NodeId {
        target
    }

    /// Route one interpolation through the MEMOIZER, consuming the PRE-REWRITTEN
    /// op text: a `has_call` chunk is hoisted into a `$N` placeholder (its rewritten
    /// expression becomes a `() => <expr>` dep on the shared memoizer); a bare read
    /// stays inline. The rewrite + has_call were computed at BUILD time (the
    /// fallible rewrite already ran), so this is a pure lookup.
    fn memoized_interp(&self, interp: NodeId, memoizer: &mut Memoizer) -> String {
        let (rewritten, has_call) = self.reactive_text_for(interp);
        memoizer.add(rewritten, has_call)
    }

    /// The pre-rewritten reactive-text body + `has_call` for the interpolation node,
    /// from the narrow plan ops (the op's `target` is the interp node id). A node
    /// with no op (unreachable for a reactive interpolation) yields its raw text.
    fn reactive_text_for(&self, interp: NodeId) -> (String, bool) {
        for op in &self.plan.ops {
            if let ClientRuntimeOp::ReactiveText {
                target,
                rewritten,
                has_call,
                ..
            } = op
            {
                if NodeId(target.0) == interp {
                    return (rewritten.clone(), *has_call);
                }
            }
        }
        // Fallback (unreachable on the accept path): the interpolation's raw source.
        if let IrNode::Interpolation { expr, .. } = self.ir().node(interp) {
            let analyzed = self.ir().analysis.expressions.get(*expr);
            return (analyzed.source.to_string(), false);
        }
        (String::new(), false)
    }

    /// Determine the text-node template shape for an interpolation's DOM text node.
    ///
    /// Recovers the owning text RUN (the maximal text/interpolation sibling run the
    /// interpolation belongs to) and decides whether the run is a PURE single
    /// interpolation or a MIXED literal/interpolation run, building the mixed
    /// template literal (with `?? ''` per interp) from the run parts. A `has_call`
    /// interp in a mixed run is MEMOIZED into a `$N` placeholder substituted into
    /// the template literal (`${$N ?? ''}`), matching the official memoizer.
    fn text_node_template(&self, interp: NodeId, memoizer: &mut Memoizer) -> TextNodeShape {
        let run = self.owning_text_run(interp);
        // A run that is exactly one interpolation with no literal text → pure.
        if let [RunPart::Interp(_)] = run.as_slice() {
            return TextNodeShape::PureInterp;
        }
        // Mixed: build the `` `lit${expr ?? ''}lit` `` template literal.
        let mut tmpl = String::from("`");
        for part in &run {
            match part {
                RunPart::Literal(text) => tmpl.push_str(&escape_template_literal(text)),
                RunPart::Interp(interp_node) => {
                    let value = self.memoized_interp(*interp_node, memoizer);
                    tmpl.push_str(&format!("${{{value} ?? ''}}"));
                }
            }
        }
        tmpl.push('`');
        TextNodeShape::Mixed(tmpl)
    }

    /// Recover the maximal text/interpolation run (in source order) that the given
    /// interpolation belongs to, as an ordered list of literal-text + interpolation
    /// NODE parts. The run is found among the interpolation's PARENT element's
    /// children (or the root scope's roots when at the top level).
    ///
    /// The whitespace + drop rules are the SHARED `clean_nodes` authority
    /// ([`cleaned_text_run_parts`]) — the SAME cleaner the skeleton/DOM walk key on,
    /// so the run reconstruction cannot disagree with the skeleton. A dropped node
    /// (comment / non-rendering / non-body special) never breaks the run; a REAL
    /// element does. Interior whitespace WITHIN a text node is preserved verbatim;
    /// the run's OUTER boundary (the leading whitespace of the first text and the
    /// trailing whitespace of the last text in the cleaned sequence) is stripped; a
    /// space ADJACENT to an interpolation is preserved (so `Hello {name}!` keeps the
    /// space after `Hello`); the boundary whitespace between two texts made adjacent
    /// by a dropped comment is collapsed by the cleaner's neighbor-aware rule.
    fn owning_text_run(&self, interp: NodeId) -> Vec<RunPart> {
        let (siblings, ctx) = self.owning_siblings_and_ctx(interp);
        // Reconstruct the run through the SHARED `clean_nodes` whitespace + drop-set
        // authority. Each literal is the cleaned text of ONE source text node (not
        // entity-decoded by the cleaner — the skeleton needs raw HTML); `set_text`
        // writes `textContent`, so each literal is decoded HERE, per text node, BEFORE
        // the template builder concatenates them (a `&amp` reference split across a
        // dropped comment decodes the two text nodes independently, never merging into
        // one `&amp;`). (Pre-fix this reconstructed from RAW children with a SECOND
        // whitespace path that treated a `<!--x-->` comment as a run break, truncating
        // the run after the interpolation.)
        if let Some(parts) = cleaned_text_run_parts(self.ir(), &siblings, ctx, interp) {
            return parts
                .into_iter()
                .map(|p| match p {
                    RunTextPart::Literal(text) => RunPart::Literal(decode_text_entities(&text)),
                    RunTextPart::Interp(node) => RunPart::Interp(node),
                })
                .collect();
        }
        // Fallback: the interpolation alone.
        if matches!(self.ir().node(interp), IrNode::Interpolation { .. }) {
            return vec![RunPart::Interp(interp)];
        }
        Vec::new()
    }

    /// The sibling node sequence (the parent element's children, or a template
    /// region's roots) that directly contains `interp`, paired with the
    /// [`CleanContext`] those siblings are cleaned in — the ancestor-folded child
    /// context of the parent element, or [`CleanContext::region_root`] at the region
    /// level. The context drives the SHARED whitespace cleaner over the run.
    fn owning_siblings_and_ctx(&self, interp: NodeId) -> (Vec<NodeId>, CleanContext<'_>) {
        // Search every element's children + every template region's roots for the
        // sequence that lists `interp` directly.
        for (idx, node) in self.ir().nodes.iter().enumerate() {
            if let IrNode::Element(el) = node {
                if el.children.contains(&interp) {
                    let ctx = self.clean_ctx_for_children_of(NodeId(idx as u32));
                    return (el.children.clone(), ctx);
                }
            }
        }
        for scope in &self.ir().template_scopes {
            if scope.roots.contains(&interp) {
                return (scope.roots.clone(), CleanContext::region_root());
            }
        }
        (vec![interp], CleanContext::region_root())
    }

    /// The [`CleanContext`] for the CHILDREN of element `parent` — the region-root
    /// context folded through every ANCESTOR element's `for_children_of` (root →
    /// parent), so inherited namespace / `<pre>` whitespace-preservation / SVG-`<text>`
    /// significance are threaded exactly as the skeleton walk threads them. The IR is
    /// a flat node list, so the parent chain is recovered by scanning for the element
    /// whose `children` list each node (the runtime IR is small).
    fn clean_ctx_for_children_of(&self, parent: NodeId) -> CleanContext<'_> {
        // Walk up to collect the ancestor ELEMENT tags (innermost-first), starting at
        // `parent` itself. Only intrinsic elements thread `for_children_of` (component
        // / special / block ancestors do not change the namespace/pre context here).
        let mut tags_inner_first: Vec<&str> = Vec::new();
        let mut current = Some(parent);
        while let Some(node_id) = current {
            if let IrNode::Element(el) = self.ir().node(node_id) {
                tags_inner_first.push(el.tag.as_str());
            }
            current = self.dom_parent_of(node_id);
        }
        // Fold root → parent so each element's `for_children_of` is applied in order.
        let mut ctx = CleanContext::region_root();
        for tag in tags_inner_first.iter().rev() {
            ctx = ctx.for_children_of(tag);
        }
        ctx
    }

    /// The DOM parent of `node` — the element / component / special whose `children`
    /// list contains it. `None` at a region root. (The flat IR carries no parent
    /// pointers; the runtime IR is small enough to scan.)
    fn dom_parent_of(&self, node: NodeId) -> Option<NodeId> {
        for (idx, candidate) in self.ir().nodes.iter().enumerate() {
            let children = match candidate {
                IrNode::Element(el) => &el.children,
                IrNode::Component(c) => &c.children,
                IrNode::Special(s) => &s.children,
                _ => continue,
            };
            if children.contains(&node) {
                return Some(NodeId(idx as u32));
            }
        }
        None
    }

    /// Emit any `bind:this` op targeting `node` INLINE during the walk, right after
    /// the node has been named (and after `$.remove_input_defaults`). The official
    /// compiler emits `$.bind_this(node, …)` as a RENDER-side binding interleaved
    /// into element setup — BEFORE the next sibling walk and BEFORE the grouped
    /// `$.template_effect` for sibling reactive text — whereas `$.bind_value` /
    /// delegated events are emitted post-walk (after the text effect). Emitting
    /// `bind:this` here, and SKIPPING the `This` arm in [`Self::emit_ops`], matches
    /// that order byte-for-byte.
    fn emit_inline_bind_this(&mut self, out: &mut String, node: NodeId) {
        // The plan ops are cloned out so the `&self` borrow does not conflict with
        // the `&mut self` `emit_bind` call (the op set is small — one pass).
        let binds: Vec<(String, String)> = self
            .plan
            .ops
            .iter()
            .filter_map(|op| match op {
                ClientRuntimeOp::Bind {
                    target,
                    bind_target: ClientBindTarget::This,
                    getter,
                    setter,
                    ..
                } if NodeId(target.0) == node => Some((getter.clone(), setter.clone())),
                _ => None,
            })
            .collect();
        for (getter, setter) in binds {
            self.emit_bind(out, node, ClientBindTarget::This, &getter, &setter);
        }
    }

    /// Emit a `bind:value` / `bind:this` op from its already-rewritten getter +
    /// setter bodies (the narrow plan op).
    fn emit_bind(
        &mut self,
        out: &mut String,
        target: NodeId,
        bind_target: ClientBindTarget,
        getter: &str,
        setter: &str,
    ) {
        let var = self
            .node_var
            .get(&target)
            .cloned()
            .unwrap_or_else(|| "node".to_string());
        match bind_target {
            ClientBindTarget::Value => {
                // `$.bind_value(input, () => GET, ($$value) => SET)`.
                out.push_str(&format!(
                    "\t$.bind_value({var}, () => {getter}, ($$value) => {setter});\n"
                ));
            }
            ClientBindTarget::This => {
                out.push_str(&format!(
                    "\t$.bind_this({var}, ($$value) => {setter}, () => {getter});\n"
                ));
            }
        }
    }

    /// Emit a delegated event op from its already-rewritten handler body (the narrow
    /// plan op).
    fn emit_event(&mut self, out: &mut String, target: NodeId, event_type: &str, handler: &str) {
        let var = self
            .node_var
            .get(&target)
            .cloned()
            .unwrap_or_else(|| "node".to_string());
        out.push_str(&format!(
            "\t$.delegated('{event_type}', {var}, {handler});\n"
        ));
    }
}

/// The text-node template shape for an interpolation.
enum TextNodeShape {
    /// A pure single interpolation — `$.set_text(var, EXPR)`.
    PureInterp,
    /// A mixed literal/interpolation run — `$.set_text(var, `..${EXPR ?? ''}..`)`.
    Mixed(String),
}

/// One part of a text run: a literal text chunk or an interpolation NODE.
enum RunPart {
    /// A literal text chunk (whitespace-collapsed).
    Literal(String),
    /// An interpolation NODE (the reactive-text op's target node id).
    Interp(NodeId),
}

/// The official `Memoizer` for a `$.template_effect` group — it hoists each
/// `has_call` reactive-text chunk into a `$N` placeholder and a `() => <expr>`
/// dependency, SHARED across the whole effect so the placeholders are numbered
/// `$0, $1, …` in collection order. A non-call chunk is returned inline (no
/// memoization). Mirrors `phases/3-transform/client/visitors/shared/utils.js`'s
/// `Memoizer` (the synchronous-deps half — async/`has_await` text is fail-closed
/// at 5j and never reaches here).
#[derive(Default)]
struct Memoizer {
    /// The collected `() => <expr>` dependency bodies, in placeholder order.
    deps: Vec<String>,
}

impl Memoizer {
    /// Route a rewritten chunk through the memoizer: a `has_call` chunk is hoisted
    /// (its rewritten expression becomes the next `() => <expr>` dep and a `$N`
    /// placeholder is returned); a non-call chunk is returned inline unchanged.
    fn add(&mut self, rewritten: String, has_call: bool) -> String {
        if !has_call {
            return rewritten;
        }
        let placeholder = format!("${}", self.deps.len());
        self.deps.push(rewritten);
        placeholder
    }

    /// The collected dependency bodies (`[expr0, expr1, …]`), consuming the
    /// memoizer.
    fn into_deps(self) -> Vec<String> {
        self.deps
    }
}

/// Emit the grouped reactive-text `$.template_effect`, choosing the official shape:
///
/// - NO writes → nothing.
/// - No memoized deps, one write → the inline `$.template_effect(() => <write>)`.
/// - No memoized deps, many writes → the block `$.template_effect(() => { … })`.
/// - Any memoized deps → the deps-array form `$.template_effect(($0, …) => <body>,
///   [() => dep0, …])` (the parameter list is `$0 … $N-1`; the body is the single
///   write or a block of writes; the deps array is the second argument).
fn emit_text_effect(out: &mut String, text_writes: &[String], deps: &[String]) {
    if text_writes.is_empty() {
        return;
    }
    if deps.is_empty() {
        // The non-memoized shapes (unchanged from the §1.2 / bare-read path).
        if text_writes.len() == 1 {
            out.push_str(&format!("\t$.template_effect(() => {});\n", text_writes[0]));
        } else {
            out.push_str("\t$.template_effect(() => {\n");
            for body in text_writes {
                out.push_str(&format!("\t\t{body};\n"));
            }
            out.push_str("\t});\n");
        }
        return;
    }
    // The MEMOIZED deps-array form. The arrow params are `$0 … $N-1`.
    let params = (0..deps.len())
        .map(|i| format!("${i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let deps_array = deps
        .iter()
        .map(|d| format!("() => {d}"))
        .collect::<Vec<_>>()
        .join(", ");
    if text_writes.len() == 1 {
        out.push_str(&format!(
            "\t$.template_effect(({params}) => {}, [{deps_array}]);\n",
            text_writes[0]
        ));
    } else {
        out.push_str(&format!("\t$.template_effect(({params}) => {{\n"));
        for body in text_writes {
            out.push_str(&format!("\t\t{body};\n"));
        }
        out.push_str(&format!("\t}}, [{deps_array}]);\n"));
    }
}

/// The base a walk descent starts from.
#[derive(Debug, Clone, Copy)]
enum WalkBase<'n> {
    /// The cloned multi-root fragment (descend via `$.first_child`).
    Fragment(&'n str),
    /// A named element (descend into its children via `$.child`).
    Element(&'n str),
}

/// The first descent expression from a base to cleaned-sequence position `idx`.
///
/// When the descended-to position is a pure-interp TEXT node (`is_text`), the
/// official trailing `true` boolean is emitted on the helper that LANDS on the
/// text node — `$.child(node, true)` / `$.first_child(frag, true)`.
///
/// The `$.sibling` step that advances PAST the first child applies the same
/// offset-omission rule [`sibling_descent`] does: `$.sibling(node, 1)` collapses to
/// `$.sibling(node)` (the `count` default is `1`) — UNLESS `is_text`, which forces
/// the explicit offset so the trailing `true` boolean stays positioned (the
/// oracle's `$.sibling($.child(div), 1, true)` form). A higher offset stays
/// explicit.
fn first_descent(base: WalkBase, idx: usize, is_text: bool) -> String {
    let text_arg = if is_text { ", true" } else { "" };
    match base {
        WalkBase::Fragment(name) => {
            if idx == 0 {
                format!("$.first_child({name}{text_arg})")
            } else {
                // `$.sibling($.first_child(fragment)[, idx][, true])` — descend then
                // advance; the offset is omitted at `idx == 1` (non-text), explicit
                // otherwise (or when the text flag must trail it).
                let inner = format!("$.first_child({name})");
                sibling_descent(&inner, idx, is_text)
            }
        }
        WalkBase::Element(name) => {
            if idx == 0 {
                format!("$.child({name}{text_arg})")
            } else {
                let inner = format!("$.child({name})");
                sibling_descent(&inner, idx, is_text)
            }
        }
    }
}

/// A `$.sibling(prev[, delta][, true])` descent (delta omitted when 1 UNLESS the
/// landed-on node is a pure-interp text — official forces the explicit offset when
/// `is_text`, e.g. `$.sibling(prev, 1, true)`).
fn sibling_descent(prev: &str, delta: usize, is_text: bool) -> String {
    if is_text {
        // `is_text` forces the explicit offset, then the trailing `true`.
        format!("$.sibling({prev}, {delta}, true)")
    } else if delta == 1 {
        format!("$.sibling({prev})")
    } else {
        format!("$.sibling({prev}, {delta})")
    }
}

/// Whether a cleaned DOM position needs a named walk var (it is dynamic, or hosts
/// a dynamic descendant).
fn item_needs_name(ir: &SvelteRuntimeIr, item: &CleanItem) -> bool {
    match item {
        CleanItem::TextRun { interps, .. } => !interps.is_empty(),
        CleanItem::Node(node) => node_or_descendant_dynamic(ir, *node),
    }
}

/// Whether any cleaned position in the sequence needs a named walk var (so a
/// `$.reset(parent)` is emitted after the parent's children).
fn any_item_needs_name(ir: &SvelteRuntimeIr, items: &[CleanItem]) -> bool {
    items.iter().any(|item| item_needs_name(ir, item))
}

/// Whether a node is dynamic or hosts a dynamic descendant.
fn node_or_descendant_dynamic(ir: &SvelteRuntimeIr, node_id: NodeId) -> bool {
    match ir.node(node_id) {
        IrNode::Interpolation { .. } => true,
        IrNode::Element(el) => {
            el.attrs.iter().any(|a| !matches!(a, AttrIr::Static { .. }))
                || el
                    .children
                    .iter()
                    .any(|&c| node_or_descendant_dynamic(ir, c))
        }
        _ => false,
    }
}

/// Whether an `<input>` element needs `$.remove_input_defaults` — the official
/// `RegularElement.js` rule: an `<input>` with a `value` / `checked` / `group`
/// binding (or `files`) and NO static `defaultValue` / `defaultChecked`
/// attribute. (The non-spread `bind:value` branch is handled; the rule keys on
/// the typed `AttrIr`, never a source scan.)
fn input_needs_remove_defaults(el: &super::ir::ElementIr) -> bool {
    if el.tag != "input" {
        return false;
    }
    let has_value_bind = el.attrs.iter().any(|a| {
        matches!(a, AttrIr::Bind { target, .. }
            if matches!(target.as_str(), "value" | "checked" | "group" | "files"))
    });
    if !has_value_bind {
        return false;
    }
    // A static `defaultValue` / `defaultChecked` attribute suppresses the helper
    // (the default is set explicitly).
    let has_static_default = el.attrs.iter().any(|a| {
        matches!(a, AttrIr::Static { name, .. }
            if matches!(name.as_str(), "defaultValue" | "defaultChecked"))
    });
    !has_static_default
}

/// Emit the module imports from the import plan, interleaving the `<script module>`
/// user imports in the official slot.
///
/// The official import order is: the `disclose-version` side-effect import (the
/// leading byte), the flag side-effect imports, then the `import * as $ from
/// 'svelte/internal/client'` runtime namespace. (A `<script module>` / instance-script
/// USER import is fail-closed upstream — the script-hoisting deferral — so the
/// supported surface emits no user imports.)
fn emit_imports(out: &mut String, imports: &ImportPlan) {
    if imports.disclose_version {
        out.push_str("import 'svelte/internal/disclose-version';\n");
    }
    if imports.legacy_flag {
        out.push_str("import 'svelte/internal/flags/legacy';\n");
    }
    if imports.async_flag {
        out.push_str("import 'svelte/internal/flags/async';\n");
    }
    if imports.tracing_flag {
        out.push_str("import 'svelte/internal/flags/tracing';\n");
    }
    let ns = match imports.runtime {
        RuntimeImport::Client => "svelte/internal/client",
        RuntimeImport::Server => "svelte/internal/server",
    };
    out.push_str(&format!("import * as $ from '{ns}';\n"));
}

/// Emit the root template-factory hoist (`var root = $.from_html(...)`), returning
/// whether the region mounts a multi-root FRAGMENT (vs a single clone-root element).
fn emit_root_hoist(out: &mut String, root_var: &str, factory: Option<&TemplateFactory>) -> bool {
    match factory {
        Some(TemplateFactory::FromHtml {
            html,
            fragment_flag,
        }) => {
            let escaped = escape_template_literal(html);
            match fragment_flag {
                Some(flag) => {
                    out.push_str(&format!(
                        "var {root_var} = $.from_html(`{escaped}`, {});\n",
                        flag.literal()
                    ));
                    true
                }
                None => {
                    out.push_str(&format!("var {root_var} = $.from_html(`{escaped}`);\n"));
                    false
                }
            }
        }
        Some(TemplateFactory::TextNode { seed }) => {
            match seed {
                Some(text) => {
                    let escaped = escape_template_literal(text);
                    out.push_str(&format!("var {root_var} = $.text(`{escaped}`);\n"));
                }
                None => out.push_str(&format!("var {root_var} = $.text();\n")),
            }
            false
        }
        Some(TemplateFactory::CommentAnchor { .. }) => {
            out.push_str(&format!("var {root_var} = $.comment();\n"));
            false
        }
        // A standalone root has no factory; components/snippets are refused
        // upstream, so this is unreachable for a supported component. Emit a
        // comment anchor as a
        // defensive fallback (never silently nothing).
        Some(TemplateFactory::Standalone { .. }) | None => {
            out.push_str(&format!("var {root_var} = $.comment();\n"));
            false
        }
    }
}

/// Emit the `$.delegate([...])` module epilogue from the first-seen-ordered
/// delegated event-type set.
fn emit_delegate_epilogue(out: &mut String, events: &[String]) {
    let list = events
        .iter()
        .map(|e| format!("'{e}'"))
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!("$.delegate([{list}]);\n"));
}

/// Escape a string for embedding inside a backtick template literal (the
/// `$.from_html` / `$.text` argument): backslash, backtick, and `${`.
fn escape_template_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push_str("\\\\"),
            '`' => out.push_str("\\`"),
            '$' if chars.peek() == Some(&'{') => {
                out.push_str("\\$");
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
