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
// The opaque queue TYPE, not `PendingNavRequest` itself, is what the
// shared `template::code_gen::types` module needs — see `PendingNavQueue`'s
// own doc comment for why.
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

/// Append `body` to `stmt`, shifting `body_anchors` (relative to `body`'s own
/// start) to their ABSOLUTE position within `stmt` and accumulating them into
/// `out_anchors`. Shared by every `write_vif_branches` arm that splices a
/// closure body string into the enclosing `_createIf(...)` statement.
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

/// The second argument to official Vapor's real `setInsertionState(parent,
/// anchor)` runtime call — confirmed to take exactly these three shapes
/// against the vendored rc.3 runtime and compiler:
/// - omitted entirely → append as the container's last child.
/// - a plain number → insert at that 0-based DOM child index (a one-time
///   position, no persistent marker — components/slot outlets, which
///   mount exactly once).
/// - a node ref → insert before that specific (already-navigated-to)
///   anchor node (v-if/v-for, whose content can be removed and
///   re-inserted across reactivity updates and so needs a STABLE marker).
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
    /// `body`'s own embedded interpolation anchors, relative to `body`'s own
    /// text start (see `code_transform::segmented`'s module doc). Empty when
    /// the branch carries no interpolation.
    anchors: Vec<SegmentAnchor>,
    /// Whether this branch registered no effects/nav/text-extractions/
    /// statements — official's `canSkipIfBranchScope` NO_SCOPE eligibility
    /// signal (see `build_closure_body`'s `is_static` doc).
    is_static: bool,
    /// This branch's own sequential region-wide if-node index — official's
    /// `context.root.nextIfIndex()`, allocated once per `v-if`/`v-else-if`
    /// node (never for `v-else`, which isn't its own IfNode). `None` for a
    /// `v-else` branch.
    own_if_index: Option<u32>,
}

/// The shape of a v-if construct's negative (else) branch, for
/// `compute_if_flags` — official's `getNegativeIfBranchShape` distinguishes
/// a continuing `v-else-if` chain (never NO_SCOPE-eligible: negativeNoScope
/// requires `negative.type !== 14`) from a terminal `v-else` block (carries
/// its own static-ness for the FALSE_NO_SCOPE bit).
#[derive(Clone, Copy)]
enum IfNegative {
    /// No `v-else`/`v-else-if` at all.
    None,
    /// A `v-else-if` continuing the chain.
    Chain,
    /// A terminal `v-else`, carrying whether ITS OWN branch is static.
    Terminal(bool),
}

/// Compute official's `_createIf` 4th argument — the numeric bitflags plus,
/// in dev mode, a `/* NAME, NAME, ... */` comment — matching
/// `genIfFlags`/`genIfFlagNames` in the vendored rc.3 `@vue/compiler-vapor`
/// source bit-for-bit and name-for-name. Returns `None` when official's own
/// `flags === 1` special case omits the argument entirely (a bare v-if with
/// no negative branch, whose positive branch isn't itself NO_SCOPE-eligible
/// and isn't nested where NO_SCOPE is disallowed).
///
/// Verter's Vapor emitter currently only ever builds SINGLE_ROOT closures
/// (never EMPTY/MULTI_ROOT — `build_closure_body` always returns exactly
/// one node) and doesn't support `v-once`/slot-root `v-if`, so this covers
/// exactly the reachable case space: both branches' `blockShape` is always
/// `1` (SINGLE_ROOT) when present, and `once`/`slotRoot` are always false —
/// the resulting bit layout is `1 | (has_negative ? 4 : 0) | (positive
/// NO_SCOPE-eligible ? 32 : 0) | (negative NO_SCOPE-eligible ? 64 : 0) |
/// (has_negative ? (own_if_index + 1) << 8 : 0)`.
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

/// Compute official's `_createFor` trailing bitflags argument — matching
/// `genForFlags` in the vendored rc.3 `@vue/compiler-vapor` source
/// bit-for-bit and name-for-name. `is_single_node` is unconditionally `true`
/// for every v-for the current Vapor emitter reaches: `build_closure_body`
/// always registers a real hoisted template for the item body, exactly
/// official's own `isSingleNodeBlock` condition (`child.template != null`).
/// Component v-for, `v-once` v-for, and slot-root v-for aren't supported
/// yet (Verter's `build_v_for_root` never sets `component`/`once`/
/// `slot_root`-equivalent state) — this covers exactly the reachable case
/// space, same discipline as `compute_if_flags`. Official's `!flags` early
/// return (omit the argument entirely) is unreachable here: `onlyChild` and
/// `isSingleNode` alone can be false, but nothing SETS them without also
/// producing at least one bit in the reachable case space — still checked
/// explicitly, never assumed.
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

/// Build a v-for's loop-variable rename map for [`BindingResolver::
/// push_for_scope`] — official's real `itemVar = _for_item${depth}` +
/// `buildDestructureIdMap`, confirmed directly against the vendored rc.3
/// source. `param_part` is [`helpers::parse_v_for_expression`]'s first
/// return value (parens already stripped, e.g. `"item"` or
/// `"item, index"`); positions map value → key → index, per official's
/// own `parseFor` — [`helpers::split_v_for_params`]'s doc comment.
///
/// Only a BARE identifier position gets a rename; a destructuring pattern
/// (`{ id, name }`, `[a, b]`) in any position is left un-renamed — a
/// disclosed, narrower gap (official's path-based `buildDestructureIdMap`
/// rewriting for destructured sub-bindings isn't implemented yet). `_`
/// (official's placeholder for "skip this position") never gets an entry.
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

/// Build the v-for main closure's own parameter list — the RENAMED
/// accessor names (`_for_item{depth}`, ...) for each BARE identifier
/// position, in source order, stopping at the first absent (trailing)
/// position — contiguous prefix only, matching v-for's own positional
/// syntax (you cannot have an index without a value). A destructured
/// position (no [`build_for_scope_map`] entry) is left as its ORIGINAL
/// text, since nothing inside the body was renamed for it either — see
/// `build_for_scope_map`'s doc comment for the same disclosed gap.
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

