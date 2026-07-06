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

use super::client_codegen_helpers::js_single_quoted;
use super::client_effect::Memoizer;
use super::client_event::emit_delegate_epilogue;
use super::client_module_frame::{emit_imports, escape_template_literal};
use super::client_plan::{ClientBlock, ClientModulePlan, ClientNode, ClientRuntimeOp};
use super::client_shapes::GroupBindKey;
use super::client_walk::{
    any_item_needs_name, first_descent, item_needs_name, sibling_descent, WalkBase,
};
use super::entity_decode::decode_text_entities;
use super::html::StaticTemplatePlan;
use super::ir::{IrNode, NodeId, SvelteRuntimeIr, TemplateScopeId};
use super::topology::ClientTopologyPlan;
use super::whitespace::{
    clean_nodes, clean_nodes_indexed, cleaned_text_run_parts, CleanContext, CleanItem, RunTextPart,
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

/// The official function-pair component-bind getter local STEM — the
/// `scope.generate('bind_get')` name svelte mints for a `bind:x={get, set}`. Allocated
/// through the shared scope-aware allocator (keyed by the pair index), so a user binding of
/// the same name pushes the generated one to `bind_get_1`, … and two function-pair binds
/// never alias the same `var`.
pub(super) const FN_PAIR_BIND_GET_NAME: &str = "bind_get";

/// The official function-pair component-bind setter local STEM — the
/// `scope.generate('bind_set')` name (sibling to [`FN_PAIR_BIND_GET_NAME`]). Each stem is
/// reserved INDEPENDENTLY, so a user `bind_get` renames only the getter (`bind_get_1`) while
/// the free `bind_set` keeps its stem — matching official `scope.generate`.
pub(super) const FN_PAIR_BIND_SET_NAME: &str = "bind_set";

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
    pub(super) plan: &'a ClientModulePlan<'a>,
    /// Reserved + already-allocated names (collision avoidance).
    used: rustc_hash::FxHashSet<String>,
    /// The emitted var name reaching each named DOM node (populated by the walk,
    /// read by the op emission).
    pub(super) node_var: rustc_hash::FxHashMap<NodeId, String>,
    /// The emitted var name reaching each interpolation's text node (populated by
    /// the walk, read by the reactive-text op emission).
    pub(super) interp_var: rustc_hash::FxHashMap<NodeId, String>,
    /// The allocated class/style accumulator name per `(node, kind)` — the
    /// `let <name>;` is emitted INLINE in the walk right after the node's var
    /// (matching the official per-element `init` placement), and the same name is
    /// read by the post-walk `$.template_effect` for the `prev` arg + the `<name> =`
    /// assignment. The key carries the [`AccKind`] so a single element bearing BOTH a
    /// reactive class op AND a reactive style op keeps each op's accumulator
    /// independent (a directive-less class op adjacent to a directive-bearing style op
    /// must NOT borrow the style accumulator). Populated by the walk's inline-init
    /// emission, read by the reactive class/style op.
    pub(super) acc_name: rustc_hash::FxHashMap<(NodeId, AccKind), String>,
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
    /// INIT-DOMAIN inline render ops (inline `bind:this` + `Action` / `Attachment`
    /// lifecycle + the effect-wrapped LEGACY `on:` events of `use:` action hosts)
    /// pre-indexed by target node in plan-op (attribute source) order
    /// (built ONCE in [`Self::new`]) so [`Self::emit_inline_render_ops`] is an O(1)
    /// drain, not an O(ops) per-node re-scan — ONE ordered sequence per node, so a
    /// `bind:this` and an adjacent `use:` / `{@attach}` / wrapped event keep their
    /// official source interleave.
    pub(super) inline_render_ops:
        rustc_hash::FxHashMap<NodeId, Vec<super::client_lifecycle::InlineRenderOp>>,
    /// The nodes hosting a `use:` action — the trigger set for the official
    /// effect wrap of co-located LEGACY `on:` events. Computed ONCE in
    /// [`Self::new`] via [`super::client_lifecycle::action_host_nodes`] and
    /// shared by the inline index build AND both post-walk event stages through
    /// the single [`super::client_lifecycle::event_emission_slot`] classifier,
    /// so the sides can never disagree on where an event emits.
    pub(super) action_hosts: rustc_hash::FxHashSet<NodeId>,
    /// The EULER-TOUR rank pair of every narrow template node — the AFTER-UPDATE
    /// stream emission order. A MODERN `on*` event registration joins the stream
    /// at its element's ENTER position (`pre` — official pushes it onto the
    /// enclosing after_update at attribute-visit time), while the element's own
    /// directive-batch items (`$.transition` / `$.animation` / bare legacy
    /// `$.event` / bare non-`this` `$.bind_*`) join at its EXIT position (`post`
    /// — the `…child_state.after_update, …element_state.after_update` merge), so
    /// a child element's batch precedes its parent's while items WITHIN one
    /// element keep attribute source order. Computed ONCE in [`Self::new`] over
    /// the narrow node arena ([`super::client_lifecycle::after_update_ranks`]);
    /// ranks are compared only within a template scope.
    pub(super) after_update_rank:
        rustc_hash::FxHashMap<NodeId, super::client_lifecycle::AfterUpdateRank>,
    /// The function-pair component-bind locals (`bind_get` / `bind_set`) per
    /// component-function-scoped pair INDEX, minted ONCE in [`Self::new`] through the shared
    /// scope-aware allocator (so they avoid every user binding AND each other). The component
    /// emitter reads the `(get, set)` names back by index for BOTH the hoisted `var`
    /// declarations and the prop getter/setter bodies (one resolved name pair, never a
    /// re-minted one). Empty when there is no function-pair component bind.
    pub(super) fn_pair_bind_names: rustc_hash::FxHashMap<usize, (String, String)>,
    /// The per-region clone frame, keyed by [`TemplateScopeId`] — a module-hoisted
    /// `$.from_html` factory + walk base ([`RegionFrame::FromHtml`]), an in-closure
    /// `$.text(...)` text-first body ([`RegionFrame::TextNode`]), or an in-body
    /// `$.comment()` anchor ([`RegionFrame::CommentAnchor`]). Built ONCE in [`Self::emit`]
    /// by a POST-ORDER region traversal (a block body's template is hoisted BEFORE its
    /// parent's, matching the official depth-first hoist order), so every region's frame is
    /// looked up here, never re-synthesized.
    pub(super) region_frame: rustc_hash::FxHashMap<TemplateScopeId, RegionFrame>,
}

