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

use super::client_effect::{emit_text_effect, EffectBody, Memoizer};
use super::client_event::{emit_delegate_epilogue, render_event_registration};
use super::client_plan::{
    AttrValue, ClientDynAttrEmit, ClientModulePlan, ClientNode, ClientRuntimeOp, EventEmit,
};
use super::client_shapes::{BindGetSetForm, ClientBindShape, GroupBindKey};
use super::client_walk::{
    any_item_needs_name, first_descent, item_needs_name, sibling_descent, WalkBase,
};
use super::entity_decode::decode_text_entities;
use super::helpers::{ImportPlan, RuntimeImport};
use super::html::{StaticTemplatePlan, TemplateFactory};
use super::ir::{IrNode, NodeId, SvelteRuntimeIr, TemplateScopeId};
use super::topology::ClientTopologyPlan;
use super::whitespace::{
    clean_nodes, cleaned_text_run_parts, CleanContext, CleanItem, RunTextPart,
};

pub use super::unsupported::UnsupportedSvelteRuntimeSurface;

/// The component-FUNCTION-scoped `bind:group` accumulator name (`const binding_group
/// = []`). It is declared at the TOP of the component function body (NOT module
/// scope — module scope would share binding-group selection state across every
/// component instance, a correctness bug) and passed as the first argument to every
/// `$.bind_group(binding_group, [], el, get, set)` call. Matches the pinned
/// `svelte@5.56.3` emit (oracle CASE `group`).
///
/// `pub(super)` (the minimum widening): the `bind:group` emit body
/// ([`ClientEmitter::format_dom_bind`]) lives in the sibling `client_bind` module,
/// and the component-prelude `const binding_group = []` declaration is emitted here
/// — both reference this single canonical accumulator name.
pub(super) const GROUP_BINDING_NAME: &str = "binding_group";

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
pub(super) struct ClientEmitter<'a> {
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
    /// The allocated class/style accumulator name per `(node, kind)` — the
    /// `let <name>;` is emitted INLINE in the walk right after the node's var
    /// (matching the official per-element `init` placement), and the same name is
    /// read by the post-walk `$.template_effect` for the `prev` arg + the `<name> =`
    /// assignment. The key carries the [`AccKind`] so a single element bearing BOTH a
    /// reactive class op AND a reactive style op keeps each op's accumulator
    /// independent (a directive-less class op adjacent to a directive-bearing style op
    /// must NOT borrow the style accumulator). Populated by the walk's inline-init
    /// emission, read by the reactive class/style op.
    acc_name: rustc_hash::FxHashMap<(NodeId, AccKind), String>,
    /// The collision-safe `bind:group` accumulator name PER DISTINCT GROUP, keyed by the
    /// structural bind target + scope ([`GroupBindKey`]). Populated ONCE in [`Self::new`]
    /// (one [`Self::alloc_name`] per distinct key, in source order — `binding_group`,
    /// `binding_group_1`, …, bumped past a user `binding_group`); empty with no `bind:group`.
    /// Every `$.bind_group(<name>, …)` call reads its accumulator back through its op's key,
    /// so two INDEPENDENT groups reference DISTINCT accumulators (never a single
    /// component-wide name) while two inputs sharing a target share one.
    pub(super) group_binding_names: rustc_hash::FxHashMap<GroupBindKey, String>,
    /// The DISTINCT `bind:group` accumulator names in SOURCE ORDER (the order each group's
    /// key was first seen). The component body declares one `const <name> = [];` per entry,
    /// in this order — matching official svelte's insertion-order accumulator decl loop.
    pub(super) group_binding_decls: Vec<String>,
    /// Inline `bind:this` ops pre-indexed by target node (built ONCE in [`Self::new`]) so
    /// [`Self::emit_inline_bind_this`] is an O(1) drain, not an O(ops) per-node re-scan.
    /// Each entry carries the bind's [`BindGetSetForm`] (identifier-thunk vs function-pair)
    /// alongside its rewritten getter/setter bodies.
    pub(super) inline_this_binds:
        rustc_hash::FxHashMap<NodeId, Vec<(BindGetSetForm, String, String)>>,
}

