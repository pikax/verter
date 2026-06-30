//! The recursive PER-REGION client emitter for the control-flow blocks
//! (`{#if}`/`{#each}`/`{#await}`/`{#key}`) + the declaration / `{@const}` / `{@debug}` tags.
//!
//! A block body is its OWN template-scope region: the emitter walks each region (the root
//! PLUS every block body / branch), hoisting that region's declared `{@const}` / `{const}`
//! consts at the top, cloning its static template frame, emitting any `{@debug}` effect,
//! walking the DOM (interleaving nested block calls at their `<!>` anchor positions), then
//! emitting the region's reactive ops. Block-head closures recurse into their child regions
//! through the same [`ClientEmitter::emit_region`] path.

use rustc_hash::FxHashSet;

use super::client::{ClientEmitter, RegionFrame};
use super::client_block_plan::EACH_IS_CONTROLLED;
use super::client_codegen_helpers::js_single_quoted;
use super::client_module_frame::emit_root_hoist;
use super::client_plan_types::{
    ClientAwait, ClientBlock, ClientDeclKeyword, ClientDeclaration, ClientEach, ClientIfBranch,
    ClientNode,
};
use super::html::{synthesize_region, TemplateFactory};
use super::ir::{BlockIr, IrNode, NodeId, SvelteRuntimeIr, TemplateScope, TemplateScopeId};
use super::whitespace::{clean_nodes, clean_nodes_indexed, CleanContext, CleanItem};

impl<'a> ClientEmitter<'a> {
    /// Plan + emit every region's module-hoisted template factory, in POST-ORDER (a block
    /// body's `$.from_html` is hoisted BEFORE its parent's, matching the official
    /// depth-first hoist order — so the `root` / `root_1` / … numbering matches). Each
    /// region's clone frame lands in [`Self::region_frame`]; a text-first region records NO
    /// hoist (its `$.text(...)` is emitted in the body), and a comment-anchor / standalone
    /// region records no hoist (its `$.comment()` frame is created in the body).
    pub(super) fn plan_region_factories(&mut self, out: &mut String) {
        let (regions, each_scopes) = self.post_order_regions();
        for scope_id in regions {
            let factory = synthesize_region(self.ir(), self.ir().template_scope(scope_id));
            let region_frame = match &factory {
                // ONLY a `$.from_html(...)` clone factory is module-hoisted + factory-called.
                TemplateFactory::FromHtml {
                    html,
                    fragment_flag,
                } => {
                    let var = self.alloc_name("root");
                    let mounts_fragment = emit_root_hoist(out, &var, html, *fragment_flag);
                    RegionFrame::FromHtml {
                        hoist_var: var,
                        mounts_fragment,
                    }
                }
                // A text-first region (lone static text / lone-or-mixed accepted interp run)
                // is emitted INLINE in the body (`var text = $.text(<seed>)`), NEVER a hoisted
                // `$.text(...)` called as a clone factory. The `$.next()` prelude is owner-kind
                // metadata: an `{#each}` body / else-fallback emits it, an `{#if}` / `{#key}` /
                // `{#await}` text-first body does not.
                TemplateFactory::TextNode { seed } => RegionFrame::TextNode {
                    seed: seed.clone(),
                    prelude_next: each_scopes.contains(&scope_id),
                },
                // A comment-anchor (block-only / empty / lone-`{@html}`) region has NO
                // module hoist — its `$.comment()` frame is created in the body, and the
                // body treats the comment as a fragment (walk via `$.first_child`).
                TemplateFactory::CommentAnchor { .. } => RegionFrame::CommentAnchor,
                // A STANDALONE component / static-`{@render}` root emits NO clone frame and
                // NO `$.append` — the component call / static render targets the region
                // anchor directly. Record the sole standalone root node for `emit_region`.
                TemplateFactory::Standalone { .. } => {
                    let scope = self.ir().template_scope(scope_id);
                    let node = standalone_root_node(self.ir(), scope);
                    match node {
                        Some(node) => RegionFrame::Standalone { node },
                        // Defensive: a standalone factory with no resolvable node falls
                        // back to a comment anchor (never reached on the accept path).
                        None => RegionFrame::CommentAnchor,
                    }
                }
            };
            self.region_frame.insert(scope_id, region_frame);
        }
    }

