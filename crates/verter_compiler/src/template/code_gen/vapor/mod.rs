//! Vapor mode template code generation backend (first generation).
//!
//! Produces direct DOM manipulation code using `_template()`, `_setText()`,
//! `_setClass()`, `_renderEffect()`, etc. This targets Vue's upcoming
//! reactivity-based rendering engine that bypasses the virtual DOM.
//!
//! ## Output shape
//!
//! ```js
//! const t0 = _template("<div> </div>", true)
//!
//! function render(_ctx) {
//!   const n0 = t0()
//!   const x0 = _txt(n0)
//!   _renderEffect(() => {
//!     _setText(x0, _toDisplayString(_ctx.msg))
//!   })
//!   return n0
//! }
//! ```
//!
//! ## Codegen strategy
//!
//! Unlike the VDOM backend which does in-place source overwrites, the Vapor
//! backend **builds output strings and replaces the entire `<template>` block**
//! in a single overwrite. The output has three sections:
//!
//! 1. **Hoisted template declarations** — `const t0 = _template("<div>...</div>")`
//!    extracted from static HTML.
//! 2. **Delegated events** — `_delegateEvents("click", "input")` for event delegation.
//! 3. **Render function body** — template instantiation (`const n0 = t0()`),
//!    DOM navigation (`_child(n0)`, `_next(x0)`), text node creation, effects,
//!    and return statement.
//!
//! Key concepts:
//!
//! - **Counter-based variable naming** — `VaporCounters` allocates sequential
//!   names: `n0`/`n1` for node refs, `t0`/`t1` for templates, `x0`/`x1` for
//!   text nodes.
//! - **Element state stack** — `VaporElementState` is pushed on enter and
//!   popped on leave, accumulating HTML, navigation, effects, and text parts.
//!   Recycled via `state_pool` to retain `Vec` capacities.
//! - **Root element assembly** — each root child produces a `VaporRootElement`
//!   with its template HTML, node ref, nav instructions, and effects. These
//!   are assembled into the final output in `assemble_output()`.
//! - **Structural directives** — `v-if` chains are accumulated across siblings
//!   into `VIfChain` and flushed as nested `_createIf()` calls. `v-for` uses
//!   `_createFor()` with closure bodies built by `build_closure_body()`.
//!
//! ## Shared vs unique logic
//!
//! Binding resolution, runtime helper constants, and the DFS walker are shared
//! with the VDOM backend (see [`super::binding`], [`super::shared`],
//! [`super::walker`]). The `needs_quoted_key()` utility is reused from `vdom::props`.
//! This backend's unique elements are its stacked element state model,
//! counter-based naming, and root element assembly pattern.

pub mod comment;
pub mod element;
pub mod interpolation;
pub mod props;
pub mod text;

use crate::ast::types::{
    AstNodeKind, ChildrenFlags, CommentNode, ElementNode, ElementNodeConditionKind,
    InterpolationNode, TagType, TemplateAst, TextNode,
};
use crate::parser::types::RootNodeTemplate;
use crate::template::oxc::types::{OxcParsedElement, OxcParsedExpression};
use crate::types::NodeId;

use rustc_hash::FxHashSet;

use super::binding::BindingResolver;
use super::shared::helpers::{self, VaporHelper};
use super::types::{CodeGenOutput, VaporCounters, VaporElementState, VaporRootElement};
use super::vdom::props::needs_quoted_key;
use super::{TemplateCodeGen, TemplateCodeGenOptions};

/// Push a prop key to a buffer, quoting it if it contains hyphens or
/// other characters that make it an invalid bare JS identifier.
fn push_prop_key(buf: &mut String, key: &str) {
    if needs_quoted_key(key) {
        buf.push('"');
        helpers::escape_js_string_into(buf, key);
        buf.push('"');
    } else {
        buf.push_str(key);
    }
}

/// Extract the v-memo deps expression from an element's props.
/// Returns `Some("[dep1, dep2]")` if the element has `v-memo="[dep1, dep2]"`.
fn extract_v_memo_expr(el: &ElementNode, source: &str) -> Option<String> {
    for prop in &el.props {
        if !prop.is_directive {
            continue;
        }
        let name = &source[prop.start as usize..prop.name_end as usize];
        if name == "v-memo" {
            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                return Some(source[vs as usize..ve as usize].to_string());
            }
        }
    }
    None
}

/// Look up the OXC-parsed expression data for a given prop index.
///
/// `OxcParsedProp.prop_index` maps back to `ElementNode.props[prop_index]`. This
/// is an O(1) wrapper over the element's dense `prop_lookup` table
/// ([`OxcParsedElement::prop`]) — no linear scan over the sparse `props` vec.
pub(crate) fn find_prop_oxc_exp<'a, 'alloc>(
    oxc_el: Option<&'a OxcParsedElement<'alloc>>,
    prop_index: usize,
) -> Option<&'a OxcParsedExpression<'alloc>> {
    oxc_el?.prop(prop_index).and_then(|p| p.exp.as_ref())
}

/// Resolve an expression using OXC binding data when available, falling back
/// to simple identifier resolution.
///
/// This is the unified entry point for all Vapor expression resolution:
/// - If OXC binding data is present, uses `build_prefixed_expr` to walk
///   individual bindings and insert `_ctx.`/`$setup.`/`.value` at each position.
/// - Otherwise, falls back to `resolve_simple_expr` (simple identifiers only).
fn resolve_expr(
    expr: &str,
    value_start: u32,
    oxc_exp: Option<&OxcParsedExpression<'_>>,
    resolver: &BindingResolver<'_>,
    force_js: bool,
) -> String {
    if let Some(oxc) = oxc_exp {
        let ts_skip: Vec<(u32, u32)> = if force_js {
            oxc.expression
                .as_ref()
                .map(|e| crate::strip_types::typescript::collect_ts_removal_spans(e))
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        interpolation::build_prefixed_expr(expr, value_start, oxc, resolver, &ts_skip)
    } else {
        resolver.resolve_simple_expr(expr)
    }
}

/// A branch in a v-if/v-else-if/v-else chain.
struct VIfBranch<'alloc> {
    /// The condition expression (None for v-else).
    condition: Option<&'alloc str>,
    /// The closure body string (template instantiation, nav, effects, return).
    body: String,
}