/// The leading JS identifier at the very start of `expr` — the ROOT of a
/// member/computed-access/call chain (`item.tags`, `item[0]`,
/// `item.tags.filter(x)`, ...), always the chain's own textual PREFIX since
/// member/call expressions are left-recursive. `None` when `expr` does not
/// begin with an identifier character at all (a literal, a parenthesized
/// expression, a leading unary operator, ...) — used by
/// [`VaporCodeGen::resolve_v_for_source`] to find a v-for source's
/// potential outer-loop-variable root without parsing the whole expression;
/// see that method's own doc comment for the exact scope this covers.
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
    /// The AST id of the MOST RECENTLY accumulated branch — updated as each
    /// branch is added, so at flush time it names the chain's LAST branch.
    /// Used (at depth > 0 only) to look ahead for a following sibling —
    /// whether the WHOLE chain's mount position needs a `<!>` anchor —
    /// since that's a property of where the chain's final branch sits in
    /// its parent's children, not any earlier branch.
    last_branch_id: NodeId,
    /// `element_stack`'s own index of this chain's TRUE DOM PARENT's entry,
    /// captured when the chain's FIRST branch is created (the one moment
    /// `element_stack.last()` is unambiguously that parent — this element's
    /// own entry, if any, has already been popped by that point, and no
    /// sibling's entry has been pushed yet). `None` means the chain's
    /// parent is the template root (flush routes to `root_elements`).
    ///
    /// A chain with no following sibling can stay pending until something
    /// ELSE'S `leave_element` triggers the eventual flush — either the
    /// chain's own structural PARENT finishing (its own last child was the
    /// chain) or a LATER SIBLING finishing (a plain element, or another
    /// structural element like a `v-for`, that itself pushed and later
    /// pops its OWN stack entry in between). Both cases can leave a
    /// DIFFERENT number of `element_stack` entries above the true parent's
    /// own entry at the moment of the actual flush — an index captured
    /// once at chain-creation time is stable under either, where a fresh
    /// `element_stack.last_mut()` read AT FLUSH TIME is not: it would read
    /// whatever the FLUSHING element's own (still- or no-longer-pushed)
    /// entry happens to be, not the chain's actual parent.
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
    /// Dynamic BLOCK nesting depth — DISTINCT from `depth` (plain AST/DOM
    /// element nesting). Incremented only when entering a construct that
    /// creates a genuinely new official-compiler "block" (a v-if/v-else
    /// branch, a v-for item body, a slot outlet's fallback, a component's
    /// or `<template v-slot>`'s own slot content) — never for an ordinary
    /// wrapping element like `<div>`. Mirrors official's `context.block`
    /// identity check (`allowNoScope = context.block ===
    /// context.root.block`): a v-if directly inside any number of PLAIN
    /// wrapping elements is still eligible for NO_SCOPE (block_depth stays
    /// 0), but one nested inside ANOTHER block-creating construct is not.
    /// `depth == 0` was the pre-existing (wrong) proxy — it conflated "is
    /// the document root" with "is in the root's own top-level block",
    /// producing a missing FALSE_NO_SCOPE bit for a v-if merely nested one
    /// DOM level inside a plain root wrapper (confirmed against the pinned
    /// rc.3 golden for `basic-interpolation.vue`).
    block_depth: u32,
    /// v-for NESTING depth (a genuine push/pop stack depth, distinct from
    /// `block_depth`) — official's real `context.scopeLevel`
    /// (`enterScope()`/`exitScope()`), used to name each v-for's loop
    /// variables `_for_item{depth}`/`_for_key{depth}`/`_for_index{depth}`.
    /// Confirmed directly against the vendored rc.3 source: SIBLING
    /// (non-nested) v-for loops both get depth 0 (the counter returns to 0
    /// between them); only a v-for genuinely NESTED inside another v-for's
    /// own item body gets depth 1, etc. Official also shares this exact
    /// counter with slot-props destructuring (`_slotProps{depth}`) — not
    /// yet implemented here, so this counter is v-for-only for now; a
    /// future slot-props-destructure feature must reuse this SAME field,
    /// not add a second one.
    for_scope_depth: u32,
    /// Pool of recycled VaporElementState instances (retains Vec capacities).
    state_pool: Vec<VaporElementState<'alloc>>,
    /// Collected delegated event names (in insertion order, deduplicated).
    delegated_events: Vec<&'alloc str>,
    /// Set for O(1) dedup of delegated events.
    delegated_events_set: FxHashSet<&'alloc str>,
    /// Templates hoisted by structural directives (v-if/v-for closures).
    /// Each entry is (template_idx, html_string, is_static) — `is_static`
    /// mirrors official's `canUseStaticTemplate()`: this closure registered
    /// NO effects, navigation, text extractions, or statements, so its
    /// template is 100% static markup. A closure's own template is NEVER the
    /// document root (`root` is always false for these), matching official's
    /// `templateRoot` propagation, which never reaches into a v-if/v-for
    /// branch or slot-fallback closure.
    hoisted_templates: Vec<(u32, String, bool)>,
    /// Pending v-if chain being accumulated across sibling elements.
    pending_vif_chain: Option<VIfChain<'alloc>>,
    /// Counter for v-memo cache slot allocation.
    memo_cache_idx: u32,
    /// Region-wide sequential v-if/v-else-if node-index counter — official's
    /// `context.root.nextIfIndex()`. Incremented once per `v-if`/`v-else-if`
    /// node (never per `v-else`), independent of whether that node ends up
    /// using the index in its emitted `_createIf` flags (only a node WITH a
    /// negative branch consumes its own index — see `compute_if_flags`).
    if_index_counter: u32,
    /// Per-open-element-scope reserved CONSTRUCT-OWN id for a v-if/v-for
    /// structural root, pushed/popped in lockstep with `element_stack`.
    /// `Some(id)` for a `v-if`/`v-for` element (the id its `_createIf`/
    /// `_createFor` statement's own `const nN = ` binds to); `None` for a
    /// `v-else-if`/`v-else` element (no construct-own id — see
    /// `handle_v_if_chain`'s doc comment) or a non-structural element.
    ///
    /// Reserved at ENTER time (before descending into children) because
    /// official's real id-allocation order (confirmed by instrumenting the
    /// vendored rc.3 compiler directly) allocates the construct's own id,
    /// THEN one wasted id from entering its own block scope, BEFORE any of
    /// its children — including interpolations, which Verter's bottom-up
    /// walker would otherwise resolve (and consume an id for) before this
    /// element's own `leave_element` ever runs.
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
    /// Returns `(body, is_static, anchors)` — `is_static` mirrors official's
    /// `canUseStaticTemplate()`/`canSkipIfBranchScope()` signal (no effects,
    /// navigation, text extractions, or statements were registered), reused
    /// by both the hoisted template's own `_template()` flags (this
    /// function) and a v-if branch's `_createIf()` NO_SCOPE flag (the
    /// caller). `anchors` are `body`'s own embedded interpolation anchors,
    /// relative to `body`'s own text start (see
    /// `code_transform::segmented`'s module doc), populated from this
    /// closure's OWN effects only: a nested construct bubbled in through
    /// `child_statements` carries its OWN anchors alongside its OWN
    /// statement text (`Vec<(&str, &[SegmentAnchor])>` — see
    /// `VaporElementState::child_statements`), so its anchors stay attached
    /// to that entry rather than being flattened into this closure's own
    /// top-level `anchors` list.
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

        // Resolve any nav requests THIS closure's own direct children
        // queued (see `PendingNavRequest`) BEFORE reading `state.child_nav`/
        // `child_statements`/`node_ref` below — every direct child of this
        // closure's root has now been visited (e.g. a v-if branch's body
        // containing its own nested wrapping element), matching official's
        // `processDynamicChildren` timing.
        self.resolve_pending_nav_requests(&mut state, out);

        // Strip trailing close tags (Vue 3.6 minimization)
        element::strip_trailing_close_tags(&mut state.html);

        // Register the template as hoisted
        let template_idx = self.counters.next_template();
        out.add_vapor_import(VaporHelper::Template);

        // Mirrors official's `canUseStaticTemplate()`: static iff this
        // closure's subtree registered NO effects, navigation, text
        // extractions, or statements — nothing dynamic at all, so the
        // template is 100% static markup (confirmed directly against the
        // vendored rc.3 source).
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
        // This closure's own root's direct text extraction (e.g. a v-if
        // branch whose entire body is `{{ expr }}`) — see the identical fix
        // in `element::finalize_root_element` for the general case and why
        // it is needed (a bare `_setText(xN, …)` reference with no `const
        // xN = _txt(nRef)` statement and no Txt/SetText import is a runtime
        // `ReferenceError`, not merely a cosmetic omission).
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

        // Statements — official ALWAYS emits a block's non-reactive one-time
        // OPERATIONS before its aggregated effects (see the identical fix
        // in `assemble_output`'s root-element serialization for the general
        // rule and the confirming golden).
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

        // The construct's own id (If only) was already reserved at ENTER
        // time, before this element's children (including any
        // interpolations) were visited — see `pending_construct_ref`'s doc
        // comment and `enter_element`'s reservation for why: official's
        // real allocation order (confirmed by instrumenting the vendored
        // rc.3 compiler directly, cross-checked against 2 independent
        // pinned goldens) puts the construct's own id AND one wasted
        // branch-entry id BEFORE any of the branch's own content, but
        // Verter's bottom-up walker resolves a child interpolation's id
        // before this element's own leave-time processing ever runs — so
        // the reservation cannot happen here, only at enter.
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
                    // This element's own `element_stack` entry (if any) was
                    // already popped above, and no sibling's entry has been
                    // pushed yet — `element_stack.last()` right now is
                    // unambiguously this chain's true DOM parent.
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

        // Build nested structure from branches, accumulating every branch's
        // own embedded interpolation anchors (absolute within `stmt`).
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

    /// Recursively write nested _createIf calls for v-if chain branches.
    /// `anchors` accumulates every branch's OWN interpolation anchors,
    /// shifted to their ABSOLUTE position within `stmt`.
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
        // Official's `allowNoScope = context.block === context.root.block`:
        // the NO_SCOPE optimization applies to a v-if in the root's own
        // top-level BLOCK — any number of plain wrapping elements (e.g. a
        // root `<div>`) don't create a new block, so it stays eligible —
        // but never one nested inside ANOTHER block-creating construct
        // (v-for, a slot fallback, a component's default slot, ...). See
        // `block_depth`'s doc comment for why this is NOT `self.depth == 0`
        // (that conflates DOM/AST nesting with block nesting).
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
                // A negative branch is always present here, so official's
                // flags always carry at least the FALSE_SINGLE_ROOT bit —
                // `compute_if_flags` never returns `None` in this arm.
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
                // Official's `genMulti` placeholder rule: a present (truthy)
                // flags argument with an ABSENT negative renders an explicit
                // `null` placeholder for the skipped 3rd argument, rather
                // than shifting flags into its slot — confirmed directly
                // against the vendored rc.3 `genCall`/`genMulti` source.
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

    /// Resolve a v-for SOURCE expression (the `items` in `item in items`),
    /// applying an active OUTER v-for's loop-variable rename to it exactly
    /// like every other in-body reference does. `resolve_simple_expr` alone
    /// cannot do this: it resolves only a BARE identifier, passing anything
    /// else (a member/computed-access/call chain — `item.tags`, `item[0]`,
    /// `item.tags.filter(x)`, a nested v-for's real-world source shape)
    /// through UNCHANGED. A raw outer-loop reference reaching generated
    /// code is a runtime `ReferenceError` — `item` is not in scope, only
    /// `_for_item0` is — not merely a cosmetic divergence.
    ///
    /// Two cases:
    /// - The source IS the bare outer variable (`v-for="cell in row"` where
    ///   `row` is itself an outer loop's item) — rename it wholesale.
    /// - The source is a chain ROOTED at the outer variable (`item.tags`,
    ///   ...) — member/computed-access/call expressions are left-recursive,
    ///   so the chain's root is always source's own leading identifier;
    ///   rewrite only that leading identifier, leaving the rest of the
    ///   chain (`.tags`, `[0]`, `.filter(x)`, ...) verbatim.
    ///
    /// A source whose reference to the outer variable is NOT in leading
    /// position (`someFn(item).tags`, `[item, other]`) is a KNOWN,
    /// DISCLOSED residual: it falls through to `resolve_simple_expr`'s
    /// plain identifier-only resolution, unchanged. Closing that residual
    /// needs the v-for parser's own scope-local reference SPANS (today it
    /// exposes only NAMES, via `VForWithBindings::scope_local_reference_names`
    /// — see that field's doc comment), a producer-side addition with call
    /// sites beyond this one.
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

        // Reserved at ENTER time, before this element's children (including
        // any interpolations) were visited — see `pending_construct_ref`'s
        // doc comment and `handle_v_if_chain`'s (the same pattern applies
        // to v-for's own construct id).
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

        // Build the closure body. v-for bodies do not thread interpolation
        // anchors (no mapped interpolation anchor sits inside a v-for closure today
        // — see `push_body_with_anchors`'s call sites for the covered
        // shape); the returned anchors are intentionally discarded here.
        let (closure_body, _is_static, _closure_body_anchors) =
            self.build_closure_body(state, has_dynamic_text, "  ", out);

        // Extract :key expression if present
        let key_expr = self.extract_key_expr(el, source);

        // Build the _createFor statement
        let resolved_source = self.resolve_v_for_source(source_part);
        // The MAIN closure's own param list uses the RENAMED accessor names
        // (`_for_item{depth}`, ...) — official's real `itemVar`/`keyVar`/
        // `indexVar`. `self.for_scope_depth` at this point already equals
        // the depth THIS v-for used at enter time (push/pop is symmetric —
        // see `for_scope_depth`'s doc comment and `leave_element`'s pop,
        // which runs before this function is reached).
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

        // Add :key callback if present. UNLIKE the main closure above, the
        // key callback's own param list stays the RAW source names —
        // official's real `genCallback`/`genSimpleIdMap` leaves rawKey/
        // rawIndex (and any destructured value) bare and unrenamed here,
        // confirmed against the pinned rc.3 golden (`(item) => (item)`,
        // never `(_for_item0) => (_for_item0.value)`).
        let has_key = key_expr.is_some();
        if let Some(key) = key_expr {
            stmt.push_str(", (");
            stmt.push_str(param_part);
            stmt.push_str(") => (");
            stmt.push_str(key);
            stmt.push(')');
        }

        // Trailing bitflags argument (`_createFor`'s 4th positional arg) —
        // see `compute_for_flags`. The flags-present, key-absent
        // combination needs an explicit `undefined` key-slot placeholder to
        // stay positionally valid — confirmed directly against the real
        // rc.3 compiler (`_createFor(() => (_ctx.items), (_for_item0) =>
        // {...}, undefined, 9 /* FAST_REMOVE, IS_SINGLE_NODE */)` for a
        // bare `v-for` with no `:key` at all).
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

    /// Ensure the CURRENT parent scope (`element_stack`'s top) has its own
    /// Emit `const n{target_ref} = _child(n{container_ref})` — the FIRST
    /// navigation within the current parent scope — or `const n{target_ref}
    /// = _next(n{prev_ref})` — chained from the previous navigation in this
    /// SAME scope — matching official Vapor's real single-argument runtime
    /// signature (`next(node) => node.nextSibling`, confirmed directly
    /// against the vendored rc.3 runtime). This is a SEPARATE mechanism
    /// from `element::merge_into_parent`'s existing dom-index-based
    /// navigation (a plain element's own dynamic text/props/effects,
    /// untouched) — this one backs multi-anchor navigation within one
    /// parent (a wrapping element reaching a nested slot/component, or a
    /// v-if/v-for anchor reaching a following sibling position), where the
    /// dom-index-absolute style cannot express "the Nth sibling of a node
    /// that is itself not the parent."
    ///
    /// `chain` is the caller's OWN local nav-chain-state for the scope being
    /// resolved (see [`resolve_pending_nav_requests`]) — NOT module-level
    /// state, since every scope's chain is now resolved in one shot, in one
    /// call, well after the element_stack entry it would have lived on was
    /// already popped. Updated to `target_ref` so a subsequent call for the
    /// SAME scope chains from here instead of recomputing from
    /// `container_ref`.
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

    /// Bubble a plain wrapping element's OWN establishment (when something
    /// nested inside it needed to navigate TO it — `state.node_ref` is
    /// `Some`) plus any structural content (v-if/v-for/slot/component
    /// creation statements bubbled from a descendant, `state.child_statements`)
    /// up into the CURRENT parent scope. Called ALONGSIDE (not instead of)
    /// `element::merge_into_parent` — that function still runs first for
    /// this element's own dynamic text/props/effects, untouched; this one
    /// activates only when there is structural content to forward, using
    /// `emit_chained_nav` rather than an absolute dom-child-index (no `<!>`
    /// anchor needed here — a plain wrapping element like `<header>` is
    /// already a REAL static tag in the skeleton, unlike a v-if/v-for
    /// region which has no corresponding static content at all).
    fn bubble_structural_content_into_parent(
        &mut self,
        mut state: VaporElementState<'alloc>,
        already_navigated: bool,
        _out: &mut CodeGenOutput<'alloc>,
    ) -> VaporElementState<'alloc> {
        if state.node_ref.is_none() && state.child_statements.is_empty() {
            return state;
        }
        // `already_navigated` is true when `element::merge_into_parent`'s
        // OWN gate (this element's own dynamic text/props/effects) already
        // ran and established navigation to `state.node_ref` via its
        // existing dom-index formula — that field is never cleared by
        // that path, so checking `node_ref.is_some()` alone here would
        // navigate to the SAME ref a second time via a DIFFERENT
        // mechanism, producing a reference-before-declaration ordering bug
        // (confirmed as a genuine regression during development on plain
        // nested dynamic-text elements, e.g. `<div><section><article><p>{{
        // deep }}</p></article></section></div>`). Bubbling
        // `child_statements` (structural content from a descendant) is
        // always safe/needed regardless, since `merge_into_parent` never
        // touches that field at all.
        //
        // The establishment's TEXT SLOT is reserved right here (preserving
        // this exact DFS position in `child_nav`), but its NUMBER is
        // resolved later — this scope's own ref cannot be minted until it
        // (the parent whose slot this becomes) has visited every one of ITS
        // OWN direct children; see `PendingNavRequest`'s doc comment.
        // `resolve_pending_nav_requests` overwrites the reserved slot once
        // that parent scope is ready to finalize.
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

    /// Merge a non-root structural element (component, slot outlet,
    /// v-if/v-for chain) into its parent — the SAME mechanism for all of
    /// them (confirmed directly against the real vendored
    /// `@vue/compiler-vapor` rc.3: it is NOT limited to v-if/v-for needing
    /// a persistent anchor — a component followed by dynamic text
    /// (`<div><MyComp>x</MyComp>after {{ a }}</div>`) ALSO gets a `<!>`
    /// anchor: `t1 = "<div><!> "`, `_setInsertionState(n4, n3)`; the real
    /// distinguishing factor is purely whether something else in the SAME
    /// parent scope needs to navigate PAST this exact position, regardless
    /// of what kind of element this is):
    ///
    /// - nothing else at all in the container (neither before nor after)
    ///   → `_setInsertionState(container)`, 1-arg append.
    /// - a MEANINGFUL sibling follows (`has_following_sibling`) → that
    ///   sibling (or whatever comes after it) may need to `_next()` PAST
    ///   this position, so it needs a stable node to navigate from — a
    ///   `<!>` placeholder inserted into the shared scope HTML buffer at
    ///   exactly this position (DFS order already guarantees correct
    ///   placement), referenced via `_setInsertionState(container, anchorRef)`.
    /// - only static, non-referenced siblings precede it and nothing
    ///   follows → nothing will ever navigate past or from this position,
    ///   so the cheaper one-time numeric `_setInsertionState(container,
    ///   domChildIndex)` suffices, no anchor node needed at all
    ///   (`<div><a>x</a><b>y</b><c>z</c></div>` → `_setInsertionState(n2, 2)`,
    ///   zero `<!>` nodes in the skeleton).
    fn merge_non_root_into_parent(
        &mut self,
        id: NodeId,
        root: VaporRootElement<'alloc>,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        // The direct parent's own entry is unambiguously `element_stack`'s
        // last entry for every caller of THIS method: each one merges the
        // construct it JUST finished building for `id`, immediately after
        // `id`'s own `element_stack` entry was popped in `leave_element`,
        // before anything else pushes or pops. `merge_vif_chain_into_target`
        // is the one case that does NOT hold — see its own doc comment.
        let Some(parent_index) = self.element_stack.len().checked_sub(1) else {
            return;
        };
        self.merge_into_stack_index(parent_index, id, root, source, out);
    }

    /// Merge a v-if chain's finished construct into ITS OWN true DOM
    /// parent's `element_stack` entry — `chain.target_stack_index`, NOT
    /// necessarily `element_stack`'s current last entry.
    ///
    /// A chain whose last branch has no following sibling stays pending
    /// until SOMETHING ELSE'S `leave_element` triggers the eventual flush:
    /// either the chain's own structural parent finishing (its last child
    /// was the chain — flushed before that parent's own entry pops, so the
    /// parent IS still `element_stack`'s last entry) or, when the chain
    /// instead has a LATER SIBLING, that sibling's own `leave_element`
    /// (flushed before THAT sibling's own depth decrements — so the
    /// sibling's own entry is what's currently last, one level too deep;
    /// the chain's true parent sits at `chain.target_stack_index`,
    /// recorded when the chain was created and stable regardless of how
    /// many further entries have been pushed above it since). Using
    /// `element_stack.last()` here would silently merge into whichever of
    /// those unrelated entries happens to be on top at flush time instead
    /// of the chain's actual parent.
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

    /// Shared merge body for [`merge_non_root_into_parent`] and
    /// [`merge_vif_chain_into_target`] — the only difference between the two
    /// callers is which `element_stack` index they resolve as the target.
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
        // The `<!>` anchor CHARACTER is meant to land at this construct's
        // exact DFS position in the shared scope HTML buffer — unlike the
        // numeric id/statement text below, its POSITION is a property of
        // traversal order, not of any id value. This holds for every DIRECT
        // caller of this merge (component/slot-outlet/element merges: each
        // runs immediately inside its OWN `leave_element`, before any later
        // sibling's own markup has been appended). A v-if chain whose flush
        // is ITSELF deferred past a following PLAIN sibling — e.g.
        // `<li><p v-if>A</p><footer>after</footer></li>`, where the
        // `<footer>` leaves (and appends its own markup) before the pending
        // chain's true parent `<li>` does — is a KNOWN, CONFIRMED exception:
        // the anchor still gets appended here, but "here" is now AFTER that
        // sibling's markup, not at the chain's original DFS point (official
        // emits `<li><!><footer>after`; this emits `<li><footer>after<!>`).
        // The mount still inserts the branch content in the right place
        // (`_child` reads the container's actual first child, not
        // specifically this comment), so this is a skeleton-text-only
        // divergence — reserving the anchor's DFS position at chain-creation
        // time (mirroring how `child_nav`/`child_statements` reserve their
        // NUMBER slots below) would close it, but is a materially larger
        // change than a single-site fix.
        if has_following {
            self.html.push_str("<!>");
        }

        // The TEXT SLOTS are reserved right here (preserving this exact DFS
        // position — `root.statements`, e.g. `const n0 = _createIf(...)`,
        // is pushed immediately after, unchanged from before), but the
        // NUMBERS filled into them are resolved later: the container's own
        // ref (and, when `has_following`, the anchor id) cannot be minted
        // until the WHOLE parent scope's direct children have been
        // visited. `resolve_pending_nav_requests` overwrites the reserved
        // slot(s) once that parent scope is ready to finalize.
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

    /// Emit `_setInsertionState(nContainer)` (append as last child) or
    /// `_setInsertionState(nContainer, nAnchor)` (insert before the anchor)
    /// — official Vapor's REAL 2-argument signature (`setInsertionState(parent,
    /// anchor)`, confirmed directly against the vendored rc.3 runtime),
    /// NOT `merge_non_root_into_parent`'s former 4-argument call (which
    /// passed two extra arguments the real function doesn't accept).
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

    /// Resolve every deferred [`PendingNavRequest`] queued onto `state` by
    /// its direct children, now that ALL of them have been visited — the
    /// exact moment official's `processDynamicChildren` runs (once, at the
    /// end of `transformChildren`, before returning control to whatever
    /// scope contains THIS element). Mints `state`'s own container ref
    /// (memoized — a scope with no establishing children never mints one)
    /// and any per-request anchor ids, in queued (DFS visit) order, ANCHOR
    /// before container-ref within each request — confirmed directly
    /// against the vendored rc.3 source's `processDynamicChildren`: a fresh
    /// `context.increaseId()` for the anchor precedes the memoized
    /// `context.reference()` call for the container.
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
                        // The `<!>` anchor character was already pushed into
                        // the shared scope HTML buffer eagerly, at DFS visit
                        // time — see `merge_non_root_into_parent`. Only the
                        // number and statement text are resolved here.
                        // Anchor FIRST, container ref second (memoized) —
                        // matches official's real allocation order exactly.
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

    /// Whether `id` has any semantically-relevant following sibling within
    /// its own AST parent's children — i.e. whether its dynamic content
    /// needs an explicit `<!>` anchor placeholder (something follows it,
    /// so the runtime needs a stable position to insert before) or can
    /// simply append as the parent's last child (nothing does).
    /// Whitespace-only text between elements is not semantically relevant.
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

    /// Whether an AST child renders an actual DOM node — used by
    /// `has_following_sibling` to skip whitespace-only text between tags
    /// (which Vue's own runtime never observes as separating content).
    fn is_meaningful_sibling(&self, id: NodeId, source: &str) -> bool {
        match &self.ast.nodes[id.0].kind {
            AstNodeKind::Text(t) => source
                .get(t.start as usize..t.end as usize)
                .is_some_and(|s| !s.trim().is_empty()),
            AstNodeKind::Comment(_) => self.options.comments,
            _ => true,
        }
    }

    /// Official's `_createFor`'s FAST_REMOVE bit source: `isOnlyChild =
    /// parent && parent.block.node !== parent.node && parent.node.children
    /// .length === 1` (confirmed directly against the vendored rc.3
    /// `processFor` source) — the v-for element is the SOLE meaningful
    /// child of its own AST parent, AND that parent is a PLAIN element
    /// (not itself a block-creating construct — a component, slot outlet,
    /// `<template v-slot>`, or another v-if/v-for branch, all of which own
    /// their OWN block whose `.node` equals themselves, making
    /// `block.node !== node` false).
    fn v_for_is_only_child(&self, id: NodeId, source: &str) -> bool {
        let Some(parent_id) = self.ast.nodes.get(id.0).and_then(|n| n.parent) else {
            return false;
        };
        let AstNodeKind::Element(parent_el) = &self.ast.nodes[parent_id.0].kind else {
            return false;
        };
        // Official's real `isOnlyChild` (`transformFor`'s exit callback,
        // vendored rc.3 `@vue/compiler-vapor`): `parent.block.node !==
        // parent.node && parent.node.children.length === 1`. A v-if/v-for
        // parent does NOT, by itself, disqualify `onlyChild` — confirmed
        // directly against the real compiler: a `<p v-if>`/`<p v-for>`
        // whose ONLY child is this v-for still yields FAST_REMOVE, and a
        // sibling at that SAME immediate-parent level (not any ancestor
        // further up) is what disqualifies it, v-if/v-for present or not.
        // Only a genuinely DIFFERENT block-root shape — a component, a
        // `<slot>` outlet, or a `<template v-slot>` — disqualifies: those
        // route through the entirely separate SLOT_ROOT flag encoding
        // (confirmed empirically: `40 /* IS_SINGLE_NODE, SLOT_ROOT */`,
        // never a FAST_REMOVE bit at all), not this sibling-count story.
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
                // Implicit default slot from non-template children. No
                // mapped interpolation anchor sits inside a slot-fallback closure
                // today — see `push_body_with_anchors`'s call sites for the
                // covered shape — so the returned anchors are discarded.
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
        // No mapped interpolation anchor sits inside a default-slot closure today —
        // see `push_body_with_anchors`'s call sites for the covered shape —
        // so the returned anchors are discarded.
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

    /// Build a root element for a slot outlet: `_createSlot(name, props,
    /// fallback)`, trailing default-valued args omitted (official's own
    /// convention — confirmed against the pinned rc.3 golden for
    /// `slots.vue`: a named slot WITH fallback content emits all three
    /// args, `<slot />` with neither a name nor fallback content emits
    /// `_createSlot()`).
    ///
    /// `state` carries the slot outlet's own children (its FALLBACK
    /// content, e.g. `<slot name="header">Untitled</slot>`'s "Untitled"
    /// text) — accumulated into `state.html`/nav/effects during the DFS
    /// exactly like any other template-scope root (slot outlets open their
    /// own HTML scope in `enter_element`), then handed over via
    /// `take_scope_html` before this function runs. It is built into its
    /// own hoisted template + closure via `build_closure_body` (the SAME
    /// mechanism v-if branches use), NOT discarded.
    fn build_slot_outlet_root(
        &mut self,
        el: &ElementNode,
        source: &'alloc str,
        node_ref: u32,
        mut state: VaporElementState<'alloc>,
        out: &mut CodeGenOutput<'alloc>,
    ) -> VaporRootElement<'alloc> {
        use super::shared::helpers::push_u32;

        // Determine slot name from static `name` attribute, and the
        // ATTRIBUTE NAME's own source start (not the value's) — the pinned
        // official Vue vapor compiler anchors the generated name-string
        // literal's opening quote to the `name` ATTRIBUTE's own start (a
        // `delimiter-anchor` relation: a punctuation-classified generated
        // token at a non-word-interior source position — confirmed directly
        // against the rc.3 oracle's own map for `slots.vue`).
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
            // `state.node_ref` already holds the SLOT's OWN ref (allocated
            // by the caller for `_createSlot`'s LHS, `const nN = ...`) —
            // clear it before handing `state` to `build_closure_body`,
            // which otherwise reuses that SAME number for the fallback's
            // own template-instantiation ref (`ensure_node_ref` returns an
            // already-`Some` value unchanged) instead of allocating a
            // fresh one, colliding with the outer slot's own `nN`.
            state.node_ref = None;
            // No mapped interpolation anchor sits inside a slot-fallback closure
            // today, so the returned anchors are discarded.
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

        // Trailing default-valued args are omitted: fallback (undefined),
        // then props (null) once fallback is gone, then name ("default")
        // once props is gone too.
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
            // The opening quote's own generated position — a single
            // punctuation byte, matching official's exact anchor shape.
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
        // Absolute anchor positions in `buf` — the opt-in segmented-overwrite
        // primitive's anchor shape (see `code_transform::segmented`'s module
        // doc). Populated by every write path that can embed an
        // interpolation identifier: this function's own top-level effects
        // loop below, plus `root.statements` entries pre-computed by
        // `flush_vif_chain`/`build_closure_body` (v-if/v-for branch bodies).
        let mut anchors: Vec<SegmentAnchor> = Vec::new();

        // 1. Hoisted template declarations, in ASCENDING template-index
        // order regardless of source — official emits `t0`, `t1`, … in
        // allocation order, and a nested closure (a v-if branch, v-for
        // item, or slot fallback) allocates its own template BEFORE the
        // enclosing root's skeleton template (the root's `finalize_root_element`
        // / `next_template()` call only runs once ALL of its descendants —
        // including any nested closures — have already left, since the DFS
        // visits children before their parent). A root's own template and
        // the closure-hoisted ones are two SEPARATE collections
        // (`root_elements`' `template_idx` vs `hoisted_templates`) that
        // must be interleaved by index, not concatenated source-by-source
        // — confirmed against the pinned rc.3 golden for `slots.vue`
        // (`t0` = the fallback closure's own template, `t1` = the root
        // skeleton with the fallback nested INSIDE it).
        //
        // `root`/`static` bitflags: official `@vue/compiler-vapor`'s
        // `genTemplates` (confirmed directly against the vendored rc.3
        // source) — `root` is true ONLY for the SFC's own single top-level
        // template root (never for a v-if/v-for/slot-fallback closure's own
        // template, and never for any root when the template has MULTIPLE
        // top-level roots — a multi-root fragment's `hasSingleRootChild` is
        // false, confirmed against the pinned rc.1 golden for
        // `elements-text/multi-root.vue`, where every root template carries
        // flag 2 only, never 1). `static` mirrors `canUseStaticTemplate()`:
        // the template's own subtree registered no effects/nav/statements.
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

        // 3. Render function signature. Official `@vue/compiler-vapor`
        // (`generate()`): `if (bindingMetadata && !inline) args.push("$props",
        // "$emit", "$attrs", "$slots")`. Unlike VDOM/SSR, `bindingMetadata` is
        // ALWAYS truthy for non-inline vapor regardless of whether a script
        // exists — `@vue/compiler-sfc`'s `compileTemplate` defaults it to `{}`
        // specifically for `vapor && !ssr` when the caller passed none
        // (`compiler-sfc.cjs.js`: `vapor && !ssr && compilerOptions.bindingMetadata
        // == null ? {} : compilerOptions.bindingMetadata`) — proven against the
        // exact rc.3 goldens: even the script-less `slots.vue` vapor cell emits
        // the full 5-param signature. So non-inline vapor is unconditional.
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
            // This root's OWN direct text extraction (see `own_text_ref`'s
            // doc comment and `element::finalize_root_element` for why this
            // is a separate write from the child-bubbled `text_creations`
            // above).
            if let Some(text_ref) = root.own_text_ref {
                buf.push_str("  const x");
                push_u32(&mut buf, text_ref);
                buf.push_str(" = _txt(n");
                push_u32(&mut buf, root.node_ref);
                buf.push_str(")\n");
            }

            // Statements — official (`flushPendingOperations`, confirmed
            // directly against the vendored rc.3 source) ALWAYS emits a
            // block's non-reactive one-time OPERATIONS (event listeners via
            // `_on()`, etc.) BEFORE its aggregated `effect` array, regardless
            // of the directives' own SOURCE order on the element (confirmed
            // against the pinned rc.3 golden for `props-emit.vue`, whose
            // `:disabled` — source-first — still prints its `_renderEffect`
            // AFTER `@click`'s `_on(...)` — source-second).
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

        // Assemble the complete Vapor output, plus every embedded
        // interpolation/static-attribute anchor (see
        // `code_transform::segmented`'s module doc).
        let (output, anchors) = self.assemble_output(out);

        // Overwrite the entire template (open tag → close tag) with generated
        // code through the opt-in segmented primitive: bytes outside every
        // anchor (including the whole block when `anchors` is empty) are
        // synthetic scaffolding and carry no source-map token — only each
        // anchor's own authored position maps back to the source.
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

        // A new template scope begins at every root-level element, at every
        // component / slot outlet / slot template (each becomes its own
        // `_template(...)`), and at every v-if/v-else-if/v-else/v-for
        // element regardless of depth: its content is entirely dynamic and
        // gets its OWN hoisted template via `build_closure_body`, never the
        // enclosing static skeleton's buffer. A plain element normally
        // shares its parent's buffer, which is wrong for structural content
        // at any depth — sharing it produces an empty hoisted template with
        // the branch content leaking into the ancestor's own skeleton HTML
        // instead. Save the
        // enclosing scope's HTML buffer and start a fresh one; plain
        // descendants append into it directly.
        let is_structural_root = el.v_condition.is_some() || el.v_for.is_some();
        if self.depth == 0 || !builds_open_tag || is_structural_root {
            self.html_scope_stack.push(std::mem::replace(
                &mut self.html,
                String::with_capacity(128),
            ));
        }
        // `!builds_open_tag || is_structural_root` is exactly "this element
        // creates a new official-compiler block" — see `block_depth`'s doc
        // comment. Symmetric decrement in `leave_element`, recomputing the
        // SAME condition (nothing needs to be stored — `el` is available at
        // both sites).
        if !builds_open_tag || is_structural_root {
            self.block_depth += 1;
        }

        if builds_open_tag {
            element::build_open_tag(el, source, &mut self.html);
        }

        // Reserve the construct-own id (v-if/v-for) AND burn one wasted
        // branch/item-entry id, HERE at enter time, before descending into
        // this element's children — see `pending_construct_ref`'s doc
        // comment for why leave-time (`handle_v_if_chain`/
        // `build_v_for_root`) is too late: official's real allocation
        // order (confirmed by instrumenting the vendored rc.3 compiler
        // directly) puts these two ids before ANY of a branch's/item's own
        // content, but a child interpolation's id would otherwise already
        // be resolved by the time this element's own leave runs.
        let construct_ref = if let Some(cond) = &el.v_condition {
            let outer = matches!(cond.kind, ElementNodeConditionKind::If)
                .then(|| self.counters.next_node());
            let _branch_entry_id = self.counters.next_node();
            outer
        } else if let Some(v_for_prop) = &el.v_for {
            let outer = Some(self.counters.next_node());
            let _item_entry_id = self.counters.next_node();
            // Push this v-for's loop-variable rename map BEFORE descending
            // into children — an interpolation/expression inside the v-for
            // body is resolved during ITS OWN visit (bottom-up), which
            // happens well before this element's own `leave_element`
            // (where `build_v_for_root` would otherwise run too late — see
            // `pending_construct_ref`'s doc comment for the identical
            // timing reasoning). Popped in `leave_element`.
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
            // A slot outlet's OWN construct id, reserved here for the SAME
            // reason as v-if/v-for above. Fallback content additionally
            // burns one wasted id entering ITS OWN block scope — matching
            // `createSlot`'s real `enterBlock()`, confirmed directly
            // against the vendored rc.3 compiler and the pinned rc.3
            // golden for `slots.vue` (`const n0 = _createSlot("header",
            // null, () => { const n2 = t0() ... })` — id 1 is consumed but
            // never printed). A fallback-less `<slot/>` enters no such
            // scope, so it burns no wasted id.
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
        // Flush any pending v-if chain whose recorded true DOM parent is
        // EXACTLY this element — `chain.target_stack_index` equals this
        // element's own (not-yet-popped) `element_stack` index — before
        // that index disappears below. A chain whose last branch has no
        // following sibling (nothing else ever triggers a flush inside
        // that branch's own scope) stays pending until whatever leaves
        // NEXT that IS its recorded parent; for a v-if that is the LAST
        // child of a structural element (v-for, a component, a `<slot>`
        // outlet, or ANOTHER v-if/v-else — a conditional element can
        // itself be the DOM parent of a deeper, unrelated pending chain),
        // that is exactly THIS element's own `leave_element` call.
        // Flushing any later than here — after `self.depth -= 1` and the
        // `element_stack` pop below — merges the chain via a stale index
        // that no longer names this element, so the chain's construct is
        // either dropped entirely (`merge_into_stack_index` silently
        // no-ops against an out-of-bounds index) or lands as a SIBLING of
        // this element's own construct instead of nested inside it — for a
        // v-for parent, a runtime `ReferenceError` (the chain can reference
        // this element's own loop variable, which is then out of scope at
        // the mis-placed position).
        //
        // The index comparison — rather than `el.v_condition.is_none()` —
        // is what correctly excludes an element that ITSELF continues the
        // pending chain (v-else-if/v-else): a continuation's chain was
        // created by ITS OWN preceding v-if sibling and its recorded
        // target is the SHARED PARENT one level above this element, never
        // this element's own index, so the comparison naturally leaves it
        // pending for `handle_v_if_chain` (called later in this function)
        // to extend instead of complete.
        let my_own_stack_index = self.element_stack.len().checked_sub(1);
        if self
            .pending_vif_chain
            .as_ref()
            .is_some_and(|chain| chain.target_stack_index == my_own_stack_index)
        {
            self.flush_vif_chain(source, out);
        }
        self.depth -= 1;
        // Symmetric with `enter_element`'s increment — see `block_depth`'s
        // doc comment. Decremented here, BEFORE `handle_v_if_chain`/
        // `flush_vif_chain` run below for a v-if/v-else element, so
        // `write_vif_branches`'s `allow_no_scope` check sees the ENCLOSING
        // scope's block depth, not this element's own (already-closed)
        // branch body.
        let builds_open_tag_here = el.tag_type != TagType::Component
            && el.tag_type != TagType::SlotOutlet
            && !(el.tag_type == TagType::Template && el.v_slot.is_some());
        if !builds_open_tag_here || el.v_condition.is_some() || el.v_for.is_some() {
            self.block_depth -= 1;
        }
        // Symmetric with the `push_for_scope` in `enter_element` — pop
        // BEFORE `build_v_for_root` runs below so a later `:key="..."`
        // extraction (which official leaves unrenamed, matching its own
        // `genSimpleIdMap`'s bare key/index treatment) is unaffected either
        // way, and no v-for scope leaks into this element's own siblings.
        if el.v_for.is_some() {
            self.resolver.pop_for_scope();
            self.for_scope_depth -= 1;
        }

        let mut state = self.element_stack.pop().expect("leave without enter");
        // Popped exactly once here (matching every enter_element push,
        // regardless of which leave path this element takes below) and
        // threaded into `handle_v_if_chain`/`build_v_for_root` — see
        // `pending_construct_ref`'s doc comment.
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

        // === Component elements ===
        if el.tag_type == TagType::Component {
            // Any pending v-if chain has already been flushed above, before
            // this element's own depth/state teardown.
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

        // === Slot outlets ===
        if el.tag_type == TagType::SlotOutlet {
            // Any pending v-if chain has already been flushed above, before
            // this element's own depth/state teardown.
            // Reserved at ENTER time above — never mint fresh here (a
            // fallback's own body, visited before this leave runs, would
            // otherwise steal this id first).
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

        // === Template slot wrappers (<template v-slot:name="params">) ===
        if el.tag_type == TagType::Template && el.v_slot.is_some() {
            let has_dynamic_text = el.children_flag.has(ChildrenFlags::HasInterpolation);
            // No mapped interpolation anchor sits inside a named-slot closure today,
            // so the returned anchors are discarded.
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

        // === Normal elements ===
        let is_void = el.is_self_closing || el.content.is_none();
        element::close_html_tag(&mut self.html, tag_name, is_void);
        if self.depth == 0 || el.v_condition.is_some() || el.v_for.is_some() {
            // Root-level element, or a nested v-if/v-for element (its own
            // template scope opened on `enter` — see the matching
            // `is_structural_root` check there), owns the scope buffer it
            // opened.
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

        // === v-if/v-else-if/v-else structural directive ===
        // Depth-agnostic: `handle_v_if_chain`'s accumulation is a plain
        // field with no depth gate, and `flush_vif_chain` routes to
        // `merge_non_root_into_parent` at depth > 0 — the same anchor/
        // navigation mechanism `<slot>` forwarding uses.
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

        // Any pending v-if chain has already been flushed above, before
        // this element's own depth/state teardown.

        // === v-for structural directive ===
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
            // Root element → register template and collect into root_elements
            //
            // Resolve any nav requests THIS scope's direct children queued
            // (see `PendingNavRequest`) BEFORE `finalize_root_element` reads
            // `state.child_nav`/`child_statements`/`node_ref` — every direct
            // child has now been visited, so this is the correct point,
            // official's own `processDynamicChildren` timing.
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
            // Resolve any nav requests THIS scope's direct children queued
            // (see `PendingNavRequest`) BEFORE bubbling `state.child_nav`/
            // `child_statements` further up — every direct child of `state`
            // has now been visited, so this is the correct point, matching
            // official's own `processDynamicChildren` timing.
            self.resolve_pending_nav_requests(&mut state, out);
            // Non-root → merge into parent; DOM index from the parent's running
            // child cursor, advanced once per observed child.
            if let Some(parent) = self.element_stack.last_mut() {
                let dom_child_index = parent.observe_dom_element();
                // `merge_into_parent`'s OWN bubble gate — captured BEFORE
                // the call, since it drains `own_effects`/`child_effects`
                // (their post-call emptiness can't distinguish "already
                // handled" from "never had anything").
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
                // A plain wrapping element (e.g. `<header>` around a
                // `<slot>`, or `<ul>` around a `v-for`) with no dynamic
                // text/props/effects of its OWN never reaches
                // `merge_into_parent`'s bubble condition above — that path
                // is scoped to this element's OWN dynamic content, not
                // structural content forwarded from a descendant. Bubble
                // that separately.
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
        // Whitespace-only text CONTAINING a newline between tags (the
        // common indentation/line-break shape) renders no DOM node at all
        // under Vue's condense rules — official's own minimized template
        // strings have ZERO bytes for it (confirmed directly against the
        // pinned rc.3 golden for both `basic-interpolation.vue` and
        // `slots.vue`: `<div class=panel><header></header><main>`, no
        // inter-tag whitespace whatsoever). Emitting it unconditionally
        // does two things wrong at once: it pollutes the static HTML with
        // bytes official never produces, and — the actively-breaking part
        // for nested `_child`/`_next` navigation — it occupies a REAL DOM
        // sibling position the generated navigation never accounts for, so
        // a later `_child`/`_next` call lands on the whitespace text node
        // instead of the intended element (`HierarchyRequestError: Node
        // can't be inserted in a #text parent`).
        // Whitespace-only WITHOUT a newline condenses to a single space
        // (HTML's own inline-whitespace-collapse rule) and DOES still
        // occupy a real position, so it's kept — as exactly one space,
        // not the raw run. Reuses `vdom::text`'s existing classifier
        // rather than a second whitespace-detection implementation.
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
