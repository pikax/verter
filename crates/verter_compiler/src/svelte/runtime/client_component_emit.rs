//! The client EMITTER for component invocations, `{#snippet}` definitions,
//! `{@render}` tags, and the SHARED region-callback emitter.
//!
//! [`ClientEmitter::emit_region_callback`] is the shared region→callback emitter for the
//! COMPONENT-vertical callback surfaces — a component's `children` / named-slot closure, a
//! `{#snippet}` body, and a `<svelte:component>` render callback — each emitting a
//! `(<params>) => { <region> }` arrow (optionally a `const <name> = …;` declaration). It
//! recurses through the same [`ClientEmitter::emit_region`] authority the control-flow
//! block closures (`emit_if_block` / `emit_each_block`) use, but those hand-build their own
//! consequent / each callback wrappers rather than routing through this helper. It is NEVER
//! a snippet-only clone — the `<svelte:boundary>` body region (when that host lands) reuses
//! it unchanged.

use super::client::ClientEmitter;
use super::client_codegen_helpers::object_key;
use super::client_plan_types::{
    ClientComponent, ClientNode, ClientRender, ComponentCallee, ComponentMember, ComponentProps,
    ComponentSpreadPart, SlotEntry,
};
use super::ir::{BlockIr, IrNode, LetBinding, NodeId, PatternId, TemplateScopeId};

/// Where a region-callback is PLACED — a module-scope `const` (a hoistable snippet),
/// a local `const` (a capturing / component-nested snippet), or an inline arrow
/// argument (a `children` / slot / `<svelte:component>` callback).
pub(super) enum CallbackPlacement<'a> {
    /// `const <name> = (<params>) => { … };` — placement is the CALLER's responsibility
    /// (module scope for a hoistable snippet, the component-fn body / wrapping block for
    /// a capturing / nested one); this only spells the `const <name> = …;` form.
    Const(&'a str),
    /// `(<params>) => { … }` — an inline arrow argument (a slot / `children` /
    /// `<svelte:component>` render callback).
    InlineArg,
}

impl<'a> ClientEmitter<'a> {
    /// The SHARED region→callback emitter. Emits `(<params>) => { <let-deriveds> <region>
    /// }` (or `const <name> = (<params>) => { … };`), recursing into `scope_id` through
    /// the same [`emit_region`](ClientEmitter::emit_region) authority the block-body
    /// closures use. `lets` are the slot's `let:` slot props, prepended as `const <name> =
    /// $.derived(() => $$slotProps.<key>);` (the official `let_directives`).
    pub(super) fn emit_region_callback(
        &mut self,
        out: &mut String,
        scope_id: TemplateScopeId,
        params: &[String],
        lets: &[LetBinding],
        placement: CallbackPlacement<'_>,
    ) {
        self.emit_region_callback_full(out, scope_id, params, lets, "", "", placement);
    }

    /// [`emit_region_callback`](Self::emit_region_callback) with an `after_update` statement — a
    /// callback-body statement emitted INSIDE the region's frame, AFTER its reactive ops and
    /// BEFORE its `$.append` (the official fragment `after_update` slot; a no-DOM region emits it
    /// after its ops). The `<svelte:head>` callback uses it for the `<title>` →
    /// `$.document.title` effect, which sits between the head's `<meta>`/`<link>` body clone and
    /// its mount. A `after_update` is pre-built by the caller; the other callers pass `""`.
    pub(super) fn emit_region_callback_with_after_update(
        &mut self,
        out: &mut String,
        scope_id: TemplateScopeId,
        params: &[String],
        lets: &[LetBinding],
        after_update: &str,
        placement: CallbackPlacement<'_>,
    ) {
        self.emit_region_callback_full(out, scope_id, params, lets, "", after_update, placement);
    }

    /// [`emit_region_callback`](Self::emit_region_callback) with an additional `prelude` — a
    /// block of callback-body statements emitted AFTER the `{` and the `let:` deriveds but
    /// BEFORE the region body. The `<svelte:element>` callback uses it for the element's own
    /// setup (the `$.set_class` / `$.attribute_effect` fold + the `$$element`-hosted binds + the
    /// legacy `on:` `$.event` registrations, which precede the child-content region). A `prelude`
    /// is pre-built by the caller (it may allocate names); the existing component/snippet/slot
    /// callers pass `""`.
    pub(super) fn emit_region_callback_with_prelude(
        &mut self,
        out: &mut String,
        scope_id: TemplateScopeId,
        params: &[String],
        lets: &[LetBinding],
        prelude: &str,
        placement: CallbackPlacement<'_>,
    ) {
        self.emit_region_callback_full(out, scope_id, params, lets, prelude, "", placement);
    }

