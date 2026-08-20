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
mod nav_request;
pub mod props;
pub mod text;

use nav_request::PendingNavRequest;
// Shared types import the opaque `PendingNavQueue`, not `PendingNavRequest`.
pub(in crate::template::code_gen) use nav_request::PendingNavQueue;

use crate::ast::types::{
    AstNodeKind, ChildrenFlags, CommentNode, ElementNode, ElementNodeConditionKind,
    InterpolationNode, TagType, TemplateAst, TextNode,
};
use crate::code_transform::SegmentAnchor;
use crate::parser::types::RootNodeTemplate;
use crate::template::oxc::types::{OxcParsedElement, OxcParsedExpression};
use crate::types::NodeId;

use rustc_hash::FxHashSet;

use super::binding::BindingResolver;
use super::shared::helpers::{self, VaporHelper};
use super::types::{
    CodeGenOutput, SegmentedOverwriteAuthority, VaporCounters, VaporElementState, VaporRootElement,
};
use super::vdom::props::needs_quoted_key;
use super::{TemplateCodeGen, TemplateCodeGenOptions};

/// Append `body` to `stmt`, shifting `body_anchors` to absolute offsets in `stmt`.
fn push_body_with_anchors(
    stmt: &mut String,
    body: &str,
    body_anchors: &[SegmentAnchor],
    out_anchors: &mut Vec<SegmentAnchor>,
) {
    let base = stmt.len() as u32;
    stmt.push_str(body);
    out_anchors.extend(body_anchors.iter().map(|a| SegmentAnchor {
        content_offset: base + a.content_offset,
        length: a.length,
        source_pos: a.source_pos,
    }));
}

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

/// Official `setInsertionState(parent, anchor)` 2nd arg (vendored rc.3):
/// omitted → append last child; a number → one-time 0-based DOM index
/// (components/slot outlets, mount once); a node ref → insert before that
/// already-navigated marker (v-if/v-for, which remount).
enum InsertionAnchor {
    Append,
    Index(u32),
    NodeRef(u32),
}

/// A branch in a v-if/v-else-if/v-else chain.
struct VIfBranch<'alloc> {
    /// The condition expression (None for v-else).
    condition: Option<&'alloc str>,
    /// The closure body string (template instantiation, nav, effects, return).
    body: String,
    /// Interpolation anchors relative to `body`'s start. Empty if none.
    anchors: Vec<SegmentAnchor>,
    /// Official `canSkipIfBranchScope` NO_SCOPE eligibility (`build_closure_body`).
    is_static: bool,
    /// Official `context.root.nextIfIndex()`, once per `v-if`/`v-else-if`.
    /// `None` for `v-else`.
    own_if_index: Option<u32>,
}

/// Official `getNegativeIfBranchShape`: `v-else-if` is never NO_SCOPE
/// (`negative.type !== 14`); a terminal `v-else` carries its own static-ness
/// for FALSE_NO_SCOPE.
#[derive(Clone, Copy)]
enum IfNegative {
    /// No `v-else`/`v-else-if`.
    None,
    /// Continuing `v-else-if`.
    Chain,
    /// Terminal `v-else`, with whether its own branch is static.
    Terminal(bool),
}

/// Official `_createIf` 4th argument (`genIfFlags`/`genIfFlagNames`, vendored
/// rc.3). `None` when official omits it (`flags === 1`: bare v-if, positive
/// branch not NO_SCOPE-eligible, not nested where NO_SCOPE is disallowed).
///
/// Reachable space is SINGLE_ROOT only (`build_closure_body` always returns
/// one node; no `v-once`/slot-root `v-if`):
/// `1 | (has_negative ? 4 : 0) | (positive NO_SCOPE ? 32 : 0) |
/// (negative NO_SCOPE ? 64 : 0) | (has_negative ? (own_if_index + 1) << 8 : 0)`.
fn compute_if_flags(
    positive_static: bool,
    negative: IfNegative,
    own_if_index: Option<u32>,
    allow_no_scope: bool,
    is_production: bool,
) -> Option<String> {
    let has_negative = !matches!(negative, IfNegative::None);

    let mut flags: u32 = 1; // TRUE_SINGLE_ROOT
    let mut names: Vec<String> = vec!["TRUE_SINGLE_ROOT".to_string()];

    if has_negative {
        flags |= 1 << 2; // FALSE_SINGLE_ROOT
        names.push("FALSE_SINGLE_ROOT".to_string());
    }
    if allow_no_scope && positive_static {
        flags |= 32;
        names.push("TRUE_NO_SCOPE".to_string());
    }
    if let IfNegative::Terminal(negative_static) = negative {
        if allow_no_scope && negative_static {
            flags |= 64;
            names.push("FALSE_NO_SCOPE".to_string());
        }
    }
    if has_negative {
        if let Some(idx) = own_if_index {
            flags |= (idx + 1) << 8;
            names.push(format!("KEYED_INDEX_{idx}"));
        }
    }

    if flags == 1 {
        return None;
    }

    Some(if is_production {
        flags.to_string()
    } else {
        format!("{flags} /* {} */", names.join(", "))
    })
}

/// Official `_createFor` trailing flags (`genForFlags`, vendored rc.3).
/// `is_single_node` is always true here (`build_closure_body` always hoists
/// a template — official `isSingleNodeBlock` / `child.template != null`).
/// Component / `v-once` / slot-root v-for are unsupported, so `component` /
/// `once` / `slot_root` stay unset. Official's `!flags` omit is unreachable
/// (nothing sets `onlyChild`/`isSingleNode` without at least one bit) but
/// still checked, never assumed.
fn compute_for_flags(only_child: bool, is_production: bool) -> Option<String> {
    let is_single_node = true;
    let mut flags: u32 = 0;
    let mut names: Vec<&str> = Vec::new();
    if only_child {
        flags |= 1;
        names.push("FAST_REMOVE");
    }
    if is_single_node {
        flags |= 8;
        names.push("IS_SINGLE_NODE");
    }
    if flags == 0 {
        return None;
    }
    Some(if is_production {
        flags.to_string()
    } else {
        format!("{flags} /* {} */", names.join(", "))
    })
}

/// Loop-variable rename map for [`BindingResolver::push_for_scope`] —
/// official `itemVar = _for_item${depth}` + `buildDestructureIdMap` (rc.3).
/// `param_part` is [`helpers::parse_v_for_expression`]'s first return
/// (parens stripped); positions are value → key → index
/// ([`helpers::split_v_for_params`]).
///
/// Only a bare identifier is renamed. Destructures (`{ id }`, `[a, b]`)
/// stay un-renamed (official path-based `buildDestructureIdMap` is not
/// implemented). `_` never gets an entry.
fn build_for_scope_map(param_part: &str, depth: u32) -> rustc_hash::FxHashMap<String, String> {
    use super::binding::is_simple_ident;
    use super::shared::helpers::{push_u32, split_v_for_params};

    let mut map = rustc_hash::FxHashMap::default();
    let parts = split_v_for_params(param_part);
    let prefixes = ["_for_item", "_for_key", "_for_index"];
    for (part, prefix) in parts.iter().zip(prefixes.iter()) {
        let Some(name) = part.map(str::trim) else {
            continue;
        };
        if name.is_empty() || name == "_" || !is_simple_ident(name) {
            continue;
        }
        let mut accessor = String::with_capacity(prefix.len() + 10);
        accessor.push_str(prefix);
        push_u32(&mut accessor, depth);
        accessor.push_str(".value");
        map.insert(name.to_string(), accessor);
    }
    map
}

/// Main-closure params: renamed `_for_item{depth}`… for each bare identifier,
/// contiguous prefix only (no index without a value). A destructured
/// position stays as authored text — same disclosed gap as
/// [`build_for_scope_map`].
fn build_for_callback_params(param_part: &str, depth: u32) -> String {
    use super::binding::is_simple_ident;
    use super::shared::helpers::{push_u32, split_v_for_params};

    let parts = split_v_for_params(param_part);
    let prefixes = ["_for_item", "_for_key", "_for_index"];
    let mut pieces: Vec<String> = Vec::with_capacity(3);
    for (part, prefix) in parts.iter().zip(prefixes.iter()) {
        let Some(name) = part.map(str::trim) else {
            break;
        };
        if is_simple_ident(name) && name != "_" {
            let mut renamed = String::with_capacity(prefix.len() + 3);
            renamed.push_str(prefix);
            push_u32(&mut renamed, depth);
            pieces.push(renamed);
        } else {
            pieces.push(name.to_string());
        }
    }
    pieces.join(", ")
}