/// One template-scope region's clone frame — HOW the region's body materializes its root
/// DOM. Only a [`RegionFrame::FromHtml`] region module-hoists a clone factory and
/// factory-calls it; a text-first region emits an IN-CLOSURE `$.text(...)` (no hoist, no
/// `root()` call); a comment-anchor region creates its `$.comment()` frame in the body.
#[derive(Debug, Clone)]
pub(super) enum RegionFrame {
    /// A module-hoisted `$.from_html(...)` clone factory: the body clones it via
    /// `var <region> = <hoist_var>();`. `mounts_fragment` → the clone is a multi-root
    /// FRAGMENT (walk descends via `$.first_child`); else it IS the single clone-root
    /// element (walk via `$.child`).
    FromHtml {
        /// The module-hoisted factory var (`root` / `root_1` / …).
        hoist_var: String,
        /// Whether the clone var is the fragment (vs the single root element).
        mounts_fragment: bool,
    },
    /// A TEXT-FIRST region: the whole cleaned body is a SINGLE text run (a lone static
    /// text, or one-or-more accepted interpolations, with no element/block sibling). It
    /// is emitted INLINE in the closure (`var text = $.text(<seed>)`) — NO module hoist
    /// and NO clone-factory call, matching official `svelte@5.56.3`. The official text
    /// NODE is created in-body and `$.append`ed, never a hoisted `$.text(...)` called as
    /// a clone factory.
    TextNode {
        /// The seed text: `Some` for a PURE static-text run (official `$.text('hello')`),
        /// `None` for a run with any interpolation (official `$.text()`, the reactive
        /// `$.set_text` fills it).
        seed: Option<String>,
        /// Whether the OWNING block kind prepends a `$.next()` hydration-cursor advance
        /// before the `$.text(...)`. This is owner-kind metadata (an `{#each}` body /
        /// else-fallback emits it; an `{#if}` / `{#key}` / `{#await}` text-first body does
        /// NOT) — decided by the owning block, never inferred from the `TextNode` shape.
        prelude_next: bool,
    },
    /// A `$.comment()` anchor created IN the body — a block-only / empty / lone-`{@html}`
    /// region. The walk treats the comment as a fragment (`$.first_child`).
    CommentAnchor,
    /// A STANDALONE component / static-`{@render}` root (`is_standalone`): NO clone frame,
    /// NO `$.append` — the region emits the component call / static render DIRECTLY against
    /// the region anchor (`Child($$anchor, …)` / `pair($$anchor, …)`). The node is the
    /// region's sole standalone root.
    Standalone {
        /// The standalone root node (a component or a resolved-snippet render).
        node: NodeId,
    },
}