    /// The shared callback emitter both the `prelude` and `after_update` variants route through:
    /// the header (`const NAME = (params) => {` / `(params) => {`), the `let:` slot-prop
    /// deriveds, the `prelude` statements, the region body (with its `after_update` slot), and
    /// the closing `}` / `};`. `prelude` is emitted BEFORE the region body; `after_update` INSIDE
    /// the region frame after its ops; a caller needing neither passes `""` for both (an empty
    /// `after_update` makes `emit_region_with_after_update` byte-identical to `emit_region`).
    #[allow(clippy::too_many_arguments)]
    fn emit_region_callback_full(
        &mut self,
        out: &mut String,
        scope_id: TemplateScopeId,
        params: &[String],
        lets: &[LetBinding],
        prelude: &str,
        after_update: &str,
        placement: CallbackPlacement<'_>,
    ) {
        let param_str = params.join(", ");
        match &placement {
            CallbackPlacement::Const(name) => {
                out.push_str(&format!("const {name} = ({param_str}) => {{"));
            }
            CallbackPlacement::InlineArg => {
                out.push_str(&format!("({param_str}) => {{"));
            }
        }
        for binding in lets {
            out.push_str(&format!(
                "const {} = $.derived(() => $$slotProps.{});",
                binding.name, binding.key
            ));
        }
        out.push_str(prelude);
        self.emit_region_with_after_update(out, scope_id, "$$anchor", after_update);
        out.push('}');
        if matches!(placement, CallbackPlacement::Const(_)) {
            out.push(';');
        }
    }

    /// The snippet-body param list: `$$anchor` plus each declared param defaulting to
    /// `$.noop` (`a = $.noop`). A snippet reads its params as thunks (`a()`).
    fn snippet_params(&self, params: &[PatternId]) -> Vec<String> {
        let mut out = vec!["$$anchor".to_string()];
        for &pat in params {
            for &binding in self.ir().pattern_bindings(pat) {
                let name = self.ir().analysis.bindings.get(binding).name.clone();
                out.push(format!("{name} = $.noop"));
            }
        }
        out
    }

    /// Emit a `{#snippet}` definition as a `const <name> = ($$anchor, p = $.noop, …) => {
    /// … };` declaration (the CALLER places it — module scope, the component-fn body, or a
    /// component's wrapping block).
    pub(super) fn emit_snippet_decl(&mut self, out: &mut String, node: NodeId) {
        let IrNode::Block(BlockIr::Snippet {
            name, params, body, ..
        }) = self.ir().node(node)
        else {
            return;
        };
        let (params, body) = (params.clone(), *body);
        let snippet_name = self.ir().analysis.bindings.get(*name).name.clone();
        let param_list = self.snippet_params(&params);
        self.emit_region_callback(
            out,
            body,
            &param_list,
            &[],
            CallbackPlacement::Const(&snippet_name),
        );
    }

    /// Mint the function-pair component-bind locals (`bind_get` / `bind_set`) through the
    /// shared scope-aware allocator, keyed by each pair's component-function-scoped INDEX.
    /// Iterates the planned component nodes in INDEX order so the first pair is
    /// `bind_get`/`bind_set`, the next `bind_get_1`/`bind_set_1`, … (each stem pushed past a
    /// user binding of the same name by the seeded [`alloc_name`](ClientEmitter::alloc_name),
    /// and each stem reserved INDEPENDENTLY). Populates
    /// [`fn_pair_bind_names`](ClientEmitter::fn_pair_bind_names). Called ONCE from
    /// [`ClientEmitter::new`], mirroring
    /// [`plan_group_accumulators`](ClientEmitter::plan_group_accumulators).
    pub(super) fn plan_fn_pair_bind_names(&mut self) {
        // Copy the plan ref so the node scan does not borrow `self` while `alloc_name` mutates
        // `self.used` + the name map.
        let plan = self.plan();
        let mut indices: Vec<usize> = Vec::new();
        for node in &plan.nodes {
            if let ClientNode::Component(comp) = node {
                for pair in &comp.fn_pair_binds {
                    indices.push(pair.index);
                }
            }
        }
        // Allocate in INDEX order (source order across every component call), so the suffixes
        // line up with the official per-function `scope.generate` order.
        indices.sort_unstable();
        for index in indices {
            let get = self.alloc_name(super::client::FN_PAIR_BIND_GET_NAME);
            let set = self.alloc_name(super::client::FN_PAIR_BIND_SET_NAME);
            self.fn_pair_bind_names.insert(index, (get, set));
        }
    }