/// Accumulator for v-if chains that span multiple sibling elements.
struct VIfChain<'alloc> {
    /// The outer node ref for the entire chain.
    outer_ref: u32,
    /// Accumulated branches.
    branches: Vec<VIfBranch<'alloc>>,
}

/// Vapor mode code generation backend.
///
/// Produces direct DOM manipulation code using `_template()`, `_setText()`,
/// `_setClass()`, `_renderEffect()`, etc.
pub struct VaporCodeGen<'ast, 'alloc> {
    /// Reference to the template AST arena for O(1) node lookups.
    ast: &'ast TemplateAst,
    resolver: BindingResolver<'alloc>,
    options: TemplateCodeGenOptions,
    /// Element state stack for tracking parent context during DFS.
    element_stack: Vec<VaporElementState<'alloc>>,
    /// Counter allocator for variable names.
    counters: VaporCounters,
    /// Completed root elements (ready for assembly).
    root_elements: Vec<VaporRootElement<'alloc>>,
    /// Depth counter (0 = root-level children of <template>).
    depth: u32,
    /// Pool of recycled VaporElementState instances (retains Vec capacities).
    state_pool: Vec<VaporElementState<'alloc>>,
    /// Collected delegated event names (in insertion order, deduplicated).
    delegated_events: Vec<&'alloc str>,
    /// Set for O(1) dedup of delegated events.
    delegated_events_set: FxHashSet<&'alloc str>,
    /// Templates hoisted by structural directives (v-if/v-for closures).
    /// Each entry is (template_idx, html_string).
    hoisted_templates: Vec<(u32, String)>,
    /// Pending v-if chain being accumulated across sibling elements.
    pending_vif_chain: Option<VIfChain<'alloc>>,
    /// Counter for v-memo cache slot allocation.
    memo_cache_idx: u32,
}

impl<'ast, 'alloc> VaporCodeGen<'ast, 'alloc> {
    pub fn new(
        ast: &'ast TemplateAst,
        resolver: BindingResolver<'alloc>,
        _source: &'alloc str,
        options: &TemplateCodeGenOptions,
    ) -> Self {
        Self {
            ast,
            resolver,
            options: options.clone(),
            element_stack: Vec::new(),
            counters: VaporCounters::default(),
            root_elements: Vec::new(),
            depth: 0,
            state_pool: Vec::new(),
            delegated_events: Vec::new(),
            delegated_events_set: FxHashSet::default(),
            hoisted_templates: Vec::new(),
            pending_vif_chain: None,
            memo_cache_idx: 0,
        }
    }

    /// Compute the DOM child index for a node within its parent element.
    ///
    /// Adjacent Text/Interpolation nodes coalesce into a single DOM child.
    /// Elements and Comments (when enabled) each count as one DOM child.
    /// Comments are invisible when `options.comments` is false.
    ///
    /// Only called for non-root elements (root elements use `finalize_root_element`).
    fn compute_dom_child_index(&self, child_id: NodeId) -> u32 {
        let child_node = &self.ast.nodes[child_id.0];
        let parent_id = child_node
            .parent
            .expect("root elements don't call compute_dom_child_index");
        let parent_el = match &self.ast.nodes[parent_id.0].kind {
            AstNodeKind::Element(el) => el,
            _ => unreachable!("parent of an element must be an element"),
        };
        let siblings = &parent_el
            .content
            .as_ref()
            .expect("parent must have content")
            .children;

        let mut dom_idx = 0u32;
        let mut in_text = false;
        for &sib_id in siblings {
            if sib_id == child_id {
                break;
            }
            match &self.ast.nodes[sib_id.0].kind {
                AstNodeKind::Text(_) | AstNodeKind::Interpolation(_) => {
                    if !in_text {
                        in_text = true;
                        dom_idx += 1;
                    }
                }
                AstNodeKind::Comment(_) => {
                    if self.options.comments {
                        in_text = false;
                        dom_idx += 1;
                    }
                    // When comments disabled, skip — invisible to DOM
                }
                AstNodeKind::Element(_) => {
                    in_text = false;
                    dom_idx += 1;
                }
            }
        }
        dom_idx
    }

    /// Build the inner closure body for a structural directive (v-if/v-for).
    ///
    /// Takes a finalized element state and produces the body lines:
    /// ```js
    /// const n2 = t0()
    /// [nav, text_creations, effects, statements]
    /// return n2
    /// ```
    fn build_closure_body(
        &mut self,
        mut state: VaporElementState<'alloc>,
        has_dynamic_text: bool,
        indent: &str,
        out: &mut CodeGenOutput<'alloc>,
    ) -> String {
        use super::shared::helpers::push_u32;

        // Finalize text parts
        element::finalize_text_parts(&mut state, has_dynamic_text);

        // Strip trailing close tags (Vue 3.6 minimization)
        element::strip_trailing_close_tags(&mut state.html);

        // Register the template as hoisted
        let template_idx = self.counters.next_template();
        out.add_vapor_import(VaporHelper::Template);

        self.hoisted_templates
            .push((template_idx, std::mem::take(&mut state.html)));

        // Allocate inner node ref
        let inner_ref = state.ensure_node_ref(&mut self.counters);

        // Collect all effects
        let mut all_effects = Vec::new();
        all_effects.append(&mut state.own_effects);
        all_effects.append(&mut state.child_effects);

        let mut body = String::with_capacity(128);

        // Template instantiation
        body.push_str(indent);
        body.push_str("  const n");
        push_u32(&mut body, inner_ref);
        body.push_str(" = t");
        push_u32(&mut body, template_idx);
        body.push_str("()\n");

        // Navigation
        for nav in &state.child_nav {
            body.push_str(indent);
            body.push_str("  ");
            body.push_str(nav);
            body.push('\n');
        }
        if !state.child_nav.is_empty() {
            out.add_vapor_import(VaporHelper::Child);
            out.add_vapor_import(VaporHelper::Next);
        }

        // Text node creations
        for tc in &state.child_text_creations {
            body.push_str(indent);
            body.push_str("  ");
            body.push_str(tc);
            body.push('\n');
        }
        if !state.child_text_creations.is_empty() {
            out.add_vapor_import(VaporHelper::Txt);
            out.add_vapor_import(VaporHelper::SetText);
        }

        // Effects
        if !all_effects.is_empty() {
            body.push_str(indent);
            body.push_str("  _renderEffect(() => ");
            if all_effects.len() == 1 {
                all_effects[0].write_code_into(&mut body);
            } else {
                body.push_str("{\n");
                for effect in &all_effects {
                    body.push_str(indent);
                    body.push_str("    ");
                    effect.write_code_into(&mut body);
                    body.push('\n');
                }
                body.push_str(indent);
                body.push_str("  }");
            }
            body.push_str(")\n");
            out.add_vapor_import(VaporHelper::RenderEffect);
        }

        // Statements
        for stmt in &state.child_statements {
            body.push_str(indent);
            body.push_str("  ");
            body.push_str(stmt);
            body.push('\n');
        }

        // Return
        body.push_str(indent);
        body.push_str("  return n");
        push_u32(&mut body, inner_ref);
        body.push('\n');

        body
    }