/// Which coalesced reactive op an accumulator belongs to. A node has at most one
/// class op and one style op, so `(node, Class)` / `(node, Style)` are distinct
/// accumulator slots — the discriminant the per-node accumulator map keys on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum AccKind {
    Class,
    Style,
}

impl<'a> ClientEmitter<'a> {
    fn new(plan: &'a ClientModulePlan<'a>) -> Self {
        // Reserve every user binding name + the runtime-magic identifiers so a generated
        // stem never collides with ANY user/template binding that can share an emitted JS
        // scope (matching the official `scope.generate`, which reserves ALL source
        // bindings). The GENERATED stems (`root`, `fragment`, tag names, `text`,
        // `binding_group`, `bind_get`/`bind_set`) are NOT pre-reserved — they are the names
        // we want to allocate; a user binding of the same name pushes the generated one to a
        // `_N` suffix via `alloc_name`. The reservation UNION (top-level script bindings +
        // the full binding table + free template-expression references + the runtime-magic
        // reserved literals) is the shared `seed_reserved_names` — the SAME set the
        // plan-time `rest_excludes` allocation seeds from, so the two allocators agree.
        let mut used = super::client_plan::seed_reserved_names(&plan.build);
        // The plan-time-allocated `rest_excludes` Set name is reserved so a later DOM-var
        // stem never re-picks it (the official `scope.root.unique('rest_excludes')` name is
        // reserved in the root scope every child scope generates against).
        if let Some(rest) = &plan.build.rest_props {
            used.insert(rest.set_name.clone());
        }
        let action_hosts = super::client_lifecycle::action_host_nodes(plan);
        let mut emitter = Self {
            plan,
            used,
            node_var: rustc_hash::FxHashMap::default(),
            interp_var: rustc_hash::FxHashMap::default(),
            acc_name: rustc_hash::FxHashMap::default(),
            group_binding_names: rustc_hash::FxHashMap::default(),
            group_binding_decls: Vec::new(),
            inline_render_ops: super::client_lifecycle::build_inline_render_index(
                plan,
                &action_hosts,
            ),
            action_hosts,
            after_update_rank: super::client_lifecycle::after_update_ranks(plan),
            region_frame: rustc_hash::FxHashMap::default(),
            fn_pair_bind_names: rustc_hash::FxHashMap::default(),
        };
        // Allocate ONE collision-safe `bind:group` accumulator per DISTINCT group (keyed by
        // the structural bind target + scope), in source order, through the seeded DOM-var
        // allocator — so a user `binding_group` pushes the accumulators to `binding_group_1`,
        // … (matching official `scope.generate`), and two independent groups get distinct
        // names. (Lives in `client_bind` with the rest of the bind emission machinery.)
        emitter.plan_group_accumulators();
        // Mint the function-pair component-bind locals (`bind_get` / `bind_set`) through the
        // SAME seeded allocator, keyed by each pair's component-function-scoped index — so a
        // user `bind_get` pushes the generated getter to `bind_get_1`, and two function-pair
        // binds get distinct `var`s (official `scope.generate` uniquing).
        emitter.plan_fn_pair_bind_names();
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
    pub(super) fn client_node(&self, id: NodeId) -> &'a ClientNode {
        &self.plan.nodes[id.0 as usize]
    }