    /// The `(bind_get, bind_set)` local names minted for the function-pair component bind at
    /// `index` (populated in [`Self::new`]). A missing index is a plan/emit desync (a bug), so
    /// it panics loudly rather than silently defaulting to a colliding name.
    fn fn_pair_bind_name(&self, index: usize) -> &(String, String) {
        self.fn_pair_bind_names.get(&index).unwrap_or_else(|| {
            panic!("function-pair bind index {index} has no allocated name pair (plan/emit desync)")
        })
    }

    /// Emit a projected component invocation against `anchor_var` (a sole-root standalone
    /// region anchor, or a walked `<!>` node var). Wraps in a `{ … }` block when there are
    /// snippet-def consts or hoisted pre-statements (deriveds / function-pair bind vars).
    pub(super) fn emit_component(&mut self, out: &mut String, node_id: NodeId, anchor_var: &str) {
        let ClientNode::Component(comp) = self.client_node(node_id) else {
            return;
        };
        let comp: ClientComponent = comp.clone();
        // The block wraps snippet-def consts + prop deriveds (the official `statements`
        // block, emitted when `statements.length > 1`). The function-pair bind vars are
        // `state.init` — emitted at the call's statement level (NOT in the block).
        let needs_block = !comp.snippet_defs.is_empty() || !comp.block_statements.is_empty();

        out.push('\t');
        // (a) The function-pair bind vars (`var bind_get = …`) at the call's statement level —
        // `var` hoists, so they sit beside the call without forcing a block. The local names
        // were minted by the shared scope-aware allocator (keyed by pair index), so they never
        // collide with a user binding.
        for pair in &comp.fn_pair_binds {
            let (get_local, set_local) = self.fn_pair_bind_name(pair.index);
            out.push_str(&format!("var {get_local} = {};", pair.get_expr));
            out.push_str(&format!("var {set_local} = {};", pair.set_expr));
        }
        if needs_block {
            out.push('{');
        }
        // (b) The component-nested `{#snippet}` defs — local consts inside the block.
        for &snippet in &comp.snippet_defs {
            self.emit_snippet_decl(out, snippet);
        }
        // (c) The prop deriveds (`let $0 = $.derived(…)`) inside the block.
        for stmt in &comp.block_statements {
            out.push_str(stmt);
        }
        // (d) The call.
        self.emit_component_call(out, &comp, anchor_var);
        if needs_block {
            out.push('}');
        }
        out.push('\n');
    }

    /// Emit the component call itself (props + callee + `bind:this` / `$.component`
    /// wrappers), terminated with `;`.
    fn emit_component_call(&mut self, out: &mut String, comp: &ClientComponent, anchor_var: &str) {
        match &comp.callee {
            ComponentCallee::Static { name } => {
                // A `bind:this` wraps the call in `$.bind_this(<call>, set, get)`.
                if let Some(bind_this) = &comp.bind_this {
                    out.push_str("$.bind_this(");
                    self.emit_call_expr(out, name, &comp.props, anchor_var);
                    out.push_str(&format!(", {}, {});", bind_this.setter, bind_this.getter));
                } else {
                    self.emit_call_expr(out, name, &comp.props, anchor_var);
                    out.push(';');
                }
            }
            // `<svelte:component this={expr}>` → `$.component(node, () => <this>,
            // ($$anchor, $$component) => { $$component($$anchor, <props>); });`. A `bind:this`
            // wraps the inner component call in `$.bind_this(<call>, set, get)` — the SAME wrap
            // the static callee uses (`project_bind_this` already projects the (setter, getter)
            // for ANY callee), so the dynamic host is not a second bind:this path.
            ComponentCallee::Dynamic { this_expr } => {
                out.push_str(&format!(
                    "$.component({anchor_var}, () => {this_expr}, ($$anchor, $$component) => {{"
                ));
                if let Some(bind_this) = &comp.bind_this {
                    out.push_str("$.bind_this(");
                    self.emit_call_expr(out, "$$component", &comp.props, "$$anchor");
                    out.push_str(&format!(", {}, {});", bind_this.setter, bind_this.getter));
                } else {
                    self.emit_call_expr(out, "$$component", &comp.props, "$$anchor");
                    out.push(';');
                }
                out.push_str("});");
            }
        }
    }