    /// Start or continue a v-if chain for a root-level v-if/v-else-if/v-else element.
    ///
    /// - v-if: start a new chain (flushes any pending one first)
    /// - v-else-if: add a conditional branch to the pending chain
    /// - v-else: add the final branch and flush
    fn handle_v_if_chain(
        &mut self,
        el: &ElementNode,
        source: &'alloc str,
        oxc_el: Option<&OxcParsedElement<'alloc>>,
        state: VaporElementState<'alloc>,
        has_dynamic_text: bool,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        let cond = el.v_condition.as_ref().unwrap();
        let body = self.build_closure_body(state, has_dynamic_text, "  ", out);

        match cond.kind {
            ElementNodeConditionKind::If => {
                // Flush any pending chain first (shouldn't normally happen)
                self.flush_vif_chain(out);

                let outer_ref = self.counters.next_node();
                // Resolve condition eagerly using OXC binding data
                let cond_expr =
                    if let (Some(vs), Some(ve)) = (cond.prop.value_start, cond.prop.value_end) {
                        let raw = &source[vs as usize..ve as usize];
                        let oxc_cond = oxc_el.and_then(|o| o.condition.as_ref());
                        out.alloc_str(&resolve_expr(
                            raw,
                            vs,
                            oxc_cond,
                            &self.resolver,
                            self.options.force_js,
                        ))
                    } else {
                        "true"
                    };

                self.pending_vif_chain = Some(VIfChain {
                    outer_ref,
                    branches: vec![VIfBranch {
                        condition: Some(cond_expr),
                        body,
                    }],
                });
            }
            ElementNodeConditionKind::ElseIf => {
                // Resolve condition eagerly using OXC binding data
                let cond_expr =
                    if let (Some(vs), Some(ve)) = (cond.prop.value_start, cond.prop.value_end) {
                        let raw = &source[vs as usize..ve as usize];
                        let oxc_cond = oxc_el.and_then(|o| o.condition.as_ref());
                        out.alloc_str(&resolve_expr(
                            raw,
                            vs,
                            oxc_cond,
                            &self.resolver,
                            self.options.force_js,
                        ))
                    } else {
                        "true"
                    };

                if let Some(chain) = &mut self.pending_vif_chain {
                    chain.branches.push(VIfBranch {
                        condition: Some(cond_expr),
                        body,
                    });
                }
                // Orphan v-else-if without preceding v-if — diagnostic is
                // already emitted by the parser's validate_v_condition_adjacency.
            }
            ElementNodeConditionKind::Else => {
                if let Some(chain) = &mut self.pending_vif_chain {
                    chain.branches.push(VIfBranch {
                        condition: None,
                        body,
                    });
                }
                // Flush immediately — chain is complete
                self.flush_vif_chain(out);
            }
        }
    }

    /// Flush the pending v-if chain, producing a single root element.
    ///
    /// Generates nested `_createIf` calls:
    /// ```js
    /// _createIf(() => (a), () => { A }, () => _createIf(() => (b), () => { B }, () => { C }))
    /// ```
    fn flush_vif_chain(&mut self, out: &mut CodeGenOutput<'alloc>) {
        let Some(chain) = self.pending_vif_chain.take() else {
            return;
        };

        use super::shared::helpers::push_u32;

        let mut stmt = String::with_capacity(256);
        stmt.push_str("const n");
        push_u32(&mut stmt, chain.outer_ref);
        stmt.push_str(" = ");

        // Build nested structure from branches
        self.write_vif_branches(&chain.branches, 0, &mut stmt);

        out.add_vapor_import(VaporHelper::CreateIf);

        self.root_elements.push(VaporRootElement {
            html: String::new(),
            template_idx: None,
            node_ref: chain.outer_ref,
            nav: Vec::new(),
            text_creations: Vec::new(),
            effects: Vec::new(),
            statements: vec![out.alloc_str(&stmt)],
            v_once: false,
            v_memo_expr: None,
        });
    }