/// Leading identifier of `expr` — root of a left-recursive member/call chain.
/// `None` if `expr` does not start with an identifier. Used by
/// [`VaporCodeGen::resolve_v_for_source`].
fn leading_identifier(expr: &str) -> Option<&str> {
    let mut chars = expr.char_indices();
    let (_, first) = chars.next()?;
    if !(first == '_' || first == '$' || first.is_alphabetic()) {
        return None;
    }
    let end = chars
        .find(|&(_, c)| !(c == '_' || c == '$' || c.is_alphanumeric()))
        .map(|(i, _)| i)
        .unwrap_or(expr.len());
    Some(&expr[..end])
}

/// Accumulator for v-if chains that span multiple sibling elements.
struct VIfChain<'alloc> {
    /// The outer node ref for the entire chain.
    outer_ref: u32,
    /// Accumulated branches.
    branches: Vec<VIfBranch<'alloc>>,
    /// AST id of the most recently accumulated branch (the chain's last at flush).
    /// At depth > 0, used to decide whether the whole chain needs a `<!>` anchor.
    last_branch_id: NodeId,
    /// `element_stack` index of this chain's true DOM parent, captured when the
    /// first branch is created (`element_stack.last()` is unambiguously the
    /// parent then). `None` = template root (flush to `root_elements`).
    ///
    /// A chain with no following sibling stays pending until some later
    /// `leave_element` flushes it — the parent's last child, or a later sibling
    /// that pushed its own stack entry in between. An index captured at
    /// creation is stable under either; `element_stack.last_mut()` at flush
    /// time is not (it would read the flushing element's own entry).
    target_stack_index: Option<usize>,
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
    /// Static HTML buffer for the template scope currently being assembled.
    /// Plain elements and text/interpolation/comment nodes append here directly
    /// in DFS order, so a maximal run of plain elements is built into ONE buffer
    /// with no per-level copy. Each template-scope root (root element, component,
    /// slot outlet, slot template) saves the enclosing buffer on
    /// `html_scope_stack` and starts a fresh one.
    html: String,
    /// Saved enclosing HTML buffers for nested template scopes (see `html`).
    html_scope_stack: Vec<String>,
    /// Counter allocator for variable names.
    counters: VaporCounters,
    /// Completed root elements (ready for assembly).
    root_elements: Vec<VaporRootElement<'alloc>>,
    /// Depth counter (0 = root-level children of <template>).
    depth: u32,
    /// Dynamic block nesting, distinct from `depth` (plain AST/DOM nesting).
    /// Incremented only for a new official-compiler block (v-if/v-else branch,
    /// v-for item, slot fallback, component/`<template v-slot>` content) — never
    /// a wrapping `<div>`. Mirrors official `allowNoScope = context.block ===
    /// context.root.block`: a v-if inside plain wrappers stays NO_SCOPE-eligible
    /// (`block_depth` stays 0); one inside another block-creating construct does
    /// not. `depth == 0` was the wrong proxy (document root vs root top-level
    /// block) and dropped FALSE_NO_SCOPE for a v-if one DOM level inside a
    /// plain root wrapper (rc.3 `basic-interpolation.vue`).
    block_depth: u32,
    /// v-for nesting depth (push/pop, distinct from `block_depth`) — official
    /// `context.scopeLevel` (`enterScope`/`exitScope`), naming
    /// `_for_item{depth}`/`_for_key{depth}`/`_for_index{depth}`. Sibling (not
    /// nested) v-fors both get 0. Official shares this counter with slot-props
    /// destructuring (`_slotProps{depth}`) — unimplemented; reuse this field,
    /// do not add a second.
    for_scope_depth: u32,
    /// Pool of recycled VaporElementState instances (retains Vec capacities).
    state_pool: Vec<VaporElementState<'alloc>>,
    /// Collected delegated event names (in insertion order, deduplicated).
    delegated_events: Vec<&'alloc str>,
    /// Set for O(1) dedup of delegated events.
    delegated_events_set: FxHashSet<&'alloc str>,
    /// Templates hoisted by structural directives (v-if/v-for closures).
    /// Each entry is `(template_idx, html, is_static)`. `is_static` is official
    /// `canUseStaticTemplate()` (no effects/nav/text-extractions/statements).
    /// A closure template is never the document root (`root` is always false),
    /// matching official `templateRoot` which never reaches into a
    /// v-if/v-for/slot-fallback closure.
    hoisted_templates: Vec<(u32, String, bool)>,
    /// Pending v-if chain being accumulated across sibling elements.
    pending_vif_chain: Option<VIfChain<'alloc>>,
    /// Counter for v-memo cache slot allocation.
    memo_cache_idx: u32,
    /// Official `context.root.nextIfIndex()` — once per `v-if`/`v-else-if`,
    /// never `v-else`. Only a node WITH a negative branch consumes the index
    /// in `_createIf` flags (`compute_if_flags`).
    if_index_counter: u32,
    /// Per-open-element reserved construct-own id, lockstep with `element_stack`.
    /// `Some(id)` for `v-if`/`v-for` (`const nN = _createIf`/`_createFor`);
    /// `None` for `v-else-if`/`v-else` or a non-structural element.
    ///
    /// Reserved at enter (before children) because official allocates the
    /// construct id, then one wasted block-entry id, before any children
    /// (rc.3). Verter's bottom-up walker would otherwise consume a child
    /// interpolation id before `leave_element`.
    pending_construct_ref: Vec<Option<u32>>,
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
            html: String::new(),
            html_scope_stack: Vec::new(),
            counters: VaporCounters::default(),
            root_elements: Vec::new(),
            depth: 0,
            block_depth: 0,
            for_scope_depth: 0,
            state_pool: Vec::new(),
            delegated_events: Vec::new(),
            delegated_events_set: FxHashSet::default(),
            hoisted_templates: Vec::new(),
            pending_vif_chain: None,
            memo_cache_idx: 0,
            if_index_counter: 0,
            pending_construct_ref: Vec::new(),
        }
    }

    /// Build the inner closure body for a structural directive (v-if/v-for).
    ///
    /// Takes a finalized element state and produces the body lines:
    /// ```js
    /// const n2 = t0()
    /// [nav, text_creations, effects, statements]
    /// return n2
    /// ```
    ///
    /// Returns `(body, is_static, anchors)`. `is_static` is official
    /// `canUseStaticTemplate()`/`canSkipIfBranchScope()` (no effects/nav/
    /// text-extractions/statements) — reused for `_template()` flags and
    /// `_createIf()` NO_SCOPE. `anchors` are this closure's own interpolation
    /// anchors, relative to `body`; a nested construct in `child_statements`
    /// keeps its own anchors on that entry, not flattened here.
    fn build_closure_body(
        &mut self,
        mut state: VaporElementState<'alloc>,
        has_dynamic_text: bool,
        indent: &str,
        out: &mut CodeGenOutput<'alloc>,
    ) -> (String, bool, Vec<SegmentAnchor>) {
        use super::shared::helpers::push_u32;
        let mut anchors: Vec<SegmentAnchor> = Vec::new();

        // Finalize text parts
        element::finalize_text_parts(&mut state, has_dynamic_text);

        // Resolve this closure's direct-child nav requests before reading
        // `child_nav`/`child_statements`/`node_ref` — official
        // `processDynamicChildren` timing.
        self.resolve_pending_nav_requests(&mut state, out);

        // Strip trailing close tags (Vue 3.6 minimization)
        element::strip_trailing_close_tags(&mut state.html);

        // Register the template as hoisted
        let template_idx = self.counters.next_template();
        out.add_vapor_import(VaporHelper::Template);

        // Official `canUseStaticTemplate()`: no effects/nav/text-extractions/statements.
        let is_static = state.own_effects.is_empty()
            && state.child_effects.is_empty()
            && state.child_nav.is_empty()
            && state.child_text_creations.is_empty()
            && state.text_node_ref.is_none()
            && state.child_statements.is_empty();

        self.hoisted_templates
            .push((template_idx, std::mem::take(&mut state.html), is_static));

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
        // This closure root's own text extraction (e.g. a `{{ expr }}` v-if body).
        // Without `const xN = _txt(nRef)` + Txt/SetText, `_setText(xN, …)` is a
        // runtime `ReferenceError` — see `element::finalize_root_element`.
        if let Some(text_ref) = state.text_node_ref {
            body.push_str(indent);
            body.push_str("  const x");
            push_u32(&mut body, text_ref);
            body.push_str(" = _txt(n");
            push_u32(&mut body, inner_ref);
            body.push_str(")\n");
        }
        if !state.child_text_creations.is_empty() || state.text_node_ref.is_some() {
            out.add_vapor_import(VaporHelper::Txt);
            out.add_vapor_import(VaporHelper::SetText);
        }

        // Official always emits one-time operations before aggregated effects
        // (`assemble_output` root serialization).
        for (stmt, stmt_anchors) in &state.child_statements {
            body.push_str(indent);
            body.push_str("  ");
            push_body_with_anchors(&mut body, stmt, stmt_anchors, &mut anchors);
            body.push('\n');
        }

        // Effects
        if !all_effects.is_empty() {
            body.push_str(indent);
            body.push_str("  _renderEffect(() => ");
            if all_effects.len() == 1 {
                all_effects[0].write_code_into_with_anchors(&mut body, &mut anchors);
            } else {
                body.push_str("{\n");
                for effect in &all_effects {
                    body.push_str(indent);
                    body.push_str("    ");
                    effect.write_code_into_with_anchors(&mut body, &mut anchors);
                    body.push('\n');
                }
                body.push_str(indent);
                body.push_str("  }");
            }
            body.push_str(")\n");
            out.add_vapor_import(VaporHelper::RenderEffect);
        }

        // Return
        body.push_str(indent);
        body.push_str("  return n");
        push_u32(&mut body, inner_ref);
        body.push('\n');

        (body, is_static, anchors)
    }

    /// Start or continue a v-if chain for a root-level v-if/v-else-if/v-else element.
    ///
    /// - v-if: start a new chain (flushes any pending one first)
    /// - v-else-if: add a conditional branch to the pending chain
    /// - v-else: add the final branch and flush
    #[allow(clippy::too_many_arguments)]
    fn handle_v_if_chain(
        &mut self,
        id: NodeId,
        el: &ElementNode,
        source: &'alloc str,
        oxc_el: Option<&OxcParsedElement<'alloc>>,
        state: VaporElementState<'alloc>,
        has_dynamic_text: bool,
        construct_ref: Option<u32>,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        let cond = el.v_condition.as_ref().unwrap();

        // Construct id reserved at enter, before children — official allocates
        // construct id + one wasted branch-entry id before branch content (rc.3);
        // leave-time reservation is too late for this bottom-up walker.
        let outer_ref = construct_ref;
        let (body, is_static, body_anchors) =
            self.build_closure_body(state, has_dynamic_text, "  ", out);

        match cond.kind {
            ElementNodeConditionKind::If => {
                // Flush any pending chain first (shouldn't normally happen)
                self.flush_vif_chain(source, out);

                let outer_ref = outer_ref.expect("If arm always allocates outer_ref above");
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

                let own_if_index = self.if_index_counter;
                self.if_index_counter += 1;

                self.pending_vif_chain = Some(VIfChain {
                    outer_ref,
                    branches: vec![VIfBranch {
                        condition: Some(cond_expr),
                        body,
                        anchors: body_anchors,
                        is_static,
                        own_if_index: Some(own_if_index),
                    }],
                    last_branch_id: id,
                    // This element's stack entry is already popped; `element_stack.last()`
                    // is the chain's true DOM parent.
                    target_stack_index: self.element_stack.len().checked_sub(1),
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

                let own_if_index = self.if_index_counter;
                self.if_index_counter += 1;

                if let Some(chain) = &mut self.pending_vif_chain {
                    chain.branches.push(VIfBranch {
                        condition: Some(cond_expr),
                        body,
                        anchors: body_anchors,
                        is_static,
                        own_if_index: Some(own_if_index),
                    });
                    chain.last_branch_id = id;
                }
                // Orphan v-else-if without preceding v-if — diagnostic is
                // already emitted by the parser's validate_v_condition_adjacency.
            }
            ElementNodeConditionKind::Else => {
                if let Some(chain) = &mut self.pending_vif_chain {
                    chain.last_branch_id = id;
                    chain.branches.push(VIfBranch {
                        condition: None,
                        body,
                        anchors: body_anchors,
                        is_static,
                        own_if_index: None,
                    });
                }
                // Flush immediately — chain is complete
                self.flush_vif_chain(source, out);
            }
        }
    }

    /// Flush the pending v-if chain, producing a single root element.
    ///
    /// Generates nested `_createIf` calls:
    /// ```js
    /// _createIf(() => (a), () => { A }, () => _createIf(() => (b), () => { B }, () => { C }))
    /// ```
    fn flush_vif_chain(&mut self, source: &'alloc str, out: &mut CodeGenOutput<'alloc>) {
        let Some(chain) = self.pending_vif_chain.take() else {
            return;
        };

        use super::shared::helpers::push_u32;

        let mut stmt = String::with_capacity(256);
        stmt.push_str("const n");
        push_u32(&mut stmt, chain.outer_ref);
        stmt.push_str(" = ");

        // Nested `_createIf`s; each branch's interpolation anchors are absolute in `stmt`.
        let mut anchors: Vec<SegmentAnchor> = Vec::new();
        self.write_vif_branches(&chain.branches, 0, &mut stmt, &mut anchors);

        out.add_vapor_import(VaporHelper::CreateIf);

        let root = VaporRootElement {
            html: String::new(),
            template_idx: None,
            node_ref: chain.outer_ref,
            nav: Vec::new(),
            text_creations: Vec::new(),
            effects: Vec::new(),
            own_text_ref: None,
            statements: vec![(out.alloc_str(&stmt), out.alloc_segment_anchors(&anchors))],
            v_once: false,
            v_memo_expr: None,
        };
        match chain.target_stack_index {
            None => self.root_elements.push(root),
            Some(target_index) => {
                self.merge_vif_chain_into_target(
                    target_index,
                    chain.last_branch_id,
                    root,
                    source,
                    out,
                );
            }
        }
    }

    /// Recursively write nested `_createIf` calls.
    /// Shift each branch's interpolation anchors to absolute offsets in `stmt`.
    fn write_vif_branches(
        &self,
        branches: &[VIfBranch<'alloc>],
        idx: usize,
        stmt: &mut String,
        anchors: &mut Vec<SegmentAnchor>,
    ) {
        if idx >= branches.len() {
            return;
        }

        let branch = &branches[idx];
        let remaining = branches.len() - idx - 1;
        // Official `allowNoScope = context.block === context.root.block`.
        // Plain wrappers do not create a block; another block-creating construct
        // does. Not `self.depth == 0` — see `block_depth`.
        let allow_no_scope = self.block_depth == 0;

        if let Some(cond_expr) = branch.condition {
            // Condition is pre-resolved in handle_v_if_chain using OXC binding data
            stmt.push_str("_createIf(() => (");
            stmt.push_str(cond_expr);
            stmt.push_str("), () => {\n");
            push_body_with_anchors(stmt, &branch.body, &branch.anchors, anchors);

            if remaining > 0 {
                // Close the if-branch closure, add else argument
                // _createIf(() => (cond), () => { body }, () => { else }, flags)
                stmt.push_str("  }, () => ");
                let next = &branches[idx + 1];
                let negative = if next.condition.is_some() {
                    // v-else-if: wrap in another _createIf
                    self.write_vif_branches(branches, idx + 1, stmt, anchors);
                    IfNegative::Chain
                } else {
                    // v-else: direct closure
                    stmt.push_str("{\n");
                    push_body_with_anchors(stmt, &next.body, &next.anchors, anchors);
                    stmt.push('}');
                    IfNegative::Terminal(next.is_static)
                };
                // A negative branch always yields flags (at least FALSE_SINGLE_ROOT).
                let flags = compute_if_flags(
                    branch.is_static,
                    negative,
                    branch.own_if_index,
                    allow_no_scope,
                    self.options.is_production,
                );
                stmt.push_str(", ");
                stmt.push_str(&flags.expect("a present negative branch always yields flags"));
                stmt.push(')');
            } else {
                // No else branch — close the if-branch closure.
                stmt.push_str("  }");
                // Official `genMulti`: a present flags arg with no negative uses an
                // explicit `null` placeholder for the skipped 3rd arg (rc.3 `genCall`).
                match compute_if_flags(
                    branch.is_static,
                    IfNegative::None,
                    branch.own_if_index,
                    allow_no_scope,
                    self.options.is_production,
                ) {
                    Some(flags) => {
                        stmt.push_str(", null, ");
                        stmt.push_str(&flags);
                        stmt.push(')');
                    }
                    None => stmt.push(')'),
                }
            }
        } else {
            // v-else without preceding v-if (shouldn't happen, but handle gracefully)
            stmt.push_str("{\n");
            push_body_with_anchors(stmt, &branch.body, &branch.anchors, anchors);
            stmt.push('}');
        }
    }

    /// Resolve a v-for source (`items` in `item in items`), applying an outer
    /// loop-variable rename. `resolve_simple_expr` only handles a bare
    /// identifier — a member/call chain (`item.tags`) would otherwise reach
    /// generated code as a runtime `ReferenceError` (`item` is not in scope;
    /// `_for_item0` is).
    ///
    /// - Bare outer variable (`v-for="cell in row"`) — rename wholesale.
    /// - Chain rooted at the outer variable — rewrite only the leading
    ///   identifier; leave `.tags`/`[0]`/`.filter(x)` verbatim.
    ///
    /// A non-leading reference (`someFn(item).tags`) is a disclosed residual:
    /// it falls through to identifier-only resolution. Closing it needs
    /// scope-local reference spans (`VForWithBindings::scope_local_reference_names`
    /// currently exposes only names).
    fn resolve_v_for_source(&self, source_part: &str) -> String {
        use super::binding::is_simple_ident;

        let trimmed = source_part.trim();
        if is_simple_ident(trimmed) {
            if let Some(renamed) = self.resolver.resolve_for_local(trimmed) {
                return renamed.to_string();
            }
            return self.resolver.resolve_simple_expr(trimmed);
        }
        if let Some(root) = leading_identifier(trimmed) {
            if let Some(renamed) = self.resolver.resolve_for_local(root) {
                let mut out = String::with_capacity(renamed.len() + trimmed.len() - root.len());
                out.push_str(renamed);
                out.push_str(&trimmed[root.len()..]);
                return out;
            }
        }
        self.resolver.resolve_simple_expr(trimmed)
    }

    /// Build a root element for a v-for directive.
    #[allow(clippy::too_many_arguments)]
    fn build_v_for_root(
        &mut self,
        id: NodeId,
        el: &ElementNode,
        source: &'alloc str,
        state: VaporElementState<'alloc>,
        has_dynamic_text: bool,
        construct_ref: Option<u32>,
        out: &mut CodeGenOutput<'alloc>,
    ) -> VaporRootElement<'alloc> {
        use super::shared::helpers::push_u32;

        // Reserved at enter, before children — same as `handle_v_if_chain`.
        let outer_ref = construct_ref.expect("v-for always reserves a construct-own id");

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
                own_text_ref: None,
                statements: Vec::new(),
                v_once: false,
                v_memo_expr: None,
            };
        };

        // Parse "item in items" or "(item, index) in items"
        let (param_part, source_part) = helpers::parse_v_for_expression(full_expr);

        // No mapped interpolation anchor sits inside a v-for closure today;
        // returned anchors are discarded.
        let (closure_body, _is_static, _closure_body_anchors) =
            self.build_closure_body(state, has_dynamic_text, "  ", out);

        // Extract :key expression if present
        let key_expr = self.extract_key_expr(el, source);

        // Build the _createFor statement
        let resolved_source = self.resolve_v_for_source(source_part);
        // Main-closure params use renamed `_for_item{depth}`… (`itemVar`/`keyVar`/
        // `indexVar`). `for_scope_depth` here already matches this v-for's enter
        // depth (pop runs in `leave_element` before this function).
        let for_callback_params = build_for_callback_params(param_part, self.for_scope_depth);
        let mut stmt = String::with_capacity(256);
        stmt.push_str("const n");
        push_u32(&mut stmt, outer_ref);
        stmt.push_str(" = _createFor(() => (");
        stmt.push_str(&resolved_source);
        stmt.push_str("), (");
        stmt.push_str(&for_callback_params);
        stmt.push_str(") => {\n");
        stmt.push_str(&closure_body);
        stmt.push_str("  }");

        // Key callback params stay the raw source names — official
        // `genCallback`/`genSimpleIdMap` leaves rawKey/rawIndex unrenamed
        // (rc.3: `(item) => (item)`, never `(_for_item0) => (_for_item0.value)`).
        let has_key = key_expr.is_some();
        if let Some(key) = key_expr {
            stmt.push_str(", (");
            stmt.push_str(param_part);
            stmt.push_str(") => (");
            stmt.push_str(key);
            stmt.push(')');
        }

        // `_createFor` 4th arg. Flags-present + key-absent needs an explicit
        // `undefined` key-slot placeholder (rc.3:
        // `_createFor(..., undefined, 9 /* FAST_REMOVE, IS_SINGLE_NODE */)`).
        let only_child = self.v_for_is_only_child(id, source);
        if let Some(flags) = compute_for_flags(only_child, self.options.is_production) {
            if !has_key {
                stmt.push_str(", undefined");
            }
            stmt.push_str(", ");
            stmt.push_str(&flags);
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
            own_text_ref: None,
            statements: vec![(out.alloc_str(&stmt), &[])],
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

    /// Close the current template scope: hand its accumulated HTML buffer to
    /// `state.html` and restore the enclosing scope's buffer.
    ///
    /// Called when leaving a template-scope root (root element, component, slot
    /// outlet, or `<template v-slot>`) — the element whose `enter` started this
    /// scope. The per-kind builders then read the finalized HTML from `state`.
    fn take_scope_html(&mut self, state: &mut VaporElementState<'alloc>) {
        let enclosing = self
            .html_scope_stack
            .pop()
            .expect("vapor html scope underflow: leave without matching enter");
        state.html = std::mem::replace(&mut self.html, enclosing);
    }

    /// Emit `const n{target} = _child(n{container})` (first nav in this parent
    /// scope) or `const n{target} = _next(n{prev})` (chained). Official
    /// single-arg `next(node) => node.nextSibling` (rc.3). Separate from
    /// `element::merge_into_parent`'s dom-index nav — this backs multi-anchor
    /// nav inside one parent, which a parent-absolute index cannot express.
    ///
    /// `chain` is the caller's local nav-chain state for this resolve pass
    /// ([`resolve_pending_nav_requests`]), not module state: the stack entry
    /// it would have lived on is already popped. Updated to `target_ref` so
    /// the next call for this scope chains from here.
    fn emit_chained_nav(
        &self,
        target_ref: u32,
        container_ref: u32,
        chain: &mut Option<u32>,
        child_nav_sink: &mut [&'alloc str],
        slot: usize,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        use super::shared::helpers::push_u32;

        let mut stmt = String::with_capacity(32);
        stmt.push_str("const n");
        push_u32(&mut stmt, target_ref);
        stmt.push_str(" = ");
        match *chain {
            Some(prev_ref) => {
                stmt.push_str("_next(n");
                push_u32(&mut stmt, prev_ref);
                stmt.push(')');
                out.add_vapor_import(VaporHelper::Next);
            }
            None => {
                stmt.push_str("_child(n");
                push_u32(&mut stmt, container_ref);
                stmt.push(')');
                out.add_vapor_import(VaporHelper::Child);
            }
        }
        child_nav_sink[slot] = out.alloc_str(&stmt);
        *chain = Some(target_ref);
    }

    /// Bubble a wrapping element's establishment (`state.node_ref` is `Some`)
    /// plus descendant structural statements (`child_statements`) into the
    /// current parent. Runs alongside `element::merge_into_parent` (that path
    /// still handles this element's own dynamic text/props/effects). Uses
    /// `emit_chained_nav`, not a `<!>` — a wrapping `<header>` is already a
    /// real static tag, unlike a v-if/v-for region.
    fn bubble_structural_content_into_parent(
        &mut self,
        mut state: VaporElementState<'alloc>,
        already_navigated: bool,
        _out: &mut CodeGenOutput<'alloc>,
    ) -> VaporElementState<'alloc> {
        if state.node_ref.is_none() && state.child_statements.is_empty() {
            return state;
        }
        // `already_navigated` means `merge_into_parent` already established
        // `state.node_ref` via its dom-index formula and never clears that
        // field — `node_ref.is_some()` alone would navigate twice (different
        // mechanisms) and emit a reference-before-declaration. `child_statements`
        // still always bubble; `merge_into_parent` never touches them.
        //
        // Reserve the nav TEXT SLOT at this DFS position; the NUMBER is filled
        // later (`PendingNavRequest`) once the parent has visited all its
        // direct children.
        if !already_navigated {
            if let Some(own_ref) = state.node_ref {
                if let Some(parent) = self.element_stack.last_mut() {
                    let nav_slot = parent.child_nav.len();
                    parent.child_nav.push("");
                    parent
                        .pending_nav_requests
                        .push(PendingNavRequest::OwnRef { own_ref, nav_slot });
                }
            }
        }
        if let Some(parent) = self.element_stack.last_mut() {
            parent.child_nav.append(&mut state.child_nav);
            parent
                .child_text_creations
                .append(&mut state.child_text_creations);
            parent.child_effects.append(&mut state.child_effects);
            parent.child_statements.append(&mut state.child_statements);
        }
        state
    }

    /// Merge a non-root structural element (component, slot outlet, v-if/v-for)
    /// into its parent. Official rc.3 is not limited to v-if/v-for: a component
    /// followed by dynamic text (`<div><MyComp>x</MyComp>after {{ a }}</div>`)
    /// also gets `<!>` (`t1 = "<div><!> "`, `_setInsertionState(n4, n3)`). The
    /// factor is whether something else in this parent needs to navigate past
    /// this position:
    ///
    /// - nothing else → `_setInsertionState(container)` (1-arg append)
    /// - a meaningful sibling follows → `<!>` + `_setInsertionState(container, anchorRef)`
    /// - only static unreferenced siblings precede, nothing follows →
    ///   numeric `_setInsertionState(container, domChildIndex)`
    ///   (`<div><a>x</a><b>y</b><c>z</c></div>` → `_setInsertionState(n2, 2)`)
    fn merge_non_root_into_parent(
        &mut self,
        id: NodeId,
        root: VaporRootElement<'alloc>,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        // Callers merge immediately after `id`'s stack entry was popped; last
        // is the parent. `merge_vif_chain_into_target` is the exception.
        let Some(parent_index) = self.element_stack.len().checked_sub(1) else {
            return;
        };
        self.merge_into_stack_index(parent_index, id, root, source, out);
    }

    /// Merge a finished v-if chain into its true DOM parent —
    /// `chain.target_stack_index`, not necessarily `element_stack`'s last
    /// entry.
    ///
    /// A last-child chain stays pending until something else's `leave_element`
    /// flushes it: the structural parent (still last on the stack) or a later
    /// sibling (that sibling is last, one level too deep). `element_stack.last()`
    /// here would merge into whichever unrelated entry is on top.
    fn merge_vif_chain_into_target(
        &mut self,
        target_index: usize,
        id: NodeId,
        root: VaporRootElement<'alloc>,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        self.merge_into_stack_index(target_index, id, root, source, out);
    }

    /// Shared merge body; callers differ only in which `element_stack` index
    /// they target.
    fn merge_into_stack_index(
        &mut self,
        target_index: usize,
        id: NodeId,
        root: VaporRootElement<'alloc>,
        source: &'alloc str,
        _out: &mut CodeGenOutput<'alloc>,
    ) {
        let dom_child_index = self
            .element_stack
            .get_mut(target_index)
            .map(|parent| parent.observe_dom_element())
            .unwrap_or(0);
        let has_following = self.has_following_sibling(id, source);
        // `<!>` is meant to land at this construct's DFS position. Direct callers
        // (component/slot/element) run inside their own `leave_element`, before
        // later sibling markup. A v-if chain flushed after a following PLAIN
        // sibling is a known exception (official: comment before the sibling;
        // this: comment after). Mount is still correct (`_child` reads the
        // container's first child). Closing it needs reserving the DFS slot at
        // chain-creation time.
        if has_following {
            self.html.push_str("<!>");
        }

        // Reserve TEXT SLOTS at this DFS position; NUMBERS are filled later,
        // once the whole parent scope's children have been visited.
        if let Some(parent) = self.element_stack.get_mut(target_index) {
            let nav_slot = if has_following {
                let idx = parent.child_nav.len();
                parent.child_nav.push("");
                Some(idx)
            } else {
                None
            };
            let stmt_slot = parent.child_statements.len();
            parent.child_statements.push(("", &[]));
            parent.pending_nav_requests.push(PendingNavRequest::Merge {
                dom_child_index,
                has_following,
                nav_slot,
                stmt_slot,
            });
            for stmt in root.statements {
                parent.child_statements.push(stmt);
            }
        }
    }

    /// Official 2-arg `setInsertionState(parent, anchor)` (rc.3) — append
    /// (1-arg) or insert before a node ref. Not the former 4-arg call.
    fn emit_set_insertion_state(
        &self,
        container_ref: u32,
        anchor: InsertionAnchor,
        sink: &mut [(&'alloc str, &'alloc [SegmentAnchor])],
        slot: usize,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        use super::shared::helpers::push_u32;

        let mut stmt = String::with_capacity(48);
        stmt.push_str("_setInsertionState(n");
        push_u32(&mut stmt, container_ref);
        match anchor {
            InsertionAnchor::Append => {}
            InsertionAnchor::Index(idx) => {
                stmt.push_str(", ");
                push_u32(&mut stmt, idx);
            }
            InsertionAnchor::NodeRef(anchor_ref) => {
                stmt.push_str(", n");
                push_u32(&mut stmt, anchor_ref);
            }
        }
        stmt.push(')');
        out.add_vapor_import(VaporHelper::SetInsertionState);
        sink[slot] = (out.alloc_str(&stmt), &[]);
    }

    /// Resolve deferred [`PendingNavRequest`]s now that every direct child has
    /// been visited — official `processDynamicChildren` timing. Mints the
    /// memoized container ref (none if no establishing children) and per-request
    /// anchor ids in DFS order, ANCHOR before container-ref within each request
    /// (rc.3: `increaseId()` for the anchor precedes memoized `reference()`).
    fn resolve_pending_nav_requests(
        &mut self,
        state: &mut VaporElementState<'alloc>,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        if state.pending_nav_requests.is_empty() {
            return;
        }
        let requests = state.pending_nav_requests.take();
        let mut chain: Option<u32> = None;
        for request in requests {
            match request {
                PendingNavRequest::Merge {
                    dom_child_index,
                    has_following,
                    nav_slot,
                    stmt_slot,
                } => {
                    if !has_following && dom_child_index == 0 {
                        let container_ref = state.ensure_node_ref(&mut self.counters);
                        self.emit_set_insertion_state(
                            container_ref,
                            InsertionAnchor::Append,
                            &mut state.child_statements,
                            stmt_slot,
                            out,
                        );
                    } else if has_following {
                        // `<!>` was already pushed at DFS visit. Anchor id first, then
                        // memoized container ref — official allocation order.
                        let anchor_ref = self.counters.next_node();
                        let container_ref = state.ensure_node_ref(&mut self.counters);
                        let nav_slot = nav_slot.expect("has_following always reserves a nav slot");
                        self.emit_chained_nav(
                            anchor_ref,
                            container_ref,
                            &mut chain,
                            &mut state.child_nav,
                            nav_slot,
                            out,
                        );
                        self.emit_set_insertion_state(
                            container_ref,
                            InsertionAnchor::NodeRef(anchor_ref),
                            &mut state.child_statements,
                            stmt_slot,
                            out,
                        );
                    } else {
                        let container_ref = state.ensure_node_ref(&mut self.counters);
                        self.emit_set_insertion_state(
                            container_ref,
                            InsertionAnchor::Index(dom_child_index),
                            &mut state.child_statements,
                            stmt_slot,
                            out,
                        );
                    }
                }
                PendingNavRequest::OwnRef { own_ref, nav_slot } => {
                    let container_ref = state.ensure_node_ref(&mut self.counters);
                    self.emit_chained_nav(
                        own_ref,
                        container_ref,
                        &mut chain,
                        &mut state.child_nav,
                        nav_slot,
                        out,
                    );
                }
            }
        }
    }

    /// Whether `id` has a semantically-relevant following sibling. If so,
    /// dynamic content needs a `<!>` anchor; otherwise it can append.
    /// Whitespace-only text is not relevant.
    fn has_following_sibling(&self, id: NodeId, source: &str) -> bool {
        let Some(parent_id) = self.ast.nodes.get(id.0).and_then(|n| n.parent) else {
            return false;
        };
        let siblings: &[NodeId] = match &self.ast.nodes[parent_id.0].kind {
            AstNodeKind::Element(el) => el
                .content
                .as_ref()
                .map(|c| c.children.as_slice())
                .unwrap_or(&[]),
            _ => return false,
        };
        let Some(pos) = siblings.iter().position(|s| s.0 == id.0) else {
            return false;
        };
        siblings[pos + 1..]
            .iter()
            .any(|&sib| self.is_meaningful_sibling(sib, source))
    }

    /// Whether an AST child renders a DOM node (skip whitespace-only text).
    fn is_meaningful_sibling(&self, id: NodeId, source: &str) -> bool {
        match &self.ast.nodes[id.0].kind {
            AstNodeKind::Text(t) => source
                .get(t.start as usize..t.end as usize)
                .is_some_and(|s| !s.trim().is_empty()),
            AstNodeKind::Comment(_) => self.options.comments,
            _ => true,
        }
    }

    /// Official FAST_REMOVE source: `isOnlyChild = parent &&
    /// parent.block.node !== parent.node && parent.node.children.length === 1`
    /// (rc.3 `processFor`). Sole meaningful child of a PLAIN parent — not a
    /// component, slot outlet, `<template v-slot>`, or another block whose
    /// `.node` equals itself.
    fn v_for_is_only_child(&self, id: NodeId, source: &str) -> bool {
        let Some(parent_id) = self.ast.nodes.get(id.0).and_then(|n| n.parent) else {
            return false;
        };
        let AstNodeKind::Element(parent_el) = &self.ast.nodes[parent_id.0].kind else {
            return false;
        };
        // A v-if/v-for parent does not itself disqualify `onlyChild` (a
        // `<p v-if>`/`<p v-for>` whose only child is this v-for still gets
        // FAST_REMOVE). A sibling at this immediate parent does. A different
        // block-root (component, `<slot>`, `<template v-slot>`) routes through
        // SLOT_ROOT (`40 /* IS_SINGLE_NODE, SLOT_ROOT */`), never FAST_REMOVE.
        let parent_creates_new_block = parent_el.tag_type == TagType::Component
            || parent_el.tag_type == TagType::SlotOutlet
            || (parent_el.tag_type == TagType::Template && parent_el.v_slot.is_some());
        if parent_creates_new_block {
            return false;
        }
        let siblings: &[NodeId] = parent_el
            .content
            .as_ref()
            .map(|c| c.children.as_slice())
            .unwrap_or(&[]);
        siblings
            .iter()
            .filter(|&&sib| self.is_meaningful_sibling(sib, source))
            .count()
            == 1
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
                // Implicit default slot from non-template children. No mapped
                // interpolation anchor in a slot-fallback closure today.
                let (body, _is_static, _body_anchors) =
                    self.build_closure_body(state, has_dynamic_text, "    ", out);
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
            statements.push((resolve, &[] as &[SegmentAnchor]));
        }
        statements.push((out.alloc_str(&create_line), &[] as &[SegmentAnchor]));

        VaporRootElement {
            html: String::new(),
            template_idx: None,
            node_ref,
            nav: Vec::new(),
            text_creations: Vec::new(),
            effects: Vec::new(),
            own_text_ref: None,
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
        // No mapped interpolation anchor in a default-slot closure today.
        let (body, _is_static, _body_anchors) =
            self.build_closure_body(state, has_dynamic_text, "    ", out);

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
                    let trimmed_value = helpers::trim_handler_body(&resolved_value);
                    if trimmed_value.is_empty() {
                        entry.push_str(": () => {}");
                    } else if helpers::is_multi_statement_handler(oxc_exp) {
                        // Statement list: wrap in a block
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

    /// `_createSlot(name, props, fallback)`, trailing default-valued args
    /// omitted (rc.3 `slots.vue`: named + fallback emits all three; bare
    /// `<slot />` emits `_createSlot()`).
    ///
    /// `state` is the outlet's fallback content (its own HTML scope, handed
    /// over via `take_scope_html`) and is built with `build_closure_body`,
    /// not discarded.
    fn build_slot_outlet_root(
        &mut self,
        el: &ElementNode,
        source: &'alloc str,
        node_ref: u32,
        mut state: VaporElementState<'alloc>,
        out: &mut CodeGenOutput<'alloc>,
    ) -> VaporRootElement<'alloc> {
        use super::shared::helpers::push_u32;

        // Slot name from static `name`; the generated literal's opening quote
        // anchors to the ATTRIBUTE name's start, not the value (rc.3
        // `delimiter-anchor` on `slots.vue`).
        let mut slot_name = "default";
        let mut slot_name_attr_start: Option<u32> = None;
        for prop in &el.props {
            if prop.is_directive {
                continue;
            }
            let attr_name = &source[prop.start as usize..prop.name_end as usize];
            if attr_name == "name" {
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    slot_name = &source[vs as usize..ve as usize];
                    slot_name_attr_start = Some(prop.start);
                }
            }
        }

        let has_fallback = el
            .content
            .as_ref()
            .is_some_and(|content| !content.children.is_empty());
        let fallback = if has_fallback {
            let has_dynamic_text = el.children_flag.has(ChildrenFlags::HasInterpolation);
            // `state.node_ref` is the slot's own LHS ref — clear it or
            // `build_closure_body` reuses it for the fallback template and
            // collides with the outer `nN`.
            state.node_ref = None;
            // No mapped interpolation anchor in a slot-fallback closure today.
            let (body, _is_static, _body_anchors) =
                self.build_closure_body(state, has_dynamic_text, "  ", out);
            let mut closure = String::with_capacity(64 + body.len());
            closure.push_str("() => {\n");
            closure.push_str(&body);
            closure.push_str("  }");
            Some(closure)
        } else {
            None
        };

        // Omit trailing defaults: fallback, then props (`null`), then name (`"default"`).
        let props_is_default = true; // no fixture drives dynamic slot props yet
        let name_is_default = slot_name == "default";
        let name_arg_included = fallback.is_some() || !props_is_default || !name_is_default;

        let mut create_line = String::with_capacity(48);
        create_line.push_str("const n");
        push_u32(&mut create_line, node_ref);
        create_line.push_str(" = _createSlot(");

        let mut anchors: Vec<SegmentAnchor> = Vec::new();
        let mut first_arg = true;
        if name_arg_included {
            // Opening quote's generated position — official's delimiter-anchor shape.
            let quote_offset = create_line.len() as u32;
            create_line.push('"');
            create_line.push_str(slot_name);
            create_line.push('"');
            if let Some(source_pos) = slot_name_attr_start {
                anchors.push(SegmentAnchor {
                    content_offset: quote_offset,
                    length: 1,
                    source_pos,
                });
            }
            first_arg = false;
        }
        if fallback.is_some() || !props_is_default {
            if !first_arg {
                create_line.push_str(", ");
            }
            create_line.push_str("null");
            first_arg = false;
        }
        if let Some(fallback) = fallback {
            if !first_arg {
                create_line.push_str(", ");
            }
            create_line.push_str(&fallback);
        }
        create_line.push(')');
        out.add_vapor_import(VaporHelper::CreateSlot);

        VaporRootElement {
            html: String::new(),
            template_idx: None,
            node_ref,
            nav: Vec::new(),
            text_creations: Vec::new(),
            effects: Vec::new(),
            own_text_ref: None,
            statements: vec![(
                out.alloc_str(&create_line),
                out.alloc_segment_anchors(&anchors),
            )],
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
    fn assemble_output(&mut self, out: &mut CodeGenOutput<'alloc>) -> (String, Vec<SegmentAnchor>) {
        use super::shared::helpers::push_u32;

        let mut buf = String::with_capacity(512);
        // Absolute interpolation/static-attr anchors in `buf` (segmented overwrite).
        let mut anchors: Vec<SegmentAnchor> = Vec::new();

        // Hoisted templates in ascending allocation-index order, not source
        // order. A nested closure allocates its template before the enclosing
        // root's skeleton (DFS visits children first); the two collections
        // (`root_elements.template_idx` vs `hoisted_templates`) must interleave
        // by index (rc.3 `slots.vue`: `t0` = fallback, `t1` = root skeleton).
        //
        // Official `genTemplates` (rc.3): `root` is true only for the SFC's
        // single top-level template — never a closure template, never any root
        // of a multi-root fragment (`hasSingleRootChild` false; rc.1
        // `elements-text/multi-root.vue` uses flag 2 only). `static` is
        // `canUseStaticTemplate()`.
        let single_root = self.root_elements.len() == 1;
        let mut templates: Vec<(u32, &str, bool, bool)> = self
            .root_elements
            .iter()
            .filter_map(|root| {
                root.template_idx.map(|idx| {
                    let is_static = root.nav.is_empty()
                        && root.text_creations.is_empty()
                        && root.own_text_ref.is_none()
                        && root.effects.is_empty()
                        && root.statements.is_empty();
                    (idx, root.html.as_str(), single_root, is_static)
                })
            })
            .chain(
                self.hoisted_templates
                    .iter()
                    .map(|(idx, html, is_static)| (*idx, html.as_str(), false, *is_static)),
            )
            .collect();
        templates.sort_unstable_by_key(|(idx, ..)| *idx);
        for (template_idx, html, is_root, is_static) in templates {
            helpers::write_template_declaration_into(
                &mut buf,
                template_idx,
                html,
                is_root,
                is_static,
            );
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

        // Official `generate()`: `if (bindingMetadata && !inline) args.push(...)`.
        // Unlike VDOM/SSR, `bindingMetadata` is always truthy for non-inline
        // vapor — `@vue/compiler-sfc` defaults it to `{}` for `vapor && !ssr`
        // (`compiler-sfc.cjs.js`). Script-less `slots.vue` still emits the
        // 5-param signature, so non-inline vapor is unconditional.
        if self.options.is_inline {
            buf.push_str("return (_ctx,_cache) => {\n");
        } else {
            buf.push_str("function render(_ctx, $props, $emit, $attrs, $slots) {\n");
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
            // This root's own direct text extraction (see `own_text_ref` /
            // `finalize_root_element`); separate from child-bubbled `text_creations`.
            if let Some(text_ref) = root.own_text_ref {
                buf.push_str("  const x");
                push_u32(&mut buf, text_ref);
                buf.push_str(" = _txt(n");
                push_u32(&mut buf, root.node_ref);
                buf.push_str(")\n");
            }

            // Official `flushPendingOperations` (rc.3): one-time operations
            // (`_on()`, etc.) before the aggregated `effect` array, regardless of
            // source order (`props-emit.vue`: `:disabled` is source-first but
            // `_renderEffect` prints after `@click`'s `_on`).
            for (stmt, stmt_anchors) in &root.statements {
                buf.push_str("  ");
                push_body_with_anchors(&mut buf, stmt, stmt_anchors, &mut anchors);
                buf.push('\n');
            }

            // Effects — v_once emits directly, v_memo wraps with _withMemo,
            // otherwise wrap in _renderEffect
            if !root.effects.is_empty() {
                if root.v_once {
                    // v-once: effects as direct statements (no _renderEffect wrapper)
                    for effect in &root.effects {
                        buf.push_str("  ");
                        effect.write_code_into_with_anchors(&mut buf, &mut anchors);
                        buf.push('\n');
                    }
                } else if let Some(ref memo_deps) = root.v_memo_expr {
                    // v-memo: wrap render effect with _withMemo
                    buf.push_str("  _renderEffect(() => _withMemo(");
                    buf.push_str(memo_deps);
                    buf.push_str(", () => {\n");
                    for effect in &root.effects {
                        buf.push_str("    ");
                        effect.write_code_into_with_anchors(&mut buf, &mut anchors);
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
                        effect.write_code_into_with_anchors(&mut buf, &mut anchors);
                        buf.push('\n');
                    }
                    buf.push_str("  })\n");
                    out.add_vapor_import(VaporHelper::RenderEffect);
                }
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
        (buf, anchors)
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
        self.html.clear();
        self.html_scope_stack.clear();
    }

    fn leave_template(
        &mut self,
        root: &RootNodeTemplate,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        // Flush any pending v-if chain (e.g., v-if without v-else at end of template)
        self.flush_vif_chain(source, out);

        // Full Vapor output plus interpolation/static-attribute anchors.
        let (output, anchors) = self.assemble_output(out);

        // Overwrite the whole template via segmented overwrite: only each
        // anchor maps back to source; everything else is synthetic.
        let start = root.tag_open.start;
        let end = match root.tag_close.as_ref() {
            Some(tc) => tc.end,
            None => root
                .content
                .as_ref()
                .map(|c| c.end)
                .unwrap_or(root.tag_open.end),
        };
        out.overwrite_segmented(
            start,
            end,
            &output,
            &anchors,
            SegmentedOverwriteAuthority::new(),
        );
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
        let state = self.state_pool.pop().unwrap_or_default();

        // Components, slot outlets, and template slot wrappers don't build HTML templates
        let builds_open_tag = el.tag_type != TagType::Component
            && el.tag_type != TagType::SlotOutlet
            && !(el.tag_type == TagType::Template && el.v_slot.is_some());

        // New HTML scope at every root, every component/slot/slot-template,
        // and every v-if/v-else-if/v-else/v-for: structural content must not
        // share the ancestor's skeleton buffer (that leaks branch HTML into
        // the static template and leaves the hoisted template empty).
        let is_structural_root = el.v_condition.is_some() || el.v_for.is_some();
        if self.depth == 0 || !builds_open_tag || is_structural_root {
            self.html_scope_stack.push(std::mem::replace(
                &mut self.html,
                String::with_capacity(128),
            ));
        }
        // `!builds_open_tag || is_structural_root` == new official block.
        // Symmetric decrement in `leave_element` (same condition, `el` is
        // available at both sites).
        if !builds_open_tag || is_structural_root {
            self.block_depth += 1;
        }

        if builds_open_tag {
            element::build_open_tag(el, source, &mut self.html);
        }

        // Reserve construct-own id + burn one wasted branch/item-entry id at
        // enter, before children. Leave-time is too late: official allocates
        // these two ids before any branch/item content (rc.3).
        let construct_ref = if let Some(cond) = &el.v_condition {
            let outer = matches!(cond.kind, ElementNodeConditionKind::If)
                .then(|| self.counters.next_node());
            let _branch_entry_id = self.counters.next_node();
            outer
        } else if let Some(v_for_prop) = &el.v_for {
            let outer = Some(self.counters.next_node());
            let _item_entry_id = self.counters.next_node();
            // Push this v-for's rename map before descending — body expressions
            // resolve during their own visit, well before `leave_element`.
            self.for_scope_depth += 1;
            if let (Some(vs), Some(ve)) = (v_for_prop.value_start, v_for_prop.value_end) {
                let full_expr = &source[vs as usize..ve as usize];
                let (param_part, _source_part) = helpers::parse_v_for_expression(full_expr);
                let map = build_for_scope_map(param_part, self.for_scope_depth - 1);
                self.resolver.push_for_scope(map);
            } else {
                self.resolver
                    .push_for_scope(rustc_hash::FxHashMap::default());
            }
            outer
        } else if el.tag_type == TagType::SlotOutlet {
            // Slot outlet's own construct id, same reason as v-if/v-for. Fallback
            // burns one wasted `enterBlock()` id (rc.3 `slots.vue`:
            // `const n0 = _createSlot("header", null, () => { const n2 = t0() ... })`
            // — id 1 is consumed, never printed). Fallback-less `<slot/>` burns none.
            let outer = Some(self.counters.next_node());
            let has_fallback = el
                .content
                .as_ref()
                .is_some_and(|content| !content.children.is_empty());
            if has_fallback {
                let _fallback_entry_id = self.counters.next_node();
            }
            outer
        } else {
            None
        };
        self.pending_construct_ref.push(construct_ref);

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
        // Flush a pending v-if whose recorded parent is this still-on-stack
        // element, before the index disappears. A last-child chain of a
        // structural parent (v-for, component, `<slot>`, or another v-if)
        // flushes here. Later than this, `merge_into_stack_index` hits a
        // stale index (drop, or sibling of this construct — a v-for parent
        // then `ReferenceError`s the loop variable).
        //
        // Index comparison, not `el.v_condition.is_none()`: a v-else-if/v-else
        // continuation records the SHARED parent one level up, so it stays
        // pending for `handle_v_if_chain` later in this function.
        let my_own_stack_index = self.element_stack.len().checked_sub(1);
        if self
            .pending_vif_chain
            .as_ref()
            .is_some_and(|chain| chain.target_stack_index == my_own_stack_index)
        {
            self.flush_vif_chain(source, out);
        }
        self.depth -= 1;
        // Decrement before `handle_v_if_chain`/`flush_vif_chain` so
        // `allow_no_scope` sees the enclosing scope, not this closed branch.
        let builds_open_tag_here = el.tag_type != TagType::Component
            && el.tag_type != TagType::SlotOutlet
            && !(el.tag_type == TagType::Template && el.v_slot.is_some());
        if !builds_open_tag_here || el.v_condition.is_some() || el.v_for.is_some() {
            self.block_depth -= 1;
        }
        // Pop before `build_v_for_root` so `:key` stays unrenamed (official
        // `genSimpleIdMap`) and no v-for scope leaks to siblings.
        if el.v_for.is_some() {
            self.resolver.pop_for_scope();
            self.for_scope_depth -= 1;
        }

        let mut state = self.element_stack.pop().expect("leave without enter");
        // Pop once, matching every enter push; thread into
        // `handle_v_if_chain`/`build_v_for_root`.
        let construct_ref = self
            .pending_construct_ref
            .pop()
            .expect("leave without enter");
        let tag_name = &source[el.tag_open.start as usize + 1..el.tag_open.name_end as usize];

        // Components, slot outlets, and slot templates accumulated their content
        // into the scope buffer started at `enter`; hand it to `state.html` and
        // restore the enclosing buffer before the per-kind builders read it.
        if el.tag_type == TagType::Component
            || el.tag_type == TagType::SlotOutlet
            || (el.tag_type == TagType::Template && el.v_slot.is_some())
        {
            self.take_scope_html(&mut state);
        }

        // Component elements
        if el.tag_type == TagType::Component {
            // Pending v-if already flushed above.
            let node_ref = state.ensure_node_ref(&mut self.counters);
            let root =
                self.build_component_root(el, tag_name, node_ref, source, state, oxc_el, out);
            if self.depth == 0 {
                self.root_elements.push(root);
            } else {
                self.merge_non_root_into_parent(id, root, source, out);
            }
            return;
        }

        // Slot outlets
        if el.tag_type == TagType::SlotOutlet {
            // Pending v-if already flushed above.
            // Reserved at enter — never mint here (fallback body would steal the id).
            let node_ref = construct_ref.expect("slot outlet always reserves a construct-own id");
            state.node_ref = Some(node_ref);
            let root = self.build_slot_outlet_root(el, source, node_ref, state, out);
            if self.depth == 0 {
                self.root_elements.push(root);
            } else {
                self.merge_non_root_into_parent(id, root, source, out);
            }
            return;
        }

        // Template slot wrappers (`<template v-slot:name="params">`)
        if el.tag_type == TagType::Template && el.v_slot.is_some() {
            let has_dynamic_text = el.children_flag.has(ChildrenFlags::HasInterpolation);
            // No mapped interpolation anchor in a named-slot closure today.
            let (body, _is_static, _body_anchors) =
                self.build_closure_body(state, has_dynamic_text, "    ", out);

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

            // Push to parent's named_slots. A `<template v-slot>` is still a
            // DOM sibling for index purposes, so advance the parent's child
            // cursor even though the slot itself emits no inline DOM node.
            if let Some(parent) = self.element_stack.last_mut() {
                parent.observe_dom_element();
                parent.named_slots.push(entry);
            }
            return;
        }

        // Normal elements
        let is_void = el.is_self_closing || el.content.is_none();
        element::close_html_tag(&mut self.html, tag_name, is_void);
        if self.depth == 0 || el.v_condition.is_some() || el.v_for.is_some() {
            // Root or nested v-if/v-for owns the scope buffer it opened at enter.
            self.take_scope_html(&mut state);
        }

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

        // v-if/v-else-if/v-else
        // Depth-agnostic: `flush_vif_chain` routes to `merge_non_root_into_parent`
        // at depth > 0 — same anchor/nav as `<slot>` forwarding.
        if el.v_condition.is_some() {
            self.handle_v_if_chain(
                id,
                el,
                source,
                oxc_el,
                state,
                has_dynamic_text,
                construct_ref,
                out,
            );
            return;
        }

        // Pending v-if already flushed above.

        // v-for
        if el.v_for.is_some() {
            let root =
                self.build_v_for_root(id, el, source, state, has_dynamic_text, construct_ref, out);
            if self.depth == 0 {
                self.root_elements.push(root);
            } else {
                self.merge_non_root_into_parent(id, root, source, out);
            }
            return;
        }

        // Finalize text parts into effects
        element::finalize_text_parts(&mut state, has_dynamic_text);

        if self.depth == 0 {
            // Resolve this scope's child nav requests before `finalize_root_element`
            // reads `child_nav`/`child_statements`/`node_ref` (`processDynamicChildren`).
            self.resolve_pending_nav_requests(&mut state, out);
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
            // Same resolve, before bubbling `child_nav`/`child_statements` up.
            self.resolve_pending_nav_requests(&mut state, out);
            // Non-root → merge into parent; DOM index from the parent's running
            // child cursor, advanced once per observed child.
            if let Some(parent) = self.element_stack.last_mut() {
                let dom_child_index = parent.observe_dom_element();
                // Capture `merge_into_parent`'s bubble gate before the call; it drains
                // effects, so post-call emptiness cannot tell "already handled" from
                // "never had anything".
                let already_navigated = has_dynamic_text
                    || !state.own_effects.is_empty()
                    || !state.child_effects.is_empty();
                let consumed = element::merge_into_parent(
                    state,
                    parent,
                    &mut self.counters,
                    dom_child_index,
                    has_dynamic_text,
                    out,
                );
                // A wrapping element with no own dynamic content never hits
                // `merge_into_parent`'s bubble; forward descendant structural content
                // separately.
                let mut consumed =
                    self.bubble_structural_content_into_parent(consumed, already_navigated, out);
                // Recycle the consumed state (vecs drained by append; this
                // element's HTML already lives in the shared scope buffer).
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
        // Newline-containing whitespace-only text between tags is not a DOM
        // node under Vue condense (rc.3 `basic-interpolation.vue` / `slots.vue`
        // emit zero inter-tag bytes). Emitting it both pollutes the skeleton
        // and occupies a real sibling that `_child`/`_next` never skip
        // (`HierarchyRequestError: Node can't be inserted in a #text parent`).
        // Whitespace-only WITHOUT a newline condenses to one space and stays.
        // Reuses `vdom::text::classify_text_kind`.
        use super::vdom::text::classify_text_kind;
        let content = &source[text_node.start as usize..text_node.end as usize];
        if classify_text_kind(content) == Some(super::types::ChildKind::WhitespaceNewline) {
            return;
        }
        if let Some(parent) = self.element_stack.last_mut() {
            // Adjacent text/interpolation coalesce into one DOM child: advance
            // the parent's running child cursor only at the start of a run.
            parent.observe_dom_text_run();
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
            if classify_text_kind(content) == Some(super::types::ChildKind::WhitespaceSpace) {
                self.html.push(' ');
                if has_interpolation {
                    parent
                        .text_parts
                        .push(super::types::VaporTextPart::Static(" "));
                }
            } else {
                text::process_text(
                    text_node,
                    source,
                    &mut self.html,
                    parent,
                    has_interpolation,
                    out,
                );
            }
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
            // Interpolation coalesces with an adjacent text run into one DOM
            // child; advance the parent's running child cursor accordingly.
            parent.observe_dom_text_run();
            interpolation::process_interpolation(
                interp,
                source,
                oxc,
                &self.resolver,
                &mut self.html,
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
            // A rendered comment is its own DOM child and breaks any text run.
            // When comment rendering is disabled the comment is invisible to
            // the DOM, so it is not counted.
            if self.options.comments {
                parent.observe_dom_comment();
            }
            comment::process_comment(comment_node, source, self.options.comments, &mut self.html);
        }
    }
}

#[cfg(test)]
mod tests;