    /// The region scopes in POST-ORDER (every block body / branch BEFORE its enclosing
    /// region), the official depth-first template-hoist order, PLUS the set of scopes that
    /// are an `{#each}` render-callback body / else-fallback (the owner-kind metadata the
    /// text-first frame uses for its `$.next()` prelude). Both are gathered in the SAME walk.
    fn post_order_regions(&self) -> (Vec<TemplateScopeId>, FxHashSet<TemplateScopeId>) {
        let mut out = Vec::new();
        let mut each_scopes = FxHashSet::default();
        self.collect_post_order(self.ir().root, &mut out, &mut each_scopes);
        (out, each_scopes)
    }

    /// Append `scope_id`'s descendant block-body regions (depth-first) THEN `scope_id`,
    /// recording each `{#each}` body / else-fallback scope into `each_scopes`.
    fn collect_post_order(
        &self,
        scope_id: TemplateScopeId,
        out: &mut Vec<TemplateScopeId>,
        each_scopes: &mut FxHashSet<TemplateScopeId>,
    ) {
        let roots = self.ir().template_scope(scope_id).roots.clone();
        for root in roots {
            self.collect_child_regions(root, out, each_scopes);
        }
        out.push(scope_id);
    }

    /// Recurse into a node's nested block-body regions (and through element / component /
    /// special children), collecting them in post-order and recording `{#each}` body /
    /// else-fallback scopes. A `{#snippet}` declaration is the component/snippet surface
    /// (refused upstream) — its body region is not collected here.
    fn collect_child_regions(
        &self,
        node_id: NodeId,
        out: &mut Vec<TemplateScopeId>,
        each_scopes: &mut FxHashSet<TemplateScopeId>,
    ) {
        match self.ir().node(node_id) {
            IrNode::Element(el) => {
                for &child in &el.children {
                    self.collect_child_regions(child, out, each_scopes);
                }
            }
            // A component-family node's regions are its SLOT regions (default + named) +
            // its `{#snippet}`-def body regions — NOT its raw `children` (the slot content
            // lives in the slot regions; recursing children would double-collect it into
            // the parent region). Each is collected in its own post-order.
            IrNode::Component(component) => {
                self.collect_component_slot_regions(&component.slots, out, each_scopes);
            }
            IrNode::Special(special) => {
                self.collect_component_slot_regions(&special.slots, out, each_scopes);
            }
            IrNode::Block(block) => match block {
                BlockIr::If { branches } => {
                    for branch in branches {
                        self.collect_post_order(branch.body, out, each_scopes);
                    }
                }
                BlockIr::Each {
                    body, else_body, ..
                } => {
                    // The `{#each}` body + else-fallback carry the text-first `$.next()`
                    // prelude (the official each render/fallback callback advance) — record
                    // them so the frame planner can mark their text-first bodies.
                    each_scopes.insert(*body);
                    self.collect_post_order(*body, out, each_scopes);
                    if let Some(else_body) = else_body {
                        each_scopes.insert(*else_body);
                        self.collect_post_order(*else_body, out, each_scopes);
                    }
                }
                BlockIr::Await {
                    pending,
                    then_body,
                    catch_body,
                    ..
                } => {
                    // The official await TEMPLATE-hoist order is `then`, `catch`, `pending`
                    // (then's body is `root`, pending's is `root_1`) — distinct from the
                    // `$.await(node, get, pending, then, catch)` CALL-arg order.
                    for ts in [then_body, catch_body, pending].into_iter().flatten() {
                        self.collect_post_order(*ts, out, each_scopes);
                    }
                }
                BlockIr::Key { body, .. } => self.collect_post_order(*body, out, each_scopes),
                // A `{#snippet}` body is its OWN region (its `$.from_html` factory must be
                // hoisted) AND a render CALLBACK (`($$anchor, …) => {…}`), so a text-first
                // body emits the `$.next()` prelude.
                BlockIr::Snippet { body, .. } => {
                    each_scopes.insert(*body);
                    self.collect_post_order(*body, out, each_scopes);
                }
            },
            _ => {}
        }
    }