    /// Recursively write nested _createIf calls for v-if chain branches.
    fn write_vif_branches(&self, branches: &[VIfBranch<'alloc>], idx: usize, stmt: &mut String) {
        if idx >= branches.len() {
            return;
        }

        let branch = &branches[idx];
        let remaining = branches.len() - idx - 1;

        if let Some(cond_expr) = branch.condition {
            // Condition is pre-resolved in handle_v_if_chain using OXC binding data
            stmt.push_str("_createIf(() => (");
            stmt.push_str(cond_expr);
            stmt.push_str("), () => {\n");
            stmt.push_str(&branch.body);

            if remaining > 0 {
                // Close the if-branch closure, add else argument
                // _createIf(() => (cond), () => { body }, () => { else })
                stmt.push_str("  }, () => ");
                let next = &branches[idx + 1];
                if next.condition.is_some() {
                    // v-else-if: wrap in another _createIf
                    self.write_vif_branches(branches, idx + 1, stmt);
                } else {
                    // v-else: direct closure
                    stmt.push_str("{\n");
                    stmt.push_str(&next.body);
                    stmt.push('}');
                }
                stmt.push(')');
            } else {
                // No else branch — close the if-branch closure and _createIf
                stmt.push_str("  })");
            }
        } else {
            // v-else without preceding v-if (shouldn't happen, but handle gracefully)
            stmt.push_str("{\n");
            stmt.push_str(&branch.body);
            stmt.push('}');
        }
    }

    /// Build a root element for a v-for directive.
    fn build_v_for_root(
        &mut self,
        el: &ElementNode,
        source: &'alloc str,
        state: VaporElementState<'alloc>,
        has_dynamic_text: bool,
        out: &mut CodeGenOutput<'alloc>,
    ) -> VaporRootElement<'alloc> {
        use super::shared::helpers::push_u32;

        let outer_ref = self.counters.next_node();

        // Get the v-for expression: "item in items" → source="items", param="item"
        let v_for_prop = el.v_for.as_ref().unwrap();
        let full_expr = if let (Some(vs), Some(ve)) = (v_for_prop.value_start, v_for_prop.value_end)
        {
            &source[vs as usize..ve as usize]
        } else {
            // Fallback: skip v-for with no expression
            return VaporRootElement {
                html: String::new(),
                template_idx: None,
                node_ref: outer_ref,
                nav: Vec::new(),
                text_creations: Vec::new(),
                effects: Vec::new(),
                statements: Vec::new(),
                v_once: false,
                v_memo_expr: None,
            };
        };

        // Parse "item in items" or "(item, index) in items"
        let (param_part, source_part) = helpers::parse_v_for_expression(full_expr);

        // Build the closure body
        let closure_body = self.build_closure_body(state, has_dynamic_text, "  ", out);

        // Extract :key expression if present
        let key_expr = self.extract_key_expr(el, source);

        // Build the _createFor statement
        let resolved_source = self.resolver.resolve_simple_expr(source_part);
        let mut stmt = String::with_capacity(256);
        stmt.push_str("const n");
        push_u32(&mut stmt, outer_ref);
        stmt.push_str(" = _createFor(() => (");
        stmt.push_str(&resolved_source);
        stmt.push_str("), (");
        stmt.push_str(param_part);
        stmt.push_str(") => {\n");
        stmt.push_str(&closure_body);
        stmt.push_str("  }");

        // Add :key callback if present
        if let Some(key) = key_expr {
            stmt.push_str(", (");
            stmt.push_str(param_part);
            stmt.push_str(") => (");
            stmt.push_str(key);
            stmt.push(')');
        }

        stmt.push(')');

        out.add_vapor_import(VaporHelper::CreateFor);

        VaporRootElement {
            html: String::new(),
            template_idx: None,
            node_ref: outer_ref,
            nav: Vec::new(),
            text_creations: Vec::new(),
            effects: Vec::new(),
            statements: vec![out.alloc_str(&stmt)],
            v_once: false,
            v_memo_expr: None,
        }
    }