/// Which coalesced reactive op an accumulator belongs to. A node has at most one
/// class op and one style op, so `(node, Class)` / `(node, Style)` are distinct
/// accumulator slots — the discriminant the per-node accumulator map keys on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum AccKind {
    Class,
    Style,
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
        // ALSO reserve every FREE template-expression reference (reads AND writes), so a
        // generated DOM-var stem never collides with a free identifier the template emits.
        // `<p {...p}>` would otherwise emit `var p` (the `<p>` element local) shadowing the
        // `...p` spread payload — official renames the DOM local to `p_1`. The official
        // `scope.generate` reserves referenced free identifiers generally; seeding them here
        // makes `alloc_name` rename the synthesized stem (→ `p_1`) across ALL surfaces.
        for analyzed in plan.build.ir.analysis.expressions.all() {
            for reference in &analyzed.references {
                used.insert(reference.name.clone());
            }
        }
        for reserved in ["$", "$$anchor", "$$props", "$$value"] {
            used.insert(reserved.to_string());
        }
        let mut emitter = Self {
            plan,
            used,
            node_var: rustc_hash::FxHashMap::default(),
            interp_var: rustc_hash::FxHashMap::default(),
            acc_name: rustc_hash::FxHashMap::default(),
            group_binding_names: rustc_hash::FxHashMap::default(),
            group_binding_decls: Vec::new(),
            inline_this_binds: super::client_bind::build_inline_this_index(plan),
        };
        // Allocate ONE collision-safe `bind:group` accumulator per DISTINCT group (keyed by
        // the structural bind target + scope), in source order, through the seeded DOM-var
        // allocator — so a user `binding_group` pushes the accumulators to `binding_group_1`,
        // … (matching official `scope.generate`), and two independent groups get distinct
        // names. (Lives in `client_bind` with the rest of the bind emission machinery.)
        emitter.plan_group_accumulators();
        emitter
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
    pub(super) fn ir(&self) -> &'a SvelteRuntimeIr<'a> {
        self.plan.build.ir
    }

    /// The narrow plan (the SOLE emission input) — read by the sibling spread/`{@html}`
    /// emission helpers for the op set. Returns the `'a`-lifetime reference (copied out of
    /// the plan field, like [`Self::ir`]) so reading the op set does NOT borrow `self` — the
    /// per-group accumulator pass walks `plan.ops` while still mutating `self`'s name maps.
    pub(super) fn plan(&self) -> &'a ClientModulePlan<'a> {
        self.plan
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
    /// counter). `pub(super)` so the sibling bind-emission module can allocate the
    /// per-group `bind:group` accumulator names through the same seeded allocator.
    pub(super) fn alloc_name(&mut self, stem: &str) -> String {
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
        // A lone `{@html}` root is `$.comment()`-anchored IN THE BODY (no module hoist),
        // so it reserves no `root` factory var.
        let comment_anchor_root = matches!(
            root_factory,
            Some(TemplateFactory::CommentAnchor {
                reason: super::html::AnchorReason::RawHtmlRoot,
            })
        );
        let root_var = if comment_anchor_root {
            String::new()
        } else {
            self.alloc_name("root")
        };
        let mounts_fragment = if comment_anchor_root {
            // No `var root = …` module hoist; the `$.comment()` is created in the body.
            true
        } else {
            emit_root_hoist(&mut out, &root_var, root_factory)
        };
        out.push('\n');

        // (3) The component body.
        self.emit_body(&mut out, &root_var, mounts_fragment, comment_anchor_root);

        // (4) The `$.delegate([...])` epilogue.
        if !topology.delegated_events.is_empty() {
            out.push('\n');
            emit_delegate_epilogue(&mut out, topology.delegated_events.ordered());
        }

        ClientModule { code: out }
    }

    /// Emit the component function body. `comment_anchor_root` marks the lone-`{@html}`
    /// root whose fragment is `$.comment()` created in the body (no `root()` clone frame).
    fn emit_body(
        &mut self,
        out: &mut String,
        root_var: &str,
        mounts_fragment: bool,
        comment_anchor_root: bool,
    ) {
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

        // The component-FUNCTION-scoped `bind:group` accumulators, declared at the TOP of
        // the body (before the state decls) — ONE `const <name> = [];` per DISTINCT group, in
        // source order (the pinned svelte@5.56.3 shape — oracle CASE `group`: two independent
        // groups emit `const binding_group = []` AND `const binding_group_1 = []`).
        // Component-function scope (NOT module scope) is load-bearing: module scope would
        // share binding-group selection state across every component instance, a correctness
        // bug. The accumulator set is populated from the PRESENCE of group binds (one per
        // distinct target+scope), NOT `group_values`: a `bind:group` with no static `value`
        // attr has empty `group_values` yet still emits the `$.bind_group(<name>, …)` call,
        // so each accepted group bind's key is what mints (and references) its accumulator.
        for group_name in &self.group_binding_decls {
            out.push_str(&format!("\tconst {group_name} = [];\n"));
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
        if mounts_fragment && !comment_anchor_root && self.root_region_is_text_first(root_scope_id)
        {
            out.push_str("\t$.next();\n");
        }
        let region_var = if mounts_fragment {
            self.alloc_name("fragment")
        } else {
            // The single clone-root element's own var (named by its tag).
            self.single_root_var_name(root_scope_id)
        };
        // A lone-`{@html}` root creates its fragment via `$.comment()` (no `root()` clone
        // frame); every other root clones the module-hoisted `root` factory.
        if comment_anchor_root {
            out.push_str(&format!("\tvar {region_var} = $.comment();\n"));
        } else {
            out.push_str(&format!("\tvar {region_var} = {root_var}();\n"));
        }

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
            // The clone-root element's own bind PRELUDE cleanup, INIT-domain attribute
            // ops, and `bind:this` are emitted right after the clone frame named it.
            // The official per-element order (`RegularElement.js`) is: the bind prelude
            // (`remove_input_defaults` for an input value/checked/group bind,
            // `remove_textarea_child` for a `<textarea bind:value>`) → the init-domain
            // writes (`$.autofocus` / `$.set_class` / `$.set_attribute` / the reactive
            // accumulator decls) → `$.bind_this` LAST (a render-side binding emitted
            // after the element's own inits).
            self.emit_bind_prelude(out, only, region_var);
            self.emit_node_inline_inits(out, only);
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
            // A `{@html}` that is the SOLE controlled child of its parent gets NO `<!>`
            // anchor and NO walk descent: its `$.html(parent, …, true)` was emitted at the
            // parent's init position. It still counts as a named position (so the parent's
            // `$.reset` emits), but it allocates no var and advances no cursor.
            if let CleanItem::Node(node) = item {
                if matches!(self.client_node(*node), ClientNode::RawHtml { .. })
                    && self.html_op_is_only_child(*node)
                {
                    continue;
                }
            }
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
                    // A `{@html}` with siblings reaches its OWN `<!>` anchor var (just
                    // descended to); emit `$.html(node, payload)` here (NO trailing `true`
                    // — that is the only-child form). The parent's `$.reset` is emitted by
                    // the parent's child walk.
                    if let ClientNode::RawHtml { .. } = self.client_node(*node) {
                        if let Some(payload) = self.html_op_payload(*node) {
                            out.push_str(&format!("\t$.html({var}, {payload});\n"));
                        }
                    }
                    // The element is a NARROW `ClientNode::Element` (the emission
                    // decision); its children are the IR geometry the cleaner
                    // partitions. A non-element narrow node has no children to walk.
                    if let ClientNode::Element { tag, .. } = self.client_node(*node) {
                        // An element bearing a bind whose routing carries a prelude
                        // (`<input>` value/checked/group → `$.remove_input_defaults`;
                        // `<textarea bind:value>` → `$.remove_textarea_child`) emits it
                        // right after the element is named and BEFORE its `$.bind_*`
                        // (matching the official emission order). The bind/default facts
                        // are read DATA-DRIVEN from the IR element + the shared routing.
                        if let IrNode::Element(el) = self.ir().node(*node) {
                            self.emit_bind_prelude(out, *node, &var);
                            // The element's INIT-domain attribute ops (non-reactive
                            // attr/property writes, `$.autofocus`, non-reactive
                            // class/style, and the reactive class/style `let <acc>;`
                            // decl) emit at THIS walk position — the official
                            // per-element `init` placement — after the input cleanup
                            // and BEFORE the next sibling.
                            self.emit_node_inline_inits(out, *node);
                            // `bind:this` is a RENDER-side binding emitted inline,
                            // right after the node's own inits (matching the official
                            // per-element order where `bind_this` follows the element's
                            // init-domain writes), BEFORE the next sibling.
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

    /// Emit the POST-WALK reactive ops for a region: the single combined
    /// `$.template_effect` (the reactive text plus the reactive dynamic attr / class /
    /// style, in source order), then the binds + events. The NON-REACTIVE attribute
    /// inits (autofocus, non-reactive attr/property/class/style) and the reactive
    /// class/style `let <acc>;` accumulator declarations are emitted INLINE during the
    /// walk ([`Self::emit_node_inline_inits`]) at each element's `init` position —
    /// matching official — so this stage does NOT emit them. Every op is the NARROW
    /// [`ClientRuntimeOp`] vocabulary — no broad `RuntimeOp` is matched.
    fn emit_ops(&mut self, out: &mut String, scope_id: TemplateScopeId) {
        let _ = scope_id;

        // The single combined `$.template_effect` — ALL reactive updates in source
        // order: reactive text (one `$.set_text` per text-node run, the official
        // `flush_sequence` dedup), then the reactive dynamic attr / class / style
        // writes. A reactive-text chunk that `has_call` is MEMOIZED through the shared
        // deps-array form (`$0, $1, …`); a bare read stays inline. A reactive
        // class/style op reads the accumulator name allocated for its node during the
        // walk (`self.acc_name[node]`), so the `prev` arg + `<acc> =` assignment match
        // the inline-declared `let <acc>;`.
        let mut memoizer = Memoizer::default();
        let mut update_bodies: Vec<EffectBody> = Vec::new();
        let mut seen_text_vars = rustc_hash::FxHashSet::default();
        for op in &self.plan.ops {
            match op {
                ClientRuntimeOp::ReactiveText { target, .. } => {
                    let node = NodeId(target.0);
                    let var = self
                        .interp_var
                        .get(&node)
                        .cloned()
                        .unwrap_or_else(|| "text".to_string());
                    if !seen_text_vars.insert(var) {
                        continue;
                    }
                    let body = self.emit_set_text(node, &mut memoizer);
                    update_bodies.push(EffectBody::Expr(body));
                }
                // A `bind:group` input with a REACTIVE dynamic/mixed `value={…}` folds its
                // guarded change-detection write into THIS combined effect, in source order
                // (the input precedes a sibling reactive text), BEFORE the post-walk
                // `$.bind_group`. It is a STATEMENT body (the `if (…) { … }` guard), so it
                // forces the block form. The value routes through the SHARED memoizer (a
                // `has_call` value → a `$N` deps slot, reused in the guard + write).
                ClientRuntimeOp::Bind {
                    shape:
                        ClientBindShape::DomBind {
                            group_key: Some(_), ..
                        },
                    target,
                    ..
                } => {
                    if let Some(body) =
                        self.emit_group_dynamic_value_effect(NodeId(target.0), &mut memoizer)
                    {
                        update_bodies.push(EffectBody::Stmt(body));
                    }
                }
                ClientRuntimeOp::ReactiveAttr {
                    emit,
                    reactive: true,
                    target,
                } => {
                    update_bodies.push(EffectBody::Expr(self.emit_reactive_attr(
                        NodeId(target.0),
                        emit,
                        &mut Some(&mut memoizer),
                    )));
                }
                ClientRuntimeOp::SetClass {
                    target,
                    value,
                    css_hash,
                    directives,
                    directives_has_call,
                    reactive: true,
                    ..
                } => {
                    let node = NodeId(target.0);
                    let acc = self.acc_name.get(&(node, AccKind::Class)).cloned();
                    update_bodies.push(EffectBody::Expr(self.emit_set_class(
                        node,
                        value,
                        css_hash.as_deref(),
                        directives.as_deref(),
                        *directives_has_call,
                        acc.as_deref(),
                        &mut Some(&mut memoizer),
                    )));
                }
                ClientRuntimeOp::SetStyle {
                    target,
                    value,
                    directives,
                    directives_has_call,
                    reactive: true,
                    ..
                } => {
                    let node = NodeId(target.0);
                    let acc = self.acc_name.get(&(node, AccKind::Style)).cloned();
                    update_bodies.push(EffectBody::Expr(self.emit_set_style(
                        node,
                        value,
                        directives.as_deref(),
                        *directives_has_call,
                        acc.as_deref(),
                        &mut Some(&mut memoizer),
                    )));
                }
                _ => {}
            }
        }
        let deps = memoizer.into_deps();
        emit_text_effect(out, &update_bodies, &deps);

        // (d) The POST-walk binds + events in source order — every op a NARROW
        // `ClientRuntimeOp` (already-rewritten getter / setter / handler bodies). A
        // `bind:this` is NOT emitted here: it is a render-side binding emitted INLINE
        // during the walk (see `emit_inline_bind_this`), BEFORE this grouped text
        // effect — matching the official op order. Only `bind:value` and delegated
        // events are post-walk.
        for op in &self.plan.ops {
            match op {
                ClientRuntimeOp::Bind {
                    shape: ClientBindShape::This { .. },
                    ..
                } => {
                    // `bind:this` is a render-side binding emitted INLINE in the walk
                    // (see `emit_inline_bind_this`); skip here.
                }
                ClientRuntimeOp::Bind {
                    target,
                    shape,
                    getter,
                    setter,
                    ..
                } => self.emit_bind(out, NodeId(target.0), shape, getter, setter),
                ClientRuntimeOp::Event { emit, .. } => self.emit_event(out, emit),
                // The reactive-text / reactive-attr / class / style ops were grouped
                // above; the non-reactive attr inits were emitted in (b). The
                // `$.attribute_effect` spread fold and the `$.html` raw-markup op are
                // INLINE init-domain ops (emitted during the walk at the element's init
                // position / the `{@html}` anchor descent), so they are not emitted here.
                ClientRuntimeOp::ReactiveText { .. }
                | ClientRuntimeOp::ReactiveAttr { .. }
                | ClientRuntimeOp::SetClass { .. }
                | ClientRuntimeOp::SetStyle { .. }
                | ClientRuntimeOp::AttributeEffect { .. }
                | ClientRuntimeOp::Html { .. } => {}
            }
        }
    }

    /// Emit a dynamic plain-attribute write body (`$.set_attribute(node, 'name',
    /// value)` / `node.<prop> = value` / `$.autofocus(node, value)`), resolving the
    /// node var and building the structured value.
    ///
    /// `memoizer` is `Some` on the REACTIVE (in-effect) path: a `has_call` expression
    /// part is hoisted into a `$N` deps-array slot (the official `build_template_chunk`
    /// rule). It is `None` on the INIT path (`$.autofocus` / a non-reactive write),
    /// where the value is read once and is emitted INLINE with no memoization.
    fn emit_reactive_attr(
        &self,
        target: NodeId,
        emit: &ClientDynAttrEmit,
        memoizer: &mut Option<&mut Memoizer>,
    ) -> String {
        let var = self.dom_var(target);
        match emit {
            ClientDynAttrEmit::SetAttribute { name, value } => {
                let v = self.build_attr_value(value, memoizer);
                format!("$.set_attribute({var}, '{name}', {v})")
            }
            ClientDynAttrEmit::Property { prop, value } => {
                let v = self.build_attr_value(value, memoizer);
                format!("{var}.{prop} = {v}")
            }
            ClientDynAttrEmit::Autofocus { value } => {
                // Autofocus is init-only — its value is a pre-flattened string (never
                // memoized).
                format!("$.autofocus({var}, {value})")
            }
        }
    }

    /// Memoize a class/style ARGUMENT (the base `value` or the `next` directives
    /// object/array) when it `has_call` and the op is reactive (`memoizer` is `Some`) —
    /// the official `build_set_class` / `build_set_style` rule. On the init path
    /// (`memoizer` is `None`) the argument is emitted inline. A non-`has_call` argument
    /// always stays inline.
    fn memoize_arg(
        &self,
        arg: &str,
        has_call: bool,
        memoizer: &mut Option<&mut Memoizer>,
    ) -> String {
        match memoizer {
            Some(m) => m.add(arg.to_string(), has_call),
            None => arg.to_string(),
        }
    }

    /// Assemble the coalesced `$.set_class(node, 1, value, css_hash, prev, next)` call
    /// body from the structured op pieces, with the real DOM var + accumulator name.
    /// `prev` is the accumulator name (reactive directives), `{}` (non-reactive
    /// directives), or absent (no directives); a reactive directive call prefixes the
    /// `<acc> = ` assignment. The base `value` is routed through `build_attr_value` (so
    /// a mixed base memoizes each EXPRESSION PART, a `$.clsx(...)` base memoizes the
    /// whole wrap — the official `build_set_class`); the directives object is memoized
    /// as a whole through `memoizer` when it `has_call`.
    #[allow(clippy::too_many_arguments)]
    fn emit_set_class(
        &self,
        target: NodeId,
        value: &AttrValue,
        css_hash: Option<&str>,
        directives: Option<&str>,
        directives_has_call: bool,
        acc: Option<&str>,
        memoizer: &mut Option<&mut Memoizer>,
    ) -> String {
        let var = self.dom_var(target);
        let value = self.build_attr_value(value, memoizer);
        // `prev`: the accumulator name (reactive) or `{}` (non-reactive); present only
        // when there are directives (the same condition that produced `css_hash`).
        let prev = directives.map(|_| acc.map(str::to_string).unwrap_or_else(|| "{}".to_string()));
        let next = directives.map(|d| self.memoize_arg(d, directives_has_call, memoizer));
        let args = super::client_codegen_helpers::trim_trailing_none(vec![
            Some(var),
            Some("1".to_string()),
            Some(value),
            css_hash.map(str::to_string),
            prev,
            next,
        ]);
        let call = format!("$.set_class({})", args.join(", "));
        match acc {
            Some(name) => format!("{name} = {call}"),
            None => call,
        }
    }

    /// Assemble the coalesced `$.set_style(node, value, prev, next)` call body from the
    /// structured op pieces (see [`Self::emit_set_class`]).
    #[allow(clippy::too_many_arguments)]
    fn emit_set_style(
        &self,
        target: NodeId,
        value: &AttrValue,
        directives: Option<&str>,
        directives_has_call: bool,
        acc: Option<&str>,
        memoizer: &mut Option<&mut Memoizer>,
    ) -> String {
        let var = self.dom_var(target);
        let value = self.build_attr_value(value, memoizer);
        let prev = directives.map(|_| acc.map(str::to_string).unwrap_or_else(|| "{}".to_string()));
        let next = directives.map(|d| self.memoize_arg(d, directives_has_call, memoizer));
        let args = super::client_codegen_helpers::trim_trailing_none(vec![
            Some(var),
            Some(value),
            prev,
            next,
        ]);
        let call = format!("$.set_style({})", args.join(", "));
        match acc {
            Some(name) => format!("{name} = {call}"),
            None => call,
        }
    }

    /// The emitted DOM-variable name reaching a node (from the walk's `node_var` map),
    /// or the `node` fallback (unreachable on the accept path).
    pub(super) fn dom_var(&self, target: NodeId) -> String {
        self.node_var
            .get(&target)
            .cloned()
            .unwrap_or_else(|| "node".to_string())
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

    /// Emit a node's INIT-domain attribute ops INLINE during the walk, right after
    /// the node has been named (and after `$.remove_input_defaults`), BEFORE its
    /// `bind:this`, in source order. This is the official per-element `init` placement
    /// (`RegularElement.js`): the non-reactive dynamic-attr / property writes, the
    /// init-only `$.autofocus`, and the non-reactive `$.set_class` / `$.set_style`
    /// are emitted at the element's WALK position — NOT collected post-walk. A
    /// REACTIVE class/style op contributes ONLY its `let <acc>;` accumulator
    /// declaration here (the reactive `$.set_class` / `$.set_style` body itself joins
    /// the post-walk `$.template_effect`); the allocated name is recorded in
    /// [`Self::acc_name`] for that effect to read. A reactive `ReactiveAttr`
    /// (a stateful property write like `video.muted = $.get(v)`) contributes nothing
    /// here — it joins the effect.
    ///
    /// Iterating `self.plan.ops` (source order) and filtering to `node` keeps a
    /// single element's multiple init ops in source order, and across elements the
    /// walk visits nodes in DOM order, so the accumulator names allocate
    /// `classes`, `classes_1`, … in the official order.
    fn emit_node_inline_inits(&mut self, out: &mut String, node: NodeId) {
        // The op set is cloned out (small) so the `&self` read does not conflict with
        // the `&mut self` accumulator allocation. Each entry is the already-assembled
        // init statement, OR the directive pieces a reactive class/style op needs so
        // its `let <acc>;` decl + name allocation happen here in walk order.
        enum InlineInit {
            /// A ready-to-emit init statement (`$.set_attribute(...)`, `node.p = v`,
            /// `$.autofocus(...)`, a non-reactive `$.set_class` / `$.set_style`).
            Stmt(String),
            /// A reactive class/style op needs a `let <acc>;` accumulator declared at
            /// this walk position; the stem is allocated to a collision-free name and
            /// recorded under the op's [`AccKind`] slot.
            Accumulator(&'static str, AccKind),
        }
        let inits: Vec<InlineInit> = self
            .plan
            .ops
            .iter()
            .filter_map(|op| match op {
                // A non-reactive plain-attribute / property / autofocus init. The
                // value is read once (no effect), so it is emitted INLINE with NO
                // memoizer — even a `has_call` constant value stays inline.
                ClientRuntimeOp::ReactiveAttr {
                    emit,
                    reactive: false,
                    target,
                } if NodeId(target.0) == node => Some(InlineInit::Stmt(
                    self.emit_reactive_attr(node, emit, &mut None),
                )),
                // A reactive class op declares its `classes` accumulator here.
                ClientRuntimeOp::SetClass {
                    accumulator_stem: Some(stem),
                    reactive: true,
                    target,
                    ..
                } if NodeId(target.0) == node => {
                    Some(InlineInit::Accumulator(stem, AccKind::Class))
                }
                // A reactive style op declares its `styles` accumulator here.
                ClientRuntimeOp::SetStyle {
                    accumulator_stem: Some(stem),
                    reactive: true,
                    target,
                    ..
                } if NodeId(target.0) == node => {
                    Some(InlineInit::Accumulator(stem, AccKind::Style))
                }
                // A NON-reactive class/style init statement — emitted inline (no
                // effect), so its arguments are never memoized (`memoizer` is `None`).
                ClientRuntimeOp::SetClass {
                    target,
                    value,
                    css_hash,
                    directives,
                    directives_has_call,
                    reactive: false,
                    ..
                } if NodeId(target.0) == node => Some(InlineInit::Stmt(self.emit_set_class(
                    node,
                    value,
                    css_hash.as_deref(),
                    directives.as_deref(),
                    *directives_has_call,
                    None,
                    &mut None,
                ))),
                ClientRuntimeOp::SetStyle {
                    target,
                    value,
                    directives,
                    directives_has_call,
                    reactive: false,
                    ..
                } if NodeId(target.0) == node => Some(InlineInit::Stmt(self.emit_set_style(
                    node,
                    value,
                    directives.as_deref(),
                    *directives_has_call,
                    None,
                    &mut None,
                ))),
                // The `$.attribute_effect` spread fold for a spread element — emitted at
                // the element's init position (the official `Element.js` spread emission
                // order: after the input cleanup, before the children / reset).
                ClientRuntimeOp::AttributeEffect {
                    target,
                    fold_body,
                    input_trailing,
                } if NodeId(target.0) == node => Some(InlineInit::Stmt(
                    self.emit_attribute_effect(node, fold_body, *input_trailing),
                )),
                // A `{@html}` that is the SOLE controlled child of THIS element — emitted
                // at the element's init position, operating on the element var with the
                // trailing `true` (the `$.reset(element)` follows via the child walk).
                ClientRuntimeOp::Html {
                    target,
                    payload,
                    only_child: true,
                } if self.html_only_child_parent(NodeId(target.0)) == Some(node) => {
                    Some(InlineInit::Stmt(self.emit_html_only_child(node, payload)))
                }
                _ => None,
            })
            .collect();
        for init in inits {
            match init {
                InlineInit::Stmt(stmt) => out.push_str(&format!("\t{stmt};\n")),
                InlineInit::Accumulator(stem, kind) => {
                    let name = self.alloc_name(stem);
                    out.push_str(&format!("\tlet {name};\n"));
                    self.acc_name.insert((node, kind), name);
                }
            }
        }
    }

    /// Emit a DOM event registration from its [`EventEmit`] substrate — the official
    /// `$.event` (direct) / `$.delegated` (delegated) shape:
    /// `$.<helper>('<type>', <target>, <wrapped-handler>[, <capture>][, <passive>])`.
    ///
    /// - The handler is nested inner→outer in its modifier wrappers (`$.<modifier>(…)`).
    /// - The 4th positional `capture` arg is `true` when capture is enabled; when a 5th
    ///   `passive` arg is present without capture, the capture slot is the `void 0`
    ///   placeholder (mirroring the official `b.call` falsy-arg trimming).
    /// - The 5th positional `passive` boolean is emitted only when `passive` is set.
    ///
    /// The target host resolves the global hosts (`$.window` / `$.document` /
    /// `$.document.body`) for the reusable special-element event substrate; the
    /// regular-element surface feeds only the regular-`Node` host.
    fn emit_event(&mut self, out: &mut String, emit: &EventEmit) {
        out.push_str(&render_event_registration(emit, &self.node_var));
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
///
/// The fragment decision is the `TEMPLATE_FRAGMENT` bit ONLY — a flag carrying just
/// `TEMPLATE_USE_IMPORT_NODE` (a lone `<video>`/custom-element template, flag `2`)
/// is still a SINGLE clone-root element: `$.from_html` returns the element, so the
/// walk must take the single-element path (`var video = root();`), NOT the fragment
/// path (`$.first_child(root())` → null on a single element).
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
                    flag.is_fragment()
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