    /// Collect a component's slot regions (default + named) + its `{#snippet}`-def body
    /// regions in post-order, recording each as a render-CALLBACK scope (so a text-first
    /// slot / children / snippet body emits the official `$.next()` prelude — like an
    /// `{#each}` render callback).
    fn collect_component_slot_regions(
        &self,
        slots: &super::ir::ComponentSlots,
        out: &mut Vec<TemplateScopeId>,
        each_scopes: &mut FxHashSet<TemplateScopeId>,
    ) {
        for &snippet in &slots.snippet_defs {
            self.collect_child_regions(snippet, out, each_scopes);
        }
        if let Some(default) = slots.default {
            each_scopes.insert(default);
            self.collect_post_order(default, out, each_scopes);
        }
        for named in &slots.named {
            each_scopes.insert(named.region);
            self.collect_post_order(named.region, out, each_scopes);
        }
    }

    /// Emit ONE template-scope region: its hoisted `{@const}` / `{const}` declarations, the
    /// clone frame, any `{@debug}` effect, the DOM walk (which interleaves nested block
    /// calls), the reactive ops, and the mount into `anchor`. Recurses through the walk +
    /// block calls into every nested body region.
    pub(super) fn emit_region(
        &mut self,
        out: &mut String,
        scope_id: TemplateScopeId,
        anchor: &str,
    ) {
        // A CONTENT-FREE region emits an EMPTY closure body (`($$anchor) => {}`) — no clone
        // frame, no mount — matching official. The present-but-empty pending body of a
        // catch-only `{#await p}{:catch e}` is the canonical case.
        if self.region_emits_nothing(scope_id) {
            return;
        }

        // (a) Block-local declarations (`{@const}` derived + `{const}`/`{let}` inert),
        // hoisted at the region top BEFORE the clone frame (the official `state.consts`).
        self.emit_region_consts(out, scope_id);

        // (a2) Block-body `{#snippet}` consts — a snippet declared directly inside a block
        // body (`{#if}` / `{#each}` / …) is a LOCAL `const` in that body region (the
        // official `context.state.snippets`). The ROOT region's snippets are MODULE /
        // INSTANCE consts (emitted by `emit`/`emit_body`), so they are skipped here.
        if scope_id != self.ir().root {
            self.emit_region_snippets(out, scope_id);
        }

        // A region with NO DOM skeleton (a `{@debug}`-only / `{@const}`-only body): the
        // official emits a body of `state.consts` + `state.init` statements with NO clone
        // frame and NO `$.append`. The hoisted decls already emitted in (a); emit the
        // non-rendering `{@debug}` effects directly — they ARE the region roots, sitting at
        // the start of an empty DOM sequence — then stop; there is no clone/walk/ops/mount.
        let roots = self.ir().template_scope(scope_id).roots.clone();
        let has_dom = !clean_nodes(self.ir(), &roots, CleanContext::region_root()).is_empty();
        if !has_dom {
            let gaps = self.debug_gaps(&roots, &[]);
            self.emit_debug_gap(out, &gaps[0]);
            return;
        }

        // (b) The clone frame. A TEXT-FIRST region emits an in-closure `$.text(...)`; a
        // `FromHtml` region module-hoists a clone factory + factory-calls it; a comment-anchor
        // region creates its `$.comment()` frame here. The frame was planned ONCE in
        // [`Self::plan_region_factories`]; clone it out so the body emission can mutate `self`.
        match self.region_frame[&scope_id].clone() {
            RegionFrame::TextNode { seed, prelude_next } => {
                self.emit_text_first_region(out, scope_id, anchor, seed.as_deref(), prelude_next);
            }
            RegionFrame::FromHtml {
                hoist_var,
                mounts_fragment,
            } => {
                // The official PRE-CLONE `$.next();` for a text-first MULTI-ROOT fragment.
                if mounts_fragment && self.region_is_text_first(scope_id) {
                    out.push_str("\t$.next();\n");
                }
                let region_var = if mounts_fragment {
                    self.alloc_name("fragment")
                } else {
                    self.single_root_var_name(scope_id)
                };
                out.push_str(&format!("\tvar {region_var} = {hoist_var}();\n"));
                // (c) The DOM walk populates the node/interp var maps + interleaves nested
                // block calls at their `<!>` anchor positions, AND emits each `{@debug}`
                // snapshot effect at its DOCUMENT position (interleaved, never hoisted).
                self.emit_walk(out, scope_id, &region_var, mounts_fragment);
                // (d) The region's reactive ops (combined `$.template_effect` + binds + events).
                self.emit_ops(out, scope_id);
                // (e) The mount.
                out.push_str(&format!("\t$.append({anchor}, {region_var});\n"));
            }
            RegionFrame::CommentAnchor => {
                let region_var = self.alloc_name("fragment");
                out.push_str(&format!("\tvar {region_var} = $.comment();\n"));
                self.emit_walk(out, scope_id, &region_var, true);
                self.emit_ops(out, scope_id);
                out.push_str(&format!("\t$.append({anchor}, {region_var});\n"));
            }
            // A STANDALONE component / static-`{@render}` root: NO clone frame, NO
            // `$.append` — the call targets `anchor` directly.
            RegionFrame::Standalone { node } => match self.client_node(node) {
                ClientNode::Component(_) => self.emit_component(out, node, anchor),
                ClientNode::Render(_) => self.emit_render(out, node, anchor),
                _ => {}
            },
        }
    }