    /// Extract the :key expression from an element's props.
    fn extract_key_expr<'s>(&self, el: &ElementNode, source: &'s str) -> Option<&'s str> {
        for prop in &el.props {
            if !prop.is_directive {
                continue;
            }
            if let (Some(arg_start), Some(arg_end)) = (prop.arg_start, prop.arg_end) {
                let arg = &source[arg_start as usize..arg_end as usize];
                if arg == "key" {
                    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                        return Some(&source[vs as usize..ve as usize]);
                    }
                }
            }
        }
        None
    }

    /// Merge a non-root structural element (component, slot outlet) into its parent.
    ///
    /// Emits `_setInsertionState(parentRef, null, domChildIndex, true)` and pushes
    /// the element's creation statements into the parent's `child_statements`.
    fn merge_non_root_into_parent(
        &mut self,
        child_id: NodeId,
        root: VaporRootElement<'alloc>,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        use super::shared::helpers::push_u32;

        // Compute DOM child index before borrowing element_stack mutably
        let dom_child_index = self.compute_dom_child_index(child_id);

        if let Some(parent) = self.element_stack.last_mut() {
            let ref_idx = parent.ensure_node_ref(&mut self.counters);

            // Build _setInsertionState(nN, null, childIndex, true)
            let mut insertion = String::with_capacity(48);
            insertion.push_str("_setInsertionState(n");
            push_u32(&mut insertion, ref_idx);
            insertion.push_str(", null, ");
            push_u32(&mut insertion, dom_child_index);
            insertion.push_str(", true)");
            parent.child_statements.push(out.alloc_str(&insertion));
            out.add_vapor_import(VaporHelper::SetInsertionState);

            // Push all creation statements from the structural element
            for stmt in root.statements {
                parent.child_statements.push(stmt);
            }
        }
    }

    /// Build a root element for a component (`_resolveComponent` + `_createComponentWithFallback`).
    #[allow(clippy::too_many_arguments)]
    fn build_component_root(
        &mut self,
        el: &ElementNode,
        tag_name: &str,
        node_ref: u32,
        source: &'alloc str,
        mut state: VaporElementState<'alloc>,
        oxc_el: Option<&OxcParsedElement<'alloc>>,
        out: &mut CodeGenOutput<'alloc>,
    ) -> VaporRootElement<'alloc> {
        use super::shared::helpers::{is_builtin_component, push_u32, to_pascal_case};

        // Resolve the component reference.
        // Priority: 1) direct binding, 2) PascalCase binding, 3) built-in, 4) _resolveComponent
        let (resolve_line, comp_ref) = if self.resolver.get(tag_name).is_some() {
            // Direct binding — no _resolveComponent needed
            let prefix = self.resolver.resolve_prefix(tag_name);
            let suffix = self.resolver.resolve_suffix(tag_name);
            let mut s = String::with_capacity(32);
            s.push_str(prefix);
            s.push_str(tag_name);
            s.push_str(suffix);
            (None, out.alloc_str(&s))
        } else {
            let pascal = to_pascal_case(tag_name);
            if self.resolver.get(&pascal).is_some() {
                // PascalCase binding match (for kebab-case tags)
                let prefix = self.resolver.resolve_prefix(&pascal);
                let suffix = self.resolver.resolve_suffix(&pascal);
                let mut s = String::with_capacity(32);
                s.push_str(prefix);
                s.push_str(&pascal);
                s.push_str(suffix);
                (None, out.alloc_str(&s))
            } else if let Some((flag, helper_name)) =
                is_builtin_component(tag_name).or_else(|| is_builtin_component(&pascal))
            {
                // Vue built-in component (Transition, KeepAlive, Teleport, Suspense)
                out.add_builtin_component(flag);
                (None, out.alloc_str(helper_name))
            } else {
                // Need _resolveComponent
                let comp_var = {
                    let mut s = String::with_capacity(32);
                    s.push_str("_component_");
                    for c in tag_name.chars() {
                        match c {
                            '-' | '.' => s.push('_'),
                            _ => s.push(c),
                        }
                    }
                    s
                };
                let mut line = String::with_capacity(64);
                line.push_str("const ");
                line.push_str(&comp_var);
                line.push_str(" = _resolveComponent(\"");
                line.push_str(tag_name);
                line.push_str("\")");
                out.add_vapor_import(VaporHelper::ResolveComponent);
                let resolve = out.alloc_str(&line);
                let comp_ref = out.alloc_str(&comp_var);
                (Some(resolve), comp_ref)
            }
        };

        // Build component props object
        let props_str = self.build_component_props(el, source, oxc_el, out);

        // Build slot closures from children
        let named_slots = std::mem::take(&mut state.named_slots);
        let has_default_content = !state.html.is_empty()
            || !state.child_nav.is_empty()
            || !state.child_effects.is_empty()
            || !state.child_statements.is_empty()
            || !state.child_text_creations.is_empty();

        let slots_str = if !named_slots.is_empty() {
            // Has named slots (and possibly an implicit default slot)
            let has_dynamic_text = el.children_flag.has(ChildrenFlags::HasInterpolation);
            let mut result = String::with_capacity(256);
            result.push_str("{ ");
            for (i, entry) in named_slots.iter().enumerate() {
                if i > 0 {
                    result.push_str(", ");
                }
                result.push_str(entry);
            }
            if has_default_content {
                // Implicit default slot from non-template children
                let body = self.build_closure_body(state, has_dynamic_text, "    ", out);
                if !named_slots.is_empty() {
                    result.push_str(", ");
                }
                result.push_str("default: () => {\n");
                result.push_str(&body);
                result.push_str("    }");
            }
            result.push_str(", _: 2 }");
            Some(result)
        } else if has_default_content {
            Some(self.build_default_slot_closure(state, el, out))
        } else {
            None
        };

        let mut create_line = String::with_capacity(128);
        create_line.push_str("const n");
        push_u32(&mut create_line, node_ref);
        create_line.push_str(" = _createComponentWithFallback(");
        create_line.push_str(comp_ref);

        if props_str.is_some() || slots_str.is_some() {
            create_line.push_str(", ");
            if let Some(props) = &props_str {
                create_line.push_str(props);
            } else {
                create_line.push_str("null");
            }
            if let Some(slots) = &slots_str {
                create_line.push_str(", ");
                create_line.push_str(slots);
            }
        } else {
            create_line.push_str(", null, null, true");
        }
        create_line.push(')');
        out.add_vapor_import(VaporHelper::CreateComponentWithFallback);

        let mut statements = Vec::new();
        if let Some(resolve) = resolve_line {
            statements.push(resolve);
        }
        statements.push(out.alloc_str(&create_line));

        VaporRootElement {
            html: String::new(),
            template_idx: None,
            node_ref,
            nav: Vec::new(),
            text_creations: Vec::new(),
            effects: Vec::new(),
            statements,
            v_once: false,
            v_memo_expr: None,
        }
    }

    /// Build a default slot closure from a component's accumulated child state.
    ///
    /// Produces: `{ default: () => { const n1 = t0(); return n1 }, _: 2 }`
    fn build_default_slot_closure(
        &mut self,
        state: VaporElementState<'alloc>,
        el: &ElementNode,
        out: &mut CodeGenOutput<'alloc>,
    ) -> String {
        let has_dynamic_text = el.children_flag.has(ChildrenFlags::HasInterpolation);
        let body = self.build_closure_body(state, has_dynamic_text, "    ", out);

        let mut result = String::with_capacity(128);
        result.push_str("{ default: () => {\n");
        result.push_str(&body);
        result.push_str("    }, _: 2 }");
        result
    }

    /// Build a component props object string from element props.
    ///
    /// Returns None if no props, or Some("{ key: value, ... }").
    /// Static props: `title: "hello"`
    /// Dynamic props: `title: () => (expr)`
    /// Events: `onClick: () => handler`
    fn build_component_props(
        &self,
        el: &ElementNode,
        source: &'alloc str,
        oxc_el: Option<&OxcParsedElement<'alloc>>,
        _out: &mut CodeGenOutput<'alloc>,
    ) -> Option<String> {
        if el.props.is_empty() {
            return None;
        }

        let mut entries: Vec<String> = Vec::new();

        for (prop_idx, prop) in el.props.iter().enumerate() {
            let name = &source[prop.start as usize..prop.name_end as usize];

            if prop.is_directive {
                // Event listeners: @click or v-on:click → onClick
                if name.starts_with('@') || name == "v-on" {
                    let event_name = if let Some(after_at) = name.strip_prefix('@') {
                        if after_at.is_empty() {
                            // @ shorthand with arg in arg_start/arg_end
                            match (prop.arg_start, prop.arg_end) {
                                (Some(s), Some(e)) => &source[s as usize..e as usize],
                                _ => continue,
                            }
                        } else {
                            after_at
                        }
                    } else {
                        // v-on with arg
                        match (prop.arg_start, prop.arg_end) {
                            (Some(s), Some(e)) => &source[s as usize..e as usize],
                            _ => continue,
                        }
                    };
                    let (value, vs) = match (prop.value_start, prop.value_end) {
                        (Some(vs), Some(ve)) => (&source[vs as usize..ve as usize], vs),
                        _ => continue,
                    };
                    let mut entry = String::with_capacity(32);
                    // Convert event name to onXxx camelCase format
                    // e.g., "popup-block" → "onPopupBlock", "update:modelValue" → "onUpdateModelValue"
                    entry.push_str("on");
                    let mut capitalize_next = true;
                    for c in event_name.chars() {
                        if c == '-' || c == ':' {
                            capitalize_next = true;
                        } else if capitalize_next {
                            entry.push(c.to_ascii_uppercase());
                            capitalize_next = false;
                        } else {
                            entry.push(c);
                        }
                    }
                    let oxc_exp = find_prop_oxc_exp(oxc_el, prop_idx);
                    let resolved_value =
                        resolve_expr(value, vs, oxc_exp, &self.resolver, self.options.force_js);
                    let trimmed_value = resolved_value.trim_end().trim_end_matches(';').trim_end();
                    if trimmed_value.is_empty() {
                        entry.push_str(": () => {}");
                    } else if value.contains(';') {
                        // Multi-statement handler: wrap in a block
                        entry.push_str(": () => { ");
                        entry.push_str(trimmed_value);
                        entry.push_str(" }");
                    } else {
                        // Wrap in parens to prevent comma operator from being
                        // misinterpreted as prop separator in the object literal
                        entry.push_str(": () => (");
                        entry.push_str(trimmed_value);
                        entry.push(')');
                    }
                    entries.push(entry);
                    continue;
                }

                // Dynamic bindings: :title → title: () => (expr)
                let arg = match (prop.arg_start, prop.arg_end) {
                    (Some(as_), Some(ae)) => Some(&source[as_ as usize..ae as usize]),
                    _ => None,
                };
                let (value, vs) = match (prop.value_start, prop.value_end) {
                    (Some(vs), Some(ve)) => (&source[vs as usize..ve as usize], vs),
                    _ => continue,
                };

                if let Some(attr_name) = arg {
                    if attr_name == "key" {
                        continue; // :key handled separately
                    }
                    let mut entry = String::with_capacity(32);
                    push_prop_key(&mut entry, attr_name);
                    entry.push_str(": () => (");
                    let oxc_exp = find_prop_oxc_exp(oxc_el, prop_idx);
                    entry.push_str(&resolve_expr(
                        value,
                        vs,
                        oxc_exp,
                        &self.resolver,
                        self.options.force_js,
                    ));
                    entry.push(')');
                    entries.push(entry);
                }
            } else {
                // Static attribute
                let value = match (prop.value_start, prop.value_end) {
                    (Some(vs), Some(ve)) => &source[vs as usize..ve as usize],
                    _ => continue,
                };
                let mut entry = String::with_capacity(32);
                push_prop_key(&mut entry, name);
                entry.push_str(": \"");
                // Escape characters that would break a JS string literal
                for c in value.chars() {
                    match c {
                        '\\' => entry.push_str("\\\\"),
                        '"' => entry.push_str("\\\""),
                        '\n' => entry.push_str("\\n"),
                        '\r' => entry.push_str("\\r"),
                        _ => entry.push(c),
                    }
                }
                entry.push('"');
                entries.push(entry);
            }
        }

        if entries.is_empty() {
            return None;
        }

        let mut result = String::with_capacity(64);
        result.push_str("{ ");
        for (i, entry) in entries.iter().enumerate() {
            if i > 0 {
                result.push_str(", ");
            }
            result.push_str(entry);
        }
        result.push_str(" }");
        Some(result)
    }

    /// Build a root element for a slot outlet (`_createSlot("name", null)`).
    fn build_slot_outlet_root(
        el: &ElementNode,
        source: &'alloc str,
        node_ref: u32,
        _state: VaporElementState<'alloc>,
        out: &mut CodeGenOutput<'alloc>,
    ) -> VaporRootElement<'alloc> {
        use super::shared::helpers::push_u32;

        // Determine slot name from static `name` attribute
        let mut slot_name = "default";
        for prop in &el.props {
            if prop.is_directive {
                continue;
            }
            let attr_name = &source[prop.start as usize..prop.name_end as usize];
            if attr_name == "name" {
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    slot_name = &source[vs as usize..ve as usize];
                }
            }
        }

        let mut create_line = String::with_capacity(48);
        create_line.push_str("const n");
        push_u32(&mut create_line, node_ref);
        create_line.push_str(" = _createSlot(\"");
        create_line.push_str(slot_name);
        create_line.push_str("\", null)");
        out.add_vapor_import(VaporHelper::CreateSlot);

        VaporRootElement {
            html: String::new(),
            template_idx: None,
            node_ref,
            nav: Vec::new(),
            text_creations: Vec::new(),
            effects: Vec::new(),
            statements: vec![out.alloc_str(&create_line)],
            v_once: false,
            v_memo_expr: None,
        }
    }

    /// Assemble the final Vapor output from accumulated root elements.
    ///
    /// Generates:
    /// 1. Hoisted template declarations (`const t0 = _template("...")`)
    /// 2. Render function body:
    ///    - Template instantiation (`const n0 = t0()`)
    ///    - Navigation instructions (`const p0 = _child(n0)`)
    ///    - Text node creations (`const x0 = _txt(p0)`)
    ///    - Render effects (`_renderEffect(() => { ... })`)
    ///    - Return statement
    fn assemble_output(&mut self, out: &mut CodeGenOutput<'alloc>) -> String {
        use super::shared::helpers::push_u32;

        let mut buf = String::with_capacity(512);

        // 1. Hoisted template declarations (write directly into buf)
        for root in &self.root_elements {
            if let Some(template_idx) = root.template_idx {
                helpers::write_template_declaration_into(
                    &mut buf,
                    template_idx,
                    &root.html,
                    true, // single-root for now
                );
                buf.push('\n');
            }
        }
        // Also emit templates hoisted by structural directives (v-if/v-for closures)
        for (template_idx, html) in &self.hoisted_templates {
            helpers::write_template_declaration_into(&mut buf, *template_idx, html, true);
            buf.push('\n');
        }

        // 2. Delegated events (sorted for deterministic output)
        if !self.delegated_events.is_empty() {
            self.delegated_events.sort();
            buf.push_str("_delegateEvents(");
            for (i, event) in self.delegated_events.iter().enumerate() {
                if i > 0 {
                    buf.push_str(", ");
                }
                buf.push('"');
                helpers::escape_js_string_into(&mut buf, event);
                buf.push('"');
            }
            buf.push_str(")\n");
            out.add_vapor_import(VaporHelper::DelegateEvents);
        }

        // 3. Render function signature (Vapor uses simpler signature than VDOM)
        if self.options.is_inline {
            buf.push_str("return (_ctx,_cache) => {\n");
        } else {
            buf.push_str("function render(_ctx) {\n");
        }

        // 4. Body for each root element
        for root in &self.root_elements {
            // Template instantiation — only for template-based roots
            if let Some(template_idx) = root.template_idx {
                buf.push_str("  const n");
                push_u32(&mut buf, root.node_ref);
                buf.push_str(" = t");
                push_u32(&mut buf, template_idx);
                buf.push_str("()\n");
            }

            // Navigation instructions
            for nav in &root.nav {
                buf.push_str("  ");
                buf.push_str(nav);
                buf.push('\n');
            }

            // Text node creations
            for tc in &root.text_creations {
                buf.push_str("  ");
                buf.push_str(tc);
                buf.push('\n');
            }

            // Effects — v_once emits directly, v_memo wraps with _withMemo,
            // otherwise wrap in _renderEffect
            if !root.effects.is_empty() {
                if root.v_once {
                    // v-once: effects as direct statements (no _renderEffect wrapper)
                    for effect in &root.effects {
                        buf.push_str("  ");
                        effect.write_code_into(&mut buf);
                        buf.push('\n');
                    }
                } else if let Some(ref memo_deps) = root.v_memo_expr {
                    // v-memo: wrap render effect with _withMemo
                    buf.push_str("  _renderEffect(() => _withMemo(");
                    buf.push_str(memo_deps);
                    buf.push_str(", () => {\n");
                    for effect in &root.effects {
                        buf.push_str("    ");
                        effect.write_code_into(&mut buf);
                        buf.push('\n');
                    }
                    buf.push_str("  }, _cache, ");
                    push_u32(&mut buf, self.memo_cache_idx);
                    buf.push_str("))\n");
                    self.memo_cache_idx += 1;
                    out.add_vapor_import(VaporHelper::RenderEffect);
                    out.add_vapor_import(VaporHelper::WithMemo);
                } else {
                    buf.push_str("  _renderEffect(() => {\n");
                    for effect in &root.effects {
                        buf.push_str("    ");
                        effect.write_code_into(&mut buf);
                        buf.push('\n');
                    }
                    buf.push_str("  })\n");
                    out.add_vapor_import(VaporHelper::RenderEffect);
                }
            }

            // Statements
            for stmt in &root.statements {
                buf.push_str("  ");
                buf.push_str(stmt);
                buf.push('\n');
            }
        }

        // 5. Return statement (avoid format!)
        match self.root_elements.len() {
            0 => buf.push_str("  return null\n"),
            1 => {
                buf.push_str("  return n");
                push_u32(&mut buf, self.root_elements[0].node_ref);
                buf.push('\n');
            }
            _ => {
                buf.push_str("  return [");
                for (i, root) in self.root_elements.iter().enumerate() {
                    if i > 0 {
                        buf.push_str(", ");
                    }
                    buf.push('n');
                    push_u32(&mut buf, root.node_ref);
                }
                buf.push_str("]\n");
            }
        }

        buf.push('}');
        buf
    }
}