    /// Emit `<callee>(<anchor>, <props>)` — the props as a `{ … }` object or a
    /// `$.spread_props(…)` call (NO trailing `;`).
    fn emit_call_expr(
        &mut self,
        out: &mut String,
        callee: &str,
        props: &ComponentProps,
        anchor_var: &str,
    ) {
        out.push_str(&format!("{callee}({anchor_var}, "));
        match props {
            ComponentProps::Object(members) => {
                out.push('{');
                self.emit_members(out, members);
                out.push('}');
            }
            ComponentProps::Spread(parts) => {
                out.push_str("$.spread_props(");
                for (i, part) in parts.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    match part {
                        ComponentSpreadPart::Group(members) => {
                            out.push('{');
                            self.emit_members(out, members);
                            out.push('}');
                        }
                        ComponentSpreadPart::Spread { arg } => out.push_str(arg),
                    }
                }
                out.push(')');
            }
        }
        out.push(')');
    }

    /// Emit a comma-separated member list into the props object.
    fn emit_members(&mut self, out: &mut String, members: &[ComponentMember]) {
        for (i, member) in members.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            self.emit_member(out, member);
        }
    }

    /// Emit one props-object member. EVERY object key / accessor name on a component-props
    /// surface routes through [`object_key`] — a non-identifier name (`aria-label`, `foo-bar`)
    /// quotes (`'aria-label'`), an identifier stays bare. (A `SnippetProp` shorthand is a
    /// `{#snippet}` name, always a valid identifier by language rule, so it stays bare.)
    fn emit_member(&mut self, out: &mut String, member: &ComponentMember) {
        match member {
            ComponentMember::Init { key, value } => {
                out.push_str(&format!("{}: {value}", object_key(key)))
            }
            ComponentMember::Getter { key, body } => {
                out.push_str(&format!("get {}() {{return {body};}}", object_key(key)));
            }
            ComponentMember::GetSet {
                key,
                get_body,
                set_body,
            } => {
                let key = object_key(key);
                out.push_str(&format!(
                    "get {key}() {{return {get_body};}}, set {key}($$value) {{{set_body};}}"
                ));
            }
            ComponentMember::FnPairGetSet { key, index } => {
                let (get_local, set_local) = self.fn_pair_bind_name(*index);
                let key = object_key(key);
                out.push_str(&format!(
                    "get {key}() {{return {get_local}();}}, set {key}($$value) {{{set_local}($$value);}}"
                ));
            }
            ComponentMember::Events { entries } => {
                out.push_str("$$events: {");
                for (i, (event_type, handler)) in entries.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&format!("{}: {handler}", object_key(event_type)));
                }
                out.push('}');
            }
            ComponentMember::SnippetProp { name } => out.push_str(name),
            ComponentMember::DefaultChildren { region } => {
                out.push_str("children: ");
                self.emit_region_callback(
                    out,
                    *region,
                    &["$$anchor".to_string(), "$$slotProps".to_string()],
                    &[],
                    CallbackPlacement::InlineArg,
                );
            }
            ComponentMember::InvalidDefaultSnippet => {
                out.push_str("children: $.invalid_default_snippet");
            }
            ComponentMember::Slots { entries } => {
                out.push_str("$$slots: {");
                for (i, entry) in entries.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    self.emit_slot_entry(out, entry);
                }
                out.push('}');
            }
        }
    }

    /// Emit one `$$slots` entry. The slot key routes through [`object_key`] — a hyphenated slot
    /// name (`<svelte:fragment slot="foo-bar">`) quotes to `'foo-bar'`, an identifier stays bare.
    fn emit_slot_entry(&mut self, out: &mut String, entry: &SlotEntry) {
        match entry {
            SlotEntry::TrueMarker { name } => out.push_str(&format!("{}: true", object_key(name))),
            SlotEntry::Callback { name, region, lets } => {
                out.push_str(&format!("{}: ", object_key(name)));
                // The slot's `let:` deriveds were PLANNED at projection (the component's own
                // `default_lets`, or the named slot's `lets`) — the emitter consumes the
                // planned fact DIRECTLY and never rescans the IR / binding table per closure.
                self.emit_region_callback(
                    out,
                    *region,
                    &["$$anchor".to_string(), "$$slotProps".to_string()],
                    lets,
                    CallbackPlacement::InlineArg,
                );
            }
        }
    }

    /// Emit a projected `{@render}` tag against `anchor_var`.
    pub(super) fn emit_render(&mut self, out: &mut String, node_id: NodeId, anchor_var: &str) {
        let ClientNode::Render(render) = self.client_node(node_id) else {
            return;
        };
        let render: ClientRender = render.clone();
        out.push('\t');
        let args = render.args.join(", ");
        let arg_tail = if args.is_empty() {
            String::new()
        } else {
            format!(", {args}")
        };
        if render.dynamic {
            // `$.snippet(node, () => <fn>, …args);`
            out.push_str(&format!(
                "$.snippet({anchor_var}, () => {}{arg_tail});\n",
                render.callee
            ));
        } else {
            // A static snippet call — `pair(node, () => 1, () => 2);`.
            out.push_str(&format!("{}({anchor_var}{arg_tail});\n", render.callee));
        }
    }
}