    /// Emit a TEXT-FIRST region body — the official `svelte@5.56.3` topology for a region
    /// whose whole cleaned sequence is a SINGLE text run (a lone static text, or one-or-more
    /// accepted interpolations, with no element/block sibling). It is the in-closure form,
    /// NOT a hoisted clone factory:
    ///
    /// - `$.next();` FIRST when `prelude_next` (an `{#each}` body / else-fallback);
    /// - `var text = $.text('seed');` for static text, `var text = $.text();` for any interp;
    /// - the sole text run's interpolations bound to `text` (so `emit_ops`'s `$.set_text`
    ///   reuses the bound var instead of the unbound `"text"` fallback);
    /// - the `{@debug}` snapshot effects at their document gaps (all BEFORE the reactive ops,
    ///   in source order — the walk-before-ops convention the official follows here);
    /// - the reactive ops (`$.template_effect(() => $.set_text(text, …))`);
    /// - `$.append($$anchor, text);`.
    fn emit_text_first_region(
        &mut self,
        out: &mut String,
        scope_id: TemplateScopeId,
        anchor: &str,
        seed: Option<&str>,
        prelude_next: bool,
    ) {
        if prelude_next {
            out.push_str("\t$.next();\n");
        }
        let text_var = self.alloc_name("text");
        match seed {
            Some(seed) => out.push_str(&format!(
                "\tvar {text_var} = $.text({});\n",
                js_single_quoted(seed)
            )),
            None => out.push_str(&format!("\tvar {text_var} = $.text();\n")),
        }
        // Bind the sole text run's interpolations to `text_var` so the reactive
        // `$.set_text(text_var, …)` reuses it (instead of the unbound `"text"` fallback).
        self.bind_text_first_run(scope_id, &text_var);
        // The `{@debug}` effects (non-rendering, dropped from the clean sequence) emit at
        // their document gaps — all before the reactive ops, in source order.
        let roots = self.ir().template_scope(scope_id).roots.clone();
        let (_, last_indices) = clean_nodes_indexed(self.ir(), &roots, CleanContext::region_root());
        let gaps = self.debug_gaps(&roots, &last_indices);
        for gap in &gaps {
            self.emit_debug_gap(out, gap);
        }
        // The reactive ops (`$.template_effect(() => $.set_text(text, …))`).
        self.emit_ops(out, scope_id);
        // The mount.
        out.push_str(&format!("\t$.append({anchor}, {text_var});\n"));
    }

    /// Whether `scope_id`'s region emits NOTHING: no hoisted `{@const}`/`{const}`/`{let}`
    /// declaration, no `{@debug}` effect, no rendered DOM position, and no reactive op. The
    /// official compiler emits an EMPTY closure body for such a region rather than a
    /// `$.comment()` clone frame + `$.append` — the present-but-empty pending body of a
    /// catch-only `{#await p}{:catch e}` is the canonical case.
    fn region_emits_nothing(&self, scope_id: TemplateScopeId) -> bool {
        let roots = &self.ir().template_scope(scope_id).roots;
        // A non-rendering root that still emits output: a `{@const}`/`{const}`/`{let}`
        // hoist (`Declarations`), a `{@debug}` snapshot effect (`Debug`), or a `{#snippet}`
        // DECLARATION (a block-body local `const`, emitted in a non-root region).
        for &root in roots {
            if matches!(
                self.client_node(root),
                ClientNode::Declarations { .. } | ClientNode::Debug { .. }
            ) {
                return false;
            }
            if scope_id != self.ir().root
                && matches!(self.client_node(root), ClientNode::SnippetDecl { .. })
            {
                return false;
            }
        }
        clean_nodes(self.ir(), roots, CleanContext::region_root()).is_empty()
            && self.plan().ops_in(scope_id).is_empty()
    }