    /// Allocate a deterministic variable name from a preferred stem, appending a
    /// `_N` suffix on collision (mirroring the official allocator's stem +
    /// counter). `pub(super)` so the sibling bind-emission module can allocate the
    /// per-group `bind:group` accumulator names through the same seeded allocator.
    pub(super) fn alloc_name(&mut self, stem: &str) -> String {
        super::client_plan::alloc_unique_name(&mut self.used, stem)
    }

    /// Emit the full client module.
    fn emit(
        &mut self,
        html_plan: &StaticTemplatePlan,
        topology: &ClientTopologyPlan,
    ) -> ClientModule {
        let mut out = String::new();

        // (1) Module imports — disclose-version + flags, the `<script module>` USER
        // imports (source order), the runtime namespace, then the INSTANCE-script USER
        // imports (source order) — the official two-slot prelude order. Every static
        // import form rides the typed `UserImport` carriers.
        emit_imports(&mut out, &topology.imports, &self.plan.user_imports);
        out.push('\n');

        // (2) Module hoists — the per-region `$.from_html(...)` template factories, allocated
        // + emitted in POST-ORDER (a block body's template is hoisted BEFORE its parent's,
        // matching the official depth-first hoist order). Each region's clone frame lands in
        // `self.region_frame`; a text-first region records NO hoist (its `$.text(...)` is
        // emitted in the body), and a comment-anchor / empty body region records no hoist (its
        // `$.comment()` frame is created in the body). The `html_plan` is no longer consulted
        // here — the emitter synthesizes each region's factory through the same
        // `synthesize_region` the plan uses.
        //
        // The hoists go to a SEPARATE buffer so the MODULE-scope snippet consts (which are
        // emitted AFTER them in source order — `const pair = …; var root = $.from_html(…)`)
        // can be written first while still reading the now-populated region frames.
        let _ = html_plan;
        let mut hoists = String::new();
        self.plan_region_factories(&mut hoists);
        // (2a) Module-scope `{#snippet}` consts — between the imports and the `$.from_html`
        // hoists (the official `module_level_snippets` slot). Emitted now that the region
        // frames are planned (the snippet body region clones a hoisted `root`).
        for snippet in self.plan.module_snippets.clone() {
            self.emit_snippet_decl(&mut out, snippet);
            out.push('\n');
        }
        // (2b) The `$props()` rest / whole-object `rest_excludes` Set — module scope,
        // after the imports / snippets, IMMEDIATELY before the `$.from_html` factories
        // (the official `state.hoisted` slot). `var <name> = new Set([<quoted keys>]);`
        // where the keys are the fixed prefix then each non-rest source key in source
        // order (a whole-object capture carries the prefix only). The name was allocated
        // ONCE at plan build; the body declarator references the same one.
        if let Some(rest) = &self.plan.build.rest_props {
            let keys = rest
                .excludes
                .iter()
                .map(|k| js_single_quoted(k))
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!("var {} = new Set([{}]);\n", rest.set_name, keys));
        }
        out.push_str(&hoists);
        out.push('\n');

        // (3) The component body (the root region plus every nested block body, recursive).
        self.emit_body(&mut out);

        // (4) The `$.delegate([...])` epilogue.
        if !topology.delegated_events.is_empty() {
            out.push('\n');
            emit_delegate_epilogue(&mut out, topology.delegated_events.ordered());
        }