impl<'ast, 'alloc> TemplateCodeGen<'alloc> for VaporCodeGen<'ast, 'alloc> {
    fn enter_template(
        &mut self,
        _root: &RootNodeTemplate,
        _source: &'alloc str,
        _out: &mut CodeGenOutput<'alloc>,
    ) {
        // Reset state for the template
        self.depth = 0;
    }

    fn leave_template(
        &mut self,
        root: &RootNodeTemplate,
        _source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        // Flush any pending v-if chain (e.g., v-if without v-else at end of template)
        self.flush_vif_chain(out);

        // Assemble the complete Vapor output
        let output = self.assemble_output(out);

        // Overwrite the entire template (open tag → close tag) with generated code
        let start = root.tag_open.start;
        let end = match root.tag_close.as_ref() {
            Some(tc) => tc.end,
            None => root
                .content
                .as_ref()
                .map(|c| c.end)
                .unwrap_or(root.tag_open.end),
        };
        out.overwrite(start, end, &output);
    }

    fn enter_element(
        &mut self,
        _id: NodeId,
        el: &ElementNode,
        _oxc: Option<&OxcParsedElement<'alloc>>,
        source: &'alloc str,
        _out: &mut CodeGenOutput<'alloc>,
    ) -> super::WalkAction {
        helpers::debug_assert_element_bounds(
            source,
            el.tag_open.start,
            el.tag_open.end,
            el.tag_open.name_end,
        );
        // Take a recycled state from the pool (retains capacity) or create new
        let mut state = self.state_pool.pop().unwrap_or_default();

        // Components, slot outlets, and template slot wrappers don't build HTML templates
        if el.tag_type != TagType::Component
            && el.tag_type != TagType::SlotOutlet
            && !(el.tag_type == TagType::Template && el.v_slot.is_some())
        {
            element::build_open_tag(el, source, &mut state);
        }

        self.element_stack.push(state);
        self.depth += 1;
        super::WalkAction::Continue
    }

    fn leave_element(
        &mut self,
        id: NodeId,
        el: &ElementNode,
        oxc_el: Option<&OxcParsedElement<'alloc>>,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        helpers::debug_assert_element_bounds(
            source,
            el.tag_open.start,
            el.tag_open.end,
            el.tag_open.name_end,
        );
        self.depth -= 1;

        let mut state = self.element_stack.pop().expect("leave without enter");
        let tag_name = &source[el.tag_open.start as usize + 1..el.tag_open.name_end as usize];

        // === Component elements ===
        if el.tag_type == TagType::Component {
            if self.depth == 0 {
                self.flush_vif_chain(out);
            }
            let node_ref = state.ensure_node_ref(&mut self.counters);
            let root =
                self.build_component_root(el, tag_name, node_ref, source, state, oxc_el, out);
            if self.depth == 0 {
                self.root_elements.push(root);
            } else {
                self.merge_non_root_into_parent(id, root, out);
            }
            return;
        }

        // === Slot outlets ===
        if el.tag_type == TagType::SlotOutlet {
            if self.depth == 0 {
                self.flush_vif_chain(out);
            }
            let node_ref = state.ensure_node_ref(&mut self.counters);
            let root = Self::build_slot_outlet_root(el, source, node_ref, state, out);
            if self.depth == 0 {
                self.root_elements.push(root);
            } else {
                self.merge_non_root_into_parent(id, root, out);
            }
            return;
        }

        // === Template slot wrappers (<template v-slot:name="params">) ===
        if el.tag_type == TagType::Template && el.v_slot.is_some() {
            let has_dynamic_text = el.children_flag.has(ChildrenFlags::HasInterpolation);
            let body = self.build_closure_body(state, has_dynamic_text, "    ", out);

            // Extract slot name from v-slot directive
            let slot_name = if let Some(ref v_slot) = el.v_slot {
                if let (Some(as_), Some(ae)) = (v_slot.arg_start, v_slot.arg_end) {
                    &source[as_ as usize..ae as usize]
                } else {
                    "default"
                }
            } else {
                "default"
            };

            // Extract scoped slot params (e.g., "{ item }" from v-slot="{ item }")
            let slot_params = el
                .v_slot
                .as_ref()
                .and_then(|v| match (v.value_start, v.value_end) {
                    (Some(vs), Some(ve)) => {
                        let params = &source[vs as usize..ve as usize];
                        if params.trim().is_empty() {
                            None
                        } else {
                            Some(params)
                        }
                    }
                    _ => None,
                });

            // Build the slot entry string: `name: (params) => { ... }`
            let mut entry = String::with_capacity(128);
            push_prop_key(&mut entry, slot_name);
            entry.push_str(": (");
            if let Some(params) = slot_params {
                entry.push_str(params);
            }
            entry.push_str(") => {\n");
            entry.push_str(&body);
            entry.push_str("    }");

            // Push to parent's named_slots
            if let Some(parent) = self.element_stack.last_mut() {
                parent.named_slots.push(entry);
            }
            return;
        }

        // === Normal elements ===
        let is_void = el.is_self_closing || el.content.is_none();
        element::close_html_tag(&mut state.html, tag_name, is_void);

        // Process dynamic props → effects
        {
            let mut props_ctx = props::VaporPropsContext {
                source,
                resolver: &self.resolver,
                state: &mut state,
                counters: &mut self.counters,
                out,
                delegated_events: &mut self.delegated_events,
                delegated_events_set: &mut self.delegated_events_set,
                force_js: self.options.force_js,
            };
            props::process_dynamic_props(el, &mut props_ctx, oxc_el);
        }

        // Derive has_dynamic_text from the AST children flags
        let has_dynamic_text = el.children_flag.has(ChildrenFlags::HasInterpolation);

        // === v-if/v-else-if/v-else structural directive ===
        if el.v_condition.is_some() && self.depth == 0 {
            self.handle_v_if_chain(el, source, oxc_el, state, has_dynamic_text, out);
            return;
        }

        // Flush any pending v-if chain (this element is not part of the chain)
        if self.depth == 0 {
            self.flush_vif_chain(out);
        }

        // === v-for structural directive ===
        if el.v_for.is_some() && self.depth == 0 {
            let root = self.build_v_for_root(el, source, state, has_dynamic_text, out);
            self.root_elements.push(root);
            return;
        }

        // Finalize text parts into effects
        element::finalize_text_parts(&mut state, has_dynamic_text);

        if self.depth == 0 {
            // Root element → register template and collect into root_elements
            let mut root =
                element::finalize_root_element(state, &mut self.counters, out, has_dynamic_text);
            // v-once: effects become direct statements (no _renderEffect wrapper)
            if el.v_once.is_some() {
                root.v_once = true;
            }
            // v-memo: effects are wrapped in _withMemo(deps, ...)
            if let Some(memo_expr) = extract_v_memo_expr(el, source) {
                root.v_memo_expr = Some(memo_expr);
            }
            self.root_elements.push(root);
        } else {
            // Non-root → merge into parent with DOM child index from AST
            // Compute before borrowing element_stack mutably
            let dom_child_index = self.compute_dom_child_index(id);
            if let Some(parent) = self.element_stack.last_mut() {
                let mut consumed = element::merge_into_parent(
                    state,
                    parent,
                    &mut self.counters,
                    dom_child_index,
                    has_dynamic_text,
                    out,
                );
                // Recycle the consumed state (vecs drained by append, html still present)
                consumed.reset();
                self.state_pool.push(consumed);
            }
        }
    }

    fn visit_text(
        &mut self,
        id: NodeId,
        text_node: &TextNode,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        helpers::debug_assert_slice_bounds(source, text_node.start, text_node.end, "visit_text");
        if let Some(parent) = self.element_stack.last_mut() {
            // Check if the parent element has interpolation children.
            // If not, skip text_parts allocation (they'd never be consumed).
            let has_interpolation = self
                .ast
                .nodes
                .get(id.0)
                .and_then(|node| node.parent)
                .and_then(|pid| self.ast.nodes.get(pid.0))
                .map(|parent_node| match &parent_node.kind {
                    AstNodeKind::Element(el) => {
                        el.children_flag.has(ChildrenFlags::HasInterpolation)
                    }
                    _ => false,
                })
                .unwrap_or(false);
            text::process_text(text_node, source, parent, has_interpolation, out);
        }
    }

    fn visit_interpolation(
        &mut self,
        _id: NodeId,
        interp: &InterpolationNode,
        oxc: &OxcParsedExpression<'alloc>,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        helpers::debug_assert_slice_bounds(
            source,
            interp.inner_start,
            interp.inner_end,
            "visit_interpolation",
        );
        if let Some(parent) = self.element_stack.last_mut() {
            interpolation::process_interpolation(
                interp,
                source,
                oxc,
                &self.resolver,
                parent,
                &mut self.counters,
                out,
            );
        }
    }

    fn visit_comment(
        &mut self,
        _id: NodeId,
        comment_node: &CommentNode,
        source: &'alloc str,
        _out: &mut CodeGenOutput<'alloc>,
    ) {
        helpers::debug_assert_slice_bounds(
            source,
            comment_node.start,
            comment_node.end,
            "visit_comment",
        );
        if let Some(parent) = self.element_stack.last_mut() {
            comment::process_comment(comment_node, source, self.options.comments, parent);
        }
    }
}

#[cfg(test)]
mod tests;