    /// Emit a NON-ROOT region's block-body `{#snippet}` consts (the official
    /// `context.state.snippets`): each `{#snippet name}` declared directly in the region's
    /// roots emits as a local `const name = ($$anchor, …) => {…};`.
    fn emit_region_snippets(&mut self, out: &mut String, scope_id: TemplateScopeId) {
        let roots = self.ir().template_scope(scope_id).roots.clone();
        for root in roots {
            if matches!(self.client_node(root), ClientNode::SnippetDecl { .. }) {
                out.push('\t');
                self.emit_snippet_decl(out, root);
                out.push('\n');
            }
        }
    }

    /// Emit a region's hoisted block-local declarations (the `{@const}` derived memos +
    /// `{const}`/`{let}` inert declarations + rune declarators), in source order. The
    /// declaration nodes are NON-RENDERING (dropped from the walk's clean sequence), so they
    /// are gathered DIRECTLY from the region's roots.
    fn emit_region_consts(&mut self, out: &mut String, scope_id: TemplateScopeId) {
        let roots = self.ir().template_scope(scope_id).roots.clone();
        for root in roots {
            let ClientNode::Declarations { decls } = self.client_node(root) else {
                continue;
            };
            for decl in decls {
                match decl {
                    ClientDeclaration::Derived { name, init } => {
                        out.push_str(&format!("\tconst {name} = $.derived(() => {init});\n"));
                    }
                    ClientDeclaration::Inert {
                        keyword,
                        name,
                        init,
                    } => {
                        let keyword = match keyword {
                            ClientDeclKeyword::Const => "const",
                            ClientDeclKeyword::Let => "let",
                        };
                        match init {
                            Some(init) => out.push_str(&format!("\t{keyword} {name} = {init};\n")),
                            None => out.push_str(&format!("\t{keyword} {name};\n")),
                        }
                    }
                    ClientDeclaration::Rune { code } => {
                        // The shared `$state` lowering already terminates `code` with `;`.
                        out.push_str(&format!("\t{code}\n"));
                    }
                }
            }
        }
    }

    /// The non-rendering `{@debug}` nodes among `children`, grouped by the clean-item gap
    /// they precede in document order: `gaps[g]` are the debugs before clean-item `g`, and
    /// `gaps[last_indices.len()]` the trailing ones. A `{@debug}` rides a gap (never a DOM
    /// position) because it is dropped from the clean sequence; `last_indices` maps each
    /// clean item to its last original-child index, so a debug at original index `d` falls
    /// in the gap counting the clean items whose last index precedes `d`. This places each
    /// debug effect at its official source-order slot during the walk — never hoisted.
    pub(super) fn debug_gaps(
        &self,
        children: &[NodeId],
        last_indices: &[usize],
    ) -> Vec<Vec<NodeId>> {
        let mut gaps: Vec<Vec<NodeId>> = vec![Vec::new(); last_indices.len() + 1];
        for (orig, &child) in children.iter().enumerate() {
            if matches!(self.client_node(child), ClientNode::Debug { .. }) {
                let gap = last_indices.iter().filter(|&&li| li < orig).count();
                gaps[gap].push(child);
            }
        }
        gaps
    }

    /// Emit each `{@debug}` in `debugs` at the current walk position (a gap slot from
    /// [`Self::debug_gaps`]).
    pub(super) fn emit_debug_gap(&self, out: &mut String, debugs: &[NodeId]) {
        for &node in debugs {
            self.emit_debug_effect(out, node);
        }
    }