        // (5) The custom-element epilogue — `customElements.define(tag,
        // $.create_custom_element(…))` for a tagged descriptor, the bare
        // `$.create_custom_element(…)` statement otherwise. Directly after the
        // delegate epilogue (the official slot; a blank line separates it from
        // the component function when there is no delegate line).
        if let Some(ce) = &self.plan.custom_element {
            if topology.delegated_events.is_empty() {
                out.push('\n');
            }
            super::client_custom_element::emit_custom_element_epilogue(
                &mut out,
                &self.plan.component.name,
                ce,
            );
        }

        ClientModule { code: out }
    }

    /// Emit the component function shell + the ROOT region (which recursively emits every
    /// nested block body through [`Self::emit_region`]).
    fn emit_body(&mut self, out: &mut String) {
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

        // The hoisted `$props.id()` declaration — the ABSOLUTE body-top slot,
        // ABOVE the `$.push` frame line (the official hoist order; the plan owns
        // the decision, the emitter only reads it).
        if let Some(decl) = &self.plan.props_id_hoist {
            out.push('\t');
            out.push_str(decl);
            out.push('\n');
        }

        // The push FLAG is the reactivity MODE: `true` for a runes component,
        // `false` for a legacy (non-runes) one — the official `5.56.3` shape
        // (`$.push($$props, true)` runes / `$.push($$props, false)` legacy).
        // Derived from the component mode, NEVER from store presence.
        let legacy_mode = self.plan.component.mode == super::ir::SvelteMode::Legacy;
        if needs_push {
            let flag = if legacy_mode { "false" } else { "true" };
            out.push_str(&format!("\t$.push($$props, {flag});\n"));
        }

        // The `$store` auto-subscription setup — driven SOLELY by
        // `has_store_subscriptions`, never by the frame: one accessor thunk per
        // subscribed store in first-seen order, then the ONE shared
        // `$.setup_stores()` registry destructure (the accessor thunks are lazy,
        // so the forward reference to `$$stores` is sound). Emitted directly
        // after the `$.push` line when a frame exists, at the body top
        // otherwise (a clean local store has NO frame — oracle-verified).
        if self.plan.has_store_subscriptions {
            for sub in &self.plan.store_subscriptions {
                let name = &sub.name;
                let base = sub.base();
                out.push_str(&format!(
                    "\tconst {name} = () => $.store_get({base}, '{name}', $$stores);\n"
                ));
            }
            out.push_str("\tconst [$$stores, $$cleanup] = $.setup_stores();\n");
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

        // The INSTANCE-scope `{#snippet}` consts — emitted at the TOP of the body (before
        // the script statements), the official `instance_level_snippets` slot. A capturing
        // top-level snippet (`{#snippet label()}` reading component state) is local here.
        for snippet in self.plan.instance_snippets.clone() {
            out.push('\t');
            self.emit_snippet_decl(out, snippet);
            out.push('\n');
        }

        // The component-function BODY statements (already lowered by the plan).
        for item in &self.plan.body_statements {
            out.push('\t');
            out.push_str(item.code());
            out.push('\n');
        }

        // The custom-element `$$exports` accessor object — AFTER the script
        // statements, BEFORE the template body (the official
        // `component_returned_object` slot). Emitted only when prop accessors
        // exist (a no-props custom element omits `$$exports`; it may still
        // carry the `$.push`/`$.pop` context frame via `needs_context`).
        if !self.plan.ce_exports.is_empty() {
            super::client_custom_element::emit_exports_object(out, &self.plan.ce_exports);
        }

        // The LEGACY-frame instance-init hook: a LEGACY component that opened
        // the context frame emits `$.init();` after every instance statement,
        // before the template body (oracle-verified: a runes framing component
        // and a frame-less legacy component both emit NO `$.init()`).
        if legacy_mode && needs_push {
            out.push_str("\t$.init();\n");
        }

        // The ROOT region: its clone frame, walk (interleaving nested block calls),
        // reactive ops, and mount into `$$anchor` — recursively emitting every nested
        // block body region. (`emit_region` lives in `client_block_emit`.)
        self.emit_region(out, self.ir().root, "$$anchor");

        // The context close + the `$store` subscription FINALIZER (`$$cleanup();`),
        // ordered per the official emission (all four combinations oracle-verified
        // against svelte@5.56.3):
        //
        // - frame, no `$$exports`, store   → `$.pop();` then `$$cleanup();`
        // - frame, `$$exports`,  no store  → `return $.pop($$exports);`
        // - frame, `$$exports`,  store     → the PRE-RETURN finalizer slot
        //   `var $$pop = $.pop($$exports); $$cleanup(); return $$pop;` — the
        //   returned object is captured into `$$pop`, the store `$$cleanup()`
        //   runs, THEN the captured value returns; a bare `return
        //   $.pop($$exports);` would strand `$$cleanup()` after the return.
        // - no frame, store                → `$$cleanup();` at the body end.
        if needs_push {
            if self.plan.ce_exports.is_empty() {
                out.push_str("\t$.pop();\n");
                if self.plan.has_store_subscriptions {
                    out.push_str("\t$$cleanup();\n");
                }
            } else if self.plan.has_store_subscriptions {
                out.push_str("\tvar $$pop = $.pop($$exports);\n");
                out.push_str("\t$$cleanup();\n");
                out.push_str("\treturn $$pop;\n");
            } else {
                out.push_str("\treturn $.pop($$exports);\n");
            }
        } else if self.plan.has_store_subscriptions {
            out.push_str("\t$$cleanup();\n");
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
    pub(super) fn region_is_text_first(&self, scope_id: TemplateScopeId) -> bool {
        let scope = self.ir().template_scope(scope_id);
        let ctx = CleanContext::region_root();
        let items = clean_nodes(self.ir(), &scope.roots, ctx);
        matches!(items.first(), Some(CleanItem::TextRun { .. }))
    }

    /// Bind every interpolation in a TEXT-FIRST region's sole text run to `text_var`, so the
    /// reactive `$.set_text(text_var, …)` op (emitted by [`Self::emit_ops`]) reuses the
    /// in-closure `$.text(...)` node var instead of the unbound `"text"` fallback (the X8
    /// ReferenceError). A no-op when the region is not a single text run. The owning
    /// emission (`emit_text_first_region`) lives in the sibling block-emit module, so this is
    /// the `pub(super)` seam through which the walk populates the sibling-shared
    /// `pub(super)` `interp_var` map.
    pub(super) fn bind_text_first_run(&mut self, scope_id: TemplateScopeId, text_var: &str) {
        let scope = self.ir().template_scope(scope_id);
        let items = clean_nodes(self.ir(), &scope.roots, CleanContext::region_root());
        if let [CleanItem::TextRun { interps, .. }] = items.as_slice() {
            for &interp in interps {
                self.interp_var.insert(interp, text_var.to_string());
            }
        }
    }

    /// The variable name for a single-element clone-root region (named by the
    /// root element's tag, e.g. `button`).
    pub(super) fn single_root_var_name(&mut self, scope_id: TemplateScopeId) -> String {
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
    pub(super) fn emit_walk(
        &mut self,
        out: &mut String,
        scope_id: TemplateScopeId,
        region_var: &str,
        mounts_fragment: bool,
    ) {
        let scope = self.ir().template_scope(scope_id);
        let ctx = CleanContext::region_root();
        let (items, last_indices) = clean_nodes_indexed(self.ir(), &scope.roots, ctx);
        // The region-root `{@debug}` effects, grouped by the clean-item gap they precede
        // (a `{@debug}` is non-rendering — dropped from `items` — so it rides a gap, never
        // a DOM position). Emitted INTERLEAVED at their document position, never hoisted.
        let gaps = self.interleaved_gaps(&scope.roots, &last_indices);

        if mounts_fragment {
            // Multi-root fragment: the clone var IS the fragment.
            self.emit_walk_over_items(out, &items, &gaps, WalkBase::Fragment(region_var), ctx);
        } else {
            // Single-element clone-root: the clone var IS the element; descend into
            // its children directly, and the element itself is reachable as
            // `region_var` (a dynamic op on it operates on the clone var directly).
            let [CleanItem::Node(only)] = items.as_slice() else {
                // No rendered element — a region whose only content is non-rendering
                // (e.g. a `{@debug}`-only block body). Emit the region-root debug effects
                // at their document position (never dropped); there is no DOM walk.
                self.emit_interleaved_gap(out, &gaps[0]);
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
            // A region-root `{@debug}` that PRECEDES the clone-root element emits right
            // after the clone frame (gaps[0]), before the element's own inits.
            self.emit_interleaved_gap(out, &gaps[0]);
            // The clone-root element's own bind PRELUDE cleanup and INIT-domain
            // attribute ops are emitted right after the clone frame named it. The
            // official per-element order (`RegularElement.js`) is: the bind prelude
            // (`remove_input_defaults` for an input value/checked/group bind,
            // `remove_textarea_child` for a `<textarea bind:value>`) → the init-domain
            // writes (`$.autofocus` / `$.set_class` / `$.set_attribute` / the reactive
            // accumulator decls) → the element's CHILD block → the inline RENDER ops
            // (below, after the walk).
            self.emit_bind_prelude(out, only, region_var);
            self.emit_node_inline_inits(out, only);
            let (child_items, child_last) = clean_nodes_indexed(self.ir(), &el.children, child_ctx);
            let child_gaps = self.interleaved_gaps(&el.children, &child_last);
            self.emit_walk_over_items(
                out,
                &child_items,
                &child_gaps,
                WalkBase::Element(region_var),
                child_ctx,
            );
            // `$.reset(region_var)` after the clone-root element's children, when
            // any child was named (matches official's innermost-first reset order).
            if any_item_needs_name(self.ir(), &child_items) {
                out.push_str(&format!("\t$.reset({region_var});\n"));
            }
            // The element's inline RENDER ops (`$.bind_this` / `$.action` / `$.attach`
            // / the action-host effect-wrapped events, in attribute source order) emit
            // AFTER the element's ENTIRE child block — the walk descents and the
            // `$.reset` — matching the official per-element order (child fragment
            // first, render-side setup after, before the grouped `$.template_effect`).
            // With no named child the block above is empty, so the ops land right
            // after the element's own inits — the static-children form.
            self.emit_inline_render_ops(out, only);
            // A region-root `{@debug}` that FOLLOWS the clone-root element emits after its
            // subtree (gaps[1]).
            self.emit_interleaved_gap(out, &gaps[1]);
        }
    }

    /// Emit a chained walk over a cleaned DOM-position sequence, populating the
    /// node/interp var maps.
    ///
    /// `gaps[g]` holds the non-rendering `{@debug}` nodes that fall in document order
    /// BEFORE clean-item `g` (and `gaps[items.len()]` the trailing ones). Each is emitted
    /// INTERLEAVED at its document position — never hoisted, never dropped — so the reactive
    /// `$.template_effect(debug)` lands in the official source-order slot.
    fn emit_walk_over_items(
        &mut self,
        out: &mut String,
        items: &[CleanItem],
        gaps: &[Vec<NodeId>],
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
        // A sole CONTROLLED `{#each}` child of a regular element anchors on the element
        // ITSELF (the official `is_controlled`): no `<!>` marker in the cloned skeleton, no
        // `$.first_child`/`$.child` descent — the each call targets the base element var
        // directly with the `EACH_IS_CONTROLLED` flag bit. The caller's `$.reset(base)` still
        // fires (the each is a named position). Only an `{#each}` qualifies (matching the
        // skeleton serializer's `is_sole_controlled`).
        if let WalkBase::Element(base_var) = base {
            if let [CleanItem::Node(only)] = items {
                if matches!(
                    self.client_node(*only),
                    ClientNode::Block(ClientBlock::Each(_))
                ) {
                    // A `{@debug}` sibling around a sole-controlled `{#each}` still emits at
                    // its document position (the each occupies the element itself).
                    self.emit_interleaved_gap(out, &gaps[0]);
                    self.node_var.insert(*only, base_var.to_string());
                    self.emit_block_call(out, *only, base_var, true);
                    self.emit_interleaved_gap(out, gaps.get(1).map_or(&[][..], |g| g));
                    return;
                }
            }
        }
        for (idx, item) in items.iter().enumerate() {
            // Any `{@debug}` falling in document order BEFORE this clean position emits
            // here — at its source-order slot — before the position's own walk/getter.
            self.emit_interleaved_gap(out, &gaps[idx]);
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
                    // The dynamic anchor-var nodes are MUTUALLY EXCLUSIVE (a node is exactly
                    // one `ClientNode` kind), so an `else if` chain classifies once and emits
                    // the matching inline form against the walked `<!>` anchor var.
                    if matches!(self.client_node(*node), ClientNode::Component(_)) {
                        // A component invocation — emit the `Child(node, …)` call INLINE here.
                        self.emit_component(out, *node, &var);
                    } else if matches!(self.client_node(*node), ClientNode::Render(_)) {
                        // A `{@render}` tag — emit the static snippet call / `$.snippet(node,
                        // …)` INLINE here.
                        self.emit_render(out, *node, &var);
                    } else if matches!(self.client_node(*node), ClientNode::Block(_)) {
                        // A control-flow block (`{#if}`/`{#each}`/`{#await}`/`{#key}`): emit
                        // its runtime call INLINE here (interleaved into the parent walk at
                        // the anchor position), recursing into the body region(s).
                        // (`emit_block_call` lives in `client_block_emit`.)
                        self.emit_block_call(out, *node, &var, false);
                    } else if matches!(self.client_node(*node), ClientNode::SvelteElement(_)) {
                        // A `<svelte:element this={…}>` dynamic element — emit its
                        // `$.element(node, get_tag, is_svg, callback)` call INLINE here against
                        // the walked `<!>` anchor var. (`emit_svelte_element` lives in
                        // `client_svelte_element`.)
                        self.emit_svelte_element(out, *node, &var);
                    } else if matches!(self.client_node(*node), ClientNode::Boundary(_)) {
                        // A `<svelte:boundary>` — emit its `$.boundary(node, props, callback)`
                        // call (with the hoisted `failed`/`pending` snippet block) INLINE here
                        // against the walked `<!>` anchor var. (`emit_svelte_boundary` lives in
                        // `client_svelte_boundary`.)
                        self.emit_svelte_boundary(out, *node, &var);
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
                            let child_ctx = ctx.for_children_of(tag);
                            let (child_items, child_last) =
                                clean_nodes_indexed(self.ir(), &el.children, child_ctx);
                            let child_gaps = self.interleaved_gaps(&el.children, &child_last);
                            self.emit_walk_over_items(
                                out,
                                &child_items,
                                &child_gaps,
                                WalkBase::Element(&var),
                                child_ctx,
                            );
                            if any_item_needs_name(self.ir(), &child_items) {
                                out.push_str(&format!("\t$.reset({var});\n"));
                            }
                            // `bind:this` + the init-domain lifecycle ops (`$.action`
                            // / `$.attach`) + the action-host effect-wrapped events
                            // are RENDER-side, emitted (in attribute SOURCE order)
                            // AFTER the element's ENTIRE child block — the walk
                            // descents and the `$.reset` — and BEFORE the next
                            // sibling's descent, matching the official per-element
                            // order (child fragment first, render-side setup after).
                            // With no named child the block above is empty, so the
                            // ops land right after the element's own inits — the
                            // static-children form.
                            self.emit_inline_render_ops(out, *node);
                        }
                    }
                }
            }
        }

        // Any trailing `{@debug}` (after the last clean position) emits before the
        // hydration cursor advance — at its document-order slot following the last node.
        self.emit_interleaved_gap(out, &gaps[items.len()]);

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
    pub(super) fn emit_set_text(&self, target: NodeId, memoizer: &mut Memoizer) -> String {
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
        for op in self.plan.all_ops() {
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
            .all_ops()
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

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