    /// Emit one `{@debug}` reactive snapshot-logging effect:
    /// `$.template_effect(() => {console.log({ … }); debugger;})`, each entry a
    /// `key: $.snapshot(<rewritten>)` object property (a no-arg `{@debug}` logs `{}`).
    fn emit_debug_effect(&self, out: &mut String, node: NodeId) {
        let ClientNode::Debug { entries } = self.client_node(node) else {
            return;
        };
        let object = entries
            .iter()
            .map(|entry| format!("{}: $.snapshot({})", entry.key, entry.snapshot_arg))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!(
            "\t$.template_effect(() => {{console.log({{{object}}}); debugger;}});\n"
        ));
    }

    /// Emit the runtime call for a control-flow block at its anchor var, recursing into the
    /// body region(s). Dispatched from the DOM walk when it names a block-anchor position (a
    /// `<!>` comment → `controlled = false`), or when an `{#each}` is the SOLE child of an
    /// element and uses the parent element as its anchor (`controlled = true`). Only an
    /// `{#each}` is ever controlled (the official `is_controlled`); `{#if}`/`{#await}`/`{#key}`
    /// always get a `<!>` anchor.
    pub(super) fn emit_block_call(
        &mut self,
        out: &mut String,
        node: NodeId,
        anchor_var: &str,
        controlled: bool,
    ) {
        let ClientNode::Block(block) = self.client_node(node) else {
            return;
        };
        match block {
            ClientBlock::If { branches } => self.emit_if_block(out, anchor_var, branches),
            ClientBlock::Each(each) => self.emit_each_block(out, anchor_var, each, controlled),
            ClientBlock::Await(awaited) => self.emit_await_block(out, anchor_var, awaited),
            ClientBlock::Key { expr, body } => self.emit_key_block(out, anchor_var, expr, *body),
        }
    }

    /// `{ var consequent = ($$anchor) => {…}; … $.if(node, ($$render) => {if (t) $$render(c);
    /// else if (t1) $$render(c1, 1); else $$render(alt, -1);}); }`.
    fn emit_if_block(
        &mut self,
        out: &mut String,
        anchor_var: &str,
        branches: &'a [ClientIfBranch],
    ) {
        out.push_str("\t{");
        // (1) The branch closures, in source order — `consequent` / `consequent_1` / … for a
        // tested branch, `alternate` for the `{:else}`.
        let mut names = Vec::with_capacity(branches.len());
        for branch in branches {
            let stem = if branch.test.is_some() {
                "consequent"
            } else {
                "alternate"
            };
            let name = self.alloc_name(stem);
            out.push_str(&format!("var {name} = ($$anchor) => {{"));
            self.emit_region(out, branch.body, "$$anchor");
            out.push_str("};");
            names.push(name);
        }
        // (2) The `$.if` render selector. The first branch carries no ordinal; each later
        // else-if carries its branch index; the `{:else}` carries -1.
        out.push_str(&format!("$.if({anchor_var}, ($$render) => {{"));
        for (index, branch) in branches.iter().enumerate() {
            let name = &names[index];
            if index == 0 {
                let test = branch.test.as_deref().unwrap_or("true");
                out.push_str(&format!("if ({test}) $$render({name});"));
            } else if let Some(test) = &branch.test {
                out.push_str(&format!(" else if ({test}) $$render({name}, {index});"));
            } else {
                out.push_str(&format!(" else $$render({name}, -1);"));
            }
        }
        out.push_str("});}\n");
    }

    /// `$.each(node, FLAG, () => SOURCE, KEYFN, ($$anchor, item[, index]) => {…}[, ($$anchor)
    /// => {…else…}]);`.
    fn emit_each_block(
        &mut self,
        out: &mut String,
        anchor_var: &str,
        each: &'a ClientEach,
        controlled: bool,
    ) {
        // The `EACH_IS_CONTROLLED` bit is a DOM-position fact (the each is the sole child of a
        // regular element, anchoring on it directly), OR'd in here onto the projected
        // item/index/immutability flag.
        let flags = each.flags | if controlled { EACH_IS_CONTROLLED } else { 0 };
        out.push_str(&format!(
            "\t$.each({anchor_var}, {flags}, () => {}, ",
            each.source
        ));
        // The key callback: `(item[, index]) => key` (keyed) or the `$.index` literal.
        match &each.key {
            Some(key) => {
                let params = key.params.join(", ");
                out.push_str(&format!("({params}) => {}, ", key.expr));
            }
            None => out.push_str("$.index, "),
        }
        // The render callback params: `($$anchor, item[, index])`.
        out.push_str("($$anchor");
        if let Some(item) = &each.item_param {
            out.push_str(&format!(", {item}"));
        }
        if each.emit_index {
            if let Some(index) = &each.index_param {
                out.push_str(&format!(", {index}"));
            }
        }
        out.push_str(") => {");
        self.emit_region(out, each.body, "$$anchor");
        out.push('}');
        // The `{:else}` fallback callback.
        if let Some(else_body) = each.else_body {
            out.push_str(", ($$anchor) => {");
            self.emit_region(out, else_body, "$$anchor");
            out.push('}');
        }
        out.push_str(");\n");
    }

    /// `$.await(node, () => PROMISE, PENDING, THEN, CATCH);` — trailing absent callbacks
    /// are omitted; a middle-absent slot carries the official per-slot sentinel: an absent
    /// PENDING is `null`, an absent THEN before a following catch is `void 0` (matching
    /// `svelte@5.56.3`'s `$.await(node, get, null, void 0, catch)` for the no-then shapes).
    ///
    /// The branch closures are EMITTED (their DOM vars allocated) in TEMPLATE-HOIST order
    /// (`then`, `catch`, `pending` — matching the post-order factory allocation) into
    /// buffers, then placed into the call in CALL-arg order (`pending`, `then`, `catch`), so
    /// the DOM var spellings (`p` / `p_1`) match the official's compile-order allocation.
    fn emit_await_block(&mut self, out: &mut String, anchor_var: &str, awaited: &'a ClientAwait) {
        let then_closure = awaited
            .then_body
            .map(|body| self.branch_closure(awaited.then_param.as_deref(), body));
        let catch_closure = awaited
            .catch_body
            .map(|body| self.branch_closure(awaited.catch_param.as_deref(), body));
        let pending_closure = awaited.pending.map(|body| self.branch_closure(None, body));

        out.push_str(&format!(
            "\t$.await({anchor_var}, () => {}, ",
            awaited.promise
        ));
        // Call-arg order: pending (0), then (1), catch (2). Trailing absent slots are
        // omitted; a MIDDLE-absent slot carries the official per-position sentinel — an
        // absent PENDING is `null`, an absent THEN (only ever middle-absent when a catch
        // follows) is `void 0` (svelte@5.56.3's `$.await(node, get, null|pending, void 0,
        // catch)` no-then shapes). The catch slot is never a middle (it is last or omitted).
        let slots = [pending_closure, then_closure, catch_closure];
        let last = slots.iter().rposition(Option::is_some).unwrap_or(0);
        for (index, slot) in slots.iter().enumerate().take(last + 1) {
            if index > 0 {
                out.push_str(", ");
            }
            match slot {
                Some(closure) => out.push_str(closure),
                None if index == 1 => out.push_str("void 0"),
                None => out.push_str("null"),
            }
        }
        out.push_str(");\n");
    }

    /// Emit a branch body closure (`($$anchor[, param]) => { <region> }`) to a fresh buffer,
    /// allocating its DOM vars at call time (the caller controls the allocation ORDER).
    fn branch_closure(&mut self, param: Option<&str>, body: TemplateScopeId) -> String {
        let mut buf = String::from("($$anchor");
        if let Some(param) = param {
            buf.push_str(&format!(", {param}"));
        }
        buf.push_str(") => {");
        self.emit_region(&mut buf, body, "$$anchor");
        buf.push('}');
        buf
    }

    /// `$.key(node, () => EXPR, ($$anchor) => {…});`.
    fn emit_key_block(
        &mut self,
        out: &mut String,
        anchor_var: &str,
        expr: &'a str,
        body: TemplateScopeId,
    ) {
        out.push_str(&format!(
            "\t$.key({anchor_var}, () => {expr}, ($$anchor) => {{"
        ));
        self.emit_region(out, body, "$$anchor");
        out.push_str("});\n");
    }
}

/// The SOLE standalone root node of a region — the one cleaned `CleanItem::Node` (a
/// component or a resolved-snippet `{@render}`) the `is_standalone` factory identifies.
/// `None` when the region is not a single standalone node (never reached on the accept
/// path for a `Standalone` factory).
fn standalone_root_node(ir: &SvelteRuntimeIr, scope: &TemplateScope) -> Option<NodeId> {
    let items = clean_nodes(ir, &scope.roots, CleanContext::region_root());
    match items.as_slice() {
        [CleanItem::Node(only)] => Some(*only),
        _ => None,
    }
}
